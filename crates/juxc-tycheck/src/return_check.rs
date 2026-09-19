//! Return-completeness analysis (E0451) — every path through a value-returning
//! function must `return` (or `throw`); none may fall off the end.
//!
//! This mirrors Java's "missing return statement" reachability rule (JLS 14.21)
//! and is deliberately **conservative**: a construct is treated as *diverging*
//! (does not fall through) only when that is provably true. When in doubt we
//! answer "can fall through", but for the constructs that genuinely never
//! complete (`while (true)`, `for (;;)` with no `break`, a `switch` whose every
//! arm diverges) we recognise the divergence so valid code isn't falsely
//! flagged. The single public entry point [`body_can_fall_through`] returns
//! `true` when the body can reach its closing brace without returning — exactly
//! the missing-return condition for a non-void function.

use juxc_ast::{Block, ElseBranch, Expr, IfStmt, Literal, Stmt, SwitchBody};

/// True when `body` can complete normally — reach its end without a
/// `return`/`throw` on at least one path. A non-void function for which this
/// holds is missing a return (E0451).
pub fn body_can_fall_through(body: &Block) -> bool {
    body_can_fall_through_with(body, &|_| false)
}

/// [`body_can_fall_through`], with `diverging_call` saying which expression
/// statements never complete: a call to a function whose return type is
/// `never` (JUX-CORE-LIB-ADDENDUM K.4.1). The checker knows the signatures;
/// this module only sees the syntax.
pub fn body_can_fall_through_with(body: &Block, diverging_call: &dyn Fn(&Expr) -> bool) -> bool {
    !block_diverges(body, diverging_call)
}

/// True when executing `block` never falls through to the statement after it —
/// every path `return`s, `throw`s, or loops forever. A block diverges as soon
/// as one of its (reachable) statements diverges, since everything after an
/// unconditional divergence is unreachable.
fn block_diverges(block: &Block, dc: &dyn Fn(&Expr) -> bool) -> bool {
    block.statements.iter().any(|s| stmt_diverges(s, dc))
}

/// True when `stmt` definitely does not complete normally (it returns, throws,
/// or loops forever). Conservative: unknown shapes answer `false`.
fn stmt_diverges(stmt: &Stmt, dc: &dyn Fn(&Expr) -> bool) -> bool {
    match stmt {
        // An explicit `return` / `throw` is the canonical divergence.
        Stmt::Return(_, _) | Stmt::Throw(..) => true,
        // A call to a `never` function (K.4.1) does not come back.
        Stmt::Expr(e) if dc(e) => true,
        // `if` diverges only when it has an `else` AND both arms diverge —
        // otherwise the missing/short arm provides a fall-through path.
        Stmt::If(i) => if_diverges(i, dc),
        // `while (true) { … }` with no `break` never completes. Any other
        // loop may run zero times (or break out), so it can fall through.
        Stmt::While(w) => is_true_literal(&w.condition) && !block_has_break(&w.body),
        // `for (;;)` — an empty condition is "always true"; no `break` makes
        // it an infinite loop.
        Stmt::ForC(fc) => fc.cond.is_none() && !block_has_break(&fc.body),
        // `do { … } while (…)` runs its body at least once, so if the body
        // diverges, so does the statement.
        Stmt::DoWhile(d) => {
            block_diverges(&d.body, dc) || (is_true_literal(&d.condition) && !block_has_break(&d.body))
        }
        // A `switch` used as a statement diverges when EVERY arm diverges.
        // Statement-form switches over sealed types are exhaustiveness-checked
        // (E0440) elsewhere, so all-arms-diverge implies the whole switch does.
        Stmt::Expr(Expr::Switch(sw)) => {
            !sw.arms.is_empty() && sw.arms.iter().all(|a| switch_body_diverges(&a.body, dc))
        }
        // Transparent wrappers — recurse into the inner statement / block.
        // A labeled BLOCK is the exception: `break name;` inside it finishes
        // the block normally, so it diverges only when nothing leaves it.
        Stmt::Labeled { label, stmt } => match stmt.as_ref() {
            Stmt::Block(b) => block_diverges(b, dc) && !block_breaks_to(b, &label.text),
            _ => stmt_diverges(stmt, dc),
        },
        Stmt::Unsafe(b) => block_diverges(b, dc),
        // `try` diverges when a `finally` diverges, or when the try body and
        // every catch body diverge (no normal-completion path remains).
        Stmt::Try(t) => {
            if let Some(fin) = &t.finally {
                if block_diverges(fin, dc) {
                    return true;
                }
            }
            block_diverges(&t.body, dc) && t.catches.iter().all(|c| block_diverges(&c.body, dc))
        }
        // Everything else (plain expression, local decl, break, continue,
        // super-call) can complete normally.
        _ => false,
    }
}

