//! End-to-end test for `examples/generics_full_matrix.jux` — generics crossed
//! with every construct that composes with them, and inheritance in every shape
//! Jux permits, in ONE file.
//!
//! What it pins down, beyond "it runs":
//!
//! - **An abstract class calling an interface `default`.** `Shape.describe()`
//!   calls `this.tag()`, which only `IdNamed` defines. An abstract class emits
//!   no ordinary trait impl, so this used to have nothing to resolve against;
//!   it now implements what it can and marks the rest unreachable.
//! - **`@Override` on a method that satisfies an interface reached through the
//!   `extends` chain** — `Round.kindOf()` implements `Shape`'s `IdNamed`.
//! - **A polymorphic base's `Kind` trait must not shadow its interface.**
//!   `Node` implements `Source<V>`, so `NodeKind<K, V>: Source<V>`; declaring
//!   `take` on both makes `handle.take()` ambiguous (E0034).
//! - **A trait impl's generic bounds must match the inherent impl's.** `Node`
//!   keys a `HashMap` on `K`, so its inherent impl carries `K: Eq + Hash`. A
//!   weaker bound on `impl Source<V> for Node<K, V>` put the inherent method
//!   out of scope and turned the delegating body into infinite recursion — a
//!   lint, not an error, so it built and overflowed the stack at runtime.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generics_and_inheritance_compose_in_every_permitted_shape() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("generics_full_matrix.jux");
    let emit_dir = workspace_root.join("target").join("it-generics-full-matrix");

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
            // arity 1 / 3, a generic method, a const generic, an
            // intersection bound
            "1",
            "1|y|2.5",
            "c=2.5",
            "4",
            "i4#4",
            // a generic polymorphic base: recursion over children, a virtual
            // call, and an upcast to the generic interface it implements
            "3",
            "Node",
            "r",
            // a 4-level chain ending in `final`, then the abstract level's own
            // `describe()` reached through it
            "Dot#7",
            "Round#1",
            // sealed + permits
            "black",
            // the three wildcard forms
            "Woof",
            "any",
            "Woof",
            // two classes that reference each other
            "ping+pong",
        ],
        "unexpected output:\n{stdout}",
    );
}
