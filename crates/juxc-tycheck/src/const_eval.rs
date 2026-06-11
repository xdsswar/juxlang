//! Compile-time const-expression evaluation (§T.11 subset).
//!
//! Reduces an integer/bool expression to a concrete value at compile time,
//! over: literals, reads of `const`/`final` bindings (whose initializers are
//! themselves const), arithmetic / bitwise / comparison / logical operators,
//! and calls to free functions whose bodies are const-evaluable. Per grammar
//! §A.2.2 **const-evaluability is a property of the expression, not a `const fn`
//! modifier** — there is no `const fn` keyword in Jux; the evaluator simply
//! tries, and a call to a function whose body isn't const-legal yields
//! [`ConstEvalError::NonConst`] (E0841).
//!
//! The same evaluator runs in tycheck (to accept/reject a const position) and
//! in the backend (to emit the computed literal). Callers attach their own
//! position span to the returned error.
//!
//! **Deferred — generic const params.** Any expression mentioning an in-scope
//! generic const param (`<int N>`) returns [`ConstEvalError::Generic`], a
//! "defer" signal (NOT a user error): the caller keeps emitting `E0445`, since
//! `byte[N + 1]` over a generic `N` needs Rust nightly / monomorphization.

use std::collections::{HashMap, HashSet};

use juxc_ast::{BinaryOp, Block, Expr, Literal, Stmt, UnaryOp};

use crate::symbol_table::SymbolTable;

/// A reduced compile-time value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstVal {
    Int(i64),
    Bool(bool),
}

/// Why a const evaluation did not produce a value.
#[derive(Clone, Debug)]
pub enum ConstEvalError {
    /// E0841 — the expression isn't const-evaluable (heap, I/O, non-const call,
    /// field/index read, unsupported construct). Carries a human message.
    NonConst(String),
    /// E0842 — the evaluation panicked: overflow / divide-by-zero / bad shift.
    Panic(String),
    /// E0840 — the op/recursion budget was exhausted.
    LimitExceeded,
    /// NOT a user error: the expression mentions a generic const param, so the
    /// caller should defer (keep emitting E0445).
    Generic,
}

/// Resolution context shared by tycheck and the backend.
pub struct ConstCtx<'a> {
    pub symbols: &'a SymbolTable,
    /// Names of in-scope GENERIC const params (`<int N>`). Reading one →
    /// [`ConstEvalError::Generic`].
    pub generic_param_names: &'a HashSet<String>,
}

const MAX_OPS: u32 = 100_000;
const MAX_DEPTH: u32 = 64;

struct Budget {
    ops: u32,
    depth: u32,
}

/// Per-evaluation scratch: locals/params in the active call frame + a memo of
/// already-evaluated top-level const bindings.
struct Frame<'a> {
    ctx: &'a ConstCtx<'a>,
    budget: &'a mut Budget,
    locals: HashMap<String, ConstVal>,
    memo: &'a mut HashMap<String, ConstVal>,
}

/// Evaluate `expr` to an `i64`, or report why not.
pub fn eval_const_int(expr: &Expr, ctx: &ConstCtx) -> Result<i64, ConstEvalError> {
    match eval_top(expr, ctx)? {
        ConstVal::Int(i) => Ok(i),
        ConstVal::Bool(_) => Err(ConstEvalError::NonConst(
            "expected an integer constant, found a boolean".to_string(),
        )),
    }
}

/// Evaluate `expr` to a `bool`, or report why not.
pub fn eval_const_bool(expr: &Expr, ctx: &ConstCtx) -> Result<bool, ConstEvalError> {
    match eval_top(expr, ctx)? {
        ConstVal::Bool(b) => Ok(b),
        ConstVal::Int(_) => Err(ConstEvalError::NonConst(
            "expected a boolean constant, found an integer".to_string(),
        )),
    }
}

fn eval_top(expr: &Expr, ctx: &ConstCtx) -> Result<ConstVal, ConstEvalError> {
    let mut budget = Budget { ops: MAX_OPS, depth: MAX_DEPTH };
    let mut memo = HashMap::new();
    let mut frame = Frame {
        ctx,
        budget: &mut budget,
        locals: HashMap::new(),
        memo: &mut memo,
    };
    eval(expr, &mut frame)
}

