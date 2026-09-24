//! Static-initializer dependency cycles (§S.4.2, `ERRATA.md` E89).
//!
//! A static field's initializer may read other static fields, and those
//! initializers may read more. When that dependency graph has a cycle there is
//! no order in which the fields can be given their values, and the Phase-1
//! lowering cannot even fail usefully: a static lowers to a `LazyLock`, and
//! re-entering a `LazyLock`'s initializer blocks forever. So
//!
//! ```text
//! class A3 { public static int X = A3.X + 1; }
//! public void main() { print(A3.X); }
//! ```
//!
//! checked clean, built, printed nothing and never exited. §S.4.2 reserved
//! `W0530` for the case and promised a partial-value read at run time; neither
//! ever existed, and an infinite hang with no diagnostic is worse than any
//! error. E89 makes a statically determinable cycle the hard error `E0497` and
//! retires `W0530`.
//!
//! **The graph.** One node per static field that has an initializer. An edge
//! from field `F` to field `G` when `F`'s initializer reads `G`, either
//! directly (`C.G`, or a bare `G` inside `C`'s own body) or inside a static
//! method the initializer calls, following those calls transitively. A
//! dependency only a runtime value can reveal, through a virtual call or a
//! function value, is not in the graph and stays §S.4.2's runtime trap.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use juxc_ast::{ClassDecl, CompilationUnit, Expr, TopLevelDecl};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

use crate::symbol_table::SymbolTable;

/// A static field, named by its class's fully qualified name and its own.
type FieldKey = (String, String);

/// A static method, named the same way.
type MethodKey = (String, String);

/// What one body reads and calls, before any transitive closure.
#[derive(Default)]
struct Direct {
    /// Static fields this body reads by name.
    fields: BTreeSet<FieldKey>,
    /// Static methods this body calls by name.
    calls: BTreeSet<MethodKey>,
}

/// Every class the program declares, indexed for this pass.
struct Program<'a> {
    /// Fully qualified class name to its declaration.
    classes: BTreeMap<String, &'a ClassDecl>,
    /// The package each class was declared in, for bare-name resolution.
    packages: HashMap<String, String>,
}

