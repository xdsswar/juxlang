//! End-to-end test that `?.` and `??` short-circuit.
//!
//! `??` lowered to Rust's `unwrap_or`, which takes its argument by value and
//! therefore evaluates it whether or not it is needed -- so the fallback's side
//! effects happened even when the value was present, silently. `?.` on a
//! non-nullable receiver emitted `recv.as_ref()`, which a plain struct has no
//! method for, so the program did not build.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn null_safe_operators_short_circuit() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("null_safe_operators.jux");
    let emit_dir = workspace_root.join("target").join("it-null-safe-operators");

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
            "5",  // the value is present
            "0",  // ... so the fallback never ran
            "9",  // the value is missing
            "1",  // ... so the fallback ran, exactly once
            "-1", // a null receiver skips the call and its side effect
            "1",  // a receiver that cannot be null is simply called
        ],
        "unexpected output:\n{stdout}",
    );
}
