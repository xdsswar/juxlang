//! Phase C of the type checker — **expression inference**.
//!
//! [`infer_expr`] walks one [`Expr`] bottom-up and returns its inferred
//! [`Ty`]. [`infer_block`] walks a [`Block`] for its **side effects on
//! the env** — declaring locals as it descends through statements so
//! that subsequent expressions inside the block can be inferred against
//! the correct local-binding types.
//!
//! ## Silent failure
//!
//! Phase C is the inference phase, not the diagnostic phase. **No
//! diagnostics are emitted from this module.** When inference can't
//! determine a type — unknown name, unsupported expression shape,
//! field lookup on a non-class receiver — we return [`Ty::Unknown`]
//! and let Phase D produce the user-facing error at the point where
//! the type is actually needed.
//!
//! ## What the walker doesn't cover (yet)
//!
//! - **Arithmetic coercion** between operands of different numeric
//!   widths. We currently return the left operand's type for any
//!   arithmetic op; Phase D will introduce a real common-type rule.
//! - **Multi-segment paths**. `foo.bar.baz` as an expression returns
//!   `Unknown`. Once imports/qualified-name resolution lands this can
//!   resolve module-level constants or static members.
//! - **`Range` and `null` literals** — neither has a first-class type
//!   in the v1 spec; they stay `Unknown`.
//! - **Cross-extends generic substitution**. When `Dog extends
//!   Animal<int>` and `Animal<T>` exposes `get() -> T`, calling
//!   `d.get()` on a `Dog` still returns `Ty::Param("T")` rather than
//!   `int`. Substitution only fires when the member is declared on the
//!   receiver's own class — threading the extends-clause args needs a
//!   distinct pass that builds the full inheritance substitution chain.

use juxc_ast::{
    BinaryExpr, BinaryOp, Block, CallExpr, CastExpr, ElseBranch, Expr, FieldExpr,
    FloatKind, FloatLit, IndexExpr, IntKind, IntLit, Literal, NewArrayExpr, NewArrayLitExpr,
    NewObjectExpr, OperatorKind, ReturnType, Stmt, SwitchBody, TypeRef, UnaryExpr, UnaryOp,
};

use crate::env::TypeEnv;
use crate::symbol_table::{MethodSig, SymbolTable};
use crate::ty::{
    compose_extends_substitution, explicit_generic_arg_map, infer_generic_args,
    lower_member_type, primitive_from_name, substitute, substitute_via_inference,
    ty_from_ref, ArrayKind, Primitive, Ty,
};

// ============================================================================
// Expression inference
// ============================================================================

