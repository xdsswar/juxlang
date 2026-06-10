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
    /// E0304 — A `var` or typed-local declaration uses a name
    /// already bound in the **same** lexical scope. Per
    /// `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 / `JUX-LANG-V1.md` §6.1,
    /// re-declaring a name in the same block silently shadows in
    /// many languages but bites users; Jux forbids it. Outer-scope
    /// shadowing (a new scope re-using a name) is still allowed —
    /// only same-scope collisions fire this code.
    E0304_DuplicateLocalDeclaration,
    /// E0320 — Entry file has both top-level statements and a `main` function.
    E0320_AmbiguousEntryPoint,
    /// E0326 — A class member named `main` with an entry-shaped signature is
    /// not `static`. Per `JUX-ENTRY-POINTS-ADDENDUM.md` §E.1.2.2, a `main`
    /// inside a class must be `static` (it has no receiver — the runtime can't
    /// construct an instance to call it on). A non-static `main` is an ordinary
    /// method, not an entry point; the spec makes the likely mistake an error.
    E0326_ClassMainNotStatic,
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
    /// E0427 — A `static` method (on a class or interface) was
    /// called via an **instance** receiver — `obj.staticMethod()`
    /// instead of `Type.staticMethod()`. Java rejects this form
    /// because the receiver isn't actually used (the dispatch is
    /// resolved at the declaration's owning type), so the syntax
    /// suggests a per-instance call that doesn't happen. Jux
    /// follows suit: name the type explicitly.
    E0427_StaticCalledOnInstance,
    /// E0428 — `new X(...)` where `X` is not a class or record —
    /// for instance, an interface, enum, or type alias. Only
    /// classes and records can be instantiated. Without this
    /// code, the emitted Rust falls through to a confusing
    /// `expected a type, found a trait` from rustc.
    E0428_CannotInstantiate,
    /// E0429 — A class with `implements` doesn't supply every
    /// abstract method from the interface(s) it implements.
    /// Missing implementations are listed in the diagnostic so
    /// the fix is mechanical. Java raises the same error at
    /// compile time; Rust would surface it as E0046 with much
    /// less context.
    E0429_AbstractNotImplemented,
    /// E0430 — Diamond default-method conflict. A class
    /// `implements A, B` where both `A` and `B` provide a default
    /// implementation of the same method, and the class does
    /// not override it. The fix is to either pick one or
    /// override the method explicitly. Java requires this in the
    /// same shape; without it, rustc surfaces a much less
    /// readable "multiple applicable items" error.
    E0430_AmbiguousDefaultMethod,
    /// E0431 — A method carries a combination of modifiers that
    /// cannot coexist. Examples (per `classes-rules.md` §1.4):
    /// `abstract` declared inside a non-abstract class; `abstract`
    /// paired with `static`, `final`, or `private`. The
    /// diagnostic names the offending combination so the fix is
    /// mechanical.
    E0431_InvalidMethodModifiers,
    /// E0432 — A top-level class or interface declared `private`
    /// or `protected`. Per `classes-rules.md` §1.1 / §3.1 the
    /// only legal visibility for a top-level type is `public` or
    /// package-private (no modifier). Nested types can use the
    /// narrower modifiers, but Phase 1 doesn't have nested types
    /// yet.
    E0432_InvalidTopLevelVisibility,
    /// E0433 — An overriding method narrows its visibility
    /// relative to the method it overrides. Per
    /// `classes-rules.md` §1.4 the override must be **at least as
    /// visible** as the parent's. Without this code the lowered
    /// Rust still compiles but the narrowed override silently
    /// breaks Liskov substitutability — callers holding the
    /// parent type can't reach the override.
    E0433_OverrideNarrowsAccess,
    /// E0434 — A class's `extends` chain forms a cycle (direct
    /// `class A extends A` or transitive `A extends B extends A`).
    /// Per `classes-rules.md` §1.2 inheritance must be a DAG. The
    /// pre-fix symptom was a runtime OOM in the backend's ancestor
    /// walk; with this code the cycle is caught at tycheck.
    E0434_CyclicInheritance,
    /// E0435 — An interface is used as a **dynamically-dispatched value
    /// type** (an interface-typed local / parameter / field / return —
    /// lowered to `Rc<dyn Trait>`) but its shape isn't object-safe / not
    /// yet supported in this form. Per stage-1 interface dispatch, two
    /// Jux-expressible shapes are deferred: a **generic interface**
    /// (`interface A<T>` — a value slot would need `dyn A<Arg>`, threaded
    /// args land later) and an interface with a **generic method**
    /// (`<R> R map(...)` — genuinely not `dyn`-compatible in Rust).
    /// Firing here keeps the emitted `Rc<dyn Trait>` from leaking rustc's
    /// `E0038`/`E0107`. The interface itself remains a perfectly valid
    /// declaration — only its use as a `dyn` value type is restricted; it
    /// can still be implemented and called through concrete classes.
    E0435_InterfaceNotDynDispatchable,
    /// E0436 — A class that **extends the exception hierarchy** also
    /// `implements` an interface. Stage-1 interface dispatch makes
    /// interface trait methods `&self`, which is only satisfiable by the
    /// interior-mutable wrapper representation (`Rc<RefCell<…>>`). An
    /// exception class can't use that representation — the payload of
    /// `panic_any` must be `Send`, and `Rc<RefCell<…>>` is `!Send` — so it
    /// stays on the legacy `&mut self` value path, which a `&self`
    /// interface impl can't back. Rejecting the combination here keeps the
    /// emitted `impl Trait for ExcClass` from leaking rustc's `E0308` /
    /// `E0596`. (Exception classes and interfaces are each fine on their
    /// own; only their combination is deferred.)
    E0436_InterfaceOnExceptionClass,
    /// E0437 — A **data field is accessed through a polymorphic-base
    /// reference** (`Animal a = new Dog(); … a.someField …`). A polymorphic
    /// base lowers to a `Rc<dyn <Name>Kind>` trait object so virtual method
    /// dispatch works; a trait object can't expose the underlying struct's
    /// fields, so field access through such a reference isn't supported yet.
    /// Use an accessor method (`a.getSomeField()`) instead, or hold the value
    /// at its concrete type. (Stage-2 polymorphism; auto-generated field
    /// accessors are a planned follow-up.) Field access on `this` and on a
    /// concrete (non-base) receiver is unaffected.
    E0437_FieldThroughPolymorphicBase,
    /// E0438 — A **polymorphic base class declares a virtual method with its
    /// own generic type parameters** (`<R> R map(...)`). The base lowers to a
    /// `dyn <Name>Kind` trait object for virtual dispatch, and a generic
    /// method makes the trait not object-safe (rustc `E0038`). Make the method
    /// non-generic, mark the class (or method) `final`, or seal the hierarchy.
    /// (Stage-2 polymorphism; mirrors the interface rule E0435.)
    E0438_GenericVirtualMethod,
    /// E0442 — A **reference cast / type-test between unrelated types**
    /// (`(Dog) someString`, `x as Cat` where `x` can't be a `Cat`,
    /// `x => Unrelated`). A class/interface cast or `=>` test is only valid
    /// when the source and target are in a subtype relationship (one is the
    /// other's ancestor/implementer), or the target is `any` — otherwise the
    /// cast can never succeed. Use the related type, or `=>` to test before
    /// casting. (Sealed-type narrowing should go through `switch` instead.)
    E0442_UnrelatedCast,
    /// E0441 — A **type-test smart-cast binder used outside an `if`
    /// condition** (`var b = x => Dog d;`). The bound form `x => T name`
    /// introduces `name` as a smart-cast and is only meaningful as (or within)
    /// an `if` condition's then-branch. In any other position write the bare
    /// boolean test `x => T` (no binder).
    E0441_TypeTestBinderMisplaced,
    /// E0440 — A `switch` over a sealed type (enum or sealed
    /// class) doesn't cover every variant / permitted subclass
    /// and has no wildcard arm. Per `JUX-DIAGNOSTICS-ADDENDUM.md`
    /// §D.4 / type-system §T.5.5: exhaustiveness is mandatory
    /// for sealed-shape scrutinees so missed cases are caught at
    /// compile time, not via a runtime panic.
    E0440_NotExhaustive,
    /// E0431 — Generic type inference has no solution. Per the type-system
    /// addendum §T.4.2, a bare `new X<>()` whose type argument can't be
    /// inferred from the construction site AND is never pinned by later use
    /// (an unused, ambiguous local) fires this code — instead of leaking
    /// `rustc`'s `E0282 type annotations needed`. Write the argument
    /// explicitly (`new Vec<String>()`).
    E0431_GenericInferenceNoSolution,
    /// E0443 — A malformed **explicit call-site type-argument list** —
    /// the `<…>` in `id<int>(5)` / `obj.pick<String>(x)`. Fires when:
    /// the callee isn't generic (no type params to apply the args to),
    /// the count of explicit args doesn't match the callee's type-param
    /// count, or an argument names a type that doesn't resolve. Catching
    /// it here keeps `rustc`'s `E0107` ("wrong number of generic
    /// arguments") / `E0412` ("cannot find type") from leaking out of
    /// the emitted crate. Drop the `<…>` to rely on inference, or fix
    /// the argument list to match the declaration.
    E0443_ExplicitTypeArgs,
    /// E0444 — A **bounded wildcard used as a storage type over a
    /// user-defined generic class** — `Box<? extends Animal>` as a
    /// field, local-variable, or return slot. Phase 1 lowers such a
    /// slot by erasing the wildcard arg to a trait object inside the
    /// container (`Box<Rc<dyn AnimalKind>>`), but Rust generics are
    /// invariant: a concrete `Box<Dog>` can't flow into that slot
    /// without a structural conversion the compiler doesn't synthesize
    /// (Java gets this for free via erasure). Catching it here keeps
    /// `rustc`'s `E0308` from leaking. Wildcards in **parameter**
    /// position still work (they lift to a synthetic function generic);
    /// for storage, use a concrete type argument (`Box<Animal>`) or pass
    /// the value through a parameter instead. Full covariant-container
    /// storage is deferred to a later phase.
    E0444_WildcardStorageUnsupported,
    /// E0447 — An **or-pattern alternative introduces bindings** —
    /// `case Circle(var r) | Square(var s) ->` or `case var x | 0 ->`.
    /// Per grammar §A.3, the alternatives of `p1 | p2` must be
    /// binding-free: an arm body can't use a name that only exists
    /// when one specific alternative matched. (Rust surfaces this as
    /// `E0408 variable not bound in all patterns`; we catch it first.)
    /// Split the arm into one `case` per alternative, or drop the
    /// binders and re-test inside the body.
    /// E0212 — A **variadic parameter that isn't last** —
    /// `void f(int... xs, int y)`. The call-site packer maps every
    /// trailing argument into the varargs slot, so no parameter can
    /// follow it (§7.2, Entry Points §E.1.2.1). Move the `T...` to
    /// the end of the parameter list.
    E0212_VarargsNotLast,
    /// E0450 — An **ambiguous overload**: more than one candidate
    /// (constructor today; methods when §T.3 lands) can accept the
    /// call's argument count, and the Phase-1 arity-based selector
    /// has no way to rank them. Declared eagerly at the DEFINITION
    /// when two constructors' acceptable-argument-count ranges
    /// overlap (counting omittable defaults and varargs), since any
    /// call in the overlap would be unresolvable. Make the ranges
    /// disjoint, or fold the variants into one constructor with
    /// default parameters.
    /// E0260 — An **if-expression without an `else` branch** —
    /// `var x = if (cond) a;`. The value form must produce a value on
    /// every path (grammar §A.2.9: `if-expr = 'if' '(' expr ')' expr
    /// 'else' expr`), so `else` is mandatory; only the STATEMENT form
    /// may omit it. Add the `else`, or restructure as a statement.
    E0260_IfExprMissingElse,
    /// W0720 — A **`return` inside a `finally` block** (§X.3.5). The
    /// finally's return wins over everything: it discards a value
    /// being returned from the `try`/`catch` body AND swallows any
    /// in-flight exception — almost never what was meant. Move the
    /// return after the `try` statement, or compute the value in the
    /// body and return it there.
    W0720_ReturnInFinally,
    E0450_AmbiguousOverload,
    E0447_OrPatternBinding,
    /// E0448 — A **malformed named-argument list**: a positional
    /// argument after a named one, a name that doesn't match any
    /// declared parameter, or the same parameter supplied twice
    /// (by name, or by name AND position). Named arguments
    /// (`connect("h", port: 443)`) per grammar §A.2.9 / type-system
    /// §T.3.2: positional args fill parameter slots left-to-right,
    /// named args fill their named slot, every slot at most once.
    E0448_BadNamedArgument,
    /// E0449 — A **default-value expression references another
    /// parameter** (`int[] buf = new int[n]`). §S.1.3 allows
    /// defaults to read EARLIER parameters, but Phase 1 lowers a
    /// default by cloning it into the call site, where the
    /// parameter name doesn't exist — so any parameter reference
    /// is rejected with this code until the temp-hoisting lowering
    /// lands. Inline the computation into the function body
    /// (`if (buf == null) buf = new int[n];` with a `T?` param)
    /// as the Phase-1 workaround.
    E0449_DefaultArgParamRef,
    /// E0445 — A **const-generic form outside the Phase-1 core subset**.
    /// The core subset (grammar §A.2.6, type-system §T.11.3) covers:
    /// declaring `<int N>` / `<bool B>` params, using `N` as a fixed
    /// array size (`T[N]`) or as an int value, and instantiating with a
    /// literal (`new RingBuffer<float, 256>()`). This code fires on the
    /// deferred rest: const params of other value types (`<long N>`),
    /// non-literal const arguments (`new R<float, x>()`), const-generic
    /// arithmetic in array sizes (`byte[N + 1]` — needs the const-eval
    /// interpreter, spec phase 16), and a kind mismatch between the
    /// param and the argument (a type where a const value is expected,
    /// or vice versa). Catching these here keeps rustc's E0747/E0308/
    /// `generic_const_exprs` errors from leaking. (E0840–E0842 stay
    /// reserved for the real const-eval phase.)
    E0445_ConstGenericUnsupported,

    // ---- Async / Generators (E0700–E0799) ----
    /// E0710 — `throw` of a non-`Exception` value. Per the exceptions
    /// addendum §X.2.1 (`JUX-EXCEPTIONS-ADDENDUM.md`), `throw expr`
    /// requires `expr` to be of type `Exception` or a subclass. Throwing
    /// a primitive / `String` / unrelated value fires this code at type
    /// check, instead of leaking a `rustc` trait-bound error from the
    /// emitted `panic_any`.
    E0710_ThrowRequiresException,
    /// E0701 — `async` declared in a profile that has no async runtime. Per the
    /// async addendum §18.1.11, the `jux-core` profile has no event loop, so
    /// declaring an `async` function/method is a compile error; rewrite it with
    /// `Result<T, E>` and explicit state machines (§16.7).
    E0701_AsyncNotInProfile,
    /// E0702 — A **class object captured by a `Worker.spawn` closure**.
    /// `Worker.spawn` runs its closure on another OS thread (async
    /// addendum §18.2), but Phase-1 Jux objects are single-threaded
    /// shared references (`Rc<RefCell<…>>` — `!Send`), so the capture
    /// can't cross the thread boundary. Catching it here keeps rustc's
    /// `Rc<…> cannot be sent between threads safely` (E0277) from
    /// leaking. Pass primitive / `String` data into the closure and
    /// return results out; share state after `block_on` joins the task.
    E0702_ObjectCapturedBySpawn,
    /// E0720 — An unreachable `catch` clause. Per the exceptions addendum
    /// §X.3.4, catch clauses are tried in source order; a clause whose type is
    /// the same as, or a subtype of, an earlier clause's type can never run
    /// (the earlier, broader clause already caught it). The likely mistake —
    /// ordering `catch (Exception)` before `catch (IOException)` — is an error.
    E0720_UnreachableCatch,
    /// E0700 — `await` used outside an async context. Per the async
    /// addendum §18.1.2 (`JUX-ASYNC-ADDENDUM-v2.md`), `await` is only
    /// permitted inside an `async` function/method, an async lambda, or
    /// an `async main`. Using it in a plain function, a constructor, or a
    /// non-async lambda fires this code — catching it here keeps the
    /// `.await outside async fn` failure from leaking out of `rustc`.
    E0700_AwaitRequiresAsyncContext,

    // ---- Memory / Unsafe (E0500–E0599) ----
    /// E0506 — An `unsafe` operation used outside an `unsafe` context. Per
    /// the layout/ABI addendum §L.5.2, calling an `unsafe` function (e.g. a
    /// foreign `unsafe fn` such as `libc::getpid`) — or, in the future,
    /// dereferencing a raw pointer — is only legal inside an `unsafe { … }`
    /// block or the body of another `unsafe` fn. Catching it here turns what
    /// would be a `rustc` E0133 ("call to unsafe function") into a precise Jux
    /// diagnostic.
    E0506_UnsafeOpOutsideUnsafe,

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

    // ---- Properties (E0970–E0979) — JUX-MISSING-DEFS §M.7 ----
    /// E0970 — Write to a read-only or `init`-only property outside
    /// the place where it's settable. Per §M.7.2, a `{ get; }`
    /// property is settable only inside the declaring type's
    /// constructor, and a `{ get; init; }` property only during
    /// construction (`new`, or a record `with(...)`). An assignment
    /// reaching this code is a post-construction write, which the
    /// property forbids.
    E0970_PropertyNotWritable,
    /// E0972 — Property accessor visibility violation. Per §M.7.2 /
    /// §M.7.7, writing through a property whose `set` / `init`
    /// accessor is more restrictive than the access site allows
    /// (e.g. a `{ get; private set; }` property written from outside
    /// the declaring class) fires this code.
    E0972_PropertyAccessorVisibility,
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
            Code::E0304_DuplicateLocalDeclaration => "E0304",
            Code::E0320_AmbiguousEntryPoint      => "E0320",
            Code::E0323_MainSignatureMismatch    => "E0323",
            Code::E0326_ClassMainNotStatic       => "E0326",
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
            Code::E0427_StaticCalledOnInstance   => "E0427",
            Code::E0428_CannotInstantiate        => "E0428",
            Code::E0429_AbstractNotImplemented   => "E0429",
            Code::E0430_AmbiguousDefaultMethod   => "E0430",
            Code::E0431_InvalidMethodModifiers   => "E0431",
            Code::E0432_InvalidTopLevelVisibility => "E0432",
            Code::E0433_OverrideNarrowsAccess    => "E0433",
            Code::E0434_CyclicInheritance        => "E0434",
            Code::E0435_InterfaceNotDynDispatchable => "E0435",
            Code::E0436_InterfaceOnExceptionClass => "E0436",
            Code::E0437_FieldThroughPolymorphicBase => "E0437",
            Code::E0438_GenericVirtualMethod     => "E0438",
            Code::E0442_UnrelatedCast            => "E0442",
            Code::E0441_TypeTestBinderMisplaced  => "E0441",
            Code::E0440_NotExhaustive            => "E0440",
            Code::E0431_GenericInferenceNoSolution => "E0431",
            Code::E0443_ExplicitTypeArgs         => "E0443",
            Code::E0444_WildcardStorageUnsupported => "E0444",
            Code::E0212_VarargsNotLast           => "E0212",
            Code::E0260_IfExprMissingElse        => "E0260",
            Code::W0720_ReturnInFinally          => "W0720",
            Code::E0450_AmbiguousOverload        => "E0450",
            Code::E0447_OrPatternBinding         => "E0447",
            Code::E0448_BadNamedArgument         => "E0448",
            Code::E0449_DefaultArgParamRef       => "E0449",
            Code::E0445_ConstGenericUnsupported  => "E0445",
            Code::E0700_AwaitRequiresAsyncContext => "E0700",
            Code::E0701_AsyncNotInProfile        => "E0701",
            Code::E0702_ObjectCapturedBySpawn    => "E0702",
            Code::E0710_ThrowRequiresException   => "E0710",
            Code::E0720_UnreachableCatch         => "E0720",
            Code::E0506_UnsafeOpOutsideUnsafe    => "E0506",
            Code::E0930_OperatorConflict         => "E0930",
            Code::E0931_EqWithoutHash            => "E0931",
            Code::E0935_DeletedOperator          => "E0935",
            Code::E0970_PropertyNotWritable      => "E0970",
            Code::E0972_PropertyAccessorVisibility => "E0972",
        }
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
