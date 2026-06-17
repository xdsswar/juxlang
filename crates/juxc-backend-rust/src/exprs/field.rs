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
        // Take-and-clear the method-RECEIVER marker (S7): set when this
        // field read is the receiver place of a method call
        // (`h.item` in `h.item.set(x)`). Suppresses the plain-read
        // auto-`.clone()` below so the call borrows the place in-place
        // instead of mutating a discarded copy.
        let is_method_receiver = std::mem::take(&mut self.emitting_method_receiver);
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
        // §P.3.2: `<prop>.observers.clear` / `<prop>.observers.size` —
        // parenthesis-free command accessors on a property's observer
        // namespace. Routed before the generic field logic (the chain
        // would otherwise read like fields on the property's VALUE).
        if matches!(f.field.text.as_str(), "clear" | "size") {
            if let Expr::Field(obsf) = &*f.object {
                if obsf.field.text == "observers" {
                    if let Some((recv, prop, _)) = self.resolve_observable_prop(&obsf.object) {
                        let op = f.field.text.clone();
                        self.emit_observers_command(recv, &prop, &op);
                        return;
                    }
                    // P7: `Config.Level.observers.size` / `.clear` —
                    // class-scoped static observer storage.
                    if let Some((class, prop)) =
                        self.resolve_static_observable_prop(&obsf.object)
                    {
                        let op = f.field.text.clone();
                        self.emit_static_observers_command(&class, &prop, &op);
                        return;
                    }
                }
            }
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
                            // Non-private fields get a `__get_`/`__set_` accessor
                            // on the `Kind` trait (see the field-hook gate in
                            // classes.rs); private fields stay struct-local.
                            !matches!(fsig.visibility, juxc_ast::Visibility::Private)
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
        // receiver uses the foreign symbol's REAL Rust name. Bindgen surfaces
        // Rust names verbatim (§G.4), so the Jux spelling already IS the Rust
        // spelling — the only transform needed is `r#`-escaping a name that is a
        // Rust keyword (`p.r#match()`). The call path appends `(args)` after this
        // returns; a plain field access is complete here. External types are
        // plain Rust values (not the Rc/RefCell wrapper representation), so none
        // of the `.0.borrow()` rewrites apply.
        if let Some(Ty::User { name, .. }) = self
            .expr_types
            .get(&expr_span_of(&f.object))
            .cloned()
            .map(strip_nullable)
        {
            // **`this` is never external.** A `this`-rooted read is always the
            // enclosing user class instance — never a bare `rust.std` value —
            // so it takes the wrapper-class path below, not this one. The guard
            // matters because a desugared accessor body (`this.__prop_X`)
            // carries the PROPERTY's span on its `this`, and tycheck recorded
            // that span with the property's (possibly external, e.g. `Vec`)
            // type; without the guard the span collision misroutes
            // `this.__prop_X` here and emits a raw `self.__prop_X`, skipping the
            // `.0.borrow()` the `Rc<RefCell>` rep needs (rustc E0609).
            let external = if matches!(&*f.object, Expr::This(_)) {
                false
            } else if self.symbols.classes.contains_key(&name) {
                self.symbols.classes.get(&name).map(|c| c.is_external).unwrap_or(false)
            } else {
                self.lookup_class_by_bare_or_fqn(name.rsplit('.').next().unwrap_or(&name))
                    .map(|c| c.is_external)
                    .unwrap_or(false)
            };
            if external {
                // `.length` is Jux's universal collection-size field. A
                // rust.std collection (Vec/VecDeque/HashSet/HashMap/…) exposes
                // its size as `.len()`, so mirror the array facade and emit
                // `<recv>.len() as isize` rather than the verbatim `.length`
                // (which is not a real Rust field — rustc E0609). Only a plain
                // read; a `.length()` call callee falls through to verbatim.
                if f.field.text == "length" && !is_call_callee {
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
                // When this Field is a method-call CALLEE, its object
                // is the method's RECEIVER — a place, not a value:
                //  - suppress the collection-field auto-`.clone()`
                //    (cloning the receiver would mutate a throwaway
                //    copy — `this.slots.push(10)` silently lost the
                //    push);
                //  - for MUTATING methods (discovered from the stub's
                //    `@MutSelf` marker — the real Rust `&mut self`
                //    signature), read a wrapper field through
                //    `borrow_mut()` so the mutation lands in the real
                //    cell (rustc E0596 otherwise).
                let mutates = is_call_callee
                    && self.external_method_mutates_receiver(&name, &f.field.text);
                let prev_recv = self.emitting_method_receiver;
                let prev_out = self.emitting_out_place;
                let prev_lv = self.emitting_lvalue;
                if is_call_callee {
                    self.emitting_method_receiver = true;
                    if mutates {
                        self.emitting_out_place = true;
                        self.emitting_lvalue = true;
                    }
                }
                self.emit_expr(&f.object);
                self.emitting_method_receiver = prev_recv;
                self.emitting_out_place = prev_out;
                self.emitting_lvalue = prev_lv;
                self.w.push('.');
                self.w.push_str(&crate::backend_fqn::to_rust_ident(&f.field.text));
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
                // Walk the `extends` chain (`symbols.lookup_method`),
                // not just the receiver's own class — `this.Score`
                // inside a SUBCLASS override must still route to the
                // getter when `Score` is a property of a base class.
                // `lookup_method` wants the symbol-table key, which may
                // be the FQN; resolve the bare name first.
                let class_key = if self.symbols.classes.contains_key(bare) {
                    Some(bare.to_string())
                } else {
                    self.symbols.find_fqn_by_bare(bare)
                };
                let is_property = class_key
                    .and_then(|k| {
                        self.symbols
                            .lookup_method(&k, f.field.text.as_str())
                            .map(|(m, _)| m.is_property)
                    })
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
        // Receiver-place propagation (S7): when this field is the
        // method name of a call (`is_call_callee`), its object is the
        // RECEIVER — mark it so its read skips the auto-`.clone()`.
        // A plain receiver field in a longer chain (`h.a.b.set()`)
        // propagates the marker down so the whole place stays
        // clone-free; a wrapper-borrowed link does not (its clone-out
        // of the `Ref` guard is required, and for wrapper-class
        // fields the clone is an `Rc` share anyway).
        self.emitting_method_receiver =
            is_call_callee || (is_method_receiver && wrapper_depth.is_none());
        // A composite receiver (notably a raw-pointer deref `*q`) must be
        // parenthesized before `.field`, or `(*q).x` would emit as `*q.x`
        // (= `*(q.x)`). Atoms (path, `this`, field/call/index chains) don't.
        let recv_parens = receiver_needs_parens(&f.object);
        if recv_parens {
            self.w.push('(');
        }
        self.emit_expr(&f.object);
        if recv_parens {
            self.w.push(')');
        }
        self.emitting_method_receiver = false;
        if let Some(depth) = wrapper_depth {
            // `RcRefCell` rep: reach the interior through a statement-scoped
            // borrow. The bare-`Rc` rep (read-only shared, never mutated) has no
            // cell — `Rc<C_Inner>` derefs straight to the fields, so emit `.0`
            // with no borrow.
            if self.receiver_is_refcell_class(&f.object) {
                // An `out` field place needs an exclusive `&mut` into the
                // interior, so take the mutable borrow; the `RefMut` temporary
                // lives to the end of the call statement (§M.4).
                if self.emitting_out_place {
                    self.w.push_str(".0.borrow_mut()");
                } else {
                    self.w.push_str(".0.borrow()");
                }
            } else {
                self.w.push_str(".0");
            }
            for _ in 0..depth {
                self.w.push_str(".__parent");
            }
        }
        self.w.push('.');
        self.w.push_str(&f.field.text);
                        if let Some(sfx) = &method_suffix { self.w.push_str(sfx); }
        // `ref` field READ (§M.13): the slot is a shared cell — a
        // value-position read clones the VALUE out (statement-scoped
        // borrow). Writes never reach here (`emit_assign`'s
        // store-through arm intercepts the whole statement).
        if !is_call_callee
            && !self.emitting_lvalue
            && self.field_decl_is_ref(&f.object, &f.field.text)
        {
            if self.emitting_ref_handle {
                // Aliasing pass into a `ref` parameter — share the
                // HANDLE, not the value.
                self.w.push_str(".clone()");
            } else {
                self.w.push_str(".borrow().clone()");
            }
            return;
        }
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
        // S7: a plain (non-wrapper-borrow) field read serving as a
        // method-call receiver place must stay clone-free — the call
        // borrows the place directly (`h.item.set(x)`), and a clone
        // would silently discard `&mut self` mutations. The
        // wrapper-borrow clone is NOT suppressed: cloning out of the
        // statement-scoped `Ref` guard is mandatory there.
        let receiver_place_read = is_method_receiver && wrapper_depth.is_none();
        if !callee_is_method
            && (wrapper_borrow_clone
                || (!self.emitting_lvalue
                    && !in_borrow_context
                    && !receiver_place_read
                    && self.field_read_needs_clone(f)))
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

    /// Like [`Self::receiver_is_wrapper_class`] but true only for the
    /// **interior-mutable** (`Rc<RefCell>`) rep — i.e. the receiver's class
    /// needs a `.0.borrow()` to reach a field. A bare-`Rc` (read-only shared)
    /// receiver is a wrapper but NOT refcell: its fields read through plain
    /// `.0` with no borrow. Gates the borrow rewrite in `emit_field`/`emit_assign`.
    pub(crate) fn receiver_is_refcell_class(&self, recv: &Expr) -> bool {
        if matches!(recv, Expr::This(_)) {
            return self.emitting_wrapper_class
                && self
                    .enclosing_class
                    .as_deref()
                    .map(|c| self.refcell_classes.contains(c))
                    .unwrap_or(false);
        }
        self.receiver_class_bare(recv)
            .map(|bare| self.refcell_classes.contains(&bare))
            .unwrap_or(false)
    }

    /// True when `recv`'s class is the `Box` rep (unique owner, `C(Box<C_Inner>)`).
    /// Such a receiver's `.0` is a `Box`, not an `Rc`, so identity (`===`) must
    /// use `std::ptr::eq` rather than `Rc::ptr_eq`.
    pub(crate) fn receiver_is_box_class(&self, recv: &Expr) -> bool {
        if matches!(recv, Expr::This(_)) {
            return self.emitting_wrapper_class
                && self
                    .enclosing_class
                    .as_deref()
                    .map(|c| self.box_classes.contains(c))
                    .unwrap_or(false);
        }
        self.receiver_class_bare(recv)
            .map(|bare| self.box_classes.contains(&bare))
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
                // A generic-INSTANTIATED TypeRef (`Vec<int>`, `HashMap<K,V>`,
                // `ArrayList<int>`, a user `Box<T>`, …) converts to
                // `Ty::Unknown` (see `ty_kind_from_ref_with_params`: any name
                // carrying `generic_args` short-circuits to Unknown). A bare
                // NESTED-type name (`HttpServer.Config`) also lands here. Every
                // such type — collection, generic wrapper, OR wrapper class — is
                // Clone, never Copy, so a value-position read out of the
                // statement-scoped `.0.borrow()` guard MUST clone, or it moves
                // out of the `Ref` (rustc E0507). For a wrapper class the clone
                // is a cheap `Rc` refcount bump (same identity, mutation still
                // hits the real object); for a collection it is the value copy.
                // (A wrapper-class base used to be EXEMPTED on the theory its own
                // `Rc` machinery shared it, but a receiver read through a borrow
                // guard — e.g. the receiver-hoist `let __jux_recv =
                // self.0.borrow().config;` — has no other clone site, so the
                // exemption moved the field out of the guard and failed to
                // compile once every class became `Rc<RefCell>`.)
                if matches!(ty, Ty::Unknown) && field.ty.array_shape.is_none() {
                    return true;
                }
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
    /// True when `expr` is a **place** (variable / field / index) whose static
    /// type is a user **record** — a value type that copies on pass (§7.6). A
    /// record place handed to a by-value parameter must `.clone()` so the caller
    /// keeps its copy (value semantics) and a self-referential call like
    /// `r.plus(r)` doesn't MOVE the receiver out from under its own `&self`
    /// borrow (rustc E0505 / E0382). Records derive `Clone`, so this is sound.
    pub(crate) fn record_place_needs_clone(&self, expr: &Expr) -> bool {
        if !matches!(expr, Expr::Path(_) | Expr::Field(_) | Expr::Index(_)) {
            return false;
        }
        if let Some(juxc_tycheck::Ty::User { name, .. }) =
            self.expr_types.get(&expr_span_of(expr))
        {
            let bare = name.rsplit('.').next().unwrap_or(name);
            return self.symbols.records.contains_key(name)
                || self.symbols.records.contains_key(bare)
                || self
                    .symbols
                    .find_fqn_by_bare(bare)
                    .map_or(false, |fqn| self.symbols.records.contains_key(&fqn));
        }
        false
    }

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
                        // Async-runtime handles are Arc/Rc-backed —
                        // passing one shares it (refcount bump), the
                        // same rule wrapper places follow. Atomic
                        // counters share the same way (§S.6.2);
                        // streams (§18.6) are Rc-backed handles too.
                        if matches!(
                            bare,
                            "Channel" | "AsyncMutex" | "AtomicInt" | "AtomicLong" | "Stream"
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
        // Walk the `extends` chain (Java semantics): a property of an
        // ancestor is a property of the receiver — its setter/getter
        // copies live on the receiver's wrapper via inherited-method
        // inlining, so the write/read routing works identically.
        let mut class = self.receiver_class_ast(recv)?;
        for _ in 0..64 {
            if let Some(p) = class.properties.iter().find(|p| p.name.text == prop_name) {
                return Some(p);
            }
            let parent_bare = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.first().map(|s| s.text.clone()))?;
            class = self.class_ast_by_bare(&parent_bare)?;
        }
        None
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
        // `.and_then` flattens when the projected field is itself `T?`; `.map`
        // otherwise. (`__t` is `&Underlying` from `as_ref()`.)
        let combinator = if self.safe_field_is_nullable(f) {
            ".as_ref().and_then(|__t| "
        } else {
            ".as_ref().map(|__t| "
        };
        self.w.push_str(combinator);
        // Reach the field by rep (§CR.4.1): the interior-mutable `RcRefCell` rep
        // reads through a `.0.borrow()` guard; the bare-`Rc` / `Box` reps keep
        // the `.0` newtype but deref straight to the fields (no borrow); Inline
        // classes and records access the field directly. (A bare `__t.field` on
        // a newtype would hit the tuple wrapper — `available field is: 0`.)
        let bare = self.safe_nav_member_class_bare(&f.object);
        let is_newtype = bare.as_deref().map(|b| self.wrapper_classes.contains(b)).unwrap_or(false);
        let is_refcell = bare.as_deref().map(|b| self.refcell_classes.contains(b)).unwrap_or(false);
        if is_refcell {
            self.w.push_str("__t.0.borrow().");
        } else if is_newtype {
            self.w.push_str("__t.0.");
        } else {
            self.w.push_str("__t.");
        }
        self.w.push_str(&f.field.text);
        self.w.push_str(".clone())");
    }

    /// Resolve the bare class name that an expression used as a `?.` receiver
    /// evaluates to (nullability peeled). Falls back to structural resolution
    /// through field/method chains when `expr_types` has no entry for an
    /// intermediate safe-nav sub-expression — tycheck doesn't record those
    /// spans, so a bare `expr_types` lookup misses on `a.b?.c?.…`.
    pub(crate) fn safe_nav_member_class_bare(&self, obj: &Expr) -> Option<String> {
        // Recorded-type / tracked-local fast path.
        if let Some(bare) = self.receiver_class_bare(obj) {
            return Some(bare);
        }
        match obj {
            // `recv.field` (plain or `?.`): the field's declared type's class.
            Expr::Field(f2) => {
                let owner = self.safe_nav_member_class_bare(&f2.object)?;
                let mut cur = self.lookup_class_by_bare_or_fqn(&owner);
                while let Some(s) = cur {
                    if let Some(fs) = s.fields.get(&f2.field.text) {
                        let n = fs.ty.name.segments.last()?.text.as_str();
                        return Some(n.rsplit('.').next().unwrap_or(n).to_string());
                    }
                    cur = s.extends_fqn.as_deref().and_then(|p| self.symbols.classes.get(p));
                }
                None
            }
            // `recv.method(...)` (plain or `?.`): the method's return type's class.
            Expr::Call(c) => {
                let Expr::Field(m) = c.callee.as_ref() else { return None };
                let recvc = self.safe_nav_member_class_bare(&m.object)?;
                let mut cur = self.lookup_class_by_bare_or_fqn(&recvc);
                while let Some(s) = cur {
                    if let Some(ms) = s.methods.get(&m.field.text) {
                        let (juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t)) =
                            &ms.return_type
                        else {
                            return None;
                        };
                        let n = t.name.segments.last()?.text.as_str();
                        return Some(n.rsplit('.').next().unwrap_or(n).to_string());
                    }
                    cur = s.extends_fqn.as_deref().and_then(|p| self.symbols.classes.get(p));
                }
                None
            }
            _ => None,
        }
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
        // Resolve the receiver's class structurally so nested safe-nav chains
        // (`a.b?.c?.field`) — whose intermediate spans tycheck doesn't record —
        // still pick `.and_then` when the projected field is itself `T?`.
        let Some(recv_bare) = self.safe_nav_member_class_bare(&f.object) else {
            return false;
        };
        let mut cur = self.lookup_class_by_bare_or_fqn(&recv_bare);
        while let Some(class) = cur {
            if let Some(field) = class.fields.get(&f.field.text) {
                return field.ty.nullable;
            }
            cur = class.extends_fqn.as_deref().and_then(|p| self.symbols.classes.get(p));
        }
        // Record fallback: records are FQN-keyed; match on the bare last segment.
        if let Some(record) = self.symbols.records.iter().find_map(|(k, v)| {
            (k.rsplit('.').next().unwrap_or(k) == recv_bare).then_some(v)
        }) {
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
        // Fallback (S15): a `this`-rooted receiver often has no
        // `expr_types` entry, so the span-keyed lookup above fails —
        // resolve through the enclosing class's field table instead
        // (the same extends-chain walk the wrapper path uses).
        if matches!(&*f.object, Expr::This(_)) {
            return self.wrapper_field_read_needs_clone(&f.object, &f.field.text);
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
            // Collection / array fields are non-Copy `Vec`s etc. — a
            // VALUE-position read (`return this.items;`, S15) must
            // clone or it moves out of `&self`. Receiver positions
            // (`this.items.add(x)`, `this.items[0]`) are exempted by
            // the `emitting_method_receiver` marker, so in-place
            // mutation keeps working.
            Ty::Array { .. } => true,
            Ty::User { name, .. } => {
                // The `Ty::User { name }` here can be either an FQN
                // (multi-package programs) or a bare class name
                // (`ty_kind_from_ref_with_params` doesn't resolve
                // FQNs from a TypeRef). Try direct lookup, then
                // fall back to a suffix scan on each kind of
                // user-type slot in the symbol table.
                // Match on the SIMPLE name (last component after both the
                // package separator `.` AND the nested-type separator `__`) of
                // BOTH the key and the queried name. A nested type's name
                // arrives mangled — `HttpServer__Config`, possibly under a
                // package (`nested.HttpServer__Config`) — which is neither a
                // bare name nor a plain dotted FQN, so the old last-`.`-segment
                // compare missed it and a wrapper-class field of a nested type
                // read out of a `.0.borrow()` guard moved instead of cloning
                // (rustc E0507). This predicate only asks "is this a (non-Copy)
                // class/enum/interface/record"; a simple-name match is
                // sufficient (a same-named type in another scope is still a
                // reference type that needs the clone).
                // Local `fn` (not a closure) so each call gets fresh borrow
                // lifetimes — a `|&str| -> &str` closure would unify the
                // input/output lifetimes and reject the cross-lifetime compares
                // below.
                fn simple(k: &str) -> &str {
                    let after_pkg = k.rsplit('.').next().unwrap_or(k);
                    after_pkg.rsplit("__").next().unwrap_or(after_pkg)
                }
                let want = simple(name.as_str());
                let resolve_record = || -> Option<&juxc_tycheck::symbol_table::RecordSig> {
                    self.symbols
                        .records
                        .get(name.as_str())
                        .or_else(|| self.symbols.records.iter().find(|(k, _)| simple(k) == want).map(|(_, v)| v))
                };
                if let Some(record) = resolve_record() {
                    let all_copy = record
                        .components
                        .iter()
                        .all(|c| crate::analysis::field_supports_copy(&c.ty));
                    return !all_copy;
                }
                // Class / enum / interface / nested user type — clone. Classes
                // and enums derive Clone (never Copy); an interface slot is an
                // `Rc<dyn …>` (Clone = handle share). Reading any of them in a
                // value position out of the borrow guard must clone.
                let class_hit = self.symbols.classes.keys().any(|k| simple(k) == want);
                let enum_hit = self.symbols.enums.keys().any(|k| simple(k) == want);
                let iface_hit = self.symbols.interfaces.keys().any(|k| simple(k) == want);
                class_hit || enum_hit || iface_hit
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
        // Strip one layer of nullable so `e.message` where `e: Exception?` (after
        // a null-check smart-cast) still resolves through the class chain.
        let object_ty = match object_ty {
            Ty::Nullable(inner) => inner.as_ref(),
            other => other,
        };
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
    pub(crate) fn lookup_class_field_ty_in_chain(
        &self,
        class_name: &str,
        field_name: &str,
    ) -> Option<Ty> {
        // Use owned String so the cursor isn't tied to a specific ClassSig borrow — the
        // extends chain may cross package boundaries where the bare name stored in
        // `extends` (e.g. "Throwable") differs from the FQN key in `symbols.classes`
        // (e.g. "jux.std.exceptions.Throwable"). `lookup_class_by_bare_or_fqn` resolves
        // both bare and FQN spellings, so the walk succeeds across stdlib hierarchy.
        let mut cursor: Option<String> = Some(class_name.to_string());
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return None;
            }
            let Some(class) = self.lookup_class_by_bare_or_fqn(&name) else {
                break;
            };
            if let Some(field) = class.fields.get(field_name) {
                let params: std::collections::HashSet<&str> = class
                    .generic_params
                    .iter()
                    .map(|p| p.name.text.as_str())
                    .collect();
                let ty = ty_kind_from_ref_with_params(&field.ty, &params);
                // A generic-instantiated TypeRef (`ArrayList<int>`)
                // converts to `Ty::Unknown` — recover the base name
                // when it resolves to a known class so downstream
                // decisions (notably the value-position auto-clone,
                // S15) see a real user type instead of a blind spot.
                if matches!(ty, Ty::Unknown) && field.ty.array_shape.is_none() {
                    if let Some(base) = field.ty.name.segments.last() {
                        if self.lookup_class_by_bare_or_fqn(&base.text).is_some() {
                            return Some(Ty::User {
                                name: base.text.clone(),
                                generic_args: Vec::new(),
                            });
                        }
                    }
                }
                return Some(ty);
            }
            cursor = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
            depth += 1;
        }
        None
    }

    /// Walk the `extends` chain of `class_name` to find a field, returning the
    /// bare name of the class that DECLARES it plus its `(is_static, is_final)`
    /// storage flags. Mirrors [`Self::lookup_class_field_ty_in_chain`] but hands
    /// back what the bare implicit-`this` rewrite needs. Crucially, an INHERITED
    /// field (declared in an ancestor) is found too — so a bare reference to a
    /// base-class field inside a subclass method (whether the subclass's own
    /// method or a copied inherited body, §CR.5) resolves to `this.field`, which
    /// the field emitter then routes through the `__parent` hops. The declaring
    /// class flows back so a bare reference to an inherited STATIC names the
    /// class that actually holds the storage, not the using subclass.
    pub(crate) fn lookup_class_field_owner_in_chain(
        &self,
        class_name: &str,
        field_name: &str,
    ) -> Option<(String, bool, bool)> {
        let mut cursor: Option<String> = Some(class_name.to_string());
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return None;
            }
            let Some(class) = self.lookup_class_by_bare_or_fqn(&name) else {
                break;
            };
            if let Some(field) = class.fields.get(field_name) {
                return Some((name, field.is_static, field.is_final));
            }
            cursor = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
            depth += 1;
        }
        None
    }

    /// True when the field `field_name` of `class_name` or any ancestor is
    /// declared nullable (`T?`). Reads the field's `TypeRef.nullable` flag
    /// directly (not the erased `Ty`, which can drop nullability), so it drives
    /// the assign-time `Some(...)` coercion for both own and inherited fields.
    pub(crate) fn class_field_is_nullable_in_chain(
        &self,
        class_name: &str,
        field_name: &str,
    ) -> bool {
        let mut cursor: Option<String> = Some(class_name.to_string());
        let mut depth = 0usize;
        while let Some(cn) = cursor {
            if depth > 64 {
                return false;
            }
            let Some(class) = self.lookup_class_by_bare_or_fqn(&cn) else {
                return false;
            };
            if let Some(field) = class.fields.get(field_name) {
                return field.ty.nullable;
            }
            cursor = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
            depth += 1;
        }
        false
    }

    /// True when `name` is a PROPERTY of `class_name` or any ancestor (walking
    /// the `extends` chain over the class ASTs). Drives the bare implicit-`this`
    /// rewrite for a property READ: `Title` ≡ `this.Title`, which the field
    /// emitter then routes to the synthesized getter. A property and a field
    /// never share a name (the backing slot is `__prop_<Name>`), so this is
    /// consulted only after the field lookup misses.
    pub(crate) fn bare_name_is_property_in_chain(&self, class_name: &str, name: &str) -> bool {
        let mut cursor: Option<String> = Some(class_name.to_string());
        let mut depth = 0usize;
        while let Some(cn) = cursor {
            if depth > 64 {
                return false;
            }
            let Some(cd) = self.lookup_class_ast_by_bare_or_fqn(&cn) else {
                return false;
            };
            if cd.properties.iter().any(|p| p.name.text == name) {
                return true;
            }
            cursor = cd
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
            depth += 1;
        }
        false
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

impl RustEmitter {
    /// Is `field_name` of the class `object_expr` evaluates to a `ref`
    /// field (§M.13 — an `Rc<RefCell<T>>` shared-reference cell)?
    /// Walks the `extends` chain; `this`/ctor-inner receivers resolve
    /// through the enclosing class.
    pub(crate) fn field_decl_is_ref(
        &self,
        object_expr: &Expr,
        field_name: &str,
    ) -> bool {
        let bare = if matches!(object_expr, Expr::This(_)) {
            self.enclosing_class.clone()
        } else {
            self.receiver_class_bare(object_expr)
                .or_else(|| self.enclosing_class.clone())
        };
        let Some(mut bare) = bare else { return false };
        for _ in 0..64 {
            let Some(cd) = self.class_ast_by_bare(&bare) else { return false };
            if let Some(f) = cd.fields.iter().find(|f| f.name.text == field_name) {
                return f.is_ref;
            }
            match cd
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.first().map(|s| s.text.clone()))
            {
                Some(parent) => bare = parent,
                None => return false,
            }
        }
        false
    }

    /// Does calling `method` on a value of the external class
    /// `type_name` MUTATE the receiver? DISCOVERED from the stub's
    /// `@MutSelf` annotation — bindgen records the real Rust
    /// `&mut self` receiver on every generated `.jux.d` method, so
    /// this stays correct when the library surface changes. The
    /// hardcoded name list below is only the fallback for stub caches
    /// generated before the marker existed.
    pub(crate) fn external_method_mutates_receiver(
        &self,
        type_name: &str,
        method: &str,
    ) -> bool {
        let class = self.symbols.classes.get(type_name).or_else(|| {
            self.lookup_class_by_bare_or_fqn(
                type_name.rsplit('.').next().unwrap_or(type_name),
            )
        });
        if let Some(m) = class.and_then(|c| c.methods.get(method)) {
            if m.annotations.iter().any(annotation_is_mut_self) {
                return true;
            }
            // The signature exists but carries no marker — either a
            // genuinely read-only method or a pre-marker stub cache;
            // the name fallback covers the latter.
        }
        rust_std_container_method_mutates(method)
    }
}

/// True when an annotation names the bindgen `@MutSelf` marker
/// (annotations are case-insensitive per the Jux rules).
pub(crate) fn annotation_is_mut_self(a: &juxc_ast::Annotation) -> bool {
    a.name.segments.len() == 1
        && a.name.segments[0].text.eq_ignore_ascii_case("mutself")
}

/// FALLBACK ONLY (see [`RustEmitter::external_method_mutates_receiver`]):
/// known mutating methods of the Rust-std containers, used when a stub
/// predates the `@MutSelf` marker. New/renamed library methods are
/// covered by the marker, not this list.
fn rust_std_container_method_mutates(method: &str) -> bool {
    matches!(
        method,
        "push"
            | "pop"
            | "insert"
            | "remove"
            | "clear"
            | "truncate"
            | "retain"
            | "dedup"
            | "sort"
            | "sort_by"
            | "sort_by_key"
            | "sort_unstable"
            | "sort_unstable_by"
            | "reverse"
            | "extend"
            | "append"
            | "resize"
            | "fill"
            | "swap"
            | "swap_remove"
            | "drain"
            | "split_off"
            | "push_back"
            | "push_front"
            | "pop_back"
            | "pop_front"
            | "rotate_left"
            | "rotate_right"
            | "make_contiguous"
            | "push_str"
            | "shrink_to_fit"
            | "reserve"
    )
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
