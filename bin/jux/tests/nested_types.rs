//! End-to-end test for **static nested types** (§M.9): qualified
//! `new Outer.Inner()` construction, unqualified sibling access from
//! inside the owner, same-named nested types in different owners,
//! depth-2 nesting, and nested records.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn nested_types() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("nested_types.jux");
    let emit_dir = workspace_root.join("target").join("it-nested-types");

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
            "8080", "localhost:9090", "localhost:9090",
            "4", "true", "db01", "5432",
        ],
        "unexpected output:\n{stdout}",
    );
}
