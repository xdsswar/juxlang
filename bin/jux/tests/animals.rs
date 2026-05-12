//! End-to-end test for Turn-1 inheritance.
//!
//! Runs `examples/animals.jux`. Exercises:
//! - `abstract class Animal { … abstract String speak(); … }`.
//! - `class Dog extends Animal { … super(name); … }`.
//! - Inherited `getName()` called on the subclass — auto-deref through
//!   `impl Deref<Target = Animal> for Dog`.
//! - Subclass override of `speak()` shadows the abstract stub.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn abstract_animal_with_dog_and_cat_subclasses_dispatch_correctly() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("animals.jux");
    let emit_dir = workspace_root.join("target").join("it-animals");

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
        ["Rex says Woof!", "Whiskers says Meow!"],
        "unexpected output:\n{stdout}",
    );
}
