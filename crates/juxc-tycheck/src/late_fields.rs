//! A nullable slot for a field whose initializer runs against the finished
//! object and whose type has no stand-in value (ERRATA E1XX-GAP39e).
//!
//! An initializer that uses the object runs after the object is built
//! ([`crate::field_init`]), so the field needs *some* value in between. A
//! number, a string, a collection, an optional or a one-method interface has
//! one; a class, an interface with several methods or a type parameter does
//! not, and `Worker w = new Worker(this);` is exactly that case. Java's
//! answer is `null` until the initializer runs, and so is this pass's: the
//! driver gives such a field a nullable slot (`Worker?`), and each read of it
//! anywhere in the program asserts it is set (`w!!`), which it always is once
//! construction is over. A read during construction that comes before the
//! initializer (a method the constructor calls that reads a later field)
//! stops with the same exception an unset `!!` raises, not with a value that
//! was never written. The program is then checked again, so the backend sees
//! a nullable field and non-null reads, both of which it already lowers.

use std::collections::{HashMap, HashSet};

use juxc_ast::{ClassDecl, CompilationUnit, Expr, TopLevelDecl, TypeRef};
use juxc_source::Span;

use crate::{SymbolTable, Ty, TypeCheckResult};

/// The reads [`apply`] rewrites.
pub(crate) struct LateReads<'a> {
    /// `(declaring class FQN, field)` of every field given a nullable slot.
    pub(crate) late: HashSet<(String, String)>,
    pub(crate) symbols: &'a SymbolTable,
    pub(crate) expr_types: &'a HashMap<Span, Ty>,
    pub(crate) bare: &'a HashMap<Span, (String, String)>,
}

impl LateReads<'_> {
    /// Whether `e` reads one of the late fields: a bare name the checker read
    /// as one, or `obj.f` on an object whose class has it.
    pub(crate) fn hits(&self, e: &Expr) -> bool {
        match e {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                self.bare.get(&qn.span).is_some_and(|key| self.late.contains(key))
            }
            Expr::Field(f) => {
                let Some(ty) = self.expr_types.get(&crate::check::expr_span_pub(&f.object)) else {
                    return false;
                };
                let ty = match ty {
                    Ty::Nullable(inner) => inner.as_ref(),
                    other => other,
                };
                let Ty::User { name, .. } = ty else { return false };
                self.symbols
                    .lookup_field(name, &f.field.text)
                    .is_some_and(|(sig, decl)| !sig.is_static && self.late.contains(&(decl.to_string(), f.field.text.clone())))
            }
            _ => false,
        }
    }
}

/// Give each late field a nullable slot and assert every read of it. Returns
/// whether anything changed (the caller then checks the program again).
pub fn apply(units: &mut [CompilationUnit], typed: &TypeCheckResult) -> bool {
    let late = plan(units, &typed.symbols);
    if late.is_empty() {
        return false;
    }
    for unit in units.iter_mut() {
        if unit.is_external {
            continue;
        }
        let pkg = unit_package(unit);
        for item in &mut unit.items {
            if let TopLevelDecl::Class(cd) = item {
                let fqn = if pkg.is_empty() { cd.name.text.clone() } else { format!("{pkg}.{}", cd.name.text) };
                mark_nullable(cd, &fqn, &late);
            }
        }
    }
    let reads = LateReads { late, symbols: &typed.symbols, expr_types: &typed.expr_types, bare: &typed.bare_field_refs };
    crate::expand::rewrite_late_reads(units, &reads);
    true
}

fn mark_nullable(cd: &mut ClassDecl, fqn: &str, late: &HashSet<(String, String)>) {
    for f in &mut cd.fields {
        if late.contains(&(fqn.to_string(), f.name.text.clone())) {
            if let Some(ty) = &mut f.ty {
                ty.nullable = true;
            }
        }
    }
    for nested in &mut cd.nested_types {
        if let TopLevelDecl::Class(inner) = nested {
            mark_nullable(inner, &format!("{fqn}__{}", inner.name.text), late);
        }
    }
}

fn unit_package(unit: &CompilationUnit) -> String {
    unit.package
        .as_ref()
        .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
        .unwrap_or_default()
}

