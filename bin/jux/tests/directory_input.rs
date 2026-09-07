//! `jux run <dir>` -- pointing the compiler at a directory rather than a file.
//!
//! Two shapes reach the same command, and both used to be rejected outright:
//! you had to `cd` into a project before `jux run` would look at it, and a
//! plain folder of `.jux` files had no way in at all.
//!
//! - A directory holding a `jux.toml` is a PROJECT: manifest, dependencies,
//!   entry point.
//! - A directory without one is a SOURCE SET: every `.jux` beneath it (minus
//!   `target/` and dotted directories) compiles as one program.
//!
//! The source-set case is written to a temporary directory rather than added
//! to `examples/`, because what is under test is the argument handling, not a
//! language feature -- an example would have to be gated for its own sake.

use std::path::PathBuf;
use std::process::Command;

mod common;

/// Run `jux <action> <dir>` and return its trimmed stdout lines, asserting
/// that it succeeded.
fn run_dir(dir: &std::path::Path) -> Vec<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("run")
        .arg(dir)
        .output()
        .unwrap_or_else(|e| panic!("spawning jux run for {}: {e}", dir.display()));

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "jux run {} exited with {:?}\n{stdout}{stderr}",
        dir.display(),
        output.status.code(),
    );
    stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("jux: built"))
        .map(str::to_string)
        .collect()
}

#[test]
fn a_project_directory_runs_without_cd() {
    let dir = common::workspace_root().join("examples").join("stress_shop");
    let got = run_dir(&dir);
    assert!(
        got.iter().any(|l| l.starts_with("[user: Alice")),
        "expected stress_shop's own output, got:\n{}",
        got.join("\n"),
    );
}

#[test]
fn a_folder_of_sources_compiles_as_one_program() {
    let dir: PathBuf = std::env::temp_dir().join("jux-dir-input-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).expect("creating the temp source set");

    // Two files, in different directories, forming one program -- so the walk
    // has to recurse, and the pieces have to be compiled together.
    std::fs::write(
        dir.join("main.jux"),
        "public void main() {\n    print(greeting());\n}\n",
    )
    .expect("writing main.jux");
    std::fs::write(
        dir.join("sub").join("greeting.jux"),
        "public String greeting() {\n    return \"from a folder\";\n}\n",
    )
    .expect("writing greeting.jux");

    let got = run_dir(&dir);
    assert_eq!(got, ["from a folder"]);

    let _ = std::fs::remove_dir_all(&dir);
}
