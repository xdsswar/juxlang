//! A mutable walk over every place a unit NAMES A TYPE.
//!
//! [`crate::visit`] reaches expressions and statements; this reaches type
//! names wherever they are written, so a pass can rewrite them in place:
//! every [`TypeRef`] (declarations, locals, casts, type tests, `catch`,
//! lambda parameters, generic arguments and bounds, function types), the class
//! of a `new`, a `throws` name, a method reference's receiver, the type of a
//! `case T t` pattern, and the head of an expression path (`Circle.unit()`),
//! which may name a type or a value.
//!
//! The walk keeps the names that would SHADOW a type name at each point: the
//! generic parameters in scope, and the locals, parameters and fields of the
//! enclosing body. A rewriter reads them from the [`Scope`] it is handed. The
//! first use is import aliases (`import app.model.Circle as C;`), which are
//! written back as the type they name before anything resolves them
//! (ERRATA E133).

use std::collections::HashSet;

use crate::{
    AccessorBody, Block, CompilationUnit, ElseBranch, Expr, FnDecl, GenericArg, Ident,
    InterpSegment, LambdaBody, NewObjectExpr, Param, Pattern, QualifiedName, ReturnType, Stmt,
    SwitchBody, TopLevelDecl, TypeParam, TypeRef, WildcardBound,
};

/// The names that shadow a type name where a rewriter is called.
#[derive(Debug, Default)]
pub struct Scope {
    generics: Vec<HashSet<String>>,
    locals: Vec<HashSet<String>>,
}

impl Scope {
    /// `name` is a generic parameter in scope.
    pub fn is_generic(&self, name: &str) -> bool {
        self.generics.iter().any(|s| s.contains(name))
    }

    /// `name` is a local, a parameter or a field of the enclosing body: in an
    /// expression it names that value, never a type (JLS 6.4.2).
    pub fn is_value(&self, name: &str) -> bool {
        self.locals.iter().any(|s| s.contains(name))
    }
}

/// What a pass does at each type name. Every method defaults to nothing.
pub trait TypeNameRewriter {
    /// A type reference, before its own generic arguments are walked.
    fn type_ref(&mut self, _ty: &mut TypeRef, _scope: &Scope) {}
    /// The class of a `new` (with its generic arguments), before its own
    /// arguments are walked.
    fn new_object(&mut self, _new: &mut NewObjectExpr, _scope: &Scope) {}
    /// A type named by a bare [`QualifiedName`]: a `throws` entry, a method
    /// reference's receiver.
    fn type_name(&mut self, _name: &mut QualifiedName, _scope: &Scope) {}
    /// The path of an expression (`Circle.unit`, `Color.GREEN`, `x`).
    fn expr_path(&mut self, _path: &mut QualifiedName, _scope: &Scope) {}
    /// The type of a `case T t` pattern.
    fn type_pattern(&mut self, _name: &mut Ident, _scope: &Scope) {}
}

/// Walk every type name of `unit`.
pub fn rewrite_type_names(unit: &mut CompilationUnit, r: &mut dyn TypeNameRewriter) {
    let mut w = Walker { r, scope: Scope::default() };
    for item in &mut unit.items {
        w.top_level(item);
    }
}

struct Walker<'a> {
    r: &'a mut dyn TypeNameRewriter,
    scope: Scope,
}

fn generic_names(params: &[TypeParam]) -> HashSet<String> {
    params.iter().map(|p| p.name.text.clone()).collect()
}

