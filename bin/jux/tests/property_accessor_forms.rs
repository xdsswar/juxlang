//! End-to-end test for `examples/property_accessor_forms.jux` — the four
//! property forms, the implicit `value` in a custom setter, and a property
//! satisfying an interface's accessor.
//!
//! Its real job is being in the corpus. The plugin's highlighting test runs
//! every example through the annotator and all eighteen inspections, so a shape
//! the corpus does not contain is a shape that test cannot guard — and until
//! now the corpus had no custom `set { }` body at all. Two editor bugs lived in
//! exactly that blind spot: `value` reported as an unresolved symbol, and a
//! property implementing `Named.Name()` reported as unimplemented.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn property_forms_and_the_implicit_setter_value() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("property_accessor_forms.jux");
    let emit_dir = workspace_root.join("target").join("it-property-accessor-forms");

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
            // an auto-property with an initializer, written and read back
            "2",
            // an expression-bodied property, which also satisfies `Named.Name()`
            "widget-2",
            // the interface default calling through that property
            "hi widget-2",
            // a full accessor pair: the setter passes the value through
            "Hello",
            // and then rewrites it, which is what `value` is for
            "(empty)",
            // an auto-property with no initializer is implicitly nullable
            "true",
            "set",
        ],
        "unexpected output:\n{stdout}",
    );
}
