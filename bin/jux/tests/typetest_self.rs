//! `=>` type-test against the value's own static (polymorphic-base) type.
//! Regression for a codegen bug: `if (z => Animal za)` where `z` is already
//! `Animal` emitted `z.__jux_as_Animal()`, a hook generated only for subtypes,
//! so it failed to compile (E0599). An identity test is always true and binds
//! the value itself. Subtype downcasts and the bool form must still work.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn typetest_against_own_type_is_identity() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("typetest_self.jux");
    let emit_dir = root.join("target").join("it-typetest-self");

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
        "compile/run failed (a missing __jux_as_<Self> hook means the identity \
         type-test regressed):\nstderr:\n{stderr}\nstdout:\n{stdout}"
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(
        lines.as_slice(),
        ["meow", "cat meow", "not a dog", "true"],
        "unexpected output:\n{stdout}"
    );
}