fn eval(expr: &Expr, f: &mut Frame) -> Result<ConstVal, ConstEvalError> {
    if f.budget.ops == 0 {
        return Err(ConstEvalError::LimitExceeded);
    }
    f.budget.ops -= 1;

    match expr {
        Expr::Literal(Literal::Int(i)) => Ok(ConstVal::Int(i.value)),
        Expr::Literal(Literal::Bool(b)) => Ok(ConstVal::Bool(*b)),
        Expr::Literal(_) => Err(ConstEvalError::NonConst(
            "only integer and boolean literals are const-evaluable".to_string(),
        )),

        Expr::Path(qn) if qn.segments.len() == 1 => {
            let name = qn.segments[0].text.as_str();
            // Generic const param → defer to the caller's E0445.
            if f.ctx.generic_param_names.contains(name) {
                return Err(ConstEvalError::Generic);
            }
            if let Some(v) = f.locals.get(name) {
                return Ok(*v);
            }
            if let Some(v) = f.memo.get(name) {
                return Ok(*v);
            }
            // A top-level `const`/`final` binding: evaluate its initializer in a
            // FRESH frame (a const has no locals), then memoize.
            if let Some((_, sig)) = lookup_const(f.ctx.symbols, name) {
                let init = sig.init.clone();
                let v = {
                    let mut sub = Frame {
                        ctx: f.ctx,
                        budget: f.budget,
                        locals: HashMap::new(),
                        memo: f.memo,
                    };
                    eval(&init, &mut sub)?
                };
                f.memo.insert(name.to_string(), v);
                return Ok(v);
            }
            Err(ConstEvalError::NonConst(format!(
                "`{name}` is not a compile-time constant"
            )))
        }
        Expr::Path(_) => Err(ConstEvalError::NonConst(
            "qualified names are not const-evaluable in this phase".to_string(),
        )),

        Expr::Binary(b) => eval_binary(b.op, &b.left, &b.right, f),
        Expr::Unary(u) => eval_unary(u.op, &u.operand, f),

        Expr::Ternary(t) => {
            if eval_bool(&t.condition, f)? {
                eval(&t.then_branch, f)
            } else {
                eval(&t.else_branch, f)
            }
        }

        // Pass an int/bool cast through (the value is already the right kind in
        // Phase 1; a narrowing cast keeps the value — overflow checks on the
        // declared width are a tycheck concern).
        Expr::Cast(c) => eval(&c.value, f),

        Expr::Call(c) => eval_call(c, f),

        _ => Err(ConstEvalError::NonConst(
            "this expression is not const-evaluable".to_string(),
        )),
    }
}

fn eval_int(e: &Expr, f: &mut Frame) -> Result<i64, ConstEvalError> {
    match eval(e, f)? {
        ConstVal::Int(i) => Ok(i),
        ConstVal::Bool(_) => Err(ConstEvalError::NonConst(
            "expected an integer operand, found a boolean".to_string(),
        )),
    }
}

fn eval_bool(e: &Expr, f: &mut Frame) -> Result<bool, ConstEvalError> {
    match eval(e, f)? {
        ConstVal::Bool(b) => Ok(b),
        ConstVal::Int(_) => Err(ConstEvalError::NonConst(
            "expected a boolean operand, found an integer".to_string(),
        )),
    }
}

