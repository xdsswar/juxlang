//! Phase B/C of the type checker — the inferred-type representation.
//!
//! This module owns the [`Ty`] enum (and its helper enums [`Primitive`]
//! and [`ArrayKind`]) — the value-type the inference phase produces for
//! every expression in the program.
//!
//! ## Why a separate enum?
//!
//! [`juxc_ast::TypeRef`] is the **syntactic** form of a type — what the
//! user wrote, with span info attached. It's a tree of `QualifiedName`s
//! and isn't easy to compare structurally. [`Ty`] is the **semantic**
//! form — primitives are concrete enum tags, the Jux `String` type has
//! its own variant (so it's distinct from numeric primitives), array
//! shapes are recursive, and user types are name + resolved generic
//! arguments.
//!
//! Conversion from `TypeRef` to `Ty` happens via [`ty_from_ref`] which
//! consults the [`TypeEnv`] for in-scope generic parameters and the
//! [`SymbolTable`] for user-defined type names.
//!
//! ## Unknown
//!
//! [`Ty::Unknown`] is the **silent failure** marker. Whenever inference
//! can't determine a type — an unresolved name, an unsupported expr
//! shape, a field lookup on a non-class receiver — the result is
//! `Unknown`. Phase C is intentionally silent; Phase D will turn these
//! into proper diagnostics by comparing inferred against expected at
//! statement boundaries.

use std::fmt;

use juxc_ast::{TypeParam, TypeRef};

use crate::env::TypeEnv;
use crate::symbol_table::SymbolTable;

// ============================================================================
// Type representation
// ============================================================================

/// Inferred type of an expression or local.
///
/// Construction is via [`ty_from_ref`] (lowering a syntactic
/// [`TypeRef`]) or directly from a literal/expression in
/// [`crate::infer::infer_expr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// A primitive scalar — bool, char, integer, or float. See
    /// [`Primitive`] for the full list.
    Primitive(Primitive),
    /// The Jux `String` type. Distinct from any numeric primitive so
    /// downstream code can match on string-ness without enumerating the
    /// `Primitive` tags.
    String,
    /// An array — either fixed-size (`T[N]`) or dynamic (`T[]`).
    /// `element` is the type of each element; `kind` discriminates
    /// the two array flavors.
    Array {
        /// Element type. Boxed because Rust's enum-size analysis is
        /// otherwise unhappy with the recursion.
        element: Box<Ty>,
        /// Fixed-size vs dynamic.
        kind: ArrayKind,
    },
    /// A user-defined type — class, record, enum, or interface — by
    /// name, with any generic-args resolved to their own [`Ty`].
    User {
        /// Type name as written.
        name: String,
        /// Generic arguments in declaration order. Empty for non-generic
        /// types.
        generic_args: Vec<Ty>,
    },
    /// A reference to a generic parameter currently in scope, e.g. the
    /// `T` inside a `class Box<T> { … }`. Distinct from `User { name: "T", … }`
    /// because the param has no signature in the symbol table.
    Param(String),
    /// The unit/return-nothing type. Methods declared `void` return
    /// this. Expressions are never `Void` — that's reserved for
    /// statement-context constructs.
    Void,
    /// Inference failed for this position. Phase D may flag this; Phase
    /// C is silent.
    Unknown,
}

