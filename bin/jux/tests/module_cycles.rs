//! A dependency cycle between modules (`jux.toml` packages) is `E0308`,
//! reported with the whole circle (JUX-BUILD-SYSTEM-ADDENDUM §B.4.6,
//! ERRATA E41).
//!
//! Two shapes reach it: two standalone projects that name each other as
//! `path` dependencies, and two members of one workspace. The standalone
//! shape used to load the root's own sources back in as a dependency and
//! report the first duplicate (`main is declared more than once`), which
//! says nothing about the cycle.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The freshly-built `jux` binary, courtesy of cargo's test harness.
fn jux_binary() -> &'static str {
    env!("CARGO_BIN_EXE_jux")
}

/// Two `..`s up from `bin/jux/` is the workspace root.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

/// Write module `name` at `dir`: an app (`main`) or a library, depending
/// on `other` by path.
fn write_module(dir: &Path, name: &str, other: &str, is_lib: bool) {
    std::fs::create_dir_all(dir.join("src")).expect("creating module dir");
    let lib = if is_lib { "\n[lib]\n" } else { "" };
    std::fs::write(
        dir.join("jux.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n{lib}\n[dependencies]\n\"{other}\" = {{ path = \"../{}\" }}\n",
            other.rsplit('.').next().unwrap_or(other),
        ),
    )
    .expect("writing jux.toml");
    let source = if is_lib {
        ("lib.jux", "public int one() { return 1; }\n")
    } else {
        ("main.jux", "public void main() { print(\"a\"); }\n")
    };
    std::fs::write(dir.join("src").join(source.0), source.1).expect("writing source");
}

/// Run `jux build` in `dir`; return (success, stderr).
fn build_in(dir: &Path) -> (bool, String) {
    let output = Command::new(jux_binary())
        .arg("build")
        .current_dir(dir)
        .output()
        .expect("running jux build");
    (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
}

/// The error names E0308 and the circle, and nothing about duplicates.
fn assert_cycle_error(stderr: &str) {
    assert!(stderr.contains("[E0308]"), "expected E0308, got:\n{stderr}");
    assert!(
        stderr.contains("`cyc.a` -> `cyc.b` -> `cyc.a`"),
        "expected the whole circle, got:\n{stderr}",
    );
    assert!(!stderr.contains("more than once"), "a duplicate report leaked:\n{stderr}");
}

#[test]
fn two_standalone_modules_depending_on_each_other_are_e0308() {
    let base = workspace_root().join("target").join("it-module-cycle-standalone");
    let _ = std::fs::remove_dir_all(&base);
    write_module(&base.join("a"), "cyc.a", "cyc.b", false);
    write_module(&base.join("b"), "cyc.b", "cyc.a", true);
    let (ok, stderr) = build_in(&base.join("a"));
    assert!(!ok, "a module cycle must not build");
    assert_cycle_error(&stderr);
}

#[test]
fn two_workspace_members_depending_on_each_other_are_e0308() {
    let base = workspace_root().join("target").join("it-module-cycle-workspace");
    let _ = std::fs::remove_dir_all(&base);
    write_module(&base.join("a"), "cyc.a", "cyc.b", false);
    write_module(&base.join("b"), "cyc.b", "cyc.a", true);
    std::fs::write(base.join("jux.toml"), "[workspace]\nmembers = [\"a\", \"b\"]\n").expect("writing workspace");
    let (ok, stderr) = build_in(&base);
    assert!(!ok, "a module cycle must not build");
    assert_cycle_error(&stderr);
}
