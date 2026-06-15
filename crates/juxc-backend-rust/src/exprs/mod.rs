//! Expression-level lowering — split into action-focused submodules
//! so each file stays readable.
//!
//! - [`field`]  — `obj.field` reads + auto-clone decisions
//! - [`array`]  — `arr[i]`, `new T[N]`, `{a, b, c}` literals
//! - [`simple`] — leaf-shaped emitters (cast, range, unary)
//! - [`binary`] — `+`/`-`/`==` etc., string-concat, operator-overload rewrite
//! - [`call`]   — generic calls + `print(...)` built-in
//!
//! `mod.rs` itself owns the dispatch ([`RustEmitter::emit_expr`]),
//! the [`ArgRef`] / [`UNARY_PREC`] cross-module constants, the
//! precedence-aware paren wrapper ([`RustEmitter::emit_expr_with_parent_prec`]),
//! and the free helpers ([`expr_span_of`], [`ty_kind_from_ref_with_params`],
//! [`binary_prec`]) the submodules and other backend modules call
//! through `crate::exprs::…`.
//!
//! Behavior identical to the pre-split `exprs.rs` — pure file
//! reorganization.

use juxc_ast::{BinaryOp, Expr};
use juxc_tycheck::Ty;

use crate::RustEmitter;

pub(crate) mod array;
pub(crate) mod binary;
pub(crate) mod call;
pub(crate) mod field;
pub(crate) mod simple;

/// Discriminator for `emit_interp_string`'s deferred-arg emission —
/// records the order in which Bare-ident and full-expression arguments
/// appear in the format-string placeholders so we can emit them in
/// matching order after the format string is closed.
pub(crate) enum ArgRef {
    Bare(usize),
    Expr(usize),
}

/// Precedence value for prefix unary operators. Per §A.4 level 18 —
/// tighter than every binary operator currently modeled.
pub(crate) const UNARY_PREC: u8 = 18;

impl RustEmitter {
    /// The real Rust path of an external stub type named by `class_name`
    /// (`std::path::PathBuf` for `PathBuf` / `rust.std.PathBuf`), or `None` for
    /// a non-external type or a stub without a recorded `@rust` path (§G.9.2).
    /// Resolves a bare name through `find_fqn_by_bare` (which prefers a
    /// non-external type, so an unqualified `Box`/`HashMap` is NOT treated as the
    /// Rust-std stub here); a dotted name is taken as the FQN directly.
    pub(crate) fn external_class_real_path(
        &self,
        class_name: &juxc_ast::QualifiedName,
    ) -> Option<String> {
        if class_name.segments.is_empty() {
            return None;
        }
        let fqn = if class_name.segments.len() == 1 {
            self.symbols.find_fqn_by_bare(&class_name.segments[0].text)?
        } else {
            class_name
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(".")
        };
        // A foreign type may be a class OR an enum stub — each records its real
        // Rust path on its own sig. Without the enum arm a `rust.<crate>` enum
        // (`minifb::key::Key`) used in TYPE position would miss the external
        // branch and lower to the bogus `crate::rust::minifb::Key` module path.
        if let Some(sig) = self.symbols.classes.get(&fqn) {
            if sig.is_external {
                return sig.rust_path.clone();
            }
        }
        if let Some(sig) = self.symbols.enums.get(&fqn) {
            if sig.is_external {
                // Prefer the crate's RE-EXPORT path (`minifb::Key`) for a
                // non-std foreign crate: the `@rust("…")` annotation may record
                // the canonical path through a PRIVATE module (`minifb::key::Key`,
                // which trips rustc E0603), while the crate re-exports the type at
                // its root — matching the import's own `use minifb::Key;`. `std`
                // keeps its annotation path (the stub flattens nested std modules,
                // so `std::collections::HashSet` is the only valid form).
                let segs: Vec<&str> = fqn.split('.').collect();
                if segs.first() == Some(&"rust") && segs.len() >= 3 && segs[1] != "std" {
                    return Some(segs[1..].join("::"));
                }
                return sig.rust_path.clone();
            }
        }
        None
    }

