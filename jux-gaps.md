# Jux — Gap Analysis (implementation vs. spec)

**Author:** Claude (Opus 4.8) — grounded review.
**Last updated:** 2026-06-11 (re-verification pass; branch `polymorphism`)

> Re-verification note (this pass): every Resolved row below was re-checked against
> source and is accurate — `emit_call_with_hoisted_receiver` (`exprs/call.rs:1420`),
> `compute_wrapped_set` (`lib.rs:210`/`:348`), `W0457`/`check_unannotated_cycles`
> (`symbol_table.rs:1060`), `source_map.rs::rewrite_rustc_output`. The
> `reentrancy_stress` and `exception_cause` tests both pass. **One correction:**
> exception chaining (`getCause`/`addSuppressed`/`getSuppressed`/2-arg ctor) is in
> fact wired and tested — moved from backlog to Resolved (G9).

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

### G9. Try/catch/finally + exception chaining — **done** ✅
Lowering (`stmts.rs:933`) via `catch_unwind`; return-in-catch parks past `finally`,
return-in-finally overrides, multi-catch + subtype dispatch all implemented and tested
(`catch_finally_order`, `multi_catch`, `try_finally_semantics`). **Exception chaining
is also wired and tested** — `new Exception(message, cause)`, `getCause()`,
`addSuppressed()` / `getSuppressed()` all work; `exception_cause` test passes
(re-verified this pass). The earlier "chaining deferred" note is closed.

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

### G13. Multi-file Rust output — **done** ✅
Closed (commit `4ff2df1`). The backend emits **one `.rs` per `.jux` file** mirroring the package
tree (`emit_package_files` / `split_files` in `lib.rs`), with `mod.rs` re-exports
(`mod x; pub use x::*;`) keeping type paths flat (`crate::pkg::Type`). Module/file names are
lower-cased to avoid the module-vs-type name clash; `use super::*;` resolves same-package siblings;
split-mode items are `pub(crate)`. `main.rs` keeps the prelude + no-package units + `pub mod` decls
+ `fn main` shim. All ~144 example runners compile + run the multi-file output.

### G15a. Const evaluation (§T.11 subset) — **done** ✅
Closed this session. A shared const-expression evaluator (`juxc-tycheck/src/const_eval.rs`, used by
both tycheck and the backend) folds integer/bool const expressions over concrete constants —
literals, `const`/`final` binding reads, arithmetic/bitwise/comparison/logical ops, and calls to
const-evaluable Java-style functions (const-ness is a property of the expression, §A.2.2 — **no `fn`
keyword**). Wired into const-binding initializers (`const int CACHE = doubled(1024);` → `2048`),
fixed-array sizes (`byte[SIZE + 1]` → `[u8; 33]`), and const-generic args (`Ring<float, SIZE>` →
`Ring::<f32, 32>`). New codes `E0840`/`E0841`/`E0842`. Generic-`N` arithmetic (`byte[N+1]`) stays
`E0445` (Rust-blocked — see G14). §T.11 spec corrected from Rust-flavored `const fn … -> T` to Jux.

---

## Backlog (real, tracked, each its own effort)

| ID  | Gap                                                        | Notes |
|-----|------------------------------------------------------------|-------|
| G1b | Box / Arc class representations (beyond Inline + Rc)        | Further perf tiers; §CR.2–3. Inline + Rc shipped. |
| G2  | Classes across threads (`Arc<Mutex>` rep)                  | Hard-errors today (E0702); depends on G1b. |
| G5  | No MIR — flow analyses ride on the AST                      | Architectural; decide explicitly if Phase 1 ships without it. |
| G10 | FFI (C/C++)                                                | Deferred by user; path known (build.rs + bindgen/autocxx). |
| G11 | `StackString`, `Volatile`, `SharedRef`                     | Escape-selector / unsafe-dependent. |
| G12 | Nested / inner / anonymous classes                         | Partially landed; full support post-Phase-1. |
| G14 | Per-instantiation generic representation                   | One rep across instantiations today (§CR.5.3); blocks generic-`N` const arithmetic. |
| G15 | Remaining wave-6 items (annotations, `module.jux`, out/move) | Tracked in wave-6 notes. |

---

## The one-line take

The two flagged risk hotspots are addressed: the escape-selector's biggest lever (Inline
demotion) is shipped, and the one load-bearing soundness claim (G3) turned out to hide a
real re-entrancy bug — now fixed and CI-guarded. What remains in the backlog is genuine but
scheduled: further representation tiers, cross-thread sharing, MIR, and the deferred
language/library features.
