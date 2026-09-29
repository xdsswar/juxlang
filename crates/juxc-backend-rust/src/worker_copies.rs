//! `E0702` for a WRITE to a collection copied out of a worker-shared object
//! (ERRATA E1XX-GAP40b).
//!
//! A class whose instances cross a worker boundary stores its collections
//! inline, under the object's lock, so reading one out of the object hands
//! back a copy (JUX-LANG-V1 §6.5.1, the one place a collection does not
//! alias; `sync_boundary.rs` converts). A copy that is only read cannot be
//! told from the original. One that is written drops the write without a
//! word, which is the surprise §6.5.1 exists to rule out, so the write is
//! refused, with the two ways to say what was meant: write through the
//! object, or ask for the copy with `clone()`.
//!
//! What counts as a copy: a field of a worker-shared object read from
//! outside its class (`var xs = reg.items;`); the result of a method of the
//! class that hands out one of its own collection fields (`reg.getLog()`);
//! and, inside the class, a collection field bound to a local (`var mine =
//! this.log;`). A write is a mutating method on the copy or a store into one
//! of its elements, through the local it was bound to or directly on the
//! call that produced it. A mutating call on the field itself through the
//! object (`reg.items.push(x)`) is in place, and not a copy.

use std::collections::{HashMap, HashSet};

use juxc_ast::visit::{for_each_node, Node};
use juxc_ast::{Block, ClassDecl, CompilationUnit, Expr, ReturnType, Stmt, TopLevelDecl};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;
use juxc_tycheck::{SymbolTable, Ty};

/// `E0702` for every write to a copy of a worker-shared object's collection,
/// with the index of the unit it is in.
pub fn worker_copy_diagnostics(
    units: &[CompilationUnit],
    symbols: &SymbolTable,
    expr_types: &HashMap<Span, Ty>,
) -> Vec<(usize, Diagnostic)> {
    let sync = crate::worker::compute_worker_shared_class_fqns(units, expr_types, symbols);
    if sync.is_empty() {
        return Vec::new();
    }
    let mut pass = Pass { symbols, expr_types, sync: &sync, getters: HashSet::new(), out: Vec::new(), unit: 0 };
    // The methods of a worker-shared class that hand out one of its own
    // collection fields.
    for unit in units {
        let pkg = package_of(unit);
        for item in &unit.items {
            if let TopLevelDecl::Class(c) = item {
                let fqn = if pkg.is_empty() { c.name.text.clone() } else { format!("{pkg}.{}", c.name.text) };
                if sync.contains(&fqn) {
                    pass.collect_getters(&fqn, c);
                }
            }
        }
    }
    for (idx, unit) in units.iter().enumerate() {
        pass.unit = idx;
        let pkg = package_of(unit);
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(f) => {
                    if let Some(b) = &f.body {
                        pass.body(b, None);
                    }
                }
                TopLevelDecl::Class(c) => {
                    let fqn = if pkg.is_empty() { c.name.text.clone() } else { format!("{pkg}.{}", c.name.text) };
                    let own = sync.contains(&fqn).then_some(fqn);
                    for m in &c.methods {
                        if let Some(b) = &m.body {
                            pass.body(b, own.as_deref().map(|f| (f, c)));
                        }
                    }
                    for ctor in &c.constructors {
                        pass.body(&ctor.body, own.as_deref().map(|f| (f, c)));
                    }
                }
                _ => {}
            }
        }
    }
    pass.out
}

fn package_of(unit: &CompilationUnit) -> String {
    unit.package
        .as_ref()
        .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
        .unwrap_or_default()
}

struct Pass<'a> {
    symbols: &'a SymbolTable,
    expr_types: &'a HashMap<Span, Ty>,
    sync: &'a HashSet<String>,
    /// `(class FQN, method)` pairs that hand out a collection field.
    getters: HashSet<(String, String)>,
    out: Vec<(usize, Diagnostic)>,
    unit: usize,
}

