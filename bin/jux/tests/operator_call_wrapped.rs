//! `operator()` on a wrapped (Rc<RefCell>) class whose body mutates a field.
//! Regression: it was emitted with `&mut self` (like an inline class), but a
//! wrapper mutates through interior `self.0.borrow_mut()` and all its methods
//! take `&self`. The `&mut self` forced a `let mut` at the call site that the
//! binding emitter doesn't produce, leaking a rustc E0596.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn operator_call_on_wrapped_class_takes_shared_self() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("operator_call_wrapped.jux");
    let emit_dir = root.join("target").join("it-operator-call-wrapped");

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
        "compile/run failed (an E0596 means operator() emitted &mut self on a \
         wrapper):\nstderr:\n{stderr}\nstdout:\n{stdout}"
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(lines.as_slice(), ["5", "8"], "unexpected output:\n{stdout}");
}
