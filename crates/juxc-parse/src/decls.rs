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
        self.parse_class_like(annotations, visibility, is_abstract, is_final, is_sealed, false)
    }

    /// Parse a `struct Name { … }` declaration (grammar §A.2.5
    /// `struct-decl = visibility? 'struct' identifier generic-params?
    /// struct-body`). A Jux struct is a value-type aggregate with **no**
    /// inheritance, so it accepts neither `extends`, `implements`, nor
    /// `permits`; its body is the same field/method member set as a class.
    ///
    /// Phase 1 reuses the [`ClassDecl`] node (see [`Self::parse_class_like`]):
    /// the result is an implicitly-`final` class flagged [`ClassDecl::is_struct`]
    /// so downstream phases can recover the `struct` origin. This is what lets
    /// bindgen-generated `.jux.d` stubs (Rust structs → §G.6.3) parse, resolve,
    /// and autocomplete in Jux syntax.
    pub(crate) fn parse_struct_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
    ) -> Option<ClassDecl> {
        // Structs are implicitly final (no subtyping) and never abstract/sealed.
        self.parse_class_like(annotations, visibility, false, true, false, true)
    }

    /// Shared body of [`Self::parse_class_decl`] and [`Self::parse_struct_decl`].
    /// `is_struct` selects the leading keyword (`struct` vs `class`) and is
    /// recorded on the produced [`ClassDecl`]; everything else — generics, the
    /// optional inheritance clauses (absent on structs, so they simply parse to
    /// empty), and the member loop — is identical.
    fn parse_class_like(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
        is_abstract: bool,
        is_final: bool,
        is_sealed: bool,
        is_struct: bool,
    ) -> Option<ClassDecl> {
        let start = self.peek_span();
        if is_struct {
            self.expect_kw(Keyword::Struct, "expected `struct` keyword");
        } else {
            self.expect_kw(Keyword::Class, "expected `class` keyword");
        }
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
        let mut properties: Vec<juxc_ast::PropertyDecl> = Vec::new();
        let mut nested_types: Vec<juxc_ast::TopLevelDecl> = Vec::new();
        let mut init_blocks: Vec<juxc_ast::Block> = Vec::new();
        let mut drop_blocks: Vec<juxc_ast::Block> = Vec::new();
        let mut static_init_blocks: Vec<juxc_ast::Block> = Vec::new();

        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            // Per grammar §A.2.4 each class member may carry its own
            // annotations — captured first, then routed to the
            // member's parser.
            let member_anns = self.parse_annotations();
            let member_vis = self.parse_visibility();

            // Initializer blocks (JUX-MISSING-DEFS §M.1, JUX-SEMANTICS §S.4.1).
            // These carry no visibility / return type, just a leading keyword
            // and a block:
            //   - `static { … }` — a static-init block (runs once on first use).
            //   - `init { … }`    — an instance-init block (runs after each ctor).
            // Detected by the keyword immediately followed by `{`, so a `static`
            // FIELD or METHOD (`static int x;`, `static void m()`) is unaffected.
            if self.at_kw(Keyword::Static)
                && matches!(self.tokens.get(self.pos + 1).map(|t| &t.kind), Some(TokenKind::LBrace))
            {
                self.advance(); // 'static'
                static_init_blocks.push(self.parse_block());
                continue;
            }
            if self.at_kw(Keyword::Init)
                && matches!(self.tokens.get(self.pos + 1).map(|t| &t.kind), Some(TokenKind::LBrace))
            {
                self.advance(); // 'init'
                init_blocks.push(self.parse_block());
                continue;
            }
            // `drop { … }` — destructor block (§6.6 / §S.5). At most
            // one per class; duplicates are diagnosed in tycheck.
            if self.at_kw(Keyword::Drop)
                && matches!(self.tokens.get(self.pos + 1).map(|t| &t.kind), Some(TokenKind::LBrace))
            {
                self.advance(); // 'drop'
                drop_blocks.push(self.parse_block());
                continue;
            }

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
                        | Some(TokenKind::Kw(Keyword::Struct))
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
                        // A nested `struct` reuses the struct parser; like other
                        // nested types it is lifted to the top level as a Class
                        // node (flagged `is_struct`).
                        TokenKind::Kw(Keyword::Struct) => self
                            .parse_struct_decl(member_anns.clone(), member_vis)
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
                // The single-constructor Turn-1 limitation is enforced in
                // tycheck (`check_single_constructor`) rather than here, so a
                // `.jux.d` declaration stub — which legitimately declares
                // overloaded constructors (`HashMap()` + `HashMap(int)`,
                // JUX-BINDGEN-ADDENDUM §G.5.1/§G.5.2) — parses cleanly. The
                // parser is source-origin-agnostic and can't tell a stub from
                // a normal source; tycheck knows the unit's `is_external` flag
                // and exempts stubs from the limit.
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
                // Skip an optional Java-style leading generic-params clause
                // `<T, …>` so `public <T> T identity(...)` is classified as a
                // method (the `<` would otherwise abort the return-type scan).
                if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Lt)) {
                    i += 1;
                    let mut depth: u32 = 1;
                    while depth > 0 {
                        match self.tokens.get(i).map(|t| &t.kind) {
                            Some(TokenKind::Lt) => depth += 1,
                            Some(TokenKind::Gt) => depth -= 1,
                            Some(TokenKind::GtGt) => depth = depth.saturating_sub(2),
                            Some(TokenKind::Eof) | None => break,
                            _ => {}
                        }
                        i += 1;
                    }
                }
                // Advance `i` past the return type — a nominal type (with
                // generics / array / `?` suffixes, glued-`>>` aware) OR a
                // function type `(A) -> R` (so a member that returns a closure,
                // `(int) -> void onClick();`, is classified as a method, not a
                // field). `scan_type_at` returns `None` when no type is present.
                let after_return_type = self.scan_type_at(i);
                if let Some(j) = after_return_type {
                    i = j;
                    // Member name.
                    if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Ident(_))) {
                        i += 1;
                        // Optional method-level generic params `<T, …>` between
                        // the name and the parameter list (§A.2.4
                        // `function-decl … identifier generic-params? '('`).
                        // Without skipping these, a generic method like
                        // `T map<U>(U f)` lands on `<` instead of `(` and is
                        // misclassified as a field (→ "expected ';'"). Balance
                        // the angle brackets exactly as the return-type scan above.
                        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Lt)) {
                            i += 1;
                            let mut depth: u32 = 1;
                            while depth > 0 {
                                match self.tokens.get(i).map(|t| &t.kind) {
                                    Some(TokenKind::Lt) => depth += 1,
                                    Some(TokenKind::Gt) => depth -= 1,
                                    // A glued `>>` closes two nested generic lists.
                                    Some(TokenKind::GtGt) => depth = depth.saturating_sub(2),
                                    Some(TokenKind::Eof) | None => break,
                                    _ => {}
                                }
                                i += 1;
                            }
                        }
                        matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen))
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            // **C#-style property check** (JUX-MISSING-DEFS §M.7).
            // Scan past `[modifiers] type name` and see whether the
            // next token opens a property body — `=>` (expression-
            // bodied read-only) or `{` (accessor block). Both shapes
            // route to `parse_property_decl`, which produces a
            // lossless `PropertyDecl`. Lookahead is non-consuming.
            // (A plain field is `type name [= …] ;`; a method is
            // `type name (`. Neither can be followed by `=>` / `{`,
            // so this discriminator is unambiguous.)
            let lookahead_is_property = {
                let mut i = self.pos;
                // Skip leading modifiers (same shape as field/method).
                while matches!(
                    self.tokens.get(i).map(|t| &t.kind),
                    Some(TokenKind::Kw(Keyword::Static))
                        | Some(TokenKind::Kw(Keyword::Final))
                        | Some(TokenKind::Kw(Keyword::Const))
                ) {
                    i += 1;
                }
                // Type tokens (best-effort skip — single Ident
                // optionally followed by generics / array / nullable).
                if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Ident(_))) {
                    i += 1;
                    if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Lt)) {
                        i += 1;
                        let mut depth: u32 = 1;
                        while depth > 0 {
                            match self.tokens.get(i).map(|t| &t.kind) {
                                Some(TokenKind::Lt) => depth += 1,
                                Some(TokenKind::Gt) => depth -= 1,
                                // A glued `>>` closes two nested generic lists.
                                Some(TokenKind::GtGt) => depth = depth.saturating_sub(2),
                                Some(TokenKind::Eof) | None => break,
                                _ => {}
                            }
                            i += 1;
                        }
                    }
                    while matches!(
                        self.tokens.get(i).map(|t| &t.kind),
                        Some(TokenKind::LBracket)
                    ) {
                        i += 1;
                        let mut depth: u32 = 1;
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
                    if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Question)) {
                        i += 1;
                    }
                    // Now expect Ident + (`->` | `{`) for a property.
                    // NB: Jux uses `->` (not C#'s `=>`) for expression-bodied
                    // property/accessor bodies, because `=>` is the type-test
                    // (instanceof) operator in Jux and must stay unambiguous.
                    if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Ident(_))) {
                        i += 1;
                        matches!(
                            self.tokens.get(i).map(|t| &t.kind),
                            Some(TokenKind::Arrow) | Some(TokenKind::LBrace),
                        )
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
            } else if lookahead_is_property {
                if let Some(prop) = self.parse_property_decl(member_anns, member_vis) {
                    properties.push(prop);
                }
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
            is_struct,
            permits,
            name,
            generic_params,
            extends,
            implements,
            fields,
            constructors,
            methods,
            operators,
            properties,
            nested_types,
            init_blocks,
            static_init_blocks,
            drop_blocks,
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
        // Type may be omitted for inference: `const PI = 3.14;`. Detected by
        // `IDENT =` (the identifier is the name, with no preceding type token).
        let no_type = matches!(self.peek(), TokenKind::Ident(_))
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Eq)
            );
        let ty = if no_type { None } else { Some(self.parse_type_ref()?) };
        let name = self.parse_ident()?;
        // The initializer is optional: a `.jux.d` declaration stub declares a
        // constant by type and name only (`public const char SEP;`, §G.2 — the
        // real value lives in the foreign crate). When `=` is absent we
        // synthesize an inert `null` placeholder; the stub is never lowered and
        // its diagnostics are suppressed, so the value is never observed.
        let value = if self.eat(&TokenKind::Eq) {
            self.parse_expr()?
        } else {
            juxc_ast::Expr::Literal(juxc_ast::Literal::Null)
        };
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
        // Optional `extends Parent, Parent2, …` — interface
        // inheritance per Java. Each entry is a TypeRef so generic
        // parents (`extends Collection<T>`) carry their args.
        let mut extends: Vec<juxc_ast::TypeRef> = Vec::new();
        if self.eat_kw(Keyword::Extends) {
            loop {
                if let Some(t) = self.parse_type_ref() {
                    extends.push(t);
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::LBrace, "'{' to start interface body");

        let mut methods = Vec::new();
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            // Interface members carry annotations like class members
            // (grammar §A.2.4) — bindgen stubs also emit machine
            // markers here (`@MutSelf` on trait methods with a
            // `&mut self` receiver).
            let member_annotations = self.parse_annotations();
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
                        let mut depth: u32 = 1;
                        i += 1;
                        while depth > 0 {
                            match self.tokens.get(i).map(|t| &t.kind) {
                                Some(TokenKind::Lt) => depth += 1,
                                Some(TokenKind::Gt) => depth -= 1,
                                // A glued `>>` closes two nested generic lists.
                                Some(TokenKind::GtGt) => depth = depth.saturating_sub(2),
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
            extends,
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
        let mut is_weak = false;
        loop {
            if self.eat_kw(Keyword::Static) {
                is_static = true;
            } else if self.eat_kw(Keyword::Final) || self.eat_kw(Keyword::Const) {
                is_final = true;
            } else if self.eat_kw(Keyword::Weak) {
                // `weak` field (§6.5): does not contribute to refcount, read via
                // `.get()` → `T?`. Validity (class-typed, non-generic, no
                // initializer) is enforced in tycheck (`E0455`).
                is_weak = true;
            } else {
                break;
            }
        }
        // Type may be omitted for inference: `const I = 2;` / `x = 5;`. We
        // detect it by peeking for `IDENT =` or `IDENT ;` (an identifier that
        // is immediately the field name, with no type token before it). A real
        // typed field is `Type Name …`, where the token after the first
        // identifier is another identifier or a type continuation (`<`, `[`,
        // `.`, `?`), never `=`/`;`.
        let no_type = matches!(self.peek(), TokenKind::Ident(_))
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Eq) | Some(TokenKind::Semicolon)
            );
        let ty = if no_type { None } else { Some(self.parse_type_ref()?) };
        let name = self.parse_ident()?;
        // C#-style property bodies (`{ get; set; }` / `=> expr`) are
        // routed to `parse_property_decl` by the class-member
        // dispatcher *before* reaching here, so a plain field never
        // sees a `{` / `=>` suffix — it's always `type name [= expr] ;`.
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
            is_weak,
            ty,
            name,
            default,
            span: start.join(end),
        })
    }

    /// Parse a C#-style property declaration per JUX-MISSING-DEFS
    /// §M.7. Handles every accessor form losslessly into a
    /// [`juxc_ast::PropertyDecl`]:
    ///
    /// - `T Name { get; set; } [= init] ;?` — accessor block (auto /
    ///   expression-bodied / full-block accessors, per-accessor
    ///   visibility, `init`-only setters),
    /// - `T Name => expr ;?` — expression-bodied read-only property.
    ///
    /// The caller's dispatcher has already confirmed (via lookahead)
    /// that `[modifiers] type name` is followed by `{` or `=>`.
    /// Desugaring into backing field + getter / setter methods runs
    /// later in [`juxc_ast::desugar_properties`].
    pub(crate) fn parse_property_decl(
        &mut self,
        annotations: Vec<juxc_ast::Annotation>,
        visibility: Visibility,
    ) -> Option<juxc_ast::PropertyDecl> {
        use juxc_ast::{AccessorBody, PropertyAccessor, PropertyDecl, PropertySetter};
        let start = self.peek_span();
        // Modifiers — `static` is meaningful (§M.7.9); `final` /
        // `const` are accepted but don't change the property shape
        // (a read-only property is already non-reassignable).
        let mut is_static = false;
        loop {
            if self.eat_kw(Keyword::Static) {
                is_static = true;
            } else if self.eat_kw(Keyword::Final) || self.eat_kw(Keyword::Const) {
                // accepted, no-op for properties
            } else {
                break;
            }
        }
        let ty = self.parse_type_ref()?;
        let name = self.parse_ident()?;

        let mut getter: Option<PropertyAccessor> = None;
        let mut setter: Option<PropertySetter> = None;
        let mut has_backing_field = false;

        if self.eat(&TokenKind::Arrow) {
            // Expression-bodied read-only property: `T Name -> expr;`.
            // Equivalent to `{ get -> expr; }` — no setter, no backing
            // field (the expression is a computed value). Jux uses `->`,
            // not C#'s `=>`, because `=>` is the instanceof operator.
            let expr_start = self.peek_span();
            let expr = self.parse_expr()?;
            let expr_span = expr_start.join(self.last_consumed_span());
            let _ = self.eat(&TokenKind::Semicolon);
            getter = Some(PropertyAccessor {
                visibility: None,
                body: AccessorBody::Expr(expr),
                span: expr_span,
            });
        } else {
            // Accessor block: `{ accessor+ }`.
            self.expect(&TokenKind::LBrace, "'{' to start property body");
            while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                let acc_start = self.peek_span();
                // Optional per-accessor visibility (`private set;`).
                let acc_vis = self.parse_accessor_visibility();
                // Accessor kind: `get` / `set` (contextual idents) or
                // the `init` keyword.
                enum Kind {
                    Get,
                    Set,
                    Init,
                }
                let kind = match self.peek() {
                    TokenKind::Ident(s) if s == "get" => Some(Kind::Get),
                    TokenKind::Ident(s) if s == "set" => Some(Kind::Set),
                    TokenKind::Kw(Keyword::Init) => Some(Kind::Init),
                    _ => None,
                };
                let Some(kind) = kind else {
                    let here = self.peek_span();
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "expected `get` or `set` inside property body",
                        )
                        .with_span(here),
                    );
                    break;
                };
                // §P (observable-properties addendum): the `init`
                // accessor was REMOVED — accessor kinds are `get` and
                // `set` only. A construction-time read-only property is
                // `{ get; }` (settable in the constructor, §M.7.2).
                // Parse it as a plain `set` for recovery so downstream
                // checks still see a coherent property.
                if matches!(kind, Kind::Init) {
                    let here = self.peek_span();
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "the `init` accessor was removed (§P) — use `{ get; }` for a \
                             read-only property settable in the constructor, or `{ get; set; }`",
                        )
                        .with_span(here),
                    );
                }
                self.advance(); // consume the accessor keyword/ident
                let is_setter = matches!(kind, Kind::Set | Kind::Init);
                // Accessor body: `;` (auto) | `=> expr ;` | block.
                let (body, is_auto) = if self.eat(&TokenKind::Semicolon) {
                    (AccessorBody::Auto, true)
                } else if self.eat(&TokenKind::Arrow) {
                    // Accessor arrow body uses `->` (Jux), e.g. `get -> e;`
                    // / `set -> _x = value;`. Setter `->` bodies may be an
                    // assignment — assignment is a statement in Jux, so the
                    // setter arrow body is parsed as a statement, not a bare
                    // expression.
                    let b = self.parse_accessor_arrow_body(is_setter)?;
                    (b, false)
                } else if self.at(&TokenKind::LBrace) {
                    let b = self.parse_block();
                    (AccessorBody::Block(b), false)
                } else {
                    let here = self.peek_span();
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "expected `;`, `=> expr;`, or `{ … }` for accessor body",
                        )
                        .with_span(here),
                    );
                    (AccessorBody::Auto, true)
                };
                let acc_span = acc_start.join(self.last_consumed_span());
                match kind {
                    Kind::Get => {
                        // An auto getter implies a backing field.
                        if is_auto {
                            has_backing_field = true;
                        }
                        getter = Some(PropertyAccessor {
                            visibility: acc_vis,
                            body,
                            span: acc_span,
                        });
                    }
                    Kind::Set | Kind::Init => {
                        if is_auto {
                            has_backing_field = true;
                        }
                        setter = Some(PropertySetter {
                            visibility: acc_vis,
                            is_init: matches!(kind, Kind::Init),
                            body,
                            span: acc_span,
                        });
                    }
                }
            }
            self.expect(&TokenKind::RBrace, "'}' to close property body");
        }

        // Optional `= init` field-initializer (auto-properties).
        let initializer = if self.eat(&TokenKind::Eq) {
            self.parse_expr()
        } else {
            None
        };
        // Optional trailing `;` per §M.7.1's
        // `property-decl = ... property-body? property-init? ';'?`.
        let _ = self.eat(&TokenKind::Semicolon);
        let end = self.last_consumed_span();
        Some(PropertyDecl {
            annotations,
            visibility,
            is_static,
            ty,
            name,
            getter,
            setter,
            initializer,
            has_backing_field,
            span: start.join(end),
        })
    }

    /// Parse the body following a `=>` in a property accessor. The
    /// leading `=>` has already been consumed.
    ///
    /// - **Getter** (`is_setter == false`) → an [`AccessorBody::Expr`]
    ///   holding the value expression.
    /// - **Setter** (`is_setter == true`) → the body may be a side-
    ///   effecting assignment (`set => _x = value;`). Assignment is a
    ///   statement in Jux, so a setter arrow body is parsed as one
    ///   `Stmt` and wrapped in an [`AccessorBody::Block`]. A non-
    ///   assignment setter expression (`set => list.reserve(value);`)
    ///   is wrapped as an expression statement.
    fn parse_accessor_arrow_body(
        &mut self,
        is_setter: bool,
    ) -> Option<juxc_ast::AccessorBody> {
        use juxc_ast::AccessorBody;
        let body_start = self.peek_span();
        let expr = self.parse_expr()?;
        if is_setter {
            // Detect an assignment / compound-assignment tail.
            let assign_op = self.assignment_op_at_cursor();
            if assign_op.is_some() || self.at(&TokenKind::Eq) {
                let stmt = self.parse_assignment_tail(expr, assign_op)?;
                let span = body_start.join(self.last_consumed_span());
                return Some(AccessorBody::Block(juxc_ast::Block {
                    statements: vec![stmt],
                    span,
                }));
            }
            // Plain side-effecting expression — wrap as an Expr stmt
            // inside a block so the value is discarded.
            self.expect(&TokenKind::Semicolon, "';' after expression-bodied accessor");
            let span = body_start.join(self.last_consumed_span());
            return Some(AccessorBody::Block(juxc_ast::Block {
                statements: vec![juxc_ast::Stmt::Expr(expr)],
                span,
            }));
        }
        // Getter — value expression.
        self.expect(&TokenKind::Semicolon, "';' after expression-bodied accessor");
        Some(AccessorBody::Expr(expr))
    }

    /// If the cursor is at a compound-assignment operator
    /// (`+=`, `-=`, …), return the corresponding [`juxc_ast::BinaryOp`]
    /// WITHOUT consuming it; otherwise `None`. A plain `=` returns
    /// `None` too (the caller checks for it separately, since it
    /// carries no `BinaryOp`).
    fn assignment_op_at_cursor(&self) -> Option<juxc_ast::BinaryOp> {
        use juxc_ast::BinaryOp;
        match self.peek() {
            TokenKind::PlusEq => Some(BinaryOp::Add),
            TokenKind::MinusEq => Some(BinaryOp::Sub),
            TokenKind::StarEq => Some(BinaryOp::Mul),
            TokenKind::SlashEq => Some(BinaryOp::Div),
            TokenKind::PercentEq => Some(BinaryOp::Rem),
            TokenKind::AmpEq => Some(BinaryOp::BitAnd),
            TokenKind::PipeEq => Some(BinaryOp::BitOr),
            TokenKind::CaretEq => Some(BinaryOp::BitXor),
            TokenKind::LtLtEq => Some(BinaryOp::Shl),
            TokenKind::GtGtEq => Some(BinaryOp::Shr),
            _ => None,
        }
    }

    /// Parse an optional per-accessor visibility modifier
    /// (`public` / `private` / `protected` / `internal`) preceding a
    /// property accessor. Returns `None` when no modifier is present
    /// (the accessor inherits the property's outer visibility).
    fn parse_accessor_visibility(&mut self) -> Option<Visibility> {
        if self.eat_kw(Keyword::Public) {
            Some(Visibility::Public)
        } else if self.eat_kw(Keyword::Private) {
            Some(Visibility::Private)
        } else if self.eat_kw(Keyword::Protected) {
            Some(Visibility::Protected)
        } else if self.eat_kw(Keyword::Internal) {
            Some(Visibility::Internal)
        } else {
            None
        }
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
        // Constructor parameters reject the `final` binding mode (§A.2.4).
        let params = self.parse_param_list_with(/*allow_final=*/ false);
        self.expect(&TokenKind::RParen, "')' to close constructor parameter list");
        // A `;` body marks an **elided** constructor — the signature-only
        // form `.jux.d` declaration stubs use (JUX-BINDGEN-ADDENDUM.md §G.2).
        // It parses to an empty block; the backend never lowers it because
        // the whole unit is flagged `external`. A normal `.jux` source that
        // writes `Foo();` gets the same empty block, which is harmless (the
        // constructor just does nothing).
        let body = if self.eat(&TokenKind::Semicolon) {
            let sp = self.last_consumed_span();
            juxc_ast::Block { statements: Vec::new(), span: sp }
        } else {
            self.parse_block()
        };
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
                                // A glued `>>` closes two nested generic lists.
                                Some(TokenKind::GtGt) => depth = depth.saturating_sub(2),
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
        // Optional generic parameters per §A.2.4 — `enum Cow<B>`,
        // `enum Entry<K, V, A>`. Variant payloads may reference them.
        let generic_params = self.parse_generic_params();
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
        let mut methods: Vec<FnDecl> = Vec::new();
        let mut constants: Vec<juxc_ast::FieldDecl> = Vec::new();
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
                } else if self.at_kw(Keyword::Const) || self.at_kw(Keyword::Final) {
                    // Enum CONSTANT (§A.2.5) — `const int MAX = 9;`,
                    // implicitly static (interface-constant rules).
                    // `parse_field_decl` consumes the modifier itself
                    // and records is_final; we force is_static below.
                    if let Some(mut field) = self.parse_field_decl(Vec::new(), member_vis) {
                        field.is_static = true;
                        field.is_final = true;
                        constants.push(field);
                    }
                } else {
                    // Enum METHOD (§A.2.5) — same shape as a class
                    // method; `this` is the enum value (typically
                    // dispatched with `switch (this)`).
                    if let Some(m) = self.parse_fn_decl(Vec::new(), member_vis) {
                        methods.push(m);
                    } else {
                        // Recovery: skip to the closing brace so one
                        // malformed member can't loop forever.
                        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                            self.advance();
                        }
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
            generic_params,
            variants,
            operators,
            methods,
            constants,
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
        // Java-style LEADING generic parameters: `public <T> T identity(T v)`
        // (spec §A.2.4 / §7, e.g. line 1217). They sit before the return type;
        // we also still accept the trailing form `T identity<T>(...)` below.
        let leading_generics = self.parse_generic_params();
        let return_type = self.parse_return_type()?;
        let name = self.parse_ident()?;
        // Optional trailing generic parameters `<T>` between name and `(`.
        // Turn-1 limitation: no bounds, no defaults.
        let trailing_generics = self.parse_generic_params();
        let generic_params = if leading_generics.is_empty() {
            trailing_generics
        } else {
            leading_generics
        };

        self.expect(&TokenKind::LParen, "'(' to start parameter list");
        let params = self.parse_param_list();
        self.expect(&TokenKind::RParen, "')' to close parameter list");

        // throws-clause per §A.2.4 / §7.11: `throws Type (, Type)*`. The error
        // types are recorded as qualified names so the type checker and the
        // backend can map `throws E` ↔ `Result<T, E>` (§16.7). Stubs emitted by
        // `juxc bindgen` (§G.5.4) carry this clause for `Result`-returning
        // foreign functions, so it must parse here.
        let throws = if self.eat_kw(Keyword::Throws) {
            let mut tys = Vec::new();
            loop {
                let qn = self.parse_qualified_name();
                if qn.segments.is_empty() {
                    break;
                }
                tys.push(qn);
                // The throws-clause grammar is `'throws' type-list` and a `type`
                // admits `generic-args` (§A.2.4 / §A.2.7), e.g. a foreign
                // `throws OccupiedError<K, V, A>` from a `.jux.d` stub. The AST
                // records only the error type's qualified *name*, so we parse and
                // discard any `<…>` argument list rather than letting the leading
                // `<` derail the signature into a "expected '{'" block error.
                if self.at(&TokenKind::Lt) {
                    self.skip_balanced_angle_brackets();
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            tys
        } else {
            Vec::new()
        };

        // where-clause per §O.5.1: `where T has operator OP(types) -> R`
        // (comma-separated). `where` and `has` are contextual — they
        // lex as identifiers.
        let mut wheres = Vec::new();
        if matches!(self.peek(), TokenKind::Ident(s) if s == "where") {
            self.advance(); // 'where'
            loop {
                let c_start = self.peek_span();
                let Some(param) = self.parse_ident() else { break };
                if !matches!(self.peek(), TokenKind::Ident(s) if s == "has") {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "expected `has` in where-constraint — `where T has operator OP(..) -> R`",
                        )
                        .with_span(self.peek_span()),
                    );
                    break;
                }
                self.advance(); // 'has'
                self.expect_kw(Keyword::Operator, "`operator` in where-constraint");
                let Some(kind) = self.parse_operator_symbol() else { break };
                self.expect(&TokenKind::LParen, "'(' in where-constraint operator shape");
                let mut param_tys = Vec::new();
                if !self.at(&TokenKind::RParen) {
                    loop {
                        let Some(t) = self.parse_type_ref() else { break };
                        param_tys.push(t);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RParen, "')' in where-constraint operator shape");
                let ret = if self.eat(&TokenKind::Arrow) {
                    self.parse_type_ref()
                } else {
                    None
                };
                let end = self.last_consumed_span();
                wheres.push(juxc_ast::WhereConstraint {
                    param,
                    kind,
                    param_tys,
                    ret,
                    span: c_start.join(end),
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }

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
            wheres,
            body,
            is_property: false,
            span: start.join(end),
        })
    }

    /// Consume a balanced `< … >` token run starting at the current `<`,
    /// tolerating nesting (`Map<K, Vec<V>>`). The cursor must be on the opening
    /// `Lt`; on return it sits just past the matching `Gt`. Used where a
    /// generic-argument list appears in a position whose AST keeps only the bare
    /// name (e.g. a `throws` error type), so the arguments are parsed-and-dropped
    /// rather than left to derail the surrounding declaration.
    /// Non-consuming lookahead: if a **function type** (`(A, …) -> R`, grammar
    /// §A.2.7) begins at token index `i`, return the index just past it;
    /// otherwise `None`. Used by the class-member discriminator so a member that
    /// *returns* a function type (`(int) -> void onClick();`) is classified as a
    /// method rather than mis-read as a field. Handles the balanced parameter
    /// parens, an optional `async` / `throws` prefix on the result, and a
    /// nominal-or-nested-function result type.
    fn scan_fn_type_at(&self, mut i: usize) -> Option<usize> {
        if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen)) {
            return None;
        }
        // Balanced parameter parens.
        i += 1;
        let mut depth: u32 = 1;
        while depth > 0 {
            match self.tokens.get(i).map(|t| &t.kind) {
                Some(TokenKind::LParen) => depth += 1,
                Some(TokenKind::RParen) => depth -= 1,
                Some(TokenKind::Eof) | None => return None,
                _ => {}
            }
            i += 1;
        }
        // Optional `async`, then optional `throws Name (, Name)*`.
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Kw(Keyword::Async))) {
            i += 1;
        }
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Kw(Keyword::Throws))) {
            i += 1;
            loop {
                if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Ident(_))) {
                    return None;
                }
                i += 1;
                if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Comma)) {
                    break;
                }
                i += 1;
            }
        }
        // The `->` is what distinguishes a function type from a parenthesised /
        // tuple type. Without it, this isn't a function type.
        if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Arrow)) {
            return None;
        }
        i += 1;
        // Result type: `void`, a nominal type (with generics / array / nullable
        // suffixes), or a nested function type.
        Some(self.scan_type_at(i)?)
    }

    /// Non-consuming lookahead: return the index just past a **type** starting at
    /// `i` — a nominal type (`Map<K, V>`, `int[]`, `T?`, with glued-`>>`
    /// handling) or a function type (`(A) -> R`). `None` if no type is there.
    /// `void` is accepted as a (result-position) type. Shared by the member
    /// discriminator and `scan_fn_type_at`.
    fn scan_type_at(&self, mut i: usize) -> Option<usize> {
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::LParen)) {
            return self.scan_fn_type_at(i);
        }
        match self.tokens.get(i).map(|t| &t.kind) {
            Some(TokenKind::Kw(Keyword::Void)) => {
                return Some(i + 1);
            }
            Some(TokenKind::Ident(_)) => {
                i += 1;
            }
            _ => return None,
        }
        // Generic args `<…>` (glued `>>` closes two levels).
        if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Lt)) {
            i += 1;
            let mut depth: u32 = 1;
            while depth > 0 {
                match self.tokens.get(i).map(|t| &t.kind) {
                    Some(TokenKind::Lt) => depth += 1,
                    Some(TokenKind::Gt) => depth -= 1,
                    Some(TokenKind::GtGt) => depth = depth.saturating_sub(2),
                    Some(TokenKind::Eof) | None => break,
                    _ => {}
                }
                i += 1;
            }
        }
        // Dotted name segments (`a.b.C`) — a fully-qualified return
        // type is one nominal type.
        while matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Dot))
            && matches!(
                self.tokens.get(i + 1).map(|t| &t.kind),
                Some(TokenKind::Ident(_))
            )
        {
            i += 2;
        }
        // Interleaved `?` / `[…]` / `*` suffixes (`*` is the FFI raw
        // pointer marker, §G — bindgen stubs surface `T*` returns).
        loop {
            match self.tokens.get(i).map(|t| &t.kind) {
                Some(TokenKind::Question) | Some(TokenKind::Star) => i += 1,
                Some(TokenKind::LBracket) => {
                    i += 1;
                    let mut depth: u32 = 1;
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
                _ => break,
            }
        }
        Some(i)
    }

    fn skip_balanced_angle_brackets(&mut self) {
        if !self.at(&TokenKind::Lt) {
            return;
        }
        self.advance(); // opening `<`
        let mut depth: u32 = 1;
        while depth > 0 && !self.at_eof() {
            match self.peek() {
                TokenKind::Lt => depth += 1,
                TokenKind::Gt => depth -= 1,
                // A glued `>>` closes two nested generic lists at once.
                TokenKind::GtGt => depth = depth.saturating_sub(2),
                _ => {}
            }
            self.advance();
        }
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
        // Unary fix-up (§O.2.4): a zero-parameter `operator-` is the
        // unary negation overload, distinct from binary subtraction.
        let kind = if kind == OperatorKind::Minus && params.is_empty() {
            OperatorKind::Neg
        } else {
            kind
        };
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
            // `operator in` — containment, declared on the CONTAINER
            // (§O.2.4): `bool operator in(T element)`.
            TokenKind::Ident(text) if text == "in" => {
                self.advance();
                OperatorKind::In
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
                    ptr_depth: 0,
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
        self.parse_param_list_with(/*allow_final=*/ true)
    }

    /// `parse_param_list` variant that controls whether a `final`/`const`
    /// parameter binding mode is accepted. Constructors pass `false` (a `final`
    /// constructor parameter is rejected); methods / functions / operators pass
    /// `true`.
    pub(crate) fn parse_param_list_with(&mut self, allow_final: bool) -> Vec<Param> {
        let mut params = Vec::new();
        if self.at(&TokenKind::RParen) {
            return params;
        }
        loop {
            let Some(param) = self.parse_param(allow_final) else { break };
            params.push(param);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        // E0212 — a variadic parameter must be the LAST parameter
        // (§7.2 / Entry Points §E): the packer maps every trailing
        // call-site argument into it, so nothing can follow.
        for (i, p) in params.iter().enumerate() {
            if p.is_varargs && i + 1 != params.len() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0212_VarargsNotLast,
                        format!(
                            "variadic parameter `{}` must be the last parameter",
                            p.name.text,
                        ),
                    )
                    .with_span(p.span),
                );
            }
        }
        params
    }

    /// Per §A.2.4 `param = annotation* param-mode? type identifier ('=' expression)?`.
    /// Supports the `final` / `const` binding mode (`param-mode`); annotations and
    /// `out` / defaults are still future work. `allow_final` is `false` for
    /// constructor parameters, where a `final` mode is a diagnostic.
    pub(crate) fn parse_param(&mut self, allow_final: bool) -> Option<Param> {
        let start = self.peek_span();
        // Optional `final` (or its synonym `const`) binding mode.
        let final_span = self.peek_span();
        let is_final = self.eat_kw(Keyword::Final) || self.eat_kw(Keyword::Const);
        if is_final && !allow_final {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "`final` is not allowed on a constructor parameter — a constructor \
                     parameter is forwarded into a field, whose binding mode applies instead",
                )
                .with_span(final_span),
            );
        }
        // Optional `out` mode (§M.4) — the contextual keyword `out` immediately
        // before a type. A parameter literally NAMED `out` (`int out`) keeps
        // working because there `out` isn't the leading token. `out` is the mode
        // only when it's followed by something that begins a type (an
        // identifier or `(` for a function type).
        let out_span = self.peek_span();
        let is_out = matches!(self.peek(), TokenKind::Ident(s) if s == "out")
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                // A type-start (identifier / `(` function type) — or a `final`
                // / `const` binding mode, so `out final T x` is recognized as
                // the `out` mode and rejected by E0944 (not mis-parsed as a
                // parameter typed `out`). `final out T x` is handled by the
                // `final`-first eat above.
                Some(TokenKind::Ident(_))
                    | Some(TokenKind::LParen)
                    | Some(TokenKind::Kw(Keyword::Final))
                    | Some(TokenKind::Kw(Keyword::Const))
            );
        // `out final` / `out const` — `out` then a trailing binding mode. Fold
        // it into `is_final` so the E0944 check below fires in either order.
        let is_final = if is_out {
            self.advance(); // `out`
            is_final || self.eat_kw(Keyword::Final) || self.eat_kw(Keyword::Const)
        } else {
            is_final
        };
        // A leading `&` marks a borrowed parameter in a bindgen-generated stub
        // (§G.9.2). It carries no Jux type meaning (borrows vanish, §G.3.4) — we
        // record it as a flag so codegen re-adds the call-site borrow.
        let is_ref = self.eat(&TokenKind::Amp);
        let mut ty = self.parse_type_ref()?;
        // Variadic marker — `T... name` (§7.2). Desugars the declared
        // type to the dynamic-array form so the body sees `T[]`; the
        // flag drives call-site packing and the E0212 position check.
        let is_varargs = self.eat(&TokenKind::Ellipsis);
        if is_varargs {
            if ty.array_shape.is_some() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "`...` can't follow an array type — a variadic parameter is already an array of its element type",
                    )
                    .with_span(self.last_consumed_span()),
                );
            } else {
                ty.array_shape = Some(juxc_ast::ArrayShape::Dynamic);
            }
        }
        let name = self.parse_ident()?;
        // Optional default value — `int port = 80` (spec 7.2). Evaluated
        // at the call site when the argument is omitted (S.1.3); the
        // expansion pass clones it into each such call.
        let default = if self.eat(&TokenKind::Eq) { self.parse_expr() } else { None };
        let end = self.last_consumed_span();
        // `out` misuse (§M.4.1, E0944): not combinable with `final`, not on a
        // varargs / defaulted parameter, and not on a constructor parameter
        // (those forward into a field — there's nothing to write back through).
        if is_out {
            let bad = if is_final {
                Some("an `out` parameter can't also be `final`")
            } else if is_varargs {
                Some("a varargs parameter can't be `out`")
            } else if default.is_some() {
                Some("an `out` parameter can't have a default value")
            } else if !allow_final {
                Some("a constructor parameter can't be `out`")
            } else {
                None
            };
            if let Some(msg) = bad {
                self.diagnostics.push(
                    Diagnostic::error(code::Code::E0944_OutParamModifierMisuse, msg)
                        .with_span(out_span),
                );
            }
        }
        Some(Param {
            name,
            ty,
            is_final,
            is_ref,
            default,
            is_varargs,
            is_out,
            span: start.join(end),
        })
    }
}
