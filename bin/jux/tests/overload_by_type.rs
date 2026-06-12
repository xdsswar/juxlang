//! End-to-end test for **type-based method overloading** (§T.3):
//! `add(int)` / `add(double)` / `add(String)` / `add(int, int)` on one
//! class, dispatched by argument type — including through
//! function-call arguments whose types come from inference.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn same_arity_overloads_dispatch_by_argument_type() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let source = root.join("examples").join("overload_by_type.jux");
    let emit_dir = root.join("target").join("it-overload-by-type");

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
        "jux exited with {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    assert!(
        stdout.contains("i7;d2.5;shey;ii3;i6;d4.5;"),
        "overload dispatch order wrong:\n{stdout}",
    );
}

/// Two overloads with IDENTICAL parameter types are true ambiguity —
/// still rejected at the declaration with E0450.
#[test]
fn identical_signatures_still_rejected() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let source = root.join("probes").join("probe_overload_ambig.jux");

    let output = Command::new(jux)
        .arg("check")
        .arg(&source)
        .output()
        .expect("spawn jux");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[E0450]") && stderr.contains("same parameter types"),
        "expected the E0450 true-ambiguity diagnostic:\n{stderr}",
    );
}
