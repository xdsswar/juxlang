//! End-to-end test for inherited interface `default` methods called on
//! a concrete class (§7.4.3).
//!
//! Runs `examples/interface_default_inherit.jux`. Regression for the
//! E0599 codegen gap: a `default` interface method lowered to a Rust
//! trait-default body only, so `obj.label()` on the concrete (wrapper)
//! class failed to resolve inherent-first ("trait `Holder` … not in
//! scope"). The backend now emits a fully-qualified forwarding inherent
//! method, so the call resolves like an overridden method while reusing
//! the trait default's body.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn interface_default_inherited_on_concrete_class() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("interface_default_inherit.jux");
    let emit_dir = workspace_root
        .join("target")
        .join("it-interface-default-inherit");

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
        ["label=Holder.default", "lastPut=42", "== done =="],
        "unexpected output:\n{stdout}",
    );
}