    /// True when `ty` (after unwrapping `T?`) is an external (`rust.std` / crate)
    /// stub type. Used to mark external-typed locals `mut` (§G.9.2) and to route
    /// member names through the camelCase→snake_case rewrite.
    pub(crate) fn is_external_user_ty(&self, ty: &juxc_tycheck::Ty) -> bool {
        use juxc_tycheck::Ty;
        match ty {
            Ty::Nullable(inner) => self.is_external_user_ty(inner),
            Ty::User { name, .. } => {
                if let Some(c) = self.symbols.classes.get(name) {
                    return c.is_external;
                }
                self.lookup_class_by_bare_or_fqn(name.rsplit('.').next().unwrap_or(name))
                    .map(|c| c.is_external)
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    pub(crate) fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(lit) => self.emit_literal(lit),
            // `typeof(expr)` (§5.9.10) — replaced at compile time by a
            // `String` literal of the operand's STATIC Jux type name
            // (tycheck's `Ty` Display gives the source spelling). The
            // operand is never evaluated.
            Expr::TypeOf(inner, _) => {
                let name = self
                    .expr_types
                    .get(&expr_span_of(inner))
                    .map(|t| bare_type_spelling(&t.to_string()))
                    .unwrap_or_else(|| "<unknown>".to_string());
                self.w.push('"');
                self.w.push_str(&name.escape_default().to_string());
                self.w.push('"');
                // Owned-String coercion, same rule as string literals:
                // format args and const contexts stay `&str`.
                if !self.emitting_format_arg && !self.emitting_const_context {
                    self.w.push_str(".to_string()");
                }
            }
            // `out <place>` (§M.4) — pass the place by exclusive reference. The
            // place is emitted as an lvalue (no value-read deref / clone), so
            // `out n` → `&mut n`. (Normally reached via `emit_call_arg_value`,
            // which routes here.)
            Expr::Out(place, _) => {
                self.w.push_str("&mut ");
                let prev_lv = self.emitting_lvalue;
                let prev_out = self.emitting_out_place;
                self.emitting_lvalue = true;
                // A wrapper-class field place must take the mutable interior
                // borrow (`b.0.borrow_mut().f`) — see `emitting_out_place`.
                self.emitting_out_place = true;
                self.emit_expr(place);
                self.emitting_lvalue = prev_lv;
                self.emitting_out_place = prev_out;
            }
            // Try-expression (§X.3.3) — produce-or-recover value form.
            Expr::TryExpr(t) => self.emit_try_expr(t),
            // `expr?` — error propagation (§X.4.1). Result operands
            // unwrap `Ok` / early-return `Err`; nullable operands
            // unwrap the value / early-return `None`. The operand
            // class comes from the recorded type (tycheck validated
            // the enclosing return shape).
            Expr::ErrorProp(inner, _) => {
                let inner_ty = self
                    .expr_types
                    .get(&expr_span_of(inner))
                    .cloned();
                let is_result = matches!(
                    &inner_ty,
                    Some(juxc_tycheck::Ty::User { name, generic_args })
                        if name.rsplit('.').next() == Some("Result")
                            && generic_args.len() == 2
                );
                self.w.push_str("(match ");
                self.emit_expr(inner);
                if is_result {
                    self.w.push_str(
                        " { crate::jux::std::result::Result::Ok(__jux_q) => __jux_q, crate::jux::std::result::Result::Err(__jux_e) => return crate::jux::std::result::Result::Err(__jux_e) })",
                    );
                } else {
                    self.w.push_str(
                        " { Some(__jux_q) => __jux_q, None => return None })",
                    );
                }
            }
            // Tuple literal (§5.3) — Rust's identical `(a, b)` form.
            // Value semantics for free; elements emit as ordinary
            // value-position expressions.
            Expr::TupleLit(elems, _) => {
                self.w.push('(');
                for (i, el) in elems.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(el);
                }
                self.w.push(')');
            }
            Expr::Path(qn) => {
                // **Captured enclosing local** (anonymous-class capture): a bare
                // read of a captured name is a field on the synthetic struct, so
                // `name` → `self.name`. A method param of the same name shadows
                // the capture and keeps the bare name. Empty set outside an
                // anon body, so no effect on ordinary code.
                if qn.segments.len() == 1
                    && self.captured_locals.contains(&qn.segments[0].text)
                    && !self.current_fn_params.contains(&qn.segments[0].text)
                {
                    self.w.push_str("self.");
                    self.w.push_str(&qn.segments[0].text);
                    return;
                }
                // **`ref` binding read** (§M.13): a value-position read
                // of a `ref` local/param clones the value OUT of the
                // shared cell (statement-scoped borrow). Lvalue targets
                // are handled by `emit_assign`'s store-through arm and
                // never reach here with the flag set.
                if qn.segments.len() == 1
                    && !self.emitting_lvalue
                    && self.ref_locals.contains(&qn.segments[0].text)
                {
                    self.w.push_str(&qn.segments[0].text);
                    self.w.push_str(".borrow().clone()");
                    return;
                }
                // **`out` parameter access** (§M.4): an out param lowers to
                // `&mut T`, so a value read derefs to `(*result)` and an
                // assignment LHS to `*result`. An inner block local that shadows
                // the parameter name wins (and is NOT an out param).
                if qn.segments.len() == 1
                    && self.out_params.contains(&qn.segments[0].text)
                    && !self
                        .local_types
                        .iter()
                        .any(|s| s.contains_key(&qn.segments[0].text))
                {
                    let name = &qn.segments[0].text;
                    if self.emitting_lvalue {
                        self.w.push('*');
                        self.w.push_str(name);
                    } else {
                        self.w.push_str("(*");
                        self.w.push_str(name);
                        self.w.push(')');
                    }
                    return;
                }
                // Bare-name rewrite for enclosing-class static fields:
                // inside `class Test`, the name `a` resolves to
                // `Test.a` (Java/Jux rule). Detect that case here and
                // forward to the same shape `emit_field` produces
                // for the explicit `Test.a` access — keeps the
                // mutable-static lock/unlock machinery in one place.
                // The implicit-`this` rewrite below applies to a bare *value*
                // reference, never the callee of a `foo(...)` call — a bare
                // method call is resolved by `emit_call` (Java's `foo()` ≡
                // `this.foo()` for methods), and rewriting it here as a field
                // access would call the field instead of the method.
                // Bare reference to an enclosing-class field. NOT a call callee
                // (a bare `foo()` method call is resolved by `emit_call`).
                // **Const-generic param read in value position** — `N` of an
                // enclosing `<int N>` declares as Rust `const N: usize`, but a
                // Jux `int` value is `isize`, so a bare read emits
                // `(N as isize)`. Array-size position (`[T; N]`) wants the raw
                // `usize` and suppresses the cast; a local/param named `N`
                // shadows the generic and wins.
                if qn.segments.len() == 1
                    && !self.in_array_size_position
                    && self.const_int_params.contains(&qn.segments[0].text)
                {
                    let name = &qn.segments[0].text;
                    let shadowed = self.current_fn_params.contains(name)
                        || self.local_types.iter().any(|s| s.contains_key(name));
                    if !shadowed {
                        self.w.push('(');
                        self.w.push_str(name);
                        self.w.push_str(" as isize)");
                        return;
                    }
                }
                if qn.segments.len() == 1 && !self.emitting_call_callee {
                    if let Some(class_name) = self.enclosing_class.clone() {
                        let name = qn.segments[0].text.clone();
                        // A bare name that shadows a field (a parameter or a
                        // local in scope) is NOT a field reference — leave it.
                        let shadowed = self.current_fn_params.contains(&name)
                            || self.local_types.iter().any(|s| s.contains_key(&name));
                        let field = self
                            .lookup_class_by_bare_or_fqn(&class_name)
                            .and_then(|c| c.fields.get(name.as_str()))
                            .map(|f| (f.is_static, f.is_final));
                        if let (false, Some((is_static, is_final))) = (shadowed, field) {
                            if is_static {
                                // Static field — no `this` needed (works in a
                                // static method too).
                                self.emit_enclosing_class_static_ref(&class_name, &name, is_final);
                                return;
                            }
                            // Implicit-`this` for an INSTANCE field (Java rule:
                            // bare `f` ≡ `this.f`), only where a `self`/`__self`
                            // alias is in scope — a static method has no `this`,
                            // so a bare instance-field name there is a real error.
                            if self.this_alias.is_some() {
                                let span = qn.span;
                                let this_field = juxc_ast::FieldExpr {
                                    object: Box::new(Expr::This(span)),
                                    field: juxc_ast::Ident { text: name, span },
                                    safe: false,
                                    span,
                                };
                                self.emit_field(&this_field);
                                return;
                            }
                        }
                    }
                }
                // Dot-separated Jux paths become `::`-separated Rust paths.
                // Module mapping is a TODO — for milestone 1 we emit
                // identical structure on faith.
                let path = qn
                    .segments
                    .iter()
                    .map(|i| i.text.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
                self.w.push_str(&path);
            }
            Expr::Call(c) => {
                // A call to a foreign (`.jux.d`) function/method whose `throws E`
                // maps a Rust `Result<T, E>` (§G.5.4): unwrap the `Result` so the
                // Jux-visible value is `T`, re-throwing the error via `panic_any`
                // on `Err` so an enclosing Jux `try`/`catch` recovers it.
                if self.call_is_foreign_result(c) {
                    self.w.push('(');
                    self.emit_call(c);
                    self.w
                        .push_str(").unwrap_or_else(|__e| std::panic::panic_any(__e))");
                } else {
                    self.emit_call(c);
                }
            }
            Expr::Binary(b) => self.emit_binary(b),
            Expr::IncDec(i) => self.emit_incdec_value(i),
            Expr::Unary(u) => self.emit_unary(u),
            Expr::Range(r) => self.emit_range(r),
            Expr::Cast(c) => self.emit_cast(c),
            Expr::TypeTest(t) => self.emit_type_test(t),
            Expr::SizeOf(s) => self.emit_sizeof(s),
            Expr::NewArray(n) => self.emit_new_array(n),
            Expr::NewArrayLit(n) => self.emit_new_array_lit(n),
            Expr::Index(i) => self.emit_index(i),
            Expr::Field(f) => self.emit_field(f),
            Expr::InterpString(s) => self.emit_interp_string(s),
            Expr::This(_) => {
                // Lowers to `self` in a method or `__self` in a
                // constructor. `this_alias` is set by `emit_method` /
                // `emit_constructor` before they walk the body. Outside
                // any class body it'd be `None`, but the resolver has
                // already flagged that as a use-before-declared.
                let alias = self.this_alias.as_deref().unwrap_or("self");
                self.w.push_str(alias);
            }
            Expr::Super(_) => {
                // `super` as a receiver lowers to the same `self` handle —
                // the static-dispatch semantics of `super.method()` are
                // realized in the call path (`emit_call`), which rewrites the
                // call to a `__jux_super_<m>` shim that runs the ancestor's
                // body. A bare `super` is rejected by tycheck; emitting the
                // `self` alias here keeps the fallback well-formed.
                let alias = self.this_alias.as_deref().unwrap_or("self");
                self.w.push_str(alias);
            }
            Expr::Switch(s) => self.emit_switch(s),
            Expr::NewObject(n) if n.anonymous_body.is_some() => {
                self.emit_anonymous_class(n);
            }
            Expr::NewObject(n) => {
                // **Stdlib compiler primitives** — `new HashMap()`
                // / `new HashSet()` / `new ArrayList()` lower
                // directly to the Rust std container's `new()`
                // with turbofish-spliced generic args. The Jux
                // source files document the API; the compiler
                // knows the mapping by bare name (same small
                // fixed set as the type-position rule above).
                if n.class_name.segments.len() == 1 {
                    let bare = n.class_name.segments[0].text.as_str();
                    if bare == "HashMap" && n.generic_args.len() == 2 {
                        self.w.push_str("std::collections::HashMap::<");
                        let args: Vec<juxc_ast::TypeRef> = n.generic_args.clone();
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.w.push_str(", ");
                            }
                            self.emit_type_as_rust(arg);
                        }
                        self.w.push_str(">::new()");
                        return;
                    }
                    if bare == "ArrayList" && n.generic_args.len() == 1 {
                        self.w.push_str("Vec::<");
                        if let Some(arg) = n.generic_args.first() {
                            self.emit_type_as_rust(arg);
                        }
                        self.w.push_str(">::new()");
                        return;
                    }
                    if bare == "HashSet" && n.generic_args.len() == 1 {
                        self.w.push_str("std::collections::HashSet::<");
                        if let Some(arg) = n.generic_args.first() {
                            self.emit_type_as_rust(arg);
                        }
                        self.w.push_str(">::new()");
                        return;
                    }
                    if bare == "Deque" && n.generic_args.len() == 1 {
                        self.w.push_str("std::collections::VecDeque::<");
                        if let Some(arg) = n.generic_args.first() {
                            self.emit_type_as_rust(arg);
                        }
                        self.w.push_str(">::new()");
                        return;
                    }
                    // Atomic counters (§S.6.2) — Arc-wrapped so the
                    // handle shares across spawn boundaries.
                    if (bare == "AtomicInt" || bare == "AtomicLong")
                        && n.generic_args.is_empty()
                    {
                        let inner = if bare == "AtomicInt" {
                            "AtomicIsize"
                        } else {
                            "AtomicI64"
                        };
                        self.w.push_str("std::sync::Arc::new(std::sync::atomic::");
                        self.w.push_str(inner);
                        self.w.push_str("::new(");
                        let prev = self.emitting_format_arg;
                        self.emitting_format_arg = false;
                        if let Some(arg) = n.args.first() {
                            self.emit_expr(arg);
                        } else {
                            self.w.push('0');
                        }
                        self.emitting_format_arg = prev;
                        self.w.push_str("))");
                        return;
                    }
                }
                // `new Foo(args)`              → `Foo::new(args)`.
                // `new com.lib.Foo(args)`      → `crate::com::lib::Foo::new(args)`.
                // `new Foo<int>(args)`         → `Foo::<isize>::new(args)`
                //                                (Rust turbofish form).
                //
                // **`crate::` prefix on multi-segment names.** The
                // path the user wrote is absolute from the crate
                // root — `poll.lib.Animal` always means the class
                // at `crate::poll::lib::Animal` regardless of how
                // deep the surrounding `pub mod` nest is. Without
                // the `crate::` prefix, Rust would try to resolve
                // `poll::lib::…` relative to the enclosing module
                // and fail. Single-segment names depend on the
                // unit's `use` statements (or same-package
                // visibility) for resolution, so they're emitted
                // bare.
                // Cross-package auto-import lookup: single-segment
                // names that resolve to an FQN in a different
                // package get the fully-qualified `crate::a::b::…`
                // form. Same-package single-segment names stay
                // bare. Mirrors the type-position rule in
                // `emit_type_as_rust`.
                // §G.9.2: constructing an external stub type (`new PathBuf()`)
                // lowers to its REAL Rust path's `::new()` —
                // `std::path::PathBuf::new()` — never the Jux
                // `crate::rust::std::PathBuf`. The real path is recorded on
                // `ClassSig::rust_path` from the `@rust("…")` annotation.
                // `new Channel<T>(capacity)` — async-runtime builtin
                // type (§18.3); lowers to the emitted JuxChannel
                // helper. Recognized before user-class resolution so
                // no stdlib stub is needed.
                // `new AsyncMutex<T>(v)` — §18.3 runtime helper.
                if n.class_name.segments.len() == 1
                    && n.class_name.segments[0].text == "AsyncMutex"
                    && !self.symbols.classes.contains_key("AsyncMutex")
                {
                    self.w.push_str("crate::JuxAsyncMutex::new(");
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    if let Some(v) = n.args.first() {
                        self.emit_expr(v);
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
                if n.class_name.segments.len() == 1
                    && n.class_name.segments[0].text == "Channel"
                    && !self.symbols.classes.contains_key("Channel")
                {
                    self.w.push_str("crate::JuxChannel::new(");
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    if let Some(cap) = n.args.first() {
                        self.emit_expr(cap);
                    } else {
                        self.w.push('1');
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
                let (path, prepend_crate) = if let Some(real) =
                    self.external_class_real_path(&n.class_name)
                {
                    (real, false)
                } else if n.class_name.segments.len() == 1 {
                    let bare = n.class_name.segments[0].text.as_str();
                    if let Some(fqn) = self.symbols.find_fqn_by_bare(bare) {
                        if fqn.contains('.') {
                            let cur_pkg = self.symbols.package.join(".");
                            let fqn_pkg = fqn
                                .rsplit_once('.')
                                .map(|(p, _)| p.to_string())
                                .unwrap_or_default();
                            if fqn_pkg != cur_pkg {
                                let joined = fqn
                                    .split('.')
                                    .collect::<Vec<_>>()
                                    .join("::");
                                (joined, true)
                            } else {
                                (bare.to_string(), false)
                            }
                        } else {
                            (bare.to_string(), false)
                        }
                    } else {
                        (bare.to_string(), false)
                    }
                } else {
                    // §M.9 qualified nested-type construction:
                    // `new HttpServer.Config()` targets the lifted
                    // `HttpServer__Config`. Try the mangled form
                    // first (same-package bare / cross-package
                    // crate-rooted); package-qualified class paths
                    // fall through to the `::` join.
                    let first = n.class_name.segments[0].text.as_str();
                    let rest = n.class_name.segments[1..]
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .join("__");
                    let mangled = format!("{first}__{rest}");
                    let is_lifted = |f: &String| {
                        self.symbols.classes.contains_key(f)
                            || self.symbols.records.contains_key(f)
                            || self.symbols.enums.contains_key(f)
                    };
                    let mangled_fqn = if is_lifted(&mangled) {
                        Some(mangled.clone())
                    } else {
                        self.symbols.find_fqn_by_bare(&mangled).filter(is_lifted)
                    };
                    if let Some(fqn) = mangled_fqn {
                        let cur_pkg = self.symbols.package.join(".");
                        let fqn_pkg = fqn
                            .rsplit_once('.')
                            .map(|(p, _)| p.to_string())
                            .unwrap_or_default();
                        if fqn.contains('.') && fqn_pkg != cur_pkg {
                            (fqn.split('.').collect::<Vec<_>>().join("::"), true)
                        } else {
                            (mangled, false)
                        }
                    } else {
                        let joined = n
                            .class_name
                            .segments
                            .iter()
                            .map(|s| s.text.as_str())
                            .collect::<Vec<_>>()
                            .join("::");
                        (joined, true)
                    }
                };
                // A foreign ctor lowered from `new() -> Result<Self, E>`
                // (§G.5.4): wrap the whole call so the `Result` is unwrapped at
                // the use site, re-throwing on `Err` via `panic_any` for an
                // enclosing Jux `try`/`catch` (mirrors `call_is_foreign_result`).
                let ctor_is_foreign_result = n
                    .class_name
                    .segments
                    .last()
                    .map(|s| s.text.as_str())
                    .and_then(|nm| self.lookup_class_by_bare_or_fqn(nm))
                    .and_then(|c| {
                        c.constructors
                            .iter()
                            .find(|ct| ct.params.len() == n.args.len())
                            .or_else(|| c.constructors.first())
                    })
                    .map(|ct| ct.is_foreign_result)
                    .unwrap_or(false);
                if ctor_is_foreign_result {
                    self.w.push('(');
                }
                // Mark the writer BEFORE the callee so a re-ordered
                // named ctor (§S.1.4 / C7) can split off `Class::new(`
                // and re-emit it after the lexical-order arg temps.
                let mark_callee = self.w.len();
                if prepend_crate {
                    self.w.push_str("crate::");
                }
                self.w.push_str(&path);
                if !n.generic_args.is_empty() {
                    self.w.push_str("::<");
                    // Clone to release the immutable borrow on `n` before
                    // the `emit_type_as_rust` calls (which need `&mut self`).
                    let args: Vec<juxc_ast::TypeRef> = n.generic_args.clone();
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.emit_type_as_rust(arg);
                    }
                    self.w.push('>');
                }
                // §G.5.1 — implicit zero-arg constructor for a foreign type with
                // no `new`. A Rust type built via `Default::default()` (the
                // idiomatic "give me one with defaults" for option/config structs
                // like `WindowOptions`) has no inherent constructor, so bindgen
                // surfaces none. `new T()` is then the single Jux construction
                // idiom and lowers to `T::default()` — `default` itself is never
                // exposed as a member. Only fires for external (`@rust`) types
                // with no zero-arg constructor; user classes keep their `::new`.
                let foreign_default_ctor = n.args.is_empty()
                    && self.external_class_real_path(&n.class_name).is_some()
                    && !n
                        .class_name
                        .segments
                        .last()
                        .map(|s| s.text.as_str())
                        .and_then(|nm| self.lookup_class_by_bare_or_fqn(nm))
                        .map(|c| c.constructors.iter().any(|ct| ct.params.is_empty()))
                        .unwrap_or(false);
                if foreign_default_ctor {
                    self.w.push_str("::default()");
                    return;
                }
                // Constructor-overload pick (§7.3.1): count-based
                // suffix re-derived against the class's ctor list.
                // Derived from the EMITTED path's final segment so a
                // lifted nested type (`HttpServer__Config`) keys the
                // lookup by its real registered name.
                let ctor_bare = path
                    .rsplit("::")
                    .next()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let ctor_sfx =
                    self.ctor_overload_suffix_for_span(&ctor_bare, n.args.len(), n.span);
                self.w.push_str("::new");
                self.w.push_str(&ctor_sfx);
                self.w.push('(');
                // Constructor args consume their values, so any
                // nested string literal needs the Fix-1 self-coerce
                // — clear the format-arg flag for the arg emission.
                // Per-arg nullable-wrap: when a positional ctor
                // parameter is `T?`, a non-nullable arg is lifted
                // into `Some(arg)` so the field's `Option<T>`
                // type-check passes.
                //
                // Two callee shapes carry constructor signatures:
                // **classes** (declared `constructors`) and
                // **records** (synthesized canonical ctor matching
                // the component list). We consult both — records
                // were missing in the original wiring, which left
                // `new Maybe<String>("hello")` un-wrapped when
                // `Maybe`'s component is `String?`.
                let bare_class = n.class_name.segments.last().map(|s| s.text.as_str());
                // A param is nullable when its DECLARED type is `T?` — or
                // when it's a bare generic param (`T v`) whose explicit
                // type argument at this `new` site is nullable
                // (`new Box<int?>(7)` must wrap the 7 in `Some`). The
                // substitution check lines class generic params up with
                // `n.generic_args` positionally.
                let param_nullable = |param_ty: &juxc_ast::TypeRef,
                                      generic_params: &[juxc_ast::TypeParam]|
                 -> bool {
                    if param_ty.nullable {
                        return true;
                    }
                    if param_ty.array_shape.is_some()
                        || param_ty.fn_shape.is_some()
                        || !param_ty.generic_args.is_empty()
                        || param_ty.name.segments.len() != 1
                    {
                        return false;
                    }
                    let bare = param_ty.name.segments[0].text.as_str();
                    generic_params
                        .iter()
                        .position(|gp| gp.name.text == bare)
                        .and_then(|i| n.generic_args.get(i))
                        .map(|arg| arg.nullable)
                        .unwrap_or(false)
                };
                let ctor_nullable_flags: Vec<bool> = bare_class
                    .and_then(|name| {
                        // FQN-tolerant lookup: classes/records may be
                        // keyed under their full package name when
                        // imported across packages, while the
                        // `new C(...)` syntax site only carries the
                        // bare or imported name. Helper falls back
                        // to a suffix scan so cross-package ctor
                        // auto-`Some()` wrapping works the same as
                        // single-file emission.
                        self.lookup_class_by_bare_or_fqn(name)
                            .map(|c| {
                                let gp = c.generic_params.clone();
                                // Pick the overload matching this call's arity so
                                // nullable flags come from the right constructor —
                                // always using `first()` broke calls to a second
                                // overload whose later params are nullable (e.g.
                                // Exception(msg, cause?) with arg count 2).
                                let ctor = c
                                    .constructors
                                    .iter()
                                    .find(|ctor| ctor.params.len() == n.args.len())
                                    .or_else(|| c.constructors.first());
                                ctor.map(|ctor| {
                                        ctor.params
                                            .iter()
                                            .map(|p| param_nullable(&p.ty, &gp))
                                            .collect()
                                    })
                                    .unwrap_or_default()
                            })
                            .or_else(|| {
                                self.symbols
                                    .records
                                    .iter()
                                    .find(|(k, _)| {
                                        k.as_str() == name
                                            || k.rsplit('.').next().unwrap_or(k.as_str()) == name
                                    })
                                    .map(|(_, r)| {
                                        let gp = r.generic_params.clone();
                                        r.components
                                            .iter()
                                            .map(|c| param_nullable(&c.ty, &gp))
                                            .collect()
                                    })
                            })
                    })
                    .unwrap_or_default();
                // Constructor parameter TYPES — for coercing a subclass /
                // implementer argument into an interface / polymorphic-base
                // (`Rc<dyn …>`) parameter slot, mirroring the function-call
                // arg path. Without this, `new Holder(new Dog())` where the
                // param is `Animal`/an interface would pass a raw value.
                let ctor_param_types: Vec<juxc_ast::TypeRef> = n
                    .class_name
                    .segments
                    .last()
                    .map(|s| s.text.as_str())
                    .and_then(|name| {
                        self.lookup_class_by_bare_or_fqn(name)
                            // Pick the constructor overload matching this call's
                            // arity (was `constructors.first()`, which took the
                            // wrong overload's param types — so args past the
                            // first ctor's arity got no coercion). Fall back to
                            // the first ctor when no arity matches (e.g. varargs).
                            .and_then(|c| {
                                c.constructors
                                    .iter()
                                    .find(|ctor| ctor.params.len() == n.args.len())
                                    .or_else(|| c.constructors.first())
                            })
                            .map(|ctor| ctor.params.iter().map(|p| p.ty.clone()).collect())
                            .or_else(|| {
                                self.symbols
                                    .records
                                    .iter()
                                    .find(|(k, _)| {
                                        k.as_str() == name
                                            || k.rsplit('.').next().unwrap_or(k.as_str()) == name
                                    })
                                    .map(|(_, r)| {
                                        r.components.iter().map(|c| c.ty.clone()).collect()
                                    })
                            })
                    })
                    .unwrap_or_default();
                // §G.9.2: foreign-borrow (`&T`) constructor parameters get the
                // call-site `&` re-added — mirrors the method-call path so a
                // `Window(&str name, …)` receives `&"…".to_string()` (coerces to
                // `&str`), not an owned `String`.
                let ctor_param_is_ref: Vec<bool> = n
                    .class_name
                    .segments
                    .last()
                    .map(|s| s.text.as_str())
                    .and_then(|name| {
                        self.lookup_class_by_bare_or_fqn(name).and_then(|c| {
                            c.constructors
                                .iter()
                                .find(|ctor| ctor.params.len() == n.args.len())
                                .or_else(|| c.constructors.first())
                                .map(|ctor| ctor.params.iter().map(|p| p.is_ref).collect())
                        })
                    })
                    .unwrap_or_default();
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                // The per-arg coercion (iface/base wrap, nullable
                // `Some()`, wrapper share-clone), shared by the inline
                // and the lexical-hoist paths.
                let emit_one = |this: &mut Self, i: usize, arg: &juxc_ast::Expr| {
                    if let Some(pty) = ctor_param_types.get(i) {
                        if !matches!(
                            this.iface_coercion_to(pty, arg),
                            crate::analysis::IfaceCoercion::None,
                        ) {
                            this.emit_expr_coerced_to_iface(pty, arg);
                            return;
                        }
                    }
                    let by_ref = ctor_param_is_ref.get(i).copied().unwrap_or(false);
                    if by_ref {
                        this.w.push('&');
                    }
                    let nullable = ctor_nullable_flags.get(i).copied().unwrap_or(false);
                    this.emit_arg_with_nullable_wrap(arg, nullable);
                    if !nullable && this.wrapper_value_needs_clone(arg) {
                        this.w.push_str(".clone()");
                    }
                };
                if !n.eval_order.is_empty() {
                    // §S.1.4 (C7): the named args were re-ordered. Pull
                    // the already-emitted `Class::new(` back out, bind
                    // each (coerced) arg to `__jux_arg{slot}` in
                    // call-site LEXICAL order, then re-emit the callee
                    // passing the temps POSITIONALLY — side effects fire
                    // left-to-right as written, slots stay correct.
                    let callee = self.w.split_off_from(mark_callee);
                    self.w.push_str("{ ");
                    for &slot in &n.eval_order {
                        if let Some(arg) = n.args.get(slot) {
                            self.w.push_str(&format!("let __jux_arg{slot} = "));
                            emit_one(self, slot, arg);
                            self.w.push_str("; ");
                        }
                    }
                    self.w.push_str(&callee);
                    for i in 0..n.args.len() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.w.push_str(&format!("__jux_arg{i}"));
                    }
                    self.w.push_str(") }");
                } else {
                    for (i, arg) in n.args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        emit_one(self, i, arg);
                    }
                    self.w.push(')');
                }
                if ctor_is_foreign_result {
                    self.w
                        .push_str(").unwrap_or_else(|__e| std::panic::panic_any(__e))");
                }
                self.emitting_format_arg = prev;
            }
            Expr::Lambda(l) => self.emit_lambda(l),
            Expr::Elvis(e) => self.emit_elvis(e),
            Expr::MethodRef(m) => self.emit_method_ref(m),
            Expr::Ternary(t) => self.emit_ternary(t),
            Expr::Await(inner, _) => self.emit_await(inner),
            // `expr!!` — non-null assertion (§A.4 level 19). A nullable
            // operand is an `Option<T>` in the emitted Rust: unwrap it,
            // panicking `NullPointerException` on `None` (same panic
            // convention as the ClassCastException downcast hook). A
            // non-nullable operand makes the assert a no-op — emit the
            // operand bare rather than a broken `.unwrap_or_else` on a
            // non-Option value.
            Expr::NotNullAssert(inner, _) => {
                if self.expression_is_already_nullable(inner) {
                    // Parenthesize so postfix chains bind to the
                    // unwrapped value (`(expr).unwrap…().id`). The
                    // operand emits with the format/comparison flags
                    // cleared — the asserted value is consumed by the
                    // unwrap, not by the surrounding Display slot, so a
                    // wrapper-borrowed field read must keep its normal
                    // clone-out (`a.0.borrow().peer.clone()`); the
                    // suppressed form would try to MOVE the Option out
                    // of the `Ref` (rustc E0507).
                    let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
                    let prev_cmp =
                        std::mem::take(&mut self.emitting_comparison_operand);
                    self.w.push('(');
                    self.emit_expr(inner);
                    self.w.push(')');
                    self.emitting_format_arg = prev_fmt;
                    self.emitting_comparison_operand = prev_cmp;
                    self.w.push_str(
                        ".unwrap_or_else(|| panic!(\"NullPointerException: \
                         `!!` asserted on a null value\"))",
                    );
                } else {
                    self.emit_expr(inner);
                }
            }
        }
    }

