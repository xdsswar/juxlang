# Jux Spec Addendum — Class Representation (Draft)

**Status:** Proposed insertion. Locks the design for **how classes are
represented in lowered code** so the backend stops calcifying any
single choice (currently a tacit "every class is `Arc<C_Inner>`")
before more features land on top. The user-visible class surface
doesn't change — this addendum is entirely about lowering strategy
and what the compiler may choose without asking.

**Companion docs:**

- `JUX-LANG-V1.md` §5.2, §7 — class semantics from the user's
  perspective (heap-allocated reference type, identity).
- `JUX-INHERITANCE-BORROW-ADDENDUM.md` — borrow rules for class
  hierarchies; the representation selector must respect those.
- `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.9 — backend lowering
  pipeline; this addendum slots in as new sub-section §C.9.3.1.

**Insertion points:**

- New §CR.1 ("Design Philosophy")
- New §CR.2 ("The Four Representations")
- New §CR.3 ("Selector Algorithm")
- New §CR.4 ("User-Visible Semantics")
- New §CR.5 ("Interaction with Other Features")
- New §CR.6 ("Rust Lowering Rules")
- New §CR.7 ("Diagnostics")
- New §CR.8 ("Worked Examples")
- New §CR.9 ("Implementation Phasing")
- New §CR.10 ("Supersedes")

---

## §CR.1 — Design Philosophy

Three principles drive every decision in this addendum:

1. **The user writes one `class`.** They don't pick between value /
   boxed / refcounted / atomic — those are implementation details. The
   compiler picks the cheapest representation that preserves the
   spec's reference semantics.

2. **No representation annotations.** Java doesn't have them. Kotlin
   doesn't have them. The user shouldn't have to learn `inline class`,
   `data class`, `@Rc`, `@Arc`. The compiler chooses; the user gets
   identity semantics regardless.

3. **Conservative is safe.** When the analysis can't prove a tighter
   representation is sound, fall back to the most general
   (`Arc<C_Inner>`). A missed optimization is invisible to users; an
   incorrect representation breaks the borrow checker or surfaces
   data races.

The result: a Jux program that allocates a class instance and never
escapes it from the function pays **zero heap-allocation cost**. A
class instance shared across threads pays the full `Arc` price. The
user wrote `class C { … }` in both cases.

---

## §CR.2 — The Four Representations

Each Jux class compiles to **exactly one** of the following Rust
representations. Selection is per-class and global across the
program; per-instance variation isn't supported (and isn't useful —
escape analysis happens at the type level).

| Rep                | Rust lowering              | Cost                          | Sound when                                                                                        |
|--------------------|----------------------------|-------------------------------|---------------------------------------------------------------------------------------------------|
| **Inline**         | `struct C { … }`           | Zero. Lives in the stack frame or enclosing struct. | No escape from the introducing scope; no `dyn` dispatch; no weak refs; no `===` comparison.       |
| **Owned heap**     | `Box<C>`                   | One alloc per `new`; no atomics. | The instance escapes its scope (returned, stored in a heap-rooted slot) but is never **aliased**. |
| **Local refcount** | `Rc<C>`                    | Alloc + non-atomic refcount.  | Aliased (multiple live references) but never crosses a thread boundary.                           |
| **Shared refcount**| `Arc<C>`                   | Alloc + atomic refcount.      | Default. Used whenever the analysis can't prove a tighter rep is sound — including when the class crosses threads, is stored in a static, or escapes through a polymorphic interface boundary. |

### §CR.2.1. Inline

The struct lives directly in its owner — a stack frame, an enclosing
field, an array element. No indirection, no allocator call. The Rust
compiler treats the type as `Copy`-eligible if all its fields are
`Copy` (rare for classes, common for tiny coordinate-like types).

The user's mental model still says "this is a class" — equality
defaults to **identity**, which for inline classes means
**address-of-the-stack-slot**. That works because inline classes
never escape: every access happens through a live owning binding, so
the address is meaningful at every point in scope.

**Trade-off:** inline classes can't participate in cycles, can't be
stored in a Map keyed by another instance's identity, and can't be
compared via `===` across function boundaries (the address changes
on move). The selector rejects inline when any of those is observed.

### §CR.2.2. Owned heap (`Box<C>`)

The struct lives on the heap with a unique owner. Moves are a
pointer-sized memcpy; `===` is `Box::ptr_eq` (or
`std::ptr::eq` on `&*box`). One allocation per `new C(...)`; one
deallocation when the owning binding drops.

**Trade-off:** no sharing. Two `Box<C>` values are different
allocations; assigning `let b = a` moves out of `a` (Rust's standard
move semantics). For Jux's "assignment shares" semantics to hold, the
selector only picks `Box` when the user never aliases — which is
rare but happens (think: a class instance built up locally inside a
function and returned exactly once).

### §CR.2.3. Local refcount (`Rc<C>`)

The struct lives on the heap with non-atomic refcount. `Rc::clone` is
cheap (no atomic op). Used when:

- The class IS aliased (assignments share, e.g. `let b = a` doesn't
  move).
- It's NEVER sent across a thread boundary.

Jux's `spawn` / `Task` boundary is the thread boundary the analyzer
watches. Anything that crosses into a `spawn(...)` closure or is
stored in a type the spec marks `Send` becomes `Arc`. Everything else
stays `Rc`.

**Trade-off:** `Rc<T>: !Send`. The selector errs toward `Arc` whenever
thread-crossing is ambiguous.

### §CR.2.4. Shared refcount (`Arc<C>`)

The default — atomic refcount, `Send + Sync` when the inner is. This
is the current Phase-1 backend's tacit choice for every class.

**Trade-off:** atomic ops on every clone/drop. Real but tiny in
practice. The selector picks `Arc` whenever the conservative analysis
can't prove a cheaper rep is sound.

---

## §CR.3 — Selector Algorithm

The selector runs once per compilation unit, after type checking but
before MIR/lowering. It's a fixed-point escape-and-aliasing analysis
on the AST + the symbol table; conservative when in doubt.

### §CR.3.1. Inputs

- The full `SymbolTable` (every class, its fields, its method
  signatures).
- The full AST (every `new C(...)`, every assignment, every closure
  capture).
- The `Send`/`Sync`-bearing type set from the standard library
  (`Task`, `Channel`, `spawn`'s closure, `Mutex`, etc.).

### §CR.3.2. Per-Class Properties Collected

For each class `C`, the selector computes:

| Property            | Meaning                                                                   |
|---------------------|---------------------------------------------------------------------------|
| `escapes`           | An instance is ever returned from a function, stored in a heap-rooted slot (class field, Vec, Map, …), or captured by a closure that outlives its enclosing scope. |
| `aliased`           | Two or more live bindings ever refer to the same instance (`var b = a;` while `a` is still live, or stored in a shared collection). |
| `cross_thread`      | An instance is ever passed across a thread boundary — captured into `spawn(...)` / `move` closures, sent through a `Channel`, stored in a static, or shared via a `Sync` field of another class. |
| `dyn_dispatched`    | A `Box<dyn I>` (or similar `dyn`-typed slot) ever holds the class. Once Phase 2 polymorphism lands, this flips a lot more classes to `Arc`. |
| `cycle_capable`     | Any field of the class has a type that transitively references `C`. Cycles can only form on heap-owned representations. |
| `weak_ref_target`   | The class is the target of a `weak` reference (per §M-WR of the missing-defs addendum, when it lands). |

### §CR.3.3. Decision Table

Given the per-class properties, the selector picks the rep by the
first matching row:

| escapes | aliased | cross_thread | weak_target | cycle_capable | dyn | → Rep |
|---------|---------|--------------|-------------|---------------|-----|-------|
| false   | false   | false        | false       | false         | false | **Inline** |
| true    | false   | false        | false       | false         | false | **Box** |
| `_`     | true    | false        | false       | `_`           | false | **Rc**  |
| `_`     | `_`     | `_`          | `_`         | `_`           | `_`   | **Arc** (default) |

**Notes:**

- "`_`" means "any value" — the row only requires the explicitly named
  columns to match.
- `weak_ref_target` forces `Arc` (Rust's `Weak` works against
  `Arc`/`Rc`; for cross-thread weak refs `Arc` is mandatory).
- `cycle_capable` plus `aliased` could in principle stay `Rc`, but
  cyclic `Rc<T>` leaks unless the user inserts `Weak` manually.
  Phase 1 doesn't auto-break cycles; until that lands, the spec
  prefers `Arc` (uniform behavior) over `Rc` (leaks). When the
  cycle-breaker pass arrives this row relaxes.
- `dyn_dispatched` forces `Arc` because trait objects in Jux are
  always reference-counted (the user never sees `Box<dyn T>`
  ownership semantics).

### §CR.3.4. Conservative Defaults on Generics

A generic class `C<T>` is selected based on the **union** of usages
the program contains. If the program calls `new C<int>()` (no escape)
and `new C<String>()` (escapes to a return), `C` as a whole is
selected at the more general level. Per-instantiation differentiation
is a future optimization; Phase 1 keeps one selection per class.

### §CR.3.5. Inheritance Hierarchies

When `class D extends C`, the selector picks the **least-general
representation that's sound for every class in the chain**. A child
class can never have a "tighter" representation than its parent —
that'd break upcasts (`D` → `C`).

So if `C` is `Rc<C>` and `D` adds a field that escapes, the whole
chain rolls up to `Arc`: `C` becomes `Arc<C>` and `D` becomes
`Arc<D>`. The user sees the same `class D extends C` syntax in both
cases.

### §CR.3.6. Stability Across Recompilation

The selector is **deterministic** given the same compilation unit. A
class's representation can change between releases of the same
program when usage patterns shift — that's expected and intentional.
The user-visible semantics (identity equality, sharing on assignment,
GC-like lifetime) stay constant.

---

## §CR.4 — User-Visible Semantics

Regardless of which representation the selector picks:

| Operation       | Behavior                                                                                  |
|-----------------|-------------------------------------------------------------------------------------------|
| `new C(args)`   | Allocates and constructs an instance. Allocation cost varies by rep (zero for Inline, one allocator call for the others). |
| `var b = a;`    | `b` and `a` refer to the same instance. For Inline, the language pretends they share by enforcing that the second binding can't outlive the first. |
| `a === b`       | Reference identity. For Inline classes, address-of comparison within the same scope. For boxed/Rc/Arc, pointer comparison. |
| `a == b`        | Per `JUX-OPERATORS-ADDENDUM.md` §O.2.6: if `operator==` is defined, dispatch through it; otherwise reference identity (same as `===`). |
| `null`-like sentinel | Per `JUX-LANG-V1.md` §5.3, classes are non-null by default. Nullable variants land via `T?` and lower to `Option<Rep<C>>` (preserving the inner representation). |
| Field reads     | `a.field` — same Jux syntax regardless of rep. Rust lowering varies (`a.field` vs `(*a).field` vs `Arc::deref` autoref) but the user never sees it. |
| Field writes    | `a.field = v;` — same Jux syntax. The receiver-mutation analysis still drives `&mut self` vs `&self` for methods (see `JUX-INHERITANCE-BORROW-ADDENDUM.md`). |

In particular, **`===` works on Inline classes** despite the absence
of indirection: address-of-binding is a stable identity within the
binding's lifetime. The selector forces Box/Rc/Arc if a program tries
to compare two Inline instances across function boundaries (where
addresses would change).

---

## §CR.5 — Interaction with Other Features

### §CR.5.1. Inheritance

Per §CR.3.5 the whole chain rolls up to the least-general
representation. The current backend's `__parent: Parent` field-
embedding scheme survives — when `Parent` is `Arc<Parent>` and
`Child` is `Arc<Child>`, the embed becomes
`__parent: Arc<Parent>`. The `Deref` / `DerefMut` impls between
`Child` and `Parent` continue to work; the receiver type just gets
wrapped in `Arc`.

### §CR.5.2. Interfaces (`dyn` dispatch)

A `Box<dyn Interface>` slot forces every class implementing that
interface to **Arc** representation (per §CR.3.3 row 4). Reason:
trait objects in Jux are reference-counted at the spec level, so the
user's `interface I { … }` declaration carries this implication. The
trait-impl emitter doesn't change shape — only the receiver wrapping
does.

### §CR.5.3. Generics

Per §CR.3.4 generic classes pick one representation across all
instantiations. `class Box<T> { T value; }` is selected based on the
most general usage of any `Box<X>` in the program.

A future Phase-2 pass can split the analysis per-instantiation when
the size win warrants it — but the current pipeline (mono-erased,
single-emission) keeps it uniform.

### §CR.5.4. Async + thread boundaries

`spawn(closure)` is the canonical thread boundary. Any class
instance captured by a `spawn` closure (or by a closure that's later
spawned) is **cross_thread** in §CR.3.3 terms — selector forces Arc.
Same for instances stored in `Task<T>` payloads.

`async fn` itself isn't a thread boundary; awaiting doesn't change
the executing thread in Jux's runtime model. But the compiler is
conservative: if the analysis can't prove an `async fn`'s captures
won't be spawned, it picks Arc.

### §CR.5.5. Weak references

The spec's `weak T` (per the future missing-defs §M-WR addendum) is
the only feature that forces a refcount-based representation
*regardless* of escape analysis. A class that's the target of any
`weak` ref must be `Rc` or `Arc` (Rust's `Weak` doesn't work against
plain `Box` or inline structs). The selector forces Arc when the
weak ref crosses threads, Rc otherwise.

### §CR.5.6. `final` / `sealed`

`final class C` doesn't affect the representation — `final` is about
inheritance, not memory layout. `sealed interface I permits A, B`
fixes the set of possible types behind a `dyn I`, but the dispatch
mechanism is the same and the rep still rolls up to Arc.

### §CR.5.7. Static fields

A class field declared `static` lives on the class, not on
instances. Two lowering shapes by immutability:

| Form                              | Rust shape                                                              | Access lowering                                       |
|-----------------------------------|-------------------------------------------------------------------------|-------------------------------------------------------|
| `static final T x = init;`        | `pub const x: T = init;` inside the class's inherent `impl` block.      | `C.x` → `C::x`                                        |
| `static final T x = init;` *(const synonym)* | same as above                                                          | same as above                                         |
| `static T x = init;`              | `static C_x: LazyLock<Mutex<T>> = LazyLock::new(\|\| Mutex::new(init));` at module scope. | Read: `C.x`     → `C_x.lock().unwrap().clone()`<br>Write: `C.x = e` → `*C_x.lock().unwrap() = e` |

**Why split the shape.** Rust forbids `static` items inside `impl`
blocks. The compile-time-evaluable `final` path stays as a `pub const`
associated item (zero runtime cost). Mutable statics need both a
runtime initializer (so `new Foo(...)` and other allocations work) and
`Sync` for global mutable state — `LazyLock<Mutex<T>>` satisfies both
in one shape, at the cost of one lock acquisition per access. The
const-emission `T` in field-position type-mapping (`String` →
`&'static str`) doesn't apply here — the inner storage owns its data
just like a regular instance field, so `String` stays `String`.

