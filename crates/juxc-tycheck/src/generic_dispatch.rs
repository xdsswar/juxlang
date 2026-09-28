//! Methods with type parameters of their own reached through a supertype
//! (ERRATA E1XX-GAP39b): the visitor pattern's `<R> R accept(Visitor<R> v)`
//! declared on an interface or an extended class and called on a value typed
//! by it.
//!
//! A supertype value is a trait object, and a trait object cannot carry a
//! method with type parameters. Jux compiles the whole program, so the set of
//! types that can stand behind such a value is closed: every concrete class
//! and record that is one. The call is dispatched on that set. The object
//! answers what it is (`JuxDynAny`, the one piece that goes through the
//! vtable), the call site's type arguments are whatever the call has, and the
//! chosen type's own method is monomorphized for them by the Rust compiler.
//! That needs no closed set of type ARGUMENTS, so a type argument that is
//! itself a type parameter of the caller works as well as a concrete one.
//!
//! What it does need is that a value of the supertype says which of those
//! types it is, arguments included. [`unpinned_implementer_param`] finds the
//! one shape where it cannot: a subtype with a type parameter the supertype
//! does not fix (`class Weird<T, U> extends Tree<T>`).

use std::collections::HashMap;

use juxc_ast::TypeRef;

use crate::symbol_table::{fqn_bare, SymbolTable};

/// The supertypes (classes and interfaces) that declare a method, other than
/// a `static` or `private` one, with type parameters of its own: the names of
/// those methods, by the supertype's FQN.
pub fn generic_virtual_owners(symbols: &SymbolTable) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for (fqn, iface) in &symbols.interfaces {
        if iface.is_external {
            continue;
        }
        let mut names: Vec<String> = iface
            .methods
            .iter()
            .filter(|(_, m)| !m.is_static && !m.generic_params.is_empty())
            .map(|(n, _)| n.clone())
            .collect();
        if !names.is_empty() {
            names.sort();
            out.insert(fqn.clone(), names);
        }
    }
    for (fqn, class) in &symbols.classes {
        if class.is_external {
            continue;
        }
        let mut names: Vec<String> = class
            .methods
            .iter()
            .filter(|(_, m)| {
                !m.is_static
                    && !m.generic_params.is_empty()
                    && !matches!(m.visibility, juxc_ast::Visibility::Private)
            })
            .map(|(n, _)| n.clone())
            .collect();
        if !names.is_empty() {
            names.sort();
            out.insert(fqn.clone(), names);
        }
    }
    out
}

/// The package part of an FQN (`""` for the root package).
fn package_of(fqn: &str) -> &str {
    fqn.rsplit_once('.').map(|(p, _)| p).unwrap_or("")
}

/// The FQN a supertype reference written in `from`'s declaration names.
fn resolve_written(symbols: &SymbolTable, from: &str, written: &TypeRef) -> Option<String> {
    let name = written
        .name
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    let known = |k: &str| symbols.classes.contains_key(k) || symbols.interfaces.contains_key(k);
    if known(&name) {
        return Some(name);
    }
    let pkg = package_of(from);
    if !pkg.is_empty() {
        let local = format!("{pkg}.{name}");
        if known(&local) {
            return Some(local);
        }
    }
    symbols.find_fqn_by_bare_in(&name, pkg).filter(|k| known(k))
}

/// The type arguments `base` is instantiated with when reached from `sub`,
/// written in `sub`'s own parameter names: `[T]` for `Leaf<T> extends
/// Tree<T>`, `[int]` for `IntLeaf extends Tree<int>`, `[]` for a non-generic
/// `base`. Walks `extends` and `implements` (and an interface's `extends`).
/// `None` when `base` is not a supertype of `sub`.
pub fn supertype_args(symbols: &SymbolTable, sub: &str, base: &str) -> Option<Vec<TypeRef>> {
    let own = |fqn: &str| -> Vec<String> {
        symbols
            .classes
            .get(fqn)
            .map(|c| &c.generic_params)
            .or_else(|| symbols.interfaces.get(fqn).map(|i| &i.generic_params))
            .or_else(|| symbols.records.get(fqn).map(|r| &r.generic_params))
            .map(|ps| ps.iter().filter(|p| !p.is_const()).map(|p| p.name.text.clone()).collect())
            .unwrap_or_default()
    };
    let start: Vec<TypeRef> = own(sub).iter().map(|n| bare_type_ref(n)).collect();
    let mut stack: Vec<(String, Vec<TypeRef>)> = vec![(sub.to_string(), start)];
    let mut seen = std::collections::HashSet::new();
    while let Some((cur, args)) = stack.pop() {
        if cur == base {
            return Some(args);
        }
        if !seen.insert(cur.clone()) {
            continue;
        }
        let map: HashMap<String, TypeRef> = own(&cur).into_iter().zip(args.iter().cloned()).collect();
        let mut supers: Vec<(Option<String>, TypeRef)> = Vec::new();
        if let Some(c) = symbols.classes.get(&cur) {
            if let Some(e) = &c.extends {
                supers.push((c.extends_fqn.clone(), e.clone()));
            }
            for i in &c.implements {
                supers.push((None, i.clone()));
            }
        } else if let Some(i) = symbols.interfaces.get(&cur) {
            for e in &i.extends {
                supers.push((None, e.clone()));
            }
        } else if let Some(r) = symbols.records.get(&cur) {
            for i in &r.implements {
                supers.push((None, i.clone()));
            }
        }
        for (fqn, written) in supers {
            let Some(fqn) = fqn.or_else(|| resolve_written(symbols, &cur, &written)) else { continue };
            let next: Vec<TypeRef> = written
                .generic_args
                .iter()
                .filter_map(|a| a.as_type())
                .map(|t| crate::symbol_table::substitute_type_ref(t, &map))
                .collect();
            stack.push((fqn, next));
        }
    }
    None
}

