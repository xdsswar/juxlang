//! Token types — the lexer's output alphabet.
//!
//! Conforms to the lexical grammar in `JUX-GRAMMAR-ADDENDUM.md` §A.1.
//!
//! ## Design choices
//!
//! - **Keywords carry their own enum** ([`Keyword`]) rather than inflating
//!   [`TokenKind`] with 51 lookalike variants. Saves boilerplate at the
//!   parser's match sites.
//! - **`true`, `false`, `null` are NOT keywords** per §A.1.3 — they're
//!   listed as `literal` in §A.2.9. We give them dedicated `TokenKind`
//!   variants so `var true = 1;` is a parse error (variable named `true`
//!   is impossible), not an accidental assignment.
//! - **String/number variants carry the raw source text**, not a cooked
//!   value. Escape processing, underscore stripping, and suffix
//!   interpretation happen in later phases — keeping them out of the
//!   lexer means we can point at the exact source bytes when diagnosing
//!   overflow, invalid escape, etc.

use juxc_source::Span;

/// A single lexical token, with the source span it came from.
///
/// Tokens are owned (the lexer copies identifier and literal text into
/// `String`s). This is a deliberate trade: cheaper memory model for the
/// parser at the cost of one allocation per atom token. Once the parser
/// is hot enough to care, we can move to interned `Symbol`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// What kind of token this is.
    pub kind: TokenKind,
    /// Byte range in the source file. `[span.start, span.end)`.
    pub span: Span,
}