    /// Lower `await expr` to Rust's postfix `.await`.
    ///
    /// Rust spells await as `expr.await`, not `await expr`, so we
    /// emit the operand first, then the suffix. The operand is
    /// parenthesized when it isn't already a self-delimiting
    /// expression (path, call, field) — `await (a + b)` needs to
    /// land as `(a + b).await`, not `a + b.await` (which Rust
    /// parses as `a + (b.await)`).
    pub(crate) fn emit_await(&mut self, operand: &Expr) {
        // Self-delimiting expressions don't need wrapping parens —
        // a path, call, field access, or this/new is already a
        // postfix-friendly receiver. Everything else (binary,
        // unary, range, etc.) does, since `.await` binds tightly.
        let needs_parens = !matches!(
            operand,
            Expr::Path(_)
                | Expr::Call(_)
                | Expr::Field(_)
                | Expr::Index(_)
                | Expr::This(_)
                | Expr::NewObject(_)
                | Expr::Literal(_)
        );
        if needs_parens {
            self.w.push('(');
        }
        self.emit_expr(operand);
        if needs_parens {
            self.w.push(')');
        }
        self.w.push_str(".await");
    }

    /// Lower `cond ? then : else` to Rust's `if cond { then }
    /// else { else }` expression form — Rust's only multi-arm
    /// value expression that matches the ternary's semantics.
    /// We use the inline form (no statement-style braces around
    /// the whole thing) so it composes inside larger expressions
    /// (`var y = x > 0 ? "+" : "-"` becomes
    /// `let y = if x > 0 { "+" } else { "-" }`).
    ///
    /// Per-arm `Some(...)` wrap propagates from `emitting_nullable_target`
    /// — the same discipline `emit_switch` uses for nullable-
    /// returning fns. A ternary returning `String?` with mixed
    /// `T` / `null` branches produces `if cond { Some(...) }
    /// else { None }`.
    pub(crate) fn emit_ternary(&mut self, t: &juxc_ast::TernaryExpr) {
        let wrap_each_arm = self.emitting_nullable_target;
        let prev = self.emitting_nullable_target;
        self.emitting_nullable_target = false;
        self.w.push_str("if ");
        self.emit_expr(&t.condition);
        self.w.push_str(" { ");
        self.emit_ternary_arm(&t.then_branch, wrap_each_arm);
        self.w.push_str(" } else { ");
        self.emit_ternary_arm(&t.else_branch, wrap_each_arm);
        self.w.push_str(" }");
        self.emitting_nullable_target = prev;
    }

