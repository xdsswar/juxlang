# Jux — Gap Analysis (implementation vs. spec)

**Author:** Claude (Opus 4.8) — grounded review, not the historical `considerations.md` snapshot.
**Date:** 2026-06-11
**Branch reviewed:** `polymorphism`

## How this was produced

Every "Verified" claim below was checked against source this session, not inferred
from docs. File:line anchors are given so each can be re-confirmed or disputed.
Items I could *not* verify from code are marked **(unverified)** and should be
treated as questions, not findings.

Three honest framing notes up front:

1. **Most of these are already written down somewhere** — `JUX-CLASS-REPRESENTATION-ADDENDUM.md`,
   `JUX-GAPS-ROADMAP.md`, the wave-progress notes. The value of this file is
   collecting them in one place, marking *current code state* (not spec intent),
   and flagging the few that are genuine soundness risks rather than scheduled work.
2. **Phase A is intentionally shipped before Phase B.** The class addendum (§CR.9)
   explicitly chose "correct-but-uniform `Rc<RefCell>` first, fast tier later."
   So the cost gaps below are *known interim*, not oversights — the gap is schedule
   risk, not blindness.
3. **One memory-model claim is load-bearing and unproven** (G3). That's the single
   item I'd treat as higher-than-its-priority-tag.

---

## Tier 1 — Memory model (the core risk surface)

### G1. Escape-analysis selector (Phase B) is not implemented — every class pays full `Rc<RefCell>` cost
**Severity:** High (performance / "Rust speed" promise) · **Status:** Verified

`decls/classes.rs:711` unconditionally emits `struct C(std::rc::Rc<std::cell::RefCell<C_Inner>>)`
for **every** class. There is no `Rep` enum, no `select_rep`, no `Inline`/`Box`
branch anywhere in `classes.rs` (grep for `Inline|selector|Representation` → none).

The four-representation selector designed in `JUX-CLASS-REPRESENTATION-ADDENDUM.md`
§CR.2–§CR.3 (Inline / Box / Rc / Arc, chosen by escape+aliasing analysis) exists
only on paper. **Consequence today:** a purely local `Point` that never escapes
still pays a heap allocation, a non-atomic refcount, and a runtime borrow check on
every field touch. The addendum's headline promise — "a class that never escapes
pays zero heap-allocation cost" (§CR.1) — is currently false.

*Recommendation:* this is the biggest single perf lever. Build the Phase-B selector
as a pass between tycheck and lowering; even shipping just the `Inline` demotion
for non-escaping, non-aliased classes would reclaim most of the cost.

### G2. Classes cannot cross threads at all — `Arc<Mutex>` rep unbuilt; `spawn` capture is a hard error
**Severity:** High (concurrency capability) · **Status:** Verified

Because every class is `Rc<RefCell>` (`!Send`), `E0702_ObjectCapturedBySpawn`
(`tycheck/src/check.rs:3549`, `diagnostics/src/code.rs:417`) **rejects** any
class-typed object captured by a `Worker.spawn` closure. The `Arc<Mutex<C_Inner>>`
cross-thread representation described in addendum §CR.4.1 (table row 4) and §CR.5.4
is not emitted by `classes.rs` (no `Arc<`/`Mutex<` wrapping of class inners there —
the `Mutex` hits are all the unrelated `LazyLock<Mutex>` static-field path).

**Consequence:** sharing a mutable object across threads — a routine Java pattern —
is impossible in Phase 1. You can pass primitives/values into `spawn`, but not
objects. This is stricter than the spec intends and should be documented as a
current limitation, not just an internal interim.

*Recommendation:* the cross-thread upgrade depends on G1's selector
(`cross_thread` property → `Arc<Mutex>`). Until then, E0702's message should say
"not yet supported in Phase 1" rather than implying a permanent rule.

### G3. Statement-scoped borrow soundness is asserted, not proven  ⚠️ load-bearing
**Severity:** High (correctness) · **Status:** Verified mechanism; soundness unproven

The whole shared-mutation model rests on §CR.4.1's claim that statement-scoped
`borrow()`/`borrow_mut()` makes `already borrowed: BorrowMutError` panics
"unreachable in well-formed Jux." The mechanism is real and implemented — `field.rs`
emits per-field-access `.0.borrow()` guards that drop within one statement
(`field.rs:426`, `:456`). But "unreachable" is an assertion backed by the test
suite, not a proof.

