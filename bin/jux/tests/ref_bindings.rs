//! End-to-end tests for `ref` bindings (§M.13 — shared references to
//! value types): locals, parameters, and fields.

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn run_example(name: &str, emit: &str) -> Vec<String> {
    let jux = env!("CARGO_BIN_EXE_jux");
    let source = root().join("examples").join(name);
    let emit_dir = root().join("target").join(emit);
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
    stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[test]
fn ref_locals_and_params() {
    let lines = run_example("ref_bindings.jux", "it-ref-bindings");
    assert_eq!(
        lines,
        [
            "a: second",   // alias store-through
            "b: renamed",  // write-through a ref param
            "m: 15",       // primitive aliasing
            "double: 30",
            "n: 16",       // self-referential store-through
            "plain: keep", // plain value into ref param: caller keeps copy
            "copy: renamed", // value read out is a copy
            "done",
        ],
    );
}

#[test]
fn ref_fields() {
    let lines = run_example("ref_fields.jux", "it-ref-fields");
    assert_eq!(
        lines,
        [
            "title: renamed",    // store-through via a second class handle
            "views: 7",          // ref int field += through methods
            "retag: shared-tag", // ref FIELD passed to a ref param aliases it
            "t: shared-tag",     // value flows into a ref local's cell
            "score: 101",        // default-initialized ref field
            "done",
        ],
    );
}