fn eval_binary(
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
    f: &mut Frame,
) -> Result<ConstVal, ConstEvalError> {
    use BinaryOp::*;
    // Short-circuit logical ops — also keeps a generic/non-const RHS from being
    // touched when the LHS already decides the result.
    match op {
        And => return Ok(ConstVal::Bool(eval_bool(left, f)? && eval_bool(right, f)?)),
        Or => return Ok(ConstVal::Bool(eval_bool(left, f)? || eval_bool(right, f)?)),
        _ => {}
    }

    // For the rest, evaluate both operands. `Generic` from either taints the
    // whole expression (so `N + 1` over a generic N stays Generic → E0445).
    let l = eval(left, f)?;
    let r = eval(right, f)?;

    // Comparisons produce bool; the operands may be int or bool (for ==/!=).
    match op {
        Eq => return Ok(ConstVal::Bool(l == r)),
        NotEq => return Ok(ConstVal::Bool(l != r)),
        _ => {}
    }

    let (li, ri) = match (l, r) {
        (ConstVal::Int(a), ConstVal::Int(b)) => (a, b),
        _ => {
            return Err(ConstEvalError::NonConst(
                "this operator requires integer operands".to_string(),
            ))
        }
    };
    let panic = |m: &str| ConstEvalError::Panic(m.to_string());
    let v = match op {
        Add => ConstVal::Int(li.checked_add(ri).ok_or_else(|| panic("arithmetic overflow"))?),
        Sub => ConstVal::Int(li.checked_sub(ri).ok_or_else(|| panic("arithmetic overflow"))?),
        Mul => ConstVal::Int(li.checked_mul(ri).ok_or_else(|| panic("arithmetic overflow"))?),
        Div => ConstVal::Int(li.checked_div(ri).ok_or_else(|| panic("divide by zero"))?),
        Rem => ConstVal::Int(li.checked_rem(ri).ok_or_else(|| panic("divide by zero"))?),
        WrapAdd => ConstVal::Int(li.wrapping_add(ri)),
        WrapSub => ConstVal::Int(li.wrapping_sub(ri)),
        WrapMul => ConstVal::Int(li.wrapping_mul(ri)),
        BitAnd => ConstVal::Int(li & ri),
        BitOr => ConstVal::Int(li | ri),
        BitXor => ConstVal::Int(li ^ ri),
        Shl | WrapShl => {
            let s: u32 = ri.try_into().map_err(|_| panic("shift amount out of range"))?;
            match op {
                Shl => ConstVal::Int(li.checked_shl(s).ok_or_else(|| panic("shift amount out of range"))?),
                _ => ConstVal::Int(li.wrapping_shl(s)),
            }
        }
        Shr | WrapShr => {
            let s: u32 = ri.try_into().map_err(|_| panic("shift amount out of range"))?;
            match op {
                Shr => ConstVal::Int(li.checked_shr(s).ok_or_else(|| panic("shift amount out of range"))?),
                _ => ConstVal::Int(li.wrapping_shr(s)),
            }
        }
        Lt => ConstVal::Bool(li < ri),
        Le => ConstVal::Bool(li <= ri),
        Gt => ConstVal::Bool(li > ri),
        Ge => ConstVal::Bool(li >= ri),
        // Eq/NotEq/And/Or handled above; the rest aren't const.
        _ => {
            return Err(ConstEvalError::NonConst(format!(
                "operator `{}` is not const-evaluable",
                op.as_rust_str()
            )))
        }
    };
    Ok(v)
}

fn eval_unary(op: UnaryOp, operand: &Expr, f: &mut Frame) -> Result<ConstVal, ConstEvalError> {
    match op {
        UnaryOp::Neg => {
            let v = eval_int(operand, f)?;
            Ok(ConstVal::Int(v.checked_neg().ok_or_else(|| {
                ConstEvalError::Panic("arithmetic overflow".to_string())
            })?))
        }
        UnaryOp::Not => Ok(ConstVal::Bool(!eval_bool(operand, f)?)),
        UnaryOp::BitNot => Ok(ConstVal::Int(!eval_int(operand, f)?)),
        _ => Err(ConstEvalError::NonConst(
            "this unary operator is not const-evaluable".to_string(),
        )),
    }
}

