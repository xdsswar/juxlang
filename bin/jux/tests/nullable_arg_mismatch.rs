//! Negative: a nullable `Animal?` value flowing into a non-null `Animal`
//! parameter slot must be rejected by juxc itself with [E0410], not leak a rustc
//! type error on the generated code (the "juxc catches its own errors"
//! initiative).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn nullable_into_nonnull_param_is_rejected() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("nullable_arg_mismatch.jux");
    let emit_dir = root.join("target").join("it-nullable-arg-mismatch");

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
    assert!(!output.status.success(), "nullable-into-nonnull unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0410]"), "expected E0410, got:\n{all}");
    assert!(!all.contains("E0308"), "rustc type error leaked:\n{all}");
}
