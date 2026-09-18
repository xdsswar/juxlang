//! `any` (JUX-TYPE-SYSTEM-ADDENDUM §T.1.2): putting a value in, and getting
//! it back out with `=>`.
//!
//! An `any` lowers to the prelude's `crate::JuxAny`, which holds the value
//! behind `Rc<dyn Any>`. Rust keeps the value's concrete type, so the `=>`
//! test is `held.is::<T>()` and the smart-cast binding is `held.get::<T>()`.
//!
//! Two things cannot be asked of a value from behind `dyn Any`: how `===`
//! compares it, and what it prints. Both are fixed where the value goes in,
//! where its type is still known:
//!
//! - a value type (primitive, `String`, record, struct, enum, tuple) goes in
//!   with `JuxAny::of_value`, and `===` compares values;
//! - a class instance, array or collection goes in with `JuxAny::of_ref`, and
//!   `===` compares the object's address (the `JuxIdentity` every class and
//!   handle carries);
//! - anything else (a foreign value, a type parameter's value) goes in with
//!   `JuxAny::of_opaque`: it is the same only as copies of the same `any`.
//!   (A function value never gets here; the checker refuses it.)
//!
//! The text is the closure `|v| format!("{}", …)` rendering `v` exactly the way
//! `${x}` would, built by the same interpolation renderer.
//!
//! **One value, several Rust shapes.** A class value can arrive as its own
//! struct (`Dog`), or as a trait object for a base (`Rc<dyn AnimalKind>`) or an
//! interface (`Rc<dyn Named>`), depending on the static type it had when it
//! went in. The `=>` test tries every shape a `T` could be held in, each
//! discovered from the program's own classes and interfaces, and turns what it
//! finds into `T`'s own value shape with the ordinary upcast coercion or the
//! runtime-type hook `__jux_as_T`.

use juxc_ast::{Expr, Ident, QualifiedName, TypeRef, TypeTestExpr};
use juxc_source::Span;
use juxc_tycheck::Ty;

use crate::RustEmitter;

/// The span a synthetic read carries. The coercion helpers look an
/// expression's type up by span; no source file has this index, so the entry
/// never collides with a real one.
const SYNTH_SPAN: Span = Span { start: 0, end: 0, file: u32::MAX };

/// How `===` treats a value put into an `any`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AnyKind {
    /// Compared by value.
    Value,
    /// Compared by the object's address.
    Reference,
    /// Compared by the `any` it went into.
    Opaque,
}

/// One Rust shape a value an `=>` test is looking for can be held in, and how
/// to turn it into the target's own value shape.
enum HeldForm {
    /// Held exactly as the target's value type: take it as it is.
    Exact(String),
    /// Held as a class struct, wanted as a trait object: `Rc::new(v) as …`.
    Wrap(String),
    /// Held as a trait object for a SUBtype: a trait upcast, `v as …`.
    Upcast(String),
    /// Held as a class struct, wanted as the struct of a base class: the
    /// ordinary coercion from the Jux type `from`.
    Coerce { rust: String, from: Ty },
    /// Held as a trait object for a SUPERtype: ask its runtime-type hook,
    /// through the trait so the call names one method.
    Hook { rust: String, hook: String },
}

impl HeldForm {
    /// The Rust type the value is held as.
    fn rust(&self) -> &str {
        match self {
            HeldForm::Exact(r) | HeldForm::Wrap(r) | HeldForm::Upcast(r) => r,
            HeldForm::Coerce { rust, .. } | HeldForm::Hook { rust, .. } => rust,
        }
    }
}

/// The trait inside a `std::rc::Rc<dyn Trait>` spelling.
fn dyn_trait(rust: &str) -> &str {
    rust.strip_prefix("std::rc::Rc<dyn ")
        .and_then(|r| r.strip_suffix('>'))
        .unwrap_or(rust)
}

impl RustEmitter {
    /// True when `t` is the built-in `any`: bare, not generic, not an array
    /// or a function type. Nullability is the caller's business.
    pub(crate) fn type_ref_is_any(&self, t: &TypeRef) -> bool {
        t.array_shape.is_none()
            && t.fn_shape.is_none()
            && t.generic_args.is_empty()
            && t.name.segments.len() == 1
            && t.name.segments[0].text == "any"
            && self.lookup_class_by_bare_or_fqn("any").is_none()
    }

