//! Call-expression emission — generic function/method calls plus the
//! built-in `print(...)` special case. Both paths share the enum-
//! variant String-payload coercion that injects `.to_string()` on
//! positional args matching a `String` slot.

use juxc_ast::{BinaryExpr, CallExpr, Expr, Literal};

use crate::analysis::is_string_literal;
use crate::exprs::ArgRef;
use crate::RustEmitter;

/// How a single foreign-call argument crosses the C boundary (§L.7).
#[derive(Clone, Copy)]
enum FfiArg {
    /// A numeric/pointer value passed verbatim.
    Plain,
    /// A Jux `String` marshalled through a `CString` temp to a `const char*`.
    Str,
    /// A Jux `char` (4-byte) truncated to a C `char` (`core::ffi::c_char`).
    Char,
    /// An `out <place>` argument (§M.4): the C callee writes through it, so we
    /// pass `addr_of_mut!(place)` (a `*mut T`) instead of the value.
    Out,
}

/// How a foreign-call RETURN crosses the C boundary (§L.7).
#[derive(Clone, Copy)]
enum FfiRet {
    /// A numeric/pointer value used verbatim.
    Plain,
    /// A C `const char*` copied out into an owned Jux `String`. `nullable` maps
    /// a null pointer to Jux `null` (`String?`) rather than the empty string.
    Str { nullable: bool },
    /// A C `char` widened back to a Jux `char`.
    Char,
}

/// Classify a foreign parameter type into its [`FfiArg`] marshalling kind.
fn ffi_arg_kind(t: &juxc_ast::TypeRef) -> FfiArg {
    if type_ref_is_string(t) {
        FfiArg::Str
    } else if type_ref_is_char(t) {
        FfiArg::Char
    } else {
        FfiArg::Plain
    }
}

/// True when `t` is the Jux `String` type (or `String?`) at the value level: a
/// single-segment `String` name with no pointer / array / generic shape. Unlike
/// `analysis::is_jux_string_type` this accepts the nullable form, so FFI
/// marshalling recognizes a `String?` return (null → `None`). Used to decide
/// which foreign args/returns cross the boundary as C `const char*`.
fn type_ref_is_string(t: &juxc_ast::TypeRef) -> bool {
    t.ptr_depth == 0
        && t.array_shape.is_none()
        && t.fn_shape.is_none()
        && t.generic_args.is_empty()
        && t.name.segments.len() == 1
        && t.name.segments[0].text == "String"
}

/// True when `arg` is a string-valued literal — a plain `"…"` or an interpolated
/// `$"…"`. Used to marshal a TRAILING argument of a C-variadic call (`printf`)
/// into a `const char*`, since those args have no declared parameter type to key
/// the marshalling off (§L.4.2). A non-literal `String` in a variadic slot is not
/// recognized here (the backend has no inference) and would reach `rustc` as a
/// non-FFI-safe `String` — pass a literal or cast to a `String*`/`char*`.
fn expr_is_string_literal(arg: &juxc_ast::Expr) -> bool {
    matches!(
        arg,
        juxc_ast::Expr::Literal(juxc_ast::Literal::String(_)) | juxc_ast::Expr::InterpString(_)
    )
}

/// True when `t` is a plain Jux `char` (no pointer / array / generic / nullable
/// shape) — the value-level `char` that maps to a C `char` at the FFI boundary.
fn type_ref_is_char(t: &juxc_ast::TypeRef) -> bool {
    t.ptr_depth == 0
        && t.array_shape.is_none()
        && t.fn_shape.is_none()
        && t.generic_args.is_empty()
        && !t.nullable
        && t.name.segments.len() == 1
        && t.name.segments[0].text == "char"
}

/// Mirror of `binary::collect_string_concat_operands` for the
/// `print(...)`-collapse hot path. Kept here to avoid exposing the
/// binary-module helper across modules.
/// A synthesized NULLABLE return-type ref for `Stream.generate` body
/// emission (§18.6.4): only the `nullable` flag matters — the return
/// lowering reads it to apply the `Some(...)` lift, and the dummy name
/// resolves to nothing, so no interface/upcast coercion fires.
fn synth_nullable_type_ref() -> juxc_ast::TypeRef {
    let mut t = crate::analysis::synth_iface_type_ref(
        "__jux_stream_elem",
        juxc_source::Span::DUMMY,
    );
    t.nullable = true;
    t
}

/// The dotted place-path of a pure place expression: `x` → `"x"`,
/// `this` → `"this"`, `h.item` → `"h.item"`, `this.a.b` → `"this.a.b"`.
/// `None` for anything that isn't a simple chain of field reads rooted
/// at a single-segment path or `this` (call results, indexes,
/// multi-segment qualified names) — those aren't borrowable places the
/// hoist machinery can reason about (S7).
fn place_path_of(e: &Expr) -> Option<String> {
    match e {
        Expr::This(_) => Some("this".to_string()),
        Expr::Path(qn) if qn.segments.len() == 1 => Some(qn.segments[0].text.clone()),
        Expr::Field(f) => {
            let base = place_path_of(&f.object)?;
            Some(format!("{base}.{}", f.field.text))
        }
        _ => None,
    }
}

fn flatten_concat<'a>(b: &'a BinaryExpr, out: &mut Vec<&'a Expr>) {
    push_concat_operand(&b.left, out);
    push_concat_operand(&b.right, out);
}

fn push_concat_operand<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary(inner) = e {
        if inner.op == juxc_ast::BinaryOp::Add
            && (is_string_literal(&inner.left) || is_string_literal(&inner.right))
        {
            flatten_concat(inner, out);
            return;
        }
    }
    out.push(e);
}

/// True iff the print path should treat `e` as `String`-typed.
/// Mirrors `binary.rs::operand_is_string_typed` — kept module-
/// local rather than sharing the helper to avoid cross-module
/// privacy churn. Both paths use the same `expr_types` lookup so
/// the trigger fires consistently.
impl super::super::RustEmitter {
    fn operand_is_string_typed_for_print(&self, e: &Expr) -> bool {
        let recorded = self.expr_types.get(&crate::exprs::expr_span_of(e));
        // Mirror `binary::operand_is_string_typed`'s smart-cast
        // unwrap: when `e` is a path that the smart-cast pass
        // has removed from `nullable_locals`, peel a recorded
        // `Ty::Nullable` so the inner `String` matches the
        // type-driven concat trigger.
        let effective = if let (Expr::Path(qn), Some(juxc_tycheck::Ty::Nullable(inner))) =
            (e, recorded)
        {
            if qn.segments.len() == 1
                && !self.nullable_locals.contains(&qn.segments[0].text)
            {
                Some(inner.as_ref())
            } else {
                recorded
            }
        } else {
            recorded
        };
        matches!(effective, Some(juxc_tycheck::Ty::String))
    }
}

/// Print-path mirror of `binary::fold_concat_into_format`. Folds
/// `Literal::String` operands directly into the `println!` template
/// (re-escaped + brace-doubled); non-literal operands become runtime
/// args with a single `{}` placeholder each.
fn fold_concat_for_print<'a>(operands: &[&'a Expr]) -> (String, Vec<&'a Expr>) {
    let mut template = String::new();
    let mut runtime: Vec<&'a Expr> = Vec::new();
    for op in operands {
        if let Expr::Literal(Literal::String(s)) = op {
            for ch in s.chars() {
                match ch {
                    '{' => template.push_str("{{"),
                    '}' => template.push_str("}}"),
                    '"' => template.push_str("\\\""),
                    '\\' => template.push_str("\\\\"),
                    '\n' => template.push_str("\\n"),
                    '\r' => template.push_str("\\r"),
                    '\t' => template.push_str("\\t"),
                    c => template.push(c),
                }
            }
        } else {
            template.push_str("{}");
            runtime.push(op);
        }
    }
    (template, runtime)
}

