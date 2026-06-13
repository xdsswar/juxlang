//! Project-manifest (`jux.toml`) parsing for binary-resource metadata.
//!
//! Per `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.2.2, a project's `jux.toml`
//! carries a `[package]` table whose `version`/`authors`/`description`/
//! `license`/`homepage`/`repository` flow into every emitted target, and
//! whose `icon`/`company`/`copyright` drive the Windows version-info
//! resource embedded into executables.
//!
//! This module is intentionally tolerant: a missing manifest, a manifest
//! without a `[package]` table, or a `[package]` with only some keys all
//! parse successfully (every field except `name` is optional, and even
//! `name` has a sensible default). Unknown keys — `[dependencies]`,
//! `[lib]`, `[[bin]]`, `[profile.*]`, etc. — are simply ignored here;
//! `juxc` is the file-level compiler and only needs the resource metadata
//! at this stage.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Parsed `[package]` metadata from a project's `jux.toml`.
///
/// Construct via [`Manifest::load`]. The `icon` path (when present) is
/// resolved to an absolute path relative to the project root during
/// loading, so downstream code never has to re-resolve it.
#[derive(Debug, Default, Clone)]
pub struct PackageMetadata {
    /// `[package] name`. Reverse-DNS package name in real projects;
    /// defaults to `"app"` when absent so the manifest is never fatal.
    pub name: String,
    /// `[package] version` — SemVer. Optional; the Cargo.toml emitter
    /// falls back to `"0.0.0"` when this is `None`.
    pub version: Option<String>,
    /// `[package] edition` — language edition string. Parsed for
    /// completeness; the emitted Rust crate always uses Rust edition
    /// 2021 regardless (Phase-1 backend detail).
    pub edition: Option<String>,
    /// `[package] description` — one-line summary. Doubles as the
    /// `FileDescription` Windows resource.
    pub description: Option<String>,
    /// `[package] authors` — list of strings. Empty when absent.
    pub authors: Vec<String>,
    /// `[package] license` — SPDX identifier.
    pub license: Option<String>,
    /// `[package] homepage` — project URL.
    pub homepage: Option<String>,
    /// `[package] repository` — source-repository URL.
    pub repository: Option<String>,
    /// `[package] company` — `CompanyName` Windows resource. Defaults to
    /// the joined `authors` at build-script generation time when absent.
    pub company: Option<String>,
    /// `[package] copyright` — `LegalCopyright` Windows resource.
    pub copyright: Option<String>,
    /// `[package] icon` — Windows executable icon (`.ico`), resolved to an
    /// absolute path relative to the project root. `None` when absent.
    pub icon: Option<PathBuf>,
}

/// A `[lib]` target — the package produces a library artifact.
///
/// Per `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.2.2/§B.2.3. A `[lib]` is optional
/// and absent for executable-only projects. When present (or when a
/// `src/lib.jux` exists), the package emits a Rust **library** crate whose
/// `crate-type` is derived from the manifest's `crate-type` list.
#[derive(Debug, Clone)]
pub struct LibTarget {
    /// Source entry point for the library. Resolved to an absolute path
    /// against the project root. Default: `<root>/src/lib.jux`.
    pub path: PathBuf,
    /// Library crate name. Default: the package name's last `.`-segment.
    pub name: String,
    /// `crate-type` list. Maps Jux's spec values onto Rust crate-types:
    /// `"lib"` → `"lib"`, `"dylib"` → `"dylib"`, `"staticlib"` →
    /// `"staticlib"`, `"cdylib"` → `"cdylib"`. Empty defaults to `["lib"]`
    /// when emitting the Cargo.toml.
    pub crate_type: Vec<String>,
}

/// A `[[bin]]` target — an executable produced by the package.
///
/// Per §B.2.2/§B.2.3 and §B.15.2. Multiple `[[bin]]` blocks are allowed;
/// each must have a unique `name`. The default (when no `[[bin]]` is
/// declared but `src/main.jux` exists) is a single binary named after the
/// package's last segment with path `src/main.jux`.
#[derive(Debug, Clone)]
pub struct BinTarget {
    /// Executable name. Drives the produced `target/<profile>/<name>.exe`.
    pub name: String,
    /// Source entry point. Resolved to an absolute path against the
    /// project root. Default: `<root>/src/main.jux`.
    pub path: PathBuf,
}

