//! Operator overload lowering (`JUX-OPERATORS-ADDENDUM.md` §O.2).
//! Each operator becomes an inherent `__op_*` method on the class
//! ([`Self::emit_operator_as_method`]); recognized kinds also get a
//! trait wrapper that bridges from `std::ops::Add` / `PartialEq` /
//! `Display` / `Hash` / etc. to the inherent method
//! ([`Self::emit_operator_trait_impl`]).

use std::collections::HashSet;

use juxc_ast::{OperatorDecl, OperatorKind, ReturnType};

use crate::analysis::{body_writes_to_this, collect_mutated_names};
use crate::decls::synthetic_op_method_name;
use crate::RustEmitter;
use juxc_lex::to_rust_ident;

impl RustEmitter {
    /// Emit one operator-overload body as an inherent method on the
    /// enclosing class, using the synthetic name from
    /// [`synthetic_op_method_name`]. Caller (`emit_class_decl`) has the
    /// writer positioned inside the class's `impl` block at indent 0;
    /// this method drives the same indent dance as [`Self::emit_method`].
    ///
    /// Receiver kind: `&self` for everything except writes-through-this
    /// (mirroring `emit_method` exactly). Operator bodies that mutate
    /// fields are rare but we honor the same rule so `operator+=` (when
    /// it lands) would Just Work.
    ///
    /// Operators in this turn always have a body (no `= delete;` form
    /// in the parser yet), so the `None` branch is unreachable; we
    /// still guard against it so a future parser change can't silently
    /// drop the method.
    pub(crate) fn emit_operator_as_method(&mut self, op: &OperatorDecl) {
        // `= delete;` operators have no implementation — they exist
        // only to suppress an auto-derive. Skip both the inherent
        // method and (in `emit_operator_trait_impl`) the trait wrapper.
        if op.is_deleted {
            return;
        }
        let body = op.body.as_ref();
        // A wrapper (Rc<RefCell>) class mutates through interior `self.0.borrow_mut()`,
        // so EVERY method — operators included — takes `&self`, matching the rest of
        // the wrapper's inherent methods. Only an inline class needs `&mut self` when
        // its operator body writes to `this`; emitting `&mut self` on a wrapper forces
        // a `let mut` at the call site that the binding emitter never produces (E0596).
        let needs_mut_self = !self.emitting_wrapper_class
            && body
                .map(|b| {
                    body_writes_to_this(b)
                        || crate::analysis::body_calls_mut_method_on_this(b, &self.user_mut_methods)
                })
                .unwrap_or(false);

        self.w.indent_inc();
        self.w.emit_indent();
        // Visibility intentionally drops to `pub` — the trait-impl
        // wrappers below need to call these from outside the class's
        // own module if we ever split classes into separate modules.
        // `async T` operator → `async fn`. Rare in practice (operators
        // are typically pure), but the parser accepts the keyword on
        // any return-type position, so we honor it here too.
        if matches!(op.return_type, ReturnType::AsyncType(_)) {
            self.w.push_str("pub async fn ");
        } else {
            self.w.push_str("pub fn ");
        }
        self.w.push_str(synthetic_op_method_name(op.kind));
        self.w.push('(');
        if needs_mut_self {
            self.w.push_str("&mut self");
        } else {
            self.w.push_str("&self");
        }
        for param in &op.params {
            self.w.push_str(", ");
            self.w.push_str(&to_rust_ident(&param.name.text));
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&param.ty);
        }
        self.w.push(')');
        match &op.return_type {
            ReturnType::Void => {}
            ReturnType::Type(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
            ReturnType::AsyncType(t) => {
                // `async T` → `async fn (...) -> T`. The `async`
                // keyword was already emitted ahead of `fn` above.
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
        }
        self.w.push_str(" {\n");
        self.w.indent_inc();
        if let Some(body) = body {
            self.this_alias = Some("self".to_string());
            let mut muts = HashSet::new();
            collect_mutated_names(body, &mut muts, &self.user_mut_methods);
            self.collect_mut_slot_locals(body, &mut muts);
            self.mutated_in_fn = muts;
            // Operators have declared return types (`bool` for `==`,
            // `String` for `string`, etc.). Tracking it here lets a
            // String-returning operator return a bare string literal
            // and pick up the `.to_string()` coercion automatically.
            self.current_fn_params = op.params.iter().map(|p| p.name.text.clone()).collect();
            let saved = self.current_return_type.take();
            self.current_return_type = Some(op.return_type.clone());
            self.emit_fn_body_at(body, &op.return_type);
            self.current_return_type = saved;
            self.current_fn_params.clear();
            self.this_alias = None;
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Emit a Rust trait impl that bridges from the standard library's
    /// operator trait to the inherent `__op_*` method produced by
    /// [`Self::emit_operator_as_method`].
    ///
    /// Coverage table:
    ///
    /// | Jux operator        | Arity | Rust trait          | Notes                                       |
    /// |---------------------|-------|---------------------|---------------------------------------------|
    /// | `==`                | 1     | `PartialEq`         | Wrapper: `self.__op_eq(other.clone())`      |
    /// | `string`            | 0     | `std::fmt::Display` | Wrapper: `f.write_str(&self.__op_string())` |
    /// | `hash`              | 0     | `std::hash::Hash`   | Wrapper writes `__op_hash()` into Hasher    |
    /// | `+`                 | 1     | `std::ops::Add`     | Output = user return type                   |
    /// | `+`                 | 0     | — (unary plus has no Rust trait)            |                                             |
    /// | `-`                 | 1     | `std::ops::Sub`     |                                             |
    /// | `-`                 | 0     | `std::ops::Neg`     |                                             |
    /// | `*` `/` `%`         | 1     | `Mul` / `Div` / `Rem`                       |                                             |
    /// | `&` `\|` `^`        | 1     | `BitAnd` / `BitOr` / `BitXor`               |                                             |
    /// | `~`                 | 0     | `std::ops::Not`     |                                             |
    /// | `<<` `>>`           | 1     | `Shl` / `Shr`       |                                             |
    ///
    /// Still NOT mapped (inherent method emitted, no trait wrapper):
    /// `<=>` (PartialOrd's `partial_cmp` returns `Option<Ordering>`),
    /// individual `<`/`<=`/`>`/`>=` (need all-or-nothing PartialOrd
    /// emission), `[]` / `[]=` (Index returns `&Output`), `()`
    /// (`Fn*` traits are nightly), and `..` / `..=` (no Rust trait).
    ///
    /// **`==` vs `===`.** Per spec §O.2.5 `===` is **never overridable**
    /// — it's always reference identity. The emitted `impl PartialEq`
    /// rebinds Rust's `==` (the EqEq token) to the user's body; Jux
    /// `===` (StrictEq) is not yet a parsed expression in any case, and
    /// when it lands it'll lower to `Arc::ptr_eq` / `std::ptr::eq`
    /// directly, bypassing PartialEq entirely.
    pub(crate) fn emit_operator_trait_impl(&mut self, class_name: &str, op: &OperatorDecl) {
        // Deleted operators contribute no trait impl — the `is_deleted`
        // declaration's purpose is purely to suppress an auto-derive
        // (records) or signal "this operator is intentionally missing."
        if op.is_deleted {
            return;
        }
        let arity = op.params.len();
        let synth = synthetic_op_method_name(op.kind);
        match op.kind {
            OperatorKind::Eq if arity == 1 => {
                let arg = self.operator_other_arg(op);
                self.emit_partial_eq_wrapper(class_name, arg);
            }
            OperatorKind::ToString if arity == 0 => {
                self.emit_display_wrapper(class_name);
            }
            OperatorKind::Hash if arity == 0 => {
                self.emit_hash_wrapper(class_name);
            }
            OperatorKind::Cmp if arity == 1 => {
                let arg = self.operator_other_arg(op);
                self.emit_partial_ord_wrapper(class_name, arg);
            }
            // Binary arithmetic / bitwise / shift family — single
            // shape. Output type comes from the user's return type.
            OperatorKind::Plus if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::Add", "add", op, synth);
            }
            OperatorKind::Minus if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::Sub", "sub", op, synth);
            }
            OperatorKind::Mul if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::Mul", "mul", op, synth);
            }
            OperatorKind::Div if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::Div", "div", op, synth);
            }
            OperatorKind::Rem if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::Rem", "rem", op, synth);
            }
            OperatorKind::BitAnd if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::BitAnd", "bitand", op, synth);
            }
            OperatorKind::BitOr if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::BitOr", "bitor", op, synth);
            }
            OperatorKind::BitXor if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::BitXor", "bitxor", op, synth);
            }
            OperatorKind::Shl if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::Shl", "shl", op, synth);
            }
            OperatorKind::Shr if arity == 1 => {
                self.emit_binary_op_wrapper(class_name, "std::ops::Shr", "shr", op, synth);
            }
            // Unary family — receiver-only, Output from return type.
            // (The parser re-kinds a zero-param `operator-` to Neg.)
            OperatorKind::Neg => {
                self.emit_unary_op_wrapper(class_name, "std::ops::Neg", "neg", op, synth);
            }
            OperatorKind::BitNot if arity == 0 => {
                self.emit_unary_op_wrapper(class_name, "std::ops::Not", "not", op, synth);
            }
            // Anything else (wrong arity, or operators without a Rust
            // counterpart yet): inherent method only.
            _ => {}
        }
    }

    /// `impl PartialEq for Class { fn eq(...) { self.__op_eq(other.clone()) } }`.
    fn emit_partial_eq_wrapper(&mut self, class_name: &str, arg: &str) {
        self.w.emit_indent();
        self.w.push_str("impl PartialEq for ");
        self.w.push_str(class_name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn eq(&self, other: &Self) -> bool {");
        self.w.indent_inc();
        // `other.clone()` produces the by-value Self the user wrote
        // (`operator==(Path other)` — `other: Path`). Classes derive
        // `Clone`, so this is cheap (Arc-clone-shaped under current
        // class representation).
        self.w.line(&format!("self.__op_eq({arg})"));
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// Identity-format Display (§O.4.1): a class with NO
    /// `operator string` still prints — as `ClassName@<addr>`. The
    /// address is the shared cell for wrapper classes (stable
    /// identity across aliases) and the value's own address for
    /// inline classes.
    pub(crate) fn emit_identity_display(
        &mut self,
        class_name: &str,
        wrapper: bool,
        generic_params: &[juxc_ast::TypeParam],
    ) {
        self.w.emit_indent();
        self.w.push_str("impl");
        // A generic class needs its parameters on the impl, with the same
        // baseline bounds its struct declares. The identity form prints an
        // address, so it adds no requirement of its own -- no `Display` on the
        // parameters, which is the whole point: this impl is what LETS a class
        // satisfy someone else's `Display` bound.
        if !generic_params.is_empty() {
            let none: std::collections::HashSet<String> = std::collections::HashSet::new();
            self.emit_generic_params_with_clone_bound_plus_display(generic_params, &none, &none);
        }
        self.w.push_str(" std::fmt::Display for ");
        self.w.push_str(class_name);
        self.emit_generic_params_as_args(generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w
            .line("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {");
        self.w.indent_inc();
        self.w.emit_indent();
        if wrapper {
            self.w.push_str("write!(f, \"");
            self.w.push_str(class_name);
            // Both handles expose the address of the shared cell; the atomic
            // one does it as an inherent method rather than `Rc`'s associated fn.
            if self.sync_classes.contains(class_name) {
                self.w.push_str("@{:p}\", self.0.as_ptr())
");
            } else {
                self.w.push_str("@{:p}\", std::rc::Rc::as_ptr(&self.0))
");
            }
        } else {
            self.w.push_str("write!(f, \"");
            self.w.push_str(class_name);
            self.w.push_str("@{:p}\", self as *const Self)\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// Whether `class_decl` or an ancestor declares `operator==`, `operator<=>`
    /// or `operator hash`, any of which gives the class structural equality.
    pub(crate) fn class_chain_declares_equality(&self, class_decl: &juxc_ast::ClassDecl) -> bool {
        let mut current = Some(class_decl.clone());
        let mut depth = 0;
        while let Some(class) = current {
            if class.operators.iter().any(|o| {
                matches!(o.kind, OperatorKind::Eq | OperatorKind::Cmp | OperatorKind::Hash)
            }) {
                return true;
            }
            depth += 1;
            if depth > 64 {
                break;
            }
            current = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last())
                .and_then(|seg| self.class_ast_named(&seg.text));
        }
        false
    }

    /// `impl PartialEq`, `Eq` and `Hash` by identity for a wrapper class: two
    /// handles are equal when they share one cell, and the cell's address is
    /// the hash (§O.4.1).
    pub(crate) fn emit_identity_eq_hash(&mut self, class_name: &str, generic_params: &[juxc_ast::TypeParam]) {
        let address = if self.sync_classes.contains(class_name) {
            "self.0.as_ptr()"
        } else if self.is_box_class(class_name) {
            return;
        } else {
            "std::rc::Rc::as_ptr(&self.0)"
        };
        let other_address = address.replace("self.0", "other.0");
        let none: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (trait_path, body) in [
            ("PartialEq", Some(format!("fn eq(&self, other: &Self) -> bool {{ std::ptr::eq({address}, {other_address}) }}"))),
            ("Eq", None),
            ("std::hash::Hash", Some(format!("fn hash<H: std::hash::Hasher>(&self, state: &mut H) {{ std::ptr::hash({address}, state) }}"))),
        ] {
            self.w.emit_indent();
            self.w.push_str("impl");
            if !generic_params.is_empty() {
                self.emit_generic_params_with_clone_bound_plus_display(generic_params, &none, &none);
            }
            self.w.push(' ');
            self.w.push_str(trait_path);
            self.w.push_str(" for ");
            self.w.push_str(class_name);
            self.emit_generic_params_as_args(generic_params);
            match body {
                Some(body) => {
                    self.w.push_str(" {\n");
                    self.w.indent_inc();
                    self.w.line(&body);
                    self.w.indent_dec();
                    self.w.line("}");
                }
                None => self.w.push_str(" {}\n"),
            }
        }
        self.w.newline();
    }

    /// `impl crate::JuxIdentity for Name { fn __jux_identity(&self) -> *const () { … } }`.
    ///
    /// `address` is the expression naming the object's storage: the shared
    /// cell for a class handle, `self` for a value. Every class, record and
    /// enum gets one, whatever operators it declares, because the root `Kind`
    /// trait and every interface trait require it (see the prelude's
    /// `JuxIdentity`).
    pub(crate) fn emit_jux_identity_impl(
        &mut self,
        type_name: &str,
        generic_params: &[juxc_ast::TypeParam],
        address: &str,
    ) {
        self.w.emit_indent();
        self.w.push_str("impl");
        if !generic_params.is_empty() {
            let none: std::collections::HashSet<String> = std::collections::HashSet::new();
            self.emit_generic_params_with_clone_bound_plus_display(generic_params, &none, &none);
        }
        self.w.push_str(" crate::JuxIdentity for ");
        self.w.push_str(&to_rust_ident(type_name));
        self.emit_generic_params_as_args(generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line(&format!("fn __jux_identity(&self) -> *const () {{ {address} as *const () }}"));
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// How a trait wrapper hands `other: &Self` to a one-operand operator.
    ///
    /// The operator's parameter is written as the class it compares with. When
    /// that class is a polymorphic base the parameter is a `Rc<dyn …Kind>`, so
    /// the concrete `Self` converts with the `From` impl every class in the
    /// hierarchy has; otherwise the clone already has the parameter's type.
    pub(crate) fn operator_other_arg(&self, op: &OperatorDecl) -> &'static str {
        let dyn_param = op.params.first().is_some_and(|p| {
            !p.ty.nullable
                && p.ty.array_shape.is_none()
                && p.ty.name.segments.last().is_some_and(|s| self.is_poly_base_class(&s.text))
        });
        if dyn_param {
            "other.clone().into()"
        } else {
            "other.clone()"
        }
    }

    /// Every operator `class_decl` has: its own, then each ancestor's that no
    /// closer class redeclares (§O.2.9). An inherited operator's parameter and
    /// return types are read through the `extends` arguments, as an inherited
    /// method's are, so `Store<T>`'s `operator+(Store<T>)` reads as
    /// `operator+(Store<String>)` in `Names extends Store<String>`.
    pub(crate) fn class_effective_operators(&self, class_decl: &juxc_ast::ClassDecl) -> Vec<OperatorDecl> {
        use super::classes::substitute_type_ref;
        let mut out: Vec<OperatorDecl> = class_decl.operators.clone();
        let mut subst: std::collections::HashMap<String, juxc_ast::TypeRef> = std::collections::HashMap::new();
        let mut cursor = class_decl.extends.clone();
        for _ in 0..64 {
            let Some(parent_ref) = cursor else { break };
            let Some(parent) = parent_ref.name.segments.last().and_then(|s| self.class_ast_named(&s.text)) else {
                break;
            };
            let mut next = std::collections::HashMap::new();
            for (param, arg) in parent.generic_params.iter().zip(parent_ref.generic_args.iter()) {
                if let juxc_ast::GenericArg::Type(t) = arg {
                    next.insert(param.name.text.clone(), substitute_type_ref(t, &subst));
                }
            }
            subst = next;
            for op in &parent.operators {
                if out.iter().any(|o| o.kind == op.kind) {
                    continue;
                }
                let mut inherited = op.clone();
                if !subst.is_empty() {
                    for p in &mut inherited.params {
                        p.ty = substitute_type_ref(&p.ty, &subst);
                    }
                    inherited.return_type = match &inherited.return_type {
                        ReturnType::Type(t) => ReturnType::Type(substitute_type_ref(t, &subst)),
                        ReturnType::AsyncType(t) => ReturnType::AsyncType(substitute_type_ref(t, &subst)),
                        ReturnType::Void => ReturnType::Void,
                    };
                }
                out.push(inherited);
            }
            cursor = parent.extends.clone();
        }
        out
    }

    /// The topmost class in `class_bare`'s chain (itself included) that
    /// declares operator `kind`. That class's `Kind` trait owns the operator's
    /// slot; every class below it fills or overrides the slot.
    pub(crate) fn operator_introducer(&self, class_bare: &str, kind: OperatorKind) -> Option<juxc_ast::ClassDecl> {
        let mut found = None;
        let mut cursor = self.class_ast_named(class_bare);
        for _ in 0..64 {
            let Some(class) = cursor else { break };
            if class.operators.iter().any(|o| o.kind == kind && !o.is_deleted) {
                found = Some(class.clone());
            }
            cursor = class
                .extends
                .as_ref()
                .and_then(|e| e.name.segments.last())
                .and_then(|s| self.class_ast_named(&s.text));
        }
        found
    }

    /// The class whose `Kind` trait carries `__jux_share` for `class_bare`'s
    /// hierarchy: the polymorphic base that introduces `operator==` taking its
    /// own type.
    ///
    /// A base-typed `==` runs through `dyn …Kind`, where the other operand is
    /// only a `&dyn …Kind`, but the operator takes the handle `Rc<dyn …Kind>`.
    /// `__jux_share` rebuilds that handle from the borrowed value.
    pub(crate) fn equality_share_owner(&self, class_bare: &str) -> Option<String> {
        let owner = self.operator_introducer(class_bare, OperatorKind::Eq)?;
        let name = owner.name.text.clone();
        if !owner.generic_params.is_empty() || !self.is_poly_base_class(&name) {
            return None;
        }
        let op = owner.operators.iter().find(|o| o.kind == OperatorKind::Eq)?;
        let takes_own_type = op.params.len() == 1
            && op.params[0].ty.generic_args.is_empty()
            && op.params[0].ty.array_shape.is_none()
            && !op.params[0].ty.nullable
            && op.params[0].ty.name.segments.last().is_some_and(|s| {
                self.resolve_bare_class_fqn(&s.text).is_some()
                    && self.resolve_bare_class_fqn(&s.text) == self.resolve_bare_class_fqn(&name)
            });
        takes_own_type.then_some(name)
    }

    /// `PartialEq` / `Eq` / `Hash` for `dyn <class>Kind` through the
    /// hierarchy's own operators (§O.2.9), so a base-typed value compares with
    /// the most-derived `operator==` and hashes with its `operator hash`.
    pub(crate) fn emit_dyn_operator_eq_hash(&mut self, class_bare: &str) {
        let target = format!("dyn {}Kind", to_rust_ident(class_bare));
        let eq_owner = self.equality_share_owner(class_bare);
        let hash_owner = self
            .operator_introducer(class_bare, OperatorKind::Hash)
            .filter(|c| c.generic_params.is_empty())
            .map(|c| c.name.text);
        if let Some(owner) = &eq_owner {
            let path = format!("{}{}Kind", self.cross_package_prefix(owner), to_rust_ident(owner));
            self.w.line(&format!("impl PartialEq for {target} {{"));
            self.w.indent_inc();
            self.w.line(&format!(
                "fn eq(&self, other: &Self) -> bool {{ {path}::__op_eq(self, {path}::__jux_share(other)) }}"
            ));
            self.w.indent_dec();
            self.w.line("}");
            if hash_owner.is_some() {
                self.w.line(&format!("impl Eq for {target} {{}}"));
            }
        }
        if let Some(owner) = &hash_owner {
            let path = format!("{}{}Kind", self.cross_package_prefix(owner), to_rust_ident(owner));
            self.w.line(&format!("impl std::hash::Hash for {target} {{"));
            self.w.indent_inc();
            self.w.line(&format!(
                "fn hash<H: std::hash::Hasher>(&self, state: &mut H) {{ state.write_isize({path}::__op_hash(self)) }}"
            ));
            self.w.indent_dec();
            self.w.line("}");
        }
        if eq_owner.is_some() || hash_owner.is_some() {
            self.w.newline();
        }
    }

    /// `PartialEq`, `Eq` and `Hash` for `dyn <trait_name>`, by identity.
    ///
    /// A base-typed or interface-typed value is a `Rc<dyn …>`, and `Rc` only
    /// compares and hashes when its pointee does. Without these a
    /// `HashSet<Animal>` or `a == b` on two `Animal`s did not compile, even
    /// though the class compares by identity (§O.2.6, §O.4.1).
    pub(crate) fn emit_dyn_identity_eq_hash(&mut self, trait_name: &str) {
        let target = format!("dyn {}", to_rust_ident(trait_name));
        let id = |who: &str| format!("crate::JuxIdentity::__jux_identity({who})");
        self.w.line(&format!("impl PartialEq for {target} {{"));
        self.w.indent_inc();
        self.w.line(&format!(
            "fn eq(&self, other: &Self) -> bool {{ std::ptr::eq({}, {}) }}",
            id("self"),
            id("other")
        ));
        self.w.indent_dec();
        self.w.line("}");
        self.w.line(&format!("impl Eq for {target} {{}}"));
        self.w.line(&format!("impl std::hash::Hash for {target} {{"));
        self.w.indent_inc();
        self.w.line(&format!(
            "fn hash<H: std::hash::Hasher>(&self, state: &mut H) {{ std::ptr::hash({}, state) }}",
            id("self")
        ));
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// The `__jux_identity` address of a class's instance, by handle shape.
    pub(crate) fn class_identity_address(&self, class_name: &str, wrapper: bool) -> &'static str {
        if !wrapper {
            "self as *const Self"
        } else if self.sync_classes.contains(class_name) {
            "self.0.as_ptr()"
        } else if self.is_box_class(class_name) {
            "&*self.0 as *const _"
        } else {
            "std::rc::Rc::as_ptr(&self.0)"
        }
    }

    /// `impl Display for Class { fn fmt(...) { f.write_str(&self.__op_string()) } }`.
    fn emit_display_wrapper(&mut self, class_name: &str) {
        self.w.emit_indent();
        self.w.push_str("impl std::fmt::Display for ");
        self.w.push_str(class_name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {");
        self.w.indent_inc();
        self.w.line("f.write_str(&self.__op_string())");
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// `impl Hash for Class { fn hash(...) { state.write_isize(self.__op_hash()) } }`.
    ///
    /// Jux's `operator hash()` returns `int` (Rust `isize`); we forward
    /// that value into the Hasher via `Hasher::write_isize` so the
    /// user's body stays in a "return a value" shape and the bridging
    /// happens in the wrapper.
    fn emit_hash_wrapper(&mut self, class_name: &str) {
        self.w.emit_indent();
        self.w.push_str("impl std::hash::Hash for ");
        self.w.push_str(class_name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line(
            "fn hash<H: std::hash::Hasher>(&self, state: &mut H) {",
        );
        self.w.indent_inc();
        self.w.line("std::hash::Hasher::write_isize(state, self.__op_hash());");
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// `impl PartialOrd for Class { fn partial_cmp(...) -> Option<Ordering> { … } }`.
    ///
    /// Bridges from Jux's `operator<=>` (which returns `int`) to Rust's
    /// `Option<Ordering>`. The conversion is exactly the standard
    /// three-way-compare-to-Ordering mapping — isize's own `Ord` impl
    /// turns a comparison result into `Less`/`Equal`/`Greater` via
    /// `.cmp(&0)`. Wrapped in `Some(...)` since our cmp is total.
    ///
    /// **Auto-derived from `<=>`**: this PartialOrd impl unlocks `<`,
    /// `<=`, `>`, `>=` for free per spec §O.2.1 — they go through
    /// Rust's default `PartialOrd::lt/le/gt/ge` which all dispatch
    /// through `partial_cmp`.
    fn emit_partial_ord_wrapper(&mut self, class_name: &str, arg: &str) {
        self.w.emit_indent();
        self.w.push_str("impl PartialOrd for ");
        self.w.push_str(class_name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line(
            "fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {",
        );
        self.w.indent_inc();
        // `self.__op_cmp(other.clone())` returns isize; `.cmp(&0)`
        // converts it to Ordering via isize's own Ord impl
        // (negative → Less, zero → Equal, positive → Greater).
        self.w.line(&format!("Some(self.__op_cmp({arg}).cmp(&0))"));
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// `impl PartialEq for Class` bridging through `__op_cmp` — emitted
    /// when the user declared `operator<=>` but not `operator==`. Rust's
    /// `PartialOrd: PartialEq` constraint means we can't emit the
    /// PartialOrd wrapper without a matching PartialEq, and the spec's
    /// `<=>` auto-derives the four ordering ops but NOT `==`. Bridging
    /// "a == b iff cmp(a, b) == 0" is the consistent fill-in.
    ///
    /// The class-level emitter (`emit_class_decl`) calls this when it
    /// sees `Cmp` without `Eq` after the per-operator trait loop runs.
    pub(super) fn emit_partial_eq_from_cmp(&mut self, class_name: &str, arg: &str) {
        self.w.emit_indent();
        self.w.push_str("impl PartialEq for ");
        self.w.push_str(class_name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn eq(&self, other: &Self) -> bool {");
        self.w.indent_inc();
        self.w.line(&format!("self.__op_cmp({arg}) == 0"));
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// `impl Eq for Class {}` — the marker trait we emit when the user
    /// declares both `operator==` AND `operator hash` on the same
    /// class (per the spec §O.2.7 pairing rule). With both present,
    /// the user has signalled full-equality + hashing intent so the
    /// class can serve as a `HashMap` / `HashSet` key.
    pub(super) fn emit_eq_marker(&mut self, class_name: &str) {
        self.w.emit_indent();
        self.w.push_str("impl Eq for ");
        self.w.push_str(class_name);
        self.w.push_str(" {}\n");
        self.w.newline();
    }

    /// True when an operator's operand type is the receiver's own type, so the
    /// Rust trait's default `Rhs = Self` already says it.
    ///
    /// Deliberately strict: only a bare, single-segment, non-nullable,
    /// non-array, non-function name equal to the class counts. Anything else —
    /// including a generic spelling of the same class — gets the type argument
    /// written out, which is always correct even where it was not required.
    fn op_param_is_self(class_name: &str, ty: &juxc_ast::TypeRef) -> bool {
        ty.name.segments.len() == 1
            && ty.name.segments[0].text == class_name
            && ty.generic_args.is_empty()
            && ty.array_shape.is_none()
            && ty.fn_shape.is_none()
            && !ty.nullable
    }

    /// Binary operator wrapper: `impl <Trait> for Class { type Output = R;
    /// fn <method>(self, rhs: U) -> Self::Output { self.__op_*(rhs) } }`.
    ///
    /// Rust's binary op traits take `self` by value (consuming). For
    /// classes that's an Arc-shaped clone semantically. The wrapper
    /// forwards to the inherent `&self` method by auto-borrowing.
    fn emit_binary_op_wrapper(
        &mut self,
        class_name: &str,
        trait_path: &str,
        method: &str,
        op: &OperatorDecl,
        synth: &str,
    ) {
        let rhs_ty = op.params.first();
        self.w.emit_indent();
        self.w.push_str("impl ");
        self.w.push_str(trait_path);
        // **The operand type is the trait's type argument.** Rust's `std::ops`
        // traits default `Rhs = Self`, so an operator over the receiver's own
        // type needs nothing written — but a SCALAR operand (`Vec2 operator
        // *(double k)`, the canonical case in any vector or matrix code) is
        // `Mul<f64>`. Emitting a bare `impl Mul for Vec2` while writing
        // `fn mul(self, rhs: f64)` is E0053: the signature does not match the
        // trait it claims to implement, and the whole crate fails to build.
        //
        // Omitted only where the operand really is the receiver's bare type, so
        // the common same-type operator keeps reading as idiomatic Rust.
        let rhs_is_self = rhs_ty.is_some_and(|p| Self::op_param_is_self(class_name, &p.ty));
        if !rhs_is_self {
            if let Some(p) = rhs_ty {
                self.w.push('<');
                let ty = p.ty.clone();
                self.emit_value_type_as_rust(&ty);
                self.w.push('>');
            }
        }
        self.w.push_str(" for ");
        self.w.push_str(class_name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        // `type Output = …;` from the user's declared return type.
        self.w.emit_indent();
        self.w.push_str("type Output = ");
        self.emit_op_output_type(&op.return_type);
        self.w.push_str(";\n");
        // `fn <method>(self, rhs: <param-ty>) -> Self::Output {`.
        self.w.emit_indent();
        self.w.push_str("fn ");
        self.w.push_str(method);
        self.w.push_str("(self, rhs: ");
        // A polymorphic base's own type lowers to `Rc<dyn …Kind>` in a
        // parameter, but `impl Add for Money` fixes `rhs` to `Money`. The
        // trait keeps `Self` and the call converts it.
        let rhs_into = rhs_is_self && self.is_poly_base_class(class_name);
        if rhs_into {
            self.w.push_str("Self");
        } else if let Some(p) = rhs_ty {
            self.emit_value_type_as_rust(&p.ty);
        } else {
            // Defensive — caller (`emit_operator_trait_impl`) only
            // dispatches binary wrappers when `arity == 1`, so a
            // missing param is a compiler bug, not user input.
            self.w.push_str("()");
        }
        self.w.push_str(") -> Self::Output {\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("self.");
        self.w.push_str(synth);
        self.w.push_str(if rhs_into { "(rhs.into())\n" } else { "(rhs)\n" });
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// Unary operator wrapper: `impl <Trait> for Class { type Output = R;
    /// fn <method>(self) -> Self::Output { self.__op_*() } }`.
    fn emit_unary_op_wrapper(
        &mut self,
        class_name: &str,
        trait_path: &str,
        method: &str,
        op: &OperatorDecl,
        synth: &str,
    ) {
        self.w.emit_indent();
        self.w.push_str("impl ");
        self.w.push_str(trait_path);
        self.w.push_str(" for ");
        self.w.push_str(class_name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("type Output = ");
        self.emit_op_output_type(&op.return_type);
        self.w.push_str(";\n");
        self.w.emit_indent();
        self.w.push_str("fn ");
        self.w.push_str(method);
        self.w.push_str("(self) -> Self::Output {\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("self.");
        self.w.push_str(synth);
        self.w.push_str("()\n");
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// Emit the `Output` type for a trait wrapper from the operator's
    /// declared return type. `void` collapses to `()` (no arithmetic
    /// operator really returns void, but be defensive).
    fn emit_op_output_type(&mut self, rt: &ReturnType) {
        match rt {
            ReturnType::Void => {
                self.w.push_str("()");
            }
            ReturnType::Type(t) => self.emit_return_type_as_rust(t),
            ReturnType::AsyncType(_) => {
                self.w.push_str("()");
            }
        }
    }
}
