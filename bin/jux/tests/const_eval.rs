//! End-to-end test for **compile-time const-expression evaluation** (§T.11):
//! const-binding initializers (incl. a call to a const-evaluable function),
//! fixed-array sizes, and const-generic arguments all fold to concrete literals
//! in the emitted Rust and run to the expected values.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn const_eval() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("const_eval.jux");
    let emit_dir = workspace_root.join("target").join("it-const-eval");

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
        ["2048", "33", "64", "32"],
        "unexpected output:\n{stdout}",
    );
}
