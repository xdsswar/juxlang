//! End-to-end integration test for milestone-9 (string concat + `int` as
//! platform-sized).
//!
//! Runs `jux run examples/concat.jux` and asserts stdout contains the
//! expected greetings + integer values.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn concat_example_prints_greetings_and_numbers() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("concat.jux");
    let emit_dir = workspace_root.join("target").join("it-concat");

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
    for needle in ["hello, Alice!", "hello, Bob!", "42", "7"] {
        assert!(
            stdout.contains(needle),
            "stdout missing '{needle}':\n{stdout}",
        );
    }
}