    /// True when `expr`'s static type is `any` (or `any?`).
    pub(crate) fn expr_is_any(&self, expr: &Expr) -> bool {
        match self.expr_recorded_ty(expr) {
            Some(Ty::Any) => true,
            Some(Ty::Nullable(inner)) => matches!(*inner, Ty::Any),
            _ => false,
        }
    }

    /// Emit `expr` into a slot of type `target`, which is `any` or `any?`.
    pub(crate) fn emit_expr_into_any(&mut self, target: &TypeRef, expr: &Expr) {
        // `any? x = null;`: nothing to box.
        if matches!(expr, Expr::Literal(juxc_ast::Literal::Null)) {
            self.w.push_str("None");
            return;
        }
        let src = self.any_source_ty(expr);
        // Already an `any`: an `any` is shared like a class handle, so a read
        // of a place the program reads again takes a copy of the handle.
        let already_any = match &src {
            Some(Ty::Any) => Some(false),
            Some(Ty::Nullable(inner)) if matches!(**inner, Ty::Any) => Some(true),
            _ => None,
        };
        if let Some(src_nullable) = already_any {
            if target.nullable && !src_nullable {
                self.w.push_str("Some(");
            }
            self.emit_expr(expr);
            if crate::analysis::expr_is_place_pub(expr) {
                self.w.push_str(".clone()");
            }
            if target.nullable && !src_nullable {
                self.w.push(')');
            }
            return;
        }
        let src = src.unwrap_or(Ty::Unknown);
        let src_nullable = matches!(src, Ty::Nullable(_)) || self.expression_is_already_nullable(expr);
        let inner = match &src {
            Ty::Nullable(t) => (**t).clone(),
            other => other.clone(),
        };
        let kind = self.any_kind_of(&inner, expr);
        if target.nullable && src_nullable {
            // `T?` into `any?`: box the value when there is one.
            self.w.push('(');
            self.emit_expr(expr);
            if crate::analysis::expr_is_place_pub(expr) {
                self.w.push_str(".clone()");
            }
            self.w.push_str(").map(|held| ");
            let read = self.synth_read("held", inner.clone());
            self.emit_any_box(&read, &inner, kind, true);
            self.w.push(')');
            self.expr_types.remove(&SYNTH_SPAN);
            return;
        }
        if target.nullable {
            self.w.push_str("Some(");
        }
        self.emit_any_box(expr, &inner, kind, false);
        if target.nullable {
            self.w.push(')');
        }
    }

    /// The checker's type for a value going into an `any`, with the one gap
    /// filled in: an untyped integer literal is an `int` (§S.2.6).
    fn any_source_ty(&self, expr: &Expr) -> Option<Ty> {
        if juxc_tycheck::infer::untyped_int_literal(expr) {
            return Some(Ty::Primitive(juxc_tycheck::Primitive::Int));
        }
        // A literal's type is in the literal; the checker does not always
        // record one under its span.
        match expr {
            Expr::Literal(lit) => return Some(juxc_tycheck::infer::infer_literal(lit)),
            Expr::InterpString(_) => return Some(Ty::String),
            _ => {}
        }
        self.expr_recorded_ty(expr)
    }

    /// Which constructor a value of type `ty` goes in with.
    fn any_kind_of(&self, ty: &Ty, expr: &Expr) -> AnyKind {
        match ty {
            Ty::Primitive(_) | Ty::String => AnyKind::Value,
            Ty::Array { .. } => AnyKind::Reference,
            Ty::User { name, .. } => {
                let bare = name.rsplit('.').next().unwrap_or(name);
                if name == juxc_ast::TUPLE_SENTINEL || self.type_name_is_value_type(name) {
                    return AnyKind::Value;
                }
                if self.enum_named(bare) || self.struct_named(bare) {
                    return AnyKind::Value;
                }
                if self.expr_is_collection_handle(expr) {
                    return AnyKind::Reference;
                }
                let own_class = self.lookup_class_by_bare_or_fqn(bare).is_some_and(|c| !c.is_external);
                let own_iface = self.lookup_interface_by_bare_or_fqn(bare).is_some_and(|(_, i)| !i.is_external);
                if own_class || own_iface {
                    AnyKind::Reference
                } else {
                    AnyKind::Opaque
                }
            }
            _ => AnyKind::Opaque,
        }
    }

