//! Lint levels: `[lints]` in `jux.toml`, `@lint(...)` on a declaration, and
//! `-Werror` (JUX-DIAGNOSTICS-ADDENDUM §D.5.4).
//!
//! A warning is reported at the level its configuration gives it. `allow`
//! drops it, `warn` keeps it, `deny` turns it into an error that fails the
//! build, and `warnings-as-errors` (or `-Werror` / `--deny-warnings` on the
//! command line) turns every warning that is still a warning into one too.
//!
//! The level is decided in ONE place, [`apply`], which every compile and check
//! entry point runs after the last diagnostic is produced and before anything
//! asks whether there were errors. That ordering is the whole contract: a
//! setting applied after the has-errors decision would print `error:` and still
//! build, and a setting applied in only some entry points would make `jux
//! check` and `jux build` disagree about the same program.
//!
//! A promoted warning keeps its `W` code, the way rustc keeps a lint's name
//! under `#[deny]`, and says in a note which setting promoted it: an error that
//! the author did not know they had asked for has to say where it came from.
//!
//! ## Precedence
//!
//! For one warning, innermost first: an `@lint(...)` on a declaration that
//! encloses it, then the per-lint key in the file's package's `[lints]`, then
//! that table's `all`, then `warn`. `warnings-as-errors` and `-Werror` apply
//! last, to what is still a warning.
//!
//! Each package's `[lints]` governs that package's own files, so a path
//! dependency is checked by its own table rather than its consumer's. A file
//! under no package (a loose `jux run foo.jux`) has the defaults. `-Werror`
//! governs the whole build.

use std::collections::BTreeMap;

use juxc_ast::{Annotation, AnnotationArg, CompilationUnit, Expr, Literal, TopLevelDecl};
use juxc_diagnostics::{code::Code, Diagnostic, Severity};
use juxc_source::{SourceFile, Span};

/// A lint's level (§D.5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintLevel {
    /// Silent: the warning is not reported.
    Allow,
    /// Reported as a warning. The default.
    Warn,
    /// Reported as an error, which fails the build.
    Deny,
}

impl LintLevel {
    /// The level a `[lints]` value or an `@lint` key spells, if it spells one.
    pub fn parse(text: &str) -> Option<LintLevel> {
        match text {
            "allow" => Some(LintLevel::Allow),
            "warn" => Some(LintLevel::Warn),
            "deny" => Some(LintLevel::Deny),
            _ => None,
        }
    }

    fn word(self) -> &'static str {
        match self {
            LintLevel::Allow => "allow",
            LintLevel::Warn => "warn",
            LintLevel::Deny => "deny",
        }
    }
}

/// The lint names §D.5.4 defines, and the warning code each one reports as.
///
/// A name with no code is specified but not raised by this compiler yet. It is
/// accepted as a name, so the spec's own example manifest is not refused, and
/// `[lints]` validation says that setting it has no effect (`W0902`).
///
/// Any warning code (`W0820`) is also a lint key in its own right, so a warning
/// with no name here can still be configured.
pub const LINT_NAMES: &[(&str, Option<&str>)] = &[
    ("unsafe-without-justification", Some("W0820")),
    ("unused-import", None),
    ("shadowed-name", None),
];

/// What a lint key (a `[lints]` key or an `@lint` value) refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintKey {
    /// A warning this compiler can raise, by its code.
    Warning(String),
    /// A name §D.5.4 defines that nothing raises yet.
    NotRaised,
    /// An error code: errors have no level.
    Error(String),
    /// Neither a lint name nor a known warning code.
    Unknown,
}

