//! Generic foreign parameter slots (JUX-BINDGEN-ADDENDUM.md §G.3.6).
//!
//! A Rust parameter written `impl Into<WidgetText>`, `impl IntoAtoms` or
//! `&mut dyn TextBuffer` is not one type: it takes any value that converts
//! into `WidgetText`, that implements `IntoAtoms`, that implements
//! `TextBuffer`. The stub declares the slot with that target
//! (`@RustImpl WidgetText text`), and this module answers whether a Jux value
//! fits it, from what the stubs record about the target:
//!
//! - a CLASS target converts from the sources its own `From` impls name
//!   (`@RustFrom("RichText,String")`), and from anything that converts into a
//!   type a blanket `impl<T: Into<Y>> From<T>` names (`@RustFromInto("Y")`);
//! - an INTERFACE target is met by a type that implements it, by a Rust type
//!   its impls name directly (`@RustImplementedBy("String")` for
//!   `impl TextBuffer for String`), and by whatever meets the bound of one of
//!   its blanket impls (`@RustBlanket("Into<Atom>")`,
//!   `@RustBlanket("Hash + Debug")`).
//!
//! Nothing here is a list of crate names or type names: every answer is read
//! off the markers bindgen discovered in the crate's own rustdoc.

use crate::symbol_table::SymbolTable;
use crate::ty::{Primitive, Ty};

/// How deep the conversion chains are followed (`String` -> `WidgetText` ->
/// `AtomKind` -> `Atom` is three).
const MAX_DEPTH: u8 = 5;

/// Whether `found` fits the generic foreign slot declared as `expected`.
pub fn generic_slot_accepts(expected: &Ty, found: &Ty, symbols: &SymbolTable) -> bool {
    let (expected, found) = match (expected, found) {
        (Ty::Nullable(e), Ty::Nullable(f)) => (e.as_ref(), f.as_ref()),
        (Ty::Nullable(e), f) => (e.as_ref(), f),
        (e, f) => (e, f),
    };
    let Ty::User { name, .. } = expected else { return false };
    converts_or_satisfies(found, name, symbols, 0)
}

/// `found` converts into the foreign class `target`, or meets the foreign
/// trait `target`.
fn converts_or_satisfies(found: &Ty, target: &str, symbols: &SymbolTable, depth: u8) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    if let Some((fqn, iface)) = foreign_interface(target, symbols) {
        return satisfies(found, &fqn, iface, symbols, depth);
    }
    converts_into(found, target, symbols, depth)
}

/// `found` converts into the foreign type `target` through a `From` impl the
/// target records.
fn converts_into(found: &Ty, target: &str, symbols: &SymbolTable, depth: u8) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let Some((fqn, annotations)) = foreign_type_annotations(target, symbols) else {
        return false;
    };
    if same_type(found, &fqn) {
        return true;
    }
    let pkg = package_of(&fqn);
    if marker_list(annotations, "rustfrom").iter().any(|src| source_matches(found, src, symbols)) {
        return true;
    }
    // A borrowed VIEW (`Path`, `OsStr`: the types with `@RustOwnedAs`) is what
    // `impl AsRef<Path>` asks for, and Rust's std makes one from text: `str`
    // and `String` are `AsRef<Path>` and `AsRef<OsStr>`. So text fills such a
    // slot, which is how `render_to_file("out.pdf")` reads in Rust too.
    if matches!(found, Ty::String) && has_marker(annotations, "rustownedas") {
        return true;
    }
    marker_list(annotations, "rustfrominto")
        .iter()
        .any(|via| converts_or_satisfies(found, &qualify(pkg, via), symbols, depth + 1))
}

