# ERRATA.md — Spec reconciliations

**Status:** Active. Listed contradictions are resolved here, and as
of 2026-06-10 every entry has been applied back into the canonical
addenda (see the per-entry **Spec status** lines).

This file collects implicit contradictions or under-specified
behaviors across the Architecture addenda. Each item names the
conflicting addenda, picks the canonical interpretation, and
records the rationale. The interpretations here are normative —
the implementation follows them, and any future addendum edit
that touches these areas must agree with this file or land a
companion ERRATA update.

Items roughly ordered by leverage on downstream design.

---

## E1 — Panic vs Exception

**Conflict.** `JUX-EXCEPTIONS-ADDENDUM.md` says `throws ↔ Result`
— exceptions are checked, value-shaped, and propagated through
the type system. `JUX-SEMANTICS-ADDENDUM.md` talks about
per-profile panic behavior — `jux-full` panics on arithmetic
overflow / null-deref / array bounds, `jux-bare` aborts.

The contradiction: are panics catchable? If yes, they overlap
with exceptions; if no, they're a separate runtime mechanism.

**Resolution.** Two orthogonal layers, not one. **Exceptions are
the user-level error model**; they're values, declared in
`throws`, propagated through `Result` or `try`/`catch`.
**Panics are an abort-only runtime mechanism for "this should
never happen"** — array bounds, integer overflow in debug, null
dereference of a value that the type system said couldn't be
null. Panics are **NOT catchable from Jux source**. The user
only knows about exceptions.

**Java-parity carve-out (2026-06-12).** Integer division and
remainder by zero are the one condition moved from the panic
column to the exception column: `x / 0` and `x % 0` (integer
operands) **throw `ArithmeticException("/ by zero")`**, exactly
as in Java. Rationale: dividing by a runtime-zero is ordinary,
recoverable program logic (parsing user input, ratios over
empty collections), not a "this should never happen" invariant
violation — every Java-family programmer expects to `catch` it.
Overflow on division (`int.MIN / -1`) remains governed by the
overflow row above (debug panic / release wrap), consistent
with `+`/`-`/`*`.

The catalogue of conditions that panic vs. throw:

| Condition                              | Mechanism  | Catchable? |
|----------------------------------------|------------|------------|
| Arithmetic overflow (`jux-full` debug) | Panic      | No         |
| Arithmetic overflow (`jux-full` release) | Wrap     | N/A        |
| Array bounds violation                 | Panic      | No         |
| Division by zero (integer)             | Exception (`ArithmeticException`) | Yes |
| Null deref via `!!` (force-unwrap)     | Exception (`NullPointerException`) | Yes |
| Failing downcast (`(Dog) animal`)      | Exception (`ClassCastException`) | Yes |
| `T?` null deref via the type system    | Type error at compile time | N/A |
| File not found, parse error, etc.      | Exception  | Yes        |
| User `throw new MyException(...)`      | Exception  | Yes        |

**Spec edits this implies:**
- `JUX-SEMANTICS-ADDENDUM.md` §S.2.X should add a note that
  panic conditions are not user-catchable. The existing
  `Result<T, E>` story remains the value-shaped alternative.
- `JUX-EXCEPTIONS-ADDENDUM.md` §E.* should explicitly state
  that the `catch` clauses only match `Exception` subclasses,
  not `Panic` instances (which don't exist as user-visible
  types).

**Spec status:** Aligned (applied 2026-06-10) — `JUX-SEMANTICS-ADDENDUM.md`
§S.2.1 / §S.2.2 / §S.7.1 / §S.7.3 and `JUX-EXCEPTIONS-ADDENDUM.md`
§X.1.2 / §X.8 now carry the resolution.

---

## E2 — Init block ordering relative to `super()`

**Conflict.** `JUX-INHERITANCE-BORROW-ADDENDUM.md` mentions init
blocks executing during construction. `JUX-LANG-V1.md` §7.3.1
documents `super(args)` as the first statement of a child
constructor's body. The two together leave the order
ambiguous: does an init block run before or after the
`super(args)` call?

**Resolution.** **`super(args)` runs first**, before any code in
the child class. Init blocks (when added) run **after** the
parent's construction completes but **before** the child's
constructor body resumes. Concretely, the construction order
is:

1. Evaluate `super(args)` — parent's constructor (including its
   own ancestor chain, init blocks, and constructor body)
   completes.
2. Run the child class's own init blocks in source order.
3. Run the child's constructor body (after the implicit / explicit
   `super(...)` statement, which is statement zero).

This matches Java semantics. The rationale: a child's init block
may reference inherited fields, which only exist after the
parent has constructed. Allowing init blocks before `super(...)`
would force them to operate on uninitialized memory.

**Spec edits this implies:**
- `JUX-LANG-V1.md` §7.3.* should add the construction-order list.
- `JUX-INHERITANCE-BORROW-ADDENDUM.md` should cross-reference it.

**Spec status:** Aligned (applied 2026-06-10) — `JUX-LANG-V1.md` §7.3.1
("Construction order"), `JUX-SEMANTICS-ADDENDUM.md` §S.1.5 / §S.4.4
(init blocks now precede the constructor body), and
`JUX-INHERITANCE-BORROW-ADDENDUM.md` §6.9.5 now carry the resolution.

---

## E3 — Async borrow rule enforcement phase

**Conflict.** `JUX-COMPILER-PIPELINE-ADDENDUM.md` lists borrow
inference as Phase 11. `JUX-ASYNC-ADDENDUM-v2.md` describes
async-specific borrow rules (futures can't hold non-Send borrows
across await points, etc.). The two leave it unclear whether
async borrow enforcement is a separate sub-phase, lives inside
borrow inference, or runs alongside async lowering (Phase 14+).

**Resolution.** **One borrow checker, one phase.** Borrow
inference (Phase 11) is the sole arbiter of all borrow rules,
including the async-specific ones. Async lowering (Phase 14)
consumes the borrow-checked MIR and emits `Future` shapes; it
does NOT re-check borrows. Putting the rules in two places
would risk drift between sync and async semantics.

When borrow inference encounters an async function body, it
consults the same rule set with one addition: an `await`
expression inserts a yield point that breaks any non-`Send`
borrows whose lifetime spans the await. This is implemented as
a single visitor pass — no separate "async-borrow" check.

**Spec edits this implies:**
- `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.5 (borrow inference —
  *phase* 11; an earlier draft of this entry said "§C.11", which is
  actually Build Orchestration) should note that phase 11's borrow
  checker handles both sync and async bodies.
- `JUX-ASYNC-ADDENDUM-v2.md` should cross-reference §C.5 for
  the where, and define the rule's content here too.

**Spec status:** Aligned (applied 2026-06-10) —
`JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.5.1 and
`JUX-ASYNC-ADDENDUM-v2.md` §18.1.6 ("Who enforces it") now carry
the resolution.

---

## E4 — Cross-module `protected` access

**Conflict.** `JUX-INHERITANCE-BORROW-ADDENDUM.md` describes
`protected` as accessible to subclasses, suggesting cross-
package subclassing keeps the access. `JUX-TYPE-SYSTEM-
ADDENDUM.md` discusses visibility scoping in module terms
("package-private"), suggesting visibility composes with module
boundaries.

The contradiction: when a subclass in package `b` extends a
class in package `a` that has a `protected` field, can the
subclass's body in package `b` reach that field?

**Resolution.** **Yes — `protected` follows the inheritance
chain regardless of package boundary.** A subclass in any
package may read or write `protected` members it inherits from
an ancestor in any other package. The "subclass-only" rule
applies; "same package" is not an additional restriction.

This matches Java's `protected` semantics. Distinct from
`internal` and package-private which are scoped by package /
module.

| Modifier         | Subclass access? | Same-package access? |
|------------------|------------------|----------------------|
| `private`        | No               | No                   |
| `package` (default) | No            | Yes                  |
| `protected`      | Yes              | Yes                  |
| `internal`       | No               | Yes (same module)    |
| `public`         | Yes              | Yes                  |

**Spec edits this implies:**
- `JUX-LANG-V1.md` §7.4 (visibility) should formalize the table
  above.
- `JUX-INHERITANCE-BORROW-ADDENDUM.md` should cross-reference
  the cross-package allowance.

**Spec status:** Aligned (applied 2026-06-10) — `JUX-LANG-V1.md` §7.4
("Visibility across the hierarchy" table) and
`JUX-INHERITANCE-BORROW-ADDENDUM.md` §6.9.5 now carry the resolution.

---

## E5 — Nullable primitive types

**Conflict.** `JUX-LANG-V1.md` §7.10 example shows
`var length = findName(42)?.length();    // returns int?`,
implying `int?` is a valid type. The general spec design treats
primitives (`int`, `bool`, `float`, etc.) as **value types**
that never hold null.

**Resolution (revised 2026-06-17).** **Primitives CAN be nullable.**
The original resolution below rejected `int?` etc.; it was superseded
by commit `516fb1c` (2026-06-10), which removed the rejecting pre-pass.
`T?` is well-formed for any type `T`, reference OR primitive. A nullable
primitive (`int?`, `bool?`, `char?`, `float?`, and the unsigned /
width-explicit numerics) lowers to a stack `Option<T>`: `None` is a
discriminant, so a null primitive costs no heap allocation (no
`Integer`-style boxing). `T?` and `Option<T>` denote the same shape
(§K.3.1). The `findName(42)?.length()` example is therefore normative
and has type `int?`.

Nested nullability (a generic `T?` parameter where `T` is itself
instantiated to a nullable type, e.g. `f<int?>(5)`) NESTS into two
`Option` layers (`Option<Option<isize>>`); flattening is not possible
under Rust monomorphization. See `JUX-MISSING-DEFS-ADDENDUM.md` §M.15.

**Superseded original resolution.** Primitives were once rejected as
nullable (`E0410_TypeMismatch`, "primitive type X cannot be nullable"),
with `T?` restricted to reference inner shapes. No longer in force.

**Spec status:** Realigned 2026-06-17 to "primitives can be nullable".
`JUX-LANG-V1.md` §7.10 states the rule; the nested-nullability rule
lives in `JUX-MISSING-DEFS-ADDENDUM.md` §M.15.

---

## E6 — `?:` and `??` as elvis aliases

**Conflict.** `JUX-LANG-V1.md` §7.10 example uses `?:`; common
C#/JavaScript prose talks about `??`. The grammar addendum only
listed `?:` originally.

**Resolution.** **Both spellings are valid.** `?:` (Kotlin /
Groovy) and `??` (C# / JavaScript / TypeScript) parse to the
same `Expr::Elvis` AST. The grammar addendum has been updated
to list both. Pick whichever reads better at the call site;
diagnostics quote the spelling the user typed.

**Spec status:** Already reflected in
`JUX-GRAMMAR-ADDENDUM.md` (punctuation alphabet, elvis-expr
production, precedence table) and in `JUX-LANG-V1.md` §7.10's
example.

---

## E7 — Switch exhaustiveness diagnostic

**Conflict.** The diagnostics roster at
`JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 lists `E0440 — Switch is not
exhaustive`. The implementation enum (`juxc_diagnostics::Code`)
had no matching variant.

**Resolution.** Implementation now matches the spec: `Code`
exposes `E0440_NotExhaustive`. The check fires for `switch`
expressions whose scrutinee resolves to an enum or sealed
class and whose arms collectively miss variants / permitted
subclasses without a wildcard / bind catchall.

**Spec status:** Aligned. No further addendum edit needed —
this entry exists to record the formerly-divergent state.

---

## E8 — Duplicate local declaration diagnostic

**Conflict.** Spec §S.1.4 / §6.1 forbid re-declaring a local in
the same scope, but no E-code was allocated.

**Resolution.** `E0304_DuplicateLocalDeclaration` allocated in
`JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 and implemented in the
resolver. Outer-scope shadowing (a nested block reusing a name)
is still allowed; only same-scope collisions fire E0304.

**Spec status:** Aligned.

---

## E9 — Observable properties: `init` accessor removed, diagnostics renumbered

**Conflict.** `JUX-OBSERVABLE-PROPERTIES-ADDENDUM.md` (§P) arrived with
three tensions against the existing spec surface:

1. Its draft diagnostic codes `E0401`–`E0407` / `W0401`–`W0404`
   collided with published codes (`E0401`–`E0403` are the
   duplicate-field/method/variant errors). §D.5.1 forbids
   reassigning published codes.
2. §M.7 (and the grammar §A.2.5) specified a C# 9-style `init`
   accessor (`{ get; init; }`), which §P's design — and the
   project's direction — drops.
3. §P's draft made PascalCase property naming a hard error;
   the decision is that it is a **preferred convention only**.

**Resolution.**

1. Property diagnostics live in the established `E097x`/`W097x`
   family: `E0970` (write to read-only/computed property) and
   `E0972` (accessor visibility violation) already existed and
   absorb §P's draft E0402/E0401+E0403; new allocations are
   `E0973` (write to bound property), `E0974` (`bindBidirectional`
   type mismatch), `E0975` (observer lambda shape), and warnings
   `W0970`–`W0974`.
2. The `init` **accessor** is removed from the language: grammar
   §A.2.5 is now `accessor-kind = 'get' | 'set'`, and §M.7 was
   rewritten — read-only construction-time properties are
   `{ get; }` (settable in constructors and `init { }` blocks).
   The `init` *keyword* stays reserved for init-blocks (§M.1).
   **Compiler follow-up:** `juxc-parse` still accepts the `init`
   accessor (`is_init`) and `juxc-tycheck` still carries an
   init-only write path under `E0970` — both must be removed.
3. PascalCase property naming is surfaced as suppressible
   warning `W0974` and an IDE rename hint. It never blocks
   compilation: `private String test { get; set; } = "test";`
   is legal. No renames are applied to any existing spec or
   stdlib surface.

**Spec status:** Aligned (docs). Compiler still implements the
`init` accessor and none of `E0973`–`E0975` / `W0970`–`W0974`;
tracked as the observable-properties implementation work.

---

## E10 — Parameter modifiers: `final` semantics, `weak` parameters, defaults ordering

**Conflict.** Parameter modifiers were specified piecemeal and left two
gaps and one terse contradiction:

1. `final` appeared in the grammar (`param-mode`, §A.2.4) but its
   *semantics* were never written — what does `final` on a parameter do?
2. `weak` was specified for fields only (§6.5); whether it could apply to
   a parameter was undefined.
3. `JUX-MISSING-DEFS-ADDENDUM.md` §M.4.1 wrote `param-mode = 'final' | 'out'`,
   while the grammar (§A.2.4) writes `param-mode = binding-immut | 'out'`
   (with `binding-immut = 'const' | 'final'`). The `const` synonym was
   dropped in the §M.4 snippet.
4. The default-parameter *ordering* rule (defaults must be trailing) and
   the legal *combinations* of all the modifiers were never stated.

**Resolution.** A new consolidated section, **§M.14 "Parameters:
Comprehensive Reference"**, is normative for all of the above:

1. A `final`/`const` parameter is an **immutable binding** — it cannot be
   reassigned in the body (new code `E0464`); reading it and mutating the
   *fields* of a `final` class parameter remain legal. Lowering omits Rust
   `mut`.
2. `weak T` is allowed as a **parameter** (T a class), mirroring weak
   fields: read via `.get()` → `T?`, lowers to `Weak<RefCell<T_Inner>>`,
   call-site downgrades the argument. Reuses `E0455`/`E0456` (now
   "field **or parameter**"). No default-valued weak parameter in Phase 1.
3. `param-mode = binding-immut | 'out'` (the grammar form) is canonical;
   the terse §M.4.1 line is read as shorthand.
4. Defaults must be trailing (`E0467`); the full combination matrix
   (§M.14.5) allows `final` to compose with `ref`/`weak`/defaults/varargs,
   keeps `ref`/`weak`/`out` mutually exclusive, and forbids the
   meaningless combos via `E0466` (and the generalized `E0944`).

**Spec status:** Aligned (docs) — §M.14 added, grammar §A.2.4 annotated,
diagnostics §D rows added (`E0464`/`E0466`/`E0467`, generalized
`E0455`/`E0456`/`E0944`). Compiler implementation tracked as the
parameter-features work (`final` enforcement, `weak` parameters, combination
validation, default-ordering check).

---

## E11 — `!!` on null: panic or `NullPointerException`?

**Conflict.** E1's catalogue (`ERRATA.md` above) lists "Null deref via `!!`
(force-unwrap)" as a **Panic**, not catchable, and
`JUX-SEMANTICS-ADDENDUM.md` §S.5 agrees ("null deref of `T`" is in the panic
list, and panics "are not catchable from Jux source").
`JUX-GRAMMAR-ADDENDUM.md` §A.5's conversion table says the opposite: both
`T? -> T` via `as` and `T? -> T` via `!!` "throw `NullPointerException`",
which is an `Exception` and therefore catchable. `JUX-LANG-V1.md` §7.10 adds
a third voice: "This eliminates NullPointerException as a runtime failure
mode."

**Today (2026-09-08).** `!!` throws a catchable `NullPointerException`, and
a failing downcast throws a catchable `ClassCastException`. §A.5's table row
was right.

**Resolution.** DECIDED for the exception, and what decided it was not the
argument. `NullPointerException` and `ClassCastException` are declared classes
in the embedded stdlib extending `RuntimeException`, so
`catch (NullPointerException e)` type-checked and compiled -- and then never
ran, because the raise was `panic!("...")` whose `&str` payload no catch arm
can downcast. The program aborted THROUGH a handler written to stop it.

That is not one of the two defensible readings. It is a fourth behaviour --
a handler that compiles and does nothing -- and it is the only one that is
indefensible. Integer division by zero had already picked the mechanism that
works, `panic::panic_any(<the exception value>)`; both sites now use it.

Both exceptions catch as themselves, as `RuntimeException`, and as
`Throwable`, per `examples/runtime_exceptions.jux`.

**Spec status:** RESOLVED toward §A.5. The E1 catalogue rows above are
updated. `JUX-SEMANTICS-ADDENDUM.md` §S.5 still lists "null deref" among the
panics and wants the same correction; the array-bounds row is unchanged and
still panics.

---

## E12 — `(char)` from an out-of-range integer

**Conflict.** Three answers. `JUX-GRAMMAR-ADDENDUM.md` §A.5 says `int` ->
`char` is "bit-equivalent" with no runtime check, and §S.2.4's blanket
promise is that all numeric `as` conversions are "infallible at runtime --
they always produce a value of the target type, never throw, never panic".
`JUX-SEMANTICS-ADDENDUM.md` §S.3.4 says constructing a `char` from an
out-of-range integer "panics in debug, wraps to a valid scalar via masking in
release".

**Today.** Neither: the compiler substitutes U+FFFD (the replacement
character) for a value outside the Unicode scalar range.

**Resolution.** DEFERRED. The implementation's answer is the most forgiving
of the three and the only one that keeps §S.2.4's infallibility promise, but
it is not what either document says, and picking one is an observable change.

**Spec status:** Unresolved. §A.5, §S.2.4 and §S.3.4 disagree; the
implementation matches none of them exactly.

---

## E13 — `String`'s method surface

**Conflict.** `JUX-CORE-LIB-ADDENDUM.md` §K.7 gives `String` a closed
declaration -- `byteLength`, `charLength`, `bytes`, `chars`, `concat`,
`repeat`, `substring`, `substringBytes`, plus operators -- and states that
`equals` and `compareTo` do not exist because `==` and `<=>` already say it.
But `JUX-LANG-V1.md` §7.10's example calls `?.toUpperCase()` and `?.length()`,
neither of which §K.7 declares, and `ERRATA.md` E5 marks that example
**normative**. `JUX-SEMANTICS-ADDENDUM.md` §S.3.3 specifies
`s.compareTo(t)` as byte-wise lexicographic order -- the method §K.7 says
does not exist. `JUX-GAPS-ROADMAP.md` lists `toUpperCase` and friends as not
yet specified, under a future `std.string`.

**Today.** The surface is §K.7's, plus a compiler-known set (`length`,
`toUpperCase`, `toLowerCase`, `trim`, `split`, `replace`, `indexOf`,
`charAt`, `contains`, `startsWith`, `endsWith`, `isEmpty`), plus whatever the
rustdoc scan found on Rust's `String` (`push_str` and the rest). Anything
else is `E0413`, and `equals` / `compareTo` get a hint naming `==` and `<=>`.
So §7.10's normative example compiles and §S.3.3's `compareTo` does not.

**Resolution.** DEFERRED, but this is the one most worth resolving: the
implementation made the surface CLOSED, which turns a documentation
inconsistency into a compile error. §K.7's declaration needs to grow the
methods the language actually offers, or §S.3.3's sentence needs to go.