    fn emit_ternary_arm(&mut self, arm: &Expr, wrap_each_arm: bool) {
        let wrap = wrap_each_arm
            && !matches!(arm, Expr::Literal(juxc_ast::Literal::Null))
            && !self.expression_is_already_nullable(arm);
        if wrap {
            self.w.push_str("Some(");
        }
        self.emit_expr(arm);
        if wrap {
            self.w.push(')');
        }
    }

    /// Lower `Receiver::member` to an `Rc<dyn Fn(...) -> R>` —
    /// always a closure wrapper, even for static methods, so the
    /// value flows into Jux function-typed slots
    /// (`Rc<dyn Fn(...)>` shape). Rust function items don't auto-
    /// coerce to `dyn Fn`; wrapping unifies both shapes.
    ///
    /// **Shapes emitted:**
    ///
    /// - **Instance method** `User::greet` (arity N) →
    ///   `Rc::new(move |__r, a0, a1, …| __r.greet(a0, a1, …))`
    ///   The receiver is the first closure parameter; the method's
    ///   declared positional args follow.
    /// - **Static method** `Math::abs` (arity N) →
    ///   `Rc::new(move |a0, a1, …| Math::abs(a0, a1, …))`
    ///   No receiver — the args mirror the method's signature.
    ///
    /// Param types are elided in the closure so Rust infers them
    /// from the surrounding function-typed slot. The receiver type
    /// is the only explicit annotation on the instance form,
    /// since it can't be inferred from context. Multi-segment
    /// receivers get the `crate::` prefix the same way
    /// `NewObject` does.
    ///
    /// When the symbol table doesn't carry signature info (member
    /// is on a record / enum / unknown type, or arity can't be
    /// looked up), we default to the **zero-arg instance** shape;
    /// Rust will surface any real mismatch.
    pub(crate) fn emit_method_ref(&mut self, m: &juxc_ast::MethodRefExpr) {
        let receiver_name = m
            .receiver
            .segments
            .last()
            .map(|s| s.text.as_str())
            .unwrap_or("");
        // Constructor reference (§M.8): `Type::new` becomes a closure
        // constructing the type — `Rc::new(move |a0, ..| Type::new(a0, ..))`.
        // Overloaded constructors: Phase 1 binds the FIRST declared
        // one (a ref site carries no argument count to select by).
        if m.member.text == "new" {
            let ctor_params: Vec<juxc_ast::Param> = self
                .symbols
                .classes
                .get(receiver_name)
                .or_else(|| {
                    self.symbols
                        .find_fqn_by_bare(receiver_name)
                        .and_then(|fqn| self.symbols.classes.get(&fqn))
                })
                .and_then(|c| c.constructors.first())
                .map(|c| {
                    c.params
                        .iter()
                        .map(|p| juxc_ast::Param {
                            name: juxc_ast::Ident {
                                text: p.name.clone(),
                                span: m.span,
                            },
                            ty: p.ty.clone(),
                            is_final: false,
                            is_ref: false,
                            default: None,
                            is_varargs: false,
                            is_out: false,
                            is_shared_ref: false,
                            is_weak: false,
                            span: m.span,
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.w.push_str("std::rc::Rc::new(move |");
            for (i, p) in ctor_params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str("__a");
                self.w.push_str(&i.to_string());
                self.w.push_str(": ");
                let _ = p;
                self.emit_value_type_as_rust(&ctor_params[i].ty);
            }
            self.w.push_str("| ");
            // Crate-root cross-package receivers, same as `new` sites.
            let bare_is_local = self.symbols.classes.contains_key(receiver_name);
            if let Some(fqn) = (!bare_is_local)
                .then(|| self.symbols.find_fqn_by_bare(receiver_name))
                .flatten()
            {
                self.w.push_str("crate::");
                self.w
                    .push_str(&fqn.split('.').collect::<Vec<_>>().join("::"));
            } else {
                for (i, seg) in m.receiver.segments.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str("::");
                    }
                    self.w.push_str(&seg.text);
                }
            }
            self.w.push_str("::new(");
            for i in 0..ctor_params.len() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str("__a");
                self.w.push_str(&i.to_string());
            }
            self.w.push_str("))");
            return;
        }
        // The class/interface is keyed by FQN, so a BARE receiver name
        // (`Stats::cube`) needs the same `find_fqn_by_bare` fallback the
        // constructor branch above uses — otherwise the lookup misses, the
        // method's `is_static` defaults to `false`, and a STATIC method ref
        // wrongly lowers to the instance form `|__r| __r.cube()` (rustc E0599:
        // `cube` is an associated function, not a method).
        let class_method = self
            .symbols
            .classes
            .get(receiver_name)
            .or_else(|| {
                self.symbols
                    .find_fqn_by_bare(receiver_name)
                    .and_then(|fqn| self.symbols.classes.get(&fqn))
            })
            .and_then(|c| c.methods.get(m.member.text.as_str()));
        // Interface lookup runs in parallel — `MathLike::doubled`
        // doesn't appear in `classes` but does in `interfaces`. The
        // call-site spelling for an interface static is the free
        // function `Iface_method` (see `emit_interface_decl`); for
        // instance / default methods the closure still takes a
        // receiver and calls through the trait method on it.
        let iface_method = self
            .symbols
            .interfaces
            .get(receiver_name)
            .or_else(|| {
                self.symbols
                    .find_fqn_by_bare(receiver_name)
                    .and_then(|fqn| self.symbols.interfaces.get(&fqn))
            })
            .and_then(|i| i.methods.get(m.member.text.as_str()));
        let is_interface_static = iface_method
            .map(|mi| mi.is_static)
            .unwrap_or(false);
        let method_info = class_method.or(iface_method);
        let is_static = method_info.map(|mi| mi.is_static).unwrap_or(false);
        let arity = method_info.map(|mi| mi.params.len()).unwrap_or(0);

        self.w.push_str("std::rc::Rc::new(move |");
        if !is_static {
            // Receiver parameter, with explicit type so the
            // closure body's method call resolves.
            self.w.push_str("__r: ");
            if m.receiver.segments.len() > 1 {
                self.w.push_str("crate::");
            }
            for (i, seg) in m.receiver.segments.iter().enumerate() {
                if i > 0 {
                    self.w.push_str("::");
                }
                self.w.push_str(&seg.text);
            }
            for i in 0..arity {
                self.w.push_str(", ");
                self.w.push_str(&format!("__a{i}"));
            }
        } else {
            for i in 0..arity {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&format!("__a{i}"));
            }
        }
        self.w.push_str("| ");
        if is_static {
            if m.receiver.segments.len() > 1 {
                self.w.push_str("crate::");
            }
            if is_interface_static {
                // Interface statics are free functions named
                // `Iface_method`. Concatenate with `_` rather
                // than the class-side `::` so we hit the
                // companion definition site.
                for (i, seg) in m.receiver.segments.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str("::");
                    }
                    self.w.push_str(&seg.text);
                }
                self.w.push('_');
                self.w.push_str(&m.member.text);
            } else {
                for (i, seg) in m.receiver.segments.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str("::");
                    }
                    self.w.push_str(&seg.text);
                }
                self.w.push_str("::");
                self.w.push_str(&m.member.text);
            }
            self.w.push('(');
            for i in 0..arity {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&format!("__a{i}"));
            }
            self.w.push(')');
        } else {
            self.w.push_str("__r.");
            self.w.push_str(&m.member.text);
            self.w.push('(');
            for i in 0..arity {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&format!("__a{i}"));
            }
            self.w.push(')');
        }
        self.w.push(')');
    }

    /// Lower `value ?: fallback` to Rust. `value` has type
    /// `Option<T>`; `fallback` has type `T`. The simple
    /// `value.unwrap_or(fallback)` works as long as we don't
    /// re-use `value` after — which is the case for an Elvis
    /// expression's own evaluation (the result IS the consumption).
    ///
    /// `unwrap_or` evaluates the fallback eagerly. When the user
    /// puts a side-effecting expression there (`x ?: launch()`),
    /// Rust still runs `launch()` exactly as Jux semantics expect.
    /// `unwrap_or_else` would defer it; for now eager matches the
    /// spec text "else `b`".
    pub(crate) fn emit_elvis(&mut self, e: &juxc_ast::ElvisExpr) {
        let value_needs_parens = !matches!(
            *e.value,
            Expr::Path(_)
                | Expr::This(_)
                | Expr::Field(_)
                | Expr::Call(_)
                | Expr::Index(_)
                | Expr::Literal(_)
                | Expr::InterpString(_)
                | Expr::NewObject(_)
                | Expr::NewArray(_)
                | Expr::NewArrayLit(_)
        );
        // Both sides are value-consuming positions (`unwrap_or`
        // takes `self` and `default: T` by value). Inside a
        // `println!`/`format!` arg this matters: the format-arg
        // flag is set on the way in, so any literal nested inside
        // — e.g. the fallback `"no note"` in
        // `note ?? "no note"` — must still self-coerce to `String`
        // because `unwrap_or`'s `T` is `String`. Clear the flag
        // for the whole elvis emission and restore after.
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        if value_needs_parens {
            self.w.push('(');
        }
        self.emit_expr(&e.value);
        if value_needs_parens {
            self.w.push(')');
        }
        // Preserve the LHS binding when it's a Path or Field read:
        // `x ?? b` should leave `x` usable after the expression.
        // `.clone().unwrap_or(b)` clones the `Option<T>` (which
        // for `T: Clone` clones the inner `T`) so the original
        // binding stays whole. For non-Path / non-Field LHS
        // (call results, indices, switch expressions, …) the
        // value is fresh — no need to clone, the bare
        // `.unwrap_or(b)` move is fine.
        //
        // Field-read auto-clone (see `emit_field`) only fires for
        // `Ty::String`/`Ty::Param` field types today and would
        // skip nullable fields, so we add the clone here at the
        // elvis level instead of relying on the field-read path.
        let preserve_lhs = matches!(*e.value, Expr::Path(_) | Expr::Field(_));
        if preserve_lhs {
            self.w.push_str(".clone()");
        }
        self.w.push_str(".unwrap_or(");
        self.emit_expr(&e.fallback);
        self.w.push(')');
        self.emitting_format_arg = prev;
    }

    /// Emit a Jux lambda as a Rust closure, wrapped in `Rc::new`
    /// so it can flow into `std::rc::Rc<dyn Fn(...) -> ...>` slots
    /// (the Phase-1 lowering of `(A, B) -> R` function types).
    /// Rust's `CoerceUnsized` on `Rc` auto-converts `Rc<{closure}>`
    /// to `Rc<dyn Fn>` at the call site, so the same emission
    /// works whether the lambda is stored locally or passed to a
    /// function-typed param.
    ///
    /// Shape mapping:
    /// - `x -> x + 1`                 → `Rc::new(|x| x + 1)`
    /// - `(a, b) -> a + b`           → `Rc::new(|a, b| a + b)`
    /// - `(int x) -> x * 2`          → `Rc::new(|x: isize| x * 2)`
    /// - `(x) -> { … return x; }`   → `Rc::new(|x| { …; x })`
    ///
    /// Capture semantics (borrow vs `move`) are left to Rust's own
    /// closure inference. Phase 1 doesn't insert an explicit `move`.
    /// Emit a Jux lambda as a bare `move |args| body` Rust closure
    /// — no `Rc<dyn Fn>` wrapper. Used by call sites like
    /// `Worker.spawn(...)` where the closure is consumed directly
    /// (FnOnce + Send + 'static); the wrapping `Rc` of the regular
    /// emit path is incompatible with cross-thread transfer
    /// because `Rc` isn't `Send`.
    /// O1 patch-up for lambda blocks ending in `try`/`catch`: the try
    /// lowering threads `return` values through a `__jux_ret` Option and
    /// re-issues the `return` AFTER the `catch_unwind` machinery — so the
    /// try statement itself has type `()`. When that try is the LAST
    /// statement of a value-returning lambda, Rust would infer the
    /// closure's return type as `()` (E0308). Emitting `unreachable!()`
    /// after it gives the tail position type `!`, which coerces to any
    /// `T`. Only fires for VALUED returns (`return expr`) — a void
    /// lambda may legally fall through past its try, and panicking
    /// there would be wrong.
    fn patch_lambda_tail_try(&mut self, b: &juxc_ast::Block) {
        if let Some(juxc_ast::Stmt::Try(t)) = b.statements.last() {
            let threads_value = crate::stmts::block_contains_valued_return(&t.body)
                || t.catches
                    .iter()
                    .any(|c| crate::stmts::block_contains_valued_return(&c.body));
            if threads_value {
                self.w.line("unreachable!()");
            }
        }
    }

    pub(crate) fn emit_bare_move_lambda(&mut self, l: &juxc_ast::LambdaExpr) {
        self.w.push_str("move ");
        self.w.push('|');
        for (i, p) in l.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&p.name.text);
            if let Some(t) = &p.ty {
                self.w.push_str(": ");
                self.emit_type_as_rust(t);
            }
        }
        self.w.push_str("| ");
        // S17: an ASYNC lambda handed to `Worker.spawn` runs on an OS
        // thread that has no ambient executor — drive the async body
        // to completion right there. Without this the body emitted as
        // a plain closure and any `await` inside was rustc E0728.
        if l.is_async {
            self.w
                .push_str("futures::executor::block_on(async move ");
            match &l.body {
                juxc_ast::LambdaBody::Expr(e) => {
                    self.w.push_str("{ ");
                    self.emit_expr(e);
                    self.w.push_str(" }");
                }
                juxc_ast::LambdaBody::Block(b) => {
                    let prev_lam = self.in_lambda_body;
                    self.in_lambda_body = true;
                    self.w.push_str("{\n");
                    self.w.indent_inc();
                    for stmt in &b.statements {
                        self.emit_stmt(stmt);
                    }
                    self.patch_lambda_tail_try(b);
                    self.w.indent_dec();
                    self.w.emit_indent();
                    self.w.push('}');
                    self.in_lambda_body = prev_lam;
                }
            }
            self.w.push(')');
            return;
        }
        match &l.body {
            juxc_ast::LambdaBody::Expr(e) => self.emit_expr(e),
            juxc_ast::LambdaBody::Block(b) => {
                // S9: see `emit_lambda` — lambda-body tries type
                // their return channels by inference.
                let prev_lam = self.in_lambda_body;
                self.in_lambda_body = true;
                self.w.push_str("{\n");
                self.w.indent_inc();
                for stmt in &b.statements {
                    self.emit_stmt(stmt);
                }
                self.patch_lambda_tail_try(b);
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push('}');
                self.in_lambda_body = prev_lam;
            }
        }
    }

    /// Collect the **wrapper-class captures** of a lambda — bare names
    /// read in the body (minus the lambda's own params) whose type
    /// resolves to a wrapper class. Each needs a share-clone before the
    /// `move` (see `emit_lambda`): without it the closure would STEAL
    /// the caller's `Rc` handle, killing the binding for code after the
    /// lambda (rustc E0382) — Java closures capture the reference, not
    /// the variable.
    fn collect_wrapper_captures(&self, l: &juxc_ast::LambdaExpr) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        collect_bare_names_in_lambda(l, &mut |name| {
            if seen.insert(name.to_string()) {
                names.push(name.to_string());
            }
        });
        let params: std::collections::HashSet<&str> =
            l.params.iter().map(|p| p.name.text.as_str()).collect();
        names.retain(|n| {
            if params.contains(n.as_str()) {
                return false;
            }
            // Resolve the name's class via the local-type registry; a
            // name we can't type keeps the old capture-by-move shape.
            let class = self
                .local_types
                .iter()
                .rev()
                .find_map(|s| s.get(n))
                .and_then(|ty| match ty {
                    juxc_tycheck::Ty::User { name, .. } => {
                        Some(name.rsplit('.').next().unwrap_or(name).to_string())
                    }
                    _ => None,
                });
            class
                .map(|c| self.wrapper_classes.contains(&c))
                .unwrap_or(false)
        });
        names
    }

    pub(crate) fn emit_lambda(&mut self, l: &juxc_ast::LambdaExpr) {
        // `move` is unconditional: Phase-1 lambdas wrap in
        // `Rc<dyn Fn>`, which often outlives the enclosing scope
        // (e.g. a function that returns a closure capturing its
        // parameters). Capturing by value via `move` keeps the
        // emission valid in both the local-binding and the
        // escaping-closure cases. The cost is one extra clone per
        // captured value, which Rust optimizes away when the
        // capture is a single use.
        //
        // **Wrapper-class captures share, not steal.** A captured
        // wrapper variable gets a shadowing `let c = c.clone();`
        // (cheap `Rc` refcount bump) in a block around the closure, so
        // `move` grabs the CLONE and the caller's binding stays live —
        // both handles point at the same `RefCell`, matching Java's
        // capture-the-reference semantics.
        // A `return` inside the lambda belongs to the LAMBDA, not any
        // enclosing try-closure — clear the threading flag for the body.
        let prev_try = std::mem::take(&mut self.in_try_closure);
        let captures = self.collect_wrapper_captures(l);
        if !captures.is_empty() {
            self.w.push_str("{ ");
            for name in &captures {
                self.w.push_str("let ");
                self.w.push_str(name);
                self.w.push_str(" = ");
                self.w.push_str(name);
                self.w.push_str(".clone(); ");
            }
        }
        self.w.push_str("std::rc::Rc::new(move ");
        self.w.push('|');
        for (i, p) in l.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&p.name.text);
            if let Some(t) = &p.ty {
                self.w.push_str(": ");
                self.emit_type_as_rust(t);
            }
        }
        self.w.push_str("| ");
        // Take-and-clear the void-target marker (§TS.3): an expression
        // body under a `() -> void` slot discards its value.
        let void_target = std::mem::take(&mut self.lambda_void_target);
        match &l.body {
            juxc_ast::LambdaBody::Expr(e) => {
                if void_target {
                    self.w.push_str("{ ");
                    self.emit_expr(e);
                    self.w.push_str("; }");
                } else {
                    self.emit_expr(e);
                }
            }
            juxc_ast::LambdaBody::Block(b) => {
                // Mark the body as lambda territory (S9): a `try`
                // with returns in here can't type its return channel
                // from `current_return_type` (that's the enclosing
                // FUNCTION's), so the try machinery switches to
                // inference-typed channels.
                let prev_lam = self.in_lambda_body;
                self.in_lambda_body = true;
                self.w.push_str("{\n");
                self.w.indent_inc();
                for stmt in &b.statements {
                    self.emit_stmt(stmt);
                }
                self.patch_lambda_tail_try(b);
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push('}');
                self.in_lambda_body = prev_lam;
            }
        }
        self.w.push(')');
        if !self.collect_wrapper_captures(l).is_empty() {
            self.w.push_str(" }");
        }
        self.in_try_closure = prev_try;
    }

    /// Emit `e` inside a parent context with the given precedence,
    /// wrapping in `( … )` only when grouping would otherwise be lost.
    ///
    /// `right_of_left_assoc` indicates that `e` sits on the right side
    /// of a left-associative parent operator — in that case an
    /// equal-precedence child also needs parens.
    pub(crate) fn emit_expr_with_parent_prec(
        &mut self,
        e: &Expr,
        parent_prec: u8,
        right_of_left_assoc: bool,
    ) {
        let needs_paren = match e {
            Expr::Binary(b) => {
                let p = binary_prec(b.op);
                if right_of_left_assoc {
                    p <= parent_prec
                } else {
                    p < parent_prec
                }
            }
            // Unary expressions sit at level 18, tighter than every
            // binary we model — so they never need wrapping under a
            // binary parent. (Inside another unary, multiple prefix
            // operators chain naturally as `--x` without extra parens.)
            Expr::Unary(_) => false,
            // Atomic and postfix expressions never need parens — they
            // bind tighter than any binary operator.
            _ => false,
        };
        if needs_paren {
            self.w.push('(');
        }
        self.emit_expr(e);
        if needs_paren {
            self.w.push(')');
        }
    }
}

