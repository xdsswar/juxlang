//! Calls: which declaration a call site invokes, and the signature popup.
//!
//! Signature help, named-argument completion and parameter-name inlay hints
//! all ask the same question of a call site, so they share one answer
//! ([`callee_overloads`]) and cannot disagree about a parameter list.

use tower_lsp::lsp_types::*;

use crate::doc::Document;
use crate::intel;
use crate::text::{ident_start_before, is_new_context, receiver_dot_before};

/// The callee of the call whose `(` sits at `open_paren`, as (name, parameter
/// list) pairs — one per overload a constructor set provides, otherwise one.
///
/// Shared by signature help and named-argument completion, which ask the same
/// question about the same position: what does the thing being called take?
/// Keeping one answer means the popup and the parameter hints cannot disagree.
pub(crate) fn callee_overloads(
    doc: &Document,
    text: &str,
    open_paren: usize,
) -> Vec<(String, Vec<juxc_tycheck::symbol_table::ParamSig>)> {
    let bytes = text.as_bytes();
    let mut name_end = open_paren;
    while name_end > 0 && (bytes[name_end - 1] as char).is_ascii_whitespace() {
        name_end -= 1;
    }
    let name_start = ident_start_before(text, name_end);
    let callee = &text[name_start..name_end];
    if callee.is_empty() {
        return Vec::new();
    }

    if is_new_context(text, name_start) {
        // `new X(…)` — every constructor of class `X`.
        return match intel::resolve_type(&doc.symbols, callee) {
            Some(intel::Resolved::Class(_, sig)) if !sig.constructors.is_empty() => sig
                .constructors
                .iter()
                .map(|c| (callee.to_string(), c.params.clone()))
                .collect(),
            // A class with no explicit constructor → a single no-arg form.
            Some(intel::Resolved::Class(_, _)) => vec![(callee.to_string(), Vec::new())],
            _ => Vec::new(),
        };
    }
    if let Some(dot) = receiver_dot_before(text, name_start) {
        // `recv.method(…)` — resolve the receiver's type, then the method.
        return match doc.type_ending_at(dot) {
            Some(recv_ty) => match intel::resolve_member(&doc.symbols, recv_ty, callee) {
                Some(intel::Resolved::Method(_, sig)) => {
                    vec![(callee.to_string(), sig.params.clone())]
                }
                _ => Vec::new(),
            },
            None => Vec::new(),
        };
    }
    // Free function call.
    match intel::resolve_function(&doc.symbols, callee) {
        Some(intel::Resolved::Function(_, sig)) => vec![(callee.to_string(), sig.params.clone())],
        _ => Vec::new(),
    }
}

/// Scanning left from `offset`, find the `(` that opens the call whose argument
/// list the caret sits inside, plus the active parameter index (the number of
/// top-level commas before the caret). Nested parentheses are balanced; a
/// top-level `;`/`{`/`}` ends the search (we're no longer inside a call).
/// Returns `None` when the caret isn't inside any call's `( … )`.
pub(crate) fn find_enclosing_call(text: &str, offset: usize) -> Option<(usize, u32)> {
    let bytes = text.as_bytes();
    let mut i = offset.min(bytes.len());
    let mut depth = 0i32;
    let mut commas = 0u32;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                if depth == 0 {
                    return Some((i, commas));
                }
                depth -= 1;
            }
            b',' if depth == 0 => commas += 1,
            b';' | b'{' | b'}' if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

/// Build one [`SignatureInformation`] for `callee` from its parameter list,
/// rendering each parameter as `Type name` (Jux syntax) so the popup reads like
/// the declaration.
pub(crate) fn signature_info(
    callee: &str,
    params: &[juxc_tycheck::symbol_table::ParamSig],
) -> SignatureInformation {
    let labels: Vec<String> = params
        .iter()
        .map(|p| format!("{} {}", intel::render_type(&p.ty), p.name))
        .collect();
    let label = format!("{callee}({})", labels.join(", "));
    let parameters = labels
        .into_iter()
        .map(|l| ParameterInformation {
            label: ParameterLabel::Simple(l),
            documentation: None,
        })
        .collect();
    SignatureInformation {
        label,
        documentation: None,
        parameters: Some(parameters),
        active_parameter: None,
    }
}

/// Parameter info (`textDocument/signatureHelp`). When the caret is inside a
/// call's argument list, resolve the callee (a free function, a `new X`
/// constructor, or a `recv.method`) and return its signature(s) with the
/// active parameter (the count of top-level commas before the caret)
/// highlighted.
pub(crate) fn signature_help(doc: &Document, pos: Position) -> Option<SignatureHelp> {
    let offset = doc.offset_at(pos);
    let text = doc.rope.to_string();

    // The `(` that opens the call the caret sits inside, and how many
    // arguments precede the caret.
    let (open_paren, active_param) = find_enclosing_call(&text, offset)?;

    let signatures: Vec<SignatureInformation> = callee_overloads(doc, &text, open_paren)
        .iter()
        .map(|(name, params)| signature_info(name, params))
        .collect();
    if signatures.is_empty() {
        return None;
    }
    Some(SignatureHelp {
        signatures,
        active_signature: Some(0),
        active_parameter: Some(active_param),
    })
}
