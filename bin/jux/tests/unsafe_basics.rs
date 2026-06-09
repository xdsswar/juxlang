//! End-to-end test for the `unsafe` feature.
//!
//! Runs `examples/unsafe_basics.jux`. Exercises:
//! - `unsafe int f(int)` lowering to a Rust `unsafe fn`.
//! - `unsafe { ... }` block lowering to a Rust `unsafe { ... }` block.
//! - calling an `unsafe` fn from another `unsafe` fn body (unsafe context).
//!
//! The same machinery is what lets a foreign `unsafe` Rust binding be called
//! from Jux without leaking rustc's E0133 — the front end requires the
//! `unsafe` opt-in, and the call-site enforcement (E0506) is unit-tested in
//! `juxc-tycheck`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn unsafe_fn_and_block_run() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("unsafe_basics.jux");
    let emit_dir = workspace_root.join("target").join("it-unsafe-basics");

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
    // rawDouble(10) = 20; rawQuad(3) = rawDouble(rawDouble(3)) = 12; total = 32.
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(lines.as_slice(), ["unsafe result: 32"], "unexpected output:\n{stdout}");
}
