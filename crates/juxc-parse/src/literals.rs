//! Numeric- and string-literal text parsers.
//!
//! Free helpers used by both `parse_primary` (in `exprs.rs`) and the
//! pattern parser (in `patterns.rs`) to convert lexer-captured digit
//! text into typed AST literals. Pure functions — no parser state.
//!
//! Also hosts [`process_string_escapes`], the parser-time decoder for
//! Jux string escape sequences (`\n`, `\\`, `\u{…}`, …). The lexer
//! captures raw bytes between the quotes verbatim; the parser turns
//! those bytes into real characters by walking escape sequences here.
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

/// Decode a Jux string-literal body (the raw bytes captured by the
/// lexer between the `"` quotes — or between the `$"` quotes for
/// interpolated strings). Returns the unescaped content as a real
/// Rust `String`, plus any human-readable error messages that fired
/// for invalid escape sequences. Callers wrap each error in a
/// `Diagnostic` against the literal's span.
///
/// Per `JUX-GRAMMAR-ADDENDUM.md` §A.1.5 the recognized escapes are:
/// - `\n` `\r` `\t` `\b` `\f` `\0` — control codes
/// - `\\` `\'` `\"` `\$` — literal of the same char (the `\$` form
///   suppresses interpolation in interp strings; in plain strings
///   it's a tolerated alias for a literal `$`)
/// - `\xHH` — single byte, 2 hex digits (must be ≤ `\x7F` per Rust;
///   higher values are rejected because the resulting byte isn't a
///   valid UTF-8 leading code unit on its own)
/// - `\u{H+}` — one Unicode scalar value, 1–6 hex digits in braces
///
/// Anything else after a `\` is reported as an invalid-escape error;
/// the offending sequence is dropped from the output to keep parsing
/// going. EOF mid-escape produces a single "trailing backslash" error
/// and the lone backslash is also dropped.
///
/// **Raw strings** (lexer's triple-quote form) bypass this helper —
/// their grammar explicitly preserves contents verbatim.
pub(crate) fn process_string_escapes(raw: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(raw.len());
    let mut errors: Vec<String> = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            // Plain UTF-8 char — preserve verbatim.
            if let Some(ch) = raw[i..].chars().next() {
                out.push(ch);
                i += ch.len_utf8();
            } else {
                i += 1;
            }
            continue;
        }
        // We're at a backslash. Decode the escape that follows.
        let after = i + 1;
        if after >= bytes.len() {
            errors.push("trailing `\\` in string literal".to_string());
            break;
        }
        match bytes[after] {
            b'n' => { out.push('\n'); i = after + 1; }
            b'r' => { out.push('\r'); i = after + 1; }
            b't' => { out.push('\t'); i = after + 1; }
            b'b' => { out.push('\u{0008}'); i = after + 1; }
            b'f' => { out.push('\u{000C}'); i = after + 1; }
            b'0' => { out.push('\0'); i = after + 1; }
            b'\\' => { out.push('\\'); i = after + 1; }
            b'\'' => { out.push('\''); i = after + 1; }
            b'"' => { out.push('"'); i = after + 1; }
            b'$' => { out.push('$'); i = after + 1; }
            b'x' => {
                // `\xHH` — exactly two hex digits, value ≤ 0x7F.
                let hex_end = after + 3;
                if hex_end > bytes.len() {
                    errors.push("`\\x` requires two hex digits".to_string());
                    break;
                }
                let hex = &raw[after + 1..hex_end];
                if let Ok(n) = u32::from_str_radix(hex, 16) {
                    if n <= 0x7F {
                        if let Some(ch) = char::from_u32(n) {
                            out.push(ch);
                            i = hex_end;
                            continue;
                        }
                    }
                    errors.push(format!("`\\x{hex}` is out of range (must be \\x00–\\x7F)"));
                } else {
                    errors.push(format!("`\\x{hex}` is not two hex digits"));
                }
                i = hex_end;
            }
            b'u' => {
                // `\u{H+}` — Unicode scalar in braces. Up to six hex
                // digits, must encode a valid scalar (no surrogates).
                let brace_open = after + 1;
                if brace_open >= bytes.len() || bytes[brace_open] != b'{' {
                    errors.push("`\\u` must be followed by `{HEX}`".to_string());
                    i = brace_open.min(bytes.len());
                    continue;
                }
                let hex_start = brace_open + 1;
                let mut j = hex_start;
                while j < bytes.len() && bytes[j] != b'}' {
                    j += 1;
                }
                if j >= bytes.len() {
                    errors.push("unterminated `\\u{...}` escape".to_string());
                    break;
                }
                let hex = &raw[hex_start..j];
                let consumed_end = j + 1;
                if hex.is_empty() || hex.len() > 6 {
                    errors.push(format!("`\\u{{{hex}}}` must have 1–6 hex digits"));
                } else if let Ok(n) = u32::from_str_radix(hex, 16) {
                    if let Some(ch) = char::from_u32(n) {
                        out.push(ch);
                    } else {
                        errors.push(format!("`\\u{{{hex}}}` is not a valid Unicode scalar"));
                    }
                } else {
                    errors.push(format!("`\\u{{{hex}}}` is not a hex number"));
                }
                i = consumed_end;
            }
            other => {
                // Unknown escape — preserve the user-written sequence
                // so downstream pretty-printing doesn't silently change
                // appearance, but flag the error so the user knows.
                let display = if let Some(ch) = raw[after..].chars().next() {
                    let mut s = String::from("\\");
                    s.push(ch);
                    s
                } else {
                    format!("\\{}", other as char)
                };
                errors.push(format!("unknown escape sequence `{display}`"));
                if let Some(ch) = raw[after..].chars().next() {
                    out.push('\\');
                    out.push(ch);
                    i = after + ch.len_utf8();
                } else {
                    i = after + 1;
                }
            }
        }
    }
    (out, errors)
}
