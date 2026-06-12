//! End-to-end test for **`break`/`continue` inside an ASYNC `try`**
//! (gap O9). The `async move` body can't thread a plain flag local
//! out, so the loop-control channel is an `Arc<AtomicU8>` (the sync
//! shape keeps O2's `u8` local).
//!
//! Runs `examples/async_try_loopctl.jux`:
//! - `continue` + `break` from an async try body, finally running
//!   BEFORE the loop control (Java ordering: `fin 2` then the skip,
//!   `fin 4` then the break);
//! - `break` from a catch arm;
//! - labeled `break outer;` two loops deep;
//! - `return`/`break` coexisting in one valued async try;
//! - the same shape spawned onto a pool thread (Send proof for the
//!   `Arc` channel — an `Rc<Cell>` channel would fail to compile).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn async_try_loop_control_threads_through_atomic_channel() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("async_try_loopctl.jux");
    let emit_dir = workspace_root.join("target").join("it-async-loopctl");

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
            "work 1",
            "fin 1",
            "fin 2",
            "work 3",
            "fin 3",
            "fin 4",
            "bodyEscapes done",
            "ok 1",
            "ok 2",
            "caught stop",
            "catchEscapes done",
            "cell 11",
            "cleanup",
            "cell 12",
            "cleanup",
            "cell 13",
            "cleanup",
            "cell 21",
            "cleanup",
            "cleanup",
            "labeledEscape done",
            "probe 1",
            "probe 2",
            "probe 3",
            "9",
            "probe 1",
            "probe 2",
            "probe 3",
            "probe 4",
            "-1",
            "33",
        ],
        "unexpected output:\n{stdout}",
    );
}
