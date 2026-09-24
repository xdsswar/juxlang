//! Workspace indexing — scan every `.jux` file in the project so completion
//! knows about types/functions declared in *other* files and modules.
//!
//! The single-document analysis (`analysis.rs`) only sees the open buffer plus
//! the stdlib. This module analyses the whole source tree and collects the
//! in-scope names, which the server merges into completion. Non-`.jux` files
//! (resources) and build output (`target/`, hidden dirs) are skipped, matching
//! the compiler's directory walk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use juxc_source::SourceFile;
use juxc_tycheck::SymbolTable;

/// Cached workspace index state held by the server.
#[derive(Default)]
pub struct Workspace {
    /// Project root (from the LSP `rootUri` / first workspace folder).
    pub root: Option<PathBuf>,
    /// Bare names of every declared type (class / interface / enum / record /
    /// struct) across all project modules.
    pub type_names: Vec<String>,
    /// Bare names of every callable/member: free functions, methods, fields,
    /// enum variants, and record components.
    pub member_names: Vec<String>,
    /// Bare type name → declaring **package** (the FQN minus its last segment),
    /// powering auto-import. A bare name with multiple declaring packages keeps
    /// every candidate so the code action can offer each `import` choice.
    /// No-package (bare-FQN) types don't appear — there's nothing to import.
    pub type_packages: HashMap<String, Vec<String>>,
    /// The merged symbol table of the last whole-project pass, for
    /// `workspace/symbol` when no document is open to ask. `None` until the
    /// first index.
    pub symbols: Option<Arc<SymbolTable>>,
    /// Paths parallel to [`Self::symbols`]' unit indices.
    pub source_paths: Arc<Vec<PathBuf>>,
    /// Project file texts parallel to [`Self::source_paths`] (`None` for the
    /// standard library and generated stubs).
    pub source_texts: Arc<Vec<Option<Arc<str>>>>,
}

/// The result of one workspace scan.
#[derive(Default)]
pub struct WorkspaceIndex {
    pub type_names: Vec<String>,
    pub member_names: Vec<String>,
    /// Bare type name → declaring package(s). See [`Workspace::type_packages`].
    pub type_packages: HashMap<String, Vec<String>>,
    /// The merged symbol table the names came from. See [`Workspace::symbols`].
    pub symbols: Option<Arc<SymbolTable>>,
    /// See [`Workspace::source_paths`].
    pub source_paths: Arc<Vec<PathBuf>>,
    /// See [`Workspace::source_texts`].
    pub source_texts: Arc<Vec<Option<Arc<str>>>>,
}

