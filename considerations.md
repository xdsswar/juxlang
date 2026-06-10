# Jux Project — Deep Audit & Suggestions

> **Status (2026-06-10): historical snapshot.** This audit describes the repo
> as it stood when it was written; the "headline state" below is out of date
> (the real type-checker, borrow-stress hardening, generics, and polymorphism
> have since landed). Items completed since the audit are marked **✅ DONE**
> inline. Unmarked items remain open recommendations, not commitments.

*Prepared from a full read of `Architecture/*.md`, the compiler crates under `crates/`, and the `.jux` examples. Items are roughly ordered by leverage — top entries are decisions that get harder the longer you wait.*

---

## Headline state

The spec is **far ahead** of the implementation, and on purpose — but the gap has reached the point where decisions made in the next ~3 months of coding will be hard to retrofit. The compiler is **~40% of Phase 1**: lex/parse/AST/resolve are solid, the type-checker is a stub (188 lines vs. the ~1000 the spec implies), MIR doesn't exist, the borrow checker doesn't exist, and the backend is a 4.7k-line single file that hardcodes representation assumptions which would need to change for half the spec's promises.

This is fine. It just means the next moves have to be careful.

---

## Tier 1 — Decisions to make BEFORE more code lands

These are the items where shipping more code first makes them harder to do later.

### 1.1 Decide on class representation NOW, then formalize — ✅ DONE
*(`JUX-CLASS-REPRESENTATION-ADDENDUM.md` exists; classes lower as shared
mutable references — `Rc<RefCell<…>>` — and the backend redesign landed.)*

The backend currently assumes every class is `Arc<C_Inner>`. The right move is a **compiler-chosen** representation (inline | `Box` | `Rc` | `Arc`) with no user-visible annotations. **This decision should be written into `JUX-COMPILER-PIPELINE-ADDENDUM.md` as §C.9.3.1 before more backend code calcifies the Arc assumption.**

Evidence the concern is real: `juxc-backend-rust/lib.rs` lines 99–113 have defensive `.clone()` everywhere on String fields specifically *because* the type table doesn't exist. The same pattern is starting to grow around generics. Each new backend feature added before the representation pass exists is one more place to clean up later.

**Action:** draft `JUX-CLASS-REPRESENTATION-ADDENDUM.md` now, even before the analysis is implemented. Lock the design.

Proposed selector logic (already feasible from existing analyses):

| Representation | Rust lowering | Cost | Conditions |
|---|---|---|---|
| Inline | plain `struct C` | Zero — C-tier | No escape; no `dyn` needed; no weak refs |
| Owned heap | `Box<C>` | One alloc, no atomics | Escapes but never aliased |
| Local refcount | `Rc<C>` | Alloc + non-atomic refcount | Aliased but never crosses thread boundary |
| Shared refcount | `Arc<C>` | Current behavior | Crosses threads or analysis is conservative |

### 1.2 Split the backend before it grows past 6k lines

`juxc-backend-rust/lib.rs` at 4748 lines is already painful. The audit counted 50+ `emit_*` methods, five fragile pre-pass tables (`string_field_names`, `enum_string_slots`, `generic_field_names`, `user_mut_methods`, `interface_methods`), and explicit `\n    ` indentation pushes. Adding any new feature touches multiple emit paths.

**Action:** split into modules along the same axes the spec already uses — `types.rs`, `exprs.rs`, `stmts.rs`, `decls.rs`, plus a `rust_writer.rs` that handles indentation/formatting through a tiny IR (even just `Doc::Indent / Group / Break` pretty-printer). This is a 1–2 day refactor today, 1–2 weeks at 8k lines.

### 1.3 Plan for MIR, even if you don't build it yet

The spec's headline promises (borrow checker, async lowering, drop insertion, refcount elision) all require MIR. The current backend goes AST → Rust directly. When MIR lands, the entire backend has to be rewritten to emit from MIR instead.

You don't have to *build* MIR now. But you should **stop adding features that pretend AST is the final IR**. Concretely: don't add async/await codegen, drop blocks, or anything from spec phases 11–15 to the AST→Rust path. Those should wait for MIR.

### 1.4 Resolve the four spec contradictions before they spread — ✅ DONE
*(`Architecture/ERRATA.md` exists, resolves all four — E1–E4 — plus more,
and as of 2026-06-10 every resolution is applied back into the addenda.)*

The spec audit found four real ones:

