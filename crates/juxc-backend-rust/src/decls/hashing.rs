//! `Eq` and `Hash` for the value types: records, structs and enums.
//!
//! JUX-OPERATORS-ADDENDUM §O.3.1: a record (and a struct, ERRATA E20, and an
//! enum) derives `operator==` and `operator hash` from its components, and the
//! §O.3.1 example is `record Point(double x, double y)`. Rust derives neither
//! `Eq` nor `Hash` for a struct holding an `f64`, so a float component is
//! hashed by hand, by its bits (`crate::jux_f64_bits`, where `0.0` and `-0.0`
//! agree because they are `==`). Everything else is derived.
//!
//! WHETHER a type hashes at all is `juxc_tycheck`'s answer
//! ([`juxc_tycheck::symbol_table::SymbolTable::declaration_is_hashable`]),
//! the same one behind the checker's `E0933`, so a key the checker accepts
//! always compiles.

use juxc_ast::{OperatorDecl, OperatorKind, TypeParam, TypeRef};
use juxc_lex::to_rust_ident;

use crate::RustEmitter;

/// How a value type gets its `Eq` and `Hash` impls.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HashPlan {
    /// Put `Eq` in the `#[derive]` list.
    pub derive_eq: bool,
    /// Put `Hash` in the `#[derive]` list.
    pub derive_hash: bool,
    /// Write `impl Hash` by hand: a float component rules the derive out.
    pub manual_hash: bool,
    /// Write `impl Eq for T {}`: the type has `PartialEq` (derived with a
    /// float component, or the user's own `operator==`) and a hash, and a
    /// key needs the `Eq` promise too.
    pub eq_marker: bool,
}

/// The width of a float component, and whether it is nullable (`double?`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct FloatComponent {
    /// `true` for `double` / `f64`, `false` for `float` / `f32`.
    pub wide: bool,
    /// The component is `T?`, lowered to `Option<f64>`.
    pub nullable: bool,
}

/// `Some` when `ty` is a float component (possibly nullable), which a derive
/// cannot hash.
pub(crate) fn float_component(ty: &TypeRef) -> Option<FloatComponent> {
    if ty.array_shape.is_some() || !ty.generic_args.is_empty() || ty.fn_shape.is_some() || ty.name.segments.len() != 1 {
        return None;
    }
    let wide = match ty.name.segments[0].text.as_str() {
        "double" | "f64" => true,
        "float" | "f32" => false,
        _ => return None,
    };
    Some(FloatComponent { wide, nullable: ty.nullable })
}

impl RustEmitter {
    /// Decide `Eq` / `Hash` for a value type named `name` (bare or FQN) with
    /// these component types and declared operators.
    ///
    /// `legacy_eq` is the older, purely syntactic answer ("every component is
    /// a non-float primitive or `String`"), still used for a type that cannot
    /// hash: its `Eq` never depended on hashing.
    pub(crate) fn value_hash_plan(
        &self,
        name: &str,
        components: &[&TypeRef],
        operators: &[OperatorDecl],
        legacy_eq: bool,
    ) -> HashPlan {
        let declared = |kind: OperatorKind| operators.iter().any(|o| o.kind == kind && !o.is_deleted);
        let deleted = |kind: OperatorKind| operators.iter().any(|o| o.kind == kind && o.is_deleted);
        let user_eq = declared(OperatorKind::Eq);
        let user_hash = declared(OperatorKind::Hash);
        let eq_deleted = deleted(OperatorKind::Eq);
        let hash_deleted = deleted(OperatorKind::Hash);
        let floats = components.iter().any(|t| float_component(t).is_some());
        let mut plan = HashPlan::default();
        if eq_deleted || hash_deleted || !self.symbols.declaration_is_hashable(name) {
            // No hash: `Eq` only where it always was.
            plan.derive_eq = !user_eq && !eq_deleted && legacy_eq;
            return plan;
        }
        if user_hash {
            // The user's `operator hash` is bridged with the other operators;
            // what the type still needs is `Eq`.
            if !user_eq && !floats {
                plan.derive_eq = true;
            } else {
                plan.eq_marker = true;
            }
            return plan;
        }
        if floats {
            plan.manual_hash = true;
            plan.eq_marker = true;
        } else {
            plan.derive_hash = true;
            if user_eq {
                plan.eq_marker = true;
            } else {
                plan.derive_eq = true;
            }
        }
        plan
    }

