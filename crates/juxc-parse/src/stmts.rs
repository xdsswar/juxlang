//! Statement parsing — blocks, control flow, var/typed locals, assignment.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{
    ArrayDim, AssignStmt, BinaryOp, Block, CatchClause, ElseBranch, Expr, ForCStmt, ForEachStmt, IfStmt,
    NewArrayLitExpr, Stmt, TryStmt, TypeRef, VarDecl, WhileStmt,
};
use juxc_diagnostics::{code, Diagnostic};
use juxc_lex::{Keyword, TokenKind};

use crate::exprs::expr_span;
use crate::Parser;

impl<'a> Parser<'a> {
    /// `block = '{' statement* '}'` per §A.2.4 / §A.2.8.
    pub(crate) fn parse_block(&mut self) -> Block {
        let start = self.peek_span();
        self.expect(&TokenKind::LBrace, "'{' to start block");

        let mut statements = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            if let Some(stmt) = self.parse_stmt() {
                statements.push(stmt);
                // A desugaring statement (tuple destructure) may have
                // queued follow-up declarations — same scope, source
                // order.
                statements.append(&mut self.pending_stmts);
            } else {
                // Recovery: skip to the next `;` or `}` so we don't loop
                // forever on a malformed statement.
                self.recover_to_stmt_boundary();
            }
        }

