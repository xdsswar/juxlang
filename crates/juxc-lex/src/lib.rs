//! Phase 1 — lexer.
//!
//! Tokenizes a [`SourceFile`] into a stream of [`Token`] values. Conforms to
//! the lexical grammar in `JUX-GRAMMAR-ADDENDUM.md` §A.1.
//!
//! ## Design
//!
//! Byte-level scan over the source. The grammar restricts identifiers to
//! ASCII (§A.1.3), so any non-ASCII byte outside a string/char literal is
//! `E0100` (invalid character). Inside string/char literals, multi-byte
//! UTF-8 is preserved verbatim — escape processing and content validation
//! happen in a later phase, not here.
//!
//! Whitespace and comments are **trivia** and are dropped from the emitted
//! token stream. Doc comments will eventually need to survive for `juxc doc`
//! to attach them to declarations; that's deferred until the doc generator
//! lands.
//!
//! The single context-sensitive case in §A.1.6 (splitting `>>` for nested
//! generics) is the parser's job, not ours — we always emit `>>` as a
//! single [`TokenKind::GtGt`]. The parser, when closing a generic-arg list
//! at the right nesting depth, treats `GtGt` as if it were two `Gt`s.
//!
//! ## Error recovery
//!
//! The lexer never fails outright: every byte produces either a token or a
//! diagnostic + advance. The result always reaches EOF so downstream phases
//! get to run and produce additional diagnostics from whatever structure
//! survived. This matches the "no fail-fast" frontend convention used by
//! rustc, clang, javac, etc.

use juxc_diagnostics::{code, Diagnostic};
use juxc_source::{SourceFile, Span};

pub mod token;
pub use token::{Keyword, Token, TokenKind};

/// Output of [`lex`].
///
/// Diagnostics and tokens are returned together; the driver decides whether
/// to short-circuit when diagnostics contain errors or to keep running for
/// more user feedback.
pub struct LexResult {
    /// Token stream, EOF-terminated, in source order.
    pub tokens: Vec<Token>,
    /// Lexical diagnostics (E0100, E0101, E0102) emitted along the way.
    pub diagnostics: Vec<Diagnostic>,
}

/// Lex an entire source file into a stream of tokens plus any diagnostics.
///
/// This is the public entry point for Phase 1. EOF is always the last token,
/// even if there were diagnostics — downstream phases can rely on it.
pub fn lex(source: &SourceFile) -> LexResult {
    let mut lexer = Lexer::new(source.contents());
    lexer.run();
    LexResult { tokens: lexer.tokens, diagnostics: lexer.diagnostics }
}

// ----------------------------------------------------------------------------
// Lexer state
// ----------------------------------------------------------------------------

