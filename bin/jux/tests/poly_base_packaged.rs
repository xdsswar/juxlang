//! End-to-end test for **virtual dispatch through a concrete base class in a
//! packaged program**. The inheritance walk that drives the base-class upcast
//! coercion is FQN/bare-name tolerant, so a `Container b = new Box(7)` in
//! `package poly;` coerces to `Rc<dyn ContainerKind>` and the overridden
//! `describe()` dispatches at run time — previously a leaked rustc E0308.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn poly_base_packaged() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("poly_base_packaged.jux");
    let emit_dir = workspace_root.join("target").join("it-poly-base-packaged");

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
    assert_eq!(lines.as_slice(), ["7", "box of 7"], "unexpected output:\n{stdout}");
}
