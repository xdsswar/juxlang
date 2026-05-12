//! End-to-end test for Turn-1 pattern matching — expression-form
//! `switch` returning a String, with payload-binding patterns
//! (`var n`, `var w`) destructuring tuple-variant payloads and
//! interpolation rendering the bound names.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn expression_form_switch_with_payload_binding_renders_each_variant() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("match_payload.jux");
    let emit_dir = workspace_root.join("target").join("it-match-payload");

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
        ["num=42", "word=hi", "stop"],
        "unexpected output:\n{stdout}",
    );
}
