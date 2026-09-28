//! The instantiations a whole program builds (ERRATA E1XX-GAP39c).
//!
//! A method with type parameters of its own, called through a supertype, is
//! dispatched on the concrete type behind the value (`generic_dispatch`). A
//! subtype with a type parameter the supertype does not fix
//! (`class Weird<T, U> extends Tree<T>`) is only a concrete type once `U` is
//! known, so the dispatch needs every `U` the program ever builds a `Weird`
//! with. The program is compiled whole, so that set is closed: every
//! `new Weird<A, B>(..)` is recorded with the context it is in (the class and
//! the function or method whose body builds it), every call of a generic
//! function or method with the type arguments it binds, and the set is the
//! fixpoint of substituting each context's own instantiations into the facts
//! written in it. A fact whose context is never instantiated builds nothing.
//!
//! The one way the set cannot close is a program that builds ever-larger
//! types (`f<X>` calling `f<Vec<X>>`, each building a `Weird<T, X>`):
//! polymorphic recursion. The fixpoint gives up at a size no real program
//! reaches and says so ([`Closure::unbounded`]).

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::symbol_table::SymbolTable;
use crate::ty::Ty;

/// What a fact is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FactKind {
    /// `new C<args>(..)`: an instantiation of class `C` (its FQN).
    New(String),
    /// A call of a generic function (`fn:<fqn>`) or method
    /// (`method:<class fqn>.<name>`) binding its own type parameters.
    Call(String),
}

/// One recorded instantiation, in the context it was written in.
#[derive(Debug, Clone)]
pub struct Fact {
    pub kind: FactKind,
    pub args: Vec<Ty>,
    /// The class whose body it is in, if any.
    pub ctx_class: Option<String>,
    /// The function or method whose body it is in, as a [`FactKind::Call`] key.
    pub ctx_method: Option<String>,
}

/// The closed instantiation sets.
#[derive(Debug, Clone, Default)]
pub struct Closure {
    /// Every concrete argument list each class is built with.
    pub classes: HashMap<String, Vec<Vec<Ty>>>,
    /// Classes whose set did not close (polymorphic recursion), with the
    /// function or method that keeps growing it.
    pub unbounded: HashMap<String, String>,
}

const MAX_PER_KEY: usize = 512;
const MAX_ROUNDS: usize = 64;
const MAX_DEPTH: usize = 12;

fn contains_param(t: &Ty) -> bool {
    match t {
        Ty::Param(_) => true,
        // An argument the checker could not type stands for whatever it is
        // (a position the supertype fixes needs nothing from it).
        Ty::Unknown => false,
        Ty::User { generic_args, .. } => generic_args.iter().any(contains_param),
        Ty::Array { element, .. } => contains_param(element),
        Ty::Nullable(inner) => contains_param(inner),
        Ty::Fn { params, return_type, .. } => params.iter().any(contains_param) || contains_param(return_type),
        Ty::Wildcard(_) => true,
        _ => false,
    }
}

fn depth(t: &Ty) -> usize {
    match t {
        Ty::User { generic_args, .. } => 1 + generic_args.iter().map(depth).max().unwrap_or(0),
        Ty::Array { element, .. } => 1 + depth(element),
        Ty::Nullable(inner) => depth(inner),
        Ty::Fn { params, return_type, .. } => 1 + params.iter().chain(std::iter::once(&**return_type)).map(depth).max().unwrap_or(0),
        _ => 1,
    }
}

fn subst(t: &Ty, map: &HashMap<String, Ty>) -> Ty {
    match t {
        Ty::Param(p) => map.get(p).cloned().unwrap_or_else(|| t.clone()),
        Ty::User { name, generic_args } => Ty::User {
            name: name.clone(),
            generic_args: generic_args.iter().map(|a| subst(a, map)).collect(),
        },
        Ty::Array { element, kind } => Ty::Array { element: Box::new(subst(element, map)), kind: *kind },
        Ty::Nullable(inner) => Ty::Nullable(Box::new(subst(inner, map))),
        Ty::Fn { params, return_type, is_async } => Ty::Fn {
            params: params.iter().map(|a| subst(a, map)).collect(),
            return_type: Box::new(subst(return_type, map)),
            is_async: *is_async,
        },
        other => other.clone(),
    }
}

