//! End-to-end test for full Java-style generics
//! (`examples/generics_registry.jux`): F-bounded self-types, recursive
//! multi-bounds, a nested generic bound carrying a wildcard, a const generic
//! mixed with type params, a fresh method type-param bounded by a class param,
//! static generic methods with nested generic returns, PECS wildcards at use
//! sites, and nested generic construction. The program must emit valid Rust,
//! compile, and run with the expected output.

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
fn generics_registry_runs() {
    let lines = run_example("generics_registry.jux", "it-generics-registry");
    assert_eq!(
        lines,
        [
            "capacity=16",        // const generic N read as a value
            "pair.first.id=1",    // <R extends K> pairWith + nested Box<Pair<K,R>>
            "max.id=3",           // F-bounded static <E extends Entity<E>> maxById
            "drained: ada",       // ? super K consumer write through Sink
            "instances=1",        // static state on a generic class
        ],
    );
}
