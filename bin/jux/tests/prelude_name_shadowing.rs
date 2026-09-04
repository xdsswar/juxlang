//! End-to-end test that a program may name its own types after things the
//! generated runtime is written against. A user interface called `Sized` used
//! to make the emitted crate fail with "bound modifier `?` can only be applied
//! to `Sized`", because the runtime sat in the crate root beside user code.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn user_types_may_shadow_runtime_names() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("prelude_name_shadowing.jux");
    let emit_dir = workspace_root.join("target").join("it-prelude-name-shadowing");

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
        ["4", "sent", "task", "1", "dup", "interp 4"],
        "unexpected output:\n{stdout}",
    );
}
