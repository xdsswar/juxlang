//! Reject `T?` where T is a non-nullable value-type primitive.
//!
//! Per the language design, only **reference types** carry the
//! nullable marker:
//!
//! - `String?`, `MyClass?`, `MyRecord?`, `MyEnum?`, `T?` (generic
//!   param), arrays of reference types — all OK.
//! - `int?`, `long?`, `bool?`, `char?`, `float?`, `double?`, the
//!   unsigned and width-explicit numerics — all **rejected**.
//!   Primitives are value types with a meaningful default; they
//!   never hold null.
//!
//! The check is a syntactic walk over every `TypeRef` in the unit —
//! no type inference needed, just look at `name.segments` and the
//! `nullable` flag. Diagnostics fire as `E0410_TypeMismatch` (the
//! generic type-error code) with a message that names the offending
//! primitive, so the user sees the specific shape they wrote.
//!
//! Why a pre-pass instead of folding into the per-position checks?
//! TypeRefs appear in many places (params, returns, fields,
//! components, local annotations, generic args, cast targets, …)
//! and one centralized walker keeps the rule in one spot.

use juxc_ast::{
    BinaryExpr, Block, CallExpr, ClassDecl, CompilationUnit, ConstructorDecl, ElseBranch,
    EnumDecl, Expr, FieldDecl, FnDecl, ForEachStmt, GenericArg, IfStmt, IndexExpr,
    InterfaceDecl, LambdaBody, LambdaExpr, NewArrayExpr, NewArrayLitExpr, NewObjectExpr,
    OperatorDecl, Pattern, RangeExpr, RecordDecl, Stmt, SwitchBody, SwitchExpr, TopLevelDecl,
    TypeRef, UnaryExpr, WhileStmt,
};
use juxc_diagnostics::{code, Diagnostic};

/// Walk `unit` and emit a diagnostic at every `T?` where `T` is a
/// non-nullable primitive. Idempotent — call once per unit during
/// tycheck's top-level walk.
pub(crate) fn check_nullable_primitives(unit: &CompilationUnit, diags: &mut Vec<Diagnostic>) {
    for item in &unit.items {
        match item {
            TopLevelDecl::Function(fn_decl) => check_fn_decl(fn_decl, diags),
            TopLevelDecl::Class(class_decl) => check_class_decl(class_decl, diags),
            TopLevelDecl::Record(record_decl) => check_record_decl(record_decl, diags),
            TopLevelDecl::Enum(enum_decl) => check_enum_decl(enum_decl, diags),
            TopLevelDecl::Interface(iface) => check_interface_decl(iface, diags),
            TopLevelDecl::Const(const_decl) => {
                check_type_ref(&const_decl.ty, diags);
                check_expr(&const_decl.value, diags);
            }
            TopLevelDecl::TypeAlias(alias) => {
                check_type_ref(&alias.target, diags);
            }
        }
    }
}

fn check_fn_decl(fn_decl: &FnDecl, diags: &mut Vec<Diagnostic>) {
    for p in &fn_decl.params {
        check_type_ref(&p.ty, diags);
    }
    if let juxc_ast::ReturnType::Type(t) = &fn_decl.return_type {
        check_type_ref(t, diags);
    }
    if let juxc_ast::ReturnType::AsyncType(t) = &fn_decl.return_type {
        check_type_ref(t, diags);
    }
    if let Some(body) = &fn_decl.body {
        check_block(body, diags);
    }
}

fn check_class_decl(class_decl: &ClassDecl, diags: &mut Vec<Diagnostic>) {
    if let Some(ext) = &class_decl.extends {
        check_type_ref(ext, diags);
    }
    for imp in &class_decl.implements {
        check_type_ref(imp, diags);
    }
    for field in &class_decl.fields {
        check_field_decl(field, diags);
    }
    for ctor in &class_decl.constructors {
        check_ctor_decl(ctor, diags);
    }
    for method in &class_decl.methods {
        check_fn_decl(method, diags);
    }
    for op in &class_decl.operators {
        check_operator_decl(op, diags);
    }
}

fn check_record_decl(record_decl: &RecordDecl, diags: &mut Vec<Diagnostic>) {
    for comp in &record_decl.components {
        check_type_ref(&comp.ty, diags);
    }
    for op in &record_decl.operators {
        check_operator_decl(op, diags);
    }
}

fn check_enum_decl(enum_decl: &EnumDecl, diags: &mut Vec<Diagnostic>) {
    for variant in &enum_decl.variants {
        for slot in &variant.payload {
            check_type_ref(&slot.ty, diags);
        }
    }
    for op in &enum_decl.operators {
        check_operator_decl(op, diags);
    }
}

fn check_interface_decl(iface: &InterfaceDecl, diags: &mut Vec<Diagnostic>) {
    for method in &iface.methods {
        check_fn_decl(method, diags);
    }
}

