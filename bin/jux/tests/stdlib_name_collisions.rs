//! `examples/stdlib_name_collisions.jux` -- a user type may be named like a
//! standard-library one.
//!
//! Java lets a program declare `class String`, `class T` or `class Exception`,
//! and Jux is a Java-shaped language, so it does too (§M.16, ERRATA E96).
//! Before the fix, bare-name resolution was a workspace-wide index instead of a
//! per-unit question, so the standard library's own sources resolved their names
//! against whatever the program happened to declare. `class T` alone produced 60
//! errors, every one of them inside a `jux.std` file the author cannot open;
//! `class String` produced 81, `class Vec` 10, `class Exception` 8. `class Foo`
//! and `class K` were fine, so it did not even read as a rule.
//!
//! Run end to end, because every single failure was a diagnostic or a rustc
//! error pointing at a standard-library file rather than at the program.
//! `tests/expected/examples/stdlib_name_collisions.expected` pins the output;
//! this test additionally asserts that nothing in the build output NAMES
//! `jux.std`, which is the property that actually broke.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

#[test]
fn a_user_type_may_be_named_like_a_stdlib_one() {
    let root = workspace_root();
    let out = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(&root)
        .arg("run")
        .arg("--emit-dir")
        .arg(root.join("target").join("it-stdlib_name_collisions-named"))
        .arg(root.join("examples").join("stdlib_name_collisions.jux"))
        .output()
        .expect("spawning jux run for stdlib_name_collisions");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "stdlib_name_collisions failed with {:?}\n{stdout}{stderr}",
        out.status.code(),
    );

    // The whole point: no diagnostic and no rustc error may point INSIDE the
    // standard library. A user's own declaration is not a bug report against
    // `jux.std`.
    let all = format!("{stdout}{stderr}");
    assert!(
        !all.contains("jux.std"),
        "something pointed inside jux.std:\n{all}",
    );

    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.starts_with("jux: ") && !l.trim().is_empty())
        .collect();
    assert_eq!(
        lines,
        [
            "7",
            "label",
            "3",
            "12",
            "iterator over 4",
            "2",
            "5",
            "404",
            "42",
            "First",
            "Second",
            "9",
            "true",
            "pred saw null: true",
            "user assertTrue 9",
            "2",
        ],
    );
}