**Type mapping.** Field-position type rules apply inside the
`Mutex<T>` slot (`String` → `String`, `int` → `isize`, etc.) so reads
return owned values that match instance-field semantics.

**Generic classes.** A generic class cannot reference its own type
parameter in a static field (matches Java; `T` isn't in scope on a
class-level static). Generic classes therefore can't carry mutable
statics that mention `T`; the backend skips the mutable-static
emission for any class with a non-empty `generic_params` list.

**Concurrency.** The `Mutex` wrap makes mutable statics safe to share
across threads. This is stricter than Java, which leaves mutable
statics thread-unsafe by default — Jux's spec discipline (no
`synchronized`, see `feedback-no-native-synchronized`) means
synchronization has to happen at the lowering layer instead.

**Reassignment vs final-binding.** A `final` static is a Rust
`pub const` and cannot be reassigned (`Test.a = …;` is a tycheck
error against the modifier). A non-final static accepts reassignment
through the `*C_x.lock().unwrap() = e` shape above.

---

## §CR.6 — Rust Lowering Rules

For each representation, the backend's emitted Rust shape:

### §CR.6.1. Inline

```rust
#[derive(Clone, ...)]
pub struct C {
    pub field: T,
    // ...
}

impl C {
    pub fn new(args) -> Self { Self { ... } }
    pub fn method(&self) -> R { ... }
}
```

