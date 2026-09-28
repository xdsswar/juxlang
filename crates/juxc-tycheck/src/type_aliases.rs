//! `type` aliases are expanded once, before anything reads a name
//! (ERRATA E1XX-GAP37b).
//!
//! `type Dict<V> = HashMap<String, V>;` stands for its target. The checker
//! expanded an alias where it lowered a declared type, and nowhere else: `new
//! Dict<int>()` handed `<int>` straight to `HashMap` (`E0410`), `new Shapes()`
//! with `type Shapes = Vec<Shape>;` reached rustc as `Shapes::new()` of
//! nothing, and `class Sq implements Sh` implemented a type alias (`E0424`).
//! So every use is written back as the target, its parameters substituted, in
//! every place a unit names a type ([`juxc_ast::visit_mut`]): declarations,
//! supertypes, `new`, static calls through the alias (`Shapes.of(..)`), type
//! tests and patterns. Nothing after this pass sees an alias.
//!
//! The target's names mean what they mean where the alias is DECLARED
//! (`HashMap` there may be imported, the use site may not import it), so they
//! are qualified in that unit first, and written back as simple names only
//! where the use site reads the simple name as the same type.
//!
//! A use that cannot be expanded is left as written for the checker to
//! report: the wrong number of type arguments (`E0443`), or an alias that
//! reaches itself.

use std::collections::{HashMap, HashSet};

use juxc_ast::visit_mut::{rewrite_type_names, Scope, TypeNameRewriter};
use juxc_ast::{CompilationUnit, GenericArg, Ident, ImportSpec, NewObjectExpr, QualifiedName, TypeRef};

/// A `type` alias a program declares.
#[derive(Debug, Clone)]
struct AliasDef {
    /// Its generic parameters, in order.
    params: Vec<String>,
    /// What it stands for, its names qualified in the declaring unit.
    target: TypeRef,
}

/// Rewrite every use of a program's `type` alias in `units` as the type it
/// stands for. Stub aliases (a crate family's nested packages, a crate's own
/// renames) name foreign types the rest of the compiler already follows.
pub fn expand_type_aliases(units: &mut [CompilationUnit]) -> Vec<juxc_diagnostics::Diagnostic> {
    let mut diagnostics = Vec::new();
    let names = NameIndex::new(units);
    let mut defs: HashMap<String, AliasDef> = HashMap::new();
    for (idx, unit) in units.iter().enumerate() {
        if unit.is_external {
            continue;
        }
        let pkg = unit_package(unit);
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::TypeAlias(a) = item {
                let fqn = if pkg.is_empty() { a.name.text.clone() } else { format!("{pkg}.{}", a.name.text) };
                let params: Vec<String> = a.generic_params.iter().map(|p| p.name.text.clone()).collect();
                let mut target = a.target.clone();
                qualify_type(&mut target, &params, &names, idx);
                defs.insert(fqn, AliasDef { params, target });
            }
        }
    }
    if defs.is_empty() {
        return diagnostics;
    }
    for (idx, unit) in units.iter_mut().enumerate() {
        if unit.is_external {
            continue;
        }
        let mut r = TypeAliasRewriter { defs: &defs, names: &names, unit: idx, diagnostics: Vec::new() };
        rewrite_type_names(unit, &mut r);
        diagnostics.append(&mut r.diagnostics);
    }
    diagnostics
}

fn unit_package(unit: &CompilationUnit) -> String {
    unit.package
        .as_ref()
        .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
        .unwrap_or_default()
}

fn joined(q: &QualifiedName) -> String {
    q.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".")
}

/// Which simple names mean which types, per unit, read off the program's own
/// declarations and imports (the symbol table does not exist yet).
struct NameIndex {
    /// Package -> simple names of the types declared there.
    declared: HashMap<String, HashSet<String>>,
    /// Per unit: its package, single-type imports (bound name -> FQN), and
    /// the packages its wildcards import.
    units: Vec<(String, HashMap<String, String>, Vec<String>)>,
}

