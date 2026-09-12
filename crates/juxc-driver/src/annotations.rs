//! The compile-time annotation registry (JUX-ANNOTATIONS-ADDENDUM §A.8).
//!
//! Jux has no runtime reflection, and a compiled language should not grow one
//! just so a framework can ask "which methods carry `@Route`?". The compiler
//! already knows the answer at build time. So it writes it down: every
//! `RUNTIME`-retention annotation applied anywhere in the program becomes a
//! row in a generated `jux.meta.Registry`, which a framework iterates at
//! startup.
//!
//! The registry is generated as ORDINARY JUX SOURCE and compiled like any
//! other unit. That is the point of the design: no new runtime types, no
//! intrinsic the backend has to special-case, and the generated code is
//! something the user could have written by hand. A project with no
//! annotations gets an empty registry and pays nothing for it.
//!
//! Discovery is a separate, cheap parse of the user's sources, because the
//! generated source has to exist before the real pipeline lexes anything --
//! every token carries its file's index, so a source appended later would be
//! stamped wrong.

use juxc_ast::{Annotation, AnnotationArg, Expr, TopLevelDecl};
use juxc_source::SourceFile;
use std::collections::BTreeMap;

/// The package and path the generated registry is given.
const REGISTRY_PATH: &str = "jux.meta/Registry.jux";

/// One recorded application: an annotation, what it sits on, and its values.
struct Entry {
    annotation: String,
    /// `class`, `method`, `field`, or `function`.
    kind: &'static str,
    /// The enclosing type's name, empty for a top-level declaration.
    owner: String,
    /// The annotated declaration's own name.
    target: String,
    /// Every parameter of the annotation type paired with the value this
    /// site gives it -- written or defaulted, so a reader never has to know
    /// which were spelled out. An ARRAY-valued parameter appears once per
    /// element, which is what lets `AnnotatedItem.list` return the elements
    /// without the compiler having to choose a separator no value could
    /// contain.
    values: Vec<(String, String)>,
}

/// What a declared annotation type looks like, for resolving applications.
struct AnnotationType {
    /// Parameters in declaration order: name, and the default's VALUES if
    /// one was written. A list because an array default has as many values as
    /// it has elements -- and an empty one has none, which is the difference
    /// between `list("tags")` returning nothing and returning one empty
    /// string.
    params: Vec<(String, Option<Vec<String>>)>,
    /// True when `@Retention(RUNTIME)` was written. Only these are recorded:
    /// §A.4 makes `BINARY` the default, and a binary-retention annotation is
    /// compile-time information that no program asks about at runtime.
    runtime: bool,
}

/// Generate the `jux.meta.Registry` source for a program.
///
/// `sources` is the user's own files; the stdlib and stubs are not scanned
/// because annotations are applied in user code.
pub fn synthesize_registry(sources: &[SourceFile]) -> SourceFile {
    let units: Vec<juxc_ast::CompilationUnit> = sources
        .iter()
        .map(|s| juxc_parse::parse(&juxc_lex::lex(s).tokens).ast)
        .collect();

    let types = collect_annotation_types(&units);
    let mut entries = Vec::new();
    for unit in &units {
        collect_entries(unit, &types, &mut entries);
    }
    SourceFile::new(std::path::PathBuf::from(REGISTRY_PATH), render(&entries))
}

/// Every `annotation Name { … }` declared across the program.
fn collect_annotation_types(
    units: &[juxc_ast::CompilationUnit],
) -> BTreeMap<String, AnnotationType> {
    let mut out = BTreeMap::new();
    for unit in units {
        for item in &unit.items {
            let TopLevelDecl::Annotation(decl) = item else { continue };
            let params = decl
                .params
                .iter()
                .map(|p| (p.name.text.clone(), p.default.as_ref().and_then(literal_values)))
                .collect();
            out.insert(
                decl.name.text.clone(),
                AnnotationType {
                    params,
                    runtime: retention_is_runtime(&decl.annotations),
                },
            );
        }
    }
    out
}

