//! Negative (J4): an override that writes the supertype's type-parameter name
//! (`void test(T t)`) instead of the bound concrete argument (`void test(int t)`
//! under `implements Holder<int>`) must be rejected by juxc itself with [E0417],
//! NOT leak a rustc `E0412 cannot find type T` on the generated code (the "juxc
//! catches its own errors" initiative).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn unknown_type_in_override_is_rejected() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("unknown_type_in_override.jux");
    let emit_dir = root.join("target").join("it-unknown-type-in-override");

    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("spawn jux");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let all = format!("{stdout}{stderr}");
    assert!(!output.status.success(), "unknown-type override unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0417]"), "expected E0417, got:\n{all}");
    // The unresolved name must never reach rustc.
    assert!(!all.contains("E0412"), "rustc cannot-find-type leaked:\n{all}");
}