/// Classify one lint key. Codes are case-insensitive and may be written
/// without their leading zeros (`w820`), as `juxc explain` accepts them.
pub fn lint_key(key: &str) -> LintKey {
    if let Some((_, target)) = LINT_NAMES.iter().find(|(name, _)| *name == key) {
        return match target {
            Some(code) => LintKey::Warning((*code).to_string()),
            None => LintKey::NotRaised,
        };
    }
    let mut chars = key.chars();
    let letter = chars.next().map(|c| c.to_ascii_uppercase());
    let digits = chars.as_str();
    let looks_like_code = matches!(letter, Some('W' | 'E'))
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit());
    if !looks_like_code {
        return LintKey::Unknown;
    }
    let code = crate::explain::normalize(key);
    if !crate::explain::is_known_code(&code) {
        return LintKey::Unknown;
    }
    if code.starts_with('E') {
        LintKey::Error(code)
    } else {
        LintKey::Warning(code)
    }
}

/// One package's `[lints]` table, as the build reads it.
///
/// Parsed leniently: a value that is not a level, or a key that names no lint,
/// is skipped here and reported by the manifest check (`E0903` / `W0902`),
/// which has the key's line to point at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LintConfig {
    /// `warnings-as-errors = true`: every warning still a warning after the
    /// levels are applied fails the build.
    pub warnings_as_errors: bool,
    /// `all = "<level>"`: the level of every warning no key names.
    pub all: Option<LintLevel>,
    /// Per-warning levels, keyed by code (`W0820`), whichever way the key was
    /// written.
    pub levels: BTreeMap<String, LintLevel>,
}

impl LintConfig {
    /// Read a `[lints]` table.
    pub fn from_toml(table: &toml::Value) -> LintConfig {
        let mut out = LintConfig::default();
        let Some(table) = table.as_table() else { return out };
        for (key, value) in table {
            match key.as_str() {
                "warnings-as-errors" => out.warnings_as_errors = value.as_bool().unwrap_or(false),
                "all" => out.all = value.as_str().and_then(LintLevel::parse),
                _ => {
                    if let (LintKey::Warning(code), Some(level)) =
                        (lint_key(key), value.as_str().and_then(LintLevel::parse))
                    {
                        out.levels.insert(code, level);
                    }
                }
            }
        }
        out
    }

    /// Whether this table names `code` explicitly (not through `all`).
    pub fn names(&self, code: &str) -> bool {
        self.levels.contains_key(code)
    }
}

/// One `@lint(...)`: the declaration it covers and the levels it sets.
struct Scope {
    span: Span,
    levels: Vec<(String, LintLevel)>,
}

/// Apply every lint level to `diagnostics`, in place: drop what is allowed,
/// promote what is denied, then promote what `warnings-as-errors` / `-Werror`
/// covers.
///
/// `units` are the parsed units `sources` produced, index for index; they are
/// where `@lint` is read from, and a malformed `@lint` is reported here too.
pub fn apply(
    diagnostics: &mut Vec<Diagnostic>,
    sources: &[SourceFile],
    units: &[CompilationUnit],
    cfg: &crate::cfg::CfgFacts,
) {
    let mut scopes: Vec<Scope> = Vec::new();
    for unit in units.iter().filter(|u| !u.is_external) {
        for_each_declaration(unit, &mut |annotations, span| {
            let levels = read_lint_annotations(annotations, diagnostics);
            if !levels.is_empty() {
                let start = annotations.iter().map(|a| a.span.start).fold(span.start, u32::min);
                scopes.push(Scope { span: Span::in_file(start, span.end, span.file), levels });
            }
        });
    }

    let default = LintConfig::default();
    diagnostics.retain_mut(|d| {
        if d.severity != Severity::Warning {
            return true;
        }
        let code = d.code.as_str();
        let path = d.file.and_then(|f| sources.get(f)).map(|s| s.path());
        let config = path.and_then(|p| cfg.lints_for(p)).unwrap_or(&default);
        // The innermost enclosing `@lint` that names this warning.
        let scoped = d.primary_span.and_then(|at| {
            scopes
                .iter()
                .filter(|s| s.span.file == at.file && s.span.start <= at.start && at.end <= s.span.end)
                .filter_map(|s| {
                    s.levels.iter().rev().find(|(c, _)| c == code).map(|(_, l)| (s.span.len(), *l))
                })
                .min_by_key(|(len, _)| *len)
                .map(|(_, l)| l)
        });
        let (level, why) = match (scoped, config.levels.get(code), config.all) {
            (Some(l), _, _) => (l, format!("an enclosing `@lint({} = ...)` names it", l.word())),
            (None, Some(l), _) => (*l, format!("`[lints]` sets {} to \"{}\"", spelled(code), l.word())),
            (None, None, Some(l)) => (l, format!("`[lints]` sets `all = \"{}\"`", l.word())),
            (None, None, None) => (LintLevel::Warn, String::new()),
        };
        match level {
            LintLevel::Allow => return false,
            LintLevel::Deny => promote(d, &why),
            LintLevel::Warn if config.warnings_as_errors => {
                promote(d, "`[lints]` sets `warnings-as-errors = true`")
            }
            LintLevel::Warn if cfg.deny_warnings() => promote(d, "of `-Werror` / `--deny-warnings`"),
            LintLevel::Warn => {}
        }
        true
    });
}

