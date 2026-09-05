//! End-to-end test for async, worker threads and generics COMPOSED. Each of
//! these worked alone and failed in combination:
//!
//!   * an `async` method on an interface — Rust's `async fn` in a trait is not
//!     dyn-compatible, so a `Rc<dyn Fetcher>` value could not call it;
//!   * an `async` method overridden down a chain and dispatched through a
//!     base-typed reference — same problem on the generated `Kind` trait;
//!   * a generic class keyed by its own type parameter — `HashMap<K, V>` puts
//!     `Eq + Hash` on its methods, and the impl carried neither;
//!   * a worker capturing a loop variable the loop reassigns — the capture
//!     forced the local into an `Rc<RefCell<…>>` cell, which is `!Send`;
//!   * a channel across a cooperative `spawn` written as a direct call rather
//!     than a lambda — its captures were moved instead of shared.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn async_threads_and_generics_compose() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("async_threads_generics.jux");
    let emit_dir = workspace_root.join("target").join("it-async-threads-generics");

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
        ["const", "7", "Doubled", "2", "8"],
        "unexpected output:\n{stdout}",
    );
}
