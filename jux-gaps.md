# Jux — Gap Analysis (fresh scan)

**Author:** Claude (Opus 4.8) — adversarial scan, not a re-list of prior ledger.
**Date:** 2026-06-11 · **Branch:** `polymorphism`

## Resolution status (updated 2026-06-11)

| ID | Status | Commit / note |
|----|--------|---------------|
| N1 | ✅ Fixed | Mutating collection method on a wrapped field → `borrow_mut()` + arg-hoist. Runner `wrapped_collection_mutation`. |
| N2 | ✅ Fixed | Generic invariance enforced (mutual-compat on same-name args); covariant upcast now E0410, not a leaked E0308. Runner `generic_invariance`. |
| N3 | ✅ Fixed | Return-completeness pass → E0460 (`return_check.rs`). Runner `return_paths`. |
| N4 | ✅ Fixed | Call-position turbofish in the generic-iface forwarding shim. Runner `generic_iface_nullable`. |
| N5 | ◐ Partial | The **packaged** base-class dispatch half was a distinct bug in `walk_extends_reaches` (FQN vs bare) — **fixed**, runner `poly_base_packaged`. The **generic** base half (a `Container<T>`-typed var holding a `Box<T>`, virtual dispatch + the `T: Display` bound on an inherited generic-returning method) remains a **Phase-1 limitation** — see below. |
| N6 | ✅ Fixed | Nullable generic field from a nullable ctor param no longer double-`Some`. Runner `generic_iface_nullable`. |
| N7 | ✅ Fixed | `?.` safe-call routes through the stdlib-method mapping. Runner `generic_iface_nullable`. |

**N5 generic-base limitation (deferred, documented).** Making a *generic* class
(`Container<T>`) a polymorphic base needs generic `Kind` traits
(`ContainerKind<T>`) and generic trait objects (`Rc<dyn ContainerKind<isize>>`)
threaded through trait decls, impls, downcast hooks, and the upcast cast — plus a
cross-class, inherited-method return-type analysis to add `T: Display` when an
inherited generic-returning method (`this.get()`) is formatted. That's a feature,
not a bug-fix; it's the open-hierarchy + generics intersection and carries real
risk to the working monomorphic/sealed dispatch. Use a sealed hierarchy or a
non-generic base for polymorphism through a base *class* in Phase 1; generic
dispatch through an **interface** (`Container<int> c = new Box<int>(…)` where
`Container` is an `interface`) is the supported route.

## How this was produced

Three parallel adversarial passes — codegen/borrow soundness, type-checker holes,
and feature-combination miscompiles — each **writing real `.jux` probes and
compiling/running them** against the committed `polymorphism` binary
(`target/release/jux.exe`), not reasoning from docs. Every finding has a probe file
(left in `examples/`, untracked) and a `file:line` culprit. The two type-checker
holes (N2, N3) I additionally re-confirmed by reading the cited source myself.

> **Note (resolved).** When this scan was written the tree had uncommitted WIP for
> the `out` parameter feature that left it non-building; that work has since landed
> (§M.4) and the tree builds clean. All fixes above were applied and verified
> against a fresh build (suite 907/0). The original findings below are kept for
> the record — see the resolution table for current status.

---

## New findings

### N1. Mutating stdlib collection methods on a wrapped-class field — wrong mutability + guard held across re-entrant arg  ⚠️
**Severity:** High (miscompile **and** runtime panic) · **Confirmed**

The receiver-hoist fix (`callee_receiver_reads_through_borrow`, `exprs/call.rs:1293`)
only fires for **user-method** calls through a wrapper borrow. The **stdlib
mutating-collection** path (`add`/`put`/`set`/`remove`/`clear`/`insert`/…) never
hoists and never upgrades to `borrow_mut()`. For a wrapped (aliased+mutated) class:

```jux
public class Bag {
    public ArrayList<int> items;
    public int counter;
    public int next() { this.counter = this.counter + 1; return this.counter; }
    public void fill() { this.items.add(this.next()); }
}
```
emits `self.0.borrow().items.push(self.next());` — two defects in one line:
- **A (mutability):** field read through immutable `borrow()` but `.push()` needs
  `&mut` → **E0596** at compile time. Fires even without re-entrancy.
- **B (guard across call):** once A is patched to `borrow_mut()`, the guard stays
  alive while the arg `self.next()` re-enters the same cell → runtime
  `RefCell already mutably borrowed` panic.

**Culprits:** `exprs/call.rs` `emit_array_stdlib_method` (~`:2014-2029`),
`emit_map_stdlib_method` (~`:1912`), `emit_set_stdlib_method` (~`:1972`),
`emit_deque_stdlib_method` (~`:1832`) — all emit the receiver via plain
`emit_expr`, which `exprs/field.rs:449-460` always lowers to `.0.borrow()`. The
mutating-method name set already exists (`analysis.rs:591-598`) but isn't consulted
on this path. **Repro:** `examples/probe_borrow7.jux` (A+B), `probe_borrow8.jux` (A only).

