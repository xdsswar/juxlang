//! End-to-end test for stage-2 polymorphism — VIRTUAL DISPATCH through a
//! non-sealed class-hierarchy reference.
//!
//! Runs `examples/polymorphism.jux`. Unlike `animals.rs` (which dispatches
//! through concrete subclass references), every call here goes through a
//! base-class-typed value — lowered to `Rc<dyn AnimalKind>` — so the override
//! that runs is chosen at run time. Exercises:
//! - override fired through a base-typed local / parameter (`announce(Animal)`),
//! - an inherited template method (`describe`) calling a virtually-dispatched
//!   method (`sound`) through the base reference,
//! - a 3-level hierarchy (`Dog`/`Bat` → `Mammal` → `Animal`),
//! - `super.method()` (`Bat.sound` calls `super.sound()` = `Mammal`'s version),
//! - a base-class return type (the `make` factory),
//! - a heterogeneous `Animal[]` iterated with per-element dispatch,
//! - a mid-hierarchy `Mammal` reference reaching inherited + own methods.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn virtual_dispatch_through_non_sealed_base_references() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("polymorphism.jux");
    let emit_dir = workspace_root.join("target").join("it-polymorphism");

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
            "Rex says Woof",
            "Echo says some mammal noise (squeak)",
            "Woof / legs=4",
            "Woof",
            "some mammal noise (squeak)",
        ],
        "unexpected output:\n{stdout}",
    );
}
