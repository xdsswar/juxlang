//! End-to-end test for FnMut / mutable-capture closures: a closure that mutates
//! a captured local lowers to a shared `Rc<RefCell>` cell observed both inside
//! the closure and in the outer scope (Java capture-by-reference). A read-only
//! capture stays a plain capture-by-value.

use std::path::PathBuf;
use std::process::Command;

fn run_example(name: &str, emit: &str) -> Vec<String> {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let source = root.join("examples").join(name);
    let emit_dir = root.join("target").join(emit);
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
fn mutable_capture_closures() {
    let lines = run_example("mut_capture_closure.jux", "it-mut-capture-closure");
    assert_eq!(lines, ["105", "3", "abb", "2"]);
}
