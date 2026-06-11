//! End-to-end test for **`drop { }` destructors** (§6.6 / §S.5):
//! deterministic scope-exit drops, and once-per-instance semantics
//! for aliased class handles (the destructor runs when the LAST
//! strong reference releases).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn drop_blocks() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("drop_blocks.jux");
    let emit_dir = workspace_root.join("target").join("it-drop-blocks");

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
            "open a",
            "open inner",
            "using inner",
            "close inner",
            "aliased a",
            "end of main",
            "close a",
        ],
        "unexpected output:\n{stdout}",
    );
}
