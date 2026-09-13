//! `examples/same_name_classes` -- classes that share a name across packages.
//!
//! An exception `app.errors.Failure` and an ordinary `app.model.Failure`, plus
//! a user class named like the standard library's `IOException`, two class
//! hierarchies rooted at a `Base`, and two nested `Pen.Gate` classes. The backend
//! used to decide each class's representation per bare name, so the ordinary
//! class became a plain value (aliases stopped seeing changes, and a mutating
//! method did not compile), and `new Failure(...)` could construct the other
//! package's class. Run end to end, since each failure was a rustc error with
//! no Jux diagnostic.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn same_named_classes_keep_their_own_representation() {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .join("examples")
        .join("same_name_classes");

    let out = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(&project)
        .arg("run")
        .output()
        .expect("spawning jux run for same_name_classes");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "same_name_classes failed with {:?}\n{stdout}{stderr}",
        out.status.code(),
    );

    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.starts_with("jux: "))
        .collect();
    assert_eq!(
        lines,
        [
            "disk failed 2 times",
            "caught boom with code 7",
            "the path is now /var",
            "cat tom with 8 lives",
            "car JUX-1 on 4 wheels",
            "zoo gate open: true",
            "garage gate width: 3",
        ],
    );
}
