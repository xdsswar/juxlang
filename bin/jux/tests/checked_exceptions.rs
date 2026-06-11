//! End-to-end test for **checked-exception enforcement** (§X.1.3).
//!
//! Runs `examples/checked_exceptions.jux` (declared / caught /
//! unchecked / broader-declaration shapes all legal), then probes the
//! E0711 rejections: an undeclared propagating call and an undeclared
//! direct checked throw.

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
fn checked_exceptions_legal_shapes() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(root.join("target").join("it-checked-exceptions"))
        .arg(root.join("examples").join("checked_exceptions.jux"))
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
        ["recovered: bad host", "broad: bad host", "evicted: cold"],
        "unexpected output:\n{stdout}",
    );
}

#[test]
fn undeclared_checked_raise_fires_e0711() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let src = root.join("target").join("it-e0711.jux");
    std::fs::write(
        &src,
        r#"package probe;

class ConfigError extends Exception {
    ConfigError(String m) { super(m); }
}

void load() throws ConfigError {
    throw new ConfigError("bad");
}

void leaky() {
    load();
}

void thrower() {
    throw new ConfigError("oops");
}

public void main() {
}
"#,
    )
    .expect("write probe");
    let output = Command::new(jux)
        .arg("build")
        .arg("--emit-dir")
        .arg(root.join("target").join("it-e0711"))
        .arg(&src)
        .output()
        .expect("spawn jux build");
    let all = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "expected E0711 rejection:\n{all}");
    assert_eq!(
        all.matches("E0711").count(),
        2,
        "expected exactly two E0711 diagnostics (leaky + thrower):\n{all}",
    );
}
