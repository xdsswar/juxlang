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
    /// Bounded wildcard generic argument — `?`, `? extends T`,
    /// `? super T`. Only valid inside the `generic_args` of a
    /// [`Ty::User`] (or transitively inside a nested wildcard's bound).
    /// PECS variance is enforced by [`crate::check::compatible`]:
    /// `List<Dog>` matches `List<? extends Animal>` (producer →
    /// covariant), `List<Animal>` matches `List<? super Dog>`
    /// (consumer → contravariant).
    Wildcard(Wildcard),
    /// Function type per grammar §A.2.7 — `(A, B) -> R`,
    /// `() async -> R`, etc. Lowers from
    /// [`juxc_ast::FnTypeShape`]. Matches a lambda whose param
    /// arity and return-type are compatible.
    Fn {
        /// Parameter types, left to right.
        params: Vec<Ty>,
        /// Return type. `void` lands as `Ty::Void`.
        return_type: Box<Ty>,
        /// True if the function type carries the `async` marker.
        /// Phase 1 keeps this informational — `async`-aware
        /// compatibility checks land with the runtime work.
        is_async: bool,
    },
    /// The unit/return-nothing type. Methods declared `void` return
    /// this. Expressions are never `Void` — that's reserved for
    /// statement-context constructs.
    Void,
    /// A nullable wrapper — `T?` per §7.10. Carries the non-nullable
    /// inner `Ty`. Backend lowers to Rust `Option<T>`. Only
    /// reference-shaped inner types are well-formed here per spec
    /// (`String?`, user-typed, `Param`, `Array`); a primitive
    /// inner is caught by the nullable-primitive pre-pass in
    /// `crate::nullable_check`.
    Nullable(Box<Ty>),
    /// Inference failed for this position. Phase D may flag this; Phase
    /// C is silent.
    Unknown,
}

impl Ty {
    /// Convenience constructor for `T?` types — wraps `inner` in
    /// `Ty::Nullable` unless it's already nullable (no double-wrap
    /// — `T??` collapses to `T?`).
    pub fn nullable(inner: Ty) -> Ty {
        if matches!(inner, Ty::Nullable(_)) {
            inner
        } else {
            Ty::Nullable(Box::new(inner))
        }
    }

    /// True iff `self` is a `Ty::Nullable(_)`. Doesn't recurse into
    /// array / generic-arg payloads.
    pub fn is_nullable(&self) -> bool {
        matches!(self, Ty::Nullable(_))
    }
}

/// Inferred shape of a bounded wildcard. The boxed `Ty` is the
/// declared bound (a concrete type, a generic param, or another
/// user type — never another wildcard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wildcard {
    /// `?` — no bound.
    Unbounded,
    /// `? extends T` — accepts T and any subtype of T.
    Extends(Box<Ty>),
    /// `? super T` — accepts T and any supertype of T.
    Super(Box<Ty>),
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
            Ty::Wildcard(w) => match w {
                Wildcard::Unbounded => f.write_str("?"),
                Wildcard::Extends(b) => write!(f, "? extends {b}"),
                Wildcard::Super(b) => write!(f, "? super {b}"),
            },
            Ty::Fn { params, return_type, is_async } => {
                f.write_str("(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                f.write_str(")")?;
                if *is_async {
                    f.write_str(" async")?;
                }
                write!(f, " -> {return_type}")
            }
            Ty::Void => f.write_str("void"),
            Ty::Nullable(inner) => write!(f, "{inner}?"),
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
/// Convenience entry point that doesn't require a `TypeEnv`. Builds
/// a fresh empty env, then defers to [`ty_from_ref`]. Used by the
/// backend's `local_types` tracking — the emitter doesn't carry a
/// `TypeEnv` and a fresh one is sufficient for the bare-name /
/// implicit-import lookups the backend cares about.
pub fn ty_from_ref_in_env(t: &TypeRef, symbols: &SymbolTable) -> Ty {
    let env = TypeEnv::new();
    ty_from_ref(t, &env, symbols)
}

