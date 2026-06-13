//! End-to-end test for **C6 — foreign collection pass-by-reference**
//! (Java container semantics). A function parameter whose declared type
//! is a foreign/external non-`Copy` collection (`Vec` from `rust.std`)
//! and whose body MUTATES it lowers to a Rust `&mut T`; the call site
//! passes `&mut <arg>`, so the callee's `v.push(x)` is visible to the
//! caller. The two pre-existing properties are asserted unchanged:
//! object elements stay SHARED (obj-share: 15) and primitives are
//! COPIED (prim-copy: 7).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn collection_pass_by_ref() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("collection_pass_by_ref.jux");
    let emit_dir = workspace_root
        .join("target")
        .join("it-collection-pass-by-ref");

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
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(
        lines.as_slice(),
        ["obj-share: 15", "prim-copy: 7", "container-pass: 1"],
        "unexpected output:\n{stdout}",
    );
}
