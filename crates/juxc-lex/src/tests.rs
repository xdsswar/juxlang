//! Unit tests for the lexer.
//!
//! These exercise the spec corners called out in `JUX-GRAMMAR-ADDENDUM.md`
//! §A.1, plus the milestone-1 vehicle (the full `hello.jux` token stream).

use crate::{lex, Keyword, TokenKind};
use juxc_source::SourceFile;

fn kinds(src: &str) -> Vec<TokenKind> {
    let sf = SourceFile::new("test.jux", src);
    let r = lex(&sf);
    assert!(r.diagnostics.is_empty(), "unexpected diagnostics: {:?}", r.diagnostics);
    r.tokens.into_iter().map(|t| t.kind).collect()
}

fn kinds_with_diags(src: &str) -> (Vec<TokenKind>, usize) {
    let sf = SourceFile::new("test.jux", src);
    let r = lex(&sf);
    let n = r.diagnostics.len();
    (r.tokens.into_iter().map(|t| t.kind).collect(), n)
}

// ---------------------------------------------------------------------------
// Triviality
// ---------------------------------------------------------------------------

#[test]
fn empty_source_yields_only_eof() {
    assert_eq!(kinds(""), vec![TokenKind::Eof]);
}

#[test]
fn whitespace_and_comments_are_dropped() {
    let src = "   \t\n  // line comment\n  /* block */ \n";
    assert_eq!(kinds(src), vec![TokenKind::Eof]);
}

#[test]
fn block_comment_does_not_nest() {
    // Per §A.1.2: block comments do NOT nest. After the first `*/`, the
    // remaining `*/` is left to the lexer, which sees `*` then `/` —
    // emitted as Star Slash tokens.
    let src = "/* outer /* inner */ */";
    assert_eq!(kinds(src), vec![TokenKind::Star, TokenKind::Slash, TokenKind::Eof]);
}

#[test]
fn utf8_bom_is_skipped() {
    let src = "\u{FEFF}main";
    let ks = kinds(src);
    assert_eq!(ks, vec![TokenKind::Ident("main".to_string()), TokenKind::Eof]);
}

// ---------------------------------------------------------------------------
// Identifiers and keywords
// ---------------------------------------------------------------------------

#[test]
fn identifier_basic() {
    assert_eq!(kinds("foo"), vec![TokenKind::Ident("foo".to_string()), TokenKind::Eof]);
}

