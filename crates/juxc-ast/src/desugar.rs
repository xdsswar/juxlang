//! Property desugaring per JUX-MISSING-DEFS §M.7.
//!
//! C#-style properties are parsed losslessly into [`PropertyDecl`]
//! nodes hanging off [`ClassDecl::properties`]. This pass rewrites each
//! property into the lower-level members the rest of the compiler
//! already understands:
//!
//! - a **private backing field** (auto-properties only), named
//!   `__prop_<Name>`, carrying the property's `= init` default, and
//! - a **getter** [`FnDecl`] named `<Name>` with `is_property = true`
//!   (so the backend rewrites `obj.Name` reads as `obj.Name()`), and
//! - a **setter** [`FnDecl`] named `__set_<Name>(value: T)` when the
//!   property is writable.
//!
//! The synthesized members flow through the existing field / method
//! pipeline (symbol-table registration, wrapper-class lowering,
//! borrow rewrites). The [`PropertyDecl`] list is *kept* on the class
//! so tycheck can enforce §M.7.2 access control and the backend can
//! route `obj.Name = v` writes to the synthesized setter / backing
//! field.
//!
//! **Backing-field naming.** The backing field is deliberately *not*
//! named after the property — it's `__prop_<Name>`. That keeps the
//! setter's own body (`this.__prop_Name = value;`) from being
//! re-routed back through the property-setter rewrite in the backend,
//! which would recurse forever.

use juxc_source::Span;

use crate::common::{Ident, Visibility};
use crate::decls::{
    AccessorBody, ClassDecl, FieldDecl, FnDecl, Param, PropertyDecl, ReturnType, TopLevelDecl,
};
use crate::exprs::{Expr, FieldExpr};
use crate::stmts::{AssignStmt, Block, Stmt};
use crate::CompilationUnit;

/// Backing-field name for an auto-property. Distinct from the property
/// name so the synthesized setter's `this.__prop_X = value;` doesn't
/// re-trigger the property-setter rewrite.
pub fn backing_field_name(prop_name: &str) -> String {
    format!("__prop_{prop_name}")
}

/// Synthesized setter method name for a writable property.
pub fn setter_method_name(prop_name: &str) -> String {
    format!("__set_{prop_name}")
}

/// Rewrite every property on every class in `unit` into backing
/// fields + getter / setter methods. Idempotent in practice (a class
/// with no `properties` is left untouched), and recurses into nested
/// class declarations so `Outer.Inner` properties desugar too.
pub fn desugar_properties(unit: &mut CompilationUnit) {
    for item in &mut unit.items {
        desugar_top_level(item);
    }
}

fn desugar_top_level(item: &mut TopLevelDecl) {
    if let TopLevelDecl::Class(class) = item {
        desugar_class(class);
    }
}

fn desugar_class(class: &mut ClassDecl) {
    // Recurse into nested types first so a nested class's own
    // properties are handled regardless of nesting depth.
    for nested in &mut class.nested_types {
        desugar_top_level(nested);
    }
    if class.properties.is_empty() {
        return;
    }
    // Build the set of auto-property names (those with a backing
    // field). Constructor bodies that write `this.AutoProp = e` must
    // target the backing field `__prop_AutoProp` directly — a
    // constructor is the one place read-only / init-only autos may be
    // set, and routing those through the (non-existent) public setter
    // would be wrong. Computed-setter properties keep their
    // `this.Prop = e` form and get routed to the setter by the backend.
    let backing_props: std::collections::HashSet<String> = class
        .properties
        .iter()
        .filter(|p| p.has_backing_field)
        .map(|p| p.name.text.clone())
        .collect();
    if !backing_props.is_empty() {
        for ctor in &mut class.constructors {
            rewrite_block_property_writes(&mut ctor.body, &backing_props);
        }
    }
    // Instance-member names (non-static fields + non-static
    // properties) — a bare reference to one inside a custom accessor
    // body means `this.<name>` (Java/C# implicit-this). The desugarer
    // rewrites those so a getter like `FullName => First + " " + Last`
    // reads `this.First()` / `this.Last()` and a setter like
    // `set => _x = value` writes `this._x`. Static members are
    // excluded — they resolve through the class name.
    let mut instance_members: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for field in &class.fields {
        if !field.is_static {
            instance_members.insert(field.name.text.clone());
        }
    }
    for prop in &class.properties {
        if !prop.is_static {
            instance_members.insert(prop.name.text.clone());
        }
    }
    // Collect synthesized members, then prepend the backing fields and
    // append the accessor methods. We clone the property list so the
    // class keeps it for tycheck / backend metadata.
    let mut new_fields: Vec<FieldDecl> = Vec::new();
    let mut new_methods: Vec<FnDecl> = Vec::new();
    for prop in &class.properties {
        lower_one_property(prop, &instance_members, &mut new_fields, &mut new_methods);
    }
    // Backing fields go in front so the constructor's `Self { … }`
    // literal lists them; ordering among fields is otherwise
    // irrelevant.
    for f in new_fields.into_iter().rev() {
        class.fields.insert(0, f);
    }
    class.methods.extend(new_methods);
}