**Owner ruling (2026-09-19): keep `String.length()`.** It stays part of the
surface and counts characters, exactly like `charLength()` (`"héllo".length()`
is 5; `byteLength()` is 6). §K.7 now declares it, so §7.10's normative
`?.length()` example is backed by the declaration. The rest of this entry
(`toUpperCase` and the other compiler-known names, §S.3.3's `compareTo`)
is unchanged.

**Spec status:** Partly resolved (`length()`). §S.3.3 and the other
compiler-known names remain open.

---

## E14 — `Rc` or `Arc` for a base-typed handle

**Conflict.** `JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.2 and §CR.5.2 say a
dyn-dispatched slot forces **`Arc`** ("trait objects in Jux are
reference-counted at the spec level"). `JUX-INHERITANCE-BORROW-ADDENDUM.md`
§6.9.6 and `JUX-V0.1-READINESS.md` both spell the same handle
`Rc<dyn ContainerKind<T>>`.

**Today.** `Rc`, upgraded to `Arc` only when the class crosses a worker
boundary (§18.2), which is the rule the representation ladder actually
implements.

**Resolution.** DEFERRED. The implementation's behaviour is the useful one --
paying for atomics on every polymorphic value would be a real cost for a
guarantee single-threaded code does not need -- so the likely resolution is
that §CR.2's "forces Arc" is the sentence to correct.

**Spec status:** Unresolved. Two documents say `Rc`, one says `Arc`.

---

## E15 — Narrowing a literal, and an unsuffixed literal's type

**Conflict, part one.** `(byte) 300`. §S.2.4 says a larger-to-smaller
conversion truncates to the low N bits, which makes it `44`. §S.2.5 says an
untyped integer literal adapts to the type the context demands "when the
value fits", which makes `300` in a `byte` context a range error
(`E0105` / `E0202`). Both readings are supportable.

**Conflict, part two.** `JUX-LANG-V1.md` §5.1 says an unsuffixed integer
literal defaults to `i32`; `JUX-GRAMMAR-ADDENDUM.md` §A.1.4 says `int`, which
§5.1 itself makes platform-sized.

**Today.** `(byte) 300` truncates to `44`, matching Java and §S.2.4, and the
literal carries its own type into the cast so rustc does not reject it as out
of range for the target. An unsuffixed literal is `int` (`isize`).

**Resolution.** DEFERRED. The implementation follows the cast table and the
grammar; §S.2.5's "when the value fits" is about implicit ADAPTATION to a
slot, not about an explicit cast, so the two are probably compatible and the
text should say so.

**Spec status:** Unresolved wording. No behaviour is in doubt.

---

## E16 — An auto-added bound that a type argument cannot satisfy

**Conflict.** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.2.1 has the compiler add
bounds a program never wrote (`Display` for a formatted parameter, `Eq + Hash`
for a map key, `Ord`, `Default`). It never says what happens when the type
argument cannot meet one. The diagnostics catalogue has `E0446` for violating
a WRITTEN `extends` bound and `E0941` for a `where T has operator` clause, and
nothing for an inferred one.

**Today.** It reaches rustc. That is how `Loud<Store<int>>` failed before
`2afb41f`: a polymorphic class had no string form, could not satisfy the
`Display` bound §T.2.1 added, and the user saw a Rust trait-bound error about
a bound their program never mentions.

**Resolution.** PARTLY ADDRESSED, and the rest deferred. §T.2.1 now states
that every Jux type can satisfy every bound the compiler adds on its own --
`Clone` and `Debug` are universal and every type has a string form -- which
removes the failure mode for the bounds that exist today. `Eq + Hash` and
`Ord` on a user type that defines no `operator==` / `<=>` remain reachable,
and want a diagnostic of their own. So does `Hash` on a parameter a body
hashes with `.operator hash()` (§O.2.7) when the type argument is `float` or
`double`: every other type hashes (a class by identity), but Rust gives floats
no `Hash`. The `PartialEq` a compared parameter acquires is always met.

**Spec status:** §T.2.1 states the satisfiability guarantee. No code is
allocated for the residual case.

---

## E17. `move` is documented as usable and recorded as deferred

**Conflict.** `JUX-LANG-V1.md` §6.4 presents `move` as working syntax, with a
worked example and a rationale ("use `move` to make the transfer intent
unambiguous"):

```java
users.add(move alice);           // explicit move
```

`JUX-MISSING-DEFS-ADDENDUM.md`:404 says the opposite: "**Deferred.** `out
null` (§M.4.4) and the `move` call-site operator are follow-ups".

**Today.** Deferred is what the compiler does, and as of 2026-09-08 it says
so: `move` in expression position reports `E0203` naming both the section
that documents it and the addendum that defers it, then parses its operand so
nothing cascades.

**Resolution.** DOCUMENTED, not decided. Phase 1 lowers every class to a
shared handle, so a transfer has nothing to transfer -- `move` would be a
no-op with a different meaning later, which is the worst kind of syntax to
accept quietly. §6.4 keeps the syntax as the intended design; the compiler
refuses it with a diagnostic that says where the design lives.

**Spec status:** Both sections stand. The reader is warned by the compiler
rather than by the spec, which is the wrong way round and wants a "not in
Phase 1" note in §6.4 when someone next edits it.

---

## E18. Three more reserved words the spec documents and Phase 1 lacks

**Conflict.** Same shape as E17, three more times.
`annotation Name { … }` has an entire addendum
(`JUX-ANNOTATIONS-ADDENDUM.md`, "a real type kind, not a comment
convention"). The `volatile` field modifier appears in the memory-mapped I/O
section of `JUX-LANG-V1.md` and as `core.volatile.Volatile<T>` in
`JUX-CORE-LIB-ADDENDUM.md`, where the status column says "Spec'd". `yield` is
in the reserved-word list and named beside `await` and `move` as
"language-defined", with no generator form anywhere.

**Today.** All three report `E0203` and recover cleanly. Before that they
produced cascades that never mentioned the construct at fault -- a `volatile`
field cost three errors, the last two about a class that parsed fine.

**Resolution.** DOCUMENTED. Each is a real design that Phase 1 does not
carry. The rule going forward: a keyword enters the lexer's inventory only
with a production or an `E0203` site, never with neither.

**Spec status:** The addenda stand as designs. `E0203` is documented in
`JUX-DIAGNOSTICS-ADDENDUM.md`.

---

## E19. `T?[]` and `T[]?` are one type

**Conflict.** The grammar (`JUX-GRAMMAR-ADDENDUM.md` §A.2.7) builds types by
composition: `nullable-type = simple-type '?'?` and `array-type = type
array-dim+`, so `Node?[]` is an array of nullable `Node`, and `Node[]?` is a
nullable array of `Node`. They are different types.

**Today.** The compiler's type reference carries one nullable flag for the
whole type, so both spellings mean the same thing, a nullable array. An array
whose ELEMENTS may be `null` cannot be written, and `new Node?[n]` does not
parse.

**Resolution.** DOCUMENTED, not fixed. Representing the two needs nullability
per level of the type, which every consumer of a type reference would have to
learn. Until then, a sequence of nullable elements is a `Vec<Node?>`, which
works, and `new Node[n]` of a class is `E0458` (§5.5) with that advice.

**Spec status:** The grammar stands. Phase 1 accepts a subset of it.

---

## E20. Structs are value types

**Conflict.** JUX-LANG-V1 §5.2 and §7.6 define `struct` as a stack-allocated
value type copied on assignment, but §5.5 calls "an ordinary struct" a reference
type, "exactly like a class", and the grammar's Phase-1 note routes `struct`
through the class node with value semantics "a later turn". Today a struct is a
shared handle.

**Resolution.** §7.6 is normative. A `struct`:

- is a value: assignment, argument passing, return and storage into a field,
  array or collection each COPY it. A class-typed field inside a struct is a
  handle, so the copy shares that object (a shallow copy, as C# does);
- lowers to a plain Rust `struct` deriving `Clone`, plus `Copy` when every field
  is `Copy`; it is never behind `Rc<RefCell>`;
- has no identity: `===` on a struct is the same test as `==` (JUX-LANG-V1
  §7.14.3, as for a record), and no inheritance (`extends` on a struct is
  `E0423`); it may `implement` interfaces, and a struct stored in an
  interface-typed slot is copied into it;
- gets an implicit constructor taking every field in declaration order when it
  declares none (`new Point(3.0, 4.0)`), in addition to explicit constructors;
  a field with an initializer may be omitted from the positional form only by
  declaring a constructor;
- gets `operator==`, `operator hash` and `operator string` derived exactly as a
  record does (OPERATORS §O.3.2), unless a field type lacks the operator;
- has a default value (`new Point[4]`) when every field has one (§5.5);
- has public fields by default, and may declare methods and a `drop` block.

`@layout(c)` structs (LAYOUT-ABI §L.1.2) are the same value type with C layout.

**Spec status:** §5.5 and the grammar's Phase-1 note are corrected to point here.

---

## E21. Construction order with field initializers hoisted

**Conflict.** JUX-LANG-V1 §7.3.1 runs every field initializer of the whole
hierarchy before any constructor body (a deliberate divergence from Java).
`JUX-SEMANTICS-ADDENDUM.md` §S.4.4 lists field initializers per class after the
super call, and `JUX-MISSING-DEFS-ADDENDUM.md` §M.1.4 runs a class's constructor
body BEFORE its `init` blocks, contradicting §M.1.2.

**Resolution.** For `new C(args)`:

1. Every field initializer of the hierarchy runs, base class first, each class
   in textual order.
2. For each class from the root down: its `init` blocks in textual order, then
   its constructor body (with `super(...)` / `this(...)` resolved first, as the
   constructor's statement zero).

A virtual call made from a base constructor dispatches to the subclass override
and sees the subclass's initialized fields, but the subclass's `init` blocks and
constructor body have NOT run yet. A class that declares no constructor gets the
implicit one, which still runs its parent's constructor chain.

**Spec status:** §S.4.4 and §M.1.4 are corrected to match §7.3.1.
Superseded by E140: construction now runs in Java's order, each class's
initializers after its `super(..)`, by the user's choice of Java parity.

---

## E22. An auto-property with no initializer is nullable, including primitives

**Conflict.** `JUX-OBSERVABLE-PROPERTIES-ADDENDUM.md` §P.1.2 says an
uninitialized `int` property defaults to `0`; `JUX-MISSING-DEFS-ADDENDUM.md`
§M.7.3.1 says it is implicitly `int?` and reads `null`.

**Resolution.** §M.7.3.1 is normative: no initializer (or `= null`) makes the
property `T?`, for primitives too. `Count = Count + 1` on such a property is a
Jux type error (`E0410`/`E0418`), and `= 0` gives a non-nullable `int`.

**Spec status:** §P.1.2 is corrected.

---

## E23. The borrow model is the runtime-checked shared handle

**Conflict.** JUX-LANG-V1 §6.3, §6.4, §6.9 and `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.7
describe a static borrow checker with compile errors for conflicting borrows,
use-after-move and override mutability changes. `JUX-DIAGNOSTICS-ADDENDUM.md`
records the decision that Jux has no user-visible borrow checker: every class is
a shared handle (§CR), aliasing is the normal case, and conflicting mutation is
caught at run time by the handle's cell.

**Resolution.** The diagnostics decision is normative. §6.3/§6.4/§6.9 and §T.7
describe a representation Jux does not use; their compile errors do not reject
programs. Where a static check is cheap and certain it may be reported as a
warning. Structs (E20) are copied, not borrowed, so they add no borrow rules.

**Spec status:** §T.7 and V1 §6.3/§6.4/§6.9 carry a pointer to this entry.

---

## E24. Collection combinators live on `Iterable<T>`, not on `Vec`

**Conflict.** JUX-LANG-V1 §7.9 shows `users.map(...)`, `.filter(...)` and
`.reduce(...)` on a list, while the standard library is Rust's std with Rust's
names, and `JUX-CORE-LIB-ADDENDUM.md` §K.5 puts the combinators on the Jux
`Iterable<T>` protocol.

**Resolution.** `Vec` and the other `rust.std` collections keep their Rust
surface (`iter()`, `len()`, `push()`); the default combinators (`map`,
`filter`, `reduce`, `take`, `skip`, `zip`, `chain`, `any`, `all`, `count`) are
default methods of `Iterable<T>`, available on every Jux iterable and on
`iter()` results.

**Spec status:** §7.9's example is rewritten to that surface.

---

## E25. Diagnostic codes given two meanings

**Conflict.** The catalog lists `E0302` as "cyclic module import", but the
compiler has raised `E0302` for a same-package import since it was published.
`E0908` means "dynamic linkage in the core profile" in the build system and "C++
template without `instantiate`" in bindgen. Several addenda quote codes the
implementation raises under another number.

**Resolution.**

- `E0302` keeps its published meaning, same-package import. Cyclic module
  imports get `E0308` *(reserved)*.
- `E0908` keeps the build-system meaning; the bindgen C++ template diagnostic is
  `E0909` *(reserved, C++ is deferred)*.
- The quoted numbers follow the implementation: or-pattern bindings `E0447`
  (was `E0270`), unrelated casts `E0442` (was `E0310`), default-method conflicts
  `E0430` (was `E0810`), protected access through a base type `E0415` (was
  `E0830`), `@Override` on a non-override `E0426` (was `E0471`).
- Bindgen's `E0306`, `E0907`, `W0305`, `W0306`, `W0307` join the catalog as
  reserved rows.

**Spec status:** the catalog and the addenda are corrected.

---

## E26. How a program calls `operator hash`

**Conflict.** JUX-OPERATORS-ADDENDUM §O.4.2 wrote `value.hash()` to hash a
field, while the §O.8.3 `Path` example and the Map/Set description wrote
`key.operator hash()`. Neither was implemented, so an `operator hash` over a
`String` field could not be written at all, and the IDE's "Generate operator==
and hash" had nothing it could emit.

**Resolution.** `x.operator hash()` and `x.operator string()`, for every value
(§O.2.7). `value.hash()` contradicts §O.2.2, which says operators are not
magic-name methods, and would collide with a user method named `hash`.

**Spec status:** §O.2.7 states the rule; the §O.4.2 example is corrected.

---

## E27. A worker capture of a collection

**Conflict.** JUX-ASYNC-ADDENDUM §18.2 listed `Vec<T>` and `Map<K, V>` as
transferable because "their internal sharing is thread-safe by construction".
Since JUX-LANG-V1 §6.5.2 and §6.5.1 made collections and arrays shared handles,
that is no longer true: the handle is single-threaded. Every collection or
array capture, including §18.2's own `parallelSum` example over `int[] data`,
failed in rustc, and so did a closure reaching `this` and a capture of a
function value, an interface or a stream.

**Resolution.** A worker takes a captured collection or array by value, one
level deep, as §18.2 already says for every capture: the worker gets its own
copy. Changes in the worker stay in the worker, which is the one place a
collection does not alias (§6.5.1 already made the same exception for a
collection read out of a worker-shared object). A collection of collections, a
record holding a collection, and the values that are never transferable
(function values, interface handles, streams) are `E0702`, named by value. A
closure reading a field or method of its own class captures `this`, and the
class is upgraded to the atomic handle like any captured object.

**Spec status:** §18.2 states the rule.

---

## E28. Which values an `any` holds, and what it prints

**Conflict.** JUX-TYPE-SYSTEM-ADDENDUM §T.1.2 describes `===` on an `any`
holding a value type ("primitive equality for value types"), while §T.3.6 let
only reference types convert to `any`. Neither said whether an `any` has a
string form, though every other Jux type has one (JUX-OPERATORS-ADDENDUM
§O.7.1).

**Resolution.** Every value converts except a nullable one (it needs `any?`)
and a function value, which has neither an identity nor a string form. An
`any` prints as the value it holds. Everything but `===`, `=>` and the text
is `E0489`.

**Spec status:** §T.1.2 states the rules; §T.3.6 is corrected.

---

## E29. An open range pattern's bound, and a `switch` over an integer

**Conflict.** JUX-MISSING-DEFS-ADDENDUM §M.6.4 labelled `case ..0 ->`
"negative or zero", while `..` excludes its bound everywhere else (§M.6.1, and
JUX-TYPE-SYSTEM-ADDENDUM §T.5.3 reads `0..10` as `[0, 10)`). §T.5.2 said an
integer scrutinee is one interval to cover, but the compiler checked nothing
for it, so a `switch` over an `int` missing a value failed inside rustc.

**Resolution.** `..0` is "below 0" and `..=0` is "0 or less", as the closed
forms read. A `switch` over an integer is checked against the type's range and
reports the lowest missing value; `char`, floats and `String` need an arm that
matches everything. Statements are checked like expressions (§T.5).

**Spec status:** §M.6.4's example and prose are corrected; §T.5.2 and §T.5.3
state the rules.

---

## E32. The enum lookups and `cases()`

**Conflict.** JUX-LANG-V1 §7.7.3 listed `fromName`, `fromOrdinal` and
`cases()` in its table and then said none of them existed: `fromName` waited
on a case-insensitivity decision and `cases()` on an `EnumCase<T>` type
nobody had defined. The same section had already made the decision
(case-insensitive, with `fromNameStrict` for exact matching), and a call
reached rustc instead of failing in Jux.

**Resolution.** All four exist. `fromName` is case-insensitive; when two
variants differ only in case the exact spelling wins, then the first
declared. `EnumCase<T>` is a stdlib class in `jux.std.meta` with `name()`,
`ordinal()`, `payload()`, `hasPayload()` and `value()`. `value()` is not in
the original description; it gives the descriptor's type parameter a use (a
payload-free variant's own value, `null` for a payload variant), which is
also what lets a program go from a descriptor back to the variant. `cases()`
is on every enum without type parameters: `Tree.cases()` is a static call,
with no type arguments to say what `EnumCase<Tree<T>>` holds.

**Spec status:** JUX-LANG-V1 §7.7.3 states the rules and the `EnumCase`
members.

---

## E33. Modifiers on records and enums

**Conflict.** Grammar §A.2.5 permitted `sealed` only on classes and
interfaces and gave records and enums no modifiers at all, while JUX-LANG-V1
writes `public final record Circle(...)` (§7.5) and `public sealed enum
HttpResponse` (§7.7.1), and JUX-CORE-LIB-ADDENDUM writes `sealed enum
Option<T> permits Some, None`. The parser rejected all three, and `const
class`, which §7.3.1 defines as a synonym for `final class`.

**Resolution.** A modifier that restates what the type already is, is
accepted and changes nothing: `final` or `const` on a record, `sealed` or
`final` on an enum. `const class` is `final class`. A modifier that
contradicts the type is an error: `abstract` on a record or an enum, `sealed`
on a record. An enum's `permits` list may name only its own variants, and all
of them, or it is `E0490`.

**Spec status:** Grammar §A.2.5 states the rule; `E0490` is in the catalog.

---

## E34. Values that belong to each enum variant

**Conflict.** JUX-LANG-V1 §7.7.4 showed a Java-style enum with fields and a
constructor (`Planet`) without saying how a variant's arguments reach the
constructor, when the constructor runs, or what a constructor may do, and the
grammar (§A.2.5) had no field or constructor members for an enum at all. The
grammar also let any enum write an explicit discriminator (`Ok = 200`), while
Layout-ABI §L.1.3 rejects one outside `@layout(c)` with `E0510`.

**Resolution.** In an enum that declares a constructor, a variant's
parentheses hold constructor arguments. The per-variant fields are `final`
(`E0494`); a constructor gives each one its value exactly once and does
nothing else (`E0495`); the values are computed once per variant, in
declaration order, on first use. Such an enum has no payload variants and no
type parameters, and arguments without a constructor are an error (all
`E0496`). An explicit discriminator stays a C-enum feature: its value is the
variant's integer representation, which only `@layout(c)` pins, and a value
that merely belongs to each variant is what the per-variant field is for.

**Spec status:** JUX-LANG-V1 §7.7.4 and Grammar §A.2.5 state the rules;
Layout-ABI §L.1.3 points from `E0510` to the field form.

---

## E35. What an interface property may declare, and what meets one

**Conflict.** JUX-MISSING-DEFS §M.7.10 let an interface declare property
contracts (`{ get; }`, `{ get; set; }`) and said an implementing type meets one
with "matching properties (or fields with the right visibility)". It never said
which fields count, and it had no default property, although an interface can
give a method a default body and a computed property is the everyday way to
expose derived state (`IsEmpty` from `Size`).

**Resolution.** A contract is met by a property of the same name and type, or a
`public` instance field of that name and type (a non-`final` one for
`{ get; set; }`). `default T Name -> expr;` (or the `get` accessor forms)
declares a read-only default property; a property with a body must be marked
`default`, has no setter and no initializer, and reads the interface's other
properties through `this`.

**Spec status:** §M.7.10 states both rules.

---

## E36. A `while` condition's null test and the loop body

**Conflict.** JUX-TYPE-SYSTEM-ADDENDUM §T.6.2 lists the `if` forms of null-test
refinement and §T.6.5 covers refinements made before a loop, but nothing said
whether `while (x != null)` refines `x` in its own body. The linked-list walk,
`while (cur != null) { use(cur.v); cur = cur.next; }`, was rejected with E0418
on every read of `cur`.

**Resolution.** The condition refines the body until the first statement that
assigns the name (§T.6.3 already ends a refinement at an assignment); the
right side of a direct `x = e;` still reads the refined `x`.

**Spec status:** §T.6.5 states the rule.

---

## E38. There is no `module.jux`

**Conflict.** JUX-LANG-V1 §4.3 and JUX-BUILD-SYSTEM-ADDENDUM §B.3 specified a
`module.jux` declaration file beside `jux.toml`, with its own `version`,
`requires` and `feature` statements, each of which had to agree with the
manifest. Two files described one module, and nothing implemented the second.

**Resolution.** Owner ruling, 2026-09-18: `jux.toml` is the only manifest.
Dependencies, features, version and binaries live there; a symbol consumers
must not see is declared `internal`. `W0210` (an empty module declaration) is
withdrawn with the file.

**Spec status:** §4.3 and §B.3 are rewritten; the bindgen and LSP addenda
point at `jux.toml`.

---

## E39. How a constructor is declared

**Conflict.** JUX-GRAMMAR-ADDENDUM §A.2.4 declared a constructor with the
keyword `new` (`public new(int n) { ... }`), while JUX-LANG-V1 §7.3.1 says the
`new(...)` form was dropped: a constructor takes its class name, as in Java,
and `new` is only the call-site operator. The compiler implemented §7.3.1, and
a `new(...)` declaration produced a cascade that ended in "a class cannot be
declared inside a function body".

**Resolution.** §7.3.1 is normative. §A.2.4's `constructor-decl` names the
class; a `new(...)` declaration is `E0263`, read as the constructor it means.

**Spec status:** §A.2.4 is corrected; §7.3.1 names `E0263`.

---

## E40. The `assert` statement

**Conflict.** JUX-SEMANTICS-ADDENDUM §S.7.2 specified only the built-in call,
`assert(condition)` / `assert(condition, message)`. Java writes
`assert condition;` and `assert condition : message;`, and the IDE already
parsed that form, so the editor accepted a statement the compiler rejected.

**Resolution.** Both spellings are the same built-in: the statement form is
parsed into the call. `assert(x);` stays the call.

**Spec status:** §S.7.2 states the statement form.

---

## E41. Which import cycles are an error

**Conflict.** JUX-BUILD-SYSTEM-ADDENDUM §B.4.6 rejected cycles between "source
modules" and said cyclic imports across packages are "always rejected". It was
written for the `module.jux` model that E38 removed, and it forbade something
Java allows and Jux programs rely on: two packages of one project importing
each other.

**Resolution.** A module is a `jux.toml` package (E38). A dependency cycle
between modules is `E0308`, reported with the whole circle; it cannot build,
since each module is its own library built after its dependencies. Imports
between packages inside one module may be cyclic, as in Java.

**Spec status:** §B.4.6 is rewritten; the catalog row for `E0308` is live.

---

## E42. Sharing a `T[N]` array with a `T[]` slot

**Conflict.** JUX-LANG-V1 §5.5 says `T[N]` passes wherever `T[]` is expected,
and §6.5.2 says an array is a reference: the function receives the caller's
array. The two lower to different storage (a fixed Rust array and a vector),
so the same array under both types cannot exist, and a copy would make the
callee's writes vanish without a word. Every such call failed in rustc.

**Resolution.** A local declared `T[N]` that is handed to a `T[]` slot is
stored as a `T[]` is, keeping its `T[N]` type: §6.5.2 already says `T[N]`
constrains the length, not the storage. Where no single storage serves, the
program is told (`E0468`): one local handed to both a `T[]` and a `T[N]` slot,
or a fixed-size parameter or field handed to a `T[]` slot.

**Spec status:** §5.5 states the rule; E0468 is in the catalog.

---

## E43. A field-initializer lambda that uses its own object

**Conflict.** JUX-OBSERVABLE-PROPERTIES-ADDENDUM §P.2.3 shows an observer
field whose lambda writes one of the object's own properties
(`nameObs = (old, now) -> { Label.Text = now; }`), and LANG-V1 §7.9 lets a
lambda capture `this`. A field initializer runs while the object is being
built, before the shared handle such a capture needs exists, and the Phase 1
lowering had no way to capture it: the program reached rustc.

**Resolution.** Phase-1 restriction, now diagnosed: a field-initializer
lambda that uses `this`, or a bare instance field, property or method of its
class, is `E0981`. Lambdas written in constructors and methods capture `this`
as before. Lifting the restriction needs the object's handle to exist before
its fields are initialized (a cyclic construction).

**Spec status:** the catalog has E0981; §P.2.3 describes the intended
behaviour and stands. Resolved by E139 for every field but an
observer: a field-initializer lambda that uses the object runs against the
finished object; `E0981` is left for an observer's lambda.

---

## E50. A `? super T` argument whose `T` another argument fixes

**Conflict.** JUX-TYPE-SYSTEM-ADDENDUM §T.2.6 has
`copyAll<T>(List<? extends T> source, List<? super T> dest)` take a
`List<Dog>` and a `List<Animal>`: `T` is `Dog`, and the destination holds a
supertype of it. Phase 1 lowers a type parameter to one Rust type, so both
parameters became `Vec<T>` with the same `T`. A `Vec<Dog>` and a
`Vec<Animal>` (whose elements are `Rc<dyn AnimalKind>` handles) are two
different Rust types, and the call reached rustc as a mismatch.

Doing it soundly needs a second, hidden element type for the destination
plus a conversion from `T` to it at every write through the wildcard: the
same upcast an assignment `Animal a = dog;` performs, but threaded through
generic code as a bound (`D: From<T>`) the program never wrote. That is
readable for this one shape and not for the next (a `? super T` handed on to
a second generic function, or a wildcard inside a field), so it is not done
piecemeal.

**Resolution.** Phase-1 restriction, now diagnosed: when a `? super T`
parameter's `T` is fixed by another argument, the argument passed for it must
be a container of exactly `T`; a container of a strict supertype is `E0410`,
naming both types and this entry. Everything else about wildcards stands: a
concrete lower bound (`List<? super Dog>`, no type parameter) takes a
container of any supertype and writes through it, and `? extends` reads work
through a polymorphic base.

**Spec status:** §T.2.6 describes the intended behaviour and stands.

## E56. `Result.ok`/`Result.err` are both statics and instance methods

**Conflict.** JUX-CORE-LIB-ADDENDUM §K.4 lists, on `Result<T, E>`, the instance
methods `Option<T> ok()` and `Option<E> err()` and ALSO the static constructors
`Result<T, never> ok<T>(T value)` and `Result<never, E> err<E>(E error)`. One
type cannot carry an instance method and a static of the same name with one
lowering, and a call `r.ok()` next to `Result.ok(5)` reads as the same member.
Separately, §K.4 and §X.4 write `unwrap() throws E` (an `Err` throws its own
error), but `E` carries no `extends Exception` bound, so a `Result<int, String>`
has nothing throwable to throw.

**Resolution.** Construction uses the variant forms every example in the spec
already uses, `Result.Ok(v)` and `Result.Err(e)`; the two statics are not
provided. The instance `ok()` / `err()` stay. `unwrap()` on an `Err` throws
`IllegalStateException`, as it did before; a `Result` whose error type is an
exception can rethrow it explicitly with a `switch`. The combinators §K.3, §K.4
and §X.4 name are provided as written (`map`, `mapErr`, `flatMap`,
`unwrapOrElse`, `ok`, `err`; `Option.map`, `flatMap`, `toNullable`,
`ofNullable`), plus the everyday companions `orElse`, `filter`,
`isSomeAnd`/`isOkAnd` and `Option.unwrapOrElse`.

**Spec status:** §K.4 should drop the two static constructors and say how
`unwrap` fails for a non-exception `E`.

## E57. The K.5 combinators are on `Iterator` too

**Conflict.** JUX-CORE-LIB-ADDENDUM §K.5 declares the combinators (`map`,
`filter`, `reduce`, `take`, `skip`, `zip`, `chain`, `count`, `any`, `all`,
`firstOrNull`, `minOrNull`, `maxOrNull`) as default methods of `Iterable<T>`
only. §K.5.4 and MISSING-DEFS §M.2 make a generator return an `Iterator<T>`,
not an `Iterable<T>`, so the most natural source of a lazy sequence had none
of them: `numbers().filter(...)` did not exist.

**Resolution.** The same combinators, with the same names and signatures,
are also default methods of `Iterator<T>`, returning `Iterator<R>` where the
`Iterable` form returns `Iterable<R>`. An `Iterator` is single-pass: the lazy
ones pull from their source as they are pulled, and the eager ones use it
up. The `Iterable` forms are built on them and stay re-iterable: each walk of
the result starts a fresh `iterator()` of the source (`LazyIterable<T>`). The
lazy steps are generators in `jux.std.collections.Iterators`.

**Spec status:** §K.5 should list the combinators on `Iterator<T>` as well.

## E58. `seconds(n)` and `milliseconds(n)` are not specified

**Conflict.** JUX-ASYNC-ADDENDUM §18.1.9 and JUX-LANG-V1 §10.1.9 write
`withTimeout(seconds(5), ...)` and `sleep(milliseconds(10))`, and §18.1.4
gives `Task.delay(Duration d)`. No addendum defines `Duration` or those
constructor functions; JUX-GAPS-ROADMAP lists them as a future `std.time`
tier. The standard library is Rust's (Core lib §K.12), which already has a
`Duration`.

**Resolution.** A time span is Rust's `std::time::Duration`, reached as
`import rust.std.Duration;` and built with its own constructors
(`Duration.from_secs(5)`, `Duration.from_millis(10)`). `withTimeout` and
`Task.delay` take either that `Duration` or an integer count of
milliseconds (the form Phase 1 always accepted); anything else is `E0487`.
No free `seconds`/`milliseconds` functions are added.

**Spec status:** §18.1.9 and §10.1.9 should write `Duration.from_secs(5)`.

---

## E61. `sizeof` in safe code, and the shape of `alignof`

**Conflict.** JUX-LANG-V1 §5.9 makes `sizeof(T)` / `sizeof(expr)` a keyword
form that returns a `uint` compile-time constant "usable in any context", and
the compiler has shipped it that way since v0.1 (examples, lessons, the
diagnostics E0461-E0463). Layout-ABI §L.1.5 instead writes the query as a
generic call, `sizeof<T>()`, available only inside `unsafe`, beside an
`alignof<T>()` of the same shape.

**Resolution.** §5.9 stands: `sizeof(...)` is available in safe code. A size
query reads nothing and writes nothing, so gating it behind `unsafe` would
protect nothing while breaking every program that prints a layout. `alignof`
takes `sizeof`'s shape rather than §L.1.5's: `alignof(T)` / `alignof(expr)`,
the same syntactic type-or-value rule (§5.9.3), the same errors (E0461-E0463,
worded for alignment), a `uint` result, lowered to `std::mem::align_of::<T>()`
or `std::mem::align_of_val(&expr)`. `alignof` is a contextual word: it has
this meaning only directly before `(`, so it is not a new keyword.

**Spec status:** §L.1.5 is updated to match; its `sizeof<T>()` / `alignof<T>()`
spelling is withdrawn.

---

## E62. `@align` diagnostics and field alignment

**Conflict.** Layout-ABI §L.1.4 reports aligning down with `E0710`, but
`E0710` was already published for "`throw` of a non-`Exception` value"
(Exceptions §X.2.1), and §D.5.1 forbids reusing a number. The band the new
checks belong in, `E0500`-`E0505`, is permanently reserved for the borrow
checker Jux does not have. §L.1.4 also permits `@align(N)` on a field, which
Rust (the Phase-1 backend) cannot express: Rust aligns types, never
individual fields.

**Resolution.** Two new codes after the band's last published one:
`E0519` for an `@align(N)` that cannot hold (`N` not an integer literal, not a
power of two, above 2^29, or below an alignment a fixed-width field already
needs) and `E0520` for `@align` where it cannot apply (a field, an enum, an
interface). Field alignment is a Phase-1 restriction with a direct
workaround the diagnostic names: an `@align(N) struct` holding the value,
used as the field's type, gives the field that alignment. Type-level
`@align(N)` on a `class`, `struct` or `record` lowers to `#[repr(align(N))]`
(on a class, on the object the shared handle points at).

**Spec status:** §L.1.4 is updated with the codes and the restriction.

---

## E63. `transmute`: which size, and which code

**Conflict.** Layout-ABI §L.7.4 permits `transmute<A, B>` "only when
`sizeof<A>() == sizeof<B>()`" and reports a mismatch with `E0840`. `E0840`
is already published for "const evaluation exceeded its resource limits"
(§T.11.4). The size condition is also stated for "the current target", which
the type checker does not see: a `jux build --target` for a 32-bit machine
changes the size of `int`, `uint` and every pointer. The section's own
example, `transmute<float, uint>(f)`, is wrong on a 64-bit target, where a
`uint` is 8 bytes and a `float` is 4.

**Resolution.** The mismatch is `E0522`, together with a wrong argument count
and a type that has no size fixed on every target. The size check is
*portable*: a transmute must be correct for every target the program can be
built for. The fixed-width primitives and `@layout(c)` aggregates made only of
them have one byte count everywhere; `int`, `uint`, pointers and function
pointers are exactly one machine word and match only each other. A class, a
`String`, a collection or an aggregate holding a pointer has no size fixed
that way and cannot be transmuted. The example becomes
`transmute<float, u32>(f)`.

**Spec status:** §L.7.4 is updated to match.

---

## E64. Exporting a mutable `static`

**Conflict.** Layout-ABI §L.3.3 says mutable `static` items "have a single
global address per process and follow the same rules" as exported constants.
A Jux mutable static is not bare memory: any thread may read or write it, so
it lowers behind a lock (a `LazyLock<Mutex<T>>`, or a thread-local where the
value cannot be shared). C cannot take that lock. Exporting its address would
let C read and write the value while Jux code holds the lock, a data race the
language otherwise rules out.

**Resolution.** Only constants are exported as data: a top-level `const`, or
a `static final` field, of a numeric or `bool` type (C sees it at its C type,
§8.1.1). `@export` on a mutable `static`, on an instance field, or on a
constant of another type is `E0508`, and the message names the alternative:
export functions that read and write the value. This matches how the rest of
the FFI surface already treats shared state (functions cross, lock-guarded
data does not).

**Spec status:** §L.3.3 is updated to match.

---

## E65. Operator coherence without module identity

**Conflict.** Runtime/ABI §R.3 states coherence in terms of *modules*: an
operator may be defined only by the module that owns an operand. Phase 1
compiles a dependency module by including its sources (Build §B, see
`crates/juxc-driver/src/project.rs`), so the checker sees one program and
does not know which module a user type came from. §R.3.3 also detects the
cross-module duplicate (`E0951`) "at the build's link step", which the
source-inclusion model never reaches, and §R.3.6's `E0952` covers free-function
`operator hash` / `operator string`, which the grammar does not accept at all
(a free operator must be arithmetic or bitwise, §7.14, reported as `E0200`).

**Resolution.** Ownership is by *program*: a type is owned when the program
being compiled declares it; primitives, `String`, the standard library,
`rust.<crate>` types and bare type parameters never are. A free operator none
of whose operands, and not its record/struct/enum result, is owned is
`E0950`, with the §R.3.4 newtype escape hatch in the help. Two free operators
with the same operand types are `E0951`, reported where the second is
declared (it used to print as `E0400` with the internal `__op_*` name).
`E0952` stays reserved: an orphan free `operator hash`/`string` cannot be
written. A dependency module's types count as owned by its dependents until
modules compile as separate crates, so an orphan across that line is missed
rather than misreported.

**Spec status:** §R.3.3 and §R.3.6 describe the intended multi-module rule and
stand; the Phase-1 reading above is noted there.

## E66. `move` closures and `E0820` under reference counting

**Conflict.** Type system §T.9.1 lets a closure be written `move () -> ...`
to force a move capture, and §T.9.4 reports `E0820` when an inferred capture
mode would outlive what it borrows. JUX-LANG-V1 §6.4 reserves `move` and
JUX-MISSING-DEFS defers it: `move` as an operator is `E0203` today, and the
`move` closure form rests on the same keyword.

**Resolution.** In the `full` profile a closure never borrows: every capture
is owned by the closure, a class or collection as a shared reference-counted
handle and a value as a copy (LANG-V1 §7.9, §7.9.1). No capture can outlive
what it refers to, so `E0820` has nothing to report and stays reserved, and
`move ()` would change nothing about what a closure holds. Both are left for
the profiles without reference counting (`jux-core`, `jux-embedded`), where
§T.9's borrow-based capture modes are the real model, together with the
`move` operator itself.

**Spec status:** §T.9 describes the borrow-model profiles and stands; the
`full`-profile reading above is noted here.

---

## E71. The `jux new` template's `[module]` table

**Conflict.** Build-system §B.15.1 prints the manifest `jux new` writes with a
`[module]` table (`name`, `version`, `authors`, `license`) and puts `edition`
under `[build]`. Everywhere else the manifest's package table is `[package]`
(§B.2.1, §B.2.2), `edition` belongs to it (§B.2.4), and §B.3 (ERRATA E38) makes
`jux.toml` the only manifest with no separate module declaration. A project
created from the template as printed would load with no package name at all.

**Resolution.** `jux new` writes `[package]` with the template's keys plus
`edition = "2026"`, and keeps the template's `[build]` keys (`profile`,
`target`, `optimization`) and the empty `[dependencies]`. The other three
files (`src/main.jux`, `README.md`, `.gitignore`) are written as printed.
`jux new --lib` adds `[lib]` and puts the library's code in its package
directory (§B.1.1) under a package-less `src/lib.jux`; `jux new --workspace`
writes a `[workspace]` root with an empty `members` list.

**Spec status:** §B.15.1 is updated to match.

---

## E72. The default diagnostic format when nobody is watching

**Conflict.** Diagnostics §D.1.3 makes the multi-line, colored block the
default terminal format and §D.1.4 offers `compact` "for older tooling". Before
those formats existed `juxc` and `jux` printed one line per diagnostic,
`file:line:col: [E0410] error: message`, and that line is what the blessed UI
tests pin, what the IDE's run console turns into links, and what scripts
around the compiler read. Switching every consumer to the block at once would
break all of them without the output being any more correct.

**Resolution.** The default depends on who is reading. On a terminal it is the
§D.1.3 block (`human`), colored unless `NO_COLOR` is set or `--color never` is
given. When stderr is not a terminal (a pipe, a file, a test, an editor) it is
the one-line form, now named `line`. Every format can be asked for by name with
`--diagnostic-format human|compact|short|line|json`, on both `juxc` and `jux`.
`compact` and `short` are exactly §D.1.4 and §D.1.5 (`error[E0410]:` after the
location); `line` keeps its bracketed code so nothing that parses it breaks.
The human block ends with a pointer to `juxc explain <code>` rather than to
the docs URL, because the explanation ships with the compiler (§D.5.3) and the
URL is not served yet; the JSON keeps its `docs_url` field.

**Spec status:** §D.1 carries a Phase-1 note to match.

---

## E79. A guard clause that leaves with `break` or `continue`

**Conflict.** LANG-V1 §7.10 narrows a null-tested name past a guard clause
"only when its branch cannot fall through -- it ends in `return` or
`throw`", and gives the reason: that is exactly when the code after the `if`
is unreachable from the null case. A `break` or `continue` meets the same
reason, but the wording left it out, so the commonest reading loop in the
language was rejected:

```java
while (true) {
    final Command? next = reader.next();
    if (next == null) {
        break;
    }
    run(next);        // E0410: expected Command, found Command?
}
```

**Resolution.** The guard clause counts `break` and `continue` as leaving,
alongside `return`, `throw` and loops that never end. The narrowing still
covers only the rest of the enclosing block, which a jump leaves along with
everything after it. This is separate from missing-return analysis (E0451):
a `break` does not leave the function, so a body ending in one still needs
its `return`.

**Spec status:** §7.10 is amended to name `break` and `continue` and shows
the loop above.

---

## E80. A call on a foreign value whose type the stub never defined

