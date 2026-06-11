# Jux Spec Addendum — Execution Semantics

**Status:** Proposed insertion. Replaces the silence in JUX-LANG-V1 on numeric overflow, string/Unicode handling, evaluation order, initialization order, drop order, and the concurrency memory model.

**Insertion points:**
- New §S.1 ("Evaluation Order")
- New §S.2 ("Numeric Semantics")
- New §S.3 ("String and Unicode Semantics")
- New §S.4 ("Initialization Order")
- New §S.5 ("Destruction Order")
- New §S.6 ("Concurrency Memory Model")
- Small clarifying edits to §5.1 (primitives), §5.5 (String), §6.6 (Destructors)

The text below is written to drop directly into the dossier as a new chapter §S between §6 and §7.

---

## Design Philosophy (Non-Normative)

Jux compiles to native code through LLVM. That means execution semantics must be specified at the level of **what the machine actually does**, not at the level of an abstract VM. C and C++ get this wrong by leaving things "implementation-defined" or "undefined" — both invite the compiler to surprise the user. Rust gets this right by specifying overflow, conversion, and ordering rules even when they cost a few cycles in debug builds. Jux follows Rust here:

- **Defined behavior is the default.** No "undefined behavior" categories from C/C++. Either the program does something specific or the compiler rejects it.
- **Wrap, trap, or check, but never UB.** Every numeric operation has a defined result.
- **One semantics per profile.** Where two reasonable semantics exist (e.g., overflow check in debug, wrap in release), the rule is fixed per build profile so reasoning is local.
- **Java's intuition where it costs nothing.** When Java's behavior is unambiguous and matches user expectations, Jux matches it.

The user-visible promise: **a Jux program that compiles and runs has no memory corruption, no data races, no NaN-vs-NaN surprises, and no order-of-evaluation traps.** Inside `unsafe { }` blocks (specified in `JUX-LAYOUT-ABI-ADDENDUM.md`) this promise is suspended in exchange for raw-pointer access — but only there.

---

## §S.1 — Evaluation Order

### S.1.1. Sub-expression Order

Every sub-expression is evaluated **left-to-right**. There is no implementation-defined or unspecified order. Specifically:

- In `f(g(), h(), i())`, `g()` runs before `h()`, which runs before `i()`, which runs before `f`'s body.
- In `a.b.c`, `a` is evaluated, then `.b` is read, then `.c` is read. (For class instances this matters because each `.` may take a borrow.)
- In `arr[index] = value`, the receiver `arr` is evaluated, then `index`, then `value`, then the assignment is performed via `operator[]=`.
- In `a + b * c`, `a` is read, then `b`, then `c`, then `b * c` is computed, then the result is added to `a`.
- In compound assignment `a += b`, the LHS is evaluated **once**: `a += b` is `a = (a) + (b)` where `a` is read once, not twice. (C++ users: this matches `a += b` as a single operator call, not the desugaring trap.)
- Arguments to a constructor (`new Foo(a, b)`) are evaluated left-to-right before `Foo`'s constructor body runs.

### S.1.2. Short-Circuiting

`&&` and `||` short-circuit. The right operand is evaluated **only if** the left did not determine the result. The Elvis operator `?:` short-circuits: `a ?: b` evaluates `b` only when `a` is null. Safe-navigation `a?.b` evaluates `a` once; if `a` is null, the result is null and `b` is not evaluated.

Operator overloading cannot override `&&`, `||`, `?:`, or `?.` (per JUX-LANG-V1 §7.14.2). User-defined types cannot lose short-circuit behavior.

### S.1.3. Default Argument Evaluation

When a call omits a parameter that has a default-value expression, the default is evaluated **at the call site**, in left-to-right order with explicit arguments, **once per call**. This avoids the Python "shared mutable default" foot-gun:

```jux
public void log(String msg, List<String> tags = new List<>()) {
    tags.add(msg);
    print(tags);
}

log("a");   // tags = []
log("b");   // tags = []   — fresh list each call
```

Default-argument expressions can reference earlier parameters of the same function (`f(int n, int[] buf = new int[n])`), but not later ones. They are otherwise plain expressions evaluated in the caller's scope.