/// The late fields: deferred ([`crate::field_init`]) and of a type with no
/// stand-in value.
fn plan(units: &[CompilationUnit], symbols: &SymbolTable) -> HashSet<(String, String)> {
    let mut classes: HashMap<String, &ClassDecl> = HashMap::new();
    fn collect<'u>(cd: &'u ClassDecl, fqn: String, out: &mut HashMap<String, &'u ClassDecl>) {
        for nested in &cd.nested_types {
            if let TopLevelDecl::Class(inner) = nested {
                collect(inner, format!("{fqn}__{}", inner.name.text), out);
            }
        }
        out.insert(fqn, cd);
    }
    for unit in units.iter().filter(|u| !u.is_external) {
        let pkg = unit_package(unit);
        for item in &unit.items {
            if let TopLevelDecl::Class(cd) = item {
                let fqn = if pkg.is_empty() { cd.name.text.clone() } else { format!("{pkg}.{}", cd.name.text) };
                collect(cd, fqn, &mut classes);
            }
        }
    }
    let mut memo: HashMap<String, Vec<usize>> = HashMap::new();
    let mut late = HashSet::new();
    let names: Vec<String> = classes.keys().cloned().collect();
    for fqn in names {
        let deferred = deferred_of(&fqn, &classes, symbols, &mut memo, 0);
        let cd = classes[&fqn];
        for i in deferred {
            let f = &cd.fields[i];
            let Some(ty) = &f.ty else { continue };
            if !has_stand_in(ty, cd, symbols) {
                late.insert((fqn.clone(), f.name.text.clone()));
            }
        }
        // The same holds for a field only a constructor or an `init` block
        // assigns, from a value that uses the object (`this.w = new
        // Worker(this);`): that store runs against the handle, which the
        // field is part of.
        for f in cd.fields.iter().filter(|f| !f.is_static && f.default.is_none()) {
            let Some(ty) = &f.ty else { continue };
            if has_stand_in(ty, cd, symbols) {
                continue;
            }
            let is_member = |n: &str| {
                cd.fields.iter().any(|g| !g.is_static && g.name.text == n)
                    || cd.methods.iter().any(|m| m.name.text == n && !m.modifiers.contains(&juxc_ast::FnModifier::Static))
                    || symbols.lookup_field(&fqn, n).is_some_and(|(g, _)| !g.is_static)
                    || symbols.lookup_method(&fqn, n).is_some_and(|(m, _)| !m.is_static)
            };
            let mut stores_object = false;
            let mut scan = |block: &juxc_ast::Block, params: &[String]| {
                juxc_ast::visit::for_each_node(block, &mut |node| {
                    let juxc_ast::visit::Node::Stmt(juxc_ast::Stmt::Assign(a)) = node else { return };
                    let targets_field = match &a.target {
                        Expr::Field(fe) => matches!(fe.object.as_ref(), Expr::This(_)) && fe.field.text == f.name.text,
                        Expr::Path(qn) => {
                            qn.segments.len() == 1 && qn.segments[0].text == f.name.text && !params.contains(&f.name.text)
                        }
                        _ => false,
                    };
                    if targets_field
                        && crate::field_init::uses_object(&a.value, &|n| !params.iter().any(|p| p == n) && is_member(n))
                    {
                        stores_object = true;
                    }
                });
            };
            for ctor in &cd.constructors {
                let params: Vec<String> = ctor.params.iter().map(|p| p.name.text.clone()).collect();
                scan(&ctor.body, &params);
            }
            for b in &cd.init_blocks {
                scan(b, &[]);
            }
            if stores_object {
                late.insert((fqn.clone(), f.name.text.clone()));
            }
        }
    }
    late
}

fn deferred_of(
    fqn: &str,
    classes: &HashMap<String, &ClassDecl>,
    symbols: &SymbolTable,
    memo: &mut HashMap<String, Vec<usize>>,
    depth: usize,
) -> Vec<usize> {
    if let Some(v) = memo.get(fqn) {
        return v.clone();
    }
    let Some(cd) = classes.get(fqn).copied() else { return Vec::new() };
    let ancestor = depth < 64
        && symbols
            .classes
            .get(fqn)
            .and_then(|c| c.extends_fqn.clone())
            .is_some_and(|p| !deferred_of(&p, classes, symbols, memo, depth + 1).is_empty() || ancestor_defers(&p, classes, symbols, memo, depth + 1));
    let is_member = |n: &str| {
        cd.fields.iter().any(|f| !f.is_static && f.name.text == n)
            || cd.properties.iter().any(|p| p.name.text == n)
            || cd.methods.iter().any(|m| m.name.text == n && !m.modifiers.contains(&juxc_ast::FnModifier::Static))
            || symbols.lookup_field(fqn, n).is_some_and(|(f, _)| !f.is_static)
            || symbols.lookup_property(fqn, n).is_some_and(|(p, _)| !p.is_static)
            || symbols.lookup_method(fqn, n).is_some_and(|(m, _)| !m.is_static)
    };
    let v = crate::field_init::deferred_indices(&cd.fields, ancestor, &is_member);
    memo.insert(fqn.to_string(), v.clone());
    v
}

fn ancestor_defers(
    fqn: &str,
    classes: &HashMap<String, &ClassDecl>,
    symbols: &SymbolTable,
    memo: &mut HashMap<String, Vec<usize>>,
    depth: usize,
) -> bool {
    depth < 64
        && symbols.classes.get(fqn).and_then(|c| c.extends_fqn.clone()).is_some_and(|p| {
            !deferred_of(&p, classes, symbols, memo, depth + 1).is_empty()
                || ancestor_defers(&p, classes, symbols, memo, depth + 1)
        })
}

/// Whether a slot of type `ty` has a value to hold before its initializer
/// runs: the backend's stand-in (a default, `null`, an empty collection, a
/// one-method interface that throws). A class, an interface with several
/// methods and a type parameter have none.
pub fn has_stand_in(ty: &TypeRef, owner: &ClassDecl, symbols: &SymbolTable) -> bool {
    if ty.nullable || ty.array_shape.is_some() || ty.fn_shape.is_some() || ty.ptr_depth > 0 {
        return true;
    }
    let Some(last) = ty.name.segments.last() else { return true };
    let bare = last.text.as_str();
    if ty.name.segments.len() == 1 && owner.generic_params.iter().any(|g| g.name.text == bare) {
        return false;
    }
    let written = ty.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
    let matches = |key: &str| {
        key == written || key.rsplit('.').next().is_some_and(|k| k == bare || k.rsplit("__").next() == Some(bare))
    };
    if let Some((_, iface)) = symbols.interfaces.iter().find(|(k, _)| matches(k)) {
        if iface.is_external {
            return true;
        }
        let abstract_methods = iface.methods.values().filter(|m| m.is_abstract && !m.is_static && !m.is_property).count();
        return abstract_methods == 1;
    }
    if let Some((_, class)) = symbols.classes.iter().find(|(k, _)| matches(k)) {
        return class.is_external || class.is_struct;
    }
    true
}
