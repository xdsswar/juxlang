# Jux v0.1 Production-Readiness Checklist

**Purpose:** one place to answer "what must ship for v0.1, and where does it
stand?" — unifying the three independent gap ledgers (`jux-gaps.md` = compiler
bugs, `plugin-gap.md` = IDE plugin, `Architecture/JUX-GAPS-ROADMAP.md` = spec
gaps) into a single status view. Status reflects the `polymorphism` branch.

**Legend:** ✅ done · ◐ partial · ⛔ open (v0.1 blocking) · ⏭ deferred (post-v0.1).

---

## 1. Compiler soundness & correctness

| Area | Status | Reference |
|------|--------|-----------|
| Core pipeline (lex→parse→resolve→tycheck→lower→codegen→build) | ✅ | end-to-end, 950+/0 suite |
| Generics: explicit type-args, wildcards (`? extends`/`? super`), const generics, invariance | ✅ | `project_generics_status`; N2 |
| Polymorphism: interface + class virtual dispatch, `super` (incl. statement position), downcast, `=>` smart-cast | ✅ | `project_polymorphism_stages`; S11 |
| Borrow discipline (`Rc<RefCell>` shared-mutable): re-entrancy, wrapped fields, collections, `!!`, for-each, operators, **field-path receivers, async tries, higher-order stdlib calls, observers** | ✅ | N1, G3, H1, H5, H6, H9 + wave-3 S1–S15 (all closed, runner `borrow_stress_wave3`) |
| `?.` safe-navigation over wrapped classes + multi-level chains | ✅ | H5 |
| Exceptions: try/catch/finally ordering, chaining, multi-catch, subclass→base cause upcast, `/ 0` → catchable `ArithmeticException`, uncaught-exception report | ✅ | H8, O1–O9 all closed |
| Diagnostics: juxc catches its own errors (no rustc leaks), 61 E/W codes | ◐ | E0454/E0974 added; remaining known leaks: S16–S18 (async edges) |
| Generic *class* as a polymorphic base | ⏭ | N5 — rejected cleanly with E0454; use a generic interface |
| Remaining known gaps (async edges: un-awaited async call, `Worker.spawn` + async lambda, outer-local mutation in async try) | ⛔ | `jux-gaps.md` S16–S18 (low frequency) |

## 2. Codegen quality

| Item | Status |
|------|--------|
| String literals → owned `String`; no `&str` for `String` slots | ✅ |
| `rustfmt` on emitted output (idempotent); `JUX_NO_RUSTFMT` debug escape hatch | ✅ |
| `#![allow(...)]` banner — zero rustc warnings on the corpus | ✅ |
| `Display` impls print payloads with field names (§7.7.2) | ✅ |
| Non-interpolated `$"…"` → `.to_string()` (no `format!`) | ✅ |
| Multi-file Rust output (mirror Jux files) / optimized Rust | ⏭ deferred (`project_codegen_quality_requests`) |

*(All five `JUX-CODEGEN-FIXES.md` items complete.)*

## 3. Standard library (Phase 1 = thin wrappers over Rust std)

| Item | Status | Reference |
|------|--------|-----------|
| String API, numerics, wrapping ops, Deque, I/O+Time, Atomics | ✅ | `project_wave5_progress` |
| Collections (List/Map/Set/Deque) backed by Vec/HashMap/HashSet/VecDeque | ◐ | spec surface incomplete |
| Value semantics — equality / ordering / hashing / formatting | ✅ | **operator** overrides (`==`/`hash`/`string`/ordering), C++-style, not interfaces; consistency enforced (E0930/E0931). See `JUX-CORE-LIB-ADDENDUM` §72 |
| `Iterable<T>` / `Iterator<T>` (only nominal foundational interfaces) + `for-each` desugaring | ✅ | `JUX-CORE-LIB-ADDENDUM` §K.5; `user_iterable.jux` |
| Exception hierarchy + Result lowering | ✅ | `JUX-EXCEPTIONS-ADDENDUM` |
| Async streams (`Stream<T>`, `for await`, of/from/generate, combinators) | ✅ | §18.6 specced + implemented; E0703/E0704; runner `async_streams` |
| Networking / HTTP / JSON | ⏭ | needs metaprogramming (roadmap §3.4) |

## 4. Toolchain & IDE

| Item | Status |
|------|--------|
| `juxc` / `jux` CLI, manifest-driven builds, per-module binary metadata + icon | ✅ |
| LSP server (`juxc-lsp`) — single source of truth | ✅ |
| IntelliJ plugin: PSI parser, semantic highlighting, formatter, native inspections + quick-fixes, goto, completion, LSP4IJ fallback | ✅ |
| IntelliJ refactoring (move/rename/extract/inline/change-signature), debugger, test-runner UI | ⏭ | `plugin-gap.md` |
| Build system / package manager (`jux.toml`), multi-module workspaces, **path + git deps (GitHub URLs, `jux update`), `--target` cross-compile** | ✅ | §B.2.2; registry deps + `jux.lock` remain post-v0.1 |
| Testing framework (`@Test` + hooks, `jux.std.testing` assertions, `jux test [pattern] [--release]`, async tests) | ✅ | `JUX-TESTING-ADDENDUM.md`; runner `test_runner` |

---

## What blocks calling it v0.1

**Nothing structural remains.** Async streams (§18.6) and the testing
framework (§21) — the last two feature blockers — landed 2026-06-12. The
only ⛔ row left is the trio of low-frequency async edges (S16–S18 in
`jux-gaps.md`), which are quality items rather than missing features.
The O-series is fully closed and
the borrow machinery survived a 15-probe adversarial wave with every finding
fixed (2026-06-12) — the **inferred borrow checker is release-grade for the
common feature set**: no known rustc borrow-error leaks, RefCell panics, or
silent-wrong lowerings on valid input. Jux libraries are consumable straight
from GitHub (`"com.x.lib" = "https://github.com/u/repo"`), and `jux build
--target <triple>` cross-compiles to any installed rustc target. Value
semantics (equality/ordering/hashing/formatting) and the `Iterable` contract
are done via the operator-based design — the roadmap's old interface-based
§1.1/§19.1 plan was superseded by `JUX-CORE-LIB-ADDENDUM` and is not a gap.
Everything else is either done or an explicit post-v0.1 deferral.