/// A loaded `jux.toml` together with the project root it was found in.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// The directory containing `jux.toml` (the project root). Relative
    /// paths in the manifest (notably `icon`) are resolved against this.
    pub project_root: PathBuf,
    /// The parsed `[package]` metadata.
    pub package: PackageMetadata,
    /// The `[lib]` target, if the project declares one (or if a
    /// `src/lib.jux` exists on disk, in which case a default is synthesized).
    pub lib: Option<LibTarget>,
    /// The `[[bin]]` targets. When the manifest declares none but a
    /// `src/main.jux` exists, a single default binary is synthesized.
    pub bins: Vec<BinTarget>,
    /// `[dependencies]` path-dependencies (reverse-DNS name → relative
    /// path). Only `path` deps are modeled in Phase 1 (registry/git deps
    /// are recorded as `None` paths so resolution can report them).
    pub dependencies: Vec<Dependency>,
    /// `[workspace] members` — present only in workspace-root manifests.
    /// Each entry is a member directory relative to the workspace root.
    pub workspace_members: Vec<String>,
    /// `[build] profile` — the language profile (`full` / `embedded` / `core`,
    /// async addendum §18.1.11). Drives async availability (`core` forbids it,
    /// E0701). Defaults to [`juxc_tycheck::Profile::Full`].
    pub profile: juxc_tycheck::Profile,
    /// `[build] optimization` (§B.9) — the default build type when the CLI
    /// passes no `--release`. `"release"`/`"size"` select an optimized build,
    /// `"debug"`/`"none"` (and absence) select a debug build. See
    /// [`Manifest::effective_release`].
    pub optimization: Option<String>,
    /// `[build] target` (§B.9) — default cross-compilation target triple
    /// (`"native"` or a Rust triple). The CLI `--target` flag overrides it.
    pub build_target: Option<String>,
    /// `[profile.<name>]` tables (§B.9) — Cargo build-profile overrides
    /// (`opt-level`, `lto`, `strip`, …). Lowered to
    /// [`juxc_backend_rust::CargoProfile`] by [`Manifest::cargo_profiles`]
    /// and emitted into the generated `Cargo.toml`.
    pub profiles: Vec<ProfileSpec>,
}

/// A `[profile.<name>]` table (§B.9). Fields mirror the spec's profile
/// knobs; each is optional and only emitted when set. The raw values are
/// kept as [`toml::Value`] for the int-or-string fields (`opt-level`,
/// `debug`, `codegen-units`) so the Cargo renderer can choose bare-vs-quoted
/// output faithfully.
#[derive(Debug, Clone)]
pub struct ProfileSpec {
    /// Profile name: a Cargo built-in (`dev`/`release`/`test`/`bench`) or a
    /// custom name (which is emitted with an `inherits` line).
    pub name: String,
    /// `opt-level` — `0`–`3` (integer) or `"s"`/`"z"` (string).
    pub opt_level: Option<toml::Value>,
    /// `debug` — `false`/`true`, `0`–`2`, or `"line-tables"`/`"full"`/`"none"`.
    pub debug: Option<toml::Value>,
    /// `strip` — `false`, `"debuginfo"`, `"all"` (→ Cargo `"symbols"`).
    pub strip: Option<toml::Value>,
    /// `lto` — `"off"`/`"thin"`/`"fat"` (or a bool).
    pub lto: Option<toml::Value>,
    /// `overflow-checks` — trap on integer overflow.
    pub overflow_checks: Option<bool>,
    /// `panic` — `"unwind"`/`"abort"`.
    pub panic: Option<String>,
    /// `incremental` — cache intermediates.
    pub incremental: Option<bool>,
    /// `codegen-units` — integer (Cargo rejects non-integers, so a string is
    /// dropped during lowering).
    pub codegen_units: Option<toml::Value>,
    /// `extends` (spec) / `inherits` (Cargo) — the parent profile a custom
    /// profile derives from.
    pub extends: Option<String>,
}

/// A single `[dependencies]` entry. Phase 1 supports `path` and `git`
/// dependencies for Jux packages (§B.2.2); a registry dep records its
/// `version` for diagnostics but isn't resolvable yet (no registry).
#[derive(Debug, Clone)]
pub struct Dependency {
    /// Reverse-DNS dependency name as written in `[dependencies]`.
    pub name: String,
    /// `path = "..."` — a local filesystem dependency, resolved to an
    /// absolute path against the depending package's root. `None` for
    /// version/registry/git deps. Per §B.5.5 source priority
    /// (`path > git > registry`), a dep carrying BOTH `path` and `git`
    /// uses the path.
    pub path: Option<PathBuf>,
    /// `version = "..."` requirement string, if given.
    pub version: Option<String>,
    /// `git = "https://github.com/user/repo"` — a git-hosted Jux
    /// package (§B.2.2). Fetched into the user-level cache by
    /// [`crate::git_deps::fetch_git_dep`], after which it behaves
    /// exactly like a `path` dependency.
    pub git: Option<String>,
    /// Which ref the git source is pinned to. `None` = the remote's
    /// default branch.
    pub git_ref: Option<GitRef>,
}

/// The ref a `git` dependency pins to — `branch` (moves), `tag`
/// (effectively immutable), or `rev` (immutable commit). Per §B.2.2 the
/// three keys are mutually exclusive; when several are given, the most
/// specific wins (`rev` > `tag` > `branch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRef {
    /// `branch = "main"` — track a branch head.
    Branch(String),
    /// `tag = "v1.2"` — a tag.
    Tag(String),
    /// `rev = "abc123…"` — an exact commit.
    Rev(String),
}

impl GitRef {
    /// The user-facing label for diagnostics and cache-key hashing.
    pub fn describe(&self) -> String {
        match self {
            GitRef::Branch(b) => format!("branch={b}"),
            GitRef::Tag(t) => format!("tag={t}"),
            GitRef::Rev(r) => format!("rev={r}"),
        }
    }
}

