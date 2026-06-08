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

/// A loaded `jux.toml` together with the project root it was found in.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// The directory containing `jux.toml` (the project root). Relative
    /// paths in the manifest (notably `icon`) are resolved against this.
    pub project_root: PathBuf,
    /// The parsed `[package]` metadata.
    pub package: PackageMetadata,
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

/// Top-level serde shape: just the `[package]` table. Other top-level
/// tables (`[dependencies]`, `[lib]`, `[[bin]]`, …) are ignored because
/// they're absent from this struct and `toml` permits extra keys.
#[derive(Debug, Default, Deserialize)]
struct RawManifest {
    package: Option<RawPackage>,
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

        Some(Manifest { project_root: project_root.to_path_buf(), package })
    }
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
        }
    }
}
