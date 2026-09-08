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

**Spec status:** Unresolved. §K.7, §7.10 + E5, §S.3.3 and the gaps roadmap
give four different answers.

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
and want a diagnostic of their own.

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

## How to use this file

When you edit any addendum that touches one of the items above,
either:

1. Drop the corresponding ERRATA entry (the addendum now says
   the right thing on its own), or
2. Update the ERRATA entry with the new conflict, if the
   addendum edit creates a new tension.

The compiler implementation follows the **Resolution** line of
each entry. Any divergence is a bug in either the compiler or
this file; cross-check before assuming the other source is wrong.
