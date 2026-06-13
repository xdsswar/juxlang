//! End-to-end test for §M.14 parameter features: default parameters,
//! varargs, `final` parameters, `ref` parameters, `weak` parameters, and
//! their legal combinations (`final` + default, `final` + varargs, `final ref`).

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
fn parameters_full() {
    let lines = run_example("parameters_full.jux", "it-parameters-full");
    assert_eq!(
        lines,
        [
            "Hello, Jux!",  // default greeting omitted
            "Hi, Jux!",     // default greeting supplied
            "total0 = 10",  // varargs: zero trailing args
            "totalN = 10",  // varargs: 1+2+3+4 over base 1... = 10
            "x = 105",      // final ref param store-through (5 + 100)
            "counter = 7",  // weak param read via .get()
            "done",
        ],
    );
}
