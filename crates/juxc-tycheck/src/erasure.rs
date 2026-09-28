//! Polymorphic recursion, lowered by erasing type arguments (ERRATA
//! E1XX-GAP39g).
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
//! it is seen. A type parameter with a bound cannot be erased (a boxed value
//! has none of the bound's members); such a cycle is still `E0438`.

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
    // descendants (a `Nested<T>` seen as its `Nest<T>` is one erased type).
    loop {
        let mut grew = false;
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
    // A bound cannot be erased: that part of the program is `E0438`.
    let grower = fns.iter().next().cloned().or_else(|| closure.unbounded.values().next().cloned()).unwrap_or_default();
    let mut refused: Vec<(String, String, juxc_ast::TypeParam)> = Vec::new();
    for c in &classes {
        for p in generic_params_of(symbols, c).unwrap_or_default() {
            if p.is_const() || !p.bounds.is_empty() {
                refused.push((c.clone(), grower.clone(), p));
            }
        }
    }
    for k in &fns {
        for p in key_generic_params(symbols, k) {
            if p.is_const() || !p.bounds.is_empty() {
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
        let what = if param.is_const() { "a `const` parameter".to_string() } else { format!("bounded (`extends {bound}`)") };
        out.push((
            unit,
            juxc_diagnostics::Diagnostic::error(
                juxc_diagnostics::code::Code::E0438_GenericVirtualMethod,
                format!(
                    "`{grower}` calls itself at an ever-larger type argument (polymorphic recursion), which is \
                     compiled by erasing the type arguments of every function and class on that cycle -- but \
                     `{}` of `{name}` is {what}, and an erased value has none of a bound's members",
                    param.name.text,
                ),
            )
            .with_span(param.span)
            .with_help(format!(
                "drop the bound on `{}`, or keep `{grower}` from calling itself at a larger type argument",
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