    /// Whether `bare` names a Jux enum (always a value type, `Copy` or not).
    fn enum_named(&self, bare: &str) -> bool {
        self.symbols.enums.contains_key(bare)
            || self.symbols.enums.keys().any(|k| k.rsplit('.').next() == Some(bare))
    }

    /// Whether `bare` names a value `struct` (ERRATA E20).
    fn struct_named(&self, bare: &str) -> bool {
        self.class_ast_named(bare).is_some_and(|c| c.is_struct)
    }

    /// `crate::JuxAny::of_value(<value>, |v| <text>)` and its siblings.
    /// `owned` says the value is already a fresh owned binding (a closure
    /// parameter), so it needs no copy even though it reads as a place.
    fn emit_any_box(&mut self, expr: &Expr, ty: &Ty, kind: AnyKind, owned: bool) {
        self.w.push_str(match kind {
            AnyKind::Value => "crate::JuxAny::of_value(",
            AnyKind::Reference => "crate::JuxAny::of_ref(",
            AnyKind::Opaque => "crate::JuxAny::of_opaque(",
        });
        // An untyped `5` is an `int`; left alone Rust would store an `i32`,
        // and `v => int` would be false.
        if juxc_tycheck::infer::untyped_int_literal(expr) {
            if matches!(expr, Expr::Literal(_)) {
                self.emit_expr(expr);
                self.w.push_str("_isize");
            } else {
                self.w.push('(');
                self.emit_expr(expr);
                self.w.push_str(" as isize)");
            }
        } else {
            self.emit_expr(expr);
            // The value is shared, not moved out of a place the program keeps
            // using: a class or handle copies the pointer, a value copies itself.
            let copy = match ty {
                Ty::Primitive(_) => true,
                Ty::User { name, .. } => self.enum_named(name.rsplit('.').next().unwrap_or(name)) && self.enum_is_copy(name),
                _ => false,
            };
            if !owned && crate::analysis::expr_is_place_pub(expr) && !copy {
                self.w.push_str(".clone()");
            }
        }
        self.w.push_str(", |v| ");
        self.emit_any_text(ty);
        self.w.push(')');
    }

    /// The body of the text closure: `v` rendered as `${v}` would render it.
    fn emit_any_text(&mut self, ty: &Ty) {
        // The closure parameter shadows any local of the same name; make sure
        // the renderer does not mistake it for a nullable one.
        let was_nullable = self.nullable_locals.remove("v");
        let read = self.synth_read("v", ty.clone());
        let mut scope = std::collections::HashMap::new();
        scope.insert("v".to_string(), ty.clone());
        self.local_types.push(scope);
        let prev = std::mem::replace(&mut self.emitting_format_arg, true);
        self.w.push_str("format!(\"{}\", ");
        self.emit_format_arg(&read);
        self.w.push(')');
        self.emitting_format_arg = prev;
        self.local_types.pop();
        self.expr_types.remove(&SYNTH_SPAN);
        if was_nullable {
            self.nullable_locals.insert("v".to_string());
        }
    }

    /// A read of the local `name`, typed `ty` for every helper that asks.
    fn synth_read(&mut self, name: &str, ty: Ty) -> Expr {
        self.expr_types.insert(SYNTH_SPAN, ty);
        Expr::Path(QualifiedName {
            segments: vec![Ident { text: name.to_string(), span: SYNTH_SPAN }],
            span: SYNTH_SPAN,
        })
    }

