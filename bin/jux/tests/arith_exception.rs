//! End-to-end tests for **integer division/remainder by zero →
//! catchable `ArithmeticException`** (ERRATA.md E1 Java-parity
//! carve-out; gap O8).
//!
//! - `examples/arith_exception.jux`: `/ 0` and `% 0` caught as
//!   `ArithmeticException` / `RuntimeException` / `Exception`,
//!   compound `/=`, a literal `1 / 0` (must not trip rustc's
//!   `unconditional_panic` lint on the emitted Rust), catch-then-
//!   finally ordering, and the untouched float / integer-division
//!   behaviors.
//! - `examples/arith_uncaught.jux`: an uncaught divide-by-zero exits
//!   non-zero and prints the Java-style
//!   `Exception in thread "main" <fqn>: <message>` report.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

#[test]
fn divide_by_zero_throws_catchable_arithmetic_exception() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let source = root.join("examples").join("arith_exception.jux");
    let emit_dir = root.join("target").join("it-arith-exception");

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
            "exact: / by zero",
            "runtime: / by zero",
            "base: / by zero",
            "literal: / by zero",
            "handled",
            "cleanup",
            "2",
            "inf",
            "3.5",
        ],
        "unexpected output:\n{stdout}",
    );
}

#[test]
fn uncaught_divide_by_zero_reports_and_exits_nonzero() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let source = root.join("examples").join("arith_uncaught.jux");
    let emit_dir = root.join("target").join("it-arith-uncaught");

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
        !output.status.success(),
        "uncaught exception must exit non-zero\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
    // The part of main BEFORE the throw ran; the part after did not.
    assert!(stdout.contains("before"), "missing pre-throw output:\n{stdout}");
    assert!(!stdout.contains("after"), "post-throw code must not run:\n{stdout}");
    // Java-style uncaught report on stderr.
    assert!(
        stderr.contains(
            "Exception in thread \"main\" jux.std.exceptions.ArithmeticException: / by zero"
        ),
        "missing uncaught-exception report:\nstderr:\n{stderr}",
    );
}
