//! Regression: the blessed rust.std collection API (auto-prelude Vec/VecDeque,
//! imported HashMap/HashSet, Rust method names, `get` -> `T?`) runs end to end.
use std::path::PathBuf;
use std::process::Command;
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().and_then(|p| p.parent()).expect("ws").to_path_buf()
}
fn run_example(name: &str, emit: &str) -> Vec<String> {
    let jux = env!("CARGO_BIN_EXE_jux");
    let out = Command::new(jux).arg("run").arg("--emit-dir").arg(root().join("target").join(emit)).arg(root().join("examples").join(name)).output().expect("spawn");
    let so = String::from_utf8_lossy(&out.stdout); let se = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "exit {:?}\nstderr:\n{se}\nstdout:\n{so}", out.status.code());
    so.lines().map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}
#[test]
fn rust_collections_runs() {
    assert_eq!(run_example("rust_collections.jux", "it-rust-collections"), [
        "vec len=3 sum=60 v1=20",
        "map has_a=true a=1 miss_null=true",
        "set len=2 iterated=2 has2=true",
        "deque len=3 front=1 back=3",
    ]);
}
