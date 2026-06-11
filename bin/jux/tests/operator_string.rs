//! End-to-end test for **`operator string()` formatting** (§O.2.2 /
//! §O.7.1) and the **§O.4.1 identity default** (`Class@<addr>` for
//! classes without the operator).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn operator_string_and_identity_default() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("operator_string.jux");
    let emit_dir = workspace_root.join("target").join("it-operator-string");

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
        ["(3, 4)", "at (3, 4)", "point: (3, 4)", "true"],
        "unexpected output:\n{stdout}",
    );
}
