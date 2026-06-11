//! End-to-end test for **AtomicInt / AtomicLong + MemoryOrder**
//! (§S.6.2): SeqCst-default and explicit-order overloads, fetch*
//! returning the previous value, 64-bit values, and genuine
//! cross-thread sharing (two Worker.spawn closures bumping the same
//! Arc-backed cell).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn atomics() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("atomics.jux");
    let emit_dir = workspace_root.join("target").join("it-atomics");

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
            "10", "42", "42", "50", "50", "30", "14", "30", "225",
            "5000000000", "5000000001",
            "2000",
        ],
        "unexpected output:\n{stdout}",
    );
}
