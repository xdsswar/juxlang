//! The internal-compiler-error path, end to end.
//!
//! A compiler bug reaches the user as a panic, and until `juxc_driver::ice`
//! existed that meant Rust's default hook: a thread name, a path inside the
//! compiler, and a note about `RUST_BACKTRACE`. Nothing in it said which `.jux`
//! file was being compiled, which compiler version produced it, that the fault
//! was the compiler's rather than the program's, or where to report it. Worse,
//! it read exactly like a crash in the user's own code.
//!
//! These tests drive the real binary with `JUX_ICE_SELFTEST` set, which trips a
//! panic **inside the large-stack compilation thread**. That is deliberate: the
//! hook that records the panic location runs on that thread while the report is
//! rendered on the main one, so a panic raised anywhere else would exercise the
//! easy half of the path and leave the interesting half uncovered.

use std::path::PathBuf;
use std::process::Command;

/// Repository root, from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

/// Run `juxc --check` on an example with the self-test trip armed.
fn run_tripped(extra_env: &[(&str, &str)]) -> (String, Option<i32>) {
    let root = workspace_root();
    let mut command = Command::new(env!("CARGO_BIN_EXE_juxc"));
    command
        .arg("--check")
        .arg(root.join("examples").join("abs.jux"))
        .env("JUX_ICE_SELFTEST", "1");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let output = command.output().expect("spawn juxc");
    (
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

/// The report has to carry everything an issue needs, or it is just a nicer
/// looking dead end.
#[test]
fn a_panic_is_reported_as_a_compiler_bug() {
    let (stderr, code) = run_tripped(&[]);

    assert!(
        stderr.contains("internal compiler error"),
        "the report must name itself as one:\n{stderr}"
    );
    assert!(
        stderr.contains("not in your program"),
        "the report must say whose fault it is:\n{stderr}"
    );
    assert!(
        stderr.contains("abs.jux"),
        "the report must name the input being compiled:\n{stderr}"
    );
    assert!(
        stderr.contains("github.com/xdsswar/juxlang/issues"),
        "the report must say where to send it:\n{stderr}"
    );
    assert!(
        stderr.contains(env!("CARGO_PKG_VERSION")),
        "the report must stamp the compiler version:\n{stderr}"
    );
    assert!(
        stderr.contains("ice.rs:"),
        "the panic location must survive the hop from the compilation thread \
         to the main one:\n{stderr}"
    );
    assert_eq!(
        code,
        Some(101),
        "an ICE exits 101, distinct from a compile failure's 1:\n{stderr}"
    );
}

/// Rust's own output would say the same thing in a worse way, so it is
/// suppressed. Both halves matter: the `thread '...' panicked at` banner and
/// the note steering the user to `RUST_BACKTRACE`, which our report already
/// covers in its own words.
#[test]
fn rusts_default_panic_output_is_suppressed() {
    let (stderr, _) = run_tripped(&[]);

    assert!(
        !stderr.contains("panicked at"),
        "Rust's default hook should not print alongside the report:\n{stderr}"
    );
    assert!(
        !stderr.contains("note: run with `RUST_BACKTRACE=1`"),
        "the report gives its own backtrace hint:\n{stderr}"
    );
}

/// Suppressing the default hook must not take the backtrace away from someone
/// who explicitly asked for one. Setting `RUST_BACKTRACE` chains the default
/// hook back in, which is the same bargain rustc strikes.
#[test]
fn rust_backtrace_still_produces_a_backtrace() {
    let (stderr, code) = run_tripped(&[("RUST_BACKTRACE", "1")]);

    assert!(
        stderr.contains("stack backtrace"),
        "RUST_BACKTRACE=1 must still yield a backtrace:\n{stderr}"
    );
    assert!(
        stderr.contains("internal compiler error"),
        "and the report is still rendered:\n{stderr}"
    );
    assert!(
        !stderr.contains("Re-run with RUST_BACKTRACE=1"),
        "the hint is pointless once the backtrace is already there:\n{stderr}"
    );
    assert_eq!(code, Some(101), "{stderr}");
}

/// The trip is opt-in. Without the variable the same command compiles cleanly,
/// which is what stops the self-test hook from being a foot-gun in a release
/// build.
#[test]
fn the_selftest_is_off_unless_asked_for() {
    let root = workspace_root();
    let output = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg("--check")
        .arg(root.join("examples").join("abs.jux"))
        .output()
        .expect("spawn juxc");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "a clean check must stay clean:\n{stderr}"
    );
    assert!(
        !stderr.contains("internal compiler error"),
        "and must not report an ICE:\n{stderr}"
    );
}
