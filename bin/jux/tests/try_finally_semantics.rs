//! End-to-end test pinning **Java-faithful try/catch/finally**.
//!
//! Runs `examples/try_finally_semantics.jux`:
//! - `finally` on the normal path,
//! - `finally` BEFORE a try-body `return` completes (was: rustc E0308
//!   — the closure swallowed the return),
//! - `finally` on the caught path,
//! - `finally` BEFORE an uncaught exception propagates (was: the
//!   payload was dropped and propagation lost),
//! - runtime subtype dispatch + rethrow to an outer handler,
//! - inherited `e.message` / `e.getMessage()` through user subclasses
//!   (was: the user→stdlib extends chain didn't resolve).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn try_finally_java_semantics() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("try_finally_semantics.jux");
    let emit_dir = workspace_root.join("target").join("it-try-finally");

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
        [
            "normal",
            "finally-normal",
            "finally-before-return",
            "1",
            "caught: nf",
            "finally-caught",
            "finally-before-propagate",
            "outer caught: up",
            "rethrow: rethrown",
            "done",
        ],
        "unexpected output:\n{stdout}",
    );
}
