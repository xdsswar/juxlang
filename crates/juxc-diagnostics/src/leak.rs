//! The one Rust-leak detector (GAPS.md gap 33).
//!
//! A Jux programmer never sees Rust: not a Rust type, path, trait, lifetime or
//! borrow, not a rustc or cargo message, not a `.rs` location, not Rust's panic
//! text. The compiler, the language server and the emitted program each have a
//! few places where text leaves them for a person to read. Every one of those
//! exits asks [`find_rust_leak`] (or [`find_rust_leak_quoting`]) about its text
//! before it goes out, and the test suite asks it about every pinned output.
//! Keeping ONE detector is the point: a pattern added here guards every exit
//! at once.
//!
//! ## What counts as Rust
//!
//! Syntax Jux has no spelling for (`std::` paths, `&mut `, `Rc<`, `RefCell`,
//! `Arc<`, `Box<dyn`, lifetimes such as `'a` and `'static` in type position,
//! `impl Trait`), the build tools' own words (`rustc`, `cargo`, `error[E0502]`
//! inside a sentence, a `main.rs` or `file.rs:12` location), the emitted
//! crate's internal names (`__jux_...`, `JuxCell`), and the text of Rust's
//! runtime panics (`thread 'main' panicked`, `RUST_BACKTRACE`, `called
//! Option::unwrap() on a None value`, `index out of bounds: the len is`, the
//! overflow and borrow messages).
//!
//! ## What does not
//!
//! Text the PROGRAMMER wrote. A diagnostic quotes the user's names and code in
//! backticks (`` `cargo` `` when a local is called `cargo`), a program prints
//! its own strings in double quotes, and the `human` format shows source
//! excerpts in `N |` frames. [`find_rust_leak`] skips all three: a quoted span
//! and a frame line are never examined. [`find_rust_leak_quoting`] is given the
//! program's text and examines a quoted span unless the program itself
//! contains it, so a compiler-authored `` `Rc<dyn Trait>` `` is still caught
//! while a user's `` `Rc<Node>` `` (a class of theirs named `Rc`) is not. In
//! both, a hit whose matched text the program contains is not a leak either.
//!
//! Jux's own syntax does not trip it: `Type::method` references, `Map<String,
//! int>`, `Vec<Box<T>>`, char literals such as `'a'`, and the `error[E0410]:`
//! header of the `human` and `compact` formats at the start of a line.

/// Where [`find_rust_leak`] found Rust, and which rule matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeakHit {
    /// What kind of Rust this is, in a few words (`"a Rust std path"`).
    pub rule: &'static str,
    /// Byte offset of the match in the text that was examined.
    pub at: usize,
    /// The matched text.
    pub matched: String,
    /// The line the match is on, trimmed, for a report.
    pub line: String,
}

impl std::fmt::Display for LeakHit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (`{}`) in: {}", self.rule, self.matched, self.line)
    }
}

/// The first Rust-shaped text in `text`, skipping quoted spans (backticks,
/// double quotes, fenced code blocks) and source-frame lines (`  12 | ...`).
///
/// This is the form for text whose quoted parts may be the user's own and
/// nothing is known about the program: pinned outputs, panic messages.
pub fn find_rust_leak(text: &str) -> Option<LeakHit> {
    scan(text, None)
}

/// [`find_rust_leak`], told what the program says: `user_wrote(s)` is true
/// when the program's text contains `s`. A quoted span is examined unless
/// `user_wrote` says the program contains it, and any hit whose matched text
/// the program contains is passed over. This is the form the compiler's own
/// renderers use, since they hold the sources.
pub fn find_rust_leak_quoting(text: &str, user_wrote: &dyn Fn(&str) -> bool) -> Option<LeakHit> {
    scan(text, Some(user_wrote))
}

/// A `user_wrote` predicate over a set of source texts.
pub fn contained_in<'a>(sources: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
    move |s: &str| !s.trim().is_empty() && sources.iter().any(|src| src.contains(s))
}

