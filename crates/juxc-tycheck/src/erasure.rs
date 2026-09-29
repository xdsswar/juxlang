//! Polymorphic recursion, lowered by erasing type arguments (ERRATA
//! E141).
//!
//! A generic function or method that calls itself at an ever-larger type
//! argument (`grow<A, B>` calling `grow<A, Vec<B>>`, `build<T>` calling
//! `build<Pair<T>>`) instantiates without end in a language that
//! monomorphizes, and the classes it builds from those arguments are built
//! at ever-larger arguments too. Java runs such a program through erasure,
//! and so does Jux, for exactly the functions, methods and classes on that
//! cycle ([`crate::instantiations`] finds them): each of their type
//! parameters is instantiated at one type only, `JuxErased`, a boxed value
//! that carries its own type, and a value is boxed where it enters such a
//! slot and unboxed where it leaves one for a slot of a known type.
//! Everything else keeps its own instantiations.
//!
//! The erased family is closed over the class hierarchy: a class whose
//! instantiations cannot be closed brings its generic ancestors and
//! descendants with it, so one erased value is the same Rust type wherever
//! it is seen. An erased value keeps each bound as a dispatch object, through
//! an adapter when the bound names the parameter (ERRATA E142, E143), and a
//! collection holding the parameter is linked to one storage on both sides
//! (E144). A bound holding the parameter inside a foreign type that
//! is neither a Jux class nor a collection has no conversion, and such a
//! cycle is `E0438`.

use std::collections::BTreeSet;

use crate::symbol_table::SymbolTable;

/// The name the instantiation sets and the backend use for the erased type.
pub const ERASED_TYPE: &str = "__JuxErased";

/// What is erased.
#[derive(Debug, Clone, Default)]
pub struct Erasure {
    /// Class, record and interface FQNs every type parameter of which is
    /// erased.
    pub classes: BTreeSet<String>,
    /// Generic functions and methods, as instantiation keys (`fn:<fqn>`,
    /// `method:<class>.<name>`), every type parameter of which is erased.
    pub fns: BTreeSet<String>,
    /// A cycle that could not be erased: the function, method or class on it
    /// (its key or FQN), the one that keeps growing it, and the bounded type
    /// parameter that stops it (`E0438`).
    pub refused: Vec<(String, String, juxc_ast::TypeParam)>,
}

impl Erasure {
    /// Whether the class, record or interface `fqn` is erased.
    pub fn class(&self, fqn: &str) -> bool {
        self.classes.contains(fqn)
    }

    /// Whether the function `fqn` is erased.
    pub fn function(&self, fqn: &str) -> bool {
        self.fns.contains(&format!("fn:{fqn}"))
    }

    /// Whether the method `name` of the class `class` is erased.
    pub fn method(&self, class: &str, name: &str) -> bool {
        self.fns.contains(&format!("method:{class}.{name}"))
    }
}

fn generic_params_of(symbols: &SymbolTable, fqn: &str) -> Option<Vec<juxc_ast::TypeParam>> {
    symbols
        .classes
        .get(fqn)
        .map(|c| c.generic_params.clone())
        .or_else(|| symbols.records.get(fqn).map(|r| r.generic_params.clone()))
        .or_else(|| symbols.interfaces.get(fqn).map(|i| i.generic_params.clone()))
}

fn key_generic_params(symbols: &SymbolTable, key: &str) -> Vec<juxc_ast::TypeParam> {
    if let Some(f) = key.strip_prefix("fn:") {
        return symbols.functions.get(f).map(|s| s.generic_params.clone()).unwrap_or_default();
    }
    if let Some(m) = key.strip_prefix("method:") {
        if let Some((class, method)) = m.rsplit_once('.') {
            return symbols.lookup_method(class, method).map(|(s, _)| s.generic_params.clone()).unwrap_or_default();
        }
    }
    Vec::new()
}

