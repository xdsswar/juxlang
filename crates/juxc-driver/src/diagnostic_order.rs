//! The order diagnostics are shown in, for every binary that shows them.
//!
//! This lived in `bin/juxc/src/main.rs` and nowhere else, which meant the two
//! tools disagreed about the same program. `juxc --check` sorted; `jux check`
//! printed whatever order the phases happened to produce. On a file with two
//! warnings, one at line 8 and one at line 16, `juxc` said 8 then 16 and `jux`
//! said 16 then 8, and `jux` is the one people actually type.
//!
//! It also meant only one of them de-duplicated, so a diagnostic that error
//! recovery visited twice printed twice under `jux`.
//!
//! One implementation, used by both.

use juxc_diagnostics::Diagnostic;

/// Diagnostics in the order a reader meets them: by file, then by byte offset,
/// then by code so two at one position stay stable.
///
/// The phases produce them in whatever order they run (resolve before tycheck,
/// each walking its own way), which puts line 6 ahead of line 3. Sorting is
/// what makes the list scannable against the source, and what lets an
/// expected-output test pin the result at all.
///
/// Duplicates are dropped as well. The same error twice is a bug in the report,
/// not information: error recovery can revisit a construct, since the top-level
/// loop rewinds a failed declaration and retries it as a statement, and a
/// recovery anchor can land the cursor back where it started. The user should
/// not have to work out that two identical lines are one problem. Sorting
/// happens first, so duplicates are adjacent.
pub fn in_source_order(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    let mut out: Vec<&Diagnostic> = diagnostics.iter().collect();
    out.sort_by_key(|d| {
        (
            d.file.unwrap_or(usize::MAX),
            d.primary_span.map(|s| s.start).unwrap_or(u32::MAX),
            d.code.as_str(),
        )
    });
    out.dedup_by(|a, b| {
        a.code == b.code
            && a.file == b.file
            && a.primary_span.map(|s| (s.start, s.end)) == b.primary_span.map(|s| (s.start, s.end))
            && a.message == b.message
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use juxc_diagnostics::code::Code;
    use juxc_source::Span;

    fn at(file: usize, start: u32, code: Code, message: &str) -> Diagnostic {
        let mut d = Diagnostic::error(code, message.to_string());
        d.file = Some(file);
        d.primary_span = Some(Span::new(start, start + 1));
        d
    }

    /// The reason this exists: phase order is not reading order.
    #[test]
    fn sorts_by_position_not_by_the_order_phases_ran() {
        let diagnostics = vec![
            at(0, 160, Code::E0410_TypeMismatch, "later in the file"),
            at(0, 80, Code::E0410_TypeMismatch, "earlier in the file"),
        ];
        let ordered = in_source_order(&diagnostics);
        assert_eq!(ordered[0].message, "earlier in the file");
        assert_eq!(ordered[1].message, "later in the file");
    }

    /// A file with a lower index comes first, whatever the offsets say.
    #[test]
    fn sorts_by_file_before_offset() {
        let diagnostics = vec![
            at(1, 10, Code::E0410_TypeMismatch, "second file"),
            at(0, 999, Code::E0410_TypeMismatch, "first file"),
        ];
        let ordered = in_source_order(&diagnostics);
        assert_eq!(ordered[0].message, "first file");
    }

    /// The same problem reported twice is one problem.
    #[test]
    fn drops_an_exact_duplicate() {
        let diagnostics = vec![
            at(0, 40, Code::E0301_NameNotFound, "cannot find `x`"),
            at(0, 40, Code::E0301_NameNotFound, "cannot find `x`"),
        ];
        assert_eq!(in_source_order(&diagnostics).len(), 1);
    }

    /// But two different problems at one position are two problems.
    #[test]
    fn keeps_two_different_diagnostics_at_one_position() {
        let diagnostics = vec![
            at(0, 40, Code::E0301_NameNotFound, "cannot find `x`"),
            at(0, 40, Code::E0410_TypeMismatch, "expected int"),
        ];
        assert_eq!(in_source_order(&diagnostics).len(), 2);
    }

    /// A diagnostic with no span still has to come out somewhere predictable,
    /// rather than moving around between runs.
    #[test]
    fn unspanned_diagnostics_sort_last_and_stay_put() {
        let mut bare = Diagnostic::error(Code::E0400_DuplicateDeclaration, "no span".to_string());
        bare.file = Some(0);
        let diagnostics = vec![bare, at(0, 10, Code::E0410_TypeMismatch, "spanned")];
        let ordered = in_source_order(&diagnostics);
        assert_eq!(ordered[0].message, "spanned");
        assert_eq!(ordered[1].message, "no span");
    }
}
