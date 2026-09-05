//! End-to-end test for `examples/generics_declaration_kinds.jux` — generics and
//! inheritance crossed with the declaration kinds that are not classes.
//!
//! What it pinned down:
//!
//! - **`enum X implements I`.** §A.2.5 has allowed it all along; the parser
//!   never accepted it, so an enum could not satisfy an interface, flow into an
//!   interface-typed slot, or be passed to an interface parameter.
//! - **Annotations on record and enum members.** `@Override` in front of a
//!   record or enum method was a parse error, which is exactly the annotation
//!   those members need once they may implement an interface.
//! - **A nullable value out of a conditional.** `return has ? value : fallback;`
//!   in a `T?`-returning method wrapped the WHOLE conditional in `Some(…)`,
//!   which does not fit an arm that is already `Option`-shaped. The wrap now
//!   goes to the arms, which is where the switch expression already put it.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn records_enums_and_bounds_compose_with_generics() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("generics_declaration_kinds.jux");
    let emit_dir = workspace_root
        .join("target")
        .join("it-generics-declaration-kinds");

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
            // two generic interfaces on one class, one contributing a default
            "1",
            "L=one",
            // a record, a generic record, and an enum implementing an interface
            "t",
            "cell(5)",
            "high",
            // an F-bounded parameter: 9 outranks the pivot, 1 does not
            "1",
            // three levels of nesting, mutated through the middle one
            "1",
            // a nullable generic out of a conditional: present, then absent
            "x",
            "none",
            // a generic method and a concrete overload of the same name
            "T:7",
            "S:hi",
            // a lambda whose parameter and result are the caller's parameters
            "n=3",
        ],
        "unexpected output:\n{stdout}",
    );
}
