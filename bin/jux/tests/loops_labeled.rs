//! End-to-end test for **do-while** and **labeled loops** (§A.2.8).
//!
//! Runs `examples/loops_labeled.jux`:
//! - `do { } while (cond);` — body before first check, run-at-least-once;
//! - `outer: for … { continue outer; / break outer; }` across nesting;
//! - `break label;` escaping a labeled `while` from inside a nested
//!   do-while (the label lands on the right Rust loop, including the
//!   C-style for's INNER `loop` past its init scope block).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn do_while_and_labeled_loops() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("loops_labeled.jux");
    let emit_dir = workspace_root.join("target").join("it-loops-labeled");

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
        ["0", "1", "2", "once", "0", "1", "10", "11", "done"],
        "unexpected output:\n{stdout}",
    );
}
