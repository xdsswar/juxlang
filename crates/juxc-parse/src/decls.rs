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
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
        is_abstract: bool,
        is_final: bool,
        is_sealed: bool,
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
        // Optional `permits Foo, Bar, …` clause — only meaningful on
        // a sealed class. Parsed unconditionally so a `permits` on a
        // non-sealed class can be flagged with a diagnostic.
        let permits = self.parse_permits_clause();
        if !is_sealed && !permits.is_empty() {
            // Permits without sealed — fold into the same diagnostic
            // surface so the user sees one tidy message.
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "`permits` clause is only valid on a `sealed` class",
                )
                .with_span(name.span),
            );
        }
        self.expect(&TokenKind::LBrace, "'{' to start class body");

        let mut fields = Vec::new();
        let mut constructors = Vec::new();
        let mut methods = Vec::new();
        let mut operators = Vec::new();
        let mut nested_types: Vec<juxc_ast::TopLevelDecl> = Vec::new();

        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            // Per grammar §A.2.4 each class member may carry its own
            // annotations — captured first, then routed to the
            // member's parser.
            let member_anns = self.parse_annotations();
            let member_vis = self.parse_visibility();
            // Nested-type lookahead: walk forward without consuming
            // through `[static] [abstract|final|sealed]*` and see
            // if a `class`/`interface`/`record`/`enum` keyword
            // eventually appears. If yes, commit (consume the
            // modifiers + dispatch to the matching parser); else
            // leave the cursor alone so the field/method/operator
            // path can do its own static/abstract/final consumption.
            {
                let mut probe = self.pos;
                if matches!(
                    self.tokens.get(probe).map(|t| &t.kind),
                    Some(TokenKind::Kw(Keyword::Static))
                ) {
                    probe += 1;
                }
                while matches!(
                    self.tokens.get(probe).map(|t| &t.kind),
                    Some(TokenKind::Kw(Keyword::Abstract))
                        | Some(TokenKind::Kw(Keyword::Final))
                        | Some(TokenKind::Kw(Keyword::Sealed))
                ) {
                    probe += 1;
                }
                let is_nested_keyword = matches!(
                    self.tokens.get(probe).map(|t| &t.kind),
                    Some(TokenKind::Kw(Keyword::Class))
                        | Some(TokenKind::Kw(Keyword::Interface))
                        | Some(TokenKind::Kw(Keyword::Record))
                        | Some(TokenKind::Kw(Keyword::Enum))
                );
                if is_nested_keyword {
                    let _ = self.eat_kw(Keyword::Static);
                    let is_abstract = self.eat_kw(Keyword::Abstract);
                    let is_final = self.eat_kw(Keyword::Final);
                    let is_sealed = self.eat_kw(Keyword::Sealed);
                    let nested = match self.peek() {
                        TokenKind::Kw(Keyword::Class) => self
                            .parse_class_decl(
                                member_anns.clone(),
                                member_vis,
                                is_abstract,
                                is_final,
                                is_sealed,
                            )
                            .map(juxc_ast::TopLevelDecl::Class),
                        TokenKind::Kw(Keyword::Interface) => self
                            .parse_interface_decl(member_anns.clone(), member_vis)
                            .map(juxc_ast::TopLevelDecl::Interface),
                        TokenKind::Kw(Keyword::Record) => self
                            .parse_record_decl(member_anns.clone(), member_vis)
                            .map(juxc_ast::TopLevelDecl::Record),
                        TokenKind::Kw(Keyword::Enum) => self
                            .parse_enum_decl(member_anns.clone(), member_vis)
                            .map(juxc_ast::TopLevelDecl::Enum),
                        _ => None,
                    };
                    if let Some(nt) = nested {
                        nested_types.push(nt);
                        continue;
                    }
                }
            }
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
                let ctor = self.parse_constructor_decl(member_anns, member_vis)?;
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
                    // Skip a generic-arg list `<...>` after the type
                    // name (`Pair<A, B>`, `Map<K, V>`, …) by
                    // balancing angle brackets. Without this the
                    // lookahead bails on the leading `<` and the
                    // dispatcher routes a perfectly good method
                    // declaration to the field parser. Mirrors the
                    // same scan in `looks_like_typed_local`.
                    if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Lt)) {
                        i += 1;
                        let mut depth: u32 = 1;
                        while depth > 0 {
                            match self.tokens.get(i).map(|t| &t.kind) {
                                Some(TokenKind::Lt) => depth += 1,
                                Some(TokenKind::Gt) => depth -= 1,
                                Some(TokenKind::Eof) | None => break,
                                _ => {}
                            }
                            i += 1;
                        }
                    }
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
                let method = self.parse_fn_decl(member_anns, member_vis)?;
                methods.push(method);
            } else {
                let field = self.parse_field_decl(member_anns, member_vis)?;
                fields.push(field);
            }
        }

        self.expect(&TokenKind::RBrace, "'}' to close class body");
        let end = self.last_consumed_span();
        Some(ClassDecl {
            annotations,
            visibility,
            is_abstract,
            is_final,
            is_sealed,
            permits,
            name,
            generic_params,
            extends,
            implements,
            fields,
            constructors,
            methods,
            operators,
            nested_types,
            span: start.join(end),
        })
    }

    /// Consume any `@Name`, `@Name(args)`, `@pkg.Name(args)`
    /// annotations at the cursor and return them in source order.
    /// Empty vec when no `@` is present. Used by every decl
    /// dispatch point (top-level, class members, etc.).
    ///
    /// Args support both positional expressions and named
    /// (`key = value`) bindings per grammar §A.2.3. Block-form
    /// annotations (`@export { … }`) are NOT recognized here
    /// — they're a future extension.
    pub(crate) fn parse_annotations(&mut self) -> Vec<juxc_ast::Annotation> {
        let mut out = Vec::new();
        while self.at(&TokenKind::At) {
            let Some(ann) = self.parse_single_annotation() else { break };
            out.push(ann);
        }
        out
    }

    fn parse_single_annotation(&mut self) -> Option<juxc_ast::Annotation> {
        let start = self.peek_span();
        self.expect(&TokenKind::At, "'@' to start annotation");
        let name = self.parse_qualified_name();
        if name.segments.is_empty() {
            return None;
        }
        let mut args = Vec::new();
        if self.eat(&TokenKind::LParen) {
            if !self.at(&TokenKind::RParen) {
                loop {
                    let arg = self.parse_single_annotation_arg()?;
                    args.push(arg);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen, "')' to close annotation arguments");
        }
        let end = self.last_consumed_span();
        Some(juxc_ast::Annotation { name, args, span: start.join(end) })
    }

    fn parse_single_annotation_arg(&mut self) -> Option<juxc_ast::AnnotationArg> {
        // Named arg shape — `identifier '=' expression`. Detected by
        // peeking two tokens ahead so the bare-identifier expression
        // case still works for `@Cfg(linux)` etc.
        let is_named = matches!(
            (
                self.tokens.get(self.pos).map(|t| &t.kind),
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
            ),
            (Some(TokenKind::Ident(_)), Some(TokenKind::Eq)),
        );
        if is_named {
            let name = self.parse_ident()?;
            self.expect(&TokenKind::Eq, "'=' in named annotation arg");
            let value = self.parse_expr()?;
            return Some(juxc_ast::AnnotationArg::Named { name, value });
        }
        let value = self.parse_expr()?;
        Some(juxc_ast::AnnotationArg::Positional(value))
    }

    /// Parse a top-level constant declaration per grammar §A.2.2:
    /// ```text
    /// const-decl = ('const' | 'final') type identifier '=' expression ';'
    /// ```
    /// Caller has already consumed the visibility token and the
    /// `const` / `final` keyword. `used_final_keyword` records which
    /// spelling the user wrote so error messages echo it back.
    pub(crate) fn parse_const_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
        used_final_keyword: bool,
    ) -> Option<juxc_ast::ConstDecl> {
        let start = self.peek_span();
        let ty = self.parse_type_ref()?;
        let name = self.parse_ident()?;
        self.expect(&TokenKind::Eq, "'=' in const declaration");
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after const declaration");
        let end = self.last_consumed_span();
        Some(juxc_ast::ConstDecl {
            annotations,
            visibility,
            used_final_keyword,
            ty,
            name,
            value,
            span: start.join(end),
        })
    }

    /// Parse a `type Name<...>? = TypeRef;` declaration per grammar
    /// §A.2.4. Caller has already eaten the visibility token and
    /// confirmed the next token is `type`.
    pub(crate) fn parse_type_alias_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
    ) -> Option<juxc_ast::TypeAliasDecl> {
        let start = self.peek_span();
        self.expect_kw(Keyword::Type, "expected `type` keyword");
        let name = self.parse_ident()?;
        let generic_params = self.parse_generic_params();
        self.expect(&TokenKind::Eq, "'=' in type-alias declaration");
        let target = self.parse_type_ref()?;
        self.expect(&TokenKind::Semicolon, "';' after type-alias declaration");
        let end = self.last_consumed_span();
        Some(juxc_ast::TypeAliasDecl {
            annotations,
            visibility,
            name,
            generic_params,
            target,
            span: start.join(end),
        })
    }

    /// Parse an optional `permits Foo, Bar, …` clause. Returns an
    /// empty vec when `permits` isn't the next token. Each entry is
    /// the bare class identifier — generic args aren't accepted in
    /// the permits position (you list the kind, not an instantiation).
    pub(crate) fn parse_permits_clause(&mut self) -> Vec<juxc_ast::Ident> {
        if !self.eat_kw(Keyword::Permits) {
            return Vec::new();
        }
        let mut names = Vec::new();
        loop {
            let Some(name) = self.parse_ident() else { break };
            names.push(name);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        names
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
    pub(crate) fn parse_interface_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
    ) -> Option<InterfaceDecl> {
        let start = self.peek_span();
        self.expect_kw(Keyword::Interface, "expected `interface` keyword");
        let name = self.parse_ident()?;
        let generic_params = self.parse_generic_params();
        self.expect(&TokenKind::LBrace, "'{' to start interface body");

        let mut methods = Vec::new();
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            // Interface methods don't yet take their own annotations
            // in the Phase-1 cut — leave empty.
            let member_annotations = Vec::new();
            let member_vis = self.parse_visibility();
            // Field-vs-method lookahead. Per `classes-rules.md` §3.3
            // any field in an interface is implicitly `public static
            // final`, so we accept `int X = 10;` as the canonical
            // shape and tolerate redundant `static` / `final` /
            // `const` prefixes. The discriminator: after walking
            // past field-only modifiers and the type, we land on
            // either `Ident =`/`Ident ;` (field) or `Ident (` (method).
            let lookahead_is_field = {
                let mut i = self.pos;
                while matches!(
                    self.tokens.get(i).map(|t| &t.kind),
                    Some(TokenKind::Kw(Keyword::Static))
                        | Some(TokenKind::Kw(Keyword::Final))
                        | Some(TokenKind::Kw(Keyword::Const)),
                ) {
                    i += 1;
                }
                let starts_with_type = matches!(
                    self.tokens.get(i).map(|t| &t.kind),
                    Some(TokenKind::Ident(_)),
                );
                if starts_with_type {
                    i += 1;
                    // Eat any `<T, ...>` generic arg list — naive depth
                    // counter handles nested generics.
                    if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Lt)) {
                        let mut depth = 1;
                        i += 1;
                        while depth > 0 {
                            match self.tokens.get(i).map(|t| &t.kind) {
                                Some(TokenKind::Lt) => depth += 1,
                                Some(TokenKind::Gt) => depth -= 1,
                                Some(TokenKind::Eof) | None => break,
                                _ => {}
                            }
                            i += 1;
                        }
                    }
                    // Optional `?` nullable marker — fields can be
                    // nullable just like locals.
                    if matches!(
                        self.tokens.get(i).map(|t| &t.kind),
                        Some(TokenKind::Question),
                    ) {
                        i += 1;
                    }
                    // The member identifier sits here. If `=`/`;`
                    // follows, we're parsing a field; if `(` follows,
                    // it's a method.
                    if matches!(
                        self.tokens.get(i).map(|t| &t.kind),
                        Some(TokenKind::Ident(_)),
                    ) {
                        i += 1;
                        matches!(
                            self.tokens.get(i).map(|t| &t.kind),
                            Some(TokenKind::Eq) | Some(TokenKind::Semicolon),
                        )
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if lookahead_is_field {
                // Interfaces don't admit `private` / `protected` on
                // members (§3.3) — emit a diagnostic but still parse
                // so the rest of the body is recovered.
                if matches!(member_vis, Visibility::Private | Visibility::Protected) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "interface fields cannot be `private` or `protected` — they are implicitly public",
                        )
                        .with_span(self.peek_span()),
                    );
                }
                if let Some(mut field) = self.parse_field_decl(member_annotations, member_vis) {
                    // Per §3.3: interface fields are implicitly
                    // public static final — promote the declared
                    // visibility to public if the user wrote
                    // package-private, and force is_static /
                    // is_final on whatever was parsed. An interface
                    // field without an initializer would have no
                    // value (interfaces don't have constructors),
                    // so require one here.
                    if field.default.is_none() {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0200_UnexpectedToken,
                                "interface field must be initialized — every interface field is implicitly `public static final`",
                            )
                            .with_span(field.span),
                        );
                    }
                    field.visibility = Visibility::Public;
                    field.is_static = true;
                    field.is_final = true;
                    fields.push(field);
                }
                continue;
            }
            // Java-style interface-method modifiers per
            // `JUX-LANG-V1.md` §7.6:
            //
            // - **abstract** (no modifier, no body) — implementing
            //   class must provide.
            // - **default** (body required) — implementing class
            //   inherits the body unless it overrides.
            // - **static** (body required) — interface-scoped;
            //   not inherited; accessed as `Interface.method()`.
            //
            // `default` and `static` are mutually exclusive. The
            // body-vs-no-body contract is enforced after
            // parse_fn_decl below.
            let method_annotations = member_annotations;
            let method_vis = member_vis;
            let is_default = self.eat_kw(Keyword::Default);
            let default_kw_span = if is_default {
                Some(self.last_consumed_span())
            } else {
                None
            };
            let is_static = self.eat_kw(Keyword::Static);
            let static_kw_span = if is_static {
                Some(self.last_consumed_span())
            } else {
                None
            };
            if is_default && is_static {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "`default` and `static` are mutually exclusive on interface methods",
                    )
                    .with_span(static_kw_span.unwrap_or_else(|| self.peek_span())),
                );
            }
            // Reuse `parse_fn_decl` — its semicolon-or-block body
            // dispatch lets it land an abstract signature naturally
            // when the user writes `void foo();`.
            let Some(mut method) = self.parse_fn_decl(method_annotations, method_vis)
            else { break };
            // Promote `static` to the method's modifier list so
            // backend / tycheck see it the same way as static
            // class methods. (parse_fn_decl already absorbed any
            // modifiers it found inside its own pre-loop; the
            // `static` here was consumed before that and needs
            // to be re-attached.)
            if is_static {
                method
                    .modifiers
                    .push(juxc_ast::FnModifier::Static);
            }
            // Enforce the body / no-body contract.
            let has_body = method.body.is_some();
            match (is_default, is_static, has_body) {
                (true, _, false) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "`default` interface method must have a body",
                        )
                        .with_span(default_kw_span.unwrap_or(method.span)),
                    );
                }
                (_, true, false) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "`static` interface method must have a body",
                        )
                        .with_span(static_kw_span.unwrap_or(method.span)),
                    );
                }
                (false, false, true) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "interface method with a body must be marked `default` \
                             or `static`; omit the body to declare an abstract method",
                        )
                        .with_span(method.span),
                    );
                }
                _ => {}
            }
            methods.push(method);
        }

        self.expect(&TokenKind::RBrace, "'}' to close interface body");
        let end = self.last_consumed_span();
        Some(InterfaceDecl {
            annotations,
            visibility,
            name,
            generic_params,
            methods,
            fields,
            span: start.join(end),
        })
    }

    /// Parse a single field declaration: `[static] [final|const] Type name [= expr] ;`.
    /// Visibility has already been consumed by the caller. `static`
    /// promotes the field to class scope; `final` / `const` (same
    /// meaning) marks it non-reassignable and picks the
    /// `pub const`-shape Rust emission for statics.
    pub(crate) fn parse_field_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
    ) -> Option<FieldDecl> {
        let start = self.peek_span();
        let mut is_static = false;
        let mut is_final = false;
        loop {
            if self.eat_kw(Keyword::Static) {
                is_static = true;
            } else if self.eat_kw(Keyword::Final) || self.eat_kw(Keyword::Const) {
                is_final = true;
            } else {
                break;
            }
        }
        let ty = self.parse_type_ref()?;
        let name = self.parse_ident()?;
        let default = if self.eat(&TokenKind::Eq) {
            self.parse_expr()
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon, "';' to end field declaration");
        let end = self.last_consumed_span();
        Some(FieldDecl {
            annotations,
            visibility,
            is_static,
            is_final,
            ty,
            name,
            default,
            span: start.join(end),
        })
    }

    /// Parse a constructor: `Name(params) { body }`. The leading
    /// identifier is the class name (already validated by the caller).
    pub(crate) fn parse_constructor_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
    ) -> Option<ConstructorDecl> {
        let start = self.peek_span();
        // Consume the class-name identifier (matches the surrounding class).
        self.parse_ident()?;
        self.expect(&TokenKind::LParen, "'(' to start constructor parameter list");
        let params = self.parse_param_list();
        self.expect(&TokenKind::RParen, "')' to close constructor parameter list");
        let body = self.parse_block();
        let end = self.last_consumed_span();
        Some(ConstructorDecl {
            annotations,
            visibility,
            params,
            body,
            span: start.join(end),
        })
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
    pub(crate) fn parse_record_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
    ) -> Option<RecordDecl> {
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
        let mut static_fields: Vec<juxc_ast::FieldDecl> = Vec::new();
        if self.eat(&TokenKind::LBrace) {
            while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                let member_vis = self.parse_visibility();
                // Member-shape lookahead: walk past modifiers and
                // the return type, then probe what follows. Three
                // shapes are recognized:
                //   - `operator …` → operator override
                //   - `IDENT(` → method declaration
                //   - `IDENT =` / `IDENT ;` → static field (Java
                //     records allow these; instance fields are
                //     still forbidden — we reject any `is_static =
                //     false` field with a clean diagnostic below)
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
                // For field detection we may need to skip a generic
                // arg list right after the type identifier
                // (`Map<K, V> CONSTANT = …;`).
                let after_type_skipped: Option<usize> = after_type.map(|mut j| {
                    if matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Lt)) {
                        j += 1;
                        let mut depth: u32 = 1;
                        while depth > 0 {
                            match self.tokens.get(j).map(|t| &t.kind) {
                                Some(TokenKind::Lt) => depth += 1,
                                Some(TokenKind::Gt) => depth -= 1,
                                Some(TokenKind::Eof) | None => break,
                                _ => {}
                            }
                            j += 1;
                        }
                    }
                    j
                });
                let next_kind = after_type.and_then(|j| self.tokens.get(j).map(|t| &t.kind));
                let after_member_name: Option<&TokenKind> =
                    after_type_skipped.and_then(|j| self.tokens.get(j + 1).map(|t| &t.kind));
                match next_kind {
                    Some(TokenKind::Kw(Keyword::Operator)) => {
                        if let Some(op) = self.parse_operator_decl(member_vis) {
                            operators.push(op);
                        }
                    }
                    Some(TokenKind::Ident(_))
                        if matches!(
                            after_member_name,
                            Some(TokenKind::Eq) | Some(TokenKind::Semicolon)
                        ) =>
                    {
                        // Static-field shape. Reuse the class
                        // field-decl parser (which already handles
                        // modifiers + initializer); reject any
                        // non-static result so the "no instance
                        // fields" rule still bites.
                        if let Some(field) = self.parse_field_decl(Vec::new(), member_vis) {
                            if !field.is_static {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0200_UnexpectedToken,
                                        "records cannot have instance fields — the header \
                                         components are the only instance state; mark this \
                                         field `static` or move it to a class",
                                    )
                                    .with_span(field.span),
                                );
                            } else {
                                static_fields.push(field);
                            }
                        }
                    }
                    Some(TokenKind::Ident(_)) => {
                        // Method shape: `[modifiers] returnType
                        // methodName(params) { ... }`. Reuses the
                        // class fn-decl parser unchanged.
                        if let Some(m) = self.parse_fn_decl(Vec::new(), member_vis) {
                            methods.push(m);
                        }
                    }
                    _ => {
                        let here = self.peek_span();
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0200_UnexpectedToken,
                                "record bodies support operator overrides, methods, and \
                                 static fields only (instance fields and extra \
                                 constructors are class-exclusive)",
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
            annotations,
            visibility,
            name,
            generic_params,
            components,
            operators,
            methods,
            static_fields,
            span: start.join(end),
        })
    }

    pub(crate) fn parse_enum_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
    ) -> Option<EnumDecl> {
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
        Some(EnumDecl {
            annotations,
            visibility,
            name,
            variants,
            operators,
            span: start.join(end),
        })
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
    pub(crate) fn parse_fn_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
    ) -> Option<FnDecl> {
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
            annotations,
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
    // `Final` and `Const` deliberately push the same value
    // (synonyms per §5.6); the branch-per-keyword shape keeps
    // the cascade readable. Clippy's `if_same_then_else` is a
    // false positive here — silenced locally.
    #[allow(clippy::if_same_then_else)]
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

    /// Per §A.2.4 `return-type = 'void' | type | 'async' ( type | 'void' )`.
    ///
    /// `async void` is a common shape from JUX-ASYNC-ADDENDUM-v2 (fire-and-
    /// forget async work; e.g. `async void main()`), so we accept it as a
    /// proper return-type variant. It still lowers to `async fn name() -> ()`
    /// in Rust — the `()` return type is represented by an `AsyncType` whose
    /// inner `TypeRef` carries the `void` sentinel name. To keep the
    /// downstream representation uniform with the existing `ReturnType::Void`
    /// shape, we emit the `()` directly through a fresh `ReturnType::Void`
    /// wrapped semantically as async via the `is_async` flag on the
    /// surrounding `FnDecl` — but since the AST doesn't carry that flag
    /// separately today, we synthesize a sentinel `TypeRef { name: "void" }`
    /// that the backend's return-type emitter recognizes.
    pub(crate) fn parse_return_type(&mut self) -> Option<ReturnType> {
        if self.eat_kw(Keyword::Void) {
            return Some(ReturnType::Void);
        }
        if self.eat_kw(Keyword::Async) {
            // `async void` → synthesize a unit-typed AsyncType so the
            // backend's `ReturnType::AsyncType(t)` arm still emits the
            // `async fn name() -> T` shape with T being the unit `()`.
            if self.eat_kw(Keyword::Void) {
                let span = self.last_consumed_span();
                let void_ty = juxc_ast::TypeRef {
                    name: juxc_ast::QualifiedName {
                        segments: vec![juxc_ast::Ident {
                            text: "void".to_string(),
                            span,
                        }],
                        span,
                    },
                    generic_args: Vec::new(),
                    nullable: false,
                    array_shape: None,
                    fn_shape: None,
                    span,
                };
                return Some(ReturnType::AsyncType(void_ty));
            }
            let ty = self.parse_type_ref()?;
            return Some(ReturnType::AsyncType(ty));
        }
        if matches!(self.peek(), TokenKind::Ident(_)) {
            let ty = self.parse_type_ref()?;
            return Some(ReturnType::Type(ty));
        }
        // A function-type return — `(A) -> R foo() { … }`. The `(`
        // gives the function-type's parameter list away, so route
        // through `parse_type_ref` which already handles the
        // function-type prefix.
        if matches!(self.peek(), TokenKind::LParen) {
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
