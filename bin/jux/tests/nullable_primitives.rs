//! End-to-end test for **nullable primitives** — `int?` / `bool?` /
//! `<int?>` generic args (spec: `T?` ≡ `Option<T>`, no reference-type
//! restriction).
//!
//! Runs `examples/nullable_primitives.jux`. A null primitive is an
//! unallocated `Option::None` (stack tag — no Java-style boxing).
//! Covers locals, smart-cast, elvis, `!!`, params/returns, fields,
//! `Box<int?>` ctor `Some`-lifting, and `ArrayList<int?>` /
//! `HashMap<String, int?>` element lifting.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn nullable_primitives_end_to_end() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("nullable_primitives.jux");
    let emit_dir = workspace_root.join("target").join("it-nullable-prims");

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
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(
        lines.as_slice(),
        ["6", "-1", "5", "10", "0", "7", "-1", "42", "2", "1", "-1", "9", "-1"],
        "unexpected output:\n{stdout}",
    );
}
