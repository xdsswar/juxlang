//! Literal values — integers, floats, strings, booleans, and `null`.
//!
//! Leaf module: no internal AST dependencies (and no [`juxc_source::Span`]
//! usage either — every type here is span-free, since literals are wrapped
//! in [`crate::Expr::Literal`] or [`crate::Pattern::Literal`] which carry
//! the span at the outer level).

/// A literal value.
#[derive(Debug, Clone)]
pub enum Literal {
    /// Integer literal with optional Jux-side type info from a suffix.
    Int(IntLit),
    /// Floating-point literal.
    Float(FloatLit),
    /// String literal — raw bytes between the Jux quotes; escape
    /// interpretation is deferred to a later phase.
    String(String),
    /// `true` / `false`.
    Bool(bool),
    /// `null`.
    Null,
}

/// An integer literal value with optional type info from its suffix.
///
/// The lexer recognizes Jux's full integer-suffix grammar (per §A.1.4):
/// `L`, `u`, `uL`, `b`, `ub`, `s`, `us`. The parser strips the suffix
/// and stores the canonical [`IntKind`] here; `kind == None` is the
/// default int (32-bit signed → Rust `i32`).
///
/// The original numeric base (`0x` / `0b` / `0o` / decimal) is also
/// preserved in [`IntLit::radix`] so the backend can re-emit the
/// literal in its source form — `0xF0` stays `0xF0`, not `240`. The
/// number of digits (after stripping underscore separators and the
/// radix prefix) is preserved in [`IntLit::digit_width`] so leading
/// zeros survive — `0x0F` stays `0x0F`, not `0xF`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntLit {
    /// The numeric value. `i64` is wide enough to hold any of the Jux
    /// integer types' source-level values; tycheck later validates that
    /// the value fits the declared type.
    pub value: i64,
    /// The suffix kind, or `None` for the default `int`.
    pub kind: Option<IntKind>,
    /// The base the user wrote the literal in. Lets the backend
    /// preserve `0xF0` / `0b1010` / `0o17` shapes in emitted Rust.
    pub radix: IntRadix,
    /// Number of digits in the source (after stripping underscore
    /// separators and the radix prefix). The backend uses this to
    /// preserve leading zeros — `0x0F` stays `0x0F`.
    /// Decimal literals don't use this for emission (decimal has no
    /// leading-zero ambiguity); it's still set for completeness.
    pub digit_width: u32,
}

/// The numeric base a [`IntLit`] was written in. Preserved through
/// parsing so the backend can re-emit the literal in its original form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntRadix {
    /// Decimal — no prefix (`42`, `1_000_000`).
    Decimal,
    /// Hexadecimal — `0x` / `0X` prefix (`0xF0`, `0xCAFE`).
    Hex,
    /// Binary — `0b` / `0B` prefix (`0b1010`).
    Binary,
    /// Octal — `0o` / `0O` prefix (`0o17`).
    Octal,
}

/// Suffixed Jux integer type. None of these values is `int` (the
/// default), so this enum only enumerates the kinds that need an explicit
/// type annotation in the emitted Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntKind {
    /// `b` — 8-bit signed, Rust `i8`.
    Byte,
    /// `ub` — 8-bit unsigned, Rust `u8`.
    UByte,
    /// `s` — 16-bit signed, Rust `i16`.
    Short,
    /// `us` — 16-bit unsigned, Rust `u16`.
    UShort,
    /// `u` — 32-bit unsigned, Rust `u32`.
    UInt,
    /// `L` — 64-bit signed, Rust `i64`.
    Long,
    /// `uL` — 64-bit unsigned, Rust `u64`.
    ULong,
}

impl IntKind {
    /// The Rust type-suffix to append to the emitted literal — e.g.
    /// `IntKind::Long.as_rust_suffix() == "i64"`, producing `42i64`.
    pub fn as_rust_suffix(self) -> &'static str {
        match self {
            IntKind::Byte   => "i8",
            IntKind::UByte  => "u8",
            IntKind::Short  => "i16",
            IntKind::UShort => "u16",
            IntKind::UInt   => "u32",
            IntKind::Long   => "i64",
            IntKind::ULong  => "u64",
        }
    }
}

/// A floating-point literal with optional type info from its suffix.
#[derive(Debug, Clone, PartialEq)]
pub struct FloatLit {
    /// The numeric value, always parsed as `f64` regardless of suffix.
    /// The backend handles narrowing to f32 when the suffix demands it.
    pub value: f64,
    /// `Some(FloatKind::Float)` for `f`-suffixed literals; `None` for
    /// default (double, Rust `f64`).
    pub kind: Option<FloatKind>,
}

/// Suffixed Jux float type. `Double` is the default — `None` covers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatKind {
    /// `f` — 32-bit, Rust `f32`.
    Float,
}

impl FloatKind {
    /// The Rust type-suffix to append to the emitted literal — e.g.
    /// `FloatKind::Float.as_rust_suffix() == "f32"`, producing `1.5f32`.
    pub fn as_rust_suffix(self) -> &'static str {
        match self {
            FloatKind::Float => "f32",
        }
    }
}