impl Pass<'_> {
    fn is_collection(&self, t: Option<&Ty>) -> bool {
        matches!(t, Some(Ty::User { name, .. }) if self.symbols.is_rust_collection(name))
    }

    fn class_fqn(&self, name: &str) -> Option<String> {
        if self.symbols.classes.contains_key(name) {
            return Some(name.to_string());
        }
        self.symbols.resolve_class(name).map(|(k, _)| k.clone())
    }

    fn collection_field(c: &ClassDecl, name: &str, symbols: &SymbolTable) -> bool {
        c.fields.iter().any(|f| {
            f.name.text == name
                && f.ty.as_ref().is_some_and(|t| {
                    let n = t.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                    symbols.is_rust_collection(&n)
                        || t.name.segments.last().is_some_and(|s| symbols.is_rust_collection(&s.text))
                })
        })
    }

    fn collect_getters(&mut self, fqn: &str, c: &ClassDecl) {
        for m in &c.methods {
            let ReturnType::Type(_) = &m.return_type else { continue };
            let Some(body) = &m.body else { continue };
            let mut hands_out = false;
            for_each_node(body, &mut |n| {
                if let Node::Stmt(Stmt::Return(Some(e), _)) = n {
                    let field = match e {
                        Expr::Field(f) if matches!(&*f.object, Expr::This(_)) => Some(f.field.text.as_str()),
                        Expr::Path(qn) if qn.segments.len() == 1 => Some(qn.segments[0].text.as_str()),
                        _ => None,
                    };
                    if field.is_some_and(|f| Self::collection_field(c, f, self.symbols)) {
                        hands_out = true;
                    }
                }
            });
            if hands_out {
                self.getters.insert((fqn.to_string(), m.name.text.clone()));
            }
        }
    }

    /// The class FQN of a receiver expression's type, when it is
    /// worker-shared.
    fn sync_receiver(&self, recv: &Expr) -> Option<String> {
        let ty = self.expr_types.get(&crate::exprs::expr_span_of(recv))?;
        let ty = match ty {
            Ty::Nullable(inner) => inner.as_ref(),
            other => other,
        };
        let Ty::User { name, .. } = ty else { return None };
        self.class_fqn(name).filter(|f| self.sync.contains(f))
    }

    /// What `e` copies out of a worker-shared object, described, when it
    /// does. `own` is the worker-shared class whose code this is, if any.
    fn copy_source(&self, e: &Expr, own: Option<(&str, &ClassDecl)>) -> Option<String> {
        match e {
            Expr::Call(c) => {
                let Expr::Field(f) = c.callee.as_ref() else { return None };
                if matches!(&*f.object, Expr::This(_)) {
                    let (fqn, _) = own?;
                    return self
                        .getters
                        .contains(&(fqn.to_string(), f.field.text.clone()))
                        .then(|| format!("the `{}()` of a worker-shared `{}`", f.field.text, bare(fqn)));
                }
                let fqn = self.sync_receiver(&f.object)?;
                self.getters
                    .contains(&(fqn.clone(), f.field.text.clone()))
                    .then(|| format!("what `{}()` hands out of a worker-shared `{}`", f.field.text, bare(&fqn)))
            }
            Expr::Field(f) => {
                if matches!(&*f.object, Expr::This(_)) {
                    let (fqn, c) = own?;
                    return Self::collection_field(c, &f.field.text, self.symbols)
                        .then(|| format!("the field `{}` of a worker-shared `{}`", f.field.text, bare(fqn)));
                }
                let fqn = self.sync_receiver(&f.object)?;
                self.is_collection(self.expr_types.get(&f.span))
                    .then(|| format!("the field `{}` of a worker-shared `{}`", f.field.text, bare(&fqn)))
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let (fqn, c) = own?;
                Self::collection_field(c, &qn.segments[0].text, self.symbols)
                    .then(|| format!("the field `{}` of a worker-shared `{}`", qn.segments[0].text, bare(fqn)))
            }
            _ => None,
        }
    }

    /// Whether `method` writes the collection it is called on.
    fn mutates(&self, recv_ty: Option<&Ty>, method: &str) -> bool {
        let Some(Ty::User { name, .. }) = recv_ty else { return false };
        let Some((_, sig)) = self.symbols.resolve_class(name) else { return false };
        sig.methods.get(method).is_some_and(|m| {
            m.annotations
                .iter()
                .any(|a| a.name.segments.len() == 1 && a.name.segments[0].text.eq_ignore_ascii_case("mutself"))
        })
    }

    fn body(&mut self, body: &Block, own: Option<(&str, &ClassDecl)>) {
        // The locals bound to a copy, with what they copy. A local declared
        // with a written type shadows nothing here: the names are the body's.
        let mut copies: HashMap<String, String> = HashMap::new();
        for_each_node(body, &mut |n| match n {
            Node::Stmt(Stmt::VarDecl(v)) => {
                if let Some(init) = &v.init {
                    // A field read out of the object's own code is a copy only
                    // when it is bound to a name, which is what this is.
                    if let Some(what) = self.copy_source(init, own) {
                        copies.insert(v.name.text.clone(), what);
                    }
                }
            }
            Node::Stmt(Stmt::Assign(a)) if a.op.is_none() => {
                if let (Expr::Path(qn), Some(what)) = (&a.target, self.copy_source(&a.value, own)) {
                    if qn.segments.len() == 1 {
                        copies.insert(qn.segments[0].text.clone(), what);
                    }
                }
            }
            _ => {}
        });
        let mut hits: Vec<(Span, String, String)> = Vec::new();
        for_each_node(body, &mut |n| match n {
            Node::Expr(Expr::Call(c)) => {
                let Expr::Field(f) = c.callee.as_ref() else { return };
                let recv_ty = self.expr_types.get(&crate::exprs::expr_span_of(&f.object));
                if !self.mutates(recv_ty, &f.field.text) {
                    return;
                }
                match f.object.as_ref() {
                    Expr::Path(qn) if qn.segments.len() == 1 => {
                        if let Some(what) = copies.get(&qn.segments[0].text) {
                            hits.push((c.span, qn.segments[0].text.clone(), what.clone()));
                        }
                    }
                    // Directly on a getter's result: the copy has no name.
                    inner @ Expr::Call(_) => {
                        if let Some(what) = self.copy_source(inner, own) {
                            hits.push((c.span, String::new(), what));
                        }
                    }
                    _ => {}
                }
            }
            Node::Stmt(Stmt::Assign(a)) => {
                if let Expr::Index(ix) = &a.target {
                    if let Expr::Path(qn) = ix.array.as_ref() {
                        if let Some(what) = copies.get(&qn.segments[0].text).filter(|_| qn.segments.len() == 1) {
                            hits.push((a.span, qn.segments[0].text.clone(), what.clone()));
                        }
                    }
                }
            }
            _ => {}
        });
        for (span, name, what) in hits {
            let subject = if name.is_empty() { "this collection".to_string() } else { format!("`{name}`") };
            self.out.push((
                self.unit,
                Diagnostic::error(
                    code::Code::E0702_ObjectCapturedBySpawn,
                    format!(
                        "{subject} is written, but it is a COPY of {what}: an object shared with workers keeps \
                         its collections under its own lock, and one read out of it is copied (§6.5.1), so the \
                         write would never reach the object"
                    ),
                )
                .with_span(span)
                .with_help(
                    "write through the object instead (give the class a method that makes the change), or say \
                     that a separate list is meant with `.clone()`",
                ),
            ));
        }
    }
}

fn bare(fqn: &str) -> &str {
    fqn.rsplit('.').next().unwrap_or(fqn)
}
