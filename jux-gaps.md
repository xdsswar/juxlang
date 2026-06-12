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
| S16 | (O9 work) | A | Calling an async fn WITHOUT `await` from a sync lambda (`Worker.spawn(() -> { return asyncFn(); })`) leaks rustc E0277 (`Display` not impl for `impl Future`) — tycheck should reject the un-awaited async call with a clean diagnostic. | ⛔ open |
| S17 | (O9 work) | A | `Worker.spawn(async () -> …)` emits a plain `move \|\|` closure (no async block) → E0728 `await` outside async. Free `spawn(async () -> …)` works; the Worker path misses the async-lambda lowering. | ⛔ open |
| S18 | (O9 work) | C | An async `try` body that mutates an OUTER primitive local (`total = total + await …`) silently loses the mutation — the `async move` block captures by value and writes the copy. Worse than S10's E0382 (no error at all). Needs a tycheck error or a threading shape. | ⛔ open |

### §P observable properties — follow-ups (core landed 46834eb/4391bc4)

| ID | Issue | Severity | Notes |
|----|-------|----------|-------|
| P1 | Computed (get-only) properties don't fire observers — §P.1.5 dependency tracking not implemented; they work as plain getters | Medium | needs static dep extraction from the getter body + re-fire hooks in each dependency's setter |
| P2 | E0973 (direct assignment to a bound property) not enforced — neither compile-time nor the debug-build runtime throw | Low | setter would need a `__bind_X.is_some()` gate with an internal raw-set path for the binding itself |
| P3 | E0974 (bind type mismatch) surfaces as a rustc error in the emitted code, not a juxc diagnostic | Low | tycheck check on the two resolved property types at `bind`/`bindBidirectional` sites |
| P4 | `unbind()` after `bindBidirectional` breaks only the caller's incoming direction — the reverse direction stays until the OTHER side unbinds | Low | keep-alive slot would need to record the peer's closure too |
| P5 | Shape-2 (3-arg, property-reference) observers: Phase 1 passes the property NAME as a `String`; the Strong adapter closure is never pruned after the owner dies (stops firing, tiny leak) | Low | real property-handle type + adapter liveness tied to the wrapped observer |
| P6 | `bind()` on a property of `this` inside a constructor → clean `compile_error!` (no wrapper handle exists yet); §P.9's ctor-bind shape needs a post-construction hook | Medium | move binding setup to first-use or an implicit post-ctor init |
| P7 | `static` properties have no observer infrastructure (storage/firing skipped) | Low | needs statics-shaped storage (`LazyLock<Mutex<…>>`) |
| P8 | W0970–W0973 inspections + §P.5/§P.7 IDE work (native coloring, gutter icons, rename quick-fix) not started | Medium | IntelliJ plugin / jux-ls effort, separate from juxc |