/// Report `E0497` for every cycle among static-field initializers.
///
/// Run once per workspace, after the symbol table is built, so the graph spans
/// every file the program compiles.
pub fn check_static_init_cycles(
    units: &[CompilationUnit],
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let program = collect_program(units);
    if program.classes.is_empty() {
        return;
    }

    // What each static method's body reads and calls, and the same for each
    // static field's initializer.
    let mut method_direct: BTreeMap<MethodKey, Direct> = BTreeMap::new();
    let mut field_direct: BTreeMap<FieldKey, Direct> = BTreeMap::new();
    let mut field_span: BTreeMap<FieldKey, Span> = BTreeMap::new();
    for (fqn, decl) in &program.classes {
        for m in &decl.methods {
            let is_static = m.modifiers.contains(&juxc_ast::FnModifier::Static);
            let Some(body) = m.body.as_ref().filter(|_| is_static) else { continue };
            let mut direct = Direct::default();
            juxc_ast::visit::for_each_expr(body, &mut |e| {
                note_expr(e, fqn, &program, table, &mut direct);
            });
            method_direct.insert((fqn.clone(), m.name.text.clone()), direct);
        }
        for f in &decl.fields {
            let Some(init) = f.default.as_ref().filter(|_| f.is_static) else { continue };
            let mut direct = Direct::default();
            juxc_ast::visit::for_each_expr_in(init, &mut |e| {
                note_expr(e, fqn, &program, table, &mut direct);
            });
            let key = (fqn.clone(), f.name.text.clone());
            field_span.insert(key.clone(), f.span);
            field_direct.insert(key, direct);
        }
    }

    // Close the method graph: what a method reads INCLUDING what the methods it
    // calls read. Iterated to a fixpoint rather than recursed, so two mutually
    // recursive static methods terminate instead of overflowing the stack.
    let mut method_reads: BTreeMap<MethodKey, BTreeSet<FieldKey>> = method_direct
        .iter()
        .map(|(k, d)| (k.clone(), d.fields.clone()))
        .collect();
    loop {
        let mut grew = false;
        for (key, direct) in &method_direct {
            let mut merged = method_reads[key].clone();
            for callee in &direct.calls {
                if let Some(reads) = method_reads.get(callee) {
                    merged.extend(reads.iter().cloned());
                }
            }
            if merged.len() > method_reads[key].len() {
                method_reads.insert(key.clone(), merged);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    // The field graph: a field depends on what its initializer reads directly,
    // plus everything the static methods it calls read.
    let mut edges: BTreeMap<FieldKey, BTreeSet<FieldKey>> = BTreeMap::new();
    for (key, direct) in &field_direct {
        let mut outs = direct.fields.clone();
        for callee in &direct.calls {
            if let Some(reads) = method_reads.get(callee) {
                outs.extend(reads.iter().cloned());
            }
        }
        edges.insert(key.clone(), outs);
    }

    // The standard library initializes itself and is not the user's concern:
    // the same carve-out the `W0457` cycle lint makes.
    let is_stdlib = |fqn: &str| {
        fqn.starts_with("jux.std")
            || fqn.starts_with("jux.core")
            || fqn.starts_with("core.")
            || fqn.starts_with("rust.")
    };

    // One report per cycle, at the field that closes it. `BTreeMap` iterates
    // in sorted key order, so one unchanged program names the same field on
    // every run (a `HashMap` here reorders between two runs of one build,
    // which is the determinism bug `W0457` already had once).
    let mut reported: HashSet<FieldKey> = HashSet::new();
    for key in edges.keys() {
        if is_stdlib(&key.0) || reported.contains(key) {
            continue;
        }
        let Some(cycle) = find_cycle(key, &edges) else { continue };
        if cycle.iter().any(|k| reported.contains(k)) {
            continue;
        }
        reported.extend(cycle.iter().cloned());
        let Some(span) = field_span.get(key).copied() else { continue };
        let short = |k: &FieldKey| {
            format!("{}.{}", k.0.rsplit('.').next().unwrap_or(&k.0), k.1)
        };
        let path = cycle
            .iter()
            .map(short)
            .chain(std::iter::once(short(key)))
            .collect::<Vec<_>>()
            .join(" -> ");
        diagnostics.push(
            Diagnostic::error(
                code::Code::E0497_StaticInitializerCycle,
                format!(
                    "static initializer cycle: {path} -- each of these static fields \
                     needs another's value before it has one, so no order initializes \
                     them; compute one of them in a `static {{ }}` block or a method \
                     called after startup instead (§S.4.2)",
                ),
            )
            .with_span(span),
        );
    }
}

/// The path from `start` back to itself, or `None` when `start` is not on a
/// cycle. Depth-first over the sorted edge sets, so the path printed is the
/// same on every run.
fn find_cycle(
    start: &FieldKey,
    edges: &BTreeMap<FieldKey, BTreeSet<FieldKey>>,
) -> Option<Vec<FieldKey>> {
    fn walk(
        at: &FieldKey,
        start: &FieldKey,
        edges: &BTreeMap<FieldKey, BTreeSet<FieldKey>>,
        stack: &mut Vec<FieldKey>,
        seen: &mut HashSet<FieldKey>,
    ) -> bool {
        stack.push(at.clone());
        if let Some(outs) = edges.get(at) {
            for next in outs {
                if next == start {
                    return true;
                }
                if seen.insert(next.clone()) && walk(next, start, edges, stack, seen) {
                    return true;
                }
            }
        }
        stack.pop();
        false
    }
    let mut stack: Vec<FieldKey> = Vec::new();
    let mut seen: HashSet<FieldKey> = HashSet::new();
    seen.insert(start.clone());
    walk(start, start, edges, &mut stack, &mut seen).then_some(stack)
}

/// Record what one expression reads or calls, inside the body of class `owner`.
fn note_expr(e: &Expr, owner: &str, program: &Program<'_>, table: &SymbolTable, out: &mut Direct) {
    match e {
        // `C.field` written as a member access.
        Expr::Field(f) => {
            if let Expr::Path(qn) = f.object.as_ref() {
                if let Some(class) = resolve_class(qn, owner, program, table) {
                    if has_static_field(&class, &f.field.text, program) {
                        out.fields.insert((class, f.field.text.clone()));
                    }
                }
            }
        }
        // `C.field` written as a dotted path, and a bare `field` naming one of
        // the owner's own statics.
        Expr::Path(qn) => {
            if qn.segments.is_empty() {
                return;
            }
            let last = qn.segments.len() - 1;
            if last == 0 {
                let name = &qn.segments[0].text;
                if has_static_field(owner, name, program) {
                    out.fields.insert((owner.to_string(), name.clone()));
                }
                return;
            }
            let head = juxc_ast::QualifiedName {
                segments: qn.segments[..last].to_vec(),
                span: qn.span,
            };
            if let Some(class) = resolve_class(&head, owner, program, table) {
                let name = &qn.segments[last].text;
                if has_static_field(&class, name, program) {
                    out.fields.insert((class, name.clone()));
                }
            }
        }
        // `C.method(...)`, and a bare `method(...)` naming one of the owner's.
        Expr::Call(c) => match c.callee.as_ref() {
            Expr::Field(f) => {
                if let Expr::Path(qn) = f.object.as_ref() {
                    if let Some(class) = resolve_class(qn, owner, program, table) {
                        if has_static_method(&class, &f.field.text, program) {
                            out.calls.insert((class, f.field.text.clone()));
                        }
                    }
                }
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = &qn.segments[0].text;
                if has_static_method(owner, name, program) {
                    out.calls.insert((owner.to_string(), name.clone()));
                }
            }
            _ => {}
        },
        _ => {}
    }
}

/// The fully qualified class a path names, or `None` when it names anything
/// else: a variable, a package, a type this program does not declare.
fn resolve_class(
    qn: &juxc_ast::QualifiedName,
    owner: &str,
    program: &Program<'_>,
    table: &SymbolTable,
) -> Option<String> {
    let joined: String = qn
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    if program.classes.contains_key(&joined) {
        return Some(joined);
    }
    if qn.segments.len() != 1 {
        return None;
    }
    let bare = qn.segments[0].text.as_str();
    let pkg = program.packages.get(owner).cloned().unwrap_or_default();
    let fqn = table.find_fqn_by_bare_in(bare, &pkg)?;
    program.classes.contains_key(&fqn).then_some(fqn)
}

/// Does `class` declare a static field called `name`?
fn has_static_field(class: &str, name: &str, program: &Program<'_>) -> bool {
    program
        .classes
        .get(class)
        .is_some_and(|d| d.fields.iter().any(|f| f.is_static && f.name.text == name))
}

/// Does `class` declare a static method called `name`?
fn has_static_method(class: &str, name: &str, program: &Program<'_>) -> bool {
    program
        .classes
        .get(class)
        .is_some_and(|d| {
            d.methods.iter().any(|m| {
                m.name.text == name && m.modifiers.contains(&juxc_ast::FnModifier::Static)
            })
        })
}

/// Index every class the workspace declares, static nested types included, by
/// fully qualified name.
fn collect_program(units: &[CompilationUnit]) -> Program<'_> {
    let mut classes: BTreeMap<String, &ClassDecl> = BTreeMap::new();
    let mut packages: HashMap<String, String> = HashMap::new();
    for unit in units {
        if unit.is_external {
            continue;
        }
        let pkg = unit
            .package
            .as_ref()
            .map(|p| {
                p.name
                    .segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .unwrap_or_default();
        for item in &unit.items {
            index_decl(item, &pkg, "", &mut classes, &mut packages);
        }
    }
    Program { classes, packages }
}

/// Index one declaration and, for a class, its static nested types. A nested
/// type is keyed by the lifted `Outer__Inner` name the rest of the compiler
/// uses for it.
fn index_decl<'a>(
    item: &'a TopLevelDecl,
    pkg: &str,
    prefix: &str,
    classes: &mut BTreeMap<String, &'a ClassDecl>,
    packages: &mut HashMap<String, String>,
) {
    let TopLevelDecl::Class(c) = item else { return };
    let bare = if prefix.is_empty() {
        c.name.text.clone()
    } else {
        format!("{prefix}__{}", c.name.text)
    };
    let fqn = if pkg.is_empty() {
        bare.clone()
    } else {
        format!("{pkg}.{bare}")
    };
    packages.insert(fqn.clone(), pkg.to_string());
    classes.insert(fqn, c);
    for nested in &c.nested_types {
        index_decl(nested, pkg, &bare, classes, packages);
    }
}
