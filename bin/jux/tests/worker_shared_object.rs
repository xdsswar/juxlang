//! End-to-end test for a shared, mutable OBJECT crossing worker threads. Per
//! JUX-ASYNC-ADDENDUM §18.2 a class whose refcount can be made atomic is
//! transferable, and the compiler performs that upgrade — including for the
//! classes the captured one reaches through its fields. Three workers pounding
//! one registry must agree, and the caller must still be looking at the same
//! object afterwards. This used to be rejected outright with `[E0702]`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn a_shared_object_crosses_worker_threads() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("worker_shared_object.jux");
    let emit_dir = workspace_root.join("target").join("it-worker-shared-object");

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
        ["1500", "1500", "1500"],
        "unexpected output:\n{stdout}",
    );
}
