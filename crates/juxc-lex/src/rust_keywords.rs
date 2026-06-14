//! The Rust reserved-word set, shared across the compiler.
//!
//! Jux lowers to Rust source, so two passes need to know what Rust treats as a
//! keyword:
//!
//!   * the resolver, to reject a *user-declared* Jux name that equals a Rust
//!     keyword (a clean Jux diagnostic instead of a leaked rustc error), and
//!   * the Rust backend, to wrap an emitted identifier in `r#` raw-identifier
//!     syntax when it would otherwise collide.
//!
//! Keeping the list here — in the lowest crate both depend on — makes it the
//! single source of truth so the two passes can never drift apart.

/// Rust's reserved words (strict + reserved-for-future, the 2018+ set). A Jux
/// identifier equal to one of these cannot survive lowering without escaping,
/// so the resolver rejects user declarations using these names and the backend
/// `r#`-escapes any that reach emission (e.g. a foreign `match()` method).
pub const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const",
    "continue", "crate", "do", "dyn", "else", "enum", "extern", "false",
    "final", "fn", "for", "if", "impl", "in", "let", "loop", "macro",
    "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "union", "unsafe", "unsized", "use", "virtual", "where",
    "while", "yield",
];

/// True when `name` is a Rust reserved word (see [`RUST_KEYWORDS`]).
pub fn is_rust_keyword(name: &str) -> bool {
    RUST_KEYWORDS.contains(&name)
}

/// Wrap a Jux identifier in Rust's `r#` raw-identifier syntax if it would
/// otherwise collide with a Rust reserved word.
///
/// Two narrow exceptions: `self` and `Self` cannot become raw identifiers in
/// Rust at all, so they pass through unchanged — letting rustc surface its
/// native error if they ever slip into emitter output (the resolver should
/// already have caught the user-source case).
pub fn to_rust_ident(name: &str) -> String {
    if name == "self" || name == "Self" {
        return name.to_string();
    }
    if is_rust_keyword(name) {
        let mut out = String::with_capacity(name.len() + 2);
        out.push_str("r#");
        out.push_str(name);
        return out;
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_escapes_keywords() {
        assert_eq!(to_rust_ident("match"), "r#match");
        assert_eq!(to_rust_ident("loop"), "r#loop");
        assert_eq!(to_rust_ident("box"), "r#box");
        assert_eq!(to_rust_ident("default"), "default"); // not a Rust keyword
        assert_eq!(to_rust_ident("is_open"), "is_open");
    }

    #[test]
    fn self_passes_through() {
        assert_eq!(to_rust_ident("self"), "self");
        assert_eq!(to_rust_ident("Self"), "Self");
    }

    #[test]
    fn keyword_predicate() {
        assert!(is_rust_keyword("fn"));
        assert!(is_rust_keyword("impl"));
        assert!(!is_rust_keyword("window"));
    }
}
