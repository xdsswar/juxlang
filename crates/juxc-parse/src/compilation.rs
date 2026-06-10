//! Top-level entry walk — compilation-unit, package, imports, top-level
//! declaration dispatch, visibility, and top-level error recovery.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{
    CompilationUnit, ImportDecl, ImportItem, ImportSpec, PackageDecl, QualifiedName, TopLevelDecl,
    Visibility,
};
use juxc_diagnostics::{code, Diagnostic};
use juxc_lex::{Keyword, TokenKind};
use juxc_source::Span;

use crate::Parser;

impl<'a> Parser<'a> {
    // ------------------------------------------------------------------
    // Compilation unit (§A.2.1)
    //
    //   compilation-unit = package-decl? import-decl* top-level-decl*
    // ------------------------------------------------------------------

    /// Top-level entry point. Always returns a `CompilationUnit`, even if
    /// fatally malformed (with diagnostics explaining the damage).
    pub(crate) fn parse_compilation_unit(&mut self) -> CompilationUnit {
        let start = self.peek_span();

        let package = self.try_parse_package_decl();
        let imports = self.parse_imports();

        let mut items = Vec::new();
        // Script mode (§E.1.1): bare statements at the top level are
        // collected here and wrapped in a synthetic `void main()` at
        // the end of the unit. Declarations and statements may mix
        // freely; the statements run in source order.
        let mut script_stmts: Vec<juxc_ast::Stmt> = Vec::new();
        let mut script_start: Option<Span> = None;
        while !self.at_eof() {
            // Remember the cursor so we can guarantee forward progress: a
            // `parse_top_level_decl` / `recover_to_top_level` pair that
            // consumes *nothing* (e.g. recovery anchored on a keyword the
            // dispatch doesn't actually handle) would otherwise spin forever
            // and blow the heap. The guard below force-advances in that case.
            let before = self.pos;
            // Unambiguous statement-leading keywords never start a
            // declaration — route them straight to the statement
            // parser. Everything else tries the declaration grammar
            // first, and on failure REWINDS and retries as a
            // statement (covers `print("hi");`, `x = 1;`, etc.).
            if self.at_script_stmt_keyword() {
                if script_start.is_none() {
                    script_start = Some(self.peek_span());
                }
                if let Some(stmt) = self.parse_stmt() {
                    script_stmts.push(stmt);
                    script_stmts.append(&mut self.pending_stmts);
                } else {
                    self.recover_to_stmt_boundary();
                }
            } else {
                let diags_before = self.diagnostics.len();
                if let Some(item) = self.parse_top_level_decl() {
                    items.push(item);
                } else {
                    // Speculative-rewind: stash the decl-attempt
                    // diagnostics, retry from the same spot as a
                    // statement. If the statement parse succeeds the
                    // decl errors were misfires and stay dropped; if
                    // it fails too, the decl diagnostics were the
                    // better story — restore them and recover.
                    let stashed: Vec<_> = self.diagnostics.drain(diags_before..).collect();
                    self.pos = before;
                    let stmt_diags_before = self.diagnostics.len();
                    match self.parse_stmt() {
                        Some(stmt) => {
                            if script_start.is_none() {
                                script_start = Some(self.peek_span());
                            }
                            script_stmts.push(stmt);
                            script_stmts.append(&mut self.pending_stmts);
                        }
                        None => {
                            self.diagnostics.truncate(stmt_diags_before);
                            self.diagnostics.extend(stashed);
                            self.pos = before;
                            self.advance_past_decl_failure();
                            // Recovery: jump to the next plausible top-level keyword.
                            self.recover_to_top_level();
                        }
                    }
                }
            }
            if self.pos == before {
                // No token was consumed this iteration — the current token is
                // a recovery anchor with no matching production. Step over it
                // so the loop always terminates; the failed parse attempts
                // already emitted a diagnostic.
                self.advance();
            }
        }
        // Wrap collected script statements in the synthetic entry
        // (§E.1.1). Span: first statement through last. An explicit
        // `main` in the same file collides in the symbol table and
        // surfaces as the usual duplicate-declaration diagnostic.
        if !script_stmts.is_empty() {
            let start_span = script_start.unwrap_or(start);
            let end_span = self.last_consumed_span();
            let body_span = start_span.join(end_span);
            items.push(TopLevelDecl::Function(juxc_ast::FnDecl {
                annotations: Vec::new(),
                visibility: Visibility::Public,
                modifiers: Vec::new(),
                return_type: juxc_ast::ReturnType::Void,
                name: juxc_ast::Ident { text: "main".to_string(), span: start_span },
                generic_params: Vec::new(),
                params: Vec::new(),
                throws: Vec::new(),
                body: Some(juxc_ast::Block { statements: script_stmts, span: body_span }),
                is_property: false,
                span: body_span,
            }));
        }
        // Flatten nested-type declarations out to the top level so
        // the symbol table, resolver, and backend treat them like
        // any other top-level type. Their names stay unchanged
        // (`new Inner()` resolves via the FQN-suffix scan); the
        // Java `Outer.Inner` qualified-access path is a follow-up
        // when nested types need real namespacing. Per spec §1379
        // nested types are *all* implicitly `static` so this lift
        // doesn't lose semantics.
        let mut flattened: Vec<TopLevelDecl> = Vec::new();
        for item in items.drain(..) {
            collect_nested_flat(item, &mut flattened);
        }
        let items = flattened;

        // Span the whole file from the first token to the EOF marker.
        let end = self.peek_span();
        // The parser is source-origin-agnostic: every freshly-parsed unit
        // starts non-external. The driver flips `is_external` on after the
        // fact for `.jux.d` declaration stubs (§G.9.1).
        CompilationUnit { package, imports, items, is_external: false, span: start.join(end) }
    }