    /// `v => T` on an `any`: true when the held value is a `T`.
    pub(crate) fn emit_any_type_test(&mut self, t: &TypeTestExpr) {
        // `v => any` holds for every value (§T.1.4).
        if self.type_ref_is_any(&t.ty) {
            self.w.push_str("({ let _ = &(");
            self.emit_expr(&t.value);
            self.w.push_str("); true })");
            return;
        }
        let forms = self.held_forms(&t.ty);
        let simple = matches!(&*t.value, Expr::Path(_) | Expr::This(_));
        if !simple {
            self.w.push_str("({ let held = &(");
            self.emit_expr(&t.value);
            self.w.push_str("); ");
        } else {
            self.w.push('(');
        }
        for (i, form) in forms.iter().enumerate() {
            if i > 0 {
                self.w.push_str(" || ");
            }
            if simple {
                self.emit_expr(&t.value);
            } else {
                self.w.push_str("held");
            }
            match form {
                HeldForm::Hook { rust, hook } => {
                    self.w.push_str(".get::<");
                    self.w.push_str(rust);
                    self.w.push_str(">().is_some_and(|v| ");
                    self.w.push_str(dyn_trait(rust));
                    self.w.push_str("::");
                    self.w.push_str(hook);
                    self.w.push_str("(&*v).is_some())");
                }
                other => {
                    self.w.push_str(".is::<");
                    self.w.push_str(other.rust());
                    self.w.push_str(">()");
                }
            }
        }
        if forms.is_empty() {
            self.w.push_str("false");
        }
        self.w.push_str(if simple { ")" } else { " })" });
    }

    /// `v => T t` on an `any`: the held value as a `T`, or `None`.
    pub(crate) fn emit_any_type_test_get(&mut self, value: &Expr, target: &TypeRef) {
        if self.type_ref_is_any(target) {
            self.w.push_str("Some(");
            self.emit_expr(value);
            self.w.push_str(".clone())");
            return;
        }
        let forms = self.held_forms(target);
        let mut plain = target.clone();
        plain.nullable = false;
        let own_value = self.rust_type_text(&plain, true);
        let simple = matches!(value, Expr::Path(_) | Expr::This(_));
        if !simple {
            self.w.push_str("{ let held = &(");
            self.emit_expr(value);
            self.w.push_str("); ");
        }
        if forms.is_empty() {
            self.w.push_str("None");
        }
        for (i, form) in forms.iter().enumerate() {
            if i > 0 {
                self.w.push_str(".or_else(|| ");
            }
            if simple {
                self.emit_expr(value);
            } else {
                self.w.push_str("held");
            }
            self.w.push_str(".get::<");
            self.w.push_str(form.rust());
            self.w.push_str(">()");
            match form {
                HeldForm::Exact(_) => {}
                HeldForm::Wrap(_) => {
                    self.w.push_str(".map(|v| std::rc::Rc::new(v) as ");
                    self.w.push_str(&own_value);
                    self.w.push(')');
                }
                HeldForm::Upcast(_) => {
                    self.w.push_str(".map(|v| v as ");
                    self.w.push_str(&own_value);
                    self.w.push(')');
                }
                HeldForm::Coerce { from, .. } => {
                    self.w.push_str(".map(|v| ");
                    let read = self.synth_read("v", from.clone());
                    let mut plain = target.clone();
                    plain.nullable = false;
                    self.emit_expr_coerced_to_iface(&plain, &read);
                    self.expr_types.remove(&SYNTH_SPAN);
                    self.w.push(')');
                }
                HeldForm::Hook { rust, hook } => {
                    self.w.push_str(".and_then(|v| ");
                    self.w.push_str(dyn_trait(rust));
                    self.w.push_str("::");
                    self.w.push_str(hook);
                    self.w.push_str("(&*v))");
                }
            }
            if i > 0 {
                self.w.push(')');
            }
        }
        if !simple {
            self.w.push_str(" }");
        }
    }

    /// The text of [`Self::emit_any_type_test_get`] on the local `name`,
    /// for a caller that assembles its own Rust (a `switch` arm guard).
    pub(crate) fn any_getter_text(&mut self, name: &str, target: &TypeRef) -> String {
        let saved = std::mem::replace(&mut self.w, crate::writer::Writer::new());
        let mut scope = std::collections::HashMap::new();
        scope.insert(name.to_string(), Ty::Any);
        self.local_types.push(scope);
        let read = self.synth_read(name, Ty::Any);
        self.emit_any_type_test_get(&read, target);
        self.expr_types.remove(&SYNTH_SPAN);
        self.local_types.pop();
        std::mem::replace(&mut self.w, saved).into_string()
    }

