//! `jux.toml` validation, per `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.2.5.
//!
//! ## Why this module exists
//!
//! §B.2.1 draws the minimum-viable manifest as three keys, §B.2.2 marks all
//! three REQUIRED, §B.2.3 gives `package.name` a regex and §B.2.4 says v0.1
//! supports only edition `"2026"`. None of it was checked. This passed
//! `jux check` with no output whatsoever:
//!
//! ```toml
//! [package]
//! name = "MyApp"          # not lowercase, not reverse-DNS
//!
//! [nonsense]              # no such table; nothing reads it
//! whatever = 1
//! ```
//!
//! and so did a missing `version`, a missing `edition`, `edition = "2015"`, and
//! a misspelt `[depedencies]` that silently took no effect. ERRATA E106 records
//! the decision this module implements.
//!
//! ## The rule behind the table
//!
//! **A key's absence is a warning when the default is the only value it could
//! have had; a value the compiler cannot honour is an error.** A project written
//! before a key was required simply lacks it, and a compiler upgrade must not
//! break it over a key whose only legal value is the one that would have been
//! assumed. `edition = "2015"`, by contrast, asks for a language this compiler
//! does not implement, and compiling it as 2026 anyway would be a substitution
//! the user never gets to see.
//!
//! The one absence that IS an error is `name`: it is the only key with no
//! defensible default, being what a consumer writes in its own
//! `[dependencies]`, the default package path for every file under `src/`, and
//! the stem of the emitted artifact's name.
//!
//! ## How it reports
//!
//! The manifest is handed back as a [`SourceFile`] of its own, so the ordinary
//! renderer gives a manifest diagnostic the same `path:line:col`, the same
//! source snippet and the same caret as a diagnostic about a `.jux` file. That
//! is also why this module works from the raw TOML text rather than from a
//! parsed [`Manifest`]: a `Manifest` has already lost the position of the key
//! that is wrong.

use std::path::{Path, PathBuf};

use juxc_diagnostics::{code::Code, Diagnostic, Severity};
use juxc_source::{SourceFile, Span};

use crate::manifest::{Manifest, ManifestError};

/// The only edition v0.1 implements (§B.2.4), and the one assumed when the
/// manifest names none.
pub const CURRENT_EDITION: &str = "2026";

/// Top-level tables `jux.toml` may hold (§B.2.2, §B.8.1, §B.9, §B.14, and
/// DIAGNOSTICS §D.5.4's `[lints]`).
///
/// Several of these are specified ahead of their implementation (`publish`, the
/// `[ffi.<name>]` sub-tables), which is exactly why an unknown key is a warning
/// and not an error: a manifest has to stay forward-compatible. But an unread
/// key is otherwise indistinguishable from a key that works and happens to do
/// nothing, and that is how `[depedencies]` used to pass.
const KNOWN_TABLES: &[&str] = &[
    "package",
    "lib",
    "bin",
    "dependencies",
    "dev-dependencies",
    "build-dependencies",
    "features",
    "workspace",
    "build",
    "profile",
    "ffi",
    "publish",
    "lints",
];

/// Keys `[package]` may hold (§B.2.2's full schema).
const KNOWN_PACKAGE_KEYS: &[&str] = &[
    "name",
    "version",
    "edition",
    "description",
    "authors",
    "license",
    "license-file",
    "homepage",
    "repository",
    "documentation",
    "readme",
    "keywords",
    "categories",
    "icon",
    "company",
    "copyright",
    // `[package.sign]` is a reserved seam (§B.2.2): a sub-table rather than a
    // key, listed here so it is not reported as unknown before it is built.
    "sign",
];

/// What validating one `jux.toml` found.
///
/// Carries the manifest AS A SOURCE FILE so the caller can hand it straight to
/// the renderer: every span in `diagnostics` is relative to `source`, whose
/// index is 0.
pub struct ManifestCheck {
    /// The `jux.toml`, read, with index 0 so the renderer can resolve spans.
    /// Empty contents when the file could not be read at all.
    pub source: SourceFile,
    /// Everything §B.2.5 had to say about it, in file order.
    pub diagnostics: Vec<Diagnostic>,
}