pub fn ty_from_ref(t: &TypeRef, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    let inner = ty_from_ref_unnullable(t, env, symbols);
    // Wrap in `Ty::Nullable` when the source `TypeRef` carries
    // the `?` suffix. The inner lowering treats `t` as-if non-
    // nullable; we apply the wrap once at the outermost layer
    // so nested wrappers (array of nullable, etc.) compose
    // naturally with the array branch's recursion.
    if t.nullable {
        Ty::nullable(inner)
    } else {
        inner
    }
}

/// Lower the non-nullable shape of `t` — same as [`ty_from_ref`]
/// but ignores the `?` suffix. The public entry point wraps the
/// result for nullable types; this routine handles every other
/// shape (function, array, primitive, user, generic param).
fn ty_from_ref_unnullable(t: &TypeRef, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // 0. Function-type shape — `(A, B) -> R` per grammar §A.2.7.
    //    `fn_shape` lowers to `Ty::Fn` with each param/return
    //    recursively resolved. Checked first because the rest of
    //    `TypeRef` carries no useful info in this case.
    if let Some(fn_shape) = &t.fn_shape {
        let params: Vec<Ty> = fn_shape
            .params
            .iter()
            .map(|p| ty_from_ref(p, env, symbols))
            .collect();
        let return_type = Box::new(ty_from_ref(&fn_shape.return_type, env, symbols));
        return Ty::Fn {
            params,
            return_type,
            is_async: fn_shape.is_async,
        };
    }
    // 1. Array shape — peel one shape, recurse on the element form.
    if let Some(shape) = &t.array_shape {
        let element_ref = TypeRef {
            name: t.name.clone(),
            generic_args: t.generic_args.clone(),
            nullable: t.nullable,
            array_shape: None,
            fn_shape: t.fn_shape.clone(),
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

    // Phase-1 stdlib concrete-collection shortcut. `ArrayList<T>`
    // is structurally identical to `T[]` (Rust `Vec<T>`), so we
    // lower it to `Ty::Array { kind: Dynamic }` here. This makes
    // the existing `BUILTIN_ARRAY_METHODS` dispatch (push/get/
    // contains/…) fire on `ArrayList<T>` receivers without
    // duplicating the table. `List<T>` (the interface) keeps its
    // `Ty::User` shape — users implement it with their own classes
    // or with the stdlib `ArrayList<T>`.
    //
    // The shortcut is suppressed when the user has declared their
    // own `class ArrayList<T>` (symbol-table evidence wins).
    if t.name.segments.len() == 1
        && t.name.segments[0].text == "ArrayList"
        && t.generic_args.len() == 1
        && !symbols.classes.contains_key("ArrayList")
        && !symbols.records.contains_key("ArrayList")
    {
        if let juxc_ast::GenericArg::Type(inner) = &t.generic_args[0] {
            return Ty::Array {
                element: Box::new(ty_from_ref(inner, env, symbols)),
                kind: ArrayKind::Dynamic,
            };
        }
    }

    // 5. User-defined type — single-segment name. Three paths,
    //    tried in order:
    //
    //    (a) Explicit import → `env.unqualified[bare] = FQN`.
    //        The user wrote `import a.b.Foo;`; bare `Foo` means
    //        `a.b.Foo`.
    //    (b) The bare name itself is a registered key in
    //        `symbols.*` (no-package classes; same compilation
    //        unit; etc.). Resolve to itself.
    //    (c) Implicit auto-import — the bare name matches the
    //        last segment of some registered FQN (`Map` matches
    //        `jux.std.collections.Map`). Resolve to the matching
    //        FQN. This is the "java.lang.* is auto-imported"
    //        rule applied to every stdlib package.
    if t.name.segments.len() == 1 {
        let bare = &t.name.segments[0].text;
        if let Some(fqn) = env.unqualified.get(bare) {
            if symbols.is_type_name_or_stdlib(fqn) {
                if let Some(expanded) = expand_alias(fqn, &t.generic_args, env, symbols)
                {
                    return expanded;
                }
                let generic_args = t
                    .generic_args
                    .iter()
                    .map(|g| lower_generic_arg(g, env, symbols))
                    .collect();
                return Ty::User {
                    name: fqn.clone(),
                    generic_args,
                };
            }
        }
        if symbols.is_type_name(bare) {
            if let Some(expanded) = expand_alias(bare, &t.generic_args, env, symbols) {
                return expanded;
            }
            let generic_args = t
                .generic_args
                .iter()
                .map(|g| lower_generic_arg(g, env, symbols))
                .collect();
            return Ty::User {
                name: bare.clone(),
                generic_args,
            };
        }
        // Implicit auto-import: walk every known FQN looking
        // for one whose last segment matches `bare`.
        if let Some(fqn) = symbols.find_fqn_by_bare(bare) {
            if let Some(expanded) = expand_alias(&fqn, &t.generic_args, env, symbols) {
                return expanded;
            }
            let generic_args = t
                .generic_args
                .iter()
                .map(|g| lower_generic_arg(g, env, symbols))
                .collect();
            return Ty::User {
                name: fqn,
                generic_args,
            };
        }
    }

    // 5b. Multi-segment qualified name — the user wrote
    //     `com.lib.Foo` directly. Join the segments with `.` and
    //     look up by FQN.
    if t.name.segments.len() > 1 {
        let fqn: String = t
            .name
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".");
        if symbols.is_type_name_or_stdlib(&fqn) {
            if let Some(expanded) = expand_alias(&fqn, &t.generic_args, env, symbols) {
                return expanded;
            }
            let generic_args = t
                .generic_args
                .iter()
                .map(|g| lower_generic_arg(g, env, symbols))
                .collect();
            return Ty::User {
                name: fqn,
                generic_args,
            };
        }
    }

    // 6. Fallthrough — unknown names, etc. Stay silent and let
    //    Phase D handle the diagnostics.
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
        Ty::Wildcard(w) => match w {
            Wildcard::Unbounded => ty.clone(),
            Wildcard::Extends(bound) => Ty::Wildcard(Wildcard::Extends(Box::new(
                substitute_inner(bound, params, args),
            ))),
            Wildcard::Super(bound) => Ty::Wildcard(Wildcard::Super(Box::new(
                substitute_inner(bound, params, args),
            ))),
        },
        Ty::Fn { params: ps, return_type, is_async } => Ty::Fn {
            params: ps
                .iter()
                .map(|p| substitute_inner(p, params, args))
                .collect(),
            return_type: Box::new(substitute_inner(return_type, params, args)),
            is_async: *is_async,
        },
        Ty::Nullable(inner) => Ty::Nullable(Box::new(substitute_inner(inner, params, args))),
        Ty::Primitive(_) | Ty::String | Ty::Void | Ty::Unknown => ty.clone(),
    }
}

