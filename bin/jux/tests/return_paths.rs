//! End-to-end test for **return-completeness** (E0460, gap N3): a non-void
//! function must `return`/`throw` on every path. The positive example exercises
//! the shapes recognised as diverging (if/else both return, trailing
//! `while (true)`, `throw`); the negative probes assert E0460 fires when a path
//! falls through, and that `void` / `async void` are exempt.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

fn build_probe(name: &str, source: &str) -> (bool, String) {
    let jux = env!("CARGO_BIN_EXE_jux");
    let dir = workspace_root().join("target").join(format!("it-ret-{name}"));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let src = dir.join("probe.jux");
    std::fs::write(&src, source).expect("write probe");
    let output = Command::new(jux)
        .arg("build")
        .arg("--emit-dir")
        .arg(dir.join("emit"))
        .arg(&src)
        .output()
        .expect("spawn jux");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    (output.status.success(), format!("{stdout}\n{stderr}"))
}

#[test]
fn return_paths() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let source = root.join("examples").join("return_paths.jux");
    let emit_dir = root.join("target").join("it-return-paths");

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
    assert_eq!(lines.as_slice(), ["-1", "0", "1", "7"], "unexpected output:\n{stdout}");
}

#[test]
fn missing_return_on_else_path_is_e0451() {
    let (ok, all) = build_probe(
        "fallthrough",
        r#"package badret;

int classify(int x) {
    if (x > 0) {
        return 1;
    }
    // falls off the end on the false path
}

public void main() { print(classify(5)); }
"#,
    );
    assert!(!ok, "missing-return unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0460]"), "expected E0460, got:\n{all}");
}

#[test]
fn void_and_async_void_are_exempt() {
    // Neither a plain `void` nor an `async void` body needs a return value.
    let (ok, all) = build_probe(
        "void-exempt",
        r#"package okret;

void doNothing() {}

public async void main() {
    doNothing();
    print("ok");
}
"#,
    );
    assert!(ok, "void / async void wrongly required a return:\n{all}");
}
