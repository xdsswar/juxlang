//! End-to-end test for the Java builder pattern (`return this;` chaining).
//!
//! Runs `examples/builder_pattern.jux`. Guards the wrapper-class
//! share-on-return fix: a tail `return this;` must lower to `self.clone()`
//! (a cheap Rc bump) so the fluent chain compiles — it used to emit a bare
//! `self` (`&C`) where owned `C` was expected and leak rustc E0308.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn builder_chain_returns_this_and_runs() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("builder_pattern.jux");
    let emit_dir = workspace_root.join("target").join("it-builder-pattern");

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
    assert_eq!(lines.as_slice(), ["sum=15"], "unexpected output:\n{stdout}");
}