/// Internal lexer state. Borrowed from the caller for the duration of a
/// single [`lex`] call; not exposed.
struct Lexer<'a> {
    /// The full source as a byte slice. UTF-8, possibly with a leading BOM
    /// that we skip in [`Lexer::new`].
    bytes: &'a [u8],
    /// Current byte offset into `bytes`. Monotonically non-decreasing across
    /// the lexer's lifetime — every step of [`Lexer::run`] advances `pos`
    /// by at least one to guarantee termination.
    pos: usize,
    /// Tokens emitted so far, in source order. EOF is appended last.
    tokens: Vec<Token>,
    /// Diagnostics emitted so far.
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    /// Construct a fresh lexer over `src`. Skips a leading UTF-8 BOM if
    /// present (§A.1.1: `source-file = utf8-bom? token*`).
    fn new(src: &'a str) -> Self {
        let bytes = src.as_bytes();
        let pos = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
        Self { bytes, pos, tokens: Vec::new(), diagnostics: Vec::new() }
    }

    /// Main loop: skip trivia, lex one token, repeat until EOF is emitted.
    fn run(&mut self) {
        loop {
            self.skip_trivia();
            if self.pos >= self.bytes.len() {
                // EOF is always the last token, with a zero-length span at
                // the end of the source. Downstream phases use this as a
                // termination sentinel.
                self.emit(TokenKind::Eof, self.pos, self.pos);
                break;
            }
            self.lex_one();
        }
    }

    /// Dispatch on the byte at `pos` to the right per-kind lexer.
    ///
    /// The order of arms is intentional: cheap single-byte punctuation
    /// first, then multi-char punctuation, then atoms. The `_` arm at the
    /// end catches any byte that isn't a valid Jux start character — those
    /// produce `E0100` and we advance past them to keep making progress.
    fn lex_one(&mut self) {
        let start = self.pos;
        let b = self.bytes[self.pos];
        match b {
            // -------- Single-byte punctuation --------
            b'(' => { self.bump(); self.emit(TokenKind::LParen,   start, self.pos); }
            b')' => { self.bump(); self.emit(TokenKind::RParen,   start, self.pos); }
            b'[' => { self.bump(); self.emit(TokenKind::LBracket, start, self.pos); }
            b']' => { self.bump(); self.emit(TokenKind::RBracket, start, self.pos); }
            b'{' => { self.bump(); self.emit(TokenKind::LBrace,   start, self.pos); }
            b'}' => { self.bump(); self.emit(TokenKind::RBrace,   start, self.pos); }
            b',' => { self.bump(); self.emit(TokenKind::Comma,    start, self.pos); }
            b';' => { self.bump(); self.emit(TokenKind::Semicolon,start, self.pos); }
            b'~' => { self.bump(); self.emit(TokenKind::Tilde,    start, self.pos); }
            b'@' => { self.bump(); self.emit(TokenKind::At,       start, self.pos); }

            // -------- Multi-byte punctuation --------
            // Each helper consumes its lead byte and any continuation, then
            // emits the longest-matching token kind.
            b':' => self.lex_colon(start),
            b'.' => self.lex_dot(start),
            b'?' => self.lex_question(start),
            b'!' => self.lex_bang(start),
            b'=' => self.lex_eq(start),
            b'<' => self.lex_lt(start),
            b'>' => self.lex_gt(start),
            b'+' => self.lex_plus(start),
            b'-' => self.lex_minus(start),
            b'*' => self.lex_star(start),
            // `/` after trivia is either `/=` or bare `/`. The trivia
            // scanner has already eaten `//` and `/*`, so we don't see
            // those forms here.
            b'/' => self.lex_slash(start),
            b'%' => self.lex_percent(start),
            b'&' => self.lex_amp(start),
            b'|' => self.lex_pipe(start),
            b'^' => self.lex_caret(start),

            // -------- Quoted atoms --------
            b'"'  => self.lex_string(start, /*interp=*/false),
            b'\'' => self.lex_char(start),
            // `$` is only legal as the prefix of `$"..."` / `$"""..."""`.
            // Anywhere else (in an identifier, alone, etc.) it's E0100.
            b'$'  => self.lex_dollar(start),

            // -------- Numbers and identifiers --------
            b'0'..=b'9'                       => self.lex_number(start),
            b'A'..=b'Z' | b'a'..=b'z' | b'_'  => self.lex_ident_or_keyword(start),

            // -------- Anything else is invalid --------
            _ => self.error_invalid_char(start, b),
        }
    }

    // ----------------------------------------------------------------------
    // Trivia: whitespace and comments
    // ----------------------------------------------------------------------

    /// Skip any run of whitespace and/or comments. Called before every
    /// token-producing step in the main loop.
    ///
    /// Per §A.1.2:
    /// - Whitespace includes space, tab, CR, LF, FF (0x0C), VT (0x0B).
    /// - `//` introduces a line comment that ends at the next `\n`.
    /// - `/* ... */` introduces a block comment. **Block comments do not
    ///   nest** — the first `*/` terminates them.
    fn skip_trivia(&mut self) {
        loop {
            if self.pos >= self.bytes.len() { return; }
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' | 0x0C /* FF */ | 0x0B /* VT */ => {
                    self.bump();
                }
                b'/' if self.peek1() == Some(b'/') => self.skip_line_comment(),
                b'/' if self.peek1() == Some(b'*') => self.skip_block_comment(),
                _ => return,
            }
        }
    }

    /// Skip a `// …` line comment, including the leading `//` but not the
    /// terminating `\n` (which becomes whitespace and gets skipped next loop).
    fn skip_line_comment(&mut self) {
        self.bump(); self.bump(); // `//`
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
            self.bump();
        }
    }

    /// Skip a `/* … */` block comment.
    ///
    /// Per §A.1.2 block comments do **not** nest, so we don't track depth.
    /// An unterminated block comment falls through to EOF without emitting
    /// a diagnostic — the spec doesn't allocate a code for it. (We may
    /// add one in a follow-up; for now the parser will see an empty body
    /// and complain.)
    fn skip_block_comment(&mut self) {
        self.bump(); self.bump(); // `/*`
        while self.pos < self.bytes.len() {
            if self.bytes[self.pos] == b'*' && self.peek1() == Some(b'/') {
                self.bump(); self.bump(); // `*/`
                return;
            }
            self.bump();
        }
    }

    // ----------------------------------------------------------------------
    // Multi-char punctuation
    //
    // Every helper here is called with the cursor sitting on the leading
    // byte. The helper consumes the lead byte plus any continuation bytes
    // it needs and emits a single token. Order of `match` arms is
    // longest-match first (e.g. `<=>` before `<<`, `<<=` before `<<`).
    // ----------------------------------------------------------------------

    /// `:` or `::`.
    fn lex_colon(&mut self, start: usize) {
        self.bump();
        if self.peek() == Some(b':') {
            self.bump();
            self.emit(TokenKind::ColonColon, start, self.pos);
        } else {
            self.emit(TokenKind::Colon, start, self.pos);
        }
    }

    /// `.`, `..`, `..=`, or `...`.
    fn lex_dot(&mut self, start: usize) {
        self.bump();
        if self.peek() == Some(b'.') {
            self.bump();
            if self.peek() == Some(b'.') {
                // `...` — only legal as a variadic-parameter marker per
                // §A.1.6, but the lexer doesn't enforce that.
                self.bump();
                self.emit(TokenKind::Ellipsis, start, self.pos);
            } else if self.peek() == Some(b'=') {
                self.bump();
                self.emit(TokenKind::DotDotEq, start, self.pos);
            } else {
                self.emit(TokenKind::DotDot, start, self.pos);
            }
        } else {
            self.emit(TokenKind::Dot, start, self.pos);
        }
    }

    /// `?`, `?.`, or `?:`.
    fn lex_question(&mut self, start: usize) {
        self.bump();
        match self.peek() {
            Some(b'.') => { self.bump(); self.emit(TokenKind::QuestionDot,   start, self.pos); }
            Some(b':') => { self.bump(); self.emit(TokenKind::QuestionColon, start, self.pos); }
            _          => self.emit(TokenKind::Question, start, self.pos),
        }
    }

    /// `!`, `!=`, `!==`, or `!!`.
    fn lex_bang(&mut self, start: usize) {
        self.bump();
        match self.peek() {
            Some(b'=') => {
                self.bump();
                if self.peek() == Some(b'=') {
                    self.bump();
                    self.emit(TokenKind::StrictNotEq, start, self.pos);
                } else {
                    self.emit(TokenKind::NotEq, start, self.pos);
                }
            }
            Some(b'!') => { self.bump(); self.emit(TokenKind::BangBang, start, self.pos); }
            _          => self.emit(TokenKind::Bang, start, self.pos),
        }
    }

    /// `=`, `==`, `===`, or `=>`.
    fn lex_eq(&mut self, start: usize) {
        self.bump();
        match self.peek() {
            Some(b'=') => {
                self.bump();
                if self.peek() == Some(b'=') {
                    self.bump();
                    self.emit(TokenKind::StrictEq, start, self.pos);
                } else {
                    self.emit(TokenKind::EqEq, start, self.pos);
                }
            }
            Some(b'>') => { self.bump(); self.emit(TokenKind::FatArrow, start, self.pos); }
            _          => self.emit(TokenKind::Eq, start, self.pos),
        }
    }

    /// `<`, `<=`, `<=>`, `<<`, or `<<=`.
    fn lex_lt(&mut self, start: usize) {
        self.bump();
        match self.peek() {
            Some(b'=') => {
                self.bump();
                if self.peek() == Some(b'>') {
                    self.bump();
                    self.emit(TokenKind::Spaceship, start, self.pos);
                } else {
                    self.emit(TokenKind::Le, start, self.pos);
                }
            }
            Some(b'<') => {
                self.bump();
                if self.peek() == Some(b'=') {
                    self.bump();
                    self.emit(TokenKind::LtLtEq, start, self.pos);
                } else {
                    self.emit(TokenKind::LtLt, start, self.pos);
                }
            }
            _ => self.emit(TokenKind::Lt, start, self.pos),
        }
    }

    /// `>`, `>=`, `>>`, or `>>=`.
    ///
    /// Note: `>>` is always emitted as a single token per §A.1.6. The parser
    /// is responsible for splitting it when it occurs at a position that
    /// closes nested generics (`List<List<T>>`).
    fn lex_gt(&mut self, start: usize) {
        self.bump();
        match self.peek() {
            Some(b'=') => { self.bump(); self.emit(TokenKind::Ge, start, self.pos); }
            Some(b'>') => {
                self.bump();
                if self.peek() == Some(b'=') {
                    self.bump();
                    self.emit(TokenKind::GtGtEq, start, self.pos);
                } else {
                    self.emit(TokenKind::GtGt, start, self.pos);
                }
            }
            _ => self.emit(TokenKind::Gt, start, self.pos),
        }
    }

    /// `+` or `+=`.
    fn lex_plus(&mut self, start: usize) {
        self.bump();
        if self.peek() == Some(b'=') { self.bump(); self.emit(TokenKind::PlusEq, start, self.pos); }
        else                          { self.emit(TokenKind::Plus, start, self.pos); }
    }

    /// `-`, `-=`, or `->`.
    fn lex_minus(&mut self, start: usize) {
        self.bump();
        match self.peek() {
            Some(b'=') => { self.bump(); self.emit(TokenKind::MinusEq, start, self.pos); }
            Some(b'>') => { self.bump(); self.emit(TokenKind::Arrow,   start, self.pos); }
            _          => self.emit(TokenKind::Minus, start, self.pos),
        }
    }

    /// `*` or `*=`.
    fn lex_star(&mut self, start: usize) {
        self.bump();
        if self.peek() == Some(b'=') { self.bump(); self.emit(TokenKind::StarEq, start, self.pos); }
        else                          { self.emit(TokenKind::Star, start, self.pos); }
    }

    /// `/` or `/=`. The forms `//` and `/*` are handled by trivia.
    fn lex_slash(&mut self, start: usize) {
        self.bump();
        if self.peek() == Some(b'=') { self.bump(); self.emit(TokenKind::SlashEq, start, self.pos); }
        else                          { self.emit(TokenKind::Slash, start, self.pos); }
    }

    /// `%` or `%=`.
    fn lex_percent(&mut self, start: usize) {
        self.bump();
        if self.peek() == Some(b'=') { self.bump(); self.emit(TokenKind::PercentEq, start, self.pos); }
        else                          { self.emit(TokenKind::Percent, start, self.pos); }
    }

    /// `&`, `&&`, or `&=`.
    fn lex_amp(&mut self, start: usize) {
        self.bump();
        match self.peek() {
            Some(b'&') => { self.bump(); self.emit(TokenKind::AndAnd, start, self.pos); }
            Some(b'=') => { self.bump(); self.emit(TokenKind::AmpEq,  start, self.pos); }
            _          => self.emit(TokenKind::Amp, start, self.pos),
        }
    }

    /// `|`, `||`, or `|=`.
    fn lex_pipe(&mut self, start: usize) {
        self.bump();
        match self.peek() {
            Some(b'|') => { self.bump(); self.emit(TokenKind::OrOr,   start, self.pos); }
            Some(b'=') => { self.bump(); self.emit(TokenKind::PipeEq, start, self.pos); }
            _          => self.emit(TokenKind::Pipe, start, self.pos),
        }
    }

    /// `^` or `^=`.
    fn lex_caret(&mut self, start: usize) {
        self.bump();
        if self.peek() == Some(b'=') { self.bump(); self.emit(TokenKind::CaretEq, start, self.pos); }
        else                          { self.emit(TokenKind::Caret, start, self.pos); }
    }

    // ----------------------------------------------------------------------
    // Identifiers and keywords
    // ----------------------------------------------------------------------

    /// Consume an ASCII identifier run and classify it as either a keyword,
    /// one of the literal-keyword tokens (`true`/`false`/`null`), or an
    /// ordinary identifier.
    ///
    /// Per §A.1.3, identifiers are pure ASCII: `[A-Za-z_][A-Za-z0-9_]*`.
    /// Non-ASCII letters never become identifiers — they reach
    /// [`Self::error_invalid_char`] and produce `E0100`.
    fn lex_ident_or_keyword(&mut self, start: usize) {
        while let Some(b) = self.peek() {
            if is_ident_continue(b) { self.bump(); } else { break; }
        }
        // Safe to unwrap: the run is ASCII by construction.
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .expect("ident is ASCII by construction")
            .to_string();

        // Keyword classification is case-sensitive. The spec lists all
        // keywords as lowercase, so e.g. `Public` is an identifier.
        let kind = match Keyword::lookup(&text) {
            Some(kw) => TokenKind::Kw(kw),
            None => match text.as_str() {
                // true/false/null aren't keywords per §A.1.3; they're listed
                // under `literal` in §A.2.9. We lex them as distinct token
                // kinds so e.g. `var true = 1` fails in the parser, not by
                // accidentally treating `true` as an identifier.
                "true"  => TokenKind::Bool(true),
                "false" => TokenKind::Bool(false),
                "null"  => TokenKind::Null,
                _       => TokenKind::Ident(text),
            },
        };
        self.emit(kind, start, self.pos);
    }

    // ----------------------------------------------------------------------
    // Numeric literals
    // ----------------------------------------------------------------------

    /// Consume a numeric literal per §A.1.4.
    ///
    /// Supports:
    /// - Decimal (`42`), hex (`0xFF`), binary (`0b1010`), and octal (`0o17`)
    ///   integers, each with optional `_` digit separators.
    /// - Float literals with `.fraction`, `e±exp`, and `f`/`d` suffixes.
    /// - Integer suffixes `L`, `u`, `uL`, `b`, `ub`, `s`, `us`.
    ///
    /// The token text is preserved exactly (underscores included). The
    /// parser/sema is responsible for stripping underscores and
    /// interpreting suffixes — this lets us point at the right column when
    /// reporting overflow/precision errors later.
    fn lex_number(&mut self, start: usize) {
        let mut is_float = false;

        // Detect radix prefix: `0x`, `0b`, `0o` (case-insensitive on the
        // letter). Decimal integers and floats fall through to the
        // `radix_consumed_prefix == false` branch.
        let mut radix_consumed_prefix = false;
        if self.bytes[self.pos] == b'0' && self.peek1().is_some() {
            match self.peek1().unwrap() {
                b'x' | b'X' => { self.bump(); self.bump(); self.scan_digits(is_hex_digit, start); radix_consumed_prefix = true; }
                b'b' | b'B' => { self.bump(); self.bump(); self.scan_digits(is_bin_digit, start); radix_consumed_prefix = true; }
                b'o' | b'O' => { self.bump(); self.bump(); self.scan_digits(is_oct_digit, start); radix_consumed_prefix = true; }
                _ => {}
            }
        }

        if !radix_consumed_prefix {
            // Decimal integer part.
            self.scan_digits(is_dec_digit, start);

            // Fractional part. Per §A.1.4 the fraction requires a digit on
            // *both* sides of the dot, so `42.foo` lexes as `42 . foo`, not
            // as `42.f` (which would be a float anyway, but only because
            // `f` happens to be the float suffix — we don't want that
            // surprise). The `peek1` check enforces "digit after dot".
            if self.peek() == Some(b'.') && self.peek1().map(is_dec_digit).unwrap_or(false) {
                is_float = true;
                self.bump(); // consume `.`
                self.scan_digits(is_dec_digit, start);
            }

            // Exponent part.
            if matches!(self.peek(), Some(b'e' | b'E')) {
                is_float = true;
                self.bump();
                if matches!(self.peek(), Some(b'+' | b'-')) { self.bump(); }
                self.scan_digits(is_dec_digit, start);
            }
        }

        // Suffix dispatch. Per §A.1.4 the legal suffixes are:
        //   int-suffix   = 'L' | 'u' | 'uL' | 'b' | 'ub' | 's' | 'us'
        //   float-suffix = 'f' | 'd'
        if let Some(b) = self.peek() {
            match b {
                b'f' | b'F' | b'd' | b'D' => { is_float = true; self.bump(); }
                b'L' => { self.bump(); }
                b'u' | b'U' => {
                    // `u` may stand alone or chain with one of L/b/B/s/S.
                    self.bump();
                    if matches!(self.peek(), Some(b'L' | b'b' | b'B' | b's' | b'S')) { self.bump(); }
                }
                b'b' | b'B' | b's' | b'S' => { self.bump(); }
                _ => {}
            }
        }

        // The literal's raw text is what we emit. Underscores are preserved
        // so later phases can point at them in diagnostics if needed.
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .expect("number literal is ASCII")
            .to_string();
        let kind = if is_float { TokenKind::Float(text) } else { TokenKind::Int(text) };
        self.emit(kind, start, self.pos);
    }

    /// Scan a run of digits (matched by `pred`) interleaved with `_`
    /// separators. Emits `E0102` if a `_` is in a forbidden position:
    /// leading the run, trailing the run, or if the run is empty (e.g.
    /// `0x` with no hex digits after it).
    ///
    /// `literal_start` is the byte offset of the literal as a whole, used
    /// to anchor the "no digits at all" diagnostic if needed.
    fn scan_digits(&mut self, pred: fn(u8) -> bool, literal_start: usize) {
        let run_start = self.pos;
        let mut saw_digit = false;
        let mut last_was_underscore = false;

        while let Some(b) = self.peek() {
            if pred(b) {
                saw_digit = true;
                last_was_underscore = false;
                self.bump();
            } else if b == b'_' {
                // Leading `_` in the run: invalid per §A.1.4.
                if self.pos == run_start {
                    self.diagnostics.push(Diagnostic::error(
                        code::Code::E0102_BadDigitSeparator,
                        "digit separator `_` cannot lead a digit run",
                    ).with_span(Span::new(self.pos as u32, (self.pos + 1) as u32)));
                }
                last_was_underscore = true;
                self.bump();
            } else {
                break;
            }
        }

        // Trailing `_` is also invalid.
        if last_was_underscore {
            self.diagnostics.push(Diagnostic::error(
                code::Code::E0102_BadDigitSeparator,
                "digit separator `_` cannot trail a digit run",
            ).with_span(Span::new((self.pos - 1) as u32, self.pos as u32)));
        }

        // No digits at all (e.g. `0x` with nothing after).
        if !saw_digit {
            self.diagnostics.push(Diagnostic::error(
                code::Code::E0102_BadDigitSeparator,
                "expected at least one digit after radix prefix",
            ).with_span(Span::new(literal_start as u32, self.pos as u32)));
        }
    }

    // ----------------------------------------------------------------------
    // Strings, chars, and interpolation
    // ----------------------------------------------------------------------

    /// Dispatch on whether a string literal starts with `"""` (raw / triple
    /// quoted) or `"` (simple). `start` points at the leading `"` (the `$`
    /// if any has already been consumed by [`Self::lex_dollar`]).
    fn lex_string(&mut self, start: usize, interp: bool) {
        debug_assert_eq!(self.bytes[self.pos], b'"');
        let triple = self.bytes.get(self.pos + 1) == Some(&b'"')
                  && self.bytes.get(self.pos + 2) == Some(&b'"');
        if triple {
            self.lex_raw_string(start, interp);
        } else {
            self.lex_simple_string(start, interp);
        }
    }

    /// Lex a `"..."` (or `$"..."`) string literal.
    ///
    /// Per §A.1.5: a simple string does not span newlines. Embedded `\n`
    /// is `E0101`. Escape sequences (`\n`, `\\`, `\u{…}`, etc.) are
    /// consumed but not validated — that's a later phase's job. The token
    /// preserves the raw content between (but not including) the quotes.
    fn lex_simple_string(&mut self, start: usize, interp: bool) {
        self.bump(); // opening `"`
        let content_start = self.pos;
        // Brace-depth tracker for interpolation expressions per §3.4.
        // Inside `${ … }`, neither `"` nor `}` should terminate the
        // outer string until the brace stack returns to zero. We only
        // bother tracking when this is an interpolated string; plain
        // strings get the original simple scan.
        let mut interp_brace_depth: u32 = 0;
        loop {
            match self.peek() {
                None => {
                    // EOF inside string — unterminated.
                    self.diagnostics.push(Diagnostic::error(
                        code::Code::E0101_UnterminatedString,
                        "unterminated string literal",
                    ).with_span(Span::new(start as u32, self.pos as u32)));
                    break;
                }
                Some(b'\n') => {
                    // Newline inside non-raw string — also unterminated by
                    // the spec; the raw form `"""..."""` is what permits
                    // line breaks.
                    self.diagnostics.push(Diagnostic::error(
                        code::Code::E0101_UnterminatedString,
                        "newline in non-raw string literal",
                    ).with_span(Span::new(start as u32, self.pos as u32)));
                    break;
                }
                Some(b'\\') => {
                    // Escape: skip the backslash and the byte after it
                    // without validating. A trailing `\` at EOF (no byte
                    // after) is gracefully ignored — the next iteration
                    // hits the `None` arm.
                    self.bump();
                    if self.peek().is_some() { self.bump(); }
                }
                // `${` inside an interp string opens an expression chunk.
                // Increment brace depth so we don't close on its inner `}`.
                Some(b'$') if interp
                    && self.peek1() == Some(b'{')
                    && interp_brace_depth == 0 =>
                {
                    self.bump(); // `$`
                    self.bump(); // `{`
                    interp_brace_depth += 1;
                }
                // Nested `{` inside an already-open `${…}` chunk.
                Some(b'{') if interp && interp_brace_depth > 0 => {
                    self.bump();
                    interp_brace_depth += 1;
                }
                // Matching `}` for a `${…}` chunk.
                Some(b'}') if interp && interp_brace_depth > 0 => {
                    self.bump();
                    interp_brace_depth -= 1;
                }
                // Only a brace-depth-zero `"` ends the string. Inside
                // `${…}`, embedded `"` characters are part of the inner
                // expression's text and don't terminate.
                Some(b'"') if interp_brace_depth == 0 => {
                    let content_end = self.pos;
                    self.bump();
                    let text = String::from_utf8_lossy(&self.bytes[content_start..content_end]).into_owned();
                    let kind = if interp { TokenKind::InterpStr(text) } else { TokenKind::Str(text) };
                    self.emit(kind, start, self.pos);
                    return;
                }
                Some(_) => { self.bump(); }
            }
        }
        // Recovery: we hit EOF or newline. Emit whatever content we
        // accumulated as a (possibly malformed) string token so the parser
        // sees a string-shaped thing rather than a parse error cascade.
        let text = String::from_utf8_lossy(&self.bytes[content_start..self.pos]).into_owned();
        let kind = if interp { TokenKind::InterpStr(text) } else { TokenKind::Str(text) };
        self.emit(kind, start, self.pos);
    }

    /// Lex a `"""..."""` (or `$"""..."""`) raw string literal.
    ///
    /// Per §A.1.5: raw strings preserve their contents verbatim — no escape
    /// processing — and may span any number of lines. They terminate at the
    /// first `"""` after the opening triple-quote.
    fn lex_raw_string(&mut self, start: usize, interp: bool) {
        self.bump(); self.bump(); self.bump(); // three opening quotes
        let content_start = self.pos;
        loop {
            if self.pos >= self.bytes.len() {
                self.diagnostics.push(Diagnostic::error(
                    code::Code::E0101_UnterminatedString,
                    "unterminated raw string literal",
                ).with_span(Span::new(start as u32, self.pos as u32)));
                break;
            }
            // Look for the closing `"""`.
            if self.bytes[self.pos] == b'"'
                && self.bytes.get(self.pos + 1) == Some(&b'"')
                && self.bytes.get(self.pos + 2) == Some(&b'"')
            {
                let content_end = self.pos;
                self.bump(); self.bump(); self.bump();
                let text = String::from_utf8_lossy(&self.bytes[content_start..content_end]).into_owned();
                let kind = if interp { TokenKind::InterpRawStr(text) } else { TokenKind::RawStr(text) };
                self.emit(kind, start, self.pos);
                return;
            }
            self.bump();
        }
        // Recovery path on unterminated.
        let text = String::from_utf8_lossy(&self.bytes[content_start..self.pos]).into_owned();
        let kind = if interp { TokenKind::InterpRawStr(text) } else { TokenKind::RawStr(text) };
        self.emit(kind, start, self.pos);
    }

    /// Lex a `'…'` character literal.
    ///
    /// Per §A.1.5 char literals carry exactly one Unicode code point, but
    /// validating "exactly one" is the parser's job — we just scan for the
    /// closing quote. Escape sequences are consumed without validation.
    fn lex_char(&mut self, start: usize) {
        self.bump(); // opening `'`
        let content_start = self.pos;
        loop {
            match self.peek() {
                None => {
                    self.diagnostics.push(Diagnostic::error(
                        code::Code::E0101_UnterminatedString,
                        "unterminated character literal",
                    ).with_span(Span::new(start as u32, self.pos as u32)));
                    break;
                }
                Some(b'\n') => {
                    self.diagnostics.push(Diagnostic::error(
                        code::Code::E0101_UnterminatedString,
                        "newline in character literal",
                    ).with_span(Span::new(start as u32, self.pos as u32)));
                    break;
                }
                Some(b'\\') => {
                    self.bump();
                    if self.peek().is_some() { self.bump(); }
                }
                Some(b'\'') => {
                    let content_end = self.pos;
                    self.bump();
                    let text = String::from_utf8_lossy(&self.bytes[content_start..content_end]).into_owned();
                    self.emit(TokenKind::Char(text), start, self.pos);
                    return;
                }
                Some(_) => { self.bump(); }
            }
        }
        let text = String::from_utf8_lossy(&self.bytes[content_start..self.pos]).into_owned();
        self.emit(TokenKind::Char(text), start, self.pos);
    }

    /// `$` begins either `$"..."` or `$"""..."""` per §A.1.5.
    ///
    /// Outside those, `$` is not part of any token — it's `E0100`. Jux does
    /// not allow `$` in identifiers (unlike Java).
    fn lex_dollar(&mut self, start: usize) {
        if self.peek1() == Some(b'"') {
            self.bump(); // consume the `$`
            self.lex_string(start, /*interp=*/true);
        } else {
            self.error_invalid_char(start, b'$');
        }
    }

    // ----------------------------------------------------------------------
    // Error production
    // ----------------------------------------------------------------------

    /// Emit `E0100` for an invalid source character and advance past it.
    ///
    /// We step by one UTF-8 character (1–4 bytes) rather than one byte so
    /// that a single non-ASCII codepoint doesn't produce a flurry of
    /// duplicate diagnostics — one per byte. `utf8_char_width` gives us
    /// the right step.
    fn error_invalid_char(&mut self, start: usize, byte: u8) {
        let width = utf8_char_width(byte).max(1);
        for _ in 0..width {
            if self.pos < self.bytes.len() { self.bump(); }
        }
        self.diagnostics.push(Diagnostic::error(
            code::Code::E0100_InvalidCharacter,
            "invalid character in source",
        ).with_span(Span::new(start as u32, self.pos as u32)));
    }

    // ----------------------------------------------------------------------
    // Cursor primitives
    // ----------------------------------------------------------------------

    /// Byte at the cursor, or `None` at EOF.
    #[inline] fn peek(&self) -> Option<u8> { self.bytes.get(self.pos).copied() }

    /// Byte just after the cursor, or `None` if at or past EOF.
    #[inline] fn peek1(&self) -> Option<u8> { self.bytes.get(self.pos + 1).copied() }

    /// Advance one byte. Callers should only call when at least one byte
    /// is available (i.e. after a successful `peek`); otherwise this walks
    /// past EOF (still safe because the next `peek` will return `None`).
    #[inline] fn bump(&mut self) { self.pos += 1; }

    /// Push a token covering `[start, end)`.
    #[inline]
    fn emit(&mut self, kind: TokenKind, start: usize, end: usize) {
        self.tokens.push(Token { kind, span: Span::new(start as u32, end as u32) });
    }
}

