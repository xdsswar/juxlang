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
///
/// Not `Copy`: a constant String expression folds to an owned `String`
/// (§T.11.7), and the strings involved are short and folded once.
#[derive(Clone, Debug, PartialEq)]
pub enum ConstVal {
    Int(i64),
    Bool(bool),
    Str(String),
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
    /// The class whose body the expression was written in, if any. A bare
    /// `NAME` inside `class Brand` means `Brand.NAME`, and two classes may
    /// each declare a `NAME`; without this the lookup can only accept a name
    /// that is unique program-wide.
    pub enclosing_class: Option<&'a str>,
    /// The package the expression was written in, `[]` for the root package.
    /// A bare `MAX` means this package's `MAX` first: two packages may each
    /// declare one (§M.16).
    pub package: &'a [String],
    /// The writing unit's bare-name → FQN map (its imports and aliases), when
    /// the caller has one. Consulted after the package, before the library.
    pub imports: Option<&'a HashMap<String, String>>,
}

impl<'a> ConstCtx<'a> {
    /// A context with no enclosing class -- for a top-level const, an array
    /// size, or any position that is not inside a class body -- in the root
    /// package with no imports.
    pub fn new(symbols: &'a SymbolTable, generic_param_names: &'a HashSet<String>) -> Self {
        Self { symbols, generic_param_names, enclosing_class: None, package: &[], imports: None }
    }

    /// The same context, placed in the unit `unit` describes: its package and
    /// its imports. `None` leaves it in the root package with no imports.
    pub fn in_unit(mut self, unit: Option<&'a crate::symbol_table::UnitContext>) -> Self {
        if let Some(unit) = unit {
            self.package = &unit.package;
            self.imports = Some(&unit.unqualified);
        }
        self
    }
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
        other => Err(ConstEvalError::NonConst(format!(
            "expected an integer constant, found {}",
            describe(&other),
        ))),
    }
}

/// Evaluate `expr` to a `bool`, or report why not.
pub fn eval_const_bool(expr: &Expr, ctx: &ConstCtx) -> Result<bool, ConstEvalError> {
    match eval_top(expr, ctx)? {
        ConstVal::Bool(b) => Ok(b),
        other => Err(ConstEvalError::NonConst(format!(
            "expected a boolean constant, found {}",
            describe(&other),
        ))),
    }
}

/// Evaluate `expr` to a compile-time `String` (§T.11.7), or report why not.
///
/// A numeric or boolean constant is accepted and rendered, so `"v" + 1` folds
/// the way it reads. Anything that would allocate or run at run time -- a
/// method call, an interpolation, a non-constant name -- is a `NonConst`.
pub fn eval_const_string(expr: &Expr, ctx: &ConstCtx) -> Result<String, ConstEvalError> {
    Ok(render(&eval_top(expr, ctx)?))
}

/// The folded value as it appears inside a string.
fn render(v: &ConstVal) -> String {
    match v {
        ConstVal::Int(i) => i.to_string(),
        ConstVal::Bool(b) => b.to_string(),
        ConstVal::Str(s) => s.clone(),
    }
}

