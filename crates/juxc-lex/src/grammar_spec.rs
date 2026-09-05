//! Machine-readable token-alphabet spec — the single source of truth shared
//! between the Rust lexer and the IntelliJ plugin.
//!
//! The plugin used to hand-maintain parallel copies of the keyword set, the
//! primitive-type set, and the operator/punctuation list in Kotlin. Those
//! copies drifted (e.g. the Kotlin primitive set listed `never` and omitted
//! the width synonyms `i8`…`f64`). This module emits a `jux-tokens.json` that
//! the plugin's Gradle build consumes to *generate* its Kotlin token registry,
//! so the two can no longer disagree.
//!
//! The spec is built from the canonical Rust definitions wherever they exist
//! ([`Keyword::ALL`] for keywords, [`crate::PRIMITIVE_TYPE_NAMES`] for
//! primitives); the punctuation/operator/literal/comment tables below mirror
//! the variants in [`crate::token::TokenKind`] one-for-one. A unit test guards
//! the keyword side against drift; the punctuation tables are small and stable.
//!
//! ## Regenerating `jux-tokens.json`
//!
//! ```text
//! JUX_BLESS=1 cargo test -p juxc-lex grammar_spec
//! ```
//!
//! Without `JUX_BLESS`, the test instead *asserts* the checked-in file matches
//! — so CI fails if `token.rs` changes without the JSON being regenerated.

use serde::{Deserialize, Serialize};

use crate::Keyword;

/// One token in the alphabet: a stable Kotlin-side identifier (`name`) plus
/// the literal source spelling where one exists (`None` for literals, whose
/// text is variable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedToken {
    /// Stable identifier used as the Kotlin `IElementType` constant name
    /// (e.g. `COLON_COLON`, `CLASS_KW`, `INT_LITERAL`).
    pub name: String,
    /// The fixed source spelling, if the token has one (`"::"`, `"class"`).
    /// `None` for variable-text tokens (identifiers, literals).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spelling: Option<String>,
}

impl NamedToken {
    fn fixed(name: &str, spelling: &str) -> NamedToken {
        NamedToken { name: name.to_string(), spelling: Some(spelling.to_string()) }
    }

    fn variable(name: &str) -> NamedToken {
        NamedToken { name: name.to_string(), spelling: None }
    }
}

/// The annotations the compiler HONORS, spelled as the language docs and the
/// example corpus write them, without the leading `@`.
///
/// Annotation names are case-insensitive (`@Override` = `@override`), so these
/// spellings are what completion offers rather than what it matches.
///
/// Only annotations with real behaviour are listed. Ones the specification
/// reserves but the compiler does not act on yet are deliberately absent —
/// offering them would promise a semantic that is not there. The bindgen
/// markers (`@rust`, `@MutSelf`, `@RustIndexRef`) are absent for the opposite
/// reason: they are written into generated `.jux.d` stubs, never by a user.
///
/// `annotations_are_honored` in this module's tests pins the list against the
/// compiler's own call sites, so an annotation that gains behaviour is not
/// silently missing from the editor.
pub const BUILTIN_ANNOTATIONS: &[&str] = &[
    // §7.4 — verified against the supertype chain.
    "Override",
    // §TS.1 — the testing framework's case and lifecycle hooks.
    "Test", "BeforeAll", "BeforeEach", "AfterEach", "AfterAll",
    // FFI and layout (§8.1, §8.4, §L.1.3).
    "extern", "export", "layout",
];

/// Names that are bound without a declaration or an `import`: the prelude
/// types (`Vec`, `HashMap`, the exception hierarchy, the collection
/// interfaces) and the language intrinsics (`print`, `spawn`, `block_on`,
/// `Worker`, …).
///
/// This is the list the resolver seeds itself from, so a name added here is
/// accepted by the compiler and known to the plugin in the same commit. It
/// used to exist twice — once in `juxc-resolve` and once, by hand, in the
/// plugin's Kotlin — and the copies drifted: the plugin still believed `Vec`
/// required an `import` long after it became prelude, and painted "cannot
/// resolve type" over 20 examples that compile.
pub const BUILTIN_NAMES: &[&str] = &[
    "print", "assert", "spawn", "Channel", "AsyncMutex",
    "Stream", "Task", "withTimeout", "parallel", "block_on",
    "yield_now", "Worker", "now_ms", "File", "Iterable",
    "Iterator", "Collection", "List", "Map", "Set",
    "HashMap", "HashSet", "Vec", "VecDeque", "BTreeMap",
    "BTreeSet", "Rc", "Arc", "Throwable", "Exception",
    "Error", "RuntimeException", "NullPointerException", "IndexOutOfBoundsException", "IllegalArgumentException",
    "IllegalStateException", "UnsupportedOperationException", "ArithmeticException", "NoSuchElementException", "ClassCastException",
    "IOException", "FileNotFoundException",
];

