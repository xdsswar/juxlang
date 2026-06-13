//! Regression: `typeof(obj.Prop)` for a C#-style property must report the
//! property's declared type, not `<unknown>` (property reads weren't typed —
//! the backing field is mangled and the getter lives among the methods).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn typeof_of_property_reports_declared_type() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join("typeof_property.jux");
    let emit_dir = root.join("target").join("it-typeof-property");

    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("spawn jux");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let all = format!("{stdout}{stderr}");
    assert!(output.status.success(), "compile/run failed:\n{all}");
    assert!(!stdout.contains("<unknown>"), "property type leaked as <unknown>:\n{all}");
    // Two String properties and one int, in order.
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.contains(&"String"), "expected String, got:\n{all}");
    assert!(lines.contains(&"int"), "expected int, got:\n{all}");
    // `typeof` inside a `$"…"` interpolation hole must resolve the same as
    // outside it (regression for interp-hole span collisions in expr_types).
    assert!(
        lines.contains(&"interp: String int"),
        "typeof inside interpolation reported the wrong type:\n{all}",
    );
}
