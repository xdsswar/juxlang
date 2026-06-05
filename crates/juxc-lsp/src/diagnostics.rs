//! Mapping `juxc-diagnostics::Diagnostic` → `lsp_types::Diagnostic` (§L.7).
//!
//! The mapping is one-to-one for the structural fields. Jux `labels` become
//! LSP `relatedInformation` (they carry a span, so they get a `Location`);
//! `notes` and `help` lines have no span of their own, so they're folded into
//! the primary `message` text where the editor will still show them.

use juxc_diagnostics::{Diagnostic as JuxDiagnostic, Severity};
use ropey::Rope;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range, Url,
};

use crate::position::span_to_range;

/// Translate one Jux diagnostic into its LSP form, resolving spans against
/// `rope` and attaching label locations under `uri`.
pub fn to_lsp(rope: &Rope, uri: &Url, d: &JuxDiagnostic) -> Diagnostic {
    // A diagnostic with no primary span (synthesized) points at the very
    // start of the file — the editor still surfaces the message.
    let range = d
        .primary_span
        .map(|s| span_to_range(rope, s))
        .unwrap_or_else(|| Range::new(Position::new(0, 0), Position::new(0, 0)));

    let severity = Some(match d.severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Note => DiagnosticSeverity::INFORMATION,
        Severity::Help => DiagnosticSeverity::HINT,
    });

    // Captioned labels → relatedInformation (each has its own span/location).
    let related: Vec<DiagnosticRelatedInformation> = d
        .labels
        .iter()
        .map(|label| DiagnosticRelatedInformation {
            location: Location::new(uri.clone(), span_to_range(rope, label.span)),
            message: label.message.clone(),
        })
        .collect();

    // Spanless `note:` / `help:` lines fold into the message body.
    let mut message = d.message.clone();
    for note in &d.notes {
        message.push_str(&format!("\nnote: {note}"));
    }
    for help in &d.help {
        message.push_str(&format!("\nhelp: {help}"));
    }

    Diagnostic {
        range,
        severity,
        // Clickable E-code (e.g. "E0410"), the stable identity tooling keys off.
        code: Some(NumberOrString::String(d.code.as_str().to_string())),
        code_description: None,
        // Groups Jux diagnostics distinctly from other tooling in the editor.
        source: Some("juxc".to_string()),
        message,
        related_information: if related.is_empty() { None } else { Some(related) },
        tags: None,
        data: None,
    }
}