/// Lower a single property into a backing field (if needed) plus
/// getter / setter methods.
fn lower_one_property(
    prop: &PropertyDecl,
    instance_members: &std::collections::HashSet<String>,
    fields: &mut Vec<FieldDecl>,
    methods: &mut Vec<FnDecl>,
) {
    let span = prop.span;
    let backing = backing_field_name(&prop.name.text);

    // ---- backing field (auto-properties only) ----
    if prop.has_backing_field {
        fields.push(FieldDecl {
            annotations: Vec::new(),
            visibility: Visibility::Private,
            is_static: prop.is_static,
            // Read-only / init-only autos are non-reassignable after
            // construction; mark the backing field `final` so the
            // backend's static-field shape and any final-aware path
            // agree. (Instance `final` is informational today.)
            is_final: prop.setter.as_ref().map_or(true, |s| s.is_init),
            ty: Some(prop.ty.clone()),
            name: ident(&backing, span),
            default: prop.initializer.clone(),
            span,
        });
    }

    // ---- getter: `<T> <Name>() { <body> }`, is_property = true ----
    // The backing-field reference inside synthesized accessor bodies:
    // `this.__prop_Name` for instance properties, or a bare
    // `__prop_Name` path for static ones (the backend resolves a bare
    // static-field reference in a static method to the class static).
    let backing_ref = |span: Span| -> Expr {
        if prop.is_static {
            path_ident(&backing, span)
        } else {
            this_field(&backing, span)
        }
    };

    if let Some(getter) = &prop.getter {
        let body = match &getter.body {
            // Auto getter → `return <backing>;`.
            AccessorBody::Auto => block_return(backing_ref(span), span),
            // Expression-bodied getter → `return <expr>;` with
            // implicit-this applied to bare member references.
            AccessorBody::Expr(e) => {
                let mut e = e.clone();
                rewrite_implicit_this(&mut e, instance_members, &mut local_set());
                block_return(e, span)
            }
            // Full block → used verbatim (implicit-this applied).
            AccessorBody::Block(b) => {
                let mut b = b.clone();
                rewrite_block_implicit_this(&mut b, instance_members, &mut local_set());
                b
            }
        };
        let vis = getter.visibility.unwrap_or(prop.visibility);
        methods.push(FnDecl {
            annotations: Vec::new(),
            visibility: vis,
            modifiers: static_modifier(prop.is_static),
            return_type: ReturnType::Type(prop.ty.clone()),
            name: prop.name.clone(),
            generic_params: Vec::new(),
            params: Vec::new(),
            throws: Vec::new(),
            body: Some(body),
            is_property: true,
            span,
        });
    }

    // ---- setter: `void __set_<Name>(<T> value) { <body> }` ----
    if let Some(setter) = &prop.setter {
        // `value` is the implicit setter parameter — it must never be
        // rewritten to `this.value`.
        let mut setter_locals = local_set();
        setter_locals.insert("value".to_string());
        let body = match &setter.body {
            // Auto setter / init → `<backing> = value;`.
            AccessorBody::Auto => block_assign(
                backing_ref(span),
                path_value(span),
                span,
            ),
            // Expression-bodied setter → run the expression for its
            // side effect (value discarded). Parser wraps an
            // assignment form (`set => _x = value;`) as a Block, so
            // this `Expr` arm only sees pure side-effect expressions.
            AccessorBody::Expr(e) => {
                let mut e = e.clone();
                rewrite_implicit_this(&mut e, instance_members, &mut setter_locals);
                Block {
                    statements: vec![Stmt::Expr(e)],
                    span,
                }
            }
            // Full block (custom setter, or a parser-wrapped
            // `set => _x = value;`) → implicit-this applied.
            AccessorBody::Block(b) => {
                let mut b = b.clone();
                rewrite_block_implicit_this(&mut b, instance_members, &mut setter_locals);
                b
            }
        };
        let vis = setter.visibility.unwrap_or(prop.visibility);
        let value_param = Param {
            name: ident("value", span),
            ty: prop.ty.clone(),
            is_final: false,
            is_ref: false,
            default: None,
            span,
        };
        methods.push(FnDecl {
            annotations: Vec::new(),
            visibility: vis,
            modifiers: static_modifier(prop.is_static),
            return_type: ReturnType::Void,
            name: ident(&setter_method_name(&prop.name.text), span),
            generic_params: Vec::new(),
            params: vec![value_param],
            throws: Vec::new(),
            body: Some(body),
            is_property: false,
            span,
        });
    }
}

