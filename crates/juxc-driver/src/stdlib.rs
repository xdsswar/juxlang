//! Standard-library auto-loader.
//!
//! The `jux.std` package tree (collections, exceptions, async, io,
//! …) ships as Jux source files next to the compiler binary. Every
//! compilation prepends those files to the user's workspace so
//! types like `Map<K, V>`, `Throwable`, and `ArrayList<T>` are
//! always in scope — same shape as Java's implicit `java.lang.*`
//! visibility.
//!
//! ## Location resolution
//!
//! In order:
//!
//! 1. `$JUX_STD_DIR` environment variable. Highest priority — lets
//!    test harnesses point at a specific tree without juggling
//!    cwd.
//! 2. `<exe-dir>/jux.std/`. Production install location: the
//!    stdlib ships next to the `juxc.exe` / `jux.exe` binaries.
//! 3. `<exe-dir>/../jux.std/` — common shape when binaries live
//!    in `target/release/` next to the repo root that holds
//!    `jux.std/`.
//! 4. `./jux.std/` relative to the current working directory.
//!    Convenient for dev builds run from the repo root.
//!
//! When none of these resolve, the loader returns an empty source
//! list and the compiler proceeds without stdlib — callers see
//! `E0301` for any `Map<...>` / `Throwable` / etc. reference. A
//! future check could surface a clearer "missing stdlib"
//! diagnostic.

use std::path::{Path, PathBuf};

use juxc_source::SourceFile;

/// Locate the `jux.std/` directory using the resolution rules
/// described in the module-level docs. Returns `None` when none
/// of the candidate paths exist.
pub(crate) fn locate_std_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("JUX_STD_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let sibling = exe_dir.join("jux.std");
            if sibling.is_dir() {
                return Some(sibling);
            }
            // Common dev shape: binary in `target/release/`, stdlib
            // at the repo root. Walk parents looking for a
            // `jux.std/` so `cargo run`-style invocations Just Work.
            let mut cursor = exe_dir.parent();
            while let Some(dir) = cursor {
                let candidate = dir.join("jux.std");
                if candidate.is_dir() {
                    return Some(candidate);
                }
                cursor = dir.parent();
            }
        }
    }
    let cwd = PathBuf::from("jux.std");
    if cwd.is_dir() {
        return Some(cwd);
    }
    None
}

/// Read every `.jux` file under `std_dir` recursively and return
/// them as `SourceFile`s in path-lexicographic order so emission
/// stays deterministic.
pub(crate) fn load_std_sources() -> Vec<SourceFile> {
    // A directory override (`$JUX_STD_DIR`, or a `jux.std/` next to the binary /
    // in the cwd) wins when present — it lets a developer iterate on the stdlib
    // declarations without rebuilding the compiler. The on-disk folder is no
    // longer shipped, so in normal use this resolves to nothing and we fall
    // through to the embedded copy below.
    if let Some(std_dir) = locate_std_dir() {
        let mut paths: Vec<PathBuf> = Vec::new();
        collect_jux_files(&std_dir, &mut paths);
        paths.sort();
        let sources: Vec<SourceFile> = paths
            .into_iter()
            .filter_map(|p| std::fs::read_to_string(&p).ok().map(|c| SourceFile::new(p, c)))
            .collect();
        if !sources.is_empty() {
            return sources;
        }
    }
    // Embedded stdlib (the former `jux.std/` tree, inlined into the binary so the
    // compiler is self-contained — see `stdlib_embedded`). Synthetic
    // `jux.std/<path>` paths keep source markers / diagnostics reading the same.
    crate::stdlib_embedded::STDLIB_SOURCES
        .iter()
        .map(|(rel, src)| SourceFile::new(PathBuf::from("jux.std").join(rel), (*src).to_string()))
        .collect()
}

/// Walk `dir` recursively, appending every `.jux` file's path to
/// `out`. Hidden directories (starting with `.`) are skipped so a
/// `.git/` inside the stdlib tree doesn't trip the loader.
fn collect_jux_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }
            collect_jux_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jux") {
            out.push(path);
        }
    }
}
