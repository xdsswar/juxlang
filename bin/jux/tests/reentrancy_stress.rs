//! Re-entrancy borrow-soundness guard (gap-analysis G3). Classes lower to
//! `Rc<RefCell>`; a method that, directly or through a callee, re-enters and
//! mutates the SAME object must not hold a `.0.borrow()` guard across the call
//! (else `BorrowMutError`). This drives that adversarial shape and must run
//! clean. The repeated `value=3` / `sum=6` lines are correct shared-mutable
//! reference semantics: every re-entrant frame observes the same final field.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn reentrancy_stress() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("reentrancy_stress.jux");
    let emit_dir = workspace_root.join("target").join("it-reentrancy");

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
        "jux exited with {:?} (a BorrowMutError panic here means the borrow \
         discipline broke under re-entrancy)\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(
        lines.as_slice(),
        ["value=3", "value=3", "value=3", "sum=6", "sum=6", "sum=6"],
        "unexpected output:\n{stdout}",
    );
}