/// Apply a manifest's own `[lints]` to the diagnostics validating it produced.
/// The manifest is a file of the package it configures, so `W0902` and its
/// siblings are subject to the same table. `@lint` does not exist in TOML.
pub fn apply_to_manifest(diagnostics: &mut Vec<Diagnostic>, config: &LintConfig, deny_warnings: bool) {
    diagnostics.retain_mut(|d| {
        if d.severity != Severity::Warning {
            return true;
        }
        let code = d.code.as_str();
        match config.levels.get(code).copied().or(config.all).unwrap_or(LintLevel::Warn) {
            LintLevel::Allow => return false,
            LintLevel::Deny => promote(d, &format!("`[lints]` denies {}", spelled(code))),
            LintLevel::Warn if config.warnings_as_errors => {
                promote(d, "`[lints]` sets `warnings-as-errors = true`")
            }
            LintLevel::Warn if deny_warnings => promote(d, "of `-Werror` / `--deny-warnings`"),
            LintLevel::Warn => {}
        }
        true
    });
}

/// `-Werror` / `--deny-warnings` as the `jux` front end records it for the
/// build (the same channel `--features` uses).
pub fn deny_warnings_requested() -> bool {
    std::env::var_os("JUX_DENY_WARNINGS").is_some_and(|v| !v.is_empty() && v != "0")
}

fn promote(d: &mut Diagnostic, why: &str) {
    d.severity = Severity::Error;
    d.notes.push(format!("this warning is an error because {why} (§D.5.4)"));
}

/// A warning code as a note names it: with its lint name when it has one, so
/// the note matches what the author wrote in `[lints]`.
fn spelled(code: &str) -> String {
    match LINT_NAMES.iter().find(|(_, c)| *c == Some(code)) {
        Some((name, _)) => format!("`{name}` (`{code}`)"),
        None => format!("`{code}`"),
    }
}

/// Every declaration in `unit` that can carry annotations, with its span:
/// types, functions, methods, constructors, fields, properties and constants.
/// Nested types are already top-level items (the parser lifts them, §M.9).
fn for_each_declaration(unit: &CompilationUnit, f: &mut dyn FnMut(&[Annotation], Span)) {
    for item in &unit.items {
        match item {
            TopLevelDecl::Function(d) => f(&d.annotations, d.span),
            TopLevelDecl::Const(d) => f(&d.annotations, d.span),
            TopLevelDecl::TypeAlias(d) => f(&d.annotations, d.span),
            TopLevelDecl::Class(c) => {
                f(&c.annotations, c.span);
                c.methods.iter().for_each(|m| f(&m.annotations, m.span));
                c.fields.iter().for_each(|m| f(&m.annotations, m.span));
                c.properties.iter().for_each(|m| f(&m.annotations, m.span));
                c.constructors.iter().for_each(|m| f(&m.annotations, m.span));
            }
            TopLevelDecl::Record(r) => {
                f(&r.annotations, r.span);
                r.methods.iter().for_each(|m| f(&m.annotations, m.span));
                r.compact_ctor.iter().chain(r.constructors.iter()).for_each(|m| f(&m.annotations, m.span));
            }
            TopLevelDecl::Enum(e) => {
                f(&e.annotations, e.span);
                e.methods.iter().for_each(|m| f(&m.annotations, m.span));
                e.fields.iter().for_each(|m| f(&m.annotations, m.span));
                e.constructors.iter().for_each(|m| f(&m.annotations, m.span));
            }
            TopLevelDecl::Interface(i) => {
                f(&i.annotations, i.span);
                i.methods.iter().for_each(|m| f(&m.annotations, m.span));
            }
            TopLevelDecl::Annotation(_) | TopLevelDecl::ExternBlock(_) => {}
        }
    }
}

