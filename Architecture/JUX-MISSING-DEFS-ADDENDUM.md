# Jux Spec Addendum — Missing Definitions

**Status:** Proposed insertion. Specifies every construct that JUX-LANG-V1 *uses* without *defining* — `init` blocks, `yield`, `@Derive`, `out` parameters, record `with(...)`, range `step`, property accessors, method references, nested classes, `@Reflectable`, the iteration interface, and resolves the `spawn`-keyword-vs-function ambiguity. **Note:** §M.10 in this document has been superseded by `JUX-OPERATORS-ADDENDUM.md`. Equality/hashing/ordering/cloning/formatting are operator overrides and magic method names, not interfaces.

**Insertion points:**
- New §M.1 ("`init` Blocks")
- New §M.2 ("Generators and `yield`")
- New §M.3 ("`@Derive`")
- New §M.4 ("`out` Parameters")
- New §M.5 ("Record `with(...)` and Withers")
- New §M.6 ("Ranges and `step`")
- New §M.7 ("Property Accessors")
- New §M.8 ("Method References")
- New §M.9 ("Nested Classes")
- New §M.10 ("Foundational Interfaces")
- New §M.11 ("`@Reflectable`")
- New §M.12 ("Resolutions to Acknowledged Inconsistencies")

The text below drops directly into the dossier. It depends on the addenda for grammar, semantics, and layout.

---

## Design Philosophy (Non-Normative)

Every item in this addendum is a small, focused fill. None of them changes the language's character; they make its references coherent. The throughline:

- **No magic.** Every keyword and annotation has a documented, mechanical meaning.
- **One way to do it.** Where there are multiple plausible designs (e.g., `hasNext()/next()` vs `next() -> T?`), pick one and mean it.
- **Match the user's existing intuition where it doesn't cost.** `init` blocks behave like Kotlin's. `with(...)` behaves like Kotlin's `copy(...)`. Method references behave like Java's.

---

## §M.1 — `init` Blocks

JUX-LANG-V1 §3.2 reserves `init` as a keyword but never specifies its role. This section gives it one.

