//! Import aliases are resolved once, before anything reads a name
//! (ERRATA E1XX-GAP37b).
//!
//! `import app.model.Circle as C;` binds the simple name `C` in its unit.
//! The checker and the backend each resolve names in many places, and every
//! place that read the SPELLED name instead of the unit's import table got an
//! alias wrong: `class Sq implements S` implemented nothing, `case C c` asked
//! the value for `__jux_as_C`, `C.Builder` named no type. So the alias is
//! written back as the type it names, in every place a unit names a type
//! ([`juxc_ast::visit_mut`]), and nothing downstream ever sees it:
//!
//! - the head of every type reference (`C`, `C.Builder`, `V<S>`, `C[]`,
//!   `(C) -> int`), in declarations, supertypes, locals, casts, type tests,
//!   `catch`, lambda parameters, generic arguments and bounds;
//! - the class of a `new`, a `throws` name, a method reference's receiver;
//! - the type of a `case C c` pattern;
//! - the head of an expression path (`C.unit()`, `Col.GREEN`), unless a
//!   local, parameter or field of that name is in scope, which then is what
//!   the name means (JLS 6.4.2).
//!
//! A generic parameter of the same name shadows the alias in a type. Once
//! every use is rewritten the alias's import is removed; an alias whose name
//! conflicts with another binding is left as written for `E0303`.

use std::collections::HashMap;

use juxc_ast::visit_mut::{rewrite_type_names, Scope, TypeNameRewriter};
use juxc_ast::{CompilationUnit, Ident, ImportSpec, NewObjectExpr, QualifiedName, TypeRef};

/// Rewrite every use of an import alias in `units` as the type it names.
/// Stubs declare no aliases and are left alone.
///
/// Where the unit can name the target by its own simple name, that is what
/// the alias becomes, with the import it then needs (`C` becomes `Circle`,
/// and the unit imports `app.model.Circle`): a simple name bound by a
/// single-type import is the form every part of the compiler reads. That is
/// the case unless the simple name already means something in the unit (a
/// type of its package, another import, a type some wildcard import brings),
/// usually the very reason for the alias (`import rust.eframe.egui.Frame as
/// EguiFrame;` beside eframe's `Frame`). Then the alias becomes the target's
/// fully-qualified name, which is exact wherever it is written.
pub fn expand_import_aliases(units: &mut [CompilationUnit]) {
    if !units.iter().any(|u| !u.is_external && !import_aliases(u).is_empty()) {
        return;
    }
    let declared = declared_type_names(units);
    for unit in units.iter_mut() {
        if unit.is_external {
            continue;
        }
        let mut aliases = import_aliases(unit);
        if aliases.is_empty() {
            continue;
        }
        // An alias whose name another import binds to something else, or
        // that a type of the unit's package already has, is the checker's
        // to report (`E0303`); it is left exactly as written.
        let pkg_types = declared.get(&unit_package(unit)).cloned().unwrap_or_default();
        let bindings = import_bindings(unit);
        // Only an alias of a TYPE is rewritten: a function's or a constant's
        // (`import jux.std.testing.assertThrows as expectThrown;`) is a call
        // the unit's name table already resolves, the compiler's intrinsics
        // included.
        aliases.retain(|alias, path| {
            let joined = path.join(".");
            let (simple, pkg) = (path.last().cloned().unwrap_or_default(), path[..path.len() - 1].join("."));
            let is_type = declared.get(&pkg).is_some_and(|names| names.contains(&simple));
            is_type && !pkg_types.contains(alias) && bindings.iter().all(|(b, t)| b != alias || *t == joined)
        });
        if aliases.is_empty() {
            continue;
        }
        let taken = names_the_unit_binds(unit, &declared);
        let mut targets: HashMap<String, Vec<String>> = HashMap::new();
        let mut synthetic: Vec<juxc_ast::ImportDecl> = Vec::new();
        for (alias, path) in &aliases {
            let simple = path.last().cloned().unwrap_or_default();
            let target_is_type = declared
                .get(&path[..path.len() - 1].join("."))
                .is_some_and(|names| names.contains(&simple));
            // A library type every unit reaches by its simple name
            // (`Exception`, `Vec`) that is not the target itself.
            let target_pkg = path[..path.len() - 1].join(".");
            let implicit = declared.iter().any(|(pkg, names)| {
                *pkg != target_pkg
                    && crate::symbol_table::is_library_realm_package(pkg)
                    && names.contains(&simple)
            });
            // Two aliases of the same type may both become its simple name;
            // an alias OF another type that happens to be that name is
            // rewritten too, and so no longer takes it.
            let one_target = aliases.values().filter(|p| p.last() == Some(&simple)).all(|p| p == path);
            let free = target_is_type && !implicit && !taken.contains(&simple) && one_target;
            if free {
                targets.insert(alias.clone(), vec![simple.clone()]);
                let span = unit
                    .imports
                    .iter()
                    .find(|i| import_binds(i, alias))
                    .map(|i| i.span)
                    .unwrap_or(unit.span);
                synthetic.push(juxc_ast::ImportDecl {
                    cfg: None,
                    spec: ImportSpec::Path {
                        name: QualifiedName {
                            segments: path.iter().map(|t| Ident { text: t.clone(), span }).collect(),
                            span,
                        },
                        wildcard: false,
                        alias: None,
                    },
                    span,
                });
            } else {
                targets.insert(alias.clone(), path.clone());
            }
        }
        rewrite_type_names(unit, &mut AliasRewriter { aliases: targets });
        // Nothing in the unit spells a rewritten alias any more. Its import
        // would still bind the name, and a lookup by simple name could then
        // take `Vec` for the `HashMap` an alias called `Vec` once named.
        unit.imports.retain_mut(|import| match &mut import.spec {
            ImportSpec::Path { alias: Some(a), .. } => !aliases.contains_key(&a.text),
            ImportSpec::Items { items, .. } => {
                items.retain(|it| !it.alias.as_ref().is_some_and(|a| aliases.contains_key(&a.text)));
                !items.is_empty()
            }
            _ => true,
        });
        for s in synthetic {
            let already = unit.imports.iter().any(|i| {
                matches!(&i.spec, ImportSpec::Path { name, wildcard: false, alias: None }
                    if matches!(&s.spec, ImportSpec::Path { name: n2, .. }
                        if n2.segments.iter().map(|x| &x.text).eq(name.segments.iter().map(|x| &x.text))))
            });
            if !already {
                unit.imports.push(s);
            }
        }
    }
}

