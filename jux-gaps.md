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
