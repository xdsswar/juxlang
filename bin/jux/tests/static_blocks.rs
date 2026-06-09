//! End-to-end test for static initializer blocks (`static { }`, §S.4.1).
//!
//! Runs `examples/static_blocks.jux`. Exercises:
//! - a `static { }` block triggered by the first observable use — here a bare
//!   static-FIELD READ, before any construction or static call.
//! - the once-guard: later uses do not re-run it.
//! - a static method reading static fields the block initialized.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn static_block_runs_once_on_first_use() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("static_blocks.jux");
    let emit_dir = workspace_root.join("target").join("it-static-blocks");

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
            "static init: configured",
            "first read: retries=3",
            "after construct: mode=production, retries=3",
        ],
        "static block should run once, triggered by the first static-field read:\n{stdout}",
    );
}
