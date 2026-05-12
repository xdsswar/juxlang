//! End-to-end test: for-each loops over arrays borrow (not move) the
//! source, so the array stays usable after the loop. Runs
//! `examples/greet_all.jux`, which iterates a `String[]` and an
//! `int[3]` and then prints each array's `.length` after the loop.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn for_each_on_arrays_keeps_source_usable_after_loop() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("greet_all.jux");
    let emit_dir = workspace_root.join("target").join("it-for-each-arrays");

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
        ["Alice", "Bob", "Carol", "3", "10", "20", "30", "3"],
        "unexpected output:\n{stdout}",
    );
}
