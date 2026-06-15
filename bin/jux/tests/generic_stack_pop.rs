//! Regression harness for examples/generic_stack_pop.jux (emit -> rustc -> run, assert output).

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn run_example(name: &str, emit: &str) -> Vec<String> {
    let jux = env!("CARGO_BIN_EXE_jux");
    let source = root().join("examples").join(name);
    let emit_dir = root().join("target").join(emit);
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
    stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[test]
fn generic_stack_pop_runs() {
    let lines = run_example("generic_stack_pop.jux", "it-generic-stack-pop");
    assert_eq!(lines, ["popped=cba"]);
}