// ---------------------------------------------------------------------
// Implicit-`this` rewriting for custom accessor bodies
// ---------------------------------------------------------------------

/// Fresh empty local-name set for an accessor body walk.
fn local_set() -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

/// Rewrite bare instance-member references to `this.<member>` across a
/// block, tracking locally-declared names so they aren't mistaken for
/// members. `locals` carries names already in scope (e.g. `value` for
/// setters); it's extended in-place as `var` decls / loop vars appear.
fn rewrite_block_implicit_this(
    block: &mut Block,
    members: &std::collections::HashSet<String>,
    locals: &mut std::collections::HashSet<String>,
) {
    for stmt in &mut block.statements {
        match stmt {
            Stmt::VarDecl(v) => {
                if let Some(init) = &mut v.init {
                    rewrite_implicit_this(init, members, locals);
                }
                // The new local shadows any same-named member from
                // here on.
                locals.insert(v.name.text.clone());
            }
            Stmt::Assign(a) => {
                rewrite_implicit_this(&mut a.target, members, locals);
                rewrite_implicit_this(&mut a.value, members, locals);
            }
            Stmt::Expr(e) | Stmt::Throw(e, _) => {
                rewrite_implicit_this(e, members, locals);
            }
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    rewrite_implicit_this(e, members, locals);
                }
            }
            Stmt::SuperCall(args, _) => {
                for a in args {
                    rewrite_implicit_this(a, members, locals);
                }
            }
            Stmt::If(s) => {
                rewrite_implicit_this(&mut s.condition, members, locals);
                rewrite_block_implicit_this(&mut s.then_block, members, &mut locals.clone());
                if let Some(eb) = s.else_branch.as_deref_mut() {
                    rewrite_else_implicit_this(eb, members, locals);
                }
            }
            Stmt::While(s) => {
                rewrite_implicit_this(&mut s.condition, members, locals);
                rewrite_block_implicit_this(&mut s.body, members, &mut locals.clone());
            }
            Stmt::ForEach(s) => {
                rewrite_implicit_this(&mut s.iter, members, locals);
                let mut inner = locals.clone();
                inner.insert(s.var_name.text.clone());
                rewrite_block_implicit_this(&mut s.body, members, &mut inner);
            }
            Stmt::Try(t) => {
                rewrite_block_implicit_this(&mut t.body, members, &mut locals.clone());
                for c in &mut t.catches {
                    let mut inner = locals.clone();
                    inner.insert(c.name.text.clone());
                    rewrite_block_implicit_this(&mut c.body, members, &mut inner);
                }
                if let Some(f) = &mut t.finally {
                    rewrite_block_implicit_this(f, members, &mut locals.clone());
                }
            }
            Stmt::Unsafe(b) => {
                rewrite_block_implicit_this(b, members, &mut locals.clone());
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }
}

fn rewrite_else_implicit_this(
    eb: &mut crate::stmts::ElseBranch,
    members: &std::collections::HashSet<String>,
    locals: &std::collections::HashSet<String>,
) {
    match eb {
        crate::stmts::ElseBranch::Block(b) => {
            rewrite_block_implicit_this(b, members, &mut locals.clone())
        }
        crate::stmts::ElseBranch::If(inner) => {
            let mut l = locals.clone();
            rewrite_implicit_this(&mut inner.condition, members, &mut l);
            rewrite_block_implicit_this(&mut inner.then_block, members, &mut l.clone());
            if let Some(e) = inner.else_branch.as_deref_mut() {
                rewrite_else_implicit_this(e, members, &l);
            }
        }
    }
}