impl ManifestCheck {
    /// True when at least one diagnostic is an error, so the caller must stop.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    /// Apply this manifest's own `[lints]` (and `-Werror` when `deny_warnings`)
    /// to what validating it found, so `all = "deny"` or `W0902 = "allow"`
    /// governs the manifest's warnings as it governs the package's sources
    /// (DIAGNOSTICS §D.5.4). Call before [`ManifestCheck::has_errors`].
    pub fn apply_lint_levels(&mut self, deny_warnings: bool) {
        let config = toml::from_str::<toml::Value>(self.source.contents())
            .ok()
            .and_then(|v| v.get("lints").map(crate::lints::LintConfig::from_toml))
            .unwrap_or_default();
        crate::lints::apply_to_manifest(&mut self.diagnostics, &config, deny_warnings);
    }
}

/// Validate the `jux.toml` directly in `project_root`. `None` when there is no
/// manifest there, which is the ordinary loose-file case and not a defect.
pub fn check(project_root: &Path) -> Option<ManifestCheck> {
    let path = project_root.join("jux.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            // The file is there and unreadable. Report it against an empty
            // source: there is no text to point into, but the path still names
            // the file the user has to go and look at.
            let d = Diagnostic::error(
                Code::E0901_ManifestUnreadable,
                format!("cannot read `{}`: {e}", path.display()),
            )
            .with_file(0);
            return Some(ManifestCheck {
                source: source_for(&path, String::new()),
                diagnostics: vec![d],
            });
        }
    };
    Some(validate(project_root, &path, text))
}

/// Validate `root`'s manifest and, when `root` is a workspace root, every
/// `[workspace] members` manifest too (§B.7.1).
///
/// A member is part of what `jux build` at the root compiles, so a member with
/// a broken manifest has to stop the same build a broken root would. Dependency
/// manifests are deliberately NOT validated: a dependency is checked when it is
/// itself the package being built, and a consumer cannot fix a defect in a
/// manifest it does not own.
pub fn check_project(root: &Path) -> Vec<ManifestCheck> {
    let mut out = Vec::new();
    let Some(root_check) = check(root) else {
        return out;
    };
    // The member list comes from the parsed manifest, which needs the root to
    // have parsed at all. When it did not, the `E0901` in `root_check` is the
    // whole answer and hunting for members would only add noise.
    let members: Vec<String> = if root_check.has_errors() {
        Vec::new()
    } else {
        Manifest::try_load(root)
            .ok()
            .flatten()
            .map(|m| m.workspace_members)
            .unwrap_or_default()
    };
    out.push(root_check);
    for rel in members {
        if let Some(member) = check(&root.join(&rel)) {
            out.push(member);
        }
    }
    out
}

