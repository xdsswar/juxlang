//! End-to-end test for a FUNCTION-TYPED LOCAL declaration —
//! `(int) -> int inc = (n) -> n + 1;`. The grammar has always allowed it
//! (§A.2.7 makes `function-type` a `simple-type`), but the statement starts
//! with `(`, which the parser read as a parenthesized expression or a lambda.
//! Also covers the two shapes it must NOT swallow, and the parenthesized
//! comparison Rust needs to keep (`a < b == true` is a parse error there even
//! though Jux's precedence makes it unambiguous).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn function_typed_locals_parse_as_declarations() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("fn_type_locals.jux");
    let emit_dir = workspace_root.join("target").join("it-fn-type-locals");

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
        ["7", "5", "5", "hey", "3", "3", "true"],
        "unexpected output:\n{stdout}",
    );
}