impl NameIndex {
    fn new(units: &[CompilationUnit]) -> NameIndex {
        let mut declared: HashMap<String, HashSet<String>> = HashMap::new();
        for unit in units {
            let names = declared.entry(unit_package(unit)).or_default();
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
        let per_unit = units
            .iter()
            .map(|u| {
                let mut singles = HashMap::new();
                let mut wild = Vec::new();
                for i in &u.imports {
                    match &i.spec {
                        ImportSpec::Path { name, wildcard: true, .. } => wild.push(joined(name)),
                        ImportSpec::Path { name, wildcard: false, alias } => {
                            if let Some(bind) = alias.as_ref().or(name.segments.last()) {
                                singles.insert(bind.text.clone(), joined(name));
                            }
                        }
                        ImportSpec::Items { prefix, items } => {
                            let pfx = joined(prefix);
                            for it in items {
                                let bind = it.alias.as_ref().unwrap_or(&it.name).text.clone();
                                let fqn =
                                    if pfx.is_empty() { it.name.text.clone() } else { format!("{pfx}.{}", it.name.text) };
                                singles.insert(bind, fqn);
                            }
                        }
                    }
                }
                (unit_package(u), singles, wild)
            })
            .collect();
        NameIndex { declared, units: per_unit }
    }

    fn declares(&self, pkg: &str, name: &str) -> bool {
        self.declared.get(pkg).is_some_and(|n| n.contains(name))
    }

    /// The FQN the simple `name` means in unit `unit`: its package's type, a
    /// single-type import, the one wildcard package that has it, the root
    /// package. `None` when none does (a primitive, a library name, a nested
    /// type), which leaves the name as written.
    fn resolve(&self, unit: usize, name: &str) -> Option<String> {
        let (pkg, singles, wild) = self.units.get(unit)?;
        let qualify = |p: &str| if p.is_empty() { name.to_string() } else { format!("{p}.{name}") };
        if self.declares(pkg, name) {
            return Some(qualify(pkg));
        }
        if let Some(fqn) = singles.get(name) {
            return Some(fqn.clone());
        }
        let hits: Vec<&String> = wild.iter().filter(|p| self.declares(p, name)).collect();
        if let [only] = hits.as_slice() {
            return Some(qualify(only));
        }
        if !pkg.is_empty() && self.declares("", name) {
            return Some(name.to_string());
        }
        None
    }
}

/// Write every name in `t` (other than an alias parameter) as the FQN it
/// means in unit `unit`, where there is one.
fn qualify_type(t: &mut TypeRef, params: &[String], names: &NameIndex, unit: usize) {
    if let Some(shape) = &mut t.fn_shape {
        for p in &mut shape.params {
            qualify_type(p, params, names, unit);
        }
        qualify_type(&mut shape.return_type, params, names, unit);
        return;
    }
    if let [head] = t.name.segments.as_slice() {
        if !params.contains(&head.text) {
            if let Some(fqn) = names.resolve(unit, &head.text) {
                let span = head.span;
                t.name.segments = fqn.split('.').map(|s| Ident { text: s.to_string(), span }).collect();
            }
        }
    }
    for a in &mut t.generic_args {
        if let GenericArg::Type(inner) = a {
            qualify_type(inner, params, names, unit);
        }
    }
}

/// Substitute the alias's parameters in `t`.
fn substitute(t: &TypeRef, subst: &HashMap<String, TypeRef>) -> TypeRef {
    if let [head] = t.name.segments.as_slice() {
        if let Some(arg) = subst.get(&head.text) {
            if t.generic_args.is_empty() && t.fn_shape.is_none() {
                return merge_shape(arg.clone(), t);
            }
        }
    }
    let mut out = t.clone();
    for a in &mut out.generic_args {
        if let GenericArg::Type(inner) = a {
            *inner = substitute(inner, subst);
        }
    }
    if let Some(shape) = &mut out.fn_shape {
        for p in &mut shape.params {
            *p = substitute(p, subst);
        }
        shape.return_type = substitute(&shape.return_type, subst);
    }
    out
}

/// `inner` written where `outer` stood: `outer`'s `?`, array dimensions and
/// pointer level apply on top of it (`Grid[]` with `type Grid = int[][];` is
/// `int[][][]`), and it takes `outer`'s place in the source.
fn merge_shape(mut inner: TypeRef, outer: &TypeRef) -> TypeRef {
    inner.nullable |= outer.nullable;
    if let Some(outer_dims) = &outer.array_shape {
        match &mut inner.array_shape {
            Some(shape) => {
                let mut dims = outer_dims.dims.clone();
                dims.append(&mut shape.dims);
                shape.dims = dims;
            }
            None => inner.array_shape = Some(outer_dims.clone()),
        }
    }
    inner.ptr_depth += outer.ptr_depth;
    inner.span = outer.span;
    inner
}

fn named(name: QualifiedName, generic_args: Vec<GenericArg>) -> TypeRef {
    let span = name.span;
    TypeRef { name, generic_args, nullable: false, array_shape: None, fn_shape: None, ptr_depth: 0, span }
}

struct TypeAliasRewriter<'a> {
    defs: &'a HashMap<String, AliasDef>,
    names: &'a NameIndex,
    unit: usize,
    diagnostics: Vec<juxc_diagnostics::Diagnostic>,
}

