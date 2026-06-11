//! End-to-end test for **generic `extends` bounds** (§T.4, E0446):
//! bounded generic METHODS dispatch on conforming args, bounded
//! classes instantiate, and a violation produces the Jux E0446
//! diagnostic instead of a leaked rustc E0277.

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
fn generic_method_bounds() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let source = root.join("examples").join("generic_method_bounds.jux");
    let emit_dir = root.join("target").join("it-generic-method-bounds");

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
        ["6.28318", "18", "4"],
        "unexpected output:\n{stdout}",
    );
}

#[test]
fn generic_bound_violation_is_e0446() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = workspace_root();
    let bad_dir = root.join("target").join("it-bound-violation");
    std::fs::create_dir_all(&bad_dir).expect("create probe dir");
    let bad_src = bad_dir.join("violation.jux");
    std::fs::write(
        &bad_src,
        r#"package badbounds;

public interface Marked { int tag(); }

public class Plain { public Plain() {} }

public class Box<T extends Marked> {
    public T item;
    public Box(T item) { this.item = item; }
}

<T extends Marked> void take(T x) {}

public void main() {
    var b = new Box<Plain>(new Plain());
    take(new Plain());
}
"#,
    )
    .expect("write violation probe");

    let output = Command::new(jux)
        .arg("build")
        .arg("--emit-dir")
        .arg(bad_dir.join("emit"))
        .arg(&bad_src)
        .output()
        .expect("spawn jux");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let all = format!("{stdout}\n{stderr}");
    assert!(
        !output.status.success(),
        "bound violation unexpectedly compiled:\n{all}",
    );
    assert!(
        all.contains("[E0446]"),
        "expected an E0446 diagnostic, got:\n{all}",
    );
    assert!(
        !all.contains("E0277"),
        "rustc trait-bound error leaked through:\n{all}",
    );
}
