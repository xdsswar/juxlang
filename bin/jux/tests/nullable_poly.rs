//! End-to-end test for **nullable polymorphic-base / interface values** —
//! `T?` slots holding a `Rc<dyn …>` trait object.
//!
//! Runs `examples/nullable_poly.jux`. Exercises every coercion site that lifts
//! a concrete subtype into a nullable dyn slot (var-init, return, param,
//! constructor field, reassignment) plus the reverse — an already-nullable
//! value (`return this.field;`) flowing through without a double `Some(...)` —
//! and a downcast from a nullable-narrowed local. Guards against the rustc
//! leaks (`expected Rc<dyn …>, found Option<…>`, `Some(Some(...))`) the
//! nullable-dyn coercion fix closed.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn nullable_polymorphic_values_coerce_without_leaks() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("nullable_poly.jux");
    let emit_dir = workspace_root.join("target").join("it-nullable-poly");

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
        [
            "woof",      // Animal? a = new Dog();   (nullable poly-base var-init)
            "dog-tag",   // Tagged? t = new Dog();   (nullable interface var-init)
            "woof",      // Animal? r = makeAnimal(); (nullable return into nullable local)
            "tagged ok", // makeTagged() != null
            "meow",      // a = new Cat();           (nullable poly-base reassign)
            "woof",      // announce(new Dog());     (nullable poly-base param)
            "meow",      // s.get() -> this.resident (already-nullable field return)
            "empty",     // Animal? none = null;
            "fetched",   // (Dog) maybe              (downcast from nullable-narrowed local)
        ],
        "unexpected output:\n{stdout}",
    );
}