impl Walker<'_> {
    fn with_generics(&mut self, params: &[TypeParam], f: impl FnOnce(&mut Self)) {
        self.scope.generics.push(generic_names(params));
        f(self);
        self.scope.generics.pop();
    }

    fn with_locals(&mut self, names: HashSet<String>, f: impl FnOnce(&mut Self)) {
        self.scope.locals.push(names);
        f(self);
        self.scope.locals.pop();
    }

    fn generic_params(&mut self, params: &mut [TypeParam]) {
        for p in params {
            for b in &mut p.bounds {
                self.ty(b);
            }
            if let Some(t) = &mut p.const_ty {
                self.ty(t);
            }
        }
    }

    fn top_level(&mut self, item: &mut TopLevelDecl) {
        match item {
            TopLevelDecl::Function(f) => self.function(f),
            TopLevelDecl::Class(c) => {
                let fields: HashSet<String> = c
                    .fields
                    .iter()
                    .map(|f| f.name.text.clone())
                    .chain(c.properties.iter().map(|p| p.name.text.clone()))
                    .collect();
                let params = c.generic_params.clone();
                self.with_generics(&params, |w| {
                    w.generic_params(&mut c.generic_params);
                    if let Some(e) = &mut c.extends {
                        w.ty(e);
                    }
                    for i in &mut c.implements {
                        w.ty(i);
                    }
                    w.with_locals(fields, |w| {
                        for f in &mut c.fields {
                            if let Some(t) = &mut f.ty {
                                w.ty(t);
                            }
                            if let Some(d) = &mut f.default {
                                w.expr(d);
                            }
                        }
                        for p in &mut c.properties {
                            w.ty(&mut p.ty);
                            if let Some(i) = &mut p.initializer {
                                w.expr(i);
                            }
                            for body in [
                                p.getter.as_mut().map(|g| &mut g.body),
                                p.setter.as_mut().map(|s| &mut s.body),
                            ]
                            .into_iter()
                            .flatten()
                            {
                                match body {
                                    AccessorBody::Auto => {}
                                    AccessorBody::Expr(e) => w.expr(e),
                                    AccessorBody::Block(b) => w.body(b, &[]),
                                }
                            }
                        }
                        for ctor in &mut c.constructors {
                            w.params(&mut ctor.params);
                            for t in &mut ctor.throws {
                                w.r.type_name(t, &w.scope);
                            }
                            let params = ctor.params.clone();
                            w.body(&mut ctor.body, &params);
                        }
                        for m in &mut c.methods {
                            w.function(m);
                        }
                        for op in &mut c.operators {
                            w.params(&mut op.params);
                            w.return_type(&mut op.return_type);
                            let params = op.params.clone();
                            if let Some(b) = &mut op.body {
                                w.body(b, &params);
                            }
                        }
                        for b in c
                            .init_blocks
                            .iter_mut()
                            .chain(c.static_init_blocks.iter_mut())
                            .chain(c.drop_blocks.iter_mut())
                        {
                            w.body(b, &[]);
                        }
                    });
                    for n in &mut c.nested_types {
                        w.top_level(n);
                    }
                });
            }
            TopLevelDecl::Record(r) => {
                let comps: HashSet<String> = r.components.iter().map(|c| c.name.text.clone()).collect();
                let params = r.generic_params.clone();
                self.with_generics(&params, |w| {
                    w.generic_params(&mut r.generic_params);
                    for c in &mut r.components {
                        w.ty(&mut c.ty);
                    }
                    for i in &mut r.implements {
                        w.ty(i);
                    }
                    w.with_locals(comps, |w| {
                        for f in &mut r.static_fields {
                            if let Some(t) = &mut f.ty {
                                w.ty(t);
                            }
                            if let Some(d) = &mut f.default {
                                w.expr(d);
                            }
                        }
                        for ctor in &mut r.constructors {
                            w.params(&mut ctor.params);
                            let params = ctor.params.clone();
                            w.body(&mut ctor.body, &params);
                        }
                        for m in &mut r.methods {
                            w.function(m);
                        }
                        for op in &mut r.operators {
                            w.params(&mut op.params);
                            w.return_type(&mut op.return_type);
                            let params = op.params.clone();
                            if let Some(b) = &mut op.body {
                                w.body(b, &params);
                            }
                        }
                    });
                });
            }
            TopLevelDecl::Enum(e) => {
                let fields: HashSet<String> = e.fields.iter().map(|f| f.name.text.clone()).collect();
                let params = e.generic_params.clone();
                self.with_generics(&params, |w| {
                    w.generic_params(&mut e.generic_params);
                    for i in &mut e.implements {
                        w.ty(i);
                    }
                    for v in &mut e.variants {
                        for p in &mut v.payload {
                            w.ty(&mut p.ty);
                        }
                        if let Some(d) = &mut v.discriminant {
                            w.expr(d);
                        }
                        for a in &mut v.args {
                            w.expr(a);
                        }
                    }
                    w.with_locals(fields, |w| {
                        for f in e.fields.iter_mut().chain(e.constants.iter_mut()) {
                            if let Some(t) = &mut f.ty {
                                w.ty(t);
                            }
                            if let Some(d) = &mut f.default {
                                w.expr(d);
                            }
                        }
                        for ctor in &mut e.constructors {
                            w.params(&mut ctor.params);
                            let params = ctor.params.clone();
                            w.body(&mut ctor.body, &params);
                        }
                        for m in &mut e.methods {
                            w.function(m);
                        }
                        for op in &mut e.operators {
                            w.params(&mut op.params);
                            w.return_type(&mut op.return_type);
                            let params = op.params.clone();
                            if let Some(b) = &mut op.body {
                                w.body(b, &params);
                            }
                        }
                    });
                });
            }
            TopLevelDecl::Interface(i) => {
                let params = i.generic_params.clone();
                self.with_generics(&params, |w| {
                    w.generic_params(&mut i.generic_params);
                    for e in &mut i.extends {
                        w.ty(e);
                    }
                    for f in &mut i.fields {
                        if let Some(t) = &mut f.ty {
                            w.ty(t);
                        }
                        if let Some(d) = &mut f.default {
                            w.expr(d);
                        }
                    }
                    for p in &mut i.properties {
                        w.ty(&mut p.ty);
                    }
                    for m in &mut i.methods {
                        w.function(m);
                    }
                    for op in &mut i.operators {
                        w.params(&mut op.params);
                        w.return_type(&mut op.return_type);
                        let params = op.params.clone();
                        if let Some(b) = &mut op.body {
                            w.body(b, &params);
                        }
                    }
                });
            }
            TopLevelDecl::TypeAlias(a) => {
                let params = a.generic_params.clone();
                self.with_generics(&params, |w| {
                    w.generic_params(&mut a.generic_params);
                    w.ty(&mut a.target);
                });
            }
            TopLevelDecl::Const(c) => {
                if let Some(t) = &mut c.ty {
                    self.ty(t);
                }
                self.expr(&mut c.value);
            }
            TopLevelDecl::Annotation(a) => {
                for p in &mut a.params {
                    self.ty(&mut p.ty);
                    if let Some(d) = &mut p.default {
                        self.expr(d);
                    }
                }
            }
            TopLevelDecl::ExternBlock(b) => {
                for f in &mut b.fns {
                    self.function(f);
                }
            }
        }
    }

    fn function(&mut self, f: &mut FnDecl) {
        let params = f.generic_params.clone();
        self.with_generics(&params, |w| {
            w.generic_params(&mut f.generic_params);
            w.params(&mut f.params);
            w.return_type(&mut f.return_type);
            for t in &mut f.throws {
                w.r.type_name(t, &w.scope);
            }
            for wc in &mut f.wheres {
                for t in &mut wc.param_tys {
                    w.ty(t);
                }
                if let Some(t) = &mut wc.ret {
                    w.ty(t);
                }
            }
            let params = f.params.clone();
            if let Some(b) = &mut f.body {
                w.body(b, &params);
            }
        });
    }

    fn params(&mut self, params: &mut [Param]) {
        for p in params {
            self.ty(&mut p.ty);
            if let Some(d) = &mut p.default {
                self.expr(d);
            }
        }
    }

    fn return_type(&mut self, r: &mut ReturnType) {
        match r {
            ReturnType::Void => {}
            ReturnType::Type(t) | ReturnType::AsyncType(t) => self.ty(t),
        }
    }

    /// A body, with the parameters and every local it declares in scope.
    fn body(&mut self, b: &mut Block, params: &[Param]) {
        let mut names: HashSet<String> = params.iter().map(|p| p.name.text.clone()).collect();
        collect_block_locals(b, &mut names);
        self.with_locals(names, |w| w.block(b));
    }

    fn ty(&mut self, t: &mut TypeRef) {
        self.r.type_ref(t, &self.scope);
        for a in &mut t.generic_args {
            match a {
                GenericArg::Type(inner) => self.ty(inner),
                GenericArg::Wildcard(wa) => match &mut wa.bound {
                    Some(WildcardBound::Extends(b)) | Some(WildcardBound::Super(b)) => self.ty(b),
                    None => {}
                },
            }
        }
        if let Some(shape) = &mut t.fn_shape {
            for p in &mut shape.params {
                self.ty(p);
            }
            self.ty(&mut shape.return_type);
            for e in &mut shape.throws {
                self.ty(e);
            }
        }
        if let Some(shape) = &mut t.array_shape {
            for d in &mut shape.dims {
                if let crate::ArrayDim::Fixed(e) = d {
                    self.expr(e);
                }
            }
        }
    }

    fn block(&mut self, b: &mut Block) {
        for s in &mut b.statements {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Yield(e, _) | Stmt::Throw(e, _) => self.expr(e),
            Stmt::Return(e, _) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            Stmt::VarDecl(v) => {
                if let Some(t) = &mut v.ty {
                    self.ty(t);
                }
                if let Some(i) = &mut v.init {
                    self.expr(i);
                }
            }
            Stmt::If(i) => self.if_stmt(i),
            Stmt::While(wh) => {
                self.expr(&mut wh.condition);
                self.block(&mut wh.body);
            }
            Stmt::DoWhile(d) => {
                self.block(&mut d.body);
                self.expr(&mut d.condition);
            }
            Stmt::ForEach(f) => {
                if let Some(t) = &mut f.var_type {
                    self.ty(t);
                }
                self.expr(&mut f.iter);
                self.block(&mut f.body);
            }
            Stmt::Assign(a) => {
                self.expr(&mut a.target);
                self.expr(&mut a.value);
            }
            Stmt::Break(..) | Stmt::Continue(..) => {}
            Stmt::Labeled { stmt, .. } => self.stmt(stmt),
            Stmt::SuperCall(args, _) => {
                for a in args {
                    self.expr(a);
                }
            }
            Stmt::Try(t) => self.try_stmt(t),
            Stmt::Unsafe(b) | Stmt::Block(b) => self.block(b),
            Stmt::IfCfg(c) => {
                self.block(&mut c.then_block);
                if let Some(b) = &mut c.else_block {
                    self.block(b);
                }
            }
            Stmt::ForC(f) => {
                if let Some(i) = &mut f.init {
                    self.stmt(i);
                }
                if let Some(c) = &mut f.cond {
                    self.expr(c);
                }
                if let Some(u) = &mut f.update {
                    self.stmt(u);
                }
                self.block(&mut f.body);
            }
        }
    }

    fn if_stmt(&mut self, i: &mut crate::IfStmt) {
        self.expr(&mut i.condition);
        self.block(&mut i.then_block);
        if let Some(e) = &mut i.else_branch {
            match &mut **e {
                ElseBranch::If(inner) => self.if_stmt(inner),
                ElseBranch::Block(b) => self.block(b),
            }
        }
    }

    fn try_stmt(&mut self, t: &mut crate::TryStmt) {
        self.block(&mut t.body);
        for c in &mut t.catches {
            self.ty(&mut c.ty);
            for a in &mut c.alt_tys {
                self.ty(a);
            }
            self.block(&mut c.body);
        }
        if let Some(f) = &mut t.finally {
            self.block(f);
        }
    }

    fn pattern(&mut self, p: &mut Pattern) {
        match p {
            Pattern::TypeBind { type_name, .. } => self.r.type_pattern(type_name, &self.scope),
            Pattern::EnumVariant { path, args, .. } => {
                self.r.expr_path(path, &self.scope);
                for a in args {
                    self.pattern(a);
                }
            }
            Pattern::Tuple(parts, _) | Pattern::Or(parts, _) => {
                for part in parts {
                    self.pattern(part);
                }
            }
            Pattern::Wildcard(_) | Pattern::Literal(..) | Pattern::Bind(_) | Pattern::Range { .. } => {}
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Literal(_) | Expr::This(_) | Expr::Super(_) => {}
            Expr::Path(p) => self.r.expr_path(p, &self.scope),
            Expr::Out(inner, _)
            | Expr::TypeOf(inner, _)
            | Expr::Throw(inner, _)
            | Expr::Await(inner, _)
            | Expr::ErrorProp(inner, _)
            | Expr::NotNullAssert(inner, _) => self.expr(inner),
            Expr::Call(c) => {
                for t in &mut c.explicit_generic_args {
                    self.ty(t);
                }
                self.expr(&mut c.callee);
                for a in &mut c.args {
                    self.expr(a);
                }
            }
            Expr::Binary(b) => {
                self.expr(&mut b.left);
                self.expr(&mut b.right);
            }
            Expr::Unary(u) => self.expr(&mut u.operand),
            Expr::Range(r) => {
                self.expr(&mut r.start);
                self.expr(&mut r.end);
                if let Some(s) = &mut r.step {
                    self.expr(s);
                }
            }
            Expr::Cast(c) => {
                self.ty(&mut c.ty);
                self.expr(&mut c.value);
            }
            Expr::SizeOf(s) => {
                if let Some(t) = &mut s.type_operand {
                    self.ty(t);
                }
                self.expr(&mut s.operand);
            }
            Expr::NewArray(n) => {
                self.ty(&mut n.element_type);
                self.expr(&mut n.size);
                for s in &mut n.inner_sizes {
                    self.expr(s);
                }
            }
            Expr::NewArrayLit(n) => {
                self.ty(&mut n.element_type);
                for x in &mut n.elements {
                    self.expr(x);
                }
            }
            Expr::Index(i) => {
                self.expr(&mut i.array);
                self.expr(&mut i.index);
            }
            Expr::Field(f) => self.expr(&mut f.object),
            Expr::InterpString(s) => {
                for seg in &mut s.segments {
                    if let InterpSegment::Expr(x) = seg {
                        self.expr(x);
                    }
                }
            }
            Expr::TypeTest(t) => {
                self.ty(&mut t.ty);
                self.expr(&mut t.value);
            }
            Expr::NewObject(n) => {
                self.r.new_object(n, &self.scope);
                for t in &mut n.generic_args {
                    self.ty(t);
                }
                for a in &mut n.args {
                    self.expr(a);
                }
                if let Some(body) = &mut n.anonymous_body {
                    for b in &mut body.init_blocks {
                        self.body(b, &[]);
                    }
                    for m in &mut body.methods {
                        self.function(m);
                    }
                }
            }
            Expr::Switch(s) => {
                self.expr(&mut s.scrutinee);
                for arm in &mut s.arms {
                    self.pattern(&mut arm.pattern);
                    if let Some(g) = &mut arm.guard {
                        self.expr(g);
                    }
                    match &mut arm.body {
                        SwitchBody::Expr(x) => self.expr(x),
                        SwitchBody::Block(b) => self.block(b),
                    }
                }
            }
            Expr::Lambda(l) => {
                for p in &mut l.params {
                    if let Some(t) = &mut p.ty {
                        self.ty(t);
                    }
                }
                match &mut l.body {
                    LambdaBody::Expr(x) => self.expr(x),
                    LambdaBody::Block(b) => self.block(b),
                }
            }
            Expr::Elvis(el) => {
                self.expr(&mut el.value);
                self.expr(&mut el.fallback);
            }
            Expr::MethodRef(m) => self.r.type_name(&mut m.receiver, &self.scope),
            Expr::Ternary(t) => {
                self.expr(&mut t.condition);
                self.expr(&mut t.then_branch);
                self.expr(&mut t.else_branch);
            }
            Expr::TupleLit(xs, _) => {
                for x in xs {
                    self.expr(x);
                }
            }
            Expr::TryExpr(t) => self.try_stmt(t),
            Expr::IncDec(i) => self.expr(&mut i.target),
        }
    }
}