/// What kind of token. Atom variants carry their raw source text — escape
/// processing for strings and underscore-stripping for numbers happens in
/// later phases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // ============================================================
    // Atoms
    // ============================================================
    /// A non-keyword identifier. ASCII letters/digits/`_`, not digit-leading.
    Ident(String),
    /// Reserved keyword. See [`Keyword`].
    Kw(Keyword),

    /// Integer literal. Includes radix prefix (if any), digits, separators,
    /// and suffix — exactly as written in source.
    Int(String),
    /// Float literal. Includes digits, fractional part, exponent, and
    /// suffix — exactly as written.
    Float(String),
    /// `'…'` character literal. Holds the bytes *between* the quotes,
    /// escapes preserved verbatim (e.g. `\n` is stored as two bytes
    /// `\\` `n`, not as a U+000A code point).
    Char(String),
    /// `"…"` string literal. Holds the bytes between the quotes verbatim.
    Str(String),
    /// `"""…"""` raw string literal. Holds the bytes between the
    /// triple-quotes; no escape processing.
    RawStr(String),
    /// `$"…"` interpolated string. Holds the raw bytes between the quotes;
    /// the `${…}` segments inside are parsed in a later phase.
    InterpStr(String),
    /// `$"""…"""` interpolated raw string.
    InterpRawStr(String),

    /// `true` / `false`. Per §A.2.9 these are `literal`, not keywords.
    Bool(bool),
    /// `null`. Per §A.2.9 this is a `literal`, not a keyword.
    Null,

    // ============================================================
    // Punctuation — per §A.1.6
    // ============================================================
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `;`
    Semicolon,
    /// `:`
    Colon,
    /// `::` — path separator (`std::io`, `Box::new`).
    ColonColon,
    /// `.` — member access.
    Dot,
    /// `..` — exclusive range.
    DotDot,
    /// `..=` — inclusive range.
    DotDotEq,
    /// `...` — variadic-parameter marker (only legal in parameter lists).
    Ellipsis,
    /// `?` — error propagation; nullable type marker.
    Question,
    /// `?.` — safe-navigation member access.
    QuestionDot,
    /// `?:` — Elvis / null-coalescing operator.
    QuestionColon,
    /// `??` — null-coalescing alias for `?:` (C#/JavaScript-style
    /// spelling). The parser treats this identically to
    /// [`Self::QuestionColon`]; the two are kept as distinct tokens
    /// so source-mapped diagnostics can report the spelling the
    /// user actually typed. Per `JUX-GRAMMAR-ADDENDUM.md` §A.1.6.
    QuestionQuestion,
    /// `!!` — non-null assertion.
    BangBang,
    /// `@` — annotation prefix.
    At,
    /// `->` — function-type return arrow.
    Arrow,
    /// `=>` — type-test operator ("is an instance of"); also lambda body sep.
    FatArrow,

    // ============================================================
    // Operators
    // ============================================================
    /// `=` — assignment.
    Eq,
    /// `==` — structural equality.
    EqEq,
    /// `===` — reference identity.
    StrictEq,
    /// `!` — logical NOT.
    Bang,
    /// `!=` — structural inequality.
    NotEq,
    /// `!==` — reference non-identity.
    StrictNotEq,
    /// `<` — less-than (also opens generic-arg list; parser disambiguates).
    Lt,
    /// `<=` — less-or-equal.
    Le,
    /// `>` — greater-than.
    Gt,
    /// `>=` — greater-or-equal.
    Ge,
    /// `<=>` — three-way comparison.
    Spaceship,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*` — multiplication; also pointer dereference in unsafe contexts.
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `+=`
    PlusEq,
    /// `-=`
    MinusEq,
    /// `*=`
    StarEq,
    /// `/=`
    SlashEq,
    /// `%=`
    PercentEq,
    /// `&&` — short-circuit logical AND.
    AndAnd,
    /// `||` — short-circuit logical OR.
    OrOr,
    /// `&` — bitwise AND; also address-of in unsafe contexts.
    Amp,
    /// `|` — bitwise OR; also or-pattern separator.
    Pipe,
    /// `^` — bitwise XOR.
    Caret,
    /// `~` — bitwise NOT.
    Tilde,
    /// `&=`
    AmpEq,
    /// `|=`
    PipeEq,
    /// `^=`
    CaretEq,
    /// `<<` — left shift.
    LtLt,
    /// `>>` — right shift. Parser splits into two `Gt` when closing nested
    /// generic-arg lists per §A.1.6.
    GtGt,
    /// `<<=`
    LtLtEq,
    /// `>>=`
    GtGtEq,

    /// End-of-file sentinel; always the final token. Has a zero-length
    /// span pointing at the EOF position so diagnostics that need to
    /// say "missing X at end of file" have somewhere to point.
    Eof,
}

/// Reserved keywords — listed in `JUX-GRAMMAR-ADDENDUM.md` §A.1.3 and
/// `JUX-LANG-V1.md` §3.2. The two lists are kept in sync.
///
/// Variants are ordered alphabetically to make the lookup table and the
/// `as_str` matcher trivially verifiable against the spec list. Adding a
/// new keyword means: (1) update the §3.2 list in the main spec, (2) update
/// the §A.1.3 grammar production, (3) add a variant here, (4) add an arm
/// in [`Keyword::as_str`] and [`Keyword::lookup`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keyword {
    Abstract,
    Annotation,
    As,
    Async,
    Await,
    Break,
    Case,
    Catch,
    Class,
    Const,
    Continue,
    Default,
    Do,
    Drop,
    Else,
    Enum,
    Extends,
    Final,
    Finally,
    For,
    If,
    Implements,
    Import,
    Init,
    Interface,
    Internal,
    Move,
    Native,
    New,
    /// `operator` — declares an operator overload on a class/record/enum.
    /// Per `JUX-OPERATORS-ADDENDUM.md` §O.2.
    Operator,
    Package,
    Permits,
    Private,
    Protected,
    Public,
    Record,
    Return,
    Sealed,
    Sizeof,
    Static,
    Struct,
    Super,
    Switch,
    This,
    Throw,
    Throws,
    Try,
    Type,
    Unsafe,
    Var,
    Void,
    Volatile,
    When,
    While,
    Yield,
}

impl Keyword {
    /// Every keyword variant, in the same alphabetical order as the enum.
    ///
    /// Used by the grammar-spec emitter (`grammar_spec`) to enumerate the
    /// reserved-word set without reflection, and by its completeness test to
    /// catch a variant that was added to the enum but forgotten here.
    pub const ALL: &'static [Keyword] = &[
        Keyword::Abstract,
        Keyword::Annotation,
        Keyword::As,
        Keyword::Async,
        Keyword::Await,
        Keyword::Break,
        Keyword::Case,
        Keyword::Catch,
        Keyword::Class,
        Keyword::Const,
        Keyword::Continue,
        Keyword::Default,
        Keyword::Do,
        Keyword::Drop,
        Keyword::Else,
        Keyword::Enum,
        Keyword::Extends,
        Keyword::Final,
        Keyword::Finally,
        Keyword::For,
        Keyword::If,
        Keyword::Implements,
        Keyword::Import,
        Keyword::Init,
        Keyword::Interface,
        Keyword::Internal,
        Keyword::Move,
        Keyword::Native,
        Keyword::New,
        Keyword::Operator,
        Keyword::Package,
        Keyword::Permits,
        Keyword::Private,
        Keyword::Protected,
        Keyword::Public,
        Keyword::Record,
        Keyword::Return,
        Keyword::Sealed,
        Keyword::Sizeof,
        Keyword::Static,
        Keyword::Struct,
        Keyword::Super,
        Keyword::Switch,
        Keyword::This,
        Keyword::Throw,
        Keyword::Throws,
        Keyword::Try,
        Keyword::Type,
        Keyword::Unsafe,
        Keyword::Var,
        Keyword::Void,
        Keyword::Volatile,
        Keyword::When,
        Keyword::While,
        Keyword::Yield,
    ];

    /// The source spelling. Round-trippable:
    /// `Keyword::lookup(kw.as_str()) == Some(kw)`.
    ///
    /// Useful for diagnostic messages ("expected `class`, found …") and
    /// for the formatter when re-emitting keyword tokens.
    pub fn as_str(self) -> &'static str {
        match self {
            Keyword::Abstract   => "abstract",
            Keyword::Annotation => "annotation",
            Keyword::As         => "as",
            Keyword::Async      => "async",
            Keyword::Await      => "await",
            Keyword::Break      => "break",
            Keyword::Case       => "case",
            Keyword::Catch      => "catch",
            Keyword::Class      => "class",
            Keyword::Const      => "const",
            Keyword::Continue   => "continue",
            Keyword::Default    => "default",
            Keyword::Do         => "do",
            Keyword::Drop       => "drop",
            Keyword::Else       => "else",
            Keyword::Enum       => "enum",
            Keyword::Extends    => "extends",
            Keyword::Final      => "final",
            Keyword::Finally    => "finally",
            Keyword::For        => "for",
            Keyword::If         => "if",
            Keyword::Implements => "implements",
            Keyword::Import     => "import",
            Keyword::Init       => "init",
            Keyword::Interface  => "interface",
            Keyword::Internal   => "internal",
            Keyword::Move       => "move",
            Keyword::Native     => "native",
            Keyword::New        => "new",
            Keyword::Operator   => "operator",
            Keyword::Package    => "package",
            Keyword::Permits    => "permits",
            Keyword::Private    => "private",
            Keyword::Protected  => "protected",
            Keyword::Public     => "public",
            Keyword::Record     => "record",
            Keyword::Return     => "return",
            Keyword::Sealed     => "sealed",
            Keyword::Sizeof     => "sizeof",
            Keyword::Static     => "static",
            Keyword::Struct     => "struct",
            Keyword::Super      => "super",
            Keyword::Switch     => "switch",
            Keyword::This       => "this",
            Keyword::Throw      => "throw",
            Keyword::Throws     => "throws",
            Keyword::Try        => "try",
            Keyword::Type       => "type",
            Keyword::Unsafe     => "unsafe",
            Keyword::Var        => "var",
            Keyword::Void       => "void",
            Keyword::Volatile   => "volatile",
            Keyword::When       => "when",
            Keyword::While      => "while",
            Keyword::Yield      => "yield",
        }
    }

    /// Classify a string as a keyword.
    ///
    /// **Case-sensitive.** Keywords are all-lowercase per §A.1.3; `Public`
    /// is a valid identifier, not a typo of `public`. (Contrast with
    /// annotation names, which §3.6 makes case-insensitive — but
    /// annotations are post-lex.)
    pub fn lookup(s: &str) -> Option<Keyword> {
        Some(match s {
            "abstract"   => Keyword::Abstract,
            "annotation" => Keyword::Annotation,
            "as"         => Keyword::As,
            "async"      => Keyword::Async,
            "await"      => Keyword::Await,
            "break"      => Keyword::Break,
            "case"       => Keyword::Case,
            "catch"      => Keyword::Catch,
            "class"      => Keyword::Class,
            "const"      => Keyword::Const,
            "continue"   => Keyword::Continue,
            "default"    => Keyword::Default,
            "do"         => Keyword::Do,
            "drop"       => Keyword::Drop,
            "else"       => Keyword::Else,
            "enum"       => Keyword::Enum,
            "extends"    => Keyword::Extends,
            "final"      => Keyword::Final,
            "finally"    => Keyword::Finally,
            "for"        => Keyword::For,
            "if"         => Keyword::If,
            "implements" => Keyword::Implements,
            "import"     => Keyword::Import,
            "init"       => Keyword::Init,
            "interface"  => Keyword::Interface,
            "internal"   => Keyword::Internal,
            "move"       => Keyword::Move,
            "native"     => Keyword::Native,
            "new"        => Keyword::New,
            "operator"   => Keyword::Operator,
            "package"    => Keyword::Package,
            "permits"    => Keyword::Permits,
            "private"    => Keyword::Private,
            "protected"  => Keyword::Protected,
            "public"     => Keyword::Public,
            "record"     => Keyword::Record,
            "return"     => Keyword::Return,
            "sealed"     => Keyword::Sealed,
            "sizeof"     => Keyword::Sizeof,
            "static"     => Keyword::Static,
            "struct"     => Keyword::Struct,
            "super"      => Keyword::Super,
            "switch"     => Keyword::Switch,
            "this"       => Keyword::This,
            "throw"      => Keyword::Throw,
            "throws"     => Keyword::Throws,
            "try"        => Keyword::Try,
            "type"       => Keyword::Type,
            "unsafe"     => Keyword::Unsafe,
            "var"        => Keyword::Var,
            "void"       => Keyword::Void,
            "volatile"   => Keyword::Volatile,
            "when"       => Keyword::When,
            "while"      => Keyword::While,
            "yield"      => Keyword::Yield,
            _ => return None,
        })
    }
}