impl RustEmitter {
    /// Emit `new Iface() { method overrides }` as a Rust block
    /// expression containing a fresh synthetic struct + `impl Trait
    /// for Struct` carrying the user's bodies, evaluating to an
    /// instance of the synthetic struct. Each call site mints its
    /// own struct (via [`Self::anonymous_class_counter`]), so two
    /// `new Iface() { … }` expressions never collide.
    ///
    /// Shape emitted:
    ///
    /// ```text
    /// {
    ///     #[derive(Clone)]
    ///     struct __JuxAnonN;
    ///     impl <Iface> for __JuxAnonN {
    ///         fn method(&self, …) -> R { /* user body */ }
    ///         …
    ///     }
    ///     __JuxAnonN
    /// }
    /// ```
    ///
    /// **Limitations** (spec §1379): no fields, no constructor,
    /// no static members in the body; no capture of enclosing
    /// `this` or locals. The body is a pure dispatch target.
    pub(crate) fn emit_anonymous_class(&mut self, n: &juxc_ast::NewObjectExpr) {
        let id = self.anonymous_class_counter;
        self.anonymous_class_counter += 1;
        let struct_name = format!("__JuxAnon{id}");
        // Target FQN path emission — same `crate::`-rooting rule
        // `new Foo(...)` uses for cross-package construction.
        let path_segs: Vec<&str> = n
            .class_name
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        let path: String = path_segs.join("::");
        // Resolve the target's kind. Interface → emit `impl Trait for
        // __JuxAnonN`; class (abstract or concrete) → embed the parent
        // and route method calls through Rust's Deref. The bare name
        // resolver consults both the unit-context alias map (for
        // grouped imports) and the FQN-suffix scan.
        let target_bare = n
            .class_name
            .segments
            .last()
            .map(|s| s.text.as_str())
            .unwrap_or("");
        let target_is_interface = self
            .lookup_interface_by_bare_or_fqn(target_bare)
            .is_some();
        let target_is_class = self.lookup_class_by_bare_or_fqn(target_bare).is_some();
        let crate_prefix = if n.class_name.segments.len() > 1 { "crate::" } else { "" };
        let body = n.anonymous_body.clone().unwrap_or_else(|| juxc_ast::AnonymousBody {
            init_blocks: Vec::new(),
            methods: Vec::new(),
        });
        // Enclosing locals the body references (the Java capture pattern). Empty
        // for a capture-less anon, so everything below stays byte-identical then.
        let captures = self.collect_anon_captures(&body);
        let capture_names: std::collections::HashSet<String> =
            captures.iter().map(|(name, _)| name.clone()).collect();
        let methods = body.methods;
        let init_blocks = body.init_blocks;

        if !target_is_interface && target_is_class {
            // Abstract-class (or any class) target — synthesize a
            // real subclass shape with `__parent: Target` and
            // route through Deref. The user's overrides land as
            // inherent methods on the synthetic struct; inherited
            // methods stay reachable via `Deref` to the parent.
            self.w.push_str("{ #[derive(Clone)] struct ");
            self.w.push_str(&struct_name);
            self.w.push_str(" { __parent: ");
            self.w.push_str(crate_prefix);
            self.w.push_str(&path);
            self.emit_anon_capture_field_decls(&captures, true);
            self.w.push_str(" } impl std::ops::Deref for ");
            self.w.push_str(&struct_name);
            self.w.push_str(" { type Target = ");
            self.w.push_str(crate_prefix);
            self.w.push_str(&path);
            self.w.push_str("; fn deref(&self) -> &Self::Target { &self.__parent } } ");
            self.w.push_str("impl std::ops::DerefMut for ");
            self.w.push_str(&struct_name);
            self.w.push_str(" { fn deref_mut(&mut self) -> &mut Self::Target { &mut self.__parent } } ");
            self.w.push_str("impl ");
            self.w.push_str(&struct_name);
            self.w.push_str(" {");
            // Captured names rewrite to `self.<field>` inside the method bodies.
            let prev_captured =
                std::mem::replace(&mut self.captured_locals, capture_names.clone());
            // Inherent override methods — `&mut self` so `this.field`
            // writes through the embedded `__parent` borrow mutably.
            for method in &methods {
                self.emit_anonymous_method(method, true);
            }
            self.w.push_str(" } ");
            // Instance-initializer blocks (Java's "double-brace
            // initialization" form) — each wraps in `{ … }` so the
            // statements run sequentially in their own scope and
            // any locals they declare don't leak into the parent
            // expression-block.
            for ib in &init_blocks {
                self.w.push_str(" {");
                self.emit_block_contents(ib);
                self.w.push_str(" }");
            }
            // Restore BEFORE the construction so capture inits name the enclosing
            // locals (`l.clone()`), not the rewritten `self.l`.
            self.captured_locals = prev_captured;
            // Instantiate the synthetic with __parent built via the
            // target class's `new(args)`.
            self.w.push(' ');
            self.w.push_str(&struct_name);
            self.w.push_str(" { __parent: ");
            self.w.push_str(crate_prefix);
            self.w.push_str(&path);
            let ctor_bare = n
                .class_name
                .segments
                .last()
                .map(|s| s.text.clone())
                .unwrap_or_default();
            let ctor_sfx =
                self.ctor_overload_suffix_for_span(&ctor_bare, n.args.len(), n.span);
            self.w.push_str("::new");
            self.w.push_str(&ctor_sfx);
            self.w.push('(');
            let args = n.args.clone();
            let prev_fmt = self.emitting_format_arg;
            self.emitting_format_arg = false;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.emit_expr(arg);
            }
            self.emitting_format_arg = prev_fmt;
            self.w.push_str(")");
            self.emit_anon_capture_field_inits(&captures, true);
            self.w.push_str(" } }");
            return;
        }
        // Default path — interface target. Empty `impl Trait for
        // __JuxAnonN { ... }` block carrying the user's overrides.
        // `Debug` join `Clone`: the interface trait carries a
        // `std::fmt::Debug` supertrait (Stage-1), so the synthetic
        // implementer must derive it too.
        self.w.push_str("{ #[derive(Clone, Debug)] struct ");
        self.w.push_str(&struct_name);
        if captures.is_empty() {
            self.w.push_str("; impl ");
        } else {
            self.w.push_str(" { ");
            self.emit_anon_capture_field_decls(&captures, false);
            self.w.push_str(" } impl ");
        }
        self.w.push_str(crate_prefix);
        self.w.push_str(&path);
        self.w.push_str(" for ");
        self.w.push_str(&struct_name);
        self.w.push_str(" {");
        // Captured names rewrite to `self.<field>` inside the method bodies.
        let prev_captured = std::mem::replace(&mut self.captured_locals, capture_names.clone());
        // Receiver mutability per method follows the implemented trait's own
        // signature: a `@MutSelf` interface method is `&mut self`, so the anon
        // impl MUST match it exactly (E0053). Discovered from the interface
        // sig (e.g. a foreign `@rust` trait's `@MutSelf` methods), not hardcoded.
        let mut_flags: Vec<bool> = {
            let iface = self.lookup_interface_by_bare_or_fqn(target_bare);
            methods
                .iter()
                .map(|m| {
                    iface
                        .and_then(|(_, i)| i.methods.get(&m.name.text))
                        .map(|ms| {
                            ms.annotations
                                .iter()
                                .any(crate::exprs::field::annotation_is_mut_self)
                        })
                        .unwrap_or(false)
                })
                .collect()
        };
        for (method, &mut_self) in methods.iter().zip(mut_flags.iter()) {
            self.emit_anonymous_method(method, mut_self);
        }
        self.w.push_str(" }");
        // Instance-initializer blocks run before returning the
        // synthetic instance. Each is its own scope so locals
        // declared inside don't leak.
        for ib in &init_blocks {
            self.w.push_str(" {");
            self.emit_block_contents(ib);
            self.w.push_str(" }");
        }
        // Restore BEFORE the construction so the capture inits name the enclosing
        // locals (`l.clone()`), not the rewritten `self.l`.
        self.captured_locals = prev_captured;
        // The instance is born as the interface's trait-object value. A FOREIGN
        // (`@rust`) trait taken by value is an OWNED `Box<dyn Trait>` — the Rust
        // convention for "implement this trait and hand it over" (e.g. minifb's
        // `set_input_callback(Box<dyn InputCallback>)`). A Jux-internal interface
        // value stays the shared `Rc<dyn Trait>`. Wrapping here (instead of
        // relying on `iface_coercion_to`, which keys off the symbol table the
        // synthetic struct isn't in) makes the coercion unconditional.
        let iface_is_external = self
            .lookup_interface_by_bare_or_fqn(target_bare)
            .map(|(_, i)| i.is_external)
            .unwrap_or(false);
        self.w
            .push_str(if iface_is_external { " Box::new(" } else { " std::rc::Rc::new(" });
        self.w.push_str(&struct_name);
        if !captures.is_empty() {
            self.w.push_str(" { ");
            self.emit_anon_capture_field_inits(&captures, false);
            self.w.push_str(" }");
        }
        self.w
            .push_str(if iface_is_external { ") as Box<dyn " } else { ") as std::rc::Rc<dyn " });
        self.w.push_str(crate_prefix);
        self.w.push_str(&path);
        self.w.push_str("> }");
    }

    /// The enclosing locals an anonymous-class body CAPTURES — every bare name
    /// read in a method body (minus that method's own params) or an init block
    /// that resolves to a typed local in the enclosing scope. A method-local or
    /// an unknown name isn't in the enclosing `local_types`, so it's left alone.
    /// Returned `(name, type)` in first-seen order; empty for a capture-less anon.
    fn collect_anon_captures(
        &self,
        body: &juxc_ast::AnonymousBody,
    ) -> Vec<(String, juxc_ast::TypeRef)> {
        let mut candidates: Vec<String> = Vec::new();
        for m in &body.methods {
            let params: std::collections::HashSet<String> =
                m.params.iter().map(|p| p.name.text.clone()).collect();
            if let Some(b) = &m.body {
                collect_bare_names_block(b, &mut |n| {
                    if !params.contains(n) {
                        candidates.push(n.to_string());
                    }
                });
            }
        }
        for b in &body.init_blocks {
            collect_bare_names_block(b, &mut |n| candidates.push(n.to_string()));
        }
        let mut out: Vec<(String, juxc_ast::TypeRef)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for n in candidates {
            if n == "this" || n == "self" || !seen.insert(n.clone()) {
                continue;
            }
            if let Some(ty) = self.local_types.iter().rev().find_map(|s| s.get(&n)) {
                if let Some(tref) = crate::analysis::ty_to_type_ref(ty) {
                    out.push((n, tref));
                }
            }
        }
        out
    }

    /// Emit captured-local struct-field declarations (`name: <Type>`). With
    /// `leading_comma`, every field is comma-prefixed (used after a `__parent`
    /// field); otherwise the first field has none (a fields-only struct).
    fn emit_anon_capture_field_decls(
        &mut self,
        captures: &[(String, juxc_ast::TypeRef)],
        leading_comma: bool,
    ) {
        for (i, (name, tref)) in captures.iter().enumerate() {
            if i > 0 || leading_comma {
                self.w.push_str(", ");
            }
            self.w.push_str(name);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(tref);
        }
    }

    /// Emit captured-local struct-literal inits (`name: name.clone()`) — the
    /// clone is the cheap `Rc` refcount bump for a captured class handle, a copy
    /// for value types. Must run with `captured_locals` NOT yet set so the RHS
    /// names the enclosing local, not `self.name`.
    fn emit_anon_capture_field_inits(
        &mut self,
        captures: &[(String, juxc_ast::TypeRef)],
        leading_comma: bool,
    ) {
        for (i, (name, _)) in captures.iter().enumerate() {
            if i > 0 || leading_comma {
                self.w.push_str(", ");
            }
            self.w.push_str(name);
            self.w.push_str(": ");
            self.w.push_str(name);
            self.w.push_str(".clone()");
        }
    }

    /// Emit one method from an anonymous-class body as an
    /// inherent-style `fn name(&self, args) -> R { body }` inline
    /// within the synthetic struct's `impl` block. Shared by
    /// the interface-target path (where the impl block is for the
    /// trait) and the class-target path (where the impl block
    /// targets the synthetic struct itself).
    fn emit_anonymous_method(&mut self, method: &juxc_ast::FnDecl, receiver_mut: bool) {
        // Method bodies own their `return`s — never thread them into an
        // enclosing try-closure (anonymous classes can be instantiated
        // inside a `try` body).
        let __prev_try = std::mem::take(&mut self.in_try_closure);
        // `async T` on a method in an anonymous-class body lowers to
        // `async fn` on the synthetic struct's impl — same shape as
        // the named-class method emitter (`decls/classes.rs`).
        if matches!(method.return_type, juxc_ast::ReturnType::AsyncType(_)) {
            self.w.push_str(" async fn ");
        } else {
            self.w.push_str(" fn ");
        }
        self.w.push_str(&method.name.text);
        // Receiver kind follows the impl target: an interface trait
        // method is `&self` (the Stage-1 dispatch flip — the impl must
        // match the trait signature exactly, rustc E0053); a
        // class-target inherent override keeps `&mut self` so
        // `this.field` writes through the embedded `__parent`.
        self.w
            .push_str(if receiver_mut { "(&mut self" } else { "(&self" });
        for param in &method.params {
            self.w.push_str(", ");
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&param.ty);
        }
        self.w.push(')');
        match &method.return_type {
            juxc_ast::ReturnType::Void => {}
            juxc_ast::ReturnType::Type(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
            juxc_ast::ReturnType::AsyncType(t) => {
                // `async fn name(...) -> T` — async sat ahead of `fn`.
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
        }
        self.w.push_str(" {");
        if let Some(body) = &method.body {
            let prev_alias = self.this_alias.take();
            self.this_alias = Some("self".to_string());
            let saved_return = self.current_return_type.take();
            self.current_return_type = Some(method.return_type.clone());
            // This method's params shadow any captured enclosing local of the
            // same name (the capture rewrite checks `current_fn_params`).
            let saved_params = std::mem::replace(
                &mut self.current_fn_params,
                method.params.iter().map(|p| p.name.text.clone()).collect(),
            );
            // Register nullable params so `${param}` in the body renders `null`
            // for an absent value through the universal `__jux_show!` path,
            // instead of Debug's `Some(..)`/`None`. Anon-method bodies aren't
            // typed into `expr_types`, so `emit_format_arg` recovers nullability
            // from this set. Value rendering itself is type-agnostic now
            // (Display-or-Debug), so no foreign-type bookkeeping is needed.
            let mut undo_nullable: Vec<String> = Vec::new();
            for p in &method.params {
                if p.ty.nullable && self.nullable_locals.insert(p.name.text.clone()) {
                    undo_nullable.push(p.name.text.clone());
                }
            }
            self.emit_fn_body_at(body, &method.return_type);
            for n in undo_nullable {
                self.nullable_locals.remove(&n);
            }
            self.current_fn_params = saved_params;
            self.current_return_type = saved_return;
            self.this_alias = prev_alias;
        }
        self.w.push('}');
        self.in_try_closure = __prev_try;
    }
}

