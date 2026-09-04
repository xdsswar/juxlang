//! End-to-end test for a collection whose ELEMENT type is polymorphic. A
//! `Vec<Speaker>` (interface) and a `Vec<Animal>` (polymorphic base class) both
//! lower to a trait-object element, so storing an implementer / subclass has to
//! WRAP it rather than store the bare struct. Both shapes previously leaked a
//! raw rustc E0308 at the `push`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn polymorphic_collection_elements() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("poly_collections.jux");
    let emit_dir = workspace_root.join("target").join("it-poly-collections");

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
        "jux exited with {:?}
stderr:
{stderr}
stdout:
{stdout}",
        output.status.code(),
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(lines.as_slice(), ["Tweet", "Beep", "rex: Woof", "tom: Meow", "1"], "unexpected output:
{stdout}");
}
