//! End-to-end test for instance initializer blocks (`init { }`, §M.1).
//!
//! Runs `examples/init_blocks.jux`. Exercises:
//! - an `init` block running after the constructor body on every `new`.
//! - the init block reading constructor-set fields and writing a derived field.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn init_blocks_run_after_constructor() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("init_blocks.jux");
    let emit_dir = workspace_root.join("target").join("it-init-blocks");

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
            "init: rect 3x4 -> area 12",
            "init: rect 5x6 -> area 30",
            "areas: 12 and 30",
        ],
        "unexpected output:\n{stdout}",
    );
}