impl TypeAliasRewriter<'_> {
    /// `E0443` for a use of a generic alias with type arguments that do not
    /// match its parameters. A use with none at all is left to inference.
    fn check_arity(&mut self, t: &TypeRef) {
        let Some((fqn, def)) = self.alias_of(&t.name) else { return };
        let given = t.generic_args.len();
        if given == 0 || given == def.params.len() {
            return;
        }
        let bare = fqn.rsplit('.').next().unwrap_or(&fqn).to_string();
        let want = def.params.len();
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        let mut d = juxc_diagnostics::Diagnostic::error(
            juxc_diagnostics::code::Code::E0443_ExplicitTypeArgs,
            format!(
                "type alias `{bare}` takes {want} type argument{}, but {given} {} supplied",
                plural(want),
                if given == 1 { "was" } else { "were" },
            ),
        )
        .with_span(t.span);
        d.file = Some(t.span.file as usize);
        self.diagnostics.push(d);
    }

    /// The alias `name` names in this unit, if it names one.
    fn alias_of(&self, name: &QualifiedName) -> Option<(String, &AliasDef)> {
        let fqn = match name.segments.as_slice() {
            [] => return None,
            [head] => self.names.resolve(self.unit, &head.text)?,
            _ => joined(name),
        };
        self.defs.get(&fqn).map(|d| (fqn, d))
    }

    /// `t` with the alias at its head expanded, and the aliases the result
    /// names in turn; `None` when its head names no alias, or the use cannot
    /// be expanded (the argument count, a cycle).
    fn expand(&self, t: &TypeRef, seen: &mut Vec<String>) -> Option<TypeRef> {
        if t.fn_shape.is_some() || seen.len() > 16 {
            return None;
        }
        let (fqn, def) = self.alias_of(&t.name)?;
        if seen.contains(&fqn) {
            return None;
        }
        let args: Vec<TypeRef> = t.generic_args.iter().filter_map(|a| a.as_type().cloned()).collect();
        if args.len() != def.params.len() || args.len() != t.generic_args.len() {
            return None;
        }
        let subst: HashMap<String, TypeRef> = def.params.iter().cloned().zip(args).collect();
        let out = merge_shape(substitute(&def.target, &subst), t);
        seen.push(fqn);
        let result = if self.alias_of(&out.name).is_some() {
            // An alias of an alias; one that reaches itself stays unexpanded.
            self.expand(&out, seen)
        } else {
            Some(out)
        };
        seen.pop();
        let mut result = result?;
        self.expand_args(&mut result);
        self.unqualify(&mut result);
        Some(result)
    }

    /// Expand the aliases inside `t`'s generic arguments and function shape.
    fn expand_args(&self, t: &mut TypeRef) {
        for a in &mut t.generic_args {
            if let GenericArg::Type(inner) = a {
                if let Some(e) = self.expand(inner, &mut Vec::new()) {
                    *inner = e;
                } else {
                    self.expand_args(inner);
                }
            }
        }
        if let Some(shape) = &mut t.fn_shape {
            for p in shape.params.iter_mut().chain(std::iter::once(&mut shape.return_type)) {
                if let Some(e) = self.expand(p, &mut Vec::new()) {
                    *p = e;
                } else {
                    self.expand_args(p);
                }
            }
        }
    }

    /// Write a qualified name back as its simple name where this unit reads
    /// the simple name as that very type: the form everything downstream
    /// reads best.
    fn unqualify(&self, t: &mut TypeRef) {
        if t.name.segments.len() > 1 {
            let fqn = joined(&t.name);
            if let Some(last) = t.name.segments.last().cloned() {
                if self.names.resolve(self.unit, &last.text).as_deref() == Some(fqn.as_str()) {
                    t.name.segments = vec![last];
                }
            }
        }
        for a in &mut t.generic_args {
            if let GenericArg::Type(inner) = a {
                self.unqualify(inner);
            }
        }
        if let Some(shape) = &mut t.fn_shape {
            for p in &mut shape.params {
                self.unqualify(p);
            }
            self.unqualify(&mut shape.return_type);
        }
    }

    /// The class an alias names as a whole, with its parameters left as
    /// themselves: what `Shapes.of(..)` or `case Sh s` reach through it.
    fn class_of(&self, name: &QualifiedName) -> Option<QualifiedName> {
        let (_, def) = self.alias_of(name)?;
        let args = def
            .params
            .iter()
            .map(|p| {
                GenericArg::Type(named(
                    QualifiedName { segments: vec![Ident { text: p.clone(), span: name.span }], span: name.span },
                    Vec::new(),
                ))
            })
            .collect();
        let e = self.expand(&named(name.clone(), args), &mut Vec::new())?;
        (!e.nullable && e.array_shape.is_none() && e.fn_shape.is_none()).then_some(e.name)
    }
}

