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
| Core pipeline (lex→parse→resolve→tycheck→lower→codegen→build) | ✅ | end-to-end, 936/0 suite |
| Generics: explicit type-args, wildcards (`? extends`/`? super`), const generics, invariance | ✅ | `project_generics_status`; N2 |
| Polymorphism: interface + class virtual dispatch, `super`, downcast, `=>` smart-cast | ✅ | `project_polymorphism_stages` |
| Borrow discipline (`Rc<RefCell>` shared-mutable): re-entrancy, wrapped fields, collections, `!!`, for-each, operators | ✅ | N1, G3, H1, H5, H6, H9 |
| `?.` safe-navigation over wrapped classes + multi-level chains | ✅ | H5 |
| Exceptions: try/catch/finally ordering, chaining, multi-catch, subclass→base cause upcast | ◐ | H8 done; O1/O2/O3/O6/O7 open |
| Diagnostics: juxc catches its own errors (no rustc leaks), 59 E/W codes | ◐ | E0454 added; a few leaks remain (O-series) |
| Generic *class* as a polymorphic base | ⏭ | N5 — rejected cleanly with E0454; use a generic interface |
| Remaining miscompiles (lambda-try, break/continue-in-try, generic exception class, `g[i]+=`, block-lambda arg) | ⛔ | `jux-gaps.md` O1–O5 (lower frequency) |

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
| Async streams (`Stream<T>`, `for await`) | ⛔ | roadmap §18.6 |
| Networking / HTTP / JSON | ⏭ | needs metaprogramming (roadmap §3.4) |

## 4. Toolchain & IDE

| Item | Status |
|------|--------|
| `juxc` / `jux` CLI, manifest-driven builds, per-module binary metadata + icon | ✅ |
| LSP server (`juxc-lsp`) — single source of truth | ✅ |
| IntelliJ plugin: PSI parser, semantic highlighting, formatter, native inspections + quick-fixes, goto, completion, LSP4IJ fallback | ✅ |
| IntelliJ refactoring (move/rename/extract/inline/change-signature), debugger, test-runner UI | ⏭ | `plugin-gap.md` |
| Build system / package manager (`jux.toml`), multi-module workspaces | ◐ |
| Testing framework (`@Test`, `juxc test`) | ⛔ | roadmap §21 |

---

## What blocks calling it v0.1

The ⛔ rows: **async streams (§18.6)**, a **testing framework (§21)**, and
closing the remaining O-series miscompiles so juxc never leaks a rustc error.
Value semantics (equality/ordering/hashing/formatting) and the `Iterable`
contract are done via the operator-based design — the roadmap's old
interface-based §1.1/§19.1 plan was superseded by `JUX-CORE-LIB-ADDENDUM` and is
not a gap. Everything else is either done or an explicit post-v0.1 deferral.
Compiler soundness for the common feature set is in good shape — the recent
hardening waves drove the suite from ~900 to 936 green with no known panics on
valid input.
