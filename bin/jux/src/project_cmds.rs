//! Project-maintenance commands of the `jux` tool (JUX-BUILD-SYSTEM-ADDENDUM
//! §B.10.5, §B.15): `new`, `init`, `clean`, `add`, `remove`, `tree`.
//!
//! None of these compile anything. They create, edit or inspect `jux.toml`
//! and the project layout, so they live apart from the build dispatch in
//! `main.rs`. Manifest edits go through `toml_edit`, which keeps the user's
//! comments, ordering and spacing: `jux add` must not reformat a file the
//! user wrote by hand.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

/// What kind of project `jux new` scaffolds (§B.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewKind {
    /// `jux new <name>`: a binary project.
    Bin,
    /// `jux new --lib <name>`: a library project.
    Lib,
    /// `jux new --workspace <name>`: an empty workspace root.
    Workspace,
}

/// A package-path segment made from a directory name: lowercase ASCII,
/// letters, digits and `_` only, not starting with a digit. `my-app` becomes
/// `my_app`, the form a `package` line and an import can spell.
pub fn package_segment(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// The `jux.toml` body for a new package (§B.15.1).
///
/// The spec's template writes a `[module]` table; `[module]` names nothing
/// else in the build system (§B.3: `jux.toml` is the only manifest, and its
/// package table is `[package]`), so the template uses `[package]` with the
/// same keys (ERRATA E71). `edition` sits in `[package]` where §B.2.1 puts it.
fn package_manifest(name: &str, lib: bool) -> String {
    let lib_table = if lib { "\n[lib]\n" } else { "" };
    format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2026\"\n\
         authors = [\"Your Name <you@example.com>\"]\n\
         license = \"Apache-2.0\"\n\
         {lib_table}\n\
         [build]\n\
         profile = \"full\"\n\
         target = \"native\"\n\
         optimization = \"release\"\n\
         \n\
         [dependencies]\n",
    )
}

/// The README a new project gets (§B.15.1).
fn readme(name: &str, lib: bool) -> String {
    let run = if lib {
        "## Testing\n\n    jux test\n"
    } else {
        "## Running\n\n    jux run\n"
    };
    format!("# {name}\n\nA Jux project.\n\n## Building\n\n    jux build\n\n{run}")
}

/// `jux new [--lib | --workspace] <name>`: scaffold a project directory.
///
/// A binary project gets exactly the four files of §B.15.1 (`jux.toml`,
/// `src/main.jux`, `README.md`, `.gitignore`). A library puts its code in its
/// package directory (`src/<name>/<Name>.jux`, §B.1.1) under a package-less
/// crate root `src/lib.jux`. A workspace gets a root manifest with an empty
/// member list. Refuses to overwrite an existing path.
pub fn cmd_new(name: &str, kind: NewKind) -> Result<ExitCode> {
    let target = PathBuf::from(name);
    if target.exists() {
        eprintln!("jux: target directory '{}' already exists", target.display());
        return Ok(ExitCode::from(1));
    }
    let base = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
        .to_string();
    std::fs::create_dir_all(&target)
        .with_context(|| format!("creating project directory {}", target.display()))?;
    write_new(&target, ".gitignore", "target/\n")?;

    match kind {
        NewKind::Workspace => {
            write_new(
                &target,
                "jux.toml",
                "[workspace]\n\
                 # Member packages, relative to this file. Patterns work: \"tools/*\".\n\
                 members = []\n\
                 \n\
                 [workspace.package]\n\
                 edition = \"2026\"\n\
                 \n\
                 [workspace.dependencies]\n",
            )?;
            write_new(
                &target,
                "README.md",
                &format!(
                    "# {base}\n\nA Jux workspace.\n\nAdd a member with `jux new --lib <name>` inside this directory and list it in `members`.\n\n## Building\n\n    jux build\n",
                ),
            )?;
        }
        NewKind::Bin => {
            write_new(&target, "jux.toml", &package_manifest(&base, false))?;
            write_new(
                &target,
                "src/main.jux",
                "public void main() {\n    print(\"Hello, world!\");\n}\n",
            )?;
            write_new(&target, "README.md", &readme(&base, false))?;
        }
        NewKind::Lib => {
            let segment = package_segment(&base);
            let type_name = type_name_for(&segment);
            write_new(&target, "jux.toml", &package_manifest(&segment, true))?;
            write_new(
                &target,
                "src/lib.jux",
                &format!(
                    "// Crate root of the `{segment}` library. It stays package-less; the\n\
                     // library's code lives under src/{segment}/ in `package {segment};`.\n",
                ),
            )?;
            write_new(
                &target,
                &format!("src/{segment}/{type_name}.jux"),
                &format!(
                    "package {segment};\n\n\
                     /** A first public function; `import {segment}.greet;` reaches it. */\n\
                     public String greet(String who) {{\n    return \"Hello, \" + who + \"!\";\n}}\n",
                ),
            )?;
            write_new(
                &target,
                &format!("test/{segment}/{type_name}Test.jux"),
                &format!(
                    "package {segment};\n\n\
                     import jux.std.testing.*;\n\n\
                     @Test\n\
                     void greetsByName() {{\n    assertEqual(\"Hello, Jux!\", greet(\"Jux\"));\n}}\n",
                ),
            )?;
            write_new(&target, "README.md", &readme(&base, true))?;
        }
    }
    let what = match kind {
        NewKind::Bin => "project",
        NewKind::Lib => "library",
        NewKind::Workspace => "workspace",
    };
    eprintln!("jux: created {what} at {}", target.display());
    let next = match kind {
        NewKind::Bin => "jux run",
        NewKind::Lib => "jux test",
        NewKind::Workspace => "jux new --lib <member>",
    };
    eprintln!("     next: `cd {name} && {next}`");
    Ok(ExitCode::SUCCESS)
}