> **Phase-1 implementation note.** The current compiler lowers a default by cloning its expression into each omitting call site, where parameter names aren't in scope — so defaults that reference another parameter are rejected with `E0449` for now. The earlier-parameter form above becomes legal when the temp-hoisting lowering lands. Additionally, when named arguments are written out of declaration order, Phase 1 evaluates them in parameter-slot order rather than the call-site lexical order promised by §S.1.4; only side-effecting reordered arguments can observe the difference.

### S.1.4. Named-Argument and Variadic Order

When a call mixes positional and named arguments, evaluation order follows **call-site lexical order**, not parameter declaration order:

```jux
public void connect(String host, int port = 80, int timeout = 30) { ... }

connect("a.com", timeout: f(), port: g());
//                       ^^^ runs first
//                                  ^^^ runs second
```

This matches user reading order. The compiler then re-orders the values into the function's parameter slots.

For variadic parameters, all variadic arguments are evaluated in source order before being packaged into the synthesized array.

### S.1.5. Constructor Initializer Order

Inside a class (order per `ERRATA.md` E2 — matches Java):

1. The constructor's explicit or implicit `super(...)` / `this(...)` call resolves first (see §S.4) — the parent's construction completes before any code of this class runs.
2. Field initializer expressions (`private int x = expr;`) are evaluated **in textual order**.
3. Then `init { }` blocks run, in textual order — **before** the constructor body.
4. Then the constructor body runs.

This deterministic order eliminates the Java footgun where field initializer order across multiple constructors becomes confusing. Init blocks run before the body so the body can rely on every field — initializer- or init-block-assigned — being in its final pre-body state; an init block may reference inherited fields because the parent has already constructed.

> **Edit to JUX-LANG-V1 §7.3:** The "Primary constructor with init block" example shows the body assigning to fields. With this addendum, field initializers run before the body, so `this.name = name` overwrites a default `""` rather than a fresh field. Update the example to use field initializer expressions where the field has a constant default, and constructor-body assignment only where the value is computed from parameters.

---

## §S.2 — Numeric Semantics

This section gives concrete behavior for every primitive numeric operation. JUX-LANG-V1 §5.1 lists the primitive types but is silent on overflow, division-by-zero, and conversion.

### S.2.1. Integer Overflow

For each profile:

