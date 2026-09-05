//! End-to-end test for every import shape over a FOREIGN package: a single
//! name, a group, a group with per-item aliases, and a wildcard. The grouped
//! form used to fall through to the generic renderer and emit
//! `use rust::std::HashMap;` — a module that does not exist.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn every_import_shape_resolves_to_a_real_rust_path() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("imports_grouped.jux");
    let emit_dir = workspace_root.join("target").join("it-imports-grouped");

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
        ["1", "1", "1", "1", "1", "1"],
        "unexpected output:\n{stdout}",
    );
}
