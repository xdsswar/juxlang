//! Negative: a GENERIC class used as a polymorphic base is a Phase-1 limitation.
//! juxc must reject it with its own [E0454] (clean span) instead of leaking a
//! rustc E0277/E0308 on the generated code. The supported generic-dispatch route
//! is a generic interface (covered by `generic_iface_poly`).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generic_base_class_polymorphism_is_rejected() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("generic_base_unsupported.jux");
    let emit_dir = root.join("target").join("it-generic-base-unsupported");

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
    assert!(!output.status.success(), "generic base unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0454]"), "expected E0454, got:\n{all}");
    // The juxc diagnostic must replace the leaked rustc type errors.
    assert!(!all.contains("E0277"), "rustc trait error leaked:\n{all}");
    assert!(!all.contains("E0308"), "rustc type error leaked:\n{all}");
}
