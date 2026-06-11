//! A generic class implementing a generic interface, used polymorphically
//! through the interface-typed slot (`Container<int> c = new Box<int>(…)`).
//! Generic dispatch through an interface is the supported Phase-1 route; the
//! inherent-forwarding shim must emit call-position turbofish (`Box::<T>::get`)
//! and the value must dispatch correctly. Correct output: `42` then `hi`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generic_class_through_generic_interface() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("generic_iface_poly.jux");
    let emit_dir = root.join("target").join("it-generic-iface-poly");

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
        "compile/run failed:\nstderr:\n{stderr}\nstdout:\n{stdout}"
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(lines.as_slice(), ["42", "hi"], "unexpected output:\n{stdout}");
}