/// A value's kind, for a diagnostic that says what was found.
fn describe(v: &ConstVal) -> &'static str {
    match v {
        ConstVal::Int(_) => "an integer",
        ConstVal::Bool(_) => "a boolean",
        ConstVal::Str(_) => "a string",
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
        // A plain string literal folds to itself (§T.11.7). An INTERPOLATED
        // one is a separate `Expr` variant and never reaches here, which is
        // right: its segments are arbitrary expressions.
        Expr::Literal(Literal::String(sl)) => Ok(ConstVal::Str(sl.clone())),
        Expr::Literal(_) => Err(ConstEvalError::NonConst(
            "only integer, boolean and string literals are const-evaluable".to_string(),
        )),

        Expr::Path(qn) if qn.segments.len() == 1 => {
            let name = qn.segments[0].text.as_str();
            // Generic const param → defer to the caller's E0445.
            if f.ctx.generic_param_names.contains(name) {
                return Err(ConstEvalError::Generic);
            }
            if let Some(v) = f.locals.get(name) {
                return Ok(v.clone());
            }
            // A top-level `const`/`final` binding: evaluate its initializer in a
            // FRESH frame (a const has no locals), in the package that declared
            // it, then memoize under its FQN. Two packages may each declare a
            // `MAX`, so neither the lookup nor the memo can be keyed by `MAX`.
            if let Some((fqn, sig)) = lookup_const(f.ctx, name) {
                let package: Vec<String> = match fqn.rsplit_once('.') {
                    Some((p, _)) => p.split('.').map(str::to_string).collect(),
                    None => Vec::new(),
                };
                return eval_in(&sig.init, f, fqn, &package, None);
            }
            // A `static const` FIELD of a class, named bare from inside the
            // class that declares it (`FULL = NAME + SUFFIX`). There is no
            // enclosing-class context here, so the name must be unambiguous
            // across the program; an ambiguous one simply does not fold, which
            // leaves behaviour exactly as it was.
            // The enclosing class first: a bare `NAME` inside `class Brand`
            // is `Brand.NAME`, even when another class also declares a `NAME`.
            if let Some(owner) = f.ctx.enclosing_class {
                if let Some(field) = lookup_static_const_field(f.ctx, owner, name) {
                    return eval_field(field, f, name);
                }
            }
            if let Some(field) = lookup_static_const_field_by_bare(f.ctx.symbols, name) {
                return eval_field(field, f, name);
            }
            Err(ConstEvalError::NonConst(format!(
                "`{name}` is not a compile-time constant"
            )))
        }
        // `Class.FIELD` -- the qualified form of the same thing. Written as a
        // path when the parser saw two plain segments.
        Expr::Path(qn) if qn.segments.len() == 2 => {
            let (owner, member) = (qn.segments[0].text.as_str(), qn.segments[1].text.as_str());
            match lookup_static_const_field(f.ctx, owner, member) {
                Some(field) => eval_field(field, f, member),
                None => Err(ConstEvalError::NonConst(format!(
                    "`{owner}.{member}` is not a compile-time constant"
                ))),
            }
        }
        Expr::Path(_) => Err(ConstEvalError::NonConst(
            "qualified names are not const-evaluable in this phase".to_string(),
        )),

        // The same `Class.FIELD`, when the parser produced a field access.
        Expr::Field(fe) => {
            let owner = match fe.object.as_ref() {
                Expr::Path(qn) if qn.segments.len() == 1 => qn.segments[0].text.as_str(),
                _ => {
                    return Err(ConstEvalError::NonConst(
                        "a field read is const-evaluable only on a class's own constants"
                            .to_string(),
                    ))
                }
            };
            let member = fe.field.text.as_str();
            match lookup_static_const_field(f.ctx, owner, member) {
                Some(field) => eval_field(field, f, member),
                None => Err(ConstEvalError::NonConst(format!(
                    "`{owner}.{member}` is not a compile-time constant"
                ))),
            }
        }

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
        // `i++` / `--i` on a local of the running evaluation (§T.11.2 allows
        // mutation of values local to it).
        Expr::IncDec(inc) => {
            let Expr::Path(qn) = inc.target.as_ref() else {
                return Err(ConstEvalError::NonConst("only a local can be incremented in a constant".to_string()));
            };
            let name = qn.segments[0].text.clone();
            let Some(ConstVal::Int(old)) = f.locals.get(&name).cloned() else {
                return Err(ConstEvalError::NonConst(format!("`{name}` is not an integer local of this evaluation")));
            };
            let new = if inc.is_inc { old.checked_add(1) } else { old.checked_sub(1) }
                .ok_or_else(|| ConstEvalError::Panic("integer overflow".to_string()))?;
            f.locals.insert(name, ConstVal::Int(new));
            Ok(ConstVal::Int(if inc.is_prefix { new } else { old }))
        }

        _ => Err(ConstEvalError::NonConst(
            "this expression is not const-evaluable".to_string(),
        )),
    }
}

fn eval_int(e: &Expr, f: &mut Frame) -> Result<i64, ConstEvalError> {
    match eval(e, f)? {
        ConstVal::Int(i) => Ok(i),
        other => Err(ConstEvalError::NonConst(format!(
            "expected an integer operand, found {}",
            describe(&other),
        ))),
    }
}