/// Serde shape mirroring the `[package]` table of `jux.toml`. Only the
/// fields `juxc` consumes are declared; everything else in the table is
/// ignored. All fields are `Option`/`Vec` so partial tables deserialize.
#[derive(Debug, Default, Deserialize)]
struct RawPackage {
    name: Option<String>,
    version: Option<String>,
    edition: Option<String>,
    description: Option<String>,
    #[serde(default)]
    authors: Vec<String>,
    license: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    company: Option<String>,
    copyright: Option<String>,
    icon: Option<String>,
}

/// Serde shape for the `[lib]` table. All fields optional; defaults are
/// applied in [`Manifest::load`].
#[derive(Debug, Default, Deserialize)]
struct RawLib {
    path: Option<String>,
    name: Option<String>,
    #[serde(default, rename = "crate-type")]
    crate_type: Vec<String>,
}

/// Serde shape for one `[[bin]]` table entry.
#[derive(Debug, Default, Deserialize)]
struct RawBin {
    name: Option<String>,
    path: Option<String>,
}

/// Serde shape for the `[workspace]` table.
#[derive(Debug, Default, Deserialize)]
struct RawWorkspace {
    #[serde(default)]
    members: Vec<String>,
}

/// Serde shape for the `[build]` table (§B.9). The language `profile`
/// (`full`/`embedded`/`core`), the default `optimization` build type, and a
/// default `target` triple are consumed; other `[build]` keys are tolerated.
#[derive(Debug, Default, Deserialize)]
struct RawBuild {
    profile: Option<String>,
    optimization: Option<String>,
    target: Option<String>,
}

/// Serde shape for one `[profile.<name>]` table (§B.9). Every key is
/// optional. Int-or-string knobs are kept as [`toml::Value`] so the renderer
/// can emit them with faithful TOML typing. Both the spec's `extends` and
/// Cargo's `inherits` spelling are accepted.
#[derive(Debug, Default, Deserialize)]
struct RawProfile {
    #[serde(rename = "opt-level")]
    opt_level: Option<toml::Value>,
    debug: Option<toml::Value>,
    strip: Option<toml::Value>,
    lto: Option<toml::Value>,
    #[serde(rename = "overflow-checks")]
    overflow_checks: Option<bool>,
    panic: Option<String>,
    incremental: Option<bool>,
    #[serde(rename = "codegen-units")]
    codegen_units: Option<toml::Value>,
    extends: Option<String>,
    inherits: Option<String>,
}

/// Serde shape for a `[dependencies]` value. A dependency value is either
/// a bare version string (`"1.0"`) or a table with `path`/`version`/etc.
/// This untagged enum captures both.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawDependency {
    /// `"name" = "1.0"` — a bare SemVer requirement string.
    Version(String),
    /// `"name" = { path = "...", version = "...", ... }` — a table form.
    Detailed(RawDependencyTable),
}

/// Table form of a `[dependencies]` value. Phase 1 models `path`,
/// `version`, and the git source keys (`git` + `branch`/`tag`/`rev`,
/// §B.2.2); `features`/`registry`/etc. are tolerated and ignored.
#[derive(Debug, Default, Deserialize)]
struct RawDependencyTable {
    path: Option<String>,
    version: Option<String>,
    git: Option<String>,
    branch: Option<String>,
    tag: Option<String>,
    rev: Option<String>,
}

/// Top-level serde shape: the `[package]`, `[lib]`, `[[bin]]`,
/// `[dependencies]`, and `[workspace]` tables. Other top-level tables
/// (`[features]`, `[profile.*]`, …) are ignored because they're absent
/// from this struct and `toml` permits extra keys.
#[derive(Debug, Default, Deserialize)]
struct RawManifest {
    package: Option<RawPackage>,
    lib: Option<RawLib>,
    #[serde(default)]
    bin: Vec<RawBin>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, RawDependency>,
    workspace: Option<RawWorkspace>,
    build: Option<RawBuild>,
    #[serde(default)]
    profile: std::collections::BTreeMap<String, RawProfile>,
}

