//! Switch-expression and pattern parsing (§A.2.8 + §A.3) — Turn 1 subset.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{Literal, Pattern, SwitchArm, SwitchBody, SwitchExpr};
use juxc_diagnostics::{code, Diagnostic};
use juxc_lex::{Keyword, TokenKind};

use crate::literals::{parse_float_literal_text, parse_int_literal_text};
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

        self.expect(&TokenKind::Arrow, "'->' after pattern in switch arm");

        // Body: a `{`-led block, or a single expression terminated by `;`.
        let body = if self.at(&TokenKind::LBrace) {
            SwitchBody::Block(self.parse_block())
        } else {
            let expr = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon, "';' after switch arm body");
            SwitchBody::Expr(Box::new(expr))
        };
        let end = self.last_consumed_span();
        Some(SwitchArm { pattern, body, span: start.join(end) })
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
            TokenKind::Bool(b) => {
                let b = *b;
                self.advance();
                Some(Pattern::Literal(Literal::Bool(b), self.last_consumed_span()))
            }
            TokenKind::Null => {
                self.advance();
                Some(Pattern::Literal(Literal::Null, self.last_consumed_span()))
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
                        "expected a pattern (`_`, literal, `var name`, or enum variant)",
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
    fn try_parse_range_tail(
        &mut self,
        start_lit: &Literal,
        start_span: Span,
    ) -> Option<Pattern> {
        let inclusive = match self.peek() {
            TokenKind::DotDot => false,
            TokenKind::DotDotEq => true,
            _ => return None,
        };
        self.advance(); // consume `..` / `..=`
        // The end literal must be Int / Float to keep the pattern
        // a valid Rust range. We restrict to numeric literals at
        // Phase 1 (string / bool ranges aren't a thing).
        match self.peek().clone() {
            TokenKind::Int(text) => {
                self.advance();
                let lit = parse_int_literal_text(&text);
                let end_lit = Literal::Int(lit);
                let end_span = self.last_consumed_span();
                Some(Pattern::Range {
                    start: start_lit.clone(),
                    end: end_lit,
                    inclusive,
                    span: start_span.join(end_span),
                })
            }
            TokenKind::Float(text) => {
                self.advance();
                let lit = parse_float_literal_text(&text);
                let end_lit = Literal::Float(lit);
                let end_span = self.last_consumed_span();
                Some(Pattern::Range {
                    start: start_lit.clone(),
                    end: end_lit,
                    inclusive,
                    span: start_span.join(end_span),
                })
            }
            _ => {
                let here = self.peek_span();
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "expected a numeric literal after `..[=]` in a range pattern",
                    )
                    .with_span(here),
                );
                None
            }
        }
    }
}
