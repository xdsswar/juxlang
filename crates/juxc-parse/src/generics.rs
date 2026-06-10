//! Generic-parameter and generic-argument parsing.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{GenericArg, TypeParam, TypeRef, WildcardArg, WildcardBound};
use juxc_lex::{Keyword, TokenKind};
use juxc_source::Span;

use crate::Parser;

/// Build the synthetic `TypeRef` that carries a **const-generic
/// argument literal** through type-shaped plumbing — single name
/// segment holding the literal text verbatim ("256", "true"). See
/// [`juxc_ast::TypeRef::const_literal_text`] for the matching
/// recognizer used by tycheck and the backend.
fn synthetic_const_arg_type_ref(raw: &str, span: Span) -> TypeRef {
    TypeRef {
        name: juxc_ast::QualifiedName {
            segments: vec![juxc_ast::Ident { text: raw.to_string(), span }],
            span,
        },
        generic_args: Vec::new(),
        nullable: false,
        array_shape: None,
        fn_shape: None,
        ptr_depth: 0,
        span,
    }
}

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
        if !self.at_generic_close() {
            loop {
                let start = self.peek_span();
                // **Const-generic parameter** (grammar §A.2.6:
                // `generic-param = 'int' identifier | type identifier`).
                // Shape: a primitive type name followed by the param
                // name — `<int N>`, `<bool B>`. Both tokens lex as
                // `Ident`, so the two-token lookahead disambiguates
                // from an ordinary `<T>` (where the *next* token is
                // `,`/`>`/`extends`, never another identifier).
                let const_ty = if let (
                    Some(TokenKind::Ident(first)),
                    Some(TokenKind::Ident(_)),
                ) = (
                    self.tokens.get(self.pos).map(|t| &t.kind),
                    self.tokens.get(self.pos + 1).map(|t| &t.kind),
                ) {
                    if crate::exprs::is_known_primitive_type_name(first) {
                        // Phase-1 core supports `int` and `bool` value
                        // types; the rest (`long`, `char`, …) parse but
                        // get a clean E0445 (deferred, not a rustc leak).
                        if first != "int" && first != "bool" {
                            self.diagnostics.push(
                                juxc_diagnostics::Diagnostic::error(
                                    juxc_diagnostics::code::Code::E0445_ConstGenericUnsupported,
                                    format!(
                                        "const-generic parameters of type `{first}` are not \
                                         supported in this phase — only `int` and `bool` are",
                                    ),
                                )
                                .with_span(start),
                            );
                        }
                        self.parse_type_ref()
                    } else {
                        None
                    }
                } else {
                    None
                };
                let Some(name) = self.parse_ident() else { break };
                // Optional bounds list: `extends Type ('&' Type)*`.
                // We use `&` between additional bounds, matching Java's
                // intersection-bound syntax. Phase 1 lowers each bound
                // verbatim to a Rust trait bound — they must therefore
                // refer to a Jux interface (which emits as a trait).
                // (A const param takes no bounds; `extends` after one
                // simply won't be present in well-formed source.)
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
                params.push(TypeParam { name, bounds, const_ty, span: start.join(end) });
                // A bound's nested generics may have parked a split `>` — that's
                // our close, so don't look for another comma-separated param.
                if self.pending_gt > 0 || !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.close_generic_angle("'>' to close generic parameters");
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
        if !self.at_generic_close() {
            loop {
                let Some(arg) = self.parse_one_generic_arg() else { break };
                args.push(arg);
                // A nested arg ending in `>>` parks a split `>` here — that `>`
                // closes THIS list (e.g. the outer `>` of `List<List<int>>`), so
                // stop before consuming a comma we don't have.
                if self.pending_gt > 0 || !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.close_generic_angle("'>' to close generic arguments");
        args
    }

    /// Parse a single generic-arg slot — a wildcard (`?`, `? extends T`,
    /// `? super T`) or a concrete type. Returns `None` only when the
    /// underlying type parse fails on the non-wildcard branch.
    fn parse_one_generic_arg(&mut self) -> Option<GenericArg> {
        let start = self.peek_span();
        // **Const-generic argument** — an integer or bool literal in a
        // generic-arg slot (`new RingBuffer<float, 256>()`,
        // `StackString<32> s`). Carried as a *synthetic* `TypeRef`
        // whose single name segment is the literal text ("256",
        // "true"): the digit-run is valid Rust verbatim in turbofish /
        // type-arg position, and keeping the `TypeRef` shape leaves
        // every `Vec<TypeRef>` consumer signature untouched. Tycheck
        // validates the slot kind (const param ⇔ literal arg) so the
        // synthetic name can't reach name resolution.
        if let TokenKind::Int(raw) = self.peek() {
            let raw = raw.clone();
            self.advance();
            let end = self.last_consumed_span();
            let span = start.join(end);
            return Some(GenericArg::Type(synthetic_const_arg_type_ref(&raw, span)));
        }
        if let TokenKind::Bool(b) = self.peek() {
            let raw = if *b { "true" } else { "false" }.to_string();
            self.advance();
            let end = self.last_consumed_span();
            let span = start.join(end);
            return Some(GenericArg::Type(synthetic_const_arg_type_ref(&raw, span)));
        }
        // A leading `-` would be a negative const arg — `<int N>` lowers
        // to Rust `const N: usize`, so negatives are out of the Phase-1
        // subset. Catch it here (it could never parse as a type anyway).
        if matches!(self.peek(), TokenKind::Minus) {
            self.diagnostics.push(
                juxc_diagnostics::Diagnostic::error(
                    juxc_diagnostics::code::Code::E0445_ConstGenericUnsupported,
                    "negative const-generic arguments are not supported in this phase",
                )
                .with_span(start),
            );
            // Consume `-` and a following literal so the arg list can
            // recover at the `,` / `>`.
            self.advance();
            if matches!(self.peek(), TokenKind::Int(_)) {
                self.advance();
            }
            return Some(GenericArg::Type(synthetic_const_arg_type_ref(
                "0",
                start.join(self.last_consumed_span()),
            )));
        }
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
