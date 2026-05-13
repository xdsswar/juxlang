# ERRATA.md — Spec reconciliations

**Status:** Active. Listed contradictions are resolved here; the
canonical addenda will be edited in their own next pass to match.

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

The catalogue of conditions that panic vs. throw:

| Condition                              | Mechanism  | Catchable? |
|----------------------------------------|------------|------------|
| Arithmetic overflow (`jux-full` debug) | Panic      | No         |
| Arithmetic overflow (`jux-full` release) | Wrap     | N/A        |
| Array bounds violation                 | Panic      | No         |
| Division by zero (integer)             | Panic      | No         |
| Null deref via `!!` (force-unwrap)     | Panic      | No         |
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
- `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.11 should note that
  Phase 11's borrow checker handles both sync and async bodies.
- `JUX-ASYNC-ADDENDUM-v2.md` should cross-reference §C.11 for
  the where, and define the rule's content here too.

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

---

## E5 — Nullable primitive types

**Conflict.** `JUX-LANG-V1.md` §7.10 example shows
`var length = findName(42)?.length();    // returns int?`,
implying `int?` is a valid type. The general spec design treats
primitives (`int`, `bool`, `float`, etc.) as **value types**
that never hold null.

**Resolution.** **Primitives cannot be marked nullable directly
in source.** `int?`, `bool?`, `char?`, `float?`, and the
unsigned / width-explicit numerics are rejected at type-check
time (`E0410_TypeMismatch` with a "primitive type X cannot be
nullable" message). Reference types — `String`, user classes,
records, enums, arrays of references — are the well-formed
inner shapes for `T?`.

For the `?.length()` example: the safe-call propagates `None`
through the chain. To keep the spec consistent with the
"no nullable primitives" rule, the result of
`findName(42)?.length()` should be lifted into a reference
shape (e.g. an explicit `int?` typedef bridged to `Optional<int>`
or a boxed `Integer?`) once we have boxed primitive types.
Until then the example in §7.10 is **non-normative** — it's a
sketch of the operator's shape, not a sanctioned program.

**Spec edits this implies:**
- `JUX-LANG-V1.md` §7.10's `?.length()` example should be
  flagged as non-normative or rewritten using a reference-typed
  field.
- A new entry in `JUX-LANG-V1.md` §5 / §7.10 should explicitly
  state the "primitives can't be `T?`" rule.

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
