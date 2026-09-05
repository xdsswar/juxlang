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

#[cfg(test)]
mod tests {
    use super::*;
    use juxc_diagnostics::code::Code;
    use juxc_source::Span;

    fn uri() -> Url {
        Url::parse("file:///t.jux").unwrap()
    }

    /// The E-code is the stable identity every consumer keys off — the IDE
    /// groups by it, the docs link to it, and the auto-import quick-fix is
    /// gated on its prefix. It has to survive translation verbatim.
    #[test]
    fn the_code_and_source_survive_translation() {
        let rope = Rope::from_str("var x = 1;\n");
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "mismatch")
            .with_span(Span::new(4, 5));
        let lsp = to_lsp(&rope, &uri(), &d);
        assert_eq!(lsp.code, Some(NumberOrString::String("E0410".to_string())));
        assert_eq!(lsp.source.as_deref(), Some("juxc"));
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(lsp.range.start, Position::new(0, 4));
        assert_eq!(lsp.range.end, Position::new(0, 5));
    }

    #[test]
    fn a_warning_maps_to_warning_severity() {
        let rope = Rope::from_str("var x = 1;\n");
        let d = JuxDiagnostic::warning(Code::E0410_TypeMismatch, "careful")
            .with_span(Span::new(0, 3));
        assert_eq!(
            to_lsp(&rope, &uri(), &d).severity,
            Some(DiagnosticSeverity::WARNING),
        );
    }

    /// A spanless diagnostic must still reach the user. Dropping it — which is
    /// what a consumer that requires a span does — is how a real error becomes
    /// invisible.
    #[test]
    fn a_spanless_diagnostic_points_at_the_file_start() {
        let rope = Rope::from_str("var x = 1;\n");
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "no span here");
        let lsp = to_lsp(&rope, &uri(), &d);
        assert_eq!(lsp.range.start, Position::new(0, 0));
        assert_eq!(lsp.range.end, Position::new(0, 0));
        assert_eq!(lsp.message, "no span here");
    }

    /// A label carries its own span, so it becomes clickable related
    /// information rather than being flattened into the message.
    #[test]
    fn labels_become_related_information() {
        let rope = Rope::from_str("var x = 1;\nvar y = 2;\n");
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "mismatch")
            .with_span(Span::new(4, 5))
            .with_label(Span::new(15, 16), "declared here");
        let lsp = to_lsp(&rope, &uri(), &d);
        let related = lsp.related_information.expect("labels become related info");
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].message, "declared here");
        // Second line, resolved against the rope rather than assumed.
        assert_eq!(related[0].location.range.start.line, 1);
    }

    /// A spanless `help:` line has nowhere to point, so it folds into the
    /// message — losing it would drop the sentence that says how to fix things.
    #[test]
    fn help_lines_fold_into_the_message() {
        let rope = Rope::from_str("var x = 1;\n");
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "mismatch")
            .with_span(Span::new(4, 5))
            .with_help("try casting it");
        let lsp = to_lsp(&rope, &uri(), &d);
        assert!(lsp.message.contains("mismatch"), "{}", lsp.message);
        assert!(lsp.message.contains("help: try casting it"), "{}", lsp.message);
    }

    /// No labels means no `relatedInformation` key at all, not an empty list —
    /// some clients render an empty group as a stray disclosure triangle.
    #[test]
    fn no_labels_means_no_related_information() {
        let rope = Rope::from_str("var x = 1;\n");
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "m").with_span(Span::new(0, 1));
        assert!(to_lsp(&rope, &uri(), &d).related_information.is_none());
    }

    /// Spans are byte offsets; LSP columns are UTF-16 units. A diagnostic after
    /// a multi-byte character has to land on the right column — the whole
    /// reason the position module exists.
    #[test]
    fn a_span_after_a_multibyte_character_maps_to_utf16_columns() {
        let text = "var s = \"\u{1F600}\"; var y = 1;";
        let rope = Rope::from_str(text);
        let start = text.rfind('y').unwrap();
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "m")
            .with_span(Span::new(start as u32, (start + 1) as u32));
        let lsp = to_lsp(&rope, &uri(), &d);
        // Four UTF-8 bytes for the emoji, but only two UTF-16 units.
        let expected = text[..start].chars().map(|c| c.len_utf16()).sum::<usize>() as u32;
        assert_eq!(lsp.range.start, Position::new(0, expected));
    }
}
