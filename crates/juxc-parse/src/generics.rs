//! Generic-parameter and generic-argument parsing.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{TypeParam, TypeRef};
use juxc_lex::{Keyword, TokenKind};

use crate::Parser;

impl<'a> Parser<'a> {
    /// Parse an optional generic-parameter list per §A.2.4:
    /// `< T, U extends Drawable & Comparable, V >`.
    /// Returns an empty vec when the next token isn't `<`.
    ///
    /// **Turn-2 scope** (this revision): parameter names plus optional
    /// `extends Type ('&' Type)*` bounds clauses. Variance markers and
    /// parameter defaults remain unsupported.
    pub(crate) fn parse_generic_params(&mut self) -> Vec<TypeParam> {
        if !self.eat(&TokenKind::Lt) {
            return Vec::new();
        }
        let mut params = Vec::new();
        if !self.at(&TokenKind::Gt) {
            loop {
                let start = self.peek_span();
                let Some(name) = self.parse_ident() else { break };
                // Optional bounds list: `extends Type ('&' Type)*`.
                // We use `&` between additional bounds, matching Java's
                // intersection-bound syntax. Phase 1 lowers each bound
                // verbatim to a Rust trait bound — they must therefore
                // refer to a Jux interface (which emits as a trait).
                let mut bounds = Vec::new();
                if self.eat_kw(Keyword::Extends) {
                    loop {
                        let Some(ty) = self.parse_type_ref() else { break };
                        bounds.push(ty);
                        if !self.eat(&TokenKind::Amp) {
                            break;
                        }
                    }
                }
                let end = self.last_consumed_span();
                params.push(TypeParam { name, bounds, span: start.join(end) });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::Gt, "'>' to close generic parameters");
        params
    }

    /// Parse an optional generic-args list in **type position**:
    /// `< Type, Type, … >`. Returns an empty vec when the next token
    /// isn't `<`. Used by `parse_type_ref` and `new`-expr to consume
    /// trailing type arguments.
    ///
    /// In type position the `<` is unambiguous — Jux types only appear
    /// after a name in a typed declaration (field/param/return/local
    /// or `new T<...>(…)`), so there's no risk of confusing it with
    /// the less-than operator.
    pub(crate) fn parse_generic_args(&mut self) -> Vec<TypeRef> {
        if !self.eat(&TokenKind::Lt) {
            return Vec::new();
        }
        let mut args = Vec::new();
        if !self.at(&TokenKind::Gt) {
            loop {
                let Some(ty) = self.parse_type_ref() else { break };
                args.push(ty);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::Gt, "'>' to close generic arguments");
        args
    }
}
