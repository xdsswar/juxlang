//! Nullable generic field `T?` from a nullable ctor param on the WRAPPED
//! (Rc<RefCell>) representation a class gets when stored in a collection. The
//! wrapped builder must not re-wrap a value that is already `Option<T>`
//! (`Some(d)` → `Option<Option<T>>` leaks a rustc E0308). Then `?.` chains off
//! the nullable field. Correct output: `5` then `-1`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn nullable_generic_field_in_collection() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("nullable_generic_collection.jux");
    let emit_dir = root.join("target").join("it-nullable-generic-collection");

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
        "compile/run failed (a double-`Some` on the wrapped builder leaks an \
         E0308 here):\nstderr:\n{stderr}\nstdout:\n{stdout}"
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(lines.as_slice(), ["5", "-1"], "unexpected output:\n{stdout}");
}
