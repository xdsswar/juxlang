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
    pub(crate) fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(lit) => self.emit_literal(lit),
            Expr::Path(qn) => {
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
            Expr::Call(c) => self.emit_call(c),
            Expr::Binary(b) => self.emit_binary(b),
            Expr::Unary(u) => self.emit_unary(u),
            Expr::Range(r) => self.emit_range(r),
            Expr::Cast(c) => self.emit_cast(c),
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
            Expr::Switch(s) => self.emit_switch(s),
            Expr::NewObject(n) => {
                // `new Foo(args)`        → `Foo::new(args)`.
                // `new Foo<int>(args)`   → `Foo::<isize>::new(args)`
                //                          (Rust turbofish — required
                //                          on the type position before
                //                          the method-call `::new`).
                // The class path is single-segment in practice today
                // but stays `path-joined` for forward compatibility.
                let path = n
                    .class_name
                    .segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
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
                self.w.push_str("::new(");
                for (i, arg) in n.args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.w.push(')');
            }
        }
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
        Expr::Path(qn) => qn.span,
        Expr::Call(c) => c.span,
        Expr::Binary(b) => b.span,
        Expr::Unary(u) => u.span,
        Expr::Range(r) => r.span,
        Expr::Cast(c) => c.span,
        Expr::SizeOf(s) => s.span,
        Expr::NewArray(n) => n.span,
        Expr::NewArrayLit(n) => n.span,
        Expr::Index(i) => i.span,
        Expr::Field(f) => f.span,
        Expr::InterpString(s) => s.span,
        Expr::This(s) => *s,
        Expr::NewObject(n) => n.span,
        Expr::Switch(s) => s.span,
    }
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
        let element_ref = juxc_ast::TypeRef {
            name: t.name.clone(),
            generic_args: t.generic_args.clone(),
            nullable: t.nullable,
            array_shape: None,
            span: t.span,
        };
        let element = ty_kind_from_ref_with_params(&element_ref, generic_params);
        let kind = match shape {
            juxc_ast::ArrayShape::Fixed(_) => ArrayKind::Fixed,
            juxc_ast::ArrayShape::Dynamic => ArrayKind::Dynamic,
        };
        return Ty::Array {
            element: Box::new(element),
            kind,
        };
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
        BinaryOp::Eq | BinaryOp::NotEq => 6,
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => 7,
        BinaryOp::BitOr  => 8,
        BinaryOp::BitXor => 9,
        BinaryOp::BitAnd => 10,
        BinaryOp::Shl | BinaryOp::Shr => 11,
        BinaryOp::Add | BinaryOp::Sub => 12,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 13,
    }
}

