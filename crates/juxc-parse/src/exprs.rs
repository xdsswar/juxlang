//! Expression parsing — the precedence cascade plus free helpers.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{
    BinaryExpr, BinaryOp, CallExpr, CastExpr, Expr, FieldExpr, IndexExpr, InterpStringExpr,
    Literal, NewArrayExpr, NewArrayLitExpr, NewObjectExpr, QualifiedName, RangeExpr, SizeOfExpr,
    TypeRef, UnaryExpr, UnaryOp,
};
use juxc_diagnostics::{code, Diagnostic};
use juxc_lex::{Keyword, TokenKind};
use juxc_source::Span;

use crate::literals::{parse_float_literal_text, parse_int_literal_text, process_string_escapes};
use crate::Parser;

impl<'a> Parser<'a> {
    // ------------------------------------------------------------------
    // Expressions (§A.2.9)
    //
    // The full grammar is a layered precedence cascade. We start with the
    // smallest layer that hello.jux needs: primary expressions plus a
    // postfix-call chain. Binary operators, assignment, and the conditional
    // are added one precedence level at a time as features need them.
    // ------------------------------------------------------------------

    /// Parse any expression. Enters the precedence cascade.
    ///
    /// Layers (loosest binding first), per §A.2.9 / §A.4:
    ///
    ///  1. logical-or    — `||` (short-circuit)
    ///  2. logical-and   — `&&` (short-circuit)
    ///  3. bit-or        — `|`
    ///  4. bit-xor       — `^`
    ///  5. bit-and       — `&`
    ///  6. equality      — `==`, `!=`
    ///  7. comparison    — `<`, `<=`, `>`, `>=`
    ///  8. range         — `..`, `..=`
    ///  9. shift         — `<<`, `>>`
    /// 10. additive      — `+`, `-`
    /// 11. multiplicative — `*`, `/`, `%`
    /// 12. unary         — prefix `-`, `!`, `~`
    /// 13. postfix       — call, member access
    /// 14. primary       — literals, paths, parenthesized
    ///
    /// Per §A.4, bitwise ops `&`/`|`/`^` are **looser than equality** in
    /// Jux (Java/Python-style precedence). This is the opposite of Rust's
    /// ordering; the emitted Rust gets parens at lowering time to preserve
    /// the Jux tree shape.
    ///
    /// Layers not yet implemented (assignment-as-expression, conditional
    /// `? :`, elvis `?:`, three-way `<=>`, type-test `=>` / `in`, cast
    /// `as`) drop straight through to the next implemented layer and
    /// land as features need them.
    pub(crate) fn parse_expr(&mut self) -> Option<Expr> {
        // Lambda forms — checked before the operator-precedence
        // chain because they bind looser than any binary op and
        // can be heralded by either `identifier ->` or `(…) ->`.
        // The lookahead in `looks_like_lambda_head` peeks past
        // matched parens and the optional `async` keyword to
        // decide; non-lambda parens fall through to `parse_primary`.
        if self.looks_like_lambda_head() {
            return self.parse_lambda();
        }
        self.parse_logic_or()
    }

