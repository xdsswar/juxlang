//! Switch-expression and pattern parsing (§A.2.8 + §A.3) — Turn 1 subset.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{Literal, Pattern, SwitchArm, SwitchBody, SwitchExpr};
use juxc_diagnostics::{code, Diagnostic};
use juxc_lex::{Keyword, TokenKind};

use crate::literals::{parse_float_literal_text, parse_int_literal_text, process_string_escapes};
use crate::Parser;

impl<'a> Parser<'a> {
    /// Parse a `switch (expr) { case PATTERN -> body; … default -> body; }`
    /// form. The leading `switch` keyword is the current token.
    pub(crate) fn parse_switch_expr(&mut self) -> Option<SwitchExpr> {
        let start = self.peek_span();
        self.expect_kw(Keyword::Switch, "expected `switch` keyword");
        self.expect(&TokenKind::LParen, "'(' after `switch`");
        let scrutinee = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "')' after switch scrutinee");
        self.expect(&TokenKind::LBrace, "'{' to start switch body");

        let mut arms = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let Some(arm) = self.parse_switch_arm() else {
                break;
            };
            arms.push(arm);
        }
        self.expect(&TokenKind::RBrace, "'}' to close switch body");
        let end = self.last_consumed_span();
        Some(SwitchExpr {
            scrutinee: Box::new(scrutinee),
            arms,
            span: start.join(end),
        })
    }

    /// Parse a single `case PATTERN -> BODY` or `default -> BODY` arm.
    /// Body is either an expression terminated by `;` or a brace block.
    pub(crate) fn parse_switch_arm(&mut self) -> Option<SwitchArm> {
        let start = self.peek_span();
        let pattern = if self.eat_kw(Keyword::Default) {
            // `default ->` is sugar for `case _ ->`; lower it to a
            // Wildcard pattern so the backend has a single path.
            Pattern::Wildcard(self.last_consumed_span())
        } else if self.eat_kw(Keyword::Case) {
            self.parse_pattern()?
        } else {
            let here = self.peek_span();
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "expected `case` or `default` in switch arm",
                )
                .with_span(here),
            );
            return None;
        };

        // Or-pattern alternatives: `case A | B | C ->` (§A.3), and the
        // comma list `case A, B, C ->` (JUX-LANG-V1 §7.5), which the grammar
        // defines as the same thing. Both fold into one Pattern::Or, so the
        // checker, exhaustiveness and the backend see a single shape. A
        // pattern's own commas (`Point(var x, 0)`, `(a, b)`) sit inside its
        // parentheses and are consumed by `parse_pattern`, so a comma here
        // always separates two whole patterns.
        let pattern = if self.at(&TokenKind::Pipe) || self.at(&TokenKind::Comma) {
            let pstart = pattern.span();
            let mut alts = vec![pattern];
            while self.eat(&TokenKind::Pipe) || self.eat(&TokenKind::Comma) {
                match self.parse_pattern() {
                    // `case A | B, C` is one flat list of three alternatives.
                    Some(Pattern::Or(inner, _)) => alts.extend(inner),
                    Some(next) => alts.push(next),
                    None => break,
                }
            }
            let pend = self.last_consumed_span();
            Pattern::Or(alts, pstart.join(pend))
        } else {
            pattern
        };
        // Optional `when <cond>` guard (§A.2.8). Guarded arms don't
        // count toward exhaustiveness (§T.5.6) — tycheck enforces.
        let guard = if self.eat_kw(Keyword::When) {
            // The arm's arrow is the first `->` outside any brackets.
            let mut depth = 0usize;
            let mut arrow = None;
            for (i, tok) in self.tokens.iter().enumerate().skip(self.pos) {
                match &tok.kind {
                    TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                    TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    TokenKind::Arrow if depth == 0 => {
                        arrow = Some(i);
                        break;
                    }
                    TokenKind::Semicolon | TokenKind::Eof if depth == 0 => break,
                    _ => {}
                }
            }
            let prev = std::mem::replace(&mut self.switch_arm_arrow, arrow);
            let guard = self.parse_expr();
            self.switch_arm_arrow = prev;
            guard
        } else {
            None
        };
        self.expect(&TokenKind::Arrow, "'->' after pattern in switch arm");

        // Body: a `{`-led block, a `throw`, or a single expression terminated
        // by `;`. `default -> throw new IllegalStateException(..);` is the
        // arrow switch's own idiom (Java 14+, EXCEPTIONS addendum): the throw
        // is a statement, so it becomes the arm's one-statement block.
        let body = if self.at(&TokenKind::LBrace) {
            SwitchBody::Block(self.parse_block())
        } else if self.at_kw(Keyword::Throw) {
            let body_start = self.peek_span();
            let stmt = self.parse_stmt()?;
            SwitchBody::Block(juxc_ast::Block {
                statements: vec![stmt],
                span: body_start.join(self.last_consumed_span()),
            })
        } else {
            let body_start = self.peek_span();
            let expr = self.parse_expr()?;
            // JUX-LANG-V1 7.5 says the arrow form "matches Java 14+
            // arrow-switch syntax exactly", and there an arm body may be any
            // expression STATEMENT -- an assignment among them. Jux's
            // assignment is a statement rather than an expression, so
            // `case 3 -> r = "high";` used to parse the expression `r` and
            // then reject the `=`. Fold it into the one-statement block the
            // user would otherwise have to write by hand.
            let assign = if self.at(&TokenKind::Eq) {
                Some(self.parse_assignment_tail(expr.clone(), None)?)
            } else if let Some(op) = crate::stmts::compound_assign_op(self.peek()) {
                Some(self.parse_assignment_tail(expr.clone(), Some(op))?)
            } else {
                None
            };
            match assign {
                Some(stmt) => SwitchBody::Block(juxc_ast::Block {
                    statements: vec![stmt],
                    span: body_start.join(self.last_consumed_span()),
                }),
                None => {
                    self.expect(&TokenKind::Semicolon, "';' after switch arm body");
                    SwitchBody::Expr(Box::new(expr))
                }
            }
        };
        let end = self.last_consumed_span();
        Some(SwitchArm { pattern, guard, body, span: start.join(end) })
    }

    /// Parse one pattern per §A.3 — Turn-1 subset: literal, wildcard,
    /// `var name` bind, enum-variant `Path[.Variant](sub, …)`.
    ///
    /// Disambiguation when the pattern starts with an identifier:
    /// - `var name` — bind.
    /// - `_` — wildcard.
    /// - `Path[.Variant]` optionally followed by `(sub-patterns…)` —
    ///   enum-variant pattern. A single-segment bare ident in pattern
    ///   position with no parens is treated as the path form, not a
    ///   bind — the user should write `var name` for binding to be
    ///   explicit. (Spec §A.3 binding-pattern says bare-ident bind is
    ///   only legal in tuple/record context.)
    pub(crate) fn parse_pattern(&mut self) -> Option<Pattern> {
        let start = self.peek_span();
        match self.peek() {
            // `_` — wildcard.
            TokenKind::Ident(text) if text == "_" => {
                self.advance();
                Some(Pattern::Wildcard(self.last_consumed_span()))
            }
            // `var name` — bind.
            TokenKind::Kw(Keyword::Var) => {
                self.advance();
                let name = self.parse_ident()?;
                Some(Pattern::Bind(name))
            }
            // `-5`, `-1.5`, and the range `-10..0`: a minus sign is part of a
            // numeric literal pattern (a pattern has no operators).
            TokenKind::Minus
                if matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokenKind::Int(_) | TokenKind::Float(_))
                ) =>
            {
                let first_lit = self.parse_range_bound()?;
                let first_span = start.join(self.last_consumed_span());
                if let Some(range) = self.try_parse_range_tail(&first_lit, first_span) {
                    return Some(range);
                }
                Some(Pattern::Literal(first_lit, first_span))
            }
            // `..x` (below x) and `..=x` (x or less): ranges open at the
            // bottom (M.6.4).
            TokenKind::DotDot | TokenKind::DotDotEq => {
                let inclusive = matches!(self.peek(), TokenKind::DotDotEq);
                self.advance();
                let end = self.parse_range_bound()?;
                Some(Pattern::Range {
                    start: None,
                    end: Some(end),
                    inclusive,
                    span: start.join(self.last_consumed_span()),
                })
            }
            // Literal patterns. Both Int and Float can optionally
            // start a range pattern (`0..10`, `'a'..='z'`) when
            // followed by a `..` / `..=` token. We parse the first
            // literal eagerly, then peek; if `..[=]` follows, parse
            // the second literal to build a Range pattern.
            TokenKind::Int(text) => {
                let text = text.clone();
                self.advance();
                let lit = parse_int_literal_text(&text);
                let first_lit = Literal::Int(lit);
                let first_span = self.last_consumed_span();
                if let Some(range) = self.try_parse_range_tail(&first_lit, first_span) {
                    return Some(range);
                }
                Some(Pattern::Literal(first_lit, first_span))
            }
            TokenKind::Float(text) => {
                let text = text.clone();
                self.advance();
                let lit = parse_float_literal_text(&text);
                let first_lit = Literal::Float(lit);
                let first_span = self.last_consumed_span();
                if let Some(range) = self.try_parse_range_tail(&first_lit, first_span) {
                    return Some(range);
                }
                Some(Pattern::Literal(first_lit, first_span))
            }
            TokenKind::Str(s) => {
                let s = s.clone();
                self.advance();
                Some(Pattern::Literal(Literal::String(s), self.last_consumed_span()))
            }
            // `'a'`, and the range `'a'..='z'` (§A.3's own example).
            TokenKind::Char(raw) => {
                let raw = raw.clone();
                self.advance();
                let first_span = self.last_consumed_span();
                let first_lit = Literal::Char(self.pattern_char(&raw, first_span));
                if let Some(range) = self.try_parse_range_tail(&first_lit, first_span) {
                    return Some(range);
                }
                Some(Pattern::Literal(first_lit, first_span))
            }
            TokenKind::Bool(b) => {
                let b = *b;
                self.advance();
                Some(Pattern::Literal(Literal::Bool(b), self.last_consumed_span()))
            }
            TokenKind::Null => {
                self.advance();
                Some(Pattern::Literal(Literal::Null, self.last_consumed_span()))
            }
            // `(p, q, …)` — tuple pattern. The grammar asks for two or more
            // elements: `(p)` would be a pattern in parentheses, which the
            // language does not have, and `()` matches nothing that exists.
            TokenKind::LParen => {
                self.advance();
                let mut elements = Vec::new();
                if !self.at(&TokenKind::RParen) {
                    loop {
                        let Some(p) = self.parse_pattern() else { break };
                        elements.push(p);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RParen, "')' to close the tuple pattern");
                let span = start.join(self.last_consumed_span());
                if elements.len() < 2 {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "a tuple pattern needs at least two elements, `(p, q)`",
                        )
                        .with_span(span),
                    );
                }
                Some(Pattern::Tuple(elements, span))
            }
            // `Path[.Variant](sub, …)` — enum-variant pattern.
            //
            // Also handles the bare type-test pattern `Type ident`
            // (no parens), which Java 21 and the Jux spec accept
            // as shorthand for "match a Type-shaped value and bind
            // it to `ident`". Detection: single-segment path
            // followed immediately by another `Ident`, with no
            // `(` or `.` between them.
            TokenKind::Ident(_) => {
                let path = self.parse_qualified_name();
                // Bare type-test bind: `Type ident` with single-seg
                // path and no parens. Promote to `TypeBind` so the
                // backend can lower it as `Sealed::Type(ident)`-
                // style destructuring without forcing the user to
                // write `Type(var ident)`.
                let bare_single_segment =
                    path.segments.len() == 1 && !self.at(&TokenKind::LParen);
                if bare_single_segment {
                    if let TokenKind::Ident(_) = self.peek() {
                        // The type name and the binder are both
                        // single-segment Idents — promote to
                        // TypeBind. The first segment from the
                        // qualified-name parse IS the type_name.
                        let type_name = path.segments.first().cloned()?;
                        let binder = self.parse_ident()?;
                        let end = self.last_consumed_span();
                        return Some(Pattern::TypeBind {
                            type_name,
                            binder,
                            span: start.join(end),
                        });
                    }
                }
                // Optional sub-pattern parens.
                let (args, has_parens) = if self.eat(&TokenKind::LParen) {
                    let mut subs = Vec::new();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            let Some(p) = self.parse_pattern() else { break };
                            subs.push(p);
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RParen, "')' to close pattern arguments");
                    (subs, true)
                } else {
                    (Vec::new(), false)
                };
                let end = self.last_consumed_span();
                Some(Pattern::EnumVariant {
                    path,
                    args,
                    has_parens,
                    span: start.join(end),
                })
            }
            _ => {
                let here = self.peek_span();
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "expected a pattern (`_`, a literal, `var name`, a tuple `(p, q)`, \
                         or a record or enum variant)",
                    )
                    .with_span(here),
                );
                None
            }
        }
    }

    /// After parsing the first literal of a pattern, check for a
    /// `..` / `..=` range continuation. Returns
    /// `Some(Pattern::Range { … })` when the lookahead matches a
    /// range, `None` otherwise (caller falls back to the plain
    /// literal pattern).
    /// Decode a character literal's raw text the way an expression does,
    /// reporting a bad escape or more than one character at `span`.
    fn pattern_char(&mut self, raw: &str, span: juxc_source::Span) -> char {
        let (decoded, errors) = process_string_escapes(raw);
        for msg in errors {
            self.diagnostics.push(Diagnostic::error(code::Code::E0200_UnexpectedToken, msg).with_span(span));
        }
        let mut chars = decoded.chars();
        let ch = chars.next().unwrap_or('\0');
        if chars.next().is_some() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "character literal must contain exactly one character",
                )
                .with_span(span),
            );
        }
        ch
    }

    fn try_parse_range_tail(
        &mut self,
        start_lit: &Literal,
        start_span: juxc_source::Span,
    ) -> Option<Pattern> {
        let inclusive = match self.peek() {
            TokenKind::DotDot => false,
            TokenKind::DotDotEq => true,
            _ => return None,
        };
        self.advance(); // consume `..` / `..=`
        // `x..` with nothing after it is open at the top ("x or more",
        // M.6.4). Only `..` can be open: `x..=` would be a closed range with
        // its end missing.
        let at_bound = matches!(
            self.peek(),
            TokenKind::Int(_) | TokenKind::Float(_) | TokenKind::Char(_) | TokenKind::Minus
        );
        if !at_bound && !inclusive {
            return Some(Pattern::Range {
                start: Some(start_lit.clone()),
                end: None,
                inclusive: false,
                span: start_span.join(self.last_consumed_span()),
            });
        }
        let end_lit = self.parse_range_bound()?;
        Some(Pattern::Range {
            start: Some(start_lit.clone()),
            end: Some(end_lit),
            inclusive,
            span: start_span.join(self.last_consumed_span()),
        })
    }

    /// One endpoint of a range pattern: an integer, float or character
    /// literal, a number possibly negative (`-10`). Reports anything else.
    fn parse_range_bound(&mut self) -> Option<Literal> {
        let negative = self.eat(&TokenKind::Minus);
        match self.peek().clone() {
            TokenKind::Int(text) => {
                self.advance();
                let mut lit = parse_int_literal_text(&text);
                if negative {
                    // Written in decimal from here on: `-0x10` is -16, and the
                    // backend prints a negative value in base 10.
                    lit.value = lit.value.wrapping_neg();
                    lit.radix = juxc_ast::IntRadix::Decimal;
                }
                Some(Literal::Int(lit))
            }
            TokenKind::Float(text) => {
                self.advance();
                let mut lit = parse_float_literal_text(&text);
                if negative {
                    lit.value = -lit.value;
                }
                Some(Literal::Float(lit))
            }
            TokenKind::Char(raw) if !negative => {
                self.advance();
                let span = self.last_consumed_span();
                Some(Literal::Char(self.pattern_char(&raw, span)))
            }
            _ => {
                let here = self.peek_span();
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "expected a number or a character literal as a range pattern's bound",
                    )
                    .with_span(here),
                );
                None
            }
        }
    }
}
