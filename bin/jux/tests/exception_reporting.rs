//! End-to-end test for `examples/exception_reporting.jux` — exception
//! reporting in a program with no `package` declaration.
//!
//! Exceptions lower to a typed panic caught by `catch_unwind`, and the entry
//! point installs two layers on top: a hook that keeps Rust's own panic message
//! off stderr for a typed throw, and a reporter that prints an escaped
//! exception Java-style. Both are attached by renaming the emitted entry point,
//! and the rename did not recognise `pub(crate) fn main` — the shape a
//! package-less program emits. So for the most common small-program shape:
//!
//! - every CAUGHT exception printed `thread 'main' panicked … Box<dyn Any>` to
//!   stderr, which reads as the runtime falling over;
//! - an UNCAUGHT one printed that instead of its type and message, losing the
//!   only information that would have identified it.
//!
//! This test therefore asserts on stderr, not just stdout, and checks the
//! emitted crate carries both layers.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn exceptions_report_cleanly_without_a_package_declaration() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("exception_reporting.jux");
    let emit_dir = workspace_root.join("target").join("it-exception-reporting");

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
            // three caught retries, then the attempt count that succeeded
            "retry 1",
            "retry 2",
            "retry 3",
            "4",
            // a throw caught by a SUPERTYPE clause, and the finally after it
            "caught as supertype: outer",
            "finally ran",
        ],
        "unexpected output:\n{stdout}",
    );

    // The real assertion: a caught exception is not an incident. Rust's own
    // panic text reaching the user is the bug this guards.
    assert!(
        !stderr.contains("panicked"),
        "a caught exception must not print a Rust panic:\n{stderr}",
    );
    assert!(
        !stderr.contains("Box<dyn Any>"),
        "the raw payload type must never reach the user:\n{stderr}",
    );

    // And the emitted crate must actually carry both layers, so a future change
    // to the entry-point shape fails here rather than silently in stderr.
    let emitted = std::fs::read_to_string(emit_dir.join("src").join("main.rs"))
        .expect("emitted crate is readable");
    assert!(
        emitted.contains("__jux_user_main"),
        "the entry point was not wrapped, so neither reporting layer is installed",
    );
    assert!(
        emitted.contains("set_hook"),
        "the quiet panic hook was not installed",
    );
}