    /// `impl<T> Trait for Name<T> where T: Trait`, up to the ` {`. The
    /// `where` clause holds exactly the bound the derive would have added.
    ///
    /// A record or enum declares its parameters bare. A struct declares them
    /// with the class bounds, which every impl must repeat; the struct path
    /// sets [`RustEmitter::op_impl_class`] to say so.
    pub(crate) fn emit_value_impl_head(&mut self, trait_path: &str, name: &str, params: &[TypeParam]) {
        self.w.emit_indent();
        self.w.push_str("impl");
        match self.op_impl_class.clone() {
            Some(decl) => self.emit_class_impl_generic_params(&decl),
            None => self.emit_generic_params(params),
        }
        self.w.push(' ');
        self.w.push_str(trait_path);
        self.w.push_str(" for ");
        self.w.push_str(&to_rust_ident(name));
        self.emit_generic_params_as_args(params);
        let bounded: Vec<&TypeParam> = params.iter().filter(|p| !p.is_const()).collect();
        if !bounded.is_empty() {
            self.w.push_str(" where ");
            for (i, p) in bounded.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&to_rust_ident(&p.name.text));
                self.w.push_str(": ");
                self.w.push_str(trait_path);
            }
        }
    }

    /// `impl Eq for Name {}`: the promise a hash key needs on top of
    /// `PartialEq`.
    pub(crate) fn emit_value_eq_marker(&mut self, name: &str, params: &[TypeParam]) {
        self.emit_value_impl_head("Eq", name, params);
        self.w.push_str(" {}\n");
        self.w.newline();
    }

    /// `impl Hash` for a record or struct, one line per field, hashing a float
    /// by its bits and everything else through its own `Hash`.
    pub(crate) fn emit_value_hash_for_fields(&mut self, name: &str, params: &[TypeParam], fields: &[(&str, &TypeRef)]) {
        self.emit_value_impl_head("std::hash::Hash", name, params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn hash<H: std::hash::Hasher>(&self, state: &mut H) {");
        self.w.indent_inc();
        self.w.line("use std::hash::Hash;");
        for (field, ty) in fields {
            let access = format!("self.{}", to_rust_ident(field));
            let line = hash_line(&access, float_component(ty), false);
            self.w.line(&line);
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// `impl Hash` for an enum: the variant, then its payloads, a float by
    /// its bits.
    pub(crate) fn emit_value_hash_for_enum(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        let name = enum_decl.name.text.clone();
        self.emit_value_impl_head("std::hash::Hash", &name, &enum_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn hash<H: std::hash::Hasher>(&self, state: &mut H) {");
        self.w.indent_inc();
        self.w.line("use std::hash::Hash;");
        self.w.line("std::mem::discriminant(self).hash(state);");
        if enum_decl.variants.iter().any(|v| !v.payload.is_empty()) {
            self.w.line("match self {");
            self.w.indent_inc();
            for variant in &enum_decl.variants {
                let tag = format!("Self::{}", to_rust_ident(&variant.name.text));
                if variant.payload.is_empty() {
                    self.w.line(&format!("{tag} => {{}}"));
                    continue;
                }
                // Bind each payload by its declared name, or by position.
                let binds: Vec<String> = variant
                    .payload
                    .iter()
                    .enumerate()
                    .map(|(i, p)| p.name.as_ref().map(|n| to_rust_ident(&n.text)).unwrap_or_else(|| format!("p{i}")))
                    .collect();
                if binds.len() == 1 {
                    let line = hash_line(&binds[0], float_component(&variant.payload[0].ty), true);
                    self.w.line(&format!("{tag}({}) => {},", binds[0], line.trim_end_matches(';')));
                } else {
                    self.w.line(&format!("{tag}({}) => {{", binds.join(", ")));
                    self.w.indent_inc();
                    for (bind, payload) in binds.iter().zip(&variant.payload) {
                        self.w.line(&hash_line(bind, float_component(&payload.ty), true));
                    }
                    self.w.indent_dec();
                    self.w.line("}");
                }
            }
            self.w.indent_dec();
            self.w.line("}");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }
}

/// One statement hashing `value` into `state`. `by_ref` says `value` is a
/// reference (a `match` binding), so a float needs a `*` to read it.
fn hash_line(value: &str, float: Option<FloatComponent>, by_ref: bool) -> String {
    match float {
        None => format!("{value}.hash(state);"),
        Some(f) => {
            let bits = if f.wide { "crate::jux_f64_bits" } else { "crate::jux_f32_bits" };
            if f.nullable {
                format!("{value}.map({bits}).hash(state);")
            } else if by_ref {
                format!("{bits}(*{value}).hash(state);")
            } else {
                format!("{bits}({value}).hash(state);")
            }
        }
    }
}
