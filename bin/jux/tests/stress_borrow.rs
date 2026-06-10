//! End-to-end **borrow-checker stress** test.
//!
//! Runs `examples/stress_borrow.jux` — a battery of adversarial
//! aliasing/mutation shapes that Java permits freely and the
//! `Rc<RefCell>` wrapper model must lower without leaking rustc borrow
//! errors. Every section was originally a leaking probe:
//! - nested mutating call (`a.addTwice(a.bump())`) → was E0499,
//! - lambda capture + mutation (`() -> { c.inc(); }`) → was E0596/E0382,
//! - static field holding an object → was E0277 (`Rc` is `!Send`),
//! - one object in two `ArrayList`s → was E0382 (move into `push`),
//! - cyclic `Node` + `!!` assertion chains → was E0072 / parse error.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn borrow_stress_battery() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("stress_borrow.jux");
    let emit_dir = workspace_root.join("target").join("it-stress-borrow");

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
        ["3", "2", "2", "1", "1", "2", "1", "99"],
        "unexpected output:\n{stdout}",
    );
}
