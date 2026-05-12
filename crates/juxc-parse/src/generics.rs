//! Generic-parameter and generic-argument parsing.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{GenericArg, TypeParam, TypeRef, WildcardArg, WildcardBound};
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
    /// `< (Type | Wildcard), … >`. Each entry is either a concrete
    /// type (`String`, `Map<String, int>`) or a wildcard (`?`,
    /// `? extends T`, `? super T`). Returns an empty vec when the
    /// next token isn't `<`. Used by `parse_type_ref` to consume
    /// trailing type arguments.
    ///
    /// In type position the `<` is unambiguous — Jux types only appear
    /// after a name in a typed declaration (field/param/return/local
    /// or `new T<...>(…)`), so there's no risk of confusing it with
    /// the less-than operator.
    pub(crate) fn parse_generic_args(&mut self) -> Vec<GenericArg> {
        if !self.eat(&TokenKind::Lt) {
            return Vec::new();
        }
        let mut args = Vec::new();
        if !self.at(&TokenKind::Gt) {
            loop {
                let Some(arg) = self.parse_one_generic_arg() else { break };
                args.push(arg);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::Gt, "'>' to close generic arguments");
        args
    }

    /// Parse a single generic-arg slot — a wildcard (`?`, `? extends T`,
    /// `? super T`) or a concrete type. Returns `None` only when the
    /// underlying type parse fails on the non-wildcard branch.
    fn parse_one_generic_arg(&mut self) -> Option<GenericArg> {
        let start = self.peek_span();
        if self.eat(&TokenKind::Question) {
            // Bounded wildcard: `? extends T` or `? super T`. Bare
            // `?` carries no bound.
            let bound = if self.eat_kw(Keyword::Extends) {
                self.parse_type_ref().map(WildcardBound::Extends)
            } else if self.eat_kw(Keyword::Super) {
                self.parse_type_ref().map(WildcardBound::Super)
            } else {
                None
            };
            let end = self.last_consumed_span();
            return Some(GenericArg::Wildcard(WildcardArg {
                bound,
                span: start.join(end),
            }));
        }
        // Plain type slot — the common case.
        let ty = self.parse_type_ref()?;
        Some(GenericArg::Type(ty))
    }

    /// Same as [`Self::parse_generic_args`] but for `new T<…>(…)`
    /// sites, where only concrete types are valid — you can't
    /// `new Box<?>()`. Wildcards in this position trigger an
    /// `E0200` parse error and are dropped from the result so
    /// downstream consumers never see them.
    pub(crate) fn parse_generic_args_concrete(&mut self) -> Vec<TypeRef> {
        let raw = self.parse_generic_args();
        let mut out = Vec::with_capacity(raw.len());
        for arg in raw {
            match arg {
                GenericArg::Type(t) => out.push(t),
                GenericArg::Wildcard(w) => {
                    self.diagnostics.push(
                        juxc_diagnostics::Diagnostic::error(
                            juxc_diagnostics::code::Code::E0200_UnexpectedToken,
                            "wildcard generic arguments are not allowed in `new` expressions",
                        )
                        .with_span(w.span),
                    );
                }
            }
        }
        out
    }
}
