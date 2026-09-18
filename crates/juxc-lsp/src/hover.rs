//! `textDocument/hover`: the declaration signature of the name under the
//! cursor, else the inferred type of the expression there.

use juxc_source::Span;
use tower_lsp::lsp_types::*;

use crate::doc::Document;
use crate::intel;
use crate::text::{doc_comment_before, receiver_dot_before, word_at};

/// Hover for `pos` in `doc` (the document at `uri`).
pub(crate) fn hover(doc: &Document, uri: &Url, pos: Position) -> Option<Hover> {
    let offset = doc.offset_at(pos);
    let text = doc.rope.to_string();

    // Declaration-signature hover. If the cursor sits on an identifier that
    // resolves to a KNOWN symbol (a type name, a free function, or a member
    // reached via the receiver's inferred type), render that declaration's
    // signature in Jux syntax plus its first-line doc comment. Falls back to
    // the expression-type hover below otherwise.
    if let Some(word) = word_at(&text, offset) {
        let resolved = receiver_dot_before(&text, word.start).and_then(|recv_end| {
            // `recv.member`: resolve `recv`'s type, then look up the member.
            doc.type_ending_at(recv_end)
                .and_then(|ty| intel::resolve_member(&doc.symbols, ty, &word.text))
        });
        // Plain identifier: try a type name, then a free function.
        let resolved = resolved
            .or_else(|| intel::resolve_type(&doc.symbols, &word.text))
            .or_else(|| intel::resolve_function(&doc.symbols, &word.text));

        if let Some(resolved) = resolved {
            let mut value = format!("```jux\n{}\n```", resolved.signature());
            // Doc comment: prefer the one at the DECLARATION site. For a type or
            // function name we can locate the declaring unit (even a generated
            // `rust.std` stub) via `definition_of` and read its `/** … */`
            // there. Falls back to the usage-site scan (members, or when the
            // declaration can't be located).
            let decl_doc = doc.symbols.definition_of(&word.text).and_then(|(unit, span)| {
                let path = doc.source_paths.get(unit)?;
                let same_as_open = Url::from_file_path(path).ok().as_ref() == Some(uri);
                let decl_text = if same_as_open {
                    text.clone()
                } else {
                    std::fs::read_to_string(path).ok()?
                };
                doc_comment_before(&decl_text, span.start as usize)
            });
            if let Some(doc_line) = decl_doc.or_else(|| doc_comment_before(&text, word.start)) {
                value.push_str("\n\n");
                value.push_str(&doc_line);
            }
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value }),
                range: Some(doc.range_of(Span::new(word.start as u32, word.end as u32))),
            });
        }
    }

    // Fallback: the inferred type at the cursor, as a Jux code block.
    let (span, ty) = doc.type_at(offset)?;
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```jux\n{ty}\n```"),
        }),
        range: Some(doc.range_of(span)),
    })
}
