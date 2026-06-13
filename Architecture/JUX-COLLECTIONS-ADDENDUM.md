# Jux Spec Addendum — Collections and Reference Semantics

**Status:** Normative. Specifies the value/reference semantics of the built-in
collection types (`List`, `Map`, `Set`, `Deque`) and their Phase-1 lowering.
Resolves gap **C6** ("collections pass by value — a callee's `add` is invisible
to the caller"). Companion to `JUX-CLASS-REPRESENTATION-ADDENDUM.md` (§CR, the
shared-handle model this reuses), `JUX-CORE-LIB-ADDENDUM.md` (§K, which places
collections outside `core`), and JUX-LANG-V1 §8.2 (the Rust-interop type table).

**Sigil:** §V

**Insertion points:**
- New §V.1 ("Goals")
- New §V.2 ("The Model — Collections Are Shared Handles")
- New §V.3 ("Type Mapping")
- New §V.4 ("Semantics, by Example")
- New §V.5 ("Operation Lowering")
- New §V.6 ("Iteration")
- New §V.7 ("Threads and `!Send`")
- New §V.8 ("Arrays Are Not Collections")
- New §V.9 ("Diagnostics")
- New §V.10 ("Worked Example")

---

## §V.1 — Goals

Jux collections must read like Java's: a collection is an **object**, a variable
holds a **reference** to it, and every alias of that reference observes the same
mutations.

```java
List<int> a = new List<int>();
List<int> b = a;        // b refers to the SAME list as a
b.add(1);
print(a.size());        // 1  — a sees b's mutation
```

Before this addendum, collections lowered to bare Rust value types (`Vec`,
`HashMap`, …) and were **copied** on assignment and parameter passing, so the
example above printed `0`. This addendum gives `List` / `Map` / `Set` / `Deque`
**reference semantics** identical to classes (§CR).

Non-goals: changing the *element* copy rules (elements still follow their own
type's value/reference semantics — §V.4.4); making arrays reference types
(§V.8); thread-shared collections (§V.7).

---

## §V.2 — The Model: Collections Are Shared Handles

A collection lowers to the **same shared-handle shape as a class** (§CR.4.1): a
newtype around `Rc<RefCell<Inner>>`, where `Inner` wraps the underlying Rust
container. Assignment and parameter passing clone the `Rc` (a cheap refcount
bump that **shares** the one `RefCell`); reads borrow it, mutations borrow it
mutably. This is the load-bearing reuse: collections are "classes whose body is
a Rust container," so the §CR machinery (interior-mutability rewrites,
share-on-pass clone, re-entrancy snapshotting) applies unchanged.

```text
List<T>   →  List<T>(Rc<RefCell<Vec<T>>>)
Map<K,V>  →  Map<K, V>(Rc<RefCell<HashMap<K, V>>>)
Set<T>    →  Set<T>(Rc<RefCell<HashSet<T>>>)
Deque<T>  →  Deque<T>(Rc<RefCell<VecDeque<T>>>)
```

The legacy bare-container spellings (`ArrayList` ≡ `Vec`, `HashMap`, `HashSet`,
`VecDeque`) name the **same** Jux types — they are accepted as aliases and lower
to the same shared handle. The underlying Rust container is unchanged (still
`Vec`/`HashMap`/…), per "collections are the Rust std collections" (§K); only the
*binding* gains a shared handle.

---

## §V.3 — Type Mapping

| Jux type            | Rust lowering                               | Element access |
|---------------------|---------------------------------------------|----------------|
| `List<T>`           | `Rc<RefCell<Vec<T_rust>>>`                   | `[i]`, `add`, `get`, … |
| `Map<K, V>`         | `Rc<RefCell<HashMap<K_rust, V_rust>>>`       | `[k]`, `put`, `get`, … |
| `Set<T>`            | `Rc<RefCell<HashSet<T_rust>>>`              | `add`, `contains`, … |
| `Deque<T>`          | `Rc<RefCell<VecDeque<T_rust>>>`             | `addFirst`, `removeLast`, … |

