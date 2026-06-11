//! Field-access emission — `obj.field` reads, with the auto-`.clone()`
//! and `.length` special cases, plus the supporting type-lookup that
//! decides when a clone is needed.

use juxc_ast::{Expr, FieldExpr};
use juxc_tycheck::Ty;

use crate::exprs::{expr_span_of, ty_kind_from_ref_with_params};
use crate::RustEmitter;

impl RustEmitter {
    pub(crate) fn emit_field(&mut self, f: &FieldExpr) {
        // Take-and-clear the method-call-callee marker on entry so
        // ONLY this (outermost) field of a `recv.method(args)` callee
        // sees it. Cleared before any nested `emit_expr(&f.object)`, so
        // a field receiver in `obj.field.method()` still borrow-wraps.
        // When set, the `.0.borrow()` wrapper rewrite below is
        // suppressed — the `.field` here names a method on the newtype.
        let is_call_callee = std::mem::take(&mut self.emitting_call_callee);
        // Overload suffix for a METHOD-position member name — taken
        // here (only when this field IS the call callee) so a nested
        // receiver's own field emissions can't consume it.
        let method_suffix: Option<String> = if is_call_callee {
            self.pending_method_suffix.take()
        } else {
            None
        };
        // Safe-navigation field access (`obj?.field`) lowers via
        // `Option::map`: the closure runs only when the receiver
        // is `Some`, and the result is `Option<FieldType>`. We
        // emit through `as_ref()` so the original `Option<T>` isn't
        // moved — the user is free to keep reading `obj` after.
        // A `?.field` access on a method-call result (`f()?.field`)
        // works the same way: the inner expression's value goes
        // through `.as_ref()` then `.map(...)`.
        //
        // Field clones inside the closure use `.clone()` for
        // ownership; the closure receives a `&T`, so we clone the
        // field out. Every Jux user type derives `Clone`, so this
        // is always valid (primitives are `Copy` and ignore the
        // call). The `length` short-circuit below stays
        // safe-aware: `obj?.length` on a nullable array produces
        // an `Option<isize>` length.
        if f.safe {
            self.emit_safe_field(f);
            return;
        }
        // §K.11 primitive-type constants: `int.MAX_VALUE` → `isize::MAX`,
        // `double.NAN` → `f64::NAN`, … The receiver is the TYPE NAME
        // itself (primitive names are keywords, so no local can shadow).
        if let Expr::Path(qn) = &*f.object {
            if qn.segments.len() == 1 {
                if let Some(rust) =
                    numeric_constant(&qn.segments[0].text, &f.field.text)
                {
                    self.w.push_str(&rust);
                    return;
                }
            }
        }
        // AsyncMutex guard (§18.3): `guard.value` derefs the guard.
        // Lvalue writes never reach here (emit_assign intercepts);
        // non-Copy protected values clone out on read.
        if f.field.text == "value" {
            let recv_ty = self
                .expr_types
                .get(&crate::exprs::expr_span_of(&f.object))
                .cloned()
                .or_else(|| {
                    if let Expr::Path(qn) = &*f.object {
                        if qn.segments.len() == 1 {
                            return self
                                .local_types
                                .iter()
                                .rev()
                                .find_map(|s| s.get(&qn.segments[0].text).cloned());
                        }
                    }
                    None
                });
            if let Some(juxc_tycheck::Ty::User { name, generic_args }) = recv_ty {
                if name == "__AsyncMutexGuard" {
                    self.w.push_str("(*");
                    self.emit_expr(&f.object);
                    self.w.push(')');
                    let copy = matches!(
                        generic_args.first(),
                        Some(juxc_tycheck::Ty::Primitive(_))
                    );
                    if !copy && !self.emitting_lvalue {
                        self.w.push_str(".clone()");
                    }
                    return;
                }
            }
        }
        // **Field READ through a polymorphic-base reference** → accessor call.
        // A base-typed value is a `Rc<dyn …Kind>` trait object that can't
        // expose struct fields, so `baseRef.f` reads via the generated
        // `__get_f()` (reachable up the `Kind` supertrait chain). `this`,
        // concrete receivers, method callees, and lvalue (write) targets keep
        // direct field access — writes go through `__set_f` in `emit_assign`.
        if !is_call_callee && !self.emitting_lvalue && !matches!(&*f.object, Expr::This(_)) {
            if let Some(bare) = self.receiver_class_bare(&f.object) {
                if self.poly_base_classes.contains(&bare) {
                    let accessor_ok = self
                        .symbols
                        .lookup_field(&bare, &f.field.text)
                        .map(|(fsig, _)| {
                            matches!(
                                fsig.visibility,
                                juxc_ast::Visibility::Public | juxc_ast::Visibility::Protected
                            )
                        })
                        .unwrap_or(false);
                    if accessor_ok {
                        self.emit_expr(&f.object);
                        self.w.push_str(".__get_");
                        self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                        self.w.push_str("()");
                        return;
                    }
                }
            }
        }
        // §G.9.2: a member access on an **external** (`rust.std` / crate)
        // receiver uses the foreign symbol's REAL Rust name, which is
        // `snake_case` — bindgen camelCased it for the Jux surface (§G.4). So
        // `p.asPath()` → `p.as_path()`, `s.isEmpty()` → `s.is_empty()`. The call
        // path appends `(args)` after this returns; a plain field access is
        // complete here. External types are plain Rust values (not the Rc/RefCell
        // wrapper representation), so none of the `.0.borrow()` rewrites apply.
        if let Some(Ty::User { name, .. }) = self
            .expr_types
            .get(&expr_span_of(&f.object))
            .cloned()
            .map(strip_nullable)
        {
            let external = if self.symbols.classes.contains_key(&name) {
                self.symbols.classes.get(&name).map(|c| c.is_external).unwrap_or(false)
            } else {
                self.lookup_class_by_bare_or_fqn(name.rsplit('.').next().unwrap_or(&name))
                    .map(|c| c.is_external)
                    .unwrap_or(false)
            };
            if external {
                self.emit_expr(&f.object);
                self.w.push('.');
                self.w.push_str(&camel_to_snake(&f.field.text));
                return;
            }
        }
        // **Expression-bodied property rewrite.** When `f.field`
        // names a method declared with the property shape
        // (`T name => expr;`), emit `obj.name()` instead of the
        // raw `obj.name`. Detection uses the receiver's inferred
        // type (looked up in `expr_types`) → class → method
        // signature. Falls through to the regular field path
        // when the receiver isn't a class or the name isn't a
        // property.
        if let Some(recv_ty) = self
            .expr_types
            .get(&crate::exprs::expr_span_of(&f.object))
            .cloned()
        {
            if let juxc_tycheck::Ty::User { name, .. } = &recv_ty {
                let bare = name.rsplit('.').next().unwrap_or(name);
                let is_property = self
                    .lookup_class_by_bare_or_fqn(bare)
                    .and_then(|c| c.methods.get(f.field.text.as_str()).cloned())
                    .map(|m| m.is_property)
                    .unwrap_or(false);
                if is_property {
                    self.emit_expr(&f.object);
                    self.w.push('.');
                    self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                    self.w.push_str("()");
                    return;
                }
            }
        }
        if f.field.text == "length" {
            // `xs.length` → `xs.len() as isize`. Wrap the receiver
            // in parens only when its shape might otherwise bind
            // looser than `.` (e.g. binary or range expression);
            // atoms like idents, field-chains, method calls, and
            // indexes don't need them and the output reads as
            // handwritten Rust without the parens. The `as isize`
            // cast is required because Rust's `.len()` returns
            // `usize` but Jux's `int` is platform-signed.
            let needs_parens = receiver_needs_parens(&f.object);
            if needs_parens {
                self.w.push('(');
            }
            self.emit_expr(&f.object);
            if needs_parens {
                self.w.push(')');
            }
            self.w.push_str(".len() as isize");
            return;
        }
        // Enum variant access: `Color.Red` (a Field whose object is a
        // single-segment Path naming a known enum type) lowers to
        // Rust's path syntax `Color::Red`. Tuple-payload variant
        // construction (`Color.Red(args)`) reuses this path through
        // the enclosing `emit_call`, which appends the arg list.
        if let Expr::Path(qn) = &*f.object {
            if qn.segments.len() == 1 {
                let bare = &qn.segments[0].text;
                // Direct FQN match (single-package programs and
                // explicitly-FQN'd uses).
                if self.symbols.enums.contains_key(bare) {
                    self.w.push_str(bare);
                    self.w.push_str("::");
                    self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                    return;
                }
                // Import-alias-aware: the current unit's
                // `unqualified` map carries `alias → FQN` for both
                // bare imports and grouped `{ X as Y }` aliases. A
                // hit there resolves enum-variant constructions
                // through the user's chosen alias name. Emit the
                // alias name on the LHS (Rust scope has it via the
                // emitted `use X as Y;`) while the FQN match
                // confirms the enum is real.
                if let Some(idx) = self.current_unit_idx {
                    if let Some(ctx) = self.symbols.units.get(idx) {
                        if let Some(fqn) = ctx.unqualified.get(bare.as_str()) {
                            if self.symbols.enums.contains_key(fqn) {
                                self.w.push_str(bare);
                                self.w.push_str("::");
                                self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                                return;
                            }
                        }
                    }
                }
                // Bare-name reference to an enum imported from
                // another package: scan all enum FQNs and pick one
                // whose last segment matches. Same shape the
                // class- and interface-FQN walks use elsewhere.
                let enum_hit = self
                    .symbols
                    .enums
                    .keys()
                    .find(|fqn| {
                        fqn.rsplit('.').next().unwrap_or(fqn.as_str()) == bare.as_str()
                    })
                    .cloned();
                if let Some(enum_fqn) = enum_hit {
                    // Cross-package auto-import: crate-root the path.
                    // A bare `Option::Some` / `Result::Ok` would
                    // otherwise resolve to Rust's PRELUDE types in
                    // the emitted module — silently the wrong enum.
                    let cur_pkg = self.symbols.package.join(".");
                    let fqn_pkg = enum_fqn
                        .rsplit_once('.')
                        .map(|(p, _)| p.to_string())
                        .unwrap_or_default();
                    if enum_fqn.contains('.') && fqn_pkg != cur_pkg {
                        self.w.push_str("crate::");
                        self.w
                            .push_str(&enum_fqn.split('.').collect::<Vec<_>>().join("::"));
                    } else {
                        self.w.push_str(bare);
                    }
                    self.w.push_str("::");
                    self.w.push_str(&f.field.text);
                    if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                    return;
                }
            }
        }
        // Static-field access: `ClassName.X` (or `pkg.Cls.X`) where
        // the path resolves to a known class. Two emission shapes:
        //
        //   - `final` static  → Rust associated const inside the
        //     inherent impl, accessed as `Path::X`. Cross-package
        //     paths get the same `crate::`-rooting `new` uses.
        //   - Plain `static`  → module-scope `LazyLock<Mutex<T>>`
        //     named `Class_X` (see `emit_mutable_static_field`).
        //     Lvalue context emits `*Class_X.lock().unwrap()` so
        //     the surrounding `=` produces a valid place
        //     expression; rvalue context emits
        //     `Class_X.lock().unwrap().clone()` to materialize an
        //     owned value before the guard drops.
        if let Expr::Path(qn) = &*f.object {
            // **Static property read (§M.7.9).** `Class.Prop` where
            // `Prop` is a static property → call the static getter
            // `Class::Prop()`. Suppressed in lvalue position so a
            // `Class.Prop = v` write still reaches the setter-routing
            // path in `emit_assign` (which handles the static case).
            if !self.emitting_lvalue && !is_call_callee {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    let is_static_prop = self
                        .lookup_class_ast_by_bare_or_fqn(
                            crate::backend_fqn::fqn_bare(&class_fqn),
                        )
                        .map(|c| {
                            c.properties
                                .iter()
                                .any(|p| p.name.text == f.field.text && p.is_static)
                        })
                        .unwrap_or(false);
                    if is_static_prop {
                        self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                        self.w.push_str("::");
                        self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                        self.w.push_str("()");
                        return;
                    }
                }
            }
            if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                let cls = self.symbols.classes.get(&class_fqn);
                if let Some(field) = cls.and_then(|c| c.fields.get(f.field.text.as_str())) {
                    if field.is_static {
                        // A `final` static normally reads as the assoc
                        // const `Class::field` — EXCEPT when the payload
                        // is `!Send` (a wrapper object): those are stored
                        // thread_local (Rust `const` can't run the
                        // wrapper ctor), so they fall through to the
                        // thread_local read below.
                        if field.is_final && !self.final_static_needs_runtime_init(&field.ty) {
                            self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                            self.w.push_str("::");
                            self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                            return;
                        }
                        // Mutable static — guarded `LazyLock<Mutex<T>>`.
                        // Reading a static field is an observable use (§S.4.1),
                        // so an rvalue read of a field whose class has
                        // `static { }` blocks runs the once-guarded initializer
                        // first, wrapped in a block expression. (`__static_init`
                        // is re-entrancy-safe, so this is harmless even when the
                        // read happens inside the static block itself.)
                        let has_si = self
                            .symbols
                            .classes
                            .get(&class_fqn)
                            .map(|c| c.has_static_init)
                            .unwrap_or(false);
                        // **`!Send` static → `thread_local!` slot.** Reads
                        // hand out a shared handle (`Rc` bump) via
                        // `.with(|__s| __s.borrow().clone())`. The direct
                        // slot WRITE (`Registry.global = …`) never reaches
                        // this lvalue path — `emit_assign` intercepts it
                        // with the `.with(|__s| *__s.borrow_mut() = …)`
                        // form; a chained write (`Registry.global.n = 5`)
                        // mutates through the read-out handle's own
                        // `RefCell`, so the rvalue shape is correct there.
                        if self.static_type_needs_thread_local(&field.ty) {
                            if has_si {
                                self.w.push_str("({ ");
                                self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                                self.w.push_str("::__static_init(); ");
                            }
                            self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                            self.w.push('_');
                            self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                            self.w.push_str(".with(|__s| __s.borrow().clone())");
                            if has_si {
                                self.w.push_str(" })");
                            }
                            return;
                        }
                        if self.emitting_lvalue {
                            self.w.push('*');
                            self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                            self.w.push('_');
                            self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                            self.w.push_str(".lock().unwrap()");
                        } else {
                            // Parenthesize the block expression so it stays an
                            // EXPRESSION in operand position (`x.base * 5`): a
                            // bare leading `{` would parse as a block statement.
                            if has_si {
                                self.w.push_str("({ ");
                                self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                                self.w.push_str("::__static_init(); ");
                            }
                            self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                            self.w.push('_');
                            self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                            self.w.push_str(".lock().unwrap().clone()");
                            if has_si {
                                self.w.push_str(" })");
                            }
                        }
                        return;
                    }
                }
            }
            // Interface static field: `IfaceName.FIELD` lowers to
            // `Iface_FIELD`. The free-`pub const` definition is
            // emitted by `emit_interface_decl` alongside the trait.
            if let Some(iface_fqn) = self.path_resolves_to_interface_in_emit(qn) {
                let iface = self.symbols.interfaces.get(&iface_fqn);
                if iface
                    .and_then(|i| i.fields.get(f.field.text.as_str()))
                    .is_some()
                {
                    self.emit_fqn_path_in_rust(&iface_fqn, qn.segments.len() > 1);
                    self.w.push('_');
                    self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
                    return;
                }
            }
        }
        // Generic member access — emit verbatim and rely on Rust to
        // resolve.
        //
        // **Wrapper-class interior-mutability read (§CR.4.1).** When
        // the receiver is a value of a wrapper-shape class, the field
        // lives inside `Rc<RefCell<C_Inner>>` — read it through a
        // statement-scoped `.0.borrow()`: `obj.f` → `obj.0.borrow().f`.
        // The borrow is dropped at the end of the enclosing
        // expression statement, so we never hold it across a call.
        // The existing auto-`.clone()` tail still fires for
        // String / generic / class fields, which is exactly what
        // makes the borrow statement-scoped (the cloned value
        // outlives the temporary `Ref`).
        //
        // **Method-call callee guard.** `obj.method(args)` parses as a
        // `Call` whose callee is this same `Field` node, so `emit_field`
        // runs for it too. Methods live on the wrapper newtype `C`
        // (called as `obj.method()`), NOT inside `C_Inner`, so the
        // borrow must only fire when `f.field` names an actual
        // instance FIELD of the wrapper class — not a method.
        // `wrapper_depth` is `Some(n)` when the receiver is a wrapper
        // class AND `f.field` names an instance field declared `n`
        // ancestors up the `extends` chain (`0` = on the receiver's own
        // class). The read then walks `n` `__parent` hops after the
        // `.0.borrow()`: e.g. a field declared two ancestors up emits
        // `recv.0.borrow().__parent.__parent.field`. `None` means
        // either a non-wrapper receiver or a method-call callee (methods
        // live on the wrapper newtype, never inside `C_Inner`), so no
        // borrow rewrite fires.
        let wrapper_depth = if !is_call_callee && self.receiver_is_wrapper_class(&f.object) {
            self.wrapper_field_parent_depth(&f.object, &f.field.text)
        } else {
            None
        };
        self.emit_expr(&f.object);
        if let Some(depth) = wrapper_depth {
            // An `out` field place needs an exclusive `&mut` into the
            // interior, so take the mutable borrow; the `RefMut` temporary
            // lives to the end of the call statement (§M.4).
            if self.emitting_out_place {
                self.w.push_str(".0.borrow_mut()");
            } else {
                self.w.push_str(".0.borrow()");
            }
            for _ in 0..depth {
                self.w.push_str(".__parent");
            }
        }
        self.w.push('.');
        self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
        // Auto-`.clone()` on field reads in two cases:
        //   1. String-field reads — so `return this.name;` and similar
        //      don't move out of `&self`.
        //   2. Generic-field reads — `class Box<T> { T value; … }`'s
        //      `return this.value;` faces the same move-out-of-&self
        //      problem; we clone the same way. The Phase-1 `T: Clone`
        //      bound emitted on the impl makes this always valid.
        // Both paths share the lvalue-suppression — we never want
        // `self.x.clone() = ...` on an assignment target.
        //
        // Phase H: the decision used to consult two name-keyed
        // `HashSet`s (`string_field_names` / `generic_field_names`)
        // computed by a pre-pass over class field decls. That worked
        // but mis-fired when a same-named field on a different class
        // had a different type. Now we consult tycheck's per-expression
        // type map directly: the field expression `obj.name` was
        // recorded with its precise `Ty`. A missing entry falls back
        // to the conservative "do the .clone()" path, matching the
        // old heuristic for the (rare) cases tycheck didn't visit.
        // Auto-`.clone()` is suppressed in any **borrow context** —
        // a position where the surrounding code only needs to *read*
        // the field, not own it. Today three such positions:
        //
        // - **lvalue context**: `self.x = ...` must never become
        //   `self.x.clone() = ...`.
        // - **format-arg context**: `println!`/`format!` borrow via
        //   `Display`; a `&String` is as good as `String` and we
        //   save the alloc.
        // - **comparison operand**: `==`, `!=`, `<`, `<=`, `>`, `>=`
        //   on Strings borrow both sides through `PartialEq`/
        //   `PartialOrd`, so the clone is redundant.
        let in_borrow_context =
            self.emitting_format_arg || self.emitting_comparison_operand;
        // **Wrapper-borrow clone (statement-scoped borrow discipline).**
        // When the read went through a `.0.borrow()` guard, the field
        // value lives inside a temporary `Ref` that drops at the end of
        // the statement. A non-`Copy` field used as a method-call
        // *receiver* (`a.f.g()`) — or any owning position — must be
        // cloned OUT of the guard so the `.g()` call (which may need
        // `&mut`) operates on an owned value, not through the immutable
        // `Ref`. The span-keyed `field_read_needs_clone` can miss the
        // type when the receiver is `this` (no `expr_types` entry), so
        // we additionally resolve the field's declared type through the
        // owning class chain here. This is exactly the `a.f.g()` →
        // "clone `f`, drop the guard, then `.g()`" rule from §CR.4.1.
        // **Method-call callee guard (field-vs-method name collision).**
        // When this `Field` is the callee of a method call
        // (`obj.method(args)`) AND `f.field` also names a non-static
        // method on the receiver's class chain, the `.field` here is the
        // METHOD, not an instance field — even if a same-named field
        // exists up the chain (e.g. `private String label;` plus
        // `public String label()`). In that case we must NOT auto-clone:
        // `obj.label.clone()` followed by the call's `()` would emit the
        // nonsensical `obj.label.clone()()`. Suppress the clone so the
        // callee reads `obj.label` and `emit_call` appends `(...)`.
        let callee_is_method = is_call_callee
            && self.field_names_method_on_receiver(&f.object, &f.field.text);
        let wrapper_borrow_clone = wrapper_depth.is_some()
            && !self.emitting_lvalue
            && !in_borrow_context
            && self.wrapper_field_read_needs_clone(&f.object, &f.field.text);
        if !callee_is_method
            && (wrapper_borrow_clone
                || (!self.emitting_lvalue && !in_borrow_context && self.field_read_needs_clone(f)))
        {
            self.w.push_str(".clone()");
        }
    }

    /// True iff `field_name` resolves to a non-static **method** on the
    /// receiver expression's class type (walking the `extends` chain).
    /// Used by [`Self::emit_field`] to disambiguate a method-call callee
    /// (`obj.method(args)`) from a same-named field read when both a
    /// field and a method share the name — the method always wins as a
    /// call callee, and must not pick up the field auto-`.clone()`.
    fn field_names_method_on_receiver(
        &self,
        receiver: &juxc_ast::Expr,
        field_name: &str,
    ) -> bool {
        let Some(Ty::User { name, .. }) =
            self.expr_types.get(&expr_span_of(receiver))
        else {
            return false;
        };
        // Resolve the receiver's class (FQN or bare), then walk its
        // `extends` chain looking for a non-static method by name.
        let bare = name.rsplit('.').next().unwrap_or(name.as_str());
        let mut cursor: Option<String> = Some(bare.to_string());
        let mut depth = 0usize;
        while let Some(cname) = cursor {
            if depth > 64 {
                break;
            }
            let Some(class) = self.lookup_class_by_bare_or_fqn(&cname) else {
                break;
            };
            if let Some(m) = class.methods.get(field_name) {
                return !m.is_static;
            }
            cursor = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
            depth += 1;
        }
        false
    }

    /// Lower `obj?.field` to a closure that runs only when the
    /// receiver is `Some`. Two shapes depending on whether the
    /// field itself is nullable:
    ///
    /// - **Non-nullable field** →
    ///   `obj.as_ref().map(|__t| __t.field.clone())`. The closure
    ///   returns the field's value; the whole expression is
    ///   `Option<FieldType>`.
    /// - **Nullable field** →
    ///   `obj.as_ref().and_then(|__t| __t.field.clone())`. The
    ///   field already returns `Option<T>`; `and_then` flattens
    ///   the two layers so the result stays `Option<T>` instead
    ///   of the wrong `Option<Option<T>>` `.map` would produce.
    ///
    /// The receiver is borrowed (via `as_ref`) so the original
    /// `Option<T>` stays usable. Inside the closure the field is
    /// cloned so the result is owned.
    ///
    /// Method-call variant `obj?.method(args)` is handled at the
    /// `emit_call` level (`emit_safe_method_call`).
    /// Emit a bare reference to a static field of the enclosing
    /// class. Mirrors the explicit-`Class.field` branch in
    /// [`Self::emit_field`] but takes the class name and field
    /// metadata directly because the caller (in `Expr::Path`
    /// emission) has already resolved both.
    ///
    /// `is_final` picks the lowering shape:
    /// - `true`  → `pub const`-style access, `Class::field`.
    /// - `false` → `LazyLock<Mutex<T>>`-style at module scope,
    ///   `Class_field`. Lvalue/rvalue context drives the lock
    ///   shape, identical to the qualified-form rule.
    pub(crate) fn emit_enclosing_class_static_ref(
        &mut self,
        class_name: &str,
        field_name: &str,
        is_final: bool,
    ) {
        // Two storage predicates, matched against the DECL routing in
        // `emit_class_decl` / `emit_mutable_static_field`:
        // - `is_thread_local` (`!Send` payload) → `thread_local!` slot,
        //   reads hand out a shared handle. Applies to finals too.
        // - `final_runtime` (class/record payload, `Send` or not) — a
        //   final that can't be a `pub const` (non-const ctor); stored
        //   module-scope like a mutable static, so its read takes the
        //   lock shape below instead of the assoc-const path.
        let field_ty = self
            .lookup_class_by_bare_or_fqn(class_name)
            .and_then(|c| c.fields.get(field_name))
            .map(|fs| fs.ty.clone());
        let is_thread_local = field_ty
            .as_ref()
            .map(|ty| self.static_type_needs_thread_local(ty))
            .unwrap_or(false);
        let final_runtime = is_final
            && field_ty
                .as_ref()
                .map(|ty| self.final_static_needs_runtime_init(ty))
                .unwrap_or(false);
        if is_final && !final_runtime && !is_thread_local {
            self.w.push_str(class_name);
            self.w.push_str("::");
            self.w.push_str(field_name);
            return;
        }
        if is_thread_local {
            self.w.push_str(class_name);
            self.w.push('_');
            self.w.push_str(field_name);
            self.w.push_str(".with(|__s| __s.borrow().clone())");
            return;
        }
        if self.emitting_lvalue {
            self.w.push('*');
            self.w.push_str(class_name);
            self.w.push('_');
            self.w.push_str(field_name);
            self.w.push_str(".lock().unwrap()");
        } else {
            self.w.push_str(class_name);
            self.w.push('_');
            self.w.push_str(field_name);
            self.w.push_str(".lock().unwrap().clone()");
        }
    }

    /// True when `recv` evaluates to a value whose type is a
    /// **wrapper-shape** class (one registered in
    /// [`RustEmitter::wrapper_classes`]). Drives the `.0.borrow()` /
    /// `.0.borrow_mut()` interior-mutability rewrites for field
    /// access (§CR.4.1).
    ///
    /// Two recognition paths:
    /// - **`this` / `self`** inside a wrapper class's own method or
    ///   operator body — recognized via `enclosing_class` while the
    ///   wrapper flag is set. (Constructor bodies clear the wrapper
    ///   flag, so `this.f` there stays a direct `__self.f` write on
    ///   the plain `C_Inner`.)
    /// - **Any other receiver** — its inferred type (resolved by
    ///   [`Self::receiver_class_bare`]) is a class whose bare name is a
    ///   wrapper class.
    pub(crate) fn receiver_is_wrapper_class(&self, recv: &Expr) -> bool {
        if matches!(recv, Expr::This(_)) {
            return self.emitting_wrapper_class
                && self
                    .enclosing_class
                    .as_deref()
                    .map(|c| self.wrapper_classes.contains(c))
                    .unwrap_or(false);
        }
        self.receiver_class_bare(recv)
            .map(|bare| self.wrapper_classes.contains(&bare))
            .unwrap_or(false)
    }

    /// Resolve the bare class name a non-`this` receiver evaluates to.
    ///
    /// **Span-collision robustness.** For a bare-`Path` receiver (a
    /// local variable) we consult the emitter's `local_types` map
    /// FIRST — it's keyed by name, so it's immune to the
    /// interpolated-string span collisions that plague `expr_types`
    /// (inner `${expr}` exprs reparse against the segment substring and
    /// carry spans local to it, so several interpolation sites can
    /// alias one key). Only when the name isn't a tracked local do we
    /// fall back to the span-keyed `expr_types`. This mirrors the same
    /// precedence the stdlib-method dispatcher uses (`try_emit_stdlib_method`).
    pub(crate) fn receiver_class_bare(&self, recv: &Expr) -> Option<String> {
        // Local-variable fast path (collision-immune).
        if let Expr::Path(qn) = recv {
            if qn.segments.len() == 1 {
                let bare = qn.segments[0].text.as_str();
                if let Some(juxc_tycheck::Ty::User { name, .. }) = self
                    .local_types
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(bare))
                {
                    return Some(name.rsplit('.').next().unwrap_or(name).to_string());
                }
            }
        }
        match self.expr_types.get(&expr_span_of(recv)) {
            Some(juxc_tycheck::Ty::User { name, .. }) => {
                Some(name.rsplit('.').next().unwrap_or(name).to_string())
            }
            _ => None,
        }
    }

    /// Return how many `__parent` hops separate the wrapper class that
    /// `recv` evaluates to from the class that declares the instance
    /// field `field_name`. `Some(0)` = the field is on the receiver's
    /// own class; `Some(1)` = its direct parent; and so on. `None` when
    /// no instance field of that name exists anywhere up the chain —
    /// which is exactly the method-call-callee case (`obj.method(...)`,
    /// where the method lives on the wrapper newtype, not inside any
    /// `C_Inner`), so the caller skips the `.0.borrow()` rewrite.
    ///
    /// The owning class is resolved the same way
    /// [`Self::receiver_is_wrapper_class`] resolves it: `this` / `self`
    /// map to `enclosing_class`; everything else consults `expr_types`.
    /// The `__parent`-embedding scheme flattens the whole chain into a
    /// single `C_Inner`, so the depth directly indexes the nested slot.
    pub(crate) fn wrapper_field_parent_depth(&self, recv: &Expr, field_name: &str) -> Option<usize> {
        let class_bare: Option<String> = if matches!(recv, Expr::This(_)) {
            self.enclosing_class.clone()
        } else {
            // Use the collision-immune resolver so interpolated-string
            // receivers (`${i.field}`) still walk the chain correctly.
            self.receiver_class_bare(recv)
        };
        let mut cursor: Option<String> = class_bare;
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return None;
            }
            let sig = self.lookup_class_by_bare_or_fqn(&name)?;
            if let Some(field) = sig.fields.get(field_name) {
                if !field.is_static {
                    return Some(depth);
                }
                // A static field of that name lives on the class, not
                // the instance — not a `.0.borrow()` target.
                return None;
            }
            // Climb to the parent's bare name and try again.
            cursor = sig
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
            depth += 1;
        }
        None
    }

    /// For a wrapper-field place `recv.field_name`, return the **target
    /// class's bare name** when that field is a `weak` field (§6.5), else
    /// `None`. Resolves the owning class exactly like
    /// [`Self::wrapper_field_parent_depth`] (`this`/`self` → `enclosing_class`,
    /// everything else via the receiver's recorded type) and walks the
    /// `extends` chain. The returned bare name is the weak field's target
    /// class — used to re-wrap the upgraded `Rc<RefCell<Target_Inner>>` back
    /// into its `Target` newtype on `.get()`, and (presence alone) to switch
    /// a store to the `Rc::downgrade` form.
    pub(crate) fn wrapper_weak_field_target(&self, recv: &Expr, field_name: &str) -> Option<String> {
        let class_bare: Option<String> = if matches!(recv, Expr::This(_)) {
            self.enclosing_class.clone()
        } else {
            self.receiver_class_bare(recv)
        };
        let mut cursor: Option<String> = class_bare;
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return None;
            }
            let sig = self.lookup_class_by_bare_or_fqn(&name)?;
            if let Some(field) = sig.fields.get(field_name) {
                if field.is_weak {
                    return field.ty.name.segments.last().map(|s| s.text.clone());
                }
                return None;
            }
            cursor = sig
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
            depth += 1;
        }
        None
    }

    /// True when an instance field named `field_name` on the wrapper
    /// class that `recv` evaluates to is non-`Copy` (so a read through a
    /// `.0.borrow()` guard must be cloned out — §CR.4.1 statement-scoped
    /// borrow discipline). Resolves the owning class the same way
    /// [`Self::wrapper_field_parent_depth`] does — `this`/`self` map to
    /// `enclosing_class`, everything else to the receiver's recorded
    /// type — then walks the `extends` chain to find the field's declared
    /// [`Ty`] (generic-params-aware, so a `T`-typed field lands as
    /// [`Ty::Param`]). This complements the span-keyed
    /// [`Self::field_read_needs_clone`], which can miss when the receiver
    /// is `this` (no `expr_types` entry for the `This` node).
    pub(crate) fn wrapper_field_read_needs_clone(&self, recv: &Expr, field_name: &str) -> bool {
        let class_bare: Option<String> = if matches!(recv, Expr::This(_)) {
            self.enclosing_class.clone()
        } else {
            self.receiver_class_bare(recv)
        };
        let Some(start) = class_bare else { return false };
        // Walk `start`'s extends chain for the field, mapping a
        // type-parameter field to `Ty::Param` via the owning class's
        // generic-params list.
        let mut cursor: Option<String> = Some(start);
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return false;
            }
            let Some(sig) = self.lookup_class_by_bare_or_fqn(&name) else {
                return false;
            };
            if let Some(field) = sig.fields.get(field_name) {
                if field.is_static {
                    return false;
                }
                let params: std::collections::HashSet<&str> = sig
                    .generic_params
                    .iter()
                    .map(|p| p.name.text.as_str())
                    .collect();
                let ty = crate::exprs::ty_kind_from_ref_with_params(&field.ty, &params);
                return self.ty_needs_clone_on_field_read(&ty);
            }
            cursor = sig
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
            depth += 1;
        }
        false
    }

    /// **Wrapper-class share-on-read (§CR.4.1, generalized).** True when
    /// `expr` is a **place of wrapped-class type** that, when emitted in
    /// a value/move position (array element, call argument, `return`,
    /// assignment RHS, …), must get a trailing `.clone()` so the move
    /// becomes a cheap `Rc` refcount bump (a SHARED reference to the same
    /// `RefCell`) instead of a destructive move out of the place.
    ///
    /// Recognized places — each must resolve (via `expr_types` / the
    /// symbol-table class chain, bare-last-segment keyed) to a class in
    /// the emitter's [`wrapper set`](RustEmitter::wrapper_classes):
    ///
    /// - **`Path`** naming a wrapped-class local/param (`x` in
    ///   `vec![x]`).
    /// - **`This`** inside a wrapped-class body (`vec![this]`).
    /// - **`Index`** read whose array's element type is a wrapped class
    ///   (`xs[0]` in `var r = xs[0]`). Resolved through the array
    ///   expression's recorded [`Ty::Array`] element.
    ///
    /// **Excluded by construction.** A `Field` access of a wrapped class
    /// already gets its `.clone()` from [`Self::emit_field`]'s class-field
    /// auto-clone, so it is NOT reported here (a value-position caller
    /// must not double-clone). Method-call *receivers* and lvalues never
    /// flow through the value-position callers that consult this helper,
    /// so they're excluded too — only owning positions ask.
    ///
    /// The callers append the `.clone()` AFTER emitting `expr`; this
    /// helper makes no decision about borrow-context flags (those callers
    /// are already in a move position by definition).
    pub(crate) fn wrapper_value_needs_clone(&self, expr: &Expr) -> bool {
        match expr {
            // Bare local/param reference of wrapped-class type.
            Expr::Path(_) | Expr::This(_) => {
                if matches!(expr, Expr::This(_)) {
                    // `this` is a wrapped place only inside a wrapped
                    // class's own (non-constructor) body — reuse the
                    // same gate `receiver_is_wrapper_class` uses.
                    return self.receiver_is_wrapper_class(expr);
                }
                if let Some(ty) = self.expr_types.get(&expr_span_of(expr)) {
                    // A **generic type-parameter**-typed place (`T x`) carries a
                    // `Clone` bound in the emitted Rust, and at instantiation `T`
                    // may be a non-`Copy` wrapper/struct — so a reused place must
                    // `.clone()` rather than move (using it twice would be Rust
                    // `E0382 use of moved value`). `.clone()` is always sound:
                    // every emitted generic param is `T: Clone`.
                    if matches!(ty, juxc_tycheck::Ty::Param(_)) {
                        return true;
                    }
                    if let juxc_tycheck::Ty::User { name, .. } = ty {
                        let bare = name.rsplit('.').next().unwrap_or(name);
                        // Async-runtime handles are Arc-backed —
                        // passing one shares it (refcount bump), the
                        // same rule wrapper places follow. Atomic
                        // counters share the same way (§S.6.2).
                        if matches!(
                            bare,
                            "Channel" | "AsyncMutex" | "AtomicInt" | "AtomicLong"
                        ) {
                            return true;
                        }
                        return self.wrapper_classes.contains(bare);
                    }
                }
                // Span-collision fallback: a bare `Path` local that the
                // span-keyed `expr_types` missed (interp-string reparse)
                // still resolves through the name-keyed `local_types`
                // that `receiver_class_bare` consults first.
                if let Some(bare) = self.receiver_class_bare(expr) {
                    return self.wrapper_classes.contains(&bare);
                }
                // Generic-param local that the span-keyed `expr_types` lacks
                // (e.g. an argument inside a generic-receiver method call,
                // where the callee's param type can't be inferred) — consult
                // the name-keyed `local_types` for a `Ty::Param`.
                if let Expr::Path(qn) = expr {
                    if qn.segments.len() == 1 {
                        if let Some(juxc_tycheck::Ty::Param(_)) = self
                            .local_types
                            .iter()
                            .rev()
                            .find_map(|scope| scope.get(qn.segments[0].text.as_str()))
                        {
                            return true;
                        }
                    }
                }
                false
            }
            // Index read whose element type is a wrapped class.
            Expr::Index(i) => self.index_element_is_wrapper_class(&i.array),
            _ => false,
        }
    }

    /// True iff `array_expr` has a recorded array type whose element is a
    /// wrapped-shape class. Drives the index-read `.clone()` decision in
    /// [`Self::wrapper_value_needs_clone`]: `xs[i]` reads a SHARED handle
    /// out of `Vec<C>` / `[C; N]`, so a value-position use must clone it
    /// (a move would be `E0507 cannot move out of index`).
    fn index_element_is_wrapper_class(&self, array_expr: &Expr) -> bool {
        let Some(juxc_tycheck::Ty::Array { element, .. }) =
            self.expr_types.get(&expr_span_of(array_expr))
        else {
            return false;
        };
        if let juxc_tycheck::Ty::User { name, .. } = element.as_ref() {
            let bare = name.rsplit('.').next().unwrap_or(name);
            return self.wrapper_classes.contains(bare);
        }
        false
    }

    /// Resolve the [`juxc_ast::ClassDecl`] (from `class_asts`) a
    /// receiver expression evaluates to, by bare or FQN name. `this`
    /// maps to the `enclosing_class`. Used by the property-write path
    /// to find a property's accessor metadata.
    pub(crate) fn receiver_class_ast(
        &self,
        recv: &Expr,
    ) -> Option<&juxc_ast::ClassDecl> {
        let bare = if matches!(recv, Expr::This(_)) {
            self.enclosing_class.clone()?
        } else {
            self.receiver_class_bare(recv)?
        };
        self.lookup_class_ast_by_bare_or_fqn(&bare)
    }

    /// Bare-or-FQN lookup into `class_asts` (the AST `ClassDecl`s the
    /// backend cached up front). Direct key hit first, then a
    /// last-segment scan — mirroring `lookup_class_by_bare_or_fqn`.
    pub(crate) fn lookup_class_ast_by_bare_or_fqn(
        &self,
        name: &str,
    ) -> Option<&juxc_ast::ClassDecl> {
        if let Some(c) = self.class_asts.get(name) {
            return Some(c);
        }
        self.class_asts
            .iter()
            .find(|(k, _)| k.rsplit('.').next().unwrap_or(k.as_str()) == name)
            .map(|(_, c)| c)
    }

    /// Find a property named `prop_name` declared on the class that
    /// `recv` evaluates to. Returns the `PropertyDecl` so callers can
    /// consult its accessor shape (read-only / settable / init-only).
    /// Does not walk the `extends` chain today — properties on a base
    /// class are accessed through the subclass's flattened inner, but
    /// the property-write rewrite only needs the declaring class's own
    /// list (Phase-1 scope, matching the field-write path).
    pub(crate) fn property_on_receiver(
        &self,
        recv: &Expr,
        prop_name: &str,
    ) -> Option<&juxc_ast::PropertyDecl> {
        // Static receiver: `Class.Prop` where `Class` is a path
        // resolving to a known class. The property's accessors are
        // static methods on that class.
        if let Expr::Path(qn) = recv {
            if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                if let Some(class) = self.lookup_class_ast_by_bare_or_fqn(
                    crate::backend_fqn::fqn_bare(&class_fqn),
                ) {
                    return class
                        .properties
                        .iter()
                        .find(|p| p.name.text == prop_name && p.is_static);
                }
            }
        }
        let class = self.receiver_class_ast(recv)?;
        class.properties.iter().find(|p| p.name.text == prop_name)
    }

    pub(crate) fn emit_safe_field(&mut self, f: &FieldExpr) {
        let needs_parens = receiver_needs_parens(&f.object);
        if needs_parens {
            self.w.push('(');
        }
        self.emit_expr(&f.object);
        if needs_parens {
            self.w.push(')');
        }
        let combinator = if self.safe_field_is_nullable(f) {
            ".as_ref().and_then(|__t| __t."
        } else {
            ".as_ref().map(|__t| __t."
        };
        self.w.push_str(combinator);
        self.w.push_str(&f.field.text);
        self.w.push_str(".clone())");
    }

    /// True iff the field named by `f` is declared `T?` on the
    /// receiver's class/record. Used by `emit_safe_field` to
    /// pick between `.map` (non-nullable field) and
    /// `.and_then` (nullable field; flattens
    /// `Option<Option<T>>`).
    ///
    /// Resolution: tycheck records the receiver's full type in
    /// `expr_types`. For `obj.inner?.note`, the receiver of
    /// `?.note` is `obj.inner` which infers to
    /// `Ty::Nullable(Inner)`; we peel the nullable wrap before
    /// looking up the field. Missing info (unrecognized class,
    /// unknown field) returns false — `.map` is the safer default
    /// when in doubt; Rust surfaces real shape mismatches.
    fn safe_field_is_nullable(&self, f: &FieldExpr) -> bool {
        let object_ty = self.expr_types.get(&crate::exprs::expr_span_of(&f.object));
        let receiver_name = match object_ty {
            Some(juxc_tycheck::Ty::Nullable(inner)) => match inner.as_ref() {
                juxc_tycheck::Ty::User { name, .. } => name.as_str(),
                _ => return false,
            },
            Some(juxc_tycheck::Ty::User { name, .. }) => name.as_str(),
            _ => return false,
        };
        if let Some(class) = self.symbols.classes.get(receiver_name) {
            if let Some(field) = class.fields.get(&f.field.text) {
                return field.ty.nullable;
            }
        }
        if let Some(record) = self.symbols.records.get(receiver_name) {
            if let Some(c) = record.components.iter().find(|c| c.name == f.field.text) {
                return c.ty.nullable;
            }
        }
        false
    }

    /// Decide whether a `.clone()` should follow a field read.
    ///
    /// Looks up the field expression's recorded `Ty` in `expr_types`:
    /// `Ty::String` or `Ty::Param(_)` require the clone (matching the
    /// two cases the old name-based pre-pass tagged). Everything else
    /// — primitives, user types, arrays — gets no clone, since their
    /// Rust counterparts are `Copy` or already passed by value.
    ///
    /// **Fallback.** When the field's type isn't in `expr_types` (the
    /// expression wasn't visited, or carries a dummy span), we fall
    /// back to looking the field up directly in `symbols.classes` /
    /// `symbols.records` via [`Self::lookup_field_type`]. If that also
    /// misses, we return `false` — the safer default in the absence of
    /// type info, since unnecessary clones on non-`Clone` types would
    /// fail to compile, while a missing clone on a `Clone` type usually
    /// just shifts a move-error around but keeps emitted Rust valid.
    pub(crate) fn field_read_needs_clone(&self, f: &FieldExpr) -> bool {
        // Resolve the field's declared type through the symbol table
        // by way of the receiver's recorded type. This is more
        // reliable than a direct `expr_types.get(&f.span)` lookup
        // because the latter is keyed by an absolute source span, and
        // interpolated-string segments (`$"… ${expr} …"`) reparse
        // their inner expressions against the segment substring —
        // those inner expressions carry spans local to the substring,
        // so several distinct interpolation sites can collide on the
        // same key in `expr_types`. Verifying via the field-name
        // lookup on the receiver's class/record signature side-steps
        // the collision: a stale receiver type just means the field
        // lookup fails and we fall back to "no clone."
        if let Some(ty) = self.lookup_field_type(f) {
            return self.ty_needs_clone_on_field_read(&ty);
        }
        false
    }

    /// True iff a field of type `ty` should auto-`.clone()` on read.
    /// Catches the standard non-`Copy` cases: `String`, generic
    /// parameters (always conservatively cloned), records (records
    /// derive `Clone` but not `Copy` unless every component is
    /// primitive — returning by value would otherwise move out of
    /// `&self`), and class references (classes always derive
    /// `Clone`, never `Copy`).
    fn ty_needs_clone_on_field_read(&self, ty: &Ty) -> bool {
        match ty {
            Ty::String | Ty::Param(_) => true,
            Ty::User { name, .. } => {
                // The `Ty::User { name }` here can be either an FQN
                // (multi-package programs) or a bare class name
                // (`ty_kind_from_ref_with_params` doesn't resolve
                // FQNs from a TypeRef). Try direct lookup, then
                // fall back to a suffix scan on each kind of
                // user-type slot in the symbol table.
                let resolve_record = || -> Option<&juxc_tycheck::symbol_table::RecordSig> {
                    self.symbols.records.get(name.as_str()).or_else(|| {
                        self.symbols
                            .records
                            .iter()
                            .find(|(k, _)| {
                                k.rsplit('.').next().unwrap_or(k.as_str()) == name.as_str()
                            })
                            .map(|(_, v)| v)
                    })
                };
                if let Some(record) = resolve_record() {
                    let all_copy = record
                        .components
                        .iter()
                        .all(|c| crate::analysis::field_supports_copy(&c.ty));
                    return !all_copy;
                }
                // Class / enum / unknown user type — always clone
                // (classes derive Clone, never Copy; enums derive
                // Clone via the auto-derive set).
                let class_hit = self.symbols.classes.contains_key(name.as_str())
                    || self.symbols.classes.keys().any(|k| {
                        k.rsplit('.').next().unwrap_or(k.as_str()) == name.as_str()
                    });
                let enum_hit = self.symbols.enums.contains_key(name.as_str())
                    || self.symbols.enums.keys().any(|k| {
                        k.rsplit('.').next().unwrap_or(k.as_str()) == name.as_str()
                    });
                class_hit || enum_hit
            }
            _ => false,
        }
    }

    /// Resolve a field access's declared type via the symbol table.
    /// Walks `f.object`'s recorded type to find the owning class /
    /// record, then looks up `f.field.text` on it. Returns `None` for
    /// anything we can't resolve (non-user-typed receiver, missing
    /// entries, etc.).
    ///
    /// Phase H: this replaces the heuristic `string_field_names` /
    /// `generic_field_names` sets that used to drive the
    /// `.clone()` / `.to_string()` decision. The new path keys on the
    /// receiver's class/record name, which means same-named fields on
    /// unrelated classes are correctly distinguished. The class's own
    /// generic-params list flows into [`ty_kind_from_ref_with_params`]
    /// so a single-segment name matching a type param (e.g. `T` in
    /// `class Box<T> { T value; }`) lands as [`Ty::Param`] rather than
    /// the misleading `Ty::User { name: "T", … }`.
    pub(crate) fn lookup_field_type(&self, f: &FieldExpr) -> Option<Ty> {
        let object_ty = self.expr_types.get(&expr_span_of(&f.object))?;
        let Ty::User { name, .. } = object_ty else {
            return None;
        };
        // Class field — walk the inheritance chain. The chain walk
        // is generic-params-aware so a class field of type `T`
        // resolves to `Ty::Param("T")` instead of `Ty::User`.
        if let Some(ty) = self.lookup_class_field_ty_in_chain(name, &f.field.text) {
            return Some(ty);
        }
        // Record component — pull the record's own generic params for
        // the same param-vs-user distinction class fields get.
        if let Some(record) = self.symbols.records.get(name) {
            if let Some(c) = record.components.iter().find(|c| c.name == f.field.text) {
                let params: std::collections::HashSet<&str> = record
                    .generic_params
                    .iter()
                    .map(|p| p.name.text.as_str())
                    .collect();
                return Some(ty_kind_from_ref_with_params(&c.ty, &params));
            }
        }
        None
    }

    /// Walk the `extends` chain of `class_name` to find a field by
    /// name, returning its declared [`Ty`]. Mirrors the lookup tycheck
    /// name, returning its declared [`Ty`]. Mirrors the lookup tycheck
    /// does in `check::Checker::lookup_field_in_chain`. The class's
    /// own generic-params list flows through
    /// [`ty_kind_from_ref_with_params`] so single-segment names
    /// matching a type parameter resolve to [`Ty::Param`]; everything
    /// else falls through to the primitive / String / user-type
    /// branches.
    fn lookup_class_field_ty_in_chain(&self, class_name: &str, field_name: &str) -> Option<Ty> {
        let mut cursor: Option<&str> = Some(class_name);
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return None;
            }
            let class = self.symbols.classes.get(name)?;
            if let Some(field) = class.fields.get(field_name) {
                let params: std::collections::HashSet<&str> = class
                    .generic_params
                    .iter()
                    .map(|p| p.name.text.as_str())
                    .collect();
                return Some(ty_kind_from_ref_with_params(&field.ty, &params));
            }
            cursor = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()));
            depth += 1;
        }
        None
    }
}