impl Manifest {
    /// Load the `jux.toml` directly in `project_root`, returning the
    /// parsed [`Manifest`] or `None`.
    ///
    /// Returns `None` when:
    /// - there is no `jux.toml` in `project_root`,
    /// - the file can't be read, or
    /// - the TOML is malformed.
    ///
    /// The two error cases are reported to stderr as warnings (rather than
    /// failing the build) so a typo in `jux.toml` doesn't block a
    /// compile that would otherwise succeed with default metadata.
    pub fn load(project_root: &Path) -> Option<Manifest> {
        let path = project_root.join("jux.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            // No manifest at all: the common loose-file case. Silent.
            Err(_) => return None,
        };
        let raw: RawManifest = match toml::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "juxc: warning: failed to parse {} ({e}); using default metadata",
                    path.display()
                );
                return None;
            }
        };
        let raw_pkg = raw.package.unwrap_or_default();

        // Resolve a relative icon path against the project root. An
        // absolute path in the manifest is taken as-is.
        let icon = raw_pkg.icon.as_ref().map(|rel| {
            let p = PathBuf::from(rel);
            if p.is_absolute() {
                p
            } else {
                project_root.join(p)
            }
        });

        let package = PackageMetadata {
            name: raw_pkg.name.unwrap_or_else(|| "app".to_string()),
            version: raw_pkg.version,
            edition: raw_pkg.edition,
            description: raw_pkg.description,
            authors: raw_pkg.authors,
            license: raw_pkg.license,
            homepage: raw_pkg.homepage,
            repository: raw_pkg.repository,
            company: raw_pkg.company,
            copyright: raw_pkg.copyright,
            icon,
        };

        // The default target name (used by both `[lib]` and the default
        // `[[bin]]`): the package name's last `.`-segment, sanitized for
        // use as a Cargo crate/binary name. `com.example.demo` → `demo`.
        let default_target_name = default_target_name(&package.name);

        // ---- [lib] target -------------------------------------------------
        // Present if the manifest declares `[lib]`, OR if `src/lib.jux`
        // exists on disk (the spec's "default: src/lib.jux if exists").
        let lib_default_path = project_root.join("src").join("lib.jux");
        let lib = match raw.lib {
            Some(rl) => {
                let path = rl
                    .path
                    .map(|p| resolve_against(project_root, &p))
                    .unwrap_or_else(|| lib_default_path.clone());
                Some(LibTarget {
                    path,
                    name: rl.name.unwrap_or_else(|| default_target_name.clone()),
                    crate_type: rl.crate_type,
                })
            }
            None if lib_default_path.is_file() => Some(LibTarget {
                path: lib_default_path,
                name: default_target_name.clone(),
                crate_type: Vec::new(),
            }),
            None => None,
        };

        // ---- [[bin]] targets ----------------------------------------------
        // Each explicit `[[bin]]` becomes a `BinTarget`. When none are
        // declared, synthesize a single default binary at `src/main.jux`
        // *if that file exists* (a lib-only project has no default bin).
        let mut bins: Vec<BinTarget> = Vec::new();
        for rb in raw.bin {
            let name = rb.name.unwrap_or_else(|| default_target_name.clone());
            let path = rb
                .path
                .map(|p| resolve_against(project_root, &p))
                .unwrap_or_else(|| project_root.join("src").join("main.jux"));
            bins.push(BinTarget { name, path });
        }
        if bins.is_empty() {
            let main_default = project_root.join("src").join("main.jux");
            if main_default.is_file() {
                bins.push(BinTarget {
                    name: default_target_name.clone(),
                    path: main_default,
                });
            }
        }

        // ---- [dependencies] -----------------------------------------------
        let dependencies = raw
            .dependencies
            .into_iter()
            .map(|(name, dep)| match dep {
                // A bare string is normally a SemVer requirement — but
                // when it LOOKS like a repository URL, treat it as the
                // shorthand git form: `"com.x.lib" = "https://github.com/u/r"`
                // ≡ `{ git = "..." }` (tracks the default branch).
                RawDependency::Version(v)
                    if v.starts_with("http://")
                        || v.starts_with("https://")
                        || v.starts_with("git@")
                        || v.starts_with("ssh://") =>
                {
                    Dependency {
                        name,
                        path: None,
                        version: None,
                        git: Some(v),
                        git_ref: None,
                    }
                }
                RawDependency::Version(v) => Dependency {
                    name,
                    path: None,
                    version: Some(v),
                    git: None,
                    git_ref: None,
                },
                RawDependency::Detailed(t) => {
                    // Ref keys are mutually exclusive per §B.2.2; the
                    // most specific wins when several are present.
                    let git_ref = t
                        .rev
                        .map(GitRef::Rev)
                        .or(t.tag.map(GitRef::Tag))
                        .or(t.branch.map(GitRef::Branch));
                    let path = t.path.map(|p| resolve_against(project_root, &p));
                    if path.is_some() && t.git.is_some() {
                        // §B.5.5 source priority: path > git. Local
                        // development override — say so, quietly.
                        eprintln!(
                            "juxc: note: dependency `{name}` declares both `path` and `git`; using the local path (source priority §B.5.5)"
                        );
                    }
                    Dependency {
                        name,
                        path,
                        version: t.version,
                        git: t.git,
                        git_ref,
                    }
                }
            })
            .collect();

        // ---- [workspace] --------------------------------------------------
        let workspace_members = raw.workspace.map(|w| w.members).unwrap_or_default();

        // ---- [build] table ------------------------------------------------
        let raw_build = raw.build.unwrap_or_default();
        let profile = raw_build
            .profile
            .map(|s| juxc_tycheck::Profile::from_manifest_str(&s))
            .unwrap_or_default();
        let optimization = raw_build.optimization;
        // `target = "native"` means "the host" — the same as leaving it
        // unset, so normalize it away here.
        let build_target = raw_build
            .target
            .filter(|t| !t.is_empty() && t != "native");

        // ---- [profile.*] tables -------------------------------------------
        // Preserve a stable, readable emission order: the Cargo built-ins
        // first (dev, release, test, bench), then any custom profiles
        // alphabetically (BTreeMap iteration).
        let mut profiles: Vec<ProfileSpec> = Vec::new();
        let order = ["dev", "release", "test", "bench"];
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut push_profile = |name: &str, rp: &RawProfile, profiles: &mut Vec<ProfileSpec>| {
            profiles.push(ProfileSpec {
                name: name.to_string(),
                opt_level: rp.opt_level.clone(),
                debug: rp.debug.clone(),
                strip: rp.strip.clone(),
                lto: rp.lto.clone(),
                overflow_checks: rp.overflow_checks,
                panic: rp.panic.clone(),
                incremental: rp.incremental,
                codegen_units: rp.codegen_units.clone(),
                // Spec `extends` is preferred; Cargo's `inherits` is accepted
                // as a synonym.
                extends: rp.extends.clone().or_else(|| rp.inherits.clone()),
            });
        };
        for name in order {
            if let Some(rp) = raw.profile.get(name) {
                push_profile(name, rp, &mut profiles);
                seen.insert(name.to_string());
            }
        }
        for (name, rp) in &raw.profile {
            if !seen.contains(name) {
                push_profile(name, rp, &mut profiles);
            }
        }

        Some(Manifest {
            project_root: project_root.to_path_buf(),
            package,
            lib,
            bins,
            dependencies,
            workspace_members,
            profile,
            optimization,
            build_target,
            profiles,
        })
    }

    /// Resolve the effective build type (`true` = release / optimized).
    ///
    /// Precedence (§B.9): an explicit CLI `--release` always wins; otherwise
    /// the manifest's `[build] optimization` decides — `"release"`/`"size"`
    /// build optimized, everything else (`"debug"`/`"none"`/absent) builds
    /// debug. `size` additionally implies `opt-level = "z"` (applied in
    /// [`Manifest::cargo_profiles`]) unless the user set `[profile.release]`.
    pub fn effective_release(&self, cli_release: bool) -> bool {
        if cli_release {
            return true;
        }
        matches!(
            self.optimization.as_deref(),
            Some("release") | Some("size"),
        )
    }

    /// Lower the parsed `[profile.*]` tables to the backend's
    /// [`juxc_backend_rust::CargoProfile`] shape — Cargo-spelled keys with
    /// pre-rendered TOML values. Also injects `opt-level = "z"` into the
    /// `release` profile when `[build] optimization = "size"` and the user
    /// didn't already pin an `opt-level` there (so the size build actually
    /// optimizes for size).
    pub fn cargo_profiles(&self) -> Vec<juxc_backend_rust::CargoProfile> {
        let mut out: Vec<juxc_backend_rust::CargoProfile> =
            self.profiles.iter().map(lower_profile).collect();

        if self.optimization.as_deref() == Some("size") {
            // Find or synthesize the `release` profile and ensure it carries a
            // size-oriented `opt-level` unless the user set one explicitly.
            let release = match out.iter_mut().find(|p| p.name == "release") {
                Some(p) => p,
                None => {
                    out.push(juxc_backend_rust::CargoProfile {
                        name: "release".to_string(),
                        entries: Vec::new(),
                    });
                    out.last_mut().unwrap()
                }
            };
            if !release.entries.iter().any(|(k, _)| k == "opt-level") {
                release.entries.insert(0, ("opt-level".to_string(), "\"z\"".to_string()));
            }
        }
        out
    }
}

