//! End-to-end test for "finish polymorphism": explicit downcast, the `=>`
//! type-test / smart-cast, and field access through a base reference.
//!
//! Runs `examples/downcast_typetest.jux`. Exercises:
//! - reading and writing a public field through a base reference (via the
//!   generated `__get_`/`__set_` accessors),
//! - explicit downcast in both syntaxes (`(Dog) a` and `a as Dog`) then a
//!   concrete-only method call,
//! - the bare `=>` boolean test,
//! - the `=>` smart-cast binder in an `else if` (`else if (b => Cat cc)`),
//! - an interface sidecast through a base reference (`b => Tagged t`).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn downcast_typetest_and_field_access_through_base() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("downcast_typetest.jux");
    let emit_dir = workspace_root.join("target").join("it-downcast-typetest");

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
            "Rex",
            "Max",
            "fetched",
            "Woof",
            "a is a Dog",
            "Felix says Meow / cat-tag",
            "tagged: cat-tag",
        ],
        "unexpected output:\n{stdout}",
    );
}
