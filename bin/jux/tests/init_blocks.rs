//! End-to-end test for instance initializer blocks (`init { }`, §M.1).
//!
//! Runs `examples/init_blocks.jux`. Exercises the §S.4.4 / ERRATA E2
//! construction order — super → field initializers → init blocks →
//! constructor body (Java's instance-initializer rule):
//! - init blocks run BEFORE the constructor body and see
//!   field-initializer values, not the body's writes;
//! - multiple init blocks run in textual order;
//! - every constructor runs every init block.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn init_blocks_run_before_constructor_body() {
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
            "init sees count=100 tag=fresh",
            "second init block, in textual order",
            "body(int)",
            "a: count=7 tag=explicit inits=1",
            "init sees count=100 tag=fresh",
            "second init block, in textual order",
            "body()",
            "b: count=100 tag=fresh",
        ],
        "unexpected output:\n{stdout}",
    );
}
