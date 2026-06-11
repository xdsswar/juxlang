//! End-to-end test for **File / Path / Console I/O and monotonic
//! time** (jux.std.io, jux.std.time): write/append/read-lines/delete
//! round-trip, Path queries with the nullable protocol, stdin reads
//! via Console.readLine (two piped lines then EOF), and
//! Instant.now()/elapsed*.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn io_and_time() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("io_and_time.jux");
    let emit_dir = workspace_root.join("target").join("it-io-and-time");
    // The example writes into this directory (relative to the cwd the
    // compiled program runs in, which is the workspace root).
    std::fs::create_dir_all(workspace_root.join("target").join("it-io-time-data"))
        .expect("create data dir");

    let mut child = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .current_dir(&workspace_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn jux");
    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all(b"ada\nlovelace\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for jux");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "jux exited with {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(
        lines.as_slice(),
        [
            "true", "3", "alpha", "gamma",
            "notes.txt", "txt", "a/b", "null", "true", "true",
            "false",
            "true", "true", "true",
            "hello ada", "lovelace", "true",
        ],
        "unexpected output:\n{stdout}",
    );
}
