//! A concrete exception subclass passed where an `Exception` cause parameter is
//! expected. Regression for two bugs: (1) the subclass argument wasn't upcast to
//! its base, leaking a rustc E0308 — it now slices up via the generated
//! `From<Sub> for Exception` (`.into()`); (2) the overloaded-constructor arg
//! coercion picked `constructors.first()`, so args past the first ctor's arity
//! got no coercion — it now resolves the ctor overload by call arity.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn exception_subclass_upcasts_to_cause_param() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("exception_cause_upcast.jux");
    let emit_dir = root.join("target").join("it-exception-cause-upcast");

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
        "compile/run failed (a leaked E0308 means the subclass→base cause upcast \
         regressed):\nstderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(stdout.contains("nested"), "unexpected output:\n{stdout}");
}