    /// `package qualified-name ';'` — optional, only valid at the very
    /// top of the file. Per §A.2.1.
    fn try_parse_package_decl(&mut self) -> Option<PackageDecl> {
        if !self.at_kw(Keyword::Package) {
            return None;
        }
        let start = self.peek_span();
        self.advance(); // 'package'
        let name = self.parse_qualified_name();
        self.expect(&TokenKind::Semicolon, "';' after package declaration");
        let end = self.last_consumed_span();
        Some(PackageDecl { name, span: start.join(end) })
    }

    /// Zero or more `import-decl`s per §A.2.1:
    /// ```text
    /// import-decl  = 'import' import-spec ';'
    /// ```
    ///
    /// The `@cfg(...)` prefix form is parsed by the annotation pass once
    /// it lands; we only handle the bare `import` form here.
    fn parse_imports(&mut self) -> Vec<ImportDecl> {
        let mut imports = Vec::new();
        while self.at_kw(Keyword::Import) {
            imports.push(self.parse_import_decl());
        }
        imports
    }

    /// Parse one `import …;` declaration. Always advances past the `;`
    /// (or to EOF) so the caller can keep going regardless of shape
    /// errors inside the spec.
    ///
    /// Grammar (§A.2.1):
    /// ```text
    /// import-spec  = qualified-name ( '.' '*' )? ( 'as' identifier )?
    ///              | qualified-name '.' '{' import-item ( ',' import-item )* '}'
    /// import-item  = identifier ( 'as' identifier )?
    /// ```
    fn parse_import_decl(&mut self) -> ImportDecl {
        let start = self.peek_span();
        self.advance(); // 'import'
        let spec = self.parse_import_spec();
        self.expect(&TokenKind::Semicolon, "';' after import declaration");
        let end = self.last_consumed_span();
        ImportDecl { spec, span: start.join(end) }
    }

