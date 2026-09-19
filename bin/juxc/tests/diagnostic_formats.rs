//! The diagnostic formats of JUX-DIAGNOSTICS-ADDENDUM §D.1 / §D.2 and
//! `juxc explain` (§D.5.3), end to end through the `juxc` binary.
//!
//! The test harness pipes stderr, so without a flag `juxc` must print the
//! one-line `line` format (ERRATA E72): the blessed UI outputs and the IDE
//! depend on it.

use std::path::PathBuf;
use std::process::Command;

fn juxc() -> &'static str {
    env!("CARGO_BIN_EXE_juxc")
}

/// A file with two errors, written once per test run.
fn bad_file(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("target")
        .join(format!("it-diag-formats-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creating dir");
    let file = dir.join("bad.jux");
    std::fs::write(&file, "public void main() {\n    int x = \"no\";\n    print(y);\n}\n").expect("writing file");
    file
}

/// Run `juxc --check <file> <args>`; return (stdout, stderr) with the path
/// written as `bad.jux` so the assertions do not depend on the machine.
fn check(file: &PathBuf, args: &[&str]) -> (String, String) {
    let out = Command::new(juxc())
        .arg("--check")
        .arg(file)
        .args(args)
        .output()
        .expect("running juxc");
    let path = file.display().to_string();
    let fwd = path.replace('\\', "/");
    let clean = |s: &[u8]| String::from_utf8_lossy(s).replace(&path, "bad.jux").replace(&fwd, "bad.jux");
    (clean(&out.stdout), clean(&out.stderr))
}

#[test]
fn piped_output_defaults_to_the_line_format() {
    let f = bad_file("line");
    let (_out, err) = check(&f, &[]);
    assert_eq!(
        err,
        "bad.jux:2:5: [E0410] error: type mismatch in declaration of `x`: expected int, found String\n\
         bad.jux:3:11: [E0301] error: cannot find `y` in this scope\n",
    );
}

#[test]
fn human_format_draws_source_frames() {
    let f = bad_file("human");
    let (_out, err) = check(&f, &["--diagnostic-format", "human", "--color", "never"]);
    assert!(err.starts_with("error[E0410]: type mismatch in declaration of `x`: expected int, found String\n"), "{err}");
    assert!(err.contains(" --> bad.jux:2:5\n"), "{err}");
    assert!(err.contains("2 |     int x = \"no\";\n  |     ^^^^^^^^^^^^^\n"), "{err}");
    assert!(err.contains("3 |     print(y);\n  |           ^\n"), "{err}");
    assert!(err.contains("try `juxc explain E0410`"), "{err}");
    assert!(err.ends_with("error: aborting due to 2 errors\n"), "{err}");
    assert!(!err.contains('\x1b'), "--color never still colored:\n{err}");

    let (_out, colored) = check(&f, &["--diagnostic-format", "human", "--color", "always"]);
    assert!(colored.contains("\x1b[1;31merror[E0410]"), "{colored}");
}

#[test]
fn compact_and_short_are_one_line_each() {
    let f = bad_file("compact");
    let (_out, err) = check(&f, &["--diagnostic-format", "short"]);
    assert_eq!(
        err,
        "bad.jux:2:5: error[E0410]: type mismatch in declaration of `x`: expected int, found String\n\
         bad.jux:3:11: error[E0301]: cannot find `y` in this scope\n",
    );
    let (_out, err) = check(&f, &["--diagnostic-format", "compact"]);
    assert!(err.starts_with("bad.jux:2:5: error[E0410]:"), "{err}");
}

#[test]
fn json_is_ndjson_on_stdout() {
    let f = bad_file("json");
    let (out, err) = check(&f, &["--diagnostic-format", "json"]);
    assert!(err.is_empty(), "JSON mode wrote to stderr:\n{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{out}");
    for line in &lines {
        serde_json::from_str::<serde_json::Value>(line).expect("each line is JSON");
    }
    assert!(lines[0].contains("\"code\":\"E0410\""), "{out}");
    assert!(lines[2].starts_with("{\"summary\":{\"errors\":2,"), "{out}");
}

#[test]
fn explain_prints_the_bundled_docs() {
    let out = Command::new(juxc()).args(["explain", "e414"]).output().expect("running juxc explain");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("E0414: "), "{text}");
    assert!(text.contains("private"), "{text}");

    let out = Command::new(juxc()).args(["explain", "E9999"]).output().expect("running juxc explain");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a diagnostic code"));
}
