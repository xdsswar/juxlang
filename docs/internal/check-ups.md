# Jux Architecture — Check-Ups

**Purpose:** A structured review of the Jux language specification corpus (29 documents, ~22,500 lines), written so another AI or reviewer can pick up any item, verify it against the spec, and act. Each finding states *what*, *where in the spec*, *why it matters*, and *what to check or do*.

> **Verification pass — 2026-06-12.** Every item below was verified against the
> compiler and spec; per-item ✅/⚠️ status blocks were appended in place. Summary
> of what the pass FOUND AND FIXED (commits on `polymorphism`):
> - **RISK-3 was real**: a 9-case re-entrancy wave found `n.bump(n.value)`-shaped
>   calls panicking `RefCell already borrowed` (argument `Ref` guards outlive
>   into the callee — Rust call-expression temporary scope). Fixed by extending
>   the borrow-hoist to arguments that read wrapper fields, for ANY callee shape
>   (`probes/probe_risk3_reentrancy.jux`, all green).
> - **RISK-1 quadrant non-empty**: two juxc-accepts/rustc-rejects leaks found and
>   fixed — `vec[i].field` on wrapper elements (missing element typing →
>   missing `.0.borrow()`, rustc E0609) and mutated by-value collection params
>   (missing `mut`, rustc E0596). Generic-class aliasing verified SHARED
>   (no silent copy semantics) — `probes/probe_risk1_generics.jux`.
> - **§4.1 init-order contradiction was real and the CODE had the wrong order**:
>   init blocks ran after the ctor body. Fixed to the normative §S.4.4/ERRATA-E2
>   order (field initializers → init → body) in all ctor paths; §M.1 prose +
>   example rewritten; `examples/init_blocks.jux` + e2e updated.
> - **§4.2 E-code collision fixed**: `E0431_GenericInferenceNoSolution` renumbered
>   to `E0453` (the catalog's own §T.4.2 reserved slot — the note's proposed
>   `E0446` had been taken by the generics wave). Catalog + §T.4.2 + collision
>   note updated; `E0446` and `W0240` rows added.
> - **RISK-6 confirmed**: reordered named args evaluate in parameter-slot order
>   (`probes/probe_risk6_evalorder.jux` prints `eval A` before `eval B`).
>   Divergence stays documented (§S.1.3 Phase-1 note) and tracked below.
> - **New unspecced semantic found**: collections pass **by value** (caller does
>   not observe a callee's `push` — `probes/probe_vec_param.jux`). Pending the
>   collections spec (next-session backlog item 2); decide share-vs-value there.

**Scope reviewed:** All 29 Architecture files, including the two core dossiers (`JUX-LANG.md` superseded, `JUX-LANG-V1.md` canonical), the compiler pipeline, type-system, grammar, semantics, operators, exceptions, inheritance-borrow, missing-defs, class-representation, layout/ABI, async, core-lib, codegen-fixes, observable-properties, annotations, entry-points, runtime-ABI, bindgen, build-system, diagnostics, testing, editor/LSP/IntelliJ tooling, ERRATA, GAPS roadmap, and v0.1 readiness.

**How to use this doc:** Work top to bottom. The "Strengths" section is context — do not try to "fix" those, but do verify the claims hold if you change adjacent code. The "Risks & Action Items" section is the work queue, ordered by leverage. Each risk has a **Verification** block: the concrete thing to test, read, or prove.

---

## 0. Orientation — what Jux is

- Java-family syntax, Rust-style memory safety (inferred borrows, no visible lifetimes), native compilation, first-class C/C++/Rust FFI.
- **Phase 1 backend transpiles to Rust source**, hands it to `rustc`. Phase 2 = custom rustc driver. Phase 3 = direct LLVM.
- Borrow checking happens **twice**: once in `juxc` (friendly errors), once in `rustc` (final correctness gate). This double-check is the single most important architectural fact for everything below.
- Three profiles: `jux-full` (refcounted, exceptions, threads, async), `jux-embedded` (optional everything, single-thread async), `jux-core` (no heap/refcount/exceptions/threads/async).
- The spec appears to be written *alongside* a working compiler (readiness doc claims 950+/0 test suite on the `polymorphism` branch, end-to-end pipeline, LSP, IntelliJ plugin). "Phase-1 implementation note" callouts mark built-vs-deferred throughout.

---

## 1. Strengths (verify-but-don't-touch)

These are sound. If you modify adjacent areas, re-confirm they still hold.

1. **Whole-object borrows on classes (§6.9.1, INHERITANCE-BORROW).** Classes borrow at instance granularity; structs/records keep field-level disjoint borrows. This is the keystone that lets "no visible lifetimes" coexist with inheritance + dynamic dispatch. Eliminates upcast-while-borrowed, virtual-dispatch-hides-fields, and `super`-call borrow problems at once.

2. **Mutation union over reachable overrides (§6.9.3, §7.4.1).** Sound answer to inferred `&self`/`&mut self` under dynamic dispatch. Sealed hierarchies make it exact; public extendable classes correctly flagged as fragile-base-class risk, reported at the override site.

3. **Class-representation §CR.4.1 amendment.** Catching that `Rc::make_mut`/`Arc::make_mut` (clone-on-write) silently violates Java shared-mutation semantics, and replacing it with `Rc<RefCell>` / `Arc<Mutex>` + statement-scoped borrows. This is a correctness save most designs would ship as a bug.

4. **Operator-first capability design (OPERATORS addendum).** Removing Equatable/Hashable/Comparable/Cloneable/Displayable/Sized interfaces; auto-derive for value types, identity-default for classes, single-operator-override customization. Cleaner than both Java and Rust for the common case. The `operator==`/`operator hash` pairing enforcement (E0931) closes the classic consistency hole.

5. **Cross-document discipline.** ERRATA.md reconciles contradictions with normative resolutions; GAPS roadmap tracks resolved items; "supersedes" sections are explicit. The integer-`/0`→`ArithmeticException` carve-out (ERRATA E1) is threaded consistently through semantics + exceptions.

6. **Honest deferral tracking.** "Phase-1 implementation note" callouts state exactly what is built vs. deferred (e.g., `byte[N+1]` const-generic arithmetic → E0445 until `generic_const_exprs`; stack-trace capture stubbed; `compareAndSwap` unspecified). This is the discipline that makes the corpus trustworthy.

---

## 2. Risks & Action Items (the work queue)

### RISK-1 — Polymorphic-accept / monomorphic-reject borrow gap (HIGHEST LEVERAGE)

- **What:** `juxc`'s borrow inference (phase 11) runs on **polymorphic source before monomorphization** (TYPE-SYSTEM §T.7.6, PIPELINE §C.1.1) and "transfers" the result to each instantiation without re-running. Meanwhile rustc re-checks the *monomorphized* emitted Rust (§C.9.1 calls rustc "a final correctness gate"). If a program is accepted polymorphically but a specific instantiation would be rejected, the user gets a **raw rustc lifetime/borrow error** — breaking the core "errors never mention lifetimes" promise.
- **Where:** `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.7.6; `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.5, §C.9.1; `JUX-LSP-SERVER-ADDENDUM.md` §L.10 (lowering-stage errors deferred).
- **Why it matters:** This is the failure mode that most directly breaks the headline pitch. The whole value proposition is "Rust safety without Rust's error model." Every uncaught rustc diagnostic is a crack in that.
- **Verification / action:**
  1. Build an adversarial test wave specifically targeting generic + borrow interactions: generic functions taking borrows, generic containers of borrowed values, generic methods that conditionally move vs. borrow depending on `T`, closures capturing generics across `await`.
  2. For each, confirm `juxc` either rejects with a friendly error OR rustc also accepts. The forbidden quadrant is "juxc accepts, rustc rejects."
  3. If the quadrant is non-empty, decide: make phase-11 inference more conservative (reject earlier with a Jux-level message) OR build the span-mapping layer (see RISK-2).
  4. Document the result as a closed/known gap in `JUX-GAPS-ROADMAP.md`.

> **STATUS (2026-06-12): ⚠️ quadrant WAS non-empty — two leaks found, both fixed.**
> `probes/probe_risk1_generics.jux` + `probes/probe_vec_param.jux`:
> (a) `vec[i].field` on a wrapper-class element leaked rustc E0609 — fixed by
> typing builtin-container indexing in `infer_index` (Vec/VecDeque → element,
> HashMap/BTreeMap → value) so the backend's `.0.borrow()` rewrite sees the class;
> (b) a mutated by-value collection param leaked rustc E0596 — fixed by `mut`
> param inference (free fns + methods) reusing `collect_mutated_names`.
> Sharing-semantics spot-checks all CORRECT: aliased generic classes share
> (`Cell<int>` G2), escape-through-generic-return shares (G1), element sharing
> through containers shares (G3). The quadrant should be re-swept after any
> emission change; the probes stay in `probes/` as the regression wave.

### RISK-2 — No rustc-diagnostic → `.jux`-span mapping layer

- **What:** There is no specified component that catches `rustc`'s errors on emitted Rust and rewrites them back to original `.jux` source spans. Both `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.9 and `JUX-LSP-SERVER-ADDENDUM.md` §L.10 treat emitted-Rust / lowering-stage errors as deferred.
- **Where:** PIPELINE §C.9; LSP §L.10; SEMANTICS / DIAGNOSTICS addenda (no span-back-mapping spec found).
- **Why it matters:** This is the load-bearing piece for the "no lifetime jargon" promise — arguably more than `juxc`'s own diagnostic catalog, because rustc errors are what users hit on anything phase-11 lets through (see RISK-1). The diagnostic quality of the *whole language* is gated on this for any borrow case the front end doesn't pre-empt.
- **Verification / action:**
  1. Confirm whether emitted Rust carries source-span provenance (the MIR is said to have "source-span links" in §T.7.1 / §C.5.1 — verify those survive into emitted Rust as `#[line]`-style markers or a sidecar map).
  2. Decide the v0.1 stance: either (a) build a minimal mapper that intercepts rustc's JSON diagnostics and remaps spans, or (b) narrow the *promised* surface (see RISK-7) so the headline claim isn't one rustc error from breaking.
  3. If deferring, add an explicit entry to GAPS and a user-facing note in the docs that advanced cases may surface lower-level diagnostics.

> **STATUS (2026-06-12): ◐ partially mitigated, mapper still unbuilt.**
> Emitted Rust DOES carry provenance — every statement gets a
> `// JUX:<file>:<line>:<col>` source marker (`emit_source_marker`), so a
> sidecar-free remap of rustc JSON diagnostics is feasible (nearest preceding
> marker). No mapper component exists yet. Current stance matches option (b):
> the front end pre-empts aggressively (each RISK-1/RISK-3-style find becomes a
> juxc diagnostic or an emission fix), and `jux build` failures print the rustc
> error labelled "this is a juxc bug" — honest, but still rustc-shaped. The
> mapper remains the right post-v0.1 investment.

### RISK-3 — Class-representation selector: statement-scoped borrow discipline is unchecked

- **What:** The §CR.4.1 amendment mandates **statement-scoped, short-lived `borrow()`/`borrow_mut()`** at every field access so `RefCell` "already borrowed" panics are unreachable in well-formed Jux. This is stated as prose discipline, not a checked invariant.
- **Where:** `JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.4.1 ("Borrow discipline — statement-scoped borrows (NORMATIVE)").
- **Why it matters:** It is the *only* thing preventing `RefCell` runtime panics from leaking to users who were promised they'd never see borrow-checker jargon. A single emitted `a.borrow().f.g()` (guard alive across `g`, where `g` re-enters `a`) is a runtime panic in front of that user. Soundness of the Java-faithful shared-mutation model depends on it.
- **Verification / action:**
  1. Confirm the lowering pass enforces statement-scoped borrows mechanically (read-then-drop-guard-then-call; evaluate-RHS-then-scoped-borrow_mut for writes; per-field-access borrows inside method bodies, never one borrow for the whole method).
  2. Build a dedicated test wave: re-entrant method calls on `this` mid-body, `a.f.g()` chains where `g` touches `a`, nested field access on the same aliased object, callbacks/observers that re-enter the observed object.
  3. Assert zero `RefCell` panics across the wave. Treat any panic as a release blocker, not a quality item.

> **STATUS (2026-06-12): ⚠️ a real panic class found — FIXED.** The wave
> (`probes/probe_risk3_reentrancy.jux`, 9 cases) found that an ARGUMENT reading
> a wrapper field (`n.bump(n.value)`, `h.get().bump(h.get().value)`,
> `n.next!!.bump(n.value)`) keeps its `Ref` guard alive across the call (Rust
> call-expression temporary scope), so the callee's `borrow_mut` panicked. The
> prior hoist machinery covered receivers and inline-class E0499 shapes but
> exempted wrapper receivers entirely. Fix: `call_needs_borrow_hoist` now fires
> for ANY call whose argument reads a wrapper field (`expr_reads_wrapper_field`
> walker), and the receiver-hoist emitter gained arg hoisting for the combined
> case. All 9 cases green, full suite green. The discipline is now mechanically
> enforced for: method bodies (read-temp-then-write), call receivers, call
> arguments, observer fire loops (take-during-fire), and setter brackets.

### RISK-4 — Representation selector stability surprises performance

- **What:** §CR.3.6 says a class's chosen representation (Inline / Box / Rc / Arc / RefCell / Mutex) can change between builds as usage patterns shift. Behavior stays constant but **performance characteristics can change silently** when an unrelated downstream file adds an alias or a cross-thread use.
- **Where:** `JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.3.4–§CR.3.6, decision table §CR.3.3 + §CR.4.1.
- **Why it matters:** A user debugging a mysterious `Arc<Mutex>` slowdown has no visibility into *why* their pure-local-looking class got promoted. This undercuts the "fast by default" promise in a hard-to-diagnose way.
- **Verification / action:**
  1. Add a `--explain-rep <Class>` (or equivalent) diagnostic that prints the selected representation and the specific property (`escapes` / `aliased` / `cross_thread` / `mutated` / `dyn_dispatched` / `weak_target` / `cycle_capable`) that forced it, with the source location of the triggering use.
  2. Consider an optional `// JUX:rep=...` comment in emitted Rust (already floated in §CR.9 Phase D) for users reading generated code.
  3. Validate the selector is deterministic given the same compilation unit (claimed in §CR.3.6) with a reproducibility test.

> **STATUS (2026-06-12): open (quality item).** `--explain-rep` not built.
> Determinism holds structurally: the selector is a pure set computation over
> the AST (no iteration-order-dependent output observed; results are name-keyed
> sets). Provenance recording (WHICH rule aliased a class) is the prerequisite
> for `--explain-rep` and isn't tracked today. Queue behind the collections
> spec work.

### RISK-5 — Selector decision table vs. borrow rules: soundness audit

- **What:** The §CR.4.1 decision table splits aliased/cross_thread rows on `mutated` to pick `Rc` vs `Rc<RefCell>` vs `Arc` vs `Arc<Mutex>`. The `mutated` property is said to reuse the receiver-mutation analysis. Inheritance "rolls up" to the least-general rep across the chain (§CR.3.5).
- **Where:** `JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.2–§CR.4.1; interacts with INHERITANCE-BORROW §6.9.3 mutation union.
- **Why it matters:** If `mutated` is computed per-class but the mutation *union* over virtual overrides (§6.9.3) is what actually determines whether a call mutates, there's a possible mismatch: a base class could be selected as immutable `Rc` while a downstream override mutates, making the rep unsound for shared mutation.
- **Verification / action:**
  1. Confirm `mutated` for a polymorphic base class includes the mutation union over all reachable overrides, not just the base's own body.
  2. Confirm the inheritance roll-up (§CR.3.5) escalates the *whole chain* to an interior-mutable rep when any member is both aliased and mutated.
  3. Test: base `Rc` + downstream override that writes a field through an aliased reference — assert shared mutation is observable (Java semantics) and no copy-on-write fork occurs.

> **STATUS (2026-06-12): ✅ sound by construction in Phase 1.** The implemented
> selector is BINARY — wrapped `Rc<RefCell>` vs inline plain struct — and the
> `mutated` property is NOT consulted: demotion requires proof of *no aliasing*
> (`compute_aliased_classes` rules 1–4), and every aliased class keeps interior
> mutability regardless of mutatedness. The §CR.3.5 component roll-up IS
> implemented (whole extends-component shares one rep, both directions). The
> RISK-5 scenario (immutable `Rc` base + mutating override) cannot arise until
> the finer `Rc`-without-`RefCell` tier lands — AT WHICH POINT the mutated
> property must take the §6.9.3 union over reachable overrides. Carry this as a
> precondition on that future work.

### RISK-6 — Default arg / named-arg evaluation-order divergence

- **What:** §S.1.4 promises named arguments evaluate in **call-site lexical order**; §S.1.3 Phase-1 note says the current compiler evaluates in **parameter-slot order** for reordered named args, and rejects defaults referencing other parameters (E0449) until temp-hoisting lands.
- **Where:** `JUX-SEMANTICS-ADDENDUM.md` §S.1.3 (Phase-1 note), §S.1.4.
- **Why it matters:** Only observable with side-effecting reordered arguments, but it's a spec-vs-implementation divergence that will produce subtly wrong programs (or surprising eval order) silently.
- **Verification / action:**
  1. Add tests with side-effecting reordered named args; document the current behavior loudly until the temp-hoisting lowering lands.
  2. Track in GAPS; ensure the spec's normative §S.1.4 and the Phase-1 note don't drift apart.

> **STATUS (2026-06-12): ⚠️ divergence confirmed, documented, tracked.**
> `probes/probe_risk6_evalorder.jux`: `f(b: trace("B", 2), a: trace("A", 1))`
> prints `eval A` then `eval B` — parameter-slot order, exactly as the §S.1.3
> Phase-1 note states (spec §S.1.4 wants lexical). The fix shape is known: the
> expansion plan (`ArgSource` per slot) still knows the lexical permutation at
> expansion time, so the backend could hoist explicit args into temps in
> lexical order and pass the temps in slot order. Deferred; the dangerous
> interaction (defaults referencing parameters) is already hard-rejected by
> E0449, so the residue is side-effect ordering only.

### RISK-7 — Strategic: transpile-to-Rust creates a standing tension between two headline promises

- **What:** "Ship only what you use / Rust-class performance" vs. "borrow errors never mention lifetimes" are in tension under the Phase-1 strategy. The `Rc<RefCell>`/`Arc<Mutex>` default (before the selector demotes) is *more* runtime machinery than idiomatic hand-written Rust (taxes promise 1), and every rustc error not pre-empted by the front end taxes promise 2 (RISK-1, RISK-2).
- **Where:** Whole-corpus; crystallized in PIPELINE §C.9.1, CLASS-REPRESENTATION §CR.1/§CR.9, LANG-V1 §1.1 design goals.
- **Why it matters:** Phase 2/3 (rustc driver → direct LLVM) ultimately resolves this and the spec knows it — but v0.1 ships on Phase 1.
- **Verification / action:**
  1. Decide whether to **narrow the promised surface for v0.1**: e.g., "friendly borrow errors for the common feature set; advanced generic+borrow interactions may surface lower-level diagnostics." This protects the headline from a single rustc-error counterexample.
  2. Make the performance promise honest about the interior-mutability cost where sharing+mutation is observable, and lean on the escape-analysis selector as the documented "fast tier."

> **STATUS (2026-06-12): position adopted.** The readiness doc already states
> the narrowed claim: "the inferred borrow checker is release-grade for the
> common feature set — no known rustc borrow-error leaks, RefCell panics, or
> silent-wrong lowerings on valid input", with the source markers + the
> "this is a juxc bug" labelling as the honesty mechanism for what slips
> through. The fast tier (inline demotion) exists and is conservative-correct.
> Keep re-running the RISK-1/RISK-3 probe waves after emission changes.

---

## 3. First-class-feature gaps (flagged by the corpus itself)

These are acknowledged deferrals, listed so a reviewer weights them. Not bugs — scope decisions to confirm.

- **No macro / derive model (GAPS §3.4).** Blocks `@Serializable`-style derives, which blocks `std.json`'s native shape. Worked around via Rust crates (`serde_json`) through `.jux.d` stubs (BINDGEN addendum). Net effect: "real" Jux serialization libraries don't exist until macros land; the ecosystem story is "use Rust's stdlib via bindgen" for a while. **Confirm this is an accepted v0.1 posture.**
- **Generic class as polymorphic base rejected with E0454 (§6.9.6 Phase-1 limit).** Sensible deferral; visible hole for Java generics-heavy hierarchies. Supported routes: generic *interface* (works) or non-generic base. **Confirm error message points users to the working routes.**
- **Closures don't capture locals in anonymous-class bodies (§7.3 Phase-1 note).** Sharp edge that will surprise Java developers immediately — it's the one place Java's capture "just works." **Confirm the diagnostic suggests the thread-through-parameters / sibling-lambda workaround.**
- **Stack-trace capture stubbed (EXCEPTIONS §X.1.1 Phase-1 note, SEMANTICS §S.7.3).** `Exception.stackTrace` returns empty until the DWARF-walk work lands. **Confirm no code path assumes non-empty stack traces.**
- **`compareAndSwap` / `AtomicRef<T>` unspecified (SEMANTICS §S.6.2 Phase-1 note).** `CasResult<T>` return type referenced but not defined. **Define `CasResult<T>` before implementing CAS.**
- **`out null` and call-site `move` operator deferred (MISSING-DEFS §M.4.6).** FFI/ownership refinements not required by the Phase-1 surface. **Confirm FFI examples that use `out null` are marked non-normative until it lands.**
- **`byte[N+1]` const-generic arithmetic over a generic param → E0445 (TYPE-SYSTEM §T.11.6 / LANG-V1 §5.5 notes).** Needs nightly `generic_const_exprs`. Bare `byte[N]` works. **Confirm.**

---

## 4. Cross-document consistency — spots to re-audit

ERRATA.md has caught the big contradictions (panic-vs-exception, init/super order, async borrow phase, cross-module `protected`, nullable primitives, elvis aliases, switch exhaustiveness code, duplicate-local code, observable-properties renumbering). A reviewer should still re-check:

1. **`init` block ordering: two different orders stated.** MISSING-DEFS §M.1 originally describes `init` running *after* the constructor body; ERRATA E2 + SEMANTICS §S.4.4 + §S.1.5 say `init` runs *before* the constructor body (step 3/4, after field initializers). The ERRATA resolution is normative (init before body) — **verify §M.1's prose was updated to match, or that the §M.1.2 "step 5 / after body" wording is reconciled.** This is a live internal contradiction to confirm closed.
   > **STATUS: ⚠️ worse than a doc contradiction — the COMPILER had the wrong order. FIXED (2026-06-12).** All ctor paths (inline simple/builder, wrapper builder) ran init blocks AFTER the body. Reordered to the normative sequence (field initializers → init → body; simple-ctor fast path now skipped when init blocks exist so the fold can't leak body writes into init's view). §M.1 prose + example rewritten (4 spots), `examples/init_blocks.jux` + e2e rewritten to assert the Java order, `probes/probe_init_order.jux` covers both representations. Side-find fixed with it: `this.<AutoProp>` in init blocks missed the ctor backing-field rewrite (desugar now applies it to `init_blocks` too).
2. **Diagnostic-code collisions.** Multiple addenda allocate E04xx, E05xx, E07xx, E09xx codes independently. ERRATA E9 fixed one observable-properties collision (E0401–E0407). **Run a global grep for duplicate E-codes across all addenda** and reconcile against DIAGNOSTICS §D.4 as the master catalog.
   > **STATUS: ⚠️ one real collision — FIXED (2026-06-12).** `Code` enum carried two variants printing `E0431` (method-modifiers + generic-inference-no-solution). The latter renumbered to `E0453` — the catalog's own reserved §T.4.2 slot (the collision note's proposed `E0446` had since been taken by `GenericBoundNotSatisfied`; note updated, `E0446` + `W0240` catalog rows added). Codes cited in docs but absent from code.rs are all marked *(reserved)* — fine. rustc codes appearing in code.rs are doc-comment attributions ("instead of leaking rustc's E0282"), not allocations — fine.
3. **`spawn` keyword vs. function.** ERRATA / MISSING-DEFS §M.12.1 resolve it to a library function and say "remove from reserved-keyword list" — but GRAMMAR §A.1.3 keyword list and LANG-V1 §3.2 should be confirmed updated (GRAMMAR notes spawn is *not* a keyword in §A.4; **verify the keyword tables agree**).
   > **STATUS: ✅ CLEAN.** All three docs + the lexer's keyword enum agree: `spawn` is a library function, not a keyword.
4. **`@Derive` status.** OPERATORS §O.9 says `@Derive` is a no-op (W0240); MISSING-DEFS §M.3 says largely obsolete; ANNOTATIONS §A.1 still lists it. **Confirm all three agree it's accepted-but-no-op.**
   > **STATUS: ⚠️ minor wording — FIXED (2026-06-12).** ANNOTATIONS table row implied @Derive generates implementations; reworded to "deprecated no-op, operators auto-derive unconditionally". W0240 added to the §D.4 catalog as reserved (warning not yet emitted by the compiler — acceptable Phase-1 gap).
5. **Foundational interfaces removed everywhere.** OPERATORS supersedes LANG-V1 §9.4, MISSING-DEFS §M.10, TYPE-SYSTEM §T.1.3, CORE-LIB §K.2. LANG.md (the old dossier, §9.4) still lists `Equatable`/`Comparable`/etc. — it's marked SUPERSEDED so that's fine, but **confirm no live addendum still references the removed interfaces as if they exist.**
   > **STATUS: ✅ CLEAN.** Every live addendum references the interfaces only as explicit removals pointing at the operator design.

---

## 5. Suggested verification sequence (for an implementing AI)

Ordered by leverage; do not reorder without reason.

1. **RISK-1 + RISK-2 together** — the polymorphic/monomorphic borrow gap and span-mapping. This protects the core pitch. Build the adversarial generic+borrow wave first; its results decide whether RISK-2's mapper is mandatory or RISK-7's narrowing is enough.
2. **RISK-3** — statement-scoped borrow enforcement + re-entrancy panic wave. Release blocker.
3. **RISK-5** — selector `mutated` vs. mutation-union soundness. Could be a silent shared-mutation correctness bug.
4. **Section 4 consistency re-audit** — cheap, unblocks correctness, especially items 1 (init order) and 2 (duplicate codes).
5. **RISK-4** — `--explain-rep` diagnostic. Quality-of-life but cheap and high-trust.
6. **RISK-6** — eval-order divergence tests + GAPS tracking.
7. **Section 3 gaps** — confirm each is an accepted v0.1 posture with a user-facing note where the edge is sharp (anonymous-class capture, generic-base E0454).

---

## 6. One-line summary for triage

The design is internally coherent and unusually well-tracked; the dominant risk is not design soundness but the **transpile-to-Rust seam** — specifically any path where `juxc` accepts what `rustc` rejects (RISK-1) with no span-back-mapping (RISK-2), and the **unchecked statement-scoped borrow discipline** that keeps `RefCell` panics away from users (RISK-3). Close those three and the headline promises hold for the common feature set.

> **Post-verification (2026-06-12):** RISK-3's panic class and RISK-1's two
> rustc leaks are closed (with regression probes); RISK-5 is sound by
> construction in Phase 1; the init-order contradiction was a live compiler
> bug, now fixed to spec; the E0431 collision is renumbered. Remaining open:
> RISK-2's diagnostic mapper (post-v0.1; source markers make it feasible),
> RISK-4's `--explain-rep`, RISK-6's lexical-order lowering, and the
> collections passing-semantics ruling (new finding — collections currently
> pass by value; spec it in the collections session).