/// Whether `@Retention(RUNTIME)` appears among `annotations` (§A.4).
fn retention_is_runtime(annotations: &[Annotation]) -> bool {
    annotations.iter().any(|a| {
        a.name.segments.last().is_some_and(|s| s.text == "Retention")
            && a.args.iter().any(|arg| {
                let expr = match arg {
                    AnnotationArg::Positional(e) => e,
                    AnnotationArg::Named { value, .. } => value,
                };
                matches!(expr, Expr::Path(qn)
                    if qn.segments.last().is_some_and(|s| s.text == "RUNTIME"))
            })
    })
}

/// Walk one unit's declarations, recording every runtime annotation applied.
fn collect_entries(
    unit: &juxc_ast::CompilationUnit,
    types: &BTreeMap<String, AnnotationType>,
    out: &mut Vec<Entry>,
) {
    for item in &unit.items {
        match item {
            TopLevelDecl::Function(f) => {
                record(&f.annotations, types, "function", "", &f.name.text, out);
            }
            TopLevelDecl::Class(c) => {
                record(&c.annotations, types, "class", "", &c.name.text, out);
                for m in &c.methods {
                    record(&m.annotations, types, "method", &c.name.text, &m.name.text, out);
                }
                for fld in &c.fields {
                    record(&fld.annotations, types, "field", &c.name.text, &fld.name.text, out);
                }
            }
            TopLevelDecl::Record(r) => {
                record(&r.annotations, types, "class", "", &r.name.text, out);
                for m in &r.methods {
                    record(&m.annotations, types, "method", &r.name.text, &m.name.text, out);
                }
            }
            TopLevelDecl::Enum(e) => {
                record(&e.annotations, types, "class", "", &e.name.text, out);
                for m in &e.methods {
                    record(&m.annotations, types, "method", &e.name.text, &m.name.text, out);
                }
            }
            TopLevelDecl::Interface(i) => {
                record(&i.annotations, types, "class", "", &i.name.text, out);
                for m in &i.methods {
                    record(&m.annotations, types, "method", &i.name.text, &m.name.text, out);
                }
            }
            // An annotation TYPE's own meta-annotations are not applications
            // of a runtime annotation to a program declaration.
            _ => {}
        }
    }
}

/// Record every runtime annotation in `annotations` against one declaration.
fn record(
    annotations: &[Annotation],
    types: &BTreeMap<String, AnnotationType>,
    kind: &'static str,
    owner: &str,
    target: &str,
    out: &mut Vec<Entry>,
) {
    for a in annotations {
        let Some(name) = a.name.segments.last().map(|s| s.text.clone()) else { continue };
        let Some(ty) = types.get(&name) else { continue }; // built-in, or unknown
        if !ty.runtime {
            continue;
        }
        out.push(Entry {
            annotation: name,
            kind,
            owner: owner.to_string(),
            target: target.to_string(),
            values: resolve_values(a, ty),
        });
    }
}

/// Pair every parameter of the annotation type with the value this site gives
/// it: the written argument, else the declared default, else the empty string.
///
/// Recording defaults too is what lets a reader ask for a parameter without
/// knowing whether the author spelled it out.
fn resolve_values(a: &Annotation, ty: &AnnotationType) -> Vec<(String, String)> {
    let mut written: BTreeMap<&str, &Expr> = BTreeMap::new();
    let mut positional = Vec::new();
    for arg in &a.args {
        match arg {
            AnnotationArg::Named { name, value } => {
                written.insert(name.text.as_str(), value);
            }
            AnnotationArg::Positional(e) => positional.push(e),
        }
    }

    let mut out = Vec::with_capacity(ty.params.len());
    for (i, (name, default)) in ty.params.iter().enumerate() {
        // Named wins; otherwise a positional argument fills parameters in
        // declaration order, which is what `@Cfg(linux)` relies on.
        let given = written
            .get(name.as_str())
            .copied()
            .or_else(|| positional.get(i).copied());

        // One row per value: an array contributes as many as it has
        // elements, a scalar exactly one, and an empty array none.
        let values = given
            .and_then(literal_values)
            .or_else(|| default.clone())
            .unwrap_or_else(|| vec![String::new()]);
        for value in values {
            out.push((name.clone(), value));
        }
    }
    out
}

/// The VALUES an annotation argument contributes: one for a scalar, one per
/// element for an array, none for an empty array.
///
/// `None` means the expression is not a compile-time constant at all, which
/// tycheck rejects; the default (or the empty string) stands in so the
/// registry stays well-formed either way.
fn literal_values(e: &Expr) -> Option<Vec<String>> {
    if let Expr::NewArrayLit(lit) = e {
        return Some(lit.elements.iter().filter_map(literal_text).collect());
    }
    literal_text(e).map(|t| vec![t])
}