fn eval_call(c: &juxc_ast::CallExpr, f: &mut Frame) -> Result<ConstVal, ConstEvalError> {
    // Callee must be a bare function name.
    let name = match c.callee.as_ref() {
        Expr::Path(qn) if qn.segments.len() == 1 => qn.segments[0].text.as_str(),
        _ => {
            return Err(ConstEvalError::NonConst(
                "only a direct call to a free function is const-evaluable".to_string(),
            ))
        }
    };
    let Some((_, sig)) = f.ctx.symbols.lookup_function(name) else {
        return Err(ConstEvalError::NonConst(format!("unknown function `{name}`")));
    };
    let Some(body) = sig.body.clone() else {
        return Err(ConstEvalError::NonConst(format!(
            "call to `{name}` is not const-evaluable (no const-legal body)"
        )));
    };
    if sig.params.len() != c.args.len() {
        return Err(ConstEvalError::NonConst(format!(
            "wrong number of arguments to `{name}`"
        )));
    }

    // Recursion guard.
    if f.budget.depth == 0 {
        return Err(ConstEvalError::LimitExceeded);
    }
    f.budget.depth -= 1;

    // Evaluate args in the CALLER's frame, then bind into a fresh callee frame.
    let mut locals = HashMap::new();
    for (p, a) in sig.params.iter().zip(c.args.iter()) {
        let v = eval(a, f)?;
        locals.insert(p.name.clone(), v);
    }
    let result = {
        let mut callee = Frame {
            ctx: f.ctx,
            budget: f.budget,
            locals,
            memo: f.memo,
        };
        eval_block(&body, &mut callee)?
    };
    f.budget.depth += 1;
    result.ok_or_else(|| {
        ConstEvalError::NonConst(format!("`{name}` did not return a value"))
    })
}

/// Walk a block; `Ok(Some(v))` means a `return` fired with value `v`.
fn eval_block(block: &Block, f: &mut Frame) -> Result<Option<ConstVal>, ConstEvalError> {
    for stmt in &block.statements {
        if f.budget.ops == 0 {
            return Err(ConstEvalError::LimitExceeded);
        }
        f.budget.ops -= 1;
        match stmt {
            Stmt::Return(Some(e)) => return Ok(Some(eval(e, f)?)),
            Stmt::Return(None) => {
                return Err(ConstEvalError::NonConst(
                    "a const-evaluable function must return a value".to_string(),
                ))
            }
            Stmt::VarDecl(v) => {
                let Some(init) = &v.init else {
                    return Err(ConstEvalError::NonConst(
                        "an uninitialized local is not const-evaluable".to_string(),
                    ));
                };
                let val = eval(init, f)?;
                f.locals.insert(v.name.text.clone(), val);
            }
            Stmt::If(i) => {
                let taken = if eval_bool(&i.condition, f)? {
                    eval_block(&i.then_block, f)?
                } else {
                    match &i.else_branch {
                        None => None,
                        Some(eb) => match eb.as_ref() {
                            juxc_ast::ElseBranch::Block(b) => eval_block(b, f)?,
                            juxc_ast::ElseBranch::If(inner) => {
                                eval_if_chain(inner, f)?
                            }
                        },
                    }
                };
                if taken.is_some() {
                    return Ok(taken);
                }
            }
            _ => {
                return Err(ConstEvalError::NonConst(
                    "this statement is not const-evaluable".to_string(),
                ))
            }
        }
    }
    Ok(None)
}

/// `eval_block` for an `else if` chain (an [`juxc_ast::IfStmt`] reached through
/// an `ElseBranch::If`).
fn eval_if_chain(i: &juxc_ast::IfStmt, f: &mut Frame) -> Result<Option<ConstVal>, ConstEvalError> {
    if eval_bool(&i.condition, f)? {
        eval_block(&i.then_block, f)
    } else {
        match &i.else_branch {
            None => Ok(None),
            Some(eb) => match eb.as_ref() {
                juxc_ast::ElseBranch::Block(b) => eval_block(b, f),
                juxc_ast::ElseBranch::If(inner) => eval_if_chain(inner, f),
            },
        }
    }
}

/// Resolve a bare const NAME to `(fqn, &ConstSig)` — exact key first, then a
/// unique last-segment match.
fn lookup_const<'a>(
    symbols: &'a SymbolTable,
    name: &str,
) -> Option<(&'a str, &'a crate::symbol_table::ConstSig)> {
    if let Some((k, c)) = symbols.consts.get_key_value(name) {
        return Some((k.as_str(), c));
    }
    let suffix = format!(".{name}");
    let mut hits = symbols.consts.iter().filter(|(k, _)| k.ends_with(&suffix));
    match (hits.next(), hits.next()) {
        (Some((k, c)), None) => Some((k.as_str(), c)),
        _ => None,
    }
}
