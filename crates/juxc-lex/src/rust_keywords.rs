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
    // `self` and `Self` are reserved too. They are NOT valid Jux keywords
    // (Jux uses `this`), so the parser accepts them as ordinary identifiers —
    // which means a user CAN write `public int self() {}`. See
    // [`NON_ESCAPABLE_RUST_KEYWORDS`] for why those four are the only ones the
    // resolver still refuses.
    "self", "Self",
];

/// The Rust keywords that **cannot** be written as raw identifiers: `r#self`,
/// `r#Self`, `r#crate` and `r#super` are all rejected by rustc.
///
/// Every other reserved word survives lowering as `r#name`, so a Jux program is
/// free to use it. That matters more than it sounds: `type`, `match`, `move`,
/// `box`, `loop`, `impl`, `mod`, `ref` and `final` are perfectly ordinary names
/// in Java or C#, and refusing them would be Rust's implementation detail
/// leaking into Jux's surface. Only this handful has no escape, so only this
/// handful is reserved.
pub const NON_ESCAPABLE_RUST_KEYWORDS: &[&str] = &["self", "Self", "crate", "super"];

/// True when `name` is a Rust reserved word with no raw-identifier form, and so
/// cannot be used as a Jux declaration name at all.
pub fn is_non_escapable_rust_keyword(name: &str) -> bool {
    NON_ESCAPABLE_RUST_KEYWORDS.contains(&name)
}

/// True when `name` is a Rust reserved word (see [`RUST_KEYWORDS`]).
pub fn is_rust_keyword(name: &str) -> bool {
    RUST_KEYWORDS.contains(&name)
}

/// Wrap a Jux identifier in Rust's `r#` raw-identifier syntax if it would
/// otherwise collide with a Rust reserved word.
///
/// The four in [`NON_ESCAPABLE_RUST_KEYWORDS`] pass through unchanged — Rust
/// has no raw form for them — letting rustc surface its native error if one
/// ever slips into emitter output. The resolver rejects those in user source.
pub fn to_rust_ident(name: &str) -> String {
    if is_non_escapable_rust_keyword(name) {
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