/// `found` meets the foreign trait `fqn`.
fn satisfies(
    found: &Ty,
    fqn: &str,
    iface: &crate::symbol_table::InterfaceSig,
    symbols: &SymbolTable,
    depth: u8,
) -> bool {
    let pkg = package_of(fqn);
    let bare = fqn.rsplit('.').next().unwrap_or(fqn);
    // A class that names the trait in its `implements` clause. When the
    // clause names the trait qualified, its crate family has another trait of
    // that simple name, and only the one named counts (ERRATA E148).
    if let Ty::User { name, .. } = found {
        if crate::ty::class_implements_interface(name, bare, symbols)
            && implements_exactly(name, fqn, symbols) != Some(false)
        {
            return true;
        }
    }
    // A Rust type the trait's impls name: `impl TextBuffer for String`.
    if iface.implemented_by.iter().any(|shape| shape_matches(found, shape, symbols)) {
        return true;
    }
    // A blanket impl's bound: `impl<T: Into<Atom>> IntoAtoms for T`,
    // `impl<T: Hash + Debug> AsId for T`.
    iface.blanket_over.iter().any(|bound| {
        bound.split('+').map(str::trim).filter(|b| !b.is_empty()).all(|b| {
            if let Some(target) = b
                .strip_prefix("Into<")
                .or_else(|| b.strip_prefix("AsRef<"))
                .or_else(|| b.strip_prefix("Borrow<"))
                .and_then(|r| r.strip_suffix('>'))
            {
                if target == "String" {
                    return matches!(found, Ty::String);
                }
                return converts_or_satisfies(found, &qualify(pkg, target), symbols, depth + 1);
            }
            std_trait_holds(found, b.strip_prefix("rust.std.").unwrap_or(b), symbols)
                .unwrap_or_else(|| converts_or_satisfies(found, &qualify(pkg, b), symbols, depth + 1))
        })
    })
}

/// Whether one of Rust's standard traits holds for `found`, or `None` when
/// `name` is not one of them. The Jux built-in types (numbers, `char`,
/// `bool`, `String`) have the ones Rust gives them; a foreign type answers
/// through the markers its stub carries; a class of the program is `Clone` and
/// `Debug`, which every one of them derives.
fn std_trait_holds(found: &Ty, name: &str, symbols: &SymbolTable) -> Option<bool> {
    let floating = matches!(
        found,
        Ty::Primitive(Primitive::Float | Primitive::Double | Primitive::F32 | Primitive::F64)
    );
    let builtin = matches!(found, Ty::Primitive(_) | Ty::String);
    let marker = |m: &str| -> bool {
        let Ty::User { name, .. } = found else { return false };
        match foreign_type_annotations(name, symbols) {
            Some((_, annotations)) => has_marker(annotations, m),
            // A class of the program derives `Clone` and `Debug`.
            None => matches!(m, "rustclone" | "rustdebug") && symbols.classes.contains_key(name.as_str()),
        }
    };
    Some(match name {
        "Hash" | "Eq" | "Ord" => (builtin && !floating) || (name == "Hash" && marker("rusthash")),
        "Debug" => builtin || marker("rustdebug"),
        "Clone" => builtin || marker("rustclone"),
        "PartialEq" => builtin || marker("rustpartialeq"),
        "Default" => builtin || marker("rustdefault"),
        "PartialOrd" | "Display" | "ToString" => builtin,
        "Copy" => matches!(found, Ty::Primitive(_)),
        "Send" | "Sync" | "Sized" | "Unpin" | "Any" => true,
        _ => return None,
    })
}

/// Whether `found` is the Rust type a `@RustFrom` entry names.
fn source_matches(found: &Ty, src: &str, symbols: &SymbolTable) -> bool {
    match (src, found) {
        ("String" | "str", Ty::String) => true,
        (_, Ty::Primitive(p)) => p.rust_name() == src,
        (_, Ty::User { name, .. }) => names_match(name, src, symbols),
        _ => false,
    }
}

/// Whether `found` is the Rust shape a `@RustImplementedBy` entry names.
fn shape_matches(found: &Ty, shape: &str, symbols: &SymbolTable) -> bool {
    match (shape, found) {
        ("String" | "str", Ty::String) => true,
        ("[]", Ty::Array { .. }) => true,
        (_, Ty::Primitive(p)) => p.rust_name() == shape,
        (_, Ty::User { name, .. }) => names_match(name, shape, symbols),
        _ => false,
    }
}

/// Whether the type `found` is the one a marker entry names. An entry is a
/// simple name, matched by simple name, or a QUALIFIED one, which bindgen
/// writes when the crate family has several types of that simple name
/// (`rust.naga.front.wgsl.Error`, ERRATA E148) and which means that
/// one type, under any of its spellings (an alias in a nested package is the
/// type it names).
fn names_match(found: &str, entry: &str, symbols: &SymbolTable) -> bool {
    match (entry.contains('.'), found.contains('.')) {
        (true, true) => symbols.canonical_type_fqn(found) == symbols.canonical_type_fqn(entry),
        (true, false) => entry.rsplit('.').next() == Some(found),
        (false, _) => found.rsplit('.').next() == Some(entry),
    }
}

