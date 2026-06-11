//! Safe-navigation (`?.`) over wrapped classes and multi-level chains. Regression
//! for three lowering bugs found by the bug-hunt: (1) `?.field` on a wrapped
//! (Rc<RefCell>) class read the field as a tuple slot (`__t.field`) instead of
//! through `.0.borrow()` → rustc E0609; (2) a `?.method()` chain whose links
//! return `T?` used `.map` instead of `.and_then`, so the next `?.` operated on
//! `Option<Inner>` → E0599; (3) the same `.map`/`borrow` misses on `?.field`
//! chains two levels deep, whose intermediate spans tycheck doesn't record.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn safe_nav_chains_over_wrapped_classes() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("safe_nav_chains.jux");
    let emit_dir = root.join("target").join("it-safe-nav-chains");

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
        "compile/run failed (a leaked E0609/E0599 means a `?.` lowering bug \
         regressed):\nstderr:\n{stderr}\nstdout:\n{stdout}"
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(
        lines.as_slice(),
        ["200", "-1", "5", "-1", "7", "7", "-2"],
        "unexpected output:\n{stdout}"
    );
}
