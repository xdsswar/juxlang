//! Lifting anonymous classes to named ones (ERRATA E1XX-GAP39c; the plan is
//! `juxc_tycheck::anon_lift`).
//!
//! For each anonymous class the checker planned, a named class is declared in
//! the same source: the enclosing type parameters it uses, a field for each
//! captured local, and a constructor taking the superclass's arguments and
//! the captures. Its methods and initializer blocks are the anonymous class's
//! own, spans and all, so a failure inside one still names its line. The
//! `new T(..) { .. }` becomes `new __JuxAnon_T_<source>_<n><G..>(.., captures)`.
//! The program is then checked again.
//!
//! The declaration's skeleton is written as Jux text and parsed, under a
//! source index of its own so none of its spans can collide with the
//! program's.

use std::collections::HashMap;

use juxc_ast::{CompilationUnit, Expr, Stmt, TopLevelDecl};
use juxc_tycheck::anon_lift::AnonLift;
use juxc_tycheck::expand::AnonSite;

/// The first source index handed to a skeleton; far past any real source.
const SKELETON_INDEX_BASE: u32 = 0x4000_0000;

/// Whether the check found no error in the program's own sources. A stub's
/// diagnostics are dropped later (they describe a crate that compiles), so
/// they do not keep the program from being lifted.
pub(crate) fn no_user_errors(diagnostics: &[juxc_diagnostics::Diagnostic], units: &[CompilationUnit]) -> bool {
    !diagnostics.iter().any(|d| {
        matches!(d.severity, juxc_diagnostics::Severity::Error)
            && d.file.map_or(true, |f| !units.get(f).is_some_and(|u| u.is_external))
    })
}

/// Lift `lifts` into `units`. Returns whether anything was lifted (the
/// caller then checks the program again).
pub(crate) fn apply(units: &mut [CompilationUnit], lifts: &[AnonLift]) -> bool {
    if lifts.is_empty() {
        return false;
    }
    let mut counters: HashMap<usize, usize> = HashMap::new();
    let mut sites: HashMap<juxc_source::Span, AnonSite> = HashMap::new();
    let mut classes: Vec<(usize, juxc_source::Span, juxc_ast::ClassDecl)> = Vec::new();
    for (k, lift) in lifts.iter().enumerate() {
        let n = counters.entry(lift.unit).or_insert(0);
        let name = format!("{}{}_{}_{}", juxc_ast::ANON_CLASS_PREFIX, lift.target_bare, lift.unit, n);
        *n += 1;
        let Some((class, site)) = skeleton(&name, lift, SKELETON_INDEX_BASE + k as u32) else { continue };
        sites.insert(lift.span, site);
        classes.push((lift.unit, lift.span, class));
    }
    if classes.is_empty() {
        return false;
    }
    let mut bodies = juxc_tycheck::expand::lift_anonymous_classes(units, sites);
    let mut any = false;
    for (unit, span, mut class) in classes {
        let Some(body) = bodies.remove(&span) else { continue };
        class.methods.extend(body.methods);
        class.init_blocks.extend(body.init_blocks);
        if let Some(u) = units.get_mut(unit) {
            u.items.push(TopLevelDecl::Class(class));
            any = true;
        }
    }
    any
}

/// The lifted class's declaration and the parts of its `new` expression.
fn skeleton(name: &str, lift: &AnonLift, index: u32) -> Option<(juxc_ast::ClassDecl, AnonSite)> {
    let params = lift
        .generics
        .iter()
        .map(|(p, b)| if b.is_empty() { p.clone() } else { format!("{p} extends {b}") })
        .collect::<Vec<_>>()
        .join(", ");
    let args = lift.generics.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>().join(", ");
    let (decl_params, use_args) = if lift.generics.is_empty() {
        (String::new(), String::new())
    } else {
        (format!("<{params}>"), format!("<{args}>"))
    };
    let relation = if lift.target_is_class { "extends" } else { "implements" };
    let mut text = format!("class {name}{decl_params} {relation} {} {{\n", lift.target_text);
    for (c, ty) in &lift.captures {
        text.push_str(&format!("    {ty} {c};\n"));
    }
    let mut ctor_params: Vec<String> = Vec::new();
    for (i, ty) in lift.super_arg_types.iter().enumerate() {
        ctor_params.push(format!("{ty} __jux_a{i}"));
    }
    for (c, ty) in &lift.captures {
        ctor_params.push(format!("{ty} {c}"));
    }
    text.push_str(&format!("    {name}({}) {{\n", ctor_params.join(", ")));
    if lift.target_is_class {
        let supers = (0..lift.super_arg_types.len()).map(|i| format!("__jux_a{i}")).collect::<Vec<_>>().join(", ");
        text.push_str(&format!("        super({supers});\n"));
    }
    for (c, _) in &lift.captures {
        text.push_str(&format!("        this.{c} = {c};\n"));
    }
    text.push_str("    }\n}\n");
    let captures = lift.captures.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>().join(", ");
    text.push_str(&format!("void __jux_anon_site() {{ var __jux_x = new {name}{use_args}({captures}); }}\n"));

    let mut file = juxc_source::SourceFile::new(format!("<anonymous {name}>"), text);
    file.set_index(index);
    let lexed = juxc_lex::lex(&file);
    let parsed = juxc_parse::parse(&lexed.tokens);
    if parsed.diagnostics.iter().any(|d| matches!(d.severity, juxc_diagnostics::Severity::Error)) {
        return None;
    }
    let mut class = None;
    let mut site = None;
    for item in parsed.ast.items {
        match item {
            TopLevelDecl::Class(c) => class = Some(c),
            TopLevelDecl::Function(f) => {
                let stmt = f.body?.statements.into_iter().next()?;
                if let Stmt::VarDecl(v) = stmt {
                    if let Some(Expr::NewObject(n)) = v.init {
                        site = Some(AnonSite {
                            class_name: n.class_name,
                            generic_args: n.generic_args,
                            capture_args: n.args,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Some((class?, site?))
}
