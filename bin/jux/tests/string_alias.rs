//! `String` and `string` name one type, C#-style.
//!
//! Asserts they interoperate — a `string` argument into a `String` parameter
//! and back — and that `typeof` reports the canonical `String` for both, so a
//! diagnostic never depends on which spelling the author used.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn string_and_lowercase_string_are_one_type() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("string_alias.jux");
    let emit_dir = workspace_root.join("target").join("it-string-alias");

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
    assert_eq!(lines.as_slice(), ["hi world!", "String String"], "unexpected output:\n{stdout}");
}
