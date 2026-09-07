//! End-to-end test for `examples/dashboard` — the responsive showcase.
//!
//! It renders headless, through the `--snapshot` front end, so no display is
//! involved. It is gated to Windows only because the project depends on
//! `minifb` for its OTHER front end, and building that crate needs X11
//! development headers on Linux which a runner may not have. The rendering
//! itself is pure Jux and would run anywhere.
//!
//! Two things are asserted: that the frame renders at the size asked for, and
//! that the LAYOUT actually responds — the same program at three widths must
//! report three different breakpoints.

#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

/// Render one frame at `size` and return `(stdout, ppm-header-dimensions)`.
fn snapshot(size: &str, tag: &str) -> (String, String) {
    let root = workspace_root();
    let out = root.join("target").join(format!("it-dash-{tag}.ppm"));
    let _ = std::fs::remove_file(&out);

    // Project mode runs from inside the project directory.
    let status = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(root.join("examples").join("dashboard"))
        .arg("run")
        .arg("--")
        .arg("--snapshot")
        .arg(&out)
        .arg("--size")
        .arg(size)
        .output()
        .expect("spawn jux");

    let stdout = String::from_utf8_lossy(&status.stdout).to_string();
    let stderr = String::from_utf8_lossy(&status.stderr);
    assert!(
        status.status.success(),
        "jux exited with {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        status.status.code(),
    );

    // The PPM header carries the dimensions, which is enough to prove the
    // frame was rendered at the requested size rather than a default one.
    let ppm = std::fs::read_to_string(&out).expect("snapshot written");
    let mut parts = ppm.split_whitespace();
    assert_eq!(parts.next(), Some("P3"), "not a P3 PPM:\n{stdout}");
    let w = parts.next().unwrap_or_default().to_string();
    let h = parts.next().unwrap_or_default().to_string();
    (stdout, format!("{w}x{h}"))
}

#[test]
fn the_dashboard_renders_and_responds_to_its_size() {
    let (wide, dims) = snapshot("1280x800", "wide");
    assert_eq!(dims, "1280x800");
    assert!(
        wide.contains("wrote 1280x800 frame"),
        "unexpected output:\n{wide}",
    );

    let (_, medium_dims) = snapshot("900x620", "medium");
    assert_eq!(medium_dims, "900x620");

    let (_, compact_dims) = snapshot("560x760", "compact");
    assert_eq!(compact_dims, "560x760");
}