/// Recursively collect `.jux` files under `root`, skipping build output and
/// hidden directories. Resource files (non-`.jux`) are ignored.
pub fn scan_jux_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            // The `.jux-stubs/` cache (JUX-BINDGEN §G.11.2) holds the project's
            // generated `.jux.d` stubs — it's hidden by convention but MUST be
            // scanned so Rust-derived types/methods surface in completion.
            if name == crate::stubs_dirname() {
                walk(&path, out);
                continue;
            }
            // Skip build output, dependency caches, and other hidden dirs. A
            // `resources` folder is fine — it simply contains no `.jux` files.
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            walk(&path, out);
        } else if is_jux_source(&path) {
            out.push(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Project boundaries (shared by `analysis.rs` and `roots.rs`)
// ---------------------------------------------------------------------------

/// The directory of the nearest `jux.toml` at or above `dir`, if any: the
/// package a file or a source root belongs to. The walk stops at `stop_at`
/// (the folder the editor was opened on) so a stray manifest further up the
/// filesystem cannot pull in an unrelated tree.
pub fn nearest_manifest(dir: &Path, stop_at: Option<&Path>) -> Option<PathBuf> {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.join("jux.toml").is_file() {
            return Some(d.to_path_buf());
        }
        if Some(d) == stop_at {
            return None;
        }
        cur = d.parent();
    }
    None
}

/// Widen `package` to the `[workspace]` root that governs it, when there is
/// one, so a member is analysed alongside the sibling packages it depends on.
/// Returns `package` unchanged when no ancestor declares `[workspace] members`.
///
/// Both callers that decide a compilation set need this, and they used to spell
/// it separately: `analysis::project_scope` widened, `roots::governing_manifest`
/// did not. A workspace member opened with the editor's source roots in play was
/// therefore checked without its siblings, and every cross-package import went
/// red once indexing finished and the roots arrived. One helper, one rule.
pub fn widen_to_workspace(package: &Path, stop_at: Option<&Path>) -> PathBuf {
    let mut ancestor = package.parent();
    while let Some(a) = ancestor {
        let declares_workspace =
            juxc_driver::Manifest::load(a).is_some_and(|m| !m.workspace_members.is_empty());
        if declares_workspace {
            return a.to_path_buf();
        }
        if Some(a) == stop_at {
            break;
        }
        ancestor = a.parent();
    }
    package.to_path_buf()
}

/// The generated `.jux.d` stubs of the package rooted at `package_root`, i.e.
/// everything under its `.jux-stubs/` cache (JUX-BINDGEN §G.11.2). Empty when
/// the project has no bound Rust crates, or has never been built or opened by
/// a language server that could generate them.
///
/// These files are what make `import rust.<crate>.<Type>;` resolve. They sit
/// BESIDE `src/`, not inside it, which is why every code path that assembles a
/// compilation set has to ask for them explicitly.
pub fn project_stub_files(package_root: &Path) -> Vec<PathBuf> {
    scan_jux_files(&package_root.join(crate::stubs_dirname()))
}

/// True for files the workspace analysis should feed to `check_workspace`:
/// ordinary `.jux` sources AND `.jux.d` declaration stubs (JUX-BINDGEN-ADDENDUM
/// §G). A stub's `file_name` ends in `.jux.d` (so `Path::extension` is `d`),
/// which is why we match on the file-name suffix rather than the extension —
/// pulling stubs into the index is what makes Rust-derived types/methods
/// surface in completion/hover in Jux syntax (§G.10).
fn is_jux_source(path: &Path) -> bool {
    match path.file_name().and_then(|s| s.to_str()) {
        Some(name) => name.ends_with(".jux.d") || name.ends_with(".jux"),
        None => false,
    }
}

/// Analyse every project file and return the bare names of all types and
/// members (classes, interfaces, enums, records, functions, methods, fields,
/// variants). `overrides` supplies the *live* editor text for open buffers so
/// the index reflects unsaved edits.
pub fn index_workspace(root: &Path, overrides: &HashMap<PathBuf, String>) -> WorkspaceIndex {
    let mut sources = Vec::new();
    for path in scan_jux_files(root) {
        let text = overrides
            .get(&path)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())
            .unwrap_or_default();
        sources.push(SourceFile::new(path, text));
    }
    // Dependency sources (§B.2.2): the src trees of `path` deps plus
    // any ALREADY-CACHED `git` deps, so completion/goto resolve types
    // declared in dependencies. The LSP never touches the network —
    // `jux build` / `jux update` populate the git cache; until then a
    // git dep simply doesn't contribute names.
    if let Some(manifest) = juxc_driver::Manifest::load(root) {
        let mut dep_roots: Vec<PathBuf> = Vec::new();
        for dep in &manifest.dependencies {
            if let Some(p) = &dep.path {
                dep_roots.push(p.clone());
            } else if let Some(url) = &dep.git {
                if let Ok(dir) =
                    juxc_driver::git_deps::git_dep_cache_dir(url, dep.git_ref.as_ref())
                {
                    if dir.join("jux.toml").is_file() {
                        dep_roots.push(dir);
                    }
                }
            }
        }
        for dep_root in dep_roots {
            for path in scan_jux_files(&dep_root.join("src")) {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                sources.push(SourceFile::new(path, text));
            }
        }
    }
    if sources.is_empty() {
        return WorkspaceIndex::default();
    }
    // `check_workspace` merges every unit (plus the auto-loaded stdlib) into
    // one symbol table — exactly the cross-module view completion needs.
    let result = juxc_driver::check_workspace(sources);
    let mut index = collect_index(&result.symbols);
    index.source_paths = Arc::new(result.sources.iter().map(|s| s.path().to_path_buf()).collect());
    index.source_texts = Arc::new(
        result
            .sources
            .iter()
            .map(|s| {
                let project = s.path().extension().is_some_and(|e| e == "jux") && s.path().is_absolute();
                project.then(|| Arc::from(s.contents()))
            })
            .collect(),
    );
    index.symbols = Some(Arc::new(result.symbols));
    index
}