fn check_field_decl(field: &FieldDecl, diags: &mut Vec<Diagnostic>) {
    check_type_ref(&field.ty, diags);
    if let Some(init) = &field.default {
        check_expr(init, diags);
    }
}

fn check_ctor_decl(ctor: &ConstructorDecl, diags: &mut Vec<Diagnostic>) {
    for p in &ctor.params {
        check_type_ref(&p.ty, diags);
    }
    check_block(&ctor.body, diags);
}

fn check_operator_decl(op: &OperatorDecl, diags: &mut Vec<Diagnostic>) {
    for p in &op.params {
        check_type_ref(&p.ty, diags);
    }
    if let juxc_ast::ReturnType::Type(t) = &op.return_type {
        check_type_ref(t, diags);
    }
    if let Some(body) = &op.body {
        check_block(body, diags);
    }
}

/// The actual leaf check: emit a diagnostic when `ty` is `T?` and
/// `T` resolves to a primitive name. Recursive on `generic_args`
/// (so `List<int?>` flags the inner `int?` even though the outer
/// `List<…>` is fine).
fn check_type_ref(ty: &TypeRef, diags: &mut Vec<Diagnostic>) {
    // The outer `T?`-on-primitive check. Only fires when:
    // - the type carries `nullable = true`,
    // - the name is a single segment (multi-segment paths like
    //   `pkg.Foo` are user types and can't be primitives),
    // - the segment text matches one of the non-nullable primitive
    //   names.
    //
    // Generic args (`List<int?>`) and array elements still recurse
    // below so nested nullable-primitives get caught.
    if ty.nullable
        && ty.name.segments.len() == 1
        && is_nonnullable_primitive(&ty.name.segments[0].text)
    {
        let name = &ty.name.segments[0].text;
        diags.push(
            Diagnostic::error(
                code::Code::E0410_TypeMismatch,
                format!(
                    "primitive type `{name}` cannot be nullable — `{name}?` is invalid; \
                     only reference types (`String`, classes, records, enums) carry `?`",
                ),
            )
            .with_span(ty.span),
        );
    }
    // Generic args: recurse into each one. Wildcards (`? extends T`)
    // don't carry their own nullable flag; the bound's TypeRef does.
    for arg in &ty.generic_args {
        match arg {
            GenericArg::Type(t) => check_type_ref(t, diags),
            GenericArg::Wildcard(w) => match &w.bound {
                None => {}
                Some(juxc_ast::WildcardBound::Extends(t))
                | Some(juxc_ast::WildcardBound::Super(t)) => check_type_ref(t, diags),
            },
        }
    }
    // Function-type shape — params and return are TypeRefs too.
    if let Some(fn_shape) = &ty.fn_shape {
        for p in &fn_shape.params {
            check_type_ref(p, diags);
        }
        check_type_ref(&fn_shape.return_type, diags);
    }
    // Array shape — the size expression may itself contain a type
    // (unusual but possible via `sizeof`).
    if let Some(juxc_ast::ArrayShape::Fixed(size)) = &ty.array_shape {
        check_expr(size, diags);
    }
}

/// True when `name` is one of Jux's value-type primitives — the ones
/// that can't carry a `?` per the language design. `String` is
/// deliberately excluded: it's a reference type even though it
/// shares the "primitive-named" namespace.
fn is_nonnullable_primitive(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "byte" | "ubyte"
            | "short" | "ushort"
            | "int" | "uint"
            | "long" | "ulong"
            | "float" | "double"
            | "char"
            | "i8" | "u8"
            | "i16" | "u16"
            | "i32" | "u32"
            | "i64" | "u64"
            | "f32" | "f64"
    )
}

// ----------------------------------------------------------------------
// Recursive walkers — blocks, statements, expressions all need to
// surface type references nested inside them.
// ----------------------------------------------------------------------

fn check_block(block: &Block, diags: &mut Vec<Diagnostic>) {
    for stmt in &block.statements {
        check_stmt(stmt, diags);
    }
}

fn check_stmt(stmt: &Stmt, diags: &mut Vec<Diagnostic>) {
    match stmt {
        Stmt::Expr(e) => check_expr(e, diags),
        Stmt::Return(Some(e)) => check_expr(e, diags),
        Stmt::Return(None) => {}
        Stmt::VarDecl(v) => {
            if let Some(t) = &v.ty {
                check_type_ref(t, diags);
            }
            if let Some(init) = &v.init {
                check_expr(init, diags);
            }
        }
        Stmt::Assign(a) => {
            check_expr(&a.target, diags);
            check_expr(&a.value, diags);
        }
        Stmt::If(s) => check_if(s, diags),
        Stmt::While(s) => check_while(s, diags),
        Stmt::ForEach(s) => check_for_each(s, diags),
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::SuperCall(args, _) => {
            for a in args {
                check_expr(a, diags);
            }
        }
    }
}