#[test]
fn keyword_classification_is_case_sensitive() {
    // `public` is a keyword; `Public` is an identifier.
    assert_eq!(
        kinds("public Public"),
        vec![
            TokenKind::Kw(Keyword::Public),
            TokenKind::Ident("Public".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn annotation_keyword_recognized() {
    // Added to §3.2 in the recent integration pass.
    assert_eq!(kinds("annotation"), vec![TokenKind::Kw(Keyword::Annotation), TokenKind::Eof]);
}

#[test]
fn true_false_null_are_literal_tokens_not_idents() {
    assert_eq!(
        kinds("true false null"),
        vec![TokenKind::Bool(true), TokenKind::Bool(false), TokenKind::Null, TokenKind::Eof]
    );
}

// ---------------------------------------------------------------------------
// Punctuation: longest-match disambiguation
// ---------------------------------------------------------------------------

#[test]
fn dot_family_longest_match() {
    // `...` Ellipsis, `..=` DotDotEq, `..` DotDot, `.` Dot.
    let src = "... ..= .. .";
    assert_eq!(
        kinds(src),
        vec![
            TokenKind::Ellipsis, TokenKind::DotDotEq, TokenKind::DotDot, TokenKind::Dot,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lt_family_longest_match() {
    // `<=>` Spaceship, `<<=` LtLtEq, `<<` LtLt, `<=` Le, `<` Lt.
    let src = "<=> <<= << <= <";
    assert_eq!(
        kinds(src),
        vec![
            TokenKind::Spaceship, TokenKind::LtLtEq, TokenKind::LtLt,
            TokenKind::Le, TokenKind::Lt, TokenKind::Eof,
        ]
    );
}

#[test]
fn wrapping_operator_family() {
    // §S.2.1: `+%` `-%` `*%` `<<%` `>>%` — wrapping arithmetic. The
    // `%` suffix must win over emitting the base operator + Percent.
    let src = "+% -% *% <<% >>% + % +=";
    assert_eq!(
        kinds(src),
        vec![
            TokenKind::PlusPercent, TokenKind::MinusPercent, TokenKind::StarPercent,
            TokenKind::LtLtPercent, TokenKind::GtGtPercent,
            TokenKind::Plus, TokenKind::Percent, TokenKind::PlusEq,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn gt_is_emitted_unsplit_even_when_doubled() {
    // §A.1.6: the lexer emits `>>` as a single token; parser splits.
    assert_eq!(
        kinds(">>"),
        vec![TokenKind::GtGt, TokenKind::Eof]
    );
}

#[test]
fn arrows() {
    assert_eq!(
        kinds("-> =>"),
        vec![TokenKind::Arrow, TokenKind::FatArrow, TokenKind::Eof]
    );
}

#[test]
fn equality_and_strict_equality() {
    assert_eq!(
        kinds("= == === != !=="),
        vec![
            TokenKind::Eq, TokenKind::EqEq, TokenKind::StrictEq,
            TokenKind::NotEq, TokenKind::StrictNotEq, TokenKind::Eof,
        ]
    );
}

#[test]
fn bangbang_distinct_from_bang() {
    assert_eq!(
        kinds("!! !"),
        vec![TokenKind::BangBang, TokenKind::Bang, TokenKind::Eof]
    );
}

#[test]
fn question_family() {
    assert_eq!(
        kinds("? ?. ?: ??"),
        vec![
            TokenKind::Question,
            TokenKind::QuestionDot,
            TokenKind::QuestionColon,
            TokenKind::QuestionQuestion,
            TokenKind::Eof,
        ],
    );
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

#[test]
fn decimal_int() {
    assert_eq!(kinds("42"), vec![TokenKind::Int("42".to_string()), TokenKind::Eof]);
}

#[test]
fn int_with_separators() {
    assert_eq!(kinds("1_000_000"), vec![TokenKind::Int("1_000_000".to_string()), TokenKind::Eof]);
}

#[test]
fn hex_int() {
    assert_eq!(kinds("0xFF"), vec![TokenKind::Int("0xFF".to_string()), TokenKind::Eof]);
}

#[test]
fn binary_int() {
    assert_eq!(kinds("0b1010"), vec![TokenKind::Int("0b1010".to_string()), TokenKind::Eof]);
}

#[test]
fn long_suffix() {
    assert_eq!(kinds("42L"), vec![TokenKind::Int("42L".to_string()), TokenKind::Eof]);
}

#[test]
fn float_with_dot_exponent_and_suffix() {
    assert_eq!(
        kinds("3.14 1e10f"),
        vec![
            TokenKind::Float("3.14".to_string()),
            TokenKind::Float("1e10f".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn dot_after_digit_is_member_access_not_float() {
    // `42.foo` — current rule is "fractional part requires digit before AND
    // digit after the dot," so `42.foo` parses as `42 . foo`.
    assert_eq!(
        kinds("42.foo"),
        vec![
            TokenKind::Int("42".to_string()),
            TokenKind::Dot,
            TokenKind::Ident("foo".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn leading_underscore_in_hex_run_is_e0102() {
    let (_, ndiag) = kinds_with_diags("0x_FF");
    assert!(ndiag >= 1, "expected at least one digit-separator diagnostic");
}

// ---------------------------------------------------------------------------
// Strings and chars
// ---------------------------------------------------------------------------

#[test]
fn simple_string() {
    assert_eq!(
        kinds(r#""Hello, world!""#),
        vec![TokenKind::Str("Hello, world!".to_string()), TokenKind::Eof]
    );
}

#[test]
fn string_with_escape_backslash_is_consumed_raw() {
    // Escape processing happens later; lexer preserves the raw bytes.
    let src = r#""a\"b""#; // source bytes: " a \ " b "
    let ks = kinds(src);
    assert!(matches!(ks[0], TokenKind::Str(ref s) if s == "a\\\"b"));
}

#[test]
fn unterminated_string_is_e0101() {
    let (_, ndiag) = kinds_with_diags(r#""unterminated"#);
    assert_eq!(ndiag, 1);
}

#[test]
fn raw_triple_quoted_string() {
    let src = r#""""raw
content""""#;
    let ks = kinds(src);
    assert!(matches!(ks[0], TokenKind::RawStr(_)));
}

#[test]
fn interpolated_string_lexed_as_single_token() {
    let src = r#"$"hello $name""#;
    let ks = kinds(src);
    assert!(matches!(ks[0], TokenKind::InterpStr(_)));
}

#[test]
fn char_literal() {
    assert_eq!(
        kinds("'a'"),
        vec![TokenKind::Char("a".to_string()), TokenKind::Eof]
    );
}

// ---------------------------------------------------------------------------
// The hello.jux vehicle
// ---------------------------------------------------------------------------

#[test]
fn hello_jux_full_token_stream() {
    let src = "public void main() {\n    print(\"Hello, world!\");\n}\n";
    let ks = kinds(src);
    assert_eq!(
        ks,
        vec![
            TokenKind::Kw(Keyword::Public),
            TokenKind::Kw(Keyword::Void),
            TokenKind::Ident("main".to_string()),
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::Ident("print".to_string()),
            TokenKind::LParen,
            TokenKind::Str("Hello, world!".to_string()),
            TokenKind::RParen,
            TokenKind::Semicolon,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

// ---------------------------------------------------------------------------
// Invalid characters
// ---------------------------------------------------------------------------

#[test]
fn non_ascii_outside_string_is_e0100() {
    let (_, ndiag) = kinds_with_diags("café");
    assert!(ndiag >= 1, "expected an invalid-character diagnostic");
}

#[test]
fn bare_dollar_is_e0100() {
    // `$` outside a `$"..."` is not an identifier character in Jux.
    let (_, ndiag) = kinds_with_diags("$");
    assert_eq!(ndiag, 1);
}
