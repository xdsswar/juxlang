//! A program that fails at run time says so in Jux terms (GAPS.md gap 33).
//!
//! Each example here ends on purpose, the way programs do: an array position
//! out of range, an integer overflow in a debug build, a failed `assert`, a
//! map key that is not there, recursion that never stops. What the program
//! prints is Jux's report, `Exception in thread "main" ...` or `panic: ...`,
//! with the `.jux` line it happened at, and never the Rust runtime's text
//! (`thread 'main' panicked at src/main.rs`, `index out of bounds: the len
//! is`, `RUST_BACKTRACE`). The first four are also pinned word for word by
//! the output corpus (`tests/run.rs`); this test holds every one of them to
//! the leak detector directly, with nothing excused.

mod common;

use std::process::Command;

/// Run `examples/<name>.jux`; its stdout and stderr, and its exit status.
fn run(name: &str) -> (String, Option<i32>) {
    let root = common::workspace_root();
    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("run")
        .arg("--emit-dir")
        .arg(root.join("target").join(format!("it-rtf-{name}")))
        .arg(root.join("examples").join(format!("{name}.jux")))
        .current_dir(&root)
        .env("JUX_SELFCHECK", "1")
        .output()
        .unwrap_or_else(|e| panic!("spawning jux for {name}: {e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .replace("\r\n", "\n");
    let text: String = text.lines().filter(|l| !l.starts_with("jux: ")).map(|l| format!("{l}\n")).collect();
    (text, output.status.code())
}

/// The run ended with status 101, printed `report`, named the `.jux` line,
/// and showed no Rust.
fn assert_jux_failure(name: &str, report: &str) {
    let (text, code) = run(name);
    assert_eq!(code, Some(101), "{name}: an uncaught failure exits 101:\n{text}");
    assert!(text.contains(report), "{name}: expected {report:?} in:\n{text}");
    assert!(text.contains(&format!("{name}.jux:")), "{name}: the report names the .jux line:\n{text}");
    assert!(
        juxc_diagnostics::leak::find_rust_leak(&text).is_none(),
        "{name} shows Rust: {:?}\n{text}",
        juxc_diagnostics::leak::find_rust_leak(&text)
    );
    assert!(!text.contains("main.rs"), "{name}: {text}");
}

#[test]
fn an_index_out_of_bounds_is_a_jux_exception() {
    assert_jux_failure(
        "runtime_index_out_of_bounds",
        "Exception in thread \"main\" jux.std.exceptions.IndexOutOfBoundsException: Index 5 out of bounds for length 3",
    );
}

#[test]
fn an_integer_overflow_is_a_jux_panic() {
    assert_jux_failure("runtime_integer_overflow", "panic: integer overflow in addition");
}

#[test]
fn a_failed_assert_names_no_generated_code() {
    assert_jux_failure("runtime_assert_failed", "panic: assertion failed\n");
}

#[test]
fn a_missing_map_key_is_a_jux_panic() {
    assert_jux_failure("runtime_missing_map_key", "panic: no entry for the key");
}

/// A stack overflow never reaches a panic hook. On Windows the prelude's
/// exception handler reports it; elsewhere the runtime's own report stands
/// (recorded in ERRATA as what gap 33 could not guard).
#[test]
fn a_stack_overflow_is_reported_in_jux_terms_on_windows() {
    let (text, code) = run("runtime_stack_overflow");
    assert!(text.contains("recursing"), "{text}");
    if cfg!(windows) {
        assert_eq!(code, Some(101), "{text}");
        assert!(text.contains("panic: stack overflow"), "{text}");
        assert!(juxc_diagnostics::leak::find_rust_leak(&text).is_none(), "{text}");
    } else {
        assert_ne!(code, Some(0), "{text}");
    }
}
