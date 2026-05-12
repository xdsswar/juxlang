//! End-to-end integration test for milestone 1.
//!
//! Runs the `juxc` binary against `examples/hello.jux` with `--run` and
//! asserts that:
//!
//! 1. The process exits with status 0.
//! 2. Its stdout contains the string `Hello, world!`.
//!
//! This is the milestone-1 canary: when this test goes green, every
//! phase from lex through `cargo run` is wired correctly. When it
//! breaks, something is wrong somewhere in the pipeline.
//!
//! ## Implementation notes
//!
//! - `env!("CARGO_BIN_EXE_juxc")` resolves to the freshly-built juxc
//!   binary at test time. We don't have to find it manually.
//! - `CARGO_MANIFEST_DIR` for this crate is `bin/juxc`, so the workspace
//!   root sits two `..`s up. We compute the path to `examples/hello.jux`
//!   from there.
//! - The test isolates its emit directory under `target/it-hello-world/`
//!   so it doesn't collide with interactive `juxc --run` invocations.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn hello_world_runs_and_prints() {
    let juxc = env!("CARGO_BIN_EXE_juxc");

    // CARGO_MANIFEST_DIR == .../bin/juxc; go up two dirs to the workspace root.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc");

    let source = workspace_root.join("examples").join("hello.jux");
    let emit_dir = workspace_root.join("target").join("it-hello-world");

    let output = Command::new(juxc)
        .arg("--run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("failed to spawn juxc");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "juxc exited with {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    assert!(
        stdout.contains("Hello, world!"),
        "expected stdout to contain 'Hello, world!'\nstderr:\n{stderr}\nstdout:\n{stdout}",
    );
}
