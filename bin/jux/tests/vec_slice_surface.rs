//! `Vec` inherits `[T]`'s methods through `Deref`, and the stub must say so.
//!
//! Before bindgen followed `Deref`, the scanned `Vec` had 58 methods and none
//! of `first`, `last`, `contains`, `reverse` or `is_empty` — which is what
//! pushed the backend into hardcoding some of them. This asserts the surface
//! is discovered, by running a program that uses it.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn vec_has_its_slice_methods() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("vec_slice_surface.jux");
    let emit_dir = workspace_root.join("target").join("it-vec-slice-surface");

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
            "first=10 last=30",
            "contains20=true contains99=false",
            "empty=false len=3",
            "reversedFirst=30",
            "hasAlpha=true",
        ],
        "unexpected output:\n{stdout}",
    );
}