fn eval_bool(e: &Expr, f: &mut Frame) -> Result<bool, ConstEvalError> {
    match eval(e, f)? {
        ConstVal::Bool(b) => Ok(b),
        other => Err(ConstEvalError::NonConst(format!(
            "expected a boolean operand, found {}",
            describe(&other),
        ))),
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

    // **Constant String concatenation (§T.11.7).** Java's rule: `+` with a
    // constant String on either side, and any constant on the other, is itself
    // a constant. Folding it here is what lets `const String FULL = NAME +
    // SUFFIX;` stay a `&'static str` -- Rust has no `const` string `+`, so an
    // unfolded one would reach rustc as an error the Jux source cannot explain.
    if matches!(op, Add) && matches!((&l, &r), (ConstVal::Str(_), _) | (_, ConstVal::Str(_))) {
        return Ok(ConstVal::Str(format!("{}{}", render(&l), render(&r))));
    }

    let (li, ri) = match (l, r) {
        (ConstVal::Int(a), ConstVal::Int(b)) => (a, b),
        (a, b) => {
            return Err(ConstEvalError::NonConst(format!(
                "this operator requires integer operands, found {} and {}",
                describe(&a),
                describe(&b),
            )))
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
    match exec_block(block, f)? {
        Flow::Return(v) => Ok(Some(v)),
        Flow::Normal => Ok(None),
        Flow::Break | Flow::Continue => Err(ConstEvalError::NonConst(
            "`break` or `continue` outside a loop".to_string(),
        )),
    }
}

/// How a statement finished: fell through, returned, or left a loop body.
enum Flow {
    Normal,
    Return(ConstVal),
    Break,
    Continue,
}

fn exec_block(block: &Block, f: &mut Frame) -> Result<Flow, ConstEvalError> {
    for stmt in &block.statements {
        match exec_stmt(stmt, f)? {
            Flow::Normal => {}
            other => return Ok(other),
        }
    }
    Ok(Flow::Normal)
}

/// One statement of a const-evaluable body (§T.11.2): locals, assignment to a
/// local, `if`, bounded `for` / `while` / `do` loops with `break` and
/// `continue`, and `return`. Every statement costs one unit of the op budget,
/// so a loop that does not end reports E0840 instead of hanging the compiler.
fn exec_stmt(stmt: &Stmt, f: &mut Frame) -> Result<Flow, ConstEvalError> {
    if f.budget.ops == 0 {
        return Err(ConstEvalError::LimitExceeded);
    }
    f.budget.ops -= 1;
    match stmt {
        Stmt::Return(Some(e), _) => Ok(Flow::Return(eval(e, f)?)),
        Stmt::Return(None, _) => Err(ConstEvalError::NonConst(
            "a const-evaluable function must return a value".to_string(),
        )),
        Stmt::VarDecl(v) => {
            let Some(init) = &v.init else {
                return Err(ConstEvalError::NonConst(
                    "an uninitialized local is not const-evaluable".to_string(),
                ));
            };
            let val = eval(init, f)?;
            f.locals.insert(v.name.text.clone(), val);
            Ok(Flow::Normal)
        }
        Stmt::Assign(a) => {
            let Expr::Path(qn) = &a.target else {
                return Err(ConstEvalError::NonConst(
                    "a constant can only assign to its own locals".to_string(),
                ));
            };
            let name = qn.segments[0].text.clone();
            if qn.segments.len() != 1 || !f.locals.contains_key(&name) {
                return Err(ConstEvalError::NonConst(format!(
                    "`{name}` is not a local of this evaluation"
                )));
            }
            let val = match a.op {
                None => eval(&a.value, f)?,
                Some(op) => eval_binary(op, &a.target, &a.value, f)?,
            };
            f.locals.insert(name, val);
            Ok(Flow::Normal)
        }
        Stmt::Expr(e @ Expr::IncDec(_)) => {
            eval(e, f)?;
            Ok(Flow::Normal)
        }
        Stmt::Block(b) => exec_block(b, f),
        Stmt::If(i) => exec_if(i, f),
        Stmt::While(w) => {
            while eval_bool(&w.condition, f)? {
                match exec_block(&w.body, f)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Normal | Flow::Continue => {}
                }
            }
            Ok(Flow::Normal)
        }
        Stmt::DoWhile(d) => {
            loop {
                match exec_block(&d.body, f)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Normal | Flow::Continue => {}
                }
                if !eval_bool(&d.condition, f)? {
                    break;
                }
            }
            Ok(Flow::Normal)
        }
        Stmt::ForC(fc) => {
            if let Some(init) = &fc.init {
                exec_stmt(init, f)?;
            }
            loop {
                if let Some(cond) = &fc.cond {
                    if !eval_bool(cond, f)? {
                        break;
                    }
                }
                match exec_block(&fc.body, f)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Normal | Flow::Continue => {}
                }
                if let Some(update) = &fc.update {
                    exec_stmt(update, f)?;
                }
                if f.budget.ops == 0 {
                    return Err(ConstEvalError::LimitExceeded);
                }
                f.budget.ops -= 1;
            }
            Ok(Flow::Normal)
        }
        Stmt::Break(None, _) => Ok(Flow::Break),
        Stmt::Continue(None, _) => Ok(Flow::Continue),
        _ => Err(ConstEvalError::NonConst(
            "this statement is not const-evaluable".to_string(),
        )),
    }
}