impl RustEmitter {
    /// True when `call` targets a foreign (`.jux.d`) function or static method
    /// whose `throws E` clause maps a Rust `Result<T, E>` return (§G.5.4) — the
    /// `is_foreign_result` flag on its signature. Such a call must have its
    /// `Result` unwrapped at the use site (see the `Expr::Call` arm of
    /// `emit_expr`). Covers bare free-function calls and `ClassName.method(...)`
    /// static calls; instance-method foreign-result calls are a later refinement.
    pub(crate) fn call_is_foreign_result(&self, call: &CallExpr) -> bool {
        match &*call.callee {
            // Free function `f(args)` — exact key, else last-segment match for an
            // imported foreign fn keyed by its full `rust.<crate>.<fn>` path.
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let bare = qn.segments[0].text.as_str();
                if let Some((_, sig)) = self.symbols.lookup_function(bare) {
                    return sig.is_foreign_result;
                }
                // Fallback: match by last segment. A USER (non-foreign) function
                // SHADOWS an auto-loaded foreign one of the same bare name — an
                // unqualified `rename(...)` must bind to the user's `rename`, not
                // `std::fs::rename` (which would spuriously unwrap a `Result`).
                // So prefer a non-`rust.`/`c.`/`cpp.` key; fall back to a foreign
                // match only when no user function shares the name.
                let is_foreign_key =
                    |k: &str| k.starts_with("rust.") || k.starts_with("c.") || k.starts_with("cpp.");
                let mut foreign_hit: Option<bool> = None;
                for (k, s) in &self.symbols.functions {
                    if k.rsplit('.').next() == Some(bare) {
                        if is_foreign_key(k) {
                            foreign_hit = Some(s.is_foreign_result);
                        } else {
                            return s.is_foreign_result; // user fn wins
                        }
                    }
                }
                foreign_hit.unwrap_or(false)
            }
            // Method call `recv.method(args)` — two shapes:
            //  - Static `ClassName.method(...)`: the receiver is a type name.
            //  - Instance `value.method(...)`: resolve the receiver's inferred
            //    type to its class, then look up the method.
            Expr::Field(f) => {
                let method = f.field.text.as_str();
                // Static: receiver is a bare/qualified class name.
                if let Expr::Path(qn) = &*f.object {
                    if let Some(last) = qn.segments.last() {
                        if let Some(fqn) = self.symbols.find_fqn_by_bare(&last.text) {
                            if let Some(cls) = self.symbols.classes.get(&fqn) {
                                if let Some(m) = cls.methods.get(method) {
                                    return m.is_foreign_result;
                                }
                            }
                        }
                    }
                }
                // Instance: resolve the receiver's type → class → method.
                if let Some(juxc_tycheck::Ty::User { name, .. }) =
                    self.receiver_ty_for_call(&f.object)
                {
                    if let Some(cls) = self.symbols.classes.get(&name) {
                        if let Some(m) = cls.methods.get(method) {
                            return m.is_foreign_result;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Best-effort inferred type of a method-call receiver — the same
    /// `local_types` → `expr_types` → string-literal resolution
    /// [`Self::try_emit_stdlib_method`] uses, factored out for the
    /// foreign-result check. Returns `None` when the receiver wasn't typed.
    fn receiver_ty_for_call(&self, recv: &Expr) -> Option<juxc_tycheck::Ty> {
        if let Expr::Path(qn) = recv {
            if qn.segments.len() == 1 {
                let bare = qn.segments[0].text.as_str();
                if let Some(ty) = self
                    .local_types
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(bare).cloned())
                {
                    return Some(ty);
                }
            }
        }
        self.expr_types
            .get(&crate::exprs::expr_span_of(recv))
            .cloned()
    }

    /// Emit a call expression. Special-cases the built-in `print` to
    /// `println!(…)`. Every other callee is emitted verbatim (the
    /// resolver guarantees the name exists).
    pub(crate) fn emit_call(&mut self, call: &CallExpr) {
        // The method-RECEIVER marker (S7) applies only to a direct
        // field-place receiver — a nested CALL inside the receiver
        // expression (`getH().item.set(x)` evaluating `getH()`) is a
        // fresh evaluation whose own fields/args must clone normally.
        // Take-and-clear so it can't leak into this call's emission.
        let _ = std::mem::take(&mut self.emitting_method_receiver);
        // Method-overload pick (§T.3 Phase-1): tycheck recorded which
        // group member this call resolved to; member K > 0 emits
        // under `name__ovK`. Armed here, consumed by the single path
        // that writes the member name. Cleared first so a stale value
        // from an aborted emission can't leak in.
        self.pending_method_suffix = None;
        if let Some(k) = self.symbols.method_selections.get(&call.span) {
            if *k > 0 {
                self.pending_method_suffix = Some(format!("__ov{k}"));
            }
        }
        // C FFI call-site marshalling (Layout-ABI §L.7): a call to a foreign
        // function declared in an `unsafe native` block marshals `String`
        // arguments/returns to/from C `const char*` and `char` arguments/returns
        // to/from C `char` (`core::ffi::c_char`). Only intercept when marshalling
        // is actually needed; a numeric/pointer-only foreign call flows through
        // the generic path unchanged (it already lowers to a bare `name(args)`).
        if let Some((args, ret)) = self.extern_c_call_shape(&call.callee) {
            if !matches!(ret, FfiRet::Plain) || args.iter().any(|a| !matches!(a, FfiArg::Plain)) {
                self.emit_extern_c_call(call, &args, ret);
                return;
            }
        }
        // `super.method(args)` (§6.9.4) — a STATIC call to the nearest
        // concrete ancestor's version of `method`, bypassing virtual dispatch
        // for this one call. We emit `<self>.__jux_super_<method>(args)`, a
        // per-class shim carrying the ancestor's body specialized to this
        // class (emitted by `emit_super_shims`). Other (virtual) calls inside
        // that body still dispatch to the subclass, matching Java.
        if let Expr::Field(f) = &*call.callee {
            if matches!(f.object.as_ref(), Expr::Super(_)) {
                let alias = self.this_alias.as_deref().unwrap_or("self").to_string();
                // Which shim in the chain runs the ancestor body. A normal
                // method body (`super_shim_depth == None`) calls level 0
                // (`__jux_super_<m>`, the nearest ancestor). Inside a level-`d`
                // shim — itself a copied ancestor body — `super.<m>()` must climb
                // ONE more level (`__jux_super_<m>__{d+1}`), so a 3+ level chain
                // walks grandparent→great-grandparent instead of looping.
                let target_level = self.super_shim_depth.map_or(0, |d| d + 1);
                self.w.push_str(&alias);
                self.w.push_str(".__jux_super_");
                self.w.push_str(&f.field.text);
                if target_level > 0 {
                    self.w.push_str(&format!("__{target_level}"));
                }
                self.w.push('(');
                for (i, arg) in call.args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.w.push(')');
                return;
            }
        }
        // `weakField.get()` (§6.5) — promote a weak reference to a strong one.
        // The field stores a `Weak<RefCell<Target_Inner>>`, so `.get()` lowers
        // to `<recv>.0.borrow(){.__parent…}.<field>.upgrade().map(Target)`,
        // yielding `Option<Target>` = Jux `Target?` (null when the target has
        // already been dropped). Intercepted before the generic call paths so
        // the bare weak-field read of the receiver is never emitted on its own.
        if let Expr::Field(getf) = &*call.callee {
            if getf.field.text == "get" && call.args.is_empty() && !getf.safe {
                if let Expr::Field(wf) = getf.object.as_ref() {
                    if let Some(target) =
                        self.wrapper_weak_field_target(&wf.object, &wf.field.text)
                    {
                        let depth = self
                            .wrapper_field_parent_depth(&wf.object, &wf.field.text)
                            .unwrap_or(0);
                        self.emit_expr(&wf.object);
                        self.w.push_str(".0.borrow()");
                        for _ in 0..depth {
                            self.w.push_str(".__parent");
                        }
                        self.w.push('.');
                        self.w.push_str(&wf.field.text);
                        self.w.push_str(".upgrade().map(");
                        self.w.push_str(&target);
                        self.w.push(')');
                        return;
                    }
                }
            }
        }
        // `weakParam.get()` (§M.14.3) — promote a weak PARAMETER. The parameter
        // IS the `Weak<RefCell<Class_Inner>>` handle, so `.get()` lowers to
        // `param.upgrade().map(Class)`, yielding `Option<Class>` = `Class?`.
        if let Expr::Field(getf) = &*call.callee {
            if getf.field.text == "get" && call.args.is_empty() && !getf.safe {
                if let Expr::Path(qn) = getf.object.as_ref() {
                    if qn.segments.len() == 1 {
                        if let Some(cls) = self.weak_params.get(&qn.segments[0].text).cloned() {
                            self.w.push_str(&qn.segments[0].text);
                            self.w.push_str(".upgrade().map(");
                            self.w.push_str(&cls);
                            self.w.push(')');
                            return;
                        }
                    }
                }
            }
        }
        // `operator()` dispatch (§O.2.4): the callee is a VALUE whose
        // type declares the call overload — `adder(5)` routes to
        // `adder.__op_call(5)`. Checked before the named-callee paths
        // so a callable local never shadows into a function lookup.
        if self.expr_declares_operator(&call.callee, juxc_ast::OperatorKind::Call) {
            self.emit_expr_with_parent_prec(&call.callee, u8::MAX, false);
            self.w.push_str(".__op_call(");
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            for (i, arg) in call.args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.emit_expr(arg);
                if self.wrapper_value_needs_clone(arg) {
                    self.w.push_str(".clone()");
                }
            }
            self.emitting_format_arg = prev;
            self.w.push(')');
            return;
        }
        // §P.3: `<prop>.observers.add(o)` / `.remove(o)` (and the legacy
        // `.attach`/`.detach` spellings) — route to the per-property observer
        // storage before any generic method dispatch (the chain would otherwise
        // read like field accesses on the property's VALUE). `add`/`remove` are
        // the canonical surface names; they normalize to the existing
        // attach/detach lowering.
        if let Expr::Field(opf) = &*call.callee {
            let raw_op = opf.field.text.as_str();
            let opname = match raw_op {
                "add" => Some("attach"),
                "remove" => Some("detach"),
                "attach" | "detach" => Some(raw_op),
                _ => None,
            };
            if let Some(opname) = opname {
                if let Expr::Field(obsf) = &*opf.object {
                    if obsf.field.text == "observers" {
                        if let Some((recv, prop, class)) =
                            self.resolve_observable_prop(&obsf.object)
                        {
                            return self.emit_observers_call(
                                recv,
                                &prop,
                                &class,
                                &opname.to_string(),
                                call,
                            );
                        }
                        // P7: `Config.Level.observers.add(o)` — the receiver
                        // names a CLASS, so the instance resolution above misses;
                        // route to the class-scoped static observer helpers.
                        if let Some((class, prop)) =
                            self.resolve_static_observable_prop(&obsf.object)
                        {
                            return self.emit_static_observers_call(
                                &class,
                                &prop,
                                &opname.to_string(),
                                call,
                            );
                        }
                    }
                }
            }
        }
        // §P.4: property binding — `target.X.bind(source.Y)`,
        // `target.X.bindBidirectional(other.Y)`, `target.X.unbind()`.
        // Only fires when the receiver chain actually names an
        // observable property; a user method named `bind` on a
        // non-property receiver falls through to normal dispatch.
        if let Expr::Field(opf) = &*call.callee {
            let opname = opf.field.text.as_str();
            if matches!(opname, "bind" | "bindBidirectional" | "unbind") {
                if let Some((t_recv, t_prop, t_class)) =
                    self.resolve_observable_prop(&opf.object)
                {
                    if opname == "unbind" && call.args.is_empty() {
                        let t_prop = t_prop.clone();
                        return self.emit_unbind(t_recv, &t_prop);
                    }
                    if opname != "unbind" {
                        if let Some(src) = call.args.first() {
                            if let Some((s_recv, s_prop, s_class)) =
                                self.resolve_observable_prop(src)
                            {
                                let bidi = opname == "bindBidirectional";
                                return self.emit_bind(
                                    (t_recv, &t_prop, &t_class),
                                    (s_recv, &s_prop, &s_class),
                                    bidi,
                                );
                            }
                        }
                    }
                }
            }
        }
        // §M.5 record wither: `r.with(field: v, …)` — a copy of the
        // record with the named components replaced. Lowers to Rust's
        // struct-update syntax: `Rec { x: v, ..(recv).clone() }`
        // (zero args → a plain `.clone()`). A user-declared `with`
        // method shadows the synthesized wither and falls through to
        // normal dispatch.
        if let Expr::Field(wf) = &*call.callee {
            if wf.field.text == "with" {
                if let Some(bare) = self.receiver_class_bare(&wf.object) {
                    let rec_sig = self.symbols.records.get(&bare).or_else(|| {
                        let suffix = format!(".{bare}");
                        let mut hits = self
                            .symbols
                            .records
                            .iter()
                            .filter(|(k, _)| k.ends_with(&suffix));
                        match (hits.next(), hits.next()) {
                            (Some((_, r)), None) => Some(r),
                            _ => None,
                        }
                    });
                    if let Some(rec) = rec_sig {
                        if !rec.methods.contains_key("with") {
                            let prev = self.emitting_format_arg;
                            self.emitting_format_arg = false;
                            if call.args.is_empty() {
                                // `v.with()` — an identical copy.
                                self.w.push('(');
                                self.emit_expr(&wf.object);
                                self.w.push_str(").clone()");
                            } else {
                                self.w.push_str(&bare);
                                self.w.push_str(" { ");
                                for (i, arg) in call.args.iter().enumerate() {
                                    if let Some(Some(n)) = call.arg_names.get(i) {
                                        self.w.push_str(&n.text);
                                        self.w.push_str(": ");
                                        self.emit_expr(arg);
                                        self.w.push_str(", ");
                                    }
                                }
                                self.w.push_str("..(");
                                self.emit_expr(&wf.object);
                                self.w.push_str(").clone() }");
                            }
                            self.emitting_format_arg = prev;
                            return;
                        }
                    }
                }
            }
        }
        // Recognize a single-segment path `print` for the built-in.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "print" {
                return self.emit_print_call(call);
            }
        }
        // `assert(cond)` / `assert(cond, msg)` (§S.7.2) → Rust's
        // `debug_assert!`: checked in debug builds, elided in release
        // — exactly the jux-full profile defaults. Under `jux test`
        // (§TS.3) it lowers to the always-checked `assert!` instead,
        // so `jux test --release` can't elide assertions. The message
        // slot goes through the format machinery so interpolated
        // strings and String values both work; the macro evaluates it
        // lazily (only on failure).
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "assert" {
                self.w.push_str(if self.test_mode {
                    "assert!("
                } else {
                    "debug_assert!("
                });
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(cond) = call.args.first() {
                    self.emit_expr(cond);
                }
                if let Some(msg) = call.args.get(1) {
                    self.w.push_str(", \"{}\", ");
                    self.emitting_format_arg = true;
                    self.emit_expr(msg);
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                return;
            }
        }
        // `withTimeout(ms, f)` — §18.1.9: race the work against a
        // timer task; the loser is dropped (cancelling the work on
        // timeout) and a TimeoutException unwinds into the normal
        // catch machinery. Produces a Future — `await` it.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "withTimeout" {
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                self.w.push_str("async {\n");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str(
                    "let __jux_timer = crate::__jux_spawn(async move { std::thread::sleep(std::time::Duration::from_millis((",
                );
                if let Some(ms) = call.args.first() {
                    self.emit_expr(ms);
                } else {
                    self.w.push('0');
                }
                self.w.push_str(") as u64)) });\n");
                self.w.emit_indent();
                self.w.push_str(
                    "match futures::future::select(std::pin::pin!(async move { ",
                );
                match call.args.get(1) {
                    Some(Expr::Lambda(l)) if l.params.is_empty() => match &l.body {
                        juxc_ast::LambdaBody::Expr(e) => self.emit_expr(e),
                        juxc_ast::LambdaBody::Block(b) => {
                            let (stmts, tail) = match b.statements.split_last() {
                                Some((juxc_ast::Stmt::Expr(t), rest)) => (rest, Some(t)),
                                _ => (&b.statements[..], None),
                            };
                            for stmt in stmts {
                                self.emit_stmt(stmt);
                            }
                            if let Some(tail) = tail {
                                self.emit_expr(tail);
                            }
                        }
                    },
                    Some(other) => {
                        self.emit_expr(other);
                        self.w.push_str(".await");
                    }
                    None => {}
                }
                self.w.push_str(" }), __jux_timer).await {\n");
                self.w.indent_inc();
                self.w.line("futures::future::Either::Left((__jux_v, _)) => __jux_v,");
                self.w.line(
                    "futures::future::Either::Right(_) => std::panic::panic_any(crate::jux::std::exceptions::TimeoutException::new(\"operation timed out\".to_string())),",
                );
                self.w.indent_dec();
                self.w.line("}");
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push('}');
                self.emitting_format_arg = prev;
                return;
            }
        }
        // `Task.all / Task.race / Task.delay` (§18.1.4) — statics on
        // the task runtime. `all` joins same-typed tasks into a
        // Task<List<T>>; `race` resolves with the first to settle;
        // `delay(ms)` is a timer task (a pool thread sleeps — fine
        // for Phase 1's pool sizes).
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1 && qn.segments[0].text == "Task" {
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    match f.field.text.as_str() {
                        "all" => {
                            self.w.push_str(
                                "crate::__jux_spawn(async move { futures::future::join_all(vec![",
                            );
                            for (i, arg) in call.args.iter().enumerate() {
                                if i > 0 {
                                    self.w.push_str(", ");
                                }
                                self.emit_expr(arg);
                            }
                            self.w.push_str("]).await })");
                            self.emitting_format_arg = prev;
                            return;
                        }
                        "race" => {
                            self.w.push_str(
                                "crate::__jux_spawn(async move { futures::future::select_all(vec![",
                            );
                            for (i, arg) in call.args.iter().enumerate() {
                                if i > 0 {
                                    self.w.push_str(", ");
                                }
                                self.emit_expr(arg);
                            }
                            self.w.push_str("]).await.0 })");
                            self.emitting_format_arg = prev;
                            return;
                        }
                        "delay" => {
                            self.w.push_str(
                                "crate::__jux_spawn(async move { std::thread::sleep(std::time::Duration::from_millis((",
                            );
                            if let Some(ms) = call.args.first() {
                                self.emit_expr(ms);
                            } else {
                                self.w.push('0');
                            }
                            self.w.push_str(") as u64)) })");
                            self.emitting_format_arg = prev;
                            return;
                        }
                        _ => {
                            self.emitting_format_arg = prev;
                        }
                    }
                }
                // `Stream.of/from/generate` (§18.6.4) — statics on the
                // emitted JuxStream helper.
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Stream"
                    && !self.symbols.classes.contains_key("Stream")
                    && matches!(f.field.text.as_str(), "of" | "from" | "generate")
                {
                    self.emit_stream_static(call, f.field.text.as_str());
                    return;
                }
            }
        }
        // `spawn(f)` — JUX-ASYNC v2 §18.1.3: schedule the zero-arg
        // lambda's body on the task pool, returning a JuxTask<T>
        // immediately. The body inlines into an `async move` block
        // (no closure indirection), so an async lambda's awaits work
        // and a sync body just computes its value.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "spawn" {
                // Clone-rebind shared captures: a lambda capture
                // moves into the task, but the caller usually keeps
                // using the value (channels especially). Re-binding
                // `let x = x.clone();` in a wrapper block hands the
                // task its own handle. Only known non-primitive
                // locals rebind (primitives are Copy; body-local
                // names aren't in scope here).
                let mut rebinds: Vec<String> = Vec::new();
                if let Some(Expr::Lambda(l)) = call.args.first() {
                    let mut names: Vec<String> = Vec::new();
                    crate::exprs::collect_bare_names_in_lambda(l, &mut |n| {
                        if !names.iter().any(|x| x == n) {
                            names.push(n.to_string());
                        }
                    });
                    for name in names {
                        let known = self
                            .local_types
                            .iter()
                            .rev()
                            .find_map(|s| s.get(&name).cloned());
                        if let Some(ty) = known {
                            if !matches!(ty, juxc_tycheck::Ty::Primitive(_)) {
                                rebinds.push(name);
                            }
                        }
                    }
                }
                if rebinds.is_empty() {
                    self.w.push_str("crate::__jux_spawn(async move { ");
                } else {
                    self.w.push_str("crate::__jux_spawn({ ");
                    for name in &rebinds {
                        self.w.push_str("let ");
                        self.w.push_str(name);
                        self.w.push_str(" = ");
                        self.w.push_str(name);
                        self.w.push_str(".clone(); ");
                    }
                    self.w.push_str("async move { ");
                }
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                match call.args.first() {
                    Some(Expr::Lambda(l)) if l.params.is_empty() => match &l.body {
                        juxc_ast::LambdaBody::Expr(e) => self.emit_expr(e),
                        juxc_ast::LambdaBody::Block(b) => {
                            // Trailing-expression block: the last
                            // expression statement is the task's
                            // value (emitted without a semicolon).
                            let (stmts, tail) = match b.statements.split_last() {
                                Some((juxc_ast::Stmt::Expr(t), rest)) => (rest, Some(t)),
                                _ => (&b.statements[..], None),
                            };
                            self.w.push('\n');
                            self.w.indent_inc();
                            for stmt in stmts {
                                self.w.emit_indent();
                                self.emit_stmt(stmt);
                            }
                            if let Some(tail) = tail {
                                self.w.emit_indent();
                                self.emit_expr(tail);
                                self.w.push('\n');
                            }
                            self.w.indent_dec();
                            self.w.emit_indent();
                        }
                    },
                    Some(other) => {
                        // Non-lambda argument: a future-valued
                        // expression — await it inside the task.
                        self.emit_expr(other);
                        self.w.push_str(".await");
                    }
                    None => {}
                }
                self.emitting_format_arg = prev;
                if rebinds.is_empty() {
                    self.w.push_str(" })");
                } else {
                    // close: async block, wrapper block, call paren.
                    self.w.push_str(" } })");
                }
                return;
            }
        }
        // `parallel(a, b, c, ...)` — async-runtime builtin per
        // JUX-ASYNC-ADDENDUM-v2. Wraps `futures::join!(...)` in an
        // `async { ... }` block, so the call evaluates to a **Future**
        // yielding the tuple `(R_a, R_b, R_c, …)`. Uniform shape:
        //
        //   - In async context: `await parallel(a, b)` resolves to
        //     the tuple after both futures complete.
        //   - From sync code:   `block_on(parallel(a, b))` drives
        //     the Future to completion via the executor.
        //
        // The `move` on the async block captures the argument
        // expressions by value (matches Rust's default for async
        // blocks and keeps lifetimes happy when the Future is
        // shuttled across `block_on`).
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "parallel" {
                self.w.push_str("async move { futures::join!(");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                for (i, arg) in call.args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.emitting_format_arg = prev;
                self.w.push_str(") }");
                return;
            }
        }
        // `block_on(future)` — async-runtime builtin: drive a Future
        // to completion synchronously, returning its resolved value.
        // Lowers to `futures::executor::block_on(future)`. The user
        // is responsible for ensuring the argument really is a
        // Future (i.e. the result of an `async` call or
        // `parallel(...)`); calling `block_on` on a non-Future
        // surfaces as a rustc type-mismatch at the emit site.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "block_on" {
                self.w.push_str("futures::executor::block_on(");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(arg) = call.args.first() {
                    self.emit_expr(arg);
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                return;
            }
        }
        // `yield_now()` — cooperative suspension point. Lowers to a
        // call into the emitted runtime helper (`__jux_yield_now()`,
        // defined in the prelude when async is detected). The
        // helper returns a Future; the caller is expected to
        // `await` it (`await yield_now()`), which is how the spec
        // shape reads.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "yield_now" {
                self.w.push_str("crate::__jux_yield_now()");
                return;
            }
        }
        // `Clock.nowMs()` — stdlib wall-clock reading. Routes
        // through the same `__jux_now_ms()` helper as the bare
        // `now_ms()` builtin; the class-qualified form is the
        // Java-shaped entry point per JUX-CORE-LIB-ADDENDUM.
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Clock"
                    && f.field.text == "nowMs"
                {
                    self.w.push_str("crate::__jux_now_ms()");
                    return;
                }
            }
        }
        // `now_ms()` — monotonic-ish clock reading. Lowers to the
        // emitted `__jux_now_ms()` helper (defined in the prelude
        // whenever async support is active, since timing is
        // commonly needed alongside async work). Returns the
        // milliseconds since the UNIX epoch as `i64` — `long` at
        // the Jux level.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "now_ms" {
                self.w.push_str("__jux_now_ms()");
                return;
            }
        }
        // `Worker.spawn(lambda)` — true multi-thread parallelism
        // per JUX-ASYNC-ADDENDUM §18.2. Runs the closure on the
        // OS thread pool, returns a `Task<T>` that can be `await`-ed
        // for the closure's value.
        //
        // Special-case the closure emit: the regular `emit_lambda`
        // wraps every Jux closure in `Rc<dyn Fn>` (so it can be
        // stored / passed around freely), but `Rc` isn't `Send`,
        // so a wrapped closure can't be shipped to a worker
        // thread. Here we strip the wrapper and emit a bare
        // `move || body` closure directly — `Worker::spawn` takes
        // an `FnOnce + Send + 'static`, which a `move ||` closure
        // capturing Send/'static values satisfies natively.
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Worker"
                    && f.field.text == "spawn"
                {
                    // Crate-rooted: the Worker shim lives at the crate
                    // root, while THIS call site may sit inside a
                    // package's module nest.
                    //
                    // Clone-rebind shared captures (same rule as the
                    // event-loop `spawn`): `move ||` would steal the
                    // caller's handle, but Arc-backed values (atomics,
                    // channels) are meant to be SHARED with the worker
                    // — rebinding `let x = x.clone();` in a wrapper
                    // block hands the closure its own handle.
                    let mut rebinds: Vec<String> = Vec::new();
                    if let Some(Expr::Lambda(l)) = call.args.first() {
                        let mut names: Vec<String> = Vec::new();
                        crate::exprs::collect_bare_names_in_lambda(l, &mut |n| {
                            if !names.iter().any(|x| x == n) {
                                names.push(n.to_string());
                            }
                        });
                        for name in names {
                            let known = self
                                .local_types
                                .iter()
                                .rev()
                                .find_map(|s| s.get(&name).cloned());
                            if let Some(ty) = known {
                                if !matches!(ty, juxc_tycheck::Ty::Primitive(_)) {
                                    rebinds.push(name);
                                }
                            }
                        }
                    }
                    self.w.push_str("crate::Worker::spawn(");
                    if !rebinds.is_empty() {
                        self.w.push_str("{ ");
                        for name in &rebinds {
                            self.w.push_str("let ");
                            self.w.push_str(name);
                            self.w.push_str(" = ");
                            self.w.push_str(name);
                            self.w.push_str(".clone(); ");
                        }
                    }
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    if let Some(arg) = call.args.first() {
                        match arg {
                            Expr::Lambda(l) => self.emit_bare_move_lambda(l),
                            // Anything else (method ref, named fn,
                            // path) goes through as-is — the user
                            // gets a clear rustc error if the
                            // value doesn't satisfy Worker.spawn's
                            // `FnOnce + Send + 'static` bound.
                            _ => self.emit_expr(arg),
                        }
                    }
                    if !rebinds.is_empty() {
                        self.w.push_str(" }");
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
            }
        }
        // `File.readText(path)` / `File.writeText(path, body)`
        // / `File.exists(path)` — stdlib I/O entry points per
        // JUX-CORE-LIB-ADDENDUM. Lowers to `std::fs::*` calls;
        // Phase-1 panic-on-error (no Result<T, IOException>
        // wiring yet).
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1 && qn.segments[0].text == "File" {
                    let method = f.field.text.as_str();
                    match method {
                        "readText" => {
                            // Borrow the path (AsRef<Path>) so a String
                            // path variable survives for later calls.
                            self.w.push_str("std::fs::read_to_string(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).unwrap()");
                            return;
                        }
                        "writeText" => {
                            // `std::fs::write(&path, &content)` — borrow
                            // BOTH (they satisfy AsRef) so the caller can
                            // keep using its Strings after the write,
                            // instead of moving them out (rustc E0382).
                            self.w.push_str("std::fs::write(&(");
                            if let Some(path) = call.args.first() {
                                self.emit_expr(path);
                            }
                            self.w.push(')');
                            if let Some(content) = call.args.get(1) {
                                self.w.push_str(", &(");
                                self.emit_expr(content);
                                self.w.push(')');
                            }
                            self.w.push_str(").unwrap()");
                            return;
                        }
                        "exists" => {
                            self.w.push_str("std::path::Path::new(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).exists()");
                            return;
                        }
                        "appendText" => {
                            // OpenOptions append+create, then write_all —
                            // wrapped in a block so the handle drops (and
                            // flushes) immediately.
                            self.w.push_str("{ use std::io::Write as _; let mut __jux_f = std::fs::OpenOptions::new().create(true).append(true).open(&(");
                            if let Some(path) = call.args.first() {
                                self.emit_expr(path);
                            }
                            self.w.push_str(")).unwrap(); __jux_f.write_all((");
                            if let Some(content) = call.args.get(1) {
                                self.emit_expr(content);
                            }
                            self.w.push_str(").as_bytes()).unwrap(); }");
                            return;
                        }
                        "readLines" => {
                            self.w.push_str("std::fs::read_to_string(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).unwrap().lines().map(|l| l.to_string()).collect::<Vec<_>>()");
                            return;
                        }
                        "delete" => {
                            self.w.push_str("std::fs::remove_file(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).unwrap()");
                            return;
                        }
                        "listDir" => {
                            self.w.push_str("std::fs::read_dir(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>()");
                            return;
                        }
                        _ => {}
                    }
                }
                // `Path.join/parent/fileName/extension/isDir/isFile` —
                // static path-string helpers (jux.std.io.Path). Paths
                // are plain Strings in Phase-1; the query forms produce
                // `Option<String>` (Jux `String?`).
                if qn.segments.len() == 1 && qn.segments[0].text == "Path" {
                    let method = f.field.text.as_str();
                    match method {
                        "join" => {
                            self.w.push_str("{ let mut __jux_p = std::path::PathBuf::from(&(");
                            if let Some(base) = call.args.first() {
                                self.emit_expr(base);
                            }
                            self.w.push_str(")); __jux_p.push(&(");
                            if let Some(child) = call.args.get(1) {
                                self.emit_expr(child);
                            }
                            self.w.push_str(")); __jux_p.to_string_lossy().into_owned() }");
                            return;
                        }
                        "parent" | "fileName" | "extension" => {
                            let accessor = match method {
                                "parent" => ".parent().map(|x| x.to_string_lossy().into_owned())",
                                "fileName" => ".file_name().map(|x| x.to_string_lossy().into_owned())",
                                _ => ".extension().map(|x| x.to_string_lossy().into_owned())",
                            };
                            self.w.push_str("std::path::Path::new(&(");
                            self.emit_call_args(call);
                            self.w.push_str("))");
                            self.w.push_str(accessor);
                            return;
                        }
                        "isDir" => {
                            self.w.push_str("std::path::Path::new(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).is_dir()");
                            return;
                        }
                        "isFile" => {
                            self.w.push_str("std::path::Path::new(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).is_file()");
                            return;
                        }
                        _ => {}
                    }
                }
                // `Console.readLine()` — stdin line read with the Jux
                // nullable protocol: `None` at EOF, trailing `\r\n` /
                // `\n` stripped on success.
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Console"
                    && f.field.text == "readLine"
                {
                    self.w.push_str("{ let mut __jux_line = String::new(); match std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut __jux_line) { Ok(0) | Err(_) => None, Ok(_) => { while __jux_line.ends_with('\\n') || __jux_line.ends_with('\\r') { __jux_line.pop(); } Some(__jux_line) } } }");
                    return;
                }
                // `Instant.now()` — monotonic time-point capture
                // (jux.std.time). The elapsed readings are instance
                // methods, dispatched in `try_emit_stdlib_method`.
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Instant"
                    && f.field.text == "now"
                {
                    self.w.push_str("std::time::Instant::now()");
                    return;
                }
            }
        }
        // Stdlib method dispatch — rewrites Jux's Java-spec
        // method names (`xs.add(v)`, `s.toUpperCase()`,
        // `m.contains(k)`, …) into the matching Rust shape
        // (`xs.push(v)`, `s.to_uppercase()`,
        // `m.contains_key(&k)`, …). Receiver-type drives the
        // routing — arrays / String / HashMap / HashSet each
        // get a bespoke emit function.
        if self.try_emit_stdlib_method(call) {
            return;
        }
        // Bare-name method-call rewrite inside a class/interface body.
        // `foo(args)` inside `class C` or `interface I` should resolve
        // to `self.foo(args)` when `foo` is a non-static method on
        // the enclosing type (Java's implicit-`this` rule). The
        // resolver pre-declares parameter and local names so a
        // bare-name reference there shadows the method lookup; we
        // only get here when no shadowing happened.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 {
                let name = &qn.segments[0].text;
                let mut on_self = false;
                // Static-method emit: when the bare call resolves
                // to a static on the enclosing class, emit
                // `EnclosingClass::method(args)` so we don't fall
                // through to the generic free-function path
                // (which would emit a bare `method(args)` that
                // Rust can't find).
                let mut as_static_on: Option<String> = None;
                if let Some(iface_name) = &self.enclosing_interface {
                    if let Some((_, iface)) = self.lookup_interface_by_bare_or_fqn(iface_name) {
                        if let Some(m) = iface.methods.get(name.as_str()) {
                            if !m.is_static {
                                on_self = true;
                            }
                        }
                    }
                }
                if !on_self {
                    // Walk the enclosing class's `extends` chain so a
                    // bare call to an inherited method (`name()` in
                    // `Dog.bark()` finding `Animal::name`) resolves
                    // through `self.method()` and Rust's `Deref` does
                    // the rest. Static methods don't inherit Java-
                    // style — we record the FQN so the emitter can
                    // produce `Class::method(args)` instead.
                    let mut cursor: Option<String> = self.enclosing_class.clone();
                    while let Some(class_name) = cursor {
                        let Some(class) = self.lookup_class_by_bare_or_fqn(&class_name) else {
                            break;
                        };
                        if let Some(m) = class.methods.get(name.as_str()) {
                            if m.is_static {
                                as_static_on = Some(class_name.clone());
                            } else {
                                on_self = true;
                            }
                            break;
                        }
                        cursor = class
                            .extends
                            .as_ref()
                            .and_then(|t| t.name.segments.first())
                            .map(|s| s.text.clone());
                    }
                }
                if let Some(class_name) = as_static_on {
                    self.w.push_str(&class_name);
                    self.w.push_str("::");
                    self.w.push_str(name);
                    if let Some(sfx) = self.pending_method_suffix.take() {
                        self.w.push_str(&sfx);
                    }
                    self.w.push('(');
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    for (i, arg) in call.args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
                if on_self {
                    let alias = self.this_alias.as_deref().unwrap_or("self");
                    self.w.push_str(alias);
                    self.w.push('.');
                    self.w.push_str(name);
                    if let Some(sfx) = self.pending_method_suffix.take() {
                        self.w.push_str(&sfx);
                    }
                    self.w.push('(');
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    for (i, arg) in call.args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
            }
        }
        // Safe-navigation method call (`obj?.method(args)`): the
        // callee parses as a `Field` with `safe: true`. Lower to
        // `obj.as_ref().map(|__t| __t.method(args))` so the result
        // is `Option<ReturnType>` and the receiver isn't moved.
        // Cleared inside the closure: args are still consumed
        // values, so the format-arg flag (if set) doesn't leak.
        if let Expr::Field(f) = &*call.callee {
            if f.safe {
                self.emit_safe_method_call(f, call);
                return;
            }
        }
        // Static interface-method call: `Interface.staticMethod(args)`
        // → `<Interface>::staticMethod(args)`. Interface methods
        // declared `static` lower to Rust trait associated functions
        // and are called the same way as class statics. We check
        // this BEFORE the class-static path because interfaces are
        // a separate namespace; `path_resolves_to_class_in_emit`
        // doesn't see them.
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1 {
                    let iface_name = &qn.segments[0].text;
                    if let Some(iface) = self.symbols.interfaces.get(iface_name) {
                        if iface
                            .methods
                            .get(f.field.text.as_str())
                            .map(|m| m.is_static)
                            .unwrap_or(false)
                        {
                            // `Iface_method` free function — see
                            // `emit_interface_decl` for the
                            // companion definition site.
                            self.w.push_str(iface_name);
                            self.w.push('_');
                            self.w.push_str(&f.field.text);
                            self.w.push('(');
                            let prev = self.emitting_format_arg;
                            self.emitting_format_arg = false;
                            for (i, arg) in call.args.iter().enumerate() {
                                if i > 0 {
                                    self.w.push_str(", ");
                                }
                                self.emit_expr(arg);
                            }
                            self.emitting_format_arg = prev;
                            self.w.push(')');
                            return;
                        }
                    }
                }
            }
        }
        // Static method call: `ClassName.staticMethod(args)` (or
        // `pkg.Cls.method(args)`) → `Path::method(args)`. Recognize
        // the receiver as a class name and switch the dot to `::`.
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    let is_static_method = self
                        .symbols
                        .classes
                        .get(&class_fqn)
                        .and_then(|c| c.methods.get(f.field.text.as_str()))
                        .map(|m| m.is_static)
                        .unwrap_or(false);
                    if is_static_method {
                        // **Generic-class static method** → free function
                        // `<Class>_<method>` (the backend lifts it out of the
                        // parameterized impl so the call doesn't need the
                        // class's K/V/N inferred — E0284). Plain statics only;
                        // synthesized `__…` helpers stay associated. The class
                        // path is emitted then joined with `_`, not `::`.
                        let class_is_generic = self
                            .symbols
                            .classes
                            .get(&class_fqn)
                            .map(|c| !c.generic_params.is_empty())
                            .unwrap_or(false);
                        let lift_to_free_fn =
                            class_is_generic && !f.field.text.starts_with("__");
                        // §G.9.2: a static call on a foreign stub class
                        // (`Url.parse(...)`) lowers through its REAL Rust path
                        // (`url::Url::parse(...)`) from the `@rust` annotation,
                        // not the flat `crate::rust::url::Url` spelling.
                        let external_real = self
                            .symbols
                            .classes
                            .get(&class_fqn)
                            .filter(|c| c.is_external)
                            .and_then(|c| c.rust_path.clone());
                        if let Some(real) = external_real {
                            self.w.push_str(&real);
                        } else {
                            self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                        }
                        // Free-fn form joins with `_`; associated form with `::`.
                        self.w.push_str(if lift_to_free_fn { "_" } else { "::" });
                        self.w.push_str(&f.field.text);
                        if let Some(sfx) = self.pending_method_suffix.take() {
                            self.w.push_str(&sfx);
                        }
                        self.w.push('(');
                        // Args of a regular call consume their values
                        // — clear the format-arg flag so any nested
                        // string literal still self-coerces into
                        // owned `String` (the param's declared type).
                        // Per-arg nullable-wrap: when the static
                        // method's matching positional parameter is
                        // `T?`, a non-nullable value is lifted into
                        // `Some(value)`.
                        let prev = self.emitting_format_arg;
                        self.emitting_format_arg = false;
                        for (i, arg) in call.args.iter().enumerate() {
                            if i > 0 {
                                self.w.push_str(", ");
                            }
                            // Interface-typed param slot: wrap a class value in
                            // `Rc<dyn Trait>` / clone a dyn handle.
                            if let Some(pty) = self.callee_param_type(&call.callee, i) {
                                if !matches!(
                                    self.iface_coercion_to(&pty, arg),
                                    crate::analysis::IfaceCoercion::None,
                                ) {
                                    self.emit_expr_coerced_to_iface(&pty, arg);
                                    continue;
                                }
                            }
                            let nullable = self.callee_param_is_nullable(&call.callee, i);
                            let upcast = self.arg_needs_sealed_upcast(&call.callee, i, arg);
                            // Foreign by-ref param (`&str`, …): re-attach the
                            // call-site borrow (§G.9.2). Resolved directly off the
                            // already-known static method, since the class-name
                            // receiver never appears in `expr_types`.
                            let is_ref = self
                                .symbols
                                .classes
                                .get(&class_fqn)
                                .and_then(|c| c.methods.get(f.field.text.as_str()))
                                .and_then(|m| m.params.get(i))
                                .map(|p| p.is_ref)
                                .unwrap_or(false);
                            if is_ref {
                                self.w.push('&');
                            }
                            self.emit_arg_with_nullable_wrap(arg, nullable);
                            if upcast {
                                self.w.push_str(".into()");
                            } else if !nullable && self.wrapper_value_needs_clone(arg) {
                                // Wrapper-class share-on-pass (§CR.4.1) —
                                // same shared-handle rule as the generic
                                // call path, for `Class.staticMethod(arg)`.
                                self.w.push_str(".clone()");
                            }
                        }
                        self.emitting_format_arg = prev;
                        self.w.push(')');
                        return;
                    }
                }
            }
        }
        // **Borrow-hoist pre-pass.** `a.addTwice(a.bump())` on a plain
        // (non-wrapper) class would emit two overlapping `&mut a`
        // borrows — rustc E0499 (two-phase borrows only cover SHARED
        // argument borrows). Java semantics evaluate the argument
        // first, so when any argument contains a call to a mutating
        // method on the SAME receiver place, hoist every argument into
        // a temp inside a block expression:
        //
        //   { let __jux_arg0 = a.bump(); a.addTwice(__jux_arg0) }
        //
        // Hoisting ALL args (not just the offending one) preserves the
        // left-to-right evaluation order. Wrapper-class receivers don't
        // need this (their methods take `&self`, interior-mutable), but
        // applying the hoist there too would be harmless — the trigger
        // simply fires on the textual shape.
        // **Lexical evaluation order (§S.1.4 / C7).** A call whose
        // NAMED arguments were re-ordered relative to declaration order
        // carries an `eval_order`; hoist the args into temps in that
        // lexical order so side effects fire left-to-right as written,
        // then pass them positionally. (`emit_call_with_hoisted_args`
        // reads `call.eval_order`.)
        if !call.eval_order.is_empty() {
            self.emit_call_with_hoisted_args(call);
            return;
        }
        // C6 self-aliasing guard: an instance-method call that passes a
        // `&mut T` foreign-collection argument rooted at the SAME
        // receiver (`this.take(this.data)`) would borrow `self` twice
        // (E0502). Route it through a `std::mem::take` + write-back
        // wrapper so the field is moved out, passed by exclusive ref,
        // and stored back — no overlapping borrow, mutation preserved.
        if self.call_has_self_aliasing_byref_arg(call) {
            self.emit_call_with_byref_writeback(call);
            return;
        }
        if self.call_needs_borrow_hoist(call) {
            // When the RECEIVER is itself read through a wrapper
            // `.0.borrow()` (field-path receiver), both the receiver
            // guard AND the argument guards must drop before the call —
            // hoist both. Otherwise args-only suffices.
            if let Some(cf) = self.callee_receiver_reads_through_borrow(&call.callee) {
                self.emit_call_with_hoisted_receiver(call, cf, true);
            } else {
                self.emit_call_with_hoisted_args(call);
            }
            return;
        }
        // **Re-entrancy borrow-hoist.** If the receiver is read through a
        // wrapper `.0.borrow()` guard, hoist it into a temp so the guard drops
        // before the call — otherwise a re-entrant method (one that, directly
        // or through a callee, mutates the same object) panics `already
        // borrowed` (§CR.4.1).
        if let Some(cf) = self.callee_receiver_reads_through_borrow(&call.callee) {
            self.emit_call_with_hoisted_receiver(call, cf, false);
            return;
        }
        // **Function-typed field call** — `obj.task()` where `task` is
        // declared as a `() -> T` field (stored as `Rc<dyn Fn(…)>`).
        // Methods live on the wrapper newtype, so `emit_call_callee=true`
        // suppresses `.0.borrow()` to avoid the guard. But function-typed
        // fields live INSIDE `C_Inner`, so the borrow IS required. Detect
        // and handle this before the generic path sets the flag.
        if let Expr::Field(f) = &*call.callee {
            let class_bare = if matches!(*f.object, Expr::This(_)) {
                self.enclosing_class.clone()
            } else {
                self.receiver_class_bare(&f.object)
            };
            // Use lookup_class_by_bare_or_fqn (bare-name aware) instead of
            // symbols.lookup_field (FQN-only) so probes.TaskRunner resolves
            // from the bare "TaskRunner" key stored in enclosing_class.
            let is_fn_field = class_bare.as_deref().and_then(|bare| {
                let class = self.lookup_class_by_bare_or_fqn(bare)?;
                class.fields.get(f.field.text.as_str())
            }).map(|fsig| fsig.ty.fn_shape.is_some()).unwrap_or(false);
            if is_fn_field {
                // Emit as `(field_read)(args)` — parens prevent Rust from
                // interpreting this as a method call on the struct/wrapper.
                // For plain structs: `(self.task)(args)`
                // For wrapper classes: `(self.0.borrow().task.clone())(args)`
                // Both are valid because Rc<dyn Fn(...)> implements Fn via Deref.
                self.w.push('(');
                self.emit_expr(&call.callee);  // emitting_call_callee=false → borrow fires
                self.w.push(')');
                self.w.push('(');
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                for (i, arg) in call.args.iter().enumerate() {
                    if i > 0 { self.w.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                return;
            }
        }
        // Generic call: emit `callee(args, …)` literally. Post Fix 1
        // every Jux `String` value is already an owned Rust `String`,
        // so the previous per-arg enum-variant payload coercion is
        // unnecessary — the string-literal site self-coerces inside
        // `emit_literal` and identifier references are typed `String`
        // directly.
        // Mark the callee so the outermost `Field` (the method name)
        // skips the wrapper `.0.borrow()` rewrite — a method lives on
        // the newtype, not in `C_Inner`, even when a same-named field
        // exists up the chain (`legs` field + `legs()` method).
        let prev_callee = self.emitting_call_callee;
        self.emitting_call_callee = true;
        // Clear the borrow-context flags while emitting the callee. The
        // *receiver* of a method call (`recv.method()`) is a fresh
        // evaluation, never a Display/comparison slot itself — only the
        // call's RESULT flows into the surrounding format-arg /
        // comparison position. Leaving these set would wrongly suppress
        // the statement-scoped clone on a wrapper-borrowed field receiver
        // (`$"${this.item.greet()}"` → the `.item` read through
        // `.0.borrow()` must clone out before `.greet()` takes `&mut`).
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        let prev_cmp = std::mem::take(&mut self.emitting_comparison_operand);
        self.emit_expr(&call.callee);
        self.emitting_format_arg = prev_fmt;
        self.emitting_comparison_operand = prev_cmp;
        self.emitting_call_callee = prev_callee;
        // Explicit call-site type arguments (`id<int>(5)`) lower to a
        // Rust turbofish `id::<i32>(5)`. Required for correctness: Rust
        // would otherwise infer the type-param from the argument
        // literals/values, silently ignoring the user's annotation
        // (`identity<long>(5)` must bind `T = i64`, not the `i32` the
        // literal would default to). Each arg is lowered as a
        // generic-arg slot (owned `String`, `Rc<dyn …>` for poly/iface
        // types) so it matches how the same `T` is monomorphized when
        // the call relies on inference.
        if !call.explicit_generic_args.is_empty() {
            self.w.push_str("::<");
            for (i, ty) in call.explicit_generic_args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                if crate::analysis::is_jux_string_type(ty) {
                    self.w.push_str("String");
                } else {
                    self.emit_value_type_as_rust(ty);
                }
            }
            self.w.push('>');
        }
        self.w.push('(');
        // Same flag discipline as above: a regular call's args
        // consume String values, so any inner string literal needs
        // the Fix-1 self-coerce — clear the format-arg context here.
        // Per-arg nullable-wrap when the callee's declared
        // parameter type is `T?` and the value isn't already
        // `Option<T>`-shaped.
        // Per-arg sealed-upcast wrap when the param is a sealed
        // parent and the arg is one of its permitted subclasses:
        // emit `arg.into()` so the auto-`From<Sub> for Sealed`
        // impl from `emit_sealed_enum` lifts the subclass into
        // the matching variant.
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        // Recursive enum-variant construction: a self-referential payload slot
        // is boxed in the enum decl (`Branch(Box<Tree>, …)`), so wrap the
        // matching argument `Box::new(arg)` here. Empty unless this is an enum
        // ctor with at least one boxed slot.
        let boxed_slots = self.enum_ctor_boxed_slots(call);
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 { self.w.push_str(", "); }
            let box_this = boxed_slots.get(i).copied().unwrap_or(false);
            if box_this {
                self.w.push_str("std::boxed::Box::new(");
            }
            // §G.9.2: a borrowed parameter (`&T`) of an external method gets the
            // call-site `&` back — `m.containsKey("a")` → `m.contains_key(&"a"…)`.
            if self.callee_param_is_ref(&call.callee, i) {
                self.w.push('&');
            }
            // C6: a foreign-collection param lowered to `&mut T` takes the
            // call-site `&mut <place>` (Java container-passing). Two-phase
            // borrows cover the common `f(v)` shape; an arg that re-reads
            // the receiver is hoisted by `call_needs_borrow_hoist`.
            if self.arg_is_byref(call, i) {
                self.emit_byref_arg(arg);
            } else {
                self.emit_call_arg_value(call, i, arg);
            }
            if box_this {
                self.w.push(')');
            }
        }
        self.emitting_format_arg = prev;
        self.w.push(')');

        // Phase-1 workaround: Rust's `Vec::pop` returns `Option<T>` but
        // Jux doesn't yet have an `Option` type, so Jux user code uses
        // `var top = stack.pop();` expecting a `T`-typed value. We
        // bridge that by appending `.unwrap()` here — pop on an empty
        // Vec then panics, which mirrors Java's `NoSuchElementException`
        // shape. Remove this special case once `Option<T>` lands and
        // pop can return `T?` directly.
        //
        // This is EXCLUSIVELY the `Vec::pop` intrinsic: a user-defined class
        // with its own `pop()` method returns its declared type directly, so
        // appending `.unwrap()` there would call it on the user's return value
        // (e.g. `String::unwrap`, which does not exist — a leaked rustc E0599).
        // Gate on the receiver NOT being a user class.
        if let Expr::Field(f) = &*call.callee {
            if f.field.text == "pop" && call.args.is_empty() {
                // `class_asts` is keyed by FQN (`x4.Stack`); the receiver bare
                // name matches a user class when it equals some key's last
                // segment.
                let receiver_is_user_class = self
                    .receiver_class_bare(&f.object)
                    .is_some_and(|bare| {
                        self.class_asts
                            .keys()
                            .any(|k| k.rsplit('.').next().unwrap_or(k.as_str()) == bare)
                    });
                if !receiver_is_user_class {
                    self.w.push_str(".unwrap()");
                }
            }
        }
    }

    /// C6: emit a COMPLETE foreign-collection argument whose matching
    /// parameter lowers to `&mut T` (Java container-passing). The whole
    /// arg is `&mut <place>` — the caller's actual place, NOT a coerced
    /// value (no `.clone()` / `Some(…)` ladder; cloning would lose the
    /// mutation visibility this feature exists to provide). When the
    /// argument is itself a `&mut T` parameter of the CURRENT body
    /// flowing onward into another `&mut` slot, emits a reborrow
    /// `&mut *v` instead of nesting (`&mut &mut Vec`).
    ///
    /// The value emit runs with the share/format/comparison flags
    /// cleared and the lvalue flag SET, so `emit_field` / `emit_path`
    /// emit the bare place (`more`, `self.items`, `g[i]`) with no
    /// auto-`.clone()`.
    pub(crate) fn emit_byref_arg(&mut self, arg: &Expr) {
        self.w.push_str("&mut ");
        if let Expr::Path(qn) = arg {
            if qn.segments.len() == 1
                && self.byref_param_names.contains(&qn.segments[0].text)
            {
                // Reborrow an inherited `&mut T` param.
                self.w.push('*');
            }
        }
        // Emit the place with coercion suppressed: a `&mut` target must
        // be the place itself, never a cloned temporary.
        let prev_lval = std::mem::replace(&mut self.emitting_lvalue, true);
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        let prev_cmp = std::mem::take(&mut self.emitting_comparison_operand);
        self.emit_expr(arg);
        self.emitting_lvalue = prev_lval;
        self.emitting_format_arg = prev_fmt;
        self.emitting_comparison_operand = prev_cmp;
    }

    /// C6: true when argument `i` of `call` is a foreign-collection
    /// slot lowered to `&mut T`. Used by the borrow-hoist forms to skip
    /// binding such an argument into a value temp (which would mutate
    /// the temp, not the caller's place) and instead borrow the place
    /// directly at the call slot.
    pub(crate) fn arg_is_byref(&self, call: &CallExpr, i: usize) -> bool {
        self.callee_byref_param(&call.callee, i)
    }

    /// For an enum-variant **construction** call (`Tree.Branch(a, b)`), return a
    /// per-argument flag vector marking which payload slots are self-referential
    /// recursive slots that the enum decl boxed (`Branch(Box<Tree>, Box<Tree>)`).
    /// Such args must be wrapped `Box::new(arg)` at the construction site so the
    /// value matches the boxed slot type. Returns an empty vector when the call
    /// is not an enum-variant construction (or no slot is boxed) — callers index
    /// it with `.get(i).copied().unwrap_or(false)`.
    ///
    /// Resolution mirrors the enum-variant path in `emit_field`: the callee must
    /// be `Enum.Variant` (a `Field` whose object is a single-segment path naming
    /// a known enum), resolved bare → import-alias → cross-package last-segment.
    pub(crate) fn enum_ctor_boxed_slots(&self, call: &CallExpr) -> Vec<bool> {
        let Expr::Field(f) = &*call.callee else {
            return Vec::new();
        };
        let Expr::Path(qn) = &*f.object else {
            return Vec::new();
        };
        if qn.segments.len() != 1 {
            return Vec::new();
        }
        let bare = qn.segments[0].text.as_str();
        // Resolve the enum's FQN key in `symbols.enums`: direct, then via the
        // current unit's import-alias map, then a cross-package last-segment scan.
        let fqn_key = if self.symbols.enums.contains_key(bare) {
            Some(bare.to_string())
        } else if let Some(idx) = self.current_unit_idx {
            self.symbols
                .units
                .get(idx)
                .and_then(|ctx| ctx.unqualified.get(bare))
                .filter(|fqn| self.symbols.enums.contains_key(fqn.as_str()))
                .cloned()
                .or_else(|| {
                    self.symbols
                        .enums
                        .keys()
                        .find(|k| k.rsplit('.').next().unwrap_or(k.as_str()) == bare)
                        .cloned()
                })
        } else {
            self.symbols
                .enums
                .keys()
                .find(|k| k.rsplit('.').next().unwrap_or(k.as_str()) == bare)
                .cloned()
        };
        let Some(fqn_key) = fqn_key else {
            return Vec::new();
        };
        let Some(sig) = self.symbols.enums.get(&fqn_key) else {
            return Vec::new();
        };
        let Some(variant) = sig.variants.get(f.field.text.as_str()) else {
            return Vec::new();
        };
        // The enum's bare name — what a self-referential payload slot's type
        // names. Use the FQN's last segment so cross-package keys resolve too.
        let enum_bare = fqn_key.rsplit('.').next().unwrap_or(fqn_key.as_str());
        let flags: Vec<bool> = variant
            .payload
            .iter()
            .map(|slot| crate::decls::enums::is_recursive_enum_slot(slot, enum_bare))
            .collect();
        if flags.iter().any(|b| *b) {
            flags
        } else {
            Vec::new()
        }
    }

    /// True when emitting `call` produces a Rust **block expression** `{ … }`
    /// rather than a plain expression — i.e. one of the receiver/argument
    /// hoist lowerings fires (re-entrancy borrow drop, self-aliasing `&mut`
    /// write-back, or lexical eval-order arg hoisting). Such an operand must be
    /// parenthesized in any position where a bare block would be misparsed,
    /// notably before a postfix `as` cast (`{ … } as T` is a parse error;
    /// `({ … }) as T` is correct). Mirrors the dispatch in [`Self::emit_call`].
    pub(crate) fn call_emits_block(&self, call: &CallExpr) -> bool {
        !call.eval_order.is_empty()
            || self.call_has_self_aliasing_byref_arg(call)
            || self.call_needs_borrow_hoist(call)
            || self.callee_receiver_reads_through_borrow(&call.callee).is_some()
    }

    /// C6: true when any `&mut T` foreign-collection argument is a
    /// FIELD place rooted at the call's own receiver
    /// (`this.m(this.data)` / `r.m(r.data)`). Such a call borrows the
    /// receiver for the method AND mutably for the field at once
    /// (rustc E0502) — it needs the `std::mem::take` write-back form.
    /// A bare-local arg (`m(local)`) never aliases the receiver, so it
    /// stays on the cheap inline path.
    fn call_has_self_aliasing_byref_arg(&self, call: &CallExpr) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let Some(recv_root) = place_path_of(&f.object) else {
            return false;
        };
        call.args.iter().enumerate().any(|(i, arg)| {
            if !self.arg_is_byref(call, i) {
                return false;
            }
            // Only FIELD places can alias the receiver; a bare local
            // cannot. Compare the arg's place root to the receiver's.
            matches!(arg, Expr::Field(_))
                && place_path_of(arg)
                    .map(|p| {
                        let arg_root = p.split('.').next().unwrap_or(&p);
                        let recv_first = recv_root.split('.').next().unwrap_or(&recv_root);
                        arg_root == recv_first
                    })
                    .unwrap_or(false)
        })
    }

    /// C6: emit `recv.m(…)` where a `&mut T` foreign-collection argument
    /// aliases the receiver, via `std::mem::take` + write-back:
    ///
    /// ```ignore
    /// { let mut __jux_byref0 = std::mem::take(&mut self.data);
    ///   let __jux_ret = self.take(&mut __jux_byref0);
    ///   self.data = __jux_byref0;
    ///   __jux_ret }
    /// ```
    ///
    /// The field is moved out (leaving `T::default()`), passed by
    /// exclusive reference, then stored back — so the two borrows of
    /// the receiver never overlap and the callee's mutation is
    /// preserved. Non-aliasing args emit normally. The receiver itself
    /// is read once for the call (its `&mut self`/`&self` borrow no
    /// longer conflicts since the field was already taken).
    /// If `callee` names a C foreign function (declared in an `unsafe native`
    /// block, `FunctionSig::is_extern_c`), return its marshalling shape: a
    /// [`FfiArg`] per parameter and the [`FfiRet`] for the return. Returns `None`
    /// for any non-foreign call. The data is owned so the caller can then take
    /// `&mut self` to emit.
    fn extern_c_call_shape(&self, callee: &Expr) -> Option<(Vec<FfiArg>, FfiRet)> {
        let Expr::Path(qn) = callee else { return None };
        if qn.segments.len() != 1 {
            return None;
        }
        let name = qn.segments[0].text.as_str();
        // Foreign fns are unique-named; match the bare key or any FQN ending in
        // `.name`.
        let sig = self
            .symbols
            .functions
            .get(name)
            .filter(|s| s.is_extern_c)
            .or_else(|| {
                self.symbols
                    .functions
                    .iter()
                    .find(|(k, v)| v.is_extern_c && k.rsplit('.').next() == Some(name))
                    .map(|(_, v)| v)
            })?;
        let args = sig
            .params
            .iter()
            .map(|p| if p.is_out { FfiArg::Out } else { ffi_arg_kind(&p.ty) })
            .collect();
        let ret = match &sig.return_type {
            juxc_ast::ReturnType::Type(t) if type_ref_is_string(t) => {
                FfiRet::Str { nullable: t.nullable }
            }
            juxc_ast::ReturnType::Type(t) if type_ref_is_char(t) => FfiRet::Char,
            _ => FfiRet::Plain,
        };
        Some((args, ret))
    }

    /// Emit a foreign C call with `String` marshalling (Layout-ABI §L.7), as a
    /// Rust block expression:
    /// ```text
    /// { let __c0 = ::std::ffi::CString::new(arg0).expect("…");
    ///   let __ret = foo(__c0.as_ptr() as *const core::ffi::c_char, n);
    ///   if __ret.is_null() { String::new() }
    ///   else { unsafe { ::std::ffi::CStr::from_ptr(__ret as *const core::ffi::c_char) }
    ///              .to_string_lossy().into_owned() } }
    /// ```
    /// Inbound is copy-out-never-free (the C buffer is read, never deallocated;
    /// UTF-8 decode is lossy). A nullable return (`String?`) maps null → `None`;
    /// a non-nullable `String` maps null → empty. Each `CString` temp lives to
    /// the end of the block, keeping its buffer alive across the call. The
    /// surrounding Jux `unsafe { }` provides the unsafe context for the foreign
    /// call and the `CStr::from_ptr` read.
    fn emit_extern_c_call(&mut self, call: &CallExpr, args: &[FfiArg], ret: FfiRet) {
        self.w.push_str("{ ");
        let prev = std::mem::take(&mut self.emitting_format_arg);
        // 1. Marshal each `String` argument into a NUL-terminated `CString` temp
        //    (kept alive until the end of the block, across the call). `char`
        //    args need no temp — they convert inline below. A C-variadic call
        //    (`printf(fmt, ...)`) has more args than declared params: a trailing
        //    string-LITERAL / interpolation arg is marshalled the same way (per
        //    §L.4.2 "String args marshal to const char* automatically"); other
        //    trailing args (ints, floats, pointers) pass through directly.
        for (i, arg) in call.args.iter().enumerate() {
            let is_str = matches!(args.get(i), Some(FfiArg::Str))
                || (i >= args.len() && expr_is_string_literal(arg));
            if is_str {
                self.w.push_str(&format!("let __c{i} = ::std::ffi::CString::new("));
                self.emit_expr(arg);
                self.w.push_str(
                    ").expect(\"string passed to C contains an interior NUL byte\"); ",
                );
            }
        }
        // 2. The call (bound to `__ret` only when we convert the return).
        let convert_ret = !matches!(ret, FfiRet::Plain);
        if convert_ret {
            self.w.push_str("let __ret = ");
        }
        self.emit_expr(&call.callee);
        self.w.push('(');
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            match args.get(i) {
                // `String` arg: pass the kept-alive `CString`'s `const char*`.
                Some(FfiArg::Str) => {
                    self.w
                        .push_str(&format!("__c{i}.as_ptr() as *const core::ffi::c_char"));
                }
                // `char` arg: a Jux/Rust `char` (4-byte) truncates to a C `char`.
                Some(FfiArg::Char) => {
                    self.w.push('(');
                    self.emit_expr(arg);
                    self.w.push_str(") as core::ffi::c_char");
                }
                // `out <place>` arg: a `*mut T` aimed at the place, so the C
                // callee writes through it. `Expr::Out(inner)` carries the place.
                Some(FfiArg::Out) => {
                    self.w.push_str("::core::ptr::addr_of_mut!(");
                    match arg {
                        Expr::Out(inner, _) => self.emit_expr(inner),
                        other => self.emit_expr(other),
                    }
                    self.w.push(')');
                }
                // Trailing variadic string-literal arg: pass the kept-alive
                // `CString`'s `const char*` (same as a fixed `String` param).
                None if expr_is_string_literal(arg) => {
                    self.w
                        .push_str(&format!("__c{i}.as_ptr() as *const core::ffi::c_char"));
                }
                _ => self.emit_expr(arg),
            }
        }
        self.w.push(')');
        self.emitting_format_arg = prev;
        // 3. Convert the return.
        match ret {
            FfiRet::Plain => {}
            // Copy a `String` out of the C buffer (read-only, never freed).
            FfiRet::Str { nullable } => {
                self.w.push_str("; ");
                let copy = "unsafe { ::std::ffi::CStr::from_ptr(__ret as *const core::ffi::c_char) }\
                            .to_string_lossy().into_owned()";
                if nullable {
                    self.w
                        .push_str(&format!("if __ret.is_null() {{ None }} else {{ Some({copy}) }}"));
                } else {
                    self.w
                        .push_str(&format!("if __ret.is_null() {{ String::new() }} else {{ {copy} }}"));
                }
            }
            // A C `char` widens back to a Jux/Rust `char` (via the byte value).
            FfiRet::Char => self.w.push_str("; (__ret as u8) as char"),
        }
        self.w.push_str(" }");
    }

    fn emit_call_with_byref_writeback(&mut self, call: &CallExpr) {
        let Expr::Field(f) = &*call.callee else {
            // Defensive: detection guarantees a Field callee.
            self.emit_call(call);
            return;
        };
        self.w.push_str("{ ");
        // 1. Take out each self-aliasing byref field into a temp.
        let recv_first = place_path_of(&f.object)
            .and_then(|p| p.split('.').next().map(str::to_string))
            .unwrap_or_default();
        let mut taken: Vec<(usize, &Expr)> = Vec::new();
        for (i, arg) in call.args.iter().enumerate() {
            let aliases = self.arg_is_byref(call, i)
                && matches!(arg, Expr::Field(_))
                && place_path_of(arg)
                    .map(|p| p.split('.').next().unwrap_or(&p) == recv_first)
                    .unwrap_or(false);
            if aliases {
                self.w.push_str(&format!("let mut __jux_byref{i} = std::mem::take(&mut "));
                let prev_lval = std::mem::replace(&mut self.emitting_lvalue, true);
                self.emit_expr(arg);
                self.emitting_lvalue = prev_lval;
                self.w.push_str("); ");
                taken.push((i, arg));
            }
        }
        // 2. Emit the call, binding its result so the write-back can
        //    follow before the block yields the value.
        self.w.push_str("let __jux_ret = ");
        let prev_callee = self.emitting_call_callee;
        self.emitting_call_callee = true;
        self.emit_expr(&call.callee);
        self.emitting_call_callee = prev_callee;
        if let Some(sfx) = self.pending_method_suffix.take() {
            self.w.push_str(&sfx);
        }
        self.w.push('(');
        let prev_fmt = std::mem::replace(&mut self.emitting_format_arg, false);
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            if self.callee_param_is_ref(&call.callee, i) {
                self.w.push('&');
            }
            if taken.iter().any(|(ti, _)| *ti == i) {
                self.w.push_str(&format!("&mut __jux_byref{i}"));
            } else if self.arg_is_byref(call, i) {
                self.emit_byref_arg(arg);
            } else {
                self.emit_call_arg_value(call, i, arg);
            }
        }
        self.emitting_format_arg = prev_fmt;
        self.w.push_str("); ");
        // 3. Write each taken field back.
        for (_, arg) in &taken {
            let prev_lval = std::mem::replace(&mut self.emitting_lvalue, true);
            self.emit_expr(arg);
            self.emitting_lvalue = prev_lval;
            // Find this arg's slot index for the temp name.
            let idx = call
                .args
                .iter()
                .position(|a| std::ptr::eq(a, *arg))
                .unwrap_or(0);
            self.w.push_str(&format!(" = __jux_byref{idx}; "));
        }
        // 4. Yield the call's result.
        self.w.push_str("__jux_ret }");
    }

    /// Emit ONE call argument through the full coercion ladder —
    /// interface/poly-base `Rc<dyn>` wrap, sealed upcast `.into()`,
    /// nullable `Some(…)` wrap, and the wrapper share-on-pass
    /// `.clone()` (§CR.4.1). Shared between the regular inline arg
    /// loop and the borrow-hoisted form (`let __jux_argN = …;`), so
    /// both produce identical values. The by-ref `&` prefix is NOT
    /// emitted here — it stays at the call slot (a hoisted temp is
    /// borrowed at the call, `x.m(&__jux_arg0)`).
    fn emit_call_arg_value(&mut self, call: &CallExpr, i: usize, arg: &Expr) {
        // `out <place>` argument (§M.4): pass `&mut <place>` — no value
        // coercion / share-clone. `emit_expr` handles the `Expr::Out` shape.
        if matches!(arg, Expr::Out(..)) {
            self.emit_expr(arg);
            return;
        }
        // `ref` parameter slot (§M.13): a `ref` argument ALIASES the
        // caller's object (handle clone); a plain value wraps into a
        // fresh shared object (the callee's writes then stay local —
        // pass a `ref` binding for write-through).
        if self.callee_param_is_shared_ref(&call.callee, i) {
            if let Expr::Path(qn) = arg {
                if qn.segments.len() == 1
                    && self.ref_locals.contains(&qn.segments[0].text)
                {
                    self.w.push_str(&qn.segments[0].text);
                    self.w.push_str(".clone()");
                    return;
                }
            }
            // A `ref` FIELD argument aliases through its handle.
            if let Expr::Field(ff) = arg {
                if self.field_decl_is_ref(&ff.object, &ff.field.text) {
                    let prev = std::mem::replace(&mut self.emitting_ref_handle, true);
                    self.emit_expr(arg);
                    self.emitting_ref_handle = prev;
                    return;
                }
            }
            self.w.push_str("std::rc::Rc::new(std::cell::RefCell::new(");
            let prev = std::mem::take(&mut self.emitting_format_arg);
            self.emit_expr(arg);
            self.emitting_format_arg = prev;
            // A PLACE argument keeps its value in the caller (the
            // fresh shared object holds a copy) — clone instead of
            // moving out of the local/field.
            if matches!(arg, Expr::Path(_) | Expr::Field(_) | Expr::Index(_)) {
                self.w.push_str(".clone()");
            }
            self.w.push_str("))");
            return;
        }
        // `weak` parameter slot (§M.14.3): downgrade the (strong) class argument
        // to a non-owning `Weak<RefCell<Class_Inner>>`. A class value lowers to
        // the newtype wrapper whose `.0` is the `Rc` we downgrade. (A weak param
        // can't be passed a bare weak source — tycheck E0456 forbids reading
        // one — so the argument is always a strong class value here.)
        if self.callee_param_is_weak(&call.callee, i) {
            self.w.push_str("std::rc::Rc::downgrade(&(");
            let prev = std::mem::take(&mut self.emitting_format_arg);
            self.emit_expr(arg);
            self.emitting_format_arg = prev;
            self.w.push_str(").0)");
            return;
        }
        // Interface-typed param slot: wrap a class value in `Rc<dyn
        // Trait>` / clone a dyn handle, before the sealed/nullable
        // paths (which never apply to an interface value slot).
        if let Some(pty) = self.callee_param_type(&call.callee, i) {
            if !matches!(
                self.iface_coercion_to(&pty, arg),
                crate::analysis::IfaceCoercion::None,
            ) {
                self.emit_expr_coerced_to_iface(&pty, arg);
                return;
            }
        }
        // A lambda flowing into a `() -> void`-typed parameter wraps
        // its EXPRESSION body as `{ expr; }` (value discarded) so the
        // closure types as `()` — `assertThrows(() -> divide(10, 0))`
        // per §TS.3. The flag is take-and-cleared by the lambda
        // emitters.
        if let Expr::Lambda(l) = arg {
            if matches!(l.body, juxc_ast::LambdaBody::Expr(_)) {
                if let Some(pt) = self.callee_param_type(&call.callee, i) {
                    if let Some(fs) = &pt.fn_shape {
                        let ret_is_void = fs
                            .return_type
                            .name
                            .segments
                            .last()
                            .map(|s| s.text == "void")
                            .unwrap_or(false)
                            && fs.return_type.fn_shape.is_none()
                            && fs.return_type.array_shape.is_none();
                        if ret_is_void {
                            self.lambda_void_target = true;
                        }
                    }
                }
            }
        }
        let nullable = self.callee_param_is_nullable(&call.callee, i);
        // Nested nullability (§7.10): an explicit `f<int?>(5)` whose param is a
        // bare generic `T`/`T?` makes the slot `Option<T>` with T = `int?`, i.e.
        // `Option<Option<isize>>`. A non-null inner-typed arg is one layer too
        // shallow — lift it into `T` with an extra `Some(…)`, INSIDE the param's
        // own `?` wrap: `T?` → `Some(Some(5))`, bare `T` → `Some(5)`. A `null`
        // arg / already-nullable arg is excluded by the predicate, so the outer
        // `None` / pass-through paths are unaffected. Gated on explicit generic
        // args, so the common call path never reaches here. (Flatten/Kotlin
        // collapse is impossible under Rust monomorphization; nest is the only
        // sound lowering and `== null` behaves identically either way.)
        if self.callee_arg_needs_nested_nullable_wrap(call, i, arg) {
            if nullable {
                self.w.push_str("Some(");
            }
            self.w.push_str("Some(");
            let prev = std::mem::take(&mut self.emitting_format_arg);
            self.emit_expr(arg);
            self.emitting_format_arg = prev;
            self.w.push(')');
            if nullable {
                self.w.push(')');
            }
            return;
        }
        let upcast = self.arg_needs_sealed_upcast(&call.callee, i, arg);
        if upcast {
            self.emit_arg_with_nullable_wrap(arg, nullable);
            self.w.push_str(".into()");
        } else {
            // Numeric coercion into a typed parameter: `f(v.len())` (uint -> int)
            // or `f(intExpr)` into a `long` param (widening). Cast the arg to the
            // param's numeric type when it differs; never narrows. Skipped under
            // the nullable wrap.
            let num_widen = if nullable {
                None
            } else {
                self.callee_param_type(&call.callee, i)
                    .as_ref()
                    .and_then(|t| self.type_ref_primitive(t))
                    .and_then(|target| self.numeric_widen_to(arg, target))
            };
            if num_widen.is_some() {
                self.w.push('(');
            }
            self.emit_arg_with_nullable_wrap(arg, nullable);
            // **Wrapper-class share-on-pass (§CR.4.1).** A wrapped
            // place passed as an argument hands the callee a SHARED
            // handle — append the cheap `Rc` refcount-bump clone so
            // the caller's binding stays live and both point at the
            // same `RefCell` (mutation through the param is observed
            // by the caller). Skipped under nullable/upcast wraps,
            // which never carry a bare wrapped place.
            if !nullable
                && (self.wrapper_value_needs_clone(arg) || self.record_place_needs_clone(arg))
            {
                // Wrapper place → shared-handle refcount bump; record place →
                // value-copy (§7.6). Both keep the caller's binding live and
                // avoid moving a place that is also the call receiver.
                self.w.push_str(".clone()");
            }
            if let Some(cast) = num_widen {
                self.w.push_str(" as ");
                self.w.push_str(cast);
                self.w.push(')');
            }
        }
    }

    /// True when `call` needs the **borrow-hoist** form — the callee is
    /// a method on a simple place (`x.m(…)` / `this.m(…)`) and some
    /// argument contains a call to a *mutating* method on that same
    /// place. Emitted inline, receiver and argument would hold two
    /// overlapping `&mut` borrows (rustc E0499/E0502 — two-phase
    /// borrows only cover shared argument borrows). Wrapper-class
    /// receivers are exempt: their methods take `&self` and mutate
    /// through the interior `RefCell`, so no conflict exists.
    fn call_needs_borrow_hoist(&self, call: &CallExpr) -> bool {
        // An argument that reads a wrapper FIELD (`n.bump(n.value)`,
        // `f(n, n.value)`) emits a `.0.borrow()` whose `Ref` guard is a
        // call-expression temporary, alive until the whole call
        // statement ends — so any callee that `borrow_mut`s the same
        // object panics `already borrowed` at runtime (§CR.4.1 /
        // RISK-3). This hazard is independent of the receiver's shape
        // (place, call result, free function): hoisting the args into
        // statement-scoped temps drops the guards before the call and
        // matches Java's args-before-call evaluation order.
        if call.args.iter().any(|a| self.expr_reads_wrapper_field(a)) {
            return true;
        }
        let Expr::Field(f) = call.callee.as_ref() else { return false };
        // The receiver as a dotted place path: `x` / `this` /
        // `h.item` / `this.a.b` (S7 — field-path receivers conflict
        // exactly like bare locals). Anything that isn't a pure
        // place (call results, indexes) bails out.
        let Some(root) = place_path_of(&f.object) else {
            return false;
        };
        let is_bare = root != "this" && !root.contains('.');
        if is_bare {
            // A class-named receiver is a static call (no instance
            // borrow).
            if self.lookup_class_by_bare_or_fqn(&root).is_some() {
                return false;
            }
            let recv_class = self
                .local_types
                .iter()
                .rev()
                .find_map(|s| s.get(&root))
                .and_then(|ty| match ty {
                    juxc_tycheck::Ty::User { name, .. } => {
                        Some(name.rsplit('.').next().unwrap_or(name).to_string())
                    }
                    _ => None,
                });
            if let Some(c) = recv_class {
                // A wrapper-class instance dispatches through `&self` —
                // no E0499 risk (the RefCell arg-guard hazard was
                // already handled by the top-of-fn check).
                if self.wrapper_classes.contains(&c) {
                    return false;
                }
            }
        } else if root == "this" {
            if let Some(enclosing) = &self.enclosing_class {
                // Inside a wrapper class's own method,
                // `this.m(this.bump())` dispatches through `&self` too.
                if self.wrapper_classes.contains(enclosing) {
                    return false;
                }
            }
        } else {
            // Field-path receiver (`h.item.set(…)`): exempt when the
            // FIELD's class is wrapper-shape (its methods take `&self`).
            if let Some(c) = self.receiver_class_bare(&f.object) {
                if self.wrapper_classes.contains(&c) {
                    return false;
                }
            }
        }
        call.args
            .iter()
            .any(|a| self.contains_mut_call_on(a, &root))
    }

    /// True when `e` contains a read that lowers to a wrapper-class
    /// `.0.borrow()` — a plain field access (or `this.<field>` inside a
    /// wrapper method) on a wrapper-class instance. Used by
    /// [`Self::call_needs_borrow_hoist`]: such a read in ARGUMENT
    /// position leaves its `Ref` guard alive across the call (Rust
    /// call-expression temporary scope), so a method that `borrow_mut`s
    /// the same object panics at runtime. Conservative: any wrapper
    /// field read triggers the (harmless) hoist, aliasing or not.
    fn expr_reads_wrapper_field(&self, e: &Expr) -> bool {
        match e {
            // A `ref` binding read (§M.13) clones out of its cell via a
            // borrow guard — same call-expression-temporary hazard as a
            // wrapper field read.
            Expr::Path(qn) => {
                qn.segments.len() == 1
                    && self.ref_locals.contains(&qn.segments[0].text)
            }
            Expr::Field(f) => {
                if let Some(c) = self.receiver_class_bare(&f.object) {
                    if self.wrapper_classes.contains(&c) {
                        return true;
                    }
                }
                if matches!(&*f.object, Expr::This(_)) {
                    if let Some(en) = &self.enclosing_class {
                        if self.wrapper_classes.contains(en) {
                            return true;
                        }
                    }
                }
                self.expr_reads_wrapper_field(&f.object)
            }
            Expr::Call(c) => {
                // A nested call's own internal guards drop when it
                // returns, but its receiver chain and arguments are
                // call-site temporaries of THIS statement — recurse.
                if let Expr::Field(cf) = &*c.callee {
                    if self.expr_reads_wrapper_field(&cf.object) {
                        return true;
                    }
                }
                c.args.iter().any(|a| self.expr_reads_wrapper_field(a))
            }
            Expr::Binary(b) => {
                self.expr_reads_wrapper_field(&b.left)
                    || self.expr_reads_wrapper_field(&b.right)
            }
            Expr::Unary(u) => self.expr_reads_wrapper_field(&u.operand),
            Expr::Cast(c) => self.expr_reads_wrapper_field(&c.value),
            Expr::TypeTest(t) => self.expr_reads_wrapper_field(&t.value),
            Expr::Index(i) => {
                self.expr_reads_wrapper_field(&i.array)
                    || self.expr_reads_wrapper_field(&i.index)
            }
            Expr::Elvis(el) => {
                self.expr_reads_wrapper_field(&el.value)
                    || self.expr_reads_wrapper_field(&el.fallback)
            }
            Expr::Ternary(t) => {
                self.expr_reads_wrapper_field(&t.condition)
                    || self.expr_reads_wrapper_field(&t.then_branch)
                    || self.expr_reads_wrapper_field(&t.else_branch)
            }
            Expr::NotNullAssert(inner, _)
            | Expr::Await(inner, _)
            | Expr::ErrorProp(inner, _)
            | Expr::Out(inner, _) => self.expr_reads_wrapper_field(inner),
            Expr::InterpString(s) => s.segments.iter().any(|seg| {
                matches!(seg, juxc_ast::InterpSegment::Expr(inner)
                    if self.expr_reads_wrapper_field(inner))
            }),
            Expr::TupleLit(elems, _) => {
                elems.iter().any(|el| self.expr_reads_wrapper_field(el))
            }
            _ => false,
        }
    }

    /// When the callee is `recv.method(...)` and `recv` is itself read through
    /// a wrapper `.0.borrow()` guard (a wrapper-class instance field), return
    /// the callee `Field`. Such a call holds the receiver's `borrow()` alive
    /// across `method(...)`; if `method` re-enters and mutates the same object
    /// (`a.bump()` → `b.ping(a)` → `a.bump()`), the re-entrant `borrow_mut()`
    /// panics `already borrowed`. The fix (see `emit_call_with_hoisted_receiver`)
    /// hoists the receiver into a temp so the guard drops before the call —
    /// upholding §CR.4.1's statement-scoped borrow discipline under re-entrancy.
    fn callee_receiver_reads_through_borrow<'c>(
        &self,
        callee: &'c Expr,
    ) -> Option<&'c juxc_ast::FieldExpr> {
        let Expr::Field(cf) = callee else { return None };
        // Look through a `!!` non-null assertion on the receiver: `this.inner!!.m()`
        // parses as `Field(NotNullAssert(Field(inner)), m)`. The `!!` doesn't change
        // that `.inner` is read through the wrapper's `.0.borrow()` guard, so the
        // same statement-scoped re-entrancy hazard applies and the receiver must
        // still be hoisted into a temp before `m(...)` runs.
        let recv = match cf.object.as_ref() {
            Expr::NotNullAssert(inner, _) => inner.as_ref(),
            other => other,
        };
        let Expr::Field(rf) = recv else { return None };
        if self.receiver_is_wrapper_class(&rf.object)
            && self
                .wrapper_field_parent_depth(&rf.object, &rf.field.text)
                .is_some()
        {
            Some(cf)
        } else {
            None
        }
    }

    /// Emit a `Stream.<ctor>` static (§18.6.4):
    ///
    /// - `Stream.of(a, b)`    → `crate::JuxStream::of(vec![a, b])`
    /// - `Stream.from(xs)`    → `crate::JuxStream::from(xs.clone())`
    ///   (snapshot — the source array stays usable, per §18.6.4)
    /// - `Stream.generate(async () -> …)` → the pull-driven producer:
    ///
    ///   ```text
    ///   crate::JuxStream::generate({
    ///       let c = c.clone();                  // closure-level rebinds
    ///       move || {
    ///           let c = c.clone();              // per-call rebinds
    ///           Box::pin(async move { <body> })
    ///               as futures::future::LocalBoxFuture<'static, _>
    ///       }
    ///   })
    ///   ```
    ///
    ///   Captures rebind TWICE: the outer clone hands the closure its
    ///   own handle (the caller keeps theirs — spawn's rule), and the
    ///   per-call clone hands each produced future its own (the
    ///   `async move` consumes its captures every pull). The lambda's
    ///   Jux return type is `T?`, so body emission runs under a
    ///   synthesized NULLABLE return type — `return k;` lifts to
    ///   `Some(k)`, `return null;` lowers to `None` — and a
    ///   value-tail body takes the same lift inline.
    fn emit_stream_static(&mut self, call: &CallExpr, method: &str) {
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        match method {
            "of" => {
                self.w.push_str("crate::JuxStream::of(vec![");
                for (i, arg) in call.args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.w.push_str("])");
            }
            "from" => {
                self.w.push_str("crate::JuxStream::from(");
                if let Some(arg) = call.args.first() {
                    self.emit_expr(arg);
                    self.w.push_str(".clone()");
                }
                self.w.push(')');
            }
            "generate" => {
                // Capture rebinds — same discovery as `spawn`.
                let mut rebinds: Vec<String> = Vec::new();
                if let Some(Expr::Lambda(l)) = call.args.first() {
                    let mut names: Vec<String> = Vec::new();
                    crate::exprs::collect_bare_names_in_lambda(l, &mut |n| {
                        if !names.iter().any(|x| x == n) {
                            names.push(n.to_string());
                        }
                    });
                    for name in names {
                        let known = self
                            .local_types
                            .iter()
                            .rev()
                            .find_map(|s| s.get(&name).cloned());
                        if let Some(ty) = known {
                            if !matches!(ty, juxc_tycheck::Ty::Primitive(_)) {
                                rebinds.push(name);
                            }
                        }
                    }
                }
                self.w.push_str("crate::JuxStream::generate({ ");
                for name in &rebinds {
                    self.w.push_str("let ");
                    self.w.push_str(name);
                    self.w.push_str(" = ");
                    self.w.push_str(name);
                    self.w.push_str(".clone(); ");
                }
                self.w.push_str("move || { ");
                for name in &rebinds {
                    self.w.push_str("let ");
                    self.w.push_str(name);
                    self.w.push_str(" = ");
                    self.w.push_str(name);
                    self.w.push_str(".clone(); ");
                }
                // Fully-qualified `Box` — a user `class Box` at the
                // crate root must not shadow std's (same rule as the
                // panic-hook splice).
                self.w.push_str("::std::boxed::Box::pin(async move { ");
                // The producer's Jux-level return type is `T?`: emit
                // the body under a synthesized nullable return so
                // `return` statements take the `Some(...)` lift.
                let saved_ret = self.current_return_type.take();
                self.current_return_type =
                    Some(juxc_ast::ReturnType::Type(synth_nullable_type_ref()));
                let prev_lam = self.in_lambda_body;
                self.in_lambda_body = false;
                match call.args.first() {
                    Some(Expr::Lambda(l)) => match &l.body {
                        juxc_ast::LambdaBody::Expr(e) => {
                            self.emit_stream_elem_value(e);
                        }
                        juxc_ast::LambdaBody::Block(b) => {
                            let (stmts, tail) = match b.statements.split_last() {
                                Some((juxc_ast::Stmt::Expr(t), rest)) => (rest, Some(t)),
                                _ => (&b.statements[..], None),
                            };
                            self.w.push('\n');
                            self.w.indent_inc();
                            for stmt in stmts {
                                self.w.emit_indent();
                                self.emit_stmt(stmt);
                            }
                            if let Some(tail) = tail {
                                self.w.emit_indent();
                                self.emit_stream_elem_value(tail);
                                self.w.push('\n');
                            }
                            self.w.indent_dec();
                            self.w.emit_indent();
                        }
                    },
                    Some(other) => {
                        // Non-lambda arg — tycheck complained; emit
                        // best-effort so the crate still parses.
                        self.emit_expr(other);
                    }
                    None => {
                        self.w.push_str("None");
                    }
                }
                self.in_lambda_body = prev_lam;
                self.current_return_type = saved_ret;
                self.w
                    .push_str(" }) as futures::future::LocalBoxFuture<'static, _> } })");
            }
            _ => {}
        }
        self.emitting_format_arg = prev;
    }

    /// Emit one stream-element VALUE position (`Stream.generate`'s
    /// expression tail): `null` → `None`, an already-nullable value
    /// flows through, anything else lifts into `Some(...)`.
    fn emit_stream_elem_value(&mut self, e: &Expr) {
        if crate::stmts::is_null_literal(e) {
            self.w.push_str("None");
        } else if self.expression_is_already_nullable(e) {
            self.emit_expr(e);
        } else {
            self.w.push_str("Some(");
            self.emit_expr(e);
            self.w.push(')');
        }
    }

    /// Emit a stdlib-method RECEIVER. Receivers are borrowed places:
    /// a plain field read here must not auto-`.clone()` (the call
    /// borrows the place in place — a clone would orphan `&mut`
    /// mutations (S7) and force needless copies of collection fields
    /// (S15)). Wrapper-borrow clone-outs still apply inside
    /// `emit_field`. The flag is take-and-cleared by `emit_field` /
    /// `emit_call`, so it never leaks past the receiver expression.
    fn emit_stdlib_receiver(&mut self, receiver: &Expr) {
        self.emitting_method_receiver = true;
        self.emit_expr(receiver);
        self.emitting_method_receiver = false;
    }

    /// True when evaluating `e` reads a field through a wrapper
    /// `.0.borrow()` guard (`s.items` on a wrapper-shape `s`, looking
    /// through `!!`). Used by the higher-order stdlib emissions
    /// (forEach / map / filter, S5) to decide whether the iterated
    /// collection must be snapshotted before the closure runs —
    /// holding the `Ref` guard across a closure that mutates the same
    /// object panics `already borrowed` at runtime.
    fn expr_reads_through_wrapper_borrow(&self, e: &Expr) -> bool {
        let e = match e {
            Expr::NotNullAssert(inner, _) => inner.as_ref(),
            other => other,
        };
        let Expr::Field(rf) = e else { return false };
        self.receiver_is_wrapper_class(&rf.object)
            && self
                .wrapper_field_parent_depth(&rf.object, &rf.field.text)
                .is_some()
    }

    /// Recursive walk: does `e` contain a call to a mutating method
    /// (per `user_mut_methods`) whose receiver is the same place path
    /// as `root` (`x.bump()` for root `x`, `this.bump()` for `this`,
    /// `h.item.bump()` for `h.item`)? Place paths are compared as
    /// dotted strings via [`place_path_of`].
    fn contains_mut_call_on(&self, e: &Expr, root: &str) -> bool {
        match e {
            Expr::Call(c) => {
                if let Expr::Field(f) = c.callee.as_ref() {
                    let on_root =
                        place_path_of(&f.object).as_deref() == Some(root);
                    if on_root && self.user_mut_methods.contains(&f.field.text) {
                        return true;
                    }
                }
                self.contains_mut_call_on(&c.callee, root)
                    || c.args.iter().any(|a| self.contains_mut_call_on(a, root))
            }
            Expr::Binary(b) => {
                self.contains_mut_call_on(&b.left, root)
                    || self.contains_mut_call_on(&b.right, root)
            }
            Expr::Unary(u) => self.contains_mut_call_on(&u.operand, root),
            Expr::Field(f) => self.contains_mut_call_on(&f.object, root),
            Expr::Index(ix) => {
                self.contains_mut_call_on(&ix.array, root)
                    || self.contains_mut_call_on(&ix.index, root)
            }
            Expr::Cast(c) => self.contains_mut_call_on(&c.value, root),
            _ => false,
        }
    }

    /// Emit `x.m(args…)` in the **borrow-hoisted** block form — every
    /// argument lands in a `let __jux_argN` temp (evaluated left to
    /// right, full coercion ladder via `emit_call_arg_value`), then the
    /// call reads only the temps:
    ///
    ///   { let __jux_arg0 = a.bump(); a.addTwice(__jux_arg0) }
    ///
    /// The argument's `&mut` borrow ends at its `;`, so the receiver
    /// borrow that follows is the only live one. Mirrors the regular
    /// path's callee flag discipline, turbofish, by-ref `&`, and the
    /// `pop()`-unwrap special.
    fn emit_call_with_hoisted_args(&mut self, call: &CallExpr) {
        self.w.push_str("{ ");
        let prev_args_fmt = self.emitting_format_arg;
        self.emitting_format_arg = false;
        // Hoist each argument into `__jux_arg{slot}` (coerced) BEFORE
        // the call. The BINDING order is what fixes evaluation order:
        // a re-ordered named call (§S.1.4) carries `eval_order` listing
        // slots in call-site LEXICAL order, so the side effects happen
        // left-to-right as written; an ordinary borrow-hoist has an
        // empty `eval_order` and binds positionally (already source
        // order). The final call always references the temps
        // POSITIONALLY, so the callee still gets its parameter slots.
        let bind_order: Vec<usize> = if call.eval_order.is_empty() {
            (0..call.args.len()).collect()
        } else {
            call.eval_order.clone()
        };
        for &i in &bind_order {
            let Some(arg) = call.args.get(i) else { continue };
            // C6: a `&mut T` foreign-collection arg must borrow the
            // caller's PLACE at the call slot, not a hoisted value temp
            // (a `&mut` into a temp would lose the caller's mutation).
            // Skip binding it here.
            if self.arg_is_byref(call, i) {
                continue;
            }
            self.w.push_str("let __jux_arg");
            self.w.push_str(&i.to_string());
            self.w.push_str(" = ");
            self.emit_call_arg_value(call, i, arg);
            self.w.push_str("; ");
        }
        self.emitting_format_arg = prev_args_fmt;
        let prev_callee = self.emitting_call_callee;
        self.emitting_call_callee = true;
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        let prev_cmp = std::mem::take(&mut self.emitting_comparison_operand);
        self.emit_expr(&call.callee);
        self.emitting_format_arg = prev_fmt;
        self.emitting_comparison_operand = prev_cmp;
        self.emitting_call_callee = prev_callee;
        if !call.explicit_generic_args.is_empty() {
            self.w.push_str("::<");
            for (i, ty) in call.explicit_generic_args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                if crate::analysis::is_jux_string_type(ty) {
                    self.w.push_str("String");
                } else {
                    self.emit_value_type_as_rust(ty);
                }
            }
            self.w.push('>');
        }
        self.w.push('(');
        for i in 0..call.args.len() {
            if i > 0 {
                self.w.push_str(", ");
            }
            if self.callee_param_is_ref(&call.callee, i) {
                self.w.push('&');
            }
            // C6 by-ref arg: borrow the original place directly (it was
            // NOT hoisted into a temp above).
            if self.arg_is_byref(call, i) {
                if let Some(arg) = call.args.get(i) {
                    self.emit_byref_arg(arg);
                }
                continue;
            }
            self.w.push_str("__jux_arg");
            self.w.push_str(&i.to_string());
        }
        self.w.push(')');
        if let Expr::Field(f) = &*call.callee {
            if f.field.text == "pop" && call.args.is_empty() {
                self.w.push_str(".unwrap()");
            }
        }
        self.w.push_str(" }");
    }

    /// Emit `recv.m(args…)` with the RECEIVER hoisted out of its `.0.borrow()`:
    ///
    ///   { let __jux_recv = <recv>; __jux_recv.m(args) }
    ///
    /// `recv` (a wrapper-class instance field) clones out of the borrow when
    /// bound, so the guard drops at the `;` — releasing it BEFORE `m(...)` runs.
    /// Without this, a re-entrant `m` that mutates the same object panics with
    /// `already borrowed` (§CR.4.1).
    ///
    /// `hoist_args` additionally binds every argument to a
    /// `__jux_arg<i>` temp between the receiver binding and the call —
    /// needed when an argument reads a wrapper field (its `Ref` guard
    /// is a call-expression temporary that would otherwise stay alive
    /// across `m(...)`, RISK-3). Order matches Java: receiver, then
    /// args left-to-right, then the call.
    fn emit_call_with_hoisted_receiver(
        &mut self,
        call: &CallExpr,
        callee: &juxc_ast::FieldExpr,
        hoist_args: bool,
    ) {
        // Parenthesize the whole hoist block: as a bare `{ … }` it would be
        // read as a statement when it lands in operand position (e.g. the LHS
        // of `{ … } == 0`, where Rust parses a leading `{` as a block stmt and
        // chokes on the `==`). `({ … })` is a valid expression everywhere,
        // including standalone-statement position.
        self.w.push_str("({ let __jux_recv = ");
        // Value position → the wrapper-field read appends `.clone()`, producing
        // an owned handle and dropping the `borrow()` temporary at the `;`.
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        let prev_cmp = std::mem::take(&mut self.emitting_comparison_operand);
        self.emit_expr(&callee.object);
        self.emitting_format_arg = prev_fmt;
        self.emitting_comparison_operand = prev_cmp;
        self.w.push_str("; ");
        if hoist_args {
            let prev_args_fmt = std::mem::take(&mut self.emitting_format_arg);
            for (i, arg) in call.args.iter().enumerate() {
                // C6 by-ref arg: borrow the place at the call slot, never
                // hoist it into a value temp (see emit_call_with_hoisted_args).
                if self.arg_is_byref(call, i) {
                    continue;
                }
                self.w.push_str("let __jux_arg");
                self.w.push_str(&i.to_string());
                self.w.push_str(" = ");
                self.emit_call_arg_value(call, i, arg);
                self.w.push_str("; ");
            }
            self.emitting_format_arg = prev_args_fmt;
        }
        self.w.push_str("__jux_recv.");
        self.w.push_str(&callee.field.text);
        if let Some(sfx) = self.pending_method_suffix.take() {
            self.w.push_str(&sfx);
        }
        if !call.explicit_generic_args.is_empty() {
            self.w.push_str("::<");
            for (i, ty) in call.explicit_generic_args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                if crate::analysis::is_jux_string_type(ty) {
                    self.w.push_str("String");
                } else {
                    self.emit_value_type_as_rust(ty);
                }
            }
            self.w.push('>');
        }
        self.w.push('(');
        let prev = std::mem::take(&mut self.emitting_format_arg);
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            if self.callee_param_is_ref(&call.callee, i) {
                self.w.push('&');
            }
            // C6 by-ref arg: borrow the place directly (not hoisted).
            if self.arg_is_byref(call, i) {
                self.emit_byref_arg(arg);
                continue;
            }
            if hoist_args {
                self.w.push_str("__jux_arg");
                self.w.push_str(&i.to_string());
            } else {
                self.emit_call_arg_value(call, i, arg);
            }
        }
        self.emitting_format_arg = prev;
        self.w.push(')');
        if callee.field.text == "pop" && call.args.is_empty() {
            self.w.push_str(".unwrap()");
        }
        self.w.push_str(" })");
    }

    /// Lower `obj?.method(args)` to
    /// `obj.as_ref().map(|__t| __t.method(args))`. Closure body
    /// emits with `emitting_format_arg=false` so any string-literal
    /// arg still self-coerces — same discipline as a regular
    /// `emit_call`'s args. The result type is `Option<ReturnType>`.
    pub(crate) fn emit_safe_method_call(
        &mut self,
        callee: &juxc_ast::FieldExpr,
        call: &CallExpr,
    ) {
        let needs_parens = !matches!(
            *callee.object,
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
        if needs_parens {
            self.w.push('(');
        }
        self.emit_expr(&callee.object);
        if needs_parens {
            self.w.push(')');
        }
        // `.and_then` flattens when the called method itself returns `T?`
        // (`a?.getC()` where `getC(): C?` yields `Option<C>`, not
        // `Option<Option<C>>` — otherwise a further `?.` chains off the wrong
        // type). `.map` for a non-nullable return. Stdlib methods stay `.map`
        // (their nullable lowering is handled inside the closure).
        if self.safe_method_returns_nullable(callee) {
            self.w.push_str(".as_ref().and_then(|__t| ");
        } else {
            self.w.push_str(".as_ref().map(|__t| ");
        }
        // **Route through the stdlib-method dispatch with `__t` as receiver**
        // (gap N7): a String/collection method on a nullable receiver
        // (`s?.length()`, `xs?.size()`) must map to its Rust equivalent
        // (`length` → `.chars().count() as isize`), not emit the raw Jux name.
        // `__t` is `&Underlying` from `as_ref()`; type it as the receiver's
        // underlying (non-nullable) type and synthesize a plain `__t.method(args)`
        // call for `try_emit_stdlib_method` to lower.
        let underlying = self
            .expr_types
            .get(&crate::exprs::expr_span_of(&callee.object))
            .map(|t| {
                let mut u = t;
                while let juxc_tycheck::Ty::Nullable(inner) = u {
                    u = inner;
                }
                u.clone()
            });
        let mut handled = false;
        if let Some(uty) = underlying {
            let synth = CallExpr {
                callee: Box::new(Expr::Field(juxc_ast::FieldExpr {
                    object: Box::new(Expr::Path(juxc_ast::QualifiedName {
                        segments: vec![juxc_ast::Ident {
                            text: "__t".to_string(),
                            span: callee.span,
                        }],
                        span: callee.span,
                    })),
                    field: callee.field.clone(),
                    safe: false,
                    span: callee.span,
                })),
                explicit_generic_args: Vec::new(),
                args: call.args.clone(),
                arg_names: vec![None; call.args.len()],
                eval_order: Vec::new(),
                span: call.span,
            };
            // Expose `__t`'s type for the duration of the synthetic dispatch
            // (the bare-receiver type lookup reads `local_types`).
            let mut scope = std::collections::HashMap::new();
            scope.insert("__t".to_string(), uty);
            self.local_types.push(scope);
            handled = self.try_emit_stdlib_method(&synth);
            self.local_types.pop();
        }
        if !handled {
            // Plain user-method (or unknown receiver): emit `__t.method(args)`.
            self.w.push_str("__t.");
            self.w.push_str(&callee.field.text);
            self.w.push('(');
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            for (i, arg) in call.args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.emit_expr(arg);
            }
            self.emitting_format_arg = prev;
            self.w.push(')');
        }
        self.w.push(')');
    }

    /// True iff the user method named by a `?.`-call returns a nullable `T?`,
    /// so [`Self::emit_safe_method_call`] flattens with `.and_then` instead of
    /// `.map`. Resolves the receiver's underlying (non-nullable) class from
    /// `expr_types` and walks the `extends` chain. Unknown / stdlib methods
    /// return false — their `.map` form is correct (any nullable stdlib
    /// lowering is produced inside the closure, already `Option`-shaped).
    fn safe_method_returns_nullable(&self, callee: &juxc_ast::FieldExpr) -> bool {
        // Resolve the receiver's class structurally (robust to unrecorded
        // intermediate safe-nav spans, e.g. `a?.b()?.c()`).
        let recvc = match self.safe_nav_member_class_bare(&callee.object) {
            Some(c) => c,
            None => return false,
        };
        let method = callee.field.text.as_str();
        let mut cursor = self.lookup_class_by_bare_or_fqn(&recvc);
        while let Some(sig) = cursor {
            if let Some(m) = sig.methods.get(method) {
                return matches!(
                    &m.return_type,
                    juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t)
                        if t.nullable
                );
            }
            cursor = sig
                .extends_fqn
                .as_deref()
                .and_then(|p| self.symbols.classes.get(p));
        }
        false
    }

    /// Lower a call to the built-in `print(…)` into the most natural Rust
    /// `println!` shape we can.
    ///
    /// Rules:
    /// - `print("literal")` → `println!("literal")`. We bake the string
    ///   directly into the format-string slot, doubling any `{` / `}` so
    ///   `println!`'s parser keeps its hands off them.
    /// - `print(expr)` (single non-literal arg) → `println!("{}", expr)`.
    /// - `print(a, b, …)` (multi-arg) → `println!("{} {} …", a, b, …)`
    ///   with one `{}` per argument. This is a placeholder shape until
    ///   `std.io.print` is properly specced.
    pub(crate) fn emit_print_call(&mut self, call: &CallExpr) {
        // Hot path: one string-literal argument. Inline it as the format.
        if call.args.len() == 1 {
            if let Expr::Literal(Literal::String(s)) = &call.args[0] {
                self.w.push_str("println!(");
                self.emit_rust_format_string_literal(s);
                self.w.push(')');
                return;
            }
            // Hot path: a string-concat chain (`"a" + b + "c"`) as
            // the sole argument. The naive lowering would be
            // `println!("{}", format!("{}{}{}", "a", b, "c"))` — a
            // wasted heap alloc for the intermediate `String`.
            // Inline the concat's operands directly as `println!`
            // args so the macro formats straight into the writer,
            // AND fold any literal operands into the template so
            // we end up with one `println!("hello, {}!", name)`
            // instead of `println!("{}{}{}", "hello, ", name, "!")`.
            if let Expr::Binary(b) = &call.args[0] {
                // Mirror the binary emitter's string-concat trigger:
                // literal-shape OR `Ty::String`-typed either side.
                // Either condition routes through the inline-print
                // path and the intermediate `format!` evaporates.
                let lhs_string = is_string_literal(&b.left)
                    || self.operand_is_string_typed_for_print(&b.left);
                let rhs_string = is_string_literal(&b.right)
                    || self.operand_is_string_typed_for_print(&b.right);
                if b.op == juxc_ast::BinaryOp::Add && (lhs_string || rhs_string) {
                    let mut operands: Vec<&Expr> = Vec::new();
                    flatten_concat(b, &mut operands);
                    let (template, runtime) =
                        fold_concat_for_print(&operands);
                    self.w.push_str("println!(\"");
                    self.w.push_str(&template);
                    self.w.push('"');
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = true;
                    for op in &runtime {
                        self.w.push_str(", ");
                        self.emit_format_arg(op);
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
            }
            // Hot path: one interpolated-string argument. Inline its
            // segments directly into the println! call instead of
            // emitting `println!("{}", format!("…", args))`. Same
            // shape format!() would produce, one less call frame.
            if let Expr::InterpString(s) = &call.args[0] {
                self.w.push_str("println!(\"");
                let mut bare_args: Vec<&juxc_ast::Ident> = Vec::new();
                let mut expr_args: Vec<&Expr> = Vec::new();
                let mut arg_order: Vec<ArgRef> = Vec::new();
                for seg in &s.segments {
                    match seg {
                        juxc_ast::InterpSegment::Literal(text) => {
                            self.emit_interp_literal_chunk(text);
                        }
                        juxc_ast::InterpSegment::Bare(ident) => {
                            self.w.push_str("{}");
                            bare_args.push(ident);
                            arg_order.push(ArgRef::Bare(bare_args.len() - 1));
                        }
                        juxc_ast::InterpSegment::Expr(expr) => {
                            self.w.push_str("{}");
                            expr_args.push(expr);
                            arg_order.push(ArgRef::Expr(expr_args.len() - 1));
                        }
                    }
                }
                self.w.push('"');
                // `println!` borrows its args, so nested string
                // literals stay `&str` (saves an alloc per literal).
                // Nullable args are wrapped in `JuxOpt(&v)` so
                // `Display` works — `Some(v)` prints `v`, `None`
                // prints `"null"`.
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = true;
                for arg_ref in &arg_order {
                    self.w.push_str(", ");
                    match arg_ref {
                        ArgRef::Bare(i) => {
                            // Bare-ident interp `$name` — synthesize
                            // a Path expression so `emit_format_arg`
                            // can run its nullable-shape check.
                            let qn = juxc_ast::QualifiedName {
                                segments: vec![bare_args[*i].clone()],
                                span: bare_args[*i].span,
                            };
                            let synth = Expr::Path(qn);
                            self.emit_format_arg(&synth);
                        }
                        ArgRef::Expr(i) => self.emit_format_arg(expr_args[*i]),
                    }
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                return;
            }
        }
        // General path: one `{}` placeholder per arg, then the args.
        self.w.push_str("println!(\"");
        for i in 0..call.args.len() {
            if i > 0 {
                self.w.push(' ');
            }
            self.w.push_str("{}");
        }
        self.w.push('"');
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = true;
        for arg in &call.args {
            self.w.push_str(", ");
            self.emit_format_arg(arg);
        }
        self.emitting_format_arg = prev;
        self.w.push(')');
    }

    /// Stdlib method dispatch — rewrites Jux's spec-level method
    /// names (`add`, `isEmpty`, `toUpperCase`, …) on arrays and
    /// `String` receivers into the matching Rust shape.
    ///
    /// Returns `true` when this path handled the call (so the
    /// surrounding `emit_call` should return immediately). Returns
    /// `false` for any call shape this method doesn't recognize —
    /// receiver type unknown, method name unknown, receiver isn't
    /// a method call's Field-callee, etc. — and lets the regular
    /// emit path proceed.
    ///
    /// The receiver's type comes from `expr_types`, the tycheck
    /// inference map. The dispatch is best-effort: if the
    /// expression hasn't been typed (e.g. inside a lambda body
    /// where inference doesn't run), the helper falls through and
    /// the user gets either the regular emit (which may compile
    /// if the method name happens to be a Vec/String method) or a
    /// clear rustc error pointing at the offending site.
    pub(crate) fn try_emit_stdlib_method(&mut self, call: &CallExpr) -> bool {
        // Must be a `receiver.method(args)` shape.
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        // A safe-navigation call (`x?.method()`) must go through the dedicated
        // `?.` lowering (`emit_safe_method_call`), which wraps the receiver in
        // `.as_ref().map(|__t| …)` and re-dispatches the method on the unwrapped
        // `__t`. Handling a stdlib method here would emit it directly on the
        // `Option` (`opt.chars()…`), silently dropping the null check.
        if f.safe {
            return false;
        }
        let method = f.field.text.as_str();
        // AUTO-PROPERTY receiver (S3): `s.Items.add(3)` where `Items`
        // is `{ get; set; }`. The getter returns a CLONE of the
        // backing field, so dispatching on the property read would
        // both miss the stdlib mapping (the read's type isn't always
        // recorded) and silently mutate a temporary. Rewrite the
        // receiver to the backing field (`s.__prop_Items`) — a real
        // field of the class — and re-dispatch: the regular
        // wrapped-field machinery (borrow_mut upgrade, arg prehoist,
        // N1) then applies. Computed properties keep the getter-call
        // path: there is no storage to mutate through.
        if let Expr::Field(pf) = &*f.object {
            if !pf.safe {
                let backing = self
                    .property_on_receiver(&pf.object, &pf.field.text)
                    .filter(|p| p.has_backing_field && !p.is_static)
                    .map(|p| juxc_ast::desugar_backing_field_name(&p.name.text));
                if let Some(backing_name) = backing {
                    let backing_recv = Expr::Field(juxc_ast::FieldExpr {
                        object: pf.object.clone(),
                        field: juxc_ast::Ident {
                            text: backing_name,
                            span: pf.field.span,
                        },
                        safe: false,
                        span: pf.span,
                    });
                    let rewritten = CallExpr {
                        callee: Box::new(Expr::Field(juxc_ast::FieldExpr {
                            object: Box::new(backing_recv),
                            field: f.field.clone(),
                            safe: false,
                            span: f.span,
                        })),
                        ..call.clone()
                    };
                    return self.try_emit_stdlib_method(&rewritten);
                }
            }
        }
        // Receiver-type lookup. Three paths:
        //   1. `local_types` map for Path receivers — keyed by
        //      name, immune to span collisions inside interp
        //      strings.
        //   2. `expr_types` map (the normal route — typed by the
        //      inference pass for paths, calls, fields).
        //   3. Literal short-circuit — literal expressions have
        //      `Span::DUMMY`, so they never appear in the map. We
        //      special-case string and array literals here.
        let recv_span = crate::exprs::expr_span_of(&f.object);
        let recv_ty_from_locals: Option<juxc_tycheck::Ty> =
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1 {
                    let bare = qn.segments[0].text.as_str();
                    self.local_types
                        .iter()
                        .rev()
                        .find_map(|scope| scope.get(bare).cloned())
                } else {
                    None
                }
            } else {
                None
            };
        let recv_ty = recv_ty_from_locals
            .or_else(|| self.expr_types.get(&recv_span).cloned())
            .or_else(|| match &*f.object {
                Expr::Literal(juxc_ast::Literal::String(_)) => {
                    Some(juxc_tycheck::Ty::String)
                }
                // Numeric / char receivers built purely from literals
                // (`255.toHex()`, `(0.0 / 0.0).isNaN()`) — literals
                // carry `Span::DUMMY`, and a binary over two literals
                // JOINS those into another DUMMY span, so neither has
                // an `expr_types` entry. Type them structurally so
                // §K.11 intrinsics still dispatch.
                e => literal_numeric_ty(e).map(juxc_tycheck::Ty::Primitive),
            })
            // An `Unknown` entry is as good as no entry — let the
            // declared-type fallback below take over (property reads
            // are often recorded as `Unknown`).
            .filter(|t| !matches!(t, juxc_tycheck::Ty::Unknown))
            .or_else(|| {
                // FIELD-read receiver fallback (S3): `this`-rooted
                // reads and property BACKING fields (`s.__prop_Items`,
                // synthesized by the rewrite above) aren't always in
                // `expr_types` — resolve the field's DECLARED type
                // through the receiver's class chain instead.
                let Expr::Field(rf) = &*f.object else {
                    return None;
                };
                let class = if matches!(&*rf.object, Expr::This(_)) {
                    self.enclosing_class.clone()
                } else {
                    self.receiver_class_bare(&rf.object)
                };
                class.and_then(|c| {
                    self.lookup_class_field_ty_in_chain(&c, &rf.field.text)
                })
            });
        let Some(recv_ty) = recv_ty else {
            return false;
        };
        // `ArrayList<T>` normalizes to `Ty::Array` in most paths
        // (tycheck's ty_from_ref shortcut), but a few — e.g. property
        // getter return types — can surface it under its user-type
        // name; accept both spellings (S3).
        let is_array = matches!(&recv_ty, juxc_tycheck::Ty::Array { .. })
            || matches!(
                &recv_ty,
                juxc_tycheck::Ty::User { name, .. }
                    if name.rsplit('.').next().unwrap_or(name) == "ArrayList"
            );
        let is_string =
            matches!(&recv_ty, juxc_tycheck::Ty::String);
        // The Java-style facade lives under `jux.std.collections`; the
        // `rust.std.*` collections share the bare names (`HashMap`/`HashSet`)
        // but carry the real Rust API via their generated stub, so they must
        // NOT take the facade lowering (which would mis-apply `put`/`get`+unwrap
        // etc.). Gate the facade detection on the `jux.std` FQN so a `rust.std`
        // collection falls through to the generic stub-method path.
        let is_map = matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if name.rsplit('.').next().unwrap_or(name) == "HashMap"
                    && name.starts_with("jux.std")
        );
        let is_set = matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if name.rsplit('.').next().unwrap_or(name) == "HashSet"
                    && name.starts_with("jux.std")
        );
        let is_deque = matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if name.rsplit('.').next().unwrap_or(name) == "Deque"
                    && name.starts_with("jux.std")
        );
        // `Instant` elapsed readings (jux.std.time) — the receiver is
        // a Copy `std::time::Instant` value.
        if matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if name.rsplit('.').next().unwrap_or(name) == "Instant"
        ) {
            let suffix = match method {
                "elapsedMs" => ".elapsed().as_millis() as i64",
                "elapsedNanos" => ".elapsed().as_nanos() as i64",
                _ => return false,
            };
            self.emit_expr(&f.object);
            self.w.push_str(suffix);
            return true;
        }
        // `AtomicInt` / `AtomicLong` (§S.6.2) — Arc<Atomic*> handles.
        // The no-order overloads default to SeqCst; explicit orders
        // pass the Jux `MemoryOrder` through the emitted
        // `__jux_order` adapter. `fetch*` return the PREVIOUS value.
        if matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if matches!(
                    name.rsplit('.').next().unwrap_or(name),
                    "AtomicInt" | "AtomicLong"
                )
        ) {
            let rust = match method {
                "load" => "load",
                "store" => "store",
                "fetchAdd" => "fetch_add",
                "fetchSub" => "fetch_sub",
                "fetchAnd" => "fetch_and",
                "fetchOr" => "fetch_or",
                "fetchXor" => "fetch_xor",
                _ => return false,
            };
            // The ordering is the LAST argument when the overload
            // carries one: load(order) has 1 arg, store/fetch*(v,
            // order) have 2.
            let order_arg = match (method, call.args.len()) {
                ("load", 1) => call.args.first(),
                (_, 2) => call.args.get(1),
                _ => None,
            };
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            self.emit_expr(&f.object);
            self.w.push('.');
            self.w.push_str(rust);
            self.w.push('(');
            // The value operand (store / fetch* first arg).
            if method != "load" {
                if let Some(value) = call.args.first() {
                    self.emit_expr(value);
                    self.w.push_str(", ");
                }
            }
            match order_arg {
                Some(order) => {
                    self.w.push_str("crate::__jux_order(");
                    self.emit_expr(order);
                    self.w.push(')');
                }
                None => self.w.push_str("std::sync::atomic::Ordering::SeqCst"),
            }
            self.w.push(')');
            self.emitting_format_arg = prev;
            return true;
        }
        // Numeric / char intrinsics (§K.11) — Primitive-typed
        // receivers get their own dispatch table.
        if let juxc_tycheck::Ty::Primitive(prim) = &recv_ty {
            return self.emit_numeric_stdlib_method(call, method, *prim);
        }
        // **Raw `rust.std.Vec<T>` mutating call on a wrapper field.** A bare
        // Vec has no facade table (its methods passthrough to Rust via the
        // generic call path), so it isn't covered by the is_array/is_map/…
        // gate below. But when the receiver is a Vec FIELD of a shared-
        // reference class and the method mutates, the generic path's clone-
        // hoist would push onto a discarded copy (and need a `mut` binding).
        // Route those through the borrow_mut() collection path instead;
        // everything else (a plain local Vec) falls through to the generic
        // call, where the direct `vec.push(v)` already works.
        // The rust.std collections (Vec/VecDeque/HashMap/HashSet/BTreeMap/
        // BTreeSet) all share this shape: no facade table, methods passthrough
        // to Rust verbatim. When the receiver is such a collection FIELD of a
        // shared-reference (wrapped) class AND the method mutates, route it
        // through the borrow_mut() path so the write lands in the real cell —
        // the generic clone-hoist would mutate a discarded copy (and need a
        // `mut` binding → rustc E0596). A plain local collection doesn't read
        // through a borrow, so it's untouched and falls to the generic call.
        if let juxc_tycheck::Ty::User { name, .. } = &recv_ty {
            let bare = name.rsplit('.').next().unwrap_or(name);
            let is_rust_std_coll = name.starts_with("rust.std")
                && matches!(
                    bare,
                    "Vec" | "VecDeque" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet",
                );
            if is_rust_std_coll
                && self.collection_method_mutates(&recv_ty, method)
                && self.callee_receiver_reads_through_borrow(&call.callee).is_some()
            {
                return self.emit_mut_collection_method(call, method, &recv_ty);
            }
        }
        let is_vec = matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if name.rsplit('.').next().unwrap_or(name) == "Vec"
        );
        if is_vec {
            // Non-mutating or non-wrapper Vec call → generic passthrough.
            return false;
        }
        if !is_array && !is_string && !is_map && !is_set && !is_deque {
            // A `rust.std` collection's ref-returning Option getter (`get`,
            // `first`/`last`, `front`/`back`) yields `Option<&V>` in Rust, but
            // Jux's `T?` is owned `Option<V>` — bindgen drops the `&` from the
            // stub return type, so the generic call path can't know. Clone the
            // borrowed value out so the nullable lines up. (Only these read
            // getters; the owned-`Option` methods like `pop`/`pop_front` already
            // line up.)
            if let juxc_tycheck::Ty::User { name, .. } = &recv_ty {
                let bare = name.rsplit('.').next().unwrap_or(name);
                let is_rust_std_coll = name.starts_with("rust.std")
                    && matches!(
                        bare,
                        "Vec" | "HashMap" | "HashSet" | "VecDeque" | "BTreeMap" | "BTreeSet",
                    );
                // `keys()` / `values()` on a `rust.std` map yield an iterator of
                // `&K` / `&V` in Rust. Clone to an owned `Vec<K>` / `Vec<V>` (as
                // the Jux facade does), so `for (k : m.keys()) m.get(k)` iterates
                // owned keys and `get`'s `&k` is `&K`, not `&&K` (rustc E0277
                // `K: Borrow<&K>`).
                if is_rust_std_coll
                    && matches!(method, "keys" | "values")
                    && call.args.is_empty()
                {
                    self.emit_stdlib_receiver(&f.object);
                    self.w.push('.');
                    self.w.push_str(method);
                    self.w.push_str("().cloned().collect::<Vec<_>>()");
                    return true;
                }
                if is_rust_std_coll
                    && matches!(method, "get" | "first" | "last" | "front" | "back")
                {
                    self.w.push('(');
                    self.emit_stdlib_receiver(&f.object);
                    self.w.push('.');
                    self.w.push_str(method);
                    self.w.push('(');
                    // `get(k)` borrows its key (`&k`); the no-arg getters take none.
                    if method == "get" && !call.args.is_empty() {
                        self.w.push_str("&(");
                        self.emit_call_args(call);
                        self.w.push(')');
                    } else {
                        self.emit_call_args(call);
                    }
                    self.w.push_str(")).cloned()");
                    return true;
                }
            }
            return false;
        }
        // **Gap N1: mutating collection method on a wrapped-class field.**
        // `this.items.add(v)` where `items` is a collection field of a
        // shared-reference class reads the field through `borrow_mut()` and
        // hoists args ahead of that borrow — see `emit_mut_collection_method`.
        // (String has no mutating-in-place methods on this path, so it's
        // excluded by `collection_method_mutates`.)
        if self.collection_method_mutates(&recv_ty, method)
            && self.callee_receiver_reads_through_borrow(&call.callee).is_some()
        {
            return self.emit_mut_collection_method(call, method, &recv_ty);
        }
        if is_array {
            return self.emit_array_stdlib_method(call, method);
        }
        if is_string {
            return self.emit_string_stdlib_method(call, method);
        }
        if is_map {
            return self.emit_map_stdlib_method(call, method);
        }
        if is_set {
            return self.emit_set_stdlib_method(call, method);
        }
        if is_deque {
            return self.emit_deque_stdlib_method(call, method);
        }
        false
    }

    /// Emit a **raw mutating method on a `rust.std.Vec<T>` receiver** —
    /// `this.data.push(v)` where `data` is a `Vec<T>` field of a wrapper
    /// class. Unlike the Jux collection facades (Deque/HashMap/…), a raw Vec
    /// has no name-translation table — its methods (`push`, `pop`, `insert`,
    /// …) are passed straight through to Rust. The point of this path is the
    /// **receiver**: it's emitted with `emitting_out_place`/`emitting_lvalue`
    /// set (by the caller), so a wrapper field reads through
    /// `self.0.borrow_mut().data` and the mutation lands in the real cell —
    /// not a clone-hoisted copy (which would also need a `mut` binding and
    /// silently drop the write). Args are pre-hoisted by the caller.
    fn emit_vec_raw_mut_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        self.emit_stdlib_receiver(&f.object);
        self.w.push('.');
        self.w.push_str(method);
        self.w.push('(');
        self.emit_call_args(call);
        self.w.push(')');
        // Same `Vec::pop -> .unwrap()` Phase-1 bridge as the generic call tail
        // (`emit_call`): Rust's `Vec::pop` yields `Option<T>` but a Jux `pop()`
        // returns `T`. This path is EXCLUSIVELY raw-Vec mutating methods, so the
        // receiver is always a real Vec — no user-`pop()` ambiguity to guard.
        if method == "pop" && call.args.is_empty() {
            self.w.push_str(".unwrap()");
        }
        true
    }

    /// Emit the Rust equivalent of a Jux `Deque<T>` method call —
    /// lowered onto `std::collections::VecDeque<T>`. The remove/peek
    /// forms return `T?` in Jux, which is exactly the `Option<T>` the
    /// Rust methods produce (peeks clone the element out).
    fn emit_deque_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let receiver = &*f.object;
        match method {
            "addFirst" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".push_front(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            "addLast" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".push_back(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            "removeFirst" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".pop_front()");
                true
            }
            "removeLast" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".pop_back()");
                true
            }
            "peekFirst" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".front().cloned()");
                true
            }
            "peekLast" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".back().cloned()");
                true
            }
            "contains" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".contains(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            "size" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            "isEmpty" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            "clear" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".clear()");
                true
            }
            _ => false,
        }
    }

    /// Emit the Rust equivalent of a Jux `HashMap<K, V>` method
    /// call. Returns `true` when the method was handled.
    fn emit_map_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let receiver = &*f.object;
        match method {
            "put" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".insert(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            "get" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".get(&(");
                self.emit_call_args(call);
                self.w.push_str(")).cloned().unwrap()");
                true
            }
            "contains" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".contains_key(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            "remove" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".remove(&(");
                self.emit_call_args(call);
                self.w.push_str(")).unwrap()");
                true
            }
            "size" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            "isEmpty" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            "clear" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".clear()");
                true
            }
            "keys" => {
                self.emit_stdlib_receiver(receiver);
                self.w
                    .push_str(".keys().cloned().collect::<Vec<_>>()");
                true
            }
            "values" => {
                self.emit_stdlib_receiver(receiver);
                self.w
                    .push_str(".values().cloned().collect::<Vec<_>>()");
                true
            }
            _ => false,
        }
    }

    /// Emit the Rust equivalent of a Jux `HashSet<T>` method call.
    fn emit_set_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let receiver = &*f.object;
        match method {
            "add" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".insert(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            "contains" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".contains(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            "remove" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".remove(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            "size" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            "isEmpty" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            "clear" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".clear()");
                true
            }
            _ => false,
        }
    }

    /// Emit a stdlib-collection method **receiver** that the method will
    /// mutate (`add`/`push`, `set`, `remove`, `put`, `clear`, …). When the
    /// receiver is a field of a shared-reference (wrapped) class, the field
    /// must be read through the **mutable** interior borrow
    /// (`self.0.borrow_mut().items`) — the default read path takes
    /// `self.0.borrow().items`, an immutable `Ref`, so `.push()`/`.insert()`
    /// fail to compile (E0596). Setting both `emitting_out_place` (selects
    /// `borrow_mut()` in `emit_field`) and `emitting_lvalue` (suppresses the
    /// auto-`.clone()` that would otherwise mutate a throwaway copy) gives the
    /// exact `self.0.borrow_mut().items` shape. A non-wrapper receiver (a plain
    /// local `Vec`) is a `Path`, never reaches `emit_field`, so the flags are
    /// harmless there.
    // Currently unreferenced (the mutating-collection paths emit the receiver
    // inline via the out-place flags). Retained as the canonical helper for that
    // shape; `#[allow(dead_code)]` rather than deleting in case a path wires it.
    #[allow(dead_code)]
    fn emit_mut_collection_receiver(&mut self, receiver: &Expr) {
        let prev_out = self.emitting_out_place;
        let prev_lv = self.emitting_lvalue;
        self.emitting_out_place = true;
        self.emitting_lvalue = true;
        self.emit_stdlib_receiver(receiver);
        self.emitting_out_place = prev_out;
        self.emitting_lvalue = prev_lv;
    }

    /// True when `method` **mutates** its stdlib-collection receiver — the
    /// methods that need `&mut` on the underlying `Vec`/`HashMap`/`HashSet`/
    /// `VecDeque`. Read-only methods (`size`, `get`, `contains`, `keys`, …)
    /// answer `false`. Drives the gap-N1 borrow_mut routing.
    fn collection_method_mutates(&self, recv_ty: &juxc_tycheck::Ty, method: &str) -> bool {
        match recv_ty {
            juxc_tycheck::Ty::Array { .. } => matches!(
                method,
                "add" | "set" | "remove" | "insert" | "clear" | "reverse" | "sort"
            ),
            juxc_tycheck::Ty::User { name, .. } => {
                match name.rsplit('.').next().unwrap_or(name) {
                    // `put`/`add`/`addFirst`/… are the legacy jux.std facade
                    // names; `insert`/`push_back`/… are the rust.std verbatim
                    // names. Both are accepted on the matching bare type — the
                    // facade never sees a rust.std name and vice-versa.
                    "HashMap" | "BTreeMap" => matches!(
                        method,
                        "put" | "insert" | "remove" | "clear" | "extend" | "retain" | "append"
                    ),
                    "HashSet" | "BTreeSet" => matches!(
                        method,
                        "add" | "insert" | "remove" | "clear" | "extend" | "retain"
                    ),
                    "Deque" => matches!(
                        method,
                        "addFirst" | "addLast" | "removeFirst" | "removeLast" | "clear"
                    ),
                    "VecDeque" => matches!(
                        method,
                        "push_back"
                            | "push_front"
                            | "pop_back"
                            | "pop_front"
                            | "clear"
                            | "insert"
                            | "remove"
                            | "truncate"
                            | "retain"
                            | "extend"
                            | "append"
                            | "rotate_left"
                            | "rotate_right"
                            | "resize"
                    ),
                    // Raw `rust.std.Vec<T>` — its mutating Rust methods are
                    // emitted as direct passthrough calls (no builtin table),
                    // so a wrapper-field receiver must read through
                    // `borrow_mut()` rather than the clone-hoist (which would
                    // push onto a discarded copy + need a `mut` binding).
                    "Vec" => matches!(
                        method,
                        "push"
                            | "pop"
                            | "clear"
                            | "insert"
                            | "remove"
                            | "truncate"
                            | "retain"
                            | "extend"
                            | "append"
                            | "swap"
                            | "swap_remove"
                            | "sort"
                            | "reverse"
                            | "dedup"
                            | "resize"
                    ),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Emit a **mutating** stdlib-collection method whose receiver is a field
    /// of a shared-reference (wrapped) class — `this.items.add(v)` (gap N1).
    /// Two defects are fixed together:
    ///   - **A (mutability):** the field is read through `borrow_mut()` (via
    ///     the receiver-mut flags) so the mutation lands in the real cell, not
    ///     a temporary `Ref` (would be rustc E0596).
    ///   - **B (re-entrancy):** every argument is hoisted into a temp BEFORE
    ///     the receiver borrow is taken, so an argument that re-enters the same
    ///     object (`this.items.add(this.next())`) runs its own short-lived
    ///     borrow first instead of colliding with the open collection borrow
    ///     (would be a runtime `already borrowed` panic).
    /// The temps carry the full element coercion ladder; the delegated per-kind
    /// emitter then reads bare temps (`collection_args_prehoisted`).
    fn emit_mut_collection_method(
        &mut self,
        call: &CallExpr,
        method: &str,
        recv_ty: &juxc_tycheck::Ty,
    ) -> bool {
        // Delegate to the per-kind emitter with the receiver-mut flags set.
        let dispatch = |this: &mut Self, c: &CallExpr| -> bool {
            let prev_out = this.emitting_out_place;
            let prev_lv = this.emitting_lvalue;
            let prev_hoist = this.collection_args_prehoisted;
            this.emitting_out_place = true;
            this.emitting_lvalue = true;
            this.collection_args_prehoisted = true;
            let handled = match recv_ty {
                juxc_tycheck::Ty::Array { .. } => this.emit_array_stdlib_method(c, method),
                juxc_tycheck::Ty::User { name, .. } => {
                    // rust.std collections carry the real Rust method name
                    // already (`insert`/`push_back`/…), so they emit as a raw
                    // passthrough on the `borrow_mut()` receiver — same handler
                    // as a raw Vec. Only the legacy jux.std facade needs the
                    // per-kind name-translation tables (`put`→`insert`, …).
                    if name.starts_with("rust.std") {
                        this.emit_vec_raw_mut_method(c, method)
                    } else {
                        match name.rsplit('.').next().unwrap_or(name) {
                            "HashMap" => this.emit_map_stdlib_method(c, method),
                            "HashSet" => this.emit_set_stdlib_method(c, method),
                            "Deque" => this.emit_deque_stdlib_method(c, method),
                            // Raw Vec mutating method — passthrough call on the
                            // `borrow_mut()` receiver (flags already set by the
                            // surrounding dispatch closure).
                            "Vec" => this.emit_vec_raw_mut_method(c, method),
                            _ => false,
                        }
                    }
                }
                _ => false,
            };
            this.emitting_out_place = prev_out;
            this.emitting_lvalue = prev_lv;
            this.collection_args_prehoisted = prev_hoist;
            handled
        };
        // No args → no re-entrancy / coercion to hoist; emit in place.
        if call.args.is_empty() {
            return dispatch(self, call);
        }
        self.w.push_str("{ ");
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        for (i, arg) in call.args.iter().enumerate() {
            self.w.push_str("let __jux_carg");
            self.w.push_str(&i.to_string());
            self.w.push_str(" = ");
            self.emit_collection_arg(call, i, arg);
            self.w.push_str("; ");
        }
        self.emitting_format_arg = prev_fmt;
        // Synthetic call: same callee/receiver, args replaced by the temps.
        let temp_args: Vec<Expr> = (0..call.args.len())
            .map(|i| {
                Expr::Path(juxc_ast::QualifiedName {
                    segments: vec![juxc_ast::Ident {
                        text: format!("__jux_carg{i}"),
                        span: call.span,
                    }],
                    span: call.span,
                })
            })
            .collect();
        let temp_call = CallExpr {
            callee: call.callee.clone(),
            explicit_generic_args: call.explicit_generic_args.clone(),
            args: temp_args,
            arg_names: vec![None; call.args.len()],
            eval_order: Vec::new(),
            span: call.span,
        };
        let handled = dispatch(self, &temp_call);
        self.w.push_str(" }");
        handled
    }

    /// Emit the Rust equivalent of a Jux `List<T>` / array method
    /// call. Returns `true` when the method was handled.
    fn emit_array_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        // Helpers — emit the receiver with proper grouping, and
        // the comma-separated arg list with format-arg flag
        // cleared so nested string literals self-coerce.
        let receiver = &*f.object;
        match method {
            // `xs.add(v)` → `xs.push(v)` — Java/spec name vs Rust.
            "add" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".push(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            // `xs.size()` → `xs.len() as isize` — same as `.length`
            // field shape but used as a method.
            "size" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            // `xs.isEmpty()` → `xs.is_empty()` — pure rename.
            "isEmpty" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            // `xs.contains(v)` → `xs.contains(&v)` — Rust needs &T.
            "contains" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".contains(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            // `xs.indexOf(v)` → linear scan returning -1 on miss.
            // Matches Java's API contract.
            "indexOf" => {
                self.w.push_str("(");
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".iter().position(|__e| *__e == ");
                self.emit_call_args(call);
                self.w.push_str(").map(|__i| __i as isize).unwrap_or(-1))");
                true
            }
            // `xs.get(i)` → `xs[i as usize].clone()` — clone so the
            // value-shape consistent with index-access elsewhere.
            "get" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str("[(");
                self.emit_call_args(call);
                self.w.push_str(") as usize].clone()");
                true
            }
            // `xs.set(i, v)` → block expression that mutates in
            // place, returning the old value (consistent with
            // Java's List.set contract).
            "set" => {
                self.w.push_str("{ let __i = (");
                // Args are (index, value). Emit index first then value.
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(idx) = call.args.first() {
                    self.emit_expr(idx);
                }
                self.w.push_str(") as usize; let __old = ");
                self.emit_stdlib_receiver(receiver);
                self.w.push_str("[__i].clone(); ");
                self.emit_stdlib_receiver(receiver);
                self.w.push_str("[__i] = ");
                if let Some(val) = call.args.get(1) {
                    self.emit_expr(val);
                }
                self.emitting_format_arg = prev;
                self.w.push_str("; __old }");
                true
            }
            // `xs.first()` / `xs.last()` — indexed access with clone.
            "first" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str("[0].clone()");
                true
            }
            "last" => {
                self.w.push('(');
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".last().cloned().unwrap())");
                true
            }
            // `xs.clear()` / `xs.reverse()` / `xs.sort()` — direct
            // Rust equivalents.
            "clear" | "reverse" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push('.');
                self.w.push_str(method);
                self.w.push_str("()");
                true
            }
            "sort" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".sort()");
                true
            }
            // `xs.remove(i)` / `xs.insert(i, v)` with isize→usize cast.
            "remove" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".remove((");
                self.emit_call_args(call);
                self.w.push_str(") as usize)");
                true
            }
            "insert" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".insert((");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(idx) = call.args.first() {
                    self.emit_expr(idx);
                }
                self.w.push_str(") as usize, ");
                if let Some(val) = call.args.get(1) {
                    self.emit_expr(val);
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                true
            }
            // `xs.join(sep)` — only well-defined for `Vec<String>`;
            // Rust's `Vec<String>::join(&str)` returns String.
            "join" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".join(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            // forEach: iterator chain calling the closure on each
            // borrowed element. Closure capture rules let it borrow
            // surrounding state. A wrapper-borrowed collection field
            // (`s.items` → `s.0.borrow().items`) is SNAPSHOTTED first
            // (S5, same rule as the H6 for-each statement fix):
            // holding the `Ref` guard across the closure panics
            // `already borrowed` the moment the closure mutates the
            // same object.
            "forEach" => {
                let snapshot = self.expr_reads_through_wrapper_borrow(receiver);
                if snapshot {
                    self.w.push_str("{ let __jux_seq = ");
                    self.emit_stdlib_receiver(receiver);
                    self.w.push_str(".clone(); __jux_seq");
                } else {
                    self.emit_stdlib_receiver(receiver);
                }
                self.w.push_str(".iter().for_each(|__e| (");
                self.emit_call_args(call);
                self.w.push_str(")(__e.clone()))");
                if snapshot {
                    self.w.push_str(" }");
                }
                true
            }
            // map / filter: collect into a fresh Vec so the result
            // stays Jux-array-shaped. Same S5 snapshot rule as forEach
            // — the closure may mutate the iterated object.
            "map" => {
                let snapshot = self.expr_reads_through_wrapper_borrow(receiver);
                if snapshot {
                    self.w.push_str("{ let __jux_seq = ");
                    self.emit_stdlib_receiver(receiver);
                    self.w.push_str(".clone(); __jux_seq");
                } else {
                    self.emit_stdlib_receiver(receiver);
                }
                self.w
                    .push_str(".iter().cloned().map(|__e| (");
                self.emit_call_args(call);
                self.w.push_str(")(__e)).collect::<Vec<_>>()");
                if snapshot {
                    self.w.push_str(" }");
                }
                true
            }
            "filter" => {
                let snapshot = self.expr_reads_through_wrapper_borrow(receiver);
                if snapshot {
                    self.w.push_str("{ let __jux_seq = ");
                    self.emit_stdlib_receiver(receiver);
                    self.w.push_str(".clone(); __jux_seq");
                } else {
                    self.emit_stdlib_receiver(receiver);
                }
                self.w
                    .push_str(".iter().cloned().filter(|__e| (");
                self.emit_call_args(call);
                self.w.push_str(")(__e.clone())).collect::<Vec<_>>()");
                if snapshot {
                    self.w.push_str(" }");
                }
                true
            }
            _ => false,
        }
    }

    /// Numeric / char intrinsics (§K.11) on primitive receivers.
    /// Numeric receivers cast to their exact Rust type first — that
    /// resolves Rust's ambiguous-`{integer}` inference, pins the
    /// method set, AND keeps width semantics honest (a `byte`
    /// wrapping-add wraps at 8 bits, not pointer width). Chars
    /// dispatch on `char` directly. Checked forms produce the Jux
    /// `Result<T, E>` enum.
    fn emit_numeric_stdlib_method(
        &mut self,
        call: &CallExpr,
        method: &str,
        prim: juxc_tycheck::Primitive,
    ) -> bool {
        use juxc_tycheck::Primitive as P;
        let Expr::Field(f) = &*call.callee else { return false };
        let receiver = &f.object;
        let is_float = matches!(prim, P::Float | P::Double | P::F32 | P::F64);
        let is_char = matches!(prim, P::Char);
        if matches!(prim, P::Bool) {
            return false;
        }
        // Exact Rust spelling of the receiver's primitive — the cast
        // target that keeps overflow/wrap behavior width-faithful.
        let rust_ty: &str = match prim {
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
            P::Char | P::Bool => "",
        };
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        let emit_recv = |this: &mut Self| {
            this.w.push_str("((");
            this.emit_expr(receiver);
            if is_char {
                this.w.push(')');
            } else {
                this.w.push_str(") as ");
                this.w.push_str(rust_ty);
            }
            this.w.push(')');
        };
        let simple: Option<&str> = if is_char {
            match method {
                "isDigit" => Some(".is_ascii_digit()"),
                "isAlphabetic" => Some(".is_alphabetic()"),
                "isWhitespace" => Some(".is_whitespace()"),
                "isUppercase" => Some(".is_uppercase()"),
                "isLowercase" => Some(".is_lowercase()"),
                "toUppercase" => Some(".to_ascii_uppercase()"),
                "toLowercase" => Some(".to_ascii_lowercase()"),
                // `codePoint()` — the Unicode scalar value as `uint`.
                "codePoint" => Some(" as usize"),
                _ => None,
            }
        } else if is_float {
            match method {
                "sqrt" => Some(".sqrt()"),
                "floor" => Some(".floor()"),
                "ceil" => Some(".ceil()"),
                // Spec: round-half-to-even (banker's rounding).
                "round" => Some(".round_ties_even()"),
                "abs" => Some(".abs()"),
                "isNaN" => Some(".is_nan()"),
                "isInfinite" => Some(".is_infinite()"),
                "isFinite" => Some(".is_finite()"),
                // IEEE bit pattern, widened to `uint` (§K.11).
                "bits" => Some(".to_bits() as usize"),
                _ => None,
            }
        } else {
            // `abs` only exists on SIGNED integers in Rust; unsigned
            // receivers fall through to the generic passthrough (and
            // rustc's method set) rather than emitting a bad call.
            let signed = !rust_ty.starts_with('u');
            match method {
                "abs" if signed => Some(".abs()"),
                "saturatingAbs" if signed => Some(".saturating_abs()"),
                "countOnes" => Some(".count_ones() as isize"),
                "leadingZeros" => Some(".leading_zeros() as isize"),
                "trailingZeros" => Some(".trailing_zeros() as isize"),
                _ => None,
            }
        };
        if let Some(suffix) = simple {
            emit_recv(self);
            self.w.push_str(suffix);
            self.emitting_format_arg = prev;
            return true;
        }
        // One-argument float forms (§K.11).
        if is_float {
            match method {
                // Exact bit equality, NaN payloads included.
                "bitsEqual" => {
                    emit_recv(self);
                    self.w.push_str(".to_bits() == ((");
                    self.emit_call_args(call);
                    self.w.push_str(") as ");
                    self.w.push_str(rust_ty);
                    self.w.push_str(").to_bits()");
                    self.emitting_format_arg = prev;
                    return true;
                }
                // IEEE 754 total order (backs `<=>` on floats):
                // -Inf < … < -0.0 < +0.0 < … < +Inf < NaN.
                "totalOrder" => {
                    emit_recv(self);
                    self.w.push_str(".total_cmp(&((");
                    self.emit_call_args(call);
                    self.w.push_str(") as ");
                    self.w.push_str(rust_ty);
                    self.w.push_str(")) as isize");
                    self.emitting_format_arg = prev;
                    return true;
                }
                // Fixed-decimal formatting: `3.14159.toFixed(2)` → "3.14".
                "toFixed" => {
                    self.w.push_str("format!(\"{:.1$}\", ");
                    emit_recv(self);
                    self.w.push_str(", (");
                    self.emit_call_args(call);
                    self.w.push_str(") as usize)");
                    self.emitting_format_arg = prev;
                    return true;
                }
                _ => {}
            }
        }
        // One-argument integer forms.
        if !is_float && !is_char {
            let one_arg: Option<&str> = match method {
                "saturatingAdd" => Some("saturating_add"),
                "saturatingSub" => Some("saturating_sub"),
                "saturatingMul" => Some("saturating_mul"),
                "wrappingAdd" => Some("wrapping_add"),
                "wrappingSub" => Some("wrapping_sub"),
                "wrappingMul" => Some("wrapping_mul"),
                _ => None,
            };
            if let Some(rust) = one_arg {
                emit_recv(self);
                self.w.push('.');
                self.w.push_str(rust);
                self.w.push_str("((");
                self.emit_call_args(call);
                self.w.push_str(") as ");
                self.w.push_str(rust_ty);
                self.w.push(')');
                self.emitting_format_arg = prev;
                return true;
            }
            let rotate: Option<&str> = match method {
                "rotateLeft" => Some("rotate_left"),
                "rotateRight" => Some("rotate_right"),
                _ => None,
            };
            if let Some(rust) = rotate {
                emit_recv(self);
                self.w.push('.');
                self.w.push_str(rust);
                self.w.push_str("((");
                self.emit_call_args(call);
                self.w.push_str(") as u32)");
                self.emitting_format_arg = prev;
                return true;
            }
            // Checked arithmetic → the Jux Result enum (§K.11).
            let checked: Option<&str> = match method {
                "checkedAdd" => Some("checked_add"),
                "checkedSub" => Some("checked_sub"),
                "checkedMul" => Some("checked_mul"),
                "checkedDiv" => Some("checked_div"),
                _ => None,
            };
            if let Some(rust) = checked {
                self.w.push_str("(match ");
                emit_recv(self);
                self.w.push('.');
                self.w.push_str(rust);
                self.w.push_str("((");
                self.emit_call_args(call);
                self.w.push_str(") as ");
                self.w.push_str(rust_ty);
                self.w.push_str(") { Some(__jux_v) => crate::jux::std::result::Result::Ok(__jux_v), None => crate::jux::std::result::Result::Err(crate::jux::std::exceptions::ArithmeticException::new(\"");
                self.w.push_str(method);
                self.w.push_str(" overflowed\".to_string())) })");
                self.emitting_format_arg = prev;
                return true;
            }
            // Width conversions to `int` (§K.11). `toInt` is checked
            // (Result); `saturatingToInt` clamps. Comparing through
            // `i128` covers every source width and signedness.
            if method == "toInt" {
                self.w.push_str("(match isize::try_from(");
                emit_recv(self);
                self.w.push_str(") { Ok(__jux_v) => crate::jux::std::result::Result::Ok(__jux_v), Err(_) => crate::jux::std::result::Result::Err(crate::jux::std::exceptions::ArithmeticException::new(\"toInt out of range\".to_string())) })");
                self.emitting_format_arg = prev;
                return true;
            }
            if method == "saturatingToInt" {
                self.w.push_str("({ let __jux_v = ");
                emit_recv(self);
                self.w.push_str(" as i128; if __jux_v > isize::MAX as i128 { isize::MAX } else if __jux_v < isize::MIN as i128 { isize::MIN } else { __jux_v as isize } })");
                self.emitting_format_arg = prev;
                return true;
            }
            // Radix formatting.
            let radix: Option<&str> = match method {
                "toHex" => Some("{:x}"),
                "toBinary" => Some("{:b}"),
                "toOctal" => Some("{:o}"),
                _ => None,
            };
            if let Some(fmt) = radix {
                self.w.push_str("format!(\"");
                self.w.push_str(fmt);
                self.w.push_str("\", ");
                emit_recv(self);
                self.w.push(')');
                self.emitting_format_arg = prev;
                return true;
            }
        }
        self.emitting_format_arg = prev;
        false
    }

    fn emit_string_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let receiver = &*f.object;
        match method {
            // `s.length()` → `s.chars().count() as isize` — Java's
            // length counts code-units, but Phase-1 lowers to
            // char-count for usability. A `len_bytes()` variant
            // can land later when raw-byte counts matter.
            "length" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".chars().count() as isize");
                true
            }
            "isEmpty" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            // §K.7: explicit length forms. `byteLength` is the
            // UTF-8 byte count (Rust `len`); `charLength` counts
            // scalar values (O(N) per the spec note).
            "byteLength" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            "charLength" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".chars().count() as isize");
                true
            }
            "repeat" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".repeat((");
                self.emit_call_args(call);
                self.w.push_str(") as usize)");
                true
            }
            // Pure renames: snake_case Rust spelling.
            "toUpperCase" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".to_uppercase()");
                true
            }
            "toLowerCase" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".to_lowercase()");
                true
            }
            "trim" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".trim().to_string()");
                true
            }
            // Accept BOTH the rust-verbatim snake_case names (`starts_with`,
            // `ends_with` — the std String API surface) and the legacy
            // camelCase Java-style aliases. Without the snake_case arms these
            // fell through to a generic call that passed a `String` where the
            // method wants `&str`/`Pattern` (rustc E0277).
            "startsWith" | "starts_with" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".starts_with(");
                self.emit_call_args(call);
                self.w.push_str(".as_str())");
                true
            }
            "endsWith" | "ends_with" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".ends_with(");
                self.emit_call_args(call);
                self.w.push_str(".as_str())");
                true
            }
            "contains" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".contains(");
                self.emit_call_args(call);
                self.w.push_str(".as_str())");
                true
            }
            "replace" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".replace(");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(needle) = call.args.first() {
                    self.emit_expr(needle);
                }
                self.w.push_str(".as_str(), ");
                if let Some(rep) = call.args.get(1) {
                    self.emit_expr(rep);
                }
                self.w.push_str(".as_str())");
                self.emitting_format_arg = prev;
                true
            }
            "indexOf" => {
                self.w.push('(');
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".find(");
                self.emit_call_args(call);
                self.w.push_str(".as_str()).map(|__i| __i as isize).unwrap_or(-1))");
                true
            }
            "split" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".split(");
                self.emit_call_args(call);
                self.w
                    .push_str(".as_str()).map(::std::string::String::from).collect::<Vec<_>>()");
                true
            }
            "substring" => {
                // `s.substring(start, end)` — char-indexed slice.
                self.w.push('(');
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".chars().skip((");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(start) = call.args.first() {
                    self.emit_expr(start);
                }
                self.w.push_str(") as usize).take(((");
                if let Some(end) = call.args.get(1) {
                    self.emit_expr(end);
                }
                self.w.push_str(") - (");
                if let Some(start) = call.args.first() {
                    self.emit_expr(start);
                }
                self.emitting_format_arg = prev;
                self.w
                    .push_str(")) as usize).collect::<String>())");
                true
            }
            "charAt" => {
                self.emit_stdlib_receiver(receiver);
                self.w.push_str(".chars().nth((");
                self.emit_call_args(call);
                self.w.push_str(") as usize).unwrap()");
                true
            }
            _ => false,
        }
    }

    /// Emit a call's args as a comma-separated list, with the
    /// format-arg flag cleared so nested string literals
    /// self-coerce. Used by the stdlib-method rewriter to splat
    /// the original args into the rewritten Rust shape.
    fn emit_call_args(&mut self, call: &CallExpr) {
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.emit_collection_arg(call, i, arg);
        }
        self.emitting_format_arg = prev;
    }

    /// Emit ONE builtin-container argument with its element coercion
    /// ladder: nullable `Some(…)` wrap and wrapper share-`.clone()`. Shared
    /// by `emit_call_args` and the arg-hoisting path so both produce the
    /// same stored value. When `collection_args_prehoisted` is set the
    /// argument is already a coerced temp, so the ladder is skipped (the
    /// bare temp is emitted) — see that flag's doc.
    fn emit_collection_arg(&mut self, call: &CallExpr, i: usize, arg: &Expr) {
        if self.collection_args_prehoisted {
            self.emit_expr(arg);
            return;
        }
        // **Nullable element slot** — storing into a container whose
        // element type-arg is `T?` (`ArrayList<int?>` → `Vec<Option
        // <isize>>`): a non-null value lifts into `Some(...)`; a
        // `null` literal / already-`Option` value passes through.
        let wrap_some = self
            .builtin_arg_elem_nullable(call, i)
            && !self.expression_is_already_nullable(arg);
        if wrap_some {
            self.w.push_str("Some(");
        }
        self.emit_expr(arg);
        // **Wrapper-class share-on-pass (§CR.4.1)** for the builtin
        // collection dispatches (`xs.add(obj)` → `xs.push(obj)`):
        // storing a wrapped place must SHARE the handle (`Rc`
        // refcount bump), not move it — `l1.add(c); l2.add(c);`
        // would otherwise be a rustc E0382 on the second use, and
        // a mutation through the container element must stay
        // visible through the original binding.
        if self.wrapper_value_needs_clone(arg) {
            self.w.push_str(".clone()");
        }
        if wrap_some {
            self.w.push(')');
        }
    }

    /// True when argument `i` of a **builtin container call** lands in
    /// an element slot whose generic type-arg is nullable — `xs.add(v)`
    /// on an `ArrayList<int?>`, `m.put(k, v)` on a `HashMap<String,
    /// int?>`, etc. Maps the arg index to the receiver's generic-arg
    /// position per method: list `add`/`set@1`/`insert@1`, set `add`,
    /// map `put@1` (values; keys stay non-null). Non-container shapes
    /// answer `false`.
    fn builtin_arg_elem_nullable(&self, call: &CallExpr, arg_idx: usize) -> bool {
        let Expr::Field(f) = call.callee.as_ref() else { return false };
        let method = f.field.text.as_str();
        // Which generic-arg slot does this argument store into?
        let generic_idx = match (method, arg_idx) {
            ("add", 0) => 0,                  // list/set value
            ("set", 1) | ("insert", 1) => 0,  // list value (idx, value)
            ("put", 1) => 1,                  // map value (key, value)
            _ => return false,
        };
        // Receiver type: span-keyed `expr_types` first, then the
        // name-keyed `local_types` fallback (span collisions and
        // unrecorded `Path` leaves miss the first map — same fallback
        // the field/receiver resolvers use).
        let recv_ty = self
            .expr_types
            .get(&crate::exprs::expr_span_of(&f.object))
            .cloned()
            .or_else(|| {
                if let Expr::Path(qn) = f.object.as_ref() {
                    if qn.segments.len() == 1 {
                        return self
                            .local_types
                            .iter()
                            .rev()
                            .find_map(|s| s.get(&qn.segments[0].text))
                            .cloned();
                    }
                }
                None
            });
        match recv_ty {
            // `ArrayList<T>` lowers to `Ty::Array { element }` (dynamic
            // kind), not `Ty::User` — the element IS generic-arg 0.
            Some(juxc_tycheck::Ty::Array { element, .. }) => {
                generic_idx == 0 && matches!(*element, juxc_tycheck::Ty::Nullable(_))
            }
            Some(juxc_tycheck::Ty::User { generic_args, .. }) => matches!(
                generic_args.get(generic_idx),
                Some(juxc_tycheck::Ty::Nullable(_)),
            ),
            _ => false,
        }
    }
}