1. **Panic vs Exception.** `JUX-EXCEPTIONS` says `throws ↔ Result`; `JUX-SEMANTICS` talks about per-profile panic behavior. Which is canonical? Are they orthogonal layers (exceptions are user-level, panics are runtime aborts) or one thing? Pick one and edit the other.
2. **Init block ordering** relative to `super()`: undefined.
3. **Async borrow rule enforcement.** Phase 11 (per pipeline) vs. inside async lowering — pick one.
4. **Cross-module `protected` access.** `JUX-INHERITANCE-BORROW` and `JUX-TYPE-SYSTEM` disagree implicitly.

**Action:** an `ERRATA.md` next to the addenda, listing reconciliations. Don't let these sit — every new addendum that touches these areas drifts further.

---

## Tier 2 — The biggest single-leverage implementation work

### 2.1 Build the real type-checker (188 → ~1000 lines) — ✅ DONE
*(`juxc-tycheck` is now a full checker: member/visibility resolution,
inheritance and interface-dispatch rules, smart-casts, exhaustiveness,
generics inference incl. explicit type-args, wildcards, and const generics.)*

This is the highest-leverage thing in the entire codebase. The current type-checker only validates `main()`'s signature. That means **every** type-dependent feature silently accepts wrong code:

- Generics aren't inferred (`identity(42)` doesn't infer `<int>`).
- Overloads aren't resolved.
- Smart-casts (`if (x => Foo f)`) don't narrow.
- Exhaustiveness on `switch` over sealed types isn't checked.
- The backend's heuristic "field named `String` → auto-clone" exists because there's no type table to ask "what's the type of this field?"

Most of the backend's complexity is *compensating for the missing type-checker*. Building it would let you delete ~30% of the backend.

**Action:** before adding any new language features. Order: symbol table → type environment → expression typing → method resolution → generic instantiation → exhaustiveness → smart-cast narrowing.

### 2.2 Source map from emitted Rust back to .jux

Right now, rustc errors land on emitted Rust lines the user never wrote. The moment someone writes a real program, this becomes the user-experience killer. The spec doesn't address this.

**Action:** emit `// JUX:file.jux:line:col` markers on every statement and span block, then post-process rustc's output to remap. Crude but works. Long-term, target rustc's debuginfo facilities or generate source maps for an IDE.

### 2.3 Lock down the standard library shape

`JUX-GAPS-ROADMAP.md` items 1–9 all block Phase 1 shipping. Critically:

- `Array<T>` / `List<T>` / `Map<K,V>` / `Set<T>` — undefined.
- `String` semantics (immutable? owned? SSO?) — half-defined.
- Standard exception roster (`IOException`, `NullPointerException`, etc.) — not enumerated.
- I/O primitives, time, duration — undefined.

**Action:** before writing a single line of stdlib code, write `JUX-CORE-LIB-V1-ADDENDUM.md` that pins down the surface API of `jux.lang.*` and `jux.util.*` in enough detail that the backend can emit calls to it.

---

## Tier 3 — User-facing simplicity (the Java-shape promise)

These are spots where the spec is silently drifting toward Swift/Rust complexity. Each is a small fix now; ignored, they compound.

### 3.1 Reduce profiles from three to two

`jux-full`, `jux-embedded`, `jux-core` is one profile too many. The behavioral matrix (refcount on/off, panic mode, exception lowering) crosses with build profiles (debug/release). Users have to learn both axes.

**Recommendation:** keep `jux-full` (the normal language) and `jux-bare` (no-refcount, no-exception, for embedded/kernel). Fold `jux-core` into `jux-bare`. Two profiles, one knob, easy to teach.

### 3.2 Annotation roster — enumerate, don't sprawl

Nine annotations are already implied (`@Override`, `@Deprecated`, `@layout(c)`, `@align`, `@cfg`, `@export`, `@reflectable`, `@main`, `@async-init`). The annotation system addendum lets users define more for DI/ORM frameworks. Without a canonical list of *standard* annotations, this becomes a mess.

**Action:** a `JUX-STANDARD-ANNOTATIONS.md` page that lists every blessed annotation with semantics. Anything not on the list is a user-defined annotation with no compiler behavior.

### 3.3 Decide: are panics user-visible or not? — ✅ DONE
*(Decided exactly as recommended: panics are aborts, not catchable;
exceptions are values. Normative in `ERRATA.md` E1.)*