/// `greeter` → `Greeter`, the file name for a new library's first source.
fn type_name_for(segment: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in segment.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    if out.is_empty() {
        "Lib".to_string()
    } else {
        out
    }
}

/// Write `contents` to `dir/rel`, creating directories on the way.
fn write_new(dir: &Path, rel: &str, contents: &str) -> Result<()> {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
}

/// `jux init`: turn the current directory into a project (§B.15). Writes a
/// `jux.toml` named after the directory, plus `src/main.jux` when there is no
/// `src/` yet and a `.gitignore` when there is none. An existing `jux.toml` is
/// never touched.
pub fn cmd_init(dir: &Path) -> Result<ExitCode> {
    if dir.join("jux.toml").exists() {
        eprintln!("jux: {} already has a jux.toml", dir.display());
        return Ok(ExitCode::from(1));
    }
    let base = dir
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().and_then(|s| s.to_str()).map(str::to_string))
        .unwrap_or_else(|| "app".to_string());
    let has_lib = dir.join("src").join("lib.jux").is_file();
    write_new(dir, "jux.toml", &package_manifest(&base, has_lib))?;
    // Loose `.jux` files already here are the user's program: point them at
    // the layout instead of adding a second `main` beside them.
    let loose: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("jux"))
                .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if !loose.is_empty() {
        eprintln!(
            "jux: note: a project builds what is under src/ (§B.1); move these there, the entry point as src/main.jux: {}",
            loose.join(", "),
        );
    } else if !dir.join("src").exists() {
        write_new(dir, "src/main.jux", "public void main() {\n    print(\"Hello, world!\");\n}\n")?;
    }
    if !dir.join(".gitignore").exists() {
        write_new(dir, ".gitignore", "target/\n")?;
    }
    eprintln!("jux: created jux.toml in {}", dir.display());
    Ok(ExitCode::SUCCESS)
}

/// `jux clean`: delete the build output (§B.15.4). That is the project's
/// `target/`, and for a workspace root also every member's own `target/`
/// (where `jux test` stages its runner). Sources are never touched.
pub fn cmd_clean(root: &Path) -> Result<ExitCode> {
    let mut dirs: Vec<PathBuf> = vec![root.join("target")];
    if let Some(manifest) = juxc_driver::Manifest::load(root) {
        for member in &manifest.workspace_members {
            dirs.push(root.join(member).join("target"));
        }
    }
    let mut removed = 0usize;
    for dir in dirs {
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("removing {}", dir.display()))?;
            eprintln!("jux: removed {}", dir.display());
            removed += 1;
        }
    }
    if removed == 0 {
        eprintln!("jux: nothing to clean");
    }
    Ok(ExitCode::SUCCESS)
}

/// Where a dependency added by `jux add` comes from.
#[derive(Debug, Default, Clone)]
pub struct DepSource {
    /// A SemVer requirement (`1.0`, `^2`).
    pub version: Option<String>,
    /// A local directory.
    pub path: Option<String>,
    /// A git repository URL.
    pub git: Option<String>,
    /// `branch` for a git dependency.
    pub branch: Option<String>,
    /// `tag` for a git dependency.
    pub tag: Option<String>,
    /// `rev` for a git dependency.
    pub rev: Option<String>,
    /// Features to enable on the dependency.
    pub features: Vec<String>,
}

