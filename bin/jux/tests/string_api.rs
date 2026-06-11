//! End-to-end test for the **String API surface** (§K.7 / §S.3):
//! byteLength vs charLength vs `length`, substring/repeat/split, and
//! the Java-spelled query/transform set.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn string_api_surface() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("string_api.jux");
    let emit_dir = workspace_root.join("target").join("it-string-api");

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
            "6", "5", "6", "ell", "hihihi", "3", "b", "pad", "ABC", "abc",
            "true", "true", "true", "2", "heLLo", "e", "false",
        ],
        "unexpected output:\n{stdout}",
    );
}
