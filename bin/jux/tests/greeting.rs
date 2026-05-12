//! End-to-end test for string interpolation per §3.4.
//!
//! Exercises both interpolation forms (`$name` bare-ident and
//! `${expr}` general-expression), embedded arithmetic, indexed array
//! access inside `${...}`, and the `print($"…")` → `println!(...)`
//! specialization that flattens out the intermediate `format!()` call.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn greeting_interpolation_renders_both_forms() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("greeting.jux");
    let emit_dir = workspace_root.join("target").join("it-greeting");

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
            "Hello, Ada!",
            "Ada is 36 years old.",
            "Next year Ada will be 37.",
            "Ada's first score: 88",
            "Ada 36 3",
        ],
        "unexpected output:\n{stdout}",
    );
}
