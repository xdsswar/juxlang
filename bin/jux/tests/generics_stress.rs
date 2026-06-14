//! End-to-end test for `examples/generics_stress.jux`: generics patterns the
//! Registry program does not cover. A multi-bound class param that also
//! `implements` a generic interface with that param
//! (`Cage<T extends Animal & CanFly> implements Swing<T>`), a generic class
//! extending a generic class with `super`, a class implementing multiple
//! interfaces, and a generic free function with a bounded-wildcard producer +
//! inference. Must emit valid Rust, compile, and run with the expected output.

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
fn generics_stress_runs() {
    let lines = run_example("generics_stress.jux", "it-generics-stress");
    assert_eq!(
        lines,
        [
            "describe=tweet @ 100", // multi-bound T used through both interfaces
            "swung=tweet",          // implements Swing<T> with the class param
            "label=answer get=42",  // NamedBox<T> extends Box<T> + super(...)
            "first=tweet",          // generic free fn firstOf, T inferred = Bird
        ],
    );
}