/// Primitive scalar types per `JUX-LANG-V1.md` §5.1.
///
/// Two naming families per the spec: Java-family names (`int`, `byte`,
/// etc.) and width-explicit names (`i32`, `u8`, …). Aliases collapse to
/// the same underlying Rust type when emitted, but at the inference
/// level we keep them distinct so a `int` literal and an `i32` literal
/// stay traceable for the diagnostics phase. The non-alias case is
/// `int`/`uint` (platform-sized) vs `i32`/`u32` (always 32-bit) — they
/// genuinely differ in width and must not be conflated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    /// `int` — platform-sized signed (Rust `isize`).
    Int,
    /// `uint` — platform-sized unsigned (Rust `usize`).
    Uint,
    /// `byte` — 8-bit signed.
    Byte,
    /// `ubyte` — 8-bit unsigned.
    Ubyte,
    /// `short` — 16-bit signed.
    Short,
    /// `ushort` — 16-bit unsigned.
    Ushort,
    /// `long` — 64-bit signed.
    Long,
    /// `ulong` — 64-bit unsigned.
    Ulong,
    /// `float` — 32-bit IEEE 754.
    Float,
    /// `double` — 64-bit IEEE 754.
    Double,
    /// `bool` — boolean.
    Bool,
    /// `char` — Unicode scalar value.
    Char,
    /// Width-explicit 32-bit signed (`i32`).
    I32,
    /// Width-explicit 32-bit unsigned (`u32`).
    U32,
    /// Width-explicit 64-bit signed (`i64`).
    I64,
    /// Width-explicit 64-bit unsigned (`u64`).
    U64,
    /// Width-explicit 8-bit signed (`i8`).
    I8,
    /// Width-explicit 8-bit unsigned (`u8`).
    U8,
    /// Width-explicit 16-bit signed (`i16`).
    I16,
    /// Width-explicit 16-bit unsigned (`u16`).
    U16,
    /// Width-explicit 32-bit float (`f32`).
    F32,
    /// Width-explicit 64-bit float (`f64`).
    F64,
}

/// Discriminates the two array flavors. Distinguishes `T[N]` from `T[]`
/// — sizing affects lowering and may eventually affect what operations
/// are permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayKind {
    /// `T[N]` — compile-time fixed length.
    Fixed,
    /// `T[]` — runtime-sized.
    Dynamic,
}

impl Ty {
    /// True iff this is the Jux `String` type. Useful when downstream
    /// code wants to take the `String`-specific path (interp string,
    /// `&str` ↔ `String` coercion) without enumerating every other
    /// variant.
    pub fn is_string(&self) -> bool {
        matches!(self, Ty::String)
    }

    /// True iff this is the unit type — the return type of a `void`
    /// function. Expressions never produce `Void`.
    pub fn is_void(&self) -> bool {
        matches!(self, Ty::Void)
    }

    /// True iff this is the `bool` primitive. Used when checking
    /// boolean-context positions (`if`, `while`, `!`-operand).
    pub fn is_bool(&self) -> bool {
        matches!(self, Ty::Primitive(Primitive::Bool))
    }

    /// True iff this is one of the numeric primitives — every
    /// [`Primitive`] tag except `Bool` and `Char`. Floats count.
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Ty::Primitive(p) if !matches!(p, Primitive::Bool | Primitive::Char)
        )
    }

    /// True iff inference returned `Unknown` for this expression.
    /// Phase D uses this to decide whether to emit a "cannot determine
    /// type" diagnostic.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }

    /// Human-readable single-line spelling, suitable for embedding in a
    /// diagnostic message. Matches the source spelling where the source
    /// has one; otherwise uses a Rust-flavored fallback.
    ///
    /// Examples: `"int"`, `"String"`, `"Box<int>"`, `"int[]"`,
    /// `"<unknown>"`.
    pub fn display(&self) -> String {
        format!("{self}")
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Primitive(p) => write!(f, "{}", primitive_name(*p)),
            Ty::String => write!(f, "String"),
            Ty::Array { element, kind } => match kind {
                ArrayKind::Fixed => write!(f, "{element}[N]"),
                ArrayKind::Dynamic => write!(f, "{element}[]"),
            },
            Ty::User { name, generic_args } => {
                f.write_str(name)?;
                if !generic_args.is_empty() {
                    f.write_str("<")?;
                    for (i, arg) in generic_args.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            }
            Ty::Param(name) => f.write_str(name),
            Ty::Void => f.write_str("void"),
            Ty::Unknown => f.write_str("<unknown>"),
        }
    }
}

