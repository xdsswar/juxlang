//! Generators (JUX-MISSING-DEFS-ADDENDUM §M.2): what makes a body one, and
//! what it yields.
//!
//! A function or method whose body contains a `yield` statement is a
//! generator. A sync generator returns `Iterator<T>`, an async one
//! `Stream<T>`, and every yielded value must fit `T`. A `yield` inside a
//! lambda or an anonymous class belongs to that inner body, not to the
//! function around it, so it does not make the outer function a generator.
//! Nor does one inside a switch EXPRESSION's arm: that is Java's arm-value
//! `yield`, which Jux does not have (E0990).

use juxc_ast::visit::{for_each_node, Node};
use juxc_ast::{Block, Expr, Stmt};
use juxc_source::Span;

use crate::ty::Ty;

/// True when `body` yields: it contains a `yield` statement that is not
/// inside a nested lambda or anonymous-class body, or a switch expression.
pub fn body_yields(body: &Block) -> bool {
    let mut inner_bodies = value_switch_spans(body);
    let mut yields: Vec<Span> = Vec::new();
    for_each_node(body, &mut |node| match node {
        Node::Expr(Expr::Lambda(l)) => inner_bodies.push(l.span),
        Node::Expr(Expr::NewObject(n)) if n.anonymous_body.is_some() => inner_bodies.push(n.span),
        Node::Stmt(Stmt::Yield(_, span)) => yields.push(*span),
        _ => {}
    });
    yields.iter().any(|y| !inner_bodies.iter().any(|b| encloses(*b, *y)))
}

/// Spans of the switch EXPRESSIONS in `body`: each `switch` whose value is
/// used, as opposed to one standing as a statement of its own.
pub fn value_switch_spans(body: &Block) -> Vec<Span> {
    let mut statements: Vec<Span> = Vec::new();
    let mut all: Vec<Span> = Vec::new();
    for_each_node(body, &mut |node| match node {
        Node::Stmt(Stmt::Expr(Expr::Switch(sw))) => statements.push(sw.span),
        Node::Expr(Expr::Switch(sw)) => all.push(sw.span),
        _ => {}
    });
    all.retain(|s| !statements.contains(s));
    all
}

/// True when `outer` covers all of `inner`.
pub fn encloses(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// The element type a generator declared to return `ret` yields: `T` of
/// `Iterator<T>`, or of `Stream<T>` for an async generator. `None` when the
/// declared type is neither, which is the E0996 case.
pub fn element_type(ret: &Ty, is_async: bool) -> Option<Ty> {
    let Ty::User { name, generic_args } = ret else { return None };
    let bare = name.rsplit('.').next().unwrap_or(name);
    let wanted = if is_async { "Stream" } else { "Iterator" };
    if bare != wanted || generic_args.len() != 1 {
        return None;
    }
    Some(generic_args[0].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(name: &str, args: Vec<Ty>) -> Ty {
        Ty::User { name: name.to_string(), generic_args: args }
    }

    #[test]
    fn element_type_reads_iterator_and_stream() {
        let int = Ty::Primitive(crate::ty::Primitive::Int);
        let it = user("jux.std.collections.Iterator", vec![int.clone()]);
        assert!(element_type(&it, false).is_some());
        assert!(element_type(&it, true).is_none());
        let st = user("Stream", vec![int.clone()]);
        assert!(element_type(&st, true).is_some());
        assert!(element_type(&user("Vec", vec![int]), false).is_none());
    }
}
