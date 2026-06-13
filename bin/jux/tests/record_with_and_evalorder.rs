//! End-to-end tests for two §M / §S features:
//!
//! - **§M.5 record `with(...)`** — the synthesized wither returns a
//!   copy with named components replaced (`examples/record_with.jux`).
//! - **§S.1.4 named-arg lexical evaluation order (C7)** — re-ordered
//!   named arguments evaluate in call-site source order while landing
//!   in the right parameter slots, for free functions, methods, and
//!   constructors (`examples/named_arg_eval_order.jux`).

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn run_example(name: &str, emit: &str) -> Vec<String> {
    let jux = env!("CARGO_BIN_EXE_jux");
    let source = root().join("examples").join(name);
    let emit_dir = root().join("target").join(emit);
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
    stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[test]
fn record_with_wither() {
    let lines = run_example("record_with.jux", "it-record-with");
    assert_eq!(
        lines,
        [
            "v2: 5,2,3",
            "v3: 5,2,7",
            "v4 eq v: true",
            "v unchanged: 1",
            "u2: Alice Lyon/FR",
            "u unchanged: Paris",
            "done",
        ],
    );
}

#[test]
fn named_arg_lexical_eval_order() {
    let lines = run_example("named_arg_eval_order.jux", "it-named-arg-order");
    assert_eq!(
        lines,
        [
            // Reversed 3-arg: lexical C,B,A; slots a=1,b=2,c=3.
            "eval C",
            "eval B",
            "eval A",
            "three a=1 b=2 c=3",
            // Partial reorder: lexical Y,X,Z.
            "eval Y",
            "eval X",
            "eval Z",
            "three a=10 b=20 c=30",
            // Non-reordered: declaration order, no hoist.
            "eval P",
            "eval Q",
            "eval R",
            "three a=1 b=2 c=3",
            // Constructor reorder: lexical BY,BX.
            "eval BY",
            "eval BX",
            "bx: 4,9",
            "done",
        ],
    );
}