/// The source-level spelling of a primitive. Used by [`Ty::Display`] and
/// by future diagnostics that want to print a type the way the user
/// would have written it.
fn primitive_name(p: Primitive) -> &'static str {
    match p {
        Primitive::Int => "int",
        Primitive::Uint => "uint",
        Primitive::Byte => "byte",
        Primitive::Ubyte => "ubyte",
        Primitive::Short => "short",
        Primitive::Ushort => "ushort",
        Primitive::Long => "long",
        Primitive::Ulong => "ulong",
        Primitive::Float => "float",
        Primitive::Double => "double",
        Primitive::Bool => "bool",
        Primitive::Char => "char",
        Primitive::I32 => "i32",
        Primitive::U32 => "u32",
        Primitive::I64 => "i64",
        Primitive::U64 => "u64",
        Primitive::I8 => "i8",
        Primitive::U8 => "u8",
        Primitive::I16 => "i16",
        Primitive::U16 => "u16",
        Primitive::F32 => "f32",
        Primitive::F64 => "f64",
    }
}

// ============================================================================
// TypeRef -> Ty lowering
// ============================================================================

/// Resolve a syntactic [`TypeRef`] into its semantic [`Ty`].
///
/// Resolution order (per the Phase B/C spec):
///
/// 1. Array shape present → recurse on a copy without the shape, wrap
///    the result in [`Ty::Array`] with the matching [`ArrayKind`].
/// 2. Single-segment primitive name (`int`, `i32`, `bool`, etc.) →
///    [`Ty::Primitive`].
/// 3. Single-segment `String` → [`Ty::String`].
/// 4. Single-segment name registered in `env.generic_params` →
///    [`Ty::Param`].
/// 5. Name registered in `symbols.is_type_name` → [`Ty::User`] with
///    each generic-arg recursively resolved.
/// 6. Anything else → [`Ty::Unknown`]. **No diagnostic is emitted.**
///    Phase D will surface unresolved-name errors at use sites.
pub fn ty_from_ref(t: &TypeRef, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // 1. Array shape — peel one shape, recurse on the element form.
    if let Some(shape) = &t.array_shape {
        let element_ref = TypeRef {
            name: t.name.clone(),
            generic_args: t.generic_args.clone(),
            nullable: t.nullable,
            array_shape: None,
            span: t.span,
        };
        let element = ty_from_ref(&element_ref, env, symbols);
        let kind = match shape {
            juxc_ast::ArrayShape::Fixed(_) => ArrayKind::Fixed,
            juxc_ast::ArrayShape::Dynamic => ArrayKind::Dynamic,
        };
        return Ty::Array {
            element: Box::new(element),
            kind,
        };
    }

    // 2–4. Single-segment shortcuts — primitives, String, generic params.
    if t.name.segments.len() == 1 && t.generic_args.is_empty() {
        let name = t.name.segments[0].text.as_str();
        if let Some(prim) = primitive_from_name(name) {
            return Ty::Primitive(prim);
        }
        if name == "String" {
            return Ty::String;
        }
        if env.generic_params.contains(name) {
            return Ty::Param(name.to_string());
        }
    }

    // 5. User-defined type — single-segment name that resolves to a
    //    class/record/enum/interface in the symbol table. Recursively
    //    lower the generic args.
    if t.name.segments.len() == 1 {
        let name = &t.name.segments[0].text;
        if symbols.is_type_name(name) {
            let generic_args = t
                .generic_args
                .iter()
                .map(|g| ty_from_ref(g, env, symbols))
                .collect();
            return Ty::User {
                name: name.clone(),
                generic_args,
            };
        }
    }

    // 6. Fallthrough — multi-segment paths, unknown bare names, etc.
    //    Stay silent and let Phase D handle the diagnostics.
    Ty::Unknown
}