/// The text of a single annotation value: a literal's spelling, or an enum
/// constant's name.
fn literal_text(e: &Expr) -> Option<String> {
    match e {
        Expr::Literal(juxc_ast::Literal::String(s)) => Some(s.clone()),
        Expr::Literal(juxc_ast::Literal::Int(i)) => Some(i.value.to_string()),
        Expr::Literal(juxc_ast::Literal::Float(f)) => Some(f.value.to_string()),
        Expr::Literal(juxc_ast::Literal::Bool(b)) => Some(b.to_string()),
        Expr::Literal(juxc_ast::Literal::Char(c)) => Some(c.to_string()),
        // A bare name is an enum constant (`@Target(METHOD)`); its spelling
        // is the value.
        Expr::Path(qn) => qn.segments.last().map(|s| s.text.clone()),
        // An array is handled by the caller, which records one row per
        // element. Reaching here means it was nested, which §A.5 does not
        // allow.
        Expr::NewArrayLit(_) => None,
        // A negative literal is a unary minus over one.
        Expr::Unary(u) if matches!(u.op, juxc_ast::UnaryOp::Neg) => {
            literal_text(&u.operand).map(|t| format!("-{t}"))
        }
        _ => None,
    }
}

/// Render the registry as Jux source.
fn render(entries: &[Entry]) -> String {
    let mut out = String::new();
    out.push_str(
        "// AUTO-GENERATED by juxc. DO NOT EDIT.\n\
         //\n\
         // Every `RUNTIME`-retention annotation applied in this program, recorded at\n\
         // compile time so a framework can find them at startup without reflection\n\
         // (JUX-ANNOTATIONS-ADDENDUM §A.8). Regenerated on every build.\n\
         \n\
         package jux.meta;\n\
         \n\
         import jux.std.meta.AnnotatedItem;\n\
         \n\
         public class Registry {\n",
    );

    out.push_str(
        "    /// Every recorded application, in source order.\n\
         \x20   public static Vec<AnnotatedItem> all() {\n\
         \x20       var out = new Vec<AnnotatedItem>();\n",
    );
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&rows_for(i, e, None));
    }
    out.push_str("        return out;\n    }\n\n");

    // The same rows again, each guarded by the requested name. Building them
    // here rather than filtering `all()` keeps every construct in this file
    // local to one function -- see the module note on why that matters.
    out.push_str(
        "    /// The applications of one annotation, by its bare name.\n\
         \x20   public static Vec<AnnotatedItem> annotated(String name) {\n\
         \x20       var out = new Vec<AnnotatedItem>();\n",
    );
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&rows_for(i, e, Some(&e.annotation)));
    }
    out.push_str("        return out;\n    }\n}\n");
    out
}

/// The statements that build and push one row.
///
/// `guard` wraps them in a name test, which is what `annotated` needs and
/// `all` does not.
fn rows_for(i: usize, e: &Entry, guard: Option<&str>) -> String {
    let (open, close, pad) = match guard {
        Some(name) => (
            format!("        if (name == {}) {{\n", jux_string(name)),
            "        }\n".to_string(),
            "    ",
        ),
        None => (String::new(), String::new(), ""),
    };

    let mut out = open;
    out.push_str(&format!("{pad}        var k{i} = new Vec<String>();\n"));
    out.push_str(&format!("{pad}        var v{i} = new Vec<String>();\n"));
    for (key, value) in &e.values {
        out.push_str(&format!("{pad}        k{i}.push({});\n", jux_string(key)));
        out.push_str(&format!("{pad}        v{i}.push({});\n", jux_string(value)));
    }
    out.push_str(&format!(
        "{pad}        out.push(new AnnotatedItem({}, {}, {}, {}, k{i}, v{i}));\n",
        jux_string(&e.annotation),
        jux_string(e.kind),
        jux_string(&e.owner),
        jux_string(&e.target),
    ));
    out.push_str(&close);
    out
}

/// A Jux string literal for `text`, escaped.
fn jux_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Control characters (the array separator among them) as an
            // escape, since a Jux literal cannot carry them raw.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{{{:X}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
