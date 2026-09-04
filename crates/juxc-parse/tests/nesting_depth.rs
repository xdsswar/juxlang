//! Deeply nested input must produce a diagnostic, never a stack overflow.
//!
//! The parser is recursive descent, so source nesting becomes compiler stack
//! depth. Before the depth budget existed, roughly sixty nested parentheses
//! aborted the process with `thread 'main' has overflowed its stack` -- no
//! error code, no file name, nothing a caller could report.
use juxc_diagnostics::code::Code;
use juxc_lex::lex;
use juxc_parse::parse;
use juxc_source::SourceFile;

/// A program whose `main` assigns `((( ... 1 ... )))` at the requested depth.
fn nested_parens(depth: usize) -> String {
    format!(
        "public void main() {{ int x = {}1{}; }}",
        "(".repeat(depth),
        ")".repeat(depth),
    )
}

/// Parse on a thread with a large stack, which is how the compiler actually
/// runs the front end (`juxc_driver::big_stack`, used by `juxc`, `jux` and the
/// language server). The depth budget bounds recursion at
/// `juxc_parse::MAX_NESTING` levels, and that many levels of parser frames do
/// not fit in the 2 MB a test-harness thread gets by default -- so testing on a
/// default thread would measure the harness, not the parser.
fn parse_src(src: &str) -> Vec<Code> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let sf = SourceFile::new("nesting.jux", &src);
            let lexed = lex(&sf);
            let parsed = parse(&lexed.tokens);
            parsed.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>()
        })
        .expect("spawning the parse thread")
        .join()
        .expect("the parser must report a diagnostic, not overflow the stack")
}

/// Nesting a program is unlikely to reach must still parse cleanly. The budget
/// is a backstop against pathological input, not a style rule -- rejecting code
/// a person might actually write would be worse than the crash it replaces.
#[test]
fn ordinary_nesting_is_accepted() {
    for depth in [1, 8, 32, 100] {
        let codes = parse_src(&nested_parens(depth));
        assert!(codes.is_empty(), "depth {depth} should parse cleanly, got {codes:?}");
    }
}

/// Past the budget the parser reports E0201 instead of recursing until the
/// stack runs out.
#[test]
fn over_deep_nesting_reports_e0201() {
    let codes = parse_src(&nested_parens(2_000));
    assert!(
        codes.contains(&Code::E0201_NestingTooDeep),
        "expected E0201 for 2000 levels of nesting, got {codes:?}",
    );
}

/// One error, not one per level. Every enclosing level would otherwise report
/// the same limit on the way back out, burying the real message under thousands
/// of copies.
#[test]
fn the_depth_limit_is_reported_once() {
    let codes = parse_src(&nested_parens(2_000));
    let hits = codes.iter().filter(|c| **c == Code::E0201_NestingTooDeep).count();
    assert_eq!(hits, 1, "expected exactly one E0201, got {hits}: {codes:?}");
}

/// The guard covers types and statements too, not just expressions: a deeply
/// nested generic type is the same recursion through `parse_type_ref`.
#[test]
fn over_deep_generic_type_reports_e0201() {
    let depth = 2_000;
    let src = format!(
        "public void main() {{ {}int{} x; }}",
        "List<".repeat(depth),
        ">".repeat(depth),
    );
    let codes = parse_src(&src);
    assert!(
        codes.contains(&Code::E0201_NestingTooDeep),
        "expected E0201 for a deeply nested generic type, got {codes:?}",
    );
}
