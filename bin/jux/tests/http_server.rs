//! `examples/http_server` — the annotation-routed HTTP service on port 9091.
//!
//! Built, started, and then actually driven over a socket: the test speaks
//! HTTP/1.0 at it and reads the bodies back, which is the only way to prove the
//! routing table came out of the registry rather than out of a hand-kept list.
//! The last request it sends is `/shutdown`, which is how the server stops.
//!
//! Gated to Windows for the same reason the `minifb` examples are: the crate
//! build is what would differ on a bare runner, and the Jux side is portable.

#![cfg(windows)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

/// The address the example binds. Fixed, because the port is part of what the
/// example demonstrates.
const ADDR: &str = "127.0.0.1:9091";

/// One request/response, as a plain HTTP/1.0 exchange. Returns the body.
fn get(path: &str) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(ADDR)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
    stream.flush()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw)?;
    // Headers and body are separated by a blank line; everything after it is
    // what the handler returned.
    Ok(match raw.split_once("\r\n\r\n") {
        Some((_headers, body)) => body.to_string(),
        None => raw,
    })
}

/// Poll until the server answers, so the test does not race the bind.
///
/// The probe is a real `GET /health`, not a bare connect: a connection that
/// never sends a request is a request the server is still waiting on, and the
/// counter the test checks later would be off by however many probes it took.
/// This one counts as request #1.
fn wait_until_listening(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            panic!("http_server exited before listening: {status:?}");
        }
        if get("/health").is_ok_and(|body| body == "ok\n") {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    panic!("http_server never listened on {ADDR}");
}

#[test]
fn http_server_routes_from_the_annotation_registry() {
    let root = workspace_root();
    let project = root.join("examples").join("http_server");

    // Build first, so a compile failure is reported as one rather than as a
    // connection refused 60 seconds later.
    let build = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(&project)
        .arg("build")
        .output()
        .expect("spawning jux build for http_server");
    assert!(
        build.status.success(),
        "http_server failed to build with {:?}\n{}{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(&project)
        .arg("run")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawning jux run for http_server");
    wait_until_listening(&mut child);

    // Each of these is answered by a method that nothing but its `@Route`
    // annotation connects to a path.
    let health = get("/health").expect("GET /health");
    assert_eq!(health, "ok\n", "health body");

    let index = get("/").expect("GET /");
    assert!(index.contains("jux http demo"), "index body: {index:?}");

    // The routing table the server prints is read back out of the registry, so
    // every annotated method must appear in it -- including the `method`
    // default, which no call site wrote.
    let routes = get("/routes").expect("GET /routes");
    for expected in [
        "GET /  -> Api.index",
        "GET /health  -> Api.health",
        "GET /routes  -> Api.routes",
        "GET /stats  -> Api.stats",
        "POST /shutdown  -> Api.shutdown",
    ] {
        assert!(routes.contains(expected), "{expected:?} missing from {routes:?}");
    }

    // The probe, `/health`, `/` and `/routes` have been counted by now; this
    // request is counted after its own handler runs.
    let stats = get("/stats").expect("GET /stats");
    assert_eq!(stats, "served 4\n", "stats body");

    let unknown = get("/nope").expect("GET /nope");
    assert!(unknown.starts_with("no route for /nope"), "{unknown:?}");

    let bye = get("/shutdown").expect("GET /shutdown");
    assert_eq!(bye, "bye\n", "shutdown body");

    // `/shutdown` returns from `main`, so the process ends on its own.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "http_server exited with {status:?}");
                return;
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                panic!("http_server did not stop after /shutdown");
            }
            Err(e) => panic!("waiting for http_server: {e}"),
        }
    }
}
