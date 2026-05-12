//! Type and function declaration parsers — class, record, enum, interface,
//! field, constructor, function, params, return type, implements clause.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{
    ClassDecl, ConstructorDecl, EnumDecl, EnumPayload, EnumVariant, FieldDecl, FnDecl, FnModifier,
    InterfaceDecl, OperatorDecl, OperatorKind, Param, RecordComponent, RecordDecl, ReturnType,
    TypeRef, Visibility,
};
use juxc_diagnostics::{code, Diagnostic};
use juxc_lex::{Keyword, TokenKind};

use crate::Parser;

impl<'a> Parser<'a> {
    // ------------------------------------------------------------------
    // Class declarations (Turn 1 — §A.2.4 class-decl subset)
    // ------------------------------------------------------------------

    /// Parse `class Name { … }`. Visibility is consumed by the caller.
    ///
    /// **Turn-1 scope**: no modifiers beyond visibility, no generics,
    /// no `extends`/`implements`, no `permits`. Members: fields,
    /// constructors, and methods only. At most one constructor.
    pub(crate) fn parse_class_decl(
        &mut self,
        visibility: Visibility,
        is_abstract: bool,
    ) -> Option<ClassDecl> {
        let start = self.peek_span();
        self.expect_kw(Keyword::Class, "expected `class` keyword");
        let name = self.parse_ident()?;
        // Optional generic parameters per §A.2.4 `generic-params`.
        // Turn-1 form is `<T>` / `<T, U>` — no bounds, no defaults.
        let generic_params = self.parse_generic_params();
        // Optional `extends Type` clause per §A.2.4. Phase 1 supports
        // single-inheritance only; multiple extends would be a parse
        // error from the next `extends` after the first.
        let extends = if self.eat_kw(Keyword::Extends) {
            self.parse_type_ref()
        } else {
            None
        };
        // Optional `implements I, J, …` clause per §A.2.4.
        // Backend lowers each interface into a delegating
        // `impl I for Class` block.
        let implements = self.parse_implements_clause();
        self.expect(&TokenKind::LBrace, "'{' to start class body");

        let mut fields = Vec::new();
        let mut constructors = Vec::new();
        let mut methods = Vec::new();
        let mut operators = Vec::new();

        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let member_vis = self.parse_visibility();
            // Three dispatch shapes after visibility:
            //   1. `Name(` → constructor (name matches class).
            //   2. `Type Name(` → method.
            //   3. `Type Name [= expr]? ;` → field.
            // Distinguish ctor from method by peeking past the first
            // identifier: if it's `(`, this is a constructor — the
            // first identifier IS the class name, not a return type.
            let is_ctor = match (self.peek(), self.tokens.get(self.pos + 1).map(|t| &t.kind)) {
                (TokenKind::Ident(text), Some(TokenKind::LParen)) => text == &name.text,
                _ => false,
            };
            if is_ctor {
                let ctor = self.parse_constructor_decl(member_vis)?;
                if !constructors.is_empty() {
                    // Turn 1 enforces single-ctor. Multiple ctors are a
                    // documented Turn-2 feature.
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "Turn-1 classes support only one constructor (overloading lands later)",
                        )
                        .with_span(ctor.span),
                    );
                }
                constructors.push(ctor);
                continue;
            }
            // Operator-decl lookahead per §O.2: an operator member has
            // the shape `[modifiers] returnType operator <symbol>(...)`.
            // If after the return type we see `Kw(Operator)`, we route
            // to the operator parser. This check runs **before** the
            // method/field discriminator so an `operator==` definition
            // isn't mis-classified as a field.
            let lookahead_is_operator = {
                let mut i = self.pos;
                // Skip modifiers (same set the method lookahead skips).
                while matches!(
                    self.tokens.get(i).map(|t| &t.kind),
                    Some(TokenKind::Kw(Keyword::Abstract))
                        | Some(TokenKind::Kw(Keyword::Static))
                        | Some(TokenKind::Kw(Keyword::Final))
                        | Some(TokenKind::Kw(Keyword::Const))
                        | Some(TokenKind::Kw(Keyword::Async))
                        | Some(TokenKind::Kw(Keyword::Native))
                        | Some(TokenKind::Kw(Keyword::Unsafe)),
                ) {
                    i += 1;
                }
                // Skip the return type token. `void` is its own keyword;
                // any other return type starts with an `Ident`. After
                // either, the next token must be `Kw(Operator)` for
                // this to be an operator declaration.
                let after_type = match self.tokens.get(i).map(|t| &t.kind) {
                    Some(TokenKind::Kw(Keyword::Void)) => Some(i + 1),
                    Some(TokenKind::Ident(_)) => Some(i + 1),
                    _ => None,
                };
                match after_type.and_then(|j| self.tokens.get(j).map(|t| &t.kind)) {
                    Some(TokenKind::Kw(Keyword::Operator)) => true,
                    _ => false,
                }
            };
            if lookahead_is_operator {
                if let Some(op) = self.parse_operator_decl(member_vis) {
                    operators.push(op);
                }
                continue;
            }

            // Method vs field by trailing punctuation after `Type Name`.
            // If `(` follows → method. If `=` or `;` → field.
            //
            // Method-only shapes can start with `void` (a keyword); a
            // field can't be `void`-typed, so we accept Kw(Void) only
            // for the method lookahead branch.
            let lookahead_is_method = {
                let mut i = self.pos;
                // Skip any leading method modifiers (`abstract`,
                // `static`, `final`, etc.). The class-member dispatch
                // sees them BEFORE the return type, so we have to walk
                // past them here too — otherwise `public abstract
                // String speak();` would be mis-classified as a field
                // because the lookahead would land on `abstract`
                // instead of `String`.
                while matches!(
                    self.tokens.get(i).map(|t| &t.kind),
                    Some(TokenKind::Kw(Keyword::Abstract))
                        | Some(TokenKind::Kw(Keyword::Static))
                        | Some(TokenKind::Kw(Keyword::Final))
                        | Some(TokenKind::Kw(Keyword::Const))
                        | Some(TokenKind::Kw(Keyword::Async))
                        | Some(TokenKind::Kw(Keyword::Native))
                        | Some(TokenKind::Kw(Keyword::Unsafe)),
                ) {
                    i += 1;
                }
                let starts_with_type = matches!(
                    self.tokens.get(i).map(|t| &t.kind),
                    Some(TokenKind::Ident(_)) | Some(TokenKind::Kw(Keyword::Void)),
                );
                if starts_with_type {
                    i += 1;
                    // Skip array dims attached to the type (not legal
                    // on `void` returns, but we let parse_return_type
                    // surface that diagnostic if it happens).
                    while matches!(
                        self.tokens.get(i).map(|t| &t.kind),
                        Some(TokenKind::LBracket)
                    ) {
                        i += 1;
                        let mut depth = 1;
                        while depth > 0 {
                            match self.tokens.get(i).map(|t| &t.kind) {
                                Some(TokenKind::LBracket) => depth += 1,
                                Some(TokenKind::RBracket) => depth -= 1,
                                Some(TokenKind::Eof) | None => break,
                                _ => {}
                            }
                            i += 1;
                        }
                    }
                    // Optional `?` nullable marker.
                    if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Question)) {
                        i += 1;
                    }
                    // Member name.
                    if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Ident(_))) {
                        i += 1;
                        matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen))
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if lookahead_is_method {
                let method = self.parse_fn_decl(member_vis)?;
                methods.push(method);
            } else {
                let field = self.parse_field_decl(member_vis)?;
                fields.push(field);
            }
        }

        self.expect(&TokenKind::RBrace, "'}' to close class body");
        let end = self.last_consumed_span();
        Some(ClassDecl {
            visibility,
            is_abstract,
            name,
            generic_params,
            extends,
            implements,
            fields,
            constructors,
            methods,
            operators,
            span: start.join(end),
        })
    }

    /// Parse an optional `implements Type, Type, …` clause. Returns an
    /// empty vec when the `implements` keyword isn't present. Used by
    /// `parse_class_decl` (and reused by record/enum parsers).
    pub(crate) fn parse_implements_clause(&mut self) -> Vec<TypeRef> {
        if !self.eat_kw(Keyword::Implements) {
            return Vec::new();
        }
        let mut tys = Vec::new();
        loop {
            let Some(ty) = self.parse_type_ref() else { break };
            tys.push(ty);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        tys
    }

    // ------------------------------------------------------------------
    // Interface declarations (Turn 1 — §A.2.4 interface-decl subset)
    // ------------------------------------------------------------------

    /// Parse `interface Name<T> { signature; signature; }`.
    ///
    /// **Turn-1 scope**: method signatures only — `void foo();`,
    /// `int bar(int x);`. No default-method bodies, no static members,
    /// no constants, no `extends` between interfaces.
    pub(crate) fn parse_interface_decl(&mut self, visibility: Visibility) -> Option<InterfaceDecl> {
        let start = self.peek_span();
        self.expect_kw(Keyword::Interface, "expected `interface` keyword");
        let name = self.parse_ident()?;
        let generic_params = self.parse_generic_params();
        self.expect(&TokenKind::LBrace, "'{' to start interface body");

        let mut methods = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let method_vis = self.parse_visibility();
            // Reuse `parse_fn_decl` — its semicolon-or-block body
            // dispatch lets it land an abstract signature naturally
            // when the user writes `void foo();`.
            let Some(method) = self.parse_fn_decl(method_vis) else { break };
            if method.body.is_some() {
                // Default-method bodies aren't supported in Turn 1.
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "interface method bodies (default methods) aren't supported yet",
                    )
                    .with_span(method.span),
                );
            }
            methods.push(method);
        }

        self.expect(&TokenKind::RBrace, "'}' to close interface body");
        let end = self.last_consumed_span();
        Some(InterfaceDecl {
            visibility,
            name,
            generic_params,
            methods,
            span: start.join(end),
        })
    }

    /// Parse a single field declaration: `Type name [= expr] ;`.
    /// Visibility has already been consumed by the caller.
    pub(crate) fn parse_field_decl(&mut self, visibility: Visibility) -> Option<FieldDecl> {
        let start = self.peek_span();
        let ty = self.parse_type_ref()?;
        let name = self.parse_ident()?;
        let default = if self.eat(&TokenKind::Eq) {
            self.parse_expr()
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon, "';' to end field declaration");
        let end = self.last_consumed_span();
        Some(FieldDecl { visibility, ty, name, default, span: start.join(end) })
    }

    /// Parse a constructor: `Name(params) { body }`. The leading
    /// identifier is the class name (already validated by the caller).
    pub(crate) fn parse_constructor_decl(&mut self, visibility: Visibility) -> Option<ConstructorDecl> {
        let start = self.peek_span();
        // Consume the class-name identifier (matches the surrounding class).
        self.parse_ident()?;
        self.expect(&TokenKind::LParen, "'(' to start constructor parameter list");
        let params = self.parse_param_list();
        self.expect(&TokenKind::RParen, "')' to close constructor parameter list");
        let body = self.parse_block();
        let end = self.last_consumed_span();
        Some(ConstructorDecl { visibility, params, body, span: start.join(end) })
    }

    // ------------------------------------------------------------------
    // Enum declarations (Turn 1 — §A.2.4 enum-decl subset)
    // ------------------------------------------------------------------

    /// Parse `enum Name { Variant, Variant(Type, Type), … ; }`.
    /// Visibility has already been consumed.
    ///
    /// **Turn-1 scope**: variants only — no methods, no `@layout`
    /// annotation, no explicit discriminants (`Foo = 200`), no
    /// generics. Variants are comma-separated; a trailing comma or
    /// `;` is tolerated.
    // ------------------------------------------------------------------
    // Record declarations (Turn 1 — §A.2.4 record-decl subset)
    // ------------------------------------------------------------------

    /// Parse `record Name<T>(Type a, Type b) [{}]`. The leading
    /// `record` keyword is the current token; visibility has already
    /// been consumed by the caller.
    ///
    /// **Turn-1 scope**: header form only. Body — if present — must be
    /// empty (a pair of braces). Methods, compact constructors, and
    /// secondary constructors arrive in a follow-up turn. `implements`
    /// clauses are silently dropped.
    pub(crate) fn parse_record_decl(&mut self, visibility: Visibility) -> Option<RecordDecl> {
        let start = self.peek_span();
        self.expect_kw(Keyword::Record, "expected `record` keyword");
        let name = self.parse_ident()?;
        let generic_params = self.parse_generic_params();

        // Header components — `(Type name, Type name, …)`.
        self.expect(&TokenKind::LParen, "'(' to start record header");
        let mut components = Vec::new();
        if !self.at(&TokenKind::RParen) {
            loop {
                let comp_start = self.peek_span();
                let ty = self.parse_type_ref()?;
                let comp_name = self.parse_ident()?;
                let comp_end = self.last_consumed_span();
                components.push(RecordComponent {
                    ty,
                    name: comp_name,
                    span: comp_start.join(comp_end),
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "')' to close record header");

        // Optional `implements …` clause — Turn 1 parses and discards.
        if self.eat_kw(Keyword::Implements) {
            // Consume a comma-separated type list, ignore for now.
            loop {
                if self.parse_type_ref().is_none() {
                    break;
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }

        // Optional body — operator overrides and methods (per
        // grammar §A.2.4: `record-body = '{' ( function-decl |
        // static-init-block )* '}'`). The static-init-block form
        // isn't parsed yet. Records don't allow additional instance
        // fields or extra constructors — the header components are
        // the only fields, and the canonical constructor is
        // synthesized.
        let mut operators = Vec::new();
        let mut methods = Vec::new();
        if self.eat(&TokenKind::LBrace) {
            while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                let member_vis = self.parse_visibility();
                // Member-shape lookahead: walk past modifiers and
                // the return type, then check whether the next
                // token is `operator` (→ operator decl) or an
                // identifier (→ method decl). Anything else is a
                // shape error.
                let mut i = self.pos;
                while matches!(
                    self.tokens.get(i).map(|t| &t.kind),
                    Some(TokenKind::Kw(Keyword::Abstract))
                        | Some(TokenKind::Kw(Keyword::Static))
                        | Some(TokenKind::Kw(Keyword::Final))
                        | Some(TokenKind::Kw(Keyword::Const))
                        | Some(TokenKind::Kw(Keyword::Async))
                        | Some(TokenKind::Kw(Keyword::Native))
                        | Some(TokenKind::Kw(Keyword::Unsafe)),
                ) {
                    i += 1;
                }
                let after_type = match self.tokens.get(i).map(|t| &t.kind) {
                    Some(TokenKind::Kw(Keyword::Void)) => Some(i + 1),
                    Some(TokenKind::Ident(_)) => Some(i + 1),
                    _ => None,
                };
                let next_kind = after_type.and_then(|j| self.tokens.get(j).map(|t| &t.kind));
                match next_kind {
                    Some(TokenKind::Kw(Keyword::Operator)) => {
                        if let Some(op) = self.parse_operator_decl(member_vis) {
                            operators.push(op);
                        }
                    }
                    Some(TokenKind::Ident(_)) => {
                        // Method shape: `[modifiers] returnType
                        // methodName(params) { ... }`. Reuses the
                        // class fn-decl parser unchanged.
                        if let Some(m) = self.parse_fn_decl(member_vis) {
                            methods.push(m);
                        }
                    }
                    _ => {
                        let here = self.peek_span();
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0200_UnexpectedToken,
                                "record bodies support operator overrides and methods only \
                                 (fields and constructors are class-exclusive)",
                            )
                            .with_span(here),
                        );
                        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                            self.advance();
                        }
                    }
                }
            }
            self.expect(&TokenKind::RBrace, "'}' to close record body");
        }

        let end = self.last_consumed_span();
        Some(RecordDecl {
            visibility,
            name,
            generic_params,
            components,
            operators,
            methods,
            span: start.join(end),
        })
    }

    pub(crate) fn parse_enum_decl(&mut self, visibility: Visibility) -> Option<EnumDecl> {
        let start = self.peek_span();
        self.expect_kw(Keyword::Enum, "expected `enum` keyword");
        let name = self.parse_ident()?;
        self.expect(&TokenKind::LBrace, "'{' to start enum body");

        let mut variants = Vec::new();
        // Variant list runs until `;` (terminator before optional methods,
        // ignored in Turn 1) or `}` (end of body).
        while !self.at(&TokenKind::RBrace)
            && !self.at(&TokenKind::Semicolon)
            && !self.at_eof()
        {
            let variant = self.parse_enum_variant()?;
            variants.push(variant);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }

        // Optional operator section after `;` per §7.7.1 — operator
        // overrides (and `= delete;` suppression per §O.3.4). Enums
        // rarely need this since the spec's variant-order semantics
        // cover most cases, but `operator string() = delete;` for
        // security-sensitive enums is a real use case.
        let mut operators = Vec::new();
        if self.eat(&TokenKind::Semicolon) {
            while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                let member_vis = self.parse_visibility();
                // Reuse the class/record member-lookahead: after
                // visibility + modifiers + return type, expect
                // `operator`. Anything else is rejected.
                let is_operator = {
                    let mut i = self.pos;
                    while matches!(
                        self.tokens.get(i).map(|t| &t.kind),
                        Some(TokenKind::Kw(Keyword::Abstract))
                            | Some(TokenKind::Kw(Keyword::Static))
                            | Some(TokenKind::Kw(Keyword::Final))
                            | Some(TokenKind::Kw(Keyword::Const))
                            | Some(TokenKind::Kw(Keyword::Async))
                            | Some(TokenKind::Kw(Keyword::Native))
                            | Some(TokenKind::Kw(Keyword::Unsafe)),
                    ) {
                        i += 1;
                    }
                    let after_type = match self.tokens.get(i).map(|t| &t.kind) {
                        Some(TokenKind::Kw(Keyword::Void)) => Some(i + 1),
                        Some(TokenKind::Ident(_)) => Some(i + 1),
                        _ => None,
                    };
                    matches!(
                        after_type.and_then(|j| self.tokens.get(j).map(|t| &t.kind)),
                        Some(TokenKind::Kw(Keyword::Operator)),
                    )
                };
                if is_operator {
                    if let Some(op) = self.parse_operator_decl(member_vis) {
                        operators.push(op);
                    }
                } else {
                    let here = self.peek_span();
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "enum bodies only support `operator` declarations after the `;` \
                             terminator (methods aren't supported yet)",
                        )
                        .with_span(here),
                    );
                    while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                        self.advance();
                    }
                }
            }
        }

        self.expect(&TokenKind::RBrace, "'}' to close enum body");
        let end = self.last_consumed_span();
        Some(EnumDecl { visibility, name, variants, operators, span: start.join(end) })
    }

    /// Parse one enum variant: `Name` or `Name(Type [name], …)`.
    ///
    /// Payload slots accept an optional name after the type (Jux's
    /// record-style payload form `Ok(int status, String body)`); the
    /// name is captured but the Turn-1 backend emits Rust tuple
    /// variants and ignores it.
    pub(crate) fn parse_enum_variant(&mut self) -> Option<EnumVariant> {
        let start = self.peek_span();
        let name = self.parse_ident()?;
        let payload = if self.eat(&TokenKind::LParen) {
            let mut slots = Vec::new();
            if !self.at(&TokenKind::RParen) {
                loop {
                    let slot_start = self.peek_span();
                    let ty = self.parse_type_ref()?;
                    // Optional payload name: `int status` → name=status.
                    let slot_name = if matches!(self.peek(), TokenKind::Ident(_)) {
                        Some(self.parse_ident()?)
                    } else {
                        None
                    };
                    let slot_end = self.last_consumed_span();
                    slots.push(EnumPayload {
                        ty,
                        name: slot_name,
                        span: slot_start.join(slot_end),
                    });
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen, "')' to close variant payload");
            slots
        } else {
            Vec::new()
        };
        let end = self.last_consumed_span();
        Some(EnumVariant { name, payload, span: start.join(end) })
    }

    // ------------------------------------------------------------------
    // Function declarations (§A.2.4)
    //
    //   function-decl = modifier* return-type identifier
    //                   generic-params? '(' param-list? ')' throws-clause?
    //                   function-body
    // ------------------------------------------------------------------

    /// Parse a function declaration. Visibility has already been consumed
    /// by [`Self::parse_top_level_decl`]. On unrecoverable failure returns
    /// `None`.
    pub(crate) fn parse_fn_decl(&mut self, visibility: Visibility) -> Option<FnDecl> {
        let start = self.peek_span();

        let modifiers = self.parse_fn_modifiers();
        let return_type = self.parse_return_type()?;
        let name = self.parse_ident()?;
        // Optional generic parameters per §A.2.4. `<T>` between name
        // and `(`. Turn-1 limitation: no bounds, no defaults.
        let generic_params = self.parse_generic_params();

        self.expect(&TokenKind::LParen, "'(' to start parameter list");
        let params = self.parse_param_list();
        self.expect(&TokenKind::RParen, "')' to close parameter list");

        // throws-clause is unimplemented for milestone 1.
        let throws = Vec::new();

        // function-body = block | '=' expression ';' | ';'
        let body = if self.eat(&TokenKind::Semicolon) {
            // Abstract or native — no body.
            None
        } else {
            Some(self.parse_block())
        };

        let end = self.last_consumed_span();
        Some(FnDecl {
            visibility,
            modifiers,
            return_type,
            name,
            generic_params,
            params,
            throws,
            body,
            span: start.join(end),
        })
    }

    /// Parse one `operator OP(...) { ... }` declaration. Caller has
    /// already consumed the visibility token. Shape (per
    /// `JUX-OPERATORS-ADDENDUM.md` §O.2):
    ///
    /// ```text
    /// operator-decl = visibility? modifier* return-type? 'operator'
    ///                 operator-symbol '(' param-list ')' block
    /// ```
    ///
    /// Notes:
    /// - `return-type` is parsed as for [`parse_fn_decl`]. Most
    ///   operators have a fixed return type per spec (`bool` for `==`,
    ///   `int` for `<=>` and `hash`, `String` for `string`); we let
    ///   the user write whatever they want and defer the validation to
    ///   tycheck.
    /// - `operator-symbol` dispatches on the next token: punctuation
    ///   tokens map directly, and the bareword forms `hash` / `string`
    ///   appear as `Ident` tokens.
    /// - `throws` clauses aren't parsed yet (same as `parse_fn_decl`).
    /// - **`= delete;` form** per §O.3.4 — after the parameter list, if
    ///   the next token is `=`, expect `delete ;` and set
    ///   `is_deleted = true` with `body = None`. Useful for records to
    ///   suppress auto-derived operators (`operator string() = delete;`
    ///   hides a security-sensitive field from default formatting).
    pub(crate) fn parse_operator_decl(&mut self, visibility: Visibility) -> Option<OperatorDecl> {
        let start = self.peek_span();

        // Skip modifiers — same set as parse_fn_decl uses. The current
        // AST doesn't store them for operators (operators don't have
        // `static` / `abstract` semantics yet); we consume and drop.
        let _modifiers = self.parse_fn_modifiers();
        let return_type = self.parse_return_type()?;
        self.expect_kw(Keyword::Operator, "expected `operator` keyword");
        let kind = self.parse_operator_symbol()?;

        self.expect(&TokenKind::LParen, "'(' to start operator parameter list");
        let params = self.parse_param_list();
        self.expect(&TokenKind::RParen, "')' to close operator parameter list");

        // `= delete;` form per §O.3.4. The `delete` token isn't a
        // reserved keyword (it's only meaningful in this position), so
        // we match it as an Ident with exact text. The trailing `;`
        // closes the declaration.
        let (body, is_deleted) = if self.eat(&TokenKind::Eq) {
            let delete_ok = matches!(self.peek(), TokenKind::Ident(s) if s == "delete");
            if !delete_ok {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "expected `delete;` after `=` in operator declaration",
                    )
                    .with_span(self.peek_span()),
                );
            } else {
                self.advance(); // consume `delete`
            }
            self.expect(&TokenKind::Semicolon, "';' after `= delete`");
            (None, true)
        } else {
            (Some(self.parse_block()), false)
        };

        let end = self.last_consumed_span();
        Some(OperatorDecl {
            visibility,
            kind,
            params,
            return_type,
            body,
            is_deleted,
            span: start.join(end),
        })
    }

    /// Parse the operator symbol that follows the `operator` keyword,
    /// returning the matching [`OperatorKind`]. On unrecognized
    /// symbols, emits `E0200` and returns `None`.
    ///
    /// Punctuator symbols (`==`, `<=>`, `+`, …) come straight from the
    /// lexer's token kinds; the bareword forms `hash` and `string`
    /// arrive as `Ident` tokens since they aren't reserved keywords.
    /// Two-token combinations: `[]` is `LBracket RBracket`, `[]=` is
    /// `LBracket RBracket Eq`, `()` is `LParen RParen`.
    fn parse_operator_symbol(&mut self) -> Option<OperatorKind> {
        let span = self.peek_span();
        let kind = match self.peek() {
            TokenKind::EqEq => { self.advance(); OperatorKind::Eq }
            TokenKind::Spaceship => { self.advance(); OperatorKind::Cmp }
            TokenKind::Lt => { self.advance(); OperatorKind::Lt }
            TokenKind::Le => { self.advance(); OperatorKind::Le }
            TokenKind::Gt => { self.advance(); OperatorKind::Gt }
            TokenKind::Ge => { self.advance(); OperatorKind::Ge }
            TokenKind::Plus => { self.advance(); OperatorKind::Plus }
            TokenKind::Minus => { self.advance(); OperatorKind::Minus }
            TokenKind::Star => { self.advance(); OperatorKind::Mul }
            TokenKind::Slash => { self.advance(); OperatorKind::Div }
            TokenKind::Percent => { self.advance(); OperatorKind::Rem }
            TokenKind::Amp => { self.advance(); OperatorKind::BitAnd }
            TokenKind::Pipe => { self.advance(); OperatorKind::BitOr }
            TokenKind::Caret => { self.advance(); OperatorKind::BitXor }
            TokenKind::Tilde => { self.advance(); OperatorKind::BitNot }
            TokenKind::LtLt => { self.advance(); OperatorKind::Shl }
            TokenKind::GtGt => { self.advance(); OperatorKind::Shr }
            TokenKind::DotDot => { self.advance(); OperatorKind::Range }
            TokenKind::DotDotEq => { self.advance(); OperatorKind::RangeInclusive }
            TokenKind::LBracket => {
                // `[]` (index read) vs `[]=` (index write). Both start
                // with `[ ]`; the assignment form adds a trailing `=`.
                self.advance();
                if !self.eat(&TokenKind::RBracket) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "expected `]` after `[` in operator symbol",
                        )
                        .with_span(self.peek_span()),
                    );
                    return None;
                }
                if self.eat(&TokenKind::Eq) {
                    OperatorKind::IndexSet
                } else {
                    OperatorKind::Index
                }
            }
            TokenKind::LParen => {
                // `()` — call operator. Look for matching `)` and
                // consume both. Avoids confusion with the upcoming
                // parameter list's own `(`.
                self.advance();
                if !self.eat(&TokenKind::RParen) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "expected `)` to close `()` operator symbol",
                        )
                        .with_span(self.peek_span()),
                    );
                    return None;
                }
                OperatorKind::Call
            }
            TokenKind::Ident(text) if text == "hash" => {
                self.advance();
                OperatorKind::Hash
            }
            TokenKind::Ident(text) if text == "string" => {
                self.advance();
                OperatorKind::ToString
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "expected an operator symbol (`==`, `<=>`, `+`, `[]`, `hash`, `string`, …)",
                    )
                    .with_span(span),
                );
                return None;
            }
        };
        Some(kind)
    }

    /// Per §A.2.4 `modifier = 'static' | binding-immut | 'abstract' |
    /// 'async' | 'native' | 'unsafe' | 'override'`. Consumes as many in a
    /// row as appear. `final` and `const` are synonyms per §5.6; we
    /// canonicalize both to `FnModifier::Final`.
    pub(crate) fn parse_fn_modifiers(&mut self) -> Vec<FnModifier> {
        let mut mods = Vec::new();
        loop {
            if self.eat_kw(Keyword::Static)        { mods.push(FnModifier::Static); }
            else if self.eat_kw(Keyword::Final)    { mods.push(FnModifier::Final); }
            else if self.eat_kw(Keyword::Const)    { mods.push(FnModifier::Final); }
            else if self.eat_kw(Keyword::Abstract) { mods.push(FnModifier::Abstract); }
            else if self.eat_kw(Keyword::Native)   { mods.push(FnModifier::Native); }
            else if self.eat_kw(Keyword::Unsafe)   { mods.push(FnModifier::Unsafe); }
            // `async` here belongs to return-type per §A.2.4, not as a
            // standalone modifier; we don't consume it in the modifier
            // loop.
            else { break; }
        }
        mods
    }

    /// Per §A.2.4 `return-type = 'void' | type | 'async' type`.
    pub(crate) fn parse_return_type(&mut self) -> Option<ReturnType> {
        if self.eat_kw(Keyword::Void) {
            return Some(ReturnType::Void);
        }
        if self.eat_kw(Keyword::Async) {
            let ty = self.parse_type_ref()?;
            return Some(ReturnType::AsyncType(ty));
        }
        if matches!(self.peek(), TokenKind::Ident(_)) {
            let ty = self.parse_type_ref()?;
            return Some(ReturnType::Type(ty));
        }
        let span = self.peek_span();
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0200_UnexpectedToken,
                "expected return type ('void', 'async T', or a type name)",
            )
            .with_span(span),
        );
        None
    }

    /// Per §A.2.4 `param-list = param ( ',' param )*` — minimal form for
    /// milestone 1: empty or a flat list of `type ident` params, no `out`,
    /// no defaults, no variadics.
    pub(crate) fn parse_param_list(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        if self.at(&TokenKind::RParen) {
            return params;
        }
        loop {
            let Some(param) = self.parse_param() else { break };
            params.push(param);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        params
    }

    /// Per §A.2.4 `param = annotation* param-mode? type identifier ('=' expression)?`.
    /// Minimal form for milestone 1: no annotations, no mode, no default.
    pub(crate) fn parse_param(&mut self) -> Option<Param> {
        let start = self.peek_span();
        let ty = self.parse_type_ref()?;
        let name = self.parse_ident()?;
        let end = self.last_consumed_span();
        Some(Param { name, ty, default: None, span: start.join(end) })
    }
}
