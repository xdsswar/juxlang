//! `textDocument/semanticTokens`: what each identifier IS, for coloring.
//!
//! The TextMate grammar colors by shape; it cannot tell a type from a value,
//! a parameter from a field, or a static method from an instance one. These
//! tokens answer exactly that (§L.5 "semanticTokens precedence"), from the
//! same resolution references and rename use (`xref`), so the editor colors
//! a name the way "find usages" sees it. Keywords, literals and comments are
//! left to the grammar: the server emits identifiers only.

use tower_lsp::lsp_types::*;

use juxc_tycheck::SymbolTable;

use crate::doc::Document;
use crate::text::receiver_dot_before;
use crate::xref::{self, BindingKind, DeclKind, FileFacts, FileModel, Target};

/// Token types, in legend order. The index is what goes on the wire.
pub(crate) const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::TYPE,
    SemanticTokenType::CLASS,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
];

/// Token modifiers, in legend order; bit `i` is modifier `i`.
pub(crate) const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];

/// The legend advertised in the server capabilities.
pub(crate) fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: TOKEN_MODIFIERS.to_vec(),
    }
}

fn ty(t: SemanticTokenType) -> u32 {
    TOKEN_TYPES.iter().position(|x| *x == t).expect("type in legend") as u32
}

const DECLARATION: u32 = 1 << 0;
const STATIC: u32 = 1 << 1;
const READONLY: u32 = 1 << 2;
const DEFAULT_LIBRARY: u32 = 1 << 3;

/// One classified identifier, before delta encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Classified {
    /// Byte range.
    pub(crate) start: usize,
    /// Exclusive end.
    pub(crate) end: usize,
    /// Legend index of the type.
    pub(crate) token_type: u32,
    /// Modifier bits.
    pub(crate) modifiers: u32,
}

/// What kind of member `name` is on `owner`, and whether it is static.
fn member_kind(symbols: &SymbolTable, owner: &str, name: &str) -> Option<(SemanticTokenType, bool)> {
    if let Some(c) = symbols.classes.get(owner) {
        if let Some(m) = c.methods.get(name) {
            let kind = if m.is_property { SemanticTokenType::PROPERTY } else { SemanticTokenType::METHOD };
            return Some((kind, m.is_static));
        }
        if let Some(f) = c.fields.get(name) {
            return Some((SemanticTokenType::PROPERTY, f.is_static));
        }
        if let Some(p) = c.properties.get(name) {
            return Some((SemanticTokenType::PROPERTY, p.is_static));
        }
    }
    if let Some(i) = symbols.interfaces.get(owner) {
        if let Some(m) = i.methods.get(name) {
            return Some((SemanticTokenType::METHOD, m.is_static));
        }
        if let Some(f) = i.fields.get(name) {
            return Some((SemanticTokenType::PROPERTY, f.is_static));
        }
    }
    if let Some(r) = symbols.records.get(owner) {
        if let Some(m) = r.methods.get(name) {
            return Some((SemanticTokenType::METHOD, m.is_static));
        }
        if r.components.iter().any(|c| c.name == name) {
            return Some((SemanticTokenType::PROPERTY, false));
        }
    }
    if let Some(e) = symbols.enums.get(owner) {
        if e.variants.contains_key(name) {
            return Some((SemanticTokenType::ENUM_MEMBER, true));
        }
        if let Some(m) = e.methods.get(name) {
            return Some((SemanticTokenType::METHOD, m.is_static));
        }
    }
    None
}