/// Whether the self-check is on: `JUX_SELFCHECK` set to anything but `0`.
/// Under it a guard that finds a leak panics, so a test sees it; without it
/// the guard replaces the leaking text and the user never does.
pub fn selfcheck() -> bool {
    std::env::var("JUX_SELFCHECK").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Whether a note of `d` is the one place a diagnostic may quote rustc: an
/// `E0900` (rustc rejected the emitted crate, a compiler bug) names rustc's
/// error and where it is in the generated crate, so the bug can be reported
/// (ERRATA E116, E125). Nothing else is exempt.
pub fn is_documented_rustc_note(d: &crate::Diagnostic, note: &str) -> bool {
    d.code == crate::code::Code::E0900_BackendEmittedInvalidRust
        && (note.starts_with("rustc reported ") || note.starts_with("rustc's error is at "))
}

/// The first leak in any text of `d` a person reads: its message, labels,
/// notes (bar [`is_documented_rustc_note`]), help lines and fix title.
pub fn leak_in_diagnostic(d: &crate::Diagnostic, user_wrote: &dyn Fn(&str) -> bool) -> Option<LeakHit> {
    let texts = std::iter::once(d.message.as_str())
        .chain(d.labels.iter().map(|l| l.message.as_str()))
        .chain(d.notes.iter().map(String::as_str).filter(|n| !is_documented_rustc_note(d, n)))
        .chain(d.help.iter().map(String::as_str))
        .chain(d.code_action.iter().map(|a| a.title.as_str()));
    for text in texts {
        if let Some(hit) = find_rust_leak_quoting(text, user_wrote) {
            return Some(hit);
        }
    }
    None
}

/// The guard every diagnostic passes on its way out (a renderer, the language
/// server). `None` when `d` is clean. When it is not:
///
/// - with `panic_on_leak` (the self-check, `JUX_SELFCHECK=1`, and tests) it
///   panics, naming the code and the leak, so the test that produced it fails;
/// - otherwise it returns what the user sees instead: an internal compiler
///   error, `E0900`, at the same place, saying which diagnostic was meant.
///   With `verbose` the original text rides along as a note.
pub fn guard_diagnostic(
    d: &crate::Diagnostic,
    user_wrote: &dyn Fn(&str) -> bool,
    panic_on_leak: bool,
    verbose: bool,
    issues_url: &str,
) -> Option<crate::Diagnostic> {
    let hit = leak_in_diagnostic(d, user_wrote)?;
    if panic_on_leak {
        panic!("diagnostic {} shows Rust to the user: {hit}\nmessage: {}", d.code, d.message);
    }
    let mut ice = crate::Diagnostic::error(
        crate::code::Code::E0900_BackendEmittedInvalidRust,
        "internal compiler error: this diagnostic's text was not written in Jux terms",
    );
    ice.severity = d.severity;
    ice.primary_span = d.primary_span;
    ice.file = d.file;
    ice.notes.push(format!(
        "the compiler meant to report {} here, and its wording named the code the program is compiled to",
        d.code
    ));
    ice.notes.push("this is a bug in the Jux compiler, not in your program".to_string());
    if verbose {
        ice.notes.push(format!("original message: {}", d.message));
        ice.notes.extend(d.notes.iter().map(|n| format!("original note: {n}")));
        ice.notes.extend(d.help.iter().map(|h| format!("original help: {h}")));
    }
    Some(ice.with_help(format!(
        "please report it at {issues_url}, with the source that triggered it; `--verbose` shows the original message"
    )))
}

/// What Rust's runtime said, in Jux words: the text of a Rust panic (from the
/// compiler itself or from a program) rewritten the way a Jux programmer
/// reads it. Text that is not a known Rust panic is returned unchanged.
///
/// The emitted program's prelude carries its own copy of these rules (it is
/// Rust source text and cannot call this crate); `crates/juxc-backend-rust`
/// tests that the two agree.
pub fn jux_panic_wording(message: &str) -> String {
    let m = message.trim();
    if let Some(rest) = m.strip_prefix("index out of bounds: the len is ") {
        // `index out of bounds: the len is 3 but the index is 5`
        if let Some((len, index)) = rest.split_once(" but the index is ") {
            return format!("index {} is out of bounds for length {}", signed_index(index.trim()), len.trim());
        }
    }
    if m.contains("on a `None` value") || m.contains("on a None value") {
        return "a null value was used where a value is required".to_string();
    }
    if m.contains("Result::unwrap()") || m.contains("Result::expect()") {
        return "an operation failed and its error was not handled".to_string();
    }
    for (op, what) in [
        ("add", "addition"),
        ("subtract", "subtraction"),
        ("multiply", "multiplication"),
        ("divide", "division"),
        ("negate", "negation"),
        ("shift left", "left shift"),
        ("shift right", "right shift"),
        ("calculate the remainder", "remainder"),
    ] {
        if m == format!("attempt to {op} with overflow") {
            return format!("integer overflow in {what}");
        }
    }
    if m == "attempt to divide by zero" || m == "attempt to calculate the remainder with a divisor of zero" {
        return "/ by zero".to_string();
    }
    if m == "no entry found for key" {
        return "no entry for the key".to_string();
    }
    if m.starts_with("already borrowed") || m.starts_with("already mutably borrowed") || m.starts_with("RefCell already") {
        return "internal error: an object was already in use. This is a bug in the Jux compiler (ERRATA E23)".to_string();
    }
    m.to_string()
}

/// `18446744073709551615` is how a negative index cast to Rust's `usize`
/// prints; show the index the program computed.
fn signed_index(index: &str) -> String {
    match index.parse::<u64>() {
        Ok(v) if v > i64::MAX as u64 => (v as i64).to_string(),
        _ => index.to_string(),
    }
}

// ---------------------------------------------------------------------------
// The scanner
// ---------------------------------------------------------------------------

/// A piece of the examined text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Prose the compiler (or the runtime) wrote.
    Plain,
    /// A backtick span, a fenced block or a double-quoted string.
    Quoted,
}