`T_rust` etc. are the §8.2 lowerings of the element types. Nullable elements
(`List<int?>`) lower the element to `Option<…>` exactly as today (§E.5 nullable
primitives). Element types that are themselves classes/collections are shared
handles, so a `List<Account>` holds shared `Account` handles (storing or reading
one shares it — §V.4.4).

---

## §V.4 — Semantics, by Example

### V.4.1. Aliasing shares

```java
var a = new List<int>();
a.add(1);
var b = a;          // SHARE: b and a are the same list
b.add(2);
print(a.size());    // 2
```
Lowers the alias to `let b = a.clone();` (an `Rc` bump). Both bindings hold the
same `Rc<RefCell<Vec>>`.

### V.4.2. Passing shares (resolves C6)

```java
void fill(List<int> xs) {
    xs.add(7);          // visible to the caller
}

var xs = new List<int>();
fill(xs);
print(xs.size());       // 1
```
The argument lowers to `xs.clone()` (share the handle); the parameter type is the
shared-handle newtype. The callee's `add` mutates the caller's list.

### V.4.3. Returning shares

A function (or getter) that returns a collection returns the **same** collection
— callers and the origin observe each other's mutations:

```java
public class Bag {
    private List<int> items = new List<int>();
    public List<int> getItems() { return this.items; }   // SHARES the field's list
}

var bag = new Bag();
bag.getItems().add(9);
print(bag.getItems().size());   // 1 — same underlying list
```
> **Supersedes the §CR snapshot-on-return behavior for collections (was gap
> S15).** Returning a collection field now shares the handle rather than deep-
> cloning a snapshot. Re-entrancy during iteration is handled separately by the
> iteration snapshot (§V.6), not by cloning on return.

### V.4.4. Element copies follow the element's own semantics

Reading an element out of a collection yields a value of the element type, copied
per *that type's* rules: a primitive/`String`/record element is copied; a class
or nested-collection element is **shared** (handle clone). Storing an element
follows the same rule.

```java
var names = new List<String>();
names.add("ada");
var n = names.get(0);   // n is a String copy
n = "grace";            // does not affect the list
print(names.get(0));    // "ada"

var accs = new List<Account>();        // Account is a class
var acc = new Account();
accs.add(acc);                          // SHARES acc (handle)
accs.get(0).deposit(100);
print(acc.balance());                   // 100 — same object
```

### V.4.5. `final` does not make a collection immutable

`final List<T> xs` fixes the *binding* (you cannot rebind `xs`), not the list:
`xs.add(...)` is still legal (the reference is fixed, the target is not), exactly
like a `final` class parameter (§M.14.2).

---

## §V.5 — Operation Lowering

Every collection operation goes through the handle's interior borrow. Reads take
`.borrow()`, mutations take `.borrow_mut()`; the borrow is **statement-scoped**
(dropped before the next statement) per the §CR.4.1 borrow discipline. The inner
container sits one tuple field in (`.0`), so the shape is
`recv.0.borrow()[.0]…` — the same rewrite a collection field of a class already
receives (gaps N1/H2-3/H2-4), now applied to standalone collection handles too.

| Jux                 | Rust (sketch)                                            |
|---------------------|---------------------------------------------------------|
| `new List<int>()`   | `List(Rc::new(RefCell::new(Vec::new())))`               |
| `xs.add(v)`         | `xs.0.borrow_mut().push(v)`                              |
| `xs.get(i)`         | `xs.0.borrow()[i as usize].clone()`                     |
| `xs.size()`         | `(xs.0.borrow().len() as isize)`                        |
| `xs[i]` (read)      | `xs.0.borrow()[i as usize].clone()`                     |
| `xs[i] = v`         | `xs.0.borrow_mut()[i as usize] = v`                     |
| `m.put(k, v)`       | `m.0.borrow_mut().insert(k, v)`                          |
| `m[k]` (read)       | `m.0.borrow()[&k].clone()`                              |
| `s.add(v)`          | `s.0.borrow_mut().insert(v)`                             |
| `d.addLast(v)`      | `d.0.borrow_mut().push_back(v)`                          |

