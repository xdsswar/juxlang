//! The testing framework's own demo (`JUX-TESTING-ADDENDUM` §TS).
//!
//! `jux test` works on a PROJECT — it searches upward for a `jux.toml` rather
//! than taking a file — so the demo is one. As a loose `.jux` it was
//! unreachable by either command: `jux run` refused it for having no `main`,
//! and `jux test` had no way to be pointed at it.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn jux_test_discovers_and_runs_the_demo() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(root.join("examples").join("testing_demo"))
        .arg("test")
        .output()
        .expect("spawn jux test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let all = format!("{stdout}{stderr}");
    assert!(
        output.status.success(),
        "jux test exited with {:?}\n{all}",
        output.status.code(),
    );
    for needle in [
        "running 3 tests",
        "PASS testing.numbersAddUp",
        "PASS testing.nullsAreDetected",
        "PASS testing.divisionThrows",
        "3 passed; 0 failed",
    ] {
        assert!(all.contains(needle), "expected {needle:?} in:\n{all}");
    }
}
