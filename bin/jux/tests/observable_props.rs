//! End-to-end test for the §P observable-property follow-ups
//! (P1/P2/P4/P5/P6/P7 in `jux-gaps.md`):
//!
//! - P1: computed (get-only) properties fire on dependency change;
//! - P2: assigning a one-way-bound property throws
//!   `IllegalStateException` (debug builds — the example runs through
//!   `jux run`, which builds debug, so the guard is active);
//! - P4: `unbind()` after `bindBidirectional` kills BOTH directions;
//! - P5: a named 3-arg observer's adapter is pruned after its owner
//!   dies;
//! - P6: `bind()` on `this` inside a constructor (deferred replay);
//! - P7: static observable properties (thread_local storage).

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn observable_property_follow_ups() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let source = root().join("examples").join("observable_props_full.jux");
    let emit_dir = root().join("target").join("it-observable-props");

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
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(
        lines.as_slice(),
        [
            // P1 — computed property fires on dependency change only.
            "full: / -> Ada/",
            "full: Ada/ -> Ada/Lovelace",
            // P4 — one unbind breaks both directions.
            "a after b=5: 5",
            "a after unbind + b=9: 5",
            "b after a=7: 9",
            // P2 — one-way-bound target refuses direct sets.
            "t follows: 3",
            "caught: bound assign refused",
            "after unbind set: 4",
            // P5 — adapter dies with its owner.
            "watch Value 0->1",
            "size with owner: 1",
            "size after owner death: 0",
            // P6 — ctor bind: initial sync + live binding.
            "initial sync: 10",
            "follows: 42",
            // P7 — static props: change-gated fires, size, clear.
            "level: 0 -> 3",
            "level: 3 -> 7",
            "static size: 1",
            "done",
        ],
        "unexpected output:\n{stdout}",
    );
}