// ============================================================================
// Tests
// ============================================================================

// `walk_extends_reaches` / `compose_extends_substitution` follow
// this `mod tests` in source order — they were added after the
// test block was already in place. Clippy normally flags
// "items after test module" because it inhibits incremental
// rebuild caching; for this file the organization is intentional
// (substitution helpers sit next to the types they substitute on),
// so we suppress locally.
#[allow(clippy::items_after_test_module)]
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

/// If `fqn` names a type alias, lower the alias's target into a
/// concrete [`Ty`] and return it. Returns `None` when the FQN
/// doesn't name an alias — the caller falls through to the
/// `Ty::User` branch unchanged.
///
/// Generic aliases: when the alias declares `<A, B>` and the
/// use-site supplies `<int, String>`, each `Ty::Param("A")` in the
/// lowered target is rewritten to `Int` via [`substitute`].
///
/// Bare-name resolution inside the alias target uses the
/// **declaring unit's** context — not the caller's. The current
/// implementation approximates this by lowering through the
/// caller's env and relying on the fact that aliases reference
/// types that are also reachable from the caller (workspace flat
/// lookup). A precise per-alias resolver is deferred.
pub fn expand_alias(
    fqn: &str,
    use_site_args: &[juxc_ast::GenericArg],
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> Option<Ty> {
    // Outer iteration handles alias-of-alias chains:
    // `type A = B; type B = C; type C = int;` lets `A` resolve all
    // the way to `int`. Capped at 16 to bound any malformed cycle
    // the build pass missed.
    let mut fqn_cursor = fqn.to_string();
    let mut args_cursor: Vec<Ty> = use_site_args
        .iter()
        .map(|g| lower_generic_arg(g, env, symbols))
        .collect();
    for _ in 0..16 {
        let alias = symbols.aliases.get(&fqn_cursor)?;
        // Lower the alias's target in a scratch env seeded from
        // the **declaring** unit's name-resolution context — same
        // pattern we used for `extends_fqn` in
        // `resolve_class_chain_fqns`. The alias's own generic
        // params are added on top so `T` inside the target reads
        // as `Ty::Param("T")`.
        let mut scratch = TypeEnv::new();
        if let Some(ctx) = alias
            .unit_index
            .and_then(|idx| symbols.units.get(idx))
        {
            scratch.current_package = ctx.package.clone();
            scratch.unqualified = ctx.unqualified.clone();
        } else {
            // No recorded unit (legacy path / direct test builds) —
            // fall back to the caller's env.
            scratch.current_package = env.current_package.clone();
            scratch.unqualified = env.unqualified.clone();
        }
        for tp in &alias.generic_params {
            scratch.add_generic_param(&tp.name.text);
        }
        // Note: the target is lowered as a TypeRef *without* its
        // alias being re-expanded mid-walk (`ty_from_ref` calls
        // back into `expand_alias` only for top-level matches, so
        // a target that itself references another alias produces
        // a `Ty::User` we then peel below).
        let lowered_target = ty_from_ref(&alias.target, &scratch, symbols);
        let substituted = if alias.generic_params.is_empty() || args_cursor.is_empty() {
            lowered_target
        } else {
            substitute(&lowered_target, &alias.generic_params, &args_cursor)
        };
        // If the result is `Ty::User(other_alias, args)`, drill
        // through to the next alias and loop. Anything else (a
        // class, primitive, etc.) is the final answer.
        if let Ty::User { name: next_fqn, generic_args: next_args } = &substituted {
            if symbols.aliases.contains_key(next_fqn) {
                fqn_cursor = next_fqn.clone();
                args_cursor = next_args.clone();
                continue;
            }
        }
        return Some(substituted);
    }
    // Cycle/over-deep — give up cleanly.
    None
}

/// Lower a [`juxc_ast::GenericArg`] to a [`Ty`]. Concrete types
/// delegate to [`ty_from_ref`]; wildcards become [`Ty::Wildcard`]
/// variants with their bound (if any) recursively lowered.
pub fn lower_generic_arg(
    arg: &juxc_ast::GenericArg,
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> Ty {
    match arg {
        juxc_ast::GenericArg::Type(t) => ty_from_ref(t, env, symbols),
        juxc_ast::GenericArg::Wildcard(w) => match &w.bound {
            None => Ty::Wildcard(Wildcard::Unbounded),
            Some(juxc_ast::WildcardBound::Extends(b)) => {
                Ty::Wildcard(Wildcard::Extends(Box::new(ty_from_ref(b, env, symbols))))
            }
            Some(juxc_ast::WildcardBound::Super(b)) => {
                Ty::Wildcard(Wildcard::Super(Box::new(ty_from_ref(b, env, symbols))))
            }
        },
    }
}

/// Structural subtype check used by PECS variance in
/// [`crate::check::compatible`].
///
/// Returns `true` when `child` is assignable to a slot of type
/// `parent`. The relation is reflexive (every type is a subtype of
/// itself) and walks the class-extends chain — `Dog` is a subtype of
/// `Animal` iff `Dog`'s extends-chain eventually reaches `Animal`.
///
/// Phase 1 keeps the rule narrow:
/// - User-types use class-extends walking; generic args are checked
///   pairwise via this same relation (invariant inside `User`).
/// - Primitives and `String` are equal-only.
/// - `Param` and `Unknown` are wildcards on both sides (matches the
///   permissive behavior of [`crate::check::compatible`]).
///
/// Wildcards are NOT handled here — they live in
/// [`crate::check::compatible`] which calls `is_subtype` to resolve
/// the bound side.
pub fn is_subtype(child: &Ty, parent: &Ty, symbols: &SymbolTable) -> bool {
    // Wildcard escape hatches mirroring `compatible`.
    if matches!(child, Ty::Unknown | Ty::Param(_))
        || matches!(parent, Ty::Unknown | Ty::Param(_))
    {
        return true;
    }
    if child == parent {
        return true;
    }
    match (child, parent) {
        (
            Ty::User { name: cn, generic_args: ca },
            Ty::User { name: pn, generic_args: pa },
        ) => {
            // Same name — recurse pairwise on generic args.
            if cn == pn {
                if ca.is_empty() || pa.is_empty() {
                    return true;
                }
                if ca.len() != pa.len() {
                    return false;
                }
                return ca
                    .iter()
                    .zip(pa.iter())
                    .all(|(x, y)| is_subtype(x, y, symbols));
            }
            // Different names — walk the class-extends chain on the
            // child side. Each hop substitutes through the
            // extends-clause's generic args (composed) until we
            // either hit the parent or run out of chain.
            walk_extends_to(cn, ca, pn, symbols).map_or(false, |composed_args| {
                if pa.is_empty() {
                    return true;
                }
                if composed_args.len() != pa.len() {
                    return false;
                }
                composed_args
                    .iter()
                    .zip(pa.iter())
                    .all(|(x, y)| is_subtype(x, y, symbols))
            })
        }
        (
            Ty::Array { element: e1, kind: k1 },
            Ty::Array { element: e2, kind: k2 },
        ) => k1 == k2 && is_subtype(e1, e2, symbols),
        _ => false,
    }
}

/// True iff `child`'s class-extends chain (transitively) reaches
/// `ancestor`. Used by [`crate::check`] for `protected` visibility
/// — a subclass can access a protected member of any ancestor.
///
/// Unlike [`walk_extends_to`], this variant doesn't compute generic
/// args along the way; it only answers the yes/no chain question.
pub fn walk_extends_reaches(
    child: &str,
    ancestor: &str,
    symbols: &SymbolTable,
) -> bool {
    if child == ancestor {
        return true;
    }
    let mut current = symbols.classes.get(child);
    let mut depth = 0usize;
    while let Some(class) = current {
        if depth > 64 {
            return false;
        }
        // Prefer the pre-resolved `extends_fqn` (set during
        // `resolve_class_chain_fqns`); fall back to the bare last
        // segment so no-package programs still chain.
        let parent_name: &str = match class.extends_fqn.as_deref() {
            Some(fqn) => fqn,
            None => match class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
            {
                Some(s) => s,
                None => return false,
            },
        };
        if parent_name == ancestor {
            return true;
        }
        current = symbols.classes.get(parent_name);
        depth += 1;
    }
    false
}

/// Walk `child_name<child_args>`'s extends chain looking for
/// `target_name`. Returns `Some(args_at_target)` when found — the
/// args are the target's own generic-arg list, composed by
/// substituting the running extends-clause args at each hop. Returns
/// `None` when the chain breaks or the target isn't an ancestor.
///
/// Used by [`is_subtype`] to resolve User-vs-User subtype checks.
/// Distinct from [`compose_extends_substitution`], which returns the
/// (params, args) pair needed for member-type substitution.
fn walk_extends_to(
    child_name: &str,
    child_args: &[Ty],
    target_name: &str,
    symbols: &SymbolTable,
) -> Option<Vec<Ty>> {
    if child_name == target_name {
        return Some(child_args.to_vec());
    }
    let mut current_name = child_name.to_string();
    let mut current_class = symbols.classes.get(&current_name)?;
    let mut current_params: Vec<TypeParam> = current_class.generic_params.clone();
    let mut current_args: Vec<Ty> = child_args.to_vec();
    let mut depth = 0usize;
    loop {
        if depth > 64 {
            return None;
        }
        let extends = current_class.extends.as_ref()?;
        // Prefer the pre-resolved FQN; fall back to the bare last
        // segment when no FQN was recorded (no-package builds).
        let parent_name: String = current_class
            .extends_fqn
            .clone()
            .or_else(|| extends.name.segments.last().map(|s| s.text.clone()))?;
        // Compose: lower extends generic args in the child's scope,
        // then substitute through the running params/args.
        let raw_parent_args: Vec<Ty> = extends
            .generic_args
            .iter()
            .map(|g| match g.as_type() {
                Some(inner) => lower_member_type(inner, &current_name, symbols),
                None => Ty::Unknown,
            })
            .collect();
        let parent_args_final: Vec<Ty> = raw_parent_args
            .iter()
            .map(|a| substitute(a, &current_params, &current_args))
            .collect();
        if parent_name == target_name {
            return Some(parent_args_final);
        }
        let parent_class = symbols.classes.get(&parent_name)?;
        current_name = parent_name;
        current_class = parent_class;
        current_params = parent_class.generic_params.clone();
        current_args = parent_args_final;
        depth += 1;
    }
}

/// Compose the substitution table needed to interpret a member
/// declared in `declaring_class` when accessed through a receiver of
/// `receiver_name<receiver_args>`.
///
/// Walks the extends-chain from receiver up to `declaring_class`. At
/// each hop, the child's `extends Parent<...>` clause names how the
/// parent's generics are bound: lowering those args in the child's
/// scope yields `Ty::Param`-bearing values that we then resolve
/// against the current substitution. The result is the (params, args)
/// pair for `declaring_class`'s own scope, ready to feed
/// [`substitute`].
///
/// Returns [`None`] when:
/// - the receiver name isn't a known class, or
/// - the chain breaks before reaching the declaring class (no
///   `extends` clause on a link), or
/// - any parent name fails to resolve (the resolver should already
///   have surfaced that as E0301).
///
/// Phase 1 caps the chain depth at 64 to keep cyclic / malformed
/// inputs from looping.
pub fn compose_extends_substitution(
    receiver_name: &str,
    receiver_args: &[Ty],
    declaring_class: &str,
    symbols: &SymbolTable,
) -> Option<(Vec<TypeParam>, Vec<Ty>)> {
    let mut current_name = receiver_name.to_string();
    let mut current_class = symbols.classes.get(&current_name)?;
    let mut current_params: Vec<TypeParam> = current_class.generic_params.clone();
    let mut current_args: Vec<Ty> = receiver_args.to_vec();
    let mut depth = 0usize;
    while current_name != declaring_class {
        if depth > 64 {
            return None;
        }
        let extends = current_class.extends.as_ref()?;
        // FQN-aware: prefer the pre-resolved parent name so the
        // chain walk keys directly into the FQN-indexed table.
        let parent_name: String = current_class
            .extends_fqn
            .clone()
            .or_else(|| extends.name.segments.last().map(|s| s.text.clone()))?;
        // Lower the extends-clause's `<...>` args in the child's own
        // generic-param scope so a `Param("U")` reference resolves.
        let raw_parent_args: Vec<Ty> = extends
            .generic_args
            .iter()
            .map(|g| match g.as_type() {
                Some(inner) => lower_member_type(inner, &current_name, symbols),
                None => Ty::Unknown,
            })
            .collect();
        // Compose with the running substitution so `Param("U")`
        // collapses to whatever the receiver bound U to.
        let parent_args_final: Vec<Ty> = raw_parent_args
            .iter()
            .map(|a| substitute(a, &current_params, &current_args))
            .collect();
        let parent_class = symbols.classes.get(&parent_name)?;
        current_name = parent_name;
        current_class = parent_class;
        current_params = parent_class.generic_params.clone();
        current_args = parent_args_final;
        depth += 1;
    }
    Some((current_params, current_args))
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