        let end = self.peek_span();
        self.expect(&TokenKind::RBrace, "'}' to close block");
        Block { statements, span: start.join(end) }
    }

    /// Parse a control-flow body that the grammar spells `statement`
    /// (§A.2.8 — `if`/`while`/`for` bodies): either a brace `{ … }`
    /// block, or a SINGLE braceless statement (`if (c) return;`)
    /// wrapped in a synthetic one-statement [`Block`] so every
    /// downstream consumer keeps seeing a block. Tuple-destructure
    /// desugaring queues follow-ups into `pending_stmts`, which are
    /// folded into the synthetic block in source order.
    pub(crate) fn parse_block_or_stmt(&mut self) -> Block {
        if self.at(&TokenKind::LBrace) {
            return self.parse_block();
        }
        let start = self.peek_span();
        let mut statements = Vec::new();
        if let Some(stmt) = self.parse_stmt() {
            statements.push(stmt);
            statements.append(&mut self.pending_stmts);
        } else {
            self.recover_to_stmt_boundary();
        }
        let end = self.last_consumed_span();
        Block { statements, span: start.join(end) }
    }

    /// Parse one statement. Returns `None` on unrecoverable parse failure;
    /// caller handles recovery.
    ///
    /// Currently recognized statement forms:
    ///
    /// - `return [expr] ;`
    /// - `var name = expr ;` (variable declaration with type inference)
    /// - `if (cond) block [else (if-stmt | block)]`
    /// - `while (cond) block`
    /// - `name = expr ;` (assignment to a previously-declared `var`)
    /// - `expr ;` (expression statement)
    pub(crate) fn parse_stmt(&mut self) -> Option<Stmt> {
        if self.at_kw(Keyword::Return) {
            return Some(self.parse_return_stmt());
        }
        // Leading `final` or `const` modifier on a local declaration
        // (per `JUX-LANG-V1.md` §549–565). Both forms are accepted in
        // statement position; we consume the modifier here, set the
        // `is_final` flag, and dispatch to either `parse_var_decl`
        // (when followed by `var`) or `parse_typed_local` (when
        // followed by a type name).
        if self.at_kw(Keyword::Final) || self.at_kw(Keyword::Const) {
            self.advance(); // 'final' | 'const'
            if self.at_kw(Keyword::Var) {
                return self.parse_var_decl_with(true).map(Stmt::VarDecl);
            }
            // Otherwise the declaration must take the typed form
            // `Type name [= init];`. We unconditionally dispatch
            // because no other statement form may follow a leading
            // `final`/`const` keyword.
            return self.parse_typed_local_with(true).map(Stmt::VarDecl);
        }
        // `ref` local declaration (§M.13): `ref Type name = init;` — a
        // SHARED reference to a value-typed object. `ref var` is not a
        // form: the shared object's type must be explicit.
        if self.at_kw(Keyword::Ref) {
            self.advance(); // 'ref'
            let mut vd = self.parse_typed_local_with(false)?;
            vd.is_ref = true;
            return Some(Stmt::VarDecl(vd));
        }
        // **Labeled loop** (§A.2.8): `name: while/do/for …`. Two-token
        // lookahead — a bare identifier followed by `:` followed by a
        // loop keyword. Anything else (e.g. `Type name = …;` locals,
        // ternary arms) never has this shape at statement start.
        if matches!(self.peek(), TokenKind::Ident(_))
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Colon),
            )
            && matches!(
                self.tokens.get(self.pos + 2).map(|t| &t.kind),
                Some(TokenKind::Kw(Keyword::While))
                    | Some(TokenKind::Kw(Keyword::Do))
                    | Some(TokenKind::Kw(Keyword::For)),
            )
        {
            let label = self.parse_ident()?;
            self.advance(); // ':'
            let inner = self.parse_stmt()?;
            return Some(Stmt::Labeled { label, stmt: Box::new(inner) });
        }
        if self.at_kw(Keyword::Var) {
            // Tuple destructuring — `var (q, r) = expr;` (§5.3 /
            // grammar §A.2.8 tuple-pattern). Desugars at parse time to
            //
            //     var __jux_tupN = expr;
            //     var q = __jux_tupN.0;
            //     var r = __jux_tupN.1;
            //
            // so every later phase sees ordinary declarations plus
            // tuple-element accesses. Phase 1 accepts flat identifier
            // patterns only (no nesting); `_` skips an element.
            if matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::LParen)
            ) {
                return self.parse_var_tuple_destructure();
            }
            return self.parse_var_decl().map(Stmt::VarDecl);
        }
        if self.at_kw(Keyword::If) {
            return Some(Stmt::If(self.parse_if_stmt()?));
        }
        if self.at_kw(Keyword::While) {
            return Some(Stmt::While(self.parse_while_stmt()?));
        }
        if self.at_kw(Keyword::Do) {
            // `do block while (cond);` per §A.2.8 — the body runs at
            // least once; the condition is checked AFTER each pass.
            let start = self.peek_span();
            self.advance(); // 'do'
            let body = self.parse_block_or_stmt();
            self.expect_kw(Keyword::While, "`while` after `do` block");
            self.expect(&TokenKind::LParen, "'(' before do-while condition");
            let condition = self.parse_expr()?;
            self.expect(&TokenKind::RParen, "')' after do-while condition");
            let end = self.peek_span();
            self.expect(&TokenKind::Semicolon, "';' after do-while condition");
            return Some(Stmt::DoWhile(juxc_ast::DoWhileStmt {
                body,
                condition,
                span: start.join(end),
            }));
        }
        if self.at_kw(Keyword::For) {
            // `for await (var x : stream)` (§18.6.3) commits to the
            // for-each form directly — a C-style `for await` is
            // nonsense, and `is_c_style_for`'s scan assumes the `(`
            // immediately follows `for`.
            if matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Kw(Keyword::Await)),
            ) {
                return Some(Stmt::ForEach(self.parse_for_each_stmt()?));
            }
            // Disambiguate the C-style `for (init; cond; update)` from the
            // enhanced `for (var x : iter)` by scanning the header for the
            // first top-level `;` (C-style) vs `:` (for-each).
            if self.is_c_style_for() {
                return self.parse_for_c_stmt().map(Stmt::ForC);
            }
            return Some(Stmt::ForEach(self.parse_for_each_stmt()?));
        }
        if self.at_kw(Keyword::Break) {
            let span = self.peek_span();
            self.advance(); // 'break'
            // §A.2.8: `break-stmt = 'break' identifier? ';'` — the
            // optional identifier targets an enclosing labeled loop.
            let label = if matches!(self.peek(), TokenKind::Ident(_)) {
                self.parse_ident()
            } else {
                None
            };
            self.expect(&TokenKind::Semicolon, "';' after `break`");
            return Some(Stmt::Break(label, span));
        }
        if self.at_kw(Keyword::Continue) {
            let span = self.peek_span();
            self.advance(); // 'continue'
            let label = if matches!(self.peek(), TokenKind::Ident(_)) {
                self.parse_ident()
            } else {
                None
            };
            self.expect(&TokenKind::Semicolon, "';' after `continue`");
            return Some(Stmt::Continue(label, span));
        }
        if self.at_kw(Keyword::Switch) {
            // Statement-form `switch (x) { … }` per §A.2.8. Uses the
            // same `Expr::Switch` AST shape as the expression form;
            // the distinguishing detail at the statement level is that
            // we don't require a trailing `;` after the closing `}`.
            let switch = self.parse_switch_expr()?;
            return Some(Stmt::Expr(Expr::Switch(switch)));
        }
        if self.at_kw(Keyword::Throw) {
            // `throw <expr> ;` per §X.2 — raises an exception. Phase-1
            // lowering panics with the expression's Display rendering.
            let start = self.peek_span();
            self.advance(); // 'throw'
            let value = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon, "';' after `throw` expression");
            let end = self.last_consumed_span();
            return Some(Stmt::Throw(value, start.join(end)));
        }
        if self.at_kw(Keyword::Try) {
            return Some(Stmt::Try(self.parse_try_stmt()?));
        }
        if self.at_kw(Keyword::Unsafe) {
            // `unsafe { … }` per §A.2.8 (`unsafe-stmt = 'unsafe' block`).
            // A bare `unsafe` block with no trailing `;`; the body is an
            // ordinary block whose statements may use unsafe operations.
            self.advance(); // 'unsafe'
            return Some(Stmt::Unsafe(self.parse_block()));
        }
        if self.at_kw(Keyword::Super)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Dot),
            )
        {
            // `super(args);` — parent-constructor delegation per §7.3.1.
            // Backend lifts this out of the body into the child struct's
            // literal as `__parent: Parent::new(args)`. We accept it
            // syntactically anywhere in a block today; semantic-level
            // "first-statement-only" enforcement lands later.
            //
            // `super.method(args);` is NOT this form — the `.` lookahead
            // sends it down the expression-statement path, where the
            // primary parser produces `Expr::Super` and the ordinary
            // postfix machinery handles the member call (S11: Java's
            // everyday "delegate to the parent implementation" idiom).
            let start = self.peek_span();
            self.advance(); // 'super'
            self.expect(&TokenKind::LParen, "'(' after `super`");
            let (args, arg_names) = self.parse_arg_list();
            if let Some(named) = arg_names.iter().flatten().next() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "named arguments aren't supported in `super(...)` — pass the parent-constructor arguments positionally",
                    )
                    .with_span(named.span),
                );
            }
            self.expect(&TokenKind::RParen, "')' to close super-call args");
            self.expect(&TokenKind::Semicolon, "';' after `super(...)`");
            let end = self.last_consumed_span();
            return Some(Stmt::SuperCall(args, start.join(end)));
        }
        // Typed local declaration: `Type name [= expr] ;` per §A.2.8's
        // alternative form. Detected by a 3-token lookahead so we don't
        // wrongly consume the leading identifier of an expression
        // statement like `print(x);`.
        if self.looks_like_typed_local() {
            return self.parse_typed_local().map(Stmt::VarDecl);
        }
        // **Prefix `++x` / `--x`** (§A) — desugar to `x += 1` / `x -= 1`
        // before the expression path. (Jux has no value-producing
        // increment in expression position; the statement form is what
        // the spec's C-style `for` and counter loops use.)
        if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
            let is_inc = matches!(self.peek(), TokenKind::PlusPlus);
            self.advance(); // '++' / '--'
            let target = self.parse_expr()?;
            let stmt = self.make_incdec(target, is_inc)?;
            self.expect(&TokenKind::Semicolon, "';' after `++`/`--` statement");
            return Some(stmt);
        }
        // Otherwise it's either an assignment statement or an expression
        // statement. We parse the leading expression first and then peek
        // at the next token — if it's `=` (or a compound assignment op
        // like `+=`) we promote the parsed expression to an assignment
        // target.
        let expr = self.parse_expr()?;
        if self.at(&TokenKind::Eq) {
            return self.parse_assignment_tail(expr, None);
        }
        if let Some(op) = compound_assign_op(self.peek()) {
            return self.parse_assignment_tail(expr, Some(op));
        }
        // **Postfix `x++` / `x--`** — same desugaring. The expression
        // we just parsed is the lvalue.
        if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
            let is_inc = matches!(self.peek(), TokenKind::PlusPlus);
            self.advance();
            let stmt = self.make_incdec(expr, is_inc)?;
            self.expect(&TokenKind::Semicolon, "';' after `++`/`--` statement");
            return Some(stmt);
        }
        self.expect(&TokenKind::Semicolon, "';' after expression statement");
        Some(Stmt::Expr(expr))
    }

    /// Build the desugared `target += 1` / `target -= 1` assignment for
    /// a `++` / `--` (§A). The target must be an assignable place
    /// (name / index / field); anything else is `E0200`.
    pub(crate) fn make_incdec(&mut self, target: Expr, is_inc: bool) -> Option<Stmt> {
        let span = expr_span(&target);
        let is_lvalue = matches!(&target, Expr::Path(qn) if qn.segments.len() == 1)
            || matches!(&target, Expr::Index(_) | Expr::Field(_));
        if !is_lvalue {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "`++`/`--` requires an assignable place (a name, array element, or field)",
                )
                .with_span(span),
            );
            return None;
        }
        let one = Expr::Literal(juxc_ast::Literal::Int(juxc_ast::IntLit {
            value: 1,
            kind: None,
            radix: juxc_ast::IntRadix::Decimal,
            digit_width: 1,
        }));
        Some(Stmt::Assign(AssignStmt {
            target,
            op: Some(if is_inc { BinaryOp::Add } else { BinaryOp::Sub }),
            value: one,
            span,
        }))
    }

    /// `for-each-stmt = 'for' '(' ( 'var' | type ) identifier ':' expression ')' block`
    /// per §A.2.8.
    ///
    /// **Only the for-each form is supported.** C-style `for (init;
    /// cond; update)` lands later; if a user writes the C-style shape
    /// today, this parser will try to consume the `init` part as a
    /// `Type identifier :` header and emit `E0200` at the `;` it didn't
    /// expect, which surfaces the spec gap clearly.
    /// Parse a `try { B0 } catch (T1 e1) { B1 } ... [finally { Bf }]`
    /// per spec §X.3.1. At least one `catch` or `finally` is
    /// required; the parser emits E0200 if both are absent.
    pub(crate) fn parse_try_stmt(&mut self) -> Option<TryStmt> {
        let start = self.peek_span();
        self.expect_kw(Keyword::Try, "expected `try`");
        let body = self.parse_block();
        let mut catches: Vec<CatchClause> = Vec::new();
        while self.at_kw(Keyword::Catch) {
            let c_start = self.peek_span();
            self.advance(); // 'catch'
            self.expect(&TokenKind::LParen, "'(' to start catch parameter");
            let ty = self.parse_type_ref()?;
            // Multi-catch alternatives — `catch (E1 | E2 e)` (§X.3.6).
            let mut alt_tys = Vec::new();
            while self.eat(&TokenKind::Pipe) {
                alt_tys.push(self.parse_type_ref()?);
            }
            let name = self.parse_ident()?;
            self.expect(&TokenKind::RParen, "')' to close catch parameter");
            let body = self.parse_block();
            let end = self.last_consumed_span();
            catches.push(CatchClause {
                ty,
                alt_tys,
                name,
                body,
                span: c_start.join(end),
            });
        }
        let finally = if self.at_kw(Keyword::Finally) {
            self.advance(); // 'finally'
            Some(self.parse_block())
        } else {
            None
        };
        if catches.is_empty() && finally.is_none() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "a `try` statement must have at least one `catch` clause or a `finally` block",
                )
                .with_span(start),
            );
        }
        let end = self.last_consumed_span();
        Some(TryStmt {
            body,
            catches,
            finally,
            span: start.join(end),
        })
    }

    pub(crate) fn parse_for_each_stmt(&mut self) -> Option<ForEachStmt> {
        let start = self.peek_span();
        self.advance(); // 'for'
        // `for await (var x : stream)` — the async stream form (§18.6.3).
        let is_await = self.eat_kw(Keyword::Await);
        self.expect(&TokenKind::LParen, "'(' after `for`");

        // `var IDENT :` (inferred) or `TYPE IDENT :` (explicit type).
        let var_type = if self.eat_kw(Keyword::Var) {
            None
        } else {
            // Try the typed form. parse_type_ref will fail with E0200
            // if there's no usable type token here.
            Some(self.parse_type_ref()?)
        };
        let var_name = self.parse_ident()?;
        self.expect(&TokenKind::Colon, "':' in for-each loop header");
        let iter = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "')' after for-each header");
        let body = self.parse_block_or_stmt();
        let end = self.last_consumed_span();
        Some(ForEachStmt { is_await, var_type, var_name, iter, body, span: start.join(end) })
    }

    /// Lookahead: is the `for (...)` header the C-style three-clause form
    /// (`init; cond; update`) rather than the enhanced `for (var x : iter)`?
    /// We scan from just past `for (` and report `true` if a top-level `;`
    /// (paren/bracket/brace depth 0 within the header) appears before a
    /// top-level `:`. The cursor is left untouched.
    fn is_c_style_for(&self) -> bool {
        // `self.pos` is at `for`; the `(` follows.
        let mut i = self.pos + 2; // past `for` and `(`
        let mut depth: i32 = 0;
        while let Some(tok) = self.tokens.get(i) {
            match &tok.kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    if depth == 0 {
                        return false; // closed the header without a `;`
                    }
                    depth -= 1;
                }
                TokenKind::Semicolon if depth == 0 => return true,
                TokenKind::Colon if depth == 0 => return false,
                TokenKind::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// `for ( init? ; cond? ; update? ) block` — the C-style counted loop.
    /// Each clause is optional. `init`/`update` are parsed as statements
    /// (a local decl or an assignment / expression); `cond` is a boolean.
    pub(crate) fn parse_for_c_stmt(&mut self) -> Option<ForCStmt> {
        let start = self.peek_span();
        self.advance(); // 'for'
        self.expect(&TokenKind::LParen, "'(' after `for`");

        // ---- init clause (terminated by `;`) ----
        let init: Option<Box<Stmt>> = if self.at(&TokenKind::Semicolon) {
            self.advance(); // empty init
            None
        } else {
            // A `var`/`final` or typed local decl consumes its own trailing
            // `;`; an assignment / expression init we terminate ourselves.
            if self.at_kw(Keyword::Var)
                || self.at_kw(Keyword::Final)
                || self.at_kw(Keyword::Const)
                || self.looks_like_typed_local()
            {
                let decl = self.parse_stmt()?; // consumes the `;`
                Some(Box::new(decl))
            } else {
                let expr = self.parse_expr()?;
                let s = if self.at(&TokenKind::Eq) {
                    self.parse_assignment_tail(expr, None)?
                } else if let Some(op) = compound_assign_op(self.peek()) {
                    self.parse_assignment_tail(expr, Some(op))?
                } else {
                    self.expect(&TokenKind::Semicolon, "';' after for-init");
                    Stmt::Expr(expr)
                };
                Some(Box::new(s))
            }
        };

        // ---- condition clause (terminated by `;`) ----
        let cond: Option<Expr> = if self.at(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(&TokenKind::Semicolon, "';' after for-condition");

        // ---- update clause (terminated by `)`) ----
        let update: Option<Box<Stmt>> = if self.at(&TokenKind::RParen) {
            None
        } else if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
            // Prefix `++i` / `--i` in the update clause.
            let is_inc = matches!(self.peek(), TokenKind::PlusPlus);
            self.advance();
            let target = self.parse_expr()?;
            Some(Box::new(self.make_incdec(target, is_inc)?))
        } else {
            let expr = self.parse_expr()?;
            let s = if self.at(&TokenKind::Eq) {
                self.parse_assignment_tail_no_semi(expr, None)?
            } else if let Some(op) = compound_assign_op(self.peek()) {
                self.parse_assignment_tail_no_semi(expr, Some(op))?
            } else if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
                // Postfix `i++` / `i--` — the common C-style for-update.
                let is_inc = matches!(self.peek(), TokenKind::PlusPlus);
                self.advance();
                self.make_incdec(expr, is_inc)?
            } else {
                Stmt::Expr(expr)
            };
            Some(Box::new(s))
        };
        self.expect(&TokenKind::RParen, "')' after for-update");

        let body = self.parse_block_or_stmt();
        let end = self.last_consumed_span();
        Some(ForCStmt { init, cond, update, body, span: start.join(end) })
    }

    /// `while-stmt = 'while' '(' expression ')' block` per §A.2.8.
    pub(crate) fn parse_while_stmt(&mut self) -> Option<WhileStmt> {
        let start = self.peek_span();
        self.advance(); // 'while'
        self.expect(&TokenKind::LParen, "'(' after `while`");
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "')' after `while` condition");
        let body = self.parse_block_or_stmt();
        let end = self.last_consumed_span();
        Some(WhileStmt { condition, body, span: start.join(end) })
    }

    /// We've parsed `target_expr` and we're sitting on `=` (or a compound
    /// assignment operator). Consume it, parse the RHS expression, expect
    /// a `;`, and return a `Stmt::Assign` — provided the target expression
    /// is a valid lvalue.
    ///
    /// **Compound assignment desugar:** when `compound_op` is `Some(op)`,
    /// we synthesize `target = target op rhs` at parse time. This keeps
    /// the AST minimal — the backend, resolver, and tycheck only ever
    /// need to handle one shape of assignment.
    ///
    /// **Lvalue restriction:** only single-segment paths (`name = …`).
    /// Anything else — `obj.field = …`, `arr[i] = …` — is rejected with
    /// `E0200` and the assignment is dropped (recovery continues past the
    /// `;`).
    pub(crate) fn parse_assignment_tail(
        &mut self,
        target_expr: Expr,
        compound_op: Option<BinaryOp>,
    ) -> Option<Stmt> {
        let op_span = self.peek_span();
        self.advance(); // '=' or compound assignment op
        let rhs_expr = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after assignment");

        // Validate the LHS shape. Supported lvalues:
        // - simple name (single-segment Path) — `x = …`
        // - array element (Index)              — `arr[i] = …`
        // - field access (Field)               — `this.x = …`, `obj.field = …`
        // - raw-pointer deref (`*p = …`)       — write through a pointer
        //   (§A.2.9, `unsafe`-only; the type checker gates the `*` on an
        //   `unsafe` context).
        // Anything else is rejected with E0200.
        let is_lvalue = matches!(
            &target_expr,
            Expr::Path(qn) if qn.segments.len() == 1
        ) || matches!(&target_expr, Expr::Index(_) | Expr::Field(_))
            || matches!(
                &target_expr,
                Expr::Unary(u) if u.op == juxc_ast::UnaryOp::Deref
            );
        if !is_lvalue {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "left-hand side of assignment must be a name, array element, or field",
                )
                .with_span(op_span),
            );
            return None;
        }

        // Compound assignment (`x += y`, `arr[f()] *= n`, …) keeps
        // the operator on the AssignStmt rather than rewriting to
        // `x = x op y` at parse time. This solves two things at
        // once:
        //
        // - **No double-eval.** `arr[next()] += 1` lowers directly to
        //   Rust's `arr[next()] += 1`, which evaluates the place
        //   expression exactly once per Rust's semantics. The old
        //   parse-time desugar produced
        //   `arr[next()] = arr[next()] + 1` and ran `next()` twice.
        // - **Readability.** The backend emits `+=` verbatim instead
        //   of reconstructing it from a Binary expression — what
        //   the user wrote is what they see in the rustc errors.
        let span = expr_span(&target_expr).join(self.last_consumed_span());
        Some(Stmt::Assign(AssignStmt {
            target: target_expr,
            op: compound_op,
            value: rhs_expr,
            span,
        }))
    }

    /// Like [`Self::parse_assignment_tail`] but does NOT consume a trailing
    /// `;` — used for the update clause of a C-style `for`, which is
    /// terminated by `)` instead. Same lvalue rules.
    pub(crate) fn parse_assignment_tail_no_semi(
        &mut self,
        target_expr: Expr,
        compound_op: Option<BinaryOp>,
    ) -> Option<Stmt> {
        let op_span = self.peek_span();
        self.advance(); // '=' or compound op
        let rhs_expr = self.parse_expr()?;
        let is_lvalue = matches!(
            &target_expr,
            Expr::Path(qn) if qn.segments.len() == 1
        ) || matches!(&target_expr, Expr::Index(_) | Expr::Field(_))
            || matches!(
                &target_expr,
                Expr::Unary(u) if u.op == juxc_ast::UnaryOp::Deref
            );
        if !is_lvalue {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "left-hand side of assignment must be a name, array element, or field",
                )
                .with_span(op_span),
            );
            return None;
        }
        let span = expr_span(&target_expr).join(self.last_consumed_span());
        Some(Stmt::Assign(AssignStmt {
            target: target_expr,
            op: compound_op,
            value: rhs_expr,
            span,
        }))
    }

    /// `return-stmt = 'return' expression? ';'`.
    pub(crate) fn parse_return_stmt(&mut self) -> Stmt {
        self.advance(); // 'return'
        let value = if self.at(&TokenKind::Semicolon) { None } else { self.parse_expr() };
        self.expect(&TokenKind::Semicolon, "';' after return");
        Stmt::Return(value)
    }

    /// `var name = expr ;` — the inferred-type local-decl form per §A.2.8.
    /// Equivalent to [`Self::parse_var_decl_with`] with `is_final = false`.
    pub(crate) fn parse_var_decl(&mut self) -> Option<VarDecl> {
        self.parse_var_decl_with(false)
    }

    /// `var '(' ident (',' ident)+ ')' '=' expr ';'` — tuple
    /// destructuring (§5.3). Returns the synthesized temp `var` and
    /// queues one element `var` per binder on
    /// [`crate::Parser::pending_stmts`] (drained by `parse_block`).
    /// `_` binders skip their element. Nested patterns are a Phase-1
    /// diagnostic.
    fn parse_var_tuple_destructure(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // 'var'
        self.advance(); // '('
        let mut binders: Vec<juxc_ast::Ident> = Vec::new();
        loop {
            if self.at(&TokenKind::LParen) {
                let here = self.peek_span();
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "nested tuple patterns aren't supported yet (Phase 1) — destructure the outer tuple first, then the element",
                    )
                    .with_span(here),
                );
                return None;
            }
            let ident = self.parse_ident()?;
            binders.push(ident);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "')' to close tuple pattern");
        if binders.len() < 2 {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "a tuple pattern needs at least two binders — use a plain `var name = …;` otherwise",
                )
                .with_span(start.join(self.last_consumed_span())),
            );
        }
        self.expect(&TokenKind::Eq, "'=' in tuple destructuring");
        let init = self.parse_expr();
        self.expect(&TokenKind::Semicolon, "';' after tuple destructuring");
        let end = self.last_consumed_span();
        let span = start.join(end);

        let tmp_name = format!("__jux_tup{}", self.tuple_tmp_counter);
        self.tuple_tmp_counter += 1;
        let tmp_ident = juxc_ast::Ident { text: tmp_name.clone(), span: start };
        // One element binding per non-`_` binder, reading `.N` off
        // the temp. Queued for `parse_block` to splice in after the
        // temp declaration.
        for (i, binder) in binders.iter().enumerate() {
            if binder.text == "_" {
                continue;
            }
            let elem_init = Expr::Field(juxc_ast::FieldExpr {
                object: Box::new(Expr::Path(juxc_ast::QualifiedName {
                    segments: vec![juxc_ast::Ident { text: tmp_name.clone(), span: binder.span }],
                    span: binder.span,
                })),
                field: juxc_ast::Ident { text: i.to_string(), span: binder.span },
                safe: false,
                span: binder.span,
            });
            self.pending_stmts.push(Stmt::VarDecl(VarDecl {
                name: binder.clone(),
                ty: None,
                init: Some(elem_init),
                is_final: false,
                is_ref: false,
                span: binder.span,
            }));
        }
        Some(Stmt::VarDecl(VarDecl {
            name: tmp_ident,
            ty: None,
            init,
            is_final: false,
            is_ref: false,
            span,
        }))
    }

    /// Underlying parser for `[final|const] var name = expr ;`.
    ///
    /// `is_final` reflects whether the caller already consumed a
    /// `final` or `const` modifier. The span on the returned
    /// [`VarDecl`] starts at the `var` token regardless — the
    /// modifier's span is folded in by the dispatcher when needed.
    pub(crate) fn parse_var_decl_with(&mut self, is_final: bool) -> Option<VarDecl> {
        let start = self.peek_span();
        self.advance(); // 'var'
        let name = self.parse_ident()?;
        self.expect(&TokenKind::Eq, "'=' in `var` declaration");
        let init = self.parse_expr();
        self.expect(&TokenKind::Semicolon, "';' after `var` declaration");
        let end = self.last_consumed_span();
        Some(VarDecl {
            name,
            ty: None,
            init,
            is_final,
            is_ref: false,
            span: start.join(end),
        })
    }

    /// Lookahead heuristic for typed local declarations.
    ///
    /// Matches the shape `IDENT (`[` … `]`)* IDENT (= | ;)` — a single
    /// identifier type, optionally followed by one or more array-dim
    /// brackets, then a binding name, then `=` or `;`. Examples:
    ///
    /// - `int x = 5;`              — IDENT IDENT =
    /// - `int[10] xs;`             — IDENT [ 10 ] IDENT ;
    /// - `String name = "Alice";`  — IDENT IDENT =
    ///
    /// Multi-segment dotted types (`com.example.Foo x = …;`) and generic
    /// types (`List<int> nums = …;`) don't trip the heuristic — those
    /// users can fall back to `var`.
    pub(crate) fn looks_like_typed_local(&self) -> bool {
        if !matches!(self.peek(), TokenKind::Ident(_)) {
            return false;
        }
        let mut i = self.pos + 1;
        // Optional generic args after the type name — `Box<int>`,
        // `Map<String, int>`. We balance angle brackets to skip over
        // the whole `< … >`. Comparison expressions don't reach this
        // point because the caller decides typed-local vs expression
        // before parsing; we just need enough lookahead to make the
        // *typed-local-with-generics* shape recognizable.
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Lt)) {
            i += 1;
            let mut depth: u32 = 1;
            while depth > 0 {
                match self.tokens.get(i).map(|t| &t.kind) {
                    Some(TokenKind::Lt) => depth += 1,
                    Some(TokenKind::Gt) => depth -= 1,
                    // A glued `>>` closes two nested generic lists at once
                    // (`List<List<int>> x = …`), so it counts double here.
                    Some(TokenKind::GtGt) => depth = depth.saturating_sub(2),
                    Some(TokenKind::Eof) | None => return false,
                    _ => {}
                }
                i += 1;
            }
        }
        // Optional nullable suffix `?` — `int? x = 5;`. Sits between
        // the type-name (with optional generics) and the optional
        // array shape, matching `parse_type_ref`'s ordering.
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Question)) {
            i += 1;
        }
        // Walk through optional `[ … ]` array dim segments. Bracket depth
        // tracking lets us skip past whatever's inside (size expression).
        while matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LBracket)) {
            i += 1;
            let mut depth: u32 = 1;
            while depth > 0 {
                match self.tokens.get(i).map(|t| &t.kind) {
                    Some(TokenKind::LBracket) => depth += 1,
                    Some(TokenKind::RBracket) => depth -= 1,
                    Some(TokenKind::Eof) | None => return false,
                    _ => {}
                }
                i += 1;
            }
        }
        // Optional trailing `*` raw-pointer markers — `int* p = …`,
        // `T** pp;`. The pointer suffix is the outermost type modifier, so
        // it comes after the array dims, matching `parse_type_ref`.
        while matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Star)) {
            i += 1;
        }
        // After the type, expect IDENT then `=` or `;`.
        matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Ident(_)))
            && matches!(
                self.tokens.get(i + 1).map(|t| &t.kind),
                Some(TokenKind::Eq) | Some(TokenKind::Semicolon)
            )
    }

    /// Parse a `Type name [= expr] ;` typed local declaration. The
    /// caller has confirmed via [`Self::looks_like_typed_local`] that
    /// the lookahead fits the shape — including any optional `[…]`
    /// array dimensions, which we delegate to [`Self::parse_type_ref`].
    /// Equivalent to [`Self::parse_typed_local_with`] with `is_final = false`.
    pub(crate) fn parse_typed_local(&mut self) -> Option<VarDecl> {
        self.parse_typed_local_with(false)
    }

    /// Underlying parser for `[final|const] Type name [= expr] ;`.
    /// `is_final` reflects whether the caller already consumed a
    /// `final`/`const` modifier.
    pub(crate) fn parse_typed_local_with(&mut self, is_final: bool) -> Option<VarDecl> {
        let ty_start = self.peek_span();
        let ty = self.parse_type_ref()?;
        let name = self.parse_ident()?;
        let init = if self.eat(&TokenKind::Eq) {
            // Bare `{a, b, c}` initializer (Java-style) — only valid
            // when the LHS is an array type. The fixed/dynamic flag is
            // carried into the AST so the backend emits the matching
            // Rust shape (`[…]` vs `vec![…]`). For non-array LHS, a
            // `{` here is a parse error — typed locals don't otherwise
            // start with `{`.
            if self.at(&TokenKind::LBrace) && ty.array_shape.is_some() {
                Some(self.parse_bare_array_initializer(&ty)?)
            } else {
                self.parse_expr()
            }
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon, "';' after typed local declaration");
        let end = self.last_consumed_span();
        Some(VarDecl { name, ty: Some(ty), init, is_final, is_ref: false, span: ty_start.join(end) })
    }

    /// Parse a bare `{a, b, c}` array initializer in typed-local RHS
    /// position. The `lhs_ty` provides both the element type and the
    /// fixed/dynamic shape for backend dispatch.
    ///
    /// Caller invariant: the next token is `{` and `lhs_ty.array_shape`
    /// is `Some(...)`.
    pub(crate) fn parse_bare_array_initializer(&mut self, lhs_ty: &TypeRef) -> Option<Expr> {
        let start = self.peek_span();
        self.expect(&TokenKind::LBrace, "'{' to open array initializer");
        let mut elements = Vec::new();
        if !self.at(&TokenKind::RBrace) {
            loop {
                let Some(e) = self.parse_expr() else { break };
                elements.push(e);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        let end = self.peek_span();
        self.expect(&TokenKind::RBrace, "'}' to close array initializer");

        // Peel ONE (outermost) dimension off the LHS to get the *element*
        // type. For a 1-D `int[]`/`int[N]` LHS the element is the scalar
        // (`peeled()` → `None`); for a multi-dim `int[][]` LHS the element
        // is itself an array (`int[]`), so its own `array_shape` is kept.
        let element_type = TypeRef {
            name: lhs_ty.name.clone(),
            generic_args: lhs_ty.generic_args.clone(),
            nullable: lhs_ty.nullable,
            array_shape: lhs_ty.array_shape.as_ref().and_then(|s| s.peeled()),
            fn_shape: lhs_ty.fn_shape.clone(),
            ptr_depth: 0,
            span: lhs_ty.span,
        };
        // Fixed-vs-dynamic dispatch keys off the OUTERMOST dimension —
        // the one this literal directly fills.
        let fixed = matches!(
            lhs_ty.array_shape.as_ref().map(|s| s.outer()),
            Some(ArrayDim::Fixed(_)),
        );
        Some(Expr::NewArrayLit(NewArrayLitExpr {
            element_type,
            elements,
            fixed,
            span: start.join(end),
        }))
    }

    /// `if-stmt = 'if' '(' expression ')' statement-block ('else' (if-stmt | block))?`
    /// per §A.2.8. (We require a `{}` block on each arm — single-statement
    /// arms without braces are a future extension.)
    pub(crate) fn parse_if_stmt(&mut self) -> Option<IfStmt> {
        let start = self.peek_span();
        self.advance(); // 'if'
        self.expect(&TokenKind::LParen, "'(' after `if`");
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "')' after `if` condition");
        // §A.2.8: an `if` body is a `statement` — a brace block OR a
        // single braceless statement (`if (c) return;`).
        let then_block = self.parse_block_or_stmt();

        // Optional else clause. After `else` we either nest another `if`
        // (else-if chain) or parse a block / single statement.
        let else_branch = if self.eat_kw(Keyword::Else) {
            if self.at_kw(Keyword::If) {
                let nested = self.parse_if_stmt()?;
                Some(Box::new(ElseBranch::If(nested)))
            } else {
                let block = self.parse_block_or_stmt();
                Some(Box::new(ElseBranch::Block(block)))
            }
        } else {
            None
        };

        let end = self.last_consumed_span();
        Some(IfStmt {
            condition,
            then_block,
            else_branch,
            span: start.join(end),
        })
    }

    /// Skip tokens until the next `;` (consumed) or `}` (left in place).
    /// Used to bail out of a busted statement so we can keep parsing
    /// the rest of the block.
    pub(crate) fn recover_to_stmt_boundary(&mut self) {
        while !self.at_eof() {
            match self.peek() {
                TokenKind::Semicolon => {
                    self.advance();
                    return;
                }
                TokenKind::RBrace => return,
                _ => self.advance(),
            }
        }
    }
}

/// If `kind` is a compound assignment operator (`+=`, `-=`, `*=`, `/=`,
/// `%=`), return the corresponding [`BinaryOp`] for the desugared
/// arithmetic. Plain `=` returns `None` — that one stays as straight
/// assignment.
pub(crate) fn compound_assign_op(kind: &TokenKind) -> Option<BinaryOp> {
    Some(match kind {
        TokenKind::PlusEq    => BinaryOp::Add,
        TokenKind::MinusEq   => BinaryOp::Sub,
        TokenKind::StarEq    => BinaryOp::Mul,
        TokenKind::SlashEq   => BinaryOp::Div,
        TokenKind::PercentEq => BinaryOp::Rem,
        // Bitwise / shift compound assignment (grammar §A.1).
        TokenKind::AmpEq     => BinaryOp::BitAnd,
        TokenKind::PipeEq    => BinaryOp::BitOr,
        TokenKind::CaretEq   => BinaryOp::BitXor,
        TokenKind::LtLtEq    => BinaryOp::Shl,
        TokenKind::GtGtEq    => BinaryOp::Shr,
        _ => return None,
    })
}