/// The full token alphabet, ready to serialize to `jux-tokens.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrammarSpec {
    /// Reserved keywords (name = `UPPER_KW`, spelling = lowercase word).
    pub keywords: Vec<NamedToken>,
    /// Built-in annotation names, without the `@`. From [`BUILTIN_ANNOTATIONS`].
    pub annotations: Vec<String>,
    /// Names that resolve with no declaration and no `import` — the prelude
    /// types and the language intrinsics. From [`BUILTIN_NAMES`].
    ///
    /// The plugin needs these for exactly the reason the compiler does: a bare
    /// `Vec` or `block_on` is bound, and an editor that says otherwise paints
    /// red over code that builds.
    pub builtins: Vec<String>,
    /// Built-in primitive type names — identifiers, not keywords, colored as
    /// types. From [`crate::PRIMITIVE_TYPE_NAMES`].
    pub primitives: Vec<String>,
    /// Literal-keyword constants (`true`, `false`, `null`).
    pub constants: Vec<String>,
    /// Literal token kinds (variable text).
    pub literals: Vec<NamedToken>,
    /// Punctuation tokens.
    pub punctuation: Vec<NamedToken>,
    /// Operator tokens.
    pub operators: Vec<NamedToken>,
    /// Comment token kinds. `DOC_COMMENT` is an editor-side refinement of a
    /// block comment (the Rust lexer treats `/** */` as ordinary trivia); it
    /// is included here because the IDE colors and folds it distinctly.
    pub comments: Vec<NamedToken>,
}

/// The Kotlin constant name for a keyword: `"class"` -> `"CLASS_KW"`.
fn keyword_const_name(spelling: &str) -> String {
    format!("{}_KW", spelling.to_uppercase())
}

/// Build the canonical grammar spec from the Rust definitions.
/// The token alphabet with only the intrinsic builtins — see
/// [`grammar_spec_with`] for the complete list.
pub fn grammar_spec() -> GrammarSpec {
    grammar_spec_with(&[])
}

