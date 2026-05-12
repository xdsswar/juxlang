# Jux Spec Addendum — Minimal `core` Library

**Status:** Proposed insertion. Specifies the smallest set of `core` types and traits the compiler must know about — the ones referenced by language constructs (smart-cast, `for-each`, `?` operator, `==`, `$"..."` interpolation, async lowering, panic handling). Everything richer than this — `List`, `Map`, full `String` API, networking, JSON, frameworks — is deliberately **out of scope**. Those will be authored in Jux itself once the compiler is bootstrapped.

**Insertion points:**
- New §K.1 ("What Belongs in `core`")
- New §K.2 ("Foundational Traits")
- New §K.3 ("`Option<T>` and Nullability")
- New §K.4 ("`Result<T, E>`")
- New §K.5 ("`Iterator<T>` and `Iterable<T>`")
- New §K.6 ("`StackString<N>`")
- New §K.7 ("`String` (Heap-Backed)")
- New §K.8 ("`Volatile<T>` and `SharedRef<T>`")
- New §K.9 ("Stream Marker Types")
- New §K.10 ("Panic Surface")
- New §K.11 ("Numeric Built-Ins")

This is the contract between the compiler and its lowering targets. Once shipped, every Jux program compiled by `juxc` can lower to these symbols.

---

## Design Philosophy (Non-Normative)

A small `core` is a feature, not a limitation. The two reasons:

1. **The compiler's job is finite.** Every type the compiler "knows about" — has lowering rules for, has special syntax forms for — is a maintenance burden. Keep this set as small as the language requires.
2. **Self-hosting later.** Once `core` is solid, every richer abstraction (collections, formatting, networking) can be authored in Jux. That's the path Rust took: `core` is tiny, `alloc` adds heap, `std` adds OS, then crates.io explodes.

The user-visible promise: **everything in `core` is available in every profile** (`jux-full`, `jux-embedded`, `jux-core`). It does not allocate (or, when it does, it accepts an explicit allocator). It does not require an OS. It does not depend on threading.

Items that need allocation or OS are in **Tier 1 (`std`)** or higher — out of scope for this addendum.

---

## §K.1 — What Belongs in `core`

A type or trait belongs in `core` if at least one of these is true:

- **The compiler synthesizes calls to it.** `Iterable.iterator()` is invoked by every `for-each` loop.
- **The compiler relies on its layout.** `Option<T>` is the lowering target of `T?`. `Result<T, E>` is the lowering target of `throws E` in non-exception profiles.
- **The language syntax depends on it.** Async function returns lower to `Task<T>` from `core.async`. `$"..."` interpolation calls each interpolated value's `operator string`.
- **The semantics chapter (`JUX-SEMANTICS-ADDENDUM.md`) refers to it.** `RuntimeException`, `Volatile<T>`, `SharedRef<T>`, the panic hook.

Items that don't meet at least one of these stay out. The full table:

| Module               | Contents                                                       | Status |
|----------------------|----------------------------------------------------------------|--------|
| `core.markers`        | `Sendable`, `Shareable` (compiler-inferred markers)           | Spec'd |
| `core.option`         | `Option<T>`                                                    | Spec'd |
| `core.result`         | `Result<T, E>`                                                 | Spec'd |
| `core.iter`           | `Iterator<T>`, `Iterable<T>`                                   | Spec'd |
| `core.string`         | `StackString<N>`, slicing primitives, char/byte iteration      | Spec'd |
| `core.heap_string`    | `String` (heap-backed; available where Tier 1 is)              | Spec'd |
| `core.exception`      | `Exception`, `RuntimeException`, standard subtypes              | Per `JUX-EXCEPTIONS-ADDENDUM.md` |
| `core.panic`          | Panic hook, `PanicInfo`                                        | Spec'd |
| `core.volatile`       | `Volatile<T>`                                                  | Spec'd |
| `core.shared`         | `SharedRef<T>` (manual refcount for embedded)                  | Spec'd |
| `core.async`          | `Task<T>`, `Stream<T>`, `MemoryOrder` (markers and minimal API) | Spec'd |
| `core.numeric`        | Methods on `int`, `long`, `float`, `double`                     | Spec'd |
| `core.cmp`            | `<=>` machinery, ordering helpers                              | Spec'd (sketch) |
| `core.range`          | `Range<T>`, `SteppedRange<T>`                                  | Per `JUX-MISSING-DEFS-ADDENDUM.md` §M.6 |
| `core.array`          | `RingBuffer<T, N>`                                             | Spec'd (sketch) |
| `core.ffi`            | `CString`, raw pointer utilities                                | Spec'd (sketch) |
| `core.embedded`       | `@register` infrastructure, MMIO helpers                        | Per JUX-LANG-V1 §16 |

