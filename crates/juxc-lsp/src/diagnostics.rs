//! Mapping `juxc-diagnostics::Diagnostic` → `lsp_types::Diagnostic` (§L.7).
//!
//! The mapping is one-to-one for the structural fields. Jux `labels` become
//! LSP `relatedInformation` (they carry a span, so they get a `Location`), each
//! located in ITS OWN file: a label like "first declared here" often points
//! into another file of the project than the diagnostic itself. `notes` and
//! `help` lines have no span of their own, so they're folded into the primary
//! `message` text, where every client shows them (IntelliJ's LSP client does
//! not render `relatedInformation`, so a note placed only there would be lost).

use juxc_diagnostics::{Diagnostic as JuxDiagnostic, Severity};
use ropey::Rope;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range, Url,
};

use crate::position::{span_to_range, PositionEncoding};

/// Where a source file lives and what it said, for placing a label that
/// points into a file other than the diagnostic's own. Given a span's `file`
/// index, the resolver answers with that file's URI and text; `None` for a
/// file the editor cannot open (the embedded standard library).
pub type FileResolver<'a> = &'a dyn Fn(u32) -> Option<(Url, Rope)>;

/// Translate one Jux diagnostic into its LSP form, resolving its primary span
/// against `rope` (the file at `uri`) and each label against its own file.
pub fn to_lsp(
    rope: &Rope,
    uri: &Url,
    d: &JuxDiagnostic,
    enc: PositionEncoding,
    files: FileResolver<'_>,
) -> Diagnostic {
    // A diagnostic with no primary span (synthesized) points at the very
    // start of the file — the editor still surfaces the message.
    let range = d
        .primary_span
        .map(|s| span_to_range(rope, s, enc))
        .unwrap_or_else(|| Range::new(Position::new(0, 0), Position::new(0, 0)));

    let severity = Some(match d.severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Note => DiagnosticSeverity::INFORMATION,
        Severity::Help => DiagnosticSeverity::HINT,
    });

    // Captioned labels → relatedInformation, each in the file its span
    // indexes. A label in the diagnostic's own file stays in this file, as
    // before; a label pointing into a file the editor cannot open (the
    // standard library) has no location to give, so its caption joins the
    // message instead.
    let own_file = d.primary_span.map(|s| s.file);
    let mut message = d.message.clone();
    let mut related: Vec<DiagnosticRelatedInformation> = Vec::new();
    for label in &d.labels {
        let elsewhere = own_file.is_some_and(|f| f != label.span.file);
        let location = if elsewhere {
            match files(label.span.file) {
                Some((other_uri, other_rope)) => {
                    Location::new(other_uri, span_to_range(&other_rope, label.span, enc))
                }
                None => {
                    message.push_str(&format!("\nnote: {}", label.message));
                    continue;
                }
            }
        } else {
            Location::new(uri.clone(), span_to_range(rope, label.span, enc))
        };
        related.push(DiagnosticRelatedInformation { location, message: label.message.clone() });
    }

    // Spanless `note:` / `help:` lines fold into the message body.
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

    const U16: PositionEncoding = PositionEncoding::Utf16;

    /// A resolver that knows no other file.
    fn no_files(_: u32) -> Option<(Url, Rope)> {
        None
    }

    /// A label pointing into ANOTHER file of the project is located there, not
    /// at the same offsets of the diagnostic's own file.
    #[test]
    fn a_label_in_another_file_is_located_in_that_file() {
        let rope = Rope::from_str("var x = 1;\n");
        let other = Url::parse("file:///other.jux").unwrap();
        let other_text = "\n\npublic class Dup {}\n";
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "duplicate")
            .with_span(Span::in_file(4, 5, 1))
            .with_label(Span::in_file(2, 20, 2), "first declared here");
        let files = |f: u32| (f == 2).then(|| (other.clone(), Rope::from_str(other_text)));
        let lsp = to_lsp(&rope, &uri(), &d, U16, &files);
        let related = lsp.related_information.expect("the label is related information");
        assert_eq!(related[0].location.uri, other);
        assert_eq!(related[0].location.range.start.line, 2);
    }

    /// A label into a file the editor cannot open keeps its caption, in the
    /// message, rather than pointing at a wrong place in this file.
    #[test]
    fn a_label_in_an_unopenable_file_joins_the_message() {
        let rope = Rope::from_str("var x = 1;\n");
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "clash")
            .with_span(Span::in_file(4, 5, 3))
            .with_label(Span::in_file(0, 1, 0), "declared in the standard library");
        let lsp = to_lsp(&rope, &uri(), &d, U16, &no_files);
        assert!(lsp.related_information.is_none());
        assert!(lsp.message.contains("note: declared in the standard library"), "{}", lsp.message);
    }

    /// The E-code is the stable identity every consumer keys off — the IDE
    /// groups by it, the docs link to it, and the auto-import quick-fix is
    /// gated on its prefix. It has to survive translation verbatim.
    #[test]
    fn the_code_and_source_survive_translation() {
        let rope = Rope::from_str("var x = 1;\n");
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "mismatch")
            .with_span(Span::new(4, 5));
        let lsp = to_lsp(&rope, &uri(), &d, U16, &no_files);
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
            to_lsp(&rope, &uri(), &d, U16, &no_files).severity,
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
        let lsp = to_lsp(&rope, &uri(), &d, U16, &no_files);
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
        let lsp = to_lsp(&rope, &uri(), &d, U16, &no_files);
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
        let lsp = to_lsp(&rope, &uri(), &d, U16, &no_files);
        assert!(lsp.message.contains("mismatch"), "{}", lsp.message);
        assert!(lsp.message.contains("help: try casting it"), "{}", lsp.message);
    }

    /// No labels means no `relatedInformation` key at all, not an empty list —
    /// some clients render an empty group as a stray disclosure triangle.
    #[test]
    fn no_labels_means_no_related_information() {
        let rope = Rope::from_str("var x = 1;\n");
        let d = JuxDiagnostic::error(Code::E0410_TypeMismatch, "m").with_span(Span::new(0, 1));
        assert!(to_lsp(&rope, &uri(), &d, U16, &no_files).related_information.is_none());
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
        let lsp = to_lsp(&rope, &uri(), &d, U16, &no_files);
        // Four UTF-8 bytes for the emoji, but only two UTF-16 units.
        let expected = text[..start].chars().map(|c| c.len_utf16()).sum::<usize>() as u32;
        assert_eq!(lsp.range.start, Position::new(0, expected));
    }
}