// ----------------------------------------------------------------------------
// Character-class helpers
//
// All identifier and number bytes are pure ASCII per §A.1.3 / §A.1.4, so
// `u8`-level predicates are sufficient. UTF-8 multi-byte sequences are
// only encountered inside string/char literals (preserved raw) and
// trigger error_invalid_char anywhere else.
// ----------------------------------------------------------------------------

/// Identifier continuation bytes: ASCII letters, digits, and `_`.
#[inline]
fn is_ident_continue(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
}

/// `0..=9`.
#[inline]
fn is_dec_digit(b: u8) -> bool { matches!(b, b'0'..=b'9') }

/// `0..=9` ∪ `a..=f` ∪ `A..=F`.
#[inline]
fn is_hex_digit(b: u8) -> bool { matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F') }

/// `0` or `1`.
#[inline]
fn is_bin_digit(b: u8) -> bool { matches!(b, b'0' | b'1') }

/// `0..=7`.
#[inline]
fn is_oct_digit(b: u8) -> bool { matches!(b, b'0'..=b'7') }

/// Width (1–4) of the UTF-8 character that starts with `first_byte`.
/// Returns 0 for continuation bytes (which shouldn't appear here in valid
/// input, but we tolerate them by treating them as 1-byte units).
fn utf8_char_width(first_byte: u8) -> usize {
    if first_byte < 0x80      { 1 } // ASCII
    else if first_byte < 0xC0 { 0 } // continuation byte (invalid lead)
    else if first_byte < 0xE0 { 2 } // 2-byte sequence
    else if first_byte < 0xF0 { 3 } // 3-byte sequence
    else                       { 4 } // 4-byte sequence
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests;