    /// Parse one `import-spec`. Returns an [`ImportSpec`] either way —
    /// on shape errors we synthesize an empty path so the caller can
    /// keep walking and the diagnostics already explain the problem.
    ///
    /// Walks the dotted path one segment at a time so we can branch on
    /// `.*` (wildcard), `.{...}` (grouped), or `.ident` (continue) at
    /// each step without needing arbitrary lookahead.
    fn parse_import_spec(&mut self) -> ImportSpec {
        let start = self.peek_span();
        let Some(first) = self.parse_ident() else {
            // No first identifier — recover by skipping to `;`. The
            // parse_ident call already emitted E0200.
            self.recover_to_import_terminator();
            return ImportSpec::Path {
                name: QualifiedName { segments: Vec::new(), span: Span::DUMMY },
                wildcard: false,
                alias: None,
            };
        };
        let mut segments = vec![first];

        // Walk dotted continuations. At each `.` decide whether we hit
        // `*` (wildcard, end of path), `{` (group, switch to Items
        // mode), or another identifier (extend the path).
        let mut wildcard = false;
        loop {
            if !self.at(&TokenKind::Dot) {
                break;
            }
            self.advance(); // '.'
            if self.at(&TokenKind::Star) {
                self.advance();
                wildcard = true;
                break;
            }
            if self.at(&TokenKind::LBrace) {
                // Grouped form. The prefix is everything we've gathered
                // so far; hand off to the items parser.
                let prefix = QualifiedName {
                    segments,
                    span: start.join(self.last_consumed_span()),
                };
                return self.parse_import_items(prefix);
            }
            match self.parse_ident() {
                Some(ident) => segments.push(ident),
                None => {
                    // `import foo.;` or `import foo.123;` — parse_ident
                    // emitted E0200. Recover and return what we have.
                    self.recover_to_import_terminator();
                    let path = QualifiedName {
                        segments,
                        span: start.join(self.last_consumed_span()),
                    };
                    return ImportSpec::Path { name: path, wildcard: false, alias: None };
                }
            }
        }

        // Path complete. Optional `as Alias`. Per the AST contract, an
        // `as` clause on a wildcard import is a shape error — the
        // wildcard imports many names, no single name to rename.
        let alias = if self.eat_kw(Keyword::As) {
            let parsed = self.parse_ident();
            if wildcard {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        "`as` rename is not allowed on a wildcard import",
                    )
                    .with_span(self.last_consumed_span()),
                );
            }
            parsed
        } else {
            None
        };

        let path = QualifiedName {
            segments,
            span: start.join(self.last_consumed_span()),
        };
        ImportSpec::Path { name: path, wildcard, alias }
    }

    /// Parse a `{ item ( ',' item )* }` group. We're positioned on the
    /// `{`. Empty groups (`{}`) and trailing commas (`{Foo,}`) are
    /// rejected — both diverge from §A.2.1's `item ( ',' item )*` form.
    fn parse_import_items(&mut self, prefix: QualifiedName) -> ImportSpec {
        self.advance(); // '{'
        let mut items: Vec<ImportItem> = Vec::new();

        // Empty group is a shape error.
        if self.at(&TokenKind::RBrace) {
            self.advance();
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "grouped import must list at least one item",
                )
                .with_span(self.last_consumed_span()),
            );
            return ImportSpec::Items { prefix, items };
        }

        loop {
            // Each item is `ident ( 'as' ident )?`.
            let Some(name) = self.parse_ident() else {
                // Recovery: consume up to `}` or `;`.
                self.recover_to_group_terminator();
                break;
            };
            let alias = if self.eat_kw(Keyword::As) {
                self.parse_ident()
            } else {
                None
            };
            items.push(ImportItem { name, alias });

            // Comma → expect another item. Closing brace → done.
            if self.eat(&TokenKind::Comma) {
                if self.at(&TokenKind::RBrace) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "trailing comma in grouped import",
                        )
                        .with_span(self.last_consumed_span()),
                    );
                    self.advance(); // consume '}'
                    break;
                }
                continue;
            }
            self.expect(&TokenKind::RBrace, "',' or '}' in grouped import");
            break;
        }

        ImportSpec::Items { prefix, items }
    }

    /// Skip ahead until we find `;` or EOF. Used when an import's spec
    /// blew up before we could parse the trailing `;`. Stops *before*
    /// the `;` so the caller's `expect(Semicolon, ...)` finishes the
    /// recovery.
    fn recover_to_import_terminator(&mut self) {
        while !matches!(self.peek(), TokenKind::Semicolon | TokenKind::Eof) {
            self.advance();
        }
    }

    /// Skip ahead until we find `}`, `;`, or EOF. Used when an item
    /// inside a grouped import is malformed.
    fn recover_to_group_terminator(&mut self) {
        loop {
            match self.peek() {
                TokenKind::RBrace => {
                    self.advance();
                    return;
                }
                TokenKind::Semicolon | TokenKind::Eof => return,
                _ => self.advance(),
            }
        }
    }

    // ------------------------------------------------------------------
    // Top-level declarations (§A.2.2)
    //
    //   top-level-decl       = annotation* visibility? top-level-decl-body
    //   top-level-decl-body  = type-decl | function-decl | const-decl | type-alias
    //
    // Milestone 1 supports only function-decl. Other body kinds emit a
    // diagnostic and let recovery skip them.
    // ------------------------------------------------------------------

    /// Parse one top-level declaration, returning `None` on unrecoverable
    /// parse failure (caller does the recovery).
    pub(crate) fn parse_top_level_decl(&mut self) -> Option<TopLevelDecl> {
        // Per grammar §A.2.2 every top-level decl is prefixed by an
        // (optional) annotation list, then a visibility modifier,
        // then the decl body. We consume both prefixes here and
        // thread the captured annotations into whichever decl
        // dispatch arm fires.
        let annotations = self.parse_annotations();
        let visibility = self.parse_visibility();

        // `const NAME …;` is unambiguously a top-level constant —
        // `const` is never a class modifier in Jux.
        if self.eat_kw(Keyword::Const) {
            let decl =
                self.parse_const_decl(annotations, visibility, /*used_final=*/ false)?;
            return Some(TopLevelDecl::Const(decl));
        }
        // `final` is overloaded — it modifies a class (`final class
        // X`) AND introduces a top-level constant (`final int X =
        // 5;`). Disambiguate by peeking past `final`: if `class`
        // appears (possibly after `abstract`/`sealed`) it's the
        // class modifier path; otherwise it's a const decl.
        if self.at_kw(Keyword::Final) && !looks_like_class_modifier_chain(self) {
            self.advance(); // eat `final`
            let decl =
                self.parse_const_decl(annotations, visibility, /*used_final=*/ true)?;
            return Some(TopLevelDecl::Const(decl));
        }
        // Top-level dispatch: `class` → class decl, `enum` → enum decl,
        // anything else routes to `parse_fn_decl`. Use `at_kw` (not
        // `at(&TokenKind::Kw(...))`) — the latter relies on
        // `mem::discriminant`, which doesn't distinguish keywords inside
        // the `Kw(_)` variant.
        //
        // `abstract` / `final` / `sealed` and `class` can appear in
        // any order at top-level. We eat any prefix combination and
        // propagate to `parse_class_decl`. `final` and `sealed` are
        // mutually exclusive (a class can't be both — final means
        // no subclasses, sealed means a restricted set); we report
        // that as E0200 below.
        let mut is_abstract_top = false;
        let mut is_final_top = false;
        let mut is_sealed_top = false;
        loop {
            if self.eat_kw(Keyword::Abstract) {
                is_abstract_top = true;
            } else if self.eat_kw(Keyword::Final) {
                is_final_top = true;
            } else if self.eat_kw(Keyword::Sealed) {
                is_sealed_top = true;
            } else {
                break;
            }
        }
        if is_final_top && is_sealed_top {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "`final` and `sealed` are mutually exclusive on the same class",
                )
                .with_span(self.peek_span()),
            );
        }
        if is_abstract_top && is_final_top {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "`abstract` and `final` cannot be combined — abstract classes need subclasses",
                )
                .with_span(self.peek_span()),
            );
        }
        if self.at_kw(Keyword::Class) {
            let class_decl = self.parse_class_decl(
                annotations,
                visibility,
                is_abstract_top,
                is_final_top,
                is_sealed_top,
            )?;
            return Some(TopLevelDecl::Class(class_decl));
        }
        if is_abstract_top || is_final_top || is_sealed_top {
            // A class modifier was consumed but no `class` followed —
            // surface the diagnostic now so the user gets a clear
            // error rather than a downstream "expected return type"
            // shape.
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "`abstract` / `final` / `sealed` modifiers are only valid on `class` declarations",
                )
                .with_span(self.peek_span()),
            );
            return None;
        }
        if self.at_kw(Keyword::Struct) {
            // Grammar §A.2.5 `struct-decl`. Phase 1 represents a struct as an
            // implicitly-`final` `ClassDecl` flagged `is_struct` (see
            // `parse_struct_decl`), so it shares the class machinery through
            // resolve / tycheck / the symbol table while keeping its origin.
            let struct_decl = self.parse_struct_decl(annotations, visibility)?;
            return Some(TopLevelDecl::Class(struct_decl));
        }
        if self.at_kw(Keyword::Enum) {
            let enum_decl = self.parse_enum_decl(annotations, visibility)?;
            return Some(TopLevelDecl::Enum(enum_decl));
        }
        if self.at_kw(Keyword::Record) {
            let record_decl = self.parse_record_decl(annotations, visibility)?;
            return Some(TopLevelDecl::Record(record_decl));
        }
        if self.at_kw(Keyword::Interface) {
            let interface_decl = self.parse_interface_decl(annotations, visibility)?;
            return Some(TopLevelDecl::Interface(interface_decl));
        }
        if self.at_kw(Keyword::Type) {
            let alias = self.parse_type_alias_decl(annotations, visibility)?;
            return Some(TopLevelDecl::TypeAlias(alias));
        }
        let fn_decl = self.parse_fn_decl(annotations, visibility)?;
        Some(TopLevelDecl::Function(fn_decl))
    }

    /// True when the cursor sits on a keyword that can ONLY start a
    /// statement (never a declaration) — the cheap script-mode
    /// dispatch (§E.1.1). `if`/`for`/`while`/… have no top-level-decl
    /// reading, so no speculation is needed for them.
    fn at_script_stmt_keyword(&self) -> bool {
        self.at_kw(Keyword::Var)
            || self.at_kw(Keyword::If)
            || self.at_kw(Keyword::While)
            || self.at_kw(Keyword::Do)
            || self.at_kw(Keyword::For)
            || self.at_kw(Keyword::Switch)
            || self.at_kw(Keyword::Try)
            || self.at_kw(Keyword::Throw)
            || self.at_kw(Keyword::Return)
            || self.at_kw(Keyword::Break)
            || self.at_kw(Keyword::Continue)
            || self.at_kw(Keyword::Unsafe)
    }

    /// After both the declaration AND statement parses failed at the
    /// same position, step past the offending token so the outer
    /// loop's forward-progress guard doesn't have to.
    fn advance_past_decl_failure(&mut self) {
        self.advance();
    }

    /// Per §A.2.2 `visibility = 'public' | 'internal' | 'protected' | 'private'`.
    /// Absence means package-private.
    pub(crate) fn parse_visibility(&mut self) -> Visibility {
        if self.eat_kw(Keyword::Public) {
            Visibility::Public
        } else if self.eat_kw(Keyword::Internal) {
            Visibility::Internal
        } else if self.eat_kw(Keyword::Protected) {
            Visibility::Protected
        } else if self.eat_kw(Keyword::Private) {
            Visibility::Private
        } else {
            Visibility::Package
        }
    }

    // ------------------------------------------------------------------
    // Error recovery
    // ------------------------------------------------------------------

    /// Skip tokens until we hit something that plausibly starts a new
    /// top-level declaration (a visibility modifier, a type-decl keyword,
    /// `import`, `package`, or EOF). This is the recovery anchor for a
    /// failed `parse_top_level_decl`.
    pub(crate) fn recover_to_top_level(&mut self) {
        while !self.at_eof() {
            match self.peek() {
                TokenKind::Kw(
                    Keyword::Public
                    | Keyword::Internal
                    | Keyword::Protected
                    | Keyword::Private
                    | Keyword::Class
                    | Keyword::Interface
                    | Keyword::Struct
                    | Keyword::Record
                    | Keyword::Enum
                    | Keyword::Annotation
                    | Keyword::Import
                    | Keyword::Package,
                ) => return,
                _ => self.advance(),
            }
        }
    }
}

