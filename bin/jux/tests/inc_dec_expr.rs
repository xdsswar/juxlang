//! End-to-end test for EXPRESSION-position `++` / `--` (§A `incdec`,
//! value form, gap N3). Exercises post (yields OLD) and pre (yields NEW)
//! inc/dec used as call arguments, initializers, and array indices, with
//! single-evaluation of the indexed place.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn inc_dec_expression_position() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let source = root.join("examples").join("inc_dec_expr.jux");
    let emit_dir = root.join("target").join("it-incdec-expr");

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
        [
            "post-inc value: 5",   // x++ yields OLD (5), x -> 6
            "x after post-inc: 6",
            "pre-inc value: 7",    // ++x yields NEW (7)
            "x after pre-inc: 7",
            "post-dec value: 5",   // y-- yields OLD (5)
            "pre-dec value: 3",    // --y yields NEW (3)
            "a: 10 b: 12 n: 12",   // a = n++ (old 10), b = ++n (new 12)
            "arr[i++]: 10",        // reads arr[0], steps i once
            "i now: 1",
            "arr[i]: 20",
            "got: 0 counts1: 1",   // counts[1]++ yields OLD (0), elem -> 1
            "pre: 2 counts1: 2",   // ++counts[1] yields NEW (2)
            "done",
        ],
    );
}
