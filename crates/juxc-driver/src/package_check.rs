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
//!
//! One more kind of file is not a member of the package tree: a `[[bin]]`
//! entry point located by `path` (§B.2.2). §B.15.2's canonical multi-binary
//! tree puts them in `src/bin/`, so deriving a package from the directory told
//! `src/bin/server.jux` to declare `package bin;` and the shape did not build.
//! The manifest already says where such a file is, so it needs no package to be
//! found, and it MAY be package-less wherever it sits (see
//! [`check_package_paths`] for what still holds).

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
///
/// `bin_entries` are the build's `[[bin]] path = "…"` entry files (from
/// [`crate::CfgFacts::bin_entries`]). Such a file is a program's entry point,
/// not a member of the package tree, so it is excused from having to declare
/// the package its directory implies — `src/bin/server.jux` of §B.15.2 is
/// package-less, not `package bin;`. Only that one requirement is lifted: a
/// `package` line the entry file DOES write is still checked against its
/// directory, because an entry that opts into a package is a member of it and
/// nothing else would notice the two disagreeing.
pub fn check_package_paths(
    units: &[CompilationUnit],
    sources: &[SourceFile],
    bin_entries: &[std::path::PathBuf],
) -> Vec<Diagnostic> {
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
                if !expected.is_empty() && !is_bin_entry(source.path(), bin_entries) {
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

/// True when `path` is one of the build's `[[bin]]` entry files.
///
/// Paths are compared by normalized [`Path::components`] so a `/`-vs-`\`
/// separator difference cannot defeat the match: the manifest builds its entry
/// path with `join("src/bin/server.jux")` while the source walker arrives at
/// the same file as `join("src").join("bin").join("server.jux")`.
fn is_bin_entry(path: &Path, bin_entries: &[std::path::PathBuf]) -> bool {
    bin_entries.iter().any(|e| e.components().eq(path.components()))
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


/// §3.1: a file declares **at most one public type**, and that type's name is
/// the file's name (`Animal.jux` holds `public class Animal`). A file that
/// declares no public type — a file of functions, or one whose types are all
/// package-private — may be named anything.
///
/// The rule is what makes a public type findable from its name alone, and it
/// is checked here for the same reason the package rule is: the alternative is
/// a reader searching the tree for where a name lives.
pub fn check_public_type_file_names(units: &[CompilationUnit], sources: &[SourceFile]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (idx, (unit, source)) in units.iter().zip(sources.iter()).enumerate() {
        if unit.is_external {
            continue;
        }
        let path = source.path();
        // Generated units (the annotation registry, the stdlib snapshot) are
        // not files a person names.
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().and_then(|e| e.to_str()) != Some("jux") {
            continue;
        }
        // A NESTED type is lifted to a top-level declaration named
        // `Outer__Inner` by the parser (§M.9). It is not a type the file
        // declares in its own right, and its name could never match a filename.
        let lifted = |name: &str| name.contains("__");
        let publics: Vec<(String, Span)> = unit
            .items
            .iter()
            .filter_map(|item| match item {
                juxc_ast::TopLevelDecl::Class(c)
                    if matches!(c.visibility, juxc_ast::Visibility::Public) && !lifted(&c.name.text) =>
                {
                    Some((c.name.text.clone(), c.name.span))
                }
                juxc_ast::TopLevelDecl::Interface(i)
                    if matches!(i.visibility, juxc_ast::Visibility::Public) && !lifted(&i.name.text) =>
                {
                    Some((i.name.text.clone(), i.name.span))
                }
                juxc_ast::TopLevelDecl::Record(r)
                    if matches!(r.visibility, juxc_ast::Visibility::Public) && !lifted(&r.name.text) =>
                {
                    Some((r.name.text.clone(), r.name.span))
                }
                juxc_ast::TopLevelDecl::Enum(e)
                    if matches!(e.visibility, juxc_ast::Visibility::Public) && !lifted(&e.name.text) =>
                {
                    Some((e.name.text.clone(), e.name.span))
                }
                _ => None,
            })
            .collect();
        if publics.is_empty() {
            continue;
        }
        if publics.len() > 1 {
            let names = publics.iter().map(|(n, _)| format!("`{n}`")).collect::<Vec<_>>().join(", ");
            out.push(
                Diagnostic::error(
                    Code::E0481_PublicTypeFileName,
                    format!(
                        "a file declares at most one public type, and this one declares {}: {names} (§3.1)",
                        publics.len(),
                    ),
                )
                .with_span(publics[1].1)
                .with_file(idx)
                .with_help("keep one public type and give it this file's name; the others drop `public` (package-private) or move to their own files"),
            );
            continue;
        }
        let (name, span) = &publics[0];
        if name != stem {
            out.push(
                Diagnostic::error(
                    Code::E0481_PublicTypeFileName,
                    format!("public type `{name}` is declared in `{stem}.jux`: a public type lives in the file named after it (§3.1)"),
                )
                .with_span(*span)
                .with_file(idx)
                .with_help(format!("rename the file to `{name}.jux`, or drop `public` to make `{name}` package-private")),
            );
        }
    }
    out
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

    /// One parsed unit for `path`, as the driver would hand it over.
    fn unit_at(path: &str, src: &str) -> (CompilationUnit, SourceFile) {
        let file = SourceFile::new(PathBuf::from(path), src.to_string());
        let unit = juxc_parse::parse(&juxc_lex::lex(&file).tokens).ast;
        (unit, file)
    }

    /// §B.15.2's `src/bin/server.jux` is a declared `[[bin]]` entry, so it is a
    /// program's entry point and not a member of a package called `bin`. Before
    /// the entry list was threaded in, the canonical multi-binary tree was told
    /// to write `package bin;` and would not build.
    #[test]
    fn a_declared_bin_entry_may_be_package_less() {
        let entry = PathBuf::from("/p/src/bin/server.jux");
        let (unit, source) = unit_at("/p/src/bin/server.jux", "public void main(){}");
        let units = [unit];
        let sources = [source];

        // Not a declared entry: the §B.1.1 rule applies as it always did.
        let diags = check_package_paths(&units, &sources, &[]);
        assert_eq!(diags.len(), 1, "an ordinary file below `src/` needs its package");
        assert!(diags[0].message.contains("package bin;"), "{}", diags[0].message);

        // Declared as a bin entry: no package required.
        assert!(check_package_paths(&units, &sources, &[entry]).is_empty());
    }

    /// Only the requirement is lifted. An entry file that DOES declare a
    /// package has opted into it, so the declaration is still checked against
    /// the directory -- nothing else would notice the two disagreeing.
    #[test]
    fn a_bin_entry_with_a_mismatched_package_is_still_an_error() {
        let entry = PathBuf::from("/p/src/bin/server.jux");
        let (unit, source) =
            unit_at("/p/src/bin/server.jux", "package other;\npublic void main(){}");
        let diags = check_package_paths(&[unit], &[source], &[entry]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("expected `bin`"), "{}", diags[0].message);
    }

    /// The exemption is per file, not per directory: a second file sitting
    /// beside an entry in `src/bin/` is ordinary package code.
    #[test]
    fn a_non_entry_beside_an_entry_still_needs_its_package() {
        let entry = PathBuf::from("/p/src/bin/server.jux");
        let (unit, source) =
            unit_at("/p/src/bin/Helper.jux", "public class Helper { public int two(){ return 2; } }");
        let diags = check_package_paths(&[unit], &[source], &[entry]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("package bin;"), "{}", diags[0].message);
    }

    /// A separator difference must not defeat the match: the manifest builds
    /// its entry path from the TOML string `"src/bin/server.jux"` while the
    /// source walker arrives at the same file segment by segment.
    #[test]
    fn entry_paths_match_across_separator_spellings() {
        let entry = PathBuf::from("/p").join("src/bin/server.jux");
        let walked = PathBuf::from("/p").join("src").join("bin").join("server.jux");
        assert!(is_bin_entry(&walked, &[entry]));
    }
}