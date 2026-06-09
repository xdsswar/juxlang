//! Package/path consistency checks (JUX-BUILD-SYSTEM-ADDENDUM §B.1).
//!
//! A `.jux` file's package identity is **derived from its path under the source
//! root** (`src/`): `src/xss/it/other/Other.jux` belongs to package
//! `xss.it.other`. Per §B.1:
//!
//! - A file in a sub-directory of the source root MUST declare a `package` that
//!   matches its location.
//! - A file directly in the source root is package-less (conventionally
//!   `main.jux`); it must NOT declare a package.
//! - Any disagreement (missing, mismatched, or a stray package on a root file)
//!   fails the build with `E0301`.
//!
//! Catching this here — before codegen — is what stops a stale layout from
//! leaking to `rustc`: without it, a file in `…/other/` that declares
//! `package xss.it;` lowers `Other` into module `xss::it` while a consumer's
//! `import xss.it.other.Other;` emits `use crate::xss::it::other::Other;`, and
//! the mismatch surfaces as a cryptic `rustc` `E0432` instead of a precise Jux
//! diagnostic pointing at the offending `package` line.
//!
//! The rule only governs files that actually live under a `src/` source root.
//! Loose files compiled directly (`juxc foo.jux`), the auto-loaded `jux.std`
//! sources, and `.jux.d` declaration stubs have no `src/` ancestor (or are
//! flagged external) and are skipped — their package identity comes from their
//! declaration alone.

use std::path::Path;

use juxc_ast::CompilationUnit;
use juxc_diagnostics::{code::Code, Diagnostic};
use juxc_source::{SourceFile, Span};

/// Validate every non-external unit's `package` declaration against its file
/// path, returning file-index-tagged `E0301` diagnostics for any mismatch.
///
/// `units[i]` must correspond to `sources[i]` (the driver builds them in
/// lock-step), so the unit's index is also its `sources` index and its
/// diagnostic `file` tag.
pub fn check_package_paths(units: &[CompilationUnit], sources: &[SourceFile]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (idx, (unit, source)) in units.iter().zip(sources.iter()).enumerate() {
        // Stubs are trusted, signature-only views — never validated.
        if unit.is_external {
            continue;
        }
        // Only files under a `src/` source root are governed by the layout rule.
        let Some(expected) = expected_package(source.path()) else {
            continue;
        };
        let expected_pkg = expected.join(".");

        match unit.package.as_ref() {
            // No `package` declaration.
            None => {
                if !expected.is_empty() {
                    // A sub-directory file must declare its package.
                    out.push(
                        Diagnostic::error(
                            Code::E0301_NameNotFound,
                            format!(
                                "missing `package` declaration: this file is in `{}`, \
                                 so it must declare `package {expected_pkg};`",
                                rel_dir_display(&expected),
                            ),
                        )
                        .with_span(header_span(unit))
                        .with_file(idx)
                        .with_help(format!("add `package {expected_pkg};` at the top of the file")),
                    );
                }
                // expected empty + no declaration → a package-less root file. OK.
            }
            // A `package` declaration is present.
            Some(decl) => {
                let declared = qualified_text(&decl.name);
                if expected.is_empty() {
                    // A root file must be package-less.
                    out.push(
                        Diagnostic::error(
                            Code::E0301_NameNotFound,
                            format!(
                                "file is at the source root and must be package-less, \
                                 but declares `package {declared};`"
                            ),
                        )
                        .with_span(decl.name.span)
                        .with_file(idx)
                        .with_help(format!(
                            "remove the `package` line, or move the file into `{}/`",
                            declared.replace('.', "/"),
                        )),
                    );
                } else if declared != expected_pkg {
                    // Declared package disagrees with the directory layout.
                    out.push(
                        Diagnostic::error(
                            Code::E0301_NameNotFound,
                            format!(
                                "package `{declared}` does not match the file's location: \
                                 expected `{expected_pkg}` (file is in `{}`)",
                                rel_dir_display(&expected),
                            ),
                        )
                        .with_span(decl.name.span)
                        .with_file(idx)
                        .with_help(format!(
                            "change the declaration to `package {expected_pkg};`, \
                             or move the file to match `{declared}`",
                        )),
                    );
                }
                // declared == expected → OK.
            }
        }
    }
    out
}

/// The package a file at `path` must belong to, derived from its location under
/// the nearest `src/` source root:
///
/// - `Some(vec![])` — the file sits directly in `src/` (package-less root).
/// - `Some(vec!["xss", "it", "other"])` — `src/xss/it/other/<file>.jux`.
/// - `None` — `path` has no `src/` ancestor, so the layout rule doesn't apply.
///
/// The **deepest** `src` ancestor wins (the most specific source root), so a
/// nested `…/src/app/src/…` resolves against the inner root.
fn expected_package(path: &Path) -> Option<Vec<String>> {
    let comps: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    if comps.len() < 2 {
        return None;
    }
    // Index of the last `src` component that still leaves room for the file
    // name after it (so `src` itself can't be the file).
    let src_idx = comps[..comps.len() - 1]
        .iter()
        .rposition(|c| *c == "src")?;
    // Segments between `src` and the file name are the package path.
    let segments = &comps[src_idx + 1..comps.len() - 1];
    Some(segments.iter().map(|s| s.to_string()).collect())
}

/// A display form of the package's relative directory: `xss/it/other`.
fn rel_dir_display(segments: &[String]) -> String {
    segments.join("/")
}

/// Dotted text of a qualified name: `["xss","it"]` → `"xss.it"`.
fn qualified_text(name: &juxc_ast::QualifiedName) -> String {
    name.segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

/// A small span at the very start of a unit, used to anchor a "missing package"
/// diagnostic (which has no declaration span to point at).
fn header_span(unit: &CompilationUnit) -> Span {
    Span::new(unit.span.start, unit.span.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn package_derived_from_src_relative_path() {
        let p = PathBuf::from("F:/proj/src/xss/it/other/Other.jux");
        assert_eq!(expected_package(&p), Some(vec!["xss".into(), "it".into(), "other".into()]));
    }

    #[test]
    fn file_directly_in_src_is_package_less() {
        let p = PathBuf::from("/proj/src/main.jux");
        assert_eq!(expected_package(&p), Some(vec![]));
    }

    #[test]
    fn file_without_src_ancestor_is_unconstrained() {
        let p = PathBuf::from("/somewhere/examples/hello.jux");
        assert_eq!(expected_package(&p), None);
    }

    #[test]
    fn deepest_src_root_wins() {
        let p = PathBuf::from("/a/src/outer/src/pkg/File.jux");
        assert_eq!(expected_package(&p), Some(vec!["pkg".into()]));
    }
}