/// The levels the `@lint(...)` annotations in `annotations` set, by code, in
/// the order written. A malformed one is reported and contributes nothing.
///
/// `@lint` takes `allow`, `warn` and `deny`, each a lint key or an array of
/// them: `@lint(allow = "unsafe-without-justification")`,
/// `@lint(deny = {"W0457", "W0470"})`.
fn read_lint_annotations(
    annotations: &[Annotation],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(String, LintLevel)> {
    let mut out = Vec::new();
    for a in annotations {
        let is_lint = a.name.segments.len() == 1 && a.name.segments[0].text.eq_ignore_ascii_case("lint");
        if !is_lint {
            continue;
        }
        let file = a.span.file as usize;
        for arg in &a.args {
            let (level, value, at) = match arg {
                AnnotationArg::Named { name, value } => match LintLevel::parse(&name.text) {
                    Some(level) => (level, value, name.span),
                    None => {
                        diagnostics.push(
                            Diagnostic::error(
                                Code::E0448_BadNamedArgument,
                                format!(
                                    "`@lint` has no parameter `{}`; it declares `allow`, `warn`, `deny`",
                                    name.text
                                ),
                            )
                            .with_span(name.span)
                            .with_file(file),
                        );
                        continue;
                    }
                },
                AnnotationArg::Positional(_) => {
                    diagnostics.push(
                        Diagnostic::error(
                            Code::E0472_MissingAnnotationParameter,
                            "`@lint` takes a level: `@lint(allow = \"<lint>\")`, or `warn` / `deny`",
                        )
                        .with_span(a.span)
                        .with_file(file),
                    );
                    continue;
                }
            };
            let keys: Vec<&Expr> = match value {
                Expr::NewArrayLit(lit) => lit.elements.iter().collect(),
                other => vec![other],
            };
            for key in keys {
                let Expr::Literal(Literal::String(text)) = key else {
                    diagnostics.push(
                        Diagnostic::error(
                            Code::E0474_AnnotationParameterType,
                            format!("`@lint({} = ...)` takes a lint name or code as a string", level.word()),
                        )
                        .with_span(at)
                        .with_file(file),
                    );
                    continue;
                };
                match lint_key(text) {
                    LintKey::Warning(code) => out.push((code, level)),
                    // Specified, not raised: nothing to set, and nothing wrong.
                    LintKey::NotRaised => {}
                    LintKey::Error(code) => diagnostics.push(
                        Diagnostic::error(
                            Code::E0474_AnnotationParameterType,
                            format!("`{code}` is an error, and an error has no lint level"),
                        )
                        .with_span(at)
                        .with_file(file),
                    ),
                    LintKey::Unknown => diagnostics.push(
                        Diagnostic::warning(
                            Code::W0242_UnknownLint,
                            format!("`@lint` names `{text}`, which is neither a lint nor a warning code"),
                        )
                        .with_span(at)
                        .with_file(file)
                        .with_help(lint_names_help()),
                    ),
                }
            }
        }
    }
    out
}

/// The `help:` line listing what a lint key can be.
pub(crate) fn lint_names_help() -> String {
    let names: Vec<String> = LINT_NAMES.iter().map(|(n, _)| format!("`{n}`")).collect();
    format!("a lint key is one of {}, or a warning code such as `W0820`", names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_key_is_a_name_or_a_code() {
        assert_eq!(lint_key("unsafe-without-justification"), LintKey::Warning("W0820".into()));
        assert_eq!(lint_key("W0820"), LintKey::Warning("W0820".into()));
        assert_eq!(lint_key("w820"), LintKey::Warning("W0820".into()));
        assert_eq!(lint_key("unused-import"), LintKey::NotRaised);
        assert_eq!(lint_key("E0410"), LintKey::Error("E0410".into()));
        assert_eq!(lint_key("W9999"), LintKey::Unknown);
        assert_eq!(lint_key("nonsense"), LintKey::Unknown);
        assert_eq!(lint_key("é"), LintKey::Unknown);
    }

    #[test]
    fn a_table_reads_leniently() {
        let table: toml::Value = toml::from_str(
            "warnings-as-errors = true\nall = \"allow\"\nunsafe-without-justification = \"deny\"\n\
             W0470 = \"loud\"\nnonsense = \"deny\"\n",
        )
        .unwrap();
        let c = LintConfig::from_toml(&table);
        assert!(c.warnings_as_errors);
        assert_eq!(c.all, Some(LintLevel::Allow));
        assert_eq!(c.levels.get("W0820"), Some(&LintLevel::Deny));
        // Not a level, and not a lint: skipped here, reported by the manifest check.
        assert_eq!(c.levels.len(), 1);
    }

    /// The codes and severities `check_workspace_cfg` reports for `src` under
    /// a `[lints]` table given as TOML.
    fn check_with(lints: &str, src: &str, deny_warnings: bool) -> Vec<(String, Severity)> {
        let config = LintConfig::from_toml(&toml::from_str(lints).unwrap());
        let facts = crate::cfg::CfgFacts::default()
            .with_package_lints(PathBuf::from(""), config)
            .with_deny_warnings(deny_warnings);
        let source = SourceFile::new(PathBuf::from("app.jux"), src.to_string());
        crate::check_workspace_cfg(vec![source], &facts)
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str().to_string(), d.severity))
            .collect()
    }

    const UNSAFE: &str = "public void main() {\n    unsafe {\n        print(1);\n    }\n}\n";

    #[test]
    fn levels_allow_deny_and_promote() {
        assert_eq!(check_with("", UNSAFE, false), [("W0820".into(), Severity::Warning)]);
        assert!(check_with("W0820 = \"allow\"", UNSAFE, false).is_empty());
        assert!(check_with("all = \"allow\"", UNSAFE, false).is_empty());
        assert_eq!(
            check_with("unsafe-without-justification = \"deny\"", UNSAFE, false),
            [("W0820".into(), Severity::Error)]
        );
        assert_eq!(check_with("warnings-as-errors = true", UNSAFE, false), [("W0820".into(), Severity::Error)]);
        assert_eq!(check_with("", UNSAFE, true), [("W0820".into(), Severity::Error)]);
        // A key beats `all`, and `allow` is not overridden by `-Werror`.
        assert!(check_with("all = \"deny\"\nW0820 = \"allow\"", UNSAFE, true).is_empty());
    }

    #[test]
    fn an_enclosing_lint_annotation_wins() {
        let src = "@lint(allow = \"unsafe-without-justification\")\npublic void main() {\n    unsafe {\n        print(1);\n    }\n}\n";
        assert!(check_with("W0820 = \"deny\"", src, false).is_empty());
        let deny = "@lint(deny = {\"W0820\"})\npublic void main() {\n    unsafe {\n        print(1);\n    }\n}\n";
        assert_eq!(check_with("", deny, false), [("W0820".into(), Severity::Error)]);
    }
}
