//! End-to-end test for async/await codegen.
//!
//! Runs `examples/async_basic.jux`. Exercises:
//! - `async int f(int)` lowering to a Rust `async fn`.
//! - `await f(...)` lowering to `f(...).await`.
//! - `async void main()` → a synchronous `fn main()` shim driving the renamed
//!   `__jux_async_main` via `futures::executor::block_on` (the entry-point
//!   shim this test guards — it used to emit no `fn main` and leak rustc E0601).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn async_main_awaits_async_function_and_runs() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("async_basic.jux");
    let emit_dir = workspace_root.join("target").join("it-async-basic");

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
    assert_eq!(lines.as_slice(), ["async: 6 12"], "unexpected output:\n{stdout}");
}