/// The field checks, given the manifest's text.
fn validate(project_root: &Path, path: &Path, text: String) -> ManifestCheck {
    let source = source_for(path, text);
    let mut diagnostics = Vec::new();

    // Parse through the loader's own front door, so a manifest this check
    // accepts is exactly one the loader accepts. A shape error only the typed
    // deserializer catches (`version = 1.0` as a float, `[[package]]` as an
    // array of tables) is an `E0901` as well: the loader cannot produce a
    // `Manifest` from it either, and used to answer `None`, which every caller
    // read as "this directory has no manifest".
    if let Err(e) = Manifest::try_load(project_root) {
        let span = match &e {
            ManifestError::Malformed { span: Some(s), .. } => {
                Span::in_file(s.start as u32, s.end as u32, 0)
            }
            // No span from the parser: point at the first byte, which at least
            // puts the file's name and a line number on the message.
            _ => Span::in_file(0, 0, 0),
        };
        let message = match &e {
            ManifestError::Malformed { message, .. } => {
                // The `toml` crate's messages run to several lines ("invalid
                // table header\nexpected `.`, `]`"), and a diagnostic's summary
                // is one line by contract: the renderer prints it beside the
                // code, above the snippet.
                format!("`{}` is not a valid manifest: {}", path.display(), one_line(message))
            }
            ManifestError::Unreadable { message, .. } => {
                format!("cannot read `{}`: {}", path.display(), one_line(message))
            }
        };
        diagnostics.push(
            Diagnostic::error(Code::E0901_ManifestUnreadable, message)
                .with_span(span)
                .with_file(0)
                .with_help("the manifest schema is JUX-BUILD-SYSTEM §B.2.2"),
        );
        return ManifestCheck { source, diagnostics };
    }

    // Re-parse as a plain table for the field checks. A `Manifest` has already
    // lost which line each key was on, and a diagnostic that cannot point at
    // the key is one the user has to go and search for.
    let Ok(mut value) = toml::from_str::<toml::Value>(source.contents()) else {
        // Unreachable in practice: `try_load` just parsed the same text.
        return ManifestCheck { source, diagnostics };
    };
    // Resolve `key.workspace = true` first, exactly as the loader does, so an
    // inherited `edition` is validated as the value it actually takes rather
    // than as the table `{ workspace = true }`. An inherited key no root
    // defines is dropped here (with §B.7.2a's warning) and then reads as
    // absent below, which is what it is.
    crate::workspace::inherit_from_workspace(&mut value, project_root);
    let Some(table) = value.as_table() else {
        return ManifestCheck { source, diagnostics };
    };

    // ---- unknown top-level tables (W0902) --------------------------------
    for key in table.keys() {
        if !KNOWN_TABLES.contains(&key.as_str()) {
            diagnostics.push(
                Diagnostic::warning(
                    Code::W0902_ManifestUnknownKey,
                    format!("unknown top-level table `{key}` in `jux.toml`: nothing reads it"),
                )
                .with_span(key_span(&source, None, key))
                .with_file(0)
                .with_help(format!(
                    "the tables `jux.toml` defines are {}",
                    KNOWN_TABLES.join(", ")
                )),
            );
        }
    }

    // ---- [lints] (DIAGNOSTICS §D.5.4) ------------------------------------
    // Before `[package]`, because a virtual workspace manifest may carry one.
    check_lints(&source, table, &mut diagnostics);

    // ---- [package] -------------------------------------------------------
    let Some(package) = table.get("package").and_then(|p| p.as_table()) else {
        // A manifest holding only `[workspace]` is a VIRTUAL manifest: it
        // declares no package, so none of the `[package]` rules apply. That is
        // the shape `jux new --workspace` writes and the shape every workspace
        // root in this repository uses.
        if !table.contains_key("workspace") {
            diagnostics.push(
                Diagnostic::error(
                    Code::E0902_ManifestMissingName,
                    format!(
                        "`{}` declares neither a `[package]` nor a `[workspace]` table",
                        path.display()
                    ),
                )
                .with_span(Span::in_file(0, 0, 0))
                .with_file(0)
                .with_help(
                    "a package manifest starts with `[package]` and its `name`, `version` \
                     and `edition`; a workspace root has `[workspace] members = [...]`",
                ),
            );
        }
        return ManifestCheck { source, diagnostics };
    };

    for key in package.keys() {
        if !KNOWN_PACKAGE_KEYS.contains(&key.as_str()) {
            diagnostics.push(
                Diagnostic::warning(
                    Code::W0902_ManifestUnknownKey,
                    format!("unknown key `{key}` in `[package]`: nothing reads it"),
                )
                .with_span(key_span(&source, Some("package"), key))
                .with_file(0),
            );
        }
    }

    check_name(&source, path, package, &mut diagnostics);
    check_version(&source, package, &mut diagnostics);
    check_edition(&source, package, &mut diagnostics);
    check_missing_required(&source, package, &mut diagnostics);

    // File order, so a manifest with several problems reads top to bottom.
    diagnostics.sort_by_key(|d| d.primary_span.map_or(0, |s| s.start));
    ManifestCheck { source, diagnostics }
}

