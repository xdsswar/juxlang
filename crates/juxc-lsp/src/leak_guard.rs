//! The language server's exits pass the one leak detector (GAPS.md gap 33).
//!
//! What an editor shows is read by the same programmer the compiler writes
//! for, so it follows the same rule: no Rust type, path, trait, lifetime or
//! borrow, no rustc or cargo words, no `.rs` location. Hover, completion
//! details, signature help and published diagnostics are rendered from the
//! checker's Jux signatures and the compiler's Jux diagnostics, so none of
//! them should ever hit; this is the safety net for the day one does.
//!
//! A hit panics under the self-check (`JUX_SELFCHECK=1`) and in tests, so the
//! test that produced it fails. Otherwise the leaking text is withheld: a
//! hover or signature is not shown, a completion keeps its name and loses its
//! detail, and a diagnostic becomes the `E0900` internal compiler error the
//! command line shows (`juxc_diagnostics::leak::guard_diagnostic`).
//!
//! Text the program itself contains is not a leak: `sources` are the texts
//! the shown text was built from (the open buffer, the declaring file), and a
//! doc comment copied from a declaration the user can open is theirs.

use juxc_diagnostics::leak;
use tower_lsp::lsp_types::{CompletionItem, Documentation, Hover, HoverContents, MarkedString, SignatureHelp};

/// Whether `text` shows Rust that none of `sources` contains. Panics on a hit
/// under the self-check.
pub(crate) fn shows_rust(what: &str, text: &str, sources: &[&str]) -> bool {
    let wrote = leak::contained_in(sources);
    match leak::find_rust_leak_quoting(text, &wrote) {
        None => false,
        Some(hit) => {
            if cfg!(test) || leak::selfcheck() {
                panic!("the language server's {what} shows Rust to the user: {hit}\n{text}");
            }
            true
        }
    }
}

/// A hover, unless its text shows Rust.
pub(crate) fn hover(h: Option<Hover>, sources: &[&str]) -> Option<Hover> {
    let h = h?;
    let text = match &h.contents {
        HoverContents::Markup(m) => m.value.clone(),
        HoverContents::Scalar(s) => marked(s),
        HoverContents::Array(parts) => parts.iter().map(marked).collect::<Vec<_>>().join("\n"),
    };
    (!shows_rust("hover", &text, sources)).then_some(h)
}

fn marked(s: &MarkedString) -> String {
    match s {
        MarkedString::String(t) => t.clone(),
        MarkedString::LanguageString(l) => l.value.clone(),
    }
}

/// Completion items with any detail or documentation that shows Rust taken
/// off, and any item whose NAME does dropped.
pub(crate) fn completions(items: Vec<CompletionItem>, sources: &[&str]) -> Vec<CompletionItem> {
    items
        .into_iter()
        .filter(|item| !shows_rust("completion label", &item.label, sources))
        .map(|mut item| {
            if item.detail.as_deref().is_some_and(|d| shows_rust("completion detail", d, sources)) {
                item.detail = None;
            }
            let doc = match &item.documentation {
                Some(Documentation::String(s)) => Some(s.clone()),
                Some(Documentation::MarkupContent(m)) => Some(m.value.clone()),
                None => None,
            };
            if doc.is_some_and(|d| shows_rust("completion documentation", &d, sources)) {
                item.documentation = None;
            }
            item
        })
        .collect()
}

/// Signature help without any signature whose text shows Rust.
pub(crate) fn signature_help(help: Option<SignatureHelp>, sources: &[&str]) -> Option<SignatureHelp> {
    let mut help = help?;
    help.signatures.retain(|s| {
        let params: Vec<String> = s
            .parameters
            .iter()
            .flatten()
            .filter_map(|p| match &p.label {
                tower_lsp::lsp_types::ParameterLabel::Simple(t) => Some(t.clone()),
                tower_lsp::lsp_types::ParameterLabel::LabelOffsets(_) => None,
            })
            .collect();
        !shows_rust("signature help", &format!("{}\n{}", s.label, params.join("\n")), sources)
    });
    (!help.signatures.is_empty()).then_some(help)
}

/// A diagnostic on its way to the editor: the compiler's own guard, with the
/// open buffer as what the program says.
pub(crate) fn diagnostic(
    d: &juxc_diagnostics::Diagnostic,
    buffer: &str,
) -> Option<juxc_diagnostics::Diagnostic> {
    let texts = [buffer];
    let wrote = leak::contained_in(&texts);
    leak::guard_diagnostic(
        d,
        &wrote,
        cfg!(test) || leak::selfcheck(),
        false,
        juxc_driver::ice::ISSUES_URL,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{MarkupContent, MarkupKind, SignatureInformation};

    #[test]
    fn jux_text_passes() {
        let h = Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```jux\npublic void push(T value)\n```\n\nAdds a value.".to_string(),
            }),
            range: None,
        };
        assert!(hover(Some(h), &[]).is_some());
        let items = completions(
            vec![CompletionItem { label: "push".to_string(), detail: Some("Map<String, int>".to_string()), ..Default::default() }],
            &[],
        );
        assert_eq!(items[0].detail.as_deref(), Some("Map<String, int>"));
        let sh = SignatureHelp {
            signatures: vec![SignatureInformation {
                label: "push(T value)".to_string(),
                documentation: None,
                parameters: None,
                active_parameter: None,
            }],
            active_signature: None,
            active_parameter: None,
        };
        assert!(signature_help(Some(sh), &[]).is_some());
    }

    #[test]
    #[should_panic(expected = "shows Rust to the user")]
    fn a_leaking_hover_fails_the_test_that_made_it() {
        let h = Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```jux\nfn push(&mut self, value: T)\n```".to_string(),
            }),
            range: None,
        };
        let _ = hover(Some(h), &[]);
    }

    #[test]
    fn a_doc_comment_from_the_declaration_is_the_users_own() {
        let decl = "/** See `std::fmt` for the format. */\npublic void show();";
        let h = Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```jux\npublic void show()\n```\n\nSee `std::fmt` for the format.".to_string(),
            }),
            range: None,
        };
        assert!(hover(Some(h), &[decl]).is_some());
    }
}
