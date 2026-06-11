//! End-to-end test for **mutating a stdlib-collection field through a
//! shared-reference (wrapped) class** (gap N1). A `this.items.add(v)` /
//! `this.seen.put(k, v)` on a wrapped instance must take the mutable interior
//! borrow (`.0.borrow_mut()`) and evaluate a re-entrant argument before that
//! borrow — otherwise the emitted Rust either fails to compile (E0596) or
//! panics at runtime (`already borrowed`).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn wrapped_collection_mutation() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("wrapped_collection_mutation.jux");
    let emit_dir = workspace_root.join("target").join("it-wrapped-collection-mutation");

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
        ["size=2", "counter=3", "a=3", "first=1"],
        "unexpected output:\n{stdout}",
    );
}
