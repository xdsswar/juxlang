//! Naming transforms — JUX-BINDGEN-ADDENDUM.md §G.4.
//!
//! Foreign names are rewritten to Jux conventions:
//!
//! | Foreign convention            | Jux convention      |
//! |-------------------------------|---------------------|
//! | Rust `snake_case` fn/method   | `camelCase`         |
//! | Rust `PascalCase` type        | `PascalCase` (kept) |
//! | Rust `SCREAMING_SNAKE` const  | `SCREAMING_SNAKE`   |
//! | Rust module path `a::b::c`    | package `a.b.c`     |
//!
//! Names that collide with a Jux reserved word are suffixed with `_` (§G.4.2).
//! The transforms are deterministic so a stub regenerates identically.

/// Jux reserved words (§3.2). A transformed identifier equal to one of these
/// is escaped with a trailing underscore. Primitive type names are included so
/// a foreign `int()`/`bool()` method can't shadow a type keyword.
const RESERVED: &[&str] = &[
    "package", "import", "public", "private", "protected", "class", "interface", "enum", "record",
    "struct", "annotation", "extends", "implements", "abstract", "final", "sealed", "static",
    "const", "var", "void", "return", "if", "else", "for", "while", "do", "switch", "break",
    "continue", "new", "this", "super", "throws", "throw", "try", "catch", "finally", "true",
    "false", "null", "async", "await", "operator", "default", "is", "in", "where", "has", "type",
    // Primitive / built-in type names that are also keywords.
    "bool", "char", "byte", "short", "int", "long", "float", "double", "ubyte", "ushort", "uint",
    "ulong", "never",
];

/// Convert a Rust `snake_case` identifier to Jux `camelCase`.
///
/// `with_capacity` → `withCapacity`, `to_string` → `toString`, `new` → `new`.
/// Leading underscores are preserved; an all-uppercase `SCREAMING_SNAKE` name
/// is returned unchanged (callers should route constants through
/// [`is_screaming_snake`] first, but this keeps such names stable regardless).
pub fn snake_to_camel(s: &str) -> String {
    if is_screaming_snake(s) {
        return s.to_string();
    }
    // Preserve any leading underscores verbatim.
    let leading = s.len() - s.trim_start_matches('_').len();
    let mut out = String::with_capacity(s.len());
    out.push_str(&s[..leading]);

    let mut upper_next = false;
    for ch in s[leading..].chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

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

/// Append `_` if `name` is a Jux reserved word, leaving it otherwise unchanged
/// (§G.4.2). The shim records the original spelling so the mapping reverses.
pub fn escape_keyword(name: &str) -> String {
    if RESERVED.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// Full method/function-name transform: `snake_case` → `camelCase`, then
/// keyword-escape. Constants (`SCREAMING_SNAKE`) pass through unchanged.
pub fn method_name(rust: &str) -> String {
    escape_keyword(&snake_to_camel(rust))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_to_camel_basics() {
        assert_eq!(snake_to_camel("with_capacity"), "withCapacity");
        assert_eq!(snake_to_camel("to_string"), "toString");
        assert_eq!(snake_to_camel("new"), "new");
        assert_eq!(snake_to_camel("len"), "len");
        assert_eq!(snake_to_camel("from_u8"), "fromU8");
        assert_eq!(snake_to_camel("is_empty"), "isEmpty");
    }

    #[test]
    fn leading_underscore_preserved() {
        assert_eq!(snake_to_camel("_private_thing"), "_privateThing");
    }

    #[test]
    fn screaming_snake_unchanged() {
        assert!(is_screaming_snake("MAX_VALUE"));
        assert!(is_screaming_snake("PI"));
        assert!(!is_screaming_snake("maxValue"));
        assert_eq!(snake_to_camel("MAX_VALUE"), "MAX_VALUE");
        assert_eq!(method_name("MAX_VALUE"), "MAX_VALUE");
    }

    #[test]
    fn module_path_dots() {
        assert_eq!(module_path_to_package("std::collections::hash_map"), "std.collections.hash_map");
        assert_eq!(module_path_to_package("serde_json"), "serde_json");
    }

    #[test]
    fn keyword_escape() {
        // Rust `type()` → camel `type` → reserved → `type_`.
        assert_eq!(method_name("type"), "type_");
        assert_eq!(method_name("new"), "new_"); // `new` is reserved (construction operator)
        assert_eq!(method_name("insert"), "insert");
    }
}
