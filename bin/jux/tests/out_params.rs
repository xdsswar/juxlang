//! End-to-end test for **`out` parameters** (§M.4): a callee-written
//! parameter lowers to Rust `&mut T`, the body's `result = v` becomes
//! `*result = v`, and the call site `out n` becomes `&mut n`. An
//! uninitialized local out-arg (`int n;`) is the canonical use.
//!
//! Negative probes assert the four guards fire as Jux diagnostics rather
//! than leaking rustc errors:
//!   * E0940 — out param not assigned on every normal-exit path
//!   * E0942 — `out` argument is not an assignable place
//!   * E0943 — `out` arg/param agreement (both directions)
//! plus a parse probe that an ordinary identifier named `out` still works.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

/// Build an inline-source probe and return the combined stdout+stderr plus
/// whether the build succeeded. Each probe gets its own emit dir.
fn build_probe(name: &str, source: &str) -> (bool, String) {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let dir = root.join("target").join(format!("it-out-{name}"));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let src = dir.join("probe.jux");
    std::fs::write(&src, source).expect("write probe");

    let output = Command::new(jux)
        .arg("build")
        .arg("--emit-dir")
        .arg(dir.join("emit"))
        .arg(&src)
        .output()
        .expect("spawn jux");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    (output.status.success(), format!("{stdout}\n{stderr}"))
}

#[test]
fn out_params() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let source = root.join("examples").join("out_params.jux");
    let emit_dir = root.join("target").join("it-out-params");

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
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(
        lines.as_slice(),
        ["ok 42", "fail 0", "cell 42", "slot 42"],
        "unexpected output:\n{stdout}",
    );
}

#[test]
fn out_param_unassigned_path_is_e0940() {
    // `result` is assigned only on the `true` path; the fall-through returns
    // without assigning it.
    let (ok, all) = build_probe(
        "e0940",
        r#"package badout;

bool tryParse(String s, out int result) {
    if (s == "42") {
        result = 42;
        return true;
    }
    return false;
}

public void main() {
    int n;
    tryParse("x", out n);
}
"#,
    );
    assert!(!ok, "unassigned out param unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0940]"), "expected E0940, got:\n{all}");
}

#[test]
fn out_arg_non_place_is_e0942() {
    // `out gen()` parses as an out-arg whose place is a call — not assignable.
    let (ok, all) = build_probe(
        "e0942",
        r#"package badout;

void sink(out int result) { result = 1; }

int gen() { return 0; }

public void main() {
    sink(out gen());
}
"#,
    );
    assert!(!ok, "non-place out arg unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0942]"), "expected E0942, got:\n{all}");
}

#[test]
fn out_arg_to_non_out_param_is_e0943() {
    // `out` on an argument whose parameter is not declared `out`.
    let (ok, all) = build_probe(
        "e0943-a",
        r#"package badout;

void plain(int x) {}

public void main() {
    int n = 0;
    plain(out n);
}
"#,
    );
    assert!(!ok, "out arg to plain param unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0943]"), "expected E0943, got:\n{all}");
}

#[test]
fn plain_arg_to_out_param_is_e0943() {
    // The converse: an `out` parameter requires the `out` keyword at the call.
    let (ok, all) = build_probe(
        "e0943-b",
        r#"package badout;

void sink(out int result) { result = 1; }

public void main() {
    int n = 0;
    sink(n);
}
"#,
    );
    assert!(!ok, "plain arg to out param unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0943]"), "expected E0943, got:\n{all}");
}

#[test]
fn out_combined_with_final_is_e0944() {
    // `out` is a parameter mode that excludes `final` (in either order).
    let (ok, all) = build_probe(
        "e0944",
        r#"package badout;

void bad(out final int x) { x = 1; }

public void main() {
    int n;
    bad(out n);
}
"#,
    );
    assert!(!ok, "out+final unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0944]"), "expected E0944, got:\n{all}");
}

#[test]
fn out_on_constructor_param_is_e0944() {
    // A constructor parameter forwards into a field — nothing to write back.
    let (ok, all) = build_probe(
        "e0944-ctor",
        r#"package badout;

public class C {
    public int v;
    public C(out int x) { this.v = 0; x = 1; }
}

public void main() {
    int n;
    var c = new C(out n);
    print(c.v);
}
"#,
    );
    assert!(!ok, "out ctor param unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0944]"), "expected E0944, got:\n{all}");
}

#[test]
fn out_is_still_a_valid_identifier() {
    // `out` is a contextual keyword — a variable named `out` must still work.
    let (ok, all) = build_probe(
        "out-ident",
        r#"package okout;

public void main() {
    int out = 7;
    print(out + 1);
}
"#,
    );
    assert!(ok, "identifier named `out` failed to compile:\n{all}");
}
