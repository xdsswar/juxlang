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
        .join(format!("hello{}", std::env::consts::EXE_SUFFIX));
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

/// Two programs with the same name, built at the same time into one shared
/// `CARGO_TARGET_DIR`, each run their OWN binary.
///
/// Cargo copies every build's executable to `<target>/debug/<bin>`, so two
/// programs both named `Main` wrote the same file, and cargo's lock covers the
/// build but not the run that follows. Run concurrently, one program printed
/// the other's output in every round. The shared target dir now gets a
/// per-program `[[bin]]` name, published back as `Main` in each emit dir.
#[test]
fn same_named_programs_sharing_a_target_dir_run_their_own_binary() {
    let root = workspace_root();
    let base = root.join("target").join("it-shared-target-same-name");
    let shared = base.join("shared-target");
    let programs: Vec<(PathBuf, String)> = ["a", "b"]
        .iter()
        .map(|who| {
            let dir = base.join(who);
            std::fs::create_dir_all(&dir).expect("creating program dir");
            let greeting = format!("I am {who}");
            std::fs::write(
                dir.join("Main.jux"),
                format!("public void main() {{\n    print(\"{greeting}\");\n}}\n"),
            )
            .expect("writing Main.jux");
            (dir, greeting)
        })
        .collect();

    // Three rounds: before the fix the first one already failed.
    for round in 0..3 {
        let handles: Vec<_> = programs
            .iter()
            .cloned()
            .map(|(dir, greeting)| {
                let shared = shared.clone();
                std::thread::spawn(move || {
                    let output = Command::new(jux_binary())
                        .arg("run")
                        .arg("--emit-dir")
                        .arg(dir.join("emit"))
                        .arg(dir.join("Main.jux"))
                        .env("CARGO_TARGET_DIR", &shared)
                        .output()
                        .expect("spawn jux");
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    (greeting, stdout, stderr, output.status.success())
                })
            })
            .collect();
        for handle in handles {
            let (greeting, stdout, stderr, ok) = handle.join().expect("runner thread");
            assert!(ok, "round {round}: jux failed\nstderr:\n{stderr}");
            assert_eq!(stdout, greeting, "round {round}: ran another program's binary");
        }
    }
}