/// An `if`/`else` diverges iff it has an `else` branch and both arms diverge.
fn if_diverges(i: &IfStmt, dc: &dyn Fn(&Expr) -> bool) -> bool {
    let then_div = block_diverges(&i.then_block, dc);
    match &i.else_branch {
        None => false,
        Some(eb) => {
            then_div
                && match eb.as_ref() {
                    ElseBranch::Block(b) => block_diverges(b, dc),
                    ElseBranch::If(inner) => if_diverges(inner, dc),
                }
        }
    }
}

/// A switch arm diverges when its body never completes normally: a `-> expr`
/// arm whose expression is itself a diverging `throw`/`switch`, or a `-> { … }`
/// block that diverges.
fn switch_body_diverges(body: &SwitchBody, dc: &dyn Fn(&Expr) -> bool) -> bool {
    match body {
        SwitchBody::Block(b) => block_diverges(b, dc),
        SwitchBody::Expr(e) => expr_diverges(e, dc),
    }
}

/// True when an expression in value position never yields (so an arm `-> e`
/// using it can't fall through): a `throw` expression, or a `switch` all of
/// whose arms diverge. Conservative: anything else answers `false`.
fn expr_diverges(e: &Expr, dc: &dyn Fn(&Expr) -> bool) -> bool {
    match e {
        Expr::Switch(sw) => {
            !sw.arms.is_empty() && sw.arms.iter().all(|a| switch_body_diverges(&a.body, dc))
        }
        other => dc(other),
    }
}

/// True when `e` is the boolean literal `true` (a constant-true loop guard).
fn is_true_literal(e: &Expr) -> bool {
    matches!(e, Expr::Literal(Literal::Bool(true)))
}

/// Does `block` contain a `break` that could exit a loop whose body is
/// `block`? A nested loop/switch captures its own `break`, so we don't descend
/// into those — a `break` there targets the inner construct, not ours.
/// (Labeled breaks to an outer loop are rare; treating the loop as non-infinite
/// when ANY break is present is the conservative, false-positive-free choice.)
fn block_has_break(block: &Block) -> bool {
    block.statements.iter().any(stmt_has_break)
}

/// True when some `break name;` anywhere inside `block` targets `name`,
/// however deeply nested (a loop or lambda in between does not stop a
/// labeled break, and a lambda cannot contain one that reaches out).
fn block_breaks_to(block: &Block, name: &str) -> bool {
    let mut found = false;
    juxc_ast::visit::for_each_node(block, &mut |n| {
        if let juxc_ast::visit::Node::Stmt(Stmt::Break(Some(l), _)) = n {
            if l.text == name {
                found = true;
            }
        }
    });
    found
}

fn stmt_has_break(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Break(..) => true,
        Stmt::If(i) => if_has_break(i),
        Stmt::Labeled { stmt, .. } => stmt_has_break(stmt),
        Stmt::Unsafe(b) => block_has_break(b),
        Stmt::Try(t) => {
            block_has_break(&t.body)
                || t.catches.iter().any(|c| block_has_break(&c.body))
                || t.finally.as_ref().is_some_and(block_has_break)
        }
        // Do NOT descend into nested loops / switches — their `break` binds to
        // them, not to the loop we're testing.
        _ => false,
    }
}

fn if_has_break(i: &IfStmt) -> bool {
    block_has_break(&i.then_block)
        || match &i.else_branch {
            None => false,
            Some(eb) => match eb.as_ref() {
                ElseBranch::Block(b) => block_has_break(b),
                ElseBranch::If(inner) => if_has_break(inner),
            },
        }
}