/// Classify every identifier of `doc` that has a meaning worth coloring.
pub(crate) fn classify(doc: &Document) -> Vec<Classified> {
    let text = doc.rope.to_string();
    let model = FileModel::build(&text);
    let facts = FileFacts::new(&doc.symbols, &doc.expr_types, doc.file);
    let symbols = &*doc.symbols;
    // A declaration in a unit with no kept text is the standard library or a
    // generated stub.
    let is_library = |unit: usize| doc.file != Some(unit as u32) && !matches!(doc.source_texts.get(unit), Some(Some(_)));
    let decl_start = |target: &Target| xref::declaration_site(symbols, target);

    let mut out = Vec::new();
    for occ in &model.idents {
        let push = |out: &mut Vec<Classified>, token_type: SemanticTokenType, modifiers: u32| {
            out.push(Classified { start: occ.start, end: occ.end, token_type: ty(token_type), modifiers });
        };
        let target = xref::resolve(&facts, &model, occ);
        match target {
            Some(Target::Local { decl, .. }) => {
                let kind = match model.binding_at(&occ.name, occ.start).map(|b| b.kind) {
                    Some(BindingKind::Param) => SemanticTokenType::PARAMETER,
                    _ => SemanticTokenType::VARIABLE,
                };
                let modifiers = if decl.0 == occ.start { DECLARATION } else { 0 };
                push(&mut out, kind, modifiers);
            }
            Some(ref t @ Target::Decl { kind, unit, .. }) => {
                let (token, mut modifiers) = match kind {
                    DeclKind::Class => {
                        let is_struct = match t {
                            Target::Decl { fqn, .. } => symbols.classes.get(fqn).is_some_and(|c| c.is_struct),
                            _ => false,
                        };
                        (if is_struct { SemanticTokenType::STRUCT } else { SemanticTokenType::CLASS }, 0)
                    }
                    DeclKind::Interface => (SemanticTokenType::INTERFACE, 0),
                    DeclKind::Enum => (SemanticTokenType::ENUM, 0),
                    DeclKind::Record => (SemanticTokenType::STRUCT, 0),
                    DeclKind::Alias => (SemanticTokenType::TYPE, 0),
                    DeclKind::Function => (SemanticTokenType::FUNCTION, 0),
                    DeclKind::Const => (SemanticTokenType::VARIABLE, READONLY),
                };
                if is_library(unit) {
                    modifiers |= DEFAULT_LIBRARY;
                }
                if doc.file == Some(unit as u32) && is_first_after(&model, occ, decl_start(t)) {
                    modifiers |= DECLARATION;
                }
                push(&mut out, token, modifiers);
            }
            Some(ref t @ Target::Member { ref owner, ref name }) => {
                let Some((token, is_static)) = member_kind(symbols, owner, name) else { continue };
                let mut modifiers = if is_static { STATIC } else { 0 };
                if let Some(&unit) = symbols.decl_unit.get(owner) {
                    if is_library(unit) {
                        modifiers |= DEFAULT_LIBRARY;
                    }
                    if doc.file == Some(unit as u32) && is_first_after(&model, occ, decl_start(t)) {
                        modifiers |= DECLARATION;
                    }
                }
                push(&mut out, token, modifiers);
            }
            None => {
                if model.in_header(occ.start) {
                    push(&mut out, SemanticTokenType::NAMESPACE, 0);
                } else if model.is_type_param(&occ.name, occ.start) {
                    push(&mut out, SemanticTokenType::TYPE_PARAMETER, 0);
                } else if receiver_dot_before(&text, occ.start).is_some() {
                    // A member whose receiver type is unknown still reads as
                    // one: a call is a method, anything else a property.
                    let called = text[occ.end..].trim_start().starts_with('(');
                    let token = if called { SemanticTokenType::METHOD } else { SemanticTokenType::PROPERTY };
                    push(&mut out, token, 0);
                }
            }
        }
    }
    out
}

/// True when `occ` is the first identifier of its name at or after the
/// declaration start `site` (the declaration's own name).
fn is_first_after(model: &FileModel, occ: &xref::Occurrence, site: Option<(usize, u32)>) -> bool {
    let Some((_, start)) = site else { return false };
    model
        .idents
        .iter()
        .find(|o| o.start >= start as usize && o.name == occ.name)
        .is_some_and(|o| o.start == occ.start)
}

/// Delta-encode `tokens` (sorted by position) for the wire, in `doc`'s
/// position encoding. Only tokens that start in `range` when one is given.
pub(crate) fn encode(doc: &Document, tokens: &[Classified], range: Option<Range>) -> Vec<SemanticToken> {
    let mut out = Vec::new();
    let (mut prev_line, mut prev_col) = (0u32, 0u32);
    for tok in tokens {
        let start = doc.position_at(tok.start);
        if let Some(r) = range {
            if start < r.start || start >= r.end {
                continue;
            }
        }
        let end = doc.position_at(tok.end);
        let length = if end.line == start.line { end.character - start.character } else { 0 };
        if length == 0 {
            continue;
        }
        let delta_line = start.line - prev_line;
        let delta_start = if delta_line == 0 { start.character - prev_col } else { start.character };
        out.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: tok.token_type,
            token_modifiers_bitset: tok.modifiers,
        });
        prev_line = start.line;
        prev_col = start.character;
    }
    out
}

/// `textDocument/semanticTokens/full` (and `/range` with `range`).
pub(crate) fn semantic_tokens(doc: &Document, range: Option<Range>) -> SemanticTokens {
    let tokens = classify(doc);
    SemanticTokens { result_id: None, data: encode(doc, &tokens, range) }
}