The adversarial case the discipline must survive: a `&self` method on object `A`
that, mid-expression, calls into object `B` which calls *back* into `A` and mutates
it — or any iteration over a collection of objects where the loop body re-enters an
element. If a single `borrow()` guard is ever held across such a call, it panics at
runtime *after* the type checker blessed the code. That's the worst failure class:
looks safe, isn't, fails in production not compilation.

*Recommendation:* write a dedicated re-entrancy stress example (callback that
mutates `this` through an alias; visitor pattern; observer firing during mutation)
and put it in CI. Until a fuzz/stress pass targets this specifically, treat
"unreachable" as "untested in the worst case."

### G4. Un-annotated reference cycles leak memory
**Severity:** Medium (correctness, but documented & bounded) · **Status:** Verified + design-acknowledged

`Rc` does not collect cycles. `weak` fields are implemented
(`classes.rs:677` → `std::rc::Weak<RefCell<Target_Inner>>`, refcount-neutral) and
*do* break cycles when the user annotates them. But a cyclic object graph with no
`weak` back-edge — parent↔child, doubly-linked list, observer↔subject — leaks,
silently. Java's GC collects these; Jux does not.

This is **explicitly accepted as a Phase-C interim** (addendum §CR.5.5; §CR.9
Phase C: "document that uncollected cycles leak… trial-deletion / arena collector
later"). So it is a known tradeoff, not an oversight. The gap is: (a) there's no
diagnostic warning a user when they build an un-annotated cycle, and (b) the
eventual cycle collector is unscheduled.

*Recommendation:* short term, a lint that flags a class field whose type
transitively re-references the class and suggests `weak`. Long term, the
trial-deletion collector is the only path to true Java fidelity here.

---

## Tier 2 — Pipeline / architecture

### G5. No MIR — borrow checking, drop insertion, and refcount elision all ride directly on the AST
**Severity:** Medium-High (long-term) · **Status:** Verified (no `juxc-mir` crate)

The crate list has no IR crate between `juxc-tycheck` and `juxc-backend-rust`;
lowering is AST → Rust source directly. The spec's heavier promises (a real borrow
checker, async lowering, drop-block insertion, the refcount elision that G1's
selector implies) are the kind of analyses that want a control-flow IR. Each feature
added to the AST→Rust path is one more thing to re-host when/if MIR lands.

*Recommendation:* not urgent to *build*, but decide explicitly whether Phase 1
ships without MIR permanently. If yes, the borrow-checker promise needs re-scoping;
if no, stop growing the AST→Rust path with flow-sensitive features.

### G6. Source maps exist but remapping is "crude" and off by default
**Severity:** Medium (DX) · **Status:** Verified

A source-map mechanism is present — `lib.rs:137` emits `// JUX:file:line:col`
markers, opt-in via a separate lowering entry point, default off (`lib.rs:550`).
The code itself calls it "crude (string-based)." So when a user's program triggers
a `rustc` error on emitted Rust, the path back to their `.jux` line is partial at
best and absent by default. The first real-world program makes this the UX killer
the audit predicted.

*Recommendation:* turn markers on by default in the driver, and verify the rustc
output remapping actually fires end-to-end (I confirmed markers are *emitted*; I did
**not** confirm rustc diagnostics get rewritten back). **(remapping path unverified)**

### G7. `juxc-backend-rust` size / single-responsibility
**Severity:** Low (maintainability) · **Status:** Partially addressed

The audit's "4.7k-line single file" has since been split (`decls/`, `exprs/`,
`stmts.rs`, `literals.rs`, etc. all exist now), so this is largely **done**. Noted
only to close it: the Tier-1.2 concern from `considerations.md` no longer applies in
its original form.

---

## Tier 3 — Type-system / semantics gaps to confirm

### G8. `int` width: `isize` vs `i64` vs spec's `i32`
**Severity:** Medium (semantics) · **Status:** Unverified this session — flagged by prior audit

