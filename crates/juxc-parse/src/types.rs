//! Type-position parsers — `TypeRef` and qualified-name parsing.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{ArrayDim, ArrayShape, FnTypeShape, QualifiedName, TypeRef};
use juxc_lex::{Keyword, TokenKind};
use juxc_source::Span;

use juxc_diagnostics::{code, Diagnostic};
use crate::Parser;

impl<'a> Parser<'a> {
    /// Per §A.2.7 `type` — a qualified-name optionally followed by `?`
    /// (nullable) and one or more array suffixes `[N]` (fixed-size) /
    /// `[]` (dynamic). Multiple suffixes form a multi-dimensional array
    /// type (`int[][]`, `int[3][4]`, `int[3][]`), stored outermost-first.
    ///
    /// Generics, pointers, function types, tuple types are still future
    /// extensions.
    pub(crate) fn parse_type_ref(&mut self) -> Option<TypeRef> {
        // Function-type shape `(A, B) async? throws? -> R` per
        // grammar §A.2.7. Detected by the `(` lead. We commit to
        // the function-type branch only after the closing `)` so
        // tuple-type misreads (eventually) stay open. The
        // disambiguation rule (§A.2.7 #4) — value-position
        // `(T) -> e` is always a lambda — is automatically
        // respected because `parse_type_ref` is only called from
        // type positions.
        if self.at(&TokenKind::LParen) {
            if let Some(fn_ty) = self.try_parse_function_type() {
                return Some(fn_ty);
            }
            // Tuple type — `(A, B, …)` (§5.3 / grammar §A.2.7
            // tuple-type). In type position a `(` not followed by a
            // function arrow can only be a tuple, so commit. The unit
            // form `()` is reserved with no v1 meaning (§A.2.7) and
            // rejected here.
            let start = self.peek_span();
            self.advance(); // '('
            let mut elems = Vec::new();
            if !self.at(&TokenKind::RParen) {
                loop {
                    let ty = self.parse_type_ref()?;
                    elems.push(ty);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                    // Tolerate a trailing comma before `)`.
                    if self.at(&TokenKind::RParen) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen, "')' to close tuple type");
            if elems.len() < 2 {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        if elems.is_empty() {
                            "the unit tuple type `()` is reserved and has no meaning yet — use `void` for no-value returns"
                        } else {
                            "a tuple type needs at least two elements — parenthesizing a single type has no effect"
                        },
                    )
                    .with_span(start.join(self.last_consumed_span())),
                );
                return elems.into_iter().next();
            }
            let end = self.last_consumed_span();
            let mut t = TypeRef::tuple(elems, start.join(end));
            // Optional `?` — a nullable tuple `(A, B)?` lowers to
            // `Option<(A, B)>`.
            if self.eat(&TokenKind::Question) {
                t.nullable = true;
                t.span = start.join(self.last_consumed_span());
            }
            return Some(t);
        }
        let qname = self.parse_qualified_name();
        if qname.segments.is_empty() {
            return None;
        }

        // Optional generic-args list per §A.2.7. Eagerly consumed so
        // `Box<int>`, `Map<String, int>`, etc. parse into TypeRef's
        // `generic_args`. Type position is unambiguous — `<` here
        // can only be generic args, never the less-than operator.
        let generic_args = self.parse_generic_args();

        // If this type's generic list closed by consuming a `>>` (so a split `>`
        // is parked for an ENCLOSING list), the cursor now sits past the `>>` on
        // whatever follows the *outer* type — any `?` / `[]` there belongs to the
        // outer type, not this one. Skip the suffix parse so it isn't stolen.
        if self.pending_gt > 0 {
            let end = self.last_consumed_span();
            return Some(TypeRef {
                name: qname.clone(),
                generic_args,
                nullable: false,
                array_shape: None,
                fn_shape: None,
                ptr_depth: 0,
                span: qname.span.join(end),
            });
        }