### N2. Generic invariance not enforced — covariant upcast admitted (unsound)  ⚠️
**Severity:** High (silent unsoundness) · **Confirmed (read myself)**

`Box<Dog> dogs; Box<Animal> animals = dogs;` (with `Dog extends Animal`) compiles
with **0 diagnostics**. The spec is explicit that generics are **invariant**
(JUX-LANG-V1 §7.8, JUX-INHERITANCE-BORROW §6.9.6: "Pass `List<Dog>` where
`List<Animal>` expected → Rejected (invariance)"). This lets you `set()` a non-`Dog`
into a `Dog` box — classic covariance hole.

**Culprit:** `check.rs:5899-5902` — for same-name `User` types, `compatible` recurses
on type args with `compatible(x, y)`, and `compatible` itself permits subclass
upcasting (`is_subtype`, line 5907). So `compatible(Animal, Dog)` → true, making
`Box<Dog>` ⊑ `Box<Animal>`. Same covariant pattern in `ty.rs:1223-1226` (`is_subtype`).
Same-name args must require **invariant equality**, not the upcast-permitting recursion.
(Also note `check.rs:5893-5895` returns `true` when either side's arg list is empty —
separate raw-type leniency worth a look.) **Repro:** `examples/probe_variance.jux`.

### N3. No missing-return / return-completeness analysis
**Severity:** Medium (rustc backstops it, but violates the "juxc catches its own
errors" initiative) · **Confirmed (read myself)**

A non-void function that falls off the end on some path (`int classify(int x){ if (x>0) return 1; }`)
compiles with **0 juxc diagnostics**. There is no return-path pass anywhere in
`juxc-tycheck/src` (grep for return-completeness → only an unrelated test name). rustc
eventually rejects the emitted Rust, but per the diagnostics initiative juxc should
catch it itself with a clean span, not leak a rustc error on generated code. Belongs
alongside `definite_assign.rs`. **Repro:** `examples/probe_return.jux`.

### N4. Generic class implementing a generic interface — missing turbofish in forwarding shim
**Severity:** High (miscompile on a core feature) · **Confirmed**

A generic class implementing a generic interface emits an inherent-forwarding shim
as `Box<T>::get(self)` / `Box<T>::put(self, v)` — in call position Rust reads `<`/`>`
as comparison ("comparison operators cannot be chained"). Must be `Box::<T>::get(self)`.

**Culprit:** `decls/classes.rs:2351-2353` emits `class_name` +
`emit_generic_params_as_args` (`<T>`, `types.rs:654`, correct only in *type* position)
+ `::method`, missing the leading `::` turbofish for value/call position.
**Repro:** `examples/probe_combo1.jux`.

### N5. Generic subclass → generic base-typed variable: no upcast coercion + missing `Display` bound
**Severity:** High (miscompile) · **Confirmed**

Two defects exercised by `Container<int> b = new Box<int>(7);` (`Box<T> extends Container<T>`):
- **Upcast not coerced:** lowers to `let b: Container<isize> = Box::<isize>::new(7);`
  with no upcast → `expected Container<isize>, found Box<isize>`.
- **Missing bound:** `"box of " + this.get()` (concat with generic `T`) lowers to
  `format!("box of {}", self.get())` without adding a `T: Display` bound → E0277.

**Culprit:** base-typed-variable generic-subclass coercion in the let/assignment
lowering (main-unit emission); `+`-concat-with-generic bound synthesis. **Repro:**
`examples/probe_combo2.jux`.

### N6. Double-`Some` on a nullable generic field initialized from a nullable ctor param
**Severity:** High (miscompile) · **Confirmed**

`class Node<T> { T? data; Node(T? d){ this.data = d; } }` emits `Self { data: Some(d) }`
where `d` is already `Option<T>` → `expected T, found Option<T>`. The **simple-ctor**
path never seeds `self.nullable_locals` from the ctor's `T?` params, so
`expression_is_already_nullable` (`constructors.rs:385`) returns false and
`emit_ctor_field_init` (`:387`) wraps again. The `__self`-builder fallback path seeds
it correctly (`constructors.rs:324-329`) — the shortcut path is missing that step.
**Culprit:** `decls/constructors.rs:257` (`emit_simple_ctor_body`). **Repro:**
`examples/probe_combo3a.jux`.

### N7. `?.` safe-call bypasses String stdlib method mapping
**Severity:** Medium (miscompile on a common pattern) · **Confirmed**

`s?.length()` (where `s` is `String?`) lowers to `.map(|__t| __t.length())` — the raw
Jux method name instead of routing through `emit_string_stdlib_method` (`call.rs:2422`,
which maps `length` → `.chars().count() as isize`). Result: `no method named 'length'
for &String`. The non-optional `s.length()` works, so it's the optional-chain lowering
that skips stdlib remapping. **Culprit:** the `?.` lowering doesn't reuse the
stdlib-method dispatch. **Repro:** `examples/probe_combo3b.jux`.

---

## Summary

| ID | Issue                                                            | Severity | Kind            |
|----|------------------------------------------------------------------|----------|-----------------|
| N1 | Mutating collection method on wrapped field: `borrow()` not `borrow_mut()` + guard across re-entrant arg | High | miscompile + panic |
| N2 | Generic invariance not enforced (covariant upcast admitted)      | High     | unsoundness     |
| N4 | Generic class impl generic interface: missing `::<T>` turbofish  | High     | miscompile      |
| N5 | Generic subclass→base var: no upcast coerce + missing Display bound | High   | miscompile      |
| N6 | Double-`Some` on nullable generic field from nullable ctor param | High     | miscompile      |
| N7 | `?.` safe-call skips String stdlib method mapping                | Medium   | miscompile      |
| N3 | No missing-return analysis (leaks to rustc)                      | Medium   | missing check   |

**Themes:** the sharp edges cluster around **generics interacting with other
features** — generic + interface (N4), generic + inheritance/upcast (N5), generic +
nullable (N6), and generic *variance* (N2) — plus the **`Rc<RefCell>` borrow
discipline not yet extended to the stdlib-collection mutation path** (N1, same class
of bug the receiver-hoist fix closed for user methods). N2 is the one true silent
unsoundness; N1 is the highest-impact because it hits ordinary "add to a list field"
code. The rest are honest rustc-caught miscompiles, not silent-wrong.

**Probe files** (untracked, in `examples/`): `probe_borrow7.jux`, `probe_borrow8.jux`,
`probe_variance.jux`, `probe_return.jux`, `probe_combo1.jux`, `probe_combo2.jux`,
`probe_combo3a.jux`, `probe_combo3b.jux`. Keep as regression seeds or delete once fixed.

---

## Bug-hunt wave 2 (2026-06-11, production hardening)

A second adversarial pass (three parallel agents over feature *intersections*)
found a fresh batch. The probe corpus was also folded into the permanent suite
(the `probe_*.jux` scratch files above were promoted to named `examples/` +
`bin/jux/tests/` runners, or deleted as duplicates). Suite now 936/0.

### Fixed (committed, each with an example + runner)

| ID | Issue | Severity | Status |
|----|-------|----------|--------|
| H1 | `<field>!!.method()` held the field-read borrow across a re-entrant call → runtime `RefCell already borrowed` | High | ✅ receiver-hoist looks through `!!` |
| H2 | Nullable generic field `T?` double-`Some`d on the wrapped builder path → E0308 | High | ✅ seed nullable params in wrapper builder |
| H3 | `new Base()` into its own polymorphic-base slot not coerced → E0308 | High | ✅ wrap construction precisely |
| H4 | Generic base CLASS used polymorphically leaked rustc E0277/E0308 | High | ✅ clean **E0454** diagnostic |
| H5 | `?.field` on a wrapped class read a tuple slot; chained `?.` (2+) didn't flatten → E0609/E0599 | High | ✅ `.0.borrow()` + `.and_then`, structural receiver resolver |
| H6 | for-each over an own collection field held the borrow across the body → re-entrancy panic | High | ✅ snapshot iterable before loop |
| H7 | `=>` identity type-test emitted a non-existent `__jux_as_<Self>` hook → E0599 | Medium | ✅ identity test is always-true |
| H8 | Concrete subclass not upcast to its base in arg position (exception causes) → E0308; overloaded-ctor arg coercion used wrong overload | High | ✅ `IntoBase` coercion + arity-resolved ctor |
| H9 | `operator()` on a wrapped class emitted `&mut self` → E0596 | Medium | ✅ `&self` on wrappers |

### O-series — ALL FIXED (2026-06-12 wave)

| ID | Issue | Severity | Status |
|----|-------|----------|--------|
| O1 | `try`/`catch`/`finally` inside a value-producing lambda left the closure with an implicit `()` tail → E0308 | High | ✅ `unreachable!()` after a tail try threading a valued return |
| O2 | `break`/`continue` inside a `try` *body* in a loop → E0267 (the body is a `catch_unwind` closure); in a *catch arm* → E0695 (`'__jux_catch` labeled block) | Medium | ✅ `__jux_loopctl` flag threading, dispatch after `finally` (Java ordering); sync shape only — async `move` blocks still can't thread |
| O3 | A generic exception class (`class E<T> extends Exception`) emits `T` out of scope + spurious `Clone`/`Debug` bounds | Medium | ✅ generic params propagated into `From`/`Deref`/`DerefMut` impl headers |
| O4 | Compound index-assign `g[i] += v` doesn't hoist the value arg → E0499 (operator[]= receiver + value both borrow `g`) | Medium | ✅ value hoisted before the `__op_index_set` call |
| O5 | Block-bodied lambda as a call argument `f(() -> { … })`; direct function-typed field call `obj.field(args)` → E0413 | Medium | ✅ fn-field dispatch `(field)(args)` + `Worker.spawn` tycheck gate + fn-field `Debug` stub + `Task::join` |
| O6 | Reading a `String` field on a by-value `Exception` parameter moves it → E0507 (field-read clone not applied for built-in Exception fields) | Medium | ✅ clone applied to built-in Exception field reads |
| O7 | Built-in `Exception(message, cause)` stub declares a non-null `cause` where the spec is `Exception?` | Low | ✅ stub signature corrected to `Exception?` |

### Open (lower-frequency; tracked for a follow-up wave)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| O8 | ~~A raw Rust panic (e.g. integer divide-by-zero) carries a `&str` payload — `catch (Exception e)` / `catch (ArithmeticException e)` can't downcast it~~ | Medium | ✅ **Fixed (2026-06-12).** Integer `/` and `%` route through checked prelude helpers (`__jux_idiv`/`__jux_irem`) that throw a real `ArithmeticException("/ by zero")` — caught by the existing typed dispatch (exact + subclass + base arms). Compound `/=`/`%=` desugar to the same path; literal `1/0` no longer trips rustc's `unconditional_panic`; stepped-range zero step throws too. Spec: ERRATA E1 row + §X.8 updated (Java-parity carve-out). BONUS: uncaught typed exceptions now print Java-style `Exception in thread "main" <fqn>: <message>` via a `catch_unwind` reporter around the renamed entry point (previously: silent exit 101). Example `arith_exception.jux` + `arith_uncaught.jux`, runner `arith_exception.rs`. |
| O9 | ~~`break`/`continue` inside an **async** `try` body still → E0267~~ | Low | ✅ **Fixed (2026-06-12).** Async tries get an `Arc<AtomicU8>` loop-control channel (`Arc` not `Rc<Cell>` — spawned futures must stay `Send`): a `_body` clone moves into the `async move` block (`.store(code, Relaxed); return`), catch arms and the post-`finally` dispatch use the original handle. Nesting composes channel-by-channel with O2's sync `u8` shape. Example + runner `async_try_loopctl`. Side-findings S16/S17/S18 logged in wave 3. |

## Bug-hunt wave 3 (2026-06-12, borrow-checker release hardening)

15 adversarial borrow-stress probes (`probes/stress_NN_*.jux`) targeting
untested intersections of the borrow machinery: observers × properties,
properties × collections, closure nesting, hoist-detector reach, async ×
wrappers, operators × self-reference, statics × self-reach, fluent chains,
getter-returned iterables. **12 findings / 3 passes** (#2 observer→sibling
field, #6 nested lambda capture, #14 fluent chain — promoted candidates).

Classification: (A) rustc-leaked compile error · (B) runtime RefCell panic ·
(C) silent wrong output.

| ID | Probe | Class | Finding | Status |
|----|-------|-------|---------|--------|
| S1 | `stress_01` | C | Observer that sets its OWN observed property: nested set applies but does NOT re-fire the observer list (prints 2, JavaFX chaining expects 5). Spec was silent — §P needed a re-entrant-set ruling first. | ✅ fixed — §P.3.6 specced (JavaFX transition chaining); setter quiescence loop fires each transition once |
| S3 | `stress_03` | A | Collection-typed auto-property: `s.Items.add(3)` → getter-call receiver skips the stdlib method mapping → E0599 `no method add on Vec`; reference semantics of the returned collection also unspecified in lowering. | ✅ fixed — auto-property collection receivers rewrite to the live backing field (`__prop_X`); declared-type fallback + `ArrayList` normalization in the stdlib dispatch |
| S4 | `stress_04` | A | Compound assign to a property (`m.Count += 1`) bypasses the setter and writes a nonexistent raw field → E0609 (`available: __prop_Count`); observer never fires either. | ✅ fixed — compound property assigns desugar to get-op-set (read via getter, write via setter, observers fire) |
| S5 | `stress_05` | B | `items.forEach(x -> …)` where the lambda mutates the SAME object: borrow held across the higher-order call → runtime `RefCell already borrowed`. | ✅ fixed — forEach/map/filter snapshot wrapper-borrowed receivers (H6 extended to higher-order stdlib calls) |
| S7 | `stress_07` | C | Arg-hoist detector misses FIELD-PATH receivers: `h.item.set(h.item.bump() + h.item.bump())` silently loses every mutation (prints 0; clones mutated then dropped). | ✅ fixed — receiver-place reads skip the auto-clone (`emitting_method_receiver`), mutation analysis promotes field-path receiver roots, arg-hoist generalizes to dotted place paths |
| S8 | `stress_08` | A | Catch binder stored into a nullable BASE-typed field (`Exception? last = e` where `e: RuntimeException`) missing subclass→base upcast inside the `Some(…)` wrap → E0308. | ✅ fixed — assigns route ANY user-class LHS through the shared coercion (IntoBase `.into()` + place clone) |
| S9 | `stress_09` | A | Value-producing lambda: `return v;` MID-BODY inside a `try` (under an `if`) breaks the return-threading shape → E0308 + unreachable tail (O1 fixed only the tail-try shape). | ✅ fixed — lambda-body tries emit inference-typed return channels (`in_lambda_body`) |
| S10 | `stress_10` | A | Async fn: wrapped receiver used across an `await` inside a `try` — the `async move` block moves the only Rc clone in; use after the try → E0382. | ✅ fixed — async tries share-clone wrapper captures before the `async move` block (lambda-capture rule) |
| S11 | `stress_11`/`11b` | A | PARSER: statement-position `super.method(args);` rejected ("expected '(' after super") — the statement parser commits to a super-CTOR call on seeing `super`. Expression position works. Any Java-style override delegating to super as a statement fails. | ✅ fixed — statement parser peeks for `super.` and routes to the expression path |
| S12 | `stress_12` | A | Plain (non-compound) `operator[]=` with self-referential RHS: `g[1] = g[0] + g.total()` → E0499 (O4 hoisted compound forms only). | ✅ fixed — plain index-assign hoists non-trivial RHS into `__jux_tmp` |
| S13 | `stress_13` | A | Nullable thread_local static: `Reg.global = new Reg()` (slot `Reg?`) missing the `Some(…)` wrap in the `.with(…)` write → E0308. | ✅ fixed — nullable-aware static slot writes (both qualified and bare-name forms) |
| S15 | `stress_15` | A | Getter returning an own collection field (`return this.items`) on a wrapped class emits a MOVE out of the shared borrow → E0507. | ✅ fixed — collection/array field reads clone in value positions; receiver/index positions exempted via the place marker |
| S16 | (O9 work) | A | ~~Un-awaited async call leaks rustc E0277~~ | ✅ **Fixed (2026-06-12).** `E0705` fires at free-fn/instance/static resolution sites whenever an `async` fn's result lands in a non-`await` slot (`in_future_slot` carve-out for `spawn`/`Task.*`/`Worker.spawn` args). Probe `probes/probe_s16_s18.jux`; e2e `bin/jux/tests/async_edges.rs`. |
| S17 | (O9 work) | A | ~~`Worker.spawn(async () -> …)` → rustc E0728~~ | ✅ **Fixed (2026-06-12).** `emit_bare_move_lambda` wraps an async lambda body in `futures::executor::block_on(async move { … })` so the worker thread drives the awaits. Example `examples/async_edges.jux`. |
| S18 | (O9 work) | C | ~~Async-`try` outer-local mutation silently lost~~ | ✅ **Fixed (2026-06-12).** `E0706` rejects assignments to outer primitive locals inside an `await`-bearing `try` body (`block_has_await_shallow` + `collect_async_try_writes`). Probe `probes/probe_s16_s18.jux`. |
| S19 | (overloading) | — | ~~Ctor overloads select by argument count only~~ | ✅ **Fixed (2026-06-12).** `select_ctor_typed` scores same-arity ctors like methods (2 exact / 1 compatible); picks recorded in `ctor_selections`, consumed by `ctor_overload_suffix_for_span`. `E0450` narrowed to identical-shape ambiguity. Example `examples/async_edges.jux` (typed `Point` ctors). |

### §P observable properties — follow-ups (core landed 46834eb/4391bc4)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| P1 | ~~Computed (get-only) properties don't fire observers~~ | Medium | ✅ **Fixed (2026-06-12).** `computed_prop_deps` walks the getter body for settable-prop reads; each dependency's setter pre-captures the computed value and fires `__obs_<C>_fire` on change (change-gated for comparable types). Probe `probes/probe_p1_computed.jux`; e2e `bin/jux/tests/observable_props.rs`. |
| P2 | ~~E0973 (assign to bound property) not enforced~~ | Low | ✅ **Fixed (2026-06-12).** Real setter emits as `__set_X_raw`; public `__set_X` gate throws `IllegalStateException` (debug builds) when a ONE-WAY binding drives the property — bidirectional targets stay settable (JavaFX semantics, bidi flag in the `__bind_X` slot). |
| P3 | ~~E0974 (bind type mismatch) surfaces as a rustc error~~ | Low | ✅ **Fixed (2026-06-12).** `check_call` resolves both ends of `bind`/`bindBidirectional` as properties and fires `E0974` with `Class.Prop (type)` labels on mismatch. Probe `probes/probe_p3_bind_mismatch.jux`. |
| P4 | ~~`unbind()` after `bindBidirectional` leaves the reverse direction live~~ | Low | ✅ **Fixed (2026-06-12).** Both directions' closures share one `Rc<Cell<bool>>` kill token stored in each `__bind_X` slot; either side's unbind/rebind deactivates both. |
| P5 | Shape-2 observers: ~~adapter never pruned after owner death~~ property NAME still passed as `String` (real property-handle type deferred) | Low | ✅ **Adapter leak fixed (2026-06-12).** `JuxObserver::StrongGuarded` self-reports death via a shared `Cell` flag on failed inner upgrade; the fire loop's retain prunes it. |
| P6 | ~~Ctor `bind()` on `this` → `compile_error!`~~ | Medium | ✅ **Fixed (2026-06-12).** `emit_bind` records ctor-inner binds in `pending_ctor_binds`; the public `new` replays them right after the `Rc<RefCell>` wrap (params cloned into `new_inner` so source receivers stay in scope). Probe `probes/probe_p6_ctor_bind.jux`. |
| P7 | ~~`static` properties have no observer infrastructure~~ | Low | ✅ **Fixed (2026-06-12).** `thread_local!` observer vectors beside the `LazyLock<Mutex>` value backing (observers are `Rc` → `!Send`; same-thread firing only) + associated-fn helpers + static setter fire bracket. Sealed-class statics + static binds remain out of scope. Probe `probes/probe_p7_static.jux`. |
| P8 | W0970–W0973 inspections + §P.5/§P.7 IDE work (native coloring, gutter icons, rename quick-fix) not started | Medium | IntelliJ plugin / jux-ls effort, separate from juxc |

### Check-ups verification wave (2026-06-12, `check-ups.md`)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| C1 | ~~RISK-3: argument `Ref` guards outlive into the callee — `n.bump(n.value)` panics `RefCell already borrowed`~~ | A | ✅ **Fixed (2026-06-12).** `call_needs_borrow_hoist` fires for ANY call whose argument reads a wrapper field (`expr_reads_wrapper_field`); receiver-hoist emitter gained arg hoisting for the combined case. Wave `probes/probe_risk3_reentrancy.jux` (9 cases). |
| C2 | ~~`vec[i].field` on wrapper-class elements leaks rustc E0609~~ | A | ✅ **Fixed (2026-06-12).** `infer_index` types builtin-container indexing (Vec/VecDeque → element, HashMap/BTreeMap → value), so the backend's `.0.borrow()` rewrite sees the class. Probe `probes/probe_vec_param.jux`. |
| C3 | ~~Mutated by-value collection param leaks rustc E0596 (missing `mut`)~~ | A | ✅ **Fixed (2026-06-12).** Param-`mut` inference (free fns + methods) reuses `collect_mutated_names`. |
| C4 | ~~Init blocks ran AFTER the constructor body (spec: before — §S.4.4/ERRATA E2)~~ | A | ✅ **Fixed (2026-06-12).** All ctor paths reordered; simple-ctor fast path skipped when init blocks exist; §M.1 prose/example rewritten; `probes/probe_init_order.jux` + rewritten `examples/init_blocks.jux` e2e. Side-fix: `this.<AutoProp>` in init blocks now gets the ctor backing-field rewrite. |
| C5 | ~~E0431 code collision (method-modifiers vs generic-inference)~~ | B | ✅ **Fixed (2026-06-12).** Inference-failure diagnostic renumbered to `E0453` (catalog's reserved §T.4.2 slot); catalog/§T.4.2/collision note updated; `E0446`+`W0240` rows added; `@Derive` annotation-table wording fixed. |
| C6 | Collections pass **by value** — a callee's `push` is invisible to the caller (`probes/probe_vec_param.jux` prints `len after call: 0`) | B | OPEN — unspecced; Java intuition says reference. Decide share-vs-value in the collections spec session (next-session backlog item 2), then implement (likely `&mut` params or container handles). |
| C7 | ~~Reordered named args evaluate in parameter-slot order, not §S.1.4's lexical order~~ | C | ✅ **Fixed (2026-06-13).** `splice_args` records the lexical permutation on `CallExpr.eval_order`/`NewObjectExpr.eval_order`; the backend hoists each (coerced) arg into a temp in lexical order, then passes them positionally — free fns, methods, AND constructors. `examples/named_arg_eval_order.jux` + e2e. Pure structural, no name lists. |
| C8 | Bare property reads (without `this.`) inside `init { }` blocks don't get the implicit-this rewrite (`cannot find value` rustc leak); `this.Prop` works | C | OPEN — the accessor-body bare-name rewrite (desugar) doesn't run over init blocks; low priority, workaround is explicit `this.`. |

### Bug-hunt wave 3 (2026-06-12, `probes/probe_hunt2_mixed.jux` + follow-ons)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| H2-1 | ~~Inherited-property READ in an override (`this.Score` in `Child.describe()`) emitted a raw field access instead of the getter call~~ | A | ✅ **Fixed (2026-06-12).** The property-getter routing consults `symbols.lookup_method` (extends-chain walk) instead of the receiver's own class only. |
| H2-2 | ~~Observable properties were NOT inherited — attach/bind/set on a subclass object's inherited property was unrouted (rustc garbage), inherited setter copies never fired observers, and attach through a base-typed (`dyn`) reference had no dispatch target~~ | A | ✅ **Fixed (2026-06-12) — full Java semantics.** Routing walks the extends chain; subclass wrappers get depth-aware helper sets reaching the ancestor's storage slice through `__parent` hops; inherited `__set_<X>` copies get the fire bracket (+P2 gate with hops); the `Kind` traits carry observer-helper signatures with per-class delegating impls, so base-typed references attach/detach/clear/size too. `examples/inherited_observers.jux` + `inheritance_features` e2e. Binding THROUGH a base-typed reference (not just to inherited props) remains open. |
| H2-3 | ~~`this.slots[i] = v` (indexed store to a collection FIELD of a wrapper class) emitted through an immutable `borrow()` → rustc E0596~~ | A | ✅ **Fixed (2026-06-12).** Dedicated wrapper-field indexed-write arm: value + index hoisted to statement temps, store through `borrow_mut()` with `__parent` hops. |
| H2-4 | ~~`this.slots.push(10)` in a constructor SILENTLY pushed into a dropped clone (collection-field auto-clone fired on a method receiver)~~ | A (silent-wrong) | ✅ **Fixed (2026-06-12).** External-receiver method emission marks the object as a receiver place (no auto-clone) and upgrades to `borrow_mut()` for mutating methods. |
| H2-5 | ~~Receiver mutability of Rust-std methods was hardcoded by NAME (drifts when the library changes)~~ | B | ✅ **Fixed (2026-06-12) — discovery.** bindgen records the real `&mut self` receiver as a `@MutSelf` annotation on every generated `.jux.d` method; the backend reads it from the symbol table (wrapper-field `borrow_mut()` upgrade + `let mut`/param-`mut` promotion). Old name lists remain only as a fallback for pre-marker stub caches. Std stub cache bumped to v8. |
| H2-6 | ~~Regenerated std stubs failed to PARSE (`T*` return types broke the member lookahead; `@MutSelf` rejected in interface bodies) — every compile broke with E0400 cascades~~ | A | ✅ **Fixed (2026-06-12).** `scan_type_at` accepts `*`/dotted-name suffixes; interface members parse annotations like class members. |
| H2-7 | `typeof(expr)` (§5.9.10) — NEW feature: compile-time static-type-name String (`typeof(xs)` → `"Vec<int>"`), operand never evaluated; `typeof` joins the reserved keywords | — | ✅ **Shipped (2026-06-12).** Spec §5.9.10 + grammar; `examples/typeof_query.jux` + e2e. Type-position `typeof` (decltype-style) deferred. |

### Bug-hunt wave 4 + `ref` bindings (2026-06-12)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| H4-1 | ~~`record.with(...)` (§M.5 wither) not implemented~~ | B | ✅ **Fixed (2026-06-13).** Synthesized wither: tycheck validates named args against the record's components (E0448 on bad name / positional); backend emits Rust struct-update copy `Rec { x: v, ..(recv).clone() }` (zero args → `.clone()`). Nested withers work. A user `with` method shadows it. `examples/record_with.jux` + e2e. |
| H4-2 | ~~`map[key]` indexing hardcoded the `(key) as usize` sequence cast — rustc E0308 on HashMap~~ | A | ✅ **Fixed (2026-06-12) — discovery.** bindgen records `Index<&K>` trait impls as the `@RustIndexRef` class annotation; the backend emits `map[&(key)]` for marked containers (name fallback only for pre-marker caches). Std stub cache v9. Probe `probes/probe_hunt4_data.jux` (map-shared-mutation, records, enum guards, tuples, string chains — all green). |
| H4-3 | `ref` bindings (§M.13) — NEW feature: shared references to value types (`ref String a = ...; ref String b = a; b = "x"` → both see it; `ref` params write through) | — | ✅ **Locals + params shipped (2026-06-12).** `Rc<RefCell<T>>` lowering; reads clone out, assigns store through (RHS-temp first), ref-arg aliases / plain-arg wraps+copies; RISK-3 arg-hoist covers ref reads; `ref` is keyword no. 58. `probes/probe_ref_bindings.jux`. |
| H4-4 | ~~`ref` FIELDS (`public ref String x`) lowering~~ | A | ✅ **Fixed (2026-06-12).** Storage wraps to `Rc<RefCell<T>>` (inline + wrapper inner structs), every ctor-literal shape seeds a fresh cell (place inits cloned — a param may feed two fields), reads clone the value out, writes store through (owner read with a SHARED borrow), and a `ref` field passed to a `ref` param shares the HANDLE (`emitting_ref_handle`). `examples/ref_fields.jux` + `ref_bindings` e2e. |
| H4-5 | ~~Bare-name free-fn lookup went ambiguous when a user fn shadows a same-named bindgen stub fn (`rename` vs `rust.std.rename`)~~ | B | ✅ **Fixed (2026-06-12).** Symbol lookup prefers the candidate without a `@rust` path (user code shadows the foreign surface — discovered property, not a name list). |

### Diagnostics polish (2026-06-13)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| D1 | ~~`obj.observers` (object-level, missing the property) gave a bare "no field `observers`" message~~ | — | ✅ **Improved (2026-06-13).** When a class has observable properties, E0412 on `.observers` now says "`.observers` is a member of an observable PROPERTY ... write `<value>.<Prop>.observers` (§P.3)". Found via user test project. |

### `++`/`--` operators + user-found issues (2026-06-13)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| U1 | ~~`++`/`--` increment/decrement operators unimplemented~~ (spec uses `for (i=0; i<n; i++)` throughout but the lexer/parser never had them) | A | ✅ **Fixed (2026-06-13).** Lexer emits `PlusPlus`/`MinusMinus`; parser desugars prefix `++x`/`--x` and postfix `x++`/`x--` to `x += 1`/`x -= 1` in STATEMENT and for-update position (lvalue = name/index/field; E0200 otherwise). Expression-position value semantics deferred. Grammar §A `incdec`. `examples/increment_decrement.jux`. Found via user test project. |

### Java-parity gaps found by empirical probing (2026-06-13)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| J1 | ~~Braceless control-flow bodies rejected~~ — grammar spells `if`/`while`/`for` bodies `statement` but the parser forced `block` | A | ✅ **Fixed (2026-06-13).** `parse_block_or_stmt()` accepts a brace block OR a single braceless statement (wrapped in a synthetic 1-stmt Block); wired into `if`/`else`/`while`/`do-while`/`for-c`/`for-each`. `try`/`catch`/`finally` keep requiring braces (Java too). `examples/braceless_control_flow.jux`. |
| J2 | ~~`int[] a = new int[N]` rejected (fixed `new int[N]` not assignable to dynamic `int[]`)~~ — and `new int[runtimeN]` emitted an invalid Rust `[v; N]` | A | ✅ **Fixed (2026-06-13).** `compatible` allows fixed→dynamic array (§5.6 "interchangeable", reverse needs a check); backend emits `vec![v; N]` for dynamic slots / runtime sizes (`dynamic_array_target` flag), `[v; N]` for fixed/const slots; repeat-length casts runtime `isize`→`usize`. `examples/dynamic_arrays.jux`. |
| J3 | `++`/`--` operators were unimplemented (see U1) — all four forms `++x`/`x++`/`--x`/`x--` now work | A | ✅ done (U1). |
| J4 | ~~Unresolved type name in a method signature (`void test(T t)` where `T` is out of scope) PASSES tycheck and leaks to rustc (E0412)~~ | B | ✅ **Done (2026-06-13).** New `E0417_UnknownType`. `validate_sig_type`/`sig_head_unresolved` in `check.rs` walk every param/return/field type; a single-segment bare name that resolves to nothing (probed via the real `ty_from_ref`) fires E0417 with a hint to name the bound type argument. Wired into check_method / check_function (passes fn generics) / check_constructor / check_class. Ruling: Jux follows Java — `implements Holder<Object>` requires the override to be `test(Object t)`, NOT `test(T t)` (an earlier lenient substitution feature was built then **reverted** per the user). `examples/unknown_type_in_override.jux` + `bin/jux/tests/unknown_type_in_override.rs`. |
| J5 | 2D/multi-dim array TYPE syntax `int[][]` doesn't parse (`new int[3][3]` parses but the type annotation doesn't) | B | OPEN — parser's array-shape only handles one dimension. |
| — | E0424 false positive on `implements Holder<Object>` is an INTELLIJ PLUGIN bug (`JuxImplementsClauseInspection.kt` resolves the generic ARG `Object` as an implemented type) — juxc itself is correct. For the plugin AI. | — | not juxc |

### Name-resolution + operator-token follow-ups (2026-06-13)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| N1 | ~~A user free function named `f` collided with a same-named LOCAL/lambda parameter inside std code (`assertThrows(() -> void f)`) — `f()` resolved to the global, giving a bogus E0411 arg-count error~~ | A | ✅ **Fixed (2026-06-13).** `check_call` now lets an in-scope local/parameter SHADOW a same-named top-level function (`shadowed_by_local` guard). Std `assertThrows` param also renamed `f`→`action` as defense-in-depth. Any function name now safe regardless of std internals. |
| N2 | `--x` was two unary minuses (double negation); now lexes as the DECREMENT token (greedy, like Java/C) | — | Expected with the `++`/`--` work; double-negation is now written `- -x` / `-(-x)`. Parse test updated. |
| N3 | Expression-position `++`/`--` (`print(x++)`, `arr[i++]` as a value) not supported — only statement + for-update positions | C | OPEN — value-returning pre/post inc-dec deferred; the common loop forms work. |