/// Hand the backend each supertype written by its qualified name
/// (`implements app.model.Shape`, as an alias that clashes becomes) as ONE
/// name, `app.model.Shape`.
///
/// The backend reads a supertype by its head segment, as the name to look
/// the type up by: the head of a qualified one is its first package
/// (`app`), which names nothing. Packed into one segment the lookup gets the
/// qualified name, which it accepts, and a Rust path is still written from
/// the type it finds. The checker has already read the supertypes, as the
/// paths they are; only the backend's copies are packed.
pub fn pack_qualified_supertypes(units: &mut [CompilationUnit], symbols: &mut crate::SymbolTable) {
    fn pack(t: &mut TypeRef) {
        if t.name.segments.len() < 2 {
            return;
        }
        let text = t.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
        let span = t.name.span;
        t.name.segments = vec![Ident { text, span }];
    }
    for unit in units.iter_mut() {
        if unit.is_external {
            continue;
        }
        for item in &mut unit.items {
            use juxc_ast::TopLevelDecl as T;
            match item {
                T::Class(c) => c.extends.iter_mut().chain(c.implements.iter_mut()).for_each(pack),
                T::Record(r) => r.implements.iter_mut().for_each(pack),
                T::Enum(e) => e.implements.iter_mut().for_each(pack),
                T::Interface(i) => i.extends.iter_mut().for_each(pack),
                _ => {}
            }
        }
    }
    for c in symbols.classes.values_mut().filter(|c| !c.is_external) {
        c.extends.iter_mut().chain(c.implements.iter_mut()).for_each(pack);
    }
    for r in symbols.records.values_mut() {
        r.implements.iter_mut().for_each(pack);
    }
    for e in symbols.enums.values_mut().filter(|e| !e.is_external) {
        e.implements.iter_mut().for_each(pack);
    }
    for i in symbols.interfaces.values_mut().filter(|i| !i.is_external) {
        i.extends.iter_mut().for_each(pack);
    }
}