/// `jux add <name>[@<version>] [--path P | --git URL [--branch|--tag|--rev]]
/// [--features a,b]`: add or replace one `[dependencies]` entry (§B.10.5).
///
/// A plain version becomes `"name" = "1.0"`; anything else becomes an inline
/// table. With no source at all the requirement is `"*"` (any version),
/// matching what a bare `cargo add` records before resolution. The rest of
/// the file is left exactly as it was.
pub fn cmd_add(root: &Path, spec: &str, mut source: DepSource) -> Result<ExitCode> {
    let (name, at_version) = match spec.split_once('@') {
        Some((n, v)) => (n.to_string(), Some(v.to_string())),
        None => (spec.to_string(), None),
    };
    if name.is_empty() {
        eprintln!("jux: add needs a dependency name");
        return Ok(ExitCode::from(2));
    }
    if source.version.is_none() {
        source.version = at_version;
    }
    let refs = [&source.branch, &source.tag, &source.rev].iter().filter(|r| r.is_some()).count();
    if refs > 1 {
        eprintln!("jux: give at most one of --branch, --tag, --rev");
        return Ok(ExitCode::from(2));
    }
    if refs == 1 && source.git.is_none() {
        eprintln!("jux: --branch/--tag/--rev need --git");
        return Ok(ExitCode::from(2));
    }
    if source.path.is_some() && source.git.is_some() {
        eprintln!("jux: give either --path or --git, not both");
        return Ok(ExitCode::from(2));
    }

    let path = root.join("jux.toml");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("jux: {} is not valid TOML: {e}", path.display());
            return Ok(ExitCode::from(1));
        }
    };
    let deps = doc
        .entry("dependencies")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let Some(deps) = deps.as_table_like_mut() else {
        eprintln!("jux: [dependencies] in {} is not a table", path.display());
        return Ok(ExitCode::from(1));
    };
    let existed = deps.contains_key(&name);
    deps.insert(&name, dependency_item(&source));
    std::fs::write(&path, doc.to_string()).with_context(|| format!("writing {}", path.display()))?;
    eprintln!(
        "jux: {} `{name}` in {}",
        if existed { "updated" } else { "added" },
        path.display(),
    );
    Ok(ExitCode::SUCCESS)
}

/// The TOML value for one dependency: a bare string for a version-only
/// dependency, an inline table otherwise.
fn dependency_item(source: &DepSource) -> toml_edit::Item {
    let only_version = source.path.is_none() && source.git.is_none() && source.features.is_empty();
    if only_version {
        let v = source.version.clone().unwrap_or_else(|| "*".to_string());
        return toml_edit::value(v);
    }
    let mut table = toml_edit::InlineTable::new();
    if let Some(v) = &source.version {
        table.insert("version", v.as_str().into());
    }
    if let Some(p) = &source.path {
        table.insert("path", p.as_str().into());
    }
    if let Some(g) = &source.git {
        table.insert("git", g.as_str().into());
    }
    for (key, value) in [("branch", &source.branch), ("tag", &source.tag), ("rev", &source.rev)] {
        if let Some(v) = value {
            table.insert(key, v.as_str().into());
        }
    }
    if !source.features.is_empty() {
        let mut array = toml_edit::Array::new();
        for f in &source.features {
            array.push(f.as_str());
        }
        table.insert("features", toml_edit::Value::Array(array));
    }
    toml_edit::value(table)
}

/// `jux remove <name>`: delete one `[dependencies]` entry (§B.10.5), in either
/// the `"name" = ...` form or a `[dependencies."name"]` table. Unknown names
/// are an error that lists the declared dependencies.
pub fn cmd_remove(root: &Path, name: &str) -> Result<ExitCode> {
    let path = root.join("jux.toml");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("jux: {} is not valid TOML: {e}", path.display());
            return Ok(ExitCode::from(1));
        }
    };
    let Some(deps) = doc.get_mut("dependencies").and_then(|d| d.as_table_like_mut()) else {
        eprintln!("jux: {} has no [dependencies]", path.display());
        return Ok(ExitCode::from(1));
    };
    if deps.remove(name).is_none() {
        let names: Vec<String> = deps.iter().map(|(k, _)| k.to_string()).collect();
        eprintln!(
            "jux: no dependency `{name}` in {}; declared: {}",
            path.display(),
            if names.is_empty() { "(none)".to_string() } else { names.join(", ") },
        );
        return Ok(ExitCode::from(1));
    }
    std::fs::write(&path, doc.to_string()).with_context(|| format!("writing {}", path.display()))?;
    eprintln!("jux: removed `{name}` from {}", path.display());
    Ok(ExitCode::SUCCESS)
}