/// Lower a member's [`TypeRef`] in the **declaring** type's generic-param
/// scope rather than the caller's. Phase E uses this when it reaches a
/// field/parameter/return type from outside the type's body: the caller
/// has no `T` registered in their env, so a plain [`ty_from_ref`] would
/// resolve `T` to [`Ty::Unknown`] instead of [`Ty::Param`].
///
/// Specifically: classes, records, and interfaces all carry
/// `generic_params`; if `declaring_class` names one of them, its params
/// are loaded into a fresh scratch env before delegating to
/// [`ty_from_ref`]. Unknown names produce an empty env (degenerate
/// case — caller is responsible for ensuring `declaring_class` exists).
pub fn lower_member_type(ty_ref: &TypeRef, declaring_class: &str, symbols: &SymbolTable) -> Ty {
    let mut env = TypeEnv::new();
    if let Some(class) = symbols.classes.get(declaring_class) {
        for tp in &class.generic_params {
            env.add_generic_param(&tp.name.text);
        }
    } else if let Some(record) = symbols.records.get(declaring_class) {
        for tp in &record.generic_params {
            env.add_generic_param(&tp.name.text);
        }
    } else if let Some(iface) = symbols.interfaces.get(declaring_class) {
        for tp in &iface.generic_params {
            env.add_generic_param(&tp.name.text);
        }
    }
    ty_from_ref(ty_ref, &env, symbols)
}

// ============================================================================
// Generic-parameter substitution (Phase E)
// ============================================================================

/// Substitute references to generic type parameters inside `ty`.
///
/// Phase E uses this to instantiate member signatures at use sites. When
/// a `Box<T>` declares `T value;` and a caller writes `var b = new
/// Box<int>(...); b.value`, the field's declared type is `Ty::Param("T")`
/// but the user sees an `int`. Walking the receiver's `generic_args` gives
/// the substitution `T → Int`, and applying it via this function yields
/// the right inferred type for downstream phases.
///
/// `params` is the declaring type's generic-parameter list in declaration
/// order. `args` is the receiver's `generic_args` (in matching position).
/// Substitution is a no-op when:
///
/// - `params` is empty (non-generic declaration), or
/// - `params.len() != args.len()` (receiver written as a raw type, e.g.
///   `new Box(...)`) — leaving `Ty::Param(...)` in place lets the
///   wildcard rule in `compatible` keep accepting calls; tightening this
///   is a later phase's job.
///
/// `ty` is returned by-value (cloned where necessary). Variants without
/// nested types (`Primitive`, `String`, `Void`, `Unknown`) clone
/// trivially; nested forms (`Array`, `User`) recurse.
pub fn substitute(ty: &Ty, params: &[TypeParam], args: &[Ty]) -> Ty {
    if params.is_empty() || params.len() != args.len() {
        return ty.clone();
    }
    substitute_inner(ty, params, args)
}

