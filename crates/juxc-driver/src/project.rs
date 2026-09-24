//! Manifest-driven project & workspace build orchestration.
//!
//! This module turns a parsed [`Manifest`] into concrete build artifacts,
//! implementing the multi-module project model of
//! `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.2 (`[lib]` / `[[bin]]` / `[dependencies]`)
//! and §B.7 (`[workspace]`).
//!
//! ## Phase 1 — manifest-driven targets
//!
//! [`build_package`] reads a package's `[[bin]]` and `[lib]` targets and
//! produces:
//!
//! - one emitted Rust **binary** crate per `[[bin]]`, whose produced
//!   executable is named from `[[bin]].name` (so `[[bin]] name="myapp"`
//!   yields `myapp(.exe)`, not the legacy `jux_emitted`), built from the
//!   `[[bin]].path` entry point;
//! - one emitted Rust **library** crate for a `[lib]`, with the requested
//!   `crate-type`, built from the `[lib].path` entry point.
//!
//! ## Phase 2 — inter-module path dependencies
//!
//! [`build_workspace`] resolves `[workspace] members`, builds them in
//! dependency (topological) order, and links cross-module references.
//!
//! ### Cross-module resolution & linking strategy (first cut)
//!
//! Consistent with the existing "stdlib sources are prepended" model, a
//! package that path-depends on another module is compiled with that
//! dependency's **public `.jux` sources prepended** to its own source set
//! (see [`collect_dependency_sources`]). This makes the dependency's public
//! types available to `app`'s symbol table *and* lowers their bodies into
//! `app`'s emitted crate. The dependency is **also** emitted as its own
//! library crate (so the `[lib]` artifact exists and the emitted Cargo
//! workspace lists both members).
//!
//! **Documented limitation:** because resolution is by source-inclusion,
//! the dependency's lowered bodies are *duplicated* into the dependent
//! crate rather than referenced across a true Rust `extern crate`
//! boundary. The path to real separate-crate linking is to (a) emit the
//! dependency crate's `pub mod` tree as the library root (already done by
//! [`juxc_backend_rust::lower_workspace_lib`]), (b) add a Cargo path-dep
//! from `app`'s crate to `greeter`'s crate (the emitter for that line
//! already exists: [`juxc_backend_rust::PathDep`]), and (c) rewrite `app`'s
//! cross-module `use greeter.*` imports to `use <greeter_crate>::greeter::*`
//! while *omitting* the dependency's bodies from `app`'s own emission. Step
//! (c) — selective body omission keyed on which package each `import`
//! targets — is the remaining work; this first cut takes the
//! source-inclusion route so the build runs end-to-end today.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use juxc_backend_rust::{CrateTarget, PathDep};
use juxc_diagnostics::{Diagnostic, Severity};
use juxc_source::SourceFile;

use crate::manifest::{default_target_name, Manifest};
use crate::{build_emitted_crate, BuildArtifact};

/// Outcome of building one package (possibly several targets).
pub struct PackageBuild {
    /// Artifacts for each `[[bin]]` target, in declaration order.
    pub binaries: Vec<BuildArtifact>,
    /// Artifact for the `[lib]` target, if the package has one.
    pub library: Option<BuildArtifact>,
    /// All diagnostics emitted while compiling this package.
    pub diagnostics: Vec<Diagnostic>,
    /// The full source list the diagnostics index into.
    pub sources: Vec<SourceFile>,
}

impl PackageBuild {
    /// True when any diagnostic is error-severity.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| matches!(d.severity, Severity::Error))
    }
}

/// Restricts which of a package's targets [`build_package_selected`] builds.
/// The default (all-permissive) builds every target — the behavior of the plain
/// [`build_package`]. The CLI sets this from `--bin`/`--lib` so a multi-target
/// package can build a single target while still excluding the *other* bins'
/// entry files from the compile (which needs the full target list).
#[derive(Debug, Clone, Default)]
pub struct TargetSelection {
    /// Build only the `[[bin]]` with this name (skips the lib and other bins).
    pub bin: Option<String>,
    /// Build only the `[lib]` target (skips every bin).
    pub lib_only: bool,
}

/// Build every target (`[lib]` + each `[[bin]]`) declared by `manifest`. See
/// [`build_package_selected`] for the target-filtered form; this is the
/// build-everything entry point used by the workspace orchestration.
///
/// `dep_sources` are extra `.jux` sources (the public sources of this
/// package's path-dependencies) to prepend so cross-module symbols resolve;
/// `path_deps` are the corresponding emitted-crate path-dependency lines.
/// For a stand-alone single-package build both are empty.
///
/// `emit_root` is the directory under which each target's emitted crate is
/// written (a per-target subdirectory is created inside it).
pub fn build_package(
    manifest: &Manifest,
    dep_sources: &[SourceFile],
    path_deps: &[PathDep],
    emit_root: &Path,
    release: bool,
) -> Result<PackageBuild> {
    build_package_selected(
        manifest,
        dep_sources,
        path_deps,
        emit_root,
        release,
        &TargetSelection::default(),
        &cfg_facts_for(manifest, release),
    )
}

