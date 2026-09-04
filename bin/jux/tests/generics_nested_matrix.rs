//! End-to-end test for generics crossed with everything that composes with
//! them: nested containers, a generic interface, a generic base, an `A & B`
//! intersection bound, and a subclass that pins its generic parent's parameter
//! (so the inherited interface must arrive instantiated). Also covers handing a
//! polymorphic-base handle to an interface slot the base implements, which is
//! an upcast rather than a second box.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generics_compose_with_interfaces_bases_and_bounds() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("generics_nested_matrix.jux");
    let emit_dir = workspace_root.join("target").join("it-generics-nested-matrix");

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
        ["3", "i1/1", "1", "i1", "i9", "i7/7"],
        "unexpected output:\n{stdout}",
    );
}