/// The type parameter names of a context key's declaration.
fn key_params(symbols: &SymbolTable, key: &str) -> Vec<String> {
    let names = |ps: &[juxc_ast::TypeParam]| ps.iter().filter(|p| !p.is_const()).map(|p| p.name.text.clone()).collect();
    if let Some(f) = key.strip_prefix("fn:") {
        return symbols.functions.get(f).map(|s| names(&s.generic_params)).unwrap_or_default();
    }
    if let Some(m) = key.strip_prefix("method:") {
        if let Some((class, method)) = m.rsplit_once('.') {
            return symbols
                .lookup_method(class, method)
                .map(|(s, _)| names(&s.generic_params))
                .or_else(|| symbols.interfaces.get(class).and_then(|i| i.methods.get(method)).map(|s| names(&s.generic_params)))
                .unwrap_or_default();
        }
    }
    Vec::new()
}

fn class_params(symbols: &SymbolTable, class: &str) -> Vec<String> {
    symbols
        .classes
        .get(class)
        .map(|c| c.generic_params.iter().filter(|p| !p.is_const()).map(|p| p.name.text.clone()).collect())
        .or_else(|| symbols.records.get(class).map(|r| r.generic_params.iter().map(|p| p.name.text.clone()).collect()))
        .unwrap_or_default()
}

