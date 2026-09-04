//! End-to-end test for the `Display` bound on a generic value formatted by a
//! method INHERITED from a generic base. The bound has to reach the subclass's
//! impl as well; without it the interpolation resolves to `Debug` and a
//! `String` prints wrapped in quotes.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generic_value_formatted_by_an_inherited_method() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("generic_inherited_display.jux");
    let emit_dir = workspace_root.join("target").join("it-generic-inherited-display");

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
    assert_eq!(lines.as_slice(), ["holding abc", "HOLDING ABC"], "unexpected output:
{stdout}");
}
