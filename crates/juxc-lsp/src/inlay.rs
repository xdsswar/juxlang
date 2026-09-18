//! `textDocument/inlayHint`: the inferred type of a `var` local, and the
//! parameter name at each positional argument.
//!
//! Both come from what the analysis already knows. A `var` hint needs the
//! initializer's inferred type (the expression-type cache, exact span); a
//! parameter hint needs the callee's parameter list, which is the same answer
//! signature help gives (`calls::callee_overloads`). A hint that would only
//! repeat what the code already says is left out: an argument that IS the
//! parameter's name (`greet(who)` for parameter `who`), a named argument, and a
//! call whose overload the argument count does not settle.

use juxc_ast::visit::{for_each_node, Node};
use juxc_ast::{Block, Expr, Stmt, TopLevelDecl};
use juxc_source::Span;
use juxc_tycheck::Ty;
use tower_lsp::lsp_types::*;

use crate::calls::callee_overloads;
use crate::doc::Document;

/// The hints for `doc` whose position falls in `range`.
pub(crate) fn inlay_hints(doc: &Document, range: Range) -> Vec<InlayHint> {
    let text = doc.rope.to_string();
    let source = juxc_source::SourceFile::new(std::path::PathBuf::from("inlay.jux"), text.clone());
    let lexed = juxc_lex::lex(&source);
    let parsed = juxc_parse::parse(&lexed.tokens);

    let mut bodies: Vec<&Block> = Vec::new();
    collect_bodies(&parsed.ast.items, &mut bodies);

    let own_types = doc.own_expr_types();
    let type_of = |span: Span| -> Option<&Ty> {
        own_types
            .iter()
            .find(|(s, _)| s.start == span.start && s.end == span.end)
            .map(|(_, t)| t)
    };

    let mut hints: Vec<InlayHint> = Vec::new();
    for body in bodies {
        for_each_node(body, &mut |node| match node {
            Node::Stmt(Stmt::VarDecl(v)) if v.ty.is_none() => {
                let Some(init) = &v.init else { return };
                let Some(ty) = type_of(init.span()) else { return };
                if matches!(ty, Ty::Unknown) {
                    return;
                }
                hints.push(InlayHint {
                    position: doc.position_at(v.name.span.end as usize),
                    label: InlayHintLabel::String(format!(": {ty}")),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: None,
                    padding_right: None,
                    data: None,
                });
            }
            Node::Expr(Expr::Call(c)) => {
                let name_end = c.callee.span().end as usize;
                param_hints(doc, &text, name_end, &c.args, &c.arg_names, &mut hints);
            }
            Node::Expr(Expr::NewObject(n)) => {
                let name_end = n.class_name.span.end as usize;
                param_hints(doc, &text, name_end, &n.args, &n.arg_names, &mut hints);
            }
            _ => {}
        });
    }
    hints.retain(|h| range.start <= h.position && h.position <= range.end);
    hints.sort_by_key(|h| (h.position.line, h.position.character));
    hints
}

/// Every body in `items`: functions, methods and constructors, nested types
/// included.
fn collect_bodies<'a>(items: &'a [TopLevelDecl], out: &mut Vec<&'a Block>) {
    for item in items {
        match item {
            TopLevelDecl::Function(f) => out.extend(f.body.as_ref()),
            TopLevelDecl::Class(c) => {
                out.extend(c.constructors.iter().map(|ctor| &ctor.body));
                out.extend(c.methods.iter().filter_map(|m| m.body.as_ref()));
                collect_bodies(&c.nested_types, out);
            }
            TopLevelDecl::Enum(e) => out.extend(e.methods.iter().filter_map(|m| m.body.as_ref())),
            TopLevelDecl::Record(r) => out.extend(r.methods.iter().filter_map(|m| m.body.as_ref())),
            TopLevelDecl::Interface(i) => out.extend(i.methods.iter().filter_map(|m| m.body.as_ref())),
            _ => {}
        }
    }
}

/// Parameter-name hints for one call whose callee name ends at `name_end`.
fn param_hints(
    doc: &Document,
    text: &str,
    name_end: usize,
    args: &[Expr],
    arg_names: &[Option<juxc_ast::Ident>],
    hints: &mut Vec<InlayHint>,
) {
    if args.is_empty() {
        return;
    }
    // The `(` after the callee name.
    let bytes = text.as_bytes();
    let mut open = name_end;
    while open < bytes.len() && bytes[open].is_ascii_whitespace() {
        open += 1;
    }
    if bytes.get(open) != Some(&b'(') {
        return;
    }
    // The overload the argument count settles; several of that arity leave
    // the parameter names undecided, so no hints.
    let overloads = callee_overloads(doc, text, open);
    let mut fitting = overloads.iter().filter(|(_, params)| params.len() == args.len());
    let (Some((_, params)), None) = (fitting.next(), fitting.next()) else { return };

    for (i, (arg, param)) in args.iter().zip(params.iter()).enumerate() {
        if arg_names.get(i).is_some_and(|n| n.is_some()) {
            continue;
        }
        let span = arg.span();
        let written = text.get(span.start as usize..span.end as usize).unwrap_or("").trim();
        let last = written.rsplit('.').next().unwrap_or(written);
        if last.eq_ignore_ascii_case(&param.name) {
            continue;
        }
        hints.push(InlayHint {
            position: doc.position_at(span.start as usize),
            label: InlayHintLabel::String(format!("{}:", param.name)),
            kind: Some(InlayHintKind::PARAMETER),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: Some(true),
            data: None,
        });
    }
}