/// Reach into an expression for its span — companion to tycheck's
/// `check::expr_span`. Lets backend helpers look up an expression's
/// type via `expr_types[expr.span]` without exposing each variant's
/// inner span field at call sites. Synthesized expressions without a
/// real source span return [`juxc_source::Span::DUMMY`], which is the
/// same value the recorder sentinels out — so `expr_types.get(...)`
/// will simply miss and the caller falls back conservatively.
pub(crate) fn expr_span_of(e: &Expr) -> juxc_source::Span {
    match e {
        Expr::Literal(_) => juxc_source::Span::DUMMY,
        Expr::TupleLit(_, s) => *s,
        Expr::TryExpr(t) => t.span,
        Expr::ErrorProp(_, s) => *s,
        Expr::Out(_, s) => *s,
        Expr::TypeOf(_, s) => *s,
        Expr::NotNullAssert(_, s) => *s,
        Expr::Path(qn) => qn.span,
        Expr::Call(c) => c.span,
        Expr::Binary(b) => b.span,
        Expr::Unary(u) => u.span,
        Expr::Range(r) => r.span,
        Expr::Cast(c) => c.span,
        Expr::TypeTest(t) => t.span,
        Expr::SizeOf(s) => s.span,
        Expr::NewArray(n) => n.span,
        Expr::NewArrayLit(n) => n.span,
        Expr::Index(i) => i.span,
        Expr::Field(f) => f.span,
        Expr::InterpString(s) => s.span,
        Expr::This(s) => *s,
        Expr::Super(s) => *s,
        Expr::NewObject(n) => n.span,
        Expr::Switch(s) => s.span,
        Expr::Lambda(l) => l.span,
        Expr::Elvis(e) => e.span,
        Expr::MethodRef(m) => m.span,
        Expr::Ternary(t) => t.span,
        Expr::Await(_, s) => *s,
        Expr::IncDec(i) => i.span,
    }
}

