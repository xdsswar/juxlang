//! W0820: an `unsafe { }` block with no `// SAFETY:` comment (Layout-ABI
//! §L.5.5).
//!
//! The spec asks every `unsafe` block to say why the obligations of §L.5.4
//! hold, in a comment beginning `SAFETY:`, so a reviewer can check the
//! argument instead of reconstructing it. Comments never reach the AST, so the
//! lint works on the file itself: it re-lexes it (which also drops comments
//! and string contents, so an `unsafe {` inside either is never mistaken for a
//! block) and, for every `unsafe` token directly followed by `{`, reads the
//! source lines around it. `unsafe native {` and `unsafe void f()` never match
//! that token pair, so only blocks are linted.
//!
//! The justification is found in either place people write it:
//!
//! - the comment lines directly above the `unsafe` line (a `//` run or a
//!   `/* ... */` block, with no blank line in between), or
//! - the comment lines at the very top of the block, right after `{`.
//!
//! It is a warning raised by the checking entry points only (`juxc --check`,
//! `jux check`, the editor): a build stays quiet, the way Rust keeps its
//! equivalent lint out of `cargo build`.

use juxc_diagnostics::{code, Diagnostic};
use juxc_lex::{Keyword, TokenKind};
use juxc_source::SourceFile;

/// W0820 for every `unsafe { }` block in `source` that has no `// SAFETY:`
/// comment above it or at the top of its body.
pub(crate) fn check_safety_comments(source: &SourceFile) -> Vec<Diagnostic> {
    let tokens = juxc_lex::lex(source).tokens;
    let text = source.contents();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for pair in tokens.windows(2) {
        if !(matches!(pair[0].kind, TokenKind::Kw(Keyword::Unsafe)) && matches!(pair[1].kind, TokenKind::LBrace)) {
            continue;
        }
        let (line, _) = source.line_col(pair[0].span.start as usize);
        let (brace_line, _) = source.line_col(pair[1].span.start as usize);
        // `line_col` is 1-based; the slice below is 0-based.
        let above = justified_above(&lines, line as usize - 1);
        let inside = justified_inside(&lines, brace_line as usize - 1);
        if !above && !inside {
            out.push(
                Diagnostic::warning(
                    code::Code::W0820_UnsafeWithoutSafetyComment,
                    "this `unsafe` block has no `// SAFETY:` comment -- say why its obligations hold \
                     (the pointer is valid, the memory is not aliased, ...) (§L.5.5)",
                )
                .with_span(pair[0].span)
                .with_file(source.index() as usize),
            );
        }
    }
    out
}

/// Whether the comment lines directly above line `at` (0-based) mention
/// `SAFETY:`. The run stops at the first line that is not a comment, a blank
/// line included, so a comment belongs to the block only when it touches it.
fn justified_above(lines: &[&str], at: usize) -> bool {
    let mut i = at;
    while i > 0 {
        i -= 1;
        let l = lines[i].trim();
        if !is_comment_line(l) {
            return false;
        }
        if l.contains("SAFETY:") {
            return true;
        }
    }
    false
}

/// Whether the comment lines at the top of the block opened on line
/// `brace_line` (0-based) mention `SAFETY:`: the rest of that line after `{`,
/// then every following line while it is a comment.
fn justified_inside(lines: &[&str], brace_line: usize) -> bool {
    if let Some(rest) = lines.get(brace_line).and_then(|l| l.split_once('{')).map(|(_, r)| r.trim()) {
        if rest.contains("SAFETY:") {
            return true;
        }
    }
    for l in lines.iter().skip(brace_line + 1) {
        let l = l.trim();
        if !is_comment_line(l) {
            return false;
        }
        if l.contains("SAFETY:") {
            return true;
        }
    }
    false
}

/// A line that is only a comment: `//`, or a line of a `/* ... */` block.
fn is_comment_line(l: &str) -> bool {
    l.starts_with("//") || l.starts_with("/*") || l.starts_with('*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lint(src: &str) -> usize {
        check_safety_comments(&SourceFile::new("t.jux", src)).len()
    }

    /// A block with no justification warns; one with a comment above or at
    /// its top does not; `unsafe native` and `unsafe` functions are not blocks.
    #[test]
    fn safety_comment_placements() {
        assert_eq!(lint("void f() {\n    unsafe {\n        g();\n    }\n}\n"), 1);
        assert_eq!(lint("void f() {\n    // SAFETY: p is valid.\n    unsafe {\n        g();\n    }\n}\n"), 0);
        assert_eq!(lint("void f() {\n    // SAFETY: two lines,\n    // still one comment.\n    unsafe { g(); }\n}\n"), 0);
        assert_eq!(lint("void f() {\n    unsafe {\n        // SAFETY: inside.\n        g();\n    }\n}\n"), 0);
        assert_eq!(lint("void f() {\n    unsafe { // SAFETY: same line.\n        g();\n    }\n}\n"), 0);
        // A blank line cuts the comment off from the block.
        assert_eq!(lint("void f() {\n    // SAFETY: too far.\n\n    unsafe { g(); }\n}\n"), 1);
        assert_eq!(lint("@extern(lib = \"c\")\nunsafe native {\n    int puts(String s);\n}\n"), 0);
        assert_eq!(lint("unsafe void f() {\n    g();\n}\n"), 0);
        // `unsafe {` inside a string or a comment is not a block.
        assert_eq!(lint("void f() {\n    print(\"unsafe { x }\");\n    // unsafe { y }\n}\n"), 0);
    }
}
