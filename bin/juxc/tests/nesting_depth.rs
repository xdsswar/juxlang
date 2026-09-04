//! End-to-end: pathological nesting is an error, not a dead process.
//!
//! The unit tests in `juxc-parse` prove the depth budget reports E0201. This
//! proves the shipped binary survives to print it -- the budget alone is not
//! enough, because the levels it does allow still need more stack than a thread
//! gets by default. Both halves have to hold, so both are tested where they
//! meet: at the command line.
use std::process::Command;

fn juxc_check(src: &str, tag: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("juxc-nesting-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("nesting.jux");
    std::fs::write(&file, src).expect("write source");
    let out = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg("--check")
        .arg(&file)
        .output()
        .expect("spawn juxc");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let _ = std::fs::remove_dir_all(&dir);
    (out.status.success(), combined)
}

fn nested_parens(depth: usize) -> String {
    format!(
        "public void main() {{ int x = {}1{}; print(x); }}",
        "(".repeat(depth),
        ")".repeat(depth),
    )
}

/// Depth a real program could plausibly reach still compiles. This is the half
/// that a naive "just lower the limit" fix would have broken.
#[test]
fn deep_but_reasonable_nesting_still_checks() {
    let (ok, out) = juxc_check(&nested_parens(100), "ok");
    assert!(ok, "100 levels should check cleanly, got:\n{out}");
}

/// Absurd nesting reports E0201 and exits like any other compile error. Before
/// the fix this aborted with STATUS_STACK_OVERFLOW and printed no diagnostic.
#[test]
fn absurd_nesting_reports_a_diagnostic_instead_of_crashing() {
    let (ok, out) = juxc_check(&nested_parens(20_000), "deep");
    assert!(!ok, "20000 levels must be rejected, but the compile succeeded:\n{out}");
    assert!(out.contains("E0201"), "expected E0201, got:\n{out}");
    assert!(
        !out.contains("overflowed its stack"),
        "the process overflowed its stack instead of reporting an error:\n{out}",
    );
}
