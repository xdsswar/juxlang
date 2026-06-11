//! for-each over a class's own collection field while the body re-enters and
//! mutates the same object. Regression for a borrow-soundness bug: the iterable
//! lowered to `for n in &self.0.borrow().items`, holding the read-guard across
//! the body, so any re-entrant `borrow_mut()` (even to a different field)
//! panicked `RefCell already borrowed`. The iterable is now snapshotted first.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn foreach_over_own_collection_allows_reentrant_mutation() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("foreach_reentrancy.jux");
    let emit_dir = root.join("target").join("it-foreach-reentrancy");

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
        "a RefCell-already-borrowed panic here means a for-each over an owned \
         collection field held the read-borrow across the body:\n\
         stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(
        lines.as_slice(),
        ["result size=3", "10", "20", "30", "total=6"],
        "unexpected output:\n{stdout}"
    );
}
