//! Numeric-literal text parsers.
//!
//! Free helpers used by both `parse_primary` (in `exprs.rs`) and the
//! pattern parser (in `patterns.rs`) to convert lexer-captured digit
//! text into typed AST literals. Pure functions — no parser state.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original free fns.

use juxc_ast::{FloatKind, FloatLit, IntKind, IntLit, IntRadix};

/// Parse the raw text of an integer literal into an [`IntLit`].
///
/// The lexer hands us the exact source text — including any radix
/// prefix, underscore separators, and type suffix. Here we:
///
/// 1. Identify and strip the radix prefix (`0x`/`0b`/`0o`).
/// 2. Walk forward consuming digit characters of that radix until we
///    hit something that isn't one — that's the suffix start.
/// 3. Classify the suffix into an [`IntKind`].
/// 4. Drop underscores and parse the remainder as `i64`.
///
/// Overflow and out-of-range checks belong in a later phase — this just
/// returns the best-effort value (0 on parse failure).
pub(crate) fn parse_int_literal_text(text: &str) -> IntLit {
    let (radix_u32, radix_enum, body): (u32, IntRadix, &str) =
        if let Some(rest) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            (16, IntRadix::Hex, rest)
        } else if let Some(rest) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
            (2, IntRadix::Binary, rest)
        } else if let Some(rest) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
            (8, IntRadix::Octal, rest)
        } else {
            (10, IntRadix::Decimal, text)
        };

    // Find the boundary between digits and suffix. For hex, this also
    // correctly treats trailing `b`/`B` as a hex digit (since `b` is in
    // `is_digit(16)`), matching the lexer's greedy-hex behavior.
    let digit_byte_end = body
        .char_indices()
        .find(|(_, c)| !(c.is_digit(radix_u32) || *c == '_'))
        .map(|(i, _)| i)
        .unwrap_or(body.len());
    let (digit_str, suffix_str) = body.split_at(digit_byte_end);

    let stripped: String = digit_str.chars().filter(|c| *c != '_').collect();
    let value = i64::from_str_radix(&stripped, radix_u32).unwrap_or(0);
    let kind = parse_int_suffix(suffix_str);
    // digit_width is the count of significant digit characters after
    // underscores are stripped. Used by the backend to preserve leading
    // zeros (`0x0F` stays `0x0F`).
    let digit_width = stripped.chars().count() as u32;
    IntLit { value, kind, radix: radix_enum, digit_width }
}

/// Classify an integer suffix string into an [`IntKind`]. Returns `None`
/// for empty (no suffix → default `int`) or an unknown sequence (the
/// lexer shouldn't produce one, but we tolerate it).
fn parse_int_suffix(s: &str) -> Option<IntKind> {
    match s {
        "" => None,
        "L"  => Some(IntKind::Long),
        "u"  | "U"  => Some(IntKind::UInt),
        "uL" | "UL" | "Ul" | "ul" => Some(IntKind::ULong),
        "b"  | "B"  => Some(IntKind::Byte),
        "ub" | "uB" | "Ub" | "UB" => Some(IntKind::UByte),
        "s"  | "S"  => Some(IntKind::Short),
        "us" | "uS" | "Us" | "US" => Some(IntKind::UShort),
        _ => None,
    }
}

/// Parse the raw text of a float literal into a [`FloatLit`].
///
/// Per §A.1.4 the suffixes are `f` (float, 32-bit) and `d` (double,
/// 64-bit, also the default). We strip and classify the suffix, drop
/// underscores from the remaining body, and parse with `f64::from_str`.
pub(crate) fn parse_float_literal_text(text: &str) -> FloatLit {
    let (body, kind): (&str, Option<FloatKind>) = if let Some(b) = text.strip_suffix(['f', 'F']) {
        (b, Some(FloatKind::Float))
    } else if let Some(b) = text.strip_suffix(['d', 'D']) {
        // Explicit `d` suffix; semantically the same as no suffix (double).
        (b, None)
    } else {
        (text, None)
    };
    let stripped: String = body.chars().filter(|c| *c != '_').collect();
    let value = stripped.parse::<f64>().unwrap_or(0.0);
    FloatLit { value, kind }
}