/// Translate one [`ProfileSpec`] into the backend's Cargo-shaped profile.
///
/// Performs the spec→Cargo key/value translations §B.9 documents:
/// `debug = "line-tables"` → Cargo `"line-tables-only"`; `strip = "all"` →
/// Cargo `"symbols"`; spec `extends` → Cargo `inherits`. Custom profile names
/// (anything but `dev`/`release`/`test`/`bench`) always get an `inherits`
/// (defaulting to `"dev"`) because Cargo requires it. A non-integer
/// `codegen-units` is dropped (Cargo only accepts integers).
fn lower_profile(p: &ProfileSpec) -> juxc_backend_rust::CargoProfile {
    let mut entries: Vec<(String, String)> = Vec::new();

    if let Some(v) = &p.opt_level {
        entries.push(("opt-level".to_string(), render_toml_value(v)));
    }
    if let Some(v) = &p.debug {
        // `"line-tables"` (spec) → `"line-tables-only"` (Cargo).
        let rendered = match v.as_str() {
            Some("line-tables") => "\"line-tables-only\"".to_string(),
            _ => render_toml_value(v),
        };
        entries.push(("debug".to_string(), rendered));
    }
    if let Some(v) = &p.strip {
        // `"all"` (spec) → `"symbols"` (Cargo).
        let rendered = match v.as_str() {
            Some("all") => "\"symbols\"".to_string(),
            _ => render_toml_value(v),
        };
        entries.push(("strip".to_string(), rendered));
    }
    if let Some(v) = &p.lto {
        entries.push(("lto".to_string(), render_toml_value(v)));
    }
    if let Some(b) = p.overflow_checks {
        entries.push(("overflow-checks".to_string(), b.to_string()));
    }
    if let Some(s) = &p.panic {
        entries.push(("panic".to_string(), format!("\"{}\"", s.replace('"', ""))));
    }
    if let Some(b) = p.incremental {
        entries.push(("incremental".to_string(), b.to_string()));
    }
    if let Some(v) = &p.codegen_units {
        // Cargo only accepts an integer here; silently skip anything else.
        if v.as_integer().is_some() {
            entries.push(("codegen-units".to_string(), render_toml_value(v)));
        }
    }

    let is_builtin = matches!(p.name.as_str(), "dev" | "release" | "test" | "bench");
    if let Some(parent) = &p.extends {
        // `inherits` is meaningful (and required) only on custom profiles.
        if !is_builtin {
            entries.push(("inherits".to_string(), format!("\"{}\"", parent.replace('"', ""))));
        }
    } else if !is_builtin {
        // Cargo rejects a custom profile with no `inherits`; default to `dev`.
        entries.push(("inherits".to_string(), "\"dev\"".to_string()));
    }

    juxc_backend_rust::CargoProfile {
        name: p.name.clone(),
        entries,
    }
}