/// `[lints]` (DIAGNOSTICS §D.5.4): every level a level, every key a lint.
///
/// A value the build cannot honour is `E0903`, by the rule at the top of this
/// module: `unsafe-without-justification = "forbid"` asks for something the
/// compiler does not do, and quietly treating it as `warn` would let an
/// unjustified `unsafe` through a build its author believed was guarded. A key
/// that names nothing is `W0902`, as any unread key is. So is a lint §D.5.4
/// names that this compiler does not raise yet: its setting is legal and has
/// no effect, and the author should know the second part.
fn check_lints(
    source: &SourceFile,
    table: &toml::map::Map<String, toml::Value>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(value) = table.get("lints") else { return };
    let Some(lints) = value.as_table() else {
        out.push(
            Diagnostic::error(Code::E0903_ManifestInvalidValue, "`lints` must be a table: `[lints]`")
                .with_span(key_span(source, None, "lints"))
                .with_file(0),
        );
        return;
    };
    let level_help = "a level is \"allow\", \"warn\" or \"deny\"";
    for (key, value) in lints {
        let span = key_span(source, Some("lints"), key);
        let bad_level = || value.as_str().and_then(crate::lints::LintLevel::parse).is_none();
        match key.as_str() {
            "warnings-as-errors" => {
                if value.as_bool().is_none() {
                    out.push(
                        Diagnostic::error(
                            Code::E0903_ManifestInvalidValue,
                            format!("`[lints] warnings-as-errors = {value}` must be `true` or `false`"),
                        )
                        .with_span(span)
                        .with_file(0),
                    );
                }
                continue;
            }
            "all" => {
                if bad_level() {
                    out.push(
                        Diagnostic::error(
                            Code::E0903_ManifestInvalidValue,
                            format!("`[lints] all = {value}` is not a lint level"),
                        )
                        .with_span(span)
                        .with_file(0)
                        .with_help(level_help),
                    );
                }
                continue;
            }
            _ => {}
        }
        match crate::lints::lint_key(key) {
            crate::lints::LintKey::Warning(_) | crate::lints::LintKey::NotRaised if bad_level() => {
                out.push(
                    Diagnostic::error(
                        Code::E0903_ManifestInvalidValue,
                        format!("`[lints] {key} = {value}` is not a lint level"),
                    )
                    .with_span(span)
                    .with_file(0)
                    .with_help(level_help),
                );
            }
            crate::lints::LintKey::Warning(_) => {}
            crate::lints::LintKey::NotRaised => out.push(
                Diagnostic::warning(
                    Code::W0902_ManifestUnknownKey,
                    format!(
                        "lint `{key}` is specified (DIAGNOSTICS §D.5.4) but this compiler does not \
                         raise it yet, so its level has no effect"
                    ),
                )
                .with_span(span)
                .with_file(0),
            ),
            crate::lints::LintKey::Error(code) => out.push(
                Diagnostic::error(
                    Code::E0903_ManifestInvalidValue,
                    format!("`{code}` is an error, and an error has no lint level"),
                )
                .with_span(span)
                .with_file(0)
                .with_help("only warnings (`W....`) and the named lints of §D.5.4 take a level"),
            ),
            crate::lints::LintKey::Unknown => out.push(
                Diagnostic::warning(
                    Code::W0902_ManifestUnknownKey,
                    format!("unknown lint `{key}` in `[lints]`: nothing reads it"),
                )
                .with_span(span)
                .with_file(0)
                .with_help(crate::lints::lint_names_help()),
            ),
        }
    }
}

/// `package.name`: present, a string, legally spelled (`E0902` / `E0903`), and
/// reverse-DNS shaped (`W0903`).
fn check_name(
    source: &SourceFile,
    path: &Path,
    package: &toml::map::Map<String, toml::Value>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(value) = package.get("name") else {
        out.push(
            Diagnostic::error(
                Code::E0902_ManifestMissingName,
                format!("`[package]` in `{}` has no `name`", path.display()),
            )
            .with_span(key_span(source, None, "package"))
            .with_file(0)
            .with_help(
                "every other `[package]` key has a default, but the name is what consumers \
                 write in their `[dependencies]` and the package path for every file under \
                 `src/`: write it reverse-DNS, as `name = \"com.example.app\"`",
            ),
        );
        return;
    };
    let span = key_span(source, Some("package"), "name");
    let Some(name) = value.as_str() else {
        out.push(
            Diagnostic::error(
                Code::E0903_ManifestInvalidValue,
                "`[package] name` must be a string".to_string(),
            )
            .with_span(span)
            .with_file(0),
        );
        return;
    };
    match name_problem(name) {
        Some(NameProblem::Illegal(what)) => out.push(
            Diagnostic::error(
                Code::E0903_ManifestInvalidValue,
                format!("`[package] name = \"{name}\"` is not a package name: {what}"),
            )
            .with_span(span)
            .with_file(0)
            .with_help(
                "§B.2.3: lowercase, dot-separated, matching \
                 `^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$`",
            ),
        ),
        Some(NameProblem::Shape(what)) => out.push(
            Diagnostic::warning(
                Code::W0903_ManifestNameNotReverseDns,
                format!("`[package] name = \"{name}\"` is not reverse-DNS shaped: {what}"),
            )
            .with_span(span)
            .with_file(0)
            .with_help(
                "the name builds as written; reverse-DNS (`com.example.app`) is what keeps \
                 two unrelated packages from claiming one name",
            ),
        ),
        None => {}
    }
}

