//! End-to-end test for String fields on a class — verifies the
//! position-aware mapping (Jux `String` → Rust `String` in field
//! position, `&str` in param/local, `String` in return position) and
//! the auto-coercions (`.to_string()` on field assignment, `.clone()`
//! on field read) compose cleanly through a small `User` class.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn user_class_with_string_field_and_owned_greeting_method() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("user.jux");
    let emit_dir = workspace_root.join("target").join("it-user");

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
            "Hello, Ada!",
            "name=Ada, age=36",
            "after birthday: age=37",
        ],
        "unexpected output:\n{stdout}",
    );
}
