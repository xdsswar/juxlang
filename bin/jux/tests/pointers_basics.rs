//! End-to-end test for raw pointers.
//!
//! Runs `examples/pointers_basics.jux`. Exercises:
//! - `T*` pointer type lowering to Rust `*mut T`.
//! - address-of `&x` lowering to `core::ptr::addr_of_mut!(x)`.
//! - dereference `*p` (read and write-through) inside an `unsafe` context.
//! - passing a pointer to an `unsafe` function.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn raw_pointers_round_trip() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("pointers_basics.jux");
    let emit_dir = workspace_root.join("target").join("it-pointers-basics");

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
    // n = 10; doubled = *p * 2 = 20; store(p, 20) writes it back.
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(lines.as_slice(), ["n after pointer write: 20"], "unexpected output:\n{stdout}");
}
