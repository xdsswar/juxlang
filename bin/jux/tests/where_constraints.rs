//! End-to-end test for **generic where-constraints** (§O.5):
//! `where T has operator OP` satisfied by primitives, String, and a
//! class declaring the operator — plus the E0941 rejection when the
//! bound type lacks it.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

#[test]
fn where_constraints_satisfied() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(root.join("target").join("it-where-constraints"))
        .arg(root.join("examples").join("where_constraints.jux"))
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
        ["7", "pear", "[v5]", "[42]"],
        "unexpected output:\n{stdout}",
    );
}

#[test]
fn unsatisfied_constraint_fires_e0941() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let src = root.join("target").join("it-e0941.jux");
    std::fs::write(
        &src,
        r#"package probe;

class Opaque {
    private int n;
    Opaque(int n) { this.n = n; }
}

T maxOf<T>(T a, T b) where T has operator<=>(T) -> int {
    return a <=> b > 0 ? a : b;
}

public void main() {
    var x = maxOf(new Opaque(1), new Opaque(2));
}
"#,
    )
    .expect("write probe");
    let output = Command::new(jux)
        .arg("build")
        .arg("--emit-dir")
        .arg(root.join("target").join("it-e0941"))
        .arg(&src)
        .output()
        .expect("spawn jux build");
    let all = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "expected E0941 rejection:\n{all}");
    assert!(all.contains("E0941"), "expected E0941 diagnostic:\n{all}");
}