fn scan(text: &str, user_wrote: Option<&dyn Fn(&str) -> bool>) -> Option<LeakHit> {
    // Phrases first: they carry backticks of their own (`called
    // `Option::unwrap()` on a `None` value`), so they are matched on the
    // text with backticks taken out, frame lines dropped.
    let unframed = drop_frames(text);
    let plain_text: String = unframed.chars().filter(|c| *c != '`').collect();
    for (phrase, rule) in PHRASES {
        if let Some(i) = plain_text.find(phrase) {
            if user_wrote.is_some_and(|f| f(phrase)) {
                continue;
            }
            return Some(hit(rule, &plain_text, i, phrase));
        }
    }
    for (start, end, kind) in segments(text) {
        let seg = &text[start..end];
        if kind == Kind::Quoted {
            match user_wrote {
                None => continue,
                Some(f) if f(seg) => continue,
                Some(_) => {}
            }
        }
        if let Some((at, len, rule)) = first_structural(text, start, end, user_wrote) {
            return Some(hit(rule, text, at, &text[at..at + len]));
        }
    }
    None
}

fn hit(rule: &'static str, text: &str, at: usize, matched: &str) -> LeakHit {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    LeakHit { rule, at, matched: matched.to_string(), line: text[line_start..line_end].trim().to_string() }
}

/// Runtime and tool phrases, matched anywhere outside source frames.
const PHRASES: &[(&str, &str)] = &[
    ("panicked at", "Rust's panic report"),
    ("thread 'main'", "Rust's panic report"),
    ("has overflowed its stack", "Rust's stack overflow report"),
    ("fatal runtime error", "Rust's runtime abort"),
    ("RUST_BACKTRACE", "Rust's backtrace hint"),
    ("Option::unwrap()", "Rust's unwrap panic"),
    ("Result::unwrap()", "Rust's unwrap panic"),
    ("on a None value", "Rust's unwrap panic"),
    ("on an Err value", "Rust's unwrap panic"),
    ("index out of bounds: the len is", "Rust's index panic"),
    ("already mutably borrowed", "Rust's RefCell panic"),
    ("already borrowed", "Rust's RefCell panic"),
    ("BorrowMutError", "Rust's RefCell panic"),
    ("BorrowError", "Rust's RefCell panic"),
    ("attempt to add with overflow", "Rust's overflow panic"),
    ("attempt to subtract with overflow", "Rust's overflow panic"),
    ("attempt to multiply with overflow", "Rust's overflow panic"),
    ("attempt to divide by zero", "Rust's division panic"),
    ("attempt to negate with overflow", "Rust's overflow panic"),
    ("attempt to shift", "Rust's overflow panic"),
    ("attempt to calculate the remainder", "Rust's division panic"),
    ("no entry found for key", "Rust's map panic"),
    ("error: could not compile", "cargo's build report"),
    ("aborting due to", "rustc's summary line"),
];