fn check_if(s: &IfStmt, diags: &mut Vec<Diagnostic>) {
    check_expr(&s.condition, diags);
    check_block(&s.then_block, diags);
    let mut cursor = s.else_branch.as_deref();
    while let Some(branch) = cursor {
        match branch {
            ElseBranch::If(inner) => {
                check_expr(&inner.condition, diags);
                check_block(&inner.then_block, diags);
                cursor = inner.else_branch.as_deref();
            }
            ElseBranch::Block(b) => {
                check_block(b, diags);
                cursor = None;
            }
        }
    }
}

fn check_while(s: &WhileStmt, diags: &mut Vec<Diagnostic>) {
    check_expr(&s.condition, diags);
    check_block(&s.body, diags);
}

fn check_for_each(s: &ForEachStmt, diags: &mut Vec<Diagnostic>) {
    if let Some(t) = &s.var_type {
        check_type_ref(t, diags);
    }
    check_expr(&s.iter, diags);
    check_block(&s.body, diags);
}

fn check_expr(e: &Expr, diags: &mut Vec<Diagnostic>) {
    match e {
        Expr::Literal(_) | Expr::Path(_) | Expr::This(_) => {}
        Expr::Call(c) => check_call(c, diags),
        Expr::Binary(b) => check_binary(b, diags),
        Expr::Unary(u) => check_unary(u, diags),
        Expr::Range(r) => check_range(r, diags),
        Expr::Cast(c) => {
            check_expr(&c.value, diags);
            check_type_ref(&c.ty, diags);
        }
        Expr::SizeOf(s) => check_expr(&s.operand, diags),
        Expr::NewArray(n) => check_new_array(n, diags),
        Expr::NewArrayLit(n) => check_new_array_lit(n, diags),
        Expr::Index(i) => check_index(i, diags),
        Expr::Field(f) => check_expr(&f.object, diags),
        Expr::InterpString(s) => {
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Expr(inner) = seg {
                    check_expr(inner, diags);
                }
            }
        }
        Expr::NewObject(n) => check_new_object(n, diags),
        Expr::Switch(s) => check_switch(s, diags),
        Expr::Lambda(l) => check_lambda(l, diags),
        Expr::Elvis(e) => {
            check_expr(&e.value, diags);
            check_expr(&e.fallback, diags);
        }
    }
}

fn check_call(c: &CallExpr, diags: &mut Vec<Diagnostic>) {
    check_expr(&c.callee, diags);
    for a in &c.args {
        check_expr(a, diags);
    }
}

fn check_binary(b: &BinaryExpr, diags: &mut Vec<Diagnostic>) {
    check_expr(&b.left, diags);
    check_expr(&b.right, diags);
}

fn check_unary(u: &UnaryExpr, diags: &mut Vec<Diagnostic>) {
    check_expr(&u.operand, diags);
}

fn check_range(r: &RangeExpr, diags: &mut Vec<Diagnostic>) {
    check_expr(&r.start, diags);
    check_expr(&r.end, diags);
}

fn check_new_array(n: &NewArrayExpr, diags: &mut Vec<Diagnostic>) {
    check_type_ref(&n.element_type, diags);
    check_expr(&n.size, diags);
}

fn check_new_array_lit(n: &NewArrayLitExpr, diags: &mut Vec<Diagnostic>) {
    check_type_ref(&n.element_type, diags);
    for e in &n.elements {
        check_expr(e, diags);
    }
}

fn check_index(i: &IndexExpr, diags: &mut Vec<Diagnostic>) {
    check_expr(&i.array, diags);
    check_expr(&i.index, diags);
}

fn check_new_object(n: &NewObjectExpr, diags: &mut Vec<Diagnostic>) {
    for g in &n.generic_args {
        check_type_ref(g, diags);
    }
    for a in &n.args {
        check_expr(a, diags);
    }
}

fn check_switch(s: &SwitchExpr, diags: &mut Vec<Diagnostic>) {
    check_expr(&s.scrutinee, diags);
    for arm in &s.arms {
        check_pattern(&arm.pattern, diags);
        match &arm.body {
            SwitchBody::Expr(e) => check_expr(e, diags),
            SwitchBody::Block(b) => check_block(b, diags),
        }
    }
}

fn check_pattern(p: &Pattern, diags: &mut Vec<Diagnostic>) {
    // Patterns don't carry TypeRefs today (no type-test pattern with
    // explicit type), but they nest into themselves recursively for
    // enum-variant args. Future smart-cast `=> Foo f` would surface
    // a type here; left as a no-op for now.
    if let Pattern::EnumVariant { args, .. } = p {
        for sub in args {
            check_pattern(sub, diags);
        }
    }
}

fn check_lambda(l: &LambdaExpr, diags: &mut Vec<Diagnostic>) {
    for p in &l.params {
        if let Some(t) = &p.ty {
            check_type_ref(t, diags);
        }
    }
    match &l.body {
        LambdaBody::Expr(e) => check_expr(e, diags),
        LambdaBody::Block(b) => check_block(b, diags),
    }
}