/// Render a scalar `toml::Value` back to its TOML source form for emission
/// into a `Cargo.toml`: integers/bools/floats bare, strings double-quoted
/// (and escaped). Non-scalar values (arrays/tables — not valid for these
/// profile keys) fall back to a quoted debug form, which Cargo will reject
/// loudly rather than mis-parse.
fn render_toml_value(v: &toml::Value) -> String {
    match v {
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        other => format!("\"{}\"", other.to_string().replace('"', "\\\"")),
    }
}

/// Resolve a possibly-relative manifest path against `base`. Absolute
/// paths are returned as-is.
fn resolve_against(base: &Path, rel: &str) -> PathBuf {
    let p = PathBuf::from(rel);
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

/// Compute the default target (crate/bin) name from a package name: the
/// last `.`-segment, sanitized so it's a valid Cargo identifier (invalid
/// characters → `_`, leading digit prefixed with `_`). `com.example.demo`
/// → `demo`; `app` → `app`.
pub fn default_target_name(package_name: &str) -> String {
    let last = package_name.rsplit('.').next().unwrap_or(package_name);
    let mut out: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        return "app".to_string();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

impl PackageMetadata {
    /// Project this manifest metadata into the backend's
    /// [`juxc_backend_rust::CargoMeta`] shape — the data the Cargo.toml
    /// emitter consumes. The `icon` flag is set from whether an icon path
    /// is present; the path itself is handled separately by the driver
    /// (it copies the `.ico` into the crate dir).
    pub fn to_cargo_meta(&self) -> juxc_backend_rust::CargoMeta {
        juxc_backend_rust::CargoMeta {
            version: self.version.clone(),
            authors: self.authors.clone(),
            description: self.description.clone(),
            license: self.license.clone(),
            homepage: self.homepage.clone(),
            repository: self.repository.clone(),
            company: self.company.clone(),
            copyright: self.copyright.clone(),
            has_icon: self.icon.is_some(),
            // Profiles live on the [`Manifest`], not on `[package]` — the
            // package-only projection carries none. Use
            // [`Manifest::to_cargo_meta`] to include them.
            profiles: Vec::new(),
        }
    }
}

impl Manifest {
    /// Project this manifest into the backend's
    /// [`juxc_backend_rust::CargoMeta`], including the lowered `[profile.*]`
    /// tables (§B.9). Prefer this over `manifest.package.to_cargo_meta()` at
    /// any call site that emits a `Cargo.toml`, so profile overrides flow
    /// through.
    pub fn to_cargo_meta(&self) -> juxc_backend_rust::CargoMeta {
        let mut meta = self.package.to_cargo_meta();
        meta.profiles = self.cargo_profiles();
        meta
    }
}

#[cfg(test)]
mod git_dep_tests {
    use super::*;

    /// Write a jux.toml into a fresh temp dir and load it back.
    fn load_from(toml: &str) -> Manifest {
        let dir = std::env::temp_dir().join(format!(
            "jux-manifest-gitdep-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("jux.toml"), toml).unwrap();
        let m = Manifest::load(&dir).expect("manifest loads");
        let _ = std::fs::remove_dir_all(&dir);
        m
    }

    #[test]
    fn git_dependency_table_form_parses_with_ref() {
        let m = load_from(
            "[package]\nname = \"com.x.app\"\n\n[dependencies]\n\"com.x.lib\" = { git = \"https://github.com/u/r\", branch = \"dev\" }\n",
        );
        let dep = &m.dependencies[0];
        assert_eq!(dep.git.as_deref(), Some("https://github.com/u/r"));
        assert_eq!(dep.git_ref, Some(GitRef::Branch("dev".to_string())));
        assert!(dep.path.is_none());
    }

    #[test]
    fn bare_url_string_is_git_shorthand() {
        // §B.2.2 shorthand: `"name" = "<url>"` ≡ `{ git = "<url>" }`.
        let m = load_from(
            "[package]\nname = \"com.x.app\"\n\n[dependencies]\n\"com.x.lib\" = \"https://github.com/u/r\"\n",
        );
        let dep = &m.dependencies[0];
        assert_eq!(dep.git.as_deref(), Some("https://github.com/u/r"));
        assert!(dep.git_ref.is_none());
        assert!(dep.version.is_none());
    }

    #[test]
    fn bare_version_string_stays_a_version() {
        let m = load_from(
            "[package]\nname = \"com.x.app\"\n\n[dependencies]\n\"rust.serde_json\" = \"1.0\"\n",
        );
        let dep = &m.dependencies[0];
        assert_eq!(dep.version.as_deref(), Some("1.0"));
        assert!(dep.git.is_none());
    }

    #[test]
    fn rev_wins_over_branch_when_both_given() {
        let m = load_from(
            "[package]\nname = \"com.x.app\"\n\n[dependencies]\n\"com.x.lib\" = { git = \"https://g/r\", branch = \"main\", rev = \"abc\" }\n",
        );
        assert_eq!(
            m.dependencies[0].git_ref,
            Some(GitRef::Rev("abc".to_string())),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Minimal RAII temp directory using only std — avoids pulling in a
    /// `tempfile` dependency just for manifest tests. Created under the
    /// OS temp dir with a per-process-unique counter; removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> TempDir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let id = N.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!(
                "juxc-manifest-test-{}-{}",
                std::process::id(),
                id,
            ));
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

    /// Write `jux.toml` into a fresh temp dir and load it. Returns the
    /// loaded manifest plus the temp dir (kept alive by the caller).
    fn load_toml(toml: &str) -> (Manifest, TempDir) {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("jux.toml"), toml).unwrap();
        let m = Manifest::load(dir.path()).expect("manifest loads");
        (m, dir)
    }

    #[test]
    fn default_target_name_takes_last_segment() {
        assert_eq!(default_target_name("com.example.demo"), "demo");
        assert_eq!(default_target_name("app"), "app");
        assert_eq!(default_target_name("a.b.my-lib"), "my-lib");
        // Leading digit gets an underscore prefix.
        assert_eq!(default_target_name("x.9foo"), "_9foo");
    }

    #[test]
    fn explicit_bin_name_and_path() {
        let (m, dir) = load_toml(
            "[package]\nname = \"com.example.demo\"\n\n\
             [[bin]]\nname = \"myapp\"\npath = \"src/main.jux\"\n",
        );
        assert_eq!(m.bins.len(), 1);
        assert_eq!(m.bins[0].name, "myapp");
        assert_eq!(m.bins[0].path, dir.path().join("src").join("main.jux"));
        assert!(m.lib.is_none());
    }

    #[test]
    fn default_bin_synthesized_when_main_exists() {
        let dir = TempDir::new();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.jux"), "public void main(){}").unwrap();
        std::fs::write(
            dir.path().join("jux.toml"),
            "[package]\nname = \"com.example.demo\"\n",
        )
        .unwrap();
        let m = Manifest::load(dir.path()).unwrap();
        // No [[bin]] declared, but src/main.jux exists → one default bin
        // named after the package's last segment.
        assert_eq!(m.bins.len(), 1);
        assert_eq!(m.bins[0].name, "demo");
    }

    #[test]
    fn lib_target_with_crate_type() {
        let (m, dir) = load_toml(
            "[package]\nname = \"com.example.mylib\"\n\n\
             [lib]\npath = \"src/lib.jux\"\nname = \"core\"\ncrate-type = [\"lib\", \"cdylib\"]\n",
        );
        let lib = m.lib.expect("lib target present");
        assert_eq!(lib.name, "core");
        assert_eq!(lib.path, dir.path().join("src").join("lib.jux"));
        assert_eq!(lib.crate_type, vec!["lib".to_string(), "cdylib".to_string()]);
    }

    #[test]
    fn path_dependency_resolved_against_root() {
        let (m, dir) = load_toml(
            "[package]\nname = \"app\"\n\n\
             [dependencies]\n\"greeter\" = { path = \"../greeter\" }\n\"reg\" = \"1.0\"\n",
        );
        let greeter = m.dependencies.iter().find(|d| d.name == "greeter").unwrap();
        assert_eq!(greeter.path, Some(dir.path().join("../greeter")));
        let reg = m.dependencies.iter().find(|d| d.name == "reg").unwrap();
        assert_eq!(reg.path, None);
        assert_eq!(reg.version.as_deref(), Some("1.0"));
    }

    #[test]
    fn build_profile_parsed_and_defaults_full() {
        let (core, _d1) = load_toml("[package]\nname = \"app\"\n\n[build]\nprofile = \"core\"\n");
        assert_eq!(core.profile, juxc_tycheck::Profile::Core);
        let (embedded, _d2) =
            load_toml("[package]\nname = \"app\"\n\n[build]\nprofile = \"embedded\"\n");
        assert_eq!(embedded.profile, juxc_tycheck::Profile::Embedded);
        // No `[build]` table → default `full`.
        let (default, _d3) = load_toml("[package]\nname = \"app\"\n");
        assert_eq!(default.profile, juxc_tycheck::Profile::Full);
    }

    #[test]
    fn workspace_members_parsed() {
        let (m, _dir) = load_toml("[workspace]\nmembers = [\"greeter\", \"app\"]\n");
        assert_eq!(
            m.workspace_members,
            vec!["greeter".to_string(), "app".to_string()]
        );
    }

    #[test]
    fn build_optimization_drives_effective_release() {
        // `optimization = "release"` builds release with no CLI flag.
        let (rel, _d1) =
            load_toml("[package]\nname = \"app\"\n\n[build]\noptimization = \"release\"\n");
        assert!(rel.effective_release(false));
        // `"size"` is also an optimized build.
        let (size, _d2) =
            load_toml("[package]\nname = \"app\"\n\n[build]\noptimization = \"size\"\n");
        assert!(size.effective_release(false));
        // `"debug"` builds debug unless the CLI forces release.
        let (dbg, _d3) =
            load_toml("[package]\nname = \"app\"\n\n[build]\noptimization = \"debug\"\n");
        assert!(!dbg.effective_release(false));
        assert!(dbg.effective_release(true)); // CLI --release wins
        // Absent → debug by default.
        let (none, _d4) = load_toml("[package]\nname = \"app\"\n");
        assert!(!none.effective_release(false));
    }

    #[test]
    fn build_target_native_normalized_away() {
        let (native, _d1) =
            load_toml("[package]\nname = \"app\"\n\n[build]\ntarget = \"native\"\n");
        assert_eq!(native.build_target, None);
        let (triple, _d2) = load_toml(
            "[package]\nname = \"app\"\n\n[build]\ntarget = \"x86_64-pc-windows-gnu\"\n",
        );
        assert_eq!(triple.build_target.as_deref(), Some("x86_64-pc-windows-gnu"));
    }

    #[test]
    fn profile_tables_parsed_and_lowered_to_cargo() {
        let (m, _dir) = load_toml(
            "[package]\nname = \"app\"\n\n\
             [profile.release]\n\
             opt-level = 3\n\
             lto = \"thin\"\n\
             strip = \"all\"\n\
             debug = false\n\
             codegen-units = 1\n\
             overflow-checks = false\n\
             panic = \"abort\"\n\n\
             [profile.dev]\n\
             opt-level = 0\n\
             debug = \"line-tables\"\n",
        );
        let profiles = m.cargo_profiles();
        // Built-in order: dev before release-by-name? We emit dev, release,
        // test, bench order — so dev first.
        let dev = profiles.iter().find(|p| p.name == "dev").unwrap();
        let release = profiles.iter().find(|p| p.name == "release").unwrap();

        // `strip = "all"` (spec) → Cargo `"symbols"`.
        assert!(release.entries.contains(&("strip".into(), "\"symbols\"".into())));
        // Integer opt-level emitted bare; string lto quoted.
        assert!(release.entries.contains(&("opt-level".into(), "3".into())));
        assert!(release.entries.contains(&("lto".into(), "\"thin\"".into())));
        assert!(release.entries.contains(&("codegen-units".into(), "1".into())));
        assert!(release.entries.contains(&("overflow-checks".into(), "false".into())));
        assert!(release.entries.contains(&("panic".into(), "\"abort\"".into())));
        // `debug = "line-tables"` (spec) → Cargo `"line-tables-only"`.
        assert!(dev.entries.contains(&("debug".into(), "\"line-tables-only\"".into())));
        // Built-in profiles never get an `inherits` line.
        assert!(!release.entries.iter().any(|(k, _)| k == "inherits"));
    }

    #[test]
    fn custom_profile_gets_inherits_from_extends() {
        let (m, _dir) = load_toml(
            "[package]\nname = \"app\"\n\n\
             [profile.embedded]\n\
             extends = \"release\"\n\
             opt-level = \"s\"\n",
        );
        let p = m.cargo_profiles();
        let embedded = p.iter().find(|p| p.name == "embedded").unwrap();
        // `extends` (spec) → Cargo `inherits`; string opt-level stays quoted.
        assert!(embedded.entries.contains(&("inherits".into(), "\"release\"".into())));
        assert!(embedded.entries.contains(&("opt-level".into(), "\"s\"".into())));
    }

    #[test]
    fn size_optimization_injects_opt_level_z_into_release() {
        // No explicit [profile.release] → one is synthesized with opt-level z.
        let (m, _dir) =
            load_toml("[package]\nname = \"app\"\n\n[build]\noptimization = \"size\"\n");
        let p = m.cargo_profiles();
        let release = p.iter().find(|p| p.name == "release").unwrap();
        assert!(release.entries.contains(&("opt-level".into(), "\"z\"".into())));

        // A user-set opt-level is NOT overridden.
        let (m2, _d2) = load_toml(
            "[package]\nname = \"app\"\n\n[build]\noptimization = \"size\"\n\n\
             [profile.release]\nopt-level = 2\n",
        );
        let p2 = m2.cargo_profiles();
        let release2 = p2.iter().find(|p| p.name == "release").unwrap();
        assert!(release2.entries.contains(&("opt-level".into(), "2".into())));
        assert!(!release2.entries.contains(&("opt-level".into(), "\"z\"".into())));
    }

    #[test]
    fn metadata_only_manifest_has_no_targets() {
        // The pre-existing `[package]`-only metadata shape must still parse,
        // with no synthesized lib/bin (no src/ files on disk).
        let (m, _dir) = load_toml(
            "[package]\nname = \"com.example.demo\"\nversion = \"1.2.3\"\n",
        );
        assert!(m.lib.is_none());
        assert!(m.bins.is_empty());
        assert_eq!(m.package.version.as_deref(), Some("1.2.3"));
    }
}