fn bare_type_ref(name: &str) -> TypeRef {
    let span = juxc_source::Span::DUMMY;
    TypeRef {
        name: juxc_ast::QualifiedName {
            segments: vec![juxc_ast::Ident { text: name.to_string(), span }],
            span,
        },
        generic_args: Vec::new(),
        nullable: false,
        array_shape: None,
        fn_shape: None,
        ptr_depth: 0,
        span,
    }
}

/// The concrete types (non-abstract classes and records, the supertype
/// itself included when it is one) that are a `base`, sorted.
pub fn concrete_implementers(symbols: &SymbolTable, base: &str) -> Vec<String> {
    let mut out: Vec<String> = symbols
        .classes
        .iter()
        .filter(|(_, c)| !c.is_abstract && !c.is_external)
        .map(|(k, _)| k.clone())
        .chain(symbols.records.keys().cloned())
        .filter(|k| supertype_args(symbols, k, base).is_some())
        .collect();
    out.sort();
    out
}

/// For a concrete `sub` of `base`: its first type parameter that `base`'s
/// arguments do not fix, so a `base` value that is a `sub` cannot say what it
/// is (`U` in `class Weird<T, U> extends Tree<T>`). `None` when every one is
/// fixed, or `sub` is not a `base`.
pub fn unpinned_implementer_param(symbols: &SymbolTable, sub: &str, base: &str) -> Option<String> {
    let args = supertype_args(symbols, sub, base)?;
    let params: Vec<String> = symbols
        .classes
        .get(sub)
        .map(|c| &c.generic_params)
        .or_else(|| symbols.records.get(sub).map(|r| &r.generic_params))
        .map(|ps| ps.iter().filter(|p| !p.is_const()).map(|p| p.name.text.clone()).collect())
        .unwrap_or_default();
    params.into_iter().find(|p| {
        !args.iter().any(|a| {
            a.generic_args.is_empty() && a.name.segments.len() == 1 && a.name.segments[0].text == *p
        })
    })
}

/// The written name of a declaration, for a message.
pub fn shown(fqn: &str) -> &str {
    fqn_bare(fqn)
}

/// Each argument type seen as the parameter's type where it is a SUBTYPE of
/// it: `Mirror<String>` passed for a `TreeVisitor<T, R>` is read as the
/// `TreeVisitor<String, Tree<String>>` it is, so inference binds `R`
/// (ERRATA E1XX-GAP39b). Inference unifies heads structurally and learned
/// nothing from a subtype before. Arguments of the parameter's own type, and
/// every other shape, pass through unchanged.
pub fn args_as_param_types(
    symbols: &SymbolTable,
    param_tys: &[&TypeRef],
    arg_tys: &[crate::ty::Ty],
) -> Vec<crate::ty::Ty> {
    use crate::ty::Ty;
    arg_tys
        .iter()
        .enumerate()
        .map(|(i, arg)| {
            let (Some(declared), Ty::User { name, generic_args }) = (param_tys.get(i), arg) else {
                return arg.clone();
            };
            let Some(head) = declared.name.segments.last().map(|s| s.text.as_str()) else {
                return arg.clone();
            };
            if declared.generic_args.is_empty() || name.rsplit('.').next() == Some(head) {
                return arg.clone();
            }
            let base = symbols
                .interfaces
                .keys()
                .chain(symbols.classes.keys())
                .find(|k| k.as_str() == head || k.rsplit('.').next() == Some(head))
                .cloned();
            let Some(base) = base else { return arg.clone() };
            let Some(args) = supertype_args(symbols, name, &base) else { return arg.clone() };
            let sub_params: Vec<juxc_ast::TypeParam> = symbols
                .classes
                .get(name)
                .map(|c| c.generic_params.clone())
                .or_else(|| symbols.records.get(name).map(|r| r.generic_params.clone()))
                .unwrap_or_default();
            let lowered: Vec<Ty> = args
                .iter()
                .map(|a| crate::ty::substitute(&crate::ty::lower_member_type(a, name, symbols), &sub_params, generic_args))
                .collect();
            Ty::User { name: base, generic_args: lowered }
        })
        .collect()
}