/// Build the `manifest`'s targets restricted by `selection` (`--bin`/`--lib`).
/// The FULL `manifest.bins` list is always consulted for sibling-entry
/// exclusion (`load_bin_sources`), so building one bin of a multi-bin package
/// still drops the other bins' top-level `main`s. Same arguments as
/// [`build_package`] otherwise.
pub fn build_package_selected(
    manifest: &Manifest,
    dep_sources: &[SourceFile],
    path_deps: &[PathDep],
    emit_root: &Path,
    release: bool,
    selection: &TargetSelection,
    cfg: &crate::cfg::CfgFacts,
) -> Result<PackageBuild> {
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    let mut all_sources: Vec<SourceFile> = Vec::new();
    let mut binaries = Vec::new();
    let mut library = None;

    // Resolve foreign (`rust.*` / `c.*` / `cpp.*`) `[dependencies]` to `.jux.d`
    // stubs (generating + caching them under `.jux-stubs/` when absent), then
    // gather every project stub source so the bound crates' APIs are in scope
    // (JUX-BINDGEN §G.6/§G.11). These units are `.jux.d`, so the front end
    // flags them `external` and the backend never lowers them.
    let stub_sources = resolve_and_load_stub_sources(manifest);

    // ---- [lib] target ---------------------------------------------------
    // Built unless a specific `--bin` was requested (a bin selection excludes
    // the lib). `--lib` keeps it; the default builds it.
    if let (Some(lib), None) = (&manifest.lib, &selection.bin) {
        let mut sources = dep_sources.to_vec();
        sources.extend(stub_sources.clone());
        sources.extend(load_lib_sources(manifest)?);
        let result = crate::compile_workspace_as_cfg(
            sources,
            juxc_backend_rust::lower_workspace_lib,
            cfg,
        )?;
        record(&result, &mut all_diagnostics, &mut all_sources);
        if let Some(crate_) = result.crate_ {
            let target = CrateTarget::Lib {
                name: lib.name.clone(),
                crate_type: lib.crate_type.clone(),
            };
            let dir = emit_root.join(format!("lib-{}", sanitize(&lib.name)));
            let artifact = build_emitted_crate(
                &crate_,
                &dir,
                &target,
                release,
                Some(manifest),
                path_deps,
                false,
            )?;
            library = Some(artifact);
        }
    }

    // ---- [[bin]] targets ------------------------------------------------
    // `--lib` skips bins entirely; `--bin X` builds only X. The loop still
    // iterates the FULL `manifest.bins` so `load_bin_sources` can exclude the
    // non-selected bins' entry files from the selected bin's compile.
    for bin in &manifest.bins {
        if selection.lib_only {
            break;
        }
        if let Some(only) = &selection.bin {
            if &bin.name != only {
                continue;
            }
        }
        // A `[[bin]] main = "xss.it.Main"` key names the entry file by dotted
        // path — validate it resolves to a real source so a typo is a clean
        // jux-level error, not a silent "no main found" later.
        if bin.entry.is_some() && !bin.path.is_file() {
            anyhow::bail!(
                "bin `{}`: entry `main = \"{}\"` resolves to {}, which does not exist",
                bin.name,
                bin.entry.as_deref().unwrap_or(""),
                bin.path.display(),
            );
        }
        let mut sources = dep_sources.to_vec();
        sources.extend(stub_sources.clone());
        sources.extend(load_bin_sources(manifest, &bin.path)?);
        // Prefer the manifest-named entry package for the `fn main` shim.
        let entry_pkg = bin.entry_package();
        let result = crate::compile_workspace_as_cfg(
            sources,
            move |u, s, e, src| {
                juxc_backend_rust::lower_workspace_with_entry(u, s, e, src, entry_pkg)
            },
            cfg,
        )?;
        record(&result, &mut all_diagnostics, &mut all_sources);
        if let Some(crate_) = result.crate_ {
            let target = CrateTarget::Bin { name: bin.name.clone() };
            let dir = emit_root.join(format!("bin-{}", sanitize(&bin.name)));
            let artifact = build_emitted_crate(
                &crate_,
                &dir,
                &target,
                release,
                Some(manifest),
                path_deps,
                false,
            )?;
            binaries.push(artifact);
        }
    }

    Ok(PackageBuild {
        binaries,
        library,
        diagnostics: all_diagnostics,
        sources: all_sources,
    })
}

/// One runnable program under a package's `examples/` directory (§B.1.3,
/// §B.15.3): either a single file `examples/<name>.jux` or a directory
/// `examples/<name>/` whose `.jux` files make one program.
#[derive(Debug, Clone)]
pub struct Example {
    /// The name `--example` takes: the file stem or the directory name.
    pub name: String,
    /// The example's own sources.
    pub files: Vec<PathBuf>,
}

