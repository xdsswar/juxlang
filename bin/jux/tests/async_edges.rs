//! End-to-end tests for the S16–S19 fixes:
//!
//! - S17: `Worker.spawn(async () -> …)` drives awaits on the worker
//!   thread (previously emitted a plain closure → rustc E0728);
//! - S19: same-arity CONSTRUCTORS select by argument type
//!   (`Point(int)` / `Point(String)` / `Point(double)`);
//! - S16: an un-awaited async call is a clean juxc E0705;
//! - S18: assigning an outer primitive local inside an async `try` is
//!   a clean juxc E0706 (it would silently mutate a captured copy).

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn worker_async_lambda_and_typed_ctor_overloads() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let source = root().join("examples").join("async_edges.jux");
    let emit_dir = root().join("target").join("it-async-edges");

    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("spawn jux");

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
        ["int:7", "str:origin", "dbl:2.5", "pair:3", "42"],
        "unexpected output:\n{stdout}",
    );
}

#[test]
fn unawaited_async_and_async_try_mutation_are_jux_level() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let source = root().join("probes").join("probe_s16_s18.jux");

    let output = Command::new(jux)
        .arg("check")
        .arg(&source)
        .output()
        .expect("spawn jux");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("[E0705]").count(),
        2,
        "expected E0705 for the free fn AND the method:\n{stderr}",
    );
    assert!(stderr.contains("[E0706]"), "missing E0706:\n{stderr}");
    assert!(!stderr.contains("error[E0"), "rustc leak:\n{stderr}");
}
