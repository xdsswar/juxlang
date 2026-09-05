//! End-to-end test for `examples/op_scalar_operand.jux` — a binary operator
//! whose operand type differs from the receiver's.
//!
//! Rust's `std::ops` traits default their operand to `Self`, so a same-type
//! operator needs nothing written: `impl Add for Vec2`. A scalar one does —
//! `Vec2 operator *(double k)` is `impl Mul<f64> for Vec2`. The type argument
//! was never emitted, so the impl claimed `Mul<Vec2>` while writing
//! `fn mul(self, rhs: f64)`, and rustc rejected the mismatch (E0053): the whole
//! crate failed to build over scaling a vector, which is the first thing anyone
//! writes with operator overloading.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn operators_take_their_declared_operand_type() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("op_scalar_operand.jux");
    let emit_dir = workspace_root.join("target").join("it-op-scalar-operand");

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
        [
            // same-type operands, which the trait's default already covers
            "(4, 6)",
            "(3, 4)",
            // a scalar operand — the case that did not compile
            "(2, 4)",
            "(1.5, 2)",
            // equality and hashing still pair correctly alongside them
            "true",
            // a second, differently-typed scalar operand in the same program
            "t12",
        ],
        "unexpected output:\n{stdout}",
    );

    // The impl headers themselves: the same-type operator stays idiomatic and
    // argument-free, the scalar one names its operand. Asserting the emitted
    // shape keeps both halves of the rule honest — always writing the argument
    // would compile too, but would make ordinary operators read as noise.
    let emitted = std::fs::read_to_string(emit_dir.join("src").join("main.rs"))
        .expect("emitted crate is readable");
    assert!(
        emitted.contains("impl std::ops::Add for Vec2"),
        "a same-type operand needs no type argument:\n{emitted}",
    );
    assert!(
        emitted.contains("impl std::ops::Mul<f64> for Vec2"),
        "a scalar operand is the trait's type argument:\n{emitted}",
    );
}
