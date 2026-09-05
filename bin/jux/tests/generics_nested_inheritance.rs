//! End-to-end test for `examples/generics_nested_inheritance.jux` — the second
//! generics × inheritance matrix, covering what the first did not reach.
//!
//! Three defects it pinned down:
//!
//! - **A generic interface's `default` body needs the bounds it uses.** The body
//!   is emitted on the trait, so `default String describe() { return "store of "
//!   + this.load(); }` on `Store<T>` needs `T: Display` on `Store` itself — and
//!   then on every class that implements it and on the `Rc<T>` forwarder.
//! - **A foreign generic with defaulted parameters lost its arguments.**
//!   `HashMap<K, V, S = RandomState, A = Global>` reaches the stub with four
//!   parameters while Jux writes two; substitution required equal lengths, so
//!   `m.get(k)` inferred `Unknown` and the whole nullable/widening ladder
//!   stopped applying. Binding is positional now.
//! - **A mutating foreign method is no longer hoisted.** `this.buckets.get_mut(k)`
//!   was bound into a temp — a clone of the field — so the mutation landed on a
//!   copy, and binding out of the `RefMut` did not compile at all.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn nested_generics_compose_with_interfaces_bounds_and_overrides() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("generics_nested_inheritance.jux");
    let emit_dir = workspace_root
        .join("target")
        .join("it-generics-nested-inheritance");

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
            // the abstract level's own method calling the generic interface's
            // default, and the interface that extends it with a concrete arg
            "store of alpha x2",
            "alpha!",
            "store of alpha",
            // the same two through interface-typed handles
            "alpha",
            "alpha!",
            // HashMap<K, Vec<V>> — a container of containers, keyed generically
            "2",
            "0",
            // a generic method with its own bounded parameter, then a static
            // method on the generic class
            "2",
            "Index",
            // an inherited generic method overridden, reaching the base
            "H(9)!",
            // `? super` against a class with a generic base
            "1",
        ],
        "unexpected output:\n{stdout}",
    );
}