/// Reduce every dotted type name inside a Jux type spelling to its
/// bare last segment — `tof.Point` → `Point`,
/// `rust.std.Vec<a.b.C>` → `Vec<C>` — for `typeof`'s user-facing
/// String (§5.9.10). Operates on the `Ty` Display output, so the
/// dotted runs are always `ident ('.' ident)+` sequences.
pub(crate) fn bare_type_spelling(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut segment = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_alphanumeric() || c == '_' {
            segment.push(c);
        } else if c == '.' && !segment.is_empty()
            && chars.peek().is_some_and(|n| n.is_alphabetic() || *n == '_')
        {
            // A dotted continuation — drop the prefix segment.
            segment.clear();
        } else {
            out.push_str(&segment);
            segment.clear();
            out.push(c);
        }
    }
    out.push_str(&segment);
    out
}

/// Cheap "what kind of Ty would this TypeRef lower to?" — primitives,
/// String, arrays, and bare class/generic names. Used by
/// [`RustEmitter::lookup_field_type`] to classify a field's declared
/// `TypeRef` without round-tripping through tycheck's full
/// `ty_from_ref` (which needs a `TypeEnv` we don't have at emission
/// time). The `generic_params` set carries the names declared on the
/// enclosing class/record so a single-segment name matching a param
/// resolves to [`Ty::Param`]. Anything more nuanced (qualified paths,
/// generic instantiations) returns [`Ty::Unknown`].
pub(crate) fn ty_kind_from_ref_with_params(
    t: &juxc_ast::TypeRef,
    generic_params: &std::collections::HashSet<&str>,
) -> Ty {
    use juxc_tycheck::{ArrayKind, Primitive};
    if let Some(shape) = &t.array_shape {
        // Peel ONE (outer) dimension; the element type carries the
        // remaining dimensions (or none, for a 1-D array's scalar elem),
        // so a multi-dim shape nests as `Array { element: Array { … } }`.
        let element_ref = juxc_ast::TypeRef {
            name: t.name.clone(),
            generic_args: t.generic_args.clone(),
            nullable: t.nullable,
            array_shape: shape.peeled(),
            fn_shape: t.fn_shape.clone(),
            ptr_depth: 0,
            span: t.span,
        };
        let element = ty_kind_from_ref_with_params(&element_ref, generic_params);
        let kind = match shape.outer() {
            juxc_ast::ArrayDim::Fixed(_) => ArrayKind::Fixed,
            juxc_ast::ArrayDim::Dynamic => ArrayKind::Dynamic,
        };
        return Ty::Array {
            element: Box::new(element),
            kind,
        };
    }
    // Phase-1 `ArrayList<T>` ≡ `T[]` shortcut, mirroring tycheck's
    // `ty_from_ref` (ty.rs): the stdlib dispatch and clone decisions
    // treat both spellings as the same `Vec<T>`.
    if t.name.segments.len() == 1
        && t.name.segments[0].text == "ArrayList"
        && t.generic_args.len() == 1
    {
        if let juxc_ast::GenericArg::Type(inner) = &t.generic_args[0] {
            return Ty::Array {
                element: Box::new(ty_kind_from_ref_with_params(inner, generic_params)),
                kind: ArrayKind::Dynamic,
            };
        }
    }
    if t.name.segments.len() != 1 || !t.generic_args.is_empty() {
        return Ty::Unknown;
    }
    let name = t.name.segments[0].text.as_str();
    let prim = match name {
        "bool" => Some(Primitive::Bool),
        "byte" => Some(Primitive::Byte),
        "ubyte" => Some(Primitive::Ubyte),
        "short" => Some(Primitive::Short),
        "ushort" => Some(Primitive::Ushort),
        "int" => Some(Primitive::Int),
        "uint" => Some(Primitive::Uint),
        "long" => Some(Primitive::Long),
        "ulong" => Some(Primitive::Ulong),
        "float" => Some(Primitive::Float),
        "double" => Some(Primitive::Double),
        "char" => Some(Primitive::Char),
        "i8" => Some(Primitive::I8),
        "u8" => Some(Primitive::U8),
        "i16" => Some(Primitive::I16),
        "u16" => Some(Primitive::U16),
        "i32" => Some(Primitive::I32),
        "u32" => Some(Primitive::U32),
        "i64" => Some(Primitive::I64),
        "u64" => Some(Primitive::U64),
        "f32" => Some(Primitive::F32),
        "f64" => Some(Primitive::F64),
        _ => None,
    };
    if let Some(p) = prim {
        return Ty::Primitive(p);
    }
    if name == "String" {
        return Ty::String;
    }
    // Generic-params-aware: a single-segment name that matches a type
    // parameter of the enclosing class/record resolves to `Ty::Param`.
    // Other identifiers — typically class names — land as `Ty::User`.
    if generic_params.contains(name) {
        Ty::Param(name.to_string())
    } else {
        Ty::User {
            name: name.to_string(),
            generic_args: Vec::new(),
        }
    }
}

