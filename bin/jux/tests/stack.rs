//! End-to-end test for push/pop methods on dynamic arrays. Runs
//! `examples/stack.jux` and confirms the stack semantics — pushing
//! three values then popping two yields the expected length
//! transitions and the LIFO values.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn stack_push_and_pop_produce_lifo_values_with_length_tracking() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("stack.jux");
    let emit_dir = workspace_root.join("target").join("it-stack");

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
        // length=3 → pop returns 3 → length=2 → pop returns 2 → length=1
        ["3", "3", "2", "2", "1"],
        "unexpected output:\n{stdout}",
    );
}
