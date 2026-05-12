//! End-to-end integration test for milestone-8 (typed locals + suffixed
//! int + float literals).
//!
//! Runs `jux run examples/primitives.jux` and asserts stdout contains
//! the three expected values in order: `1000000`, `3.14`, `1.5`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn primitives_example_prints_long_double_and_float() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("primitives.jux");
    let emit_dir = workspace_root.join("target").join("it-primitives");

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
    let p_long  = stdout.find("1000000").expect("missing long");
    let p_pi    = stdout.find("3.14").expect("missing double");
    let p_small = stdout.find("1.5").expect("missing float");
    assert!(p_long < p_pi && p_pi < p_small, "wrong order:\n{stdout}");
}
