//! The editor's source roots (`jux.sourceRoots`, JUX-INTELLIJ-PLUGIN-ADDENDUM
//! §I.4, "Coordination with `juxc-lsp`").
//!
//! The IntelliJ plugin knows which directories are Jux package roots: the ones
//! the user marked, and each `jux.toml` project's `src/` and `test/`. It sends
//! the list three ways, all carrying the same value, a list of
//! `{ "path": <absolute>, "kind": "sources" | "tests", "origin": "marked" | "manifest" }`:
//!
//! - the initialization option `{ "jux": { "sourceRoots": [...] } }`;
//! - `workspace/didChangeConfiguration` with the same object whenever roots
//!   are marked or unmarked;
//! - the answer to a `workspace/configuration` pull for section
//!   `jux.sourceRoots`, which is the list itself.
//!
//! The server uses the roots for two things:
//!
//! - **Package inference.** A file's package is its directory relative to the
//!   nearest root, dotted. Completion reads it for a file that declares no
//!   `package` line yet, so same-package types are not offered an import.
//! - **The compilation set.** The files analysed together with an open file.
//!   Roots are grouped by the `jux.toml` that governs them; a file under a root
//!   is analysed with every `.jux` file under the roots of its own group. That
//!   gives an IntelliJ project with marked roots and no manifest cross-file
//!   analysis, which it otherwise lacks, and keeps a manifest project's
//!   `examples/` or scratch files out of its `src/` analysis.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::workspace::scan_jux_files;

/// What a root holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    /// Production sources.
    Sources,
    /// Test sources: same package rules, separate bucket for the build.
    Tests,
}

/// Where the editor learned about a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootOrigin {
    /// The user marked the directory.
    Marked,
    /// Implied by a `jux.toml` project layout.
    Manifest,
}

/// One package root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRoot {
    /// Absolute directory.
    pub path: PathBuf,
    /// Production or test sources.
    pub kind: RootKind,
    /// Marked by the user or implied by a manifest.
    pub origin: RootOrigin,
}

/// The editor's current root set. Empty until the client sends one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRoots {
    roots: Vec<SourceRoot>,
}

impl SourceRoots {
    /// Roots from an explicit list.
    #[cfg(test)]
    pub fn new(roots: Vec<SourceRoot>) -> Self {
        SourceRoots { roots }
    }

    /// The roots, in the order the client listed them.
    #[cfg(test)]
    pub fn roots(&self) -> &[SourceRoot] {
        &self.roots
    }

    /// True when the client has sent no roots.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Read roots from any of the three shapes the client sends: the settings
    /// object (`{ "jux": { "sourceRoots": [...] } }`), its inner section
    /// (`{ "sourceRoots": [...] }`), or the bare list a pull returns. `None`
    /// when the value carries no root list at all (so a settings change about
    /// something else leaves the current roots alone); `Some(empty)` for an
    /// explicitly empty list. Malformed entries are skipped, not fatal.
    pub fn from_settings(value: &Value) -> Option<Self> {
        let list = match value {
            Value::Array(_) => value,
            Value::Object(o) => o
                .get("jux")
                .and_then(|j| j.get("sourceRoots"))
                .or_else(|| o.get("sourceRoots"))
                .or_else(|| o.get("jux.sourceRoots"))?,
            _ => return None,
        };
        let items = list.as_array()?;
        let roots = items
            .iter()
            .filter_map(|item| {
                let path = PathBuf::from(item.get("path")?.as_str()?);
                let kind = match item.get("kind").and_then(Value::as_str) {
                    Some("tests") => RootKind::Tests,
                    _ => RootKind::Sources,
                };
                let origin = match item.get("origin").and_then(Value::as_str) {
                    Some("manifest") => RootOrigin::Manifest,
                    _ => RootOrigin::Marked,
                };
                Some(SourceRoot { path, kind, origin })
            })
            .collect();
        Some(SourceRoots { roots })
    }

    /// The deepest root containing `file`, if any.
    pub fn root_of(&self, file: &Path) -> Option<&SourceRoot> {
        self.roots
            .iter()
            .filter(|r| file.starts_with(&r.path))
            .max_by_key(|r| r.path.components().count())
    }

    /// The package `file`'s location implies: its directory relative to its
    /// root, dotted. `Some("")` for a file directly in a root (no package);
    /// `None` for a file under no root.
    pub fn package_of(&self, file: &Path) -> Option<String> {
        let root = self.root_of(file)?;
        let dir = file.parent()?;
        let rel = dir.strip_prefix(&root.path).ok()?;
        let segments: Vec<String> = rel
            .components()
            .filter_map(|c| c.as_os_str().to_str().map(str::to_string))
            .collect();
        Some(segments.join("."))
    }

    /// The `.jux` files to analyse together with `file`: every file under the
    /// roots that share `file`'s root's governing `jux.toml` (or, for roots no
    /// manifest governs, every such root). `None` when `file` is under no
    /// root, which leaves the manifest-based default in charge.
    pub fn compilation_set(&self, file: &Path) -> Option<Vec<PathBuf>> {
        let home = self.root_of(file)?;
        let group = governing_manifest(&home.path);
        let mut files: Vec<PathBuf> = Vec::new();
        for root in &self.roots {
            if governing_manifest(&root.path) != group {
                continue;
            }
            for f in scan_jux_files(&root.path) {
                if !files.contains(&f) {
                    files.push(f);
                }
            }
        }
        Some(files)
    }
}