    /// Logical-OR layer: left-associative short-circuit `||` per §A.4
    /// level 4. Loosest binary operator currently modeled.
    pub(crate) fn parse_logic_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_logic_and()?;
        while matches!(self.peek(), TokenKind::OrOr) {
            self.advance();
            let right = self.parse_logic_and()?;
            left = make_binary(BinaryOp::Or, left, right);
        }
        Some(left)
    }

    /// Logical-AND layer: left-associative short-circuit `&&` per §A.4
    /// level 5. Tighter than `||`, looser than `|`.
    pub(crate) fn parse_logic_and(&mut self) -> Option<Expr> {
        let mut left = self.parse_bit_or()?;
        while matches!(self.peek(), TokenKind::AndAnd) {
            self.advance();
            let right = self.parse_bit_or()?;
            left = make_binary(BinaryOp::And, left, right);
        }
        Some(left)
    }

    /// Bitwise-OR layer: left-associative `|` per §A.4 level 6.
    pub(crate) fn parse_bit_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_bit_xor()?;
        while matches!(self.peek(), TokenKind::Pipe) {
            self.advance();
            let right = self.parse_bit_xor()?;
            left = make_binary(BinaryOp::BitOr, left, right);
        }
        Some(left)
    }

    /// Bitwise-XOR layer: left-associative `^` per §A.4 level 7.
    pub(crate) fn parse_bit_xor(&mut self) -> Option<Expr> {
        let mut left = self.parse_bit_and()?;
        while matches!(self.peek(), TokenKind::Caret) {
            self.advance();
            let right = self.parse_bit_and()?;
            left = make_binary(BinaryOp::BitXor, left, right);
        }
        Some(left)
    }

    /// Bitwise-AND layer: left-associative `&` per §A.4 level 8.
    pub(crate) fn parse_bit_and(&mut self) -> Option<Expr> {
        let mut left = self.parse_equality()?;
        while matches!(self.peek(), TokenKind::Amp) {
            self.advance();
            let right = self.parse_equality()?;
            left = make_binary(BinaryOp::BitAnd, left, right);
        }
        Some(left)
    }

    /// Equality layer: left-associative `==` and `!=`. Per §A.4 also
    /// includes `===` / `!==`, but those are reference-identity operators
    /// we don't model yet.
    pub(crate) fn parse_equality(&mut self) -> Option<Expr> {
        let mut left = self.parse_comparison()?;
        while let Some(op) = self.peek_eq_op() {
            self.advance();
            let right = self.parse_comparison()?;
            left = make_binary(op, left, right);
        }
        Some(left)
    }

    /// Comparison layer: left-associative `<`, `<=`, `>`, `>=`. Per §A.4
    /// these are *non-chaining* at the type-checker level — `a < b < c`
    /// parses but should be rejected later. We accept it here and let a
    /// future tycheck pass complain. Operand is [`Self::parse_range`].
    pub(crate) fn parse_comparison(&mut self) -> Option<Expr> {
        let mut left = self.parse_range()?;
        while let Some(op) = self.peek_cmp_op() {
            self.advance();
            let right = self.parse_range()?;
            left = make_binary(op, left, right);
        }
        Some(left)
    }

    /// Range layer: `a .. b` (exclusive) and `a ..= b` (inclusive) per
    /// §A.2.9 level 13. **Non-associative** — chaining `a..b..c` is a
    /// spec error (today we just stop after the first `..` and let
    /// downstream layers complain about the leftover token).
    ///
    /// Open ranges (`a..`, `..b`, `..=b`) are pattern-only per the spec
    /// and aren't accepted here. `step` is deferred.
    pub(crate) fn parse_range(&mut self) -> Option<Expr> {
        let left = self.parse_shift()?;
        let inclusive = match self.peek() {
            TokenKind::DotDot   => false,
            TokenKind::DotDotEq => true,
            _ => return Some(left),
        };
        self.advance(); // `..` or `..=`
        let right = self.parse_shift()?;
        let span = expr_span(&left).join(expr_span(&right));
        Some(Expr::Range(RangeExpr {
            start: Box::new(left),
            end: Box::new(right),
            inclusive,
            span,
        }))
    }

    /// Shift layer: left-associative `<<` and `>>` per §A.4 level 14.
    /// Operand is [`Self::parse_additive`] — additive binds tighter.
    pub(crate) fn parse_shift(&mut self) -> Option<Expr> {
        let mut left = self.parse_additive()?;
        while let Some(op) = self.peek_shift_op() {
            self.advance();
            let right = self.parse_additive()?;
            left = make_binary(op, left, right);
        }
        Some(left)
    }

    /// If the current token is a shift operator, return its [`BinaryOp`].
    pub(crate) fn peek_shift_op(&self) -> Option<BinaryOp> {
        match self.peek() {
            TokenKind::LtLt => Some(BinaryOp::Shl),
            TokenKind::GtGt => Some(BinaryOp::Shr),
            _ => None,
        }
    }

    /// Additive layer: left-associative `+` and `-`.
    pub(crate) fn parse_additive(&mut self) -> Option<Expr> {
        let mut left = self.parse_multiplicative()?;
        while let Some(op) = self.peek_add_op() {
            self.advance();
            let right = self.parse_multiplicative()?;
            left = make_binary(op, left, right);
        }
        Some(left)
    }

    /// Multiplicative layer: left-associative `*`, `/`, `%`. Operand is
    /// [`Self::parse_as`] — `as` casts bind tighter than multiplicative.
    pub(crate) fn parse_multiplicative(&mut self) -> Option<Expr> {
        let mut left = self.parse_as()?;
        while let Some(op) = self.peek_mul_op() {
            self.advance();
            let right = self.parse_as()?;
            left = make_binary(op, left, right);
        }
        Some(left)
    }

    /// `as` cast layer per §A.4 level 17 / §A.5.
    ///
    /// `e as T` is left-associative — `x as int as long` parses as
    /// `(x as int) as long`. Operand is [`Self::parse_unary`] (unary
    /// binds tighter).
    ///
    /// The `as` keyword is recognized via [`Keyword::As`]; the source
    /// representation `as` is a reserved keyword per §3.2.
    pub(crate) fn parse_as(&mut self) -> Option<Expr> {
        let mut expr = self.parse_unary()?;
        while self.at_kw(Keyword::As) {
            self.advance(); // consume `as`
            let ty = self.parse_type_ref()?;
            let span = expr_span(&expr).join(ty.span);
            expr = Expr::Cast(CastExpr {
                value: Box::new(expr),
                ty,
                span,
            });
        }
        Some(expr)
    }

    /// Unary layer: prefix `-`, `!`, `~` per §A.4 level 18.
    ///
    /// Right-associative: `--x` parses as `-(-x)`, `!!flag` as `!(!flag)`.
    /// This is implemented by recursing into `parse_unary` for the
    /// operand rather than dropping straight to postfix.
    ///
    /// The other §A.4-level-18 prefix operators (`+`, `move`, `await`,
    /// `&`, `*`) are not modeled yet.
    pub(crate) fn parse_unary(&mut self) -> Option<Expr> {
        let start_span = self.peek_span();
        let op = match self.peek() {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Bang => Some(UnaryOp::Not),
            TokenKind::Tilde => Some(UnaryOp::BitNot),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            // Right-associative: operand is itself parsed at unary precedence,
            // so a stack of prefix operators chains right-to-left.
            let operand = self.parse_unary()?;
            let span = start_span.join(self.last_consumed_span());
            return Some(Expr::Unary(UnaryExpr {
                op,
                operand: Box::new(operand),
                span,
            }));
        }
        self.parse_postfix()
    }

    /// If the current token is an equality operator, return its
    /// [`BinaryOp`]; otherwise `None`. Does **not** advance.
    pub(crate) fn peek_eq_op(&self) -> Option<BinaryOp> {
        match self.peek() {
            TokenKind::EqEq  => Some(BinaryOp::Eq),
            TokenKind::NotEq => Some(BinaryOp::NotEq),
            _ => None,
        }
    }

    /// If the current token is a comparison operator, return its
    /// [`BinaryOp`]; otherwise `None`. Does **not** advance.
    pub(crate) fn peek_cmp_op(&self) -> Option<BinaryOp> {
        match self.peek() {
            TokenKind::Lt => Some(BinaryOp::Lt),
            TokenKind::Le => Some(BinaryOp::Le),
            TokenKind::Gt => Some(BinaryOp::Gt),
            TokenKind::Ge => Some(BinaryOp::Ge),
            _ => None,
        }
    }

    /// If the current token is an additive operator, return its
    /// [`BinaryOp`]; otherwise `None`. Does **not** advance.
    pub(crate) fn peek_add_op(&self) -> Option<BinaryOp> {
        match self.peek() {
            TokenKind::Plus  => Some(BinaryOp::Add),
            TokenKind::Minus => Some(BinaryOp::Sub),
            _ => None,
        }
    }

    /// If the current token is a multiplicative operator, return its
    /// [`BinaryOp`]; otherwise `None`. Does **not** advance.
    pub(crate) fn peek_mul_op(&self) -> Option<BinaryOp> {
        match self.peek() {
            TokenKind::Star    => Some(BinaryOp::Mul),
            TokenKind::Slash   => Some(BinaryOp::Div),
            TokenKind::Percent => Some(BinaryOp::Rem),
            _ => None,
        }
    }

    /// `postfix = primary postfix-op*` per §A.2.9.
    ///
    /// Postfix operators recognized so far:
    /// - `(args)` — call.
    /// - `[index]` — element access (array index, future map lookup).
    ///
    /// Future: `.name`, `?.name`, `?`, `!!`, `::name`, …
    pub(crate) fn parse_postfix(&mut self) -> Option<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek() {
                TokenKind::LParen => {
                    // call: callee(args)
                    self.advance(); // '('
                    let args = self.parse_arg_list();
                    let end = self.peek_span();
                    self.expect(&TokenKind::RParen, "')' to close argument list");
                    let span = expr_span(&expr).join(end);
                    expr = Expr::Call(CallExpr { callee: Box::new(expr), args, span });
                }
                TokenKind::LBracket => {
                    // index: array[index]
                    self.advance(); // '['
                    let index = self.parse_expr()?;
                    let end = self.peek_span();
                    self.expect(&TokenKind::RBracket, "']' to close index expression");
                    let span = expr_span(&expr).join(end);
                    expr = Expr::Index(IndexExpr {
                        array: Box::new(expr),
                        index: Box::new(index),
                        span,
                    });
                }
                TokenKind::Dot => {
                    // field access: object.field (Java-style member access).
                    //
                    // Currently the only field the backend recognizes is
                    // `length` on array-typed expressions. Other names
                    // will surface as "unknown method/field" once we
                    // have a type table and member resolution.
                    self.advance(); // '.'
                    let field = self.parse_ident()?;
                    let span = expr_span(&expr).join(field.span);
                    expr = Expr::Field(FieldExpr {
                        object: Box::new(expr),
                        field,
                        span,
                    });
                }
                _ => break,
            }
        }
        Some(expr)
    }

    /// `primary = literal | identifier | 'this' | 'super' | …` per §A.2.9.
    /// Milestone-1 coverage: literals (int/string/bool/null) and identifier
    /// paths only.
    pub(crate) fn parse_primary(&mut self) -> Option<Expr> {
        // Clone the kind so we can advance() without borrow-checker grief.
        let span = self.peek_span();
        match self.peek().clone() {
            TokenKind::Str(text) => {
                self.advance();
                // Decode escape sequences per `JUX-GRAMMAR-ADDENDUM.md`
                // §A.1.5. The lexer handed us the raw bytes between the
                // quotes; we turn `\n`, `\u{…}`, `\xHH`, etc. into real
                // characters here so downstream phases see the literal
                // content rather than its source-form spelling. Invalid
                // escapes fire a diagnostic against the string's span;
                // the offending sequence is dropped from the value so
                // parsing keeps going.
                let (decoded, errs) = process_string_escapes(&text);
                for msg in errs {
                    self.diagnostics.push(
                        Diagnostic::error(code::Code::E0200_UnexpectedToken, msg)
                            .with_span(span),
                    );
                }
                Some(Expr::Literal(Literal::String(decoded)))
            }
            TokenKind::InterpStr(raw) => {
                // Interpolated string per §3.4 — `$"…$name…${expr}…"`.
                //
                // The lexer captured the raw bytes between the quotes;
                // `parse_interp_segments` walks them and produces the
                // segment list. For each `${…}` it recursively lex+parses
                // the contained expression as an ordinary Jux expression.
                self.advance();
                let segments = self.parse_interp_segments(&raw);
                Some(Expr::InterpString(InterpStringExpr {
                    segments,
                    span,
                }))
            }
            TokenKind::Bool(b) => {
                self.advance();
                Some(Expr::Literal(Literal::Bool(b)))
            }
            TokenKind::Null => {
                self.advance();
                Some(Expr::Literal(Literal::Null))
            }
            TokenKind::Int(text) => {
                self.advance();
                let lit = parse_int_literal_text(&text);
                Some(Expr::Literal(Literal::Int(lit)))
            }
            TokenKind::Float(text) => {
                self.advance();
                let lit = parse_float_literal_text(&text);
                Some(Expr::Literal(Literal::Float(lit)))
            }
            TokenKind::Ident(_) => {
                // Single identifier. Dotted member access (`a.b.c`) is
                // built up by `parse_postfix` as a chain of `FieldExpr`,
                // not by greedy `.`-consumption here. That keeps the
                // shape of `arr.length` as `Field(Path([arr]), length)`
                // — distinct from a multi-segment qualified name like
                // an import path.
                let ident = self.parse_ident()?;
                let span = ident.span;
                Some(Expr::Path(QualifiedName { segments: vec![ident], span }))
            }
            TokenKind::Kw(Keyword::This) => {
                // `this` — the implicit receiver inside a class
                // constructor or instance method per §7.3. Just a leaf
                // expression here; field access (`this.x`) is built up
                // through the postfix chain (`parse_postfix`).
                let span = self.peek_span();
                self.advance();
                return Some(Expr::This(span));
            }
            TokenKind::Kw(Keyword::Switch) => {
                // `switch (expr) { case PATTERN -> body; … }` per
                // §A.2.8. The same expression node serves the
                // expression form (`var y = switch (…) {…}`) and the
                // statement form (`switch (…) { … }`) — the latter
                // appears here too and is wrapped by `Stmt::Expr` in
                // statement-parsing position.
                return self.parse_switch_expr().map(Expr::Switch);
            }
            TokenKind::Kw(Keyword::New) => {
                // Three `new …` forms per §A.2.9:
                //
                //   new T(args)       -- class instantiation
                //   new T[size]       -- fixed-size array, zero-initialized
                //   new T[]{a, b, c}  -- array literal, size inferred
                //
                // We discriminate on the token after the type: `(` for
                // a constructor call, `[` for an array form.
                let start = self.peek_span();
                self.advance(); // 'new'
                let element_name = self.parse_qualified_name();
                if element_name.segments.is_empty() {
                    return None;
                }
                // Optional explicit generic-args list — `new Box<int>(42)`.
                // Type position, so `<` is unambiguous. Wildcards
                // (`new Box<? extends T>()`) aren't legal in `new`
                // expressions; `parse_generic_args_concrete` flags
                // them as E0200 and drops them from the result.
                let generic_args = self.parse_generic_args_concrete();

                // Class-instantiation form — `new Foo(args)` or
                // `new Foo<T1,T2>(args)`.
                if self.at(&TokenKind::LParen) {
                    self.advance(); // '('
                    let args = self.parse_arg_list();
                    let end = self.peek_span();
                    self.expect(&TokenKind::RParen, "')' to close constructor arguments");
                    return Some(Expr::NewObject(NewObjectExpr {
                        class_name: element_name,
                        generic_args,
                        args,
                        span: start.join(end),
                    }));
                }

                // Array forms — fall through to the `[ … ]` path.
                let element_type = TypeRef {
                    name: element_name.clone(),
                    generic_args: Vec::new(),
                    nullable: false,
                    array_shape: None,
                    fn_shape: None,
                    span: element_name.span,
                };
                self.expect(&TokenKind::LBracket, "'[' after `new T`");

                // Discriminate between `new T[size]` (size expression
                // inside the brackets) and `new T[]{…}` (empty brackets
                // followed by an initializer block) by peeking past `[`.
                if self.eat(&TokenKind::RBracket) {
                    // `new T[]{a, b, c}` — initializer-list array literal.
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
                    return Some(Expr::NewArrayLit(NewArrayLitExpr {
                        element_type,
                        elements,
                        // `new T[]{…}` always lowers to `vec![…]`.
                        // Fixed-size literals only come from the bare
                        // `{…}` initializer form in typed-local RHS.
                        fixed: false,
                        span: start.join(end),
                    }));
                }

                let size = self.parse_expr()?;
                let end = self.peek_span();
                self.expect(&TokenKind::RBracket, "']' to close `new T[size]`");
                return Some(Expr::NewArray(NewArrayExpr {
                    element_type,
                    size: Box::new(size),
                    span: start.join(end),
                }));
            }
            TokenKind::Kw(Keyword::Sizeof) => {
                // `sizeof '(' (type | expression) ')'` per §5.9.
                //
                // The operand is parsed as an expression for parser
                // simplicity. The type-vs-value disambiguation in
                // §5.9.3 is purely syntactic and applied at lowering
                // time — see `emit_sizeof` in the backend.
                let start = self.peek_span();
                self.advance(); // 'sizeof'
                self.expect(&TokenKind::LParen, "'(' after `sizeof`");
                let operand = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "')' to close `sizeof`");
                let end = self.last_consumed_span();
                Some(Expr::SizeOf(SizeOfExpr {
                    operand: Box::new(operand),
                    span: start.join(end),
                }))
            }
            TokenKind::LParen => {
                // `( expression )` — explicit grouping per §A.2.9.
                // The parser preserves grouping by *not* wrapping the
                // inner expression in any AST node; the parens only
                // affect the precedence of subsequent operators, which
                // is already captured by the tree shape we've built.
                // (If we ever need to round-trip source faithfully —
                // e.g. for `juxc fmt` — we'll add an `Expr::Paren`
                // wrapper here.)
                self.advance(); // '('
                let inner = self.parse_expr();
                self.expect(&TokenKind::RParen, "')' to close parenthesized expression");
                inner
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(code::Code::E0200_UnexpectedToken, "expected expression")
                        .with_span(span),
                );
                None
            }
        }
    }

    /// Per §A.2.9 `arg-list = argument ( ',' argument )*`, where
    /// `argument = expression | identifier ':' expression | 'out' expr |
    /// 'move' expr`. Milestone-1 supports only positional expression args.
    pub(crate) fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        if self.at(&TokenKind::RParen) {
            return args;
        }
        loop {
            let Some(arg) = self.parse_expr() else { break };
            args.push(arg);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        args
    }

    // ------------------------------------------------------------------
    // Lambdas (§A.2.9)
    // ------------------------------------------------------------------

    /// Lookahead — true iff the cursor sits at the start of a
    /// lambda. We accept four shapes:
    ///
    /// - `identifier '->' …`           — single-param, untyped
    /// - `'async' identifier '->' …`   — same with async marker
    /// - `'(' lambda-params? ')' '->' …`
    /// - `'async' '(' lambda-params? ')' '->' …`
    ///
    /// The paren-form scan skips past a balanced `(...)` and then
    /// peeks for `->`. We don't try to peek INTO the params for a
    /// syntactic guarantee — the actual lambda parser is the
    /// authority. The lookahead just decides which top-level
    /// branch fires.
    fn looks_like_lambda_head(&self) -> bool {
        let mut i = self.pos;
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Kw(Keyword::Async))) {
            i += 1;
        }
        match self.tokens.get(i).map(|t| &t.kind) {
            Some(TokenKind::Ident(_)) => {
                matches!(self.tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::Arrow))
            }
            Some(TokenKind::LParen) => {
                // Walk to matched RParen, then check for `->`.
                let mut depth = 1usize;
                let mut j = i + 1;
                while depth > 0 {
                    match self.tokens.get(j).map(|t| &t.kind) {
                        Some(TokenKind::LParen) => depth += 1,
                        Some(TokenKind::RParen) => depth -= 1,
                        Some(TokenKind::Eof) | None => return false,
                        _ => {}
                    }
                    j += 1;
                }
                matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Arrow))
            }
            _ => false,
        }
    }

    /// Parse a lambda assuming the lookahead has confirmed it.
    /// Handles both `x -> …` and `(args) -> …` forms; consumes the
    /// optional `async` prefix.
    fn parse_lambda(&mut self) -> Option<Expr> {
        let start = self.peek_span();
        let is_async = self.eat_kw(Keyword::Async);
        let params = if self.eat(&TokenKind::LParen) {
            let mut out = Vec::new();
            if !self.at(&TokenKind::RParen) {
                loop {
                    let p_start = self.peek_span();
                    // Optional type prefix. We look two tokens
                    // ahead: `ident ident` (or `ident ('<' or '[')
                    // ... ident`) means typed; lone `ident` is
                    // untyped. The simplest heuristic that's
                    // robust enough for Phase 1: try `parse_type_ref`
                    // and rewind if there isn't a following
                    // identifier. Since rewinding the parser is
                    // fiddly here, peek instead.
                    let typed = matches!(
                        (
                            self.tokens.get(self.pos).map(|t| &t.kind),
                            self.tokens.get(self.pos + 1).map(|t| &t.kind),
                        ),
                        (Some(TokenKind::Ident(_)), Some(TokenKind::Ident(_))),
                    );
                    let ty = if typed { self.parse_type_ref() } else { None };
                    let name = self.parse_ident()?;
                    let p_end = self.last_consumed_span();
                    out.push(juxc_ast::LambdaParam {
                        ty,
                        name,
                        span: p_start.join(p_end),
                    });
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen, "')' to close lambda parameters");
            out
        } else {
            // Single-param untyped form.
            let p_start = self.peek_span();
            let name = self.parse_ident()?;
            let p_end = self.last_consumed_span();
            vec![juxc_ast::LambdaParam { ty: None, name, span: p_start.join(p_end) }]
        };
        self.expect(&TokenKind::Arrow, "'->' in lambda");
        let body = if self.at(&TokenKind::LBrace) {
            juxc_ast::LambdaBody::Block(Box::new(self.parse_block()))
        } else {
            juxc_ast::LambdaBody::Expr(Box::new(self.parse_expr()?))
        };
        let end = self.last_consumed_span();
        Some(Expr::Lambda(juxc_ast::LambdaExpr {
            is_async,
            params,
            body,
            span: start.join(end),
        }))
    }
}