`considerations.md` §4.3 reported `int` mapping to `isize` in some paths and `i64`
in others, while the spec says `int = i32` / `long = i64`. I did not re-grep the
mapping this session. If still inconsistent, integer overflow/wraparound semantics
and FFI struct layout are both affected. **(verify before trusting)**

### G9. Exceptions vs panics layering — confirm the ERRATA resolution is fully implemented
**Severity:** Medium · **Status:** Spec resolved; implementation completeness unverified

`ERRATA.md` E1 decided panics = uncatchable aborts, exceptions = values
(`Result`-lowered). Memory notes flag "catch-body return/throw vs finally" ordering
as historically deferred, then partly fixed (commit c6c4454). Worth a focused pass
confirming try/catch/finally control flow (return-in-catch vs finally, exception
chaining, multi-catch ordering) matches the exceptions addendum end-to-end.

---

## Tier 4 — Deferred language/library features (known, scheduled)

These are tracked in `JUX-GAPS-ROADMAP.md` and the wave-progress notes; listed for
completeness. None are "holes" in the sense of being unanticipated — they're backlog.

- **G10. FFI (C/C++).** Marked "very important" but deferred (`project_ffi_deferred`).
  Path known (build.rs link + bindgen/autocxx); not built.
- **G11. Escape-selector-dependent stdlib types** — `StackString`, `Volatile`,
  `SharedRef` deferred (wave-5 notes).
- **G12. Nested / inner / anonymous classes** — explicitly *unsupported* in Phase 1
  (E0991–E0993), per `JUX-MISSING-DEFS-ADDENDUM.md` §M.9. Anon-class fixes partially
  landed; full support is post-Phase-1.
- **G13. Multi-file Rust output** — currently one emitted unit; user wants output
  mirroring `.jux` file structure (`project_codegen_quality_requests`). Deferred.
- **G14. Per-instantiation generic representation** — generics pick one rep across
  all instantiations (addendum §CR.5.3); per-instantiation specialization is Phase D.
- **G15. Remaining wave-6 items** — volatile, compile-time const-eval, full
  annotation processing, `module.jux`, `out`/`move` params, definite-assignment
  analysis (`project_wave6_progress`).

---

## Tier 5 — Tooling / DX

- **G16. Example coverage as end-to-end gate.** 144 examples + 88 backend tests exist
  (much better than the audit's snapshot), but confirm CI actually compiles every
  example's *emitted Rust* with `cargo build`, not just that it parses/lowers.
  Grammar acceptance ≠ end-to-end compilation. **(CI completeness unverified)**
- **G17. No cycle/leak lint** (see G4) — user gets no signal before shipping a leak.
- **G18. E0702 message framing** (see G2) — reads as a permanent rule; it's interim.

---

## Priority summary

| ID  | Gap                                             | Severity | Status                    |
|-----|-------------------------------------------------|----------|---------------------------|
| G3  | Statement-scoped borrow soundness unproven      | High ⚠️  | Mechanism verified        |
| G1  | Escape selector unbuilt — all classes Rc<RefCell> | High   | Verified                  |
| G2  | Classes can't cross threads (Arc<Mutex> unbuilt) | High    | Verified                  |
| G4  | Un-annotated cycles leak                        | Medium   | Verified + accepted       |
| G5  | No MIR; flow analyses on AST                     | Med-High | Verified                  |
| G6  | Source-map remapping crude / off by default      | Medium   | Partially verified        |
| G8  | int width isize/i64/i32 inconsistency            | Medium   | Unverified — re-check      |
| G9  | Exception/panic layering completeness            | Medium   | Spec done; impl unverified |
| G10–G15 | Deferred features (FFI, nested classes, …)  | Backlog  | Tracked                   |
| G16–G18 | Tooling/DX (CI gate, leak lint, msg framing)| Low-Med  | Mixed                     |

## The one-line take

The design *foresight* is strong — every Tier-1 item is already written down with a
planned fix, and the mechanisms (weak refs, statement-scoped borrows) have real code.
The actual risk is concentrated in two places: **schedule risk on the Phase-B escape
selector** (G1/G2 — the language is correct-but-slow and single-threaded-for-objects
until it lands) and **one unproven soundness claim** (G3) that the entire shared-
mutation model rests on. Those two are where I'd point effort and adversarial testing.