An **`init` block** is a body of code that runs once during construction of a class instance, after field initializers and **before the constructor body** (per `ERRATA.md` E2 and `JUX-SEMANTICS-ADDENDUM.md` §S.4.4 — Java's instance-initializer order), in textual order with other `init` blocks of the same class.

### M.1.1. Syntax

```
init-block        = 'init' block
```

`init` blocks may appear among the members of a `class` or `struct`. They may not appear in `record`, `interface`, or `enum`.

```jux
public class Connection {
    private String host;
    private int port;
    private Socket socket;

    private int attempts = 0;

    init {
        // Runs after field initializers, BEFORE the constructor body —
        // it sees `attempts == 0`, not anything the body assigns.
        log.info("Connection object under construction");
    }

    public Connection(String host, int port) throws IOException {
        this.host = host;
        this.port = port;
        this.socket = Socket.open(host, port);
    }

    drop {
        socket.close();
    }
}
```

### M.1.2. Semantics

`init` blocks fit into the construction sequence (per `JUX-SEMANTICS-ADDENDUM.md` §S.4.4) as step 4 — after the superclass construction and field initializers, **before the constructor body** (step 5). This is Java's instance-initializer order (ERRATA E2): an init block observes field-initializer values, never the constructor body's writes, and may not read constructor parameters (it is shared by every constructor).

When a class has multiple constructors and multiple `init` blocks, every constructor runs every `init` block, in the textual order they appear in the class body. (Kotlin's rule.) This makes `init` the right place to put validation or one-time setup that must happen on every construction path:

```jux
public class Range {
    private int start;
    private int end;

    public Range(int start, int end) {
        this.start = start;
        this.end = end;
    }

    public Range(int single) {
        this(single, single + 1);
    }

    init {
        if (start > end) throw new IllegalArgumentException("inverted range");
    }
}
```

Both constructors hit the same `init` validation. The second constructor delegates to the first, which means the `init` block runs **once** (at the end of the delegated-to constructor's chain), not twice.

### M.1.3. Borrow Rules

Inside an `init` block, `this` is treated as **exclusively borrowed** — the same as inside a constructor. References to `this` may not escape the `init` block. In particular:

- Storing `this` in a static field or in another object reachable from outside is rejected (`E0910`).
- Calling a method on `this` that escapes the receiver reference is rejected.

This prevents partially-constructed objects from being observed by other code.

### M.1.4. `init` and Inheritance

`init` blocks of a superclass run before any `init` blocks of the subclass — they are part of the superclass's construction, which completes before the subclass's body or `init` blocks run. The full order:

1. Subclass constructor's `super(...)` resolves first.
2. Superclass field initializers run.
3. Superclass constructor body runs.
4. Superclass `init` blocks run.
5. Subclass field initializers run.
6. Subclass constructor body runs.
7. Subclass `init` blocks run.

This matches the rule for `drop` (`JUX-SEMANTICS-ADDENDUM.md` §S.5.2), but inverted.

> **Edit to JUX-LANG-V1 §7.3:** The "Primary constructor with init block" example uses the term "init block" without defining it. With this addendum, that reference now resolves to §M.1.

---

## §M.2 — Generators and `yield`

JUX-LANG-V1 §3.2 reserves `yield` as a keyword but never specifies it. This section gives it the role of producing values from a **generator function**.

### M.2.1. Generator Functions

A function whose body contains a `yield` statement is a **generator**. Its return type must be `Iterator<T>` (or `Stream<T>` for async generators); the compiler produces a state machine that implements the iterator protocol.

```jux
public Iterator<int> naturals() {
    var n = 0;
    while (true) {
        yield n;
        n = n + 1;
    }
}

public Iterator<int> first10Squares() {
    for (var i = 0; i < 10; i++) {
        yield i * i;
    }
}

public void main() {
    for (var sq : first10Squares()) {
        print(sq);   // 0, 1, 4, 9, ..., 81
    }

    var nat = naturals();
    print(nat.next());     // 0
    print(nat.next());     // 1
}
```

### M.2.2. Async Generators

When the function is `async`, the return type is `Stream<T>` (defined in `std.async`):

```jux
public async Stream<String> readLines(Path path) {
    var file = await File.open(path);
    var buf = new StringBuilder();
    while (true) {
        var byte = await file.readByte();
        if (byte == null) break;
        if (byte == '\n') {
            yield buf.toString();
            buf.clear();
        } else {
            buf.append(byte);
        }
    }
    if (buf.length > 0) yield buf.toString();
}

public async void process() {
    for await (var line : readLines(Path.of("data.txt"))) {
        if (line.startsWith("#")) continue;
        process(line);
    }
}
```

This closes the async-stream gap noted in `JUX-GAPS-ROADMAP.md` §1.5.

### M.2.3. Statement Forms

```
yield-stmt        = 'yield' expression ';'
                  | 'yield' '*' expression ';'         -- yield-from (delegate)
```

`yield expr` produces one value. `yield* iter` is shorthand for "yield every value from `iter`":

```jux
public Iterator<int> chained(Iterator<int> a, Iterator<int> b) {
    yield* a;
    yield* b;
}
```

### M.2.4. Restrictions

A generator function's body may not:

- Contain `return expr` with a value (use `yield`). `return;` (no value) is permitted and ends the iterator.
- Cross an `unsafe { }` boundary that contains a `yield` (the state machine cannot reliably preserve `unsafe` invariants across suspension).

A generator function's `Iterator<T>` value:

- Holds the suspended local state of the function.
- Is dropped when the iterator is dropped; the function body runs to its next implicit cleanup point (running any `drop` blocks for locals along the way).
- Cannot be cloned (its state is owned, not shareable).

### M.2.5. Lowering

The compiler rewrites a generator into a struct holding the function's locals plus a state-machine PC. `next()` advances the state machine to the next `yield` and returns the yielded value, or null at exhaustion. This is exactly the C# / JavaScript / Python lowering, applied at compile time.

---

## §M.3 — `@Derive` (Largely Obsolete; See §O.4)

In the operator-first design (`JUX-OPERATORS-ADDENDUM.md`), records, structs, and enums **automatically gain** the relevant operators (`operator==`, `operator<=>`, `operator hash`, `operator string`) from their structure with **no annotation required**. `@Derive` is therefore unnecessary for value types.

`@Derive` remains as a **no-op annotation** in v1, accepted by the parser for forward compatibility but generating no extra code. The compiler emits a `W0240` warning suggesting removal.

```jux
@Derive(operator==, operator hash)       // no-op; record already has these
public record Point(double x, double y) {}
```

### M.3.1. What Auto-Derivation Provides (Per `JUX-OPERATORS-ADDENDUM.md` §O.3)

| Type kind | Auto-provides                                                                |
|-----------|------------------------------------------------------------------------------|
| `record`  | `operator==`, `operator hash`, `operator string`, implicit copy             |
| `struct`  | `operator==`, `operator string`, implicit copy (no `operator hash` — opt in) |
| `enum`    | `operator==`, `operator hash`, `operator string`, implicit copy             |
| `class`   | nothing — declare each operator you need                                     |

### M.3.2. Class Opt-In

A class that wants structural equality writes `operator==` and `operator hash`:

```jux
public class FilePath {
    public String path;

    public bool operator==(FilePath other) { return path == other.path; }
    public int operator hash() { return path.operator hash(); }
}
```

The compiler enforces the pairing — `operator==` without `operator hash` is `E0931`. No interface declaration. No annotation.

### M.3.3. Suppressing Auto-Derivation

A record that wants to delete an auto-derived operator (e.g., to redact a field from default formatting):

```jux
public record OpaqueToken(String secret) {
    public String operator string() = delete;
}
```

Per `JUX-OPERATORS-ADDENDUM.md` §O.3.4.

### M.3.4. Future Macros

User-defined derive-style annotations (e.g., `@Serializable`) require the macro/annotation-processing model called out in `JUX-GAPS-ROADMAP.md` §3.4 — deferred to a future edition.

---

## §M.4 — `out` Parameters

JUX-LANG-V1 §8.1 uses `out RawHandle* db` and `out String error` without specifying `out`. This section does.

### M.4.1. Syntax

```
param-mode        = binding-immut | 'out'      -- binding-immut = 'const' | 'final'
```

An `out` parameter is a parameter the function **writes to** rather than reads from. The caller passes a binding (a variable or a field) that will be assigned by the function. (The full parameter-modifier reference, including `final` semantics and the combination matrix, is §M.14.)

```jux
public bool tryParse(String s, out int result) {
    var parsed = parseInt(s);
    if (parsed != null) {
        result = parsed;        // assignment to out parameter
        return true;
    }
    return false;
}

int n;
if (tryParse("42", out n)) {
    print(n);                   // 42
}
```

### M.4.2. Semantics

An `out` parameter at the call site is denoted with the `out` keyword:

```jux
tryParse("42", out n);
```

The argument expression must be an **assignable place** (a variable, a field of a value the caller can mutate, an array element). The function's contract is:

- The function **must** assign to the `out` parameter on every code path that returns or completes normally. The compiler enforces this (`E0940`).
- The argument's prior value is **not read** by the function before the function assigns. The argument may be uninitialized at the call site.
- After the call, the argument is initialized with the value the function assigned.

This makes `out` parameters useful for:

- C-style "return a status code, write the result through a pointer" APIs without raw pointers.
- Cases where a function naturally produces multiple values and the caller already has slots for them.

### M.4.3. Borrow Rules

An `out` parameter takes an **exclusive borrow** on the argument for the duration of the call. The borrow checker treats it the same as a mutable reference passed to a function that mutates: while the call is in flight, no other code may observe the slot.

If an `out` argument is a class field (e.g., `out user.id`), the entire enclosing object is exclusively borrowed for the call (per the whole-object rule, `JUX-LANG-V1 §6.9.1`).

### M.4.4. `out null` and Optional Outputs

Some C APIs accept null for "I don't care about this output." Jux models this with `out null` syntax:

```jux
@extern(lib = "sqlite3")
unsafe native {
    int sqlite3_exec(RawHandle* db, String sql,
                     void* callback, void* arg, out String errmsg);
}

unsafe {
    var rc = sqlite3_exec(db, sql, null, null, out null);
}
```

`out null` is permitted only for `out` parameters whose declared type is a raw pointer or a nullable type. It tells the compiler to pass a null-pointer slot to the callee.

### M.4.5. Comparison with Alternatives

For Jux-to-Jux APIs, multi-return is usually better expressed via a tuple:

```jux
public (bool, int) tryParse(String s) {
    var parsed = parseInt(s);
    return (parsed != null, parsed ?: 0);
}

var (ok, n) = tryParse("42");
```

`out` is for FFI compatibility and for the rare case where `Result<T, E>` and tuples are both awkward. Style guide: prefer return values and tuples; reach for `out` when interfacing with C or matching an external convention.

### M.4.6. Implementation Notes (Phase 1)

`out` is a **contextual** keyword: it introduces the parameter mode only when it
precedes a type in a parameter list, and the call-site `out` only when it precedes
a place argument — so an ordinary identifier named `out` (variable, field, label)
continues to work.

**Lowering to Rust.** An `out int result` parameter lowers to `result: &mut isize`.
In the body, an assignment `result = v` lowers to `*result = v` and any read to
`(*result)`. At the call site, `out <place>` lowers to `&mut <place>`:

| Place form          | Jux               | Emitted Rust                     |
|---------------------|-------------------|----------------------------------|
| local               | `f(out n)`        | `f(&mut n)`                      |
| plain-struct field  | `f(out c.value)`  | `f(&mut c.value)`                |
| shared-ref field    | `f(out b.field)`  | `f(&mut b.0.borrow_mut().field)` |
| array element       | `f(out a[i])`     | `f(&mut a[i])`                   |

A shared-reference (aliased) class field takes the **mutable** interior borrow; the
`RefMut` temporary lives to the end of the call statement (Rust temporary-lifetime
extension), so the `&mut` into it stays valid for the callee. An uninitialized local
out-arg (`int n;`) is default-initialized at its declaration so the `&mut` is
well-formed (`let mut n: isize = 0;`).

**Definite assignment.** The must-assign-on-every-normal-exit check (`E0940`) reuses
the same forward dataflow engine as field definite-assignment (if/else intersection,
loops don't escape, `return`/`throw` diverge).

**Diagnostics.**

| Code    | Condition                                                              |
|---------|------------------------------------------------------------------------|
| `E0940` | `out` parameter not assigned on some normal-exit path.                 |
| `E0942` | `out` argument is not an assignable place (e.g. `out gen()`).          |
| `E0943` | `out` arg/param disagreement (`out` on a plain param, or a plain arg to an `out` param). |
| `E0944` | `out` combined with `final`/varargs/default, or on a constructor param. |

**Deferred.** `out null` (§M.4.4) and the `move` call-site operator are follow-ups —
both are FFI/ownership refinements not required by the Phase-1 (Jux-to-Rust) surface.

---

## §M.5 — Record `with(...)` and Withers

JUX-LANG-V1 §7.6 shows `v.with(x: 5.0)` without specifying it. This section does.

### M.5.1. Auto-Generated Wither

Every `record` has a synthesized `with(...)` method that returns a new record with one or more fields replaced. The method takes named arguments matching field names; arguments not provided keep the original record's values.

```jux
public record Vector3(double x, double y, double z) {}

var v = new Vector3(1.0, 2.0, 3.0);
var v2 = v.with(x: 5.0);                  // (5.0, 2.0, 3.0)
var v3 = v.with(x: 5.0, z: 7.0);          // (5.0, 2.0, 7.0)
var v4 = v.with();                         // identical copy
```

### M.5.2. Type and Visibility

The signature is:

```jux
public Self with(<each field>: <field type> = self.field);
```

— effectively a function whose every parameter has a default of the original field's value. All fields are accessible through `with(...)`, regardless of their declared visibility (the field is the record's structure, and `with(...)` is the canonical way to derive a new record from an existing one).

### M.5.3. Structs and Classes

Structs do **not** auto-generate `with(...)` because struct mutation is in-place — there is no need to return a copy. Classes do not auto-generate `with(...)` because their identity matters; copying is opt-in via a user-defined `clone()` method.

### M.5.4. Nested Records

For nested records, the `with(...)` syntax is local to the level being modified:

```jux
public record Address(String city, String country) {}
public record User(String name, Address addr) {}

var u = new User("Alice", new Address("Paris", "FR"));
var u2 = u.with(addr: u.addr.with(city: "Lyon"));
```

There is no "deep wither" syntax in v1. Deeply-nested updates spell out the path; helper utilities can reduce this if it becomes common in practice (e.g., a future `path-based update` library).

---

## §M.6 — Ranges and `step`

JUX-LANG-V1 §7.16 shows `0..10`, `0..=10`, and `10..0 step -1`. This section formalizes them.

### M.6.1. Range Types

```jux
public sealed interface Range<T> permits ExclusiveRange, InclusiveRange {}
public struct ExclusiveRange<T>(T start, T end);
public struct InclusiveRange<T>(T start, T endInclusive);
public struct SteppedRange<T>(Range<T> base, T step);
```

`a..b` produces an `ExclusiveRange<T>`. `a..=b` produces an `InclusiveRange<T>`. `r step s` produces a `SteppedRange<T>` from any range `r` and a step value `s`.

For built-in numeric types, ranges are iterable:

- `ExclusiveRange<int>` iterates `start, start+1, ..., end-1`.
- `InclusiveRange<int>` iterates `start, start+1, ..., end`.
- `SteppedRange<int>` iterates `start, start+step, start+2*step, ...` while `(step > 0 && cur < end) || (step < 0 && cur > end)`.

A negative step requires `start >= end` (for exclusive) or `start > end` (for inclusive); otherwise the iteration is empty. A zero step is a runtime panic (`ArithmeticException`).

### M.6.2. Generic Ranges

For user types, ranges work when `T` defines the appropriate operators. The exact bounds:

- Plain `a..b` requires `T has operator<=>(T) -> int` (per `JUX-OPERATORS-ADDENDUM.md` §O.5.1).
- Iterating a range requires `Steppable<T>` — a built-in interface defined as:

```jux
public interface Steppable<T> {
    T next(T value);                   // for natural step
    T offset(T value, long delta);     // for arbitrary step
}
```

Integer types and `char` implement `Steppable`. User types can implement it to participate in `for` loops.

### M.6.3. `step` Grammar

`step` is a contextual keyword (per `JUX-GRAMMAR-ADDENDUM.md` §A.1.3) and binds tighter than other operators on the right of a range expression but looser than primary expressions:

```jux
for (var i : 0..n step 2) { ... }       // 0, 2, 4, ..., (largest even < n)
for (var i : (0..n) step (k + 1)) { ... }
```

### M.6.4. Range Patterns

Ranges may appear in `switch` patterns:

```jux
switch (n) {
    case 0..10  -> "small";
    case 10..100 -> "medium";
    case 100..  -> "large";        // open-ended, "100 or more"
    case ..0    -> "negative or zero";
}
```

Open-ended range patterns (`x..`, `..x`, `..=x`) are permitted in patterns only, not as iterable values.

---

## §M.7 — Properties (C#-style)

Jux properties use **the C# property syntax, with one deliberate divergence:
expression bodies use `->`, not C#'s `=>`** — because `=>` is Jux's type-test
(instanceof) operator (`a => Type`) and must stay unambiguous. So: `{ get; set; }`
blocks, expression-bodied properties (`-> expr`), expression-bodied accessors
(`get -> expr`), full accessor bodies, and the implicit `value` parameter in setters.

This supersedes the earlier shorthand syntax (`public get String name`) referenced in JUX-LANG-V1 §7.3 — that form is removed.

> **Observable properties (§P).** Every `{ get; set; }` property is also observable
> and bindable — see `JUX-OBSERVABLE-PROPERTIES-ADDENDUM.md` for the `observer<T>`
> type, the `.observers` member, `bind`/`bindBidirectional`/`unbind`, computed-property
> dependency tracking, and the PascalCase naming convention (preferred, not enforced).
> That addendum also **removes the `init` accessor** — accessor kinds are `get` and
> `set` only. This section has been updated accordingly: read-only construction-time
> properties are written `{ get; }`.

### M.7.1. Syntax

```
property-decl     = modifier* type identifier property-body? property-init? ';'?

property-body     = '{' accessor-list '}'
                  | '->' expression                          -- expression-bodied (read-only)

accessor-list     = accessor (accessor)*

accessor          = visibility? accessor-kind accessor-body
accessor-kind     = 'get' | 'set'                             -- 'init' accessor removed per §P
accessor-body     = ';'                                       -- auto: synthesize body
                  | '->' expression ';'                       -- expression-bodied
                  | block                                      -- full body

property-init     = '=' expression                            -- field-initializer for auto-property

modifier          = visibility | 'static' | binding-immut | 'volatile'
```

A property may have:
- No body (`public String email;`) — a plain mutable field. The compiler treats this exactly like a public field; the property syntax is only relevant when one wants accessor control.
- A `{ get; set; }` body — auto-property with synthesized backing field.
- A `-> expr` body — expression-bodied read-only computed property.
- A `{ get-or-set blocks }` body — full custom accessors.

Inside a setter body, the parameter is implicitly named **`value`** (C# convention). It has the property's declared type.

### M.7.2. Auto-Properties

The compiler synthesizes a private backing field for auto-properties. The backing field is not directly nameable from user code — access goes through the property.

```jux
public class User {
    public String name { get; set; }                  // read-write
    public String id { get; }                         // read-only (settable only in constructor)
    public int age { get; }                           // read-only (settable only in constructor)
    public String email { get; private set; }         // public read, private write
    public int score { get; protected set; }          // public read, protected write

    public User(String id, String name) {
        this.id = id;            // OK: read-only auto can be set in constructor
        this.name = name;
        this.age = 0;            // OK: read-only auto can be set in constructor
    }
}

var u = new User("u-42", "Alice");
print(u.name);                   // "Alice"
u.name = "Bob";                  // OK: public set
u.id = "u-99";                   // ERROR: id is read-only (E0970)
u.age = 30;                      // ERROR: age is read-only (E0970)
u.email = "x@y";                 // ERROR: email's set is private (outside the class, E0972)
```

**Read-only auto-properties.** A property with `{ get; }` and no setter is settable
only inside the constructors (and `init { ... }` blocks, §M.1) of the declaring type.
After construction it is immutable. Records additionally allow `with(...)` copies
(per §M.5), which construct a new instance rather than mutating the original.

> There is **no `init` accessor** in Jux. The C# 9 `{ get; init; }` form was part of
> an earlier draft of this section and was removed by
> `JUX-OBSERVABLE-PROPERTIES-ADDENDUM.md` — `{ get; }` covers the use case.

### M.7.3. Property Initializer

A property may carry an `=`-initializer. The expression is evaluated once during construction (per `JUX-SEMANTICS-ADDENDUM.md` §S.4.4 step 3, alongside field initializers).

```jux
public class Config {
    public String host { get; set; } = "localhost";
    public int port { get; } = 8080;
    public List<String> tags { get; } = new List<>();
}
```

### M.7.3.1. Implicit-Nullable Auto-Properties (No Initializer)

An auto-property declared with **no initializer**, or with an explicit `= null`, is
**implicitly nullable** and defaults to `null`. The author writes the property type
without a `?`, and the compiler treats the property as `T?`: the getter returns `T?`,
the setter accepts `T?`, and reading the property before anything assigns it yields `null`.

```jux
public class Box<T> {
    public T Value { get; set; }              // implicitly T?, reads null until set
    public int Count { get; set; }            // implicitly int?, reads null until set
    public String Name { get; set; } = null;  // same: implicitly String?
}

var b = new Box<String>();
print(b.Value == null);                       // true
print(b.Count == null);                       // true
b.Count = 10;                                 // assigns Some(10)
int n = b.Count!! + 1;                        // 11 (assert non-null to read as int)
```

A property that carries a **real initializer** keeps its declared, non-nullable type:

```jux
public int Count { get; set; } = 0;           // non-nullable int, reads 0
```

This makes "declare a property and assign it later" ergonomic without forcing an explicit
initializer, while keeping null-safety honest: because the property type is `T?`, a read
must null-check (or use `!!`) before the value can be used where `T` is required. A
non-nullable read is only available once the property has a real initializer or has been
assigned a non-null value.

> **Why nullable rather than a zero value.** A uniform `null` default lets code, and
> change-observers (§P), distinguish "never set" from "set to a value": the first
> assignment is a real `null -> value` transition that observers detect.

### M.7.4. Expression-Bodied Properties

For computed read-only properties, `-> expression` is a shorthand for `{ get -> expression; }`:

```jux
public class Person {
    public String firstName { get; }
    public String lastName { get; }

    public String fullName -> firstName + " " + lastName;       // shorthand
    // Equivalent verbose form:
    // public String fullName { get -> firstName + " " + lastName; }
}
```

The expression-bodied form is read-only by definition — there is no setter.

### M.7.5. Expression-Bodied Accessors

Either accessor can use `-> expression` instead of a `{ ... }` block, when the body is a single expression:

```jux
public class Counter {
    private List<int> items = new List<>();

    public int count {
        get -> items.size();
        set -> items.reserve(value);
    }
}
```

For the setter, the expression is evaluated for its side effect; its value is discarded.

### M.7.6. Full Accessor Bodies

When validation, multiple statements, or block logic is needed:

```jux
public class User {
    private String _passwordHash;

    public String password {
        get { return _passwordHash; }
        set {
            if (value.isEmpty()) throw new IllegalArgumentException("empty password");
            _passwordHash = hash(value);
        }
    }
}
```

In setters, **`value`** is the implicit parameter name with the property's declared type.

### M.7.7. Asymmetric Visibility

Either accessor may carry its own visibility, which must be **at least as restrictive** as the property's outer visibility:

```jux
public String email { get; private set; }              // public read, private write
internal int counter { get; protected set; }            // internal read, protected write (within module + subclasses)
public bool ready { protected get; private set; }       // ERROR: outer = public, but get is protected
```

The compiler rejects accessor visibility looser than the property's outer visibility (`E0972`). The common shorthand pattern — `{ get; private set; }` — is C#'s most-used form and should be the typical case.

### M.7.8. Mixing Bodies

The two accessors can use different body forms:

```jux
public double celsius {
    get -> kelvin - 273.15;             // expression-bodied
    set { kelvin = value + 273.15; }    // full body
}
```

### M.7.9. Static Properties

Properties can be `static`, with the same body forms:

```jux
public class Config {
    public static String version { get; } = "1.0.0";
    public static Path home -> Environment.getHome();
}
```

A `static` property accesses the class, not an instance. `Config.version`, not `someConfig.version`.

### M.7.10. Properties on Records and Interfaces

Records auto-derive read-only `{ get; }` properties for every component — that's what makes them immutable and constructor-settable (`with(...)` copies construct a new instance, per §M.5). A user can additionally declare further computed properties:

```jux
public record Vector3(double x, double y, double z) {
    // x, y, z are read-only { get; } auto-properties from the record components
    public double magnitudeSquared -> x*x + y*y + z*z;
    public double magnitude -> sqrt(magnitudeSquared);
}
```

Interfaces can declare property contracts:

```jux
public interface HasName {
    String name { get; }                                 // read-only contract
    String description { get; set; }                     // read-write contract
}
```

Implementing types must provide matching properties (or fields with the right visibility).

### M.7.11. Mutation Inference

The borrow checker treats accessors as it treats methods (per `JUX-LANG-V1 §6.3`):

- A getter that does not assign to fields is non-mutating; calls require shared access.
- A setter, by definition, mutates the receiver; calls require exclusive access.
- An auto-property's synthesized accessors get exact mutation summaries (`get` non-mutating, `set` mutating).
- A computed property with no backing field state mutation is non-mutating regardless of whether it's expression-bodied or full-bodied.

### M.7.12. Java Compatibility Note

Java getters/setters (`getName()`, `setName(...)`) are **not** generated. `obj.name` is the access. The compiler's `juxc bindgen` may produce both forms only when generating Java-bytecode-targeting bindings (out of scope for Phase 1).

### M.7.13. Worked Example

```jux
public class Temperature {
    private double _kelvin;

    public Temperature(double kelvin) {
        this._kelvin = kelvin;
    }

    public double kelvin {
        get -> _kelvin;
        set {
            if (value < 0) throw new IllegalArgumentException("negative kelvin");
            _kelvin = value;
        }
    }

    public double celsius {
        get -> _kelvin - 273.15;
        set -> kelvin = value + 273.15;          // delegates through kelvin's setter (validation reused)
    }

    public double fahrenheit {
        get -> celsius * 9.0/5.0 + 32.0;
        set -> celsius = (value - 32.0) * 5.0/9.0;
    }

    public bool isFreezing -> celsius <= 0.0;     // expression-bodied read-only
}

var t = new Temperature(300.0);
print(t.celsius);           // 26.85
print(t.fahrenheit);        // 80.33
t.celsius = 100.0;
print(t.kelvin);            // 373.15
print(t.isFreezing);        // false
```

This is C# property syntax exactly. Anyone who's used C# auto-properties picks it up instantly; the difference vs. Java is dramatic — no `getX()`/`setX()` boilerplate, no four-line getter+setter pairs, just declarative property declarations.

---

## §M.8 — Method References

JUX-LANG-V1 §7.9 shows `users.forEach(User::greet)` without specifying `::` formally.

### M.8.1. Syntax

```
method-ref        = receiver '::' method-name
receiver          = qualified-name             -- type or instance
method-name       = identifier | 'new'
```

### M.8.2. Forms

- `Type::method` — references a static method on `Type`. Result is a function value of the method's type.
- `Type::new` — references a constructor of `Type`. Result is a function value `(...) -> Type`.
- `instance::method` — references an instance method bound to `instance`. Captures `instance` (per closure capture rules, `JUX-LANG-V1 §7.9`). Result is a function value of the method's type, with `this` bound.
- `Type::instanceMethod` (where `instanceMethod` is non-static) — references the instance method as an "unbound" function whose first parameter is the receiver. Result is `(Type, ...args) -> ReturnType`.

```jux
var greeter = User::greet;            // (User) -> void  (unbound)
greeter(alice);                        // calls alice.greet()

var aliceGreets = alice::greet;       // () -> void  (bound)
aliceGreets();                         // calls alice.greet()

var makeUser = User::new;             // (String, int) -> User
var u = makeUser("Bob", 30);
```

### M.8.3. Overloaded Methods

When `User::greet` could refer to one of several overloads, the compiler picks the overload based on the expected function type at the use site. If no expected type or multiple overloads match, the call is ambiguous (`E0980`); disambiguate by writing a lambda explicitly.

### M.8.4. Why `::` and Not `.`

The `::` operator is unambiguous: `Type.method` is a static method **call**, while `Type::method` is a method **reference**. Reusing `.` here (Java's choice for some cases) creates parser ambiguity that's painful to resolve. Jux uses `::` consistently for "name without invoking."

---

## §M.9 — Nested Classes

JUX-LANG-V1 §7 does not cover nested classes. Following the recommendation in `JUX-GAPS-ROADMAP.md` §2.3:

### M.9.1. Static Nested Types Only

A class, struct, record, enum, or interface declared inside another class is a **static nested type**. It has no implicit reference to the enclosing instance:

```jux
public class HttpServer {
    public final class Config {           -- static nested class (no enclosing this)
        public int port;
        public int maxConnections;
    }

    public final class Request {           -- another nested type
        public String path;
        public Map<String, String> headers;
    }

    private Config config;

    public HttpServer(Config config) {
        this.config = config;
    }
}

// Usage from outside:
var cfg = new HttpServer.Config();
cfg.port = 8080;
var server = new HttpServer(cfg);
```

The nested type is namespaced under the enclosing type. Outer-class private members are accessible from the nested type and vice-versa (same visibility relaxation as Java for nested types).

### M.9.2. What Is NOT Supported

- **Inner classes** with implicit outer reference (Java's `class Inner` inside `class Outer`). Lifetime entanglement breaks the borrow inference; users wanting this should pass an explicit reference.
- **Anonymous classes** (Java's `new Runnable() { ... }`). Lambdas and functional interfaces (any single-method interface) cover the use case.
- **Local classes** (declared inside a method body). Rare and replaceable by lambdas or by lifting to nested.

Each of these is rejected with a clear message (`E0991`–`E0993`) suggesting the recommended alternative.

### M.9.3. Visibility

A nested type's visibility is the more restrictive of its own visibility and the enclosing type's:

```jux
public class Outer {
    private class Helper { }        -- visible only inside Outer
    public class Pub { }            -- visible everywhere Outer is
}
```

### M.9.4. Phase-1 Implementation Notes

Nested types are **lifted** to the top level under an owner-qualified internal name (`Config` inside `HttpServer` becomes `HttpServer__Config`, recursively for deeper nesting), which is also the emitted Rust struct name. Both access forms resolve onto the lifted type: qualified `HttpServer.Config` (type positions and `new` expressions) and bare `Config` from inside the owner or a sibling nested type (innermost enclosing scope wins). Two classes may freely nest same-named types.

Deferred to a later phase: the M.9.3 visibility *combination* rule (Phase-1 uses each type's own declared visibility) and the cross-nesting private-member relaxation; qualified access to a nested **enum's variants** (`Outer.Status.Active`) — declare the enum top-level when variants are consumed outside the owner.

---

## §M.10 — Iteration and Markers (Operator-First Replacement)

**This section was originally a list of foundational interfaces (`Equatable`, `Hashable`, `Comparable`, `Cloneable`, `Displayable`, `Sized`).** Per `JUX-OPERATORS-ADDENDUM.md`, those interfaces **do not exist** in Jux. Equality, ordering, hashing, and formatting are operator overrides (`operator==`, `operator<=>`, `operator hash`, `operator string`) — C++-style. No interface ceremony, no magic method names.

What remains in this section: the *one* foundational interface (iteration) and the inferred markers.

### M.10.1. `Iterator<T>`

```jux
public interface Iterator<T> {
    T? next();
}
```

Returns the next element, or `null` when the iterator is exhausted. Single-method form (per `JUX-GAPS-ROADMAP.md` §1.1).

### M.10.2. `Iterable<T>`

```jux
public interface Iterable<T> {
    Iterator<T> iterator();

    default Iterable<R> map<R>((T) -> R f) { ... }
    default Iterable<T> filter((T) -> bool pred) { ... }
    default U reduce<U>(U initial, (U, T) -> U combine) { ... }
    default Iterable<T> take(int n) { ... }
    default Iterable<T> skip(int n) { ... }
    default Iterable<(T, T2)> zip<T2>(Iterable<T2> other) { ... }
    default Iterable<T> chain(Iterable<T> other) { ... }
    default int count() { ... }
    default bool any((T) -> bool pred) { ... }
    default bool all((T) -> bool pred) { ... }
    default T? firstOrNull((T) -> bool pred) { ... }
    default T? minOrNull() where T has operator<=>(T) -> int { ... }
    default T? maxOrNull() where T has operator<=>(T) -> int { ... }
}
```

Any type implementing `Iterable<T>` works in `for (var x : value)` loops. Combinators are lazy where it makes sense (`map`, `filter`, `take`, `skip`, `chain`, `zip`) and eager where they must be (`reduce`, `count`, `any`, `all`).

`Iterable<T>` is the **only** nominal foundational interface in `core`. Per `JUX-OPERATORS-ADDENDUM.md` §O.6, iteration benefits from a nominal contract (one-line opt-in for types that want it); equality/ordering/hashing don't.

### M.10.3. `Sendable` and `Shareable`

These are **inferred markers**, not implementable interfaces. The compiler determines them from a type's structure:

- `Sendable`: every field is `Sendable`. Primitives and `String` are `Sendable`. Class types are `Sendable` if they have an atomic refcount and all fields are `Sendable` (per `JUX-LAYOUT-ABI-ADDENDUM.md` §L.2.1).
- `Shareable`: every field is `Shareable`. Primitives are `Shareable`. Immutable class types (no mutating methods) are `Shareable`. Atomics are `Shareable`. Mutexes are `Shareable`.

Code may bound generics on these markers (`<T : Sendable>`); user-implemented bodies are rejected (`E0951`).

### M.10.4. Auto-Capability Summary

What each type gains automatically (per `JUX-OPERATORS-ADDENDUM.md` §O.3):

| Type kind | Auto-provides                                                                |
|-----------|------------------------------------------------------------------------------|
| primitive | `operator==`, `operator<=>`, `operator hash`, `operator string`, implicit copy |
| `struct`  | `operator==`, `operator string`, implicit copy (no `operator hash` — opt in) |
| `record`  | `operator==`, `operator hash`, `operator string`, implicit copy             |
| `enum`    | `operator==`, `operator hash`, `operator string`, implicit copy             |
| `class`   | nothing — declare each operator explicitly                                   |

`Sendable` and `Shareable` are inferred for every type.

---

## §M.11 — `@Reflectable`

JUX-LANG-V1 §14.2 leaves reflection as an open question. `JUX-GAPS-ROADMAP.md` §2.4 recommends opt-in compile-time reflection. This section locks that in.

### M.11.1. The Annotation

```jux
@Reflectable
public class User {
    public String name;
    public int age;
    public String email;
}
```

Effects:

- The compiler emits a `Type<User>` value alongside the class. This value is a compile-time constant accessible at runtime.
- The `Type<T>` API exposes:
  - `String name()` — fully qualified type name.
  - `Field[] fields()` — names, types, declared visibility of every instance field.
  - `Method[] methods()` — signatures of public methods.
  - `Annotation[] annotations()` — annotations on the type itself.
- `Field` and `Method` expose names, types, and (for fields) functions to read/write values given an instance.

### M.11.2. Cost

A `@Reflectable` type carries a fixed metadata table (typically a few hundred bytes). The metadata is dead-code-eliminated like any other constant: if the program does not reference the metadata, the linker removes it. Programs that never use reflection pay nothing.

Types **without** `@Reflectable` have no metadata generated at all — their `Type<T>` is unavailable, and reflection-based libraries that need it report a clear error at compile time pointing to the missing annotation.

### M.11.3. Use Cases

- **Serialization libraries.** `@Serializable` (from a future std.json addendum) requires `@Reflectable` and uses the metadata to walk fields.
- **DI containers.** Type-keyed registration uses `Type<T>` as the key.
- **Test frameworks.** `juxc test` reads `@Test`-annotated method metadata via `@Reflectable`.

### M.11.4. What `@Reflectable` Does NOT Do

- Does not enable runtime introspection of types not marked. Callers cannot ask "give me `Type<X>`" for arbitrary `X`; they get a compile error.
- Does not enable arbitrary method invocation by name (no Java-style `Method.invoke`). Reflection is for reading metadata, not for replacing the type system at runtime.
- Does not affect dispatch. Virtual calls and trait selection still go through the normal mechanisms.

This bounded reflection model gives serialization/DI/testing what they need, costs nothing for code that doesn't use it, and never enables the kind of action-at-a-distance that Java's reflection enables.

### M.11.5. Reachability and `@Reflectable`

`@Reflectable` types are **roots** for the reachability analysis (per `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.4.5). They are always emitted regardless of whether any static call reaches them, because runtime reflective access cannot be statically traced.

The reachability pass treats a `@Reflectable` type as having implicit references to:

- Every field's declared type (because reflection enumerates fields).
- Every method's signature (because reflection enumerates methods).
- Every annotation's declared type (because reflection enumerates annotations).
- The type's own `Type<T>` constant (the `TypeInfo` block emitted alongside).

These transitive marks pull the relevant types and methods into the live set even when no static call references them.

**Cost discipline.** The `TypeInfo` constant for a reachable `@Reflectable` type is itself subject to linker-level GC (phase 18). If the `TypeInfo` is never referenced at runtime — i.e., no code path ever calls `Type<MyReflectable>.fields()` or similar — the linker discards the metadata block from the final binary. Net result: a `@Reflectable` type that's declared but unused at runtime costs nothing in the shipped binary; the metadata exists during compilation only, then disappears.

This double-layered reachability matches the design promise: opt-in, pay-for-what-you-use, no global reflection cost.

---

## §M.12 — Resolutions to Acknowledged Inconsistencies

### M.12.1. `spawn`: Keyword OR Library Function

JUX-LANG-V1 §3.2 lists `spawn` as a reserved keyword. JUX-LANG-V1 §10.1.3 uses it as `import std.async.spawn`. Both cannot be true.

**Resolution:** `spawn` is a **library function**, not a keyword. Remove `spawn` from the reserved keyword list. The function `std.async.spawn(f)` is the canonical entry point.

This matches Kotlin (`launch`/`async` are library functions, not keywords) and Rust (`tokio::spawn`, library). It avoids the future-proofing problem of a reserved keyword that means different things in different runtimes.

> **Edit to JUX-LANG-V1 §3.2:** Remove `spawn` from the reserved-keyword list.

### M.12.2. Cross-Module Class Extension (Java-style default)

**Resolution:** Classes follow Java visibility rules for cross-module extension. The `open` keyword is **removed entirely** from the language. Instead:

- A `public` class is extendable from any module that imports it (Java rule).
- An `internal` class is extendable only within the declaring module.
- A package-private class (no visibility modifier) is extendable only within the declaring package.
- A `private` class is not extendable outside its declaring file.
- A `final` (or `const`) class is **not extendable anywhere** — Java's `final` rule.

```jux
public class Shape { ... }              // extendable from any consuming module
public final class Sealed { ... }       // not extendable; final synonymous with const
internal class Helper { ... }           // extendable only within this module
```

The mutation-union widening described in JUX-LANG-V1 §7.4.1 still applies for non-`final` public classes — adding a new override in a downstream module can change the inferred mutability of a base method. This is the Java fragile-base-class trade-off: accepted for the consistency of "Java-readable defaults." Where the trade-off matters (libraries with strict invariants), seal the hierarchy with `sealed class C permits ...` or mark the class `final`.

> **Edit to JUX-LANG-V1 §7.4:** Mark this paragraph in the addendum as the canonical statement: classes are extendable by default unless `final`/`const`, governed by Java visibility rules across modules.

### M.12.3. Mutable Static Thread Safety in Single-Threaded Profiles

JUX-LANG-V1 §7.13 mandates `AtomicInt`/`Mutex<T>` for mutable statics. JUX-LANG-V1 §19.1 notes this is unnecessary in `jux-core` (no threads).

**Resolution:** Per profile.

- `jux-full`, `jux-embedded` with workers enabled: mandatory thread-safe wrapper.
- `jux-embedded` without workers, `jux-core`: a plain `static <type>` mutable is permitted and compiles to an unsynchronized global variable. The compiler emits a warning (`W0960`) reminding the programmer that adding threading later will require migrating to atomics.

> **Edit to JUX-LANG-V1 §7.13:** Append: "In single-threaded profiles (`jux-core`, and `jux-embedded` without workers), mutable statics may be plain types; the wrapper requirement applies only when threads are present."

### M.12.4. Single-Ownership Classes + Virtual Dispatch in `jux-core`

JUX-LANG-V1 §6.9.8 / §19.1 flag this as under-specified.

**Resolution:** In `jux-core`, virtual dispatch is permitted, but:

- Public classes must be either `final` or `sealed` (a non-`final`/non-sealed public class is rejected; the compiler suggests one or the other).
- Move semantics on a class with virtual methods rebases the moved-to binding's vtable pointer to the same vtable; no slicing.
- Calling a virtual method on a moved-from binding is a compile error (the standard borrow-check rule).

> **Edit to JUX-LANG-V1 §6.9.8:** Append: "Virtual dispatch in `jux-core` requires sealed or `final` hierarchies. Unrestricted public extension is rejected to keep the dispatch graph closed and the borrow analysis exact."

---

## §M.13 — `ref` Bindings: Shared References to Value Types

### M.13.1. Motivation

Classes already have reference semantics — two variables naming the same
instance see each other's mutations (§6, §CR). VALUE types — `String`,
primitives, `struct`s, `record`s, arrays — copy on assignment and on
parameter passing. `ref` opts a binding of a value type into the same
shared-reference behavior, without wrapping it in a class:

```java
public class Profile {
    // Both fields point AT a shared String object — never a copy.
    public ref String displayName;
    public ref int counter;
}

void rename(ref String name) {
    name = "renamed";           // the CALLER's object changes
}

public void main() {
    ref String a = "first";
    ref String b = a;            // b aliases a's object
    b = "second";
    print(a);                    // "second" — shared, not copied
    rename(a);
    print(b);                    // "renamed" — same object throughout
}
```

### M.13.2. Semantics

- `ref T` is a binding mode, not a distinct type: the expression type of a
  `ref T` binding is `T` everywhere — reads produce a `T` value, method
  calls dispatch on `T`, and `typeof` reports `T`.
- **Initialization.** Initializing a `ref` binding from a plain `T` value
  creates a NEW shared object holding that value. Initializing (or
  argument-passing) from another `ref T` binding ALIASES the same object.
- **Assignment stores through.** `x = v` on a `ref` binding writes `v`
  into the shared object — every alias observes it (C++ reference /
  JavaFX-property mental model; there is no rebinding form in Phase 1).
- **Parameters.** A `ref T` parameter receives the caller's object when
  the argument is itself `ref`; a plain-value argument is wrapped into a
  fresh object (the callee's writes are then invisible to the caller —
  pass a `ref` binding when you want write-through).
- **Returns.** `ref` return types are deferred (Phase 1 rejects them).
- `ref` on a CLASS-typed binding is accepted and meaningless (classes are
  already references); the compiler is free to warn (reserved W0490).
- `ref` bindings are task-local exactly like class instances; the E0702
  spawn-capture gate applies.

### M.13.3. Lowering (Phase 1)

`ref T` slots lower to `Rc<RefCell<T_rust>>`:

| Site | Lowering |
|------|----------|
| `ref T x = <plain value>` | `let x = Rc::new(RefCell::new(v));` |
| `ref T x = <ref binding>` | `let x = y.clone();` (handle share) |
| read in value position    | `x.borrow().clone()` (statement-scoped) |
| `x = v` (store-through)   | `{ let __jux_v = v; *x.borrow_mut() = __jux_v; }` |
| `ref` field               | field type `Rc<RefCell<T>>`, same rules |
| `ref` parameter           | `Rc<RefCell<T>>`; ref-arg → `.clone()`, plain arg → wrap |

The statement-scoped borrow discipline (§CR.4.1) applies — reads clone
out, writes evaluate the RHS before taking the cell borrow.

### M.13.4. Grammar

```
ref-type   = 'ref' type
```

`ref` is valid at the START of a type in field declarations, local
variable declarations, and parameter declarations. It joins the reserved
keyword table (§3.2 / grammar §A.1.3). Nesting (`ref ref T`), `ref` array
ELEMENTS (`ref T[]`), and `ref` generic arguments (`List<ref T>`) are
rejected in Phase 1.

> **Phase-1 implementation status (2026-06-12):** fully implemented —
> locals, parameters, AND fields (`examples/ref_bindings.jux` +
> `examples/ref_fields.jux`, `ref_bindings` e2e). Passing a `ref`
> FIELD into a `ref` parameter aliases the field's object.

---

## §M.14 — Parameters: Comprehensive Reference

Parameters accumulated their modifiers across several addenda — `out` (§M.4),
`ref` (§M.13), defaults (§S.1.3), varargs (§E), and `final` (grammar §A.2.4) — but
two of them were never given prose, and the legal *combinations* were never pinned
down. This section is the single normative reference: it defines `final` parameter
semantics, introduces `weak` parameters, states the default-parameter ordering rule,
and gives the complete modifier-combination matrix. It introduces no new surface
beyond `weak` on a parameter; everything else consolidates rules stated elsewhere.

### M.14.1. The Two Modifier Axes

A parameter carries modifiers on **two independent axes**:

1. **Param-mode** — the slot *before* the type (`param-mode = binding-immut | 'out'`,
   grammar §A.2.4): `final`/`const` (an immutable binding) or `out` (a write-back
   slot, §M.4). The two are mutually exclusive — there is one slot.
2. **Binding mode** — a prefix that is part of the *type* (`ref-type = 'ref' type`,
   §M.13.4; `weak-type = 'weak' type`, §M.14.3): `ref` (a shared reference to a
   value type) or `weak` (a weak reference to a class). At most one binding-mode
   prefix may appear, and it is also mutually exclusive with `out`.

Because the two axes are orthogonal, `final` composes with `ref` and `weak`
(`final ref String x`, `final weak Node n`): the binding is both immutable (cannot
be reassigned in the body) and shared/weak. The full matrix is §M.14.5.

```
param        = annotation* param-mode? type identifier ('=' expression)?
param-mode   = binding-immut | 'out'          -- 'final'/'const', or 'out'
type         = … | ref-type | weak-type | …    -- binding-mode prefixes live here
ref-type     = 'ref' type                       -- §M.13.4
weak-type    = 'weak' type                       -- §M.14.3 (new)
```

### M.14.2. `final` Parameters

A `final` (equivalently `const` — synonyms, §A.2.4) parameter is an **immutable
binding**: the parameter name cannot be reassigned within the function body. It
mirrors a `final`/`const` local and Java's `final` parameter.

```java
public int distance(final int x, final int y) {
    x = 0;                      // ERROR E0464 — cannot reassign a final parameter
    return abs(x) + abs(y);     // reading is fine
}
```

- **Reassignment is the only thing forbidden.** `final` constrains the *binding*,
  not the object: a `final` parameter of a class type may still have its fields
  mutated (the reference is fixed, the target is not), exactly like a `final` local.
- **It is orthogonal to `ref`/`weak`/default/varargs** (§M.14.5) and mutually
  exclusive with `out` (an `out` parameter must be assigned — §M.4 — so making it
  `final` is contradictory: `E0944`).
- **Lowering.** A `final` parameter lowers to a Rust binding with no `mut`. (An
  ordinary parameter gains `mut` only when the body reassigns it; a `final`
  parameter that is reassigned has already been rejected by `E0464`, so suppressing
  `mut` is always sound.)

### M.14.3. `weak` Parameters

A `weak` parameter is a **weak reference to a class object** — the parameter form of
the `weak` field (§6.5). It does not keep the referent alive; the referent may have
been freed by the time the function runs.

```java
public void onTick(weak Sprite owner) {
    var s = owner.get();        // .get() → Sprite? — may be null if the owner died
    if (s != null) {
        s.advance();
    }
}
```

- **Type restriction.** The pointee `T` must be a plain (non-generic-applied) class,
  exactly as for `weak` fields. `weak` on a primitive, array, nullable, interface,
  record, enum, type parameter, or generic-applied class is `E0455` (the same code
  weak fields use; its scope now reads "field **or parameter** type").
- **Reads require `.get()`.** A `weak` binding's strong view is reached only through
  `.get()`, which yields `T?` (a nullable you must null-check); reading the parameter
  bare is `E0456` (shared with weak fields). This is the §6.5 rule applied to a
  parameter.
- **No default value.** A `weak` parameter may not carry a default (`weak T x = …`)
  in Phase 1 — a defaulted weak reference would bind a literal that is immediately
  unreferenced and so always dead. This is `E0466` (§M.14.5).
- **Lowering.** A `weak T` parameter lowers to `std::rc::Weak<RefCell<T_Inner>>`,
  matching the weak-field storage (§6.5). At the call site, a class argument is
  **downgraded** to a weak handle (`std::rc::Weak::clone` of an existing weak, or
  `std::rc::Rc::downgrade(&arg)` of a strong reference). `.get()` lowers to the same
  `.upgrade().map(…)` form weak fields use.
- **Composition.** `final weak T` (an immutable weak binding) is allowed; `weak` is
  mutually exclusive with `ref`, `out`, and varargs (§M.14.5).

### M.14.4. Default-Parameter Ordering

A parameter with a default value may not precede a parameter without one — defaults
fill *trailing* omitted arguments, so a non-defaulted parameter after a defaulted one
could never be omitted and the position would be unreachable.

```java
void connect(String host, int port = 80, int timeout = 30) { … }   // OK
void bad(int port = 80, String host) { … }                          // ERROR E0467
```

This is `E0467`. (The pre-existing `E0449` — a default expression that references
another parameter — is unchanged and independent.) Defaults compose with `final`
and `ref` (`final String s = "hi"`, `ref int n = 0`) but not with `weak` or `out`.

### M.14.5. The Combination Matrix

`final` is orthogonal (binding immutability); `ref`/`weak` are mutually-exclusive
binding modes on the type; `out` is a param-mode; varargs (`T…`) binds an array.

| Combination                         | Verdict | Code |
|-------------------------------------|---------|------|
| `final T`, `final T = d`            | allowed | — |
| `ref T`, `ref T = d`, `final ref T` | allowed | — |
| `weak T` (T a class), `final weak T`| allowed | — |
| `T…`, `final T…`                    | allowed | — |
| `final` + `out`                     | rejected | `E0944` |
| `out` + `ref`, `out` + `weak`       | rejected | `E0944` |
| `out` + varargs / default / on ctor | rejected | `E0944` |
| `ref` + `weak` (`ref weak T`)       | rejected | `E0466` |
| `ref T…` / `weak T…` (binding-mode varargs) | rejected | `E0466` |
| `weak T = d` (weak + default)       | rejected | `E0466` |
| `weak T` where T is not a class     | rejected | `E0455` |
| `weak` parameter read without `.get()` | rejected | `E0456` |
| reassigning a `final` parameter     | rejected | `E0464` |
| defaulted param before a plain one  | rejected | `E0467` |

`E0466` is the umbrella for an **invalid parameter binding-mode combination**:
combining `ref` with `weak`, applying `ref`/`weak` to a varargs parameter, or giving
a `weak` parameter a default. `ref`/`weak` on an array ELEMENT or generic argument
remains barred by §M.13.4 / §M.14.3; a varargs parameter binds a `T[]` array, so a
binding-mode varargs is the array-element case and falls under the same bar.

### M.14.6. Diagnostics (new + generalized)

| Code    | Condition                                                                 |
|---------|---------------------------------------------------------------------------|
| `E0464` | Reassignment of a `final`/`const` binding (parameter or local).            |
| `E0466` | Invalid parameter binding-mode combination (`ref`+`weak`, `ref`/`weak` on varargs, `weak`+default). |
| `E0467` | A defaulted parameter precedes a non-defaulted parameter.                  |
| `E0455` | (generalized) `weak` modifier on a non-class field **or parameter** type.  |
| `E0456` | (generalized) `weak` field **or parameter** read without `.get()` (weak fields also: with an initializer). |
| `E0944` | (generalized) Misuse of a write-back/immut modifier: `out` combined with `final`/`ref`/`weak`/varargs/default, or on a constructor parameter. |

---

## §M.15 — Nullable Type Parameters and Nested Nullability

This section defines how `T?` interacts with primitives and with generic type
parameters. It supersedes the stale "primitives cannot be nullable" text and gives
nested nullability a normative home. See also `ERRATA.md` E5 and JUX-LANG-V1.md §7.10.

### M.15.1. Nullable primitives are well-formed

`T?` is well-formed for ANY type `T`, reference or primitive. `int?`, `bool?`,
`char?`, `float?`, and the unsigned / width-explicit numerics are all valid. A
nullable primitive lowers to a stack `Option<T>` with no boxing: `None` is a
discriminant, so a null primitive costs no heap allocation. The two spellings `T?`
and `Option<T>` denote the same shape (§K.3.1). There is no reference-only
restriction; nullability is uniform across the type system.

```java
int? maybe = null;            // valid; lowered to Option<isize>
var list = new ArrayList<int?>();   // collection of nullable primitives
list.add(1); list.add(null);
var first = list.get(0) ?: -1;      // Elvis default
```

### M.15.2. Nested nullability nests (it does not flatten)

When a generic parameter `T` is instantiated with a nullable type, an inner `T?`
produces TWO nullability layers, NOT one. Jux does NOT flatten `T?` to `T` the way
Kotlin collapses `T??`. The reason is the lowering: a generic `T?` parameter is a
single `Option<T>` slot, and instantiating `T = int?` makes that slot
`Option<Option<isize>>`. Collapsing one layer is not expressible under monomorphized
generics, so the language nests.

```java
public void f<T>(T? val) { ... }   // slot is Option<T>
f<int?>(5);      // T = int?  =>  slot Option<Option<isize>>; arg lifts to Some(Some(5))
f<int?>(null);   // outer layer absent  =>  None
```

A non-null argument that is one layer shallower than the instantiated slot is lifted
into `T` automatically: `f<int?>(5)` passes `Some(Some(5))`. A `null` argument fills
only the outer layer (`None`). The choice of nest over flatten is observationally
transparent for the common test `val == null` (true exactly when the outer layer is
absent), so user code rarely needs to reason about the inner layer.

### M.15.3. Comparing a non-nullable type parameter to `null`

For a BARE non-nullable type parameter `T` (not `T?`), `val == null` is statically
`false` and `val != null` is statically `true`: a non-nullable value can never be
null. The compiler folds the comparison to the constant (still evaluating `val` for
any side effects) rather than emitting an `Option` test, since a bare `T` is not
`Option`-shaped.

```java
public void g<T>(T val) {
    print(val == null ? 1 : 0);   // always 0; g<int>(5) prints 0
}
```

---

## Summary

This addendum closes every dangling reference and acknowledged inconsistency identified in the gap analysis:

| Item                                         | Section | Resolution                                  |
|----------------------------------------------|---------|---------------------------------------------|
| `init` blocks                                 | §M.1    | Run once before constructor body (ERRATA E2) |
| `yield` keyword                               | §M.2    | Generator functions; `Iterator<T>` / `Stream<T>` return |
| `@Derive`                                     | §M.3    | Built-in for fixed interface set             |
| `out` parameters                              | §M.4    | Function writes through, caller binds via `out` |
| `record.with(...)`                            | §M.5    | Auto-generated wither for records            |
| Range `step`                                  | §M.6    | Modifies range; `SteppedRange<T>`            |
| Property accessors (`get`/`set`)              | §M.7    | Asymmetric visibility shorthand + custom    |
| Method references (`::`)                      | §M.8    | Static, instance-bound, unbound, constructor |
| Nested classes                                | §M.9    | Static-nested only; no inner/anonymous/local |
| Foundational interfaces                       | §M.10   | Full contracts; `Iterator` resolved to `next() -> T?` |
| `@Reflectable`                                | §M.11   | Opt-in compile-time reflection metadata     |
| `ref` bindings                                | §M.13   | Shared references to value types (`Rc<RefCell>`) |
| Parameter modifiers (final/weak/defaults/combos) | §M.14 | `final` semantics, `weak` parameters, default ordering, combination matrix |
| Nullable type parameters                      | §M.15   | Nullable primitives valid; nested `T?` nests (no flatten); bare-`T` `== null` is constant |
| `spawn` keyword/function                      | §M.12.1 | Library function only                       |
| Cross-module class extension                   | §M.12.2 | Java-style: extendable by default, `final`/`const` opts out |
| Static thread safety per profile              | §M.12.3 | Per-profile rule                            |
| `jux-core` virtual dispatch                   | §M.12.4 | Sealed required                             |

Combined with the grammar, semantics, and layout/ABI addenda, the language now has a complete enough specification for a Phase 1 implementation: every reference resolves, every behavior is defined, every escape hatch is bounded.

---

*End of missing-definitions addendum. When this lands, JUX-LANG-V1.md §3, §6, §7, §9, §10, §14, and §19 reference §M.x for items previously marked open.*
