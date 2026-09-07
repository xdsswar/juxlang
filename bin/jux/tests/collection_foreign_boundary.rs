//! End-to-end test for a collection crossing into a Rust crate
//! (JUX-LANG-V1 §6.5.1, JUX-BINDGEN §G).
//!
//! Nothing in the corpus passed a collection to a foreign function, so when
//! collections became shared handles the bridge that used to make this work
//! went quietly wrong: `foreign_arg_bridges` still accepted the argument, and
//! the backend still added a `&`, but `&Rc<RefCell<Vec<T>>>` coerces to no
//! slice at all. A plain Jux array in the same slot had never worked either --
//! a slice parameter carries no `&` marker in the stub for anything to notice.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn a_collection_lends_its_interior_to_a_rust_crate() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("collection_foreign_boundary.jux");
    let emit_dir = workspace_root.join("target").join("it-collection-foreign");

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
            "3", // a handle lends its interior to a slice slot
            "3", // a Jux array reaches the same slot
            "4", // self-aliasing copies instead of panicking
            "2", // a handle field, read through its owner's own cell
            "1", // a foreign named constructor's result is wrapped
            "1", // ... for every container, not just Vec
        ],
        "unexpected output:\n{stdout}",
    );
}
