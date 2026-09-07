//! End-to-end test that a Jux lambda flows into a Rust API that wants a
//! closure, including through a MUTATING collection method.
//!
//! `Vec::resize_with(n, F)` takes `F: FnMut() -> T`, and a mutating collection
//! method hoists its arguments into temporaries on a separate path from the
//! generic call emitter. That path did not mark the lambda as a bare-closure
//! target, so the closure arrived as the default `Rc<dyn Fn>` -- which does not
//! implement `FnMut`, and the emitted crate failed to build.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn a_jux_lambda_satisfies_a_rust_fnmut_bound() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("closures_into_rust.jux");
    let emit_dir = workspace_root.join("target").join("it-closures-into-rust");

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
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(
        lines.as_slice(),
        ["4", "10", "40"],
        "the closure must run once per element, mutating its capture:\n{stdout}",
    );
}