/// Split `text` into plain and quoted segments, leaving out source-frame
/// lines (`  12 |     int x = 1;`, `   |     ^^^`) altogether.
fn segments(text: &str) -> Vec<(usize, usize, Kind)> {
    let mut out: Vec<(usize, usize, Kind)> = Vec::new();
    let mut line_start = 0usize;
    for line in text.split_inclusive('\n') {
        let end = line_start + line.len();
        if !is_frame_line(line) {
            split_quotes(text, line_start, end, &mut out);
        }
        line_start = end;
    }
    // A fenced block spans lines: re-mark everything inside ``` fences.
    mark_fences(text, &mut out);
    out
}

/// The same text with source-frame lines removed.
fn drop_frames(text: &str) -> String {
    text.split_inclusive('\n').filter(|l| !is_frame_line(l)).collect()
}

/// `  12 | code`, `   | ^^^`, `   = note:` is not one (its text is ours).
fn is_frame_line(line: &str) -> bool {
    let t = line.trim_start();
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    let rest = t[digits..].trim_start();
    rest.starts_with('|') && (digits > 0 || t.starts_with('|'))
}

/// Split one line into plain and quoted pieces. An unmatched quote leaves the
/// rest of the line plain.
fn split_quotes(text: &str, start: usize, end: usize, out: &mut Vec<(usize, usize, Kind)>) {
    let bytes = text.as_bytes();
    let mut plain_from = start;
    let mut i = start;
    while i < end {
        let b = bytes[i];
        if b == b'`' || b == b'"' {
            if let Some(close) = text[i + 1..end].find(b as char).map(|j| i + 1 + j) {
                if plain_from < i {
                    out.push((plain_from, i, Kind::Plain));
                }
                out.push((i + 1, close, Kind::Quoted));
                i = close + 1;
                plain_from = i;
                continue;
            }
        }
        i += 1;
    }
    if plain_from < end {
        out.push((plain_from, end, Kind::Plain));
    }
}

