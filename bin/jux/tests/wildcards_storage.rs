//! End-to-end test for the **storage-position wildcard boundary**.
//!
//! Runs `examples/wildcards_storage.jux`, which demonstrates the two
//! storage forms that DO work — a field with a concrete type argument
//! (`Bag<Dog>`) and a param-position wildcard — while documenting that
//! a wildcard storage slot (`Bag<? extends Animal>` as a field) is
//! rejected with E0444 (covered separately by a tycheck unit test).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn concrete_storage_and_param_wildcard() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("wildcards_storage.jux");
    let emit_dir = workspace_root.join("target").join("it-wildcards-storage");

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
        ["Rex", "got a bag of animals", "done"],
        "unexpected output:\n{stdout}",
    );
}
