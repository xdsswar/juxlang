//! Integration test for the milestone-2 language-coverage additions.
//!
//! Compiles `examples/arithmetic.jux` end-to-end through `juxc --run` and
//! asserts that its stdout contains `big` (from the `if`-branch) and the
//! number `30` (from the printed sum). This jointly exercises:
//!
//! - `var name = expr ;` local declarations with type inference
//! - Binary `+` (additive) and `>` (comparison)
//! - `if (cond) { … } else { … }`
//! - `print(int)` (a numeric argument to the built-in)
//!
//! When this test breaks, something in the var/if/binary path regressed.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn arithmetic_example_runs_and_prints_expected_output() {
    let juxc = env!("CARGO_BIN_EXE_juxc");

    // bin/juxc → bin → workspace root.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc");

    let source = workspace_root.join("examples").join("arithmetic.jux");
    // Use a dedicated emit dir so we don't collide with the hello-world test.
    let emit_dir = workspace_root.join("target").join("it-arithmetic");

    let output = Command::new(juxc)
        .arg("--run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("failed to spawn juxc");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "juxc exited with {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    assert!(
        stdout.contains("big"),
        "expected stdout to contain 'big' (if-branch taken)\nstderr:\n{stderr}\nstdout:\n{stdout}",
    );
    assert!(
        stdout.contains("30"),
        "expected stdout to contain '30' (printed sum)\nstderr:\n{stderr}\nstdout:\n{stdout}",
    );
}
