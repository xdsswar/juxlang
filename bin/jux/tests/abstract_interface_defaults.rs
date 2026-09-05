//! End-to-end test for `examples/abstract_interface_defaults.jux` — an abstract
//! class between an interface and its concrete subclasses.
//!
//! Rust has no partial `impl`, so an abstract class used to emit no trait impl
//! at all, and its own concrete method could not call an interface `default`
//! (`this.tag()` → E0599, "no method named `tag` found"). It now implements
//! every interface it can complete, marking anything it leaves to its
//! subclasses unreachable — which it is, because an abstract class has no
//! values. This test covers both halves: `Base` writes the interface's required
//! method, `Shape` does not, and both call the default.
//!
//! It also covers `@Override` on a method that satisfies an interface reached
//! through the `extends` chain rather than the class's own `implements` clause
//! (`Tri.kindOf` implements `Shape`'s `Named`), which used to be E0426.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn an_abstract_class_can_call_an_interface_default() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("abstract_interface_defaults.jux");
    let emit_dir = workspace_root
        .join("target")
        .join("it-abstract-interface-defaults");

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
            // the abstract class satisfies `Tagged`, so its `describe()` runs
            // the interface's default against its own `id()`
            "t5",
            // the abstract class leaves `kindOf` open; the default still
            // dispatches to the subclass that writes it
            "<tri>:3",
            "<tri>",
            // and a subclass may override the default itself
            "[quad]:4",
        ],
        "unexpected output:\n{stdout}",
    );
}
