//! Shared plumbing for the example tests.
//!
//! This lives in a subdirectory on purpose: cargo autodiscovers `tests/*.rs`
//! as its own test binary, but not `tests/common/mod.rs`, so a helper module
//! belongs here rather than beside the tests that use it.
//!
//! Before this existed the same twenty lines were copy-pasted into every test
//! file, which is why so few examples had one: the cost of gating an example
//! was writing the boilerplate again.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

/// The repository root, two levels up from `bin/jux`.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

/// Build and run `examples/<name>.jux`, returning its trimmed non-empty
/// stdout lines. Panics with the compiler's own output when it fails, which
/// is the message worth reading.
pub fn run_example(name: &str, tag: &str) -> Vec<String> {
    let (lines, _) = run_example_expecting(name, tag, true);
    lines
}

/// As [`run_example`], but the caller says whether it must succeed. Returns
/// the stdout lines and the combined output, so a negative case can assert on
/// the diagnostic.
pub fn run_example_expecting(name: &str, tag: &str, must_succeed: bool) -> (Vec<String>, String) {
    let root = workspace_root();
    let source = root.join("examples").join(format!("{name}.jux"));
    let emit_dir = root.join("target").join(format!("it-{tag}"));

    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .unwrap_or_else(|e| panic!("spawning jux for {name}: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let all = format!("{stdout}{stderr}");
    if must_succeed {
        assert!(
            output.status.success(),
            "{name} exited with {:?}\n{all}",
            output.status.code(),
        );
    }
    let lines = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    (lines, all)
}

/// Assert an example's whole output, line for line.
pub fn expect_output(name: &str, tag: &str, expected: &[&str]) {
    let got = run_example(name, tag);
    assert_eq!(got, expected, "unexpected output from {name}");
}

/// Assert only that an example ran and produced these lines somewhere in its
/// output, in order. For examples whose output carries timings or other
/// values that are not the same twice.
pub fn expect_contains(name: &str, tag: &str, needles: &[&str]) {
    let got = run_example(name, tag);
    let mut it = got.iter();
    for needle in needles {
        assert!(
            it.any(|l| l.contains(needle)),
            "{name}: expected a line containing {needle:?}, in order, within:\n{}",
            got.join("\n"),
        );
    }
}