/// An `if` / `else if` / `else` chain.
fn exec_if(i: &juxc_ast::IfStmt, f: &mut Frame) -> Result<Flow, ConstEvalError> {
    if eval_bool(&i.condition, f)? {
        exec_block(&i.then_block, f)
    } else {
        match &i.else_branch {
            None => Ok(Flow::Normal),
            Some(eb) => match eb.as_ref() {
                juxc_ast::ElseBranch::Block(b) => exec_block(b, f),
                juxc_ast::ElseBranch::If(inner) => exec_if(inner, f),
            },
        }
    }
}

/// Evaluate a constant's initializer in a FRESH frame (a constant has no
/// locals of its own) and memoize the result under `key`, a name that is
/// unique program-wide.
///
/// The initializer's own names mean what they mean where it was WRITTEN: in
/// `package`, with `enclosing_class`'s constants in scope. The caller's imports
/// carry over only when the initializer lives in the caller's own package; a
/// unit's imports are its own, and another package's unit is not this one.
fn eval_in(
    init: &Expr,
    f: &mut Frame,
    key: &str,
    package: &[String],
    enclosing_class: Option<&str>,
) -> Result<ConstVal, ConstEvalError> {
    if let Some(v) = f.memo.get(key) {
        return Ok(v.clone());
    }
    let ctx = ConstCtx {
        symbols: f.ctx.symbols,
        generic_param_names: f.ctx.generic_param_names,
        enclosing_class,
        package,
        imports: if package == f.ctx.package { f.ctx.imports } else { None },
    };
    let v = {
        let mut sub = Frame {
            ctx: &ctx,
            budget: &mut *f.budget,
            locals: HashMap::new(),
            memo: &mut *f.memo,
        };
        eval(init, &mut sub)?
    };
    f.memo.insert(key.to_string(), v.clone());
    Ok(v)
}

/// A `static final` / `const` field of a class, found by name.
struct ConstField<'a> {
    /// The declaring class's FQN, which also keys the memo.
    class_fqn: &'a str,
    /// The declaring class's package.
    package: &'a [String],
    init: &'a Expr,
}

/// Evaluate `field` in its declaring class, memoized as `Class::member`.
fn eval_field(field: ConstField, f: &mut Frame, member: &str) -> Result<ConstVal, ConstEvalError> {
    let key = format!("{}::{member}", field.class_fqn);
    eval_in(field.init, f, &key, field.package, Some(field.class_fqn))
}

/// `member` of the class `(fqn, sig)`, when it is a constant field with an
/// initializer.
fn const_field<'a>(
    (fqn, class): (&'a String, &'a crate::symbol_table::ClassSig),
    member: &str,
) -> Option<ConstField<'a>> {
    let field = class.fields.get(member)?;
    if !field.is_static || !field.is_final {
        return None;
    }
    Some(ConstField { class_fqn: fqn, package: &class.package, init: field.default.as_ref()? })
}

