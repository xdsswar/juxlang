//! Shared plumbing for the example tests.
//!
//! This lives in a subdirectory on purpose: cargo autodiscovers `tests/*.rs`
//! as its own test binary, but not `tests/common/mod.rs`, so a helper module
//! belongs here rather than beside the tests that use it.
//!
//! Before this existed the same twenty lines were copy-pasted into every test
//! file, which is why so few examples had one: the cost of gating an example
//! was writing the boilerplate again.
//!
//! # What the suite costs, and where the cost is
//!
//! Measured 2026-09-09 on Windows, warm `target/`: `cargo test --workspace` is
//! **529s** for 1425 tests across 215 binaries. That number had never been
//! written down, and a gate whose cost nobody knows is a gate that gets
//! switched off.
//!
//! Nearly all of it is in one place. Cargo runs test BINARIES sequentially but
//! runs the tests INSIDE a binary in parallel, so an example costs six times
//! more when it has a test file to itself:
//!
//! | shape | binaries | examples | per example |
//! |---|---|---|---|
//! | one test per file | 158 | 158 | 2.49s |
//! | clustered (this helper) | 9 | 129 | 0.42s |
//!
//! At the clustered rate those 158 would take 66s instead of 393s. Clustering
//! is worth roughly 327s of the 529s, which is the single biggest lever in the
//! suite, and it is a reason to prefer a cluster runner over a new file per
//! example that has nothing to do with how strongly anything is asserted.
//!
//! ## One shared `CARGO_TARGET_DIR` was tried, and is not worth it
//!
//! Every emitted crate depends on `futures` with the `thread-pool` feature, and
//! each test gets its own `target/it-<tag>/target`, so that dependency tree is
//! built once per example: 314 copies at roughly 57 MB each. Pointing the whole
//! corpus at one build directory (which `juxc_driver::cargo_target_dir`
//! supports, and which `crate_name_for_input` makes collision-free) looks like
//! an obvious win. Measured on `examples_misc`, 45 tests, in isolation:
//!
//! | | cold | warm | disk |
//! |---|---|---|---|
//! | per-test directories | 59.1s | **10.7s** | ~2.5 GB |
//! | one shared directory | **28.2s** | 18.9s | ~0.6 GB |
//!
//! Cold it is twice as fast; warm it is nearly twice as slow, because cargo
//! takes a file lock on the directory and the parallel tests inside a binary
//! then serialize on it, and because each fingerprint check now walks a
//! directory holding every crate in the corpus instead of one. Warm is the case
//! that matters for a gate run before a commit, so the per-test directories
//! stay. Revisit only if the trade changes: a fresh clone, or a run where disk
//! matters more than wall-clock.

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
