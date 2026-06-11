//! End-to-end test for the **user Iterable/Iterator protocol**
//! (§O.6, §K.5): a class implementing `Iterable<int>` driven by
//! for-each, with state threaded through `T? next()`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn user_iterable_for_each() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("user_iterable.jux");
    let emit_dir = workspace_root.join("target").join("it-user-iterable");

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
        ["3", "2", "1", "10"],
        "unexpected output:\n{stdout}",
    );
}
