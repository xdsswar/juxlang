//! End-to-end tests for the bug-hunt-2 wave:
//!
//! - Java inheritance semantics for OBSERVABLE properties: a subclass
//!   object attaches/binds/fires its inherited properties, sets
//!   through a base-typed reference fire, two-level chains work
//!   (`examples/inherited_observers.jux`);
//! - `typeof(expr)` (§5.9.10): compile-time static type names
//!   (`examples/typeof_query.jux`).

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
fn inherited_observable_properties_java_semantics() {
    let lines = run_example("inherited_observers.jux", "it-inherited-observers");
    assert_eq!(
        lines,
        [
            "score 0 -> 7",
            "size: 1",
            "own 5->6",
            "score 7 -> 9", // set through a BASE-typed reference fires
            "tag none->deep",
            "gsize: 1",
            "score 9 -> 42",
            "bound: 42",
            "score 42 -> 50",
            "after unbind: 42",
            "done",
        ],
    );
}

#[test]
fn typeof_static_type_names() {
    let lines = run_example("typeof_query.jux", "it-typeof");
    assert_eq!(
        lines,
        ["int", "double", "String", "Point", "Vec<int>", "int?", "int", "done"],
    );
}
