//! Re-entrancy borrow-soundness through a `!!`-asserted wrapped field receiver.
//!
//! `this.inner!!.touch()` / `a.inner!!.touch()` read a field to obtain the call
//! receiver, opening a `.0.borrow()` guard that the `!!` non-null assertion does
//! NOT change. If that guard is held across `touch()` — which re-enters the same
//! object and takes `.0.borrow_mut()` — the program panics `RefCell already
//! borrowed`. The receiver-hoist must drop the read-borrow before the call. Both
//! the `this.<field>!!` and local `<var>.<field>!!` shapes must run clean and
//! print `count=1` (the single re-entrant `poke()`).

use std::path::PathBuf;
use std::process::Command;

fn run_example(name: &str) -> (bool, String) {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join(format!("{name}.jux"));
    let emit_dir = root.join("target").join(format!("it-{name}"));
    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("spawn jux");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    (output.status.success(), format!("{stdout}{stderr}"))
}

#[test]
fn notnull_field_receiver_hoists_borrow() {
    let (ok, all) = run_example("reentrancy_notnull_this");
    assert!(
        ok,
        "a RefCell-already-borrowed panic here means the `!!` field receiver \
         did not hoist the read-borrow before the re-entrant call:\n{all}"
    );
    assert!(all.contains("count=1"), "unexpected output:\n{all}");
}

#[test]
fn notnull_local_receiver_hoists_borrow() {
    let (ok, all) = run_example("reentrancy_notnull_local");
    assert!(ok, "re-entrancy panic on local `!!` field receiver:\n{all}");
    assert!(all.contains("count=1"), "unexpected output:\n{all}");
}