This is the **current Phase-1 emission shape** for every class. The
representation pass keeps it for classes that qualify and replaces it
with the wrapped form below for the rest.

`===` for inline classes lowers to a hash-of-address sentinel
(`&self as *const _ as usize`) — sound within a single scope, banned
by the selector if cross-scope.

### §CR.6.2. Owned heap

```rust
pub struct C_Inner { ... }

pub struct C(Box<C_Inner>);

impl C {
    pub fn new(args) -> Self { Self(Box::new(C_Inner { ... })) }
    pub fn method(&self) -> R { (*self.0).method_inner() }
    // === lowers to std::ptr::eq(&*self.0, &*other.0)
}
```

Fields are accessed through `self.0.field`. Generated methods
delegate to the same shape on `C_Inner`.

### §CR.6.3. Local refcount

```rust
pub struct C_Inner { ... }

#[derive(Clone)]
pub struct C(std::rc::Rc<C_Inner>);
```

Same field-access shape as §CR.6.2, but `Clone` is now cheap (refcount
bump). Mutating methods need `Rc::make_mut` (clone-on-write) — a small
runtime cost in the rare case the refcount is > 1.

### §CR.6.4. Shared refcount

```rust
pub struct C_Inner { ... }

#[derive(Clone)]
pub struct C(std::sync::Arc<C_Inner>);
```