/// Rewrite bare instance-member references inside an expression. A
/// single-segment `Path(name)` whose `name` is an instance member and
/// is NOT a local becomes `this.name`. Recurses into sub-expressions.
fn rewrite_implicit_this(
    expr: &mut Expr,
    members: &std::collections::HashSet<String>,
    locals: &mut std::collections::HashSet<String>,
) {
    match expr {
        Expr::Path(qn) => {
            if qn.segments.len() == 1 {
                let name = &qn.segments[0].text;
                if members.contains(name) && !locals.contains(name) {
                    let span = qn.span;
                    *expr = this_field(name, span);
                }
            }
        }
        // Field access: rewrite the object (so `member.x` works), but
        // never the field segment itself.
        Expr::Field(f) => rewrite_implicit_this(&mut f.object, members, locals),
        Expr::Binary(b) => {
            rewrite_implicit_this(&mut b.left, members, locals);
            rewrite_implicit_this(&mut b.right, members, locals);
        }
        Expr::Unary(u) => rewrite_implicit_this(&mut u.operand, members, locals),
        Expr::Call(c) => {
            rewrite_implicit_this(&mut c.callee, members, locals);
            for a in &mut c.args {
                rewrite_implicit_this(a, members, locals);
            }
        }
        Expr::Index(i) => {
            rewrite_implicit_this(&mut i.array, members, locals);
            rewrite_implicit_this(&mut i.index, members, locals);
        }
        Expr::Ternary(t) => {
            rewrite_implicit_this(&mut t.condition, members, locals);
            rewrite_implicit_this(&mut t.then_branch, members, locals);
            rewrite_implicit_this(&mut t.else_branch, members, locals);
        }
        Expr::Cast(c) => rewrite_implicit_this(&mut c.value, members, locals),
        Expr::Range(r) => {
            rewrite_implicit_this(&mut r.start, members, locals);
            rewrite_implicit_this(&mut r.end, members, locals);
        }
        Expr::Elvis(e) => {
            rewrite_implicit_this(&mut e.value, members, locals);
            rewrite_implicit_this(&mut e.fallback, members, locals);
        }
        Expr::Await(inner, _) => rewrite_implicit_this(inner, members, locals),
        Expr::InterpString(s) => {
            for seg in &mut s.segments {
                if let crate::exprs::InterpSegment::Expr(inner) = seg {
                    rewrite_implicit_this(inner, members, locals);
                }
            }
        }
        // Lambda bodies introduce their own params; we don't rewrite
        // inside them (Phase-1 scope — accessor bodies rarely contain
        // lambdas that reference outer members bare).
        _ => {}
    }
}

// ---------------------------------------------------------------------
// Constructor property-write rewriting
// ---------------------------------------------------------------------

/// Within a constructor body, rewrite every `this.<AutoProp>` field
/// access (read or write) of an auto-property to its private backing
/// field `this.__prop_<AutoProp>`. This lets the constructor set
/// read-only / init-only auto-properties directly (§M.7.2) and keeps
/// the simple-ctor fast path working (it sees a plain field name).
fn rewrite_block_property_writes(
    block: &mut Block,
    auto_props: &std::collections::HashSet<String>,
) {
    for stmt in &mut block.statements {
        rewrite_stmt_property_writes(stmt, auto_props);
    }
}

fn rewrite_stmt_property_writes(
    stmt: &mut Stmt,
    auto_props: &std::collections::HashSet<String>,
) {
    match stmt {
        Stmt::Assign(a) => {
            rewrite_expr_property_access(&mut a.target, auto_props);
            rewrite_expr_property_access(&mut a.value, auto_props);
        }
        Stmt::Expr(e) | Stmt::Throw(e, _) => {
            rewrite_expr_property_access(e, auto_props);
        }
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                rewrite_expr_property_access(e, auto_props);
            }
        }
        Stmt::VarDecl(v) => {
            if let Some(e) = &mut v.init {
                rewrite_expr_property_access(e, auto_props);
            }
        }
        Stmt::SuperCall(args, _) => {
            for a in args {
                rewrite_expr_property_access(a, auto_props);
            }
        }
        Stmt::If(s) => {
            rewrite_expr_property_access(&mut s.condition, auto_props);
            rewrite_block_property_writes(&mut s.then_block, auto_props);
            if let Some(eb) = s.else_branch.as_deref_mut() {
                rewrite_else_branch(eb, auto_props);
            }
        }
        Stmt::While(s) => {
            rewrite_expr_property_access(&mut s.condition, auto_props);
            rewrite_block_property_writes(&mut s.body, auto_props);
        }
        Stmt::ForEach(s) => {
            rewrite_expr_property_access(&mut s.iter, auto_props);
            rewrite_block_property_writes(&mut s.body, auto_props);
        }
        Stmt::Try(t) => {
            rewrite_block_property_writes(&mut t.body, auto_props);
            for c in &mut t.catches {
                rewrite_block_property_writes(&mut c.body, auto_props);
            }
            if let Some(f) = &mut t.finally {
                rewrite_block_property_writes(f, auto_props);
            }
        }
        Stmt::Unsafe(b) => rewrite_block_property_writes(b, auto_props),
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

