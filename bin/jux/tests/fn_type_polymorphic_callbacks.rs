//! Regression for function types over a POLYMORPHIC class (§T.3.6).
//!
//! `void use((Animal) -> int f)` lowers to `Rc<dyn Fn(Rc<dyn AnimalKind>) -> isize>`
//! as soon as one subclass of `Animal` exists, because that is the slot a
//! base-typed value lives in. Three pieces of that did not line up, and each one
//! leaked a raw rustc error:
//!
//! - a lambda with a WRITTEN parameter type emitted `move |a: Animal|`, a
//!   concrete struct where the slot wanted the handle (E0631);
//! - a call through a fn-typed PARAMETER passed `Animal::new()` straight into
//!   the handle slot (E0308), because the backend could only find a function
//!   type for a fn-typed LOCAL;
//! - a call through a fn-typed FIELD did not convert its arguments at all.
//!
//! Deleting `class Dog` made the identical program compile, which is what makes
//! this worth pinning: the failure appears when a second class is added, far
//! from the callback.

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn fn_type_polymorphic_callbacks_runs() {
    let source = root().join("examples").join("fn_type_polymorphic_callbacks.jux");
    let emit_dir = root().join("target").join("it-fn-type-polymorphic-callbacks");
    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
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
        .filter(|s| !s.is_empty() && !s.starts_with("jux: "))
        .collect();
    assert_eq!(
        lines,
        [
            // `describe` with a written lambda parameter type: dispatch reaches
            // each subclass's own `kind()`.
            "animal", "dog", "cat",
            // ...and again with an inferred one, reading a base field too.
            "animal/1", "dog/1", "cat/1",
            // A lambda RESULT converted into the polymorphic base slot.
            "dog",
            // An interface-typed callback parameter.
            "tagged",
            // A callback held in a FIELD, called with a fresh subclass.
            "fmt:dog",
            // A callback held in a local.
            "local:cat",
        ],
    );
}
