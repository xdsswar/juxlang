//! Regression: nullable generic-field getter must not double-wrap (E0308).
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
fn nullable_generic_getter_runs() {
    assert_eq!(run_example("nullable_generic_getter.jux", "it-nullable-generic-getter"), ["k0=none", "k1=hello t=-1"]);
}