/// The directory of the nearest `jux.toml` at or above `dir`, if any: the
/// project a root belongs to.
fn governing_manifest(dir: &Path) -> Option<PathBuf> {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.join("jux.toml").is_file() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("juxc_lsp_roots_{}_{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// All three shapes the plugin sends carry the same list.
    #[test]
    fn every_client_shape_parses() {
        let list = json!([
            { "path": "/p/src", "kind": "sources", "origin": "manifest" },
            { "path": "/p/test", "kind": "tests", "origin": "manifest" },
        ]);
        let settings = json!({ "jux": { "sourceRoots": list.clone() } });
        let section = json!({ "sourceRoots": list.clone() });
        let from_settings = SourceRoots::from_settings(&settings).unwrap();
        assert_eq!(from_settings.roots().len(), 2);
        assert_eq!(from_settings.roots()[1].kind, RootKind::Tests);
        assert_eq!(from_settings.roots()[0].origin, RootOrigin::Manifest);
        assert_eq!(SourceRoots::from_settings(&section), Some(from_settings.clone()));
        assert_eq!(SourceRoots::from_settings(&list), Some(from_settings));
    }

    /// A settings change about something else must not wipe the roots, and a
    /// malformed entry is skipped rather than failing the whole list.
    #[test]
    fn unrelated_settings_and_bad_entries() {
        assert_eq!(SourceRoots::from_settings(&json!({ "editor": {} })), None);
        assert_eq!(SourceRoots::from_settings(&Value::Null), None);
        let roots = SourceRoots::from_settings(&json!([{ "kind": "sources" }, { "path": "/a" }])).unwrap();
        assert_eq!(roots.roots().len(), 1);
        assert_eq!(roots.roots()[0].origin, RootOrigin::Marked, "a missing origin is a marked root");
    }

    /// Package inference: the directory relative to the deepest root, dotted.
    #[test]
    fn package_follows_the_directory_under_the_deepest_root() {
        let roots = SourceRoots::new(vec![
            SourceRoot { path: PathBuf::from("/p/src"), kind: RootKind::Sources, origin: RootOrigin::Marked },
            SourceRoot { path: PathBuf::from("/p/src/gen"), kind: RootKind::Sources, origin: RootOrigin::Marked },
        ]);
        assert_eq!(roots.package_of(Path::new("/p/src/com/acme/Foo.jux")).as_deref(), Some("com.acme"));
        assert_eq!(roots.package_of(Path::new("/p/src/Main.jux")).as_deref(), Some(""));
        assert_eq!(roots.package_of(Path::new("/p/src/gen/x/Y.jux")).as_deref(), Some("x"));
        assert_eq!(roots.package_of(Path::new("/elsewhere/Z.jux")), None);
    }

    /// Roots under one manifest form one compilation set; a file outside them
    /// (an `examples/` program in the same project) stays out of it.
    #[test]
    fn compilation_set_is_the_roots_of_one_project() {
        let p = temp("set");
        fs::write(p.join("jux.toml"), "[package]\nname = \"p\"\nversion = \"0.1.0\"\n").unwrap();
        fs::create_dir_all(p.join("src/app")).unwrap();
        fs::create_dir_all(p.join("test")).unwrap();
        fs::create_dir_all(p.join("examples")).unwrap();
        fs::write(p.join("src/app/A.jux"), "package app;").unwrap();
        fs::write(p.join("test/T.jux"), "").unwrap();
        fs::write(p.join("examples/E.jux"), "").unwrap();
        let roots = SourceRoots::new(vec![
            SourceRoot { path: p.join("src"), kind: RootKind::Sources, origin: RootOrigin::Manifest },
            SourceRoot { path: p.join("test"), kind: RootKind::Tests, origin: RootOrigin::Manifest },
        ]);
        let set = roots.compilation_set(&p.join("src/app/A.jux")).unwrap();
        assert!(set.contains(&p.join("src/app/A.jux")));
        assert!(set.contains(&p.join("test/T.jux")));
        assert!(!set.contains(&p.join("examples/E.jux")), "{set:?}");
        assert_eq!(roots.compilation_set(&p.join("examples/E.jux")), None);
        let _ = fs::remove_dir_all(&p);
    }

    /// Marked roots with no manifest anywhere still form one set together,
    /// which is what gives such a project cross-file analysis at all.
    #[test]
    fn marked_roots_without_a_manifest_share_a_set() {
        let p = temp("marked");
        fs::create_dir_all(p.join("a")).unwrap();
        fs::create_dir_all(p.join("b")).unwrap();
        fs::write(p.join("a/X.jux"), "").unwrap();
        fs::write(p.join("b/Y.jux"), "").unwrap();
        let roots = SourceRoots::new(vec![
            SourceRoot { path: p.join("a"), kind: RootKind::Sources, origin: RootOrigin::Marked },
            SourceRoot { path: p.join("b"), kind: RootKind::Sources, origin: RootOrigin::Marked },
        ]);
        let set = roots.compilation_set(&p.join("a/X.jux")).unwrap();
        assert_eq!(set.len(), 2, "{set:?}");
        let _ = fs::remove_dir_all(&p);
    }
}