/// True iff the current token is `final` AND that `final` is acting
/// as a class modifier rather than a constant introducer. We look
/// past any combination of `final`/`abstract`/`sealed` and check
/// whether `class` eventually follows.
///
/// Examples (cursor on the leading `final`):
/// - `final class Foo {}`                  → true
/// - `final abstract class Foo {}` (illegal but lookahead-true) → true
/// - `final int X = 5;`                    → false (const decl)
/// - `final Animal x = new Dog();`         → false (const decl)
/// Recursively flatten nested-type declarations into a top-level
/// list. Used by the compilation-unit post-pass so the resolver,
/// symbol table, and backend treat them like any other top-level
/// type. Nested types DECLARED on the class are also recursively
/// flattened (a nested class can itself contain nested classes).
/// The original `nested_types` list is drained out of the parent
/// ClassDecl so it ends up empty after this pass.
fn collect_nested_flat(item: TopLevelDecl, out: &mut Vec<TopLevelDecl>) {
    match item {
        TopLevelDecl::Class(mut class_decl) => {
            let nested = std::mem::take(&mut class_decl.nested_types);
            out.push(TopLevelDecl::Class(class_decl));
            for n in nested {
                collect_nested_flat(n, out);
            }
        }
        other => out.push(other),
    }
}

fn looks_like_class_modifier_chain<'a>(parser: &crate::Parser<'a>) -> bool {
    let mut i = parser.pos;
    loop {
        match parser.tokens.get(i).map(|t| &t.kind) {
            Some(TokenKind::Kw(Keyword::Final))
            | Some(TokenKind::Kw(Keyword::Abstract))
            | Some(TokenKind::Kw(Keyword::Sealed)) => i += 1,
            Some(TokenKind::Kw(Keyword::Class)) => return true,
            _ => return false,
        }
    }
}