/// `package.version`: a string, and SemVer 2.0 when present (`E0903`).
/// Absence is handled by [`check_missing_required`].
fn check_version(
    source: &SourceFile,
    package: &toml::map::Map<String, toml::Value>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(value) = package.get("version") else { return };
    let span = key_span(source, Some("package"), "version");
    let Some(text) = value.as_str() else {
        out.push(
            Diagnostic::error(
                Code::E0903_ManifestInvalidValue,
                "`[package] version` must be a quoted string: `version = \"0.1.0\"`, not a \
                 bare number"
                    .to_string(),
            )
            .with_span(span)
            .with_file(0),
        );
        return;
    };
    if !is_semver(text) {
        out.push(
            Diagnostic::error(
                Code::E0903_ManifestInvalidValue,
                format!("`[package] version = \"{text}\"` is not a SemVer 2.0 version"),
            )
            .with_span(span)
            .with_file(0)
            .with_help(
                "§B.2.3: `MAJOR.MINOR.PATCH`, all three, optionally with a `-prerelease` \
                 and a `+build` (`0.1.0`, `1.2.3-alpha.1`, `2.0.0+build.7`)",
            ),
        );
    }
}

/// `package.edition`: `"2026"` when present (`E0903`). Absence is handled by
/// [`check_missing_required`], because an absent edition can only have meant
/// the one edition that exists.
fn check_edition(
    source: &SourceFile,
    package: &toml::map::Map<String, toml::Value>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(value) = package.get("edition") else { return };
    // `edition = 2026` without quotes is the natural typo, so compare the
    // rendered value rather than demanding a string first: an integer 2026 is
    // still the edition the author meant, and only the quoting is wrong.
    let named = value.as_str().map_or_else(|| value.to_string(), str::to_string);
    if named == CURRENT_EDITION && value.as_str().is_none() {
        out.push(
            Diagnostic::error(
                Code::E0903_ManifestInvalidValue,
                format!("`[package] edition` must be a quoted string: `edition = \"{CURRENT_EDITION}\"`"),
            )
            .with_span(key_span(source, Some("package"), "edition"))
            .with_file(0),
        );
        return;
    }
    if named != CURRENT_EDITION {
        out.push(
            Diagnostic::error(
                Code::E0903_ManifestInvalidValue,
                format!(
                    "`[package] edition = {}` names an edition this compiler does not implement",
                    quoted(value)
                ),
            )
            .with_span(key_span(source, Some("package"), "edition"))
            .with_file(0)
            .with_help(format!(
                "§B.2.4: v0.1 supports only `edition = \"{CURRENT_EDITION}\"`"
            )),
        );
    }
}

/// One `W0901` naming every REQUIRED key the `[package]` is missing.
///
/// One diagnostic rather than one per key: across a workspace of six members
/// that is the difference between six lines and twelve, and a warning nobody
/// reads is a warning that does not work.
fn check_missing_required(
    source: &SourceFile,
    package: &toml::map::Map<String, toml::Value>,
    out: &mut Vec<Diagnostic>,
) {
    let mut missing: Vec<&str> = Vec::new();
    if !package.contains_key("version") {
        missing.push("version");
    }
    if !package.contains_key("edition") {
        missing.push("edition");
    }
    if missing.is_empty() {
        return;
    }
    let listed = missing.join("` and `");
    let assumed = match missing.as_slice() {
        ["version"] => "version `0.0.0` is assumed".to_string(),
        ["edition"] => format!("edition `\"{CURRENT_EDITION}\"` is assumed"),
        _ => format!("version `0.0.0` and edition `\"{CURRENT_EDITION}\"` are assumed"),
    };
    out.push(
        Diagnostic::warning(
            Code::W0901_ManifestMissingRequiredKey,
            format!("`[package]` is missing the required key `{listed}`: {assumed}"),
        )
        .with_span(key_span(source, None, "package"))
        .with_file(0)
        .with_help("§B.2.1: the minimum manifest is `name`, `version` and `edition`"),
    );
}

/// What is wrong with a `package.name`, if anything.
enum NameProblem {
    /// A character §B.2.3's grammar does not allow. An error: such a name
    /// cannot be written in an `import` at all, and it is mangled on its way
    /// into the emitted crate path, so the failure surfaces far from the cause.
    Illegal(String),
    /// Legal, but not the reverse-DNS shape §B.2.3 asks for. A warning: it
    /// resolves unambiguously and builds correctly.
    Shape(String),
}