fn rewrite_else_branch(
    eb: &mut crate::stmts::ElseBranch,
    auto_props: &std::collections::HashSet<String>,
) {
    match eb {
        crate::stmts::ElseBranch::Block(b) => rewrite_block_property_writes(b, auto_props),
        crate::stmts::ElseBranch::If(inner) => {
            rewrite_expr_property_access(&mut inner.condition, auto_props);
            rewrite_block_property_writes(&mut inner.then_block, auto_props);
            if let Some(e) = inner.else_branch.as_deref_mut() {
                rewrite_else_branch(e, auto_props);
            }
        }
    }
}

/// Rewrite `this.<AutoProp>` field accesses to `this.__prop_<AutoProp>`
/// anywhere inside `expr`. Only the `this`-rooted form is rewritten —
/// a property access on some *other* object inside a constructor (rare)
/// keeps going through the normal getter/setter path.
fn rewrite_expr_property_access(
    expr: &mut Expr,
    auto_props: &std::collections::HashSet<String>,
) {
    match expr {
        Expr::Field(f) => {
            // Recurse into the object first.
            rewrite_expr_property_access(&mut f.object, auto_props);
            if matches!(&*f.object, Expr::This(_)) && auto_props.contains(&f.field.text) {
                f.field.text = backing_field_name(&f.field.text);
            }
        }
        Expr::Binary(b) => {
            rewrite_expr_property_access(&mut b.left, auto_props);
            rewrite_expr_property_access(&mut b.right, auto_props);
        }
        Expr::Unary(u) => rewrite_expr_property_access(&mut u.operand, auto_props),
        Expr::Call(c) => {
            rewrite_expr_property_access(&mut c.callee, auto_props);
            for a in &mut c.args {
                rewrite_expr_property_access(a, auto_props);
            }
        }
        Expr::Index(i) => {
            rewrite_expr_property_access(&mut i.array, auto_props);
            rewrite_expr_property_access(&mut i.index, auto_props);
        }
        Expr::Ternary(t) => {
            rewrite_expr_property_access(&mut t.condition, auto_props);
            rewrite_expr_property_access(&mut t.then_branch, auto_props);
            rewrite_expr_property_access(&mut t.else_branch, auto_props);
        }
        Expr::Cast(c) => rewrite_expr_property_access(&mut c.value, auto_props),
        _ => {}
    }
}

// ---------------------------------------------------------------------
// Small AST builders
// ---------------------------------------------------------------------

fn ident(text: &str, span: Span) -> Ident {
    Ident { text: text.to_string(), span }
}

fn static_modifier(is_static: bool) -> Vec<crate::decls::FnModifier> {
    if is_static {
        vec![crate::decls::FnModifier::Static]
    } else {
        Vec::new()
    }
}

/// `this.<field>` as an expression.
fn this_field(field: &str, span: Span) -> Expr {
    Expr::Field(FieldExpr {
        object: Box::new(Expr::This(span)),
        field: ident(field, span),
        safe: false,
        span,
    })
}

/// A bare `value` path expression (the implicit setter parameter).
fn path_value(span: Span) -> Expr {
    path_ident("value", span)
}

/// A bare single-segment path expression naming `name`.
fn path_ident(name: &str, span: Span) -> Expr {
    Expr::Path(crate::common::QualifiedName {
        segments: vec![ident(name, span)],
        span,
    })
}

/// `{ return <expr>; }`.
fn block_return(expr: Expr, span: Span) -> Block {
    Block {
        statements: vec![Stmt::Return(Some(expr))],
        span,
    }
}

/// `{ <target> = <value>; }`.
fn block_assign(target: Expr, value: Expr, span: Span) -> Block {
    Block {
        statements: vec![Stmt::Assign(AssignStmt {
            target,
            op: None,
            value,
            span,
        })],
        span,
    }
}
