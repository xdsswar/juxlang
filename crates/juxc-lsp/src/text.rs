//! Byte-level scans over source text shared by several handlers: the word
//! under the cursor, the receiver before a `.`, a declaration's doc comment.
//!
//! These read raw text on purpose. They run on a buffer mid-edit, which often
//! does not parse, and they only need the shape of the characters around the
//! cursor. Anything that needs meaning goes through the analysis instead.

/// An identifier found in the source text, with its byte range.
pub(crate) struct Word {
    /// The identifier text.
    pub(crate) text: String,
    /// Inclusive start byte offset.
    pub(crate) start: usize,
    /// Exclusive end byte offset.
    pub(crate) end: usize,
}

/// True for a byte that can appear in a Jux identifier.
pub(crate) fn is_ident_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

/// Extract the identifier whose span contains `offset` (or that ends exactly at
/// `offset`, so a cursor parked just after a name still resolves it). Returns
/// `None` when `offset` isn't inside / adjacent to an identifier. ASCII-only
/// boundary scan — adequate for Jux identifiers, which are ASCII.
pub(crate) fn word_at(text: &str, offset: usize) -> Option<Word> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    if offset > len {
        return None;
    }
    // The cursor may sit just past the identifier's last byte; step back one
    // when the byte at `offset` isn't an identifier byte but the previous is.
    let probe = if offset < len && is_ident_byte(bytes[offset]) {
        offset
    } else if offset > 0 && is_ident_byte(bytes[offset - 1]) {
        offset - 1
    } else {
        return None;
    };
    let mut start = probe;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = probe;
    while end < len && is_ident_byte(bytes[end]) {
        end += 1;
    }
    // An identifier can't start with a digit — if it does, the cursor is on a
    // numeric literal, not a name.
    if bytes[start].is_ascii_digit() {
        return None;
    }
    Some(Word { text: text[start..end].to_string(), start, end })
}

/// The start offset of the identifier run that ends at `offset` (the partial
/// word the user is typing). Returns `offset` unchanged when the byte before
/// the cursor isn't an identifier byte (e.g. the cursor is right after a `.`).
pub(crate) fn ident_start_before(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut start = offset.min(bytes.len());
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

/// True when the identifier starting at `ident_start` sits in the **type
/// position of a `new <Type>` expression** — the nearest word before it
/// (skipping whitespace) is the `new` keyword. In that position completion
/// offers only type names, inserted bare, so `new X` stays `X` rather than the
/// spurious `X()` IntelliJ appends to function-shaped items.
pub(crate) fn is_new_context(text: &str, ident_start: usize) -> bool {
    let bytes = text.as_bytes();
    let mut end = ident_start.min(bytes.len());
    while end > 0 && (bytes[end - 1] as char).is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    &text[start..end] == "new"
}

/// Replace `// …` (to end of line) and `/* … */` comment spans in `src` with
/// spaces, so backward keyword scans can't read keywords out of comments.
/// Length-preserving is not required by the callers, but blanking (instead of
/// deleting) keeps any future offset use safe.
pub(crate) fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        match (chars[i], chars.get(i + 1)) {
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    out.push(' ');
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                out.push(' ');
                out.push(' ');
                i += 2;
                while i < chars.len() {
                    if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        out.push(' ');
                        out.push(' ');
                        i += 2;
                        break;
                    }
                    out.push(' ');
                    i += 1;
                }
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// If the identifier starting at `ident_start` is the member half of a
/// `receiver.member` access, return the byte offset of the receiver's last
/// byte (i.e. the `.`'s offset) so the receiver expression's span — which ends
/// there — can be looked up. Skips ASCII whitespace between the `.` and the
/// identifier. Returns `None` when there's no preceding `.` (a plain name) or
/// the `.` is part of a number / float.
pub(crate) fn receiver_dot_before(text: &str, ident_start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = ident_start;
    while i > 0 && (bytes[i - 1] as char).is_ascii_whitespace() {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] != b'.' {
        return None;
    }
    let dot = i - 1;
    // `1.0` style float: a `.` preceded by a digit isn't a member access.
    if dot > 0 && bytes[dot - 1].is_ascii_digit() {
        return None;
    }
    // The receiver expression's span ends at the `.`'s offset (exclusive end).
    Some(dot)
}

/// Extract the first line of a `///` or `/** … */` doc comment immediately
/// preceding the declaration whose name starts at `name_start`.
///
/// The AST doesn't carry doc comments, so we recover them from source: scan the
/// line(s) above the declaration. We walk backwards over blank lines and the
/// modifier/keyword run on the declaration's own line, then read a contiguous
/// run of `///` lines (or a single `/** … */`). Returns the first non-empty doc
/// line, trimmed, or `None` when there's no doc comment.
pub(crate) fn doc_comment_before(text: &str, name_start: usize) -> Option<String> {
    // A cached span can be stale against the on-disk file (external edit,
    // regenerated stub) — never slice out of bounds or mid-char for it.
    if name_start > text.len() || !text.is_char_boundary(name_start) {
        return None;
    }
    // Find the start of the line the name sits on.
    let line_start = text[..name_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
    // Walk upward, collecting `///` lines, until a non-doc line.
    let mut cursor = line_start;
    let mut doc_lines: Vec<String> = Vec::new();
    while cursor > 0 {
        // Previous line's range [prev_start, cursor-1) (cursor-1 is its '\n').
        let prev_nl = text[..cursor - 1].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line = text[prev_nl..cursor - 1].trim();
        if let Some(rest) = line.strip_prefix("///") {
            doc_lines.push(rest.trim().to_string());
            cursor = prev_nl;
            continue;
        }
        // A single-line block doc `/** text */`.
        if line.starts_with("/**") && line.ends_with("*/") {
            let inner = line
                .trim_start_matches("/**")
                .trim_end_matches("*/")
                .trim()
                .to_string();
            doc_lines.push(inner);
        }
        break;
    }
    // `doc_lines` is bottom-up; the first source line is last.
    doc_lines.into_iter().rev().find(|l| !l.is_empty())
}
