//! End-to-end test for METHOD OVERLOADING ACROSS INHERITANCE. A subclass may
//! override one member of an inherited overload group, add a new member, or
//! both; a base-typed reference dispatches to the override for the members the
//! base declares, and the subclass's own additions stay off that surface.
//! Extending a class that overloaded anything used to be rejected outright
//! with `[E0450]`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn overloading_composes_with_inheritance() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("overload_inheritance.jux");
    let emit_dir = workspace_root.join("target").join("it-overload-inheritance");

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
            "base:none",
            "loud:int 7",
            "base:str x",
            "tag[base:str y]",
            "tag:pair 1z",
            "base:none",
        ],
        "unexpected output:\n{stdout}",
    );
}
