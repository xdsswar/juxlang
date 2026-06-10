//! End-to-end test for **default parameter values** (§7.2, §S.1.3)
//! and **named arguments** (§A.2.9, §T.3.2).
//!
//! Runs `examples/default_params.jux`: omitted trailing args, named
//! re-ordering (`timeout:` before `port:`), all-defaults calls via a
//! single named arg, defaults on constructors (`new Logger()` /
//! `new Logger(prefix: ..)`), instance methods, and static methods
//! with a named arg skipping a middle default.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn default_params_and_named_args() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("default_params.jux");
    let emit_dir = workspace_root.join("target").join("it-default-params");

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
            "example.com:80 (t=30)",
            "example.com:8080 (t=30)",
            "example.com:443 (t=30)",
            "example.com:443 (t=60)",
            "jux.dev:80 (t=30)",
            "42",
            "15",
            "[log] hello",
            "[log] shout!!",
            ">> named ctor arg",
            "111",
            "13",
        ],
        "unexpected output:\n{stdout}",
    );
}
