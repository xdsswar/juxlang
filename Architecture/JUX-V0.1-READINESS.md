# Jux v0.1 Production-Readiness Checklist

**Purpose:** one place to answer "what must ship for v0.1, and where does it
stand?" — unifying the three independent gap ledgers (`docs/internal/jux-gaps.md` = compiler
bugs, `docs/internal/plugin-gap.md` = IDE plugin, `Architecture/JUX-GAPS-ROADMAP.md` = spec
gaps) into a single status view. Status reflects the `refine` working branch
(last reviewed 2026-09-15; see the dated note at the end).

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
| Diagnostics: juxc catches its own errors (no rustc leaks), 123 E/W codes in `crates/juxc-diagnostics/src/code.rs` (120 errors, 3 warnings) | ✅ | E0974, E0705/E0706 added (E0454 retired — the construct compiles now); S16–S18 async-edge leaks closed 2026-06-12 |
| Generic *class* as a polymorphic base | ✅ | N5 — generic `Kind` traits + `Rc<dyn ContainerKind<T>>`; E0454 retired |
| Async edges (un-awaited async → E0705, `Worker.spawn` async lambda → block_on, async-try outer mutation → E0706) + typed ctor overloads (S19) | ✅ | closed 2026-06-12; runner `async_edges` |
| Observable properties §P: core + ALL follow-ups (computed deps, E0973 gate, bidi unbind, adapter pruning, ctor bind, static props) | ✅ | P1–P7 closed 2026-06-12; runner `observable_props` |

## 2. Codegen quality

| Item | Status |
|------|--------|
| String literals → owned `String`; no `&str` for `String` slots | ✅ |
| `rustfmt` on emitted output (idempotent); `JUX_NO_RUSTFMT` debug escape hatch | ✅ |
| `#![allow(...)]` banner — zero rustc warnings on the corpus | ✅ |
| `Display` impls print payloads with field names (§7.7.2) | ✅ |
| Non-interpolated `$"…"` → `.to_string()` (no `format!`) | ✅ |
| Multi-file Rust output (mirror Jux files) | ◐ binary workspaces emit one `src/<pkg>/<file>.rs` per packaged Jux unit plus a per-package `mod.rs` (`emit_package_files` in `crates/juxc-backend-rust/src/lib.rs`); the library and test variants are still single-file |
| Optimized Rust output | ⏭ deferred (`project_codegen_quality_requests`) |

*(All `JUX-CODEGEN-FIXES.md` items are implemented. One Fix 4 acceptance
check, a keyword-named enum record-variant field, has no test yet and is
marked unverified there.)*

## 3. Standard library (Phase 1 = thin wrappers over Rust std)

| Item | Status | Reference |
|------|--------|-----------|
| String API, numerics, wrapping ops, Deque, I/O+Time, Atomics | ✅ | `project_wave5_progress` |
| Collections — Rust's `Vec`/`HashMap`/`HashSet`/`VecDeque` under their own names | ✅ | the Java-shaped `List`/`Map`/`Set`/`Collection` facade was removed 2026-09-08; `Iterable`/`Iterator` stay as the for-each protocol |
| Value semantics — equality / ordering / hashing / formatting | ✅ | **operator** overrides (`==`/`hash`/`string`/ordering), C++-style, not interfaces; consistency enforced (E0930/E0931). See `JUX-CORE-LIB-ADDENDUM` §72 |
| `Iterable<T>` / `Iterator<T>` (only nominal foundational interfaces) + `for-each` desugaring | ✅ | `JUX-CORE-LIB-ADDENDUM` §K.5; `user_iterable.jux` |
| Exception hierarchy + Result lowering | ✅ | `JUX-EXCEPTIONS-ADDENDUM` |
| Async streams (`Stream<T>`, `for await`, of/from/generate, combinators) | ✅ | §18.6 specced + implemented; E0703/E0704; runner `async_streams` |
| Networking / HTTP | ✅ | through `rust.*` crate bindings plus the compile-time annotation registry: `examples/http_server` (a `tiny_http` server routed by a `@Route` annotation read back from `jux.meta.Registry`) and `examples/tcp_echo.jux` |
| JSON | ⏭ | no example or gate yet; the `rust.*` crate path is the expected route |

## 4. Toolchain & IDE