/// Every example in `manifest`'s `examples/` directory, sorted by name.
/// Hidden entries and a `README.md` are not examples; a directory with no
/// `.jux` file in it is skipped.
pub fn discover_examples(manifest: &Manifest) -> Result<Vec<Example>> {
    let dir = manifest.project_root.join("examples");
    let mut out: Vec<Example> = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name.starts_with('.') || file_name == "target" {
            continue;
        }
        if path.is_dir() {
            let mut files = Vec::new();
            walk_jux(&path, &mut files)?;
            files.sort();
            if !files.is_empty() {
                out.push(Example { name: file_name.to_string(), files });
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("jux") {
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
            out.push(Example { name: name.to_string(), files: vec![path.clone()] });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Build one example as its own program (§B.1.3): the package's library code
/// (every `src/` source except the `[[bin]]` entry files, which each declare
/// their own `main`), the package's dependencies, and the example's sources.
/// The binary is named after the example and emitted under
/// `<emit_root>/example-<name>/`.
pub fn build_example(
    manifest: &Manifest,
    dep_sources: &[SourceFile],
    emit_root: &Path,
    release: bool,
    example: &Example,
    cfg: &crate::cfg::CfgFacts,
) -> Result<PackageBuild> {
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    let mut all_sources: Vec<SourceFile> = Vec::new();
    let mut binaries = Vec::new();

    let mut sources = dep_sources.to_vec();
    sources.extend(resolve_and_load_stub_sources(manifest));
    let entries: Vec<&Path> = manifest.bins.iter().map(|b| b.path.as_path()).collect();
    sources.extend(
        load_src_tree(&manifest.project_root.join("src"))?
            .into_iter()
            .filter(|s| !entries.iter().any(|e| same_path(s.path(), e))),
    );
    for file in &example.files {
        let text = std::fs::read_to_string(file)
            .with_context(|| format!("reading {}", file.display()))?;
        sources.push(SourceFile::new(file.clone(), text));
    }

    let result = crate::compile_workspace_as_cfg(
        sources,
        |u, s, e, src| juxc_backend_rust::lower_workspace_with_entry(u, s, e, src, None),
        cfg,
    )?;
    record(&result, &mut all_diagnostics, &mut all_sources);
    if let Some(crate_) = result.crate_ {
        let target = CrateTarget::Bin { name: example.name.clone() };
        let dir = emit_root.join(format!("example-{}", sanitize(&example.name)));
        let artifact =
            build_emitted_crate(&crate_, &dir, &target, release, Some(manifest), &[], false)?;
        binaries.push(artifact);
    }
    Ok(PackageBuild { binaries, library: None, diagnostics: all_diagnostics, sources: all_sources })
}

/// Outcome of a workspace build: each member's build keyed by package name,
/// in topological (dependency-first) order.
pub struct WorkspaceBuild {
    /// `(package-name, PackageBuild)` in build order.
    pub members: Vec<(String, PackageBuild)>,
}

impl WorkspaceBuild {
    /// True when any member build produced an error.
    pub fn has_errors(&self) -> bool {
        self.members.iter().any(|(_, b)| b.has_errors())
    }
}

/// Build a `[workspace]` rooted at `root`'s manifest.
///
/// Members are loaded, topologically ordered by their path-dependencies,
/// and built dependency-first. A member that path-depends on a sibling has
/// that sibling's public sources prepended (resolution) and a Cargo
/// path-dependency wired to the sibling's emitted library crate (linking
/// seam). See the module docs for the first-cut limitation.
pub fn build_workspace(root: &Manifest, release: bool) -> Result<WorkspaceBuild> {
    // Load every member manifest, keyed by package name. The member directory
    // each name came from is kept so `default-members` (written as
    // directories) can be mapped onto package names.
    let mut members: BTreeMap<String, Manifest> = BTreeMap::new();
    let mut dir_to_name: HashMap<String, String> = HashMap::new();
    for rel in &root.workspace_members {
        let dir = root.project_root.join(rel);
        let m = Manifest::load(&dir).with_context(|| {
            format!("workspace member `{rel}` has no readable jux.toml at {}", dir.display())
        })?;
        dir_to_name.insert(rel.clone(), m.package.name.clone());
        members.insert(m.package.name.clone(), m);
    }

    // Topologically sort members by intra-workspace path dependencies.
    let mut order = topo_order(&members)?;

    // A bare build builds the default members (§B.7.2) and whatever members
    // they depend on, since a dependent cannot be compiled without its
    // dependency's sources. With no `default-members` every member is a
    // default one, so nothing is dropped.
    let wanted = default_member_closure(root, &members, &dir_to_name);
    order.retain(|name| wanted.contains(name));

    // Emitted crates live under <workspace-root>/target/.rust-build/.
    let emit_root = root.project_root.join("target").join(".rust-build");

    // One feature plan for the whole workspace (§B.8.4): a member's crate is
    // built once, so every member compiled against it must see the same
    // features it was built with.
    let roots: Vec<&Manifest> = members.values().collect();
    let facts = cfg_facts_for_packages(&roots, release, root.profile);

    let mut built: Vec<(String, PackageBuild)> = Vec::new();
    for name in &order {
        let m = &with_root_profiles(&members[name], root);
        // Gather dependency sources + path-dep links for intra-workspace
        // path deps that are themselves workspace members.
        let (dep_sources, path_deps) = resolve_member_deps(m, &members, &emit_root)?;
        let build = build_package_selected(
            m,
            &dep_sources,
            &path_deps,
            &emit_root,
            release,
            &TargetSelection::default(),
            &facts.clone().with_profile(m.profile),
        )?;
        built.push((name.clone(), build));
        // If a member failed, stop — dependents would cascade-fail.
        if built.last().is_some_and(|(_, b)| b.has_errors()) {
            break;
        }
    }

    Ok(WorkspaceBuild { members: built })
}

/// Resolve a member's intra-workspace path dependencies into (prepended
/// source set, emitted-crate path-dep lines).
fn resolve_member_deps(
    m: &Manifest,
    members: &BTreeMap<String, Manifest>,
    emit_root: &Path,
) -> Result<(Vec<SourceFile>, Vec<PathDep>)> {
    check_module_cycles(m, members)?;
    let mut dep_sources: Vec<SourceFile> = Vec::new();
    let mut path_deps: Vec<PathDep> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    collect_dep_closure(m, members, emit_root, &mut dep_sources, &mut path_deps, &mut seen)?;
    Ok((dep_sources, path_deps))
}

/// The `@cfg` facts for a build of `manifest` alone (JUX-LANG-V1 §11). See
/// [`cfg_facts_for_packages`].
pub fn cfg_facts_for(manifest: &Manifest, release: bool) -> crate::cfg::CfgFacts {
    cfg_facts_for_packages(&[manifest], release, manifest.profile)
}

/// The `@cfg` facts for one build of the packages `roots` together -- a
/// workspace, or a single package -- with `release` and `profile`: the build's
/// target and optimization, and the enabled features of every root and of
/// every Jux package they depend on.
///
/// A root gets its `default` features (unless `--no-default-features`, carried
/// in `JUX_NO_DEFAULT_FEATURES`) and those `--features` names it declares
/// (`JUX_FEATURES`). A dependency gets the union of what every package
/// depending on it requests, with its own `default` set unless every one of
/// them says `default-features = false` -- Cargo's feature unification
/// (§B.8.4). Computing this once for the whole build is what keeps a
/// workspace member's crate and the members compiled against it in agreement.
pub fn cfg_facts_for_packages(
    roots: &[&Manifest],
    release: bool,
    profile: juxc_tycheck::Profile,
) -> crate::cfg::CfgFacts {
    let cli: BTreeSet<String> = std::env::var("JUX_FEATURES")
        .unwrap_or_default()
        .split(',')
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    for feature in &cli {
        if !roots.iter().any(|m| m.features.contains_key(feature)) {
            eprintln!("jux: warning: no package being built declares the feature `{feature}`");
        }
    }
    let root_defaults = std::env::var_os("JUX_NO_DEFAULT_FEATURES").is_none();
    // name -> (manifest, requested features, default set included)
    let mut packages: BTreeMap<String, (Manifest, BTreeSet<String>, bool)> = BTreeMap::new();
    let mut work = Vec::new();
    for m in roots {
        let requested = cli.iter().filter(|f| m.features.contains_key(*f)).cloned().collect();
        packages.insert(m.package.name.clone(), ((*m).clone(), requested, root_defaults));
        work.push(m.package.name.clone());
    }
    while let Some(name) = work.pop() {
        let Some((m, _, _)) = packages.get(&name) else { continue };
        let m = m.clone();
        for dep in &m.dependencies {
            if crate::stubs::foreign_dep_kind(&dep.name).is_some() {
                continue;
            }
            // The build resolves (and, for git, fetches) these itself; one
            // that cannot be found here has no sources to decide anything in.
            let dir = match (&dep.path, &dep.git) {
                (Some(p), _) => p.clone(),
                (None, Some(_)) => match crate::git_deps::fetch_git_dep(dep, false) {
                    Ok(dir) => dir,
                    Err(_) => continue,
                },
                _ => continue,
            };
            let Some(dep_manifest) = Manifest::load(&dir) else { continue };
            let key = dep_manifest.package.name.clone();
            let fresh = !packages.contains_key(&key);
            let entry = packages
                .entry(key.clone())
                .or_insert_with(|| (dep_manifest, BTreeSet::new(), false));
            let before = (entry.1.len(), entry.2);
            entry.1.extend(dep.features.iter().cloned());
            entry.2 |= dep.default_features;
            if fresh || (entry.1.len(), entry.2) != before {
                work.push(key);
            }
        }
    }
    packages.values().fold(crate::cfg::CfgFacts::new(release, profile), |facts, (m, requested, defaults)| {
        facts.with_package_features(m.project_root.clone(), m.enabled_features(requested, *defaults))
    })
}

/// Resolve a STANDALONE package's `[dependencies]` (path + git, §B.2.2)
/// into the (prepended source set, emitted-crate path-dep lines) pair
/// that [`build_package`] takes. Same closure walk the workspace path
/// uses, with no sibling members in scope. Public so the `jux` CLI's
/// single-package project mode resolves dependencies exactly like
/// workspace members do.
pub fn resolve_package_deps(
    m: &Manifest,
    emit_root: &Path,
) -> Result<(Vec<SourceFile>, Vec<PathDep>)> {
    resolve_member_deps(m, &BTreeMap::new(), emit_root)
}

/// Recursively gather the transitive closure of a member's path
/// dependencies: their public sources (for resolution) and their emitted
/// library-crate path-dep lines (for linking).
fn collect_dep_closure(
    m: &Manifest,
    members: &BTreeMap<String, Manifest>,
    emit_root: &Path,
    dep_sources: &mut Vec<SourceFile>,
    path_deps: &mut Vec<PathDep>,
    seen: &mut BTreeSet<String>,
) -> Result<()> {
    for dep in &m.dependencies {
        // A FOREIGN dep (`rust.*`, `c.*`, `cpp.*`) is not a Jux package, and
        // a `path`/`git` on one names a Rust crate directory rather than a
        // Jux project. Routing it through the loader below asked for a
        // `jux.toml` that was never going to exist, and reported its absence
        // as the error -- so binding a local crate looked like a malformed
        // dependency. It links through the emitted `Cargo.toml` instead
        // (`collect_registry_deps`).
        if crate::stubs::foreign_dep_kind(&dep.name).is_some() {
            continue;
        }
        // Source priority per §B.5.5: path > git > registry. Registry
        // deps aren't resolvable yet (no registry in Phase 1) and are
        // skipped; git deps fetch into the user cache and then behave
        // exactly like path deps.
        let dep_path: PathBuf = if let Some(p) = &dep.path {
            p.clone()
        } else if dep.git.is_some() {
            match crate::git_deps::fetch_git_dep(dep, false) {
                Ok(dir) => dir,
                Err(e) => {
                    // Fail the build with a clear, jux-level message —
                    // a missing dependency is fatal, but the user
                    // should see WHAT and WHY, not a cargo error later.
                    return Err(e.context(format!(
                        "resolving git dependency `{}` of `{}`",
                        dep.name, m.package.name
                    )));
                }
            }
        } else {
            continue;
        };
        // Find the matching workspace member (by package name).
        let Some(dep_manifest) = members.get(&dep.name) else {
            // A path/git dep that isn't a workspace member: load it
            // directly from its directory.
            let loaded = Manifest::load(&dep_path);
            if let Some(loaded) = loaded {
                add_dep(&loaded, members, emit_root, dep_sources, path_deps, seen)?;
            } else {
                anyhow::bail!(
                    "dependency `{}` of `{}` has no readable jux.toml at {}",
                    dep.name,
                    m.package.name,
                    dep_path.display(),
                );
            }
            continue;
        };
        add_dep(dep_manifest, members, emit_root, dep_sources, path_deps, seen)?;
    }
    Ok(())
}

/// Add one resolved dependency (its public sources + path-dep line) and
/// recurse into its own dependencies.
fn add_dep(
    dep_manifest: &Manifest,
    members: &BTreeMap<String, Manifest>,
    emit_root: &Path,
    dep_sources: &mut Vec<SourceFile>,
    path_deps: &mut Vec<PathDep>,
    seen: &mut BTreeSet<String>,
) -> Result<()> {
    let name = dep_manifest.package.name.clone();
    if !seen.insert(name.clone()) {
        return Ok(());
    }
    // Recurse first so transitive deps are prepended before this one.
    collect_dep_closure(dep_manifest, members, emit_root, dep_sources, path_deps, seen)?;
    // Public sources of the dependency (its [lib] entry + the package tree).
    dep_sources.extend(collect_dependency_sources(dep_manifest)?);
    // Emitted-crate path-dep line: the dependency's library crate lives at
    // `<emit_root>/lib-<name>/` and is the dependency's [lib] name.
    if let Some(lib) = &dep_manifest.lib {
        let crate_name = sanitize(&lib.name);
        let rel = format!("../lib-{}", sanitize(&lib.name));
        path_deps.push(PathDep { crate_name, rel_path: rel });
    }
    Ok(())
}

/// Topologically order workspace members so a member is built after the
/// members it path-depends on. Detects cycles (rejected per §B.4.6).
/// `member` as it builds inside `root`'s workspace: the root owns the build
/// profiles (§B.9.2), so a member that declares none takes the root's
/// `[profile.*]` tables. Without them a member's emitted crate had no custom
/// profile, and `jux build --profile <name>` at the root failed in cargo.
pub fn with_root_profiles(member: &Manifest, root: &Manifest) -> Manifest {
    let mut m = member.clone();
    if m.profiles.is_empty() {
        m.profiles = root.profiles.clone();
    }
    m
}

/// The package names a bare workspace build must compile: every
/// `default-members` package plus, transitively, the members it depends on
/// through `path` dependencies.
fn default_member_closure(
    root: &Manifest,
    members: &BTreeMap<String, Manifest>,
    dir_to_name: &HashMap<String, String>,
) -> BTreeSet<String> {
    let mut pending: Vec<String> = root
        .workspace_default_members
        .iter()
        .filter_map(|dir| dir_to_name.get(dir).cloned())
        .collect();
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if !wanted.insert(name.clone()) {
            continue;
        }
        if let Some(m) = members.get(&name) {
            for dep in m.dependencies.iter().filter(|d| d.path.is_some()) {
                if members.contains_key(&dep.name) {
                    pending.push(dep.name.clone());
                }
            }
        }
    }
    wanted
}

fn topo_order(members: &BTreeMap<String, Manifest>) -> Result<Vec<String>> {
    // Build adjacency: name → its in-workspace path-dependency names.
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    for (name, m) in members {
        let edges: Vec<String> = m
            .dependencies
            .iter()
            .filter(|d| d.path.is_some())
            .filter(|d| members.contains_key(&d.name))
            .map(|d| d.name.clone())
            .collect();
        deps.insert(name.clone(), edges);
    }
    let mut order = Vec::new();
    let mut visited: HashMap<String, u8> = HashMap::new(); // 0=unseen,1=on-stack,2=done
    let mut chain: Vec<String> = Vec::new();
    fn visit(
        name: &str,
        deps: &HashMap<String, Vec<String>>,
        visited: &mut HashMap<String, u8>,
        chain: &mut Vec<String>,
        order: &mut Vec<String>,
    ) -> Result<()> {
        match visited.get(name).copied().unwrap_or(0) {
            2 => return Ok(()),
            1 => return Err(module_cycle_error(chain, name)),
            _ => {}
        }
        visited.insert(name.to_string(), 1);
        chain.push(name.to_string());
        if let Some(edges) = deps.get(name) {
            for e in edges {
                visit(e, deps, visited, chain, order)?;
            }
        }
        chain.pop();
        visited.insert(name.to_string(), 2);
        order.push(name.to_string());
        Ok(())
    }
    for name in members.keys() {
        visit(name, &deps, &mut visited, &mut chain, &mut order)?;
    }
    Ok(order)
}

/// The E0308 error for a module dependency cycle (§B.4.6): `chain` is the
/// path of modules being resolved, and `back_to` the one it reached again.
/// Names the whole circle, `a -> b -> a`, since the fix is to break one of
/// its edges.
fn module_cycle_error(chain: &[String], back_to: &str) -> anyhow::Error {
    let start = chain.iter().position(|n| n == back_to).unwrap_or(0);
    let mut circle: Vec<String> = chain[start..].iter().map(|n| format!("`{n}`")).collect();
    circle.push(format!("`{back_to}`"));
    anyhow::anyhow!(
        "[{}] modules depend on each other in a circle: {}. A module is built after the modules \
         it depends on, so a circle has none to build first; move what they share into a module \
         both can depend on",
        juxc_diagnostics::code::Code::E0308_ModuleDependencyCycle.as_str(),
        circle.join(" -> "),
    )
}

/// Refuse a dependency cycle among the Jux modules reachable from `root`
/// through `path` dependencies (E0308), before anything is loaded. Without
/// this, a cycle between two standalone projects loaded the root's own
/// sources back in as a dependency and reported the first duplicate
/// declaration (`main is declared more than once`), not the cycle.
fn check_module_cycles(root: &Manifest, members: &BTreeMap<String, Manifest>) -> Result<()> {
    fn visit(
        m: &Manifest,
        members: &BTreeMap<String, Manifest>,
        chain: &mut Vec<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        let name = m.package.name.clone();
        if chain.contains(&name) {
            return Err(module_cycle_error(chain, &name));
        }
        if done.contains(&name) {
            return Ok(());
        }
        chain.push(name.clone());
        for dep in &m.dependencies {
            // Only Jux modules reached by path can close a circle here; a
            // foreign crate is not a Jux module, and a git dependency is a
            // published revision, which cannot depend on this working copy.
            let Some(path) = &dep.path else { continue };
            if crate::stubs::foreign_dep_kind(&dep.name).is_some() {
                continue;
            }
            if let Some(member) = members.get(&dep.name) {
                visit(member, members, chain, done)?;
            } else if let Some(loaded) = Manifest::load(path) {
                visit(&loaded, members, chain, done)?;
            }
        }
        chain.pop();
        done.insert(name);
        Ok(())
    }
    visit(root, members, &mut Vec::new(), &mut BTreeSet::new())
}

/// Load the `.jux` sources for a `[lib]` target: every `.jux` under the
/// package's `src/` directory (the library exposes its whole package tree).
fn load_lib_sources(manifest: &Manifest) -> Result<Vec<SourceFile>> {
    load_src_tree(&manifest.project_root.join("src"))
}

/// Load the `.jux` sources for a `[[bin]]` target. A binary needs its own
/// entry-point file *plus* any sibling package sources under `src/` (shared
/// library code per §B.15.2). The OTHER `[[bin]]` targets' entry files are
/// excluded: each declares its own top-level `main`, so compiling them together
/// would be a duplicate-`main` error (E0400). This bin's own entry is kept, as
/// is every non-entry source (the shared package code).
fn load_bin_sources(manifest: &Manifest, entry: &Path) -> Result<Vec<SourceFile>> {
    // The sibling bins' entry files to drop. Compare by path *components* so a
    // `/`-vs-`\` separator mismatch (manifest `join("src/main.jux")` vs the
    // walker's `join("src").join("main.jux")`) doesn't defeat the filter.
    let others: Vec<&Path> = manifest
        .bins
        .iter()
        .map(|b| b.path.as_path())
        .filter(|p| !same_path(p, entry))
        .collect();
    let all = load_src_tree(&manifest.project_root.join("src"))?;
    Ok(all
        .into_iter()
        .filter(|s| !others.iter().any(|o| same_path(s.path(), o)))
        .collect())
}

/// Path equality that is robust to separator/`.`-segment differences by
/// comparing normalized [`Path::components`] (so `a/b` == `a\b` == `./a/b`).
fn same_path(a: &Path, b: &Path) -> bool {
    a.components().eq(b.components())
}

/// Collect a dependency's *public* sources for prepending into a dependent
/// package's compile. First cut: the whole `src/` tree (Jux `public`
/// visibility is enforced by tycheck against the merged table, so private
/// members of the dependency stay inaccessible even though their source is
/// present).
pub fn collect_dependency_sources(dep_manifest: &Manifest) -> Result<Vec<SourceFile>> {
    load_src_tree(&dep_manifest.project_root.join("src"))
}

/// Resolve a package's foreign `[dependencies]` to `.jux.d` stubs and load every
/// project stub source.
///
/// For each `rust.<crate>` / `c.<lib>` / `cpp.<lib>` dependency
/// ([`crate::stubs::foreign_dep_kind`]) this generates + caches the stub under
/// `.jux-stubs/` when it's absent (Rust crates via rustdoc JSON; C/C++ require a
/// vendored stub for now — §G.6/§G.7). Generation failures are reported to
/// stderr and skipped rather than aborting the build, so an offline / non-nightly
/// environment still compiles (it just lacks that crate's autocomplete). Finally
/// every `.jux.d` under `.jux-stubs/` is loaded as a source — including the
/// default `rust.std.*` set, which the front end auto-prepends separately, so
/// only project-local crate stubs come from here.
pub fn resolve_and_load_stub_sources(manifest: &Manifest) -> Vec<SourceFile> {
    let root = &manifest.project_root;
    for dep in &manifest.dependencies {
        let Some((kind, crate_name)) = crate::stubs::foreign_dep_kind(&dep.name) else {
            continue; // ordinary Jux path dependency — handled elsewhere
        };
        if let Err(e) =
            crate::stubs::resolve_crate_stub(root, kind, crate_name, dep)
        {
            eprintln!(
                "jux: warning: could not resolve stub for `{}.{crate_name}` \
                 (autocomplete for it will be unavailable): {e}",
                kind
            );
        }
    }
    crate::stubs::load_project_stub_sources(root)
}

/// Summary of an [`ensure_project_stubs`] pass.
#[derive(Default)]
pub struct StubSyncReport {
    /// `.jux.d` stub paths now present on disk — cache hits plus any freshly
    /// generated this pass. One per foreign dependency that resolved.
    pub resolved: Vec<PathBuf>,
    /// Human-readable warnings for foreign deps whose stub couldn't be produced
    /// (offline, no nightly toolchain, or a C/C++ dep needing a vendored stub).
    /// Advisory only — the project still analyses, just without that crate's
    /// completion.
    pub warnings: Vec<String>,
}

/// Ensure every foreign (`rust.*` / `c.*` / `cpp.*`) `[dependencies]` entry of
/// the project rooted at `root` has a generated/cached `.jux.d` stub under
/// `.jux-stubs/`, so editor tooling indexes the bound crates' APIs in Jux syntax
/// **without first running a build** (JUX-BINDGEN §G.6/§G.11, §G.10).
///
/// This is the editor-side counterpart of [`build_package`]'s
/// [`resolve_and_load_stub_sources`]: the build path generates stubs as a side
/// effect of compiling, but the language server must do it up front so a
/// freshly-added Rust dependency autocompletes immediately. It processes the
/// root package's manifest and, when `root` is a workspace root, every
/// `[workspace] members` manifest — covering "all Rust crates across all
/// modules". The stubs land in each package's own `.jux-stubs/`, which the LSP's
/// workspace scan already walks.
///
/// rustdoc generation shells out (`cargo +nightly rustdoc`) and only runs for a
/// dep whose stub is **absent**, so this is meant to run once on project open
/// (or a manifest change), not per keystroke. A missing `jux.toml` or an
/// unresolvable dep degrades to a warning, never an error.
pub fn ensure_project_stubs(root: &Path) -> StubSyncReport {
    let mut report = StubSyncReport::default();
    let Some(root_manifest) = Manifest::load(root) else {
        return report; // no `jux.toml` here — nothing to resolve
    };

    // The packages to scan: the root package plus every workspace member, so a
    // workspace's modules all get their bound-crate stubs.
    let mut manifests = vec![root_manifest.clone()];
    for rel in &root_manifest.workspace_members {
        if let Some(member) = Manifest::load(&root.join(rel)) {
            manifests.push(member);
        }
    }

    for manifest in &manifests {
        let pkg_root = &manifest.project_root;
        for dep in &manifest.dependencies {
            let Some((kind, crate_name)) = crate::stubs::foreign_dep_kind(&dep.name) else {
                continue; // ordinary Jux path dependency
            };
            match crate::stubs::resolve_crate_stub(pkg_root, kind, crate_name, dep) {
                Ok(path) => report.resolved.push(path),
                Err(e) => report
                    .warnings
                    .push(format!("could not resolve stub for `{kind}.{crate_name}`: {e}")),
            }
        }
    }
    report
}

/// Walk a `src/` tree and load every `.jux` file as a [`SourceFile`].
fn load_src_tree(src_dir: &Path) -> Result<Vec<SourceFile>> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if src_dir.is_dir() {
        walk_jux(src_dir, &mut paths)?;
    }
    paths.sort();
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let contents = std::fs::read_to_string(&p)
            .with_context(|| format!("reading {}", p.display()))?;
        out.push(SourceFile::new(p, contents));
    }
    Ok(out)
}

/// Recursive `.jux` collector.
fn walk_jux(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" {
                continue;
            }
            walk_jux(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("jux") {
            out.push(path);
        }
    }
    Ok(())
}

