//! `[[bin]] main = "xss.it.Main"` (§B.2.2) — the entry point named by its
//! dotted source path. Scaffolds a temp project whose entry lives at
//! `src/xss/it/Main.jux` (package `xss.it`), builds + runs it, and asserts the
//! named entry actually executed.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Minimal std-only temp dir, removed on drop.
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let id = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "jux-bin-main-entry-{}-{}",
            std::process::id(),
            id,
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bin_main_dotted_entry_builds_and_runs() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let dir = TempDir::new();
    let root = dir.path();
    let entry_dir = root.join("src").join("xss").join("it");
    std::fs::create_dir_all(&entry_dir).unwrap();
    std::fs::write(
        root.join("jux.toml"),
        "[package]\nname = \"com.example.entry\"\n\n\
         [[bin]]\nname = \"EntryApp\"\nmain = \"xss.it.Main\"\n",
    )
    .unwrap();
    std::fs::write(
        entry_dir.join("Main.jux"),
        "package xss.it;\npublic void main() { print(\"entry-ok\"); }\n",
    )
    .unwrap();

    let output = Command::new(jux)
        .arg("run")
        .current_dir(root)
        .output()
        .expect("spawn jux run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let all = format!("{stdout}{stderr}");
    assert!(output.status.success(), "build/run failed:\n{all}");
    assert!(all.contains("entry-ok"), "named entry did not run:\n{all}");
}

#[test]
fn bin_main_missing_entry_file_is_a_clean_error() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let dir = TempDir::new();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    // Manifest points main at a file that doesn't exist.
    std::fs::write(
        root.join("jux.toml"),
        "[package]\nname = \"com.example.entry\"\n\n\
         [[bin]]\nname = \"EntryApp\"\nmain = \"does.not.Exist\"\n",
    )
    .unwrap();

    let output = Command::new(jux)
        .arg("build")
        .current_dir(root)
        .output()
        .expect("spawn jux build");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.status.success(), "missing entry unexpectedly built:\n{all}");
    assert!(
        all.contains("does not exist") && all.contains("does/not/Exist.jux") || all.contains("does\\not\\Exist.jux"),
        "expected a clean missing-entry error, got:\n{all}",
    );
}