/// Best-effort span of an [`Expr`]. Returns `Span::DUMMY` for literals
/// because the AST's `Literal` variants don't carry their own spans yet.
/// (Refactoring `Expr` to carry a uniform `span` field is a TODO; for
/// now this is fine because literals aren't valid call callees — the
/// only place we use this — in well-formed code.)
pub(crate) fn expr_span(e: &Expr) -> Span {
    match e {
        Expr::Literal(_) => Span::DUMMY,
        Expr::Path(qn) => qn.span,
        Expr::Call(c) => c.span,
        Expr::Binary(b) => b.span,
        Expr::Unary(u) => u.span,
        Expr::Range(r) => r.span,
        Expr::Cast(c) => c.span,
        Expr::SizeOf(s) => s.span,
        Expr::NewArray(n) => n.span,
        Expr::NewArrayLit(n) => n.span,
        Expr::Index(i) => i.span,
        Expr::Field(f) => f.span,
        Expr::InterpString(s) => s.span,
        Expr::This(s) => *s,
        Expr::NewObject(n) => n.span,
        Expr::Switch(s) => s.span,
        Expr::Lambda(l) => l.span,
    }
}

/// Wrap two operands and a [`BinaryOp`] into an `Expr::Binary`, joining
/// the operand spans to span the whole expression.
pub(crate) fn make_binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    let span = expr_span(&left).join(expr_span(&right));
    Expr::Binary(BinaryExpr {
        op,
        left: Box::new(left),
        right: Box::new(right),
        span,
    })
}
