//! End-to-end integration test for milestone-6 (unary operators).
//!
//! Runs `jux run examples/abs.jux` and asserts the two `abs` calls
//! print `7` and `5` respectively (in that order).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn abs_negates_and_passes_through() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("abs.jux");
    let emit_dir = workspace_root.join("target").join("it-abs");

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
    // Both `7` and `5` should appear, in that order.
    let p7 = stdout.find('7').expect("`7` not in stdout");
    let p5 = stdout.find('5').expect("`5` not in stdout");
    assert!(p7 < p5, "expected 7 before 5, got:\n{stdout}");
}