/// Recursive worker for [`substitute`]. Split out so the outer entry
/// point can short-circuit on the no-substitution cases without paying
/// for the recursion stack on the common path.
fn substitute_inner(ty: &Ty, params: &[TypeParam], args: &[Ty]) -> Ty {
    match ty {
        Ty::Param(name) => {
            // Linear scan — params lists are tiny (< 5 in practice).
            for (i, p) in params.iter().enumerate() {
                if p.name.text == *name {
                    return args[i].clone();
                }
            }
            // Param mentioned in the signature but not in the declaring
            // type's parameter list. Usually means a method-level
            // generic, which we don't substitute here (those are bound
            // at the call site, not by the receiver).
            ty.clone()
        }
        Ty::Array { element, kind } => Ty::Array {
            element: Box::new(substitute_inner(element, params, args)),
            kind: *kind,
        },
        Ty::User { name, generic_args } => Ty::User {
            name: name.clone(),
            generic_args: generic_args
                .iter()
                .map(|a| substitute_inner(a, params, args))
                .collect(),
        },
        Ty::Primitive(_) | Ty::String | Ty::Void | Ty::Unknown => ty.clone(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use juxc_ast::Ident;
    use juxc_source::Span;

    /// Build a one-name [`TypeParam`] for testing — only the `name.text`
    /// field is consulted by [`substitute`].
    fn type_param(name: &str) -> TypeParam {
        TypeParam {
            name: Ident {
                text: name.to_string(),
                span: Span::DUMMY,
            },
            bounds: Vec::new(),
            span: Span::DUMMY,
        }
    }

    /// Substituting `T` against `[T] → [Int]` yields `Int`.
    #[test]
    fn substitute_replaces_param_by_position() {
        let result = substitute(
            &Ty::Param("T".to_string()),
            &[type_param("T")],
            &[Ty::Primitive(Primitive::Int)],
        );
        assert_eq!(result, Ty::Primitive(Primitive::Int));
    }

    /// A param name that's not in the list is left alone.
    #[test]
    fn substitute_unknown_param_is_identity() {
        let result = substitute(
            &Ty::Param("U".to_string()),
            &[type_param("T")],
            &[Ty::Primitive(Primitive::Int)],
        );
        assert_eq!(result, Ty::Param("U".to_string()));
    }

    /// Length mismatch is a no-op — raw-type call sites pass an empty
    /// args list and we keep the param in place.
    #[test]
    fn substitute_with_mismatched_lengths_is_identity() {
        let result = substitute(
            &Ty::Param("T".to_string()),
            &[type_param("T")],
            &[],
        );
        assert_eq!(result, Ty::Param("T".to_string()));
    }

    /// Substitution descends through arrays.
    #[test]
    fn substitute_descends_into_arrays() {
        let original = Ty::Array {
            element: Box::new(Ty::Param("T".to_string())),
            kind: ArrayKind::Dynamic,
        };
        let result = substitute(
            &original,
            &[type_param("T")],
            &[Ty::Primitive(Primitive::Int)],
        );
        match result {
            Ty::Array { element, kind } => {
                assert_eq!(*element, Ty::Primitive(Primitive::Int));
                assert_eq!(kind, ArrayKind::Dynamic);
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    /// Substitution descends through nested User generic args.
    #[test]
    fn substitute_descends_into_user_generic_args() {
        let original = Ty::User {
            name: "List".to_string(),
            generic_args: vec![Ty::Param("T".to_string())],
        };
        let result = substitute(
            &original,
            &[type_param("T")],
            &[Ty::String],
        );
        match result {
            Ty::User { name, generic_args } => {
                assert_eq!(name, "List");
                assert_eq!(generic_args, vec![Ty::String]);
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    /// Non-param leaves (Primitive, String, Void, Unknown) clone unchanged.
    #[test]
    fn substitute_leaves_non_param_leaves_unchanged() {
        let params = [type_param("T")];
        let args = [Ty::String];
        assert_eq!(
            substitute(&Ty::Primitive(Primitive::Int), &params, &args),
            Ty::Primitive(Primitive::Int),
        );
        assert_eq!(substitute(&Ty::String, &params, &args), Ty::String);
        assert_eq!(substitute(&Ty::Void, &params, &args), Ty::Void);
        assert_eq!(substitute(&Ty::Unknown, &params, &args), Ty::Unknown);
    }
}

// ============================================================================
// Generic-parameter inference at call sites (Phase G — spec §T.4)
// ============================================================================

/// Infer generic type arguments at a call site from the declared
/// parameter types and the inferred argument types.
///
/// Phase-1 scope per `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.4.2 (steps 1
/// and 4 only — no return-type constraint, no bound constraint
/// propagation, no LUB/join):
///
/// 1. For each (declared-param-type, arg-type) pair, if the declared
///    type is a **bare** mention of a generic parameter (`T x` where
///    `T` is in `generic_params`), record `T → arg_ty`.
/// 2. If the same `T` is constrained by multiple args, they must
///    agree exactly — otherwise inference gives up and returns an
///    empty result (caller falls back to whatever it does for
///    unsolved generics today, which is "leave `Ty::Param("T")` in
///    place and let the wildcard rule keep things quiet").
///
/// **NOT yet handled** (spec describes these but they need real
/// constraint solving):
///
/// - Nested generic patterns: `T list(List<T> xs)` doesn't infer `T`
///   from a `List<int>` argument. Today this leaves `T` unsolved.
/// - Return-type-driven inference: `T identity(T x); long y =
///   identity(42)` doesn't push `long` back through `T`.
/// - Bound-driven inference: `<T extends Animal>` constraints don't
///   participate in solving.
/// - Subtype joins ("least upper bound"): when two args are different
///   classes in the same hierarchy, inference gives up rather than
///   pick their LUB.
///
/// Returns a `(name → Ty)` map. Empty map means inference produced no
/// useful info (no generic params, all params nonsensical, or a
/// conflict was found). Callers should treat an empty result as "use
/// the unsubstituted signature."
pub fn infer_generic_args(
    generic_params: &[TypeParam],
    param_tys: &[&TypeRef],
    arg_tys: &[Ty],
) -> std::collections::HashMap<String, Ty> {
    let mut inferred: std::collections::HashMap<String, Ty> =
        std::collections::HashMap::new();
    if generic_params.is_empty() {
        return inferred;
    }
    let param_names: std::collections::HashSet<&str> = generic_params
        .iter()
        .map(|p| p.name.text.as_str())
        .collect();
    for (declared, arg) in param_tys.iter().zip(arg_tys.iter()) {
        // Only the bare-name shape: `T x` where the declared type is
        // a single-segment path naming a generic param.
        if declared.array_shape.is_some()
            || declared.nullable
            || !declared.generic_args.is_empty()
            || declared.name.segments.len() != 1
        {
            continue;
        }
        let name = declared.name.segments[0].text.as_str();
        if !param_names.contains(name) {
            continue;
        }
        // Skip `Unknown` arguments — they tell us nothing about T.
        // Leaving T unresolved is better than locking it to Unknown.
        if arg.is_unknown() {
            continue;
        }
        if let Some(existing) = inferred.get(name) {
            if existing != arg {
                // Conflict — multiple args want different types for
                // the same T. Give up entirely.
                return std::collections::HashMap::new();
            }
        } else {
            inferred.insert(name.to_string(), arg.clone());
        }
    }
    inferred
}

/// Convenience wrapper around [`substitute`] that takes the
/// inference map produced by [`infer_generic_args`] instead of the
/// positional `params` + `args` slices.
///
/// For each generic param in declaration order, the map either has
/// an entry (use the inferred type) or doesn't (substitute with
/// `Ty::Unknown` so the param vanishes from the substituted result
/// — equivalent to "we couldn't solve this T").
pub fn substitute_via_inference(
    ty: &Ty,
    generic_params: &[TypeParam],
    inferred: &std::collections::HashMap<String, Ty>,
) -> Ty {
    if generic_params.is_empty() || inferred.is_empty() {
        return ty.clone();
    }
    let args: Vec<Ty> = generic_params
        .iter()
        .map(|p| inferred.get(&p.name.text).cloned().unwrap_or(Ty::Unknown))
        .collect();
    substitute(ty, generic_params, &args)
}

/// Map a bare primitive name onto its [`Primitive`] tag. Returns `None`
/// for any other identifier — including `"String"`, which lives in its
/// own [`Ty::String`] variant. Mirrors the comprehensive primitive list
/// from `juxc_backend_rust::types::jux_primitive_to_rust`.
pub(crate) fn primitive_from_name(name: &str) -> Option<Primitive> {
    Some(match name {
        // Java-family names
        "bool" => Primitive::Bool,
        "byte" => Primitive::Byte,
        "ubyte" => Primitive::Ubyte,
        "short" => Primitive::Short,
        "ushort" => Primitive::Ushort,
        "int" => Primitive::Int,
        "uint" => Primitive::Uint,
        "long" => Primitive::Long,
        "ulong" => Primitive::Ulong,
        "float" => Primitive::Float,
        "double" => Primitive::Double,
        "char" => Primitive::Char,
        // Width-explicit names
        "i8" => Primitive::I8,
        "u8" => Primitive::U8,
        "i16" => Primitive::I16,
        "u16" => Primitive::U16,
        "i32" => Primitive::I32,
        "u32" => Primitive::U32,
        "i64" => Primitive::I64,
        "u64" => Primitive::U64,
        "f32" => Primitive::F32,
        "f64" => Primitive::F64,
        _ => return None,
    })
}
