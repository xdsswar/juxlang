//! End-to-end test for **bounded wildcards in parameter position**
//! (Java PECS, Phase-1 scope).
//!
//! Runs `examples/wildcards.jux`. A `Bag<? extends Animal>` parameter
//! lifts to a synthetic function generic (`fn describe<__W0:
//! AnimalKind + Clone>(xs: Bag<__W0>)`), so the same `describe` accepts
//! both `Bag<Dog>` and `Bag<Cat>`. Storage-position wildcards are a
//! separate story — diagnosed with E0444 (see `wildcards_storage`).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn bounded_wildcard_in_parameter_position() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("wildcards.jux");
    let emit_dir = workspace_root.join("target").join("it-wildcards");

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
        ["got a bag of animals", "got a bag of animals"],
        "unexpected output:\n{stdout}",
    );
}
