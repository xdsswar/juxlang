//! Export of the machine-readable grammar spec the IntelliJ plugin is built
//! from — `ide/intellij-plugin/grammar/jux-tokens.json`.
//!
//! The token alphabet itself comes from [`juxc_lex::grammar_spec`]. This module
//! adds the one piece that crate cannot see: the names the embedded `jux.std`
//! sources DECLARE. The compiler prepends those sources to every unit, so
//! `Option`, `Result`, `Instant`, `AtomicInt` and the rest are in scope with no
//! `import` exactly as `Vec` and `print` are — the difference is only where the
//! binding comes from, and no editor should have to know that.
//!
//! Why generate it at all: the plugin used to keep its own hand-written copy of
//! "what resolves with no import", and it drifted in both directions at once —
//! it still required an `import` for `Vec` (so it painted "cannot resolve type"
//! over twenty examples that build) while accepting `println` and `panic`
//! (which are Rust macros, not Jux names, so it stayed silent where the
//! compiler errors). Deriving the list from the compiler is what makes those
//! two failure modes impossible rather than merely fixed.
//!
//! ## Regenerating
//!
//! ```text
//! JUX_BLESS=1 cargo test -p juxc-driver grammar_export
//! ```
//!
//! Without `JUX_BLESS` the test asserts the checked-in file matches, so CI
//! fails if the token list or the stdlib surface changes without the JSON being
//! regenerated.

use juxc_lex::grammar_spec::{grammar_spec_with, GrammarSpec};

/// Public top-level type names declared by the embedded `jux.std` sources, in
/// sorted order.
///
/// Read straight off the source text the compiler ships: every `public
/// class|interface|record|enum|struct <Name>` at the start of a line. A regex
/// would be wrong here for the usual reasons, but the stdlib is generated,
/// uniformly formatted Jux and the declarations it exports are exactly the
/// lines that begin at column zero — so a line scan is both sufficient and
/// impossible to get subtly wrong.
pub fn embedded_stdlib_type_names() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (_, source) in crate::stdlib_embedded::STDLIB_SOURCES {
        for line in source.lines() {
            // Top-level declarations only: nested types are indented.
            let Some(rest) = line.strip_prefix("public ") else { continue };
            let mut words = rest.split_whitespace();
            let Some(kind) = words.next() else { continue };
            if !matches!(kind, "class" | "interface" | "record" | "enum" | "struct") {
                continue;
            }
            let Some(raw) = words.next() else { continue };
            // Strip whatever follows the name on the same line: generic
            // parameters, an `extends`/`implements` clause, or the body brace.
            let name: String = raw
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                names.push(name);
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The complete spec the plugin consumes: the token alphabet plus every name
/// that is in scope without an `import`.
pub fn full_grammar_spec() -> GrammarSpec {
    grammar_spec_with(&embedded_stdlib_type_names())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Absolute path to the checked-in `jux-tokens.json` the plugin reads.
    fn json_path() -> PathBuf {
        // CARGO_MANIFEST_DIR = .../crates/juxc-driver ; repo root is two up.
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../ide/intellij-plugin/grammar/jux-tokens.json")
    }

    /// The stdlib scan must actually find the types it exists to find —
    /// otherwise a silent zero would look exactly like "the stdlib declares
    /// nothing" and quietly reintroduce the drift.
    #[test]
    fn stdlib_scan_finds_the_declared_types() {
        let names = embedded_stdlib_type_names();
        for expected in ["Option", "Result", "Instant", "AtomicInt", "Collection"] {
            assert!(
                names.iter().any(|n| n == expected),
                "embedded stdlib scan missed `{expected}`: {names:?}",
            );
        }
    }

    /// The checked-in `jux-tokens.json` must equal the freshly built spec.
    /// Run with `JUX_BLESS=1` to regenerate the file instead of asserting.
    #[test]
    fn grammar_export_matches_checked_in_json() {
        let expected = juxc_lex::grammar_spec::to_json(&full_grammar_spec());
        let path = json_path();

        if std::env::var_os("JUX_BLESS").is_some() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).expect("create grammar dir");
            }
            std::fs::write(&path, &expected).expect("write jux-tokens.json");
            return;
        }

        let actual = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        assert_eq!(
            actual.replace("\r\n", "\n"),
            expected,
            "{} is stale — regenerate with `JUX_BLESS=1 cargo test -p juxc-driver grammar_export`",
            path.display(),
        );
    }
}
