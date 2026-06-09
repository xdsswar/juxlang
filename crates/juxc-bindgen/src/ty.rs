//! Jux type representation and rendering — JUX-BINDGEN-ADDENDUM.md §G.3.
//!
//! [`JuxType`] is the language-agnostic result of mapping a foreign type. Its
//! [`Display`](std::fmt::Display) renders Jux source syntax (`List<T>`, `T?`,
//! `(A) -> B`, …) — that's what lands in a `.jux.d` stub.

use std::fmt;

/// A Jux type, as it should appear in a stub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JuxType {
    /// A primitive / built-in scalar, rendered verbatim (`int`, `bool`, `f64`→`double`…).
    Prim(&'static str),
    /// The Jux `String` type.
    String,
    /// A user or library type with optional generic args: `List<T>`,
    /// `Map<K, V>`, `HashMap`→`Map`, or a named class/interface.
    User { name: String, args: Vec<JuxType> },
    /// A generic parameter in scope — the `T` of `class Box<T>`.
    Param(String),
    /// Nullable wrapper `T?` (§G.3.2).
    Nullable(Box<JuxType>),
    /// Array — dynamic `T[]` (`size` None) or fixed `T[N]` (`size` Some).
    Array { elem: Box<JuxType>, size: Option<u64> },
    /// Tuple `(A, B)`.
    Tuple(Vec<JuxType>),
    /// Function type `(A) -> R`, optionally `async` (§G.3.1 / §7.9).
    Fn { params: Vec<JuxType>, ret: Box<JuxType>, is_async: bool },
    /// `void` — unit in return position.
    Void,
    /// `never` — the bottom type.
    Never,
    /// Raw pointer `T*` (unsafe contexts only).
    RawPtr(Box<JuxType>),
    /// Bounded wildcard generic argument: `?`, `? extends T`, `? super T`.
    Wildcard(Option<Wildcard>),
    /// An un-mappable type. Carries a best-effort name for display; the ingest
    /// layer may skip items that reference it (§G.12 `W0307`).
    Unknown(String),
}

/// A bounded wildcard (`? extends T` / `? super T`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wildcard {
    pub kind: WildcardKind,
    pub bound: Box<JuxType>,
}

/// Variance of a bounded wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WildcardKind {
    /// `? extends T` — covariant (producer).
    Extends,
    /// `? super T` — contravariant (consumer).
    Super,
}

impl JuxType {
    /// A non-generic user/library type by name.
    pub fn user(name: impl Into<String>) -> JuxType {
        JuxType::User { name: name.into(), args: Vec::new() }
    }

    /// The Jux stdlib `List<elem>`.
    pub fn list(elem: JuxType) -> JuxType {
        JuxType::User { name: "List".into(), args: vec![elem] }
    }

    /// The Jux stdlib `Map<k, v>`.
    pub fn map(k: JuxType, v: JuxType) -> JuxType {
        JuxType::User { name: "Map".into(), args: vec![k, v] }
    }

    /// The Jux stdlib `Set<elem>`.
    pub fn set(elem: JuxType) -> JuxType {
        JuxType::User { name: "Set".into(), args: vec![elem] }
    }

    /// Wrap in a nullable marker, collapsing `T??` to `T?` (idempotent).
    pub fn nullable(inner: JuxType) -> JuxType {
        match inner {
            already @ JuxType::Nullable(_) => already,
            other => JuxType::Nullable(Box::new(other)),
        }
    }
}

