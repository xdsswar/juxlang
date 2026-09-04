//! End-to-end test for a **generic class used as a polymorphic base**. A
//! `Container<int>` slot holding a `Box<int>` lowers to the generic trait
//! object `Rc<dyn ContainerKind<isize>>`, so the override dispatches, the
//! inherited getter reads through the base reference, the downcast comes back
//! to the concrete subclass, and a `Vec<Container<int>>` holds either subclass.
//! This construct used to be rejected up front with `[E0454]`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generic_base_class_polymorphism() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("generic_base_poly.jux");
    let emit_dir = workspace_root.join("target").join("it-generic-base-poly");

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
        [
            "box of 7",
            "7",
            "crate of 9",
            "b is a Box",
            "7",
            "box of 1",
            "crate of 2",
        ],
        "unexpected output:\n{stdout}",
    );
}