/// Mark everything between a pair of ``` fences as one quoted segment.
fn mark_fences(text: &str, out: &mut Vec<(usize, usize, Kind)>) {
    let mut fences: Vec<(usize, usize)> = Vec::new();
    let mut from = 0usize;
    while let Some(open) = text[from..].find("```").map(|i| from + i) {
        let body = open + 3;
        match text[body..].find("```").map(|i| body + i) {
            Some(close) => {
                fences.push((body, close));
                from = close + 3;
            }
            None => break,
        }
    }
    if fences.is_empty() {
        return;
    }
    out.retain(|(s, e, _)| !fences.iter().any(|(fs, fe)| *s >= fs.saturating_sub(3) && *e <= fe + 3));
    for (s, e) in fences {
        // The info string (`jux`) on the opening line is not code.
        let code_start = text[s..e].find('\n').map_or(s, |i| s + i + 1);
        out.push((code_start, e, Kind::Quoted));
    }
    out.sort_by_key(|(s, _, _)| *s);
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The character before byte offset `at`, if any.
fn prev_char(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

/// Structural rules, tried at every position of `text[start..end]`. Returns
/// `(offset, length, rule)` of the first hit the program did not write.
fn first_structural(
    text: &str,
    start: usize,
    end: usize,
    user_wrote: Option<&dyn Fn(&str) -> bool>,
) -> Option<(usize, usize, &'static str)> {
    let seg = &text[start..end];
    let mut found: Option<(usize, usize, &'static str)> = None;
    for (i, _) in seg.char_indices() {
        let at = start + i;
        if let Some((len, rule)) = structural_at(text, at, end) {
            let matched = &text[at..at + len];
            let word: String = matched.chars().filter(|c| is_ident(*c)).collect();
            let theirs = user_wrote.is_some_and(|f| f(matched) || (word.len() > 2 && f(&word)));
            if !theirs {
                found = Some((at, len, rule));
                break;
            }
        }
    }
    found
}

/// One structural rule matching at `at`: `(length, rule)`.
fn structural_at(text: &str, at: usize, end: usize) -> Option<(usize, &'static str)> {
    let rest = &text[at..end];
    let prev = prev_char(text, at);
    let word_start = !prev.is_some_and(|c| is_ident(c) || c == '.' || c == ':');
    // Rust std paths.
    if word_start {
        for p in ["std::", "core::", "alloc::"] {
            if rest.starts_with(p) {
                return Some((p.len(), "a Rust std path"));
            }
        }
    }
    if rest.starts_with("&mut ") || rest.starts_with("&mut\n") {
        return Some((4, "a Rust mutable borrow"));
    }
    if word_start {
        for (p, rule) in [
            ("Rc<", "Rust's Rc"),
            ("Arc<", "Rust's Arc"),
            ("Box<dyn", "a Rust trait object"),
            ("RefCell", "Rust's RefCell"),
            ("JuxCell", "the emitted crate's cell"),
            ("Cargo.toml", "cargo's manifest"),
        ] {
            if rest.starts_with(p) {
                return Some((p.len(), rule));
            }
        }
        if rest.starts_with("__jux") {
            return Some((5, "a name from the emitted crate"));
        }
        for (w, rule) in [("cargo", "cargo"), ("rustc", "rustc")] {
            if rest.starts_with(w) && !rest[w.len()..].chars().next().is_some_and(is_ident) {
                return Some((w.len(), rule));
            }
        }
        // `dyn Trait` and `impl Trait`.
        for kw in ["dyn ", "impl "] {
            if let Some(after) = rest.strip_prefix(kw) {
                if after.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    return Some((kw.len() + 1, "a Rust trait type"));
                }
            }
        }
    }
    // Lifetimes: `'static`, and `'a` right after `&` or `<`, or before `>`.
    if rest.starts_with('\'') {
        let name: String = rest[1..].chars().take_while(|c| is_ident(*c)).collect();
        let after = rest[1 + name.len()..].chars().next();
        let char_literal = after == Some('\'');
        let lifetime_start = name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_');
        if !name.is_empty() && lifetime_start && !char_literal {
            if name == "static" && !prev.is_some_and(is_ident) {
                return Some((1 + name.len(), "a Rust lifetime"));
            }
            let in_type = matches!(prev, Some('&') | Some('<'))
                || (matches!(after, Some('>') | Some(',')) && matches!(prev, Some(' ') | Some('<')));
            if in_type {
                return Some((1 + name.len(), "a Rust lifetime"));
            }
        }
    }
    // rustc's `error[E0502]` inside a sentence. The `human` and `compact`
    // formats open a line with `error[E0410]:`, which is Jux's own header.
    if rest.starts_with("error[") {
        let line_head = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let before = text[line_head..at].trim_end();
        let header = before.is_empty() || (before.ends_with(':') && is_location(before.trim_end_matches(':')));
        if !header {
            return Some((6, "a rustc error code"));
        }
    }
    // A Rust source location: `main.rs`, `x.rs:12`.
    if rest.starts_with(".rs") && !rest[3..].chars().next().is_some_and(is_ident) {
        let file_before = prev.is_some_and(is_ident);
        let line_after = rest[3..].starts_with(':') && rest[4..].chars().next().is_some_and(|c| c.is_ascii_digit());
        let stem: String = text[..at].chars().rev().take_while(|c| is_ident(*c)).collect::<Vec<_>>().into_iter().rev().collect();
        if file_before && (line_after || matches!(stem.as_str(), "main" | "lib" | "mod" | "build")) {
            return Some((3, "a Rust source location"));
        }
    }
    None
}

/// `path/to/file.jux:12:5`: what the one-line formats put before a header.
fn is_location(s: &str) -> bool {
    let mut parts = s.rsplitn(3, ':');
    let col = parts.next().unwrap_or("");
    let line = parts.next().unwrap_or("");
    !col.is_empty() && col.chars().all(|c| c.is_ascii_digit()) && !line.is_empty() && line.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaks(text: &str) -> bool {
        find_rust_leak(text).is_some()
    }

    #[test]
    fn rust_paths_and_types_are_found() {
        assert!(leaks("expected std::string::String, found int"));
        assert!(leaks("in core::option::Option<i64>"));
        assert!(leaks("an alloc::vec::Vec"));
        assert!(leaks("cannot borrow as &mut self"));
        assert!(leaks("they need an Rc<dyn Trait>"));
        assert!(leaks("wrapped in RefCell { value: 3 }"));
        assert!(leaks("an Arc<Mutex<T>> handle"));
        assert!(leaks("a Box<dyn Fn()> value"));
        assert!(leaks("takes impl Into<WidgetText>"));
        assert!(leaks("a dyn Widget parameter"));
        assert!(leaks("borrowed for 'static"));
        assert!(leaks("a &'a str slice"));
        assert!(leaks("Ref<'_, T>"));
        assert!(leaks("the helper __jux_idiv failed"));
    }

    #[test]
    fn build_tool_text_is_found() {
        assert!(leaks("rustc reported something"));
        // Quoted, so only the form that knows the program looks inside.
        assert!(!leaks("`cargo build` failed"));
        assert!(find_rust_leak_quoting("`cargo build` failed", &|_| false).is_some());
        assert!(leaks("cargo build failed"));
        assert!(leaks("note: error[E0502]: cannot borrow"));
        assert!(leaks("at src/main.rs:12:5"));
        assert!(leaks("at src\\main.rs"));
        assert!(leaks("in widget.rs:40:1 of the crate"));
        assert!(leaks("see Cargo.toml"));
        assert!(leaks("error: could not compile `app` (bin \"app\")"));
    }

    #[test]
    fn rust_panic_text_is_found() {
        assert!(leaks("thread 'main' panicked at src\\main.rs:3:5:"));
        assert!(leaks("note: run with `RUST_BACKTRACE=1` environment variable"));
        assert!(leaks("called `Option::unwrap()` on a `None` value"));
        assert!(leaks("panic: index out of bounds: the len is 3 but the index is 5"));
        assert!(leaks("panic: attempt to add with overflow"));
        assert!(leaks("already borrowed: BorrowMutError"));
        assert!(leaks("thread 'main' (1234) has overflowed its stack"));
        assert!(leaks("panic: no entry found for key"));
    }

    #[test]
    fn jux_text_is_not_a_leak() {
        assert!(!leaks("type mismatch: expected Map<String, int>, found Vec<int>"));
        assert!(!leaks("a Vec<Box<T>> of boxes"));
        assert!(!leaks("method reference String::length"));
        assert!(!leaks("the char 'a' and the char 'z'"));
        assert!(!leaks("it's the programmer's own text, isn't it"));
        assert!(!leaks("Exception in thread \"main\" jux.std.exceptions.ArithmeticException: / by zero"));
        assert!(!leaks("app.jux:3:15: [E0410] error: type mismatch"));
        assert!(!leaks("error[E0410]: type mismatch: expected int, found String"));
        assert!(!leaks("app.jux:3:15: error[E0410]: type mismatch"));
        assert!(!leaks("see `juxc explain E0410` for more"));
        assert!(!leaks("a HashMap<String, List<int?>>"));
        assert!(!leaks("impl blocks are written as classes"));
        assert!(!leaks("files ending in .rs are not Jux sources"));
    }

    #[test]
    fn quoted_user_text_is_skipped() {
        // A local the programmer named `cargo`, a string they printed.
        assert!(!leaks("cannot find `cargo` in this scope"));
        assert!(!leaks("printed \"thread main is fine\" and \"std::x\""));
        assert!(!leaks("unused variable `rustc`"));
        // Source frames are the program's own text.
        assert!(!leaks("  3 |     var cargo = std::x;\n    |         ^^^^^"));
    }

    #[test]
    fn the_quoting_form_looks_inside_quotes_the_program_did_not_write() {
        let program = "class Rc<T> { } public void main() { int count = 1; }";
        let wrote = |s: &str| program.contains(s);
        // Compiler-authored code in backticks is examined.
        assert!(find_rust_leak_quoting("needs `Rc<dyn Trait>`", &wrote).is_some());
        assert!(find_rust_leak_quoting("run `cargo build` again", &wrote).is_some());
        // The programmer's own names are not.
        let named = |s: &str| "int cargo = 1;".contains(s);
        assert!(find_rust_leak_quoting("cannot find `cargo`", &named).is_none());
        assert!(find_rust_leak_quoting("unused variable cargo", &named).is_none());
        assert!(find_rust_leak_quoting("expected `Rc<int>`, found `int`", &wrote).is_none());
        // Plain text naming the program's own type is not either.
        assert!(find_rust_leak_quoting("the class Rc<T> is generic", &wrote).is_none());
    }

    #[test]
    fn fenced_blocks_are_quoted() {
        let hover = "```jux\npublic void push(T value)\n```\n\nAdds a value.";
        assert!(!leaks(hover));
        let bad = "```jux\nfn push(&mut self)\n```";
        assert!(find_rust_leak_quoting(bad, &|_| false).is_some());
        assert!(!leaks(bad));
    }

    #[test]
    fn a_hit_says_where() {
        let h = find_rust_leak("line one\nexpected std::string::String here").unwrap();
        assert_eq!(h.matched, "std::");
        assert_eq!(h.line, "expected std::string::String here");
        assert_eq!(h.rule, "a Rust std path");
    }

    #[test]
    fn a_leaking_diagnostic_becomes_an_ice_and_the_rustc_note_is_exempt() {
        use crate::code::Code;
        use crate::Diagnostic;
        let none = |_: &str| false;
        let clean = Diagnostic::error(Code::E0410_TypeMismatch, "type mismatch: expected int, found String");
        assert!(guard_diagnostic(&clean, &none, false, false, "u").is_none());

        let mut e0900 = Diagnostic::error(Code::E0900_BackendEmittedInvalidRust, "internal compiler error: x");
        e0900.notes.push("rustc reported error[E0599]: no method named `borrow` found".to_string());
        e0900.notes.push("rustc's error is at src/main.rs:12:5 of the generated crate".to_string());
        assert!(guard_diagnostic(&e0900, &none, false, false, "u").is_none());
        // The exemption is E0900's alone.
        let mut other = clean.clone();
        other.notes.push("rustc reported error[E0599]".to_string());
        assert!(guard_diagnostic(&other, &none, false, false, "u").is_some());

        let leaky = Diagnostic::error(Code::E0436_InterfaceOnExceptionClass, "they need `Rc<dyn Trait>`")
            .with_help("try again");
        let ice = guard_diagnostic(&leaky, &none, false, false, "https://x/issues").expect("an ICE");
        assert_eq!(ice.code, Code::E0900_BackendEmittedInvalidRust);
        assert!(ice.notes[0].contains("E0436"), "{:?}", ice.notes);
        assert!(leak_in_diagnostic(&ice, &none).is_none());
        assert!(!format!("{:?}{:?}", ice.message, ice.notes).contains("Rc<"));
        let verbose = guard_diagnostic(&leaky, &none, false, true, "u").expect("an ICE");
        assert!(verbose.notes.iter().any(|n| n.contains("Rc<dyn Trait>")));
    }

    #[test]
    #[should_panic(expected = "shows Rust to the user")]
    fn the_self_check_panics_on_a_leak() {
        use crate::code::Code;
        let leaky = crate::Diagnostic::error(Code::E0410_TypeMismatch, "expected std::string::String");
        let _ = guard_diagnostic(&leaky, &|_| false, true, false, "u");
    }

    #[test]
    fn panic_wording_is_jux() {
        assert_eq!(
            jux_panic_wording("index out of bounds: the len is 3 but the index is 5"),
            "index 5 is out of bounds for length 3"
        );
        assert_eq!(
            jux_panic_wording("index out of bounds: the len is 1 but the index is 18446744073709551615"),
            "index -1 is out of bounds for length 1"
        );
        assert_eq!(jux_panic_wording("attempt to add with overflow"), "integer overflow in addition");
        assert_eq!(jux_panic_wording("attempt to divide by zero"), "/ by zero");
        assert_eq!(
            jux_panic_wording("called `Option::unwrap()` on a `None` value"),
            "a null value was used where a value is required"
        );
        assert_eq!(jux_panic_wording("no entry found for key"), "no entry for the key");
        assert_eq!(jux_panic_wording("numbers differ"), "numbers differ");
        for m in [
            "index out of bounds: the len is 3 but the index is 5",
            "attempt to multiply with overflow",
            "called `Result::unwrap()` on an `Err` value: ParseIntError { kind: InvalidDigit }",
            "already borrowed: BorrowMutError",
            "no entry found for key",
        ] {
            assert!(!leaks(&jux_panic_wording(m)), "{m}");
        }
    }
}
