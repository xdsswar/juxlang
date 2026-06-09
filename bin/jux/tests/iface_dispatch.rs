//! End-to-end test for stage-1 interface **dynamic dispatch**.
//!
//! Runs `examples/iface_dispatch.jux`. Unlike `shapes.rs` (which dispatches
//! through concrete `Circle` / `Square` references), every call here goes
//! through a `Shape`-typed value — lowered to `Rc<dyn Shape>` — so the
//! override that runs is selected at run time by the value's concrete type.
//! Exercises interface values in every flow position:
//! - interface-typed local bound to a concrete class (`Shape a = new Circle`),
//! - interface-typed parameter (`describe(Shape s)`),
//! - interface-typed return (the `make` factory),
//! - interface-typed field (`Scene.first`),
//! - an array of interface values (`Shape[]`),
//! - reassignment of an interface local to a different concrete type.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn polymorphic_dispatch_through_interface_references() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("iface_dispatch.jux");
    let emit_dir = workspace_root.join("target").join("it-iface-dispatch");

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
            "Circle area=3.14",
            "Circle area=12.56",
            "Square area=9",
            "scene starts with Square",
            "Circle area=12.56",
            "Square area=16",
            "Square area=25",
        ],
        "unexpected output:\n{stdout}",
    );
}
