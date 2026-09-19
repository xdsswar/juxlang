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
            // An empty statement between statements (`f();;`) runs nothing
            // and needs no node.
            if self.eat(&TokenKind::Semicolon) {
                continue;
            }
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
    /// Parse one statement.
    ///
    /// Guards the recursion depth (E0201) and delegates to
    /// [`Self::parse_stmt_inner`]. See [`crate::MAX_NESTING`].
    pub(crate) fn parse_stmt(&mut self) -> Option<Stmt> {
        if !self.enter_nesting() {
            return None;
        }
        let out = self.parse_stmt_inner();
        self.leave_nesting();
        out
    }

    pub(crate) fn parse_stmt_inner(&mut self) -> Option<Stmt> {
        // **A type declared inside a function body** (E0993, M.9.2). Reported
        // once, with the alternatives, and the whole declaration is skipped so
        // the rest of the body still parses. Left to the expression parser it
        // became a cascade that ended by splitting the function in two.
        if let Some(kind) = self.local_type_declaration_ahead() {
            let start = self.peek_span();
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0993_LocalTypeDeclaration,
                    format!(
                        "a {kind} cannot be declared inside a function body -- declare it as a \
                         nested type of the enclosing class, or at the top level, or use a \
                         lambda if all it holds is behaviour (M.9.2)"
                    ),
                )
                .with_span(start),
            );
            while !self.at(&TokenKind::LBrace) && !self.at(&TokenKind::Semicolon) && !self.at_eof() {
                self.advance();
            }
            self.skip_balanced_braces();
            self.eat(&TokenKind::Semicolon);
            return Some(Stmt::Block(juxc_ast::Block {
                statements: Vec::new(),
                span: start.join(self.last_consumed_span()),
            }));
        }
        if self.at_kw(Keyword::Return) {
            return Some(self.parse_return_stmt());
        }
        // `yield expr;` and `yield* iter;` (JUX-MISSING-DEFS-ADDENDUM §M.2.3):
        // the statements that make a function a generator.
        if self.at_kw(Keyword::Yield) {
            return self.parse_yield_stmt();
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
                if self.at_record_destructure() {
                    return self.parse_var_record_destructure(true);
                }
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
        // **Labeled loop or block** (§A.2.8): `name: while/do/for …` or
        // `name: { … }`, which `break name;` leaves. Two-token lookahead: a
        // bare identifier followed by `:` followed by a loop keyword or `{`.
        // Anything else (e.g. `Type name = …;` locals, ternary arms) never
        // has this shape at statement start.
        if matches!(self.peek(), TokenKind::Ident(_))
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Colon),
            )
            && matches!(
                self.tokens.get(self.pos + 2).map(|t| &t.kind),
                Some(TokenKind::Kw(Keyword::While))
                    | Some(TokenKind::Kw(Keyword::Do))
                    | Some(TokenKind::Kw(Keyword::For))
                    | Some(TokenKind::LBrace),
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
            if self.at_record_destructure() {
                return self.parse_var_record_destructure(false);
            }
            return self.parse_var_decl().map(Stmt::VarDecl);
        }
        if self.at_kw(Keyword::If) {
            // `if cfg(...)` -- no parentheses around the condition, which is
            // what tells it apart from an ordinary `if (cfg(x))`.
            if matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Ident(n)) if n == "cfg"
            ) {
                return Some(Stmt::IfCfg(self.parse_if_cfg_stmt()));
            }
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
        // A bare `{ … }` in statement position: a nested scope, per grammar
        // A.2.8 (`statement = block`). It has to come before the
        // expression-statement fallback, which would otherwise try to read the
        // brace as the start of an expression and report "expected expression"
        // at a place the program is perfectly well formed.
        if matches!(self.peek(), TokenKind::LBrace) {
            return Some(Stmt::Block(self.parse_block()));
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
                        "named arguments aren't supported in `super(...)` -- pass the parent-constructor arguments positionally",
                    )
                    .with_span(named.span),
                );
            }
            self.expect(&TokenKind::RParen, "')' to close super-call args");
            self.expect(&TokenKind::Semicolon, "';' after `super(...)`");
            let end = self.last_consumed_span();
            return Some(Stmt::SuperCall(args, start.join(end)));
        }
        // **`delete <expr>;` guidance** (§L.7-L.8). Jux has NO `delete`
        // keyword: memory is freed by calling the foreign deallocator
        // inside `unsafe`, idiomatically from a `drop { }` destructor. But
        // `delete p;` is the shape `Ident Ident ;`, which would otherwise
        // be swallowed by `looks_like_typed_local` below — reading `delete`
        // as a *type* and yielding a baffling `E0304 cannot find type
        // 'delete'`. We intercept it FIRST and emit a precise pointer to the
        // drop-block model. The trigger requires a second *operand* after
        // `delete` (another ident, `this`, or `*p`); two operands in a row
        // is never a valid expression statement, so this never steals a
        // legitimate use of `delete` as an identifier (`delete(x)` call,
        // `delete = v` assign, `delete.run()` member, bare `delete;`).
        if matches!(self.peek(), TokenKind::Ident(s) if s == "delete")
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Ident(_))
                    | Some(TokenKind::Kw(Keyword::This))
                    | Some(TokenKind::Star),
            )
        {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0507_NoDeleteKeyword,
                    "`delete` is not a keyword in Jux; free memory from a `drop { }` destructor by calling the foreign deallocator (`free`) inside `unsafe` (see §L.7-L.8)",
                )
                .with_span(self.peek_span())
                .with_help(
                    "free memory by calling the foreign deallocator (`free`, or a C++ `delete` wrapper) inside an `unsafe` block, idiomatically from the owning class's `drop { }` destructor (see §L.7-L.8)",
                ),
            );
            // Return `None`; the caller's `recover_to_stmt_boundary` consumes
            // through the `;` so parsing resumes cleanly with no cascade.
            return None;
        }
        // The empty statement `;` (§A.2.8): nothing to run. It comes back as
        // an empty block so a braceless body (`while (next());`) still has
        // one; `parse_block` drops the ones that sit between statements.
        if self.at(&TokenKind::Semicolon) {
            let span = self.peek_span();
            self.advance(); // ';'
            return Some(Stmt::Block(Block { statements: Vec::new(), span }));
        }
        // `assert cond;` / `assert cond : message;` (§S.7.2): the statement
        // spelling of the built-in, parsed into the same call as
        // `assert(cond)` / `assert(cond, message)`.
        if self.at_assert_statement() {
            return self.parse_assert_stmt();
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
        // **Postfix `x++` / `x--` as a whole STATEMENT** — desugar to the
        // value-less `x += 1` / `x -= 1` (the historical statement form;
        // §A `incdec`). Since the expression grammar now also parses a
        // trailing postfix `++`/`--` into an `Expr::IncDec` (value form,
        // §A `incdec` N3), `parse_expr` above has already consumed the
        // operator and handed us that node. When such a node is the
        // ENTIRE statement, its value is discarded — so we unwrap it back
        // into the plain compound-assign statement, keeping the emitted
        // Rust a clean `x += 1` with no temp/value block. (A nested
        // inc/dec inside a larger expression never reaches here — it sits
        // under a Call/Index/etc. and stays an `Expr::IncDec`.)
        if let Expr::IncDec(incdec) = &expr {
            if !incdec.is_prefix {
                let target = (*incdec.target).clone();
                let stmt = self.make_incdec(target, incdec.is_inc)?;
                self.expect(&TokenKind::Semicolon, "';' after `++`/`--` statement");
                return Some(stmt);
            }
        }
        self.expect(&TokenKind::Semicolon, "';' after expression statement");
        Some(Stmt::Expr(expr))
    }

    /// True at `assert` used as a STATEMENT (§S.7.2): `assert cond;` or
    /// `assert cond : message;`. The call spelling `assert(cond);` and
    /// `assert(cond, message);` stays an ordinary call, recognized by its
    /// parenthesized argument list running straight into the `;`. A
    /// parenthesized condition followed by `: message`, or by more of an
    /// expression (`assert (a) && b;`), is the statement form.
    fn at_assert_statement(&self) -> bool {
        if !matches!(self.peek(), TokenKind::Ident(n) if n == "assert") {
            return false;
        }
        match self.tokens.get(self.pos + 1).map(|t| &t.kind) {
            // `assert = …`, `assert.x`, `assert;`: a name, not the built-in.
            Some(TokenKind::Eq) | Some(TokenKind::Dot) | Some(TokenKind::Semicolon)
            | Some(TokenKind::ColonColon) | None => false,
            Some(TokenKind::LParen) => !matches!(
                self.skip_balanced_parens(self.pos + 1).and_then(|i| self.tokens.get(i)).map(|t| &t.kind),
                Some(TokenKind::Semicolon),
            ),
            _ => true,
        }
    }

    /// `assert cond [: message];`, parsed into the call `assert(cond[, message])`
    /// so the checker and backend see one form.
    fn parse_assert_stmt(&mut self) -> Option<Stmt> {
        let callee_span = self.peek_span();
        let callee = self.parse_ident()?; // `assert`
        let condition = self.parse_expr()?;
        let mut args = vec![condition];
        if self.eat(&TokenKind::Colon) {
            args.push(self.parse_expr()?);
        }
        self.expect(&TokenKind::Semicolon, "';' after `assert` statement");
        let span = callee_span.join(self.last_consumed_span());
        let arg_names = vec![None; args.len()];
        Some(Stmt::Expr(Expr::Call(juxc_ast::CallExpr {
            callee: Box::new(Expr::Path(juxc_ast::QualifiedName { segments: vec![callee], span: callee_span })),
            explicit_generic_args: Vec::new(),
            args,
            arg_names,
            eval_order: Vec::new(),
            span,
        })))
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
            } else if let Expr::IncDec(incdec) = &expr {
                // Postfix `i++` / `i--` — the common C-style for-update.
                // `parse_expr` already parsed the trailing operator into
                // an `Expr::IncDec` (value form); since the update clause
                // discards the value, unwrap it back to the value-less
                // `i += 1` / `i -= 1` statement (clean Rust output).
                if !incdec.is_prefix {
                    let target = (*incdec.target).clone();
                    self.make_incdec(target, incdec.is_inc)?
                } else {
                    Stmt::Expr(expr)
                }
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
        let kw_span = self.peek_span(); // 'return' keyword
        self.advance(); // 'return'
        let value = if self.at(&TokenKind::Semicolon) { None } else { self.parse_expr() };
        self.expect(&TokenKind::Semicolon, "';' after return");
        let span = kw_span.join(self.last_consumed_span());
        Stmt::Return(value, span)
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
    /// At `var Name(` or `var a.b.Name(`: a record-destructuring local
    /// (§5.4). `var name = …` never has `(` after the name.
    fn at_record_destructure(&self) -> bool {
        let mut i = self.pos + 1; // past `var`
        if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Ident(_))) {
            return false;
        }
        i += 1;
        while matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Dot))
            && matches!(self.tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::Ident(_)))
        {
            i += 2;
        }
        matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen))
    }

    /// `[final] var Pt(a, b) = p;` (§5.4, Grammar §A.2.8 `record-pattern`).
    ///
    /// Desugars at parse time, like the tuple form, into a temporary typed
    /// with the record and one `var` per binder reading its component by
    /// POSITION (see `juxc_ast::record_destructure_temp` for why positions,
    /// and for how the checker and driver turn them into names). A binder is
    /// a name, `var name`, `_` to skip a component, or a nested record
    /// pattern (`var Line(Pt(x1, y1), var end) = l;`), which takes its
    /// component apart the same way through a temporary of its own:
    ///
    /// ```text
    /// Line __jux_rec0_2 = l;
    /// Pt __jux_rec1_2 = __jux_rec0_2.__jux_component_0;
    /// var x1 = __jux_rec1_2.__jux_component_0;
    /// var y1 = __jux_rec1_2.__jux_component_1;
    /// var end = __jux_rec0_2.__jux_component_1;
    /// ```
    ///
    /// Each temporary is checked like a top-level one, so a component that
    /// is not always that record (a supertype, a nullable) is E0271 there.
    fn parse_var_record_destructure(&mut self, is_final: bool) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // 'var'
        let record = self.parse_type_ref()?;
        let parts = self.parse_record_pattern_parts()?;
        self.expect(&TokenKind::Eq, "'=' in record destructuring");
        let init = self.parse_expr();
        self.expect(&TokenKind::Semicolon, "';' after record destructuring");
        let span = start.join(self.last_consumed_span());

        let tmp_name = juxc_ast::record_destructure_temp(self.tuple_tmp_counter, parts.len());
        self.tuple_tmp_counter += 1;
        self.queue_record_part_reads(&tmp_name, start, &parts, is_final);
        Some(Stmt::VarDecl(VarDecl {
            name: juxc_ast::Ident { text: tmp_name, span: start },
            ty: Some(record),
            init,
            is_final: true,
            is_ref: false,
            init_error: false,
            span,
        }))
    }

    /// `( part (',' part)* )` of a record pattern in a declaration, the
    /// cursor on the `(`. A part is a binder (`x`, `var x`, `_`) or a nested
    /// record pattern (`Pt(a, b)`, `var Pt(a, b)`, `geo.Pt(a, b)`).
    fn parse_record_pattern_parts(&mut self) -> Option<Vec<RecordPatternPart>> {
        self.expect(&TokenKind::LParen, "'(' to open the record pattern");
        let mut parts = Vec::new();
        if !self.at(&TokenKind::RParen) {
            loop {
                // `var x` is accepted for symmetry with `case Pt(var x, …)`.
                self.eat_kw(Keyword::Var);
                if self.at_nested_record_pattern() {
                    let record = self.parse_type_ref()?;
                    let inner = self.parse_record_pattern_parts()?;
                    parts.push(RecordPatternPart::Nested(record, inner));
                } else {
                    parts.push(RecordPatternPart::Bind(self.parse_ident()?));
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "')' to close the record pattern");
        Some(parts)
    }

    /// At `Name(` or `a.b.Name(` inside a record pattern: a nested record
    /// pattern rather than a binder.
    fn at_nested_record_pattern(&self) -> bool {
        let mut i = self.pos;
        if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Ident(_))) {
            return false;
        }
        i += 1;
        while matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Dot))
            && matches!(self.tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::Ident(_)))
        {
            i += 2;
        }
        matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen))
    }

    /// Queue the reads that take the record in `tmp_name` apart: one `var`
    /// per binder, and for a nested pattern a typed temporary holding the
    /// component followed, recursively, by that temporary's own reads. The
    /// statements land in source order on [`crate::Parser::pending_stmts`].
    fn queue_record_part_reads(
        &mut self,
        tmp_name: &str,
        tmp_span: juxc_source::Span,
        parts: &[RecordPatternPart],
        is_final: bool,
    ) {
        for (i, part) in parts.iter().enumerate() {
            // The component READ gets the binder's (or the nested pattern's)
            // span, which is also what the checker keys the position-to-name
            // rewrite on; a temporary gets the span of what it destructures.
            let read_span = match part {
                RecordPatternPart::Bind(binder) if binder.text == "_" => continue,
                RecordPatternPart::Bind(binder) => binder.span,
                RecordPatternPart::Nested(record, _) => record.span,
            };
            let read = Expr::Field(juxc_ast::FieldExpr {
                object: Box::new(Expr::Path(juxc_ast::QualifiedName {
                    segments: vec![juxc_ast::Ident { text: tmp_name.to_string(), span: tmp_span }],
                    span: tmp_span,
                })),
                field: juxc_ast::Ident { text: juxc_ast::record_component_marker(i), span: read_span },
                safe: false,
                span: read_span,
            });
            match part {
                RecordPatternPart::Bind(binder) => self.pending_stmts.push(Stmt::VarDecl(VarDecl {
                    name: binder.clone(),
                    ty: None,
                    init: Some(read),
                    is_final,
                    is_ref: false,
                    init_error: false,
                    span: binder.span,
                })),
                RecordPatternPart::Nested(record, inner) => {
                    let inner_name = juxc_ast::record_destructure_temp(self.tuple_tmp_counter, inner.len());
                    self.tuple_tmp_counter += 1;
                    self.pending_stmts.push(Stmt::VarDecl(VarDecl {
                        name: juxc_ast::Ident { text: inner_name.clone(), span: record.span },
                        ty: Some(record.clone()),
                        init: Some(read),
                        is_final: true,
                        is_ref: false,
                        init_error: false,
                        span: record.span,
                    }));
                    self.queue_record_part_reads(&inner_name, record.span, inner, is_final);
                }
            }
        }
    }

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
                        "nested tuple patterns aren't supported yet (Phase 1) -- destructure the outer tuple first, then the element",
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
                    "a tuple pattern needs at least two binders -- use a plain `var name = …;` otherwise",
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
            // The element READ gets the binder's span; the temp it reads
            // from gets the statement's. Giving both the binder's span made
            // them collide in the span-keyed type map, and the tuple's type
            // won -- so `var (stream, _) = pair;` left `stream` typed as the
            // whole tuple, and every question the backend asks about a
            // receiver by name got the wrong answer.
            let elem_init = Expr::Field(juxc_ast::FieldExpr {
                object: Box::new(Expr::Path(juxc_ast::QualifiedName {
                    segments: vec![juxc_ast::Ident { text: tmp_name.clone(), span: start }],
                    span: start,
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
                init_error: false,
                span: binder.span,
            }));
        }
        Some(Stmt::VarDecl(VarDecl {
            name: tmp_ident,
            ty: None,
            init,
            is_final: false,
            is_ref: false,
            init_error: false,
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
        // `var x: int = 5;` (E0144): the Kotlin/TypeScript annotation. Say so
        // once, read the type so the rest of the line parses, and carry on as
        // plain `var` (JUX-LANG-V1 §5.6).
        if self.at(&TokenKind::Colon) {
            let colon = self.peek_span();
            self.advance(); // ':'
            let ty = self.parse_type_ref();
            let written = ty
                .as_ref()
                .map(|t| t.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
                .unwrap_or_default();
            let fix = if written.is_empty() {
                "write the type first, `int x = 5;`, or leave it to `var x = 5;`".to_string()
            } else {
                format!("write `{written} {} = …;`, or `var {} = …;`", name.text, name.text)
            };
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0144_ColonTypeAnnotation,
                    format!("Jux puts the type before the name, not after a `:`: {fix}"),
                )
                .with_span(colon.join(self.last_consumed_span())),
            );
        }
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
            init_error: false,
            span: start.join(end),
        })
    }

    /// Lookahead for a **function-typed local declaration**:
    /// `( type-list? ) 'async'? ( 'throws' type-list )? '->' type IDENT ( '=' | ';' )`.
    ///
    /// The discriminator against a lambda expression statement
    /// (`(a, b) -> a + b;`) is what follows the `->`: a declaration has a type
    /// AND a binding name before the `=` or `;`, and a lambda body does not.
    /// `(a) -> b + c;` fails at "binding name", so it stays an expression.
    fn looks_like_fn_type_local(&self) -> bool {
        let Some(mut i) = self.skip_balanced_parens(self.pos) else {
            return false;
        };
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Kw(Keyword::Async))) {
            i += 1;
        }
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Kw(Keyword::Throws))) {
            i += 1;
            let Some(next) = self.skip_type_list(i) else { return false };
            i = next;
        }
        if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Arrow)) {
            return false;
        }
        i += 1;
        let Some(after_ret) = self.skip_one_type(i) else {
            return false;
        };
        matches!(self.tokens.get(after_ret).map(|t| &t.kind), Some(TokenKind::Ident(_)))
            && matches!(
                self.tokens.get(after_ret + 1).map(|t| &t.kind),
                Some(TokenKind::Eq) | Some(TokenKind::Semicolon),
            )
    }

    /// Index just past the `)` matching the `(` at `i`, or `None` when the
    /// group never closes.
    fn skip_balanced_parens(&self, mut i: usize) -> Option<usize> {
        if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen)) {
            return None;
        }
        i += 1;
        let mut depth = 1u32;
        while depth > 0 {
            match self.tokens.get(i).map(|t| &t.kind) {
                Some(TokenKind::LParen) => depth += 1,
                Some(TokenKind::RParen) => depth -= 1,
                Some(TokenKind::Eof) | None => return None,
                _ => {}
            }
            i += 1;
        }
        Some(i)
    }

    /// Non-consuming scan of ONE type at `i`, returning the index just past it.
    /// Handles the named form (dotted name, generic args, `?`, array dims,
    /// pointer stars) and recurses for a nested function type, which is what a
    /// curried return type (`(int) -> (int) -> int`) needs.
    fn skip_one_type(&self, i: usize) -> Option<usize> {
        // `fn(A) -> R`: the same scan as a closure type, one token later.
        if self.at_fn_pointer_type(i) {
            return self.skip_one_type(i + 1);
        }
        let mut j = if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen)) {
            let after = self.skip_balanced_parens(i)?;
            let mut j = after;
            if matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Kw(Keyword::Async))) {
                j += 1;
            }
            if matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Kw(Keyword::Throws))) {
                j = self.skip_type_list(j + 1)?;
            }
            if matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Arrow)) {
                return self.skip_one_type(j + 1);
            }
            // No `->`: a tuple type `(A, B, ...)` (§A.2.7), two or more types
            // exactly filling the parentheses. It takes the same `?`, array
            // and pointer suffixes as a named type, below.
            if !self.is_tuple_type_group(i, after) {
                return None;
            }
            after
        } else {
            match self.tokens.get(i).map(|t| &t.kind) {
                Some(TokenKind::Kw(Keyword::Void)) => i + 1,
                Some(TokenKind::Ident(_)) => {
                    let mut j = i + 1;
                    while matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Dot))
                        && matches!(self.tokens.get(j + 1).map(|t| &t.kind), Some(TokenKind::Ident(_)))
                    {
                        j += 2;
                    }
                    if let Some(next) = self.skip_type_args(j) {
                        j = next;
                    }
                    j
                }
                _ => return None,
            }
        };
        if matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Question)) {
            j += 1;
        }
        while matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::LBracket)) {
            j += 1;
            let mut depth = 1u32;
            while depth > 0 {
                match self.tokens.get(j).map(|t| &t.kind) {
                    Some(TokenKind::LBracket) => depth += 1,
                    Some(TokenKind::RBracket) => depth -= 1,
                    Some(TokenKind::Eof) | None => return None,
                    _ => {}
                }
                j += 1;
            }
        }
        while matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Star)) {
            j += 1;
        }
        Some(j)
    }

    /// True when the parenthesized group opening at `open` (and ending just
    /// before `after`) is a tuple type: two or more types separated by commas
    /// that fill it exactly. `(a, b)` of plain names passes too, which is fine:
    /// the callers only ask once they have seen a binding name after it.
    fn is_tuple_type_group(&self, open: usize, after: usize) -> bool {
        let mut j = open + 1;
        let mut types = 0usize;
        loop {
            let Some(next) = self.skip_one_type(j) else { return false };
            types += 1;
            j = next;
            if matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Comma)) {
                j += 1;
                continue;
            }
            break;
        }
        types >= 2 && j + 1 == after
    }

    /// Non-consuming scan of a comma-separated type list at `i`.
    fn skip_type_list(&self, i: usize) -> Option<usize> {
        let mut j = self.skip_one_type(i)?;
        while matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Comma)) {
            j = self.skip_one_type(j + 1)?;
        }
        Some(j)
    }

    /// If the statement at the cursor declares a TYPE, the kind of type:
    /// `class`, `interface`, `enum`, `record` or `struct`, after any modifiers.
    fn local_type_declaration_ahead(&self) -> Option<&'static str> {
        let mut i = self.pos;
        while matches!(
            self.tokens.get(i).map(|t| &t.kind),
            Some(TokenKind::Kw(
                Keyword::Public
                    | Keyword::Private
                    | Keyword::Protected
                    | Keyword::Static
                    | Keyword::Final
                    | Keyword::Abstract
                    | Keyword::Sealed
            ))
        ) {
            i += 1;
        }
        let kind = match self.tokens.get(i).map(|t| &t.kind) {
            Some(TokenKind::Kw(Keyword::Class)) => "class",
            Some(TokenKind::Kw(Keyword::Interface)) => "interface",
            Some(TokenKind::Kw(Keyword::Enum)) => "enum",
            Some(TokenKind::Kw(Keyword::Record)) => "record",
            Some(TokenKind::Kw(Keyword::Struct)) => "struct",
            _ => return None,
        };
        // Followed by a name: `class Foo`, not some other use of the keyword.
        matches!(self.tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::Ident(_))).then_some(kind)
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
    /// Generic types (`List<int> nums = …;`), dotted types
    /// (`com.example.Foo x = …;`) and nested ones (`Order.Status s = …;`) are
    /// all recognised: a dotted name followed by another identifier is never a
    /// valid expression statement, so the shape is unambiguous. (An earlier
    /// version skipped dotted types and told users to write `var`, which is not
    /// what the grammar says.)
    pub(crate) fn looks_like_typed_local(&self) -> bool {
        // A function-pointer local, `fn(int) -> int f = double;`. Without this
        // `fn(int)` read as a call to something named `fn`.
        if self.at_fn_pointer_type(self.pos) {
            return self
                .skip_one_type(self.pos)
                .is_some_and(|after| {
                    matches!(self.tokens.get(after).map(|t| &t.kind), Some(TokenKind::Ident(_)))
                        && matches!(
                            self.tokens.get(after + 1).map(|t| &t.kind),
                            Some(TokenKind::Eq) | Some(TokenKind::Semicolon),
                        )
                });
        }
        // A FUNCTION-typed local — `(int) -> int f = (n) -> n + 1;`. Grammar
        // §A.2.7 makes `function-type` a `simple-type`, so it is legal wherever
        // a type is, `local-decl` included. It needs its own lookahead because
        // the statement starts with `(`, which otherwise reads as a
        // parenthesized expression or a lambda.
        if self.at(&TokenKind::LParen) {
            if self.looks_like_fn_type_local() {
                return true;
            }
            // A TUPLE-typed local, `(int, int) t = (1, 2);` (§A.2.7): the type,
            // then a binding name, then `=` or `;`. A tuple literal or a
            // parenthesized expression is never followed by a bare name.
            return self.skip_one_type(self.pos).is_some_and(|after| {
                matches!(self.tokens.get(after).map(|t| &t.kind), Some(TokenKind::Ident(_)))
                    && matches!(
                        self.tokens.get(after + 1).map(|t| &t.kind),
                        Some(TokenKind::Eq) | Some(TokenKind::Semicolon),
                    )
            });
        }
        // `void* p = …;` / `void** pp;` — a raw pointer to an untyped C region
        // (§L.7). `void` is a keyword (not an `Ident`) and a bare `void` is never
        // a value type, so the shape is unambiguous: `void` + at least one `*` +
        // a name + (`=` | `;`).
        if self.at_kw(Keyword::Void) {
            let mut j = self.pos + 1;
            let mut stars = 0u32;
            while matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Star)) {
                j += 1;
                stars += 1;
            }
            return stars > 0
                && matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Ident(_)))
                && matches!(
                    self.tokens.get(j + 1).map(|t| &t.kind),
                    Some(TokenKind::Eq) | Some(TokenKind::Semicolon),
                );
        }
        if !matches!(self.peek(), TokenKind::Ident(_)) {
            return false;
        }
        let mut i = self.pos + 1;
        // Further dotted segments of a qualified type name: `demo.pkg.Crate`,
        // `Order.Status`.
        while matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Dot))
            && matches!(self.tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::Ident(_)))
        {
            i += 2;
        }
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
        let mut init_error = false;
        let init = if self.eat(&TokenKind::Eq) {
            // Bare `{a, b, c}` initializer (Java-style) — only valid
            // when the LHS is an array type. The fixed/dynamic flag is
            // carried into the AST so the backend emits the matching
            // Rust shape (`[…]` vs `vec![…]`). For non-array LHS, a
            // `{` here is a parse error — typed locals don't otherwise
            // start with `{`.
            if self.at(&TokenKind::LBrace) && ty.array_shape.is_some() {
                Some(self.parse_bare_array_initializer(&ty)?)
            } else if self.at(&TokenKind::LBrace) {
                let parsed = self.parse_brace_initializer_for_named_type(&ty);
                init_error = parsed.is_none();
                parsed
            } else if self.at(&TokenKind::LBracket) && ty.array_shape.is_some() {
                // `["a", "b"]` out of habit from other languages: say once
                // that Jux writes an array literal with braces, then read it
                // as that, so nothing after it cascades (JUX-DIAGNOSTICS-
                // ADDENDUM "Java Habits").
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "an array literal is written with braces: `{a, b, c}`, not `[a, b, c]`",
                    )
                    .with_span(self.peek_span()),
                );
                Some(self.parse_array_initializer_between(&ty, TokenKind::LBracket, TokenKind::RBracket)?)
            } else {
                self.parse_expr()
            }
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon, "';' after typed local declaration");
        let end = self.last_consumed_span();
        Some(VarDecl {
            name,
            ty: Some(ty),
            init,
            is_final,
            is_ref: false,
            init_error,
            span: ty_start.join(end),
        })
    }

    /// A `{a, b, c}` initializer under a type written with no array shape.
    ///
    /// Under an alias of an array type (`type Bytes = ubyte[];` then
    /// `final Bytes data = {1, 2, 3};`) the alias supplies the shape and the
    /// initializer reads as it would under `ubyte[]`. Any other type cannot
    /// take one: that is reported once, the braces are skipped, and the local
    /// is left uninitialized, where the expression parser used to report each
    /// element as "expected expression".
    pub(crate) fn parse_brace_initializer_for_named_type(&mut self, ty: &TypeRef) -> Option<Expr> {
        let aliased = (ty.name.segments.len() == 1
            && ty.generic_args.is_empty()
            && ty.fn_shape.is_none()
            && ty.ptr_depth == 0)
            .then(|| self.array_alias(&ty.name.segments[0].text))
            .flatten();
        if let Some(target) = aliased {
            return self.parse_bare_array_initializer(&target);
        }
        let name = ty.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0200_UnexpectedToken,
                format!("a `{{...}}` initializer needs an array type, and `{name}` is not one"),
            )
            .with_span(self.peek_span())
            .with_help("write the array type out, `new T[] {a, b}`, or declare the local with an array type"),
        );
        // Skip the balanced braces so nothing inside cascades.
        let mut depth = 0usize;
        loop {
            match self.peek() {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        self.advance();
                        break;
                    }
                }
                TokenKind::Eof => break,
                _ => {}
            }
            self.advance();
        }
        None
    }

    /// Parse a bare `{a, b, c}` array initializer in typed-local RHS
    /// position. The `lhs_ty` provides both the element type and the
    /// fixed/dynamic shape for backend dispatch.
    ///
    /// Caller invariant: the next token is `{` and `lhs_ty.array_shape`
    /// is `Some(...)`.
    pub(crate) fn parse_bare_array_initializer(&mut self, lhs_ty: &TypeRef) -> Option<Expr> {
        self.parse_array_initializer_between(lhs_ty, TokenKind::LBrace, TokenKind::RBrace)
    }

    /// [`Self::parse_bare_array_initializer`] with the delimiters given: `{`
    /// and `}` for the real form, `[` and `]` when recovering from the
    /// bracket habit (the caller has already reported it).
    pub(crate) fn parse_array_initializer_between(
        &mut self,
        lhs_ty: &TypeRef,
        open: TokenKind,
        close: TokenKind,
    ) -> Option<Expr> {
        let start = self.peek_span();
        self.expect(&open, "'{' to open array initializer");

        // Peel ONE (outermost) dimension off the LHS to get the *element*
        // type. For a 1-D `int[]`/`int[N]` LHS the element is the scalar
        // (`peeled()` → `None`); for a multi-dim `int[][]` LHS the element
        // is itself an array (`int[]`), so its own `array_shape` is kept.
        // Computed BEFORE the element loop so a NESTED literal recurses with the
        // peeled type — this handles ANY depth (`int[][]`, `int[][][]`, …)
        // uniformly, with no per-rank special-casing.
        let element_type = TypeRef {
            name: lhs_ty.name.clone(),
            generic_args: lhs_ty.generic_args.clone(),
            nullable: lhs_ty.nullable,
            array_shape: lhs_ty.array_shape.as_ref().and_then(|s| s.peeled()),
            fn_shape: lhs_ty.fn_shape.clone(),
            ptr_depth: 0,
            span: lhs_ty.span,
        };
        // When the element is itself an array, a `{ … }` element is a nested
        // initializer for the next dimension down — recurse. Otherwise the
        // element is a scalar/reference expression.
        let element_is_array = element_type.array_shape.is_some();
        let mut elements = Vec::new();
        if !self.at(&close) {
            loop {
                let e = if element_is_array && self.at(&open) {
                    self.parse_array_initializer_between(&element_type, open.clone(), close.clone())?
                } else {
                    self.parse_expr()?
                };
                elements.push(e);
                // A trailing comma before `}` is allowed (grammar: array
                // initializer), so a list written one element per line can
                // end every line the same way.
                if !self.eat(&TokenKind::Comma) || self.at(&close) {
                    break;
                }
            }
        }
        let end = self.peek_span();
        self.expect(&close, "'}' to close array initializer");
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

    /// `yield expr ;` or `yield * expr ;` (§M.2.3).
    ///
    /// `yield* iter;` means "yield every value of `iter`", so it is parsed
    /// straight into that loop: `for (var __jux_yielded : iter) { yield
    /// __jux_yielded; }`. Everything downstream (the element type, the
    /// iteration protocol, the check that each value fits the generator's
    /// element type) is then the ordinary `for` and `yield` path.
    fn parse_yield_stmt(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // `yield`
        let delegate = self.eat(&TokenKind::Star);
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after `yield`");
        let span = start.join(self.last_consumed_span());
        if !delegate {
            return Some(Stmt::Yield(value, span));
        }
        let binder = juxc_ast::Ident {
            text: juxc_ast::ForEachStmt::YIELD_DELEGATE_BINDER.to_string(),
            span,
        };
        let element = Expr::Path(juxc_ast::QualifiedName { segments: vec![binder.clone()], span });
        Some(Stmt::ForEach(juxc_ast::ForEachStmt {
            is_await: false,
            var_type: None,
            var_name: binder,
            iter: value,
            body: juxc_ast::Block { statements: vec![Stmt::Yield(element, span)], span },
            span,
        }))
    }

    /// `'if' 'cfg' '(' cfg-pred ')' block ( 'else' block )?` (grammar A.2.8).
    ///
    /// Both branches are blocks, as the grammar has them; an `else` may also be
    /// followed by another `if` of either kind, kept as a one-statement block,
    /// so a chain reads the way an ordinary `else if` chain does.
    pub(crate) fn parse_if_cfg_stmt(&mut self) -> juxc_ast::IfCfgStmt {
        let start = self.peek_span();
        self.advance(); // 'if'
        self.advance(); // 'cfg'
        let predicate = self.parse_cfg_predicate_list();
        let then_block = self.parse_block();
        let else_block = if self.eat_kw(Keyword::Else) {
            if self.at_kw(Keyword::If) {
                let chain_start = self.peek_span();
                let statements = self.parse_stmt_inner().into_iter().collect();
                Some(juxc_ast::Block { statements, span: chain_start.join(self.last_consumed_span()) })
            } else {
                Some(self.parse_block())
            }
        } else {
            None
        };
        juxc_ast::IfCfgStmt {
            predicate,
            then_block,
            else_block,
            span: start.join(self.last_consumed_span()),
        }
    }

    /// `if-stmt = 'if' '(' expression ')' statement-block ('else' (if-stmt | block))?`
    /// per §A.2.8. (We require a `{}` block on each arm — single-statement
    /// arms without braces are a future extension.)
    pub(crate) fn parse_if_stmt(&mut self) -> Option<IfStmt> {
        let start = self.peek_span();
        self.advance(); // 'if'
        self.expect(&TokenKind::LParen, "'(' after `if`");
        let mut condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "')' after `if` condition");
        // §A.2.8: an `if` body is a `statement` — a brace block OR a
        // single braceless statement (`if (c) return;`).
        let then_block = self.parse_block_or_stmt();
        smart_cast_bare_type_test(&mut condition, &then_block);

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

/// `if (a => Dog) a.bark();` (Type system §T.6.2): a bare type test on a
/// local narrows it in the then-branch, as `if (a => Dog a)` would. The
/// branch sees `a` as a `Dog` because it IS that form: the binder shadows the
/// outer `a`, and the checker and backend treat it like any binder.
///
/// Skipped when the branch assigns `a` (`a = new Cat();` must still reach the
/// outer variable, whose type allows it; T.6.3 ends a refinement at an
/// assignment anyway), and when the tested value is anything but a bare name.
fn smart_cast_bare_type_test(condition: &mut juxc_ast::Expr, then_block: &juxc_ast::Block) {
    let juxc_ast::Expr::TypeTest(t) = condition else { return };
    if t.binder.is_some() {
        return;
    }
    let juxc_ast::Expr::Path(qn) = t.value.as_ref() else { return };
    if qn.segments.len() != 1 {
        return;
    }
    let name = qn.segments[0].clone();
    let mut assigned = false;
    juxc_ast::visit::for_each_node(then_block, &mut |node| {
        let target = match node {
            juxc_ast::visit::Node::Stmt(juxc_ast::Stmt::Assign(a)) => Some(&a.target),
            juxc_ast::visit::Node::Expr(juxc_ast::Expr::IncDec(i)) => Some(i.target.as_ref()),
            _ => None,
        };
        if let Some(juxc_ast::Expr::Path(p)) = target {
            if p.segments.len() == 1 && p.segments[0].text == name.text {
                assigned = true;
            }
        }
    });
    if !assigned {
        t.binder = Some(name);
    }
}

/// One position of a record pattern in a destructuring declaration (§5.4).
enum RecordPatternPart {
    /// `x`, `var x`, or `_` (skip the component).
    Bind(juxc_ast::Ident),
    /// `Pt(a, b)`: the component is a record, taken apart in turn.
    Nested(juxc_ast::TypeRef, Vec<RecordPatternPart>),
}
