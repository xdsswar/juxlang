//! End-to-end test for `examples/access_control.jux` — JLS §6.6 access
//! control, plus the nested-type resolution that using it depends on.
//!
//! Two rules were read more restrictively than Java defines them, and in both
//! cases the modifier ended up meaning something no reader would expect:
//!
//! - `private` was scoped to the immediately declaring class rather than the
//!   enclosing TOP LEVEL one (§6.6.1), so a nested helper could not touch the
//!   state it exists to manage. The only way out was to widen the field, which
//!   is the opposite of what the rule is for.
//! - `protected` was treated as subclass-only, ignoring that it also grants
//!   package access (§6.6.1). That made `protected` NARROWER than writing no
//!   modifier at all.
//!
//! Getting there also required nested types to be usable at all: a bare
//! `new Inner()` inside its owner did not resolve, a nested type named after
//! anything in the `rust.std` prelude emitted the prelude's type instead, and
//! a body's locals leaked into every function emitted after it.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn access_control_follows_the_java_rules() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("access_control.jux");
    let emit_dir = workspace_root.join("target").join("it-access-control");

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
            // a nested class reading its owner's privates, and the owner
            // reading the nested type's: 7 remaining, cursor at 3
            "703",
            // two nested types under one owner seeing each other
            "4",
            // `protected` reaching a same-package peer that is NOT a subclass
            "90",
            // and still reaching a subclass, which is what it is for
            "90",
        ],
        "unexpected output:\n{stdout}",
    );

    // The emitted crate must build the nested types under their lifted names.
    // A bare `new Inner()` that resolved to anything else is the failure this
    // guards, and it is invisible in the output above until it stops compiling.
    let emitted = std::fs::read_to_string(emit_dir.join("src").join("main.rs"))
        .expect("emitted crate is readable");
    assert!(
        emitted.contains("Parser__Cursor::new()"),
        "a bare `new Cursor()` inside its owner must build the lifted type:\n{emitted}",
    );
    assert!(
        !emitted.contains("std::collections::Entry"),
        "a nested type shadows the prelude name it collides with:\n{emitted}",
    );
}