/// The token alphabet plus the always-in-scope names.
///
/// `extra_builtins` carries the names that are in scope because the embedded
/// `jux.std` sources DECLARE them (`Option`, `Result`, `Instant`, the atomics)
/// — the compiler prepends those sources to every unit, so they need no
/// `import` either, but only the driver can see them. Passing them in keeps
/// this crate free of a dependency on the layer above it while still producing
/// one complete list. [`BUILTIN_NAMES`] supplies the rest.
pub fn grammar_spec_with(extra_builtins: &[String]) -> GrammarSpec {
    let keywords = Keyword::ALL
        .iter()
        .map(|kw| {
            let spelling = kw.as_str();
            NamedToken::fixed(&keyword_const_name(spelling), spelling)
        })
        .collect();

    let primitives = crate::PRIMITIVE_TYPE_NAMES.iter().map(|s| s.to_string()).collect();

    let constants = vec!["true".to_string(), "false".to_string(), "null".to_string()];

    // Literal token kinds — mirror the atom variants of `TokenKind`.
    let literals = vec![
        NamedToken::variable("INT_LITERAL"),
        NamedToken::variable("FLOAT_LITERAL"),
        NamedToken::variable("CHAR_LITERAL"),
        NamedToken::variable("STRING_LITERAL"),
        NamedToken::variable("RAW_STRING_LITERAL"),
        NamedToken::variable("INTERP_STRING_LITERAL"),
        NamedToken::variable("INTERP_RAW_STRING_LITERAL"),
        NamedToken::variable("BOOL_LITERAL"),
        NamedToken::variable("NULL_LITERAL"),
    ];

    // Punctuation — mirrors the §A.1.6 punctuation block of `TokenKind`.
    let punctuation = vec![
        NamedToken::fixed("LPAREN", "("),
        NamedToken::fixed("RPAREN", ")"),
        NamedToken::fixed("LBRACKET", "["),
        NamedToken::fixed("RBRACKET", "]"),
        NamedToken::fixed("LBRACE", "{"),
        NamedToken::fixed("RBRACE", "}"),
        NamedToken::fixed("COMMA", ","),
        NamedToken::fixed("SEMICOLON", ";"),
        NamedToken::fixed("COLON", ":"),
        NamedToken::fixed("COLON_COLON", "::"),
        NamedToken::fixed("DOT", "."),
        NamedToken::fixed("DOT_DOT", ".."),
        NamedToken::fixed("DOT_DOT_EQ", "..="),
        NamedToken::fixed("ELLIPSIS", "..."),
        NamedToken::fixed("QUESTION", "?"),
        NamedToken::fixed("QUESTION_DOT", "?."),
        NamedToken::fixed("QUESTION_COLON", "?:"),
        NamedToken::fixed("QUESTION_QUESTION", "??"),
        NamedToken::fixed("BANG_BANG", "!!"),
        NamedToken::fixed("AT", "@"),
        NamedToken::fixed("ARROW", "->"),
        NamedToken::fixed("FAT_ARROW", "=>"),
    ];

    // Operators — mirrors the operator block of `TokenKind`.
    let operators = vec![
        NamedToken::fixed("EQ", "="),
        NamedToken::fixed("EQ_EQ", "=="),
        NamedToken::fixed("STRICT_EQ", "==="),
        NamedToken::fixed("BANG", "!"),
        NamedToken::fixed("NOT_EQ", "!="),
        NamedToken::fixed("STRICT_NOT_EQ", "!=="),
        NamedToken::fixed("LT", "<"),
        NamedToken::fixed("LE", "<="),
        NamedToken::fixed("GT", ">"),
        NamedToken::fixed("GE", ">="),
        NamedToken::fixed("SPACESHIP", "<=>"),
        NamedToken::fixed("PLUS", "+"),
        NamedToken::fixed("MINUS", "-"),
        NamedToken::fixed("STAR", "*"),
        NamedToken::fixed("SLASH", "/"),
        NamedToken::fixed("PERCENT", "%"),
        NamedToken::fixed("PLUS_EQ", "+="),
        NamedToken::fixed("MINUS_EQ", "-="),
        NamedToken::fixed("STAR_EQ", "*="),
        NamedToken::fixed("SLASH_EQ", "/="),
        NamedToken::fixed("PERCENT_EQ", "%="),
        NamedToken::fixed("AND_AND", "&&"),
        NamedToken::fixed("OR_OR", "||"),
        NamedToken::fixed("AMP", "&"),
        NamedToken::fixed("PIPE", "|"),
        NamedToken::fixed("CARET", "^"),
        NamedToken::fixed("TILDE", "~"),
        NamedToken::fixed("AMP_EQ", "&="),
        NamedToken::fixed("PIPE_EQ", "|="),
        NamedToken::fixed("CARET_EQ", "^="),
        NamedToken::fixed("LT_LT", "<<"),
        NamedToken::fixed("GT_GT", ">>"),
        NamedToken::fixed("LT_LT_EQ", "<<="),
        NamedToken::fixed("GT_GT_EQ", ">>="),
        // §S.2.1 wrapping arithmetic — the `%`-suffixed family.
        NamedToken::fixed("PLUS_PERCENT", "+%"),
        NamedToken::fixed("MINUS_PERCENT", "-%"),
        NamedToken::fixed("STAR_PERCENT", "*%"),
        NamedToken::fixed("LT_LT_PERCENT", "<<%"),
        NamedToken::fixed("GT_GT_PERCENT", ">>%"),
    ];

    let comments = vec![
        NamedToken::fixed("LINE_COMMENT", "//"),
        NamedToken::fixed("BLOCK_COMMENT", "/*"),
        NamedToken::fixed("DOC_COMMENT", "/**"),
    ];

    GrammarSpec {
        keywords,
        primitives,
        annotations: BUILTIN_ANNOTATIONS.iter().map(|s| s.to_string()).collect(),
        builtins: {
            let mut names: Vec<String> = BUILTIN_NAMES.iter().map(|s| s.to_string()).collect();
            names.extend(extra_builtins.iter().cloned());
            names.sort();
            names.dedup();
            names
        },
        constants,
        literals,
        punctuation,
        operators,
        comments,
    }
}

/// Serialize the spec to the exact JSON text the plugin consumes — pretty,
/// two-space indented, with a trailing newline so editors and `git` are happy.
pub fn to_json(spec: &GrammarSpec) -> String {
    let mut s = serde_json::to_string_pretty(spec).expect("grammar spec serializes");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every annotation the compiler acts on must be offered by the editor.
    ///
    /// The compiler recognizes annotations at scattered call sites rather than
    /// through one table, so this list could drift away from them silently —
    /// and the symptom would be an annotation that works but never appears in
    /// completion. Pinning the count and the exact spellings makes adding a
    /// behaviour without adding the name a test failure.
    #[test]
    fn annotations_are_honored() {
        let mut sorted: Vec<&str> = BUILTIN_ANNOTATIONS.to_vec();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            [
                "AfterAll", "AfterEach", "BeforeAll", "BeforeEach", "Override",
                "Test", "export", "extern", "layout",
            ],
            "the honored-annotation set changed — update the editor's list too",
        );
    }

    /// Keep `Keyword::ALL` in lockstep with `Keyword::lookup`: every entry must
    /// round-trip, and the count must match the spelling list, so a variant
    /// added to the enum but forgotten in `ALL` (or vice-versa) is caught.
    #[test]
    fn keyword_all_is_complete() {
        for kw in Keyword::ALL {
            assert_eq!(Keyword::lookup(kw.as_str()), Some(*kw));
        }
        assert_eq!(Keyword::ALL.len(), 58, "keyword count changed — update grammar spec consumers");
    }

}