    /// Every Rust shape a `target` value can sit in inside an `any`,
    /// discovered from the program's classes and interfaces.
    fn held_forms(&mut self, target: &TypeRef) -> Vec<HeldForm> {
        let mut plain = target.clone();
        plain.nullable = false;
        let own_value = self.rust_type_text(&plain, true);
        let mut forms = vec![HeldForm::Exact(own_value.clone())];
        let bare = match plain.name.segments.last() {
            Some(s) if plain.array_shape.is_none() && plain.generic_args.is_empty() => s.text.clone(),
            _ => return forms,
        };
        let target_is_class = self.lookup_class_by_bare_or_fqn(&bare).is_some_and(|c| !c.is_external);
        let target_is_iface = self.lookup_interface_by_bare_or_fqn(&bare).is_some_and(|(_, i)| !i.is_external);
        if !target_is_class && !target_is_iface {
            return forms;
        }
        let mut seen = std::collections::HashSet::new();
        seen.insert(own_value);
        // The program's own non-generic classes, each once, by bare name.
        let classes: Vec<(String, bool)> = {
            let mut v: Vec<(String, bool)> = self
                .symbols
                .classes
                .iter()
                .filter(|(_, c)| !c.is_external && c.generic_params.is_empty())
                .map(|(k, c)| (k.rsplit('.').next().unwrap_or(k).to_string(), c.is_abstract))
                .collect();
            v.sort();
            v.dedup();
            v
        };
        let interfaces: Vec<String> = {
            let mut v: Vec<String> = self
                .symbols
                .interfaces
                .iter()
                .filter(|(_, i)| !i.is_external && i.generic_params.is_empty())
                .map(|(k, _)| k.rsplit('.').next().unwrap_or(k).to_string())
                .collect();
            v.sort();
            v.dedup();
            v
        };
        // Whether the target's own value shape is a trait object.
        let target_dyn = target_is_iface || self.is_poly_base_class(&bare);
        // A subclass held as its own struct: upcast it.
        for (c, is_abstract) in &classes {
            if *is_abstract || c == &bare || !self.class_is_a(c, &bare) {
                continue;
            }
            let rust = self.rust_type_text(&self.synth_type_ref(c), false);
            if seen.insert(rust.clone()) {
                forms.push(if target_dyn {
                    HeldForm::Wrap(rust)
                } else {
                    HeldForm::Coerce { rust, from: Ty::User { name: c.clone(), generic_args: Vec::new() } }
                });
            }
        }
        // The target's own struct, when its value shape is a trait object
        // (`new Animal(...)` went in as an `Animal`, not an `Rc<dyn …>`).
        if target_is_class && target_dyn {
            let rust = self.rust_type_text(&plain, false);
            if seen.insert(rust.clone()) {
                forms.push(HeldForm::Wrap(rust));
            }
        }
        // Trait objects: a polymorphic base or an interface a value was held
        // as. A subtype's trait object upcasts; a supertype's asks its hook,
        // which exists for every class below it.
        let mut dyn_types: Vec<String> =
            classes.iter().filter(|(c, _)| self.is_poly_base_class(c)).map(|(c, _)| c.clone()).collect();
        dyn_types.extend(interfaces);
        for d in dyn_types {
            if d == bare {
                continue;
            }
            let rust = self.rust_type_text(&self.synth_type_ref(&d), true);
            if seen.contains(&rust) {
                continue;
            }
            if target_dyn && self.class_is_a(&d, &bare) {
                seen.insert(rust.clone());
                forms.push(HeldForm::Upcast(rust));
            } else if target_is_class && self.class_is_a(&bare, &d) {
                seen.insert(rust.clone());
                forms.push(HeldForm::Hook { rust, hook: format!("__jux_as_{bare}") });
            }
        }
        forms
    }

    /// A bare, non-generic type reference to `name`.
    fn synth_type_ref(&self, name: &str) -> TypeRef {
        crate::analysis::synth_iface_type_ref(name, SYNTH_SPAN)
    }

    /// `ty`'s Rust spelling, in a value slot (`value`) or as the bare type.
    fn rust_type_text(&mut self, ty: &TypeRef, value: bool) -> String {
        let saved = std::mem::replace(&mut self.w, crate::writer::Writer::new());
        if value {
            self.emit_value_type_as_rust(ty);
        } else {
            self.emit_type_as_rust(ty);
        }
        std::mem::replace(&mut self.w, saved).into_string()
    }
}