/// Every name a block declares as a value: locals, loop variables, `catch`
/// parameters, lambda parameters and pattern binders, nested blocks included.
/// Over-approximate on purpose: a name any of them takes is left alone in an
/// expression anywhere in the body.
fn collect_block_locals(b: &Block, out: &mut HashSet<String>) {
    crate::visit::for_each_node(b, &mut |n| match n {
        crate::visit::Node::Stmt(Stmt::VarDecl(v)) => {
            out.insert(v.name.text.clone());
        }
        crate::visit::Node::Stmt(Stmt::ForEach(f)) => {
            out.insert(f.var_name.text.clone());
        }
        crate::visit::Node::Stmt(Stmt::Try(t)) => {
            out.extend(t.catches.iter().map(|c| c.name.text.clone()));
        }
        crate::visit::Node::Expr(Expr::TryExpr(t)) => {
            out.extend(t.catches.iter().map(|c| c.name.text.clone()));
        }
        crate::visit::Node::Expr(Expr::Lambda(l)) => {
            out.extend(l.params.iter().map(|p| p.name.text.clone()));
        }
        crate::visit::Node::Expr(Expr::TypeTest(t)) => {
            out.extend(t.binder.iter().map(|b| b.text.clone()));
        }
        crate::visit::Node::Expr(Expr::Switch(s)) => {
            for arm in &s.arms {
                out.extend(arm.pattern.binders().into_iter().map(|b| b.text.clone()));
            }
        }
        _ => {}
    });
}
