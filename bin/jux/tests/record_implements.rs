//! End-to-end test that a record may implement an interface — the grammar
//! (§A.2.5) allows `implements` on a record, but the parser dropped the clause
//! and the backend emitted no trait impl, so assigning one to an
//! interface-typed slot was a type mismatch.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn records_may_implement_interfaces() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("record_implements.jux");
    let emit_dir = workspace_root.join("target").join("it-record-implements");

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
        ["25", "7", "3", "a25", "2", "25"],
        "unexpected output:\n{stdout}",
    );
}