/// The exact Rust spelling of a tycheck [`juxc_tycheck::Primitive`].
/// Width-faithful: casts through this name preserve each type's
/// overflow/wrap behavior (a `byte` wraps at 8 bits, not pointer width).
pub(crate) fn rust_primitive_name(p: juxc_tycheck::Primitive) -> &'static str {
    use juxc_tycheck::Primitive as P;
    match p {
        P::Int => "isize",
        P::Uint => "usize",
        P::Byte | P::I8 => "i8",
        P::Ubyte | P::U8 => "u8",
        P::Short | P::I16 => "i16",
        P::Ushort | P::U16 => "u16",
        P::Long | P::I64 => "i64",
        P::Ulong | P::U64 => "u64",
        P::I32 => "i32",
        P::U32 => "u32",
        P::Float | P::F32 => "f32",
        P::Double | P::F64 => "f64",
        P::Bool => "bool",
        P::Char => "char",
    }
}

/// Precedence value for a binary operator. Higher = binds tighter.
///
/// **Values match Rust's relative ordering**, not Jux's. The Jux source
/// grammar (§A.4) follows Java/Python precedence — bitwise `& | ^` is
/// **looser** than equality, the opposite of Rust. The parser builds the
/// AST according to Jux's rules. When emitting Rust, we use this table
/// (Rust ordering) so the paren-on-precedence-mismatch logic adds parens
/// wherever necessary to preserve the Jux tree shape under Rust's parser.
///
/// | Level | Operators                                            |
/// |-------|------------------------------------------------------|
/// | 4     | `\|\|` (logical OR)                                  |
/// | 5     | `&&` (logical AND)                                   |
/// | 6     | `==`, `!=`                                            |
/// | 7     | `<`, `<=`, `>`, `>=`                                  |
/// | 8     | `\|` (bitwise OR)                                    |
/// | 9     | `^` (bitwise XOR)                                    |
/// | 10    | `&` (bitwise AND)                                    |
/// | 11    | `<<`, `>>` (shifts)                                   |
/// | 12    | `+`, `-`                                              |
/// | 13    | `*`, `/`, `%`                                         |
pub(crate) fn binary_prec(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Or     => 4,
        BinaryOp::And    => 5,
        // Reference identity (`===`/`!==`) shares the equality level.
        BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::RefEq | BinaryOp::RefNeq => 6,
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge | BinaryOp::In => 7,
        // `<=>` lowers structurally (method call / partial_cmp), so
        // its precedence slot only matters for paren decisions when
        // nested — treat like comparisons, one notch tighter.
        BinaryOp::Cmp => 7,
        BinaryOp::BitOr  => 8,
        BinaryOp::BitXor => 9,
        BinaryOp::BitAnd => 10,
        BinaryOp::Shl | BinaryOp::Shr => 11,
        BinaryOp::Add | BinaryOp::Sub => 12,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 13,
        // The wrapping family (§S.2.1) lowers structurally to
        // `wrapping_add` & co (method calls — postfix, max-tight), so
        // these values only matter when a wrap result nests inside
        // another operator; mirror their plain counterparts.
        BinaryOp::WrapShl | BinaryOp::WrapShr => 11,
        BinaryOp::WrapAdd | BinaryOp::WrapSub => 12,
        BinaryOp::WrapMul => 13,
    }
}


/// Walk a lambda's body and report every **single-segment bare name**
/// read anywhere inside it — the superset of the closure's captures
/// (locals declared inside the body and the lambda's own params are
/// filtered by the caller, `RustEmitter::collect_wrapper_captures`).
/// Field accesses (`x.f`) report the root `x`; multi-segment paths
/// (`pkg.Class`) are type names, not captures, and are skipped.
pub(crate) fn collect_bare_names_in_lambda(
    l: &juxc_ast::LambdaExpr,
    sink: &mut dyn FnMut(&str),
) {
    match &l.body {
        juxc_ast::LambdaBody::Expr(e) => collect_bare_names_expr(e, sink),
        juxc_ast::LambdaBody::Block(b) => collect_bare_names_block(b, sink),
    }
}

fn collect_bare_names_expr(e: &Expr, sink: &mut dyn FnMut(&str)) {
    match e {
        Expr::Path(qn) => {
            if qn.segments.len() == 1 {
                sink(&qn.segments[0].text);
            }
        }
        Expr::Call(c) => {
            collect_bare_names_expr(&c.callee, sink);
            for a in &c.args {
                collect_bare_names_expr(a, sink);
            }
        }
        Expr::NewObject(n) => {
            for a in &n.args {
                collect_bare_names_expr(a, sink);
            }
        }
        Expr::NewArrayLit(n) => {
            for el in &n.elements {
                collect_bare_names_expr(el, sink);
            }
        }
        Expr::NewArray(n) => {
            collect_bare_names_expr(&n.size, sink);
            for inner in &n.inner_sizes {
                collect_bare_names_expr(inner, sink);
            }
        }
        Expr::Binary(b) => {
            collect_bare_names_expr(&b.left, sink);
            collect_bare_names_expr(&b.right, sink);
        }
        Expr::Unary(u) => collect_bare_names_expr(&u.operand, sink),
        Expr::Range(r) => {
            collect_bare_names_expr(&r.start, sink);
            collect_bare_names_expr(&r.end, sink);
        }
        Expr::Cast(c) => collect_bare_names_expr(&c.value, sink),
        Expr::TypeTest(t) => collect_bare_names_expr(&t.value, sink),
        Expr::Index(i) => {
            collect_bare_names_expr(&i.array, sink);
            collect_bare_names_expr(&i.index, sink);
        }
        Expr::Field(f) => collect_bare_names_expr(&f.object, sink),
        Expr::InterpString(s) => {
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Expr(inner) = seg {
                    collect_bare_names_expr(inner, sink);
                }
            }
        }
        Expr::Elvis(el) => {
            collect_bare_names_expr(&el.value, sink);
            collect_bare_names_expr(&el.fallback, sink);
        }
        Expr::Ternary(t) => {
            collect_bare_names_expr(&t.condition, sink);
            collect_bare_names_expr(&t.then_branch, sink);
            collect_bare_names_expr(&t.else_branch, sink);
        }
        Expr::Await(inner, _) => collect_bare_names_expr(inner, sink),
        Expr::Lambda(inner) => collect_bare_names_in_lambda(inner, sink),
        _ => {}
    }
}

pub(crate) fn collect_bare_names_block(b: &juxc_ast::Block, sink: &mut dyn FnMut(&str)) {
    use juxc_ast::Stmt;
    for s in &b.statements {
        match s {
            Stmt::Expr(e) => collect_bare_names_expr(e, sink),
            Stmt::Return(Some(e), _) => collect_bare_names_expr(e, sink),
            Stmt::Return(None, _) => {}
            Stmt::VarDecl(v) => {
                if let Some(init) = &v.init {
                    collect_bare_names_expr(init, sink);
                }
            }
            Stmt::Assign(a) => {
                collect_bare_names_expr(&a.target, sink);
                collect_bare_names_expr(&a.value, sink);
            }
            Stmt::Throw(e, _) => collect_bare_names_expr(e, sink),
            Stmt::SuperCall(args, _) => {
                for a in args {
                    collect_bare_names_expr(a, sink);
                }
            }
            Stmt::If(i) => {
                collect_bare_names_expr(&i.condition, sink);
                collect_bare_names_block(&i.then_block, sink);
                let mut cursor = i.else_branch.as_deref();
                while let Some(branch) = cursor {
                    match branch {
                        juxc_ast::ElseBranch::If(inner) => {
                            collect_bare_names_expr(&inner.condition, sink);
                            collect_bare_names_block(&inner.then_block, sink);
                            cursor = inner.else_branch.as_deref();
                        }
                        juxc_ast::ElseBranch::Block(blk) => {
                            collect_bare_names_block(blk, sink);
                            cursor = None;
                        }
                    }
                }
            }
            Stmt::While(w) => {
                collect_bare_names_expr(&w.condition, sink);
                collect_bare_names_block(&w.body, sink);
            }
            Stmt::DoWhile(d) => {
                collect_bare_names_block(&d.body, sink);
                collect_bare_names_expr(&d.condition, sink);
            }
            Stmt::ForEach(f) => {
                collect_bare_names_expr(&f.iter, sink);
                collect_bare_names_block(&f.body, sink);
            }
            Stmt::ForC(f) => {
                if let Some(cond) = &f.cond {
                    collect_bare_names_expr(cond, sink);
                }
                collect_bare_names_block(&f.body, sink);
            }
            Stmt::Try(t) => {
                collect_bare_names_block(&t.body, sink);
                for c in &t.catches {
                    collect_bare_names_block(&c.body, sink);
                }
                if let Some(fin) = &t.finally {
                    collect_bare_names_block(fin, sink);
                }
            }
            Stmt::Unsafe(b) => collect_bare_names_block(b, sink),
            Stmt::Break(..) | Stmt::Continue(..) => {}
            Stmt::Labeled { stmt, .. } => {
                collect_bare_names_block(
                    &juxc_ast::Block {
                        statements: vec![(**stmt).clone()],
                        span: juxc_source::Span::DUMMY,
                    },
                    sink,
                );
            }
        }
    }
}
