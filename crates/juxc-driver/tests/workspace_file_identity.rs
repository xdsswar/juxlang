//! Integration tests for workspace-aware checking + per-diagnostic file
//! identity (the fix for the false `[E0413]` the single-file LSP path used to
//! report on cross-file imported types).
//!
//! Scenario mirrors the bug report: `Follow.jux` declares
//! `class Follow { void printVal() }` in package `xss.it.follow`; `main.jux`
//! imports it and calls `f.printVal()`. Checking `main.jux` ALONE resolves
//! `Follow` by name but sees an empty method table → false E0413. Checking the
//! two together (as the workspace path does) makes the error disappear.

use juxc_diagnostics::code::Code;
use juxc_source::SourceFile;

const FOLLOW: &str =
    "package xss.it.follow; public class Follow { public void printVal(){ print(1); } }";
const MAIN: &str =
    "import xss.it.follow.Follow; public static void main(){ var f = new Follow(); f.printVal(); }";

/// The whole point: checking `Follow` + `main` together yields NO diagnostics
/// at all — the `f.printVal()` call resolves against `Follow`'s real method
/// table, so the false `[E0413] no method printVal` is gone.
#[test]
fn workspace_check_has_no_false_e0413() {
    let sources = vec![
        SourceFile::new("Follow.jux", FOLLOW),
        SourceFile::new("main.jux", MAIN),
    ];
    let result = juxc_driver::check_workspace(sources);

    let e0413: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, Code::E0413_UnresolvedMethod))
        .collect();
    assert!(
        e0413.is_empty(),
        "expected no E0413 across the workspace, got: {:?}",
        e0413
            .iter()
            .map(|d| (d.code.as_str(), &d.message, d.file))
            .collect::<Vec<_>>()
    );

    // Sanity: there should be no error-severity diagnostics whatsoever.
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, juxc_diagnostics::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "expected a clean workspace, got errors: {:?}",
        errors.iter().map(|d| (d.code.as_str(), &d.message)).collect::<Vec<_>>()
    );
}

/// File identity: an error placed in `Follow.jux` is tagged with `Follow.jux`'s
/// source index — NOT `main.jux`'s. The index points into `result.sources`,
/// and that entry's path is `Follow.jux`.
#[test]
fn diagnostic_file_index_points_at_the_right_source() {
    // `boom()` calls a method that doesn't exist → a real E0413 in Follow.jux.
    let follow_with_error = "package xss.it.follow; public class Follow { \
         public void printVal(){ print(1); } public void boom(){ this.nope(); } }";
    let sources = vec![
        SourceFile::new("Follow.jux", follow_with_error),
        SourceFile::new("main.jux", MAIN),
    ];
    let result = juxc_driver::check_workspace(sources);

    let e0413: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, Code::E0413_UnresolvedMethod))
        .collect();
    assert_eq!(e0413.len(), 1, "expected exactly one E0413 (the `nope()` call)");

    let d = e0413[0];
    let idx = d.file.expect("diagnostic must carry a file index");
    let src = &result.sources[idx];
    assert!(
        src.path().ends_with("Follow.jux"),
        "E0413 should be attributed to Follow.jux, but file index {idx} is {}",
        src.path().display()
    );

    // And the message is about the missing `nope`, confirming it's the real one.
    assert!(d.message.contains("nope"), "unexpected message: {}", d.message);
}

/// The stdlib units are auto-prepended ahead of the user sources, so the user
/// indices are NOT 0/1 — `file` indexing must account for that offset. This
/// pins the contract that `file` indexes into `result.sources` (the full list
/// including stdlib), which is exactly what the CLI/LSP rely on.
#[test]
fn file_index_accounts_for_prepended_stdlib() {
    let sources = vec![SourceFile::new("main.jux", MAIN)];
    let result = juxc_driver::check_workspace(sources);
    // Whatever the count, the LAST source must be our user file.
    let last = result.sources.last().expect("at least the user source");
    assert!(last.path().ends_with("main.jux"));
    // The stdlib prepend means there's more than one source in the list.
    assert!(
        result.sources.len() > 1,
        "expected stdlib units prepended ahead of the user source"
    );
}
