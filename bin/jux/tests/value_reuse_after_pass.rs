//! End-to-end test for Java/C# value semantics over Rust ownership: a binding
//! stays readable after it is passed, stored in a field, put in an array or
//! pushed into a collection. Every one of those positions used to MOVE the
//! value and leak a raw rustc E0382 on the next read. The emitter now copies at
//! a read that still has a later reader and moves at the last one, so the
//! source never has to mention ownership — and a value passed exactly once
//! still lowers to a plain move.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn a_value_stays_readable_after_it_is_passed() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("value_reuse_after_pass.jux");
    let emit_dir = workspace_root.join("target").join("it-value-reuse-after-pass");

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
        "jux exited with {:?}
stderr:
{stderr}
stdout:
{stdout}",
        output.status.code(),
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(lines.as_slice(), [
            "apple",
            "apple",
            "apple/apple",
            "tea/tea",
            "appleteaappletea",
            "appleapple",
            "appleapple",
            "2",
        ], "unexpected output:
{stdout}");
}