/// True when emitting `expr` as the receiver of a method call (e.g.
/// `expr.len()`) requires wrapping it in parentheses to keep the
/// `.` binding correct. Atoms — identifiers, `this`, field-chains,
/// method calls, indexes — bind tighter than `.` already, so
/// they're paren-free. Composite shapes (binary ops, ranges,
/// switch-as-expression, lambdas) bind looser and need the
/// wrapping.
fn receiver_needs_parens(e: &Expr) -> bool {
    !matches!(
        e,
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
    )
}

/// Unwrap one layer of `T?` so a nullable receiver (`PathBuf?`) resolves to its
/// underlying user type for the §G.9.2 external-member check.
fn strip_nullable(ty: Ty) -> Ty {
    match ty {
        Ty::Nullable(inner) => *inner,
        other => other,
    }
}

/// Convert a Jux `camelBack` member name back to the foreign symbol's real
/// `snake_case` Rust spelling (the inverse of bindgen's §G.4 `snake→camel`):
/// `asPath` → `as_path`, `isEmpty` → `is_empty`, `withCapacity` → `with_capacity`.
/// Used only for members on external (`rust.std` / crate) receivers (§G.9.2).
fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.char_indices() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Maps a `<primitive-type-name>.<CONSTANT>` access (§K.11) to the Rust
/// associated constant, or `None` when the pair isn't a numeric constant.
/// Float `MIN_VALUE` is the smallest POSITIVE value per the spec (Java's
/// `Double.MIN_VALUE` convention), which is Rust's `MIN_POSITIVE`.
pub(crate) fn numeric_constant(type_name: &str, field: &str) -> Option<String> {
    let rust_ty = match type_name {
        "byte" => "i8",
        "ubyte" => "u8",
        "short" => "i16",
        "ushort" => "u16",
        "int" => "isize",
        "uint" => "usize",
        "long" => "i64",
        "ulong" => "u64",
        "float" => "f32",
        "double" => "f64",
        "i8" => "i8",
        "u8" => "u8",
        "i16" => "i16",
        "u16" => "u16",
        "i32" => "i32",
        "u32" => "u32",
        "i64" => "i64",
        "u64" => "u64",
        "f32" => "f32",
        "f64" => "f64",
        _ => return None,
    };
    let is_float = matches!(rust_ty, "f32" | "f64");
    let rust_const = match field {
        "MIN_VALUE" if is_float => "MIN_POSITIVE",
        "MIN_VALUE" => "MIN",
        "MAX_VALUE" => "MAX",
        "NAN" if is_float => "NAN",
        "POSITIVE_INFINITY" if is_float => "INFINITY",
        "NEGATIVE_INFINITY" if is_float => "NEG_INFINITY",
        "EPSILON" if is_float => "EPSILON",
        _ => return None,
    };
    Some(format!("{rust_ty}::{rust_const}"))
}