/// Structural typing for receivers built PURELY from numeric/char
/// literals. Literals have `Span::DUMMY`, and a binary expression over
/// two literals joins those into another DUMMY span, so none of them
/// ever land in `expr_types`. Mixed int/float arithmetic widens to
/// `double`, matching the inference pass. Returns `None` as soon as a
/// non-literal leaf appears (those have real spans and use the map).
pub(crate) fn literal_numeric_ty(e: &Expr) -> Option<juxc_tycheck::Primitive> {
    use juxc_tycheck::Primitive as P;
    match e {
        Expr::Literal(juxc_ast::Literal::Int(_)) => Some(P::Int),
        Expr::Literal(juxc_ast::Literal::Float(_)) => Some(P::Double),
        Expr::Literal(juxc_ast::Literal::Char(_)) => Some(P::Char),
        Expr::Unary(u) => literal_numeric_ty(&u.operand),
        Expr::Binary(b) => {
            let l = literal_numeric_ty(&b.left)?;
            let r = literal_numeric_ty(&b.right)?;
            if matches!(l, P::Char) || matches!(r, P::Char) {
                return None;
            }
            Some(if matches!(l, P::Double) || matches!(r, P::Double) {
                P::Double
            } else {
                l
            })
        }
        _ => None,
    }
}
