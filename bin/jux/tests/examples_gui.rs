//! The three `minifb` window examples: `desktop_window`, `gui_workspace`,
//! `paint_app`.
//!
//! They are BUILT, not run: each opens a real window and waits for the user to
//! close it, so running one in a test would hang. Building is what has value
//! anyway — every regression these have ever had was a compile error at the
//! crate boundary, not a runtime one.
//!
//! Gated to Windows because building `minifb` needs X11 development headers on
//! Linux, which a runner may not have. The Jux side is portable.

#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

fn build(project: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    // Project mode builds from inside the project directory.
    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(root.join("examples").join(project))
        .arg("build")
        .output()
        .unwrap_or_else(|e| panic!("spawning jux build for {project}: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{project} failed to build with {:?}\n{stdout}{stderr}",
        output.status.code(),
    );
}

#[test]
fn desktop_window_builds() {
    build("desktop_window");
}

#[test]
fn gui_workspace_builds() {
    build("gui_workspace");
}

#[test]
fn paint_app_builds() {
    build("paint_app");
}
