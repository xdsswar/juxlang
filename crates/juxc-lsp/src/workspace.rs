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
}

/// The result of one workspace scan.
#[derive(Default)]
pub struct WorkspaceIndex {
    pub type_names: Vec<String>,
    pub member_names: Vec<String>,
    /// Bare type name → declaring package(s). See [`Workspace::type_packages`].
    pub type_packages: HashMap<String, Vec<String>>,
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
            // Skip build output, dependency caches, and hidden dirs. A
            // `resources` folder is fine — it simply contains no `.jux` files.
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            walk(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("jux") {
            out.push(path);
        }
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
    if sources.is_empty() {
        return WorkspaceIndex::default();
    }
    // `check_workspace` merges every unit (plus the auto-loaded stdlib) into
    // one symbol table — exactly the cross-module view completion needs.
    let result = juxc_driver::check_workspace(sources);
    collect_index(&result.symbols)
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
    WorkspaceIndex { type_names: types, member_names: members, type_packages }
}
