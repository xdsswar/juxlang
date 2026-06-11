//! Polymorphic-base variable coercion + smart-cast binding survival.
//!
//! `Animal a = new Dog()` lowers `Animal` to `Rc<dyn AnimalKind>`; a later
//! `Animal other = new Animal()` constructs the base itself and must still be
//! wrapped into the trait object (a fresh construction is concrete, not an
//! existing `Rc<dyn>` handle) — otherwise a rustc E0308 leaks. The smart-cast
//! binding `d` from `if (a => Dog d)` is a fresh name and survives reassignment
//! of the scrutinee `a` (§T.6.3). Correct output: `woof`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn base_var_coercion_and_smartcast_survives_reassign() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("smartcast_reassign.jux");
    let emit_dir = root.join("target").join("it-smartcast-reassign");

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
        "compile/run failed (missing base-type→trait-object coercion leaks an \
         E0308 here):\nstderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(stdout.contains("woof"), "unexpected output:\n{stdout}");
}
