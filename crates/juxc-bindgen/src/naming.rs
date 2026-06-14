//! Naming transforms — JUX-BINDGEN-ADDENDUM.md §G.4.
//!
//! Foreign names are surfaced into Jux **verbatim** so a Jux call site reads
//! like the real Rust API (and matches rustdoc):
//!
//! | Foreign convention            | Jux convention        |
//! |-------------------------------|-----------------------|
//! | Rust `snake_case` fn/method   | `snake_case` (kept)   |
//! | Rust `PascalCase` type        | `PascalCase` (kept)   |
//! | Rust `SCREAMING_SNAKE` const  | `SCREAMING_SNAKE`     |
//! | Rust module path `a::b::c`    | package `a.b.c`       |
//!
//! Member names are kept as-is even when they collide with a Jux keyword
//! (e.g. a Rust `default()` / `match()` method): the parser accepts keyword
//! spellings in member position and in foreign stub declarations, and the
//! backend re-escapes Rust keywords with `r#` at emission. The transform is
//! the identity (modulo module-path dots), so a stub regenerates identically.

/// True if `s` is `SCREAMING_SNAKE_CASE` (uppercase letters, digits, and
/// underscores only, with at least one letter). Such names — Rust `const` /
/// `static` — map to Jux constants unchanged (§G.4 / §G.5.6).
pub fn is_screaming_snake(s: &str) -> bool {
    let mut has_letter = false;
    for ch in s.chars() {
        if ch.is_ascii_uppercase() {
            has_letter = true;
        } else if ch == '_' || ch.is_ascii_digit() {
            // allowed
        } else {
            return false;
        }
    }
    has_letter
}

/// Turn a Rust module path (`a::b::c`) into a Jux package path (`a.b.c`),
/// §G.4 / §4.2.
pub fn module_path_to_package(path: &str) -> String {
    path.split("::")
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

/// Full method/function/field-name transform: the **identity**. Rust names are
/// surfaced verbatim (§G.4) — `is_open` stays `is_open`, `with_capacity` stays
/// `with_capacity`, and keyword-spelled names like `default`/`match` are kept so
/// the real Rust API is callable. Constants (`SCREAMING_SNAKE`) are likewise
/// unchanged. The backend re-escapes any Rust keyword with `r#` at emission.
pub fn method_name(rust: &str) -> String {
    rust.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_name_is_identity() {
        // Rust names are surfaced verbatim — no camelCasing.
        assert_eq!(method_name("with_capacity"), "with_capacity");
        assert_eq!(method_name("to_string"), "to_string");
        assert_eq!(method_name("is_open"), "is_open");
        assert_eq!(method_name("is_empty"), "is_empty");
        assert_eq!(method_name("len"), "len");
        assert_eq!(method_name("from_u8"), "from_u8");
    }

    #[test]
    fn keyword_named_members_kept() {
        // Keyword-spelled Rust members survive as-is; the parser accepts them in
        // member/stub position and the backend `r#`-escapes at emission.
        assert_eq!(method_name("default"), "default");
        assert_eq!(method_name("type"), "type");
        assert_eq!(method_name("match"), "match");
        assert_eq!(method_name("box"), "box");
    }

    #[test]
    fn screaming_snake_unchanged() {
        assert!(is_screaming_snake("MAX_VALUE"));
        assert!(is_screaming_snake("PI"));
        assert!(!is_screaming_snake("maxValue"));
        assert_eq!(method_name("MAX_VALUE"), "MAX_VALUE");
    }

    #[test]
    fn module_path_dots() {
        assert_eq!(module_path_to_package("std::collections::hash_map"), "std.collections.hash_map");
        assert_eq!(module_path_to_package("serde_json"), "serde_json");
    }
}