/// `name` qualified by `package`: `a.b.NAME`, or `NAME` in the root package.
fn qualify(package: &[String], name: &str) -> String {
    if package.is_empty() {
        name.to_string()
    } else {
        format!("{}.{name}", package.join("."))
    }
}

/// The constant field `Owner.member`, where `Owner` is a class named bare or in
/// full. A bare owner is looked for as the writing unit would name it: fully
/// qualified, in its own package, through its imports, and only then by last
/// segment anywhere, a user class ahead of a library one and ties broken by
/// FQN so the answer does not follow `HashMap` order.
fn lookup_static_const_field<'a>(
    ctx: &ConstCtx<'a>,
    owner: &str,
    member: &str,
) -> Option<ConstField<'a>> {
    let symbols: &'a SymbolTable = ctx.symbols;
    let classes = &symbols.classes;
    let class = classes
        .get_key_value(owner)
        .or_else(|| classes.get_key_value(&qualify(ctx.package, owner)))
        .or_else(|| {
            let fqn = ctx.imports?.get(owner)?;
            classes.get_key_value(fqn)
        })
        .or_else(|| {
            classes
                .iter()
                .filter(|(k, _)| k.rsplit('.').next().unwrap_or(k) == owner)
                .min_by(|a, b| a.1.is_external.cmp(&b.1.is_external).then_with(|| a.0.cmp(b.0)))
        })?;
    const_field(class, member)
}

/// The constant field named `bare`, when exactly one class in the program
/// declares one by that name. Ambiguity yields `None`: folding the wrong
/// constant would be worse than not folding at all.
fn lookup_static_const_field_by_bare<'a>(
    symbols: &'a SymbolTable,
    bare: &str,
) -> Option<ConstField<'a>> {
    let mut hits = symbols.classes.iter().filter_map(|class| const_field(class, bare));
    match (hits.next(), hits.next()) {
        (Some(field), None) => Some(field),
        _ => None,
    }
}

/// The one entry `hits` yields, or `None` for none or several.
fn unique_const<'a>(
    mut hits: impl Iterator<Item = (&'a String, &'a crate::symbol_table::ConstSig)>,
) -> Option<(&'a str, &'a crate::symbol_table::ConstSig)> {
    match (hits.next(), hits.next()) {
        (Some((k, c)), None) => Some((k.as_str(), c)),
        _ => None,
    }
}

