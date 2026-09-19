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
behaviour and stands.

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