/// Collect type names and member (function/method/field/variant) names from a
/// merged symbol table. Names are bare (last path segment) and deduplicated.
fn collect_index(symbols: &SymbolTable) -> WorkspaceIndex {
    let mut types: Vec<String> = Vec::new();
    let mut members: Vec<String> = Vec::new();
    let mut type_packages: HashMap<String, Vec<String>> = HashMap::new();

    let bare = |fqn: &str| fqn.rsplit('.').next().unwrap_or(fqn).to_string();
    let push = |v: &mut Vec<String>, name: String| {
        if !v.contains(&name) {
            v.push(name);
        }
    };
    // Record the declaring package for a type FQN. `a.b.C` → bare `C` maps to
    // package `a.b`; a no-package bare FQN (`C`) contributes nothing (nothing
    // to import). Multiple distinct packages for the same bare name are all
    // kept so the auto-import action can offer each choice.
    let record_pkg = |fqn: &str, type_packages: &mut HashMap<String, Vec<String>>| {
        if let Some((pkg, name)) = fqn.rsplit_once('.') {
            // Stdlib (`jux.std.*`) is auto-imported implicitly — never offer an
            // explicit `import` for it (matches Java's `java.lang.*` rule).
            if pkg == "jux.std" || pkg.starts_with("jux.std.") {
                return;
            }
            let entry = type_packages.entry(name.to_string()).or_default();
            if !entry.iter().any(|p| p == pkg) {
                entry.push(pkg.to_string());
            }
        }
    };

    // Types.
    for k in symbols.classes.keys() {
        push(&mut types, bare(k));
        record_pkg(k, &mut type_packages);
    }
    for k in symbols.records.keys() {
        push(&mut types, bare(k));
        record_pkg(k, &mut type_packages);
    }
    for k in symbols.enums.keys() {
        push(&mut types, bare(k));
        record_pkg(k, &mut type_packages);
    }
    for k in symbols.interfaces.keys() {
        push(&mut types, bare(k));
        record_pkg(k, &mut type_packages);
    }

    // Free functions.
    for k in symbols.functions.keys() {
        push(&mut members, bare(k));
    }
    // Class members: methods + fields.
    for sig in symbols.classes.values() {
        for m in sig.methods.keys() {
            push(&mut members, m.clone());
        }
        for f in sig.fields.keys() {
            push(&mut members, f.clone());
        }
    }
    // Interface members.
    for sig in symbols.interfaces.values() {
        for m in sig.methods.keys() {
            push(&mut members, m.clone());
        }
        for f in sig.fields.keys() {
            push(&mut members, f.clone());
        }
    }
    // Record methods + enum variants.
    for sig in symbols.records.values() {
        for m in sig.methods.keys() {
            push(&mut members, m.clone());
        }
    }
    for sig in symbols.enums.values() {
        for v in sig.variants.keys() {
            push(&mut members, v.clone());
        }
    }

    types.sort();
    members.sort();
    for pkgs in type_packages.values_mut() {
        pkgs.sort();
    }
    WorkspaceIndex { type_names: types, member_names: members, type_packages, ..Default::default() }
}