| Profile         | Debug build               | Release build         |
|-----------------|---------------------------|-----------------------|
| `jux-full`      | Panic on overflow         | Wrap (two's complement) |
| `jux-embedded`  | Panic on overflow         | Wrap                  |
| `jux-core`      | Wrap                      | Wrap                  |

Panic means: abort via the panic handler (see §S.7) after reporting the condition and source location. Per `ERRATA.md` E1, panics are **not catchable from Jux source** — they are not exceptions. Code that wants a recoverable overflow uses the checked methods below, which return a `Result`. Wrap means: produce the bit-equivalent result modulo `2^N`.

This matches Rust's overflow story almost exactly. Programmers who *want* wrapping behavior unconditionally use the explicit wrapping operators:

```jux
var sum = a +% b;     // wrapping add, never panics
var diff = a -% b;    // wrapping sub
var prod = a *% b;    // wrapping mul
var shl  = a <<% n;   // wrapping shift left
```

Programmers who want **checked** arithmetic that returns a `Result<T, ArithmeticException>` use the methods on the numeric type:

```jux
var r1 = a.checkedAdd(b);     // Result<int, ArithmeticException>
var r2 = a.saturatingAdd(b);  // saturates at MAX/MIN
```

The wrapping (`+%`, `-%`, `*%`, `<<%`, `>>%`) operators are added to the lexical grammar; the `%`-suffixed family is reserved for wrapping arithmetic and is not user-overloadable.

### S.2.2. Integer Division and Remainder

- `a / 0` for integer `a` panics in all profiles — an uncatchable abort via the panic handler (`ERRATA.md` E1). There is no "implementation-defined" or "undefined" case. Code that wants a recoverable division uses `a.checkedDiv(b)` (`Result`-shaped).
- `a % 0` panics with the same diagnostic.
- `int.MIN_VALUE / -1` (the only signed-overflow case for division) panics in debug, wraps to `int.MIN_VALUE` in release. Same for `%`.
- `a / b` truncates toward zero. `a % b` has the sign of `a` (the C99/Java/Rust convention). `(a / b) * b + (a % b) == a` always holds for valid `a`, `b`.

### S.2.3. Floating-Point Semantics

Floats follow IEEE 754-2008 binary32 and binary64 exactly:

- `NaN == NaN` is `false`. So is every other comparison involving NaN. `==` returns `false`, `!=` returns `true`, `<`, `<=`, `>`, `>=` return `false`.
- `+0.0 == -0.0` is `true`. They produce the same hash code.
- Division by zero produces `+Inf`, `-Inf`, or `NaN` per IEEE 754, never panics.
- Float overflow produces `±Inf`, never panics.
- `a / 0.0` for `a == 0.0` is `NaN`.
- The default rounding mode is **round-half-to-even** (banker's rounding, IEEE default).
- The `==` operator on floats is the IEEE bit-equality after handling `+0.0/-0.0` (so `+0.0 == -0.0`). For *exact* bit equality (NaN-payload-aware), use `Double.bitsEqual(a, b)`.

For hash consistency: floats hash by their canonicalized bit pattern (NaN → a single canonical NaN, `-0.0` → `+0.0`'s hash). This is what `Map<double, V>` uses.

The `<=>` operator on floats uses the IEEE total-order predicate: `-Inf < ... < -0.0 < +0.0 < ... < +Inf < NaN`. This gives `<=>` a total order even though `<` does not. Uses: sorting, sorted maps. The `<` operator retains the partial-order IEEE behavior.

### S.2.4. Numeric Conversions

Conversions between numeric types use the `as` operator (see `JUX-GRAMMAR-ADDENDUM.md` §A.5):

| Conversion                           | Semantics                                              |
|--------------------------------------|--------------------------------------------------------|
| Smaller signed → larger signed       | Sign-extend.                                           |
| Smaller unsigned → larger unsigned   | Zero-extend.                                           |
| Smaller unsigned → larger signed     | Zero-extend; always preserves value.                   |
| Smaller signed → larger unsigned     | Sign-extend, then reinterpret. May produce a large positive value if input was negative. |
| Larger → smaller (signed or unsigned)| Truncate to low N bits. May change sign.               |
| Same-width signed ↔ unsigned         | Bit-preserving reinterpret.                            |
| Integer → float                      | Round-half-to-even. Out-of-range → ±Inf.               |
| Float → integer                      | Truncate toward zero. NaN → 0. Out-of-range → saturate to MIN/MAX. |
| Float64 → Float32                    | Round-half-to-even.                                    |
| Float32 → Float64                    | Exact (no rounding).                                   |

All numeric `as` conversions are **infallible at runtime** — they always produce a value of the target type, never throw, never panic. Programmers who want to detect lossy conversion use the explicit checked methods:

```jux
long n = 1L << 40;
int i = n as int;                      // truncates silently → some int
var safe = n.toInt();                  // Result<int, ArithmeticException>
var saturated = n.saturatingToInt();   // returns int.MAX_VALUE
```

### S.2.5. Bitwise Operations

- `<<`, `>>` shift by a count taken modulo the width of the LHS type (the same rule as Java for `int`/`long`, applied uniformly here).
- `>>` is **arithmetic** (sign-extending) on signed integer types and **logical** (zero-extending) on unsigned types. There is no separate `>>>` operator — the type determines the behavior. This eliminates the Java footgun where `int >> n` accidentally sign-extends.
- `~`, `&`, `|`, `^` operate on the binary representation of the value at the type's natural width.
- Bitwise operators on `bool` are not permitted (`E0420`); use logical operators.
- Bitwise operators on `char` are permitted and operate on the 32-bit Unicode scalar value.

### S.2.6. Mixed-Type Arithmetic

Jux does **not** silently promote operands to a common type. `int + long` is a compile error (`E0410`). The user writes `(a as long) + b` or `a + (b as int)` explicitly. This eliminates Java's silent promotion surprises.

The exceptions (where promotion is automatic and well-defined):

- Untyped integer literals are coerced to whichever numeric type the surrounding context demands, when the value fits. `long x = 1` works because `1` adapts to `long`.
- Untyped float literals adapt to `float` or `double` similarly.
- A typed literal (e.g., `1L`, `2.0f`) does not adapt; mixing it with another type requires an explicit `as`.

> **Edit to JUX-LANG-V1 §5.1:** Add a paragraph after the primitive types table: "Arithmetic between two distinct numeric types requires an explicit `as` cast. Untyped integer and float literals automatically adopt the surrounding type when the value fits. See §S.2.6."

---

## §S.3 — String and Unicode Semantics

This section makes precise what JUX-LANG-V1 §5.5 calls "UTF-8 string (reference type, immutable)."

### S.3.1. Internal Representation

A `String` value holds a UTF-8-encoded sequence of bytes plus a length in bytes. The bytes are **always** valid UTF-8 — invalid byte sequences cannot exist in a `String`. Constructors that take raw bytes (e.g., `String.fromBytes(byte[])`) validate and throw `EncodingException` on failure.

A `String` is immutable. Operations that "modify" return a new `String`. Implementations may share storage between substrings as long as the immutability promise holds.

### S.3.2. Indexing and Iteration

Jux strings expose **two** units of access — bytes and characters (Unicode scalar values) — and force the programmer to pick:

```jux
var s = "héllo";

s.length;                  // ERROR (E0510): pick `byteLength` or `charLength`

s.byteLength;              // 6   (UTF-8 bytes)
s.charLength;              // 5   (Unicode scalar values)

s[0];                      // ERROR (E0511): pick `bytes()` or `chars()`

s.bytes()[0];              // 104 (the byte 'h')
s.chars().nth(0);          // 'h' (a char, i.e. a Unicode scalar)
```

This is more verbose than Java's `s.length()` (UTF-16 code-unit count, often confusing) and Python 3's `s[i]` (sometimes by code point, depending on storage), and it is intentional: every common bug in this area comes from a programmer believing they had one unit and actually having another. Jux refuses to guess.

For most programs, the right choices are:

- `s.charLength` for "how many characters" (display purposes, user-facing length).
- `s.byteLength` for "how many bytes" (I/O, buffer sizing, network framing).
- Iteration: `for (var c : s.chars()) { ... }` (most common); `for (var b : s.bytes()) { ... }` for byte-level work.
- Slicing: `s.substring(start: 1, end: 3)` is **char-indexed** by default; `s.substringBytes(...)` works on byte indices and throws `EncodingException` if a slice would split a multi-byte character.

There is **no random byte indexing that returns a `char`**. Random char indexing is `O(N)` (Jux does not maintain a char-offset index by default); the iterator forms are O(N) total, which is what most use cases want.

### S.3.3. Equality, Hashing, Comparison

- `s == t` is byte-by-byte equality on the UTF-8 representation. Equal strings have equal bytes; two strings that look identical but are encoded with different normalization forms are **not** equal.
- `s.operator hash()` is over the bytes, with a documented stable algorithm (FxHash for Phase 1; subject to revision but always documented).
- `s.compareTo(t)` is byte-wise lexicographic order on the UTF-8 bytes. This coincides with Unicode-code-point order for valid UTF-8.
- For locale-aware comparison or Unicode-normalized comparison, use the explicit APIs in `std.string.collator` and `std.string.normalize` (Tier 1; not in `core.string`).

### S.3.4. The `char` Type

`char` is a 32-bit Unicode scalar value (per JUX-LANG-V1 §5.1). The valid range is `0x0..0xD7FF` and `0xE000..0x10FFFF`; values in the surrogate range `0xD800..0xDFFF` are not valid `char`s. Constructing a `char` from an out-of-range integer panics in debug, wraps to a valid scalar via masking in release (the same overflow policy as integers, §S.2.1).

Arithmetic on `char` is permitted (`'a' + 1 == 'b'`). The result is a `char` after range-checking. Mixing `char` with `int` requires `as` (per §S.2.6).

### S.3.5. String Interpolation

`$"...$expr..."` and `$"...${expr}..."` (per JUX-LANG-V1 §3.4) desugar to `String.concat(...)` of the parts and the stringified interpolated values. Each interpolated value is converted via its `operator string` (or its primitive-stringification function). The compiler may emit a single `StringBuilder`-backed sequence to avoid intermediate allocations; this is an implementation detail.

`$null` interpolation produces the literal text `"null"`. Non-null nullable values render their underlying string. Interpolating an expression of type `void` is a compile error (`E0512`).

> **Edit to JUX-LANG-V1 §5.5:** Replace the table row for `String` with: "`String` — UTF-8 string (reference type, immutable). Indexing requires explicit `bytes()` or `chars()`; see §S.3."

---

## §S.4 — Initialization Order

A correct native language has to specify when things run before `main`. Jux's rule: **lazy, deterministic, and observable**.

### S.4.1. Static Initializers

A class's `static { }` blocks (and `static const` field initializers) run **once**, on the **first observable use** of the class. First observable use is any of:

- Reading or writing a static field.
- Calling a static method.
- Constructing an instance.
- Using the class in a `=>` (type test) or `as` cast.

Mere mention of the class name in a type position (e.g., declaring a variable `Foo x;` without using `Foo`) is **not** observable use.

The initializer runs to completion before any other thread observes the class as initialized. Synchronization is implicit — concurrent first-uses block on a single initializer execution.

If a static initializer throws an exception, the class is marked **erroneous**; subsequent uses throw `ExceptionInInitializerError` immediately. The initializer is never retried. (Same as Java; same as Swift type initializers.)

### S.4.2. Static Initializer Order Across Classes

When class `A`'s initializer triggers class `B`'s initializer (e.g., it reads `B.constant`), `B` initializes fully before `A` continues. Cycles are detected at compile time when `A → B → A` is statically determinable; otherwise at runtime, the second entry into a partially-initialized class returns the current (partial) state — the same trap Java has. In practice, sealed cycles in the import graph are forbidden by the module system (per JUX-LANG-V1 §4.3); cycles within a single module are linted (`W0530`).

### S.4.3. Module Initializers

Modules without `@async-init` (per JUX-LANG-V1 §10.1.7) have no module-level initialization separate from their classes' static initializers. Module-level `const` declarations are constant-folded at compile time; module-level `var` is **not** permitted at the top level (only entry-file top-level statements per §7.15).

Modules with `@async-init` have an asynchronous module initializer that runs once before any of their exports are first used. Dependents wait for the initializer to complete before resolving any imported name. This matches ECMAScript's top-level-await module semantics.

### S.4.4. Constructor Initialization Sequence

For `new C(args)`:

1. **Allocate.** Memory for the instance is allocated. Layout per `JUX-LAYOUT-ABI-ADDENDUM.md`. The refcount is set to 1 (in `jux-full`).
2. **Resolve super-call.** The constructor's first statement is either `this(...)` or `super(...)` (explicit) or implicit `super()` (per JUX-LANG-V1 §A.2.4 / `JUX-GRAMMAR-ADDENDUM.md`).
   - For `super(...)`: recursively initialize the superclass portion. The vtable pointer is set to point at C's vtable (not the superclass's) — so virtual calls to overridden methods from inside the superclass constructor dispatch to **C**'s overrides. Java's rule.
   - For `this(...)`: initialize via the named alternative constructor; that constructor's chain runs to completion, then we return to step 5.
3. **Field initializers.** Field initializer expressions of class C are evaluated **in textual order** and assigned to their fields.
4. **Init blocks.** All `init { }` blocks of class C are run, in textual order (per `ERRATA.md` E2 they run **before** the constructor body, after the parent's construction completes — Java's order).
5. **Constructor body.** The body of `new(...)` runs.
6. **Done.** The reference is returned to the caller.

If any step throws:

- An exception during steps 2–5 cancels initialization. The partially-initialized instance is destroyed (its `drop` does **not** run, because it was never fully constructed). Already-fully-constructed sub-portions (the superclass) **do** drop, in reverse construction order.
- Drop-order details are in §S.5.

### S.4.5. Field Definite-Assignment

Every non-nullable, non-`weak` field of a class must be definitely assigned by the end of step 5. The compiler verifies this by flow analysis (`E0600` per `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4 if a field could remain unassigned on some path through the constructor and init blocks).

A field with a textual initializer (`private String name = "";`) is trivially definitely assigned. A field without one must be assigned in every constructor (after `super(...)` resolves and before the constructor returns), and may not be read before being assigned.

`weak` fields default to null (per JUX-LANG-V1 §6.5). Nullable fields default to null. Other fields require explicit assignment or an initializer.

---

## §S.5 — Destruction Order

JUX-LANG-V1 §6.6 introduces destructors. This section gives them precise semantics.

### S.5.1. When Destructors Run

A `drop { }` block runs when a value's lifetime ends. Lifetimes end:

- For a local variable: when its enclosing block exits, in **reverse declaration order** within that block (the last-declared variable drops first).
- For a function parameter: when the function returns or unwinds, in **reverse declaration order**.
- For a temporary expression result: at the end of the **enclosing statement**.
- For a field of an aggregate (struct, record, class): when the enclosing aggregate is dropped.
- For a `class` instance reference: when the **last strong reference is released** (refcount hits 0). Weak references do not extend lifetime.
- For an element of a collection: when the collection is dropped, or when the element is removed from the collection (`list.remove(i)`).

### S.5.2. Drop Order Inside an Aggregate

When a struct, record, or class instance is destroyed:

1. The aggregate's own `drop { }` block runs (if declared).
2. After the block returns, fields are destroyed in **reverse declaration order**.

For a class hierarchy `Subclass extends Parent`:

1. `Subclass`'s `drop { }` block runs.
2. `Subclass`'s fields are destroyed in reverse declaration order.
3. `Parent`'s `drop { }` block runs.
4. `Parent`'s fields are destroyed in reverse declaration order.
5. (Repeat up the hierarchy until `Object` or the root.)

This is the inverse of construction (§S.4.4) and matches the C++ destruction order that programmers' intuition expects.

### S.5.3. Exception Inside a Destructor

If a `drop { }` block throws an exception:

- In `jux-full` and `jux-embedded` (with exceptions enabled): the exception propagates **after** the rest of the cascading drops complete. If multiple drops throw during a single cascade, the exceptions are aggregated into a `MultipleDropException` containing all of them. The first exception raised is the `cause`; subsequent ones are `suppressed`.
- In `jux-core`: `drop` blocks may not throw. The compiler enforces this (`E0610`) and a `drop` block that calls a `throws` function must `try`-catch internally.

This is stricter than C++ (which makes drop-from-drop *undefined behavior*) and less restrictive than Rust (which forbids `panic` in `drop` from causing UB by ad-hoc means). The promise: a Jux program never has corrupt state from a destructor failure. The cost: a small runtime structure for aggregating exceptions during unwind.

### S.5.4. Drop During Move

Move semantics (per JUX-LANG-V1 §6.4) **do not** call `drop`. The destination becomes the new owner of the moved-from value's storage, and only one drop runs — at the destination's end of life. The source binding becomes inaccessible (the borrow checker enforces this, per §6.4) and is not dropped.

### S.5.5. Drop During Refcount Decrement

For `class` instances under refcounting, `drop` runs on the thread that drops the **last strong reference**. The order of finalization across multiple objects releasing concurrently is not defined globally — only within a single thread's drop chain.

The runtime guarantees `drop` runs **at most once** per instance. (Cyclic graphs of strong references would prevent any drop from running; this is why `weak` exists, per JUX-LANG-V1 §6.5.)

> **Edit to JUX-LANG-V1 §6.6:** Append: "Destruction order, exception handling during destructors, and the interaction with refcounting are specified in §S.5."

---

## §S.6 — Concurrency Memory Model

JUX-LANG-V1 §10 specifies the borrow-checker-level guarantees ("no data races, no exclusive borrow across `await`"). This section specifies what happens at the **machine level** for the operations that escape the borrow checker — atomics, volatile, and FFI.

### S.6.1. The User-Facing Promise

Code that obeys the borrow checker (the default, vast majority of Jux code) sees a **sequentially consistent** execution: every memory operation appears to occur in some single global order, consistent with the program order on each thread. This is the strongest model and the one programmers' intuition matches.

This promise holds because the borrow checker prevents the only data races a non-atomic program could have. Where atomic operations or `volatile` register access intentionally relax this — those are the operations specified below.

### S.6.2. Atomic Operations

`AtomicInt`, `AtomicLong`, `AtomicRef<T>` (in `std.concurrent`) expose explicit memory orderings on their methods:

```jux
public class AtomicInt {
    public AtomicInt(int initial);

    // Load operations
    public int load();                             // = load(SeqCst)
    public int load(MemoryOrder order);

    // Store operations
    public void store(int value);                  // = store(value, SeqCst)
    public void store(int value, MemoryOrder order);

    // Read-modify-write
    public int fetchAdd(int delta);                // = fetchAdd(delta, SeqCst)
    public int fetchAdd(int delta, MemoryOrder order);
    public int fetchSub(int delta, MemoryOrder order);
    public int fetchAnd(int mask, MemoryOrder order);
    public int fetchOr(int mask, MemoryOrder order);
    public int fetchXor(int mask, MemoryOrder order);

    // Compare-and-swap
    public CasResult<int> compareAndSwap(int expected, int desired,
                                          MemoryOrder success,
                                          MemoryOrder failure);
}

public enum MemoryOrder {
    Relaxed, Acquire, Release, AcqRel, SeqCst
}
```

The orderings have the same meaning as C++20 / Rust:

- **Relaxed** — atomic, but no ordering with respect to other operations.
- **Acquire** — load operations create an *acquire fence*: subsequent reads on this thread see writes that were release-ordered before the matching atomic store on another thread.
- **Release** — store operations create a *release fence*: prior writes on this thread are visible to a thread that performs a matching acquire load.
- **AcqRel** — for read-modify-write: acts as both acquire and release.
- **SeqCst** — sequential consistency: a single global total order across all SeqCst operations on all atomics.

The default ordering for the no-argument methods is **SeqCst**. This is the safe default — programmers who do not understand the weaker orderings get the strongest guarantees automatically. Programmers who profile and discover an atomic is hot can opt down.

There are no fence intrinsics in v1 (`atomic_thread_fence`-equivalent). If experience shows a need, they can be added in a later edition without changing the rest of the model.

**Phase-1 implementation notes.** `AtomicInt` / `AtomicLong` and `MemoryOrder` are available (`jux.std.concurrent`), lowered onto `std::sync::atomic::AtomicIsize` / `AtomicI64` behind `Arc` — handles share the same cell across `spawn` / `Worker.spawn` boundaries. The `fetch*` family carries both the SeqCst-default and explicit-order overloads. Deferred: `compareAndSwap` (its `CasResult<T>` return type is referenced above but not yet specified — it needs a definition here before implementation) and `AtomicRef<T>`.

### S.6.3. Volatile Access

`@register` fields (per JUX-LANG-V1 §16.3) and any field declared `volatile` produce **un-elidable, un-reordered** loads and stores at the machine level. Specifically:

- The compiler emits exactly one machine load for each `volatile` read in the source program, in source order.
- The compiler emits exactly one machine store for each `volatile` write in the source program, in source order.
- The compiler does not reorder a `volatile` access past another `volatile` access. It **may** reorder a `volatile` access past a non-volatile one to a different address (the volatile guarantees are about the volatile address).
- Volatile is **not** atomic and **not** ordered with respect to other threads. It is for memory-mapped I/O, where the *device* is the other party, not another CPU.
- For thread-shared variables, use atomics, never `volatile`. (Java conflated these; Jux does not.)

Volatile types are `Volatile<T>` (a wrapper exposed in `core.embedded`) or fields/parameters declared with the `volatile` keyword (the bare keyword form is permitted only on memory-mapped fields and `unsafe`-block locals).

### S.6.4. Worker Boundary Semantics

`Worker.spawn(f)` (per JUX-LANG-V1 §10.2):

- Establishes a **happens-before** relationship: every memory operation that happened on the spawning thread before `spawn(f)` is visible to `f` when it begins running.
- The **return** of the resulting `Task<T>` (when consumed via `await`) establishes the symmetric relation: every operation inside `f` before its `return` is visible to the awaiter after `await` completes.

This is identical to `tokio::spawn` + `.await` in Rust and to `Thread.start()` + `.join()` in Java's JMM, in the cases that matter.

### S.6.5. Channel and Mutex Semantics

- `Channel<T>::send(v)` happens-before `Channel<T>::receive()` returning `v`. The receiver sees every memory operation that happened-before the corresponding send.
- `Mutex<T>::lock()` acquiring the lock happens-after the previous holder's release. The block under the lock has acquire/release semantics.
- `AsyncMutex<T>` has the same guarantees, plus the §10.1.6 borrow rule's permission to span `await`.

These are the standard Java/Rust/Go guarantees.

### S.6.6. The §10.1.6 Borrow Rule, Restated

"No exclusive borrow may be held across an `await` point" (JUX-LANG-V1 §10.1.6) is now restateable in memory-model terms:

> Across an `await`, control returns to the executor and other tasks may run that observe shared state. The borrow checker forbids holding an exclusive borrow that would let another task observe a partially-mutated value. Allowed exceptions: `AsyncMutex<T>` guards, where the value is unobservable to other tasks while the guard is held.

This is the formal restatement requested in JUX-LANG-V1 §19.3.

---

## §S.7 — Panics and Aborts

A **panic** is the runtime response to a condition the language guarantees cannot occur (overflow in debug, division by zero, array bounds violation, null deref of `T`, illegal cast, exception in init block).

### S.7.1. Per-Profile Behavior

Per `ERRATA.md` E1, panics and exceptions are **two orthogonal layers**: exceptions are the user-level error model (values, declared in `throws`, caught with `try`/`catch`); panics are an abort-only runtime mechanism. **A panic is never catchable from Jux source** — `catch` clauses match only `Exception` subclasses, and there is no user-visible `Panic` type.

- **`jux-full`**: a panic reports the condition with its source location and a stack trace, then terminates the program. It does **not** unwind into user `catch` blocks. (Phase 1 lowers panics to Rust's own panic machinery, so the report format follows the Rust std.)
- **`jux-embedded`**: same as `jux-full` if the runtime-report machinery is enabled in the build profile; otherwise as `jux-core`.
- **`jux-core`**: a panic calls a configurable panic handler, which by default aborts the program (using whatever abort means on the target — `__builtin_trap`, a vendor-specific reset, a halt loop). Programs may set their own handler:
  ```jux
  import core.panic.{set_panic_handler, PanicInfo};

  set_panic_handler((info: PanicInfo) -> {
      led.errorBlink();
      reboot();
  });
  ```
  The handler is `noreturn` — the type system enforces that it does not return.

### S.7.2. `assert` Built-in

`assert(condition)` and `assert(condition, message)` are language built-ins. In debug builds, they evaluate the condition and panic if false. In release builds, they may be elided depending on profile:

- `jux-full` debug, `jux-embedded` debug, `jux-core` debug: assertion is checked.
- `jux-full` release, `jux-embedded` release: assertion is elided unless the `assertions = true` build flag is set.
- `jux-core` release: assertion is elided unconditionally.

For "must-check at runtime even in release," use `requireOrThrow(condition, () -> Exception)` from `core.result`. The user picks the trade-off.

### S.7.3. Stack Traces

Stack traces are captured when an `Exception` is constructed, and reported when a panic fires:

- `jux-full`: full DWARF-walked stack trace, function names demangled.
- `jux-embedded`: addresses only by default; symbolicate via the linker map.
- `jux-core`: no stack traces (size cost is prohibitive). The panic handler receives only the panic message and source location.

`Exception.stackTrace` returns `StackFrame[]` where each frame has `function`, `file`, and `line` fields. On profiles without stack traces, the array is empty.

---

## Summary

This addendum makes the following parts of JUX-LANG-V1 implementable rather than aspirational:

| Topic                       | Section | What was missing                           |
|-----------------------------|---------|--------------------------------------------|
| Evaluation order             | §S.1    | Sub-expression order, default-arg semantics, named-arg eval |
| Numeric overflow             | §S.2.1  | Per-profile policy, wrapping operators     |
| Numeric division             | §S.2.2  | Division-by-zero, signed-overflow corners  |
| Float semantics              | §S.2.3  | NaN equality, IEEE conformance, `<=>` total order |
| Numeric conversions          | §S.2.4  | Cast table, infallibility promise          |
| Bit operations               | §S.2.5  | `>>` arithmetic-vs-logical by signedness   |
| Mixed-type arithmetic        | §S.2.6  | Disallow silent promotion                  |
| String length and indexing   | §S.3.2  | byte-vs-char distinction                   |
| String equality, hashing     | §S.3.3  | UTF-8-bytes definition                     |
| `char` arithmetic            | §S.3.4  | Range-checked, panic-on-overflow           |
| Initialization order         | §S.4    | Lazy, deterministic, observable            |
| Constructor sequence         | §S.4.4  | Allocate → super → field initializers → init blocks → body |
| Definite assignment          | §S.4.5  | Required for non-nullable fields           |
| Drop order                   | §S.5.2  | Reverse declaration; subclass before parent |
| Drop exceptions              | §S.5.3  | Aggregation, no-throw in `jux-core`        |
| Concurrency model            | §S.6    | SeqCst by default; explicit weaker orderings on atomics |
| Volatile semantics           | §S.6.3  | MMIO only; not for inter-thread sharing    |
| Panics                       | §S.7    | Per-profile behavior; configurable handler |

The rules add up to: **a Jux program that compiles has no UB, no surprises from compiler reordering, and no JVM-style "specified ambiguity."** Programmers who want C-style pointer freedom write it inside `unsafe { }` (specified in `JUX-LAYOUT-ABI-ADDENDUM.md`). Everything outside `unsafe` is fully defined.

---

*End of execution-semantics addendum. When this lands, JUX-LANG-V1.md §5.1, §5.5, §6.6, §10, and §16 should reference §S.x for behaviors previously left implicit.*
