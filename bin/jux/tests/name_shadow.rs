//! Regression for **package-aware name resolution**: a user class whose bare
//! name collides with an auto-loaded `rust.std` stub (`Command` ~
//! `std::process::Command`) must shadow the stub and resolve to the user's
//! class, deterministically (the collision previously produced flaky codegen).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn name_shadow() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("name_shadow.jux");
    let emit_dir = workspace_root.join("target").join("it-name-shadow");

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
        ["deploy #1", "deploy #2", "total 2"],
        "unexpected output:\n{stdout}",
    );
}
