//! End-to-end test for `++` / `--` (§A `incdec`) — prefix, postfix,
//! in for-update and on array elements; desugars to `+= 1` / `-= 1`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn increment_decrement_operators() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let source = root.join("examples").join("increment_decrement.jux");
    let emit_dir = root.join("target").join("it-incdec");

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
            "sum: 10",        // for (i=0; i<5; i++) summing 0..4
            "j: 9",           // j-- from 10
            "j2: 10",         // ++j back to 10
            "arr0: 1",        // arr[k]++ on an index
            "countdown 3",    // for (n=3; n>0; --n)
            "countdown 2",
            "countdown 1",
            "done",
        ],
    );
}
