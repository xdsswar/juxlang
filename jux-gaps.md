# Jux — Gap Analysis (implementation vs. spec)

**Author:** Claude (Opus 4.8) — grounded review.
**Last updated:** 2026-06-11 (post triage + fixes; branch `polymorphism`)

## How to read this

Each gap is marked with its **current** code state, re-verified against source with
file:line anchors. The doc is a living ledger: when a gap is closed the row moves to
**Resolved**, with the commit that closed it. Backlog items are real but each is its
own multi-session effort.

---

## Resolved

### G3. Statement-scoped borrow soundness — **was a real bug, now fixed** ✅
**Closed:** this session. A re-entrancy stress test (`examples/reentrancy_stress.jux` +
`bin/jux/tests/reentrancy_stress.rs`) surfaced a genuine soundness bug: a method call
whose receiver was read through a `.0.borrow()` guard (`this.notifier.ping(this)`)
held that guard across the call, and a re-entrant `borrow_mut()` panicked
`already borrowed`. Fixed by a **receiver borrow-hoist** (`emit_call_with_hoisted_receiver`,
`exprs/call.rs`): the receiver is bound to a temp so the guard drops before the call.
The stress test (cross-object re-entry + self-recursion) now runs clean and is a CI
guard. The discipline is no longer "asserted, not proven" for these cases.

### G1 (Inline tier). Escape demotion — **done** ✅
The headline "every class pays full `Rc<RefCell>`" is **outdated**. `compute_wrapped_set`
(`backend/lib.rs:1671`) wraps only classes that are wrap-eligible AND aliased (or forced
by interface/poly/recursive/weak); a non-aliased local class emits a plain
`#[derive(Clone,Debug)] struct C { … }` (`decls/classes.rs:177`) with direct `self.field`
access (`exprs/field.rs:449`) and `&mut self` methods. A purely-local `Point` pays no heap
alloc / refcount / borrow check. (Box/Arc tiers remain — see backlog.)

### G6. Source-map remapping — **done** ✅
`juxc-driver/src/source_map.rs` `rewrite_rustc_output` rewrites rustc stderr back to `.jux`
locations, wired into the build-failure path (`driver/lib.rs:560`). `// JUX:file:line:col`
markers are ON for all real lowering entry points (`lower_with_source` / `lower_workspace`
/ `_lib` / `_test`); only the legacy `lower_with_types` omits them.

### G7. Backend file size — **done** ✅
Split into `decls/`, `exprs/`, `stmts.rs`, `literals.rs`, etc.

### G9. Try/catch/finally — **done** (chaining deferred) ✅
Lowering (`stmts.rs:933`) via `catch_unwind`; return-in-catch parks past `finally`,
return-in-finally overrides, multi-catch + subtype dispatch all implemented and tested
(`catch_finally_order`, `multi_catch`, `try_finally_semantics`). **Exception chaining**
(`cause` / `addSuppressed`) is not wired — backlog.

### G16. CI compiles emitted Rust — **done** ✅
Every `bin/jux/tests/*.rs` runner invokes `jux run` → `juxc_driver::build` → `cargo build`
on the emitted crate (`driver/lib.rs:536`), then runs the binary. Grammar acceptance is
not the gate — real rustc compilation is.

### G8. `int` width doc mismatch — **fixed** ✅
Closed this session. The **code** is consistent: `int→isize`, `uint→usize` (platform-sized,
matching the bindgen `isize↔int` contract), `long→i64`, `ulong→u64`
(`types.rs` `jux_primitive_to_rust`). The doc table claimed `i32`/`&str` — corrected to
match the code. No code change needed; the implementation was right.

### G18. E0702 message framing — **fixed** ✅
Closed this session. Reworded to frame the no-objects-across-threads rule as a Phase-1
interim ("…not yet supported … for now pass primitive/String data in") rather than a
permanent law (`tycheck/check.rs`).

### G4 / G17. Un-annotated cycle leak — **lint added** ✅
Closed this session. New **W0457** warning (`tycheck/symbol_table.rs`
`check_unannotated_cycles`): a strong (non-`weak`, non-static) field whose type
transitively references the owning class forms a leaking `Rc` cycle; the lint points at the
field and suggests `weak`. Stdlib packages are excluded (their internal cycles like
`Exception.cause` are intentional). Verified: `weak_refs.jux` is clean; the lint flags
exactly one real cycle (`stress_borrow.jux`'s `Node.peer`) across all 144 examples. The
eventual cycle *collector* (trial-deletion) is still unscheduled (backlog).

---

## Backlog (real, tracked, each its own effort)

| ID  | Gap                                                        | Notes |
|-----|------------------------------------------------------------|-------|
| G1b | Box / Arc class representations (beyond Inline + Rc)        | Further perf tiers; §CR.2–3. Inline + Rc shipped. |
| G2  | Classes across threads (`Arc<Mutex>` rep)                  | Hard-errors today (E0702); depends on G1b. |
| G5  | No MIR — flow analyses ride on the AST                      | Architectural; decide explicitly if Phase 1 ships without it. |
| G9t | Exception chaining (`cause` / suppressed)                  | Deferred tail of G9. |
| G10 | FFI (C/C++)                                                | Deferred by user; path known (build.rs + bindgen/autocxx). |
| G11 | `StackString`, `Volatile`, `SharedRef`                     | Escape-selector / unsafe-dependent. |
| G12 | Nested / inner / anonymous classes                         | Partially landed; full support post-Phase-1. |
| G13 | Multi-file Rust output mirroring `.jux` files              | User-requested; one emitted unit today. |
| G14 | Per-instantiation generic representation                   | One rep across instantiations today (§CR.5.3). |
| G15 | Remaining wave-6 items (const-eval, annotations, module.jux, out/move) | Tracked in wave-6 notes. |

---

## The one-line take

The two flagged risk hotspots are addressed: the escape-selector's biggest lever (Inline
demotion) is shipped, and the one load-bearing soundness claim (G3) turned out to hide a
real re-entrancy bug — now fixed and CI-guarded. What remains in the backlog is genuine but
scheduled: further representation tiers, cross-thread sharing, MIR, and the deferred
language/library features.
