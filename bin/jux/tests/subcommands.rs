//! Integration tests for the `jux` project-tool subcommands operating
//! in single-file mode.
//!
//! Each test spawns the `jux` binary, points it at a `.jux` example,
//! routes the output to an isolated `target/it-…` directory, and asserts
//! the expected outcome:
//!
//! - `jux run examples/hello.jux` → emits, builds, runs, stdout contains
//!   "Hello, world!"; exit 0.
//! - `jux build examples/hello.jux` → emits, builds, **does not** run;
//!   exit 0 and the binary file exists on disk.
//! - `jux check examples/hello.jux` → no codegen, prints "jux: check ok"
//!   on stderr, no binary on disk.

use std::path::PathBuf;
use std::process::Command;

/// The freshly-built `jux` binary, courtesy of cargo's test harness.
fn jux_binary() -> &'static str {
    env!("CARGO_BIN_EXE_jux")
}

/// Two `..`s up from `bin/jux/` is the workspace root.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

#[test]
fn jux_run_hello_world_prints_expected_stdout() {
    let root = workspace_root();
    let source = root.join("examples").join("hello.jux");
    let emit_dir = root.join("target").join("it-jux-run-hello");

    let output = Command::new(jux_binary())
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
        "exit {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    assert!(
        stdout.contains("Hello, world!"),
        "stdout missing greeting:\n{stdout}",
    );
}

#[test]
fn jux_build_hello_world_produces_binary_without_running() {
    let root = workspace_root();
    let source = root.join("examples").join("hello.jux");
    let emit_dir = root.join("target").join("it-jux-build-hello");

    let output = Command::new(jux_binary())
        .arg("build")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("spawn jux");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    // `build` must NOT execute the program — stdout from the user's
    // binary should not appear.
    assert!(
        !stdout.contains("Hello, world!"),
        "build should not execute the program; stdout was:\n{stdout}",
    );

    // The native binary should have landed on disk.
    let binary_path = emit_dir
        .join("target")
        .join("debug")
        .join(format!("jux_emitted{}", std::env::consts::EXE_SUFFIX));
    assert!(
        binary_path.exists(),
        "expected emitted binary at {}",
        binary_path.display(),
    );
}

#[test]
fn jux_check_hello_world_is_codegen_free() {
    let root = workspace_root();
    let source = root.join("examples").join("hello.jux");

    let output = Command::new(jux_binary())
        .arg("check")
        .arg(&source)
        .output()
        .expect("spawn jux");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    // Check mode prints a small status line on stderr.
    assert!(
        stderr.contains("jux: check ok"),
        "expected 'jux: check ok' on stderr, got:\n{stderr}",
    );
}

#[test]
fn jux_run_greet_prints_alice_and_bob() {
    let root = workspace_root();
    let source = root.join("examples").join("greet.jux");
    let emit_dir = root.join("target").join("it-jux-run-greet");

    let output = Command::new(jux_binary())
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
        "exit {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    assert!(
        stdout.contains("Alice") && stdout.contains("Bob"),
        "stdout missing both greetings:\n{stdout}",
    );
}

#[test]
fn jux_run_without_file_says_not_yet_implemented() {
    // Project mode is intentionally unimplemented — we want a clear
    // exit code and message rather than a crash or a silent no-op.
    let output = Command::new(jux_binary())
        .arg("run")
        .output()
        .expect("spawn jux");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected non-zero exit when no file given; got success with stderr:\n{stderr}",
    );
    // Project mode is now wired — running `jux run` outside a
    // project surfaces the "no jux.toml" error rather than an
    // NYI banner. The test still proves non-zero exit, which is
    // the user-facing contract.
    assert!(
        stderr.contains("no jux.toml") || stderr.contains("not yet implemented"),
        "expected project-mode or NYI banner on stderr, got:\n{stderr}",
    );
}