/// Sanitize a name into a valid Cargo crate identifier (mirrors
/// [`default_target_name`]'s tail rules but operates on an already-chosen
/// target name).
fn sanitize(name: &str) -> String {
    // A target name is already a last-segment; run it through the same
    // sanitizer so e.g. a hyphenated lib name becomes a valid crate path.
    default_target_name(name)
}

/// Fold one compile result's diagnostics/sources into the package
/// accumulators (keeping the first non-empty source list — every target of
/// a package shares the same stdlib-prefixed shape, so the first wins for
/// diagnostic indexing).
fn record(
    result: &crate::CompileResult,
    diags: &mut Vec<Diagnostic>,
    sources: &mut Vec<SourceFile>,
) {
    diags.extend(result.diagnostics.iter().cloned());
    if sources.is_empty() {
        *sources = result.sources.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A multi-`[[bin]]` package: loading the sources for one bin must include
    /// that bin's entry plus shared (non-entry) sources, but EXCLUDE the other
    /// bins' entry files (each declares a top-level `main`, which would collide
    /// — E0400). Regression for `--bin` selection on a multi-bin package.
    #[test]
    fn load_bin_sources_excludes_sibling_bin_entries() {
        let root =
            std::env::temp_dir().join(format!("juxc-bin-sources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("jux.toml"),
            "[package]\nname = \"com.example.multi\"\n\n\
             [[bin]]\nname = \"alpha\"\npath = \"src/alpha.jux\"\n\n\
             [[bin]]\nname = \"beta\"\npath = \"src/beta.jux\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src").join("alpha.jux"), "public void main(){}").unwrap();
        std::fs::write(root.join("src").join("beta.jux"), "public void main(){}").unwrap();
        // Shared, non-entry package source — included for every bin.
        std::fs::write(root.join("src").join("shared.jux"), "public int helper(){ return 1; }")
            .unwrap();

        let m = Manifest::load(&root).expect("manifest loads");
        let alpha = m.bins.iter().find(|b| b.name == "alpha").unwrap();
        let sources = load_bin_sources(&m, &alpha.path).unwrap();
        let names: Vec<String> = sources
            .iter()
            .map(|s| {
                s.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        assert!(names.contains(&"alpha.jux".to_string()), "own entry kept: {names:?}");
        assert!(names.contains(&"shared.jux".to_string()), "shared source kept: {names:?}");
        assert!(
            !names.contains(&"beta.jux".to_string()),
            "sibling bin entry excluded: {names:?}",
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// `ensure_project_stubs` discovers a `rust.<crate>` dependency in the
    /// project manifest and resolves it to its cached `.jux.d` — the cache-hit
    /// path, so the editor indexes a bound crate without shelling out to
    /// `cargo rustdoc`. Mirrors the toolchain-free stub resolution test in
    /// `stubs.rs`.
    #[test]
    fn ensure_project_stubs_resolves_cached_rust_dep() {
        let root =
            std::env::temp_dir().join(format!("juxc-ensure-stubs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // A manifest with a Rust crate dependency and an unrelated path dep.
        std::fs::write(
            root.join("jux.toml"),
            "[package]\nname = \"app\"\n\n\
             [dependencies]\n\"rust.serde_json\" = \"1.0\"\n\"greeter\" = { path = \"../greeter\" }\n",
        )
        .unwrap();

        // Pre-seed the crate stub so resolution is a cache hit (no nightly).
        let stub = crate::stubs::crate_stub_cache_path(&root, "rust", "serde_json");
        std::fs::create_dir_all(stub.parent().unwrap()).unwrap();
        std::fs::write(&stub, "package rust.serde_json;\n").unwrap();

        let report = ensure_project_stubs(&root);
        assert!(report.warnings.is_empty(), "warnings: {:?}", report.warnings);
        assert_eq!(report.resolved, vec![stub], "the rust dep resolves to its cached stub");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// No `jux.toml` at the root → an empty report, never a panic. The LSP
    /// relies on this for a loose folder that isn't a Jux project.
    #[test]
    fn ensure_project_stubs_no_manifest_is_empty() {
        let root = std::env::temp_dir().join(format!("juxc-ensure-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let report = ensure_project_stubs(&root);
        assert!(report.resolved.is_empty());
        assert!(report.warnings.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }
}