/// Whether the foreign type `found` implements exactly the trait `fqn`, when
/// its `implements` clause names a trait QUALIFIED: bindgen writes that for a
/// trait whose simple name the crate family shares (ERRATA E148), and
/// then the simple name alone would take the wrong one. `None` when the
/// clause names every trait by its simple name, or `found` is not a type
/// whose clause this can read.
fn implements_exactly(found: &str, fqn: &str, symbols: &SymbolTable) -> Option<bool> {
    let refs: &[juxc_ast::TypeRef] = if let Some(c) = symbols.classes.get(found) {
        &c.implements
    } else if let Some(e) = symbols.enums.get(found) {
        &e.implements
    } else if let Some(r) = symbols.records.get(found) {
        &r.implements
    } else {
        return None;
    };
    let written: Vec<String> = refs
        .iter()
        .map(|r| r.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
        .collect();
    if !written.iter().any(|w| w.contains('.')) {
        return None;
    }
    let pkg = package_of(found);
    let want = symbols.canonical_type_fqn(fqn);
    Some(written.iter().any(|w| symbols.canonical_type_fqn(&qualify(pkg, w)) == want))
}

/// `found` is the foreign type `fqn` itself.
fn same_type(found: &Ty, fqn: &str) -> bool {
    matches!(found, Ty::User { name, .. } if name == fqn)
}

/// The foreign interface `name` (an FQN, or a bare name) resolves to.
fn foreign_interface<'a>(
    name: &str,
    symbols: &'a SymbolTable,
) -> Option<(String, &'a crate::symbol_table::InterfaceSig)> {
    let key = resolve_key(name, symbols, |k| symbols.interfaces.contains_key(k))?;
    let sig = symbols.interfaces.get(&key)?;
    sig.is_external.then_some((key, sig))
}

/// The annotations of the foreign class or enum `name` (an FQN, or a bare
/// name), with its FQN. An alias is followed to the class it names.
fn foreign_type_annotations<'a>(
    name: &str,
    symbols: &'a SymbolTable,
) -> Option<(String, &'a [juxc_ast::Annotation])> {
    let name = symbols.alias_class(name).unwrap_or_else(|| name.to_string());
    if let Some(key) = resolve_key(&name, symbols, |k| symbols.classes.contains_key(k)) {
        let sig = symbols.classes.get(&key)?;
        return sig.is_external.then_some((key, sig.annotations.as_slice()));
    }
    let key = resolve_key(&name, symbols, |k| symbols.enums.contains_key(k))?;
    let sig = symbols.enums.get(&key)?;
    sig.is_external.then_some((key, sig.annotations.as_slice()))
}

/// `name` itself when `exists` says so, or else the one FQN of that bare
/// name among the foreign declarations.
fn resolve_key(name: &str, symbols: &SymbolTable, exists: impl Fn(&str) -> bool) -> Option<String> {
    if exists(name) {
        return Some(name.to_string());
    }
    if name.contains('.') {
        return None;
    }
    symbols.find_fqn_by_bare(name).filter(|k| exists(k))
}

/// The dotted package of an FQN (`rust.eframe` of `rust.eframe.Atom`).
fn package_of(fqn: &str) -> &str {
    fqn.rsplit_once('.').map_or("", |(p, _)| p)
}

/// `name` in package `pkg`, unless it is already qualified.
fn qualify(pkg: &str, name: &str) -> String {
    if name.contains('.') || pkg.is_empty() {
        name.to_string()
    } else {
        format!("{pkg}.{name}")
    }
}

fn has_marker(annotations: &[juxc_ast::Annotation], name: &str) -> bool {
    annotations
        .iter()
        .any(|a| a.name.segments.len() == 1 && a.name.segments[0].text.eq_ignore_ascii_case(name))
}

/// The comma-separated entries of the string-valued marker `name`.
fn marker_list(annotations: &[juxc_ast::Annotation], name: &str) -> Vec<String> {
    use juxc_ast::{AnnotationArg, Expr, Literal};
    annotations
        .iter()
        .filter(|a| a.name.segments.len() == 1 && a.name.segments[0].text.eq_ignore_ascii_case(name))
        .filter_map(|a| match a.args.first() {
            Some(AnnotationArg::Positional(Expr::Literal(Literal::String(s)))) => Some(s.clone()),
            _ => None,
        })
        .flat_map(|s| s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect::<Vec<_>>())
        .collect()
}