/// Check `name` against `^[a-z][a-z0-9]*(\.[a-z][a-z0-9_]*)+$`, splitting the
/// regex's two jobs apart: which characters are allowed (an error to break) and
/// how many segments there are (a warning).
fn name_problem(name: &str) -> Option<NameProblem> {
    if name.is_empty() {
        return Some(NameProblem::Illegal("it is empty".to_string()));
    }
    let segments: Vec<&str> = name.split('.').collect();
    for segment in &segments {
        if segment.is_empty() {
            return Some(NameProblem::Illegal(
                "it has an empty segment (a leading, trailing or doubled `.`)".to_string(),
            ));
        }
        let first = segment.chars().next().unwrap_or('.');
        if !first.is_ascii_lowercase() {
            return Some(NameProblem::Illegal(format!(
                "segment `{segment}` starts with `{first}`, and every segment starts with a \
                 lowercase ASCII letter"
            )));
        }
        // `_` is legal in every segment but the first, so it is accepted here
        // and the first-segment case is reported below as a shape problem.
        if let Some(c) = segment
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_'))
        {
            return Some(NameProblem::Illegal(format!(
                "`{c}` is not allowed; a package name is lowercase ASCII letters, digits, \
                 `_` after the first segment, and `.` between segments"
            )));
        }
    }
    // A single segment is a legal, unambiguous name, and it is what `jux new
    // myapp` writes (§B.15.1), so it is not warned about (ERRATA E106). The
    // shape rule is the regex's other half: in a DOTTED name, the first
    // segment is the reverse-DNS root and takes no `_`.
    if segments.len() >= 2 && segments[0].contains('_') {
        return Some(NameProblem::Shape(
            "the first segment holds an `_`, which §B.2.3's grammar allows only in later \
             segments"
                .to_string(),
        ));
    }
    None
}

/// SemVer 2.0: `MAJOR.MINOR.PATCH`, each a numeric identifier with no leading
/// zero, optionally `-prerelease` then optionally `+build`.
///
/// Hand-rolled rather than pulled in as a dependency, because this is the only
/// place the compiler needs it and the grammar is six lines of it.
fn is_semver(text: &str) -> bool {
    // Split the build metadata off first: it is the last `+` onward, and its
    // contents never constrain the version core.
    let (rest, build) = match text.split_once('+') {
        Some((a, b)) => (a, Some(b)),
        None => (text, None),
    };
    if build.is_some_and(|b| !b.split('.').all(is_alnum_identifier)) {
        return false;
    }
    let (core, pre) = match rest.split_once('-') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    if pre.is_some_and(|p| !p.split('.').all(is_prerelease_identifier)) {
        return false;
    }
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| is_numeric_identifier(p))
}

/// A SemVer numeric identifier: digits only, and no leading zero unless the
/// whole identifier is `0`.
fn is_numeric_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s.len() == 1 || !s.starts_with('0'))
}

/// A SemVer pre-release identifier: alphanumerics and hyphens, non-empty, with
/// numeric-only ones following the no-leading-zero rule.
fn is_prerelease_identifier(s: &str) -> bool {
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return is_numeric_identifier(s);
    }
    is_alnum_identifier(s)
}

