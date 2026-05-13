//! End-to-end test for interface fields (`classes-rules.md` §3.3).
//!
//! Runs `examples/interface_constants.jux`. Verifies the implicit
//! `public static final` lowering: `int MAX_RETRIES = 3;` and
//! `String DEFAULT_NAME = "anon";` are reachable as
//! `Settings.MAX_RETRIES` / `Settings.DEFAULT_NAME` and from an
//! implementing class's method body.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn interface_constants() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("interface_constants.jux");
    let emit_dir = workspace_root.join("target").join("it-interface-constants");

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
        ["3", "anon", "3"],
        "unexpected output:\n{stdout}",
    );
}
