//! End-to-end test for the **wrapping arithmetic operators** (§S.2.1):
//! `+%` `-%` `*%` `<<%` `>>%` — unconditional two's-complement wrap at
//! the operand's exact width (byte wraps at 8 bits, long at 64).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn wrapping_operators() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("wrapping_operators.jux");
    let emit_dir = workspace_root.join("target").join("it-wrapping-operators");

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
        ["8", "-10", "-128", "-2", "-9223372036854775808", "8", "4"],
        "unexpected output:\n{stdout}",
    );
}