/// A build-metadata identifier: alphanumerics and hyphens, non-empty, leading
/// zeros allowed.
fn is_alnum_identifier(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Collapse a multi-line message onto one line, which is what a diagnostic's
/// summary has to be: the renderer prints it beside the code and above the
/// source snippet, and a newline in the middle of it breaks that layout.
fn one_line(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A TOML value as the user would have written it, for quoting in a message:
/// `"2015"` with quotes, `2026` without.
fn quoted(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

/// The manifest as a source file with index 0, which is the index every
/// diagnostic this module produces carries.
fn source_for(path: &Path, text: String) -> SourceFile {
    let mut source = SourceFile::new(PathBuf::from(path), text);
    source.set_index(0);
    source
}

/// The span of `key`'s declaration inside `table` (`None` for the top level),
/// or of the table header itself when `key` names a table.
///
/// A hand-rolled scan rather than a spanned deserialize: `toml::Spanned` would
/// mean wrapping every field of every `Raw*` struct, which changes the loader's
/// types for the sake of a diagnostic, and the scan only has to be good enough
/// to land on the right line. When it finds nothing it answers the first byte,
/// so the diagnostic still names the file.
fn key_span(source: &SourceFile, table: Option<&str>, key: &str) -> Span {
    let text = source.contents();
    let mut current: Option<String> = None;
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let lead = line.len() - trimmed.len();
        let body = trimmed.trim_end();
        if let Some(header) = table_header(body) {
            // The header line itself, when the caller asked for a table name.
            if table.is_none() && header == key {
                let start = offset + lead;
                return Span::in_file(start as u32, (start + body.len()) as u32, 0);
            }
            current = Some(header.to_string());
        } else if current.as_deref() == table {
            if let Some(found) = line_key(body) {
                if found == key {
                    let start = offset + lead;
                    return Span::in_file(start as u32, (start + found.len()) as u32, 0);
                }
            }
        }
        offset += line.len();
    }
    Span::in_file(0, 0, 0)
}

/// `[package]` / `[[bin]]` / `[profile.dev]` to the top-level table name it
/// names (`package`, `bin`, `profile`), or `None` for a non-header line.
fn table_header(body: &str) -> Option<&str> {
    let inner = body
        .strip_prefix("[[")
        .and_then(|s| s.strip_suffix("]]"))
        .or_else(|| body.strip_prefix('[').and_then(|s| s.strip_suffix(']')))?;
    Some(inner.trim().split('.').next().unwrap_or(inner).trim())
}

/// The key a `key = value` line assigns to, unquoted, with a dotted suffix
/// dropped (`edition.workspace = true` is the key `edition`, which is the key a
/// reader of the file sees).
fn line_key(body: &str) -> Option<&str> {
    if body.starts_with('#') {
        return None;
    }
    let (left, _) = body.split_once('=')?;
    let head = left.trim().trim_matches('"').split('.').next()?;
    let head = head.trim().trim_matches('"');
    (!head.is_empty()).then_some(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write `toml` to a directory of its own and answer the codes validating
    /// it produced, in file order.
    fn codes_for(toml: &str) -> Vec<String> {
        let dir = std::env::temp_dir().join(format!(
            "jux-manifest-check-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("jux.toml"), toml).expect("write manifest");
        let check = check(&dir).expect("a manifest is present");
        let codes = check
            .diagnostics
            .iter()
            .map(|d| d.code.as_str().to_string())
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        codes
    }

    #[test]
    fn the_minimum_viable_manifest_is_clean() {
        let codes = codes_for(
            "[package]\nname = \"com.example.app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
        );
        assert!(codes.is_empty(), "{codes:?}");
    }

    /// The defect this module was written for: all three of these passed.
    #[test]
    fn the_unvalidated_manifest_is_reported() {
        let codes = codes_for("[package]\nname = \"MyApp\"\n\n[nonsense]\nwhatever = 1\n");
        assert!(codes.contains(&"E0903".to_string()), "{codes:?}");
        assert!(codes.contains(&"W0901".to_string()), "{codes:?}");
        assert!(codes.contains(&"W0902".to_string()), "{codes:?}");
    }

    #[test]
    fn an_old_edition_is_an_error_and_a_missing_one_is_a_warning() {
        let wrong = codes_for(
            "[package]\nname = \"com.example.app\"\nversion = \"0.1.0\"\nedition = \"2015\"\n",
        );
        assert_eq!(wrong, vec!["E0903"], "edition 2015 is not implementable");

        let absent = codes_for("[package]\nname = \"com.example.app\"\nversion = \"0.1.0\"\n");
        assert_eq!(absent, vec!["W0901"], "an absent edition only defaults");
    }

    #[test]
    fn a_missing_name_is_an_error_but_a_virtual_manifest_is_not() {
        let no_name = codes_for("[package]\nversion = \"0.1.0\"\nedition = \"2026\"\n");
        assert_eq!(no_name, vec!["E0902"]);
        assert!(codes_for("[workspace]\nmembers = [\"a\"]\n").is_empty());
        assert_eq!(codes_for("[features]\nfast = []\n"), vec!["E0902"]);
    }

    /// `[lints]` (DIAGNOSTICS §D.5.4): a known table, its levels checked, its
    /// keys checked, and the spec's own example accepted.
    #[test]
    fn the_lints_table_is_validated() {
        let head = "[package]\nname = \"com.example.app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[lints]\n";
        let clean = codes_for(&format!(
            "{head}warnings-as-errors = false\nall = \"warn\"\nunsafe-without-justification = \"deny\"\nW0470 = \"allow\"\n"
        ));
        assert!(clean.is_empty(), "{clean:?}");
        // Specified by §D.5.4 but not raised yet: legal, with no effect.
        assert_eq!(codes_for(&format!("{head}unused-import = \"deny\"\n")), vec!["W0902"]);
        assert_eq!(codes_for(&format!("{head}no-such-lint = \"deny\"\n")), vec!["W0902"]);
        assert_eq!(codes_for(&format!("{head}W0820 = \"forbid\"\n")), vec!["E0903"]);
        assert_eq!(codes_for(&format!("{head}all = 3\n")), vec!["E0903"]);
        assert_eq!(codes_for(&format!("{head}warnings-as-errors = \"yes\"\n")), vec!["E0903"]);
        assert_eq!(codes_for(&format!("{head}E0410 = \"allow\"\n")), vec!["E0903"]);
        // A virtual workspace manifest may carry one too.
        assert_eq!(codes_for("[workspace]\nmembers = []\n\n[lints]\nbogus = \"deny\"\n"), vec!["W0902"]);
    }

    /// A manifest's own warnings answer to its `[lints]` and to `-Werror`.
    #[test]
    fn the_manifests_own_warnings_take_its_levels() {
        let dir = std::env::temp_dir().join(format!("jux-manifest-lints-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let write_and_check = |toml: &str, deny: bool| {
            std::fs::write(dir.join("jux.toml"), toml).expect("write manifest");
            let mut c = check(&dir).expect("a manifest is present");
            c.apply_lint_levels(deny);
            (c.has_errors(), c.diagnostics.len())
        };
        let base = "[package]\nname = \"com.example.app\"\n\n[nonsense]\n";
        assert_eq!(write_and_check(base, false), (false, 2), "W0901 + W0902, both warnings");
        assert_eq!(write_and_check(base, true), (true, 2), "-Werror promotes both");
        assert_eq!(write_and_check(&format!("{base}\n[lints]\nall = \"allow\"\n"), true), (false, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_toml_is_e0901_not_a_missing_manifest() {
        assert_eq!(codes_for("[package\nname = \"com.example.app\"\n"), vec!["E0901"]);
    }

    #[test]
    fn a_single_segment_name_is_clean_and_an_illegal_one_errors() {
        // What `jux new myapp` and `jux new my-app` write.
        for one in ["linalg", "my_app"] {
            let codes = codes_for(&format!(
                "[package]\nname = \"{one}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n"
            ));
            assert!(codes.is_empty(), "`{one}` should be clean, got {codes:?}");
        }
        let root_underscore =
            codes_for("[package]\nname = \"my_co.app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n");
        assert_eq!(root_underscore, vec!["W0903"]);
        for bad in ["My.App", "my-app", "com.Example", "0com.app", ".com.app", "com..app"] {
            let codes = codes_for(&format!(
                "[package]\nname = \"{bad}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n"
            ));
            assert_eq!(codes, vec!["E0903"], "`{bad}` should be illegal");
        }
    }

    #[test]
    fn a_non_semver_version_is_an_error() {
        for bad in ["1", "1.2", "1.2.3.4", "01.2.3", "1.2.x", "v1.2.3"] {
            let codes = codes_for(&format!(
                "[package]\nname = \"com.example.app\"\nversion = \"{bad}\"\nedition = \"2026\"\n"
            ));
            assert_eq!(codes, vec!["E0903"], "`{bad}` is not SemVer");
        }
        for good in ["0.0.0", "1.2.3-alpha.1", "2.0.0+build.7", "1.2.3-rc.1+exp.sha.5114f85"] {
            let codes = codes_for(&format!(
                "[package]\nname = \"com.example.app\"\nversion = \"{good}\"\nedition = \"2026\"\n"
            ));
            assert!(codes.is_empty(), "`{good}` is SemVer, got {codes:?}");
        }
    }

    /// A diagnostic has to land on the line the user has to edit, or they get
    /// to search the file for it themselves.
    #[test]
    fn the_diagnostic_points_at_the_offending_key() {
        let dir = std::env::temp_dir().join(format!("jux-manifest-span-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join("jux.toml"),
            "[package]\nname = \"com.example.app\"\nversion = \"0.1.0\"\nedition = \"2015\"\n",
        )
        .expect("write manifest");
        let check = check(&dir).expect("a manifest is present");
        let d = &check.diagnostics[0];
        let (line, col) = check
            .source
            .line_col(d.primary_span.expect("a span").start as usize);
        assert_eq!((line, col), (4, 1), "the `edition` line");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