/// Method-overload pick (§T.3): count first, then ARGUMENT TYPES.
///
/// Members whose acceptable-count range covers the call are the
/// candidates; with one candidate the count decided (the historical
/// Phase-1 rule). With several, each candidate is scored against the
/// inferred argument types — `2` per exact parameter match, `1` per
/// merely-compatible one, disqualified on any incompatible parameter —
/// and the best score wins (`add(7)` picks `add(int)` over
/// `add(double)`; `add(2.5)` picks `add(double)`). A tie resolves to
/// the FIRST declared candidate (deterministic; identical-shape groups
/// were already rejected at the declaration with E0450).
///
/// Returns the group index (drives `name__ovK` emission) and the
/// picked signature. `None` when the name has no overload group or no
/// member accepts the count.
pub(crate) fn select_method_overload_typed<'a>(
    symbols: &'a SymbolTable,
    class_name: &str,
    method_name: &str,
    c: &CallExpr,
    env: &TypeEnv,
) -> Option<(usize, &'a MethodSig)> {
    // Same exact-key / unique-suffix class resolution as the
    // count-based selector.
    let class = symbols.classes.get(class_name).or_else(|| {
        if class_name.contains('.') {
            return None;
        }
        let suffix = format!(".{class_name}");
        let mut hits = symbols.classes.iter().filter(|(k, _)| k.ends_with(&suffix));
        match (hits.next(), hits.next()) {
            (Some((_, cl)), None) => Some(cl),
            _ => None,
        }
    })?;
    let group = class.method_overloads.get(method_name)?;
    let count = c.args.len();
    let candidates: Vec<(usize, &MethodSig)> = group
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            let (lo, hi) = crate::symbol_table::ctor_arity_range(&m.params);
            count >= lo && hi.map_or(true, |h| count <= h)
        })
        .collect();
    match candidates.len() {
        0 => None,
        1 => Some(candidates[0]),
        _ => {
            let arg_tys: Vec<Ty> =
                c.args.iter().map(|a| infer_expr(a, env, symbols)).collect();
            let mut best: Option<(i32, usize, &MethodSig)> = None;
            for (k, m) in &candidates {
                let mut score = 0i32;
                let mut ok = true;
                for (i, at) in arg_tys.iter().enumerate() {
                    // Defaults / varargs tails score neutrally.
                    let Some(p) = m.params.get(i) else { continue };
                    let pt = ty_from_ref(&p.ty, env, symbols);
                    if pt == *at {
                        score += 2;
                    } else if crate::check::compatible(&pt, at, symbols) {
                        score += 1;
                    } else {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
                if best.as_ref().map_or(true, |(bs, ..)| score > *bs) {
                    best = Some((score, *k, m));
                }
            }
            best.map(|(_, k, m)| (k, m)).or_else(|| Some(candidates[0]))
        }
    }
}

/// Infer the type of `expr` against `env` and `symbols`.
///
/// Returns [`Ty::Unknown`] for any expression the walker can't yet
/// figure out — never panics, never emits diagnostics. See the module
/// doc for the full coverage table.
pub fn infer_expr(expr: &Expr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    match expr {
        Expr::Literal(lit) => infer_literal(lit),
        // `typeof(expr)` (§5.9.10) — a compile-time String of the
        // operand's static type name.
        Expr::TypeOf(..) => Ty::String,
        // `out <place>` (§M.4) — the argument's type is the place's type, so
        // the existing call-arg type check (E0410) fires on a mismatch.
        Expr::Out(inner, _) => infer_expr(inner, env, symbols),
        // Tuple literal (§5.3) — encoded as the `__tuple` sentinel
        // user-type with the element types as generic args (same
        // encoding `ty_from_ref` produces for `(A, B)` type refs),
        // so compatibility falls out of the ordinary name+args rule.
        Expr::TupleLit(elems, _) => Ty::User {
            name: juxc_ast::TUPLE_SENTINEL.to_string(),
            generic_args: elems.iter().map(|e| infer_expr(e, env, symbols)).collect(),
        },
        // `expr?` (§X.4.1): Ok-value of a Result operand, or the
        // non-null value of a nullable one.
        Expr::ErrorProp(inner, _) => match infer_expr(inner, env, symbols) {
            Ty::Nullable(boxed) => *boxed,
            Ty::User { name, generic_args }
                if name.rsplit('.').next() == Some("Result")
                    && generic_args.len() == 2 =>
            {
                generic_args[0].clone()
            }
            _ => Ty::Unknown,
        },
        // Try-expression (§X.3.3): the value is the try block's
        // trailing expression (the catch blocks must produce the
        // same shape; rustc verifies exact agreement).
        Expr::TryExpr(t) => match t.body.statements.last() {
            Some(juxc_ast::Stmt::Expr(tail)) => infer_expr(tail, env, symbols),
            _ => Ty::Unknown,
        },
        Expr::Path(qn) => {
            // Single-segment path → look up as a local. Multi-segment
            // paths could resolve to enum-variants or imported names,
            // but neither is wired up yet — both yield Unknown.
            if qn.segments.len() == 1 {
                let name = &qn.segments[0].text;
                if let Some(ty) = env.lookup(name) {
                    return ty.clone();
                }
                // Enclosing-class static fallback: inside a class
                // body, a bare name may refer to a static field of
                // the current class (Java rule — `a` ≡ `Test.a`
                // inside `class Test`). The walker uses the FQN
                // stored in `env.current_class` and consults the
                // symbol table directly so we pick up the same
                // type substitution `infer_field`'s class-static
                // branch produces for the qualified form.
                if let Some(class_fqn) = &env.current_class {
                    if let Some(class) = symbols.classes.get(class_fqn) {
                        if let Some(field) = class.fields.get(name.as_str()) {
                            if field.is_static {
                                return lower_member_type(&field.ty, class_fqn, symbols);
                            }
                        }
                    }
                }
            }
            Ty::Unknown
        }
        Expr::This(_) => infer_this(env),
        Expr::Super(_) => infer_super(env, symbols),
        // `x => T` is a runtime type test — always boolean.
        Expr::TypeTest(_) => Ty::Primitive(Primitive::Bool),
        Expr::Field(f) => infer_field(f, env, symbols),
        Expr::Index(i) => infer_index(i, env, symbols),
        Expr::Call(c) => infer_call(c, env, symbols),
        Expr::NewObject(n) => infer_new_object(n, env, symbols),
        Expr::NewArray(n) => infer_new_array(n, env, symbols),
        Expr::NewArrayLit(n) => infer_new_array_lit(n, env, symbols),
        Expr::Cast(c) => infer_cast(c, env, symbols),
        Expr::Range(_) => Ty::Unknown,
        Expr::Unary(u) => infer_unary(u, env, symbols),
        Expr::Binary(b) => infer_binary(b, env, symbols),
        Expr::SizeOf(_) => Ty::Primitive(Primitive::Int),
        Expr::InterpString(_) => Ty::String,
        Expr::Switch(s) => {
            // The arm-unification work is Phase D's job. For now we
            // pick the first arm's body type as a representative —
            // good enough for downstream code that just wants *some*
            // type to forward.
            if let Some(first) = s.arms.first() {
                match &first.body {
                    SwitchBody::Expr(e) => infer_expr(e, env, symbols),
                    SwitchBody::Block(_) => Ty::Void,
                }
            } else {
                Ty::Unknown
            }
        }
        // Lambda — Phase-1 returns `Ty::Unknown`. A proper
        // `Ty::Fn { params, return }` lands when call-site type
        // checking actually consumes the result (e.g. when
        // passing a lambda to a `Fn`-typed param). Today the
        // emitted Rust closure infers its own type at compile
        // time, so the lack of a precise Jux-side type is
        // observationally a no-op.
        Expr::Lambda(_) => Ty::Unknown,
        // Elvis: result type is the fallback's type. Both sides are
        // expected to share an inner type (Phase 1 doesn't enforce
        // it yet — that's a future tycheck refinement).
        Expr::Elvis(e) => infer_expr(&e.fallback, env, symbols),
        // Method reference: a function-typed value. Phase 1
        // doesn't track function types as a concrete `Ty`
        // variant beyond what `Ty::Unknown` allows for
        // higher-order callbacks; emitting `Unknown` keeps the
        // call-site flow open for the backend to wire up.
        Expr::MethodRef(_) => Ty::Unknown,
        // Ternary: take the then-branch's type as the result
        // type. The else-branch should unify; Phase 1 doesn't
        // enforce that here — rustc surfaces real mismatches on
        // the emitted `if`. Generic / numeric coercion across
        // branches is a future tycheck refinement.
        Expr::Ternary(t) => infer_expr(&t.then_branch, env, symbols),
        // `await expr` resolves to the operand's value type. In a
        // proper Future model the operand would be `Future<T>` and
        // this would unwrap to `T`; Phase 1 doesn't track Future
        // shapes structurally — `expr` is already typed as `T` at
        // its definition site (the `async T` return type lowers to
        // `T` everywhere outside the emission boundary), so the
        // operand's type is the right answer.
        Expr::Await(inner, _) => infer_expr(inner, env, symbols),
        // `expr!!` asserts non-null: the result type is the operand's
        // type with the nullable layer peeled (conversion table T? -> T).
        Expr::NotNullAssert(inner, _) => match infer_expr(inner, env, symbols) {
            Ty::Nullable(t) => *t,
            other => other,
        },
        // `++place` / `place++` (§A `incdec`, value form). The result
        // is the operand's own (numeric) type — `var y = x++;` gives `y`
        // the type of `x`, and prefix/postfix don't change that (only
        // WHICH value, old vs new, is yielded). The numeric/assignable
        // validation lives in `check_expr`; inference just forwards the
        // place's type so downstream var-decl inference works.
        Expr::IncDec(i) => infer_expr(&i.target, env, symbols),
    }
}

/// Map a literal onto its Ty.
///
/// - **Int**: the suffix decides — `42L` → `long`, `42u` → `uint`, etc.
///   Unsuffixed → `int`.
/// - **Float**: `1.5f` → `float`, otherwise `double`.
/// - **String**: always `Ty::String`.
/// - **Bool**: `Ty::Primitive(Bool)`.
/// - **Null**: `Unknown` — Jux doesn't have a first-class null type.
pub(crate) fn infer_literal(lit: &Literal) -> Ty {
    match lit {
        Literal::Int(IntLit { kind, .. }) => Ty::Primitive(primitive_from_int_kind(*kind)),
        Literal::Float(FloatLit { kind, .. }) => Ty::Primitive(primitive_from_float_kind(*kind)),
        Literal::String(_) => Ty::String,
        Literal::Bool(_) => Ty::Primitive(Primitive::Bool),
        Literal::Char(_) => Ty::Primitive(Primitive::Char),
        // `null` has no concrete inner type until context fixes
        // it (e.g. `String? x = null;`). We model it as a nullable
        // wrapper around `Unknown`; the compatibility predicate
        // treats `Nullable(Unknown)` as a wildcard `null` value
        // that fits any `T?` slot. The inner stays `Unknown` so
        // type-error printers know to elide it.
        Literal::Null => Ty::Nullable(Box::new(Ty::Unknown)),
    }
}

/// Translate the lexer-supplied int-suffix into a [`Primitive`]. Used
/// only by [`infer_literal`].
fn primitive_from_int_kind(kind: Option<IntKind>) -> Primitive {
    match kind {
        None => Primitive::Int,
        Some(IntKind::Byte) => Primitive::Byte,
        Some(IntKind::UByte) => Primitive::Ubyte,
        Some(IntKind::Short) => Primitive::Short,
        Some(IntKind::UShort) => Primitive::Ushort,
        Some(IntKind::UInt) => Primitive::Uint,
        Some(IntKind::Long) => Primitive::Long,
        Some(IntKind::ULong) => Primitive::Ulong,
    }
}

/// Translate the lexer-supplied float-suffix into a [`Primitive`].
fn primitive_from_float_kind(kind: Option<FloatKind>) -> Primitive {
    match kind {
        None => Primitive::Double,
        Some(FloatKind::Float) => Primitive::Float,
    }
}

/// `this` inside a class context lowers to `Ty::User { name: <class>, … }`
/// with each in-scope generic parameter materialized as a
/// [`Ty::Param`]. Outside a class context we return `Unknown` —
/// the parser already rejects `this` outside a class, but we stay
/// silent here per Phase C's no-diagnostics rule.
///
/// Generic-arg ordering note: the env stores generic params in a
/// `HashSet`, which has no defined iteration order, so the args list
/// we produce here is **unordered**. That's acceptable for Phase C —
/// downstream code that cares about ordering (e.g. Phase D's
/// signature unification) will need to read the params off the
/// symbol table directly.
fn infer_this(env: &TypeEnv) -> Ty {
    match &env.current_class {
        Some(name) => {
            let generic_args = env
                .generic_params
                .iter()
                .map(|p| Ty::Param(p.clone()))
                .collect();
            Ty::User {
                name: name.clone(),
                generic_args,
            }
        }
        None => Ty::Unknown,
    }
}

/// Infer the type of `super` — the **superclass** of the enclosing class
/// (§6.9.4). A `super.method(args)` call then resolves the method on the
/// parent type via the ordinary call-inference path, which is exactly the
/// static-dispatch semantics `super` requires: it picks the nearest ancestor
/// definition regardless of the current class's override. `Unknown` outside a
/// class or when the class has no superclass (a bare `super` there is rejected
/// by the checker).
fn infer_super(env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    let Some(current) = &env.current_class else {
        return Ty::Unknown;
    };
    match symbols.classes.get(current).and_then(|c| c.extends_fqn.clone()) {
        Some(parent_fqn) => Ty::User {
            name: parent_fqn,
            generic_args: Vec::new(),
        },
        None => Ty::Unknown,
    }
}

/// `object.field`. Three shapes are recognized:
///
/// 1. **`.length` on an array** — every array carries a `length` of
///    type `int`. Special-cased before consulting the symbol table.
/// 2. **Field on a user class** — walks the `extends` chain
///    ([`SymbolTable::lookup_field`]) so a `Dog extends Animal` can read
///    Animal's fields. When the field is declared on the receiver's own
///    class AND the receiver carries concrete generic arguments, the
///    field's type is **substituted** through the receiver's generic
///    args before being returned: a `Box<int>` with `T value` reads as
///    `int`, not `Ty::Param("T")`.
/// 3. **Component on a record** — same idea, but records have no
///    inheritance, so no chain walk. Substitution still applies when
///    the record is generic and the receiver carries arguments.
///
/// Everything else (field on a primitive, field on an enum, etc.)
/// returns `Unknown`.
/// If `inner` (`recv.fieldName`) reads a **`weak`** field (§6.5), return that
/// field's declared type lowered in the receiver class's generic scope; else
/// `None`. Powers the `weakField.get()` → `T?` typing — mirrors the field-type
/// lowering in [`infer_field`] (lower in the declaring class's scope, then
/// compose the extends-chain substitution), but gated on `is_weak` so an
/// ordinary `obj.field.get()` is left untouched.
fn weak_field_target_ty(inner: &FieldExpr, env: &TypeEnv, symbols: &SymbolTable) -> Option<Ty> {
    let object_ty = infer_expr(&inner.object, env, symbols);
    let field_name = inner.field.text.as_str();
    if let Ty::User { name, generic_args } = &object_ty {
        if let Some((field, declaring_class)) = symbols.lookup_field(name, field_name) {
            if !field.is_weak {
                return None;
            }
            let raw = lower_member_type(&field.ty, declaring_class, symbols);
            if let Some((params, args)) =
                compose_extends_substitution(name, generic_args, declaring_class, symbols)
            {
                return Some(substitute(&raw, &params, &args));
            }
            return Some(raw);
        }
    }
    None
}

fn infer_field(f: &FieldExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // `ClassName.STATIC_FIELD` — when the receiver is a bare or
    // multi-segment path that resolves to a class FQN, look the
    // field up as a static member rather than as an instance field.
    // Instance access `obj.f` still flows through the regular
    // `infer_expr` path below; the two are distinguished by
    // whether the receiver expression names a type or a value.
    if let Expr::Path(qn) = f.object.as_ref() {
        if let Some(class_fqn) = path_resolves_to_class(qn, env, symbols) {
            if let Some(class) = symbols.classes.get(&class_fqn) {
                if let Some(field) = class.fields.get(f.field.text.as_str()) {
                    if field.is_static {
                        return lower_member_type(&field.ty, &class_fqn, symbols);
                    }
                }
            }
        }
        // `IfaceName.FIELD` — interface fields are implicitly
        // `public static final`, so the receiver-as-type-name
        // path applies. Inferring as the field's declared type
        // matches the class-static branch above.
        if let Some(iface_fqn) = path_resolves_to_interface(qn, env, symbols) {
            if let Some(iface) = symbols.interfaces.get(&iface_fqn) {
                if let Some(field) = iface.fields.get(f.field.text.as_str()) {
                    return lower_member_type(&field.ty, &iface_fqn, symbols);
                }
            }
        }
        // Enum-variant access: `Color.Red`, `Token.Number`. The
        // path resolves to the enum type itself, so the result
        // type is `Ty::User { name: <enum> }` — same shape an
        // instance of the enum would carry. This is what powers
        // smart-cast on `var c = Color.Red` and feeds the
        // exhaustiveness check (`check_switch_exhaustive`) the
        // right scrutinee type.
        if qn.segments.len() == 1 {
            let enum_name = &qn.segments[0].text;
            // FQN-aware lookup: a bare `Tier` resolves to the
            // table's `probe.Tier` key, and we return THAT name so
            // exhaustiveness / variant checks downstream hit the
            // table directly.
            if let Some((fqn, e)) =
                symbols.lookup_enum_in(enum_name, &env.current_package.join("."))
            {
                if e.variants.contains_key(&f.field.text) {
                    return Ty::User {
                        name: fqn.to_string(),
                        generic_args: Vec::new(),
                    };
                }
            }
            // §K.11 primitive-type constants: `int.MAX_VALUE`,
            // `double.NAN`, … typed as the primitive itself. The
            // receiver is the type NAME (a keyword — no shadowing).
            if let Some(prim) = primitive_from_name(&qn.segments[0].text) {
                let is_float = matches!(
                    prim,
                    Primitive::Float | Primitive::Double | Primitive::F32 | Primitive::F64
                );
                let known = match f.field.text.as_str() {
                    "MIN_VALUE" | "MAX_VALUE" => {
                        !matches!(prim, Primitive::Bool | Primitive::Char)
                    }
                    "NAN" | "POSITIVE_INFINITY" | "NEGATIVE_INFINITY" | "EPSILON" => is_float,
                    _ => false,
                };
                if known {
                    return Ty::Primitive(prim);
                }
            }
        }
    }
    let object_ty = infer_expr(&f.object, env, symbols);
    let field_name = f.field.text.as_str();

    // AsyncMutex guard (§18.3): `guard.value` is the protected T.
    if let Ty::User { name, generic_args } = &object_ty {
        if name == "__AsyncMutexGuard" && field_name == "value" {
            return generic_args.first().cloned().unwrap_or(Ty::Unknown);
        }
    }

    // Tuple element access — `pair.0` / `pair.1` (§5.3). The
    // receiver is the `__tuple` sentinel user-type; the numeric
    // field indexes its generic args.
    if let Ty::User { name, generic_args } = &object_ty {
        if name == juxc_ast::TUPLE_SENTINEL {
            if let Ok(idx) = field_name.parse::<usize>() {
                if let Some(elem) = generic_args.get(idx) {
                    return elem.clone();
                }
            }
            return Ty::Unknown;
        }
    }

    // `.length` on any array → int.
    if let Ty::Array { .. } = &object_ty {
        if field_name == "length" {
            return Ty::Primitive(Primitive::Int);
        }
    }

    // Field on a user type.
    if let Ty::User { name, generic_args } = &object_ty {
        if let Some((field, declaring_class)) = symbols.lookup_field(name, field_name) {
            // Lower in the declaring class's generic-param scope so a
            // `T value;` field reads as `Ty::Param("T")` rather than
            // `Unknown` when we're outside Box's body.
            let raw = lower_member_type(&field.ty, declaring_class, symbols);
            // Compose the substitution through the extends-chain so a
            // child's `extends Parent<int>` propagates `T → int` onto
            // an inherited field. When declaring_class == name the
            // composition reduces to one hop (the receiver's own
            // scope) and behaves like the previous direct path.
            if let Some((params, args)) = compose_extends_substitution(
                name,
                generic_args,
                declaring_class,
                symbols,
            ) {
                return substitute(&raw, &params, &args);
            }
            return raw;
        }
        // §M.7 / §P property read — `obj.Name`. The backing field is
        // mangled and the getter lives in `methods`, so the property's
        // declared type is the authoritative static type. Lower it in the
        // declaring class's scope and compose the extends substitution so an
        // inherited generic property reads through `extends Parent<int>`.
        if let Some((prop, declaring_class)) = symbols.lookup_property(name, field_name) {
            let raw = lower_member_type(&prop.ty, declaring_class, symbols);
            if let Some((params, args)) = compose_extends_substitution(
                name,
                generic_args,
                declaring_class,
                symbols,
            ) {
                return substitute(&raw, &params, &args);
            }
            return raw;
        }
        if let Some(record) = symbols.records.get(name) {
            if let Some(component) = record.components.iter().find(|c| c.name == field_name) {
                let raw = lower_member_type(&component.ty, name, symbols);
                return substitute(&raw, &record.generic_params, generic_args);
            }
        }
    }

    Ty::Unknown
}

/// `array[index]` returns the array's element type, or Unknown when
/// the LHS doesn't infer to an array.
fn infer_index(i: &IndexExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    let array_ty = infer_expr(&i.array, env, symbols);
    match array_ty {
        Ty::Array { element, .. } => *element,
        // `operator[]` (§O.2.4): the overload's declared return type.
        ref user @ Ty::User { ref name, ref generic_args } => {
            if let Some(ret) =
                lookup_user_operator_return_type(user, OperatorKind::Index, env, symbols)
            {
                return ret;
            }
            // Builtin Rust-std container indexing (no user `operator[]`
            // declared): `Vec<T>`/`VecDeque<T>` index to the element,
            // `HashMap<K,V>`/`BTreeMap<K,V>` to the value. Without this
            // the element type is `Unknown` and the backend can't see a
            // wrapper-class element behind `xs[i]` (its field reads then
            // skip the `.0.borrow()` rewrite — a rustc E0609 leak).
            match name.rsplit('.').next().unwrap_or(name) {
                "Vec" | "VecDeque" => {
                    generic_args.first().cloned().unwrap_or(Ty::Unknown)
                }
                "HashMap" | "BTreeMap" => {
                    generic_args.get(1).cloned().unwrap_or(Ty::Unknown)
                }
                _ => Ty::Unknown,
            }
        }
        _ => Ty::Unknown,
    }
}

/// `callee(args…)`. Two callee shapes are handled:
///
/// 1. **Bare single-segment path** — looks up a top-level function in
///    `symbols.functions` and returns its declared return type.
/// 2. **Field-on-receiver** — looks up a method via the
///    [`SymbolTable::lookup_method`] inheritance walk (for classes) or
///    by direct name (for interfaces). When the method is found on the
///    receiver's own class and the receiver carries concrete generic
///    arguments, the return type is substituted through them — a
///    `Box<int>::get()` reads as `int` rather than `Ty::Param("T")`.
///
/// Anything else (call on a `Call` result, call on an `Index`, etc.)
/// returns `Unknown`. Overload resolution (multiple methods sharing a
/// name) lands in a later phase — the symbol-table builder still
/// rejects duplicates with `E0402`, so today there's at most one
/// candidate per name.
fn infer_call(c: &CallExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // `operator()` (§O.2.4): a callee whose type declares the call
    // overload produces the overload's return type. Checked before
    // the named-function path so a callable LOCAL wins over a
    // same-named free function (locals shadow).
    {
        let callee_ty = infer_expr(&c.callee, env, symbols);
        if matches!(callee_ty, Ty::User { .. }) {
            if let Some(ret) =
                lookup_user_operator_return_type(&callee_ty, OperatorKind::Call, env, symbols)
            {
                return ret;
            }
        }
    }
    match c.callee.as_ref() {
        // Top-level function — `helper(x)`.
        Expr::Path(qn) if qn.segments.len() == 1 => {
            let name = &qn.segments[0].text;
            // `spawn(f)` builtin (§18.1.3) returns a Task handle the
            // Jux type system doesn't model yet — Unknown keeps the
            // method calls on it permissive (rustc verifies against
            // the emitted JuxTask). Checked BEFORE the function
            // lookup so the rust.std thread-spawn stub's JoinHandle
            // doesn't capture the name.
            if name == "spawn" || name == "withTimeout" {
                return Ty::Unknown;
            }
            // `block_on(fut)` resolves to the future's value — for
            // typed runtime calls (m.lock(), ch.receive()) the
            // argument's inferred type IS the value type.
            if name == "block_on" {
                return c
                    .args
                    .first()
                    .map(|a| infer_expr(a, env, symbols))
                    .unwrap_or(Ty::Unknown);
            }
            if let Some((_, fn_sig)) = symbols.lookup_function(name) {
                // Generic inference (spec §T.4): when the callee is
                // generic and the call site didn't write explicit
                // `<…>`, try to recover the type args from the
                // argument types. Only the bare-param-name shape is
                // handled — see `infer_generic_args` for the rules.
                if fn_sig.generic_params.is_empty() {
                    return return_type_to_ty(&fn_sig.return_type, env, symbols);
                }
                // Lower the return type in a scratch env that has the
                // function's own generic params in scope — otherwise a
                // bare `T` return type lowers to `Ty::Unknown` in the
                // caller's env and substitution has nothing to grab.
                let base = return_type_to_ty_in_fn_scope(
                    &fn_sig.return_type,
                    &fn_sig.generic_params,
                    env,
                    symbols,
                );
                // Explicit call-site type args (`id<int>(5)`) bind the
                // params directly; otherwise recover them from the
                // argument types (spec §T.4).
                let inferred = if !c.explicit_generic_args.is_empty() {
                    explicit_generic_arg_map(
                        &fn_sig.generic_params,
                        &c.explicit_generic_args,
                        env,
                        symbols,
                    )
                } else {
                    let param_tys: Vec<&TypeRef> =
                        fn_sig.params.iter().map(|p| &p.ty).collect();
                    let arg_tys: Vec<Ty> = c
                        .args
                        .iter()
                        .map(|a| infer_expr(a, env, symbols))
                        .collect();
                    infer_generic_args(
                        &fn_sig.generic_params,
                        &param_tys,
                        &arg_tys,
                    )
                };
                return substitute_via_inference(
                    &base,
                    &fn_sig.generic_params,
                    &inferred,
                );
            }
            Ty::Unknown
        }
        // Method call — `obj.method(args)`.
        Expr::Field(field) => {
            let method_name = field.field.text.as_str();
            // `ClassName.staticMethod(args)` — receiver is a type
            // name, not a value. Resolve the static method
            // directly off the class's signature and return its
            // declared return type (lowered in the class's scope).
            if let Expr::Path(qn) = field.object.as_ref() {
                if let Some(class_fqn) = path_resolves_to_class(qn, env, symbols) {
                    // Overload-group pick (§T.3, count + types) —
                    // overloads may differ in return type, so the
                    // selected member's return drives inference.
                    let picked =
                        select_method_overload_typed(symbols, &class_fqn, method_name, c, env)
                            .map(|(_, m)| m);
                    if let Some(class) = symbols.classes.get(&class_fqn) {
                        if let Some(method) =
                            picked.or_else(|| class.methods.get(method_name))
                        {
                            if method.is_static {
                                return return_type_in_class(
                                    &method.return_type,
                                    &class_fqn,
                                    symbols,
                                );
                            }
                        }
                    }
                }
            }
            // `Stream.<ctor>` statics (§18.6.4) — `Stream` is a builtin,
            // not a class, so the class-static path above can't type it.
            // The element type comes from an explicit type arg
            // (`Stream.of<int>()`), else the first argument (`of`) or
            // its array element (`from`); `generate` infers `Unknown`
            // (the lambda's return isn't tracked — rustc pins it).
            if let Expr::Path(qn) = field.object.as_ref() {
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Stream"
                    && !symbols.classes.contains_key("Stream")
                    && matches!(method_name, "of" | "from" | "generate")
                {
                    let elem = if let Some(t) = c.explicit_generic_args.first() {
                        ty_from_ref(t, env, symbols)
                    } else {
                        match method_name {
                            "of" => c
                                .args
                                .first()
                                .map(|a| infer_expr(a, env, symbols))
                                .unwrap_or(Ty::Unknown),
                            "from" => match c
                                .args
                                .first()
                                .map(|a| infer_expr(a, env, symbols))
                            {
                                Some(Ty::Array { element, .. }) => *element,
                                _ => Ty::Unknown,
                            },
                            _ => Ty::Unknown,
                        }
                    };
                    return Ty::User {
                        name: "Stream".to_string(),
                        generic_args: vec![elem],
                    };
                }
            }
            // Weak-field promotion (§6.5): `weakField.get()` → `T?`. A weak
            // field's strong view is reached only through `.get()`, which may
            // fail because the target may have been dropped — hence the
            // nullable result. Checked before the generic receiver path so the
            // bare weak-field read of `field.object` is never typed as a value.
            if method_name == "get" && c.args.is_empty() {
                if let Expr::Field(inner) = field.object.as_ref() {
                    if let Some(target) = weak_field_target_ty(inner, env, symbols) {
                        return Ty::nullable(target);
                    }
                }
                // Weak-PARAMETER promotion (§M.14.3): `weakParam.get()` → `T?`.
                // A weak param's `lookup` type is its class `T`; `.get()` is the
                // weak→strong promotion, nullable because the referent may be dead.
                if let Expr::Path(qn) = field.object.as_ref() {
                    if qn.segments.len() == 1 && env.weak_names.contains(&qn.segments[0].text) {
                        if let Some(target) = env.lookup(&qn.segments[0].text) {
                            return Ty::nullable(target.clone());
                        }
                    }
                }
            }
            let receiver_ty = infer_expr(&field.object, env, symbols);
            // Channel<T> (§18.3) — async-runtime builtin: `receive()`
            // yields `T?` (null when closed+drained); send/close are
            // void. Typed here so nullable machinery (Some-lifting,
            // `== null`, `!!`) works on channel reads.
            if let Ty::User { name, generic_args } = &receiver_ty {
                if name.rsplit('.').next() == Some("Channel") {
                    return match method_name {
                        "receive" => Ty::nullable(
                            generic_args.first().cloned().unwrap_or(Ty::Unknown),
                        ),
                        _ => Ty::Void,
                    };
                }
                // AsyncMutex<T> (§18.3): `lock()` yields the guard
                // (a sentinel user-type carrying T); `guard.value`
                // reads/writes route through infer_field below.
                if name.rsplit('.').next() == Some("AsyncMutex") && method_name == "lock" {
                    return Ty::User {
                        name: "__AsyncMutexGuard".to_string(),
                        generic_args: generic_args.clone(),
                    };
                }
                // Stream<T> (§18.6) — `next()` yields `T?` (null =
                // exhausted); combinators return streams (`mapAsync`
                // widens the element to Unknown — the lambda's return
                // isn't tracked; rustc pins it).
                if name.rsplit('.').next() == Some("Stream")
                    && !symbols.classes.contains_key("Stream")
                {
                    return match method_name {
                        "next" => Ty::nullable(
                            generic_args.first().cloned().unwrap_or(Ty::Unknown),
                        ),
                        "mapAsync" => Ty::User {
                            name: "Stream".to_string(),
                            generic_args: vec![Ty::Unknown],
                        },
                        "filterAsync" | "take" | "skip" | "chain" => receiver_ty.clone(),
                        _ => Ty::Unknown,
                    };
                }
            }
            if let Ty::User { name, generic_args } = &receiver_ty {
                // Walk the class extends-chain first.
                if let Some((method, declaring_class)) =
                    symbols.lookup_method(name, method_name)
                {
                    // Overload-group pick (§T.3, count + types): a
                    // group's members may differ in return type.
                    let method =
                        select_method_overload_typed(symbols, name, method_name, c, env)
                            .map(|(_, m)| m)
                            .unwrap_or(method);
                    // Lower in the declaring class's generic scope AND the
                    // method's own generic params so both `T get()` (class
                    // param) and `<U> U pick()` (method param) read as
                    // `Param(..)`, not `Unknown` — the call-site inference then
                    // substitutes the concrete type in.
                    let raw = return_type_in_method(
                        &method.return_type,
                        declaring_class,
                        &method.generic_params,
                        symbols,
                    );
                    // Compose the substitution through the
                    // extends-chain (see `infer_field` for the same
                    // pattern). For a direct method on the receiver
                    // this collapses to a single hop.
                    let after_class = match compose_extends_substitution(
                        name,
                        generic_args,
                        declaring_class,
                        symbols,
                    ) {
                        Some((params, args)) => substitute(&raw, &params, &args),
                        None => raw,
                    };
                    return method_infer_return(
                        &after_class,
                        method,
                        declaring_class,
                        &c.args,
                        &c.explicit_generic_args,
                        env,
                        symbols,
                    );
                }
                // Record methods — records can declare methods per
                // grammar §A.2.4. No inheritance chain (records don't
                // extend), but substitution applies for the record's
                // own generic params.
                if let Some(record) = symbols.records.get(name) {
                    // §M.5 synthesized wither: `r.with(name: v, …)`
                    // returns a NEW record of the same type. A
                    // user-declared `with` method (below) shadows the
                    // synthesized one.
                    if method_name == "with" && !record.methods.contains_key("with") {
                        return receiver_ty.clone();
                    }
                    if let Some(method) = record.methods.get(method_name) {
                        let raw = return_type_in_method(
                            &method.return_type,
                            name,
                            &method.generic_params,
                            symbols,
                        );
                        let after_class =
                            substitute(&raw, &record.generic_params, generic_args);
                        return method_infer_return(
                            &after_class,
                            method,
                            name,
                            &c.args,
                            &c.explicit_generic_args,
                            env,
                            symbols,
                        );
                    }
                }
                // Interface methods. No chain (interfaces don't extend
                // classes), but substitution still applies for the
                // interface's own generic params.
                if let Some(iface) = symbols.interfaces.get(name) {
                    if let Some(method) = iface.methods.get(method_name) {
                        let raw = return_type_in_method(
                            &method.return_type,
                            name,
                            &method.generic_params,
                            symbols,
                        );
                        let after_class =
                            substitute(&raw, &iface.generic_params, generic_args);
                        return method_infer_return(
                            &after_class,
                            method,
                            name,
                            &c.args,
                            &c.explicit_generic_args,
                            env,
                            symbols,
                        );
                    }
                }
                // Enum methods (§A.2.5) — declared in the enum body
                // after the variant list. No inheritance chain; the
                // enum's own generic params substitute.
                if let Some(enum_sig) = symbols.enums.get(name) {
                    if let Some(method) = enum_sig.methods.get(method_name) {
                        let raw = return_type_in_method(
                            &method.return_type,
                            name,
                            &method.generic_params,
                            symbols,
                        );
                        return method_infer_return(
                            &raw,
                            method,
                            name,
                            &c.args,
                            &c.explicit_generic_args,
                            env,
                            symbols,
                        );
                    }
                }
            }
            // Stdlib method return-type fallback — covers
            // `String.trim()`, `xs.map(f)`, etc. so the chained
            // method call (`s.trim().startsWith(...)`) carries a
            // typed receiver through to the next stage of
            // inference. Without this the chain collapses to
            // `Unknown` after the first stdlib hop and the
            // backend can't dispatch.
            if let Some(ty) = infer_stdlib_method(&receiver_ty, method_name, &c.args, env, symbols) {
                return ty;
            }
            Ty::Unknown
        }
        _ => Ty::Unknown,
    }
}

/// Return type inference for `BUILTIN_*_METHODS` calls. Returns
/// `Some(Ty)` when `method_name` matches a known stdlib method on
/// a String or Array receiver, `None` otherwise.
///
/// Phase-1 coverage: enough to keep method chains typed end-to-end.
/// Receivers that aren't `Ty::String` or `Ty::Array { .. }` are
/// ignored.
fn infer_stdlib_method(
    receiver_ty: &Ty,
    method_name: &str,
    _args: &[Expr],
    _env: &TypeEnv,
    _symbols: &SymbolTable,
) -> Option<Ty> {
    use crate::ty::Primitive;
    match receiver_ty {
        Ty::String => match method_name {
            // String → String
            "trim" | "toUpperCase" | "toLowerCase" | "replace" | "substring"
            | "repeat" | "to_string" | "clone" => Some(Ty::String),
            // String → uint. `len()` is the Rust `str::len()` byte count, which
            // returns `usize`; typing it `uint` keeps it consistent with the
            // emitted Rust (and with `Vec::len()`), so a mixed-type use coerces
            // through the binary-op promotion and `int x = s.len()` gives a clean
            // E0410 (use the `s.length` property, or a cast, for a signed length).
            "len" => Some(Ty::Primitive(Primitive::Uint)),
            // String → int
            "length" | "indexOf" | "byteLength" | "charLength" => {
                Some(Ty::Primitive(Primitive::Int))
            }
            // String → bool
            "contains" | "startsWith" | "endsWith" | "isEmpty" => {
                Some(Ty::Primitive(Primitive::Bool))
            }
            // String → char
            "charAt" => Some(Ty::Primitive(Primitive::Char)),
            // String → List<String>
            "split" => Some(Ty::Array {
                element: Box::new(Ty::String),
                kind: crate::ty::ArrayKind::Dynamic,
            }),
            _ => None,
        },
        // §K.11 numeric / char intrinsics on primitive receivers.
        // Checked forms produce the Jux `Result<T, ArithmeticException>`
        // enum so `switch`/`?`-propagation see the real shape.
        Ty::Primitive(prim) => {
            let prim = *prim;
            let is_float = matches!(
                prim,
                Primitive::Float | Primitive::Double | Primitive::F32 | Primitive::F64
            );
            let is_char = matches!(prim, Primitive::Char);
            let is_int = !is_float && !is_char && !matches!(prim, Primitive::Bool);
            let checked_result = |ok: Ty| Ty::User {
                name: "jux.std.result.Result".to_string(),
                generic_args: vec![
                    ok,
                    Ty::User {
                        name: "jux.std.exceptions.ArithmeticException".to_string(),
                        generic_args: Vec::new(),
                    },
                ],
            };
            if is_char {
                return match method_name {
                    "isDigit" | "isAlphabetic" | "isWhitespace" | "isUppercase"
                    | "isLowercase" => Some(Ty::Primitive(Primitive::Bool)),
                    "toUppercase" | "toLowercase" => Some(Ty::Primitive(Primitive::Char)),
                    "codePoint" => Some(Ty::Primitive(Primitive::Uint)),
                    _ => None,
                };
            }
            if is_float {
                return match method_name {
                    "sqrt" | "floor" | "ceil" | "round" | "abs" => Some(Ty::Primitive(prim)),
                    "isNaN" | "isInfinite" | "isFinite" | "bitsEqual" => {
                        Some(Ty::Primitive(Primitive::Bool))
                    }
                    "bits" => Some(Ty::Primitive(Primitive::Uint)),
                    "totalOrder" => Some(Ty::Primitive(Primitive::Int)),
                    "toFixed" => Some(Ty::String),
                    _ => None,
                };
            }
            if is_int {
                return match method_name {
                    "abs" | "saturatingAbs" | "saturatingAdd" | "saturatingSub"
                    | "saturatingMul" | "wrappingAdd" | "wrappingSub" | "wrappingMul"
                    | "rotateLeft" | "rotateRight" => Some(Ty::Primitive(prim)),
                    "countOnes" | "leadingZeros" | "trailingZeros" | "saturatingToInt" => {
                        Some(Ty::Primitive(Primitive::Int))
                    }
                    "checkedAdd" | "checkedSub" | "checkedMul" | "checkedDiv" => {
                        Some(checked_result(Ty::Primitive(prim)))
                    }
                    "toInt" => Some(checked_result(Ty::Primitive(Primitive::Int))),
                    "toHex" | "toBinary" | "toOctal" => Some(Ty::String),
                    _ => None,
                };
            }
            None
        }
        Ty::Array { element, kind } => match method_name {
            // List<T> → int
            "length" | "len" | "size" | "indexOf" => Some(Ty::Primitive(Primitive::Int)),
            // List<T> → bool
            "isEmpty" | "contains" => Some(Ty::Primitive(Primitive::Bool)),
            // List<T> → T (element type)
            "get" | "first" | "last" | "pop" | "remove" | "set" => {
                Some((**element).clone())
            }
            // List<T> → void (mutating ops, no useful return). Phase-1
            // doesn't have a Void Ty, so we use Unknown which the
            // surrounding stmt-level emit treats fine.
            "add" | "push" | "insert" | "clear" | "reverse" | "sort"
            | "forEach" => Some(Ty::Unknown),
            // List<T> → String
            "join" => Some(Ty::String),
            // List<T> → List<U> — preserves the wrapper; element is
            // a fresh unknown because we don't infer the closure's
            // result type.
            "map" => Some(Ty::Array {
                element: Box::new(Ty::Unknown),
                kind: kind.clone(),
            }),
            "filter" => Some(Ty::Array {
                element: (*element).clone(),
                kind: kind.clone(),
            }),
            "clone" => Some(Ty::Array {
                element: (*element).clone(),
                kind: kind.clone(),
            }),
            _ => None,
        },
        _ => None,
    }
}

/// Apply method-level generic inference (spec §T.4) to a return type
/// that has already had the receiver's class-level generics
/// substituted. The method's own generic params come from
/// `method.generic_params`; we only fire on the bare-param-name shape
/// in [`infer_generic_args`].
fn method_infer_return(
    after_class: &Ty,
    method: &MethodSig,
    _declaring_owner: &str,
    args: &[Expr],
    explicit_generic_args: &[TypeRef],
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> Ty {
    if method.generic_params.is_empty() {
        return after_class.clone();
    }
    // Explicit call-site type args (`obj.pick<String>(x)`) bind the
    // method's params directly; otherwise infer from the arg types.
    let inferred = if !explicit_generic_args.is_empty() {
        explicit_generic_arg_map(&method.generic_params, explicit_generic_args, env, symbols)
    } else {
        let param_tys: Vec<&TypeRef> = method.params.iter().map(|p| &p.ty).collect();
        let arg_tys: Vec<Ty> = args.iter().map(|a| infer_expr(a, env, symbols)).collect();
        infer_generic_args(&method.generic_params, &param_tys, &arg_tys)
    };
    substitute_via_inference(after_class, &method.generic_params, &inferred)
}

/// Lower a [`ReturnType`] into a [`Ty`]. `void` → [`Ty::Void`]; the
/// `async T` form unwraps to `T` for now (we don't have a `Future<T>`
/// wrapper type yet).
fn return_type_to_ty(rt: &ReturnType, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    match rt {
        ReturnType::Void => Ty::Void,
        ReturnType::Type(t) | ReturnType::AsyncType(t) => ty_from_ref(t, env, symbols),
    }
}

/// Like [`return_type_to_ty`] but extends the caller's env with the
/// callee function's generic params before lowering. Without this, a
/// bare `T` return type on `T identity<T>(T x)` lowers to
/// [`Ty::Unknown`] in the caller's env — there's no class to lower
/// against, so we have to seed the scratch env explicitly.
fn return_type_to_ty_in_fn_scope(
    rt: &ReturnType,
    fn_generics: &[juxc_ast::TypeParam],
    _caller_env: &TypeEnv,
    symbols: &SymbolTable,
) -> Ty {
    let mut scratch = TypeEnv::new();
    for tp in fn_generics {
        scratch.add_generic_param(&tp.name.text);
    }
    match rt {
        ReturnType::Void => Ty::Void,
        ReturnType::Type(t) | ReturnType::AsyncType(t) => ty_from_ref(t, &scratch, symbols),
    }
}

/// Same as [`return_type_to_ty`] but lowers in the **declaring** type's
/// generic-param scope, via [`lower_member_type`]. Used when the
/// caller's `env` doesn't carry the declaring class's params — e.g.
/// when inferring `box.get()` from outside Box's body.
fn return_type_in_class(rt: &ReturnType, declaring_class: &str, symbols: &SymbolTable) -> Ty {
    match rt {
        ReturnType::Void => Ty::Void,
        ReturnType::Type(t) | ReturnType::AsyncType(t) => {
            lower_member_type(t, declaring_class, symbols)
        }
    }
}

/// Like [`return_type_in_class`] but also brings the **method's** own generic
/// params into scope, so a generic-method return (`<U> U pick()`) lowers to a
/// [`Ty::Param`] the call-site inference can substitute (see
/// [`crate::ty::lower_member_type_in_method`]).
fn return_type_in_method(
    rt: &ReturnType,
    declaring_class: &str,
    method_generics: &[juxc_ast::TypeParam],
    symbols: &SymbolTable,
) -> Ty {
    match rt {
        ReturnType::Void => Ty::Void,
        ReturnType::Type(t) | ReturnType::AsyncType(t) => {
            crate::ty::lower_member_type_in_method(t, declaring_class, method_generics, symbols)
        }
    }
}

/// `new Foo(args)` / `new Box<int>(arg)` → [`Ty::User`] with the
/// class's name and each explicit generic arg resolved via
/// [`ty_from_ref`]. We don't infer generic args from the call's
/// argument list yet — `new Box(42)` produces `Box<>` (empty args
/// list) and Phase D / Rust will pick that up.
fn infer_new_object(n: &NewObjectExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // Resolve the class name to its FQN. Single-segment names go
    // through `env.unqualified` (the same map `ty_from_ref` uses
    // for type-position resolution). Multi-segment names are taken
    // verbatim as a dot-joined FQN. Falls back to the bare name
    // when neither resolves.
    let name = resolve_class_name(&n.class_name, env, symbols);
    // Explicit `<...>` on the `new` site wins: `new Box<int>(42)`
    // skips inference entirely.
    if !n.generic_args.is_empty() {
        let generic_args = n
            .generic_args
            .iter()
            .map(|g| ty_from_ref(g, env, symbols))
            .collect();
        return Ty::User { name, generic_args };
    }
    // Bare-form inference (spec §T.4): when the class/record has
    // generic params but the user didn't write them, infer from the
    // constructor arg types.
    let generic_args = infer_ctor_generic_args(&name, &n.args, env, symbols);
    Ty::User { name, generic_args }
}

/// Resolve a `new X(...)` or similar class-name reference to an FQN.
/// Single-segment names consult `env.unqualified` (same-package
/// siblings + imports). Multi-segment names are joined with `.` and
/// taken as already-qualified. Falls back to the literal name when
/// no entry resolves, matching the pre-FQN behavior for top-level
/// no-package builds.
/// True if `qn` names a known class (returning its FQN). Used by
/// `infer_field` / `infer_call` to recognize `ClassName.member` as
/// a static-member access rather than an instance field/method on
/// some value of type `ClassName`. Single-segment names route
/// through the unit's bare→FQN map; multi-segment names are
/// joined verbatim.
pub(crate) fn path_resolves_to_class(
    qn: &juxc_ast::QualifiedName,
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> Option<String> {
    if qn.segments.is_empty() {
        return None;
    }
    if qn.segments.len() == 1 {
        let bare = &qn.segments[0].text;
        // Locals shadow type names (e.g. a local `Foo` would beat
        // the class `Foo` in expression scope) — Java rules don't
        // really cover this since type names start with uppercase
        // by convention, but be safe.
        if env.lookup(bare).is_some() {
            return None;
        }
        if let Some(fqn) = env.unqualified.get(bare) {
            if symbols.classes.contains_key(fqn) {
                return Some(fqn.clone());
            }
        }
        if symbols.classes.contains_key(bare) {
            return Some(bare.clone());
        }
        return None;
    }
    let joined: String = qn
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    if symbols.classes.contains_key(&joined) {
        return Some(joined);
    }
    None
}

/// Mirror of [`path_resolves_to_class`] for interfaces. Recognizes
/// `IfaceName.member` as an interface-static-member access so the
/// call/field dispatch in `check.rs` can branch on it before
/// inferring a (non-existent) value type for `IfaceName`.
pub(crate) fn path_resolves_to_interface(
    qn: &juxc_ast::QualifiedName,
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> Option<String> {
    if qn.segments.is_empty() {
        return None;
    }
    if qn.segments.len() == 1 {
        let bare = &qn.segments[0].text;
        if env.lookup(bare).is_some() {
            return None;
        }
        if let Some(fqn) = env.unqualified.get(bare) {
            if symbols.interfaces.contains_key(fqn) {
                return Some(fqn.clone());
            }
        }
        if symbols.interfaces.contains_key(bare) {
            return Some(bare.clone());
        }
        return None;
    }
    let joined: String = qn
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    if symbols.interfaces.contains_key(&joined) {
        return Some(joined);
    }
    None
}

pub(crate) fn resolve_class_name(
    qn: &juxc_ast::QualifiedName,
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> String {
    if qn.segments.is_empty() {
        return String::new();
    }
    let fqn = if qn.segments.len() == 1 {
        let bare = &qn.segments[0].text;
        if let Some(fqn) = env.unqualified.get(bare) {
            if symbols.is_type_name(fqn) {
                fqn.clone()
            } else {
                bare.clone()
            }
        } else if symbols.is_type_name(bare) {
            // Direct hit: the bare name is itself a registered FQN
            // (no-package class, or same-unit declaration).
            bare.clone()
        } else if let Some(fqn) =
            symbols.find_fqn_by_bare_in(bare, &env.current_package.join("."))
        {
            // Implicit auto-import: bare name matches the last segment of a
            // known FQN. Mirrors Java's `java.lang.*` rule applied across the
            // whole stdlib tree, preferring a same-package type on a collision.
            fqn
        } else {
            bare.clone()
        }
    } else {
        let joined = qn.segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".");
        // §M.9 nested-type access: `HttpServer.Config` names a
        // NESTED type, lifted+registered as `HttpServer__Config`.
        // Resolution order: the joined dotted name verbatim (a true
        // package FQN), then the FIRST segment resolved as a class
        // (unqualified map / direct / suffix scan) with the rest
        // appended in the lifted `__` form.
        if symbols.is_type_name(&joined) {
            joined
        } else {
            let first = &qn.segments[0].text;
            let rest = qn.segments[1..]
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join("__");
            let owner_fqn = env
                .unqualified
                .get(first)
                .cloned()
                .filter(|f| symbols.is_type_name(f))
                .or_else(|| {
                    if symbols.is_type_name(first) {
                        Some(first.clone())
                    } else {
                        symbols.find_fqn_by_bare_in(first, &env.current_package.join("."))
                    }
                });
            let via_owner = owner_fqn.map(|o| format!("{o}__{rest}"));
            match via_owner.filter(|c| symbols.is_type_name(c)) {
                Some(hit) => hit,
                None => joined,
            }
        }
    };
    // Follow type aliases — `new Alias(args)` should land on the
    // underlying class. Walks at most a small chain (capped at 16)
    // to avoid runaway expansion on malformed aliases. Bare-name
    // resolution inside the alias's target uses the **declaring
    // unit's** context — important when the alias lives in a
    // different package than the call site (the call site never
    // imported the target name, only the alias).
    let mut cursor = fqn;
    for _ in 0..16 {
        let Some(alias) = symbols.aliases.get(&cursor) else {
            return cursor;
        };
        let alias_ctx = alias
            .unit_index
            .and_then(|idx| symbols.units.get(idx));
        let target_fqn = if alias.target.name.segments.len() == 1 {
            let bare = &alias.target.name.segments[0].text;
            // Prefer the alias's declaring unit's resolver, fall
            // back to the caller's (legacy single-file path).
            alias_ctx
                .and_then(|ctx| ctx.unqualified.get(bare))
                .or_else(|| env.unqualified.get(bare))
                .cloned()
                .unwrap_or_else(|| bare.clone())
        } else {
            alias
                .target
                .name
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(".")
        };
        if !symbols.is_type_name(&target_fqn) {
            return cursor;
        }
        cursor = target_fqn;
    }
    cursor
}

/// Infer a class or record's generic-arg list from the constructor
/// call's actual arg types. Returns an empty vec when the named type
/// isn't generic (or isn't known), so callers can still build a
/// `Ty::User` with the raw shape Phase 1 already accepts.
pub(crate) fn infer_ctor_generic_args(
    name: &str,
    args: &[Expr],
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> Vec<Ty> {
    // Classes: use the first constructor's params as the "shape" to
    // infer against. Phase 1 doesn't support constructor overloads,
    // so this is unambiguous when it resolves.
    if let Some(class) = symbols.classes.get(name) {
        if class.generic_params.is_empty() {
            return Vec::new();
        }
        let Some(ctor) = class.constructors.first() else {
            // No constructor at all — synthesized default takes zero
            // args, so there's nothing to unify on. Leave generics
            // unbound; downstream the wildcard rule in `compatible`
            // keeps things quiet.
            return Vec::new();
        };
        let param_tys: Vec<&TypeRef> = ctor.params.iter().map(|p| &p.ty).collect();
        let arg_tys: Vec<Ty> = args.iter().map(|a| infer_expr(a, env, symbols)).collect();
        let inferred = infer_generic_args(&class.generic_params, &param_tys, &arg_tys);
        return class
            .generic_params
            .iter()
            .map(|p| inferred.get(&p.name.text).cloned().unwrap_or(Ty::Unknown))
            .collect();
    }
    // Records: header components ARE the canonical constructor.
    if let Some(record) = symbols.records.get(name) {
        if record.generic_params.is_empty() {
            return Vec::new();
        }
        let param_tys: Vec<&TypeRef> = record.components.iter().map(|c| &c.ty).collect();
        let arg_tys: Vec<Ty> = args.iter().map(|a| infer_expr(a, env, symbols)).collect();
        let inferred = infer_generic_args(&record.generic_params, &param_tys, &arg_tys);
        return record
            .generic_params
            .iter()
            .map(|p| inferred.get(&p.name.text).cloned().unwrap_or(Ty::Unknown))
            .collect();
    }
    Vec::new()
}

/// `new T[size]` → fixed-size array of `T`.
fn infer_new_array(n: &NewArrayExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // Build a nested `Ty::Array` one level per dimension (outer `size`
    // plus each `inner_sizes` entry), so `new int[3][4]` infers as
    // `int[][]` (`Array { element: Array { element: Int } }`). The
    // concrete fixed/dynamic kind is reconciled with the LHS slot during
    // assignment compatibility; here every dimension is reported `Fixed`
    // (the construction's natural shape) and unifies with a dynamic slot
    // via the fixed→dynamic rule (§5.6).
    let mut ty = ty_from_ref(&n.element_type, env, symbols);
    // One wrap per dimension. `+ 1` accounts for the outer `size`.
    for _ in 0..(n.inner_sizes.len() + 1) {
        ty = Ty::Array {
            element: Box::new(ty),
            kind: ArrayKind::Fixed,
        };
    }
    ty
}

/// `new T[]{…}` or `T[]{…}`. Picks Fixed vs Dynamic per the AST node's
/// `fixed` flag — the parser stamps that based on the LHS context.
fn infer_new_array_lit(n: &NewArrayLitExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    let kind = if n.fixed {
        ArrayKind::Fixed
    } else {
        ArrayKind::Dynamic
    };
    Ty::Array {
        element: Box::new(ty_from_ref(&n.element_type, env, symbols)),
        kind,
    }
}

/// `value as T` always evaluates to `T` regardless of `value`'s type
/// — cast-validity is Phase D's job.
fn infer_cast(c: &CastExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    ty_from_ref(&c.ty, env, symbols)
}

/// Unary operators:
/// - `!x` → bool (Jux's `!` is logical-NOT only on booleans).
/// - `-x`, `~x` → same type as operand.
fn infer_unary(u: &UnaryExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // Operator-dispatch first — if the operand is a user type whose
    // matching operator is defined (and not deleted), the operator's
    // declared return type wins over the built-in bucket rule.
    let operand_ty = infer_expr(&u.operand, env, symbols);
    if let Some(kind) = unary_op_to_kind(u.op) {
        if let Some(ret) = lookup_user_operator_return_type(&operand_ty, kind, env, symbols) {
            return ret;
        }
    }
    match u.op {
        UnaryOp::Not => Ty::Primitive(Primitive::Bool),
        // Raw pointers erase to their pointee's `Ty` (the `ptr_depth` on
        // `TypeRef` isn't carried into `Ty`), so `*p` (deref) and `&x`
        // (address-of) both flow the operand's nominal type through. The
        // real `*mut T` / `T` distinction is enforced by rustc on the
        // emitted code.
        UnaryOp::Neg | UnaryOp::BitNot | UnaryOp::Deref | UnaryOp::AddrOf => operand_ty,
    }
}

/// Map a [`UnaryOp`] to its overloadable [`OperatorKind`], if any.
/// `!x` isn't overridable per spec §O.2.5.
fn unary_op_to_kind(op: UnaryOp) -> Option<OperatorKind> {
    Some(match op {
        UnaryOp::Neg => OperatorKind::Neg,
        UnaryOp::BitNot => OperatorKind::BitNot,
        UnaryOp::Not | UnaryOp::Deref | UnaryOp::AddrOf => return None,
    })
}

/// Binary operators bucket into three result-type groups:
/// - Comparison (`<`, `<=`, `>`, `>=`, `==`, `!=`) → `bool`.
/// - Logical (`&&`, `||`) → `bool`.
/// - Arithmetic / bitwise / shift → the **left** operand's type.
///
/// The arithmetic rule is intentionally simple; a proper common-type
/// rule (promoting `int + long` to `long`, etc.) lands in Phase D.
fn infer_binary(b: &BinaryExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // Operator-dispatch first — if the LHS is a user type whose
    // matching operator is defined, the operator's declared return
    // type takes precedence over the built-in bucket rule. Comparison
    // ops (`==`, `<`, etc.) almost always return bool either way, so
    // this mostly matters for arithmetic/bitwise/shift where a class
    // could declare a return type different from its LHS type.
    let left_ty = infer_expr(&b.left, env, symbols);
    if let Some(kind) = binary_op_to_kind(b.op) {
        if let Some(ret) = lookup_user_operator_return_type(&left_ty, kind, env, symbols) {
            return ret;
        }
    }
    // String concatenation is symmetric (`"v" + n` AND `n + "v"` are
    // both String, like Java) — the left-type rule below would call
    // `int + String` an int.
    if matches!(b.op, BinaryOp::Add) {
        if matches!(left_ty, Ty::String)
            || matches!(infer_expr(&b.right, env, symbols), Ty::String)
        {
            return Ty::String;
        }
    }
    match b.op {
        // `<=>` (§A.4 level 11) always yields int: -1 / 0 / +1.
        // (A user `operator<=>` was already consulted above via the
        // operator-dispatch return lookup.)
        BinaryOp::Cmp => Ty::Primitive(Primitive::Int),
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::RefEq
        | BinaryOp::RefNeq
        | BinaryOp::Lt
        | BinaryOp::Le
        | BinaryOp::Gt
        | BinaryOp::Ge
        | BinaryOp::In
        | BinaryOp::And
        | BinaryOp::Or => Ty::Primitive(Primitive::Bool),
        // Arithmetic / bitwise — Java-style numeric promotion of the two
        // operand types (a float operand wins; otherwise the wider integer,
        // with a same-width signed/unsigned tie going unsigned). This must
        // match the backend's operand coercion (`numeric_promote_target`) so a
        // `long / double` is `double` (not `long` → silent integer division),
        // and `int + long` is `long` (not `int` → a leaked `isize + i64`).
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Rem
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::BitAnd => {
            let right_ty = infer_expr(&b.right, env, symbols);
            numeric_promote(&left_ty, &right_ty)
                .map(Ty::Primitive)
                .unwrap_or(left_ty)
        }
        // Shift and the wrapping family (§S.2.1) preserve the LEFT operand's
        // type by construction (the right operand is only a shift/step count).
        BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::WrapAdd
        | BinaryOp::WrapSub
        | BinaryOp::WrapMul
        | BinaryOp::WrapShl
        | BinaryOp::WrapShr => left_ty,
    }
}

/// Java-style numeric promotion of two operand types: the common primitive an
/// arithmetic/bitwise op over `l` and `r` produces, or `None` when either side
/// is not a numeric primitive (so the caller keeps its existing left-type
/// fallback). Mirrors `juxc_backend_rust`'s `numeric_promote_target` exactly so
/// the inferred result type matches the operand casts the backend emits:
/// a float operand wins (`double`, or `float` only when both floats are 32-bit);
/// otherwise the wider integer, with a same-width signed/unsigned tie resolving
/// to the unsigned type.
fn numeric_promote(l: &Ty, r: &Ty) -> Option<Primitive> {
    let (Ty::Primitive(lp), Ty::Primitive(rp)) = (l, r) else {
        return None;
    };
    let (lp, rp) = (*lp, *rp);
    if matches!(lp, Primitive::Bool | Primitive::Char)
        || matches!(rp, Primitive::Bool | Primitive::Char)
    {
        return None;
    }
    if lp == rp {
        return Some(lp);
    }
    let is_float =
        |p: Primitive| matches!(p, Primitive::Float | Primitive::Double | Primitive::F32 | Primitive::F64);
    if is_float(lp) || is_float(rp) {
        let is_f64 = |p: Primitive| matches!(p, Primitive::Double | Primitive::F64);
        return Some(if is_f64(lp) || is_f64(rp) {
            Primitive::Double
        } else {
            Primitive::Float
        });
    }
    let rank = |p: Primitive| -> u8 {
        match p {
            Primitive::Byte | Primitive::I8 | Primitive::Ubyte | Primitive::U8 => 1,
            Primitive::Short | Primitive::I16 | Primitive::Ushort | Primitive::U16 => 2,
            Primitive::I32 | Primitive::U32 => 3,
            Primitive::Int | Primitive::Uint => 4,
            Primitive::Long | Primitive::I64 | Primitive::Ulong | Primitive::U64 => 5,
            _ => 0,
        }
    };
    let unsigned = |p: Primitive| {
        matches!(
            p,
            Primitive::Uint
                | Primitive::Ubyte
                | Primitive::U8
                | Primitive::Ushort
                | Primitive::U16
                | Primitive::U32
                | Primitive::Ulong
                | Primitive::U64,
        )
    };
    let (rl, rr) = (rank(lp), rank(rp));
    Some(if rl > rr {
        lp
    } else if rr > rl {
        rp
    } else if unsigned(lp) {
        lp
    } else {
        rp
    })
}

/// Map a [`BinaryOp`] to the [`OperatorKind`] that would override it,
/// if any. Returns `None` for ops that aren't user-overridable
/// (`&&`/`||`) or that auto-derive from another op (`!=` from `==`,
/// the four orderings from `<=>`). Phase-1 simplification: only the
/// primary form is dispatched — a user with only `<=>` declared
/// won't see operator-dispatch on `<`/`<=`/`>`/`>=` at the tycheck
/// level (the Rust trait layer covers that via PartialOrd's default
/// methods).
fn binary_op_to_kind(op: BinaryOp) -> Option<OperatorKind> {
    Some(match op {
        BinaryOp::Eq => OperatorKind::Eq,
        BinaryOp::Cmp => OperatorKind::Cmp,
        BinaryOp::Add => OperatorKind::Plus,
        BinaryOp::Sub => OperatorKind::Minus,
        BinaryOp::Mul => OperatorKind::Mul,
        BinaryOp::Div => OperatorKind::Div,
        BinaryOp::Rem => OperatorKind::Rem,
        BinaryOp::BitAnd => OperatorKind::BitAnd,
        BinaryOp::BitOr => OperatorKind::BitOr,
        BinaryOp::BitXor => OperatorKind::BitXor,
        BinaryOp::Shl => OperatorKind::Shl,
        BinaryOp::Shr => OperatorKind::Shr,
        _ => return None,
    })
}

/// If `receiver_ty` is a user class/record/enum AND has a non-deleted
/// declaration of `kind`, return its lowered declared return type.
/// Consults all three host kinds.
///
/// Substitution-aware: a class's `operator+(...) -> T` lowered against
/// `receiver_ty = Ty::User { generic_args: [Int] }` returns `Int`
/// rather than `Ty::Param("T")`. Mirrors the substitution Phase E
/// already does for method calls.
fn lookup_user_operator_return_type(
    receiver_ty: &Ty,
    kind: OperatorKind,
    _env: &TypeEnv,
    symbols: &SymbolTable,
) -> Option<Ty> {
    let Ty::User { name, generic_args } = receiver_ty else { return None };

    // Class lookup first; then records; then enums.
    if let Some(class) = symbols.classes.get(name) {
        if let Some(op) = class.operators.get(&kind) {
            if !op.is_deleted {
                let raw = return_type_in_class(&op.return_type, name, symbols);
                return Some(substitute(&raw, &class.generic_params, generic_args));
            }
        }
    }
    if let Some(record) = symbols.records.get(name) {
        if let Some(op) = record.operators.get(&kind) {
            if !op.is_deleted {
                let raw = return_type_in_class(&op.return_type, name, symbols);
                return Some(substitute(&raw, &record.generic_params, generic_args));
            }
        }
    }
    if let Some(enum_sig) = symbols.enums.get(name) {
        if let Some(op) = enum_sig.operators.get(&kind) {
            if !op.is_deleted {
                // Enums don't carry generic params in the AST yet, so
                // no substitution applies.
                return Some(return_type_in_class(&op.return_type, name, symbols));
            }
        }
    }
    None
}

// ============================================================================
// Block / statement walker
// ============================================================================

/// Walk `block`'s statements and **declare any locals they introduce
/// into `env`**.
///
/// This is the side-effecting pass that lets Phase C infer expressions
/// containing variable references — without it, every name lookup
/// would miss.
///
/// The walker doesn't return a type. It calls [`infer_expr`] on
/// embedded expressions purely for the **walk** (so future side-
/// effecting variants of inference can hook in here without changing
/// the public API). The returned `Ty` is discarded.
pub fn infer_block(block: &Block, env: &mut TypeEnv, symbols: &SymbolTable) {
    for stmt in &block.statements {
        infer_stmt(stmt, env, symbols);
    }
}

/// Per-statement walker. Pushes/pops scopes around nested blocks so
/// loop-vars and pattern-bindings don't leak.
fn infer_stmt(stmt: &Stmt, env: &mut TypeEnv, symbols: &SymbolTable) {
    match stmt {
        Stmt::VarDecl(v) => {
            // Prefer the declared type when present; otherwise infer
            // from the initializer.
            let ty = if let Some(declared) = &v.ty {
                ty_from_ref(declared, env, symbols)
            } else if let Some(init) = &v.init {
                infer_expr(init, env, symbols)
            } else {
                Ty::Unknown
            };
            env.declare(&v.name.text, ty);
        }
        Stmt::ForEach(f) => {
            // The loop variable's type comes from the explicit
            // annotation if present, else from the iter's element
            // type. Then push a new scope (so the binding is
            // loop-local) and walk the body.
            let iter_ty = infer_expr(&f.iter, env, symbols);
            let var_ty = if let Some(declared) = &f.var_type {
                ty_from_ref(declared, env, symbols)
            } else {
                match iter_ty {
                    Ty::Array { element, .. } => *element,
                    // Range / non-array iter: stay Unknown until we
                    // have a real iterator protocol.
                    _ => Ty::Unknown,
                }
            };
            env.push_scope();
            env.declare(&f.var_name.text, var_ty);
            infer_block(&f.body, env, symbols);
            env.pop_scope();
        }
        Stmt::ForC(f) => {
            env.push_scope();
            if let Some(init) = f.init.as_deref() {
                infer_stmt(init, env, symbols);
            }
            if let Some(cond) = &f.cond {
                let _ = infer_expr(cond, env, symbols);
            }
            if let Some(upd) = f.update.as_deref() {
                infer_stmt(upd, env, symbols);
            }
            env.push_scope();
            infer_block(&f.body, env, symbols);
            env.pop_scope();
            env.pop_scope();
        }
        Stmt::If(if_stmt) => {
            // Evaluate the condition for its side effects (declares
            // nothing today, but keeps the walker total).
            let _ = infer_expr(&if_stmt.condition, env, symbols);
            env.push_scope();
            infer_block(&if_stmt.then_block, env, symbols);
            env.pop_scope();
            if let Some(else_branch) = &if_stmt.else_branch {
                infer_else_branch(else_branch, env, symbols);
            }
        }
        Stmt::While(w) => {
            let _ = infer_expr(&w.condition, env, symbols);
            env.push_scope();
            infer_block(&w.body, env, symbols);
            env.pop_scope();
        }
        Stmt::DoWhile(d) => {
            env.push_scope();
            infer_block(&d.body, env, symbols);
            env.pop_scope();
            let _ = infer_expr(&d.condition, env, symbols);
        }
        Stmt::Expr(e) => {
            let _ = infer_expr(e, env, symbols);
        }
        Stmt::Assign(a) => {
            let _ = infer_expr(&a.target, env, symbols);
            let _ = infer_expr(&a.value, env, symbols);
        }
        Stmt::Return(opt, _) => {
            if let Some(e) = opt {
                let _ = infer_expr(e, env, symbols);
            }
        }
        Stmt::SuperCall(args, _) => {
            for arg in args {
                let _ = infer_expr(arg, env, symbols);
            }
        }
        Stmt::Throw(e, _) => {
            let _ = infer_expr(e, env, symbols);
        }
        Stmt::Try(t) => {
            env.push_scope();
            infer_block(&t.body, env, symbols);
            env.pop_scope();
            for c in &t.catches {
                env.push_scope();
                let ty = ty_from_ref(&c.ty, env, symbols);
                env.declare(&c.name.text, ty);
                infer_block(&c.body, env, symbols);
                env.pop_scope();
            }
            if let Some(fin) = &t.finally {
                env.push_scope();
                infer_block(fin, env, symbols);
                env.pop_scope();
            }
        }
        Stmt::Unsafe(b) => {
            env.push_scope();
            infer_block(b, env, symbols);
            env.pop_scope();
        }
        Stmt::Break(..) | Stmt::Continue(..) => {}
        Stmt::Labeled { stmt, .. } => infer_stmt(stmt, env, symbols),
    }
}

/// Recursive helper for `else if` chains. The terminal `else { … }`
/// pushes its own scope; an `else if` recurses through [`infer_stmt`]
/// machinery by handling the inline `IfStmt` directly.
fn infer_else_branch(branch: &ElseBranch, env: &mut TypeEnv, symbols: &SymbolTable) {
    match branch {
        ElseBranch::If(if_stmt) => {
            let _ = infer_expr(&if_stmt.condition, env, symbols);
            env.push_scope();
            infer_block(&if_stmt.then_block, env, symbols);
            env.pop_scope();
            if let Some(nested) = &if_stmt.else_branch {
                infer_else_branch(nested, env, symbols);
            }
        }
        ElseBranch::Block(block) => {
            env.push_scope();
            infer_block(block, env, symbols);
            env.pop_scope();
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_table::build;
    use juxc_ast::{CompilationUnit, FnDecl, TopLevelDecl};
    use juxc_lex::lex;
    use juxc_parse::parse;
    use juxc_source::SourceFile;

    /// Drive lex → parse → symbol-table build for the given source.
    /// Returns the symbol table plus the parsed unit so tests can
    /// reach into the AST for expressions.
    fn build_table(src: &str) -> (SymbolTable, CompilationUnit) {
        let sf = SourceFile::new("test.jux", src);
        let lex_result = lex(&sf);
        assert!(lex_result.diagnostics.is_empty(), "lex: {:?}", lex_result.diagnostics);
        let parse_result = parse(&lex_result.tokens);
        assert!(
            parse_result.diagnostics.is_empty(),
            "parse: {:?}",
            parse_result.diagnostics,
        );
        let mut diags = Vec::new();
        let table = build(&parse_result.ast, &mut diags);
        assert!(diags.is_empty(), "symtab: {:?}", diags);
        (table, parse_result.ast)
    }

    /// Helper: pull the first top-level function's body out of a unit.
    fn first_fn_body(unit: &CompilationUnit) -> &Block {
        for item in &unit.items {
            if let TopLevelDecl::Function(FnDecl { body: Some(b), .. }) = item {
                return b;
            }
        }
        panic!("no top-level function with body");
    }

    /// Helper: pull a named top-level function's body out of a unit.
    fn fn_body_by_name<'a>(unit: &'a CompilationUnit, name: &str) -> &'a Block {
        for item in &unit.items {
            if let TopLevelDecl::Function(fn_decl) = item {
                if fn_decl.name.text == name {
                    return fn_decl.body.as_ref().expect("fn has body");
                }
            }
        }
        panic!("no top-level function named `{name}`");
    }

    /// Helper: return the initializer expression of the first VarDecl
    /// found in `block`.
    fn first_var_init<'a>(block: &'a Block) -> &'a Expr {
        for stmt in &block.statements {
            if let Stmt::VarDecl(v) = stmt {
                if let Some(init) = &v.init {
                    return init;
                }
            }
        }
        panic!("no var-decl with initializer in block");
    }

    /// `42` → `Primitive::Int`.
    #[test]
    fn int_literal_is_int() {
        let (table, unit) = build_table("public void main() { var x = 42; }");
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::Primitive(Primitive::Int));
    }

    /// `1.5` → `Primitive::Double`.
    #[test]
    fn float_literal_is_double() {
        let (table, unit) = build_table("public void main() { var x = 1.5; }");
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Double),
        );
    }

    /// `"hi"` → `Ty::String`.
    #[test]
    fn string_literal_is_string() {
        let (table, unit) = build_table(r#"public void main() { var x = "hi"; }"#);
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::String);
    }

    /// `true` → `Primitive::Bool`.
    #[test]
    fn bool_literal_is_bool() {
        let (table, unit) = build_table("public void main() { var x = true; }");
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Bool),
        );
    }

    /// A var declared via the walker is resolvable by name.
    #[test]
    fn var_lookup_after_declaration() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var x = 42;
                var y = x;
            }
            "#,
        );
        let block = first_fn_body(&unit);
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("x"), Some(&Ty::Primitive(Primitive::Int)));
        assert_eq!(env.lookup("y"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// A name that was never declared → Unknown.
    #[test]
    fn unknown_var_is_unknown() {
        use juxc_ast::QualifiedName;
        let table = SymbolTable::default();
        let env = TypeEnv::new();
        // Build a synthetic Path expression by hand.
        let qn = QualifiedName {
            segments: vec![juxc_ast::Ident {
                text: "nope".to_string(),
                span: juxc_source::Span::DUMMY,
            }],
            span: juxc_source::Span::DUMMY,
        };
        assert!(infer_expr(&Expr::Path(qn), &env, &table).is_unknown());
    }

    /// `new MyClass()` → `Ty::User { name: "MyClass" }`.
    #[test]
    fn new_object_is_user_type() {
        let (table, unit) = build_table(
            r#"
            public class MyClass {}
            public void main() {
                var x = new MyClass();
            }
            "#,
        );
        let block = first_fn_body(&unit);
        let init = first_var_init(block);
        let env = TypeEnv::new();
        let ty = infer_expr(init, &env, &table);
        match ty {
            Ty::User { name, generic_args } => {
                assert_eq!(name, "MyClass");
                assert!(generic_args.is_empty());
            }
            other => panic!("expected User type, got {other:?}"),
        }
    }

    /// `obj.x` where `x` is an `int` field → `Primitive::Int`.
    #[test]
    fn field_access_returns_field_type() {
        let (table, unit) = build_table(
            r#"
            public class Point {
                public int x;
                public int y;
            }
            public void main() {
                var p = new Point();
                var v = p.x;
            }
            "#,
        );
        let block = first_fn_body(&unit);
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `new int[]{1, 2, 3}` → `Array { element: Int, kind: Dynamic }`.
    #[test]
    fn array_literal_is_dynamic_array() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var xs = new int[]{1, 2, 3};
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        match infer_expr(init, &env, &table) {
            Ty::Array { element, kind } => {
                assert_eq!(*element, Ty::Primitive(Primitive::Int));
                assert_eq!(kind, ArrayKind::Dynamic);
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    /// `arr[0]` → element type.
    #[test]
    fn index_into_array_returns_element() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var xs = new int[]{1, 2, 3};
                var e = xs[0];
            }
            "#,
        );
        let block = first_fn_body(&unit);
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("e"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `int[][] m` lowers to a nested `Array { element: Array { element: Int } }`,
    /// and indexing peels exactly one dimension per `[…]`:
    /// `m[i]` is `int[]`, `m[i][j]` is `int`.
    #[test]
    fn multi_dim_array_index_peels_one_dimension() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                int[][] m;
                var row = m[0];
                var cell = m[0][1];
            }
            "#,
        );
        let block = first_fn_body(&unit);
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        // `m` itself: a 2-D nested array of int.
        let inner_int = Ty::Array {
            element: Box::new(Ty::Primitive(Primitive::Int)),
            kind: ArrayKind::Dynamic,
        };
        assert_eq!(
            env.lookup("m"),
            Some(&Ty::Array {
                element: Box::new(inner_int.clone()),
                kind: ArrayKind::Dynamic,
            }),
            "int[][] is a nested Array of Array of Int",
        );
        // `m[0]` peels one dim → `int[]`.
        assert_eq!(env.lookup("row"), Some(&inner_int), "m[0] is int[]");
        // `m[0][1]` peels both → scalar `int`.
        assert_eq!(
            env.lookup("cell"),
            Some(&Ty::Primitive(Primitive::Int)),
            "m[0][1] is int",
        );
    }

    /// `new int[3][4]` infers a 2-D nested array, one `Array` wrap per
    /// dimension (outer size + one inner size).
    #[test]
    fn new_multi_dim_array_infers_nested_array() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var g = new int[3][4];
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        match infer_expr(init, &env, &table) {
            Ty::Array { element, .. } => match *element {
                Ty::Array { element: inner, .. } => {
                    assert_eq!(*inner, Ty::Primitive(Primitive::Int));
                }
                other => panic!("expected inner Array, got {other:?}"),
            },
            other => panic!("expected outer Array, got {other:?}"),
        }
    }

    /// `sizeof(int)` → `Primitive::Int`.
    #[test]
    fn sizeof_is_int() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var s = sizeof(int);
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::Primitive(Primitive::Int));
    }

    /// `$"hi"` → `Ty::String`.
    #[test]
    fn interp_string_is_string() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var s = $"hi";
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::String);
    }

    /// `this` inside a class context → `Ty::User { name: "Foo" }`.
    #[test]
    fn this_in_class_context() {
        let table = SymbolTable::default();
        let mut env = TypeEnv::new();
        env.set_class("Foo");
        let ty = infer_expr(&Expr::This(juxc_source::Span::DUMMY), &env, &table);
        match ty {
            Ty::User { name, .. } => assert_eq!(name, "Foo"),
            other => panic!("expected User, got {other:?}"),
        }
    }

    /// `1 < 2` → `Primitive::Bool`.
    #[test]
    fn binary_lt_is_bool() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var b = 1 < 2;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Bool),
        );
    }

    /// `1 + 2` → `Primitive::Int` (left's type).
    #[test]
    fn binary_add_returns_left_type() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var n = 1 + 2;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Int),
        );
    }

    /// `!true` → `Primitive::Bool`.
    #[test]
    fn unary_not_is_bool() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var b = !true;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Bool),
        );
    }

    /// A top-level function call returns the declared return type.
    #[test]
    fn call_returns_function_return_type() {
        let (table, unit) = build_table(
            r#"
            public int helper() { return 1; }
            public void main() {
                var x = helper();
            }
            "#,
        );
        // Two top-level fns: pick `main`'s body directly so the var
        // we want to test lives inside it.
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("x"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Cast `value as long` → `Primitive::Long`.
    #[test]
    fn cast_returns_target_type() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var n = 1 as long;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Long),
        );
    }

    /// Phase E.1 — a method declared on a superclass is reachable from
    /// the child receiver. `(new Dog()).getAge()` returns Animal's
    /// declared return type, not Unknown.
    #[test]
    fn inherited_method_resolves_in_infer() {
        let (table, unit) = build_table(
            r#"
            public class Animal {
                public int getAge() { return 0; }
            }
            public class Dog extends Animal {}
            public void main() {
                var d = new Dog();
                var n = d.getAge();
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("n"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Phase E.1 — a field declared on a superclass is reachable from
    /// the child receiver.
    #[test]
    fn inherited_field_resolves_in_infer() {
        let (table, unit) = build_table(
            r#"
            public class Animal { public int age; }
            public class Dog extends Animal {}
            public void main() {
                var d = new Dog();
                var n = d.age;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("n"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Phase E.2 — field access on an instantiated generic class
    /// substitutes the type parameter for the receiver's argument.
    #[test]
    fn generic_field_substitutes_through_receiver() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> { public T value; }
            public void main() {
                var b = new Box<int>();
                var v = b.value;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Phase E.2 — method-return substitution through the receiver's
    /// generic args. `Box<int>::get() -> T` returns int.
    #[test]
    fn generic_method_return_substitutes_through_receiver() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> {
                public T value;
                public T get() { return this.value; }
            }
            public void main() {
                var b = new Box<int>();
                var v = b.get();
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Operator-dispatch: `a + b` on a class whose `operator+` returns
    /// the class type produces that user type, not a primitive
    /// fallback. Substitution-aware: the user-declared `T` return
    /// gets substituted through the receiver's generic args.
    #[test]
    fn binary_plus_uses_operator_return_type() {
        let (table, unit) = build_table(
            r#"
            public class Money {
                public int cents;
                public Money(int cents) { this.cents = cents; }
                public Money operator+(Money other) { return new Money(0); }
            }
            public void main() {
                var a = new Money(10);
                var b = new Money(20);
                var c = a + b;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("c") {
            Some(Ty::User { name, .. }) => assert_eq!(name, "Money"),
            other => panic!("expected User(\"Money\"), got {other:?}"),
        }
    }

    /// Operator-dispatch with a custom return type: `operator+(Foo)
    /// -> Bar` returns `Bar`, not `Foo` (the LHS type the built-in
    /// fallback would pick).
    #[test]
    fn binary_plus_returns_user_declared_type() {
        let (table, unit) = build_table(
            r#"
            public class Bar {
                public Bar() {}
            }
            public class Foo {
                public Foo() {}
                public Bar operator+(Foo other) { return new Bar(); }
            }
            public void main() {
                var a = new Foo();
                var b = new Foo();
                var c = a + b;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("c") {
            Some(Ty::User { name, .. }) => assert_eq!(name, "Bar"),
            other => panic!("expected User(\"Bar\"), got {other:?}"),
        }
    }

    /// Deleted operator falls through to the built-in path — for a
    /// deleted `operator+`, `infer_binary` returns the LHS type the
    /// built-in rule would give. (At the use-site this gets flagged
    /// by E0935 in check_expr — see `check::tests`.)
    #[test]
    fn deleted_operator_falls_through_to_built_in() {
        let (table, unit) = build_table(
            r#"
            public class M {
                public int x;
                public M(int x) { this.x = x; }
                public M operator+(M other) = delete;
            }
            public void main() {
                var a = new M(1);
                var b = new M(2);
                var c = a + b;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        // Built-in fallback for `+` is the LHS type.
        match env.lookup("c") {
            Some(Ty::User { name, .. }) => assert_eq!(name, "M"),
            other => panic!("expected User(\"M\"), got {other:?}"),
        }
    }

    /// Primitive `+` still goes through the built-in path — no
    /// operator dispatch when the LHS isn't a user type.
    #[test]
    fn primitive_plus_unchanged() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var x = 1 + 2;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::Primitive(Primitive::Int));
    }

    /// Phase E.2 — a raw-type receiver (`new Box(...)` with no
    /// turbofish) leaves substitution off, so the field type stays as
    /// `Ty::Param("T")`. The wildcard rule in `compatible` then keeps
    /// downstream checks quiet.
    #[test]
    fn raw_generic_receiver_leaves_param_in_place() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> { public T value; }
            public void main() {
                var b = new Box();
                var v = b.value;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("v") {
            Some(Ty::Param(name)) => assert_eq!(name, "T"),
            other => panic!("expected Ty::Param(\"T\"), got {other:?}"),
        }
    }

    // ============================================================================
    // Generic inference at call sites — spec §T.4
    // ============================================================================

    /// `identity(42)` should infer `T = int` and report the call's
    /// type as `int` rather than `Ty::Param("T")`.
    #[test]
    fn generic_fn_inferred_from_single_arg() {
        let (table, unit) = build_table(
            r#"
            public T identity<T>(T x) { return x; }
            public void main() { var v = identity(42); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `pair_eq(1, "x")` — int vs String conflict on the same T.
    /// Phase 1 inference gives up cleanly: the return type stays
    /// `Ty::Param("T")`, which the `compatible` wildcard rule then
    /// keeps quiet at downstream use-sites. Phase D will refine
    /// this into an E04xx diagnostic at the call site once we have
    /// a join-lattice over Ty.
    #[test]
    fn generic_fn_conflicting_args_falls_back() {
        let (table, unit) = build_table(
            r#"
            public T pair_eq<T>(T a, T b) { return a; }
            public void main() { var v = pair_eq(1, "x"); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("v") {
            Some(Ty::Param(name)) => assert_eq!(name, "T"),
            other => panic!("expected Ty::Param(\"T\") on conflict, got {other:?}"),
        }
    }

    /// Non-generic functions are unaffected: the inference fast-path
    /// returns the raw lowered type.
    #[test]
    fn non_generic_fn_unaffected_by_inference() {
        let (table, unit) = build_table(
            r#"
            public int square(int x) { return x * x; }
            public void main() { var v = square(7); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    // Method-level generic inference (e.g. `class P { U pick<U>(U u) }`)
    // is wired in `append_method_generic_inference` in check.rs and in
    // `method_infer_return` here, but the class-member parser lookahead
    // doesn't yet recognize `name<T>(` as a method shape, so we can't
    // construct a syntactic test for it without first extending the
    // parser. That extension is queued behind the `class A<T>` work.

    /// `new Box(42)` should infer `Box<int>` from the constructor arg.
    #[test]
    fn generic_class_inferred_from_ctor_arg() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> {
                public T value;
                public Box(T v) { this.value = v; }
            }
            public void main() { var b = new Box(42); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("b") {
            Some(Ty::User { name, generic_args }) => {
                assert_eq!(name, "Box");
                assert_eq!(generic_args, &vec![Ty::Primitive(Primitive::Int)]);
            }
            other => panic!("expected Ty::User Box<int>, got {other:?}"),
        }
    }

    /// Explicit turbofish overrides inference: `new Box<String>(42)`
    /// keeps the `String` arg (any rustc mismatch is caught downstream).
    #[test]
    fn explicit_turbofish_overrides_inference() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> {
                public T value;
                public Box(T v) { this.value = v; }
            }
            public void main() { var b = new Box<String>(42); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("b") {
            Some(Ty::User { name, generic_args }) => {
                assert_eq!(name, "Box");
                assert_eq!(generic_args, &vec![Ty::String]);
            }
            other => panic!("expected Ty::User Box<String>, got {other:?}"),
        }
    }

    /// Records: `new Pair(1, "a")` against `record Pair<A, B>(A a, B b)`
    /// should infer `Pair<int, String>`.
    #[test]
    fn generic_record_inferred_from_components() {
        let (table, unit) = build_table(
            r#"
            public record Pair<A, B>(A a, B b) {}
            public void main() { var p = new Pair(1, "a"); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("p") {
            Some(Ty::User { name, generic_args }) => {
                assert_eq!(name, "Pair");
                assert_eq!(
                    generic_args,
                    &vec![Ty::Primitive(Primitive::Int), Ty::String],
                );
            }
            other => panic!("expected Ty::User Pair<int, String>, got {other:?}"),
        }
    }

    // ============================================================================
    // Cross-extends generic substitution
    // ============================================================================

    /// `class Dog extends Animal<int>` — a field `T value;` declared
    /// on `Animal<T>` should read as `int` when accessed through a
    /// `Dog` receiver.
    #[test]
    fn cross_extends_concrete_field_substitutes() {
        let (table, unit) = build_table(
            r#"
            public class Animal<T> {
                public T tag;
            }
            public class Dog extends Animal<int> {}
            public void main() {
                var d = new Dog();
                var t = d.tag;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("t"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `class Dog extends Animal<int>` — calling an inherited
    /// `T speak()` should return `int` from the `Dog` receiver.
    #[test]
    fn cross_extends_concrete_method_return_substitutes() {
        let (table, unit) = build_table(
            r#"
            public class Animal<T> {
                public T tag;
                public T speak() { return this.tag; }
            }
            public class Dog extends Animal<int> {}
            public void main() {
                var d = new Dog();
                var t = d.speak();
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("t"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `class Sub<U> extends Sup<U>` — the chain forwards the
    /// receiver's `U` binding into the parent's `T` slot.
    #[test]
    fn cross_extends_forwarded_param_substitutes() {
        let (table, unit) = build_table(
            r#"
            public class Sup<T> {
                public T value;
            }
            public class Sub<U> extends Sup<U> {}
            public void main() {
                var s = new Sub<String>();
                var v = s.value;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::String));
    }

    /// Two-hop chain: `C extends B<int>`, `B<T> extends A<T>`,
    /// `A<U> { U tag; }`. A `C` receiver's `tag` should be `int`.
    #[test]
    fn cross_extends_two_hop_substitutes() {
        let (table, unit) = build_table(
            r#"
            public class A<U> { public U tag; }
            public class B<T> extends A<T> {}
            public class C extends B<int> {}
            public void main() {
                var c = new C();
                var t = c.tag;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("t"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Raw extends (`extends Animal`, no `<...>`) leaves the
    /// inherited field as `Ty::Param` — there's nothing to bind T to.
    #[test]
    fn cross_extends_raw_parent_leaves_param() {
        let (table, unit) = build_table(
            r#"
            public class Animal<T> { public T tag; }
            public class Dog extends Animal {}
            public void main() {
                var d = new Dog();
                var t = d.tag;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("t") {
            Some(Ty::Param(name)) => assert_eq!(name, "T"),
            other => panic!("expected Ty::Param(\"T\"), got {other:?}"),
        }
    }

    /// Non-generic classes are unaffected — `new Plain()` still yields
    /// a zero-arg `Ty::User`.
    #[test]
    fn non_generic_class_ctor_unaffected() {
        let (table, unit) = build_table(
            r#"
            public class Plain { public int x = 0; }
            public void main() { var p = new Plain(); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("p") {
            Some(Ty::User { name, generic_args }) => {
                assert_eq!(name, "Plain");
                assert!(generic_args.is_empty());
            }
            other => panic!("expected Ty::User Plain<>, got {other:?}"),
        }
    }
}