/// Close the facts. `seeds` are concrete class types found anywhere in the
/// program (every checked expression's type), which need no context.
pub fn close(symbols: &SymbolTable, facts: &[Fact], seeds: &[Ty]) -> Closure {
    let mut sets: HashMap<FactKind, BTreeSet<Vec<String>>> = HashMap::new();
    let mut values: HashMap<(FactKind, Vec<String>), Vec<Ty>> = HashMap::new();
    fn add(
        sets: &mut HashMap<FactKind, BTreeSet<Vec<String>>>,
        values: &mut HashMap<(FactKind, Vec<String>), Vec<Ty>>,
        kind: FactKind,
        args: Vec<Ty>,
    ) -> bool {
        let key: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let set = sets.entry(kind.clone()).or_default();
        if set.len() >= MAX_PER_KEY || set.contains(&key) {
            return false;
        }
        set.insert(key.clone());
        values.insert((kind, key), args);
        true
    }
    for s in seeds {
        collect_seed(s, &mut |name, args| {
            add(&mut sets, &mut values, FactKind::New(name), args);
        });
    }
    let mut unbounded: HashMap<String, String> = HashMap::new();
    let mut growing: HashSet<String> = HashSet::new();
    for _ in 0..MAX_ROUNDS {
        let mut changed = false;
        for f in facts {
            let concrete = !f.args.iter().any(contains_param);
            if concrete {
                changed |= add(&mut sets, &mut values, f.kind.clone(), f.args.clone());
                continue;
            }
            // The context's own instantiations: its class's, times its
            // method's (and, for a method, those recorded against any
            // supertype declaring it, since a call through the supertype
            // reaches this body).
            let class_insts: Vec<Vec<Ty>> = match &f.ctx_class {
                Some(c) if !class_params(symbols, c).is_empty() => sets
                    .get(&FactKind::New(c.clone()))
                    .map(|s| s.iter().filter_map(|k| values.get(&(FactKind::New(c.clone()), k.clone())).cloned()).collect())
                    .unwrap_or_default(),
                _ => vec![Vec::new()],
            };
            let method_keys: Vec<String> = match &f.ctx_method {
                Some(m) => method_keys_reaching(symbols, m),
                None => Vec::new(),
            };
            let method_insts: Vec<Vec<Ty>> = match &f.ctx_method {
                Some(m) if !key_params(symbols, m).is_empty() => method_keys
                    .iter()
                    .flat_map(|k| {
                        let kind = FactKind::Call(k.clone());
                        sets.get(&kind)
                            .map(|s| s.iter().filter_map(|x| values.get(&(kind.clone(), x.clone())).cloned()).collect::<Vec<_>>())
                            .unwrap_or_default()
                    })
                    .collect(),
                _ => vec![Vec::new()],
            };
            let cparams = f.ctx_class.as_deref().map(|c| class_params(symbols, c)).unwrap_or_default();
            let mparams = f.ctx_method.as_deref().map(|m| key_params(symbols, m)).unwrap_or_default();
            for ci in &class_insts {
                for mi in &method_insts {
                    let mut map: HashMap<String, Ty> = HashMap::new();
                    for (p, a) in cparams.iter().zip(ci) {
                        map.insert(p.clone(), a.clone());
                    }
                    for (p, a) in mparams.iter().zip(mi) {
                        map.insert(p.clone(), a.clone());
                    }
                    let args: Vec<Ty> = f.args.iter().map(|a| subst(a, &map)).collect();
                    if args.iter().any(contains_param) {
                        continue;
                    }
                    if args.iter().any(|a| depth(a) > MAX_DEPTH) {
                        match &f.kind {
                            FactKind::New(c) => {
                                unbounded.entry(c.clone()).or_insert_with(|| f.ctx_method.clone().unwrap_or_default());
                            }
                            // A generic call whose arguments keep growing: every
                            // class its callee builds from them grows with it.
                            FactKind::Call(k) => {
                                growing.insert(k.clone());
                            }
                        }
                        continue;
                    }
                    changed |= add(&mut sets, &mut values, f.kind.clone(), args);
                }
            }
        }
        if !changed {
            break;
        }
    }
    // A class built, from its type parameters, in a body that polymorphic
    // recursion keeps instantiating at larger arguments has no closed set.
    for f in facts {
        if let (FactKind::New(c), Some(m)) = (&f.kind, &f.ctx_method) {
            if growing.contains(m) && f.args.iter().any(contains_param) {
                unbounded.entry(c.clone()).or_insert_with(|| m.clone());
            }
        }
    }
    let mut classes: HashMap<String, Vec<Vec<Ty>>> = HashMap::new();
    for (kind, set) in &sets {
        if let FactKind::New(c) = kind {
            let v: Vec<Vec<Ty>> = set.iter().filter_map(|k| values.get(&(kind.clone(), k.clone())).cloned()).collect();
            classes.insert(c.clone(), v);
        }
    }
    Closure { classes, unbounded }
}

/// The call keys whose instantiations reach the body of method key `m`: its
/// own, and a supertype's declaration of the same method.
fn method_keys_reaching(symbols: &SymbolTable, m: &str) -> Vec<String> {
    let mut out = vec![m.to_string()];
    if let Some(rest) = m.strip_prefix("method:") {
        if let Some((class, method)) = rest.rsplit_once('.') {
            let mut seen: HashSet<String> = HashSet::new();
            for owner in symbols.interfaces.keys().chain(symbols.classes.keys()) {
                if owner == class || !seen.insert(owner.clone()) {
                    continue;
                }
                if crate::generic_dispatch::supertype_args(symbols, class, owner).is_some() {
                    out.push(format!("method:{owner}.{method}"));
                }
            }
        }
    }
    out
}

fn collect_seed(t: &Ty, f: &mut dyn FnMut(String, Vec<Ty>)) {
    match t {
        Ty::User { name, generic_args } => {
            if !generic_args.is_empty() && !generic_args.iter().any(contains_param) {
                f(name.clone(), generic_args.clone());
            }
            for a in generic_args {
                collect_seed(a, f);
            }
        }
        Ty::Array { element, .. } => collect_seed(element, f),
        Ty::Nullable(inner) => collect_seed(inner, f),
        Ty::Fn { params, return_type, .. } => {
            for p in params {
                collect_seed(p, f);
            }
            collect_seed(return_type, f);
        }
        _ => {}
    }
}