/// Resolve a bare constant NAME to `(fqn, &ConstSig)` the way the writing unit
/// sees it (§M.16): its own package's constant first, then one its imports
/// name, then the library realm's (when exactly one package there declares
/// it). Past those, the root package's constant and then a unique match
/// anywhere, which is how a bare name reached across user packages before
/// (GAPS 24 keeps that open for types too).
fn lookup_const<'a>(
    ctx: &ConstCtx<'a>,
    name: &str,
) -> Option<(&'a str, &'a crate::symbol_table::ConstSig)> {
    let symbols: &'a SymbolTable = ctx.symbols;
    let consts = &symbols.consts;
    let exact = |fqn: &str| consts.get_key_value(fqn).map(|(k, c)| (k.as_str(), c));
    let suffix = format!(".{name}");
    exact(&qualify(ctx.package, name))
        .or_else(|| exact(ctx.imports?.get(name)?))
        .or_else(|| {
            unique_const(consts.iter().filter(|(k, _)| {
                k.strip_suffix(&suffix)
                    .is_some_and(crate::symbol_table::is_library_realm_package)
            }))
        })
        .or_else(|| exact(name))
        .or_else(|| unique_const(consts.iter().filter(|(k, _)| k.ends_with(&suffix))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use juxc_lex::lex;
    use juxc_parse::parse;
    use juxc_source::SourceFile;

    /// The merged table of `srcs`, one compilation unit each.
    fn table(srcs: &[&str]) -> SymbolTable {
        let units: Vec<juxc_ast::CompilationUnit> = srcs
            .iter()
            .map(|src| {
                let sf = SourceFile::new("test.jux", *src);
                let lexed = lex(&sf);
                assert!(lexed.diagnostics.is_empty());
                let parsed = parse(&lexed.tokens);
                assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
                parsed.ast
            })
            .collect();
        let mut diags = Vec::new();
        let table = crate::symbol_table::build_workspace(&units, &mut diags);
        assert!(diags.is_empty(), "{diags:?}");
        table
    }

    fn name(text: &str) -> Expr {
        Expr::Path(juxc_ast::QualifiedName {
            segments: vec![juxc_ast::Ident {
                text: text.to_string(),
                span: juxc_source::Span::DUMMY,
            }],
            span: juxc_source::Span::DUMMY,
        })
    }

    fn pkg(p: &str) -> Vec<String> {
        p.split('.').map(str::to_string).collect()
    }

    const TWO_PACKAGES: &[&str] = &[
        "package a;\npublic const int MAX = 10;\npublic const int TWICE = MAX * 2;",
        "package b;\npublic const int MAX = 7;\npublic const int TWICE = MAX * 2;",
    ];

    /// Two packages each declaring `MAX`: a bare `MAX` is the writer's own.
    #[test]
    fn a_bare_constant_is_the_writing_packages_own() {
        let symbols = table(TWO_PACKAGES);
        let none = HashSet::new();
        let (a, b) = (pkg("a"), pkg("b"));
        let in_a = ConstCtx { package: &a, ..ConstCtx::new(&symbols, &none) };
        let in_b = ConstCtx { package: &b, ..ConstCtx::new(&symbols, &none) };
        assert_eq!(eval_const_int(&name("MAX"), &in_a).ok(), Some(10));
        assert_eq!(eval_const_int(&name("MAX"), &in_b).ok(), Some(7));
        // Nowhere in particular, the name is ambiguous and does not fold.
        let root = ConstCtx::new(&symbols, &none);
        assert!(eval_const_int(&name("MAX"), &root).is_err());
    }

    /// A constant's initializer reads its OWN package's names, and one
    /// evaluation that reaches both `TWICE`s keeps them apart: the memo is
    /// keyed by FQN, not by the bare name both share.
    #[test]
    fn an_initializer_reads_its_own_package_and_the_memo_keeps_them_apart() {
        let symbols = table(TWO_PACKAGES);
        let none = HashSet::new();
        let (a, b) = (pkg("a"), pkg("b"));
        let in_a = ConstCtx { package: &a, ..ConstCtx::new(&symbols, &none) };
        let in_b = ConstCtx { package: &b, ..ConstCtx::new(&symbols, &none) };
        assert_eq!(eval_const_int(&name("TWICE"), &in_a).ok(), Some(20));
        assert_eq!(eval_const_int(&name("TWICE"), &in_b).ok(), Some(14));
    }

    /// An import names the constant when the writer's package has none.
    #[test]
    fn an_import_names_a_constant_from_another_package() {
        let symbols = table(TWO_PACKAGES);
        let none = HashSet::new();
        let c = pkg("c");
        let imports: HashMap<String, String> =
            [("MAX".to_string(), "b.MAX".to_string())].into_iter().collect();
        let in_c = ConstCtx { package: &c, imports: Some(&imports), ..ConstCtx::new(&symbols, &none) };
        assert_eq!(eval_const_int(&name("MAX"), &in_c).ok(), Some(7));
        // `TWICE` is not imported, and two packages declare it.
        assert!(eval_const_int(&name("TWICE"), &in_c).is_err());
    }

    /// Two classes' same-named constants, read in one expression, stay two
    /// values: `A.K + B.K` used to memoize both as `K`.
    #[test]
    fn two_classes_same_named_constants_are_memoized_apart() {
        let symbols = table(&[
            "class A { public static final int K = 1; }\nclass B { public static final int K = 100; }",
        ]);
        let none = HashSet::new();
        let src = "const int S = A.K + B.K;";
        let unit = table(&[src]);
        let init = unit.consts.get("S").expect("S").init.clone();
        let root = ConstCtx::new(&symbols, &none);
        assert_eq!(eval_const_int(&init, &root).ok(), Some(101));
    }
}