| Item | Status |
|------|--------|
| `juxc` / `jux` CLI, manifest-driven builds, per-module binary metadata + icon | ✅ |
| LSP server (`juxc-lsp`): diagnostics, hover and exact types for the editor (hybrid engine, see next row) | ✅ |
| IntelliJ plugin: its own PSI parser (Kotlin, tokens generated from `juxc-lex`), semantic highlighting, formatter, native inspections + quick-fixes, completion, go-to and find usages from the plugin's engine; `juxc-lsp` supplies diagnostics, hover and exact types (native LSP client or LSP4IJ fallback) | ✅ |
| IntelliJ test-runner UI (`JuxTestEventsConverter`, `JuxTestConsoleProperties`, `JuxTestLocator`, test run-configuration producer) | ✅ |
| IntelliJ refactoring: Rename, Inline Variable, Introduce Variable, Introduce Constant, Safe Delete | ✅ |
| IntelliJ refactoring: Move, Extract Method, Change Signature; debugger | ⏭ | `docs/internal/plugin-gap.md` |
| Build system / package manager (`jux.toml`), multi-module workspaces, **path + git deps (GitHub URLs, `jux update`), `--target` cross-compile** | ✅ | §B.2.2; registry deps + `jux.lock` remain post-v0.1 |
| Testing framework (`@Test` + hooks, `jux.std.testing` assertions, `jux test [pattern] [--release]`, async tests) | ✅ | `JUX-TESTING-ADDENDUM.md`; runner `test_runner` |

---

## What blocks calling it v0.1

**Nothing remains — every ⛔ row is closed.** Async streams (§18.6) and
the testing framework (§21) — the last two feature blockers — landed
2026-06-12, and the same day closed the async-edge trio (S16–S18, now
clean E0705/E0706 diagnostics + the Worker async-lambda lowering), typed
constructor overloads (S19), and the entire observable-property
follow-up series (P1–P7). The O-series is fully closed and
the borrow machinery survived a 15-probe adversarial wave with every finding
fixed (2026-06-12) — the **inferred borrow checker is release-grade for the
common feature set**: no known rustc borrow-error leaks, RefCell panics, or
silent-wrong lowerings on valid input.

> **Correction (2026-09-08).** That claim had counterexamples, found by
> differential testing against Java rather than by the probe waves. The
> sharpest was a RefCell panic on `a[j] = a[j + 1]` -- an in-place swap, the
> most ordinary thing an array is used for. Alongside it: three silent-wrong
> lowerings (a whole `double` printing as an `int`, a `String` inside a
> generic printing with quotes, `s?.length()` rendering `Some(4)`), and a
> handful of rustc leaks on ordinary programs. All are fixed and gated; the
> lesson kept is about the METHOD, not the count -- an expectation somebody
> wrote down cannot catch a bug that somebody wrote down wrong, which is why
> `tools/java-differential/` uses a second language as the oracle. Jux libraries are consumable straight
from GitHub (`"com.x.lib" = "https://github.com/u/repo"`), and `jux build
--target <triple>` cross-compiles to any installed rustc target. Value
semantics (equality/ordering/hashing/formatting) and the `Iterable` contract
are done via the operator-based design — the roadmap's old interface-based
§1.1/§19.1 plan was superseded by `JUX-CORE-LIB-ADDENDUM` and is not a gap.
Everything else is either done or an explicit post-v0.1 deferral.

> **Status refresh (2026-09-15).** Several rows above had fallen behind the
> code and were corrected against the `refine` branch:
>
> - The diagnostic catalog holds 123 codes (`crates/juxc-diagnostics/src/code.rs`),
>   not 63.
> - Multi-file Rust output exists for binary workspaces: each packaged Jux
>   unit becomes its own `.rs` file under its package directory. Library and
>   test builds still emit a single file.
> - HTTP is no longer waiting on metaprogramming. `examples/http_server` serves
>   requests through the `tiny_http` crate and finds its handlers through the
>   annotation registry.
> - The IntelliJ plugin ships a test-runner UI and five refactorings (Rename,
>   Inline Variable, Introduce Variable, Introduce Constant, Safe Delete).
>   Move, Extract Method and Change Signature are still open, as is a debugger.
> - `juxc-lsp` is no longer the single source of truth for the editor. The
>   plugin runs a hybrid engine: its own PSI parser and indexes answer
>   completion, go-to, find usages, rename and parameter info, and the language
>   server answers diagnostics, hover and code actions, which need the real
>   type checker. The division is spelled out in `JuxLspDescriptor`
>   (`ide/intellij-plugin/src/main/kotlin/dev/jux/intellij/lsp/JuxLspServerSupportProvider.kt`).