**Conflict.** Bindgen §G.6 has the generated stub describe every foreign
type a program can reach. It does not always: two types from different Rust
modules with the same short name (`str`'s `Split` and `io`'s, `str`'s
`SplitN` and a slice's) collapse into one entry, and the loser is named by
the methods that return it but never defined. `s.splitn(3, '|')` therefore
has a type nothing is known about, and the spec says nothing about what may
be called on such a value.

**Resolution.** The value is still foreign, and is lowered as foreign code:

- A collection type argument is the plain Rust collection, and the result
  arrives behind a Jux handle, so `collect<Vec<String>>()` works.
- A zero-argument call with an explicit type argument is typed as that
  argument. Nothing else could fix the type parameter, and it lets a `var`
  holding the result read as the collection it is.
- Items collected into a `Vec<String>` are owned (`String::from`), because
  every `str` splitter yields borrowed `&str`.
- A `String` slot owns what such a call hands it: `raw.trim()` is a `&str`
  in Rust and the slot is an owned `String`.

The real repair is discovery: the stub should carry both types under
distinct names. Until it does, the rules above keep ordinary text
processing compiling rather than leaking a rustc error about a type the
program never wrote.

**Spec status:** §G.6 stands; this records what the compiler does for a
type it was never given.

---

## E84. `ref` on a type that is already a reference

**Conflict.** `JUX-MISSING-DEFS-ADDENDUM.md` §M.13.2 says `ref` on a
CLASS-typed binding "is accepted and meaningless", reserving warning `W0490`
for it, while §M.13.4 rejects "`ref` array ELEMENTS (`ref T[]`)" outright.
Arrays became reference types after both lines were written
(`JUX-LANG-V1.md` §6.5.2), and collections with them (§6.5.1), so `ref int[]`
and `ref Vec<int>` are the array/collection spelling of exactly the case
§M.13.2 allows.

**Resolution.** One rule for every already-shared type. `ref` on a class,
interface, array or collection binding parses, means nothing extra, and warns
`W0490` naming the type. It is never an error. What §M.13.4 still rejects is
`ref` where a TYPE is expected rather than a binding: `ref ref T` (`E0524`)
and a `ref` generic argument such as `Vec<ref int>` (`E0526`). A `ref` RETURN
type stays deferred, as §M.13.2 says, and is `E0523`.

**Spec status:** §M.13.2 and §M.13.4 carry a note pointing here.

---

## How to use this file

## E76. A second discovery source for `alloc`'s slice and `str` methods

**Conflict.** Bindgen §G.6.1 makes rustdoc JSON *the* source of a foreign
API, and §G.6.4.4 says a `Vec` reaches the methods of the slice it derefs to.
Both cannot hold: `alloc` writes `impl<T> [T]` and `impl str` for primitives
`core` defines, and rustdoc attaches those blocks to `core`'s pages, so no
JSON carries `sort`, `sort_by`, `sort_by_key`, `to_vec`, `concat`, `join` or
`str::to_uppercase`. Checked against the prebuilt `alloc.json`, `core.json`
and `std.json`, and against a JSON built from `rust-src`.

**Resolution.** Those impls are read from the toolchain's own library source
(the `rust-src` component of the toolchain that builds the program), parsed
for inherent impls on a slice or a primitive, keeping the `pub`,
`#[stable]`, `self`-taking methods. Nothing is named by hand: the file is
chosen by the `#[rustc_allow_incoherent_impl]` marker such methods must
carry. The signatures are mapped exactly as rustdoc's are and pooled under
the same shape, so the rest of the compiler cannot tell the sources apart.
Without `rust-src` installed the surface simply lacks those methods.

**Spec status:** §G.6.4.4 carries the rule.

## E77. What a foreign method's bound asks of a Jux type

**Conflict.** `sort` is `where T: Ord`, and nothing in the spec said what
`Ord` means for a Jux type, so `people.sort()` on a `Vec<Person>` reached
rustc as E0277 -- exactly the leak §G.1 exists to prevent.

**Resolution.** A method's bounds on its type's own parameters are carried
on the stub (`@RustBounds("T: Ord")`) and checked at the call as E0446.
The reading follows the backend: `Ord`/`PartialOrd` is `operator<=>`
(LANG-V1 §7.14.4), `Hash` is `operator hash`. A floating-point element meets
`PartialOrd` but not `Ord`, an array or a collection meets neither, and a
foreign element type answers for itself.

**Spec status:** §G.6.4.4 and the E0446 catalog row carry the rule.
## E78. A path segment that is a Jux keyword

**Conflict.** §G.4.2 says a foreign member whose name is a Jux keyword is
surfaced as-is, because a member-name position cannot hold anything else. The
grammar's `qualified-name` (§A.2.1) is `identifier ( '.' identifier )*`, so the
same reasoning was never extended to the segments of an `import` or a
`package` path -- yet those positions are just as unambiguous. A crate with a
module named `record`, `type`, `when` or `drop` was therefore unimportable:
`import rust.x.record;` died at `E0200 expected identifier`, with no way to
write it at all.

**Resolution.** Every segment of a `qualified-name`, and every name in an
`import-item`, is a member-position name: a keyword there is read as a name.
The first segment is not, since a path may begin where a statement may begin.
The four Rust words with no raw form (`self`, `Self`, `crate`, `super`) stay
rejected as before, since the lowered `use` path could not name them.

**Spec status:** §A.2.1 carries the production and §G.4.2 the reasoning.

## E85. A projection over a method's own type parameter is resolved

**Conflict.** Bindgen §G.6.4.2 makes every associated-type projection an
unknown type, on the reasoning that a projection "depends on the call's own
arguments". §G.6.1 makes rustdoc JSON the source of truth for a foreign API,
and rustdoc records exactly what the call's arguments would settle: the
trait's impls and each impl's `type Assoc = ...` binding. The two cannot both
hold. Under §G.6.4.2 alone, `Vec::get`, whose Rust return is
`Option<&<I as SliceIndex<[T]>>::Output>`, surfaced as
`I.Output? get<I>(I index)`, so `v.get(0)` typed as `<unknown>?` for a
`Vec<int>`, a `Vec<string>` and a `Vec<string?>` alike, and the element type
the user asked about never reached the checker.

**Resolution.** A projection whose self type is one of the METHOD's own type
parameters, bounded by the projection's trait, is resolved from that trait's
impls, and the method fans out into one overload per resolvable RESULT: the
impl's self type becomes the parameter, the impl's associated-type binding
becomes the result, and impls that agree on the result are represented by the
simplest of their parameter shapes. `Self::Item` and any other projection
the receiver settles keep the §G.6.4.2 reading. Discovery does all of it,
by rustdoc ID,
and three guards bound it: the parameter type must be a name this stub
declares for that very Rust type, at most four overloads are kept per method,
and an impl that cannot be resolved contributes nothing.

**Spec status:** §G.6.4.5 carries the rule, §G.6.4.2 the narrowed one.

## E86. An unresolved projection is reported, not silently accepted

**Conflict.** §G.6.4.2 says an unresolved projection "takes the declared
slot's type", and §G.1 says a rustc error is always a juxc bug. An unknown
type is the checker's suppression value, so the first rule is implemented by
letting the value satisfy every slot -- including a slot it does not fit,
which is how `show(v.get(0))` with `show(string? st)` and a `Vec<string?>`
compiled clean in juxc and failed in rustc. The second rule says that outcome
is never acceptable.

**Resolution.** The two cases are separated at their source rather than in
the assignability rule. The `null` literal keeps its unknown-inner nullable
type and keeps fitting every `T?` slot; a call to a foreign method whose
declared return is an unresolved projection over one of the method's own type
parameters is `E0469` at the call, naming the method and the projection. The
suppression reading of an unknown type is untouched everywhere else, since
narrowing it broadly would turn every other deliberately-unknown type into an
error.

**Spec status:** §G.6.4.6 and the E0469 catalog row carry the rule.

---

## E87. `spawn` runs on the event loop, not on a thread pool

**Conflict.** The async addendum's design stance is "One built-in event
loop, no pinning, no `Send`/`Sync` in user code, no runtime to choose", and
§18.1.3 makes `spawn` the only way to obtain a `Task<T>`. The backend lowers
`spawn` onto a work-stealing thread pool instead, emitting
`pub fn __jux_spawn<T: Send + 'static>(..)` and scheduling through
`SpawnExt::spawn_with_handle(&mut &*__JUX_TASK_POOL, ..)`. Every Jux class
lowers to `Rc<RefCell<..>>` (CLASS-REPRESENTATION §CR), which is not `Send`,
so spawning any function that returns or captures a class fails:

```
error[E0277]: `Rc<RefCell<User_Inner>>` cannot be sent between threads safely
```

Classes are the reference type of the language, so this takes the whole
`Task<T>` surface with it: `Task.completed`, `Task.failed`, `Task.yield`,
`Task.all`, `Task.any`, `isCancelled`, `isResolved`, `map`, `flatMap` and
`parallel(items, f)` are all unreachable in the shape a program actually
uses them. The `Send` bound also leaks Rust's thread model into a surface
the stance says it must never reach.

**Resolution.** The stance governs. `spawn` schedules onto the single
built-in event loop, and a spawned computation carries NO `Send` bound: a
task is concurrent with its siblings, not parallel to them, exactly as the
addendum describes suspension. `Rc<RefCell<..>>` is therefore a legal thing
for a task to hold and to hand back. A future profile that wants true
parallelism has to say so and carry its own rule; it cannot be the default
the stance already fixed. This is separate from `Worker.spawn`
(MISSING-DEFS §M.12), which IS a thread boundary and keeps its `Send`
requirement and its E0702 diagnostic.

Two consequences the compiler owes alongside it: the `Task` receiver is
type-checked like any other (an unknown member is a Jux diagnostic, not a
leaked `error[E0423]: expected value, found struct `Task``), and `parallel`
lowers to the `Task.all` composition §18.2 already defines for it.

**Spec status:** the stance in the async addendum head and §18.1.3 stand as
written; this records that the lowering, not the spec, was wrong.

---

## E88. What a `try` statement leaves definitely assigned

**Conflict.** JUX-SEMANTICS-ADDENDUM §S.4.6 says "a `try` body may abort
partway through, so only assignments in a `finally` block survive to the
statements after it". Read literally that discards the try body's assignments
even when the body ran to the end, which makes the ordinary shape of a
constructor that converts one failure into another illegal:

```java
public Feed(String text) throws FeedError {
    try {
        this.doc = from_str<Value>(text);       // E0600: `doc` not assigned
    } catch (Exception e) {
        throw new FeedError("<document>", "the feed is not JSON");
    }
}
```

There is nothing to rewrite it into: the assignment has to be inside the
`try`, and a `finally` cannot assign a value the failing call never produced.

**Resolution.** A `try` statement leaves assigned what every branch that can
COMPLETE NORMALLY leaves assigned: the body, and each `catch` whose own body
falls out of the bottom. A branch ending in `throw` or `return` imposes
nothing, exactly as §S.4.6 already says of any such path. A `finally` runs on
every path and adds its own assignments on top. When no branch can complete
normally the whole statement cannot either, and the code after it is
unreachable. A `catch` still starts from the state at the `try`, since it may
run from any point inside the body; that part of the old rule is why the body
being able to abort matters at all.

This is Java's rule (JLS 16.2.15) and the same soundness argument the rest of
§S.4.6 makes: the value is read only on paths that wrote it.

**Spec status:** §S.4.6 carries the rule.

## E89. What `==` means between a `T?` and a `T`

**Conflict.** JUX-LANG-V1 §7.10 lists `==`, `!=`, `===` and `!==` among the
operators that "take a null by design", so a comparison with a nullable
operand is not `E0418` and the checker accepts it. What it MEANS when only one
side is nullable is not written anywhere, and the backend, having no rule to
follow, compared an `Option<T>` with a bare `T` -- a rustc `E0308` reported
against the user's line.

```java
long? recorded = ledger.checksumOf(job.name);
print(recorded == job.run());       // leaked rustc E0308
```

**Resolution.** A `T?` equals a `T` when it HOLDS that value, and a null
equals nothing: `null == v` is false and `null != v` is true, for every `v`.
The comparison is total, so it has no null case to guard and cannot throw. The
two sides must otherwise be the same type; mixed numeric widths promote first,
by the ordinary rules, and an unrelated type is `E0410` as it always was.

`===` and `!==` (identity) answer the same way: a null is the identity of
nothing.

**Spec status:** §7.10 carries the rule.

---

## E90. A `ref` binding initialized from a plain local

**Conflict.** `JUX-MISSING-DEFS-ADDENDUM.md` §M.13.2 says "Initializing a
`ref` binding from a plain `T` value creates a NEW shared object holding
that value", while `JUX-POINTERS-REFERENCES-GUIDE.md` §2.2 works the same
declaration out the other way round:

```jux
int total = 0;
ref int acc = total;     // acc aliases total
acc = acc + 5;           // total is now 5
```

The compiler implemented the first sentence, so the guide's own worked
example printed `0` where the guide says `5`, with no diagnostic. A `ref`
aliased only when its source was ALREADY a `ref` cell, which is why
`examples/ref_bindings.jux` passed: every `ref` in it is initialized from
a literal or from another `ref`, never from a plain local.

**Resolution.** The two sentences are about different initializers, and
the distinguishing word in §M.13.2 is *value*. A `ref` binding
initialized from a **variable** of the enclosing body (a local or a
parameter) ALIASES that variable: one object, two names, and every write
through either name is seen by both. A `ref` binding initialized from
something that is not a variable (a literal, a call result, an arithmetic
expression, a `new`) has nothing to alias, so it creates a fresh object
holding that value, exactly as §M.13.2 says. Initializing from another
`ref` binding keeps aliasing it, as before.

Three neighbouring positions are deliberately unchanged:

- **An argument.** Passing a plain value to a `ref` parameter still wraps
  it in a fresh object, so the callee's writes stay invisible to the
  caller (§M.13.2, and `examples/ref_bindings.jux` pins it). An argument
  list is not a binding, and Jux has no call-site `ref` with which to
  mark the one argument that should alias.
- **A field.** `ref int s = p.counter;` still copies `counter` unless the
  field is itself declared `ref`. A local's storage is chosen by the body
  that declares it, so that body can promote it; a field's storage is
  fixed by its class for every instance of it, so only the field's own
  declaration can make it shared.
- **`ref` on an already-shared type** stays the W0490 case (E84).

**Lowering.** A local or parameter named as the initializer of a `ref`
binding is PROMOTED, for the whole body, to the same `Rc<RefCell<T>>`
slot the `ref` binding uses: its declaration wraps, its reads clone out
and its writes store through. That is the machinery a closure-captured,
reassigned local already uses, so the promoted variable and the `ref`
binding end up the same kind of slot and an `Rc` clone aliases it.

**Spec status:** §M.13.2 carries the rule and points here.

---

## E91. Who owns an observer that was passed to a function

**Conflict.** `JUX-OBSERVABLE-PROPERTIES-ADDENDUM.md` §P.2.4 says an
`observer<T>` may be declared "field, local, parameter", and §P.3.2 says
`.observers.attach(o)` registers it. §P.2.3 says a property holds its
observers WEAKLY, because "if the observer's OWNER is dropped, the
observer silently stops firing". Neither section says who the owner is
when the observer reached the attach through a parameter, and both
answers the compiler could give are wrong for the other case:

```jux
class M { public int V { get; set; } = 0; }
void wire(M m, observer<int> o) { m.V.observers.attach(o); }

public void main() {
    var m = new M();
    observer<int> o = (old, now) -> { print("p " + now); };
    wire(m, o);
    m.V = 3;
    print("size=" + m.V.observers.size);   // said 0
}
```

`wire` attached a weak reference to its parameter, which held the only
strong reference left, because the call MOVED `o` out of `main` at its
last read. The parameter died with `wire`'s frame and took the
attachment with it. The program compiled, ran, printed `size=0`, and
never fired. Writing the lambda in the argument list, or letting `var`
infer its type, failed the same way.

**Resolution.** Two rules, one at each end.

- **An observer binding is a handle.** Reading one shares it, exactly as
  reading a class or a collection does; it is never moved out of the
  binding that names it. A binding declared `observer<T>`, and any
  argument that fills an `observer<T>` parameter, therefore leaves its
  owner in place. This is what §P.2.3's weak reference assumes, and the
  cleanup story it promises only works when it holds.
- **An observer with no other owner is owned by the property.** Weak
  stays the rule, and it is right whenever something else holds the
  observer. When nothing else does, a weak attach would be dead on
  arrival, so the property holds it strongly instead. The attach site
  cannot see the difference between `wire(m, o)` and
  `wire(m, (old, now) -> ...)`, since both arrive as a parameter, so the
  handle answers for itself: exactly one strong reference means this is
  the only one there is.

The second rule is what an inline `attach((old, now) -> ...)` already
did by hand (§P.2.2's examples would attach nothing otherwise). It is
now the general rule rather than a special case at one syntactic site,
which is what makes the same lambda work one call deeper.

**Spec status:** §P.2.3 carries both rules.

---

## E92. What `task.cancel()` does to the task it cancels

**Conflict.** `JUX-EXCEPTIONS-ADDENDUM.md` §X.7.3 says cancellation "is"
an exception: it "propagates the same way, runs `finally` blocks, runs
drops, and so on", and JUX-LANG-V1 §10.1.9 says a cancelled task stops at
its next `await`. The Phase-1 lowering instead DROPPED the task's
`RemoteHandle`, which drops the running future outright. A dropped future
never resumes, so no `finally` block of a cancelled task ever ran and no
Jux `drop` block ever ran, while the program compiled and ran without a
word. That makes `cancel()` unsafe for any task holding a resource, which
is the one thing §X.7.3 promises it is not.

**Resolution.** Cancellation is cooperative, as §X.7.3 describes it.
`task.cancel()` sets a flag the task shares with its handle and returns
at once; it drops nothing. Where the task next RESUMES, which is where an
`await` hands control back, it throws
`CancellationException("task was cancelled")`. From there it is an
ordinary Jux exception: it unwinds the task's body, its `finally` blocks
run in order, its values drop, and it is what `await task` re-throws at
the awaiter.

Two consequences the spec did not state, and now does:

- **A cancelled task reports nothing.** Whatever a cancelled task fails
  with, its own `CancellationException` included, never reaches the
  unhandled-rejection hook of §10.1.8. Cancelling is the caller saying it
  no longer wants the result, so there is no rejection left to be
  unhandled. The Phase-1 handle-drop achieved this by accident, and
  `examples/unhandled_task_failure.jux` pins it.
- **A task that never awaits again is never interrupted.** Cancellation
  has effect only at a resumption point, so a task that has already
  finished, or that runs to its end without suspending, completes
  normally. §X.7.3's "the next `await`" is the whole of the contract.

**Not `withTimeout`.** That is a different mechanism, a race whose loser
is DROPPED (`JUX-ASYNC-ADDENDUM-v2.md` §18.1.9). The timed-out work is
dropped, not thrown into, and Phase 1 keeps the distinction: a `finally`
inside work that runs out of time still does not run.

**Spec status:** §X.7.3 carries the rule.

---

## E93. A static initializer that depends on itself

**Conflict.** `JUX-SEMANTICS-ADDENDUM.md` §S.4.2 said two things about a
cycle between static initializers. First, "cycles are detected at compile
time when `A -> B -> A` is statically determinable", without saying what
detection then does. Second, "at runtime, the second entry into a
partially-initialized class returns the current (partial) state, the same
trap Java has", and "cycles within a single module are linted (`W0530`)".

Neither half described the compiler. `W0530` was reserved and never
raised, and the runtime half cannot hold for the Phase-1 lowering: a
static lowers to a `LazyLock`, and re-entering a `LazyLock`'s initializer
does not return a partial value, it blocks forever. So

```jux
class A3 { public static int X = A3.X + 1; }
public void main() { print(A3.X); }
```

checked clean, built, printed nothing and never exited.

**Resolution.** A cycle in the static-initializer dependency graph that
the compiler can determine statically is a hard error, **`E0497`**,
naming the cycle. It is not a warning, and there is no runtime fallback.

The reasoning, recorded because it overrides §S.4.2's own words:

- The partial-state read §S.4.2 offers is a silent wrong answer, and
  Phase 1 cannot even produce it. An initializer cycle has no value that
  is right, so there is nothing to warn about and carry on with.
- A warning would leave the program that provoked it hanging forever with
  no output, which is worse than any error.
- The graph is over declarations the compiler already has, so a
  statically determinable cycle is exactly the case where an error costs
  the programmer nothing to fix.

**What "statically determinable" covers.** One node per static field that
has an initializer, and an edge from a field to every static field its
initializer reads, following the bodies of the static methods the
initializer calls. A dependency only a runtime value can reveal, such as
one reached through a virtual call or a function value, stays outside the
graph and stays §S.4.2's runtime trap.

`W0530` is retired: it was reserved for this check, and this check is an
error. The number is not reused (§D.5.1).

**Spec status:** §S.4.2 carries the rule, and §D.4 lists `E0497` and
marks `W0530` retired.

---

## E94. `parallel`, and how its two forms are told apart

**Conflict.** The async addendum §18.1.4 defines exactly one `parallel`:
"`parallel(items, f)` is equivalent to
`Task.all(items.map(it -> spawn(() -> f(it))))`", with a worked example that
fans a `Vec<int>` out over an async function. The compiler implemented a
different call entirely, `parallel(a, b, c)` over futures, lowering it to
`async move { futures::join!(a, b, c) }` and resolving with a tuple. That form
is in no addendum, and the example corpus depends on it
(`examples/stress_async_parallel.jux`, `examples/stress_async.jux`,
`examples/stress_async_advanced.jux`, `examples/stress_async_concurrency.jux`).

So the one form the spec DID define was the one that did not work. The
arguments of the fan-out form were lowered as if they were futures:

```jux
var out = await parallel(ids, id -> twice(id));
```

```
async move { futures::join!(ids, std::rc::Rc::new(move |id| twice(id))) }.await
error[E0277]: `Rc<JuxCell<Vec<isize>>>` is not a future
```

A collection is not a future and a lambda is not a future, so the program
could not be written at all, and what it got back named types it never wrote
(§G.1: a rustc error is a juxc bug).

**Resolution.** Both forms exist, and the SHAPE of the call decides which one
is meant, never the types of the arguments:

- **Fan-out** is exactly two arguments whose second is written as a
  one-parameter lambda or as a method reference. Neither spelling can be a
  future, so no legal join call can be read as a fan-out. A first argument that
  cannot be iterated at all is `E0941`, the rule §O.7.3 already puts on a
  for-each.
- **Join** is everything else: every argument is a future or a task, and the
  result is their tuple.

A bare function NAME in the second position stays the join form. A name may
hold a task, so `parallel(first, second)` is ambiguous on its face, and the
tie goes to the form that has always had that shape. `x -> f(x)` and `Type::f`
are the two ways to say the other thing.

Fan-out lowers to the composition §18.1.4 already gives it, with the two
details a collection's reference-type representation (§6.5.1, §6.5.2) forces:

- The collection HANDLE leaves expression position. The elements are
  snapshotted out of the cell before the work starts, the way a for-each over a
  handle already snapshots, so the borrow does not span the fan-out and the
  caller keeps its handle.
- The joined `Vec` goes back INTO a handle. `parallel` hands back an ordinary
  Jux collection of `f`'s results, in the order the items were iterated, so
  everything a `Vec<R>` can do it can do. A plain Rust `Vec` answers reads, and
  even a `push`, for as long as it stays in the one binding it was assigned to;
  what it cannot do is be a second name for the same collection, which §6.5.1
  says every Jux collection is.

An empty `items` resolves with an empty collection, having run nothing.

`Task.all` and `Task.allSettled` had the second half of the same defect and are
fixed with it: each resolved with a plain Rust `Vec`, so reads worked and
`var b = a;` on the result reached rustc as
`error[E0382]: borrow of moved value` -- a move where every other Jux collection
aliases. Both now hand back a collection handle, and the checker types them
`Task<Vec<..>>` so the uses of one are emitted for what it is.

Per E87 this is concurrency: every task shares the one event loop, so the
fan-out overlaps waiting rather than computation, whatever the name suggests.
`Worker.spawn` (§18.2) remains the thread boundary.

**Spec status:** §18.1.4 of JUX-ASYNC-ADDENDUM-v2, and the same paragraph in
JUX-LANG-V1 §10.1.4, carry the rule.

---

## E95. The for-each binder could not be `final`

**Conflict.** `JUX-GRAMMAR-ADDENDUM.md` §A.2.8 wrote the enhanced `for` as

```text
for-each-stmt     = 'for' '(' ( 'var' | type ) identifier ':' expression ')'
                    statement
```

with no slot for a modifier, and the parser matched the production exactly.
So a TYPE in the header was fine and `final` was not:

| header | before |
|---|---|
| `for (String? note : notes)` | accepted |
| `for (var note : notes)` | accepted |
| `for (final var note : notes)` | `[E0200] error: expected identifier` |
| `for (final String? note : notes)` | `[E0200] error: expected identifier` |

Java accepts `for (final String s : list)`, and §A.2.2 already makes `final`
and `const` synonyms on any binding, a loop variable included. The omission
was therefore not a decision about the language: nothing in the addenda says
the for-each binder is the one binding that may not be `final`. It was a gap
in one production, and what it cost was a Java habit that reads correctly and
means something. Writing `for (final String? note : notes)` over a
`Vec<String?>` had to be downgraded to `var`, which says less.

**Resolution.** Both productions take an optional `binding-modifier`
(`final` | `const`) before the `var`-or-type, and the modifier carries its
ordinary meaning: the loop variable may not be reassigned inside the body.
The diagnostic for a reassignment is **`E0464`**, the existing `final`-binding
error (§M.14.2), and not a new code. Reusing it is the point: a `final`
for-each binder is an ordinary `final` binding, and nobody should have to
learn a second error number to be told the same thing.

Two consequences worth stating, because a modifier that parsed and meant
nothing would be worse than the parse error it replaces:

- **The modifier is enforced, not merely parsed.** The checker's `final` walker
  seeds the loop body's scope with the binder when it is `final`, where before
  it always removed the name. An unmodified binder still shadows, and so
  un-finals, an outer `final` of the same name.
- **Nothing about the iteration changes.** The binder is a fresh binding on
  every turn with or without the modifier, so `final` adds a promise about the
  body and no cost to the loop.

**Spec status:** `JUX-GRAMMAR-ADDENDUM.md` §A.2.8 carries both productions and
the prose; `JUX-ASYNC-ADDENDUM-v2.md` §18.6.3 carries the `for await`
spelling.

---

## E96. A user type may be named `T`, `String` or `Exception`

**Conflict.** Two rules in the spec pull against each other and neither one
said which wins. `JUX-LANG.md` §4.4 scopes a package-private type to its own
package, and the addenda describe an implicit prelude that makes `jux.std.*`
and `rust.std` reachable from any unit without an `import`. Nothing said how a
bare single-segment name is resolved when more than one package declares it,
and nothing said what the STANDARD LIBRARY's own units see.

Read one way, the prelude is a workspace-wide bare-name index, and a program
that declares `class T` has added `T` to it. That reading is what shipped, and
it is not survivable. This whole program

```java
class T { public int v = 1; }
public void main() { print(new T().v); }
```

produced 60 errors, every one of them pointing inside a `jux.std` source file
the author cannot open, beginning with `E0416 cannot use package-private type
T from jux.std.collections`. `class String` produced 81, `class Vec` 10,
`class Exception` 8. `class Foo` and `class K` were fine, so the failure did
not even read as a rule: it read as the compiler breaking on some names and
not others.

**Resolution.** Bare-name resolution is **unit-scoped**, and `jux.std.*` plus
the generated `rust.*` stub packages form a **library realm** that never
resolves a bare name to a user declaration. The ladder and the realm rule are
written out in `JUX-MISSING-DEFS-ADDENDUM.md` §M.16.

Five consequences, stated because the alternative reading of each one is what
the compiler used to do:

- **No name is reserved.** Java lets a program declare `String`, `T`,
  `Iterator` or `Exception`, Jux is a Java-shaped language, and a diagnostic
  that told the author to rename would be the wrong answer to a question the
  compiler should never have asked. The collision is resolved per unit, not
  forbidden.
- **A parameter beats a top-level function of the same name, in every body
  kind.** Default interface method bodies did not record their parameters, so
  a program's own `void pred(String? s)` re-shaped the arguments of
  `jux.std.collections.Iterator.any`'s `pred` parameter and took the build
  down inside the standard library, without the program ever calling `pred`.
  §M.16.2.
- **An `implements` name means what it means in the package that WROTE it.** A
  user `interface Iterable` used to answer for
  `jux.std.collections.LazyIterable implements Iterable`, so the program was
  told the standard library had an unimplemented method (`E0429`), and with
  that silenced the emitter wrote an empty trait impl and rustc said the same
  thing again (`E0046`).
- **A name that names a TYPE is a type, never a constant.** `const int
  Exception = 3;` rewrote the type `Exception` to `3` wherever it appeared,
  including `pub fn addSuppressed(&mut self, e: 3)` inside `jux.std.exceptions`,
  which rustfmt could not even parse. A constant substitutes for a name only
  when no type of that name is in scope.
- **`package jux;` is legal.** The emitted Rust nests a Jux package as a Rust
  module, so a package named `jux` lands beside the emitted `jux::std` tree;
  the emitter now pins `std` back to the crate in every module it writes so
  `std::rc::Rc` still means Rust's. §M.16.3.

Visibility is unchanged for user code: a package-private type in one user
package is still `E0416` from another (§4.4). What changed is that a user
package is no longer a candidate at all when the unit doing the resolving
belongs to the standard library.

**Spec status:** `JUX-MISSING-DEFS-ADDENDUM.md` §M.16 carries the resolution
ladder, the library realm, the parameter-shadowing rule and the `package jux;`
note.

---

## E97. A Rust type that is not `Clone + Debug` could not be held at all

**Conflict.** §G.3 and the whole `rust.<crate>` design say the Rust standard
library IS the Jux standard library: a program stores what it opens, and the
types it stores are Rust's. Nothing in the addenda says a foreign type must
implement any particular trait to be held by a class field or a record
component.

In practice it had to implement two. Every Jux aggregate lowered to a struct
carrying `#[derive(Clone, Debug)]` (a record also `PartialEq` and `Default`),
and the only question asked before dropping a derive was whether the field's
type came from a crate OTHER than `std`, on the assumption that every
`rust.std` type is `Clone`. 139 of the 296 types in the std stub are not,
`std::fs::File` and `std::net::TcpStream` among them, so:

```jux
import rust.std.File;
class Wrap {
    public File f;
    public Wrap(File f) { this.f = f; }
}
```

```text
error[E0277]: the trait bound `File: Clone` is not satisfied
1150 | #[derive(Clone, Debug)]
     |          ----- in this derive macro expansion
```

A `record Holder(File f)` got three of those at once (`Clone`, `PartialEq`,
`Default`). There was no way to write the declaration differently, and no
diagnostic: the rustc error named a derive the program never wrote. "Rust is the
standard library" was not true of any type you would actually want to hold onto.

**Resolution.** What a foreign type can derive is DISCOVERED and recorded, like
every other fact about a foreign type. bindgen reads `Clone`, `Debug`,
`PartialEq` and `Default` off the type's real impl list and stamps
`@RustClone`, `@RustDebug`, `@RustPartialEq`, `@RustDefault` on its stub
declaration (Bindgen §G.6.4.7). An aggregate derives each trait only when every
type it holds carries the matching marker, recursing into generic arguments and
exempting the two positions whose lowering does not ask (a shared array or
collection handle for `Clone`/`Default`, a `T?` for `Default`). The test is now
the same for `std` and for every bound crate, and it names no type.

**A dropped `Debug` is a dropped derive, not a dropped trait.** Printing must
not depend on one member: an interface's trait carries a `std::fmt::Debug`
supertrait, `throw` formats its payload, and a container of the values prints
through `Debug`. So the impl is written out instead: the name for a class
(which prints through its own `Display` anyway, so nothing is lost), and the
derive's own shape for a record with each component rendered through the
universal show helper.

That helper gains a **bottom tier**: a value whose type has neither `Display`
nor `Debug` prints as its own type name in angle brackets, `<TcpStream>`. This
is the choice worth justifying, and it was taken over a placeholder invented at
each emit site for two reasons. It is ONE rule in one place, so an aggregate, a
collection, an interpolation and a generic all render such a value the same way.
And it makes the helper total: `__jux_show!` now renders every Rust value there
is, which is what the "one Display-or-Debug helper" design was for. The
alternative, keeping `Debug` mandatory and reporting a diagnostic, would have
been a rule the language does not need, since naming the type says everything
there is to say about a value that cannot describe itself. One piece of
fallout is worth naming: a Jux FUNCTION value lowers to an `Rc<dyn Fn(..)>`
with neither trait, so interpolating a lambda now prints `<fn>` where it used
to be a rustc error about a trait the program never wrote.

**Known boundary: a Jux generic's type argument.** A generic Jux declaration
lowers its parameters with a `Clone + std::fmt::Debug + 'static` bound, since
reads of a generic-typed member auto-`clone()` and the declaration's marker
trait carries a `Debug` supertrait. A type without `Clone` cannot be that
argument, so `Cell<File>` over `class Cell<T> { public T value; }` still does
not compile while `Vec<File>` does. Removing that bound is a separate change
and not a small one: the core library's own `Iterable.reduce<U>` calls
`initial.clone()` in its body, so the bound is load-bearing well beyond the
aggregate rule this entry settles. Until then the limit stands as written.
(Narrowed by E118: a standalone generic class now moves the bound to
the members that need it, so `Cell<File>` compiles. Closed by E120 for
every declaration shape: a parameter keeps the bound only for a stated reason,
and a use that needs it is `E0457`.)

**Spec status:** `JUX-BINDGEN-ADDENDUM.md` §G.6.4.7 carries the markers and
their discovery; `JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.5.8 carries the
aggregate rule, what a dropped derive costs, the printing rule and the generic
boundary.

---

## E98. A function type over a polymorphic class had no stated lowering

**Conflict.** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.3.6 makes a function type an
ordinary type: `A` is assignable to `B` when `A` is "a function type compatible
with `B`'s function type (contravariant in parameters, covariant in return)".
Nothing there or in `JUX-INHERITANCE-BORROW-ADDENDUM.md` §6.9.6 said how a
function type's parameter and result slots relate to the `Rc<dyn <Name>Kind>`
handle a polymorphic base-typed slot lowers to. The two halves were lowered by
different rules, and the seam showed as raw rustc errors in a program the spec
declares legal:

```jux
class Animal { public int w = 1; }
class Dog extends Animal { }
void use((Animal) -> int f) { print(f(new Animal())); }
public void main() { use((Animal a) -> a.w); }
```

| piece | lowered as | wanted |
|---|---|---|
| the parameter `f` | `Rc<dyn Fn(Rc<dyn AnimalKind>) -> isize>` | (correct) |
| the lambda's `Animal a` | `move \|a: Animal\|` | `move \|a: Rc<dyn AnimalKind>\|` |
| the argument `new Animal()` | `Animal::new()` | `Rc::new(Animal::new()) as Rc<dyn AnimalKind>` |

`rustc` reported `E0631` for the first mismatch and `E0308` for the second.
Deleting `class Dog` made the identical program compile, because with no
subclass `Animal` is no longer a polymorphic base and both sides agree again on
the concrete struct. That is what made this worth an entry rather than a bug
report: the failure appears when a SECOND class is added, in a file that need
not mention the callback, and it took every callback, visitor, strategy and
event-handler API over a class hierarchy out of the language.

**Resolution.** A function type's parameter and result slots are value slots,
so a polymorphic base named in either is the `Rc<dyn <Name>Kind>` handle,
exactly as the same class named anywhere else is. Three conversions follow from
that one rule, and the implementation performs all three:

- **A lambda's written parameter type is the slot's type.** `(Animal a) -> a.w`
  declares `a` as the handle. A parameter type the program wrote and one it left
  to be inferred lower to the same closure, which is the property that was
  missing: the inferred form already worked.
- **An argument passed through a function VALUE converts into the function
  type's parameter**, the way an argument to a declared method does. This holds
  for every holder of the value: a local, a parameter, a field, and a call that
  returns a function. The checker records a function-typed callee's type at the
  callee's own span so one lookup serves them all.
- **A lambda result converts into the function type's result slot**, which the
  existing return-upcast path already did.

The handle is not an implementation convenience here. A `(Animal) -> String`
callback is called with a `Dog` and has to reach `Dog`'s override, so lowering
the slot to the concrete struct would slice the subclass away at the call and
silently run the base body.

**Spec status:** `JUX-INHERITANCE-BORROW-ADDENDUM.md` §6.9.6 carries the rule
and its three consequences, beside the `Kind`-trait block it belongs to.
`examples/fn_type_polymorphic_callbacks.jux` is the gated program.

---

## E99. "Every Jux type can satisfy the added bounds" is not true

**Conflict.** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.2.1 lists the bounds the lowering
adds to a type parameter from how the parameter is used, and closes with a
promise about the whole table:

> **A type argument must satisfy the bounds its parameter acquired.** Every Jux
> type can [...] a rule the compiler applies on the user's behalf must never
> make a legal type argument illegal.

Two rows break the promise, and they break it for opposite reasons. Both used to
leak a raw rustc error out of a program whose Jux text says nothing about Rust
traits.

**1. `Eq + Hash`, for a key that is not a key.** A parameter used as a
`HashMap` / `HashSet` key acquires `Eq + Hash`:

```jux
class Store<K> {
    public HashMap<K, int> m = new HashMap<K, int>();
}
public void main() { Store<double> s = new Store<double>(); }
```

gave, on `Store::<f64>::new`,

```text
error[E0599]: the associated function or constant `new` exists for struct
`Store<f64>`, but its trait bounds were not satisfied
note: the following trait bounds were not satisfied:
      `f64: Eq`
      `f64: Hash`
```

while the literal
`HashMap<double, int>` one line away gave a clean `E0933`. Here §T.2.1's promise
is not the thing that is wrong: `double` cannot be a `HashMap` key at all
(`JUX-OPERATORS-ADDENDUM.md` §O.3.1), so `Store<double>` is an illegal PROGRAM
and the bound is reporting a real error, in the wrong language.

**Resolution (1).** The rule the hash analysis already states, "a type
parameter is assumed hashable where it is declared, and the instantiation is
what gets checked", is now actually checked. A written
instantiation of a user generic whose parameter the declaration uses as a
`HashMap` / `HashSet` key reports **`E0933`** at the type ARGUMENT, in the same
words the literal container gets, naming the class and the parameter. The set of
parameters checked is the same set the lowering turns into the bound (a
parameter named, bare, as argument 0 of a hashed container anywhere in the
declaration's own field, property, constructor-parameter, method-parameter and
return types), because a bound the backend adds that the checker does not know
about is a leak by construction.

**2. `Display`, for a nullable argument. OPEN.** A parameter whose values reach
a format position acquires `Display`, so

```jux
class Box<T> { public T v; public Box(T v) { this.v = v; } public String show() { return "" + v; } }
public void main() { Box<int?> b = new Box<int?>(null); print(b.show()); }
```

still gives

```text
error[E0599]: the associated function or constant `new` exists for struct
`Box<std::option::Option<isize>>`, but its trait bounds were not satisfied
note: trait bound `std::option::Option<isize>: std::fmt::Display` was not satisfied
```

`int?` is a legal Jux type and a legal type argument, so this is the promise
being broken outright. It is recorded here as OPEN rather than resolved,
because the obvious fix does not work:

- The bound cannot simply be dropped. The universal renderer
  (`crate::__jux_show!`) picks `Display` or `Debug` by autoref specialization,
  and that choice is made ONCE, in the generic body, against the parameter's
  declared bounds. Inside `class Wrap<T>` with only `T: Debug` in scope it
  therefore takes the `Debug` arm for every instantiation, including the ones
  whose argument does have a string form, so `Wrap<Cell<String>>` prints
  `wrap(Cell { value: "in" })` instead of `wrap(Cell(value: in))`. The bound is
  what makes the renderer resolve per instantiation, which is what
  `examples/generic_declarations_print.jux` gates.
- `Option` is a foreign type, so the emitted crate cannot give it a `Display`
  impl (Rust's orphan rule), and a local trait with a blanket impl over
  `Display` plus an impl for `Option<T>` does not cohere either.

What would close it is making `Debug` canonical for Jux values, so that the
renderer's `Debug` arm produces the same text `Display` would: a hand-written
`Debug` on every class, record and enum that prints the type's string form
(§O.7.1), after which a formatted parameter needs nothing beyond the universal
`Debug` and the `Display` row can go. Until then, §T.2.1's closing paragraph
overstates what the table guarantees, and this entry is what a reader should
trust.

**Resolution (2), 2026-09-25: closed by E107,** which does exactly that.

**Spec status:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.2.1's closing paragraph is
corrected to point here. `tests/ui/generic_hash_key_arg.jux` pins the `E0933`.

---

## E100. Members reached through a wildcard over a CLASS bound

**Conflict.** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.4.8 says a class bound emits as
that class's marker trait `<Name>Kind`, and that "when the bounded value's
members are used, the marker trait ... carries those methods so they resolve".
Its worked example is a GENERIC class (`V: ContainerKind<K>`), and the
implementation read the generic-ness as part of the rule: only a class with type
parameters got a member-carrying marker. §T.4.8 also said nothing about FIELDS,
and nothing at all about what a value reached through a wildcard formats as.

An INTERFACE bound was never affected: an interface already is a Rust trait, so
every member reached through it resolves. Over a CLASS bound these five
programs, all of them accepted by §T.4.6 and §T.4.8 on paper, came out like
this:

| program | before |
|---|---|
| `Vec<? extends Animal>`, read `it.nm`, `Animal` HAS a subclass | `error[E0609]: no field 'nm' on type '__W0'` |
| the same, call `it.nm2()`, `Animal` HAS a subclass | worked |
| `Vec<? extends Animal>`, read `it.nm` and call `it.nm2()`, `Animal` has NO subclass | `E0609` **and** `error[E0599]: no method named 'nm2' found for type parameter '__W0'` |
| `n += a.nm2().length()` | `error[E0214]` from emitted `a.nm2().len() as isize()`, which is not Rust |
| `Vec<? super Dog>`, `print(v[0])` | printed `Animal(RefCell { value: Animal_Inner { nm: "a" } })` where a plain `Vec<Animal>` printed `Animal@0x…` |

The second row is the one that gives the shape of the defect away. Whether a
field read through `? extends Animal` compiled depended on whether some OTHER
class in the program extended `Animal`, which is nothing the reading code can
see. And the last row is a silent wrong answer: no diagnostic, no leak, just a
handle's derived `Debug` where the value's own string form belonged.

**Resolution.** Four rules, each of them about the same thing: a bound names a
class, so the surface reachable through it is that class's surface.

1. **A class bound's marker trait carries the class's member surface, and
   generic-ness is not part of the METHOD test.** `<Name>Kind` declares every
   public, non-static, non-abstract instance method that carries no type
   parameters of its own. Methods with their own type parameters stay off it,
   since `<Name>Kind` is also the erasure a storage-position wildcard uses
   (`Box<dyn AnimalKind>`) and a generic method would cost that.

   Two shapes were already covered and keep their own path: a **polymorphic
   base** carries its virtual surface on the same trait for `dyn` dispatch, and
   a **generic** class in bound position parameterizes the trait by its own
   params. What this adds is the plain case, a class with no type parameters and
   no subclass, which had an empty marker.

   A class with no type parameters also gets a `__get_<f>` / `__set_<f>`
   accessor pair for every non-private instance field, spanning the whole
   `extends` chain: such a trait stands alone, nothing makes it a subtrait of
   the parent's marker, so an inherited field read through the bound would have
   nowhere else to resolve. Accessors are scoped this way for a reason, not by
   oversight. Their bodies read the field through the shared handle at a fixed
   `__parent` depth, which needs the class to use that representation at all,
   and a GENERIC class's inherited field types are written in its parent's
   type-parameter vocabulary, which the bound surface does not substitute
   through. So reading a field through `? extends Container<int>` is still
   unsupported; calling a method through it is not. (Closed since by
   E115, which substitutes through the `extends` chain.)

   The one class none of this applies to is a base that is extended but is not a
   polymorphic base (a `sealed` one): its subclasses emit
   `impl <Parent>Kind for <Child> {}` with no bodies, so a populated parent
   trait there would leave required methods unimplemented.

2. **A producer is READ as its bound.** Member resolution on a
   `? extends B` value runs against `B`, so `a.nm2()` is typed by `B`'s
   declaration and `a.nm2().length()` is the String method it looks like.
   Without this the chain typed as unknown, and an unknown receiver is what
   sent row four into the array-`length` intrinsic. A **consumer**
   (`? super B`) is deliberately not peeled: PECS says it may be written and
   not read, so there is no member surface to resolve against, and peeling it
   would invent one.

3. **`length` is the array and collection FIELD form, never a method name.**
   `xs.length` is a field (`JUX-LANG-V1.md` §6.5.2) and `s.length()` is a String
   method (`JUX-CORE-LIB-ADDENDUM.md` §S.3.2); both spellings were already in
   the spec and the implementation conflated them, so the field intrinsic
   claimed any `length()` whose receiver type it could not resolve and wrote
   `.len() as isize`, after which the enclosing call appended `()`. The
   intrinsic now declines callee position, and a `length()` there is an ordinary
   method call.

4. **A `? super B` over a Jux class carries `Display`.** The lift's `From<B>`
   bound says what the callee may WRITE and nothing about what the element is,
   which is why the universal renderer had no `Display` to pick and fell back
   to `Debug`. When `B` is a Jux class the bound can state the one thing true of
   every element a caller may supply: `B` and every ancestor of `B` is a Jux
   class, and every Jux class emits an identity `Display`
   (`JUX-OPERATORS-ADDENDUM.md` §O.4.1). So the added bound rejects no caller
   that was accepted before, and `print(v[0])` reads exactly as it does over a
   concrete `Vec<Animal>`. A foreign or interface bound gets nothing added: a
   `Vec` is not `Display`, and inventing the bound there would reject the
   caller.

One consequence worth stating, because it is what makes rule 1 reachable at
all: a **wildcard** over a class puts that class in bound position even though
the source never spells it as a type parameter. `void f(Vec<? extends Animal>)`
is lifted to `fn f<__W0: AnimalKind>(…)`, so the set of bound-position classes
is read from signature types as well as from `generic-params` lists.

`examples/wildcard_bound_members.jux` covers all five rows of the table above,
and `examples/wildcards.jux` now reads `xs.head.name` through the bound instead
of documenting the inability to.

**Spec status:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.4.8 carries the bound-surface
rule, the producer-read rule and the `? super` `Display` rule.

---

## E101. A sealed class hierarchy is a reference hierarchy, not an enum

**Conflict.** `JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.5.6 says `sealed` does
not affect the representation: it "is about inheritance, not memory layout".
One cell of the Java-fidelity table in §CR.4.1 said the opposite,
"sealed→enum for closed hierarchies", and the compiler followed the cell. A
sealed base that declared no field and no static initializer became a Rust
`enum` of its permitted subclasses, and under that lowering the subclasses were
plain structs rather than the shared handles every other class gets.

That is not a spelling detail. It gives one class hierarchy VALUE semantics
while every other has reference semantics, and the foundational commitment of
this addendum (§CR.4.1) is that a Jux class is a shared mutable reference the
way a Java object is. What it cost:

| program | before |
|---|---|
| `Shape a = new Circle(); Shape b = a; b.grow();` | `a` did not see the change |
| `shapes.push(new Circle())` into a `Vec<Shape>` | rustc `E0308`, "try wrapping the expression in `Shape::Circle`" |
| a mutating `@Override` reached through a `Shape` | rustc `E0596`: the `&self` match dispatcher cannot call a `&mut self` override |

`JUX-LANG-V1.md` §7.4.2's worked example is a sealed `Shape` whose
`AnimatedRectangle.render()` mutates and is called through a `Shape`-typed
parameter, and §15's capstone holds a `sealed abstract class Animal` in a
`Vec<Animal>`; those are rows two and three of that table. A sealed base that
declared even one field already escaped the enum lowering and worked, which is
why the gap survived this long: `examples/sealed_shapes.jux` only ever touched
concrete locals.

**Resolution.** §CR.5.6 is canonical and the table cell was wrong. A sealed
class lowers exactly as any other polymorphic base class does, `Rc<dyn
<Name>Kind>` per §CR.5.1, whether or not it carries state. `sealed` contributes
closed-set knowledge to the FRONT END only:

- exhaustiveness of a `switch` over the base, which needs no `default` when the
  arms cover every permitted subclass (`E0440` otherwise, §T.5.5), and
- the exact mutation union of §7.4.2, because only a permitted subclass can
  contribute an override.

Neither is a fact about layout, so neither earns a distinct representation. The
enum lowering, its variant-wrapping upcasts and its match-dispatch wrappers are
removed rather than left unreachable.

**What a `case` pattern over a sealed hierarchy means.** §A.3 says a
`Name(part, …)` pattern is a record pattern or an enum pattern according to what
`Name` resolves to, and a sealed subclass is neither; the form was nonetheless
accepted, because under the enum lowering it fell out of Rust's own match for
free. It keeps working and is now written down: over a sealed hierarchy,
`case Sub(part, …)` tests the runtime type and binds part `i` to `Sub`'s `i`-th
INSTANCE field in declaration order, `_` skips a part, and a literal part is a
test on that field rather than a binding. `case Sub s` and `case Sub(var x)`
remain two spellings of the same runtime-type test, as §7.5 already has them
for records.

**Spec status:** `JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.4.1's table and
§CR.5.6 now agree; `JUX-GRAMMAR-ADDENDUM.md` §A.3 carries the subclass-pattern
rule; `JUX-LANG-V1.md` §7.5 carries the prose.

---

## E102. An aliased or qualified library type is not shadowed by a user class

**Conflict.** `JUX-MISSING-DEFS-ADDENDUM.md` §M.16.1 settled that a program may
declare a type with a name the standard library also uses (ERRATA E96), and
pointed at the alias import and the fully-qualified name as the way to reach the
library's type from the same unit. What it did not say is that a decision made
about a type has to be made about THAT type. The compiler resolved the name
correctly and then asked every following question of the name's last segment, so
a program's own root-package `class Vec` answered for `rust.std.Vec`:

```java
import rust.std.Vec as RVec;
class Vec { public int mine = 1; }
public void main() {
    Vec a = new Vec();
    RVec<int> b = new RVec<int>();
    b.push(5);
    print($"alias: ${a.mine} ${b.len()}");
}
```

```text
error[E0599]: no method named `push` found for struct `Rc<JuxCell<std::vec::Vec<isize>>>`
error[E0599]: no method named `len` found for struct `Rc<JuxCell<std::vec::Vec<isize>>>`
```

The two halves of one decision disagreed. The SLOT was decided from the name the
author wrote, `RVec`, which is not a user type, so it took the §6.5.1 shared
handle. The member CALL was decided from the resolved name's last segment,
`Vec`, which is a user type, so it declined the handle and emitted `push` with
no `.borrow_mut()` guard. Removing the `class Vec` line made the same program
compile, which is the shape of a shadow test being asked the wrong question.

Beside a user class the qualified form worked only because BOTH halves declined
the handle, so the disagreement cancelled: `rust.std.Vec<int>` was measured as
`Vec` in type position too. Two wrongs, and no way to tell from the outside.

**Resolution.** §M.16.6. A resolved name keeps its package for every later
question, and the shadow test is a question about a simple name: an alias import
and a qualified name both say which type is meant, so a same-named user
declaration does not shadow either. Concretely:

- a genuinely bare name still resolves to the user's class, in the root package
  and in a named one;
- a user class in a NAMED package does not make the library type of the same
  last segment look like a user type, and does not stop being a user type
  itself;
- a nested type is keyed `Outer__Inner` and written `Outer.Inner`, so the
  nested-type shadow test stays a question about a simple name; a
  package-qualified name is never a nested one.

This is a representation and routing rule, not a new restriction: no name is
reserved, and nothing tells an author to rename (§M.16.1).

**Two reads followed.** This entry fixed the member CALL and deliberately left
an INDEX read and a `@RustRefOut` read alone, because both failed through an
alias with NO user class present and so had a cause of their own rather than a
shadow test asking the wrong question. They did, and it was this rule one step
earlier: the backend lowered a local's written type with no unit context, so the
alias `RVec` (the last segment of nothing) resolved to no type at all, `v[0]`
came out with no `.borrow()` (rustc E0608) and `d.front() ?? 0` with no
`.cloned()` (rustc E0308), while the slot itself still carried the handle. The
index's KEY shape was a third site of the original mistake, measured by last
segment, so `qmap["three"] = 33` beside a program's own `class HashMap` was
emitted as an `Index` store instead of an `insert` (rustc E0594). All three are
closed, under this entry and §M.16.6: no new rule was needed.
`examples/stdlib_alias_reads.jux` runs the matrix with no collision in sight and
`examples/stdlib_alias_collisions.jux` runs it beside one.

**Spec status:** `JUX-MISSING-DEFS-ADDENDUM.md` §M.16.6 carries the rule.

---

## E103. A `[[bin]]` entry file is not a member of the package tree

**Conflict.** `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.15.2 draws the canonical
multi-binary project and §B.1.1 derives a file's package from its directory
under `src/`. Read together they contradict each other, and the shape §B.15.2
documents as ordinary did not build at all:

```
src/
├── lib.jux                     # shared code
├── main.jux                    # primary binary
└── bin/
    ├── server.jux              # additional binary
    └── migrator.jux            # additional binary
```

```text
src/bin/server.jux:1:1: [E0301] error: missing `package` declaration: this file
  is in `bin`, so it must declare `package bin;`
src/bin/migrator.jux:1:1: [E0301] error: missing `package` declaration: this
  file is in `bin`, so it must declare `package bin;`
src/main.jux:3:8: [E0400] error: `main` is declared more than once at the top level
src/bin/server.jux:3:8: [E0400] error: `main` is declared more than once at the
  top level
```

Two independent causes, both from the same unstated assumption: that every `.jux`
file under `src/` is a member of the package tree.

The `E0400` pair is the `[lib]` target, which compiled the whole `src/` tree
with no entry filter and so put all three `main`s in one library crate. §B.15.2
said nothing about what each target compiles, and the lib target is built
whenever no single `--bin` was selected, so the failure came before any binary
was reached. Each `[[bin]]` target already excluded its siblings' entry files,
which is the same rule the lib target needed.

The `E0301` pair is §B.1.1's derivation applied to a file whose location the
manifest already states. Nothing imports `bin.Server`: the `[[bin]] path` key is
how that file is found, and `package bin;` would be a package name no `import`
could ever usefully name. §B.1.1's own justification for the rule, that a public
type must be findable from its name, does not reach an entry point.

**Resolution.** §B.1.1. A `[[bin]] path = "…"` entry file is a program's entry
point, not a member of the package's type tree, and may be package-less wherever
it sits under `src/`. Only the requirement is lifted: a package it does declare
is still checked against its directory, the exemption is per file rather than per
directory (an ordinary source beside an entry in `src/bin/` still declares
`package bin;`), and the dotted form `[[bin]] main = "xss.it.Main"` states the
entry's package in the manifest, so that file must declare it.

§B.15.2. A target compiles the package's shared code plus at most one entry
file: the `[lib]` target excludes every `[[bin]]` entry, a binary excludes the
other binaries', and an example excludes all of them and brings its own `main`.
More than one entry in one target is `E0400`, which is the rule two `main`s in a
single binary's own source still meet.

One consequence had to be settled to make the fixed shape usable: because the
targets of a package do not compile the same source list, a diagnostic's file
index means nothing outside the target that produced it, and a type error on line
5 of `src/bin/server.jux` was printed against `src/com/example/myapp/Greeter.jux`
line 6. A package's build now translates each target's indices onto one list
before reporting, and reports a diagnostic every target produces (an error in the
shared code) once rather than once per target.

**Spec status:** `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.1.1 carries the entry-file
rule and §B.15.2 the per-target source lists.

---

## E104. Annotations on parameters and locals: two targets with no parser

**Conflict.** `JUX-ANNOTATIONS-ADDENDUM.md` §A.3 lists `PARAMETER`
("Function/method parameters") and `LOCAL_VARIABLE` ("Local variable
declarations") among the eight `@Target` kinds, and §A.12 makes an
out-of-target application `E0470`. `JUX-GRAMMAR-ADDENDUM.md` §A.2.4 spells the
parameter production with annotations in it
(`param = annotation* param-mode? type identifier ('=' expression)?`), and
§A.2.8 admits `annotation+ statement`. Every one of §A.11's three framework
patterns (routing, injection, ORM) annotates a parameter.

The parser implemented neither position, and the failure did not say so:

```java
annotation Note { String value(); }
void f(@Note("p") int x) { print(x); }
public void main() { f(1); }
```

```text
error[E0200]: expected identifier
error[E0200]: expected ')' to close parameter list
error[E0200]: expected '{' to start block
error[E0200]: expected expression
error[E0411]: `f` expects at most 0 arguments, got 1
```

Not one of those five names an annotation. The parameter list stopped because
`@` cannot begin a type, the cascade then tore the function in two at its own
`{`, and the last line is an arity complaint about a function whose only
parameter the compiler had already thrown away. An author reading that has no
way to learn that the `@` was the problem.

A second, quieter gap: §A.2.8's `annotation+ statement` admits an annotation
before ANY statement, while §A.3 names only the local declaration. Nothing said
what `@Note("x") print(1);` meant.

**Resolution.** §A.3.1. A parameter's annotations precede its binding mode and
carry the target `PARAMETER`, in every parameter list (function, method,
constructor, operator, interface method). In statement position the annotations
apply to the statement that follows, and because `LOCAL_VARIABLE` is §A.3's only
statement-level target, that statement must be a local variable declaration;
anything else is `E0470` naming the statement kind. A lambda parameter takes no
annotation, because §A.2.9 spells `lambda-param = type? identifier`.

`@cfg` is excluded from both positions and is `E0470` there: statement-level
conditional compilation is `if cfg(...)`, and a parameter cannot be compiled out
of a signature its callers already wrote. Stated explicitly because a `@cfg`
that parsed and was then dropped would read as if it were selecting code, which
is worse than the parse error it replaced.

Both targets are validated (`E0470`, `E0472`, `E0473`, `E0474`) and neither is
recorded in the §A.8.0 registry, whose `kind()` is one of `class`, `method`,
`field`, `function`. A parameter or local has no row, so a `RUNTIME`-retention
annotation there is checked but not reachable from `jux.meta.Registry`.

**Spec status:** `JUX-ANNOTATIONS-ADDENDUM.md` §A.3.1 carries the rule.

---

## E105. `@entry` was a documented annotation with no implementation

**Conflict.** `JUX-ENTRY-POINTS-ADDENDUM.md` §E.2 has specified `@entry` since
the addendum was written: it marks a function as the program's entry regardless
of its name, one per binary, with `symbol` and `convention` arguments and four
diagnostics of its own (`E0321`, `E0322`, `E0324`, `E0325`). None of it existed.
The name appeared in exactly one place in the compiler, the built-in annotation
allow-list that keeps `W0241` quiet, so the annotation parsed, landed in
`FnDecl.annotations`, and was read by nothing. Entry selection was by the NAME
`main` alone, in the backend, in five places.

The result was not a missing feature but a rustc leak, which is the failure mode
this compiler is least allowed:

```java
@entry public int my_start() { return 0; }
```

```text
error[E0601]: `main` function not found in crate `repro`
```

The program was accepted by every juxc phase, emitted a crate with no entry
point at all, and died inside `cargo build` pointing at a line of generated
Rust. Three of the four diagnostics §E.6 allocates for `@entry` were not even
declared in `juxc-diagnostics`.

§E.2 was also silent or over-promising in four places that an implementation has
to answer:

- the worked example's signature, `(int argc, String[] argv)`, is a C-ABI entry
  shape the hosted runtime cannot hand arguments to;
- nothing said what happens when one binary contains both a `main` and an
  `@entry` function;
- nothing said where `@entry` may be written, so on a class method it was
  silently nothing;
- `convention` and the paired-entry sets of §E.2.3 describe code generation this
  milestone does not have.

**Resolution.** §E.2. `@entry` selects the entry point, and the name `main` is
the default only when no `@entry` exists in the binary. Concretely:

- the accepted signatures are exactly §E.1.2's set for `main` (`void`/`int`,
  no parameters or one `String[]`/`String...`, optionally `async`, optionally
  `throws`), and the async / args / exit-code entry wrappers that used to key on
  the name key on the selection instead;
- any other signature, and `@entry` on anything but a free function, is `E0324`;
- both an `@entry` and a `main` entry in one binary is `E0320`, not a silent
  precedence, so the `main` that would never run cannot be mistaken for the
  program's start;
- a second `@entry` in the binary, in any file, is `E0321`;
- `@entry(symbol = "...")` publishes the symbol through the same mechanism
  `@export(name = "...")` uses, as an additional linker-visible name beside the
  hosted entry, never a rename;
- `@entry(convention = "...")` is `E0322` unless it names `c`, because the
  compiler emits no convention attribute and a silently wrong ABI is not a
  diagnostic anyone gets to see;
- `E0325` (`freestanding = true` with no `@entry`) stays unimplemented with the
  rest of §E.3, and §E.6 now records which codes are live.

**Spec status:** `JUX-ENTRY-POINTS-ADDENDUM.md` §E.2, §E.2.1, §E.2.2, §E.2.3
and §E.6 carry the rules.

---

## E106. `jux.toml` was specified as required and validated as optional

**Conflict.** `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.2.1 draws the minimum-viable
manifest as three keys, §B.2.2 marks all three REQUIRED, §B.2.3 gives
`package.name` a regex and §B.2.4 says v0.1 supports only edition `"2026"`.
Nothing was checked. Every one of these passed `jux check` with no output at
all:

```toml
[package]
name = "MyApp"          # not lowercase, not reverse-DNS, §B.2.3's regex rejects it

[nonsense]              # no such table; nothing reads it
whatever = 1
```

```toml
[package]
name = "com.example.app"
edition = "2015"        # §B.2.4: v0.1 supports only "2026"
```

A missing `version`, a missing `edition` and a missing `[package]` table were
equally silent, and a misspelt `[depedencies]` behaved exactly like a correct
table that happened to be empty.

Worse, a manifest that was not valid TOML was an uncoded `eprintln!` warning
and a `None` return from the loader, so every caller read a typo in `jux.toml`
as "this directory has no manifest": the build continued with the defaulted
name `app`, version `0.0.0` and no dependencies, and the first thing the user
saw was an unresolved import from a dependency they had declared. The module
doc on the loader stated the tolerance as deliberate, which is why the decision
is recorded here rather than simply implemented.

Three things had to be settled: which conditions are errors, which are
warnings, and where the codes come from. §B.2.2's flat REQUIRED answers none of
them, because a key that is required of a *new* manifest is not therefore worth
refusing to build an *existing* project over.

**Resolution.** §B.2.5 is the check table, and the rule behind it is: **a key's
absence is a warning when the default is the only value it could have had; a
present value the compiler cannot honour is an error.**

- `E0901` -- `jux.toml` unreadable or not valid TOML. A manifest the compiler
  cannot read is not one it may guess at, and the loader now reports the two
  cases apart: no file is still `Ok(None)`, a bad file is an `Err`.
- `E0902` -- no `[package]` table (and no `[workspace]` either), or a
  `[package]` with no `name`. `name` is the one key with no defensible default:
  it is what a consumer writes in its own `[dependencies]`, the default package
  path for every file under `src/`, and the stem of the emitted artifact's
  name. The old default, `app`, silently built a package nobody could import
  under the name they wrote. A `[workspace]`-only virtual manifest declares no
  package and is exempt, which is the shape `jux new --workspace` writes.
- `E0903` -- a `name` holding a character §B.2.3's grammar forbids, a `version`
  that is not SemVer 2.0, an `edition` other than `"2026"`. `edition = "2015"`
  names a language this compiler does not implement; compiling it as 2026
  anyway would be a substitution the user never gets to see.
- `W0901` -- `version` or `edition` absent, defaulted to `0.0.0` and `"2026"`.
  Both are REQUIRED of a manifest being written today and neither is worth
  breaking a project that built yesterday: `"2026"` is the only edition that
  exists, so the assumption cannot be wrong yet, and the version matters at
  `jux publish` rather than at `jux build`. One warning per manifest names
  every key it is missing, because two lines per package across a workspace is
  noise nobody reads.
- `W0902` -- an unknown top-level table or `[package]` key. A manifest is
  forward-compatible by design (`[publish]`, `[package.sign]` and the
  `[ffi.<name>]` sub-tables are all specified ahead of their implementations),
  so an unknown key may not be an error; but it may not be silent either.
- `W0903` -- a dotted `name` whose first segment, the reverse-DNS root, holds
  an `_` (`my_co.app`). Illegal characters are the error; the wrong shape is
  the warning. A SINGLE segment is not warned about, and §B.2.3's regex now
  admits one: §B.15.1's own `jux new myapp` template writes `name = "myapp"`,
  so a warning there would fire on the first build of every project the tool
  creates. Reverse-DNS remains the convention for a library meant to be
  published.

`jux new` and `jux init` used to write the directory name verbatim, so
`jux init` in a directory called `clean-init` wrote a manifest the very next
command refused (`E0903`). Both now write the spellable segment: `clean_init`,
and `app_3d` for `3d`.

The repository's own manifests were brought up to the table rather than the
table down to them: 98 of them predated `edition` and now declare it, and
`examples/apps/matrix_lab`'s `lab` and `linalg` became `apps.lab` and
`apps.linalg`, the prefix the other apps already use.

The codes sit in `E0900`-`E0999` beside the whole-build errors that band
already reserves for the driver (`E0905` cannot-resolve-dependency, `E0908`
linkage-unavailable, both still unraised),
per the note added to `JUX-DIAGNOSTICS-ADDENDUM.md` §D.3. The manifest is
reported as a source file of its own, with the offending line and a caret, so
`jux.toml:3:8` is as clickable as any `.jux` diagnostic.

**A second contradiction, in the same area.** §B.15.2's table says what each
*target* of a package compiles, and E103 added it, but it said nothing about
what a *dependency* contributes to a dependent. The dependency loader took the
whole `src/` tree, entry files included, so a package that path-depended on a
package having any `[[bin]]` -- including the `src/main.jux` a bin package gets
by default -- failed with the dependency's `main` reported against the
dependent's own entry file:

```text
src/main.jux:3:8: [E0400] error: `main` is declared more than once at the top level
```

§B.15.2 now carries the dependency row: a dependency contributes its shared
code minus all of its own `[[bin]]` entry files, which is exactly what it
compiles into its own `[lib]`. The same function also collected the doc items
for `jux doc`, which is why the obvious one-line filter was wrong: documentation
is about the package's source, not about a compiled target, so `jux doc` keeps
documenting `src/main.jux` and reads through a separate entry point that says so
in its name.

**Spec status:** `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.2.5 carries the validation
table, §B.15.2 the dependency source list, and `JUX-DIAGNOSTICS-ADDENDUM.md`
§D.3 / §D.4 the codes.

---

## E107. `Debug` is a Jux value's string form, so no parameter needs `Display`

**Conflict.** E99 part 2, left OPEN: a type parameter whose values reach a
format position acquired `Display` (§T.2.1), so `Box<int?>` did not compile,
because `Option` is foreign and the emitted crate may not give it a `Display`.
Dropping the bound alone is wrong, for the reason E99 records: the universal
renderer `__jux_show!` chooses `Display` or `Debug` once, in the generic body,
against the declared bounds, so with only `T: Debug` in scope
`Wrap<Cell<String>>` printed `wrap(Cell { value: "in" })`.

Two neighbouring defects had the same root, a derive list or a trait answer
written without asking the payload's own type:

- **Gap 9.** The checker's `user_type_has_default` answered `true` for every
  foreign type, so `new Holder[3]` over `record Holder(File f)` passed Jux and
  failed in rustc ("no associated function named `default`"). E97's
  `@RustDefault` marker already said otherwise.
- **Gap 10.** An `enum` still derived `Debug, Clone, PartialEq` unconditionally,
  so a variant holding a `rust.std.File` was two rustc errors. E97 fixed the
  same list for classes and records only.

**Resolution.** A Jux class, record and enum no longer derives `Debug`. Each
gets a hand-written `impl Debug` that writes its string form (§O.7.1): through
its own `Display` when that impl needs no bound the `Debug` impl lacks, through
its `operator string` when the user wrote one, and as the bare type name when
`operator string` is deleted, so a deleted operator leaks no payload. Whichever
arm the renderer takes, a Jux value now renders the same text, and the
`Display` row of §T.2.1 is removed from every declaration kind: class,
interface, record, enum, free function and method.

What `Debug` still cannot render the Jux way is a FOREIGN value reached through
a parameter. Three kinds matter, and the run-time helper `jux_debug_text`
recognises each by the value's type NAME, which is the real instantiation's
even where the trait choice was not: a float (`1e21` becomes `1.0E21`), a string
or char (the quotes and escapes come off), and an `Option` (`None` becomes
`null`, `Some(5)` becomes `5`, recursively). The last is what makes `Box<int?>`
print `Box(null)`.

Gap 9: a foreign class or enum has a default exactly when its stub carries
`@RustDefault`, so `new Holder[3]` is reported as `E0458`. Gap 10: an enum's
derive list goes through `foreign_derives_of` like a class's, dropping `Clone`,
`PartialEq` and the traits that depend on them (`Eq`, `Copy`) when a payload's
type lacks them, and `cases()`, which clones, is emitted only when `Clone` is.

A visible side effect, and an intended one: a record or class printed INSIDE a
collection used to come out in Rust's struct syntax (`[R { a: 1, b: "y" }]`) and
now comes out in its Jux form (`[R(a: 1, b: y)]`).

**Formerly open, closed by E112.** A nullable INSIDE a collection
printed Rust's form (`[Some(3), None]`): the collection's own `Debug` writes its
elements, and the name-based normalization only saw the outer type.

**Spec status:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.2.1 loses its `Display` row
and its closing paragraph now points here for the nullable argument. `JUX-
OPERATORS-ADDENDUM.md` §O.7.1 is unchanged: it already says what a value's
string form is, and this entry only makes `Debug` produce it.

---

## E108. Record components and lambda parameters: the last two annotation positions

**Conflict.** E104 gave parameters and locals their annotations and left two
positions behind, one grammatical and unparsed, one ungrammatical and badly
refused.

`JUX-GRAMMAR-ADDENDUM.md` §A.2.5 has always spelled
`record-component = annotation* type identifier`, and an enum payload variant
reuses that `record-component-list`. The parser read a component with
`parse_type_ref` straight away, so an annotation there was the same five-error
cascade E104 describes for parameters. And §A.3 had nothing to say about the
position once it parsed: its eight targets have no `RECORD_COMPONENT`, and a
component is not any one of the others. It is written once and becomes two
declarations, a field and the canonical constructor's parameter.

A lambda parameter is the opposite case. §A.2.9 spells
`lambda-param = type? identifier`, and E104 decided that a lambda parameter takes
no annotation. The decision was right and the refusal was not:

```java
var f = (@Tag int a) -> a + 1;
```

```text
error[E0200]: expected identifier
error[E0200]: expected ';' after `var` declaration
error[E0301]: cannot find `int` in this scope
error[E0200]: expected ';' after expression statement
error[E0301]: cannot find `a` in this scope
...
```

The lookahead did confirm a lambda; `parse_lambda` then stopped at the `@`, and
everything after it was read as statements.

**Resolution.** §A.3.2 is new. A record component takes annotations, and it
takes the ones that fit where it lands, which is Java's rule: an annotation is
admitted when its `@Target` names `FIELD` or `PARAMETER` (or it names no target
at all). There is deliberately no `RECORD_COMPONENT` target, because nothing
could be recorded against one that the field and parameter targets do not
already say. `METHOD` is not admitted because a Jux record generates no
accessor method. `@cfg` is `E0470` there, as on a parameter. An enum payload
slot is under the same rule rather than a second one, since the grammar gives
both positions one production.

A `RUNTIME` annotation that fits a field is recorded in the §A.8.0 registry as a
`field` row owned by the record, as it would be on a class field; one that fits
only `PARAMETER` has no row, like any parameter annotation. An enum payload
slot's annotations are checked and not recorded.

A lambda parameter's annotation is one `E0470`, "a lambda parameter takes no
annotation", per parameter. The parser reads past the annotations and parses the
parameter and the lambda normally, so nothing else is reported.

**Gap 14, closed with no change.** `GAPS.md` recorded that
`check_annotation_applications` did not recurse into nested types. It does not
need to: the parser lifts every nested declaration into the unit's items under
its owner-qualified name (§M.9), and has since before the annotation check was
written, so a nested type's annotations were always checked.
`tests/ui/annotation_target_nested_type.jux` now pins it.

**Spec status:** `JUX-ANNOTATIONS-ADDENDUM.md` §A.3 points at the new §A.3.2,
and §A.3.1's lambda paragraph states the one-diagnostic refusal.
`JUX-GRAMMAR-ADDENDUM.md` §A.2.5's `record-component` line cross-references
§A.3.2; the productions are unchanged.

---

## E109. Three diagnostics that said something untrue

**Conflict.** Grammar A.2.2 makes `final` and `const` synonyms and says the
compiler echoes the spelling that was written. E95 did that for `E0464`, the
local binding, by carrying a `FinalKw` on locals and parameters. Its sibling for
fields, `E0465`, still answered every final field with "it is a `final`/`const`
field ... Drop `final`/`const`", because `FieldDecl` kept only a `bool`.

`E0305`, raised for the four Rust words with no raw-identifier form, ended with
"Every other reserved word is fine". That is true of Rust's reserved words and
false of Jux's: `ref`, `move`, `yield` and the rest of Jux's own keywords stay
reserved (`JUX-GRAMMAR-ADDENDUM.md` §A.4.1.1 says so in as many words), so the
message invited exactly the rename that would fail next.

`E0203`'s catalog row listed `annotation` among the reserved-but-unimplemented
keywords, and its doc in the code table listed `yield`. Both have productions
now; the only live sites are `move` and `volatile`.

**Resolution.** `FieldDecl` and the symbol table's `FieldSig` carry the written
`FinalKw` beside `is_final`, as `VarDecl` and `Param` already did, and `E0465`
names it: "it is a `const` field ... Drop `const`". An auto-property's backing
slot records no keyword, since none was written. `E0305` lists the four words,
says a Rust-only word such as `loop` is escaped and fine, and says Jux's own
keywords stay reserved, citing §A.4.1.1. `E0203`'s row and prose name `move`
and `volatile` only, and say where `yield` and `annotation` went.

**Spec status:** `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4's `E0203` row and the
"Reserved But Not Implemented" section are corrected in place.

---

## E110. Lint levels, `-Werror`, and `W0820` under `jux check`

**Conflict.** `JUX-DIAGNOSTICS-ADDENDUM.md` §D.5.4 specified a `[lints]` table
(`warnings-as-errors`, `all`, a level per lint) and an `@lint(allow = ...)`
attribute, and §D.1.2 said a warning "does not fail unless `-Werror`". None of
it existed. `[lints]` was not among the manifest's known tables, so E106's
validation answered the spec's own example with `W0902` ("nothing reads it"),
which was true. Neither `jux` nor `juxc` had a `-Werror`.

§D.5.4 was also silent or wrong where an implementation has to answer: it spoke
of `L####` lint codes that no catalog allocates, named three lints of which the
compiler raises one, said nothing about precedence between the table and the
attribute, nothing about which package's table governs a dependency's files,
and nothing about what a denied warning looks like once reported.

Separately, `W0820` (an `unsafe` block without `// SAFETY:`) was documented as
raised by `juxc --check`, `jux check` and the editor, and `jux check` did not
raise it: the checking entry point `jux` uses is the compile path, and only the
editor's path called the lint. So "deny unjustified `unsafe`" could not be
expressed anywhere, not even as a warning on the command line.

**Resolution.** §D.5.4 is rewritten to what is built:

- Every warning is a lint. A key is a lint name or a warning code (`W0820`);
  there are no `L####` codes, and a name is an alias for its code.
  `unsafe-without-justification` is `W0820`; `unused-import` and
  `shadowed-name` are specified and not raised yet, and setting them is a
  `W0902` that says so rather than an error.
- Precedence, innermost first: the nearest enclosing `@lint`, the package's
  per-lint key, its `all`, then `warn`; `warnings-as-errors` and `-Werror` /
  `--deny-warnings` then promote whatever is still a warning. `allow` beats
  `-Werror`.
- A promoted warning keeps its `W` code (as rustc keeps a lint's name under
  `#[deny]`), is reported as an error, and carries a note naming the setting
  that promoted it.
- Each package's table governs its own files, a path dependency's included;
  `-Werror` governs the build; the manifest's own `W0901`-`W0903` answer to
  its table.
- One pass applies the levels, in every compile and check entry point, after
  the last diagnostic and before the has-errors decision that gates code
  generation. A level applied after that decision would print `error:` and
  still build.
- `@lint` takes `allow` / `warn` / `deny`, each a string or an array. A bad key
  is `E0448`, a non-string or an error code `E0474`, and an unknown name the
  new `W0242`.
- `[lints]` is validated with the rest of the manifest (§B.2.5): a value that
  is not a level, or an error code as a key, is `E0903`; an unknown key is
  `W0902`.

`W0820` stays a review lint: every checking entry point now raises it,
`jux check` included, and a build still does not, the way Rust keeps its
equivalent out of `cargo build`. The one exception is the point of the gap: a
package whose `[lints]` names it, by code or by name, gets it in the build as
well, so `unsafe-without-justification = "deny"` fails `jux build`. An `@lint`
alone does not make a build raise it.

**Spec status:** `JUX-DIAGNOSTICS-ADDENDUM.md` §D.5.4 is rewritten in place,
the §D.4 warnings table gains `W0242` and the `W0820` row states the build
rule. `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.2.5 gains the two `[lints]` rows.
## E111. A fully-qualified library static is the same call as the imported one

**Conflict.** `JUX-MISSING-DEFS-ADDENDUM.md` §M.16.6 (ERRATA E102) says a
qualified name and an import name the same declaration and every later question
is asked of that declaration. A fully-qualified static CALL did not get that
treatment. `rust.std.File.open(p)` is a foreign `Result` (§G.5.4), so the call
has to unwrap it, and it did, spelled `File.open(p)` after an import. Spelled in
full it reached rustc as the raw `Result` in a `rust.std.File?` slot (E0308).

The call arrives as a field chain, `((rust).std).File.open`. The lowering of the
call itself already re-shaped that chain into the class path it names, but the
questions asked around it (does it return a foreign `Result`, a bare collection,
a borrowed slice) were asked of the chain first, and a chain names no class.

**Resolution.** The re-shape happens once, before any question is asked, and
every question is asked of the re-shaped call. No new rule: this is E102's, at
one more site. `examples/qualified_static_calls.jux` opens a file both ways,
catches the missing-file case, and calls a fallible `rust.std.String` static.

**Spec status:** nothing to change; §M.16.6 already says it.

---

## E112. A nullable inside a collection prints as `null` or its value

**Conflict.** E107's run-time normalization turns `Debug`'s `None` and
`Some(5)` into `null` and `5`, but it recognises a nullable by the value's own
type NAME, so it only ever saw the outer type. `Vec<int?>` holding `3` and
`null` printed `[Some(3), None]`, which is Rust's text, not the value's string
form (`JUX-OPERATORS-ADDENDUM.md` §O.7.1).

**Resolution.** When the type name nests an `Option` anywhere, `Debug`'s text
is re-laid out token by token: `None` becomes `null`, `Some(x)` becomes `x` at
any depth, and a quoted span is copied untouched, escapes and all, so a string
element whose text is `None` stays a string. A string element keeps its quotes,
as it does in any printed collection.

The rewrite runs ONLY when every path in the type name is one whose `Debug`
text is known: the primitives, `String`, `Option`, `Vec`, `VecDeque`, the hash
and B-tree maps and sets, `Rc`, and the prelude's own `JuxCell`. Since E107 a
Jux class's `Debug` is its own string form, which may contain the text `Some(`,
and a foreign type's `Debug` is whatever its author wrote, so a collection
naming any other type prints exactly as `Debug` wrote it.
`examples/nullable_in_collections.jux` prints `[3, null]`, `["a", null, "None"]`,
a nested `[[1, null], [], [null]]`, maps and a `double?` list.

**Spec status:** nothing to change; §O.7.1 already says what a value's string
form is. E107's "Still open" note now points here.

---

## E113. A constant's name is resolved in the package that wrote it

**Conflict.** The compile-time evaluator (§T.11) looked a bare constant name up
program-wide: an exact key, then a unique last-segment match, and memoized the
value under the bare name. That contradicts §M.16's rule that a bare name means
the writing unit's own declaration first. Two packages each declaring
`const int MAX` made `MAX` unfoldable from either, and the memo was worse than
the lookup: `A.K + B.K`, two classes' `static final int K`, folded as `2 * A.K`
because both were memoized as `K`. It could not bite through `jux.std`, which
declares no constants, and it did bite between user packages.

**Resolution.** The evaluator's context carries the writing unit's package and
its imports. A bare name resolves to that package's constant, then to one the
unit imports, then to a library-realm constant when exactly one package there
declares it, and only then to the old root-package and unique-anywhere rungs
(the cross-package capture GAPS 24 deliberately keeps for types). A constant's
initializer is evaluated in ITS package, not the reader's, and the memo is keyed
by FQN (a class constant by `Class::member`). Unit tests in `const_eval.rs` pin
two packages declaring the same names, an import, and the two-class memo.

**Spec status:** nothing to change; §M.16 already says it.

---

## E114. The rest of `Task<T>`'s surface

**Conflict.** §18.1.4 lists `Task.completed(v)`, `Task.failed(e)`,
`t.map(f)`, `t.flatMap(f)`, `t.isCancelled()` and `t.isResolved()`. None was
implemented; since E87 each was an honest `E0413` rather than a rustc leak, but
a documented surface that answers "no such member" is still not there.

**Resolution.** All six, with these semantics where the section is silent:

- `completed(v)` and `failed(e)` are tasks settled at birth. Nothing runs, so
  nothing is spawned; a failure is parked exactly where a task that threw `e`
  parks its exception, so awaiting it re-throws `e`, and dropping it unawaited
  is an unhandled rejection (LANG-V1 §10.1.8), as for any failed task.
- `map(f)` and `flatMap(f)` CONSUME the task, as `await` does, and are tasks of
  their own on the one event loop. A failure of the source, or its
  cancellation, reaches the derived task as the exception it is, and `f` does
  not run. `flatMap` awaits the task `f` hands back.
- `isResolved()` is true once the task has an outcome, a value or a failure;
  a spawned task is not resolved until the loop has run it.
  `isCancelled()` is true once `cancel()` was called, whether or not the task
  has reached a suspension point since.

`map`'s and `flatMap`'s result is typed from the lambda's body, as `spawn`'s is;
a body computing from the parameter is `Unknown`, which fits any slot.
`examples/task_surface.jux` runs every case, including a failure through `map`
and a cancellation seen through `isCancelled` and `blockingGet`.

**Spec status:** nothing to change; §18.1.4 already lists the members.

---

## E115. A field read through a bound that names a generic class

**Conflict.** E100 rule 1 left reading a field through `? extends
Container<int>` unsupported, because a generic class's inherited fields are
typed in its parent's type-parameter vocabulary. Checking it found a second,
wider hole with the same symptom: a LEAF of a polymorphic hierarchy (a class
that extends something, generic or not, and that nothing extends) had an EMPTY
marker, so `? extends Tagged` then `t.weight` was rustc E0609 and a method
`Tagged` itself introduced was E0599, while the fields and methods it inherited
resolved through the parent's marker. Every class with a parent is in such a
hierarchy, so this was the common case for a subclass, not a corner.

**Resolution.** Rule 1's surface, extended to both shapes.

- A **generic** class on the element-parameterized marker (`ContainerKind<T>`)
  that nothing extends gets the `__get_<f>` / `__set_<f>` pair for every
  non-private instance field on its `extends` chain, beside its methods.
- Every accessor on a chain is typed in the class's OWN vocabulary: an
  inherited `U item` on `Holder<U>` is written through what the class passed for
  `U` (`Holder<T>`, `Holder<int>`). That substitution applies to the non-generic
  bound surface too, which had the same latent fault for a class extending a
  generic parent.
- A **leaf** of a polymorphic hierarchy in bound position declares, on its own
  marker, the accessors for the fields it declares and the methods whose names
  no ancestor declares. What it inherits already resolves through the parent's
  marker, its supertrait; declaring it again would make the call ambiguous
  (rustc E0034). A field that shadows an ancestor's of the same name keeps
  direct access rather than read the ancestor's field.

One predicate per shape is consulted by both the marker synthesis and the
field-read rewrite, so a read is rewritten exactly when the accessor it names
exists. `examples/wildcard_generic_bound_fields.jux` reads through
`? extends Crate<int>`, a declared `<C extends Crate<String>>`, a generic leaf
of a generic parent (`? extends Bin<int>`, inherited `item` as an `int`) and a
plain leaf (`Tagged extends Named<String>`).

**Spec status:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.4.8's bound-surface rule
already covers it; E100's scoping note now points here.

---

## E116. Jux never prints raw Rust

**Conflict.** E23 makes every rustc rejection of the emitted crate a compiler
bug, and §D.1 promises that every failure a user sees is a coded diagnostic.
Five places broke both promises, all after the front end had accepted the
program:

- **rustc's own errors reached the user verbatim** (gap 27). A backend bug
  surfaced as `error[E0502]: cannot borrow ... as mutable`, rustc's snippet of
  generated Rust, and only the `-->` arrow rewritten to the `.jux` line. It was
  not a `Diagnostic`, so `--diagnostic-format json` never saw it, and it exited
  1, where §D and `ice.rs` reserve 101 for "the compiler broke".
- **Linking and target selection leaked cargo** (gap 15). An unlinkable
  `@extern(lib = "c")` printed the whole `link.exe` command line; `--target`
  for a triple that was not installed printed rustc's `E0463` once per crate in
  the build; and an `[ffi.*]` entry with `linkage = "framework"` that no
  `@extern` used still went into the build script and failed every non-Apple
  build.
- **A missed borrow hoist died with Rust's panic** (gap 28): `thread 'main'
  panicked at src\main.rs:L:C: already borrowed: BorrowMutError`, naming a
  line of generated Rust and no Jux type.
- **A task used twice leaked `E0382`** (gap 8). §18.1.4 calls `Task<T>` a
  refcounted handle, but a task yields its result once: `await t`,
  `t.blockingGet()` and `Task.all`/`any`/`race`/`allSettled` each take it, and
  `Task.any(a, b)` followed by `Task.allSettled(a, b)` was rustc's "use of
  moved value". Alongside it, a failed task's `Result` printed
  `Err(Exception@0x6318..)`: an exception had no string form, so the §O.4.1
  identity form stood in for its message.
- **Diagnostics had nowhere to point back** (gap 17). §D.1.3 and §D.2.1 specify
  secondary labels ("first declared here") and §D.2.3 a `code_action`; the
  schema and both renderers had them, and no diagnostic produced either.

**Resolution.** Each failure becomes a diagnostic, rendered like every other.

- **`E0900`** (the catalog's "backend cannot lower construct", now raised).
  cargo runs with `--message-format=json`, and every rustc error in the
  emitted crate becomes one `E0900` at the `.jux` line its `// JUX:` marker
  names. The message is Jux-worded by rustc code: `E0382`/`E0505` "value used
  after it was moved", `E0499`/`E0502` "object borrowed twice",
  `E0597`/`E0716` "temporary dropped while in use", anything else "the Rust
  generated for this code does not compile". Notes carry the rustc code and
  message, and the ICE's "this is a bug in the Jux compiler, not in your
  program" (citing E23 for the borrow family); the help line is the issues URL.
  The exit status is 101, the ICE's. rustc's full report is shown under a new
  `--verbose` flag on `jux` and `juxc`. A cargo failure with no compiler error
  in it (a registry out of reach) is not a compiler bug and still passes
  cargo's text through.
- **`E0904`**: `--target` is checked before cargo runs, with `rustc --print
  target-libdir --target <triple>`. A triple rustc does not know, or one whose
  standard library is missing, is a single diagnostic with the `rustup target
  add <triple>` hint. rustc's `E0463` for `std` maps to the same code in case
  the check is ever bypassed.
- **`E0906`**: a failed link names the library (`could not link library
  `nosuchlib``) and quotes the one line of the linker's output that says why
  (`LNK1181`, `cannot find -lfoo`, `library not found for -lfoo`). The command
  line stays behind `--verbose`. A missing library is the environment's fault,
  not the compiler's, so the status is 1.
- **`E0908`**, until now reserved for dynamic linkage in the `core` profile,
  covers the other linkage a target cannot provide: `linkage = "framework"` on
  a target that is not Apple's. **`W0906`**: an `[ffi.*]` entry no `@extern`
  block names is left out of the build script, with a warning, rather than
  linked for nothing.
- **The borrow conflict names Jux.** A class handle is now
  `Rc<crate::JuxCell<C_Inner>>` rather than `Rc<RefCell<C_Inner>>`, and so is
  every `ref` cell (§M.13); `JuxCell` is the newtype the collections already
  used. Its inherent `#[track_caller]` `borrow` and `borrow_mut` take
  precedence over the `Deref` to `RefCell`, so the emitted `.0.borrow_mut()`
  text is unchanged, and on a conflict they panic with
  `internal error: object of type Box was already in use at app.jux:24:9.
  This is a bug in the Jux compiler (ERRATA E23)`. The `.jux` line comes from a
  table the driver writes into the prelude once rustfmt has run: the `// JUX:`
  markers of every emitted file, keyed by the formatted line numbers
  `Location::caller()` reports, written into the one line that declares the
  table so no other line moves. A crate built without the driver keeps the
  empty table and names the Rust line instead. `E0900` still covers a conflict
  rustc can see; this one it cannot, which is why phase 7 of the gap plan adds
  a self-check (gap 29).
- **`E0707`**: a use of a task local after something consumed it, with a label
  on the consuming site. The check follows the body in order, the way rustc's
  move check does: a task consumed on either branch is consumed after the
  branch, `t = spawn(...)` re-arms `t`, a declaration of the same name shadows
  it, and a lambda that awaits a task consumes it where the lambda is written.
  `t.map(f)` and `t.flatMap(f)` consume `t` as well, the rule E114 gave them.
  It does not follow a loop's back edge; rustc still catches a task consumed on
  every turn of a loop. `Task<T>` stays a refcounted handle in the sense that
  matters for sharing a *result*: keep the value the first `await` gives.
- **An exception's string form is its message.** `Throwable` declares
  `operator string` as `getMessage()`, and an exception class inherits it
  (§O.2.9, now honoured by the inline class shape exceptions use as well as by
  the shared handle), so `print(e)` prints `boom` and a failed task's settlement
  prints `Err(boom)`.
- **Labels and fixes have producers.** "First declared here" labels on `E0400`
  (top level, annotations, `drop` blocks, overloads), `E0401`, `E0402`,
  `E0403`, `E0303`, `E0304` and `E0951`; "declared `final` here" on `E0420`,
  `E0421`, `E0464` and `E0465`. The labels show in the `human`, `compact` and
  `json` formats; the one-line `line` format, which the UI tests pin, has no
  room for them and is unchanged. `Diagnostic` gains `code_action` (§D.2.3),
  rendered in JSON and carried through the language server's `data` slot to a
  quick fix. The first two: removing the `final`/`const` an `E0464`/`E0465`
  names (the parser now records the modifier's span), and replacing a mistyped
  import with the path its "did you mean" suggests.

**Spec status:** `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 carries `E0707`, `E0900`,
`E0904`, `E0906`, `E0908` and `W0906`, and §D.3's note on the build's band names
them. `JUX-ASYNC-ADDENDUM-v2.md` §18.1.4 says a task yields its result once.
## E117. A bare name does not reach another user package

**Conflict.** `JUX-MISSING-DEFS-ADDENDUM.md` §M.16 lists four rungs for a bare
name (a generic parameter, the unit's own package and imports, a nested type,
the implicit prelude of `jux.std.*` and `rust.std`) and says a name that
reaches none of them is `E0417`. The compiler had a fifth, unwritten rung: a
workspace-wide scan by last segment. So in

```java
// b/Widget.jux
package b;
public class Widget { public int v = 3; public static int make() { return 4; } }

// a/Use.jux
package a;
public class Use {
    public Widget mk() { return null; }       // compiled: meant b.Widget
    public int st() { return Widget.make(); } // compiled: meant b.Widget
}
```

`package a;` used `b.Widget` without importing it, which Java does not allow
and §M.16 does not describe. E96 kept the scan deliberately (GAPS 24): it was
also the road by which a package-private type of another package reached the
`E0416` check. The same scan backed constant folding (E113 kept a "unique
match anywhere" rung "in line with gap 24"), the free-function lookup's last
rung, the static-call head check and the record/enum receiver lookup.

**Resolution.** The scan is gone for user code. A bare name written in a user
package reaches, without an `import`:

- its own package (and, before that, whatever its imports bind);
- the **root package**, whose declarations are keyed by their bare names and
  which no `import` can name;
- the **library realm** (`jux.std.*`, `jux.meta`, `rust.*`, crate gating per
  §G.6.5 unchanged).

Never another user package, and a dependency's package is a user package. A
unit of the library realm still reaches the library realm only (§M.16.1), now
including for constants of the root package.

- A type name that reaches nothing is `E0417` (a signature slot, a local, the
  type of `new`, a cast, ...). When another user package declares a type of
  that name the diagnostic carries a help: `add import b.Widget;` for one
  package, the list of packages for several.
- An `extends` / `implements` head that names no visible type is `E0417` too
  (same help). It used to reach rustc as "cannot find trait", for an
  un-imported supertype and for a name declared nowhere alike.
- `Widget.make()` with the head un-imported is `E0301` "cannot find `Widget`",
  with the same help. A qualified `b.Widget.make()` is unchanged.
- `E0416` for a TYPE is still reported, through the import that names it
  (`import a.Hidden;` or `import a.*;` over a package-private `a.Hidden`).
  The import is now the only way to reach another package's type, so it is the
  only way to that diagnostic.
- A constant of another user package folds only through an import. An
  initializer written in another package is evaluated with ITS unit's imports
  (it used to get none), so `b.DOUBLE = BASE * 2` with `import c.BASE;` in
  `b` still folds when `a` reads `DOUBLE`.
- A free function of another user package is found through the unit's import
  in the checker and the backend alike; the resolver already reported an
  un-imported call as `E0301`.
- The backend's bare-type, bare-class and free-function lookups PREFER what
  the rule reaches inside a unit, so a same-named library type is never
  displaced by an unrelated user package's. They keep the open scan behind
  that preference, because the emitter also asks about names it carries over
  from another unit's signature (`Readout(.., Aggregate<int> summary)` written
  where `Aggregate` was imported, emitted at a call site that did not import
  it); every name a program wrote has already been vetted by the checker.

Expressions were already right: the resolver's known-name set is the unit's
own declarations, its imports and its package's siblings, so `new Widget()`
without the import was `E0301` before this entry. What changed is every TYPE
position and every lookup behind the resolver.

**Spec status:** `JUX-MISSING-DEFS-ADDENDUM.md` §M.16 states the root-package
rule and the import help; GAPS 24 is closed by this entry.
## E118. A generic class over a type that is not `Clone`

**Conflict.** §T.2.1 promises that a compiler-added bound never makes a legal
type argument illegal, and E97 made a foreign type that is not `Clone` (139 of
the 296 `rust.std` stub types, `std::fs::File` among them) something a class
can hold. The two did not meet: every generic declaration lowered each type
parameter with the literal `Clone + std::fmt::Debug + 'static`, so
`Cell<File>` over `class Cell<T> { T value; }` failed in rustc with a bound the
program never wrote, while `Vec<File>` worked. E97 recorded this as its known
boundary (GAPS.md gap 2). The bound cannot simply go: a field read of a `T`
auto-`.clone()`s, a `return` copies, and the renderer picks its tier against
the declared bounds, so every body that touches a `T` by value needs it.

**Resolution.** The bound MOVES, for the classes where that is provably
enough, from the declaration to the members that need it.

- **Which classes.** A generic class lowered to the `Rc<RefCell>` handle that
  stands alone: no `extends`, not extended, no `implements`, not abstract,
  and no properties, operators, instance initializer blocks or `drop` body
  (each of those adds impls or helpers that call back into the inherent impl,
  and a `Drop` impl must repeat the struct's bounds exactly). A worker-shared
  (atomic) class does not qualify. Records, enums, interfaces, free functions
  and every other class keep the baseline unchanged.
- **Which parameters.** Within such a class, a parameter is RELAXED when
  every instance field that mentions it holds it bare (`T value`, `T? value`)
  or passes it bare to another class in a position whose parameter is itself
  relaxed (`Cell<U> slot`). That last rule is a greatest fixpoint over all
  classes: start with every parameter relaxed, drop one whenever a field
  forwards it to a slot that is not, repeat until nothing changes. Any other
  field shape (`List<T>`, `T[]`, `(T) -> void`, an interface over `T`) keeps
  the parameter on the baseline, because that type's own declaration asks.
  So does a parameter a declared bound passes as a type argument
  (`K extends Comparable<K>`), for the same reason.
- **What a relaxed parameter carries.** On the struct heads, the handle, the
  inherent `impl` header and the identity impls (`Display`, `Debug`,
  `JuxIdentity`, `PartialEq`/`Eq`/`Hash`, all of which print or compare the
  handle's address) it carries `'static` plus the user's own bounds and the
  key/equality/default bounds §T.2.1 already infers. The handle's `Clone` is
  written by hand (`Self(self.0.clone())`, an `Rc` bump): the derive would
  have bounded it on `T: Clone`. Every other impl of the class (its `Kind`
  marker, the E115 bound-position accessors, lifted statics) keeps the full
  baseline, so a delegating body there still resolves to the inherent method.
- **What a member states.** Each constructor and method writes
  `where T: Clone + std::fmt::Debug` for exactly the relaxed parameters it
  touches, and nothing when it touches none. A member touches `T` when a
  parameter or return type mentions it (other than a bare `T` parameter, and
  a relaxed handle such as `Cell<T>` anywhere); when the recorded type of any
  expression in its body, or the declared type of a field, parameter, local,
  loop binder or lambda parameter it reads, mentions it (a relaxed handle
  exempt, a bare `T` not); when a catch clause, cast, type test, `new`,
  `new T[n]` or explicit type argument names it; or when it calls a method on
  a receiver whose type mentions it (the callee's own clause is not consulted).
  It touches EVERY relaxed parameter when it names `this` other than to reach
  a field, uses `super`, calls a method of the class unqualified, or takes a
  method reference. A constructor also inherits whatever the instance field
  initializers touch; the one exemption is a constructor on the fast path
  (nothing but field stores) whose same-named store `this.f = f;`, of a
  parameter named nowhere else, is the `C_Inner { f }` shorthand: the value
  is moved, not cloned. A synthesized default constructor and an
  abstract member touch everything.

So over `File`, `new Cell<File>(f)`, `c.label()`, `c.count()` and a
`Pair<File, int>`'s `getSecond()` all compile, while `c.get()` (which returns
a copy of a `T`) does not: the `where` clause names the bound it needs.

One related fix: a method called on a FIELD whose value is a foreign type
without `@RustClone` (`c.value.metadata()`, and equally the non-generic
`w.file.metadata()`) is called in place through the object's borrow. It used
to be hoisted out of the borrow, which copies the receiver, so it did not
compile at all; a foreign method runs no Jux code that could find the object
borrowed, which is what the hoist guards against.

**Known boundary.** Calling a member whose `where` clause the type argument
does not meet (`Cell<File>.get()`) is still reported by rustc, not by the
checker, since the checker does not model these bounds. A class outside the
qualifying shape (a hierarchy, an interface implementation, a class with a
property or an operator) keeps the baseline, so `Cell<File>` for such a class
is still refused in rustc. `examples/generic_over_foreign.jux` pins both
halves: the classes over `File`, and the same classes over `int` and `String`
keeping every member. (Both halves closed by E120: every generic
declaration shape now moves the bound, and the checker reports a use the
argument cannot meet as `E0457`.)

**Spec status:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.2.1's first table row now
points here; E97's "known boundary" paragraph is narrowed to the shapes this
entry leaves on the baseline.

---

## E119. The compiler checks its own borrow discipline

**Conflict.** E23 stands: Jux has no user-visible borrow checker, and the
reserved `E0500`-`E0505` stay reserved. What E23 leaves to the compiler is
JUX-CLASS-REPRESENTATION §CR.4.1's promise that no cell guard is alive while
Jux code runs, and nothing checked it. A missed hoist compiled cleanly and
stopped the program with E116's "object of type C was already in use", a
compiler bug found by the user (gap 29). The corpus pinned one such program
as its expected output. Probing the re-entrancy shapes Java programs use
found more:

- **A function stored in a field**, called through `this.hook()`, `hook()` or
  `obj.hook()`, was `(self.0.borrow().hook.clone())()`: the guard lived to
  the end of the statement, across the call, so a callback that reached back
  into its owner (an event handler, `b.hook = () -> b.bump()`) found the
  object in use. The standard library's `LazyIterable.iterator()` had the
  same shape. An argument read out of the object (`this.step(this.n)`) went
  through the method-call hoist, which wrote `self.step(..)`, a method that
  does not exist (rustc `E0599`, so `E0900`). A function taken out of a
  collection field and called, `this.fs[0](x)`, skipped the owner's borrow
  altogether (`E0609`).
- **An operand read before an operand that runs Jux code.** `a.n + a.inc()`,
  `this.state == this.step()` in an `if` or `while` condition, `$"${a.n}
  ${a.inc()}"`, `new Pair(a.n, a.inc())`, `new int[]{n, inc()}` and
  `v.x + (v + v).get()` with a user operator all kept the read's guard to
  the end of the statement while the call took a mutable borrow.
- **A field named bare as an argument** in the class's own method,
  `add(n)`, kept `self.0.borrow()` alive across `self.add(..)`: the
  argument hoist only recognised `this.n`.
- **A static helper handed `this` by its bare name**, `reset(this)` inside
  the class, passed `self` (a reference) where the method takes the object
  (rustc `E0308`); `Button.reset(this)` already shared the handle.
- **Compound assignment read the field after its right side.** The
  borrow-safe `{ let v = rhs; obj.0.borrow_mut().f += v; }` evaluated `rhs`
  first, so `n += inc()` added `inc()`'s result to the NEW `n`; JLS 15.26.2
  reads the left side first.

**Resolution.** The backend holds to one rule, and the driver checks it.

- **Lowering rule: nothing that runs Jux code runs under a guard taken
  earlier in the same statement.** A stored function is bound before it is
  called, `({ let __jux_callee = self.0.borrow().hook.clone();
  __jux_callee(args) })`, with any argument that reads a cell bound after it
  (Java's order: the target, then the arguments). A function read out of an
  indexed collection is bound the same way. For an operator chain, an
  interpolated string, constructor arguments and an array literal, the
  operands are taken in evaluation order; when one that reads through a cell
  comes before one that may run Jux code (a Jux method, function or stored
  function, a constructor, a user operator), every operand ahead of the last
  such call that reads a cell or runs code is bound to a `let` first, in
  order, and the expression reads the temps. Operands that do neither
  (literals, locals) stay in place. Whether the call really reaches the
  object is not asked, as §CR.4.1 already decides for stores: being wrong
  costs a binding, not a crash. A bare field argument counts as the read it
  is. A compound assignment whose right side may run Jux code binds the
  field's old value first and stores it back just before the operator
  applies, which is Java's order. A static method called by its bare name
  with `this` shares the handle.
- **Self-check.** Under `JUX_SELFCHECK=1` the driver parses the emitted Rust
  (after rustfmt, before cargo) and walks every function generated for Jux
  code, following Rust's temporary-scope rules (edition 2021: statement
  ends, `if`/`while` conditions, match arms, lazy `&&`/`||` operands, block
  tails that outlive their block, `match` scrutinees and `for` iterators that
  live across the body). It reports a guard still alive when the same cell is
  borrowed again with either side mutable; when a Jux method that may take a
  mutable borrow (by name, closed over its callers) is called on the guarded
  object or handed it as an argument; or when Jux code the object holds runs,
  a method of a handle read out of its field or a function stored in one.
  Copies of a handle (`let b = a.clone()`) are followed; other aliasing is
  not, so a clean report is not a proof. Each hit is an `E0900` at the
  `.jux` line, "the compiler would emit a borrow conflict here", with the
  conflict in Jux words as a note and exit status 101. Users do not pay for
  the check; the example corpus and every test that builds an example set
  the variable, and the corpus fails outright, never blessed, on a
  self-check hit or on E116's run-time message. On the corpus it reports
  nothing.
- **The pinned conflict is gone.** `examples/borrow_conflict_report.jux` is
  `examples/reentrant_stored_lambda.jux` and prints what Java prints;
  `reentrant_operands.jux` and `reentrant_callbacks.jux` pin the other
  shapes. E116's run-time message is unchanged and has no example left that
  reaches it.

**Spec status:** JUX-CLASS-REPRESENTATION §CR.4.1 is correct as written (its
read rule already says a borrow is never held across a method call); the
lowering above is how the backend meets it for stored functions and for
operands. `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4's `E0900` row names the
self-check as a second source.
## E120. Every generic declaration moves `Clone + Debug` to its members, and the checker says what they need

**Conflict.** E118 moved the bound for one shape only: a standalone generic
class. A class that extended another or was extended, implemented an
interface, was abstract, or declared a property, an operator, an
initializer block or a `drop` body kept `Clone + std::fmt::Debug + 'static`
on every parameter, and so did every interface, record and enum. `Cell<File>`
compiled; `Holder<File>` over the same body did not, the moment some class
extended `Holder`. §T.2.1 promises that a bound the compiler adds never makes
a legal type argument illegal, and that promise held for one declaration
shape out of six.

The other half was E116's: a use of a member whose `where` clause the argument
could not meet (`Cell<File>.get()`, which returns a copy of the `T`) reached
rustc as `E0277` against a clause the program never wrote, then as `E0900`.

**Resolution.** One table, computed by the checker and read by both phases.

- **Where it lives.** `juxc_tycheck::clone_needs` runs after the check walk
  (it reads the expression types) and stores its answer on the symbol table.
  The backend writes every `where` clause from it, and the checker's new
  `E0457` checks every use against it, so "the checker accepts it" and "the
  emitted bounds hold" are the same statement. The two lowering predicates
  both phases depend on (`ctor_is_pure_store`, `seed_moves_param`, below) are
  defined there too and called by the backend.
- **Which declarations.** Every generic class, interface, record and enum of
  the program, except the core library (`jux.std`, `jux.meta`, whose members
  copy their values), the shapes that lower to a plain value rather than the
  shared handle (a `struct`, a `@layout(c)` record, a class in an `extends`
  component that contains an exception or a thrown/caught class), and a record
  or enum that implements an interface (its interface impl hands out copies of
  the value).
- **Which parameters keep the baseline.** A parameter is relaxed unless one of
  six rules keeps it, and each kept parameter records its reason, which
  `E0457` quotes:
  1. a declared bound passes it on as a type argument (`K extends
     Comparable<K>`);
  2. an instance field, record component or enum payload holds it other than
     bare (`T`, `T?`) or forwarded bare into a relaxed parameter of another
     declaration of the program (`Cell<T>`); an array (`T[]`), a function
     type, a `ref`/`weak` slot, or a type the program does not declare keeps
     it;
  3. an `extends`/`implements` clause, or an interface's `extends`, passes it
     other than bare into a relaxed parameter;
  4. a settable or computed property's type mentions it: firing its observers
     compares and copies old and new values;
  5. the class's `drop` body, or its `operator string`, `==`, `hash` or `<=>`,
     reads it. Those run where no call names a type (scope end, printing, a
     map key, a sort), so a member clause could not be checked where they run;
  6. a member that overrides or implements a member of a parent or interface,
     or inherits one that hands `this` on, needs it while the declaration does
     not pass it to that parent or interface: the call through the base could
     not state the bound.
  The rules are a greatest fixpoint over the whole program, re-run until no
  parameter moves.
- **What a relaxed parameter carries.** On a class's or interface's headers
  (struct, handle, inherent impl, trait declaration, every trait impl,
  identity impls) `'static` alone; on a record's or enum's, `std::fmt::Debug +
  'static`, because its string form prints its components and every container
  and interface of it relies on that form. The class handle's `Clone` is
  written by hand (an `Rc` bump). Every function item the declaration emits
  states `where T: Clone + std::fmt::Debug` for the relaxed parameters its
  member needs: constructors, methods, operators, property accessors, the
  copies of inherited members and `super` shims (read through the
  parent-to-child map), the `Kind` trait's members, its `Rc` forwarding impl,
  its field accessors and its delegating impls, an interface's members and
  default bodies and every implementer's trait impl (read through the
  interface-to-class map). The one impl made conditional as a whole is the
  slicing upcast `From<Child> for Parent` of a non-polymorphic parent, which
  copies the parent slice. A static method lifted out of a generic class keeps
  the baseline: it names no instance.
- **What a member needs.** E118's walk, with four changes. A call of a method
  on the same object (`m()`, `this.m()`) needs what that method needs, not
  every parameter; only `this` handed on as a value, `super`, and a method
  reference still need all of them. A virtual member needs what every override
  and implementation needs, joined through the `extends` and `implements`
  maps, since a call through the base reaches any of them. A class's
  field initializers and initializer blocks join every constructor, and the
  constructor a class that declares none is given is analysed the same way
  (keyed by the class's own span). A constructor in an `extends` hierarchy
  needs every parameter unless it is PURE (below).
- **Two lowering changes that make the relaxation usable.** A constructor in
  a hierarchy used to run its body a second time against the finished object
  (ERRATA E21), which uses each parameter twice and copies it, so no hierarchy
  over `File` could be constructed. A PURE constructor, one whose body is an
  optional `super(args)` of parameters or literals followed by stores of this
  class's own fields from parameters or literals, in a class with no
  initializer block, has no effect but the values it stores, so when it runs
  is unobservable: it now stays in the builder and moves each parameter once.
  And the general constructor path, which lifts the leading field stores into
  the struct literal ("seeds"), moves a parameter the body names once into a
  field typed as a bare type parameter instead of copying it, so a class with
  an initializer block can take a `T` it only stores.
- **`E0457`.** Every use is checked against the table: a method call (the
  member's clause read through the receiver's arguments, an inherited member
  through the `extends` chain, an interface default through `implements`), a
  constructor, a property read or write (its accessor), an operator, a field
  read that copies a relaxed parameter's value out of the object (a method
  called on the field in place copies nothing), a written or inferred type
  whose class or interface keeps the baseline on an argument that lacks it,
  an `extends B<X>` / `implements I<X>` clause (the class gets a copy of every
  member it inherits, so each one's needs apply to `X`), and any member of a
  record or enum (its impls bound a relaxed parameter on `Debug`, a kept one
  on both). Capabilities: a class or interface value is a handle that copies
  by refcount and prints its string form; a primitive, `String` or array
  copies; a record or enum copies when its components do; a foreign type has
  what its stub's `@RustClone` / `@RustDebug` say, its arguments included; a
  function value copies but has no debug form. The message names the
  argument, the member and the parameter: "`File` cannot be copied, and
  `Cell<File>.get()` returns a copy of `T`"; for a kept parameter it quotes
  the rule that kept it.

`examples/generic_over_foreign.jux` now covers a hierarchy used through its
base, an interface implementer used through the interface, an abstract class,
a class with a property, an operator, an initializer block and a `drop` body,
a record and an enum, all over `File`, and the same declarations over `int`
and `String` keeping every member. `tests/ui/type_argument_*.jux` pin the
diagnostic's five forms.

**Known boundary.** What stays outside the table, stated so nobody mistakes
it for coverage:

- A record or enum over a type that cannot be copied is itself a value that
  cannot be copied. A copy of the VALUE the backend inserts (passing it twice,
  reading it out of another object) is the same case as E97's non-generic
  `record Holder(File f)` and still reaches rustc. So does binding a payload
  out of such an enum in a `switch`, which copies it.
- Printing a record or enum whose relaxed argument has no debug form (a
  function type) is not checked at the print site.
- A static method of a generic class keeps the baseline; a static call whose
  inferred class argument cannot meet it reaches rustc.
- An overloaded method call the checker recorded no selection for, with two
  candidates of the same arity, is not checked.
- The analysis is conservative where it cannot see: a virtual member needs
  what any override needs, a hierarchy constructor that does more than store
  its parameters copies them, and a settable property over `T` keeps `T` on
  the baseline. Each of those is a rule with a reason, reported as such.

**Spec status:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.2.1's first row and its
closing paragraph point here. `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 gains
`E0457`. GAPS.md gap 2 is closed.

---

## E121. The representation selector

**Conflict.** `JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.3 specifies a
selector that picks one of Inline, `Box`, `Rc`, `Rc<RefCell>`, `Arc` and
`Arc<Mutex>` per class, and §CR.7 gives it three diagnostics numbered
`E0950`-`E0952`. The compiler gave every class `Rc<RefCell>` (commit
`02bc558` removed the earlier tiering because its boundaries were where the
emitted Rust failed to borrow-check), so the selector was unbuilt (GAPS.md
gap 23). Its three codes also collided with the free-function operator
checks `E0950` (orphan operator) and `E0951` (duplicate operator), raised
since E65, and `E0952` (reserved by §R.3.6).

**Resolution.**

- **Codes.** §D.5.1 forbids reassigning a published code, so the operator
  checks keep `E0950`-`E0952` and the representation diagnostics become
  `E0953` (identity across function boundaries on an Inline class), `E0954`
  (a `weak` target left Inline) and `E0955` (a self-containing class left
  without a refcount), in §CR.7's order. §D.4 lists them.
- **Tier `Rc`: a class nothing writes has no cell.** §CR.4.1's `mutated`
  property is computed per class, by fully-qualified name
  (`rep_select.rs`). A class is `mutated` when any code writes one of its
  objects after construction: an assignment, compound assignment, `++`/`--`,
  `out` argument or `&` whose place is one of its fields (through a receiver
  typed as the class, `this`, or a bare field name in its own members); a
  write into a value struct held in one of its fields; a call that mutates a
  field in place (a known container mutator, a user method that writes
  `this`, a foreign `@MutSelf` method). A store through a receiver the types
  do not name as a class (an interface, a type parameter, an unrecorded type)
  counts against every class with a field of that name, and a write reaches
  the whole `extends` component. A constructor's or initializer's own stores
  are construction. Some classes keep the cell whatever is written: one with a
  `ref` or `observer` field or an observable property (attaching an observer
  writes its list), one implementing an interface with a settable property,
  one whose constructor runs against the finished object (it calls a method on
  `this` or hands `this` to a lambda), and, for now, every member of an
  `extends` hierarchy and every class that crosses a worker boundary (its
  handle is the atomic `JuxSync`). Every other wrap-eligible class is
  `pub struct C(Rc<C_Inner>)`: constructed with `Rc::new`, its fields read as
  `x.0.f` with no guard, identity still `Rc::ptr_eq`. A `weak` field or
  parameter aimed at it is `Weak<C_Inner>`. The emitters ask one place how to
  reach a class's fields (`cell_read`, `cell_write` in `backend_fqn.rs`), and
  a bound-position or interface accessor for such a class has a getter and no
  setter.
- **The emitter checks the selector.** Every mutable borrow the backend emits
  for a class goes through `cell_write`. If the class was given no cell, the
  request is recorded and the whole lowering runs again with that class's
  cell restored, so a write the analysis missed costs a second pass, never a
  program that does not compile. The classes restored this way are on the
  result (`RustCrate::rep_fallbacks`), and under `JUX_SELFCHECK=1` each is an
  `E0900` ("the representation selector judged class `C` never written after
  construction, and the lowering writes it"): on the corpus the analysis alone
  must be right, and it is. The borrow self-check needs no change for a class
  without a cell: it tracks cell guards, and such a class takes none.
- **Tiers Inline and `Box`: a contained class is a value.** §CR.3.3 gives
  Inline to a class that neither escapes nor is aliased, and `Box` to one that
  escapes without being aliased. Both copy the object where the lowering would
  have shared a handle, so both are sound only when a copy cannot be told from
  the object. The selector therefore decides them by a whitelist of positions
  (`compute_contained_classes`), not by a list of the ways an object could be
  observed: an object of a CONTAINED class appears only as a FRESH local's
  initializer (`var p = new C(..)` or a call returning `C`, never `var q = p`,
  and the local declared `var` or `C`), as a whole expression statement, as
  the receiver of one of the class's own fields or methods (not the universal
  `operator hash`/`operator string`, which read identity), and as the value of
  a `return` from a function declared to return exactly `C`; never inside a
  lambda or an anonymous class, and `C` is named by no field, parameter, record
  component, enum payload, bound, alias or constant, and by no checked
  expression type other than exactly `C`. So the object is never passed,
  stored, compared, printed, hashed, captured or given a second name. A
  candidate also has no type parameters, no `extends` or subclass, no
  `implements`, no properties, no `drop` body (a copy would run it twice), no
  annotation, no `async` or generator method, no `ref`/`weak`/`observer` field,
  a constructor that builds in place, and fields that are all Jux values,
  class or interface handles, arrays or collections: a FOREIGN value is out
  even when it is `Clone`, since its own methods may change it through `&self`
  where no analysis of the Jux program looks. Every method of an Inline class
  takes `&self` (nothing writes it, so the by-name guess that a same-named
  method mutates does not apply). A
  contained class that nothing writes is Inline (a plain struct, §CR.6.1,
  exactly §CR.8.1's shape) when no function returns one, and
  `Box<C_Inner>` (§CR.6.2) when one does. A contained class that IS written
  stays on the `Rc` tiers: §CR.4.1 lets a unique owner mutate a value in
  place, but the lowering copies a value where it would have shared a handle,
  and a write to a copy is lost silently. That restriction is the whole
  difference from §CR.3.3's table, and it is what makes these tiers sound.
- **Tiers `Arc` and `Arc<Mutex>`: the worker path, split on `mutated`.** A
  class whose objects cross a worker boundary (§18.2's capture analysis,
  closed over fields, unchanged) takes an atomic handle. §CR.4.1's two
  cross-thread rows now both exist: a class that needs its cell keeps
  `JuxSync` (`Arc<Mutex<C_Inner>>`), and one nothing writes, whose fields are
  all Jux values or classes (so the handle is `Sync`), is `JuxArc<C_Inner>`, an
  `Arc` with no lock that reads its fields through `Deref` and answers the same
  `new`/`as_ptr`/`ptr_eq` surface. A write the analysis missed falls back to
  `JuxSync` through the same demand mechanism. The §CR.3.5 roll-up joins
  representations rather than taking the higher rank: `Arc` joined with
  `RcRefCell` is `ArcMutex`, the one that both crosses threads and is written.

- **Hierarchies and generics.** A hierarchy no longer keeps the cell as a
  matter of course: every class of an `extends` component takes the join of
  what its members need (§CR.3.5), so a hierarchy nothing writes is a plain
  `Rc` with its `Rc<dyn Kind>` dispatch unchanged, and one written through
  any member (a subclass's method, a store through a base-typed reference,
  which the analysis attributes to the whole component) keeps the cell for
  every member. The slicing upcast, the `Kind` forwarding impls and the
  bound-position accessors of a leaf follow the representation; a hierarchy
  without a cell offers no `__set_<f>`. A constructor in a hierarchy that
  does more than store its parameters runs against the handle (E21) and so
  keeps the cell. A generic class has one representation for all its
  arguments (§CR.3.4): the analysis reads every use of every instantiation,
  so one written `Slot<int>` gives `Slot<String>` its cell too.
  Per-instantiation selection stays future work, as §CR.9 Phase D says.
- **`E0953`-`E0955` are the selector's own check.** §CR.7 describes them as
  firing "before the selector commits" when the program needs something a
  value representation cannot give. The selector here never commits: its
  whitelist rules out, before choosing, every class compared with `===`, the
  target of any `weak` reference, and every class that contains itself. So
  the three codes are a verification of the finished selection
  (`verify_selection`), run on every build: a class selected Inline or `Box`
  that is compared by identity (`E0953`), aimed at by a `weak` field or
  parameter (`E0954`) or self-containing (`E0955`) is reported as an error
  naming the class, instead of being lowered into a program that would behave
  differently from Java. None can fire unless the selector is wrong, which is
  why the help line calls it a compiler bug. §CR.7's `E0952` wording ("a hard
  error until the cycle-breaker lands") is not followed literally: a
  self-containing class takes a refcounted representation (`Rc`, or
  `Rc<RefCell>` when written), leaking uncollected cycles as §CR.9 Phase C
  documents, rather than rejecting every linked list.
- **What is reported.** Each class's representation is written above it in
  the generated Rust as `// JUX-REP: inline|box|rc|rc-refcell|arc|arc-mutex`
  (§CR.9 Phase D). §CR.9 asked for `// JUX:rep=...`; `// JUX:` is the
  source-marker prefix the driver maps lines by, so the spelling differs. On
  the example corpus (436 programs, the core library's classes left out),
  the 579 user classes select as: 293 `rc`, 196 `rc-refcell`, 81 `inline`,
  2 `box`, 2 `arc`, 5 `arc-mutex`. Every program prints exactly what it
  printed before, the borrow self-check stays clean, and no class needed the
  fallback.

**Known boundary.** Where this departs from §CR.3.3's table, it is toward
the more general representation: a contained class that is written stays on
the `Rc` tiers (Inline and `Box` copy, and a write to a copy is lost); a
contained class holding a foreign value stays there too; a candidate for a
value representation must not be generic, in a hierarchy, or implement an
interface; `Arc` without a lock needs every field to be a Jux value or class.
§CR.2 and §CR.5.2 still say a `dyn`-dispatched slot forces `Arc`; the
selector keeps E14's resolution (`Rc`, atomic only across a worker boundary).
The value tiers are rare by construction: most objects in a Java-shaped
program are passed somewhere, and a passed object is shared.

**Spec status:** §CR.7 carries the new numbers and a note on the old ones;
§D.4 has the three rows, implemented. The addendum's status line and §CR.9
say what is built. GAPS.md gap 23 is closed.

---

## E122. A parse error inside a type body stays inside it

**Conflict.** `JUX-DIAGNOSTICS-ADDENDUM.md` §D.1 asks for a diagnostic at the
place that is wrong, and `JUX-ENTRY-POINTS-ADDENDUM.md` has top-level
statements become `main` (§E.1.1). The parser met the second rule with the first one's
input. A type body with one broken member (`regular = { 278, 355 };` in a
constructor, LEAKS L22) made the whole type fail; the compilation-unit loop
then retried the type as a top-level STATEMENT, the statement parser reported
`E0993` "a class cannot be declared inside a function body", and the
recovered statement became a synthetic `main` that collided with the real
one in another file (`E0400` "`main` is declared more than once"). The real
error, and its location, were dropped with the failed declaration attempt.
Two recovery rules fed it: statement recovery stopped at the first `}` even
when it had stepped into a `{ ... }` group itself, closing the constructor
early, and every member parser's failure was the type's failure.

**Resolution.**

- A member that fails to parse is skipped whole, from its first token: through
  a `;` at brace depth zero, or through a `{ ... }` group opened at depth zero
  (a body, or a field's brace initializer, with its `;`), stopping before the
  type body's `}` or at a visibility keyword that starts the next member. The
  type keeps every other member. This holds in a class, a record, an enum and
  an interface body.
- A type declaration (`class`/`interface`/`enum`/`record`/`struct` and a name)
  that fails is never retried as a statement: its own diagnostics stand and
  the declaration is skipped whole.
- Statement recovery skips a brace group it steps into, so its `;`s and `}`
  stay the broken statement's.
- Three messages say what is wrong instead of "expected expression": the
  brace shorthand outside a declaration ("an array initializer `{ ... }` is
  only allowed where a variable or field is declared with its type ...; write
  `new int[] { 1, 2 }`", JUX-LANG-V1 §5.5), and a statement written straight
  into a type body (`count = 5;`, `print(...)`, `if`/`for`/`while`/`return`/
  `throw`/`try`), each with where it belongs. After the shorthand in an
  assignment the statement is kept as an assignment of the target to itself,
  so a field assigned there is not also reported "not definitely assigned".
- One token carries at most one syntax error (`E0200`): a production that
  gives up where another already did adds nothing.

`tests/ui/class_body_parse_recovery.jux` pins six malformed shapes.

**Still open.** An uninitialized LOCAL assigned the brace shorthand
(`int[] r; r = { 1, 2 };`) also reports `E0601` for the placeholder read.

**Spec status:** no addendum text changes; this is recovery, which the
grammar leaves to the implementation.

---

## E123. A Rust length stays a `uint`

**Conflict.** LEAKS L23: `String.len()` and `Vec.len()` are Rust's `usize`,
which `JUX-BINDGEN-ADDENDUM.md` §G.3.1 maps to `uint`, and
`n % v.len()` or `int t = n + v.len();` with an `int n` is `E0410` by
`JUX-SEMANTICS-ADDENDUM.md` §S.2.6 rule 4 (`int + uint` has no common type).
A Java programmer expects `int` sizes, and JUX-LANG-V1's older examples
(`d.tricks.size()` "copies the int out") read that way.

**Resolution.** The spec decides it, and the decision stands:

- `JUX-CORE-LIB-ADDENDUM.md` §K.12 keeps `Vec`, `HashMap`, `HashSet` and
  `VecDeque` as Rust's own types with their own methods: "a `Vec` has `push`
  and `len`, not `add` and `size`". `JUX-MISSING-DEFS-ADDENDUM.md` §M.6.1 builds
  on it (`0..v.len()` is a `uint` range). A Jux-level `int` length would
  contradict three addenda and the range rule.
- The Jux-side count is `String.length()`, an `int` (§K.7, ERRATA E13).
- §S.2.7 already carries the "size() is int" ergonomics, and they are what
  the compiler does: a `uint` flows into an `int` slot (`int n = v.len();`,
  `return v.len();`), and `+ - *` over `uint` values and literals in a signed
  slot is computed in the slot's type (`int last = v.len() - 1;` is `-1` for
  an empty list; `int at = header.len() + body.len();`).
- A comparison never needs a cast (`i < v.len()`, "comparisons are exact").
- Where a length meets an `int` in one operator, `E0410` stays, and its help
  now names the length and the two spellings that work: "`v.len()` is a Rust
  length, which is a `uint` in Jux (a `usize`): convert it where it meets an
  `int`, `(int) v.len()`, or take it into an `int` first, `int n = v.len();`".

**Spec status:** no addendum text changes.

---

## E124. What a bare name means inside a type, in the backend too

**Conflict.** ERRATA E102 and E117 settled that a name keeps the meaning the
unit it was written in gives it, and that a program's own declaration is
what a bare name in its package means. Three backend lookups still asked a
different question (LEAKS L24, L25, L26):

- **A record's or an enum's own static method, called bare.** `channel(r)`
  inside `record Color` was emitted as a bare `channel(self.r)` (rustc
  E0425: an associated function has no implicit `Self`). A class's was
  qualified through its `extends` chain; a record's and an enum's were not.
  Inside an enum the resolver did not even see its methods: `describe(this)`
  was `E0301`.
- **A user class named like a library enum.** Every constant read of
  `leaker.ui.Theme.NAV_MIN` (a user class, same package, no import of
  eframe's `Theme`) lowered as `eframe::Theme::NAV_MIN`: the enum-variant
  path scanned every enum by last segment before the static-field path ran,
  and a library enum named `Theme` answered for the user's class.
  `std::cmp::Ordering` does the same to a root-package `class Ordering`.
- **A local's type answering for another declaration's parameter.**
  Constructor bodies emitted their locals into the backend's base name-to-type
  scope, which is never popped. `int[] r` in `FontMetrics`'s constructor then
  answered for `Rect r` in `Gui.fill`, emitted later: the parameter was taken
  for an array handle and lent as `r.borrow().clone()`. A foreign-typed
  parameter the backend cannot type registers nothing, so the stale entry was
  what the lookup found. Renaming the parameter "fixed" it.

**Resolution.**

- A bare call of a record's or an enum's own static method is
  `Self::name(...)`; an enum's own instance method called bare is a call on
  `self`, as in a class. The resolver puts an enum's methods in scope in its
  bodies, as it does for classes and records.
- A bare name that the unit resolves to one of the program's own classes,
  records or interfaces (through its imports, in its own package, or in the
  root package) is never re-read as a library enum of the same last segment.
  An import of the enum still wins, as the import says which type is meant.
- A constructor's locals live in a scope of their own, and every top-level
  declaration starts from an empty base scope, so no declaration's locals can
  answer for another's names.

`examples/own_static_calls.jux`, `examples/user_type_named_like_library_enum.jux`
and `examples/local_names_do_not_leak.jux` run the three shapes; the leaker
app builds with its palette class named `Theme` and `Gui.fill(Ui, Rect r, ...)`.

**Spec status:** no addendum text changes; §M.16.6 (E102) and §M.16 (E117)
already say this.

---

## E125. An `E0900` names its rustc error in every format

**Conflict.** ERRATA E116 made a rustc rejection of the emitted crate an
`E0900` in Jux words, with rustc's code and message as a note and the full
report behind `--verbose`. The `line` format, the default whenever stderr is
not a terminal (an IDE console, a script, a pipe; ERRATA E72), prints no
notes, and neither do `short` and `compact`. So the one line a user saw was
"internal compiler error: the Rust generated for this code does not
compile", once per statement, and finding the cause meant `--verbose` and
reading the generated crate (LEAKS L27).

**Resolution.** The one-line formats append the cause to an `E0900`:

```text
Gui.jux:66:9: [E0900] error: internal compiler error: the Rust generated for this code does not compile (rustc reported error[E0599]: no method named `borrow` found for struct `Rect` in the current scope; a compiler bug, `--verbose` shows the full report)
```

The note carries rustc's first message line only; where it points in the
generated crate is a note of its own, which `human` and `json` show and the
one-line formats leave out. The message itself is unchanged (the Jux words
E116 chose), so the JSON `message` and every test keyed on it are too.

**Spec status:** `JUX-DIAGNOSTICS-ADDENDUM.md` is unchanged; this is
rendering. E116's `E0900` bullet is refined by this entry.

---

## E126. How much Rust shows through a crate's API

**Conflict.** LEAKS L28 lists what a Java developer meets when using a Rust
crate: snake_case member names, borrow markers and machine annotations in the
stubs, `(ubyte)` casts, and a nightly toolchain plus a slow first build for
stub generation.

**Resolution.** Each item against the spec:

- **Names stay verbatim.** `JUX-BINDGEN-ADDENDUM.md` §G.4 is explicit: a
  foreign name is kept as the crate spells it, "instead of a camelCased
  alias", so a call reads like the crate's own documentation. No alias is
  generated.
- **Hover, completion and signature help show Jux signatures.** They render
  from the checker's signatures, which carry no `&`/`&mut` and no machine
  annotation (§G.3.4, borrows vanish); a test now pins that. The markers are
  in the `.jux.d` text only, which go-to-definition opens (§G.10.2).
- **`(ubyte)` casts.** An integer literal that fits takes a `ubyte`
  parameter's type with no cast (§S.2.6); an `int` VALUE into a `ubyte` slot
  is a narrowing, which §S.2.7 requires to be written. Both are the rules for
  any Jux function, not a foreign special case.
- **No nightly toolchain is required.** A `rust.<crate>` stub is read from
  rustdoc JSON, which rustdoc only writes behind `-Z unstable-options`. The
  nightly toolchain is still asked first; when it is not installed, the
  default toolchain's rustdoc is run with `RUSTC_BOOTSTRAP=1`. On the pinned
  stable (1.96) that JSON is format 57, the one `rustdoc-types` 0.57 reads,
  and the `semver` and `rand` stubs generated from it are identical to the
  nightly ones but for the header line. The error, when both fail, names the
  crate and the one command that fixes it.
- **The first build.** Generating a large crate's stub (eframe) takes about
  a minute, once per crate version; the stub is cached in `.jux-stubs/`
  (§G.11.2). Lazy per-item generation remains §G.13's open question.

**Spec status:** no addendum text changes. INSTALL.md says when nightly is
used.

---

## E127. A borrowed foreign object is lent, not copied

**Conflict.** Bindgen §G.3.4 says borrows vanish: `&T` and `&mut T` have no
Jux spelling, and the call site re-derives the borrow. A Java programmer
reads that as "objects are passed by reference", and for Jux classes and
collections that is what happens. For a FOREIGN object the lowering did
something else in five places, found by the leaker app (`leaker/LEAKS.md`
L1, L8, L9, L10, L12, L14, L15; GAPS.md gap 30):

- **A lambda lent an object got a clone of it (L1).** A closure slot marked
  `@RustClosureRefs` (every egui container: `CentralPanel.show`,
  `ScrollArea.show`, `ui.horizontal`, ...) is called with references, and
  the lambda started with `let ui = ui.clone();`. egui's `Ui` has no
  `Clone`; it derefs to `Context`, so the clone silently became a `Context`
  and the body failed in rustc (`no method heading on Context`). A foreign
  object that IS `Clone` took the lambda's writes into a copy.
- **How a `Ui` parameter was passed depended on the body (L9).** A foreign
  parameter was `&mut T` only when the body called a `&mut self` method on it
  directly. Handing it on, or calling only `&self` methods, made it by value,
  which no caller holding a lent `Ui` could satisfy; programs opened every
  such method with a no-op mutating call. Handing a `&mut Ui` parameter on to
  a foreign `&mut Ui` slot then wrote `&mut ui` on a binding that is not
  `mut`.
- **A field lent to a `&mut` slot was a temporary copy (L8).**
  `ui.text_edit_singleline(this.name)` became `let __jux_arg0 =
  self.0.borrow().name.clone(); ...(&mut __jux_arg0)`: a compile error, and
  had it compiled, the edit would have gone into the copy.
- **A write through a `&mut` accessor was lost (L10).**
  `ui.spacing_mut().item_spacing = v` lowered to
  `(ui.spacing_mut()).clone().item_spacing = v`, with no diagnostic from
  either compiler. `var s = ui.spacing_mut(); s.indent = 12;` did the same.
- **Hoisting moved what it should lend (L12, L14, L15).** The E119
  argument hoist bound a `&mut Ui` argument to a `let`, moving it
  (`let __jux_arg0 = ui;`), so the next use of `ui` failed. A lambda that
  captured a collection the same call also read (`names.len()`) moved the
  collection while that read's guard was alive. A `String` read in one arm of
  a `?:`, or passed to a class's own static method by its bare name, was
  moved though the program read it again.

**Resolution.** One convention: a foreign object is the crate's object, and
Jux code works on it in place.

- **A lent object stays the reference.** In a `@RustClosureRefs` slot, a
  lambda parameter whose type is a foreign class (a stub class that is not a
  §6.5.1 collection) is not cloned: it is the `&T` / `&mut T` the crate
  passes, and the body calls through it, lends it on with a reborrow
  (`&mut *ui`), and hands it to Jux methods. Parameters of Jux value types
  (primitives, `String`, records, Jux classes, collections) are still cloned
  out, as §G.6.4.4 says for iterator adaptors.
- **One convention for a foreign parameter.** A method or function parameter
  of a foreign class with no `@RustClone` is a borrow, whatever the body calls
  on it, and every use lends it on: `&mut T` when the body writes through it
  or lends it to a slot that does (a foreign `&mut` parameter, a `&mut self`
  method, a Jux parameter that is `&mut` in turn), `&T` when it only reads,
  since a crate may lend its object shared (`Once.call_once_force` passes
  `&OnceState`). A local initialized from such a parameter (`var u = ui;`) is
  the same borrow. The exception is a body that KEEPS
  it: returns it, stores it in a field, an element or a new object, hands it
  to a slot that takes ownership (a foreign by-value parameter, or a Jux
  parameter that keeps it in turn), or lets a closure that may outlive the
  call capture it. Such a parameter is owned, as before. "Keeps in turn" is
  settled by starting every candidate as a borrow and withdrawing each one
  found kept until nothing changes. A foreign type that is `Clone` keeps the
  old rule (a copy, or `&mut T` when the body mutates it), because copying a
  value type is what passing it means. Both answers are fixpoints over the
  whole program. A local that is already a borrow (such a parameter, a lent
  lambda parameter, an alias of one) is lent on as `&mut *ui` everywhere:
  foreign `&mut` slots, Jux by-`&mut` parameters, and bare calls to the
  class's own methods, which now take the same argument path as `Class.m(..)`.
- **A place lent to a foreign `&mut` slot is lent in place.** A local is
  lent where it stands. A field or element is lent through its owner's cell,
  `&mut self.0.borrow_mut().name`, when nothing else in the call can run Jux
  code while the cell is borrowed: no lambda, method reference or function
  value among the other arguments, and a receiver that reads no cell. When
  something can, the value is copied in, lent, and stored back right after the
  call, so the program sees the write afterwards and §CR.4.1 holds during the
  call (no guard is alive while a lambda runs).
- **A `&mut` accessor is written through.** The result of a `@MutSelf
  @RustRefOut` method is not copied when the program writes through it (an
  assignment target, the receiver of a call), and a local initialized from one
  keeps the borrow: `var s = ui.spacing_mut(); s.indent = 12;` changes the
  `Ui`. A `&T` accessor (`spacing()`) is still copied out.
- **Hoisting lends.** A local lent to a `&mut` slot is not hoisted. A lambda's
  capture counts a read of the same binding earlier in the same statement as
  a later use, so it captures a shared copy of the handle. A `?:` arm and a
  bare static call's argument take a copy of a local that is read again.
- **Keeping a lent object is `E0454`.** A lambda that returns a lent foreign
  object with no `Clone`, stores it in a field or element, or passes it to a
  constructor or array literal, is reported in Jux terms, at the use, with
  the lambda parameter labelled. It exists only while the crate's call runs.

**Captured locals.** A Jux lambda captures a local by reference, not by
Java's effectively-final copy: a local the lambda (or the code after it)
reassigns is promoted to a shared cell (already the rule for `Rc<dyn Fn>`
lambdas, Async §18 for workers aside), and objects and collections are shared
handles. The same holds for a lambda lent to a crate: `counter++`,
`names.push(x)` and `obj.bump()` inside `ui.horizontal(...)` are all visible
after the call. This is a deliberate departure from Java, where the first of
the three does not compile.

**Known boundary.** The mutability of a lent reference is not in the stub
(`@RustClosureRefs` lists the slot, not whether each argument is `&` or
`&mut`), so a lambda that writes through an object the crate lends shared
still reaches rustc. `E0454` sees the lambda's own body; a lent object handed
to a Jux method that keeps its parameter is still an `E0900`. A `var`
initialized from a `&mut` accessor holds the borrow while it is live, so
using the accessor's receiver in between is a rustc borrow error.

**Spec status:** `JUX-BINDGEN-ADDENDUM.md` §G.3.4 carries the convention and
§G.6.4.4 the lent closure parameter; `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 has
`E0454`. GAPS.md gap 30 is closed. Tests: `bin/juxc/tests/borrowed_foreign.rs`
builds and runs a program against a small crate with egui's shapes under the
E119 self-check; `examples/lent_foreign_closure.jux` runs one through
`std::sync::Once`; `tests/ui/lent_object_kept.jux` pins `E0454`.

---

## E128. A crate's API as the crate's users reach it

**Conflict.** LEAKS L2-L7, L11, L13 and L16-L21: bindgen's type map (§G.3.1)
folded away facts a call needs. `impl Into<WidgetText>` was surfaced as
`WidgetText`, so a Jux `String` was refused where Rust takes it; `impl Widget`
was lowered as a `Box<dyn crate::rust::eframe::Widget>` naming a module that
does not exist; `Arc<RichText>` and `&Arc<Style>` were erased with nothing to
put them back; associated constants, tuple structs, named-field variants and
`HashMap`/`BTreeMap` parameters had no Jux spelling at all; a crate re-exported
by the bound one (`eframe` -> `egui` -> `ecolor`) was either missing or
declared twice under two Jux names; a keyword-named field (`operator`) took
the rest of a stub down and was reported as a duplicate `main` in the user's
file; a removed dependency's stub stayed loaded.

**Resolution.**

- **Generic slots (§G.3.6).** A parameter written `impl Trait` or `&dyn Trait`
  is marked `@RustImpl`. It accepts any value that converts into its declared
  class (the class's own `From` impls, `@RustFrom("String,RichText")`, and a
  blanket `From<T: Into<Y>>` as `@RustFromInto("Y")`) or meets its declared
  trait (an `implements` clause, a type the trait's impls name,
  `@RustImplementedBy("String")`, or a blanket bound,
  `@RustBlanket("Into<Atom>")` / `@RustBlanket("Hash + Debug")`, where the
  standard traits are answered from the `@RustHash`/`@RustDebug`/... markers
  and the built-in types). A text value fills a borrowed-view slot
  (`impl AsRef<Path>`). The call passes the value as it is.
- **Erased pointers.** A parameter or variant slot whose Rust type is
  `Arc<T>`/`Rc<T>`/`Box<T>` carries `@RustArc`/`@RustRc`/`@RustBox`, and the
  argument is `.into()`-wrapped. A method returning one (`&Arc<Style>`) is
  `@RustDerefOut`: the program gets the pointee, copied out.
- **Associated constants** are `static final` fields (`Color32.RED`).
- **Tuple structs** (`Mm(pub f32)`) get a `@RustTuple` constructor and `_0`,
  `_1` fields, lowered to the struct expression and `.0`.
- **Named-field variants** keep their payload; `@RustStructVariants` records
  the field names and the backend builds them with braces.
- **Maps and sets keep their own names** (`HashMap<K, V>`, `BTreeSet<T>`), as
  `Vec` does, so a program can build the value a parameter takes. The
  canonical `Map`/`Set` of §G.3.1's old rows named nothing constructible.
- **A collection is built from elements and iterated** (`Extend` or
  `FromIterator`, and `IntoIterator`). genpdf's `Style` merges into itself and
  is not iterated, so it is a value again.
- **A foreign stub's bare names are Rust's**: `Path` in `rust.genpdf` is
  `rust.std.Path`, never the `jux.std.io.Path` helper.
- **Crate families (§G.6.2.4).** A bound crate's stub also takes the crates
  it re-exports and those defining a type its signatures mention, as far as
  the host publishes them. A name several members declare means the one the
  family publishes (`emath::Rect`, not `accesskit::Rect`). Every path is
  written through the host (`eframe::egui::Color32`). A member the program
  binds in its own right is declared once, by the dependency with the
  smallest family holding it, and aliased everywhere else, so `rust.egui.Ui`
  and `rust.eframe.Ui` are one type. The stub's second line records its
  family; the cache marker records the other bound crates.
- **Only declared dependencies' stubs load (§G.9.4).** A generated stub no
  dependency names any more is deleted; a hand-vendored one is left unread.
- **Keyword-named fields.** `operator` followed by `;`, `=` or `,` is a field
  name, not an operator declaration.
- **A stub that reads as statements is `E0907`.** Text the parser took as
  top-level statements (script mode) is dropped from the stub and reported
  against the stub file and line, as a bindgen internal error; it never
  becomes a second `main`.
- **Read-only closure arguments (gap 30's follow-up).** A closure slot records
  which arguments the crate lends as `&T` rather than `&mut T`,
  `@RustClosureShared("0:0")`. A lambda that assigns through one, or calls a
  `@MutSelf` method on it, is `E0488` at the write, not rustc's E0594/E0596.

**Known boundary.** A borrowed parameter of a Jux method (`void f(Ui ui)`)
passed on to a foreign `&mut` slot still depends on gap 30's lowering (L9);
that is not a type-mapping question. A generic slot whose bound this stub
cannot see (a trait from a crate outside the family) accepts only what the
recorded markers prove.

**Spec status:** `JUX-BINDGEN-ADDENDUM.md` §G.3.1 (map and set rows), new
§G.3.6 and §G.3.7, §G.6.2.4, §G.6.6 and §G.12 carry the rules; the E0907 row
of `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 is no longer reserved.

---

## E129. Rust never reaches the user: one detector, a guard at every exit

**Conflict.** E116 promised that Jux never prints raw Rust, and gaps 27-32
kept that promise one leak at a time, each found by someone reading output.
Nothing made a new leak impossible to ship. Probing every exit found more:

- **Run-time failures printed the Rust runtime's text.** The emitted program's
  hook printed `panic: ` and Rust's message as it stood: `index out of
  bounds: the len is 3 but the index is 5` (a negative index read
  `18446744073709551615`), `attempt to add with overflow`, `no entry found for
  key`, `called Option::unwrap() on a None value` for an empty array's
  `pop()`, and an `assert` with no message named the condition as the
  generated code spelled it, `assertion failed: v.borrow().len() > 0`. A stack
  overflow printed `thread 'main' has overflowed its stack`. A failed
  `File.readText` was an `unwrap` on an `Err` carrying the OS error's Rust
  `Debug`. No report named the `.jux` line, and a borrow conflict with no
  source marker named `src/main.rs:L:C of the generated Rust`.
- **Compiler text named Rust.** `E0436` explained itself with `Rc<dyn
  Trait>`; `E0900`'s help and `E0904`'s note said `rustc`; the self-check's
  `E0900` note gave a `.rs` location; a build that failed with no compiler
  error in it (a registry out of reach) passed cargo's report through behind
  "`cargo build` failed for the emitted Rust crate"; a missing toolchain said
  "`cargo` is not installed"; the crash report of an internal compiler error
  showed the compiler's `.rs` location, Rust's panic text and
  `RUST_BACKTRACE`; `juxc --help` and `jux --help` talked about `cargo build`,
  `rustc` and `rustup target`.
- **A foreign value printed its Rust `Debug`**: `print(new Mutex<int>(5))` was
  `Mutex { data: 5, poisoned: false, .. }`.

**Resolution.** Leaks are prevented where the text is made; one detector
guards every exit as the safety net, and the test suite runs it over
everything pinned.

- **One detector**, `juxc_diagnostics::leak::find_rust_leak` (and
  `find_rust_leak_quoting`, told what the program's own text is). It knows
  Rust paths (`std::`, `core::`, `alloc::`), `&mut `, `Rc<`, `Arc<`,
  `RefCell`, `Box<dyn`, `dyn Trait`, `impl Trait`, lifetimes in type position,
  rustc's `error[E0502]` inside a sentence, `.rs` locations, `cargo`,
  `rustc`, `Cargo.toml`, the emitted crate's `__jux` names and `JuxCell`, and
  the runtime's panic phrases. It skips what the programmer wrote: backtick
  and double-quote spans (examined only when the program does not contain
  them), fenced blocks, `N |` source frames, and any match the program's text
  contains. Jux's own `Type::method`, `Map<String, int>`, `'a'` and the
  `error[E0410]:` header are not hits.
- **Exits and their guards.** Each is marked *at source* (the text can no
  longer be produced) or *at exit* (the guard catches what is left).
  - Diagnostics, `human`/`compact`/`short`/`line`/`json`
    (`render::guard_leaks`): *at exit*. A diagnostic whose message, label,
    note, help or fix title shows Rust becomes an `E0900` internal compiler
    error at the same place, naming the code it replaces ("its wording named
    the code the program is compiled to"), with the original text under
    `--verbose`. The one exemption is `E0900`'s documented rustc note (E116,
    E125). Under `JUX_SELFCHECK=1` and in unit tests the guard panics instead;
    the UI harness now sets the variable, so a leaking diagnostic fails its
    case. The texts above are fixed *at source*.
  - Errors that are not diagnostics (`ice::report_error`, both binaries):
    *at exit*; replaced by a sentence pointing at `--verbose`, status 1.
  - The build with no compiler error: *at source*, now `E0905` "a dependency
    of the program could not be fetched or built", with the first `error:`
    line when it reads as Jux and the report under `--verbose`.
  - The internal-compiler-error report: *at source*; the message in Jux words
    (`leak::jux_panic_wording`), the site as the compiler module
    (`juxc-tycheck/src/check:1204:9`), `JUX_BACKTRACE=1` (the runtime's own
    variable still works).
  - The language server (`juxc-lsp/src/leak_guard.rs`): hover, completion
    label/detail/documentation, signature help, *at exit* (withheld on a hit);
    published diagnostics through the compiler's guard, *at exit*. A doc
    comment copied from a declaration is that file's own text.
  - The emitted program, *at source*: an array, `Vec` or `VecDeque` position
    is the prelude's `JuxIx`, whose `Index`/`IndexMut` throw
    `IndexOutOfBoundsException("Index 5 out of bounds for length 3")`, a
    negative position included, as do `get`, `set`, `first` and `last`; an
    empty array's `pop()` throws `NoSuchElementException("the array is
    empty")`; `new T[n]` with a negative `n` throws
    `IllegalArgumentException("Negative array size: -2")`; a failed `File`
    operation throws `FileNotFoundException` or `IOException` with the
    system's reason; `assert` with no message says `assertion failed`; the
    `!!` and cast exceptions no longer repeat their class name in the message.
  - The emitted program, *at exit*: every entry point runs under the
    prelude's `jux_install_panic_hook`. A Rust panic left over (integer
    overflow in a debug build, a missing map key, a `Vec` `remove`/`insert`
    position, a raw cell conflict) prints `panic: <Jux wording>` and `    at
    <file>.jux:<line>:<col>` from `__JUX_LINES`; an uncaught exception prints
    `Exception in thread "main" <fqn>: <message>` and the same `at` line
    (helpers that throw carry `#[track_caller]`, so the line is the
    program's), and an instance of a generic exception class, which had no
    report at all, says so. The status of an uncaught failure stays 101, as
    before. On Windows a stack overflow is reported by a vectored exception
    handler, `panic: stack overflow: a method called itself too many times
    without finishing`, status 101.
  - `print` of a foreign value with no `Display` (`jux_debug_as_jux`), *at
    source*: a type from outside the program has its cells printed as their
    value (`RefCell`, `Cell`, `Mutex`, `RwLock`), `Some(x)` as `x`, and a
    field's `None` as `null`.
- **CI.** `bin/juxc/tests/leak_sweep.rs` runs the detector over every
  `tests/ui/*.expected` and `tests/expected/examples/*.expected`, told each
  case's own source; `tests/leak-allowlist.txt` is empty and a stale entry
  fails. `bin/jux/tests/runtime_failures.rs` runs
  `examples/runtime_index_out_of_bounds.jux`, `runtime_integer_overflow`,
  `runtime_assert_failed`, `runtime_missing_map_key` (all four also pinned
  word for word by the corpus) and `runtime_stack_overflow` and holds their
  output to the detector with nothing excused.

**Not guarded.** Resolved by gap 38 (E134): ~~a stack overflow off
Windows still prints the runtime's report~~ (Linux and macOS now report it in
Jux terms); ~~a foreign type's `Debug` text other than cells and nullables
(`OnceLock(<uninit>)`) is the crate author's~~ (laid out in Jux's form; a
type's own `Display` is still its author's text, by design); ~~a foreign `Err`
is reported under its Rust type name (`ParseFloatError`, Bindgen G.5.4)~~ (it
is a Jux exception); ~~a crate's doc comments shown on hover are the stub's own
text~~ (rewritten in Jux terms and held to the detector). `juxc explain` prints the
specification's prose, which discusses the Rust lowering. Informational lines
the `jux` tool prints itself (`jux: ...`) do not pass the detector; the
corpus sweep covers the ones that are pinned. `--verbose` output is the
build tools' report, by design.

**Spec status:** `JUX-SEMANTICS-ADDENDUM.md` §S.7 listed an array bounds
violation among the panics and said the report "follows the Rust std"; an
array or list position outside the sequence now throws
`IndexOutOfBoundsException`, as a `String` position already did (§K.7) and
integer division by zero does (E1), and the report is the one above. §S.7.1
is otherwise unchanged: overflow and a failed `assert` are still panics, not
catchable. `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4: `E0905` is raised, and the
`E0900` row names the leak guard as a third source. GAPS.md gap 33 is closed.
---

## E131. The release-blocker sweep: what a valid program could not do, and what an invalid one was told

**Conflict.** The 2026-09-24 release-blocker list (FEATURES-TODO) was
re-probed against the built toolchain on 2026-09-27. Most items had been
closed by gaps 1-33, but eighteen shapes still failed, each against a rule the
specification already states. A valid program must build and do what it says;
an invalid one must be told why in Jux terms, and an internal compiler error
(`E0900`) is a compiler bug whatever the source. These did neither:

- valid programs that did not build: `print(switch (e) { case 1 -> "ab " + e;
  default -> "c"; })`; `$v` over a generic `T` field where `${v}` worked; a
  property getter reading a field by its bare name (`get -> f.length()`,
  `get -> n` into a `long`, `get -> n * 1.5`, `get -> f.charLength`); a
  property named with a Rust keyword (`public int loop { get; set; }`); a
  record implementing an interface and calling through its component
  (`this.inner.eval()`); a null test inside an interface's `default` method; an
  `AsyncMutex` held in a class field (ASYNC §18.3's own example); a class in
  the root `main.jux` extending a class of a named package;
  `new int[-1]` written with a literal; `Task.yield()`;
- invalid programs answered with `E0900` or nothing: `Integer.MAX_VALUE` and
  any dotted read whose head names nothing; an empty file (or one with no
  entry point); `interface I extends J {}` with `interface J extends I {}`;
  `spawn(makeUser("bob"))` on a function that is not async; `Result.ok(1)`
  (§7.11's spelling, not provided since E56); `@export(convention =
  "Stdcall")` and a convention that names nothing, both silently emitted as C;
  `new app.model.Secret()` on a package-private type from another package
  (the import was `E0416`, the written FQN compiled and ran); a keyword as a
  parameter or local name (`int f(int type)`), a four-error cascade starting
  at a "bare `return`"; `ref` on a class-typed PARAMETER (no `W0490`, which a
  local and a field get);
- text that showed Rust: printing a curried lambda gave `<Rc>`; the formatter
  the build runs printed its own parse errors with the generated file's path;
  `W0457` said classes are "`Rc`-refcounted"; `juxc explain` printed the
  specification's notes about the lowering; the missing-toolchain message for
  a bound crate said "`cargo` must be on PATH".

**Resolution.** Each is fixed where the text or the code is made.

- **`$name` is `${name}`.** The parser gives `$v` the expression segment
  `${v}` produces, a one-segment path with a real span, so the two are typed
  and lowered identically (§3.4). The separate bare segment had no span and no
  recorded type, and the backend guessed `Display`.
- **A `String` switch's arms agree.** When a `String`-typed switch has any arm
  that is not a plain literal, every arm is emitted as an owned value, the rule
  a `? :` already followed.
- **A synthesized `this` has a span of its own.** An accessor body's bare
  field read becomes `this.f` (§M.7); the `this` now takes an empty span at the
  name's start rather than the name's span, which the per-expression type map
  overwrote with the class's type. Every String method, widening and promotion
  in a getter follows from that one change.
- **A property's getter is called by its escaped name** in the observer
  bracket, as every other member already was.
- **A record's methods take `&self`.** Components are final (§7.6), so a call
  on one never needs the record mutable; the name-keyed mutating-method set
  had marked `eval` mutating and the record's interface impl could not call it.
- **A bare call in an interface default method is typed by the interface's
  method**, so `var e = get(k)` is known nullable and the null test narrows it.
- **`AsyncMutex.lock()` hands out a guard that owns its share of the lock**,
  and the handle has a `Debug` form, so a field of that type compiles.
- **A root-package subclass is named from a packaged base as `crate::Phone`.**
- **A negative constant array size is a run-time size**: it throws
  `IllegalArgumentException("Negative array size: -1")` exactly as the same
  size in a variable does (Java compiles it too).
- **`Task.yield()`** is the built-in `yield_now()`: it hands the loop back
  once (LANG-V1 §10's fairness note names it).
- **A dotted read whose head names nothing visible is `E0301`**, as a bare
  unknown name is. `Integer`/`Long`/`Short`/`Byte`/`Double`/`Float` with
  `MAX_VALUE`, `MIN_VALUE`, `NaN` or an infinity get the Jux spelling as help,
  `int.MAX_VALUE` (Core lib §K.11); any other member gets the existing
  boxed-number help.
- **`E0327`**: a program built as a binary with no entry point (no `main`, no
  class `static main`, no `@entry`, no top-level statements) is reported by the
  build, since the same file is a valid library member and `jux check` of it
  stays clean. It replaces the `E0900` an empty file produced.
- **`E0434` covers interfaces**: the interface whose `extends` link closes a
  cycle is reported and that link cut, as for classes.
- **`E0708`**: `spawn(f())` where `f` is known and not async. The call has
  already run; `spawn` takes a function or an async call's future (§10.1.3).
- **`E0413` for an instance method called through its type**: `Result.ok(1)`
  says `ok` is called on a value, with help naming `Result.Ok(value)`. A record's
  or enum's static set is its variants and its `static` methods; an instance
  method of the same name no longer passes as one.
- **`E0527`**: `@export(convention = ...)` other than `"C"`. Every export is
  emitted with the C convention, so the three Windows values of Layout-ABI
  §L.4.1 are refused on every target, and a value that is not a convention is
  refused as such, the way `E0322` treats `@entry`'s.
- **`E0416` for a written FQN**: `new app.model.Secret()` is checked against
  the declaration it names.
- **`E0204`**: a Jux keyword as the name of a parameter or local variable
  (Grammar §A.1.3 reserves every one). It is reported once; the parser then
  reads the keyword as that name wherever it appears in an expression, so
  nothing cascades. A foreign stub keeps its Rust names, and a parameter
  literally named `this` parses as it did.
- **`W0490` on a `ref` parameter** of a class, interface, array or collection
  type, as on a local and a field (E84).
- **Text.** A function value held by its own closure type prints `<fn>`, as
  one held as a function type already did. The formatter's output is captured
  and dropped; it is about a file the programmer never wrote, and the build
  after it reports in Jux terms. `W0457` says "classes are reference-counted,
  and objects that refer to each other in a cycle are never freed". A bound
  crate's missing toolchain is said by what it is for, as the build's own is
  (gap 33), and the crate-description failures name the dependency, not the
  tools. `jux build` reports "built library at", and `jux target list` without
  a toolchain says what to install rather than the tool's name.
- **`juxc explain` is written for the person who got the diagnostic.** It
  keeps the catalog description, the code's own documentation and the
  addendum sections about it, and leaves out every paragraph (and every
  sentence of the description) that talks about the lowering, the generated
  code, the build tools or the ERRATA history; `Defined in` names the
  specification section without the ERRATA entry. `E0900`, `E0904` and `E0305`,
  whose catalog rows are about the compiler, have a user-facing description of
  their own. A unit test holds every code's explanation to that rule.

**Not changed.** `Result.unwrap()` on an `Err` still throws
`IllegalStateException` (E56's decision). `Result.from(() -> ...)` (§X.5.4) is
still not provided; it is a clean `E0413` with help naming the variants. A
stack overflow off Windows and a foreign type's own `Display` text stayed as
E129 left them (the first, and foreign `Debug` text, are closed by gap 38).

**Spec status:** `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 gains `E0204`, `E0327`,
`E0527` and `E0708`, and the `E0434` row names interfaces.
`JUX-ENTRY-POINTS-ADDENDUM.md` §E.6 lists `E0327`; `JUX-LAYOUT-ABI-ADDENDUM.md`
§L.4.1 records that only `"C"` is accepted today. GAPS.md gap 35 is closed.

---

## E130. An accepted program builds: the lowering heals itself

**Conflict.** E23 makes every rustc rejection of the emitted crate a compiler
bug, and E116 and E129 made sure the user sees such a bug in Jux words
(`E0900`) rather than as Rust. Neither made the program build. A valid Jux
program that met a backend bug stopped with an internal compiler error, and
two were found in one afternoon with the release build:

- `print(switch (3) { case 1 -> "ab " + 1; default -> "c"; })` was rustc's
  `E0308` ("`match` arms have incompatible types": a `format!` `String`
  against a `&str` literal);
- `$"Box($v)"` over a generic `T` field was `E0277` (`T` has no `Display`),
  while `${v}` worked.

Each fast-path judgement of the backend (move on the last read, bind an
operand only when a guard would be alive across Jux code, the tightest class
representation, a `let` left to inference) is a place such a bug lives.

**Resolution.** The backend has two lowering levels, and the driver retries a
refused build at the safer one. The user sees nothing.

- **Two levels** (`crates/juxc-backend-rust/src/lowering_level.rs`). *Fast*
  is the lowering as it stood. *Safe* is the same lowering with five switches
  turned to their general answer, per function:

  | switch | safe level | rustc errors it is designed to eliminate |
  |---|---|---|
  | local reads | every read of a non-`Copy` local copies; every lambda capture shares | `E0382`, `E0505`, `E0507` |
  | operands and call arguments | every operand of an operator chain, interpolated string, constructor or array literal, and every call argument, that reads a cell or runs Jux code is bound to a `let` first, in evaluation order | `E0499`, `E0502`, `E0506`, `E0716`, and the run-time "object of type C was already in use" |
  | class representation | `rc-refcell` for the classes a safe function belongs to or names (all classes when the whole program is safe); not reported as a selector fallback | `E0382`/`E0507` on a copied Inline or `Box` object, `E0594`/`E0596` |
  | `var` declarations | a primitive or `String` local (nullable or not) is declared with its checked type | `E0282`, `E0283` |
  | `switch` arms | a value arm of a `String` switch is made an owned `String` | `E0308` in match arms (`String` against `&str`) |

  Parameters stay as the fast level passes them: a Jux parameter is already
  an owned value or a shared handle, and a foreign one follows E127's
  convention, which the safe level must not break. A call to an overloaded
  function or method, and a call or constructor with named arguments (which
  has its own evaluation order), keep the fast argument rule: the binding
  form does not name the overload member and would reorder the arguments.
- **The retry loop** (`crates/juxc-driver/src/self_heal.rs`). The driver
  keeps the checked program alive in the crate it lowered
  (`RustCrate::relower`), with every Jux function's source region
  (`function_regions`: methods, constructors, operators, properties,
  initializer blocks; nested types; an overload set is one function). When
  cargo fails with an `E0900` (rustc refused the emitted code), each error's
  primary span in the JSON messages is traced through the `// JUX:` markers
  to its `.jux` line, and from there to the innermost function holding it.
  Those functions are lowered again at the safe level and the crate
  rebuilt; at most twice. If an error lies in no function (a declaration's
  own shape), or the retries do not converge, the whole program is lowered
  safe, once. Only if that fails too is the fast build's `E0900` reported,
  exactly as before: its error names the bug. A dependency that cannot be
  fetched (`E0905`) or a link failure (`E0906`) is not the lowering's fault
  and does not retry.
- **Cache.** A heal writes `.jux-safe-fns` in the build directory (the
  function keys, or `*` for the whole program, under a header naming the
  compiler version), and the next build lowers those functions safe from the
  start. A cache from another compiler version is ignored.
- **What the user sees.** Nothing. Under `--verbose`, `note: retrying
  `C.m` in compatibility mode` for each retry and `note: N functions
  compiled in compatibility mode` when the build succeeds.
- **The compiler's own tests.** Under `JUX_SELFCHECK=1` (set by the corpus,
  the UI harness and every test that builds an example) a heal is a failure:
  an `E0900` per function, "`C.m` compiled only in compatibility mode", at
  the `.jux` line of its fast-path error, with rustc's code and first line in
  the note the leak guard allows (E125), exit status 101. The cache is
  neither read nor written. The fast path is still fixed; users get a working
  program meanwhile.
- **Knobs.** `JUX_FORCE_SAFE=1` lowers everything at the safe level.
  `JUX_TEST_BREAK_FAST=<key>` is a test-only hook: the named function's fast
  lowering opens with a statement rustc refuses as `E0502`, so the tests can
  watch a heal.
- **Tests.** Unit tests in `self_heal.rs` (rustc JSON to errors, errors to
  functions through the markers, the retry plan, the cache, the self-check
  report) and `lowering_level.rs` (the innermost region, the plan's scope).
  `bin/jux/tests/safe_mode.rs` builds 97 examples (classes and their
  representations, re-entrancy, generics, closures, async and workers,
  collections, `rust.*` bindings) with `JUX_FORCE_SAFE=1` under the self-check
  and holds each to the fast corpus's own `.expected`; with
  `JUX_SAFE_CORPUS=all` it runs the whole corpus, and all 444 pinned examples
  print the same fully safe (five differed on the first run, each a place
  where a safe switch was too eager: a pointer local's written type, a `never`
  arm, named-argument order, an overloaded call; each is now excluded as
  above). The same file breaks `Counter.bump` with the hook and checks that
  the build heals and prints the right answer, that the cache skips the retry
  next time, and that under `JUX_SELFCHECK=1` the build fails with the
  function and `error[E0502]` named.

**The two known cases.** The `switch` arm case is rescued: the retry lowers
`main` safe and the program prints `c`. The `$v` case is not: the safe level
has no switch for a missing trait bound (`E0277`), so after the whole-program
retry the user sees the same `E0900` as before. Both are fixed at source
elsewhere (gap 35).

**Known boundary.** Safe mode is designed against the borrow, move and
inference families and `String`/`&str` arm disagreement. It does not help an
error that is not about ownership or inference: a missing trait bound
(`E0277`), a method or path that does not exist (`E0599`, `E0433`), a wrong
arity or a mismatched type outside a match arm (`E0308`). The trace goes
through the statement markers, so an error in code emitted between two
functions is charged to the function above it, which costs a retry, not a
wrong answer. A whole-program retry doubles a failing build's time. The
retry needs the checked program, so a crate lowered without the driver
(the backend's own tests) cannot heal.

**Spec status:** `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4's `E0900` row says a
successful retry is silent, that the reported error is the fast lowering's,
and what `JUX_SELFCHECK=1` reports instead. GAPS.md gap 34 is closed.

---

## E132. Two types of one simple name in a crate family, and a crate's deprecated items

**Conflict.** §G.6.2.4 (E128) gave a crate family one Jux name per simple
name: when two members declare the same one, the host's item (or the one the
family publishes) wins, and the other was dropped. `eframe::Frame` (the
window's surroundings) and `egui::Frame` (a panel's fill, stroke and margins)
are both `Frame`, so `egui::Frame` could not be named, and every egui
signature that takes one (`Panel.frame`, `CentralPanel.frame`,
`TextEdit.frame`, `Ui.dnd_drop_zone`) was written with the bare `Frame`, which
meant eframe's (LEAKS L29). The same held for any shared name: `accesskit`'s
`Rect` against `emath`'s, `naga`'s `Error` against `wgpu`'s. Separately, a
crate's `#[deprecated]` items were not marked, so the program learned of one
only from rustc's warning under `--verbose` (LEAKS L39). And nothing reported
a simple name two wildcard imports bring for different types: the first one
seen was silently used.

**Resolution.**

- **Nested packages that follow the Rust path.** A family stub keeps one
  declaration per Rust type in the host's package, as before, and adds a
  NESTED package for each module the host publishes a member's items under:
  `eframe::egui::Frame` is `rust.eframe.egui.Frame`, next to
  `rust.eframe.Frame`. The module is the item's SHORTEST public path in its own
  crate, through the host (`egui::Frame`, not `egui::containers::frame::Frame`),
  so the Jux FQN is the path a Rust program would write. Each type the host's
  package declares is also an alias in its nested package
  (`public type Ui = rust.eframe.Ui;` in `rust.eframe.egui`), so
  `import rust.eframe.egui.*;` reaches all of egui; a type that lost a shared
  simple name is DECLARED there, with its methods. A nested package's file
  imports the host's package, so its signatures read bare names as the host's
  do. A loser the host publishes no path to has no Jux name; a member that
  mentions it is left out of the stub rather than typed with the winner.
- **Every signature names the exact type.** Before any type of the family is
  mapped, bindgen decides every shared simple name (the winner by
  `FamilyPaths::rank`, as before), and every reference to a participant, in a
  parameter, return, field, variant payload or trait-object position, is
  written with its qualified name: `public Panel frame(rust.eframe.egui.Frame
  frame)`, `run((Ui, rust.eframe.Frame) -> void app)`. A member another
  package of the program declares (`rust.egui` bound too) is that package's
  type, `rust.egui.Frame`, and the nested package aliases it.
- **Files.** The nested packages of `.jux-stubs/rust/eframe.jux.d` are
  written to `.jux-stubs/rust/eframe/<nested package>.jux.d`, regenerated with
  the stub, loaded with it, and never pruned while it is declared.
  `CRATE_STUB_CACHE_VERSION` is 22.
- **Import conflicts, in Java's terms.** Two single-type imports binding one
  simple name to different types stay `E0303`, whose text now also says to
  import only one of them; two spellings of one type (`rust.eframe.Ui` and its
  alias `rust.eframe.egui.Ui`) are one import. A simple name two wildcard
  imports bring for different types is `E0303` where it is USED, in a type, a
  `new` or a static access, naming both candidates and the single-type import
  that settles it (JLS 6.5.5.1); an unused one is not an error, and a
  declaration of the unit's package or a single-type import of the name wins
  as before.
- **Deprecated items.** A crate's `#[deprecated]` function, method,
  constructor or type is marked `@Deprecated(message = "<its note>")` in the
  stub (the note, else "deprecated since <version>", else no message). A call
  to anything marked `@Deprecated`, the program's own declarations included,
  is `W0491`, "`Panel.show_inside` is deprecated: Renamed to `show`". The
  emitted crate allows rustc's `deprecated` lint, so the toolchain's copy never
  reaches `--verbose` either. `STD_STUB_CACHE_VERSION` is 47 (std's deprecated
  items are marked too) and the vendored `rust.std` snapshot is regenerated.
- **A static read releases its lock at once (found on the way, LEAKS L40).**
  A `static final` computed at run time lives in a `Mutex`, and a read was
  `X.lock().unwrap().clone()` in place, whose guard lived until the end of the
  statement. A lambda the same statement ran that read the static again
  waited on it forever: the leaker's `Panel.left(..).frame(.. Palette.NAV_BG
  ..).show(ui, (nav) -> .. Palette.NAV_BG ..)` hung before the first frame. A
  read is now `({ let v = X.lock().unwrap().clone(); v })`
  (`examples/static_read_in_chain.jux`).
- **Tests.** `crates/juxc-bindgen/tests/leaks_fixture.rs` over a two-crate
  family in eframe's and egui's shape (`fixtures/family-src/{g37app,g37ui}`,
  real rustdoc JSON): both `Frame`s declared, every signature exact, the
  nested aliases, a member bound in its own right, the deprecation marker.
  `bin/juxc/tests/family_names.rs` generates the stubs from that JSON as the
  driver does, then builds and RUNS programs against the real crates: the
  member's `Frame` imported by its nested name, written by its FQN beside an
  import of the host's, and imported under an alias beside it; importing both
  (`E0303`); two wildcards (`E0303` at a use only); `W0491`. UI:
  `tests/ui/ambiguous_wildcard_import`. The leaker app's side navigation is
  dark again, through `Panel.left(..).frame(new Frame().fill(..))` with
  `import rust.eframe.egui.Frame;`.

**Known boundary.** Two items of one simple name inside ONE crate (naga's
several `Error`s) still collapse to the first, as §G.4.1 always said, and a
reference to the other is written as the one kept. Names inside bindgen's
string markers (`@RustFrom("...")`, `implements`) stay simple.

**Spec status:** `JUX-BINDGEN-ADDENDUM.md` §G.6.2.4 (nested packages, exact
names) and new §G.5.8 (deprecated items); `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4
(`E0303` for a wildcard ambiguity at a use, new `W0491`). GAPS.md gap 37 is
closed; LEAKS L29 and L39 are fixed.

---

## E133. An import alias and a `type` alias mean their target everywhere

**Conflict.** `import app.model.Circle as C;` bound `C` in the unit's name
table, and every part of the checker and backend that asked that table got
it right. Every part that read the SPELLED name did not: `class Sq implements
S` implemented nothing the subtype relation could find (`E0410` "expected
app.model.Shape, found Sq"), the upcast to `S` was not emitted (rustc
`E0308`), `case C c` asked the value for `__jux_as_C` (rustc `E0599`), and
`C.Builder` named no type (rustc `E0223`). `type` aliases had the same shape
at `new`: `new Dict<int>()` with `type Dict<V> = HashMap<String, V>` handed
`<int>` to `HashMap` (`E0410`), `new Keyed<double>(..)` with `type Keyed<K> =
Pair<K, int>` was `E0443`, `new Shapes()` with `type Shapes = Vec<Shape>`
reached rustc (`E0061`), and `class Sq implements Sh` was refused as
implementing an alias (`E0424`). A cycle of `type` aliases overflowed the
checker's stack.

**Resolution.** Both kinds of alias are expanded ONCE, in the AST, before
anything resolves a name (`crates/juxc-tycheck/src/aliases.rs`,
`type_aliases.rs`, over the new mutable walk `juxc_ast::visit_mut`, which
reaches every place a unit names a type: declarations, supertypes, locals,
casts, type tests, `catch`, lambda parameters, generic arguments and bounds,
function types, `new`, `throws`, method references, `case T t` patterns and
the head of a static access). Nothing after it sees an alias.

- **Import aliases.** An alias becomes its target's simple name, with the
  single-type import that name then needs, when the simple name means nothing
  else in the unit (no type of its package, no other import or wildcard type,
  no library type every unit reaches by that name): that is the form every
  part of the compiler reads. Otherwise, which is usually why the alias was
  written (`import rust.eframe.egui.Frame as EguiFrame;` beside eframe's
  `Frame`), it becomes the target's fully-qualified name. The alias's import
  is then removed, since nothing spells the alias any more and its binding
  would make a lookup by simple name take `Vec` for the `HashMap` an alias
  called `Vec` named. An expression head is not rewritten where a local,
  parameter or field of that name is in scope (JLS 6.4.2), nor a type name
  where a generic parameter of that name is. An alias whose name another
  import binds to something else, or that a type of the package has, is left
  as written for `E0303`.
- **`type` aliases.** Each use is replaced by the target with the alias's
  parameters substituted, following aliases of aliases; the target's names
  are qualified where the alias is DECLARED and written back as simple names
  where the use site reads them the same way. The wrong number of type
  arguments is `E0443` ("type alias `Dict` takes 1 type argument, but 2 were
  supplied"), and a cycle is the new `E0498` at each alias in it. The Rust
  program names no alias; one of an interface is no longer emitted as a Rust
  `type` of a trait.
- **A qualified name works where a simple one does.** A type pattern may be
  written `case app.model.Circle c`, a lambda parameter `(app.model.Circle c)
  ->`, `(Vec<int> xs) ->` or `(int[] a) ->`; a supertype by its qualified name
  is found by the obligation check and the backend (which gets it as one
  name, `pack_qualified_supertypes`); `new app.model.Circle.Builder()` builds
  the nested type; a downcast hook names its target from the module it is
  emitted in (a root-package `class Sq` tested for on `app.model.Shape` was
  rustc `E0425`, with plain imports too).
- **Tests.** `examples/multifile/importalias` (every position; an alias that
  is the only name its type has in the file; two aliases of one type; an
  alias called `Vec` for `HashMap`), `examples/type_alias_everywhere.jux`
  (`new` through generic aliases fixing some parameters, of a foreign generic,
  a supertype, an alias of an alias, a static call, type tests and patterns,
  array and function aliases), `bin/juxc/tests/family_names.rs`
  (`import rust.g37app.g37ui.Frame as EguiFrame` beside the host's `Frame`),
  UI `import_alias_clash`, `type_alias_cycle`, `type_alias_arity`.

**A nested type through a qualified owner (gap 37c).** Where the alias's
target shares its simple name with another type of the file, `C.Builder`
becomes `app.model.Circle.Builder`. The checker read a qualified TYPE only as
a declaration or as `<first segment>.<nested>`, so that spelling lowered to
`Unknown`: a slot declared with it took any value, a member chained on the
value (`b.build().r`) had no type, and the backend read the member on the
file's own `Circle` (rustc `E0609`). A type position now tries every split,
longest owner first (`app.model.Circle` + `Builder`, the lifted
`app.model.Circle__Builder`), as the backend already did, and a `new` with
such a name has the same type. `examples/multifile/importalias` chains members
on both spellings.

**Spec status:** `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.4.1 (what an alias binds),
`JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 (`E0443` covers an alias, new `E0498`).

---

## E134. The four leaks E129 left: a stack overflow off Windows, a foreign value's text, a library's error, a crate's docs

**Conflict.** E129 made Rust impossible to ship through every exit it could
guard, and listed what it could not. Four of those still reached a Jux
programmer, each as Rust:

- **A stack overflow on Linux and macOS** printed the runtime's `thread 'main'
  has overflowed its stack` and `fatal runtime error: stack overflow`, and
  aborted, where Windows printed Jux's report and exited 101.
- **A foreign value with no `Display`** printed its Rust `Debug` text beyond
  the cells and nullables E129 rewrote: `OnceLock(<uninit>)`, `"report.txt"`
  (a `PathBuf`, quoted), and struct syntax, `FromUtf8Error { bytes: [104,
  255], error: Utf8Error { valid_up_to: 1, error_len: Some(1) } }`.
- **A library's error kept its Rust type name** (Bindgen §G.5.4): an uncaught
  failed parse ended the program with `Exception in thread "main"
  ParseFloatError: invalid float literal`, and no Jux exception class caught
  it but `Exception` and `Throwable`, so `catch (NumberFormatException e)` did
  not exist to write.
- **A crate's doc comment on hover** was the crate's rustdoc text: intra-doc
  links (``[`Ui::add`]``, ``[`Area`](crate::containers::area::Area)``),
  `Option<T>`, `&str`, `&mut self`, lifetimes, `rust` code blocks and `#
  Safety` sections. The language server's guard excused it as "the declaring
  file's own text"; in practice no stub doc was shown at all, because the
  stub's `@rust("...")` line between the comment and the declaration ended
  the search for it.

**Resolution.** Each is prevented where the text is made.

- **Stack overflow, Linux and macOS (x86_64, aarch64).** At startup the
  prelude's `jux_stack_overflow::install` gives the main thread an alternate
  signal stack (unless it has one; the runtime makes one for each thread it
  starts) and installs a `SIGSEGV`/`SIGBUS` handler with `SA_ONSTACK |
  SA_SIGINFO`, through `extern "C"` declarations of `sigaction`,
  `sigaltstack`, `write`, `_exit`, `getrlimit` and the `pthread` stack
  queries, with `struct sigaction`, `stack_t` and `si_addr`'s offset spelled
  out per target (Linux: glibc and musl agree; macOS). Each thread that runs
  Jux code records where its stack lies: the main thread from its current top
  down to `RLIMIT_STACK` on Linux (the stack grows on demand, and musl's
  `pthread_getattr_np` reports only what is mapped so far) and from
  `pthread_get_stackaddr_np`/`pthread_get_stacksize_np` on macOS; a `Worker`
  thread (`jux_enter_thread`, the first thing its closure runs) from
  `pthread_getattr_np` or the macOS pair. Memory inside a stack never faults,
  so a fault address from 1 MiB below the stack's lowest address (the gap the
  kernel keeps below it) up to its top is the stack running out: the handler
  writes `panic: stack overflow: a method called itself too many times without
  finishing` with `write(2)` (async-signal-safe) and `_exit(101)`, exactly
  the Windows report and status. Any other fault goes to the handler that was
  installed before it (the runtime's), or, when that was the default, restores
  the default so the fault ends the process as it would have. No `.jux` line
  is given, on any platform: the faulting frame is not one a signal handler
  can map to source without unwinding, which is not async-signal-safe. Other
  Unix targets keep the runtime's report.
- **A foreign value's text** (`jux_debug_foreign`, reached for a value whose
  type is from outside the program and has no `Display`; a type with a
  `Display` is printed through it, as before). The `Debug` text is read token
  by token, never inside quotes, and laid out again:

  | `Debug` writes | Jux prints | why |
  |---|---|---|
  | `Foo { a: 1, b: "x" }`, a variant `Circle { r: 1.0 }` | `Foo(a: 1, b: x)`, `Circle(r: 1.0)` | a record's form (`Point(x: 1, y: 2)`) |
  | a trailing `..`; a field holding `PhantomData<T>`; `Foo { .. }` | dropped; dropped; `Foo` | no value to show |
  | `RefCell { value: v }`, `Cell`, `Mutex { data: v, .. }`, `RwLock`, `ManuallyDrop` | `v` | a cell prints what it holds (E129) |
  | `OnceLock(v)`, `OnceCell`, `LazyLock`, `LazyCell`, `Wrapping`, `Saturating`, `Reverse` | `v` | wrappers Jux does not have |
  | `Some(v)` | `v` | a nullable (E107, E112) |
  | `None` in a field, argument or element; `<uninit>` | `null` | a nullable with nothing in it |
  | `None` as the whole value of a type that is not an `Option` | `None` | that is an enum's variant of the name |
  | `"text"` / `'c'` as the whole value, a field or an argument | `text` / `c` | a record's `String` component prints its text |
  | `"text"` in a list, map, set or tuple | `"text"` | a Jux collection quotes its strings (`["a", "b"]`) |
  | `std::path::PathBuf`, `PhantomData<i64>` | `PathBuf`, `PhantomData` | a path's module part and a type's arguments are not values |
  | `[1, 2]`, `{"k": 1}`, `(1, 2)`, `1.5s`, `1..5` | unchanged | already Jux's form |

  Text the reader cannot follow is copied as it stands.
- **A library's error is a Jux exception** (§G.5.4 as amended). The `Err` a
  foreign call returns is thrown as a `JuxForeignError` carrying the Jux
  exception it surfaces as, that class's ancestors, and the Rust value:

  | Rust error | Jux exception |
  |---|---|
  | `ParseIntError`, `ParseFloatError` | `NumberFormatException` (new; extends `IllegalArgumentException`) |
  | `io::Error` of kind `NotFound` / any other | `FileNotFoundException` / `IOException`, without ` (os error N)` |
  | `Utf8Error`, `FromUtf8Error`, `FromUtf16Error` | `EncodingException` |
  | `TryFromIntError` | `ArithmeticException` |
  | `ParseBoolError`, `ParseCharError`, `AddrParseError` | `IllegalArgumentException` |
  | anything else, from any crate | `LibraryException` (new; extends `RuntimeException`), `getMessage()` the error's text, `getLibrary()` the crate (`std` for Rust's own) |

  A `catch` of a Jux exception class takes the error when its Jux exception
  is that class or a subclass (`catch (NumberFormatException e)`, `catch
  (IllegalArgumentException e)`, `catch (Exception e)`), and leaves it whole
  otherwise, so a later clause still sees it. A clause naming the library's
  own error type (`catch (Error e)` for `io::Error`) still receives the Rust
  value with its methods: that is how the original stays reachable, and no
  `cause()` is added. `assertThrows<E>` and an awaited task's failure follow
  the same rule. Uncaught, the program ends with `Exception in thread "main"
  jux.std.exceptions.NumberFormatException: invalid float literal`, the
  `.jux` line, status 101; a spawned task's unhandled failure, a test's
  report and the C-boundary barrier name the Jux class the same way.
  Propagation is an exception's: a foreign call is never a `Result` in Jux,
  so `?` does not apply to it, and a Jux `Result` holding a caught exception
  prints `Err(<message>)` as before. A foreign error is unchecked, as before:
  the stub's `throws` clause does not require a `catch`.
- **A crate's doc on hover and in completion** (`juxc-lsp/src/crate_doc.rs`).
  A doc comment from a `.jux.d` stub is rewritten when it is shown (one line
  in practice, so nothing is cached): an intra-doc link is its target's Jux
  name in code (``[`crate::Slider`]`` is `` `Slider` ``, ``[`Ui::add`]`` is
  `` `Ui.add` ``), a link whose text is prose is that text, a web link stays;
  `Option<T>` is `T?`, `&T`/`&mut T` and lifetimes go, `&str`/`str` is
  `String`, `&self` is `this`, `Box`/`Rc`/`Arc`/`RefCell`/`dyn`/`impl` are
  their inner type, a module path is the name it ends in (`Type.member` for a
  member), and inside code a Rust number type is its Jux name (`u8` is
  `ubyte`); code blocks not marked `jux` or `text` and `# Safety` sections are
  dropped, and a heading left empty goes too. The result is held to the leak
  detector on its own, with only the open buffer as the program's text; a doc
  that still shows Rust is withheld (and fails a test). The doc search skips
  annotation lines between a doc comment and its declaration, so a stub's
  docs are shown at all. The rewrite runs at display time, so stubs generated
  before it, and hand-written ones, read the same.
- **The detector knows rustdoc.** `find_rust_leak` also finds `Option<`,
  `&str`, `&self`, an intra-doc link ``[`...`]``, a code block marked `rust`
  and a `# Safety` heading. No pinned output contains any of them.

**Tests.** `bin/jux/tests/runtime_failures.rs`: the stack-overflow example
must report in Jux terms with status 101 on Windows, Linux and macOS
(x86_64/aarch64); the emitted crate must type-check for
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-apple-darwin` and `aarch64-apple-darwin` where those targets are
installed; an uncaught library error is `NumberFormatException` with its line.
`crates/juxc-backend-rust/src/tests.rs`
(`foreign_debug_text_is_laid_out_the_jux_way`) compiles the prelude's
renderer on its own and holds twenty hand-written cases.
`examples/foreign_debug_text.jux`, `examples/foreign_errors_as_jux_exceptions.jux`
and `examples/rust_error_uncaught.jux` are pinned by the corpus;
`examples/apps/json_lib` tests `assertThrows<NumberFormatException>`.
`crates/juxc-lsp/src/crate_doc.rs` holds the doc rewrite to hand-written
results and runs every doc of the `rust.std` stub (and of any directory in
`JUX_TEST_CRATE_STUBS`, which is how egui's, eframe's, epaint's, emath's and
ecolor's were checked) through it and the detector;
`crates/juxc-lsp/src/tests.rs` (`hover_shows_a_crate_doc_in_jux_terms`) hovers
a fixture stub's type end to end. `crates/juxc-diagnostics/src/leak.rs` pins
the new detector rules.

**Known boundary.** The Unix handler covers the main thread and `Worker`
threads; a crate's own thread that overflows its stack while running a Jux
callback reaches the runtime's handler as before. A foreign error whose Jux
exception is not listed above is a `LibraryException` even when a closer Jux
class exists; the table grows by adding a row. A foreign value inside a Jux
`Result`, printed through `Debug`, shows its structure in the record form
under its own type name (`ParseIntError(kind: InvalidDigit)`).

**Spec status:** `JUX-BINDGEN-ADDENDUM.md` §G.5.4's "Catching a Rust error"
is replaced by the rule above; `JUX-EXCEPTIONS-ADDENDUM.md` §X.1.2 lists
`NumberFormatException` and `LibraryException`. No diagnostic code is added,
so `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 is unchanged. E129's "Not guarded" list
loses these four items. GAPS.md gap 38 is closed.


## E135. Generics at depth: every bound's members typed, a bound only a class can meet, and a parameter usable as its bound

**Conflict.** The requirement is that deeply nested generics with many
parameters, and a type parameter that extends a class and implements several
interfaces, are no issue: a valid program builds and runs, an invalid one gets
a Jux diagnostic. A sweep of probes against the built toolchain (with
`JUX_SELFCHECK=1`, so a safe-mode heal counts as a failure) found these gaps.
Nesting itself (six parameters, six levels, lambdas, records, enums, async,
`var`, the diamond, wildcards, aliases) already worked; the gaps were in what
a bound promises and how a bound is lowered.

- **A member reached through a bound had no type.** `t.age()` on a
  `<T extends Named & Aged>` was typed `Unknown`, so `t.ident() + t.age() +
  t.score()` got no numeric promotion and reached rustc as "cannot add `f64`
  to `isize`". The same for a field read (`t.id + t.score()`), a comparison,
  a ternary, a `double` slot, `+=`, and a class-level bound.
- **A bound on a type nothing else can extend was lowered as a class bound.**
  `V extends Vec<Pair<K, T>>` became `V: VecKind`, a trait that does not exist;
  a record, an enum, a primitive or `String` failed the same way. Extending a
  Rust type (`class MyVec extends Vec<int>`) was E0429 listing every method of
  `Vec` as unimplemented.
- **An intersection naming two classes was accepted**, and a type written with
  the wrong number of type arguments below the top level
  (`Vec<Vec<Pair<int>>>`, a field, a parameter, a bound) reached rustc.
- **§T.4.6 rule 4 held only for members.** `<R extends K>` is "every `R` is
  usable wherever a `K` is expected", but storing an `R` in a `Vec<K>` or
  returning it as a `K` reached rustc ("expected `K`, found `R`"), in a
  method, a free function, a class (`class Chain<K, V extends K>`) and an
  interface's default method. `<V extends K>` on a free function or an
  interface method emitted `V: K`.
- **A method's `<T>` did not shadow the class's `T`.** `<T> T echo(T t)` in
  `class Shelf<T>` was checked as the class's `T` (`echo("s")` on a
  `Shelf<int>` was E0410) and emitted as a second `T` (rustc E0403).
- **Generic supertypes.** A constructor `super(...); ts.push(t); this.u = u;`
  left `U u` to `U::default()`; an inherited method whose body names its class's
  parameter (`new Box<T>(nv)` copied into `IntBox extends Box<int>`) kept the
  `T`; `Squad<Person>.add(new Student(..))`, with `add(T)` inherited from
  `Roster<T>`, did not convert its argument to `Person`'s form.
- **Smaller ones on the way.** A generic static (`Pair.of("a", 1)`) returned
  `Pair<X, Y>` unsubstituted, so `var p = Pair.of(..)` had no field types; an
  interface default body's bare call to its own method (`items().push(k)`) was
  untyped; a program's own `interface Ord<T>` hid Rust's `Ord` from the code
  an `operator<=>` lowers to; E0438 missed a generic method on a GENERIC
  extended class and a package-private one, and advised `final` and `sealed`,
  neither of which helps.

**Resolution.**

1. **A bound's members are typed by the bound** (`infer.rs`,
   `param_bound_tys`). A method call or a field read on a receiver of type
   parameter `T` is answered by `T`'s bounds in the order written, a bound
   that names another parameter (`<R extends K>`) standing for that
   parameter's bounds. The intersection's bounds agree where they overlap
   (they are checked where declared), so the first answer is the answer, and
   numeric promotion (§S.2.6) follows from the type as everywhere else.
2. **A bound admits more than one type or it is refused** (§T.4.6, new
   `E0459`). A bound naming a Rust type, a record, an enum, a primitive,
   `String`, an array, a function or a nullable type admits exactly that type,
   and none of them has a marker trait to be lowered to; the diagnostic says
   so and says what to write instead (the type itself where the parameter was
   used, or an interface bound). A `final` Jux class stays a legal bound: it
   has a marker trait carrying its members, and Java accepts it. A Jux class
   cannot extend a Rust type: `E0420`, which already meant "final".
3. **An intersection holds at most one class** (new `E0419`), in any position
   (rule 1: order is not significant, so `T extends Named & Base` is legal and
   works). When one class extends the other the message names the one to keep.
4. **The wrong number of type arguments is `E0443` at any depth**, in every
   place a type is written: field, parameter, return, local, `new`, bound.
   A Rust type is held to nothing (§T.4.7.1: it may leave defaulted parameters
   unwritten), a `type` alias is checked where it is expanded, as before.
5. **`<R extends K>` converts.** A value of `R` flowing into a slot of type
   `K` is `(r).into()`, and the declaration acquires `R: Into<K>`: on a
   method's or free function's own generic list, written in only once the body
   is known to convert (an unconditional bound would reject a caller whose
   `R` is a class and `K` an interface, which has no conversion); on a method
   that converts a CLASS parameter, as that method's `where` clause, so the
   struct is not constrained; on an interface method, unconditionally, since
   every implementing class forwards to the trait's signature. A number
   literal passed as such an `R` takes the type `K` is bound to at the
   receiver (`putSub(9)` on a `Shelf<int>`), as Java's inference would.
6. **Shadowing.** A method type parameter named like a class parameter
   shadows it: the checker drops the class's binding of that name at the call,
   and the emitter renames the method's (`T__m`) through its signature and
   body.
7. **Generic supertypes.** The constructor-prefix lift steps over a leading
   `super(...)` and over statements that cannot observe the object (no `this`
   or `super` as a value, no call of a class method, no lambda), lifting only
   a literal or an unreassigned constructor parameter into a slot none of them
   names. An inherited method's body is emitted with the ancestor-to-child
   parameter map active, and a call's argument is converted through the
   parameter map composed along the `extends` chain to the declaring class.
8. **The rest.** A generic static is read through the types its call binds;
   an interface default body's bare call is `this.m()`; the code
   `operator<=>` lowers to names `std::cmp::Ord` by path; E0438 covers
   generic and package-private cases and advises what works (non-generic,
   `private`, or `static` taking the object).

**Still a limitation, now a diagnostic.** Resolved by E136: ~~A
generic method that can be overridden, on a class that is extended (the
visitor pattern's `<R> R accept(Visitor<R> v)`), is E0438 as before: its
dispatch goes through a trait object, which a method with its own type
parameters cannot be part of.~~

**Tests.** `examples/intersection_bounds.jux`,
`examples/generic_supertypes.jux`, `examples/dependent_bounds.jux` and
`examples/generics_deep_nesting.jux`, pinned by hand-written expectations;
`tests/ui/bound_on_foreign_type`, `bound_on_final_value_types`,
`bound_names_two_classes`, `wrong_type_argument_count_nested`,
`extends_foreign_type`, `generic_method_on_generic_base` and
`intersection_bound_one_missing`.

**Known boundary.** Resolved by E136: ~~A written `Task<T>` type is the `Worker` task of
`jux.std.concurrent`, not the task `spawn` returns, so `Task<int> t =
spawn(f())` does not build; `var t = spawn(f())` does. That is the async
surface's naming, not generics', and is left open. `<R extends K>` at the
level of a class (`class Chain<K, V extends K>`) converts inside a method
that is not reached through a trait object; in a method of an extended class
it would also need the `where` clause on the class's dispatch trait.~~

**Spec status:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.4.6 gains the rules for
`E0459`, `E0419` and shadowing, and §T.4.8 the `Into` lowering of rule 4;
`JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 lists `E0419` and `E0459`, and `E0443`
says "at any depth". GAPS.md gap 39 is closed.

## E136. The three things E135 left open: a written `Task<T>`, a generic method reached through a supertype, and `V extends K` on an extended class

**Conflict.** E135 closed gap 39 with three items left open, and the rule is
that none may stay open:

- **A written `Task<T>`** named the `jux.std.concurrent` worker task, a
  different Rust type from the one `spawn` returns (§18.1.4 gives both the
  same name), so `Task<int> t = spawn(f())` and `Vec<Task<int>>` were E0900.
  A task could not be a field, a parameter, a return or an element either,
  since the handle was neither copyable nor printable.
- **A generic method reached through a supertype** (the visitor pattern's
  `<R> R accept(Visitor<R> v)` on an interface or an extended class) was
  refused: E0435 for an interface used as a value, E0438 for a class. A
  supertype value is a Rust trait object, and a trait object cannot carry a
  method with type parameters.
- **`class C<K, V extends K>` extended and used through its base** reached
  rustc as "`K: From<V>` is not satisfied": E135's `V: Into<K>` sat on the
  class's own method and not on its dispatch trait.

**Resolution.**

1. **One task type.** A written `Task<T>` is the handle `spawn` returns
   (unless the program declares a type named `Task`), and `Worker.spawn`
   returns the same handle: its work runs on the worker thread, outside any
   event loop, and its outcome is delivered through the handle, so `await`,
   `blockingGet()` and `join()` behave as for any task, and a failure on the
   worker is rethrown where the task is awaited (it used to end the program as
   "worker task aborted"). A task is a handle (§18.1.4, "a refcounted handle"):
   copies name the same task, and whichever is awaited first takes the result.
   `join()` stays, as `blockingGet()`'s other name.
2. **Dispatch on the object's own type.** A method with type parameters of its
   own, declared on an interface or on a class that is extended, is declared
   on the trait `where Self: Sized`, which keeps it off the vtable, so the
   trait stays usable as a value type and every concrete type still implements
   it. The trait gains `JuxDynAny` (which the object answers through its
   vtable), and the handle's own impl (`impl Trait for Rc<H>`) answers the
   method by asking the object what it is and calling that type's method:
   one branch per concrete class or record that is the supertype. The program
   is compiled whole, so that set is closed; the type ARGUMENTS need not be,
   because each branch is the Rust compiler's monomorphization of that type's
   method for whatever the call's arguments are, a type parameter of the
   caller included (`<R> Vec<R> visitAll(Vec<Expr> es, Visitor<R> v)`). A
   subtype that fixes its supertype's arguments (`class Ints implements
   Bag<int>` behind a `Bag<T>`) is reached with its arguments and result seen
   through `__jux_seen_as`, the identity at run time. A default method is
   dispatched only to the types that write their own and otherwise runs its
   body on the handle, through the `__jux_default_<m>` twin; runtime types
   (a generator behind `Iterable<T>`) therefore keep working. An argument
   that is a class converting to the parameter's interface is cast to the
   instantiation the class IS of it (`Mirror<String>` to `TreeVisitor<String,
   Tree<String>>`), and inference reads a subtype argument as that
   instantiation, so `R` is bound (§T.4.7).
3. **E0438 says why, and only when it must.** Two shapes cannot be found by
   the object's type, and each is refused with its reason: a subtype with a
   type parameter the supertype does not fix (`class Weird<T, U> extends
   Tree<T>`: a `Tree<int>` that is a `Weird` does not say what `U` is), and an
   anonymous class (it has no name to be found by). E0435's generic-method
   case is gone.
4. **`V: Into<K>` on the dispatch trait.** A member of a class declared
   `<K, V extends K>` that takes a `V` states `V: Into<K>` on the trait, every
   impl and the handle's forwarding, in each impl's vocabulary; an ancestor's
   body copied into a subclass that fixes both (`PetChain extends Chain<Pet,
   Cat>`) converts between the fixed types.
5. **On the way.** An override whose parameter nests the ancestor's type
   parameter (`accept(TreeVisitor<int, R>)` over `accept(TreeVisitor<T, R>)`)
   is recognised as the override, not an overload (it was emitted as
   `accept__ov1` and the dispatch recursed); a bare call in an interface
   default body (`get(s)`) copies an argument read again, as `this.get(s)`
   did; a class receiver's argument to an inherited interface default is
   converted to the parameter's type; a program's own `Ord` interface was
   already fine (E135).

**Tests.** `examples/generic_virtual_methods.jux` (a visitor with `int`,
`String` and tree results over an interface and over a generic abstract base,
three and four subclasses, a subtype fixing the argument, a default one
subclass overrides, a type argument that is the caller's type parameter, a
generic method inherited and never overridden), `examples/task_type_written.jux`
and `examples/bounded_params_in_hierarchy.jux`, pinned by hand-written
expectations; `tests/ui/generic_method_dispatch_limits` for the two E0438
shapes (it replaces `tests/ui/generic_method_on_generic_base`, whose program
now builds).

**Known boundary.** Resolved by E137: ~~None of the three items remains open. The two E0438
shapes above are refusals with their reason, not gaps: a value that does not
carry its own type arguments cannot be asked for them. Found on the way and
not closed here: a class that fixes an interface's parameter to a class and
overrides a method bounded by it (`<V extends K> int addAll(Vec<V> vs)` in
`interface Store<K>`, overridden as `<V extends Pet>` in `class PetStore
implements Store<Pet>`) is E0900: the interface's `V: Into<K>` and the
class's `V: PetKind` are different Rust bounds, and an impl may not ask more
than its trait. It predates this entry (E135's lowering of rule 4).~~

**Spec status:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.4.6 gains rule 10 (generic
methods through a supertype) and §T.4.8 its lowering; `JUX-ASYNC-ADDENDUM-v2.md`
§18.1.4 and §18.2 say one `Task<T>` type; `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4's
E0435 and E0438 rows say what they now mean. E135's "Still a limitation" and
"Known boundary" paragraphs are resolved here. GAPS.md gap 39's open items are
closed.

## E137. What E136 still refused: an override bounded by a fixed type, an anonymous subtype, a subtype with a parameter its supertype does not fix

**Conflict.** E136 left three shapes refused or broken, and the rule is that
none may stay that way:

- **`<V extends K>` overridden as `<V extends Pet>`** (`interface Store<K>`,
  `class PetStore implements Store<Pet>`; the same with `abstract class
  Shelf<K>`) was E0900. The supertype's trait said `V: Into<K>` and the
  override's own method `V: PetKind`, and an impl may not ask more than its
  trait. A class `Kind` trait did not say `Into<K>` at all.
- **An anonymous class** could not be a subtype in generic dispatch (E0438),
  and an anonymous subclass of an extended class did not build at all (it
  implemented none of the class's dispatch trait); an anonymous visitor that
  passed `this` on as the visitor did not build either.
- **A subtype with a type parameter its supertype does not fix** (`class
  Weird<T, U> extends Tree<T>`) was E0438.

**Resolution.**

1. **The override lowers its bound as the trait does.** A method type
   parameter at a position that some supertype's declaration of the method
   bounds by the supertype's own parameter (`<V extends K>`) has its bound
   lowered to `Into<bound>` (`V: Into<Pet>`), which is the trait's `Into<K>`
   at `K = Pet`, on the method, on the class `Kind` trait (which now states
   `V: Into<K>` like an interface), on every impl and on the handle's
   forwarding. In the body, a `V` is converted to its bound where a member of
   it is used (`v.name()`, `v.legs`) and where it is stored as one. The
   dispatch from a supertype that fixes `K` (where `V: Into<Pet>` cannot be
   shown, only `V: Into<K>`) calls a `__jux_via_` twin of the method,
   generic over the supertype's `K`, which converts a `V` by way of it. A
   non-generic interface gets `impl From<C> for Rc<dyn I>` for each of its
   non-generic implementers, the conversion `V: Into<I>` asks for.
2. **Every anonymous class of a Jux type is lifted to a named class.** The
   checker records, for each one, the enclosing type parameters it uses, the
   enclosing locals it captures (with their types), and the types of its
   superclass constructor's arguments; the driver declares
   `__JuxAnon_<Target>_<source>_<n>` in the same source (fields for the
   captures, a constructor that passes the superclass its arguments), moves
   the anonymous body's methods and initializer blocks into it unchanged
   (spans included, so a failure names its line), replaces the `new T(..) {
   .. }` with a construction of it, and checks the program again. It is then
   a class like any other: dispatched by name, implementing its superclass's
   dispatch trait, handing `this` out. It prints as `Target$anon@..`, as the
   anonymous object did.
3. **A subtype's unfixed parameter is closed over the program.** The checker
   records every `new C<..>` and every call of a generic function or method
   with the type arguments it binds, each with its context (the class and the
   function or method whose body it is in). The set of instantiations of each
   class is the fixpoint of substituting each context's own instantiations
   into the facts written in it; a method's context includes calls through a
   supertype that declares it. The dispatch chain then has one branch per
   instantiation (`Weird<T, String>`, `Weird<T, bool>`, `Weird<T, Vec<X>>` at
   each `X` built), the fixed positions spelled by the trait's own parameters.
   An untyped number literal passed where a callee's own type parameter is
   the parameter's type is given that type (`wrap("x", 7)` builds a `Weird<T,
   int>`, not an `i32` one).
4. **E0438 keeps one case, and it is truly impossible:** polymorphic
   recursion that builds a subtype at ever-larger arguments (`<A, B> Tree<A>
   grow(B b, int n)` calling `grow<A, Vec<B>>`). The set of `U` has no last
   element, so there is no finite chain to write, and a whole-program compiler
   cannot close it; the message names the method that keeps growing it. (Such
   a program would not build in the language Jux compiles to either: it
   monomorphizes `grow` without end.)

**Tests.** `examples/bounded_method_overrides.jux`,
`examples/anonymous_classes_dispatch.jux` and
`examples/unfixed_subtype_params.jux`, pinned by hand-written expectations;
`tests/ui/generic_method_dispatch_limits` now holds the polymorphic-recursion
refusal only (the two shapes it held before build).

**Known boundary.** Resolved by E138: ~~A lifted anonymous class captures enclosing locals and
parameters, as before; the enclosing object's own fields read bare inside an
anonymous class were not captured before and are not now.~~

**Spec status:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.4.6 rule 10 and §T.4.8 are
amended; `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4's E0438 row names the one case
left. E136's "Known boundary" is resolved here. GAPS.md gap 39c is closed.

## E138. An anonymous class reaches the object it was built in

**Conflict.** `JUX-LANG-V1.md` §7 said an anonymous instance "has no
implicit reference to the enclosing class's `this`", and E137 left the same
as its known boundary: inside `new Listener() { .. }` written in a method of
`Counter`, a bare `count++`, `total += x` or `refresh()` meaning the
enclosing object's member did not build (the lifted class has no such
member). Java reads each of them through the enclosing instance, and it is
the ordinary way to write a listener or a visitor that keeps score.

**Resolution.**

1. **The checker finds the enclosing object's members an anonymous class
   uses bare.** When it plans the lift (E137) of an anonymous class written
   in an instance context, a single-name path in one of its bodies names the
   enclosing object's member when it is a non-static field, property or
   method of the enclosing class, and is not: `this`; a parameter or local of
   that body (its own declarations win, one body at a time); one of the
   anonymous class's own methods; a local of the enclosing method it
   captures; or a member of the type it implements or extends (that type's
   own member wins, as in Java). `this` in the body is still the anonymous
   object, so `this.total` names the anonymous class's member, never the
   enclosing one's.
2. **The lifted class holds the enclosing object.** It gets a field
   `__jux_outer` of the enclosing class's type (at its own type parameters,
   which the lifted class then takes too) and a constructor parameter for
   it; the construction passes `this`. Each such bare name in its bodies is
   rewritten to `__jux_outer.<name>`, so reads, writes (`=`, `+=`, `++`) and
   calls go through the handle and land on the enclosing object itself: a
   write is seen there afterwards. An anonymous class inside another reaches
   the same object: the inner one is passed the outer one's `__jux_outer`,
   and the outer one holds it whenever an inner one needs it. Inside a
   lambda the enclosing `this` is the method's, as for any other use of it.
3. **The handle is borrowed like any field.** A call through it drops every
   guard on the anonymous object before it runs (`__jux_outer` is a field,
   not one of the backend's temporaries), so the enclosing object can be
   busy calling into the anonymous class (iterating its own listeners,
   walking its own tree with the visitor) while the anonymous class writes
   its fields. A call on an element of a collection field (`items[0]
   .accept(v)`) releases the owner's guard the same way; it held it for the
   whole call before, and a visitor writing the owner stopped the program.
4. **Where the handle is built.** In a constructor, a body that hands `this`
   out as a value (to an anonymous class, or `r.add(this)`) runs against the
   finished object, as a body calling a method on `this` already did; a
   field of an interface type assigned bare there converts the value it is
   given. In a method of a class with subclasses, `this` handed out as that
   class is wrapped as its dispatch value. Such an enclosing object is held
   as the base's dispatch value, where a private member has no slot:
   reaching one from an anonymous class is `E0437` at the anonymous class,
   with the fix (make it protected or package-private). A private member of
   any other enclosing class is reached as Java reaches it: the body was
   checked where it was written, so the lifted copy is not checked for
   access again. A package-private field of a class with subclasses is now
   written through its setter from outside the class, as it was already
   read through its getter.

`Outer.this.x` is not part of the language (no section of the spec defines
it), so it is not added here.

**Tests.** `examples/anonymous_outer_access.jux`, pinned by a hand-written
expectation: bare writes and a private method call seen on the object
afterwards, a listener run while its owner iterates its own field, a
parameter and a captured local shadowing fields, nesting, a lambda, a
constructor, a generic class, an abstract base's method run on a subclass,
the superclass's own field winning. `tests/ui/anonymous_outer_private_poly`
holds the `E0437` refusal.

**Known boundary.** Resolved by E139, which also makes the private
members above reachable (item 4's `E0437` is retired): ~~A field initializer that uses the object being built
(an anonymous class reaching its members there, or `int y = twice(3);`
calling an instance method) is E0900 whether or not an anonymous class is
involved: field initializers run before the object exists as a handle.
That predates this entry.~~

**Spec status:** `JUX-LANG-V1.md` §7's anonymous-class paragraph is
amended. E137's "Known boundary" is resolved here. GAPS.md gap 39d is
closed.

## E139. Field initializers that use the object; private members through a dispatch value

**Conflict.** Two things Java programs do every day were left open by E138:

- **A field initializer that uses the object being built** (`int y =
  twice(3);`, `int b = a + 1;`, `Worker w = new Worker(this);`, `Listener l =
  new Listener() { .. n++ .. };`, a lambda over the object's members) was
  E0900 or, for a lambda, `E0981`: the initializers were evaluated into the
  struct the object is built from, before the object existed. `this` in an
  initializer was an unresolved name.
- **A private member of a class with subclasses**, reached through a value
  of the class, has no slot on the class's dispatch trait. From an
  anonymous class written in the class (through the enclosing object it
  holds, E138) it was `E0437`; from the class's own body (`other.hits`) a
  field was `E0437` and a method E0900.

**Resolution.**

1. **An instance field initializer may use the object.** `this` is in scope
   in it (a static field's initializer still has no object). An initializer
   that uses the object (`this`, `super`, a bare instance field, property or
   method name) is run against the finished object instead of being
   evaluated into the struct: the object is built with the field at a
   stand-in value, and the initializer then assigns it. The order does not
   change (JUX-LANG-V1 §7.3.1, E21): every initializer from the first such
   one on is run this way, so they still run in the order written; when an
   ancestor's are, all of a class's are; and the hierarchy's all run, root
   class first, before any `init` block or constructor body, once however
   the constructors delegate with `this(...)`. It holds for a class with
   no constructor of its own, several constructors, a generic class and a
   subclass. A class that runs an initializer this way has the
   interior-mutable representation (E121); the selector reads the same rule.
   A constructor whose statements are then run after the initializers keeps
   no leading store in the struct for a field that has a value to hold
   meanwhile, so an initializer reads the field's default, as in Java.
2. **What a field holds before its initializer runs.** A number, a string,
   an optional, an array, a collection and a one-method interface have a
   value to hold (their default, or a stand-in that throws when called). A
   class, an interface with several methods and a type parameter have none;
   such a field is given a nullable slot while the program is compiled, and
   each read of it asserts it is set, which it always is once construction
   is over (a method run during construction that reads it first stops with
   the exception an unset `!!` raises). The same holds for a field of such
   a type that only a constructor or an `init` block assigns, from a value
   that uses the object (`this.w = new Worker(this);`), which was E0900.
3. **`E0499`: an initializer reading a field that holds no value yet.** A
   field declared later, or one only a constructor assigns (constructors run
   after every initializer), read directly by an initializer (bare or
   `this.x`), whose type has no default value (§6.5's list: a class, an
   interface, a function type, a type parameter). A type with a default is
   read at it (`int early = this.late + 1;` reads `0`), as Java reads it.
   What a lambda or an anonymous class in the initializer reads is read when
   it is called.
4. **`E0981` is left for observers.** An observer's lambda is attached while
   the object is built, so it still cannot use the object; any other
   function-typed field's lambda initializer can (1).
5. **Private members through a dispatch value.** Where a private member is
   visible (the class's own body, an anonymous class written in it), a read,
   write or call of it through a value typed as a class with subclasses is
   renamed to a hidden stand-in the compiler declares on the declaring class:
   a property `__jux_priv_<Class>_<field>` over the field (no setter for a
   `final` one) and a method `__jux_priv_<Class>_<method>` that calls the
   method. Being non-private, each is on the dispatch trait, implemented by
   the class and reached from every subclass through what it inherits. The
   stand-ins exist only while the program is compiled: `check` and the
   editor never see them. Anywhere the member is not visible it is the
   ordinary private-access refusal, `E0414`. `E0437` is retired; its number
   is not reused. ~~An overloaded private method is not given a stand-in.~~
   Each overload is given its own since E140.

The order in item 1 ("before any `init` block or constructor body") is
superseded by E140: each class's initializers now run after its
`super(..)`, in Java's order.

**Tests.** `examples/field_initializers_use_object.jux` and
`examples/anonymous_private_members.jux`, pinned by hand-written
expectations; `tests/ui/field_read_before_initialized` holds `E0499`.
`tests/ui/anonymous_outer_private_poly` (the `E0437` refusal) is removed:
the shape it held builds.

**Spec status:** `JUX-LANG-V1.md` §7.3.1 and `JUX-SEMANTICS-ADDENDUM.md`
§S.4.4 step 3 are amended; `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 has `E0499`,
retires `E0437` and narrows `E0981`. E138's "Known boundary" is resolved
here, and E43 is resolved except for observers. GAPS.md gap 39e is closed.

## E140. Construction in Java's order; overloaded private methods through a dispatch value

**Conflict.** E21 ran every field initializer of a hierarchy before any
constructor body, a deliberate divergence from Java: a base constructor
that called an override saw the subclass's fields already initialized. The
user chose Java parity instead: a Java program's construction must print
what Java prints. Separately, E139 gave an overloaded private method no
stand-in, so reaching one through a dispatch value was still E0900.

**Resolution.**

1. **Java's construction order** supersedes E21. For `new C(..)`, each class
   of the hierarchy, root first, runs its `super(..)` (explicit, or the
   implicit no-argument one), then its own instance field initializers and
   `init` blocks, in the order they are written in the class, then the rest
   of its constructor body. A `this(..)` chain runs the initializers once, in
   the constructor that calls `super`. A class with no constructor of its own
   runs the same steps through the implicit one, also as an ancestor.
2. **What a base constructor sees.** A method it calls that reads a
   subclass's field reads the field's default value (`0`, `false`, `""`,
   `null` for an optional), as Java does (Java's `null` for a `String` is
   `""`, the String's default value, JUX-LANG-V1 §6.5). A field whose type
   has no default value (a class, an interface with several methods, a type
   parameter) holds none yet: the read throws `IllegalStateException`
   ("field 'tag' of Middle read before it was initialized"), which the
   program can catch, where Java reads `null`. Such a field gets a nullable
   slot while compiling (E139's `late_fields`), now also when only the
   constructor assigns it and an ancestor's construction can reach the
   object. An unset read of any E139 late field throws the same exception
   (it was the `!!` failure).
3. **The lowering.** The object is still built as one struct holding every
   class's fields; an initializer is evaluated into it only where nothing can
   tell (`juxc_tycheck::field_init`): it does not use the object, comes
   before the class's first `init` block, and either has no effect or no
   ancestor's construction has one. Otherwise it runs against the finished
   object at its place in the order, with every initializer after it; and
   all of a class's do when an ancestor's construction reaches the object
   (calls a method on it, hands it out). Each class's deferred initializers
   and `init` blocks run, interleaved in source order, after its ancestors'
   part and before its body (an ancestor's replayed as part of the
   ancestor's construction). A constructor whose ancestors reach the object
   runs against the handle too, and stores nothing early. The
   representation selector (E121) gives every such class its cell.
4. **Overloaded private methods.** Each private overload gets a stand-in of
   its own, all under the one hidden name `__jux_priv_<Class>_<method>`, so
   the stand-ins are an overload group (`__ovK` in the lowering) and a call
   through a dispatch value picks among them by its arguments as it picked
   among the methods. A private overload beside a non-private one of the
   same name is reached the same way; the non-private one is called as it
   is. A call whose receiver is bound to a temporary before its arguments
   run kept the overload pick of the last call in its arguments instead of
   its own (`add(x, name())` called the one-argument `add`); it keeps its
   own now.

**Tests.** `examples/construction_order_java.jux` (a three-level hierarchy:
initializers with effects, `init` blocks, `this(..)`, a base constructor
calling an override that reads a subclass field) is pinned by the output
Java prints, and is `tools/java-differential/cases/83_construction_order_java`;
`44_init_order` no longer declares a divergence and matches Java.
`examples/private_overloads_dispatch.jux` covers the overloads. Four
examples pinned the old order and now print Java's:
`construction_and_identity`, `ctor_calls_method`, `static_collection` and
`field_initializers_use_object`.

**Spec status:** `JUX-LANG-V1.md` §7.3.1, `JUX-SEMANTICS-ADDENDUM.md` §S.1.5
and §S.4.4 and `JUX-MISSING-DEFS-ADDENDUM.md` §M.1.2 and §M.1.4 are amended
to Java's order; E21 is superseded, and E139's order and its overloaded
private method boundary with it. GAPS.md gap 39f is closed.

---
When you edit any addendum that touches one of the items above,
either:

1. Drop the corresponding ERRATA entry (the addendum now says
   the right thing on its own), or
2. Update the ERRATA entry with the new conflict, if the
   addendum edit creates a new tension.

The compiler implementation follows the **Resolution** line of
each entry. Any divergence is a bug in either the compiler or
this file; cross-check before assuming the other source is wrong.