/// Every `(bound name, imported path)` the unit's single-type imports make.
fn import_bindings(unit: &CompilationUnit) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for import in &unit.imports {
        match &import.spec {
            ImportSpec::Path { name, wildcard: false, alias } => {
                let path = name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                if let Some(bind) = alias.as_ref().or(name.segments.last()) {
                    out.push((bind.text.clone(), path));
                }
            }
            ImportSpec::Items { prefix, items } => {
                let pfx = prefix.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                for it in items {
                    let bind = it.alias.as_ref().unwrap_or(&it.name).text.clone();
                    let path = if pfx.is_empty() { it.name.text.clone() } else { format!("{pfx}.{}", it.name.text) };
                    out.push((bind, path));
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether `import` is the one that binds `alias`.
fn import_binds(import: &juxc_ast::ImportDecl, alias: &str) -> bool {
    match &import.spec {
        ImportSpec::Path { alias: Some(a), .. } => a.text == alias,
        ImportSpec::Items { items, .. } => items.iter().any(|i| i.alias.as_ref().is_some_and(|a| a.text == alias)),
        _ => false,
    }
}

/// Package -> the simple names of the types its units declare, across the
/// whole program and its stubs.
fn declared_type_names(units: &[CompilationUnit]) -> HashMap<String, std::collections::HashSet<String>> {
    let mut out: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    for unit in units {
        let pkg = unit_package(unit);
        let names = out.entry(pkg).or_default();
        for item in &unit.items {
            use juxc_ast::TopLevelDecl as T;
            let name = match item {
                T::Class(c) => &c.name.text,
                T::Record(r) => &r.name.text,
                T::Enum(e) => &e.name.text,
                T::Interface(i) => &i.name.text,
                T::TypeAlias(a) => &a.name.text,
                T::Annotation(a) => &a.name.text,
                _ => continue,
            };
            names.insert(name.clone());
        }
    }
    out
}

fn unit_package(unit: &CompilationUnit) -> String {
    unit.package
        .as_ref()
        .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
        .unwrap_or_default()
}

/// Every simple name that already means a type in `unit`: its package's
/// types, what its non-alias imports bind, and what its wildcards bring.
fn names_the_unit_binds(
    unit: &CompilationUnit,
    declared: &HashMap<String, std::collections::HashSet<String>>,
) -> std::collections::HashSet<String> {
    let mut out: std::collections::HashSet<String> =
        declared.get(&unit_package(unit)).cloned().unwrap_or_default();
    for import in &unit.imports {
        match &import.spec {
            ImportSpec::Path { name, wildcard: true, .. } => {
                let pkg = name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                out.extend(declared.get(&pkg).into_iter().flatten().cloned());
            }
            ImportSpec::Path { name, wildcard: false, alias } => {
                let bound = alias.as_ref().or(name.segments.last()).map(|i| i.text.clone());
                // An alias's own import binds the alias, not the simple name.
                if alias.is_none() {
                    out.extend(bound);
                }
            }
            ImportSpec::Items { items, .. } => {
                out.extend(items.iter().filter(|i| i.alias.is_none()).map(|i| i.name.text.clone()));
            }
        }
    }
    out
}

/// Alias -> the path it stands for, from the unit's `import ... as ...;`
/// lines. An alias equal to the imported name is an ordinary import.
fn import_aliases(unit: &CompilationUnit) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    for import in &unit.imports {
        match &import.spec {
            ImportSpec::Path { name, wildcard: false, alias: Some(alias) } => {
                let path: Vec<String> = name.segments.iter().map(|s| s.text.clone()).collect();
                if path.last() != Some(&alias.text) && !path.is_empty() {
                    out.insert(alias.text.clone(), path);
                }
            }
            ImportSpec::Items { prefix, items } => {
                for item in items {
                    let Some(alias) = &item.alias else { continue };
                    if alias.text == item.name.text {
                        continue;
                    }
                    let mut path: Vec<String> = prefix.segments.iter().map(|s| s.text.clone()).collect();
                    path.push(item.name.text.clone());
                    out.insert(alias.text.clone(), path);
                }
            }
            _ => {}
        }
    }
    out
}

struct AliasRewriter {
    aliases: HashMap<String, Vec<String>>,
}

impl AliasRewriter {
    /// Replace the alias at the head of `name` with the path it names, each
    /// new segment carrying the alias's span.
    fn rewrite_head(&self, name: &mut QualifiedName) -> bool {
        let Some(head) = name.segments.first() else { return false };
        let Some(path) = self.aliases.get(&head.text) else { return false };
        let span = head.span;
        let mut segments: Vec<Ident> = path.iter().map(|t| Ident { text: t.clone(), span }).collect();
        segments.extend(name.segments.drain(1..));
        name.segments = segments;
        true
    }

    fn is_alias(&self, name: &QualifiedName) -> bool {
        name.segments.first().is_some_and(|h| self.aliases.contains_key(&h.text))
    }
}

impl TypeNameRewriter for AliasRewriter {
    fn type_ref(&mut self, ty: &mut TypeRef, scope: &Scope) {
        if ty.fn_shape.is_some() || !self.is_alias(&ty.name) {
            return;
        }
        if scope.is_generic(&ty.name.segments[0].text) {
            return;
        }
        self.rewrite_head(&mut ty.name);
    }

    fn new_object(&mut self, new: &mut NewObjectExpr, scope: &Scope) {
        if self.is_alias(&new.class_name) && !scope.is_generic(&new.class_name.segments[0].text) {
            self.rewrite_head(&mut new.class_name);
        }
    }

    fn type_name(&mut self, name: &mut QualifiedName, scope: &Scope) {
        if self.is_alias(name) && !scope.is_generic(&name.segments[0].text) {
            self.rewrite_head(name);
        }
    }

    fn expr_path(&mut self, path: &mut QualifiedName, scope: &Scope) {
        let Some(head) = path.segments.first() else { return };
        if scope.is_value(&head.text) || scope.is_generic(&head.text) {
            return;
        }
        self.rewrite_head(path);
    }

    fn type_pattern(&mut self, name: &mut Ident, scope: &Scope) {
        let head = name.text.split('.').next().unwrap_or("");
        if scope.is_generic(head) {
            return;
        }
        let Some(path) = self.aliases.get(head) else { return };
        let rest: Vec<&str> = name.text.split('.').skip(1).collect();
        let mut full = path.clone();
        full.extend(rest.into_iter().map(str::to_string));
        name.text = full.join(".");
    }
}
