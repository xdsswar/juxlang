//! Type-position parsers — `TypeRef` and qualified-name parsing.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{ArrayShape, QualifiedName, TypeRef};
use juxc_lex::TokenKind;
use juxc_source::Span;

use crate::Parser;

impl<'a> Parser<'a> {
    /// Per §A.2.7 `type` — a qualified-name optionally followed by `?`
    /// (nullable) and an array suffix `[N]` (fixed-size) or `[]`
    /// (dynamic, not yet implemented).
    ///
    /// Generics, pointers, function types, tuple types are still future
    /// extensions.
    pub(crate) fn parse_type_ref(&mut self) -> Option<TypeRef> {
        let qname = self.parse_qualified_name();
        if qname.segments.is_empty() {
            return None;
        }

        // Optional generic-args list per §A.2.7. Eagerly consumed so
        // `Box<int>`, `Map<String, int>`, etc. parse into TypeRef's
        // `generic_args`. Type position is unambiguous — `<` here
        // can only be generic args, never the less-than operator.
        let generic_args = self.parse_generic_args();

        let nullable = self.eat(&TokenKind::Question);

        // Optional array suffix per §A.2.7: `type '[' const-expr ']'`
        // or `type '[' ']'`. Only one level (single-dimensional) is
        // accepted today; multi-dim is a future extension.
        let array_shape = if self.eat(&TokenKind::LBracket) {
            if self.eat(&TokenKind::RBracket) {
                // `T[]` — dynamic. We accept the syntax but the backend
                // doesn't yet lower it (Turn 2 work).
                Some(ArrayShape::Dynamic)
            } else {
                let size = self.parse_expr()?;
                self.expect(&TokenKind::RBracket, "']' to close array size");
                Some(ArrayShape::Fixed(Box::new(size)))
            }
        } else {
            None
        };

        let end = self.last_consumed_span();
        Some(TypeRef {
            name: qname.clone(),
            generic_args,
            nullable,
            array_shape,
            span: qname.span.join(end),
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
