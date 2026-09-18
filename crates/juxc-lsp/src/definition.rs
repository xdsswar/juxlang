//! `textDocument/definition`: jump to the declaration of the name under the
//! cursor, including members (`recv.member`) and declarations inside generated
//! `rust.std` / crate `.jux.d` stubs.

use ropey::Rope;
use tower_lsp::lsp_types::*;

use crate::doc::Document;
use crate::intel;
use crate::position::span_to_range;
use crate::text::{receiver_dot_before, word_at};

/// The text of analysed source `unit`: the live buffer when it is the open
/// document, else the text the analysis checked, else the file on disk (a
/// generated stub the analysis does not keep). `None` when none is readable.
pub(crate) fn unit_text(doc: &Document, uri: &Url, unit: usize) -> Option<(Url, Rope)> {
    let path = doc.source_paths.get(unit)?;
    let target = Url::from_file_path(path).ok()?;
    if &target == uri || doc.file == Some(unit as u32) {
        return Some((uri.clone(), doc.rope.clone()));
    }
    if let Some(Some(text)) = doc.source_texts.get(unit) {
        return Some((target, Rope::from_str(text)));
    }
    let text = std::fs::read_to_string(path).ok()?;
    Some((target, Rope::from_str(&text)))
}

/// Definition of the name at `pos` in `doc` (the document at `uri`).
pub(crate) fn goto_definition(doc: &Document, uri: &Url, pos: Position) -> Option<GotoDefinitionResponse> {
    let offset = doc.offset_at(pos);
    let text = doc.rope.to_string();
    let word = word_at(&text, offset)?;

    // Member-level goto: on `recv.member`, resolve `recv`'s type, find the
    // member's declaring type (`member_owner`) and the member's span. This
    // lands inside the generated `.jux.d` stub for a Rust-crate member, since
    // stub units carry real `.jux.d` paths in `source_paths`. Falls back to a
    // plain identifier (a type / function / const / alias name) otherwise.
    let member_target = receiver_dot_before(&text, word.start).and_then(|recv_end| {
        let ty = doc.type_ending_at(recv_end)?;
        let owner = intel::member_owner(&doc.symbols, ty, &word.text)?;
        let span = intel::member_decl_span(&doc.symbols, &owner, &word.text)?;
        let unit = *doc.symbols.decl_unit.get(&owner)?;
        Some((unit, span))
    });
    let (unit, span) = member_target.or_else(|| doc.symbols.definition_of(&word.text))?;
    // Synthetic stdlib paths aren't real files, so `unit_text` gives `None`.
    let (target_uri, target_rope) = unit_text(doc, uri, unit)?;
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: target_uri,
        range: span_to_range(&target_rope, span, doc.enc),
    }))
}