Same shape as §CR.6.3 with `Arc` in place of `Rc`. Mutating methods
use `Arc::make_mut` (clone-on-write, with atomic refcount check).

### §CR.6.5. Field access through the wrapper

For Box/Rc/Arc reps, every `obj.field` access in user code lowers to
`obj.0.field`. The existing auto-`.clone()` injection on String /
generic fields (per `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.9.3.2)
fires identically — the wrapper is transparent to the field-read
walker.

### §CR.6.6. Method receivers

The `&self` / `&mut self` choice from the existing mutation analysis
carries over unchanged. The wrapper type's `Deref` impl makes
`self.field` resolve to the inner type's field; `&mut self` plus
`Rc::make_mut` / `Arc::make_mut` handles the write side.

---

## §CR.7 — Diagnostics

| Code      | Trigger                                                                         |
|-----------|---------------------------------------------------------------------------------|
| `E0950`   | A class with `===` compared across function boundaries was selected as Inline, but the comparison observably requires a stable address. Hint: the cycle/aliasing analysis demoted the class to Inline; the user can force Box/Rc/Arc by adding a side use that triggers escape. (This rule should fire **before** the selector commits to Inline — it's a sanity check.) |
| `E0951`   | A weak ref is taken against a class the selector decided was Inline. The user's `weak C` declaration forces a refcount-based representation; the selector escalates and re-runs. If escalation fails, this fires. |
| `E0952`   | A cyclic class hierarchy (class field that transitively contains the class itself) is selected as anything other than Arc + Weak. Phase 1 doesn't auto-break cycles; the user has to insert `weak` manually. Until that lands, this is a hard error. |

The selector itself is **silent** by design — picking Inline vs Arc
should never produce a diagnostic on the user's own code. The E095x
codes above only fire when the user does something the selector
can't make sound.

---

## §CR.8 — Worked Examples

### §CR.8.1. Pure-local class → Inline

```jux
public class Point {
    public int x;
    public int y;
    public Point(int x, int y) { this.x = x; this.y = y; }
}

public void main() {
    var p = new Point(3, 4);
    print(p.x + p.y);
}
```

`p` never escapes `main`. Selector picks **Inline**. Lowered Rust:

```rust
pub struct Point { pub x: isize, pub y: isize }
impl Point { pub fn new(x: isize, y: isize) -> Self { Self { x, y } } }

fn main() {
    let p = Point::new(3, 4);
    println!("{}", p.x + p.y);
}
```

Zero heap allocations. Identical to a hand-written Rust struct.

### §CR.8.2. Returned class → Box

```jux
public Point make() {
    return new Point(0, 0);
}
```

Now `make()` constructs a `Point` and returns it; `Point` escapes
its function but isn't otherwise aliased. Selector picks **Box**:

```rust
pub struct Point_Inner { x: isize, y: isize }
pub struct Point(Box<Point_Inner>);
impl Point { pub fn new(x: isize, y: isize) -> Self { Self(Box::new(Point_Inner { x, y })) } }

fn make() -> Point { Point::new(0, 0) }
```

### §CR.8.3. Shared via a collection → Rc

```jux
public void main() {
    var p = new Point(1, 2);
    var xs = new Point[]{p, p};   // p aliased twice
    print(xs[0].x);
}
```

`p` is aliased. Single-threaded. Selector picks **Rc**:

```rust
pub struct Point(std::rc::Rc<Point_Inner>);
// ...
fn main() {
    let p = Point::new(1, 2);
    let xs = vec![p.clone(), p];
    println!("{}", xs[0].0.x);
}
```

### §CR.8.4. Spawned across threads → Arc

```jux
public void main() {
    var p = new Point(1, 2);
    spawn(() -> print(p.x));
    print(p.y);
}
```

`p` captured by the spawned closure → cross-thread → selector picks
**Arc**:

```rust
pub struct Point(std::sync::Arc<Point_Inner>);
// ...
fn main() {
    let p = Point::new(1, 2);
    let p_for_spawn = p.clone();
    spawn(move || println!("{}", p_for_spawn.0.x));
    println!("{}", p.0.y);
}
```

### §CR.8.5. Inheritance roll-up

```jux
public class Animal { public int age; }
public class Dog extends Animal {}

public void main() {
    var d = new Dog();
    var registry = new Dog[]{d, d};   // Dog aliased
}
```

`Dog` is aliased → wants Rc. But `Animal` is the parent; per §CR.3.5
the chain rolls up. Both `Animal` and `Dog` become Rc. Inheritance
through `Deref`:

```rust
pub struct Animal(std::rc::Rc<Animal_Inner>);
pub struct Dog(std::rc::Rc<Dog_Inner>);

pub struct Animal_Inner { age: isize }
pub struct Dog_Inner { __parent: Animal_Inner }

impl std::ops::Deref for Dog { type Target = Animal; ... }
```

The `__parent` slot remains the inner type (not `Animal`), so the
`Deref` impl exposes the outer wrapper. Mutation through DerefMut
calls `Rc::make_mut` automatically.

---

## §CR.9 — Implementation Phasing

The audit identified this addendum as **Tier 1** — to be written
**before** more backend code calcifies the current "always Arc" (or
in Phase 1's case, "always Inline-shaped without identity") choice.
The implementation itself can land in phases:

### Phase 1 (today, no change)

Every class lowers to plain `struct C` + `Clone`. The user's
"reference semantics" promise is mostly violated (`===` doesn't work
properly, assignment copies rather than shares for non-`Clone`
fields). But Phase 1 programs don't observably rely on it.

### Phase 2 — Selector implementation

1. Collect per-class properties (§CR.3.2) in a new analysis pass
   between tycheck and lowering.
2. Apply the decision table (§CR.3.3) to pick a rep per class.
3. Extend the backend's `emit_class_decl` to branch on the selected
   rep and emit one of the four shapes (§CR.6.1–§CR.6.4).
4. Update the field-access, method-call, and `===` emitters to
   thread the rep through.

### Phase 3 — Optimization

- Per-instantiation rep selection for generics.
- Cycle auto-detection and `Weak` insertion.
- Inline classes that escape ONE scope (return + immediate consume)
  could stay Inline if the return is to a known caller — interproc
  analysis.

### Phase 4 — Polish

- `E0950` / `E0951` / `E0952` diagnostic surfaces.
- `// JUX:rep=arc` comment in emitted Rust so users debugging
  generated code know which lowering they're looking at.

---

## §CR.10 — Supersedes

This addendum locks the design for the following previously-implicit
choices:

- `juxc-backend-rust/src/decls/classes.rs::emit_class_decl` — the
  current "every class is plain struct + Clone" emission becomes a
  one-of-four selection. The Phase-1 shape stays valid for classes
  the selector picks as Inline.
- `JUX-LANG-V1.md` §5.2's bullet "heap-allocated reference type" —
  becomes precise: "heap-allocated when the selector picks anything
  other than Inline; otherwise stack-allocated with address-stable
  semantics inside its scope."
- `JUX-INHERITANCE-BORROW-ADDENDUM.md`'s field-embedding scheme —
  reconfirmed; the `__parent` slot mechanism survives across all
  four reps.

No prior addendum is invalidated. The audit's concern ("backend code
calcifies the Arc assumption") is closed by writing this down: the
Arc choice becomes one of four explicit options, not the default
shape baked into emit code.