/// `jux tree`: print the dependency tree (§B.10.5). Each line names a
/// dependency and where it comes from; a `path` dependency's own manifest is
/// followed, so the whole local graph shows. A workspace prints one tree per
/// member. A dependency seen twice on one branch is marked `(cycle)` instead
/// of looping.
pub fn cmd_tree(root: &Path) -> Result<ExitCode> {
    let Some(manifest) = juxc_driver::Manifest::load(root) else {
        eprintln!("jux: failed to load {}", root.join("jux.toml").display());
        return Ok(ExitCode::from(1));
    };
    let mut out = String::new();
    if manifest.workspace_members.is_empty() {
        render_tree(&manifest, &mut out);
    } else {
        for member in &manifest.workspace_members {
            if let Some(m) = juxc_driver::Manifest::load(&root.join(member)) {
                render_tree(&m, &mut out);
            }
        }
    }
    print!("{out}");
    Ok(ExitCode::SUCCESS)
}

/// Render one package's tree into `out`.
fn render_tree(manifest: &juxc_driver::Manifest, out: &mut String) {
    let version = manifest.package.version.as_deref().unwrap_or("0.0.0");
    out.push_str(&format!("{} v{version}\n", manifest.package.name));
    let mut stack = vec![manifest.package.name.clone()];
    render_deps(manifest, "", &mut stack, out);
}

fn render_deps(
    manifest: &juxc_driver::Manifest,
    prefix: &str,
    stack: &mut Vec<String>,
    out: &mut String,
) {
    let deps = &manifest.dependencies;
    for (i, dep) in deps.iter().enumerate() {
        let last = i + 1 == deps.len();
        let (branch, child_prefix) = if last { ("└── ", "    ") } else { ("├── ", "│   ") };
        let cycle = stack.contains(&dep.name);
        out.push_str(&format!(
            "{prefix}{branch}{} {}{}\n",
            dep.name,
            describe_source(dep),
            if cycle { " (cycle)" } else { "" },
        ));
        if cycle {
            continue;
        }
        if let Some(dir) = &dep.path {
            if let Some(child) = juxc_driver::Manifest::load(dir) {
                stack.push(dep.name.clone());
                render_deps(&child, &format!("{prefix}{child_prefix}"), stack, out);
                stack.pop();
            }
        }
    }
}

/// `(path ../ui)`, `(git https://... tag v1)`, `v1.0`, ...
fn describe_source(dep: &juxc_driver::manifest::Dependency) -> String {
    if let Some(p) = &dep.path {
        return format!("(path {})", tidy_path(p).display());
    }
    if let Some(g) = &dep.git {
        return match &dep.git_ref {
            Some(r) => format!("(git {g} {})", r.describe()),
            None => format!("(git {g})"),
        };
    }
    match &dep.version {
        Some(v) => format!("v{v}"),
        None => String::new(),
    }
}

/// `ws/app/../core` → `ws/core`: fold `.` and `..` segments without touching
/// the file system, so the tree shows the directory a person would type.
fn tidy_path(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_segments_are_spellable() {
        assert_eq!(package_segment("my-app"), "my_app");
        assert_eq!(package_segment("Greeter"), "greeter");
        assert_eq!(package_segment("3d"), "_3d");
        assert_eq!(type_name_for("my_app"), "MyApp");
    }

    #[test]
    fn dependency_items_pick_the_short_form_when_they_can() {
        let v = DepSource { version: Some("1.0".into()), ..DepSource::default() };
        assert_eq!(dependency_item(&v).to_string(), "\"1.0\"");
        let any = DepSource::default();
        assert_eq!(dependency_item(&any).to_string(), "\"*\"");
        let git = DepSource {
            git: Some("https://example.com/r".into()),
            tag: Some("v1".into()),
            ..DepSource::default()
        };
        let text = dependency_item(&git).to_string();
        assert!(text.contains("git = \"https://example.com/r\"") && text.contains("tag = \"v1\""), "{text}");
    }
}