**Argument re-entrancy.** A mutating call whose argument reads the *same*
collection (`xs.add(xs.size())`) hoists the argument into a statement temp before
taking the `borrow_mut`, exactly as the §CR receiver-hoist rule does for classes
(gap RISK-3/C1) — so the read-borrow is released before the write-borrow.

---

## §V.6 — Iteration

`for (var x : xs)` snapshots the collection's elements *before* the loop body
runs, so mutating `xs` inside the body never holds a borrow across the body
(no `RefCell already borrowed`, the same rule as iterating a class's own
collection field — gaps H6/S5):

```java
for (var x : xs) {
    if (x == 2) { xs.clear(); }   // safe — iterating a snapshot
    print(x);
}
```
Lowers to roughly `let __it = xs.0.borrow().clone(); for x in __it { … }` (a
copy of the *container*; elements still follow §V.4.4). `forEach` / `map` /
`filter` snapshot the same way.

---

## §V.7 — Threads and `!Send`

`Rc<RefCell<…>>` is `!Send`, so a collection handle **cannot cross a thread
boundary**. Capturing a collection in a `Worker.spawn(…)` / async-task closure is
rejected with the existing thread-capture diagnostic (`E0702`), the same gate
class instances hit (§S.6 / spawn rule). Thread-shared collections (an
`Arc<Mutex<…>>` lowering) are a future addition; Phase 1 keeps collections
single-thread, consistent with the class model.

---

## §V.8 — Arrays Are Not Collections

Fixed and dynamic **arrays** (`T[N]`, `T[]`, §5.5–§5.6, and the multi-dimensional
forms) remain **value** types in Phase 1: `T[N]` is a stack `[T; N]`, `T[]` is a
`Vec<T>` passed by value. An array assigned or passed is **copied**. Code that
wants reference semantics uses a `List<T>`. This is a deliberate, documented
difference from Java (where arrays are references); a reference-array lowering is
a possible future addition. (Arrays-as-class-fields already share through the
class's own handle, like any field.)

---

## §V.9 — Diagnostics

No new error codes. Collection misuse reuses existing ones:

| Code  | Condition |
|-------|-----------|
| `E0702` | A collection (or any `Rc`-backed value) captured by a `spawn`/task closure crossing a thread boundary (§V.7). |
| `E0410` | Assigning a collection of one element type where another is expected (invariant — §T.4). |
| `E0464` | Reassigning a `final` collection *binding* (the list itself stays mutable — §V.4.5). |

---

## §V.10 — Worked Example

```java
public class Inventory {
    private Map<String, int> stock = new Map<String, int>();

    public void receive(String sku, int qty) {
        var current = this.stock.get(sku) ?? 0;
        this.stock.put(sku, current + qty);
    }

    public Map<String, int> snapshot() { return this.stock; }   // shares
}

public void audit(Map<String, int> s) {
    for (var k : s.keys()) {
        print($"${k} = ${s[k]}");
    }
}

public void main() {
    var inv = new Inventory();
    inv.receive("apple", 3);
    inv.receive("apple", 2);

    var live = inv.snapshot();   // SAME map as inv.stock
    live.put("pear", 7);         // visible through inv too
    audit(inv.snapshot());       // apple = 5, pear = 7
}
```

`live`, `inv.stock`, and the `audit` parameter `s` are three aliases of one
`Rc<RefCell<HashMap>>`. Every mutation is visible through all of them — Java
collection semantics, lowered onto Rust's shared-handle idiom.

---

*End of collections addendum. With this in place, gap C6 is resolved and
collections behave like the reference objects Java programmers expect, reusing
the class shared-handle machinery (§CR) wholesale.*
