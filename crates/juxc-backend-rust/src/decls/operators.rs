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
        let needs_mut_self = body.map(|b| body_writes_to_this(b)).unwrap_or(false);

        self.w.indent_inc();
        self.w.emit_indent();
        // Visibility intentionally drops to `pub` — the trait-impl
        // wrappers below need to call these from outside the class's
        // own module if we ever split classes into separate modules.
        self.w.push_str("pub fn ");
        self.w.push_str(synthetic_op_method_name(op.kind));
        self.w.push('(');
        if needs_mut_self {
            self.w.push_str("&mut self");
        } else {
            self.w.push_str("&self");
        }
        for param in &op.params {
            self.w.push_str(", ");
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_type_as_rust(&param.ty);
        }
        self.w.push(')');
        match &op.return_type {
            ReturnType::Void => {}
            ReturnType::Type(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
            ReturnType::AsyncType(_) => {
                self.w.push_str(" -> ()");
            }
        }
        self.w.push_str(" {\n");
        self.w.indent_inc();
        if let Some(body) = body {
            self.this_alias = Some("self".to_string());
            let mut muts = HashSet::new();
            collect_mutated_names(body, &mut muts, &self.user_mut_methods);
            self.mutated_in_fn = muts;
            // Operators have declared return types (`bool` for `==`,
            // `String` for `string`, etc.). Tracking it here lets a
            // String-returning operator return a bare string literal
            // and pick up the `.to_string()` coercion automatically.
            let saved = self.current_return_type.take();
            self.current_return_type = Some(op.return_type.clone());
            self.emit_fn_body_at(body, &op.return_type);
            self.current_return_type = saved;
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
                self.emit_partial_eq_wrapper(class_name);
            }
            OperatorKind::ToString if arity == 0 => {
                self.emit_display_wrapper(class_name);
            }
            OperatorKind::Hash if arity == 0 => {
                self.emit_hash_wrapper(class_name);
            }
            OperatorKind::Cmp if arity == 1 => {
                self.emit_partial_ord_wrapper(class_name);
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
            OperatorKind::Minus if arity == 0 => {
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
    fn emit_partial_eq_wrapper(&mut self, class_name: &str) {
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
        self.w.line("self.__op_eq(other.clone())");
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
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
    fn emit_partial_ord_wrapper(&mut self, class_name: &str) {
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
        self.w.line("Some(self.__op_cmp(other.clone()).cmp(&0))");
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
    pub(super) fn emit_partial_eq_from_cmp(&mut self, class_name: &str) {
        self.w.emit_indent();
        self.w.push_str("impl PartialEq for ");
        self.w.push_str(class_name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn eq(&self, other: &Self) -> bool {");
        self.w.indent_inc();
        self.w.line("self.__op_cmp(other.clone()) == 0");
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
        if let Some(p) = rhs_ty {
            self.emit_type_as_rust(&p.ty);
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
        self.w.push_str("(rhs)\n");
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
