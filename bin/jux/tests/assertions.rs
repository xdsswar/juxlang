//! End-to-end test for the **`assert` builtin** (§S.7.2) and the
//! **W0720 return-in-finally warning** (§X.3.5).
//!
//! Runs `examples/assertions.jux` (passing assertions), then probes a
//! failing assertion (panic message on stderr + non-zero exit, NOT
//! catchable by `catch (Exception)`) and the W0720 warning (program
//! still compiles and runs).

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
fn assertions_pass_and_fail() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();

    // Passing assertions: program runs to completion.
    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(root.join("target").join("it-assertions"))
        .arg(root.join("examples").join("assertions.jux"))
        .output()
        .expect("spawn jux");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "expected success:\n{stdout}");
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(lines.as_slice(), ["7", "all assertions passed"]);

    // Failing assertion: panic message reported, non-zero exit, and
    // `catch (Exception)` does NOT absorb it (asserts are panics).
    let fail_src = root.join("target").join("it-assert-fail.jux");
    std::fs::write(
        &fail_src,
        r#"package probe;

public void main() {
    try {
        assert(1 > 2, "math broke");
        print("unreachable");
    } catch (Exception e) {
        print("caught");
    }
}
"#,
    )
    .expect("write fail probe");
    let emit = root.join("target").join("it-assert-fail");
    let build = Command::new(jux)
        .arg("build")
        .arg("--emit-dir")
        .arg(&emit)
        .arg(&fail_src)
        .output()
        .expect("spawn jux build");
    assert!(build.status.success(), "build failed: {}", String::from_utf8_lossy(&build.stderr));
    let exe = emit.join("target").join("debug").join(format!(
        "jux_emitted{}",
        std::env::consts::EXE_SUFFIX
    ));
    let run = Command::new(&exe).output().expect("run emitted exe");
    assert!(!run.status.success(), "assert failure must exit non-zero");
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(err.contains("math broke"), "panic message missing:\n{err}");
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(!out.contains("caught"), "assert must not be catchable:\n{out}");
}

#[test]
fn return_in_finally_warns_but_compiles() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let src = root.join("target").join("it-w0720.jux");
    std::fs::write(
        &src,
        r#"package probe;

int risky() {
    try {
        return 1;
    } finally {
        return 2;
    }
}

public void main() {
    print(risky());
}
"#,
    )
    .expect("write w0720 probe");
    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(root.join("target").join("it-w0720"))
        .arg(&src)
        .output()
        .expect("spawn jux");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let all = format!("{stdout}\n{stderr}");
    assert!(output.status.success(), "warning must not block:\n{all}");
    assert!(all.contains("W0720"), "expected W0720 warning:\n{all}");
    // The finally's return wins (Java semantics).
    assert!(stdout.contains('2'), "finally return should win:\n{stdout}");
}