        // Optional `?` (nullable, §A.2.7) and array suffixes (`[]` / `[N]`).
        //
        // A type may carry MULTIPLE array dimensions (`int[][]`,
        // `int[3][4]`, `int[3][]`); each consecutive `[…]` is one
        // dimension, collected OUTERMOST-first in source order (the
        // leftmost `[…]` is the outermost dimension → `dims[0]`).
        //
        // The nullable `?` can sit on EITHER side of the brackets: `T?[]`
        // (nullable element, arrayed) and `T[]?` (nullable array) both
        // occur in generated stubs (from `&[Option<V>]` and
        // `Option<&[u8]>` respectively). `TypeRef` flattens nullability
        // into one flag, so we OR it in wherever it appears.
        let mut nullable = false;
        let mut dims: Vec<ArrayDim> = Vec::new();
        loop {
            if !nullable && self.eat(&TokenKind::Question) {
                nullable = true;
                continue;
            }
            if self.eat(&TokenKind::LBracket) {
                let dim = if self.eat(&TokenKind::RBracket) {
                    // `[]` — dynamic dimension (runtime-sized, lowers to Vec).
                    ArrayDim::Dynamic
                } else {
                    // `[N]` — fixed dimension (const-sized, lowers to [T; N]).
                    let size = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket, "']' to close array size");
                    ArrayDim::Fixed(Box::new(size))
                };
                dims.push(dim);
                continue;
            }
            break;
        }
        // `Some` only when at least one dimension was read — keeps the
        // scalar case as `None` exactly as before.
        let array_shape = if dims.is_empty() { None } else { Some(ArrayShape { dims }) };

        // Trailing raw-pointer markers `*` (§5.5 / §A.2.7), the OUTERMOST
        // modifier: `T*` → `*mut T`, `T**` → `*mut *mut T`. In type position a
        // `*` is unambiguous (it's never the multiply operator there). Pointers
        // are `unsafe`-only — the type checker enforces the `unsafe` context.
        let mut ptr_depth: u8 = 0;
        while self.eat(&TokenKind::Star) {
            ptr_depth = ptr_depth.saturating_add(1);
        }

        let end = self.last_consumed_span();
        Some(TypeRef {
            name: qname.clone(),
            generic_args,
            nullable,
            array_shape,
            fn_shape: None,
            ptr_depth,
            span: qname.span.join(end),
        })
    }

    /// Attempt to parse a `function-type` per grammar §A.2.7:
    /// ```text
    /// function-type = '(' type-list? ')' ( 'async' )? ( 'throws' type-list )? '->' type
    /// ```
    ///
    /// Returns `Some(TypeRef)` when the input starts with `(` AND
    /// the closing `)` is followed (modulo `async`/`throws`) by
    /// `->`. Returns `None` otherwise so the caller can fall
    /// through to the named-type path. The scan uses a parenthesis
    /// counter so nested types (`((int) -> int) -> bool`) work.
    fn try_parse_function_type(&mut self) -> Option<TypeRef> {
        // Lookahead: find the matched `)` and check the tail.
        let mut i = self.pos + 1; // past the opening `(`
        let mut depth = 1usize;
        while depth > 0 {
            match self.tokens.get(i).map(|t| &t.kind) {
                Some(TokenKind::LParen) => depth += 1,
                Some(TokenKind::RParen) => depth -= 1,
                Some(TokenKind::Eof) | None => return None,
                _ => {}
            }
            i += 1;
        }
        // `i` now points just past the matched `)`. Skip optional
        // `async` and `throws` prefix to find `->`.
        let mut j = i;
        if matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Kw(Keyword::Async))) {
            j += 1;
        }
        if matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Kw(Keyword::Throws))) {
            // Skip a comma-separated list of names — not authoritative,
            // just enough to peek past it.
            j += 1;
            loop {
                if !matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Ident(_))) {
                    return None;
                }
                j += 1;
                if !matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Comma)) {
                    break;
                }
                j += 1;
            }
        }
        if !matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Arrow)) {
            return None;
        }
        // Commit — consume the actual tokens.
        let start = self.peek_span();
        self.expect(&TokenKind::LParen, "'(' to start function-type params");
        let mut params = Vec::new();
        if !self.at(&TokenKind::RParen) {
            loop {
                let ty = self.parse_type_ref()?;
                params.push(ty);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "')' to close function-type params");
        let is_async = self.eat_kw(Keyword::Async);
        let throws = if self.eat_kw(Keyword::Throws) {
            let mut tys = Vec::new();
            loop {
                let Some(t) = self.parse_type_ref() else { break };
                tys.push(t);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            tys
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::Arrow, "'->' in function type");
        // The function-type result may be `void` (`(A) -> void`), which is a
        // keyword `parse_type_ref` won't accept — synthesize a `void` TypeRef
        // for that case (mirrors `parse_return_type`'s `async void` handling).
        let return_type = if self.eat_kw(Keyword::Void) {
            let span = self.last_consumed_span();
            TypeRef {
                name: QualifiedName { segments: vec![juxc_ast::Ident { text: "void".to_string(), span }], span },
                generic_args: Vec::new(),
                nullable: false,
                array_shape: None,
                fn_shape: None,
                ptr_depth: 0,
                span,
            }
        } else {
            self.parse_type_ref()?
        };
        let end = self.last_consumed_span();
        // The TypeRef shape carries the function-type info in
        // `fn_shape`; `name` is a synthetic `__fn` sentinel so
        // pre-fn_shape consumers that read `name` get a stable
        // (non-matching-anything) value rather than empty.
        let sentinel = QualifiedName {
            segments: Vec::new(),
            span: Span::DUMMY,
        };
        Some(TypeRef {
            name: sentinel,
            generic_args: Vec::new(),
            nullable: false,
            array_shape: None,
            ptr_depth: 0,
            fn_shape: Some(Box::new(FnTypeShape {
                params,
                return_type,
                is_async,
                throws,
            })),
            span: start.join(end),
        })
    }

    /// Per §A.2.1 `qualified-name = identifier ( '.' identifier )*`.
    ///
    /// Always returns a `QualifiedName`; if the first identifier was
    /// missing, the result has an empty `segments` vec and a `DUMMY` span.
    /// The caller can detect that case and respond.
    pub(crate) fn parse_qualified_name(&mut self) -> QualifiedName {
        let start = self.peek_span();
        let mut segments = Vec::new();
        let Some(first) = self.parse_ident() else {
            return QualifiedName { segments, span: Span::DUMMY };
        };
        segments.push(first);
        while self.eat(&TokenKind::Dot) {
            match self.parse_ident() {
                Some(next) => segments.push(next),
                None => break,
            }
        }
        let end = self.last_consumed_span();
        QualifiedName { segments, span: start.join(end) }
    }
}
