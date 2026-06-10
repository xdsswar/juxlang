//! End-to-end test for **const generics** — `<int N>` / `<bool B>`
//! value parameters (grammar §A.2.6, type-system §T.11.3), Phase-1
//! core subset.
//!
//! Runs `examples/const_generics.jux`, which exercises:
//! - a mixed type+const generic class (`Ring<T, int N>`) with `T[N]`
//!   stack storage (lowers to Rust `[T; N]` under `const N: usize`),
//! - `N` read as an int value (`capacity()`, `N * 2`),
//! - literal instantiation (`new Ring<int, 8>(0)`, `new Buf<4>()`),
//! - a const arg in type position (`Buf<4> b2`),
//! - a `bool` const param (`Flag<true>`),
//! - a const-generic free function via explicit turbofish
//!   (`scaled<3>()`).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn const_generics_core_subset() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("const_generics.jux");
    let emit_dir = workspace_root.join("target").join("it-const-generics");

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
        ["8", "4", "8", "4", "true", "30"],
        "unexpected output:\n{stdout}",
    );
}