/// The erased family of the program's polymorphic recursion.
pub fn plan(symbols: &SymbolTable) -> Erasure {
    let closure = &symbols.instantiations;
    let mut classes: BTreeSet<String> = closure.unbounded.keys().cloned().collect();
    let mut fns: BTreeSet<String> = closure.growing.iter().cloned().collect();
    if classes.is_empty() && fns.is_empty() {
        return Erasure::default();
    }
    // The class hierarchy around each erased class: generic ancestors and
    // descendants (a `Nested<T>` seen as its `Nest<T>` is one erased type),
    // and the classes an erased parameter's bound names at its parameters
    // (`Rel<Pair<T, T>>`: a `Pair<T, T>` is one erased type too). An
    // interface that is a bound keeps its own instantiations: the erased
    // value reaches it through an adapter (ERRATA E143).
    loop {
        let bound_heads = bound_heads(symbols, &classes, &fns);
        let mut grew = false;
        for c in bound_arg_classes(symbols, &classes, &fns) {
            if classes.insert(c) {
                grew = true;
            }
        }
        let current: Vec<String> = classes.iter().cloned().collect();
        for c in &current {
            if let Some(sig) = symbols.classes.get(c) {
                let supers = sig.extends_fqn.iter().cloned().chain(sig.implements.iter().filter_map(|t| {
                    let written = t.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                    symbols
                        .interfaces
                        .keys()
                        .find(|k| **k == written || k.rsplit('.').next() == Some(written.as_str()))
                        .cloned()
                }));
                for s in supers {
                    if bound_heads.contains(&s) {
                        continue;
                    }
                    if generic_params_of(symbols, &s).is_some_and(|p| !p.is_empty()) && classes.insert(s) {
                        grew = true;
                    }
                }
            }
            for (sub, sig) in &symbols.classes {
                if sig.extends_fqn.as_deref() == Some(c.as_str())
                    && !sig.generic_params.is_empty()
                    && classes.insert(sub.clone())
                {
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    // A bound is carried along with the value (ERRATA E142): the
    // boxing site, which knows the value's type, keeps it as the bound's
    // dispatch object too. A bound that is not a fixed type (one naming a
    // type parameter, `T extends Comparable<T>`) is kept through an adapter
    // (E143), and a collection of the parameter is linked (E144); a
    // foreign type holding it that is neither is `E0438`.
    let grower = fns.iter().next().cloned().or_else(|| closure.unbounded.values().next().cloned()).unwrap_or_default();
    let mut refused: Vec<(String, String, juxc_ast::TypeParam)> = Vec::new();
    for c in &classes {
        let params = generic_params_of(symbols, c).unwrap_or_default();
        for p in &params {
            if !erasable(p, &params, symbols) {
                refused.push((c.clone(), grower.clone(), p.clone()));
            }
        }
    }
    for k in &fns {
        let mut params = key_generic_params(symbols, k);
        if let Some(m) = k.strip_prefix("method:") {
            if let Some((class, _)) = m.rsplit_once('.') {
                params.extend(generic_params_of(symbols, class).unwrap_or_default());
            }
        }
        for p in key_generic_params(symbols, k) {
            if !erasable(&p, &params, symbols) {
                refused.push((k.clone(), grower.clone(), p));
            }
        }
    }
    if !refused.is_empty() {
        return Erasure { refused, ..Erasure::default() };
    }
    // External types keep their own representation.
    classes.retain(|c| {
        !symbols.classes.get(c).is_some_and(|s| s.is_external)
            && !symbols.interfaces.get(c).is_some_and(|i| i.is_external)
    });
    fns.retain(|k| !key_generic_params(symbols, k).is_empty());
    Erasure { classes, fns, refused: Vec::new() }
}

/// Whether `t` names one of `params`, anywhere in it.
fn names_param(t: &juxc_ast::TypeRef, params: &[juxc_ast::TypeParam]) -> bool {
    (t.name.segments.len() == 1 && params.iter().any(|q| q.name.text == t.name.segments[0].text))
        || t.generic_args.iter().filter_map(|a| a.as_type()).any(|x| names_param(x, params))
}

fn user_interface(symbols: &SymbolTable, bare: &str) -> Option<String> {
    symbols
        .interfaces
        .iter()
        .find(|(k, i)| !i.is_external && k.rsplit('.').next() == Some(bare))
        .map(|(k, _)| k.clone())
}

fn user_class(symbols: &SymbolTable, bare: &str) -> Option<String> {
    symbols
        .classes
        .iter()
        .find(|(k, c)| !c.is_external && k.rsplit('.').next() == Some(bare))
        .map(|(k, _)| k.clone())
        .or_else(|| symbols.records.keys().find(|k| k.rsplit('.').next() == Some(bare)).cloned())
}

/// Every type parameter of the family, with the declaration's parameters.
fn family_params(
    symbols: &SymbolTable,
    classes: &BTreeSet<String>,
    fns: &BTreeSet<String>,
) -> Vec<(juxc_ast::TypeParam, Vec<juxc_ast::TypeParam>)> {
    let mut out = Vec::new();
    for c in classes {
        let ps = generic_params_of(symbols, c).unwrap_or_default();
        for p in &ps {
            out.push((p.clone(), ps.clone()));
        }
    }
    for k in fns {
        let own = key_generic_params(symbols, k);
        let mut all = own.clone();
        if let Some((class, _)) = k.strip_prefix("method:").and_then(|m| m.rsplit_once('.')) {
            all.extend(generic_params_of(symbols, class).unwrap_or_default());
        }
        for p in own {
            out.push((p, all.clone()));
        }
    }
    out
}

/// The interfaces the family's parameters are bounded by at their own
/// parameters (`T extends Ranked<T>`).
fn bound_heads(symbols: &SymbolTable, classes: &BTreeSet<String>, fns: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (p, params) in family_params(symbols, classes, fns) {
        for b in &p.bounds {
            if names_param(b, &params) {
                if let Some(i) = b
                    .name
                    .segments
                    .last()
                    .and_then(|s| user_interface(symbols, &s.text).or_else(|| user_class(symbols, &s.text)))
                {
                    out.insert(i);
                }
            }
        }
    }
    out
}

/// The generic classes a bound of the family names at the family's
/// parameters (`Pair` in `T extends Rel<Pair<T, T>>`).
fn bound_arg_classes(symbols: &SymbolTable, classes: &BTreeSet<String>, fns: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for (p, params) in family_params(symbols, classes, fns) {
        for b in &p.bounds {
            for a in b.generic_args.iter().filter_map(|a| a.as_type()) {
                collect_classes_naming(a, &params, symbols, &mut out);
            }
        }
    }
    out
}

fn collect_classes_naming(
    t: &juxc_ast::TypeRef,
    params: &[juxc_ast::TypeParam],
    symbols: &SymbolTable,
    out: &mut Vec<String>,
) {
    if !t.generic_args.is_empty() && names_param(t, params) {
        if let Some(c) = t.name.segments.last().and_then(|s| user_class(symbols, &s.text)) {
            out.push(c);
        }
    }
    for a in t.generic_args.iter().filter_map(|a| a.as_type()) {
        collect_classes_naming(a, params, symbols, out);
    }
}

/// Whether a type parameter can be erased. A `const` parameter is kept as
/// it is: it never grows, so each value it takes is one instantiation. A
/// bound is kept as a dispatch object when it is a fixed Jux type, and
/// through an adapter when it is a Jux interface at the declaration's
/// parameters (`T extends Ranked<T>`), each argument a parameter, a type
/// with none, a Jux class or record (erased with the family), or a
/// collection holding the parameter (linked, E144); a bound that is
/// another parameter is the identity (ERRATA E143).
fn erasable(p: &juxc_ast::TypeParam, params: &[juxc_ast::TypeParam], symbols: &SymbolTable) -> bool {
    if p.is_const() {
        return true;
    }
    p.bounds.iter().all(|b| {
        let bare = b.name.segments.last().map(|s| s.text.as_str()).unwrap_or("");
        if b.generic_args.is_empty() && b.name.segments.len() == 1 && params.iter().any(|q| q.name.text == bare) {
            return true;
        }
        if !names_param(b, params) {
            return user_interface(symbols, bare).is_some() || user_class(symbols, bare).is_some();
        }
        (user_interface(symbols, bare).is_some() || user_class(symbols, bare).is_some())
            && b.generic_args.iter().filter_map(|a| a.as_type()).all(|a| {
                !names_param(a, params)
                    || (a.generic_args.is_empty() && a.name.segments.len() == 1 && a.array_shape.is_none())
                    || a.name.segments.last().is_some_and(|s| user_class(symbols, &s.text).is_some())
                    || linked(a, params, symbols)
            })
    })
}

/// Whether `t` holds a type parameter inside a collection (`Vec<T>`,
/// `HashMap<String, Vec<T>>`, `T[]`): the erased code and the value see two
/// Rust types for it, and the adapter links the two so they share one
/// storage (ERRATA E144). Every argument of the collection is a
/// parameter, a type with none, a Jux class (erased with the family), or a
/// collection of the same kind.
pub fn linked(t: &juxc_ast::TypeRef, params: &[juxc_ast::TypeParam], symbols: &SymbolTable) -> bool {
    if !names_param(t, params) || t.nullable || t.fn_shape.is_some() || t.ptr_depth > 0 {
        return false;
    }
    let arg_ok = |a: &juxc_ast::TypeRef| {
        !names_param(a, params)
            || (a.generic_args.is_empty() && a.name.segments.len() == 1 && a.array_shape.is_none() && !a.nullable)
            || a.name.segments.last().is_some_and(|s| user_class(symbols, &s.text).is_some())
            || linked(a, params, symbols)
    };
    if let Some(shape) = &t.array_shape {
        let dynamic = shape.dims.iter().all(|d| matches!(d, juxc_ast::ArrayDim::Dynamic));
        let mut element = t.clone();
        element.array_shape = None;
        return dynamic && shape.dims.len() == 1 && !shape.elem_nullable && arg_ok(&element);
    }
    let name = t.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
    let is_collection = symbols.is_rust_collection(&name)
        || t.name.segments.last().is_some_and(|s| symbols.is_rust_collection(&s.text));
    is_collection
        && (1..=2).contains(&t.generic_args.len())
        && t.generic_args.iter().all(|a| a.as_type().is_some_and(arg_ok))
}

/// `E0438` for each cycle [`plan`] could not erase.
pub fn refused_diagnostics(symbols: &SymbolTable) -> Vec<(usize, juxc_diagnostics::Diagnostic)> {
    let mut out = Vec::new();
    for (entity, grower, param) in &symbols.erasure.refused {
        let key = entity.strip_prefix("fn:").or_else(|| entity.strip_prefix("method:")).unwrap_or(entity);
        let owner = key.rsplit_once('.').map(|(c, _)| c).unwrap_or(key);
        let unit = symbols
            .decl_unit
            .get(key)
            .or_else(|| symbols.decl_unit.get(owner))
            .copied()
            .unwrap_or(0);
        let name = key.rsplit('.').next().unwrap_or(key);
        let grower = grower.rsplit(['.', ':']).next().unwrap_or(grower);
        let bound = param
            .bounds
            .iter()
            .map(crate::symbol_table::render_type_ref)
            .collect::<Vec<_>>()
            .join(" & ");
        let what = format!(
            "bounded by `{bound}`, which holds the parameter inside a foreign type that is neither a Jux \
             class nor a collection, and an erased value has no conversion into such a type"
        );
        out.push((
            unit,
            juxc_diagnostics::Diagnostic::error(
                juxc_diagnostics::code::Code::E0438_GenericVirtualMethod,
                format!(
                    "`{grower}` calls itself at an ever-larger type argument (polymorphic recursion), which is \
                     compiled by erasing the type arguments of every function and class on that cycle -- but \
                     `{}` of `{name}` is {what}",
                    param.name.text,
                ),
            )
            .with_span(param.span)
            .with_help(format!(
                "bound `{}` by the parameter itself or a Jux class holding it, or keep `{grower}` from calling itself at a larger type argument",
                param.name.text
            )),
        ));
    }
    out
}

/// Every instantiation of an erased class is the one erased instantiation.
pub fn normalize_instantiations(symbols: &mut SymbolTable) {
    let erased: Vec<String> = symbols.erasure.classes.iter().cloned().collect();
    for c in erased {
        let n = generic_params_of(symbols, &c).map(|p| p.iter().filter(|x| !x.is_const()).count()).unwrap_or(0);
        if n == 0 {
            continue;
        }
        let marker = crate::ty::Ty::User { name: ERASED_TYPE.to_string(), generic_args: Vec::new() };
        symbols.instantiations.classes.insert(c.clone(), vec![vec![marker; n]]);
        symbols.instantiations.unbounded.remove(&c);
    }
}