impl TypeNameRewriter for TypeAliasRewriter<'_> {
    fn type_ref(&mut self, ty: &mut TypeRef, scope: &Scope) {
        if ty.name.segments.len() == 1 && scope.is_generic(&ty.name.segments[0].text) {
            return;
        }
        self.check_arity(ty);
        if let Some(expanded) = self.expand(ty, &mut Vec::new()) {
            *ty = expanded;
        }
    }

    fn new_object(&mut self, new: &mut NewObjectExpr, scope: &Scope) {
        if new.class_name.segments.len() == 1 && scope.is_generic(&new.class_name.segments[0].text) {
            return;
        }
        let written =
            named(new.class_name.clone(), new.generic_args.iter().cloned().map(GenericArg::Type).collect());
        self.check_arity(&written);
        let Some(expanded) = self.expand(&written, &mut Vec::new()) else { return };
        // Only a class is built; anything else stays for the checker.
        if expanded.nullable || expanded.array_shape.is_some() || expanded.fn_shape.is_some() {
            return;
        }
        let Some(args) = expanded.generic_args.iter().map(|a| a.as_type().cloned()).collect::<Option<Vec<_>>>()
        else {
            return;
        };
        new.class_name = expanded.name;
        new.generic_args = args;
    }

    fn type_name(&mut self, name: &mut QualifiedName, scope: &Scope) {
        if name.segments.len() == 1 && scope.is_generic(&name.segments[0].text) {
            return;
        }
        if let Some(class) = self.class_of(name) {
            *name = class;
        }
    }

    fn expr_path(&mut self, path: &mut QualifiedName, scope: &Scope) {
        // A static member reached through an alias (`Named.origin()`): the
        // head is the alias, anything after it the member.
        let Some(head) = path.segments.first().cloned() else { return };
        if scope.is_value(&head.text) || scope.is_generic(&head.text) {
            return;
        }
        let head_name = QualifiedName { segments: vec![head.clone()], span: head.span };
        let Some(class) = self.class_of(&head_name) else { return };
        let rest: Vec<Ident> = path.segments.drain(1..).collect();
        path.segments = class.segments;
        path.segments.extend(rest);
    }

    fn type_pattern(&mut self, name: &mut Ident, scope: &Scope) {
        if scope.is_generic(&name.text) {
            return;
        }
        let qn = QualifiedName {
            segments: name.text.split('.').map(|s| Ident { text: s.to_string(), span: name.span }).collect(),
            span: name.span,
        };
        if let Some(class) = self.class_of(&qn) {
            name.text = joined(&class);
        }
    }
}