The cleanest design (Rust's): panics are aborts, not catchable. Exceptions are values. The user only knows about exceptions.

If you go this way, panics disappear from the user-facing language entirely — they're a runtime mechanism for "this should never happen, abort." That removes a whole class of user-facing complexity.

### 3.4 The grammar has features without examples

The examples audit flagged that **no example exercises**: inheritance, interfaces, try/catch, async/await, sealed hierarchies, bounded generics, wildcards, nullable types, drop blocks, FFI. Every one of these is in the spec. Some are in the parser. **None of them are battle-tested.**

This isn't a small deal: the parser tests cover *grammar acceptance*, not *end-to-end compilation*. Things that parse may not lower. You don't know until you write the example.

**Action:** add 6 example vehicles in this order:

1. `inheritance.jux` — base class, override, virtual dispatch, upcast.
2. `exception_try_catch.jux` — try/catch/finally, `throws`.
3. `nullable_types.jux` — `T?`, `?.`, null narrowing.
4. `interface_impl.jux` — interface decl, multiple `implements`, default methods.
5. `async_await_basic.jux` — async fn, await, Task<T>, spawn.
6. `sealed_hierarchy.jux` — sealed + permits, exhaustiveness.

---

## Tier 4 — Code-quality and testing hygiene

### 4.1 Add integration tests for examples

Right now, examples exist as files but no CI gate confirms they still compile to valid Rust. The parser has 1.5k lines of tests; the backend has 128. Neither verifies the end-to-end pipeline.

**Action:** a test runner that compiles every `.jux` example, runs `cargo build` on the output, and snapshots the emitted Rust. Failures become regressions, not silent surprises.

### 4.2 Thread `Span`s all the way through

AST spans exist but are dropped after parsing. Resolver and type-checker errors can't point at source. Backend has no spans at all. This is the same issue as the source map (2.2) but at the diagnostic layer.

### 4.3 Reconcile `isize` vs `i64`

The backend maps Jux `int` to Rust `isize` in some places, `i64` in others. Spec says `int = i32`, `long = i64`. There's a mismatch. Pick one and grep-fix the rest.

---

## Tier 5 — Strong parts to NOT touch

The audits flagged these as already excellent. Resist the urge to refactor them.

- **Lexer** (`juxc-lex`, 870 LOC + 318 tests) — clean, edge-case correct, no smells.
- **Parser** (`juxc-parse`, 2.5k LOC + 1.5k tests) — hand-written recursive descent with principled error recovery.
- **Grammar addendum** — 20-level operator precedence table, full EBNF. Implementation-ready.
- **Type-system addendum** — wildcard inference, overload resolution, borrow inference algorithms all specced.
- **Compiler-pipeline addendum** — 18-phase architecture is rare for a language to commit to in writing.
- **Operator addendum** — orphan rule is elegant; auto-derivation of `==` / `hash` / `Display` keeps records boilerplate-free.
- **Whole-object borrow rule for classes** — clean simplification over Rust's field-level model.
- **Kotlin-style async** — functions return `T`, `Task<T>` only when spawned. Avoids the Rust `impl Future` viral generics.

---

## Suggested execution order

1. **Week 1.** Write `JUX-CLASS-REPRESENTATION-ADDENDUM.md` (the auto-selection idea). Lock the decision.
2. **Week 1.** Write `ERRATA.md` reconciling the four spec contradictions.
3. **Week 1–2.** Split `juxc-backend-rust/lib.rs` into 5 files. Add a tiny `Doc` pretty-printer.
4. **Week 2.** Add `JUX-STANDARD-ANNOTATIONS.md`. Cut profile count from 3 to 2.
5. **Weeks 3–8.** Build the real type-checker. Don't add features in parallel.
6. **Week 9–10.** Add source-map markers and an integration test runner.
7. **Week 10+.** Add the 6 missing example vehicles. Each one shakes out a subsystem.
8. **After all of that.** Start MIR.

The temptation will be to add features (async! inheritance! interfaces!) because they're spec'd and fun. Resist. The type-checker is the unsexy work that unlocks everything else.

---

## One philosophical note

Jux is Java-shaped but not Java. That's the right north star. The risk across the audit is that Jux is *also* drifting toward "Swift-shaped but not Swift" — profiles, annotations, multiple class behaviors. The recommendation is to **stay opinionated about one `class` keyword that does the right thing, one compilation profile most users will use, and a handful of blessed annotations**. The compiler does the work; the user writes Java.

Every time the spec adds a knob, ask: *"would a Java programmer learn this in their first week?"* If no, push the complexity into the compiler instead.