Anything else (`List`, `Map`, `Set`, `Deque`, `String.format`, `Regex`, `File`, `Path`, networking, JSON, time-of-day, threading) **is not in `core`** and is out of scope.

---

## §K.2 — Markers Only (No Foundational Interfaces)

Per `JUX-OPERATORS-ADDENDUM.md`, there is **no** `Equatable`, `Hashable`, `Comparable`, `Cloneable`, `Displayable`, or `Sized` interface in `core`. Equality, ordering, hashing, and formatting are **operator overrides** declared with the `operator` keyword — C++-style. A class that wants structural equality writes `operator==` and `operator hash`; no interface ceremony, no magic method names.

The only items in `core.markers` are the **inferred markers**:

```jux
package core.markers;

public interface Sendable {}
public interface Shareable {}
```

These are populated by the compiler based on a type's structure (per `JUX-MISSING-DEFS-ADDENDUM.md` §M.10.3). User code cannot implement them manually; the compiler rejects manual implementations with `E0951`.

### K.2.1. The One Foundational Interface

`Iterable<T>` (specified in §K.5 below) is the only nominal foundational interface in `core`. It exists because `for-each` benefits from a nominal contract — every other capability is operator-driven.

### K.2.2. Auto-Capabilities by Type Kind

Per `JUX-OPERATORS-ADDENDUM.md` §O.3:

| Type kind | Auto-provides                                                  |
|-----------|----------------------------------------------------------------|
| primitive | `operator==`, `operator<=>`, `operator hash`, `operator string`, implicit copy |
| `struct`  | `operator==`, `operator string`, implicit copy (no `operator hash` — opt in) |
| `record`  | `operator==`, `operator hash`, `operator string`, implicit copy  |
| `enum`    | `operator==`, `operator hash`, `operator string`, implicit copy  |
| `class`   | identity equality only — declare each capability explicitly     |

---

## §K.3 — `Option<T>` and Nullability

The lowering target of `T?`:

```jux
package core.option;

public sealed enum Option<T> permits Some, None {
    Some(T value),
    None;

    public bool isSome() {
        return switch (this) {
            case Some(_) -> true;
            case None -> false;
        };
    }

    public bool isNone() {
        return !isSome();
    }

    public T unwrap() {
        return switch (this) {
            case Some(var v) -> v;
            case None -> panic("unwrap on None");
        };
    }

    public T unwrapOr(T fallback) {
        return switch (this) {
            case Some(var v) -> v;
            case None -> fallback;
        };
    }

    public Option<R> map<R>((T) -> R f) {
        return switch (this) {
            case Some(var v) -> Option.some(f(v));
            case None -> Option.none();
        };
    }

    public Option<R> flatMap<R>((T) -> Option<R> f) {
        return switch (this) {
            case Some(var v) -> f(v);
            case None -> Option.none();
        };
    }

    public T? toNullable() {
        return switch (this) {
            case Some(var v) -> v;
            case None -> null;
        };
    }

    public static Option<T> ofNullable<T>(T? value) {
        return value != null ? Option.some(value) : Option.none();
    }
}
```

### K.3.1. `T?` ↔ `Option<T>`

In source code, `T?` is the canonical syntax for nullability. `Option<T>` exists for cases where:

