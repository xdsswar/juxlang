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
    /// E0414 — Access to a `private` member from outside the
    /// declaring class. Fires on field reads/writes, method calls,
    /// and `new T(...)` against a private constructor.
    E0414_PrivateAccess,
    /// E0415 — Access to a `protected` member from outside the
    /// declaring class's extends-chain. Subclasses (transitive) of
    /// the declaring class may use the member; unrelated code can't.
    E0415_ProtectedAccess,
    /// E0416 — Access to a package-private (default-visibility) or
    /// `internal` member from outside the declaring class's package
    /// (Phase 1 packages = compilation units). Mirrors Java's
    /// default visibility scoped to the same package.
    E0416_PackagePrivateAccess,
    /// E0420 — `class C extends F` where `F` is declared `final`.
    /// Final classes cannot be subclassed.
    E0420_FinalClassExtended,
    /// E0421 — Subclass declares a method that shadows a `final`
    /// method on the parent. Final methods cannot be overridden.
    E0421_FinalMethodOverridden,
    /// E0422 — `class C extends S` where `S` is `sealed` and `C`
    /// isn't in `S`'s `permits` list. Sealed classes restrict
    /// inheritance to the explicitly named subclasses.
    E0422_SealedClassNotPermitted,
    /// E0423 — `class C extends X` where `X` names something other
    /// than a class (e.g. an interface, record, enum, or type
    /// alias). Per Jux's inheritance rule a class can only extend
    /// another non-final class.
    E0423_ExtendsNotAClass,
    /// E0424 — `class C implements X` where `X` names something
    /// other than an interface. `implements` requires interface
    /// names.
    E0424_ImplementsNotAnInterface,
    /// E0425 — `this` referenced inside a `static` method or
    /// `static` field initializer. Static members aren't tied to
    /// an instance, so there's no `this` to refer to.
    E0425_ThisInStaticContext,
    /// E0426 — `@Override` annotation on a method that doesn't
    /// actually override an ancestor's method. Mirrors Java's
    /// `error: method does not override a method from its
    /// superclass` (which is the whole point of `@Override`).
    E0426_OverrideMissing,
    /// E0440 — A `switch` over a sealed type (enum or sealed
    /// class) doesn't cover every variant / permitted subclass
    /// and has no wildcard arm. Per `JUX-DIAGNOSTICS-ADDENDUM.md`
    /// §D.4 / type-system §T.5.5: exhaustiveness is mandatory
    /// for sealed-shape scrutinees so missed cases are caught at
    /// compile time, not via a runtime panic.
    E0440_NotExhaustive,

    // ---- Operators / Auto-derivation (E0900–E0999) ----
    /// E0930 — Conflicting operator declarations. Per
    /// `JUX-OPERATORS-ADDENDUM.md` §O.2.1, defining BOTH `operator<=>`
    /// AND any individual ordering operator (`<`, `<=`, `>`, `>=`) on
    /// the same type is a conflict — pick one form, not both. The
    /// spec's diagnostics table also lists this code for "auto-derive
    /// cannot satisfy required interface" (§O.5.1); both share the
    /// same E0930 slot and are distinguished by the diagnostic
    /// message.
    E0930_OperatorConflict,
    /// E0931 — `operator==` defined without `operator hash`. Per
    /// `JUX-OPERATORS-ADDENDUM.md` §O.2.7 and `JUX-LANG-V1.md` §7.14,
    /// a class/record/enum that defines structural equality must also
    /// define a consistent `hash` — otherwise the type behaves
    /// inconsistently as a `Map`/`Set` key. Emitting this code makes
    /// the pairing rule a build-time error rather than a runtime
    /// surprise.
    E0931_EqWithoutHash,
    /// E0935 — Call to a `delete`d operator. Per
    /// `JUX-OPERATORS-ADDENDUM.md` §O.3.4, a record/struct/enum can
    /// suppress an auto-derived operator with `operator <op>(...) = delete;`.
    /// Using the operator at a call site after deletion fires this
    /// diagnostic — most commonly seen as `print($"$myToken")` after
    /// `OpaqueToken` deleted `operator string`.
    E0935_DeletedOperator,
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
            Code::E0414_PrivateAccess            => "E0414",
            Code::E0415_ProtectedAccess          => "E0415",
            Code::E0416_PackagePrivateAccess     => "E0416",
            Code::E0420_FinalClassExtended       => "E0420",
            Code::E0421_FinalMethodOverridden    => "E0421",
            Code::E0422_SealedClassNotPermitted  => "E0422",
            Code::E0423_ExtendsNotAClass         => "E0423",
            Code::E0424_ImplementsNotAnInterface => "E0424",
            Code::E0425_ThisInStaticContext      => "E0425",
            Code::E0426_OverrideMissing          => "E0426",
            Code::E0440_NotExhaustive            => "E0440",
            Code::E0930_OperatorConflict         => "E0930",
            Code::E0931_EqWithoutHash            => "E0931",
            Code::E0935_DeletedOperator          => "E0935",
        }
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
