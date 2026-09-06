//! End-to-end guard for five Java rules Jux used to get wrong. Each one either
//! produced a wrong diagnostic on legal code or, worse, compiled and silently
//! did the wrong thing.
//!
//! The output is asserted line by line because four of the five failures were
//! *silent*: the program built and ran, and only the numbers were wrong.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn java_parity_rules_hold_end_to_end() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("java_parity_shadowing.jux");
    let emit_dir = workspace_root.join("target").join("it-java-parity-shadowing");

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
            // A file-local `Box<T>` shadows `rust.std.Box` (JLS 6.4.1). This
            // used to resolve to the foreign type and fail with "no method
            // `get` on type `rust.std.Box`".
            "42",
            // `map.get(k)!!.push(v)` mutates the map. The reference getter
            // lowered to `.get(k).cloned()`, so every push landed on a
            // temporary and these three read 0, 0, 0.
            "2",
            "1",
            "0",
            // A `private` nested class, reaching its owner's private state:
            // two bumps plus the owner's tag of 7. The class itself was
            // rejected as a top-level `private` type (E0432), because nested
            // types are lifted to a flat `Owner__Name` before the check ran.
            "9",
            // An `internal` field read from outside. It was reported as a
            // "private field" (E0437) even though the accessor it needs was
            // being generated.
            "7",
            // A `sealed` base carrying state: virtual dispatch reaches the
            // overrides. The enum lowering has nowhere to keep the parent's
            // fields, and this hierarchy used to emit Rust that referenced a
            // field no variant had.
            "12",
            "9",
            // An inherited `protected final` field and a `static final` read
            // from a `final` method on that same sealed base.
            "shape#1",
            // The sealed base's `static { }` block ran.
            "100",
        ],
        "unexpected output:\n{stdout}",
    );
}