impl fmt::Display for JuxType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JuxType::Prim(s) => f.write_str(s),
            JuxType::String => f.write_str("String"),
            JuxType::User { name, args } => {
                f.write_str(name)?;
                if !args.is_empty() {
                    write!(f, "<{}>", join(args))?;
                }
                Ok(())
            }
            JuxType::Param(s) => f.write_str(s),
            JuxType::Nullable(t) => write!(f, "{t}?"),
            JuxType::Array { elem, size } => match size {
                Some(n) => write!(f, "{elem}[{n}]"),
                None => write!(f, "{elem}[]"),
            },
            // Tuple types (`(A, B)`, grammar §A.2.7 `tuple-type`) are surfaced
            // as the nominal `Tuple<A, B>` rather than the bracketed form. The
            // parser's `tuple-type` support lands with the broader advanced-type
            // work; until then the nominal keeps the element types visible and,
            // crucially, parses — so the enclosing member survives into the
            // symbol table and autocompletes. (The unit tuple `()` never reaches
            // here: `map_type` folds it to `void`.)
            JuxType::Tuple(ts) => write!(f, "Tuple<{}>", join(ts)),
            JuxType::Fn { params, ret, is_async } => {
                if *is_async {
                    write!(f, "({}) async -> {ret}", join(params))
                } else {
                    write!(f, "({}) -> {ret}", join(params))
                }
            }
            JuxType::Void => f.write_str("void"),
            JuxType::Never => f.write_str("never"),
            // A raw pointer (`*const T` / `*mut T`) is surfaced as the nominal
            // `Ptr<T>` rather than the grammar's `T*` (`pointer-type`, §A.2.7,
            // unsafe-only). The parser's `pointer-type` support and the real
            // pointer lowering land with the deferred C/C++/unsafe interop work;
            // until then `Ptr<T>` keeps the pointee visible and, crucially,
            // parses — so the enclosing member survives into the symbol table
            // and still autocompletes. The `RawPtr` IR variant is retained so a
            // future renderer can emit true `T*` without re-deriving the shape.
            JuxType::RawPtr(t) => {
                // A pointer to unit (`*const ()` / `*mut ()`, common as an opaque
                // C `void*`) has a `Void` pointee — but `void` is not a valid
                // nominal type argument, so surface it as an opaque `Ptr<Object>`
                // rather than the unparseable `Ptr<void>`.
                if matches!(**t, JuxType::Void) {
                    f.write_str("Ptr<Object>")
                } else {
                    write!(f, "Ptr<{t}>")
                }
            }
            JuxType::Wildcard(None) => f.write_str("?"),
            JuxType::Wildcard(Some(w)) => match w.kind {
                WildcardKind::Extends => write!(f, "? extends {}", w.bound),
                WildcardKind::Super => write!(f, "? super {}", w.bound),
            },
            JuxType::Unknown(name) => f.write_str(name),
        }
    }
}

/// Comma-join a slice of types for generic-arg / tuple / param rendering.
fn join(types: &[JuxType]) -> String {
    types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_primitives_and_string() {
        assert_eq!(JuxType::Prim("int").to_string(), "int");
        assert_eq!(JuxType::String.to_string(), "String");
        assert_eq!(JuxType::Void.to_string(), "void");
        assert_eq!(JuxType::Never.to_string(), "never");
    }

    #[test]
    fn renders_generics() {
        assert_eq!(JuxType::list(JuxType::String).to_string(), "List<String>");
        assert_eq!(
            JuxType::map(JuxType::String, JuxType::Prim("int")).to_string(),
            "Map<String, int>",
        );
        assert_eq!(JuxType::user("HashMap").to_string(), "HashMap");
    }

    #[test]
    fn renders_nullable_array_ptr() {
        assert_eq!(JuxType::nullable(JuxType::String).to_string(), "String?");
        // Idempotent: T?? collapses to T?.
        assert_eq!(
            JuxType::nullable(JuxType::nullable(JuxType::String)).to_string(),
            "String?",
        );
        assert_eq!(
            JuxType::Array { elem: Box::new(JuxType::Prim("byte")), size: None }.to_string(),
            "byte[]",
        );
        assert_eq!(
            JuxType::Array { elem: Box::new(JuxType::Prim("int")), size: Some(8) }.to_string(),
            "int[8]",
        );
        // Raw pointers surface as the nominal `Ptr<T>` (parseable today; real
        // `T*` pointer-type syntax lands with the deferred unsafe/C-interop work).
        assert_eq!(JuxType::RawPtr(Box::new(JuxType::Prim("byte"))).to_string(), "Ptr<byte>");
    }

    #[test]
    fn renders_fn_and_wildcards() {
        assert_eq!(
            JuxType::Fn {
                params: vec![JuxType::Prim("int")],
                ret: Box::new(JuxType::String),
                is_async: false,
            }
            .to_string(),
            "(int) -> String",
        );
        assert_eq!(
            JuxType::Wildcard(Some(Wildcard {
                kind: WildcardKind::Extends,
                bound: Box::new(JuxType::user("Animal")),
            }))
            .to_string(),
            "? extends Animal",
        );
        assert_eq!(JuxType::Wildcard(None).to_string(), "?");
    }
}
