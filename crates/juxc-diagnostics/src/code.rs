//! Stable E-codes for the Jux compiler.
//!
//! Adding a code here is a spec change — allocate the number in
//! `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 **first**, then expose it here. The
//! `as u16` representation is stable; tooling depends on it.

/// A stable diagnostic identifier. Codes are documented in
/// `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[allow(non_camel_case_types)]
pub enum Code {
    // ---- Lexical (E0100–E0199) ----
    /// E0100 — Invalid character in source.
    E0100_InvalidCharacter,
    /// E0101 — Unterminated string literal.
    E0101_UnterminatedString,
    /// E0102 — Invalid digit separator placement.
    E0102_BadDigitSeparator,

    // ---- Syntax (E0200–E0299) ----
    /// E0200 — Unexpected token.
    E0200_UnexpectedToken,
    /// E0210 — `super(...)` or `this(...)` not first statement.
    E0210_ConstructorCallNotFirst,
    /// E0211 — Constructor missing required `super(...)` call.
    E0211_MissingSuperCall,

    // ---- Resolution (E0300–E0399) ----
    /// E0301 — Name not found in scope.
    E0301_NameNotFound,
    /// E0320 — Entry file has both top-level statements and a `main` function.
    E0320_AmbiguousEntryPoint,
    /// E0323 — `main`'s signature does not match any accepted form.
    ///
    /// Per `JUX-ENTRY-POINTS-ADDENDUM.md` §E.1.2 the accepted forms are
    /// `void main()`, `void main(String[])`, `int main()`, and
    /// `int main(String[])`, each optionally with a `throws` clause.
    E0323_MainSignatureMismatch,

    // ---- Type checking (E0400–E0499) ----
    /// E0400 — A top-level name (class, record, enum, interface, or
    /// function) is declared more than once in the same compilation
    /// unit. Per the language's single-namespace rule for top-level
    /// declarations, two `class Foo` declarations conflict.
    E0400_DuplicateDeclaration,
    /// E0401 — A class field is declared more than once in the same
    /// class body. Same name → conflict.
    E0401_DuplicateField,
    /// E0402 — A class method is declared more than once in the same
    /// class body. (Overloads will lift this restriction once method-
    /// overload resolution lands; today same-name methods conflict.)
    E0402_DuplicateMethod,
    /// E0403 — An enum variant is declared more than once in the same
    /// enum body.
    E0403_DuplicateVariant,
    /// E0410 — General type-mismatch error. Used for assignments, returns,
    /// and call arguments. The single code covers three usage sites; the
    /// `message` text distinguishes (e.g. "expected X, found Y", "cannot
    /// assign T to U", "expected return value of type X").
    E0410_TypeMismatch,
    /// E0411 — A function, method, or constructor was called with the
    /// wrong number of positional arguments.
    E0411_WrongArgCount,
    /// E0412 — `obj.field` where `field` doesn't exist on the receiver's
    /// class/record (walking the inheritance chain).
    E0412_UnresolvedField,
    /// E0413 — `obj.method(...)` where `method` doesn't exist on the
    /// receiver's class (walking the inheritance chain), or `new T(...)`
    /// where no class/record `T` is in scope.
    E0413_UnresolvedMethod,
}

impl Code {
    /// The canonical four-digit code as printed in diagnostics (`E0200`).
    pub fn as_str(self) -> &'static str {
        match self {
            Code::E0100_InvalidCharacter         => "E0100",
            Code::E0101_UnterminatedString       => "E0101",
            Code::E0102_BadDigitSeparator        => "E0102",
            Code::E0200_UnexpectedToken          => "E0200",
            Code::E0210_ConstructorCallNotFirst  => "E0210",
            Code::E0211_MissingSuperCall         => "E0211",
            Code::E0301_NameNotFound             => "E0301",
            Code::E0320_AmbiguousEntryPoint      => "E0320",
            Code::E0323_MainSignatureMismatch    => "E0323",
            Code::E0400_DuplicateDeclaration     => "E0400",
            Code::E0401_DuplicateField           => "E0401",
            Code::E0402_DuplicateMethod          => "E0402",
            Code::E0403_DuplicateVariant         => "E0403",
            Code::E0410_TypeMismatch             => "E0410",
            Code::E0411_WrongArgCount            => "E0411",
            Code::E0412_UnresolvedField          => "E0412",
            Code::E0413_UnresolvedMethod         => "E0413",
        }
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