- A function wants to return a *typed* error indicator (when null doesn't carry enough information, prefer `Result<T, E>` instead).
- A generic context needs an explicit type (`List<Option<T>>` is more readable than `List<T?>` in some signatures).
- Interop with code that uses `Option<T>` directly.

The compiler treats `T?` and `Option<T>` as **distinct types** — they are not implicitly interconvertible. The methods `Option.ofNullable(...)` and `someOption.toNullable()` provide the conversion explicitly.

### K.3.2. Compiler Lowering

`T?` for primitive `T` lowers to a tagged struct (one bit for null + the value). `T?` for class `T` lowers to a niche-optimized class reference (null is represented by all-zero bits). Per `JUX-LAYOUT-ABI-ADDENDUM.md` §L.1.6.

`Option<T>` is a regular sealed enum and lowers normally; the compiler may apply niche optimization to `Option<T>` for class types (so `Option<C>` and `C?` have the same in-memory representation), but this is a backend choice.

---

## §K.4 — `Result<T, E>`

The lowering target of `throws E` in non-exception profiles:

```jux
package core.result;

public sealed enum Result<T, E> permits Ok, Err {
    Ok(T value),
    Err(E error);

    public bool isOk() { ... }
    public bool isErr() { ... }
    public T unwrap() throws E { ... }
    public T unwrapOr(T fallback) { ... }
    public T unwrapOrElse((E) -> T f) { ... }
    public Result<R, E> map<R>((T) -> R f) { ... }
    public Result<T, F> mapErr<F>((E) -> F f) { ... }
    public Result<R, E> flatMap<R>((T) -> Result<R, E> f) { ... }
    public Option<T> ok() { ... }
    public Option<E> err() { ... }

    public static Result<T, never> ok<T>(T value);
    public static Result<never, E> err<E>(E error);

    // Wrap a throwing closure
    public static Result<T, E> from<T, E>(() throws E -> T closure);
}
```

Specified in `JUX-EXCEPTIONS-ADDENDUM.md` §X.4 — repeated here as part of the `core` surface contract.

### K.4.1. The `never` Type

`never` is a built-in type with no values — the bottom type. Used to express "this can't fail" or "this can't return":

- `Result<T, never>` is convertible to `T` (by `unwrap`, never throws).
- A function returning `never` doesn't return; the panic handler returns `never`.

`never` is a compiler-recognized type, not a user-defined one. It can appear in any type position; assignment from `never` to any type is well-typed.

---

## §K.5 — `Iterator<T>` and `Iterable<T>`

Per `JUX-MISSING-DEFS-ADDENDUM.md` §M.10.1, the canonical iterator interface is `next() -> T?`:

```jux
package core.iter;

public interface Iterator<T> {
    T? next();
}

public interface Iterable<T> {
    Iterator<T> iterator();

    // Default-method combinators (eager and lazy):
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
    default T? firstOrNull((T) -> bool pred = _ -> true) { ... }
    default T? minOrNull() where T has operator<=>(T) -> int { ... }
    default T? maxOrNull() where T has operator<=>(T) -> int { ... }
}
```

### K.5.1. `for-each` Desugaring

```jux
for (var x : expr) { body }
```

desugars to:

```jux
{
    var __it = expr.iterator();
    while (true) {
        var __next = __it.next();
        if (__next == null) break;
        var x = __next;
        body
    }
}
```

The desugaring runs in compiler phase 10 (MIR construction). Iterators implementing the `hasNext()/next()` shape (legacy) can be wrapped via `Iterator.fromLegacy(...)` from `core.iter`; the compiler does not auto-detect.

### K.5.2. Iterator and Borrow

An iterator borrows its source for the duration of iteration. The borrow checker enforces:

- A `for-each` over a collection borrows the collection shared.
- Mutating the collection during iteration is rejected (at compile time, per the borrow checker).
- An iterator captured into a closure carries the borrow into the closure's lifetime.

This is what makes `for (var x : list) list.add(...)` a clean compile error rather than a `ConcurrentModificationException` at runtime (the way Java reports it).

### K.5.3. Lazy vs Eager

| Combinator        | Laziness    |
|-------------------|-------------|
| `map`             | Lazy        |
| `filter`          | Lazy        |
| `take(n)`         | Lazy        |
| `skip(n)`         | Lazy        |
| `zip(other)`      | Lazy        |
| `chain(other)`    | Lazy        |
| `reduce(...)`     | Eager       |
| `count()`         | Eager       |
| `any(pred)`       | Eager (short-circuit) |
| `all(pred)`       | Eager (short-circuit) |
| `firstOrNull`     | Eager (short-circuit) |
| `minOrNull`/`maxOrNull` | Eager  |

Lazy combinators return wrapper iterables that compose without intermediate allocation. Conversion to a concrete collection (`.toList()`, `.toSet()`) lives in higher tiers.

### K.5.4. Generators Produce Iterators

Per `JUX-MISSING-DEFS-ADDENDUM.md` §M.2, a `yield`-using function returns `Iterator<T>` (or `Stream<T>` when async). Their state machines implement this trait directly.

---

## §K.6 — `StackString<N>`

A bounded-capacity, stack-allocated string for embedded use:

```jux
package core.string;

public struct StackString<int N> {
    private byte[N] data;
    private int byteLen;

    public StackString();
    public StackString(String initial);                         -- truncates if too long
    public <int M> StackString(StackString<M> initial) where M <= N;  -- exact-size copy

    public int byteLength() { return byteLen; }
    public int charLength();                                     -- O(N) UTF-8 walk
    public int capacity() { return N - 1; }                      -- one byte reserved for null

    public bool tryAppend(String s);                             -- false if would overflow
    public bool tryAppend(char c);

    public Iterable<byte> bytes();
    public Iterable<char> chars();

    public bool operator==(StackString<N> other);
    public int operator hash();
    public String operator string();
    public CString toCString();
}
```

`StackString<N>` is a value type — copied on assignment, lives on the stack. It does not allocate. It is the canonical string type for `jux-core` programs that have no heap.

### K.6.1. Indexing Rules

Per `JUX-SEMANTICS-ADDENDUM.md` §S.3.2: random byte/char indexing is not provided. Use `bytes()` / `chars()` iteration. This is consistent with the heap-backed `String` and avoids the surprise of "is this O(1) or O(N)?".

### K.6.2. Const-Generic Interaction

`StackString<N>` participates in const-generic arithmetic per `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.11.3:

```jux
public StackString<N + 8> withPrefix<int N>(StackString<N> s, String prefix) {
    StackString<N + 8> result = new StackString<>(prefix);
    result.tryAppend($"$s");      // interpolation invokes operator string
    return result;
}
```

---

## §K.7 — `String` (Heap-Backed)

Per `JUX-SEMANTICS-ADDENDUM.md` §S.3, `String` is UTF-8, immutable, and reference-typed. The minimal compiler-known surface:

```jux
package core.heap_string;

public final class String {                                  // operators only — no interfaces
    // Compiler synthesizes the constructor from a string literal at compile time.

    public int byteLength();
    public int charLength();                                     -- O(N) by default

    public Iterable<byte> bytes();
    public Iterable<char> chars();

    public String concat(String other);                          -- used by + and $"..."
    public String substring(int charStart, int charEnd) throws IndexOutOfBoundsException;
    public String substringBytes(int byteStart, int byteEnd) throws EncodingException, IndexOutOfBoundsException;

    public bool operator==(String other);                        -- byte-equality
    public int operator hash();
    public int operator<=>(String other);                        -- byte lex order
    public String operator string();                              -- returns this

    public CString toCString();                                  -- nul-terminated, O(N) for safety check

    public static String fromBytes(byte[] bytes) throws EncodingException;
    public static String fromBytesUnchecked(byte[] bytes);       -- unsafe; caller asserts UTF-8
}
```

### K.7.1. Profile Availability

`String` requires an allocator. In `jux-core`, where there is no heap, `String` is unavailable. Use `StackString<N>`. The compiler emits `E0850` if `String` is referenced under `jux-core`.

In `jux-embedded` and `jux-full`, `String` is available; in `jux-embedded` without an allocator, it is also unavailable (same error).

### K.7.2. The `+` and `$"..."` Lowering

`a + b` where `a` and `b` are strings calls `a.concat(b)`. The compiler may fuse a chain of concatenations into a single `StringBuilder`-style operation, but the user-visible API is `concat`.

`$"...$x..."` (per `JUX-LANG-V1 §3.4`) lowers to a sequence of `operator string` calls and `concat`s. Each interpolated value's static type is checked at the interpolation site; the compiler emits a call to that type's `operator string` (synthesized for primitives, auto-derived for records/structs/enums, explicit for classes that define one).

### K.7.3. `StringBuilder` Out of Scope

A mutable `StringBuilder` is **not** in `core`. It belongs in Tier 1 (`std.string`). Authoring in Jux: a user-mode `StringBuilder` is a thin wrapper around a growable byte buffer with append methods. Once `core` and the basic compiler are working, `StringBuilder` is a 50-line Jux file.

---

## §K.8 — `Volatile<T>` and `SharedRef<T>`

### K.8.1. `Volatile<T>`

```jux
package core.volatile;

public struct Volatile<T> where T fits machine-word {
    // Internal: a single value at a fixed address.

    public T read();
    public void write(T value);

    // Memory-mapped construction (unsafe):
    public unsafe static Volatile<T> atAddress<T>(ulong addr);
}
```

`Volatile<T>` is the language-level abstraction over volatile access (per `JUX-LAYOUT-ABI-ADDENDUM.md` §L.7.5 and `JUX-SEMANTICS-ADDENDUM.md` §S.6.3). Reads and writes go through `read()`/`write()` and are guaranteed un-elidable, un-reordered.

It is not an atomic — for inter-thread sharing, use `AtomicInt` etc. Volatile is for memory-mapped I/O.

The constraint `T fits machine-word` is informal; the actual constraint is "the platform supports atomic load/store of T's size" — typically `T` is `byte`, `ubyte`, `short`, `ushort`, `int`, `uint`, `long`, `ulong`, or a tuple/struct that fits in a machine word.

### K.8.2. `SharedRef<T>`

```jux
package core.shared;

public final class SharedRef<T> where T : Sendable {
    public SharedRef(T value);

    public T get();              -- borrows shared
    public T take();             -- consumes self; returns owned value if last ref
    public SharedRef<T> clone(); -- explicit refcount increment

    drop {
        // decrement refcount; if zero, drop the value
    }
}
```

`SharedRef<T>` is a manually-managed reference-counted wrapper. It exists for `jux-embedded` and `jux-core` profiles where automatic refcount on classes is off (per JUX-LANG-V1 §6.5). When you want shared ownership in those profiles, you wrap with `SharedRef<T>`.

In `jux-full`, where classes are already refcounted, `SharedRef<T>` is rarely needed — but it's available if a user wants explicit control over the refcount.

The compiler does not insert retain/release calls on `SharedRef` operations; the methods (`clone`, `drop`) handle them explicitly. This makes the cost visible.

---

## §K.9 — Stream Marker Types

The async runtime's central types live in `core.async`. Just enough surface that the compiler's async lowering has a place to land:

```jux
package core.async;

public final class Task<T> {
    public T blockingGet() throws ExecutionException;            -- jux-full only
    public void cancel();
    public bool isCancelled();
    public bool isResolved();

    public static Task<T> completed<T>(T value);
    public static Task<T> failed<T>(Exception error);
    public static Task<void> delay(Duration d);

    public static Task<List<T>> all<T>(List<Task<T>> tasks);
    public static Task<T> race<T>(List<Task<T>> tasks);
    public static Task<T> any<T>(List<Task<T>> tasks);
    public static Task<List<Result<T, Exception>>> allSettled<T>(List<Task<T>> tasks);
}

public interface Stream<T> {
    async T? next();
}

public sealed enum MemoryOrder {
    Relaxed, Acquire, Release, AcqRel, SeqCst
}
```

The implementation of `Task<T>` and the executor lives in the runtime — Phase 1 wraps `tokio::task::JoinHandle<T>`. The Jux compiler synthesizes `Task<T>` constructions as the result of `spawn(...)` (per JUX-LANG-V1 §10.1.3) and `async fn` calls inside `await` contexts.

### K.9.1. Stream Combinators

Stream's combinators (the async dual of `Iterable`) — `mapAsync`, `filterAsync`, `take`, `skip`, etc. — are out of scope for `core`. They get authored in Jux in the std-async module once `core` is solid.

---

## §K.10 — Panic Surface

```jux
package core.panic;

public struct PanicInfo {
    public String message;
    public StackFrame[] stackTrace;
    public String location;            -- "file:line:col"
}

// jux-full / jux-embedded with exceptions: panics throw RuntimeException subclasses.
// jux-core / no-exceptions: panics call this hook then abort.

public fn set_panic_hook(hook: (PanicInfo) -> void);
public fn current_panic_hook() -> ((PanicInfo) -> void);

// The compiler-synthesized panic invocation — never directly called by users.
public unsafe fn panic(String message) -> never;
public unsafe fn panic_at(String message, String file, int line) -> never;
```

Per `JUX-EXCEPTIONS-ADDENDUM.md` §X.8. The compiler emits calls to `panic_at(...)` for overflow, OOB, null-deref, etc. Users implementing custom logic in their panic hook do so via `set_panic_hook`.

The default hook in `jux-full`: throws `RuntimeException` (subclass per the panic kind). The default hook in `jux-core`: writes to the configured serial output (or platform default), then aborts via `__builtin_trap` or equivalent.

---

## §K.11 — Numeric Built-Ins

Each primitive numeric type has a small set of methods the compiler may invoke or that user code commonly needs. These are *intrinsics* — the compiler may inline them aggressively.

```jux
package core.numeric;

// On every signed integer type T (byte, short, int, long):
public T MIN_VALUE;
public T MAX_VALUE;
public Result<T, ArithmeticException> checkedAdd(T other);
public Result<T, ArithmeticException> checkedSub(T other);
public Result<T, ArithmeticException> checkedMul(T other);
public Result<T, ArithmeticException> checkedDiv(T other);
public T saturatingAdd(T other);
public T saturatingSub(T other);
public T saturatingMul(T other);
public T wrappingAdd(T other);                  -- same as +%
public T wrappingSub(T other);
public T wrappingMul(T other);
public T abs() throws ArithmeticException;      -- throws on T.MIN_VALUE
public T saturatingAbs();
public int countOnes();                          -- popcount
public int leadingZeros();
public int trailingZeros();
public T rotateLeft(int n);
public T rotateRight(int n);
public Result<int, ArithmeticException> toInt();
public int saturatingToInt();
public ... // similar for other target types
public String operator string();
public String toHex();
public String toBinary();
public String toOctal();

// On every unsigned integer type T (ubyte, ushort, uint, ulong): same except no abs.

// On float and double:
public T NAN;
public T POSITIVE_INFINITY;
public T NEGATIVE_INFINITY;
public T MIN_VALUE;                              -- smallest positive
public T MAX_VALUE;
public T EPSILON;
public bool isNaN();
public bool isInfinite();
public bool isFinite();
public T floor();
public T ceil();
public T round();                                -- round-half-to-even
public T sqrt();
public T abs();
public uint bits();                              -- IEEE bit pattern (for float; ulong for double)
public static T fromBits(uint bits);
public bool bitsEqual(T other);                  -- exact bit equality including NaN payloads
public int totalOrder(T other);                  -- IEEE 754 total order; used for <=>
public String operator string();
public String toFixed(int decimals);              -- formatted with N decimals

// On char:
public bool isAlphabetic();
public bool isDigit();
public bool isWhitespace();
public bool isUppercase();
public bool isLowercase();
public char toUppercase();
public char toLowercase();
public uint codePoint();                         -- the Unicode scalar value
public static char fromCodePoint(uint cp) throws IllegalArgumentException;
```

These methods are **intrinsics**: the compiler recognizes them by name and may emit direct machine instructions for performance-critical ones (`countOnes` → `popcnt`, `leadingZeros` → `lzcnt`, `sqrt` → `sqrtsd`, etc.). On targets without the corresponding instruction, the compiler emits a portable software implementation.

The full set is too large to specify here exhaustively; the canonical list is in `core.numeric`'s source as the implementation lands. The compiler must support every method named here and behave as documented.

---

## §K.12 — What's NOT in `core`

Explicitly out of scope:

- **`List<T>`, `Map<K, V>`, `Set<T>`, `Deque<T>`** — Tier 1 (`std.collections`); requires allocator.
- **`String.format`, `Regex`, `StringBuilder`** — Tier 1 (`std.string`).
- **`File`, `Path`, `Reader`, `Writer`** — Tier 2 (`std.io`); requires OS.
- **`Instant`, `Duration`, `LocalDateTime`** — Tier 1/2 (`std.time`).
- **TCP/UDP, HTTP, JSON** — Tier 2 (`std.net`, `std.http`, `std.json`).
- **Threading primitives beyond markers** (`Thread`, `Worker`, work pool config) — Tier 1+.
- **Logging, random, crypto, process management** — Tier 1+.

Once the Phase 1 compiler is producing working binaries, these get authored in Jux. The authoring will look like ordinary Jux code that consumes the `core` types specified here. No further compiler support required.

---

## §K.13 — How the Compiler Uses This

For each lowering operation, the compiler emits calls to specific `core` symbols:

| Source construct                        | Lowered call                                                |
|----------------------------------------|-------------------------------------------------------------|
| `for (var x : it) { ... }`              | `core.iter.Iterable.iterator()`, `core.iter.Iterator.next()` |
| `expr ?: fallback`                      | `core.option.Option.unwrapOr(fallback)`                     |
| `expr?` on `Result<T, E>`                | `core.result.Result.<flatMap or short-circuit>`             |
| `expr?` on `T?`                          | `core.option.Option.<flatMap or short-circuit>`             |
| `a == b` on classes with `operator==` defined | user-defined `operator==` method                     |
| `$"text $expr"`                         | each value's `operator string`, `String.concat(...)`               |
| `await expr`                            | `core.async.Task.<poll>` or `Stream.<next>` machinery       |
| `spawn(closure)`                        | `core.async.Task.spawn(closure)`                            |
| Integer overflow (debug)                 | `panic_at("integer overflow", file, line)`                  |
| Array OOB                                | `panic_at("index out of bounds: ...")`                      |
| Null deref of non-nullable               | `panic_at("null deref")`                                    |
| `volatile` field read                    | `core.volatile.Volatile.read()`                             |
| `static const` initialization            | `OnceLock` initialization (Phase 1 backend)                 |
| `try { B } catch { ... }`                | Per `JUX-EXCEPTIONS-ADDENDUM.md` §X.5                      |

Each of these mapping rules is the *contract* between the compiler and `core`. As long as `core` provides these symbols with the documented behavior, compiled programs run correctly.

---

## Summary

This addendum specifies the smallest `core` library that lets the compiler do its job:

| Module               | Purpose                                                  |
|----------------------|----------------------------------------------------------|
| `core.markers`        | `Sendable`, `Shareable` (compiler-inferred markers)                |
| `core.option`         | `Option<T>` for nullability lowering                     |
| `core.result`         | `Result<T, E>` for `throws`-to-Result lowering           |
| `core.iter`           | `Iterator<T>`, `Iterable<T>` for `for-each` desugaring   |
| `core.string`         | `StackString<N>` for embedded                           |
| `core.heap_string`    | `String` for heap-backed UTF-8                           |
| `core.exception`      | Exception class hierarchy (per `JUX-EXCEPTIONS-ADDENDUM.md`) |
| `core.panic`          | Panic hook surface                                       |
| `core.volatile`       | `Volatile<T>` for MMIO                                   |
| `core.shared`         | `SharedRef<T>` for explicit refcount                     |
| `core.async`          | `Task<T>`, `Stream<T>`, `MemoryOrder` markers            |
| `core.numeric`        | Per-primitive intrinsic methods                          |

Everything outside this list — collections, full string operations, I/O, time, networking, frameworks — gets written **in Jux** later. That's the whole point of bootstrapping a small core: the compiler stays small and stable; the language ecosystem grows in the language itself.

---

*End of `core` library addendum. This closes the last spec gap blocking a Phase 1 implementation. Subsequent work is non-spec: actually writing the compiler.*
