//! End-to-end test for JUX-LANG-V1 §6.5.2 — arrays are reference types.
//!
//! Every shape here silently did nothing before: an array bound to a second
//! name was a copy, an array passed to a function was a copy the callee wrote
//! into and dropped, and a row of a 2-D array was a copy of the row. It is the
//! same class of bug Wave 1 removed for collections, on the most primitive
//! aggregate the language has.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn arrays_alias_like_java_arrays() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("array_reference_semantics.jux");
    let emit_dir = workspace_root.join("target").join("it-array-ref-semantics");

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
            "5",   // a second name is the same array
            "9",   // a function writes into the caller's array
            "7",   // an array read out of an object is the object's
            "3",   // a row of a 2-D array is the row
            "4",   // ... and the rows are distinct from each other
            "7 2", // a getter hands back the real array
            "0",   // clone() is what copies
        ],
        "unexpected output:\n{stdout}",
    );
}
