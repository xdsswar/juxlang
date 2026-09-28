//! A nullable slot for a field whose initializer runs against the finished
//! object and whose type has no stand-in value (ERRATA E139).
//!
//! An initializer that uses the object runs after the object is built
//! ([`crate::field_init`]), so the field needs *some* value in between. A
//! number, a string, a collection, an optional or a one-method interface has
//! one; a class, an interface with several methods or a type parameter does
//! not, and `Worker w = new Worker(this);` is exactly that case. Java's
//! answer is `null` until the initializer runs, and so is this pass's: the
//! driver gives such a field a nullable slot (`Worker?`), and each read of it
//! anywhere in the program checks it is set, which it always is once
//! construction is over. A read during construction that comes before the
//! initializer (a method an ancestor's constructor calls that reads a
//! subclass field, ERRATA E140) throws `NullPointerException` (E1XX-GAP39g)
//! ("field 'w' of Owner read before it was initialized"), which the program
//! can catch, rather than reading a value that was never written. The program is then checked again, so the backend sees
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
        self.hit(e).is_some()
    }

    /// The late field `e` reads, as `(declaring class FQN, field)`.
    pub(crate) fn hit(&self, e: &Expr) -> Option<(String, String)> {
        match e {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                self.bare.get(&qn.span).filter(|key| self.late.contains(*key)).cloned()
            }
            Expr::Field(f) => {
                let ty = self.expr_types.get(&crate::check::expr_span_pub(&f.object))?;
                let ty = match ty {
                    Ty::Nullable(inner) => inner.as_ref(),
                    other => other,
                };
                let Ty::User { name, .. } = ty else { return None };
                let (sig, decl) = self.symbols.lookup_field(name, &f.field.text)?;
                let key = (decl.to_string(), f.field.text.clone());
                (!sig.is_static && self.late.contains(&key)).then_some(key)
            }
            _ => None,
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
    let mut late = HashSet::new();
    let names: Vec<String> = classes.keys().cloned().collect();
    let decl_of = |f: &str| classes.get(f).copied();
    let parent_of = |f: &str| symbols.classes.get(f).and_then(|c| c.extends_fqn.clone());
    for fqn in names {
        let deferred = crate::field_init::deferred_of(&fqn, &decl_of, &parent_of, symbols);
        let observed = crate::field_init::ancestor_facts_of(&fqn, &decl_of, &parent_of, symbols, 0).observes;
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
            // An ancestor's construction that reaches the object may read the
            // field before this class's constructor body assigns it (ERRATA
            // E140): it has to find the field unset, not a value.
            if stores_object || observed {
                late.insert((fqn.clone(), f.name.text.clone()));
            }
        }
    }
    late
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
