//! Release blockers that need a whole project to show (GAPS.md gap 35).
//!
//! The single-file ones are pinned by the corpus (`examples/release_blockers.jux`,
//! `examples/no_entry_point.jux`) and by `tests/ui`; these three need packages:
//!
//! 1. A class in the root `main.jux` extending a class of a named package. The
//!    base's package named the subclass without its crate-root path and the
//!    build failed with an internal compiler error reported against the BASE
//!    class's file.
//! 2. `new app.model.Secret()` on a package-private type from another package.
//!    `import app.model.Secret;` was `E0416`; the fully-qualified spelling
//!    compiled and ran.
//! 3. The `jux:` lines the tool prints itself while building a project. They
//!    never passed the leak detector (ERRATA E129's "not guarded"); here every
//!    one is held to it, with nothing excused.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Minimal std-only temp dir, removed on drop (the shape `multi_bin_project.rs`
/// uses).
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let id = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("jux-gap35-{tag}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n")
}

/// `jux <args>` in `dir`: combined output, CRLF folded, and the exit status.
fn jux(dir: &Path, args: &[&str]) -> (String, Option<i32>) {
    let out = Command::new(env!("CARGO_BIN_EXE_jux"))
        .args(args)
        .current_dir(dir)
        .env("JUX_SELFCHECK", "1")
        .output()
        .expect("spawn jux");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
        .replace("\r\n", "\n");
    (text, out.status.code())
}

#[test]
fn a_root_class_extends_a_packaged_class_and_the_jux_lines_show_no_rust() {
    let dir = TempDir::new("root-subclass");
    let root = dir.path();
    write(root, "jux.toml", &manifest("probe.subclass"));
    write(
        root,
        "src/lib/Gadget.jux",
        "package lib;\n\
         public class Gadget {\n\
         \x20   public String name;\n\
         \x20   public Gadget(String n) { name = n; }\n\
         \x20   public String describe() { return \"gadget \" + name; }\n\
         }\n",
    );
    write(
        root,
        "src/main.jux",
        "import lib.Gadget;\n\
         class Phone extends Gadget {\n\
         \x20   Phone() { super(\"phone\"); }\n\
         \x20   @Override public String describe() { return \"a phone called \" + name; }\n\
         }\n\
         public void main() {\n\
         \x20   Gadget g = new Phone();\n\
         \x20   print(g.describe());\n\
         }\n",
    );
    let (text, code) = jux(root, &["run"]);
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("a phone called phone"), "{text}");
    for line in text.lines().filter(|l| l.starts_with("jux:")) {
        assert!(
            juxc_diagnostics::leak::find_rust_leak(line).is_none(),
            "a `jux:` line shows Rust: {:?}\n{line}",
            juxc_diagnostics::leak::find_rust_leak(line)
        );
        assert!(!line.contains("crate"), "a `jux:` line names a crate: {line}");
    }
}

#[test]
fn a_fully_qualified_package_private_type_is_e0416() {
    let dir = TempDir::new("private-fqn");
    let root = dir.path();
    write(root, "jux.toml", &manifest("probe.private"));
    write(root, "src/app/model/Secret.jux", "package app.model;\nclass Secret { }\n");
    write(root, "src/main.jux", "public void main() {\n    var s = new app.model.Secret();\n    print(\"made\");\n}\n");
    let (text, code) = jux(root, &["check"]);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("[E0416] error: cannot use package-private type `app.model.Secret` from the root package"),
        "{text}"
    );
    assert!(!text.contains("made"), "{text}");
}

/// `jux check` reports `E0327` whenever the build would (sweep C): an empty
/// loose file, and a package's `[[bin]]` entry with no `main`. It used to say
/// `check ok` to both and leave the error to `jux build`.
#[test]
fn jux_check_reports_a_binary_with_no_entry_point() {
    let dir = TempDir::new("check-e0327");
    let root = dir.path();
    write(root, "empty.jux", "");
    let (text, code) = jux(root, &["check", "empty.jux"]);
    assert_eq!(code, Some(1), "{text}");
    assert!(text.contains("empty.jux:1:1: [E0327] error: this program has no entry point"), "{text}");
    assert!(!text.contains("check ok"), "{text}");

    // Top-level statements are an entry point.
    write(root, "script.jux", "print(\"hi\");\n");
    let (text, code) = jux(root, &["check", "script.jux"]);
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("check ok"), "{text}");

    // A project whose binary's entry declares nothing to run.
    let project = root.join("app");
    write(&project, "jux.toml", &manifest("probe.noentry"));
    write(&project, "src/main.jux", "class Unused {\n    public int n = 0;\n}\n");
    let (text, code) = jux(&project, &["check"]);
    assert_eq!(code, Some(1), "{text}");
    assert!(text.contains("[E0327] error: this program has no entry point"), "{text}");
    assert!(text.contains("main.jux:1:1"), "reported on the entry file: {text}");
}

/// A library member needs no entry point: `jux check` of a lib-only package
/// whose files declare no `main` stays clean.
#[test]
fn jux_check_of_a_library_member_stays_clean() {
    let dir = TempDir::new("check-lib");
    let root = dir.path();
    write(
        root,
        "jux.toml",
        "[package]\nname = \"probe.shapes\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[lib]\nname = \"shapes\"\n",
    );
    write(root, "src/shapes/Shape.jux", "package shapes;\n\npublic class Shape {\n    public int sides = 3;\n}\n");
    let (text, code) = jux(root, &["check"]);
    assert_eq!(code, Some(0), "{text}");
    assert!(!text.contains("E0327"), "{text}");
}
