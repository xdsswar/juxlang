//! End-to-end test for **explicit call-site type arguments** — the
//! postfix turbofish form `expr<T>(args)` (spec §A.2.9 / §T.4).
//!
//! Runs `examples/explicit_type_args.jux`, which pins generic
//! parameters explicitly across several call shapes:
//! - a single-param generic free function (`identity<int>(5)`),
//! - an explicit arg the literal alone couldn't produce
//!   (`identity<long>(5)` must bind `T = long`, lowered to a Rust
//!   turbofish `identity::<i64>(5)`),
//! - a `String`-typed explicit arg (`echo<String>("hello")`),
//! - inference still working side-by-side (`echo(42)`),
//! - an explicit arg on an instance method (`w.wrap<bool>(true)`),
//! - the less-than operator left untouched by the turbofish lookahead.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn explicit_call_site_type_arguments() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("explicit_type_args.jux");
    let emit_dir = workspace_root.join("target").join("it-explicit-type-args");

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
        ["5", "5", "hello", "42", "true", "true"],
        "unexpected output:\n{stdout}",
    );
}
