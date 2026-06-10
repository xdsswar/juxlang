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
        self.parse_ternary()
    }

    /// Ternary (conditional) layer per §A.4 level 2 — the
    /// looser-than-everything-binary form `cond ? then : else`.
    /// Right-associative: `a ? b : c ? d : e` parses as
    /// `a ? b : (c ? d : e)`. Both branches recurse into
    /// `parse_ternary` so a nested ternary on the right-hand
    /// side stacks correctly. The condition itself parses at
    /// elvis precedence (level 3) since `?` here is the
    /// ternary marker, not the postfix-error-prop operator.
    pub(crate) fn parse_ternary(&mut self) -> Option<Expr> {
        let cond = self.parse_elvis()?;
        if !matches!(self.peek(), TokenKind::Question) {
            return Some(cond);
        }
        self.advance(); // '?'
        let then_branch = self.parse_expr()?;
        self.expect(&TokenKind::Colon, "':' in ternary expression");
        let else_branch = self.parse_ternary()?;
        let span = expr_span(&cond).join(expr_span(&else_branch));
        Some(Expr::Ternary(juxc_ast::TernaryExpr {
            condition: Box::new(cond),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
            span,
        }))
    }

    /// Elvis (null-coalescing) layer per §A.4 level 3 — sits just
    /// below the ternary and just above logical-or. Right-
    /// associative: `a ?: b ?: c` parses as `a ?: (b ?: c)`. Both
    /// sides are full elvis-expressions, so a fallback that's
    /// itself a chain just nests one level deeper.
    ///
    /// Two spellings are accepted as aliases per
    /// `JUX-GRAMMAR-ADDENDUM.md` §A.1.6: `?:` (Kotlin/Groovy) and
    /// `??` (C#/JavaScript). Both produce the same `Expr::Elvis`
    /// AST node — the only difference is the surface syntax the
    /// user chose. Diagnostic spans cover the actual operator
    /// token, so an error report still names the spelling typed.
    pub(crate) fn parse_elvis(&mut self) -> Option<Expr> {
        let left = self.parse_logic_or()?;
        if matches!(
            self.peek(),
            TokenKind::QuestionColon | TokenKind::QuestionQuestion,
        ) {
            self.advance(); // '?:' or '??'
            // Right-associative: recurse into `parse_elvis` for the
            // fallback so chains stack the right way.
            let fallback = self.parse_elvis()?;
            let span = expr_span(&left).join(expr_span(&fallback));
            return Some(Expr::Elvis(juxc_ast::ElvisExpr {
                value: Box::new(left),
                fallback: Box::new(fallback),
                span,
            }));
        }
        Some(left)
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
        let mut left = self.parse_typetest()?;
        while matches!(self.peek(), TokenKind::AndAnd) {
            self.advance();
            let right = self.parse_typetest()?;
            left = make_binary(BinaryOp::And, left, right);
        }
        Some(left)
    }

    /// Type-test layer (§T.1.4) — `value => Type [binder]`. **Non-associative**
    /// and binds tighter than `&&`/`||` (so `x => Dog && y` parses as
    /// `(x => Dog) && y`), looser than the bitwise operators. The RHS is a
    /// *type* (parsed via [`Self::parse_type_ref`]) followed by an optional
    /// smart-cast binder identifier (`x => Dog d`). Yields [`Expr::TypeTest`].
    pub(crate) fn parse_typetest(&mut self) -> Option<Expr> {
        let value = self.parse_bit_or()?;
        if !matches!(self.peek(), TokenKind::FatArrow) {
            return Some(value);
        }
        self.advance(); // '=>'
        let ty = self.parse_type_ref()?;
        // Optional binder — a bare identifier right after the type
        // (`x => Dog d`). A following `&&`, `)`, etc. ends the test.
        let binder = if matches!(self.peek(), TokenKind::Ident(_)) {
            self.parse_ident()
        } else {
            None
        };
        let end = binder.as_ref().map(|b| b.span).unwrap_or(ty.span);
        let span = expr_span(&value).join(end);
        Some(Expr::TypeTest(juxc_ast::TypeTestExpr {
            value: Box::new(value),
            ty,
            binder,
            span,
        }))
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

    /// Unary layer: prefix `-`, `!`, `~`, and the C-style cast form
    /// `(T) expr` per §A.4 level 18 / §A.5.
    ///
    /// Right-associative: `--x` parses as `-(-x)`, `!!flag` as
    /// `!(!flag)`. This is implemented by recursing into
    /// `parse_unary` for the operand rather than dropping straight
    /// to postfix.
    ///
    /// **C-style cast disambiguation** (§A.5): when the cursor is at
    /// `(`, we use [`Self::looks_like_c_style_cast`] to peek past
    /// the closing `)` and check whether the parens contain a valid
    /// type AND the next token can start a unary expression. If
    /// both, `(T) expr` lowers to the same `Expr::Cast` shape as the
    /// postfix `expr as T` form — semantics are identical per the
    /// grammar addendum. Otherwise the `(` falls through to
    /// `parse_postfix` → `parse_primary`'s parenthesized-expression
    /// branch.
    ///
    /// The other §A.4-level-18 prefix operators (`+`, `move`, `await`,
    /// `&`, `*`) are not modeled yet.
    pub(crate) fn parse_unary(&mut self) -> Option<Expr> {
        // C-style cast first — looks like `(T) expr` and binds at
        // unary precedence. The lookahead is pure (no token
        // consumption, no diagnostics) so a mis-detection costs
        // nothing.
        if self.at(&TokenKind::LParen) && self.looks_like_c_style_cast() {
            return self.parse_c_style_cast();
        }
        // `await expr` per JUX-ASYNC-ADDENDUM §A.2 — a prefix unary
        // form at precedence level 18 (sibling to `-`, `!`, `~`).
        // The operand is itself parsed at unary precedence so
        // `await !flag` and `await -x` both parse correctly. The
        // tycheck pass is responsible for enforcing that `await`
        // appears only inside an `async` function body; the parser
        // accepts it anywhere and lets later phases flag misuse.
        if matches!(self.peek(), TokenKind::Kw(Keyword::Await)) {
            let start_span = self.peek_span();
            self.advance(); // 'await'
            let operand = self.parse_unary()?;
            let span = start_span.join(self.last_consumed_span());
            return Some(Expr::Await(Box::new(operand), span));
        }
        let start_span = self.peek_span();
        let op = match self.peek() {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Bang => Some(UnaryOp::Not),
            TokenKind::Tilde => Some(UnaryOp::BitNot),
            // Prefix `*` / `&` in expression position are the raw-pointer
            // deref / address-of operators (§A.2.9, `unsafe`-only). They're
            // unambiguous here — the binary `*` (multiply) and `&` (bit-and)
            // only appear after an operand, at binary precedence.
            TokenKind::Star => Some(UnaryOp::Deref),
            TokenKind::Amp => Some(UnaryOp::AddrOf),
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

    /// Peek-only check for the C-style cast shape `( TYPE ) UNARY`.
    /// Walks the token stream without consuming anything or emitting
    /// diagnostics. Returns true exactly when:
    ///
    /// - the cursor is at `(`,
    /// - the tokens after `(` form a valid type-like sequence —
    ///   either a recognized **primitive type name** (`int`,
    ///   `long`, `String`, …) optionally with `?`/`[]`/`[N]`, OR
    ///   any qualified-name carrying an explicit array/nullable
    ///   marker (`Foo?`, `Foo[]`),
    /// - the matching `)` closes the type cleanly, AND
    /// - the token after `)` can start a unary expression.
    ///
    /// **Disambiguation against parenthesized expressions** per
    /// `JUX-GRAMMAR-ADDENDUM.md` §A.5: `(int) x` is always a cast;
    /// `(x) y` (where `x` is a plain identifier with no type
    /// markers) is treated as a parenthesized expression so user
    /// code that just groups a name doesn't accidentally trip the
    /// cast path. Once name resolution can answer "is this name a
    /// type?", the bare-ident case can be re-promoted to a cast.
    ///
    /// Conservative on every ambiguity: anything we can't classify
    /// returns false, leaving `(...)` to flow through as a
    /// parenthesized expression. The real parsing still goes through
    /// [`Self::parse_type_ref`] and emits diagnostics there if the
    /// shape turns out to be wrong.
    pub(crate) fn looks_like_c_style_cast(&self) -> bool {
        let mut i = self.pos;
        // Must be at `(`.
        if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen)) {
            return false;
        }
        i += 1;
        // Qualified name: Ident ('.' Ident)*. Track the first
        // segment's text so we can decide later whether it's a
        // recognized primitive.
        let first_ident = match self.tokens.get(i).map(|t| &t.kind) {
            Some(TokenKind::Ident(s)) => s.clone(),
            _ => return false,
        };
        i += 1;
        let mut multi_segment = false;
        while matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Dot)) {
            if !matches!(self.tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::Ident(_))) {
                return false;
            }
            multi_segment = true;
            i += 2;
        }
        // Optional nullable `?`.
        let mut has_nullable = false;
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Question)) {
            has_nullable = true;
            i += 1;
        }
        // Optional array suffix `[]` or `[N]`. Generic args `<T>`
        // are skipped: `(List<int>) x` would ambiguate with
        // comparison chains and isn't worth the complexity yet.
        let mut has_array = false;
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LBracket)) {
            i += 1;
            if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Int(_))) {
                i += 1;
            }
            if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::RBracket)) {
                return false;
            }
            has_array = true;
            i += 1;
        }
        // Closing `)` of the cast.
        if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::RParen)) {
            return false;
        }
        i += 1;
        // Token after `)` must be able to start a unary expression.
        let followed_by_unary = matches!(
            self.tokens.get(i).map(|t| &t.kind),
            Some(TokenKind::Ident(_))
                | Some(TokenKind::Int(_))
                | Some(TokenKind::Float(_))
                | Some(TokenKind::Str(_))
                | Some(TokenKind::InterpStr(_))
                | Some(TokenKind::Bool(_))
                | Some(TokenKind::Null)
                | Some(TokenKind::Minus)
                | Some(TokenKind::Bang)
                | Some(TokenKind::Tilde)
                | Some(TokenKind::LParen)
                | Some(TokenKind::Kw(Keyword::New))
                | Some(TokenKind::Kw(Keyword::This))
                | Some(TokenKind::Kw(Keyword::Sizeof)),
        );
        if !followed_by_unary {
            return false;
        }
        // Final disambiguation: is the inner "type-like enough"
        // that we should treat the parens as a cast? Per the spec:
        //
        // - **Primitive name** (`int`, `String`, …) → always a cast.
        // - **User-named type with markers** (`Foo?`, `Foo[]`,
        //   `pkg.Foo[]`) → also a cast; the markers make the
        //   type-shape unambiguous.
        // - **Bare qualified name** (`Foo`, `pkg.Foo`) without any
        //   markers → could be a paren-expr around a variable
        //   reference; leave it alone until name resolution.
        if is_known_primitive_type_name(&first_ident) {
            return true;
        }
        if has_nullable || has_array || multi_segment {
            return true;
        }
        // Bare single-segment name (`(Dog) x`): a reference cast to a class /
        // interface. Treat it as a cast only when the operand begins with an
        // **unambiguous atom** — an identifier, a literal, `this`, `new`, or
        // `sizeof`. We deliberately exclude `(`, `-`, `!`, `~` (already in
        // `followed_by_unary`): `(f)(x)` is a call, `(a) - b` is a
        // subtraction, etc. A bare `(Name) atom` has no other valid reading
        // (juxtaposed expressions aren't legal), so this is unambiguous.
        matches!(
            self.tokens.get(i).map(|t| &t.kind),
            Some(TokenKind::Ident(_))
                | Some(TokenKind::Int(_))
                | Some(TokenKind::Float(_))
                | Some(TokenKind::Str(_))
                | Some(TokenKind::InterpStr(_))
                | Some(TokenKind::Bool(_))
                | Some(TokenKind::Null)
                | Some(TokenKind::Kw(Keyword::This))
                | Some(TokenKind::Kw(Keyword::New))
                | Some(TokenKind::Kw(Keyword::Sizeof)),
        )
    }

    /// Non-consuming lookahead: does the `<` at the cursor begin an
    /// explicit call-site type-argument list, `expr '<' types '>' '('`?
    ///
    /// This is the postfix turbofish form the spec gives for explicit
    /// generic calls (`id<int>(5)`, `obj.pick<String>(x)`). The `<`
    /// is otherwise the less-than operator, so we commit to the
    /// type-arg reading only when the angle list balances cleanly,
    /// contains nothing that *can't* appear in a type list, and is
    /// immediately followed by a call `(`. Same ambiguity tradeoff as
    /// C#: a pathological `a < b > (c)` double-comparison parses as a
    /// generic call — but that shape is type-nonsensical in practice.
    /// Conservative on every uncertainty: returns false so `<` flows
    /// through as a comparison operator.
    pub(crate) fn looks_like_explicit_type_args(&self) -> bool {
        let mut i = self.pos;
        if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Lt)) {
            return false;
        }
        i += 1;
        let mut depth: i32 = 1;
        // An empty `<>` is not a valid type-arg list — require at least
        // one type-ish token before the close.
        let mut saw_inner = false;
        while depth > 0 {
            match self.tokens.get(i).map(|t| &t.kind) {
                Some(TokenKind::Lt) => depth += 1,
                Some(TokenKind::Gt) => depth -= 1,
                // A glued `>>` closes two nested lists at once.
                Some(TokenKind::GtGt) => depth -= 2,
                // Tokens that legitimately appear inside a concrete
                // type-arg list: names (`int`, `pkg.Foo`), separators,
                // array markers, and the nullable `?` suffix.
                Some(TokenKind::Ident(_))
                | Some(TokenKind::Dot)
                | Some(TokenKind::Comma)
                | Some(TokenKind::LBracket)
                | Some(TokenKind::RBracket)
                | Some(TokenKind::Question) => saw_inner = true,
                // Anything else (literals, operators, `(`, EOF, …) means
                // this `<` is a comparison, not a type-arg list.
                _ => return false,
            }
            i += 1;
            if depth < 0 {
                return false;
            }
        }
        // `i` now sits just past the balanced close; an explicit
        // type-arg list is only meaningful when a call `(` follows.
        saw_inner
            && matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen))
    }

    /// Real-parse path for the C-style cast form once
    /// [`Self::looks_like_c_style_cast`] has confirmed the shape.
    /// Consumes `(`, parses the type, consumes `)`, parses the
    /// unary operand, and wraps it in `Expr::Cast` — identical to
    /// the `as T` postfix form's AST shape.
    fn parse_c_style_cast(&mut self) -> Option<Expr> {
        let start = self.peek_span();
        self.advance(); // '('
        let ty = self.parse_type_ref()?;
        self.expect(&TokenKind::RParen, "')' to close cast type");
        let operand = self.parse_unary()?;
        let span = start.join(expr_span(&operand));
        Some(Expr::Cast(CastExpr {
            value: Box::new(operand),
            ty,
            span,
        }))
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
                    expr = Expr::Call(CallExpr {
                        callee: Box::new(expr),
                        explicit_generic_args: Vec::new(),
                        args,
                        span,
                    });
                }
                // Explicit call-site type arguments: `callee<T, …>(args)`
                // (spec postfix turbofish). Only taken when the lookahead
                // confirms a balanced `<…>` immediately followed by `(`,
                // so the `<` operator path is otherwise untouched.
                TokenKind::Lt if self.looks_like_explicit_type_args() => {
                    let explicit_generic_args = self.parse_generic_args_concrete();
                    self.advance(); // '(' — guaranteed present by the lookahead
                    let args = self.parse_arg_list();
                    let end = self.peek_span();
                    self.expect(&TokenKind::RParen, "')' to close argument list");
                    let span = expr_span(&expr).join(end);
                    expr = Expr::Call(CallExpr {
                        callee: Box::new(expr),
                        explicit_generic_args,
                        args,
                        span,
                    });
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
                    // Two forms accepted here:
                    //   - `object.identifier` — the regular Java/C-like
                    //     named-member access.
                    //   - `object.0`, `object.1`, … — tuple-element
                    //     access (per JUX-LANG-V1 tuple addendum). Rust
                    //     uses the same `.0` spelling so the field
                    //     stores the numeric literal text verbatim and
                    //     the backend emits `expr.0` unchanged.
                    self.advance(); // '.'
                    let field = if let TokenKind::Int(_) = self.peek() {
                        // Tuple-element access: store the digit run as
                        // a synthetic Ident whose text is the integer
                        // literal verbatim (e.g. "0", "1"). The backend
                        // re-emits `expr.0` which matches Rust's tuple-
                        // indexing syntax exactly.
                        let span = self.peek_span();
                        let text = match self.peek() {
                            TokenKind::Int(raw) => raw.clone(),
                            _ => unreachable!(),
                        };
                        self.advance(); // consume the int literal
                        juxc_ast::Ident { text, span }
                    } else {
                        self.parse_ident()?
                    };
                    let span = expr_span(&expr).join(field.span);
                    expr = Expr::Field(FieldExpr {
                        object: Box::new(expr),
                        field,
                        safe: false,
                        span,
                    });
                }
                TokenKind::QuestionDot => {
                    // Safe-navigation: `object?.field` and `object?.method(args)`.
                    // Same parse shape as `.`, but the resulting
                    // `FieldExpr` carries `safe: true` so the
                    // backend can lower it to
                    // `object.as_ref().map(|x| x.field.clone())`.
                    // Call-form follows by the next loop iteration
                    // picking up the `(` and wrapping in `CallExpr`.
                    self.advance(); // '?.'
                    let field = self.parse_ident()?;
                    let span = expr_span(&expr).join(field.span);
                    expr = Expr::Field(FieldExpr {
                        object: Box::new(expr),
                        field,
                        safe: true,
                        span,
                    });
                }
                TokenKind::ColonColon => {
                    // Method reference: `Receiver::member` per
                    // §A.4 level 20. Only meaningful when the LHS
                    // is a single-segment Path naming a type
                    // (class / record / enum / interface). For
                    // other LHS shapes we still build the
                    // `MethodRefExpr` — the resolver / tycheck
                    // flag the bad name with the normal E0301 /
                    // E0413 diagnostics, so the parser stays
                    // permissive here.
                    self.advance(); // '::'
                    let member = self.parse_ident()?;
                    let receiver = match &expr {
                        Expr::Path(qn) => qn.clone(),
                        _ => {
                            // Synthesize an empty qualified-name
                            // and rely on the diagnostic for the
                            // shape error; expression position
                            // before `::` is supposed to be a
                            // type path.
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0200_UnexpectedToken,
                                    "left-hand side of `::` must be a type name",
                                )
                                .with_span(expr_span(&expr)),
                            );
                            return Some(expr);
                        }
                    };
                    let span = receiver.span.join(member.span);
                    expr = Expr::MethodRef(juxc_ast::MethodRefExpr {
                        receiver,
                        member,
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
            TokenKind::Kw(Keyword::Super) => {
                // `super` — superclass-qualified call receiver (§6.9.4). A
                // leaf here; the postfix chain builds `super.method(args)`.
                // tycheck rejects a bare `super` (one not followed by a
                // `.method(...)` call).
                let span = self.peek_span();
                self.advance();
                return Some(Expr::Super(span));
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
                // `new Foo<T1,T2>(args)`. Optionally followed by an
                // anonymous-class body `{ method overrides }` per
                // spec §1379. Fields and constructors are rejected
                // inside the body; static members would shadow the
                // synthetic struct's namespace and are skipped.
                if self.at(&TokenKind::LParen) {
                    self.advance(); // '('
                    let args = self.parse_arg_list();
                    let mut end = self.peek_span();
                    self.expect(&TokenKind::RParen, "')' to close constructor arguments");
                    let mut anonymous_body: Option<juxc_ast::AnonymousBody> = None;
                    if self.at(&TokenKind::LBrace) {
                        let mut methods: Vec<juxc_ast::FnDecl> = Vec::new();
                        let mut init_blocks: Vec<juxc_ast::Block> = Vec::new();
                        self.advance(); // '{'
                        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                            // Bare `{ … }` at body level → instance
                            // initializer block. Java's only
                            // constructor hook for anonymous classes
                            // (the "double-brace initialization"
                            // pattern). We parse the block as
                            // statements and run them once when the
                            // synthetic instance is constructed.
                            if self.at(&TokenKind::LBrace) {
                                let block = self.parse_block();
                                init_blocks.push(block);
                                continue;
                            }
                            // Otherwise expect a method shape:
                            // `[annotations] [visibility] [modifiers]
                            //  return-type name(params) { body }`.
                            let anns = self.parse_annotations();
                            let vis = self.parse_visibility();
                            if let Some(method) = self.parse_fn_decl(anns, vis) {
                                methods.push(method);
                            } else {
                                // Recovery: skip to the next `;` or `}`
                                // so a malformed entry doesn't loop.
                                while !self.at(&TokenKind::Semicolon)
                                    && !self.at(&TokenKind::RBrace)
                                    && !self.at_eof()
                                {
                                    self.advance();
                                }
                                if self.at(&TokenKind::Semicolon) {
                                    self.advance();
                                }
                            }
                        }
                        end = self.peek_span();
                        self.expect(&TokenKind::RBrace, "'}' to close anonymous-class body");
                        anonymous_body = Some(juxc_ast::AnonymousBody {
                            init_blocks,
                            methods,
                        });
                    }
                    return Some(Expr::NewObject(NewObjectExpr {
                        class_name: element_name,
                        generic_args,
                        args,
                        anonymous_body,
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
                    ptr_depth: 0,
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
        Expr::TypeTest(t) => t.span,
        Expr::SizeOf(s) => s.span,
        Expr::NewArray(n) => n.span,
        Expr::NewArrayLit(n) => n.span,
        Expr::Index(i) => i.span,
        Expr::Field(f) => f.span,
        Expr::InterpString(s) => s.span,
        Expr::This(s) => *s,
        Expr::Super(s) => *s,
        Expr::NewObject(n) => n.span,
        Expr::Switch(s) => s.span,
        Expr::Lambda(l) => l.span,
        Expr::Elvis(e) => e.span,
        Expr::MethodRef(m) => m.span,
        Expr::Ternary(t) => t.span,
        Expr::Await(_, s) => *s,
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

/// True when `name` is one of Jux's blessed primitive type names per
/// `JUX-LANG-V1.md` §5 — the set that the C-style cast lookahead
/// can recognize without help from name resolution. Covers both the
/// Java-family names (`int`, `long`, `String`, …) and the
/// width-explicit synonyms (`i32`, `u64`, `f64`, …). Reserved words
/// `void` and `Self` are NOT in this set: they aren't legal cast
/// targets in the first place.
pub(crate) fn is_known_primitive_type_name(name: &str) -> bool {
    // Single source of truth lives in `juxc-lex` so the compiler's cast
    // lookahead and the IntelliJ plugin's primitive set can't drift apart.
    juxc_lex::PRIMITIVE_TYPE_NAMES.contains(&name)
}
