# Jux Language — Architecture & Design Dossier

**Version:** 0.1 (design)
**Status:** Specification draft
**File extension:** `.jux`

---

## 1. Vision and Positioning

Jux is a systems programming language that combines **Java-family syntax** with **Rust-style memory safety**, **native compilation**, and **first-class C/C++/Rust interoperability**. It is designed for programmers who want the safety guarantees of Rust without the syntactic and conceptual overhead, expressed in a syntax that any Java, C#, or Kotlin developer can read on day one.

### 1.1. Design Goals

1. **Java-level simplicity.** A Java developer should be able to read Jux code without a tutorial. Borrow checker errors should never mention lifetimes, regions, or `'static`.
2. **Rust-level safety.** No data races, no use-after-free, no double-free, no null pointer dereferences (without explicit nullable types).
3. **Native compilation.** Compiles to machine code via LLVM. No VM, no interpreter, no startup tax. Performance class: Rust / C / C++.
4. **No manual memory management.** RAII via destructors. Users never write `free` or `delete`.
5. **Effortless FFI.** C, C++, and Rust libraries are first-class. No JNI-style ceremony. Any C library that exists can be used from Jux.
6. **Hybrid paradigm.** Procedural (free functions, structs) and OOP (classes, inheritance, interfaces) coexist in the same file, freely intermixable.
7. **Scales from servers to bare metal.** The same language targets cloud servers, desktops, and 32KB microcontrollers. Features adapt to the target via profiles.
8. **Ship only what you use.** Final binaries contain only the code actually reached from entry points. Importing a module costs nothing if you don't use most of it.

### 1.2. Non-Goals

- Template metaprogramming (use generics + interfaces instead)
- Higher-kinded types
- Compile-time evaluation as a Turing-complete language (limited `const` evaluation only)
- Backward compatibility with Java bytecode or the JVM

### 1.3. Comparison Matrix

| Feature                     | Jux              | Java     | Rust         | Swift     | C++       |
|-----------------------------|------------------|----------|--------------|-----------|-----------|
| Syntax family               | Java             | Java     | ML/C-hybrid  | C-family  | C-family  |
| Memory safety               | Compile-time     | GC       | Compile-time | ARC       | None      |
| Native compilation          | Yes              | No (JVM) | Yes          | Yes       | Yes       |
| Borrow checker              | Yes (inferred)   | No       | Yes (annotated)| Partial | No        |
| Lifetime annotations        | None visible     | N/A      | Required     | None      | N/A       |
| OOP support                 | Full             | Full     | Limited      | Full      | Full      |
| Procedural support          | Full             | Limited  | Full         | Full      | Full      |
| Generics                    | Monomorphized    | Erased   | Monomorphized| Specialized | Templates |
| Exceptions                  | Yes (checked)    | Yes      | No (Result)  | Yes       | Yes       |
| Null safety                 | `T?` syntax      | No       | `Option<T>`  | `T?`      | No        |
| C interop                   | Direct           | JNI      | Direct       | Direct    | Direct    |
| C++ interop                 | Via C ABI        | JNI      | Via C ABI    | Direct (5.9+)| Native|
| Rust interop                | First-class      | None     | Native       | Limited   | Limited   |

---

## 2. Compilation Model

### 2.1. Pipeline

```
.jux source files
    ↓
Lexer + Parser → Jux AST
    ↓
Name resolution + module linking
    ↓
Type checking + generic monomorphization
    ↓
Borrow inference + ownership analysis
    ↓
Lowering to typed IR
    ↓
Code generation (one of three backends, see 2.2)
    ↓
LLVM IR
    ↓
LLVM optimization + codegen
    ↓
Native binary (.so / .dylib / .dll / .exe)
```

### 2.2. Backend Strategy (Staged)

**Phase 1 — Transpile to Rust source.**
The Jux compiler emits idiomatic Rust source code that, when compiled by `rustc`, produces the desired binary. Borrow checking happens twice: once in Jux (with friendly errors), once in rustc (as a final correctness gate). This is the fastest path to a working compiler — Kotlin used a similar approach (compiling to JVM bytecode) and TypeScript still works this way (compiling to JavaScript).

**Phase 2 — Custom rustc driver.**
The Jux compiler programmatically lowers Jux AST to rustc's MIR, bypassing the source-code intermediate. Faster compilation, tighter integration with Rust crates. Cost: rustc internals are unstable, requiring continuous maintenance against nightly.

**Phase 3 — Direct LLVM frontend.**
The Jux compiler emits LLVM IR directly, with its own borrow checker, trait solver, and optimization passes. Maximum control, no upstream dependencies on rustc. Multi-year effort.

The recommended trajectory is Phase 1 → 2 → 3 over the language's first 5+ years.

### 2.3. Build Artifacts

A Jux library compiles to:

- A platform-native dynamic library (`.so`, `.dylib`, `.dll`)
- A static library (`.a`, `.lib`) for static linking
- A C header (`.h`) describing all `@export`-annotated symbols
- A Jux interface file (`.juxi`) describing the full public API for Jux consumers
- Optionally, a Cargo-compatible Rust crate manifest enabling Rust projects to depend on Jux libraries

### 2.4. Compilation Profiles

A profile selects which language features and runtime services are available. The same Jux source can target a server or a microcontroller; the profile decides what the compiler emits and what `std` provides.

| Profile | Target | Heap | Refcount | Exceptions | Threads | Typical use |
|---|---|---|---|---|---|---|
| `jux-full` | Desktop, server | Yes | Yes | Yes | Yes | Apps, services, tools |
| `jux-embedded` | MCU with OS or RTOS | Optional | Optional | Optional | Optional | ESP32, STM32, Pi Pico |
| `jux-core` | Bare metal, kernels | No | No | No | No | Bootloaders, ATmega-class MCUs |

Profile is set in `jux.toml`:

```toml
[build]
profile = "embedded"
target = "thumbv7em-none-eabihf"
```

The compiler enforces the profile at compile time. Using a feature not available in the current profile is a compile error with a clear message naming the missing feature and what to use instead.

Section 17 covers embedded and bare-metal targets in detail.

### 2.5. Dead Code Elimination

By default, the Jux compiler emits each function and constant into its own linker section. The linker walks the call graph from program entry points (`main`, `@export` functions, `@interrupt` handlers) and discards any section that is not reachable. The final binary contains only the code the program actually uses.

This applies uniformly:

- Unused functions are stripped, including from imported modules.
- Unused constants and static data are stripped.
- Generic instantiations are emitted only for the type parameters actually used. `List<int>` and `List<String>` are independent; if your program never uses `List<float>`, no code for it is generated.
- Unused class methods are stripped per method, not per class.
- Virtual methods reachable through interface dispatch are kept conservatively unless link-time optimization can prove they are unreachable.

Importing a module never costs binary size for code you don't call. The user writes `import std.collections.*;` without worrying about pulling in unused collections — the linker handles it.

Profile defaults:

| Profile | Default optimization | LTO | Strip |
|---|---|---|---|
| `jux-full` (debug) | Fast compile, no DCE | Off | No |
| `jux-full` (release) | Speed | Thin | Yes |
| `jux-embedded` | Size | Fat | Yes |
| `jux-core` | Size | Fat | Yes |

---

## 3. Lexical Structure

### 3.1. File Encoding and Layout

- Source files are UTF-8 encoded with extension `.jux`.
- Files contain a single package declaration at the top, followed by imports, followed by declarations.
- The directory structure mirrors the package path (`com/example/foo/Bar.jux` declares `package com.example.foo`).
- A file may contain multiple public top-level declarations (Java's "one public class per file" rule is not adopted).

### 3.2. Keywords (Reserved)

```
abstract     break        case         catch        class
const        default      do           drop         else
enum         extends      final        finally      for
if           implements   import       init         interface
internal     move         native       new          package
permits      private      protected    public        record
return       sealed       static       struct        super
switch       this         throw        throws       try
type         var          void         while        yield
```

### 3.3. Identifiers

- ASCII letters, digits, and underscore.
- Must not start with a digit.
- Must not collide with reserved keywords.
- Convention: `camelCase` for variables and methods, `PascalCase` for types, `SCREAMING_SNAKE_CASE` for constants, `lowercase` for packages.

### 3.4. Literals

| Kind        | Examples                                |
|-------------|-----------------------------------------|
| Integer     | `42`, `0xFF`, `0b1010`, `1_000_000`     |
| Long        | `42L`, `0xFFL`                          |
| Float       | `3.14f`, `1e10f`                        |
| Double      | `3.14`, `1.0e-5`                        |
| Boolean     | `true`, `false`                         |
| Character   | `'a'`, `'\n'`, `'\u00E9'`               |
| String      | `"hello"`, `"line\n"`                   |
| Raw string  | `"""multi\nline"""` (no escape processing) |
| Null        | `null`                                  |

**String interpolation.** Strings prefixed with `$` interpolate expressions written inside `${...}`. The simple form `$name` interpolates a bare identifier without braces:

```java
var name = "Alice";
var age = 30;

var greeting = $"Hello, $name! You are ${age + 1} next year.";
// "Hello, Alice! You are 31 next year."

var path = $"/users/${user.id}/posts/${post.id}";
```

Inside `${...}` any expression is permitted, including method calls. The result is converted to a string via the value's `toString()` method. Expressions are evaluated at runtime; the compiler generates efficient string-building code (no concatenation chains).

Raw strings can also be interpolated with `$"""..."""` and disable escape processing while still permitting `${...}`.

### 3.5. Comments and Documentation

```java
// Line comment
/* Block comment */

/** Documentation comment, attached to the following declaration. */
public void greet(String name) { /* ... */ }
```

Documentation comments use Markdown. Standard tags follow Java conventions:

```java
/**
 * Computes the great-circle distance between two points on a sphere.
 *
 * @param a the first point
 * @param b the second point
 * @param radius the radius of the sphere (e.g., 6371.0 for Earth in km)
 * @return the distance in the same units as `radius`
 * @throws GeometryError if either point has invalid coordinates
 *
 * # Example
 * ```jux
 * var d = haversine(london, tokyo, 6371.0);
 * ```
 */
public double haversine(Point a, Point b, double radius) throws GeometryError { ... }
```

Tags recognized by `juxc doc`:

| Tag         | Purpose                                            |
|-------------|----------------------------------------------------|
| `@param`    | Document a parameter                               |
| `@return`   | Document the return value                          |
| `@throws`   | Document a thrown exception                        |
| `@deprecated` | Mark the item deprecated, with a reason          |
| `@since`    | Version in which this item was added               |
| `@see`      | Cross-reference to another item                    |

Code blocks tagged with `jux` are compiled and run as part of `juxc doc` to verify they remain correct. Failed examples become CI failures.

### 3.6. Annotations

Annotations begin with `@` and precede a declaration. Some are built into the language; others are user-defined and may be processed by tooling. Built-in annotations the language gives meaning to:

| Annotation                         | Purpose                                                | Defined in |
|------------------------------------|--------------------------------------------------------|------------|
| `@Override`                        | Asserts that a method overrides a supertype's method   | §7.4       |
| `@Deprecated`                      | Marks a declaration as deprecated                      | std        |
| `@cfg(...)`                        | Conditional compilation predicate                      | §11        |
| `@extern(lib = "...", header = "...")` | Declares a foreign library binding              | §8.1       |
| `@export`, `@export(name = "...")` | Marks a function as part of the C-callable surface     | §8.4       |
| `@export(cpp_wrapper = true)`      | Additionally generates a C++ wrapper header            | §8.3       |
| `@register(address = ...)`         | Memory-mapped hardware register                        | §16.3      |
| `@interrupt(vector = ...)`         | Interrupt service routine                              | §16.4      |
| `@inline`, `@noinline`             | Hints to the compiler about inlining                   | —          |
| `@align(N)`                        | Forces alignment of a type or field                    | —          |

Annotations may take parameters. Parameters use named-argument syntax: `@cfg(os = "linux", arch = "x86_64")`. Block form is supported for annotations that apply to several declarations at once: `@export { ... }`.

User-defined annotations are declared as interfaces marked with `@AnnotationType`. They have no runtime effect by default; tooling and macros (if added in a future version) may consume them.

---

## 4. Module System

### 4.1. Packages

Packages use Java's dot-separated naming convention. The package declaration is the first non-comment statement in a file:

```java
package com.example.app;
```

The directory structure mirrors the package path. A file at `src/com/example/app/Main.jux` must declare `package com.example.app`.

### 4.2. Imports

Imports use `.` as the separator, never `::`. Java syntax exactly:

```java
import com.example.zoo.Animal;            // single import
import com.example.zoo.*;                 // wildcard
import com.example.zoo.Animal as A;       // renaming (extension over Java)
import com.example.math.{Point, distance, Vector};   // grouped (extension over Java)
```

### 4.3. Modules

A module is a unit of compilation and distribution, similar to a Cargo crate or a JPMS module. Modules are declared via a `module.jux` file at the root of the source tree:

```java
module com.mylang.json {
    version "1.0.0";

    exports com.mylang.json;              // public to consumers
    exports com.mylang.json.parser;
    // com.mylang.json.internal is NOT exported — internal to this module

    requires std.io;                      // dependencies on other modules
    requires com.mylang.collections version ">=2.0";

    requires rust.serde_json;             // Rust crate dependency
    requires c.sqlite3 lib "sqlite3";     // C library dependency
}
```

If no `module.jux` is present, the project is treated as a single-module application with all `public` symbols exported.

### 4.4. Visibility and Scope Resolution

The visibility rules form a strict hierarchy from least to most restrictive:

- `public` — visible to any code, including other modules
- `internal` — visible within this module, hidden from consumers
- (no modifier, package-private) — visible within this package only
- `protected` — visible within this package and to subclasses (Java semantics)
- `private` — visible only within the declaring class or file

For top-level (non-class) declarations: `private` means file-scope.

---

## 5. Type System

### 5.1. Primitive Types

| Type     | Size   | Range                              |
|----------|--------|------------------------------------|
| `bool`   | 1 byte | `true`, `false`                    |
| `byte`   | 8-bit  | -128 to 127 (signed)               |
| `ubyte`  | 8-bit  | 0 to 255 (unsigned)                |
| `short`  | 16-bit | signed                             |
| `ushort` | 16-bit | unsigned                           |
| `int`    | 32-bit | signed                             |
| `uint`   | 32-bit | unsigned                           |
| `long`   | 64-bit | signed                             |
| `ulong`  | 64-bit | unsigned                           |
| `float`  | 32-bit | IEEE 754 single                    |
| `double` | 64-bit | IEEE 754 double                    |
| `char`   | 32-bit | Unicode scalar value               |
| `void`   | —      | absence of value (return type only)|

Primitives are value types. They are copied on assignment and passed by value. No boxing required for use as generic type parameters (this differs from Java; see §7.8 on generics).

### 5.2. Reference Types

**`class`** — heap-allocated reference type with identity. Shared on assignment. Supports inheritance, polymorphism, virtual methods. Default visibility for fields: `private`.

**`interface`** — abstract contract. Supports default methods, may be sealed. Cannot have fields (beyond constants).

**`struct`** — stack-allocated value type without identity. Copied on assignment. No inheritance. Default visibility for fields: `public`. Used for plain data.

**`record`** — immutable value type with auto-generated equality, hashing, and accessors. No inheritance. All fields are part of the constructor.

**`enum`** — sum type / tagged union. May carry payloads (associated data). Exhaustively pattern-matchable.

### 5.3. Tuples

Tuples are anonymous, ordered, fixed-size groupings of values. Useful for returning multiple values from a function without declaring a named record:

```java
public (int, int) divmod(int a, int b) {
    return (a / b, a % b);
}

var (q, r) = divmod(17, 5);          // q = 3, r = 2

// Tuples can be named at the binding site for clarity:
var pair = divmod(17, 5);
print(pair.0);                        // 3
print(pair.1);                        // 2
```

Tuples are value types — copied on assignment, passed by value, no heap allocation. They are flattened in memory (no boxing) and have zero overhead compared to writing out the fields by hand.

When a tuple's purpose deserves a name, prefer a `record`:

```java
public record DivResult(int quotient, int remainder) {}
public DivResult divmod(int a, int b) {
    return new DivResult(a / b, a % b);
}
```

Use tuples for one-off cases; use records when the shape is part of an API or appears more than once.

### 5.4. Destructuring

Tuples and records can be destructured into their components on assignment or in pattern matching:

```java
var (q, r) = divmod(17, 5);

public record Point(double x, double y) {}
var Point(x, y) = somePoint;          // record destructuring

// Inside a switch:
switch (shape) {
    case Circle(var r) -> useRadius(r);
    case Rectangle(var w, var h) -> useArea(w * h);
}
```

### 5.5. Special Types

| Type                | Description                                     | Available in |
|---------------------|-------------------------------------------------|--------------|
| `String`            | UTF-8 string (reference type, immutable)        | full, embedded |
| `T?`                | Nullable T (T or null)                          | all profiles |
| `T[]`               | Array of T, size set at construction            | all profiles |
| `T[N]`              | Array of T with statically-known size N         | all profiles |
| `RingBuffer<T, N>`  | Fixed-capacity circular buffer                  | all profiles |
| `StackString<N>`    | Inline string with bounded capacity, no heap    | all profiles |
| `List<T>`           | Growable list (uses heap)                       | full, embedded |
| `Map<K, V>`         | Hash map (uses heap)                            | full, embedded |
| `Set<T>`            | Hash set (uses heap)                            | full, embedded |
| `(A, B) -> R`       | Function type                                   | all profiles |
| `T*`                | Raw pointer (only inside `native` blocks)       | all profiles |
| `CString`           | Null-terminated C string for FFI                | all profiles |

**Arrays.** Jux uses Java's `T[]` syntax for arrays. Every array carries its length, accessed via the `.length` field. Indexing is bounds-checked at runtime; when the index and size are both compile-time constants, bounds are checked at compile time.

```java
var data = new int[10];                   // size 10, set at construction
var names = new String[]{"Alice", "Bob"}; // size inferred from initializer
print(data.length);                       // 10

for (var i = 0; i < data.length; i++) {
    data[i] = i * 2;
}

for (var name : names) {
    print(name);
}
```

When the size is a compile-time constant, the type can carry it explicitly (`T[N]`). This enables stricter typing and stack allocation:

```java
var samples = new int[64];                // size 64 known at compile time, on the stack
samples[80];                              // COMPILE ERROR: index 80 out of bounds for int[64]

// Functions can require an exact size:
public byte[32] sha256(byte[] input) {
    // returns exactly 32 bytes
}

// Or be generic over size:
public <int N> byte[N] copy(byte[N] src) {
    var dst = new byte[N];
    for (var i = 0; i < N; i++) dst[i] = src[i];
    return dst;
}
```

`T[]` and `T[N]` are interchangeable when passing a fixed-size array to a function expecting a runtime-sized one — the size information is simply not preserved. Going the other way requires a check.

The other fixed-size types (`RingBuffer<T, N>`, `StackString<N>`) carry their capacity as a const generic parameter and live entirely on the stack. They never allocate. This makes them safe to use in `jux-core` and in real-time code where heap allocation is forbidden.

```java
var buffer = new RingBuffer<float, 256>();   // 256-slot circular buffer
var name: StackString<32> = "sensor_01";     // up to 31 chars + null
```

### 5.6. Type Inference

`var` infers the declared type from the initializer:

```java
var name = "Alice";              // String
var count = 42;                  // int
var users = new List<User>();    // List<User>
```

Type inference is local — it never crosses function boundaries. Public function signatures must always have explicit types.

### 5.7. Type Aliases

```java
public type UserId = long;
public type Callback<T> = (T) -> void;
public type StringMap = Map<String, String>;
```

Type aliases are transparent — a `UserId` is interchangeable with a `long`. For nominal types that share an underlying representation but should not be interchangeable, use a wrapper struct or record.

---

## 6. Memory Model and Borrow Inference

### 6.1. Ownership

Every value has a single owner at any moment. When the owner goes out of scope, the value's destructor runs. This applies to:

- `class` instances (heap-allocated, refcount-managed when shared, see §6.5)
- `struct` instances (stack-allocated when possible)
- Native resources (file handles, sockets, GPU buffers, FFI handles)

### 6.2. Default Passing Semantics

| Type kind | Pass to function | Assign to variable     |
|-----------|------------------|------------------------|
| primitive | copied           | copied                 |
| `struct`  | copied           | copied                 |
| `record`  | copied           | copied                 |
| `class`   | borrowed (shared)| shared reference       |

This matches Java's intuition: primitives are values, objects are references. The compiler enforces that shared references cannot outlive their target and that mutation through a reference does not race with other accesses.

### 6.3. Mutation

Methods do not declare whether they mutate `this` — the compiler infers this from the body. If a method assigns to a field of `this` (or calls another mutating method on `this`), it requires exclusive access to the receiver. Otherwise it requires only shared access.

```java
public class Counter {
    private int count = 0;

    public int get() {                  // compiler sees no mutation: shared access
        return count;
    }

    public void increment() {           // compiler sees mutation: exclusive access
        count = count + 1;
    }
}
```

The borrow checker enforces the inferred contract at call sites:

```java
var counter = new Counter();
counter.increment();                    // OK: counter is `var`, exclusive access available

final var locked = new Counter();
locked.increment();                     // ERROR: cannot mutate through `final` binding
```

The same inference applies to free functions: parameters that are mutated through their bindings require exclusive access from the caller.

```java
public void resetCounter(Counter c) {
    c.increment();                      // c needs exclusive access — inferred
}
```

This keeps method declarations identical to Java. The safety guarantees are unchanged; only the bookkeeping moves from the syntax to the compiler.

### 6.4. Move Semantics

Ownership transfers in three situations:

1. **Storing a value in a longer-lived location** (e.g., adding to a collection that will outlive the current scope)
2. **Returning a value from a function**
3. **Explicit `move` keyword**

```java
public void main() {
    var alice = new User("Alice", 30);
    var users = new List<User>();
    users.add(alice);            // alice is moved into the list
    alice.greet();               // ERROR: alice was moved
}
```

To keep using the value, either copy it explicitly or use `move` to make the transfer intent unambiguous:

```java
users.add(move alice);           // explicit move
users.add(alice.clone());        // or copy if Cloneable
```

### 6.5. Reference Counting and Cycles

In `jux-full`, class instances are reference-counted. This is invisible to users and resolves the "Java has cycles, Rust doesn't" tension. Cycles are handled via the `weak` modifier:

```java
public class Parent {
    private List<Child> children;
}

public class Child {
    private weak Parent parent;  // does not contribute to refcount
}
```

A `weak` reference can be promoted to a strong reference for use:

```java
public void notifyParent(Child c) {
    var p = c.parent.get();      // returns Parent? (nullable — parent may be gone)
    if (p != null) {
        p.handleNotification();
    }
}
```

This matches Swift's ARC + weak-reference model. It is the proven approach for Java-style reference semantics without a tracing GC.

**In `jux-embedded` and `jux-core`**, refcounting is off by default because atomic operations are expensive (or unavailable) on small MCUs. Class instances follow single-ownership semantics — assignment moves rather than shares. To opt back into shared ownership, wrap a value in `SharedRef<T>`:

```java
import std.embedded.SharedRef;

var alice = new SharedRef<>(new User("Alice"));
var bob = alice.clone();         // explicit refcount increment
```

This is the only meaningful semantic difference between profiles. All other Jux code reads the same regardless of target.

### 6.6. Destructors

Classes and structs may declare a `drop` block that runs when the value is destroyed:

```java
public class Database {
    private SqliteHandle* handle;

    public Database(String path) throws DbError { /* ... */ }

    drop {
        sqlite3_close(handle);
    }
}
```

Destructors run deterministically when the value's lifetime ends — when its scope exits, when it is removed from a collection, when its containing object is destroyed. There are no finalizers in the Java sense; destruction is predictable and immediate.

### 6.7. Lifetime Inference (No Annotations)

Jux does not expose lifetime parameters. The compiler infers them from function bodies. The user-visible model: "a borrow cannot outlive what it borrows from." When the compiler cannot prove safety, the error message describes the conflict in terms of code structure, not lifetime variables:

```
Error: cannot use `users` here
  --> main.jux:14:5
   |
12 |     var first = users.get(0);
   |                 ----- borrows from `users` here
13 |     users.add(new User("Bob"));
   |     ^^^^^ cannot mutate `users` while a borrow is active
14 |     print(first.name);
   |     ----- the borrow is still in use here

Hint: clone the result if you need to keep it across the mutation:
   var first = users.get(0).clone();
```

### 6.8. Trade-offs Accepted

This design accepts two principled limitations to keep the language simple:

1. **Some programs that Rust would accept will be rejected by Jux.** When inference fails, the user must restructure rather than annotate.
2. **Refcount overhead on class instances.** This is small (a few cycles per assignment) but nonzero. Performance-critical code can use `struct` for stack allocation and avoid refcounting entirely.

The combination still produces faster code than Java (no GC pauses, no JIT warmup, no boxing) while being safer than C/C++ (no UB, no use-after-free).

---

## 7. Syntax Reference

### 7.1. Top-Level Structure

A `.jux` file consists of:

```java
package com.example.app;

import com.example.foo.Bar;
import std.io.print;

// Type declarations: class, interface, struct, record, enum, sealed
public class Foo { /* ... */ }

// Free functions
public void greet(String name) { /* ... */ }

// Top-level constants
public const double PI = 3.14159265358979;

// Type aliases
public type UserId = long;
```

The entry-point file (conventionally `main.jux`) may contain top-level statements that execute at program start; non-entry files must contain only declarations.

### 7.2. Function Declarations

```java
// Top-level free function
public String formatUser(User user) {
    return user.name + " (" + user.age + ")";
}

// Function with no return value
public void greet(User user) {
    print("Hi, " + user.name);
}

// Generic free function
public <T> T identity(T value) {
    return value;
}

// Function with default arguments
public void connect(String host, int port = 80, int timeout = 30) {
    // ...
}

// Function with named-argument call sites
connect("example.com");
connect("example.com", port: 443);
connect("example.com", timeout: 60, port: 443);

// Function that throws checked exceptions
public String readFile(String path) throws IOException {
    // ...
}

// Variadic function — accepts zero or more trailing arguments of the given type
public void log(String level, String... messages) {
    for (var msg : messages) {
        print($"[$level] $msg");
    }
}

log("info", "starting", "loaded config", "ready");
log("error", "connection failed");
```

The variadic parameter is bound to a `String[]` inside the function body. Variadic must be the last parameter; only one variadic parameter per function.

### 7.3. Class Declarations

```java
public class User {
    // Fields with explicit visibility
    private String passwordHash;
    public get String name;                 // public read, private write
    public get private set int age;         // explicit asymmetric form
    public String email;                    // public read and write

    // Primary constructor with init block
    public User(String name, int age, String email = "") {
        this.name = name;
        this.age = age;
        this.email = email;
        this.passwordHash = "";
    }

    // Secondary constructor
    public User(String name) {
        this(name, 0);
    }

    // Method (does not mutate `this`)
    public String describe() {
        return name + " (" + age + ")";
    }

    // Mutating method
    public void birthday() {
        age = age + 1;
    }

    // Static method
    public static User anonymous() {
        return new User("Anonymous", 0);
    }

    // Destructor
    drop {
        // cleanup logic if any
    }
}
```

### 7.4. Inheritance and Interfaces

```java
public abstract class Animal {
    protected String name;
    protected int age;

    public Animal(String name, int age = 0) {
        this.name = name;
        this.age = age;
    }

    public abstract void speak();

    public void introduce() {
        print("I am " + name);
    }
}

public interface Trainable {
    void learn(String command);

    // Default method
    default void learnAll(List<String> commands) {
        for (var cmd : commands) {
            learn(cmd);
        }
    }
}

public final class Dog extends Animal implements Trainable {
    private List<String> tricks;

    public Dog(String name, int age = 0) {
        super(name, age);
        this.tricks = new List<String>();
    }

    @Override
    public void speak() {
        print(name + " says woof");
    }

    @Override
    public void learn(String command) {
        tricks.add(command);
    }
}
```

A class may extend exactly one class and implement any number of interfaces. The `final` modifier prevents further inheritance.

### 7.5. Sealed Types and Pattern Matching

```java
public sealed interface Shape permits Circle, Square, Triangle {}

public final record Circle(double radius) implements Shape {}
public final record Square(double side) implements Shape {}
public final record Triangle(double base, double height) implements Shape {}

public double area(Shape s) {
    return switch (s) {
        case Circle(var r) -> 3.14159 * r * r;
        case Square(var side) -> side * side;
        case Triangle(var b, var h) -> 0.5 * b * h;
    };
}

public String describe(Shape s) {
    return switch (s) {
        case Circle(var r) when r > 100 -> "huge circle";
        case Circle(var r) -> "circle of radius " + r;
        case Square s -> "square";
        case Triangle t -> "triangle";
    };
}
```

The compiler verifies exhaustiveness for sealed types — no `default` clause needed when all cases are covered.

### 7.6. Structs and Records

```java
public struct Point {
    double x;
    double y;
}

public record Vector3(double x, double y, double z) {}

public void main() {
    var p = new Point(3.0, 4.0);
    var p2 = p;                  // copies (value type)
    p.x = 5.0;                   // does not affect p2

    var v = new Vector3(1.0, 2.0, 3.0);
    // v.x = 5.0;                // ERROR: records are immutable
    var v2 = v.with(x: 5.0);     // returns a copy with x changed
}
```

### 7.7. Enums (Sum Types)

```java
public enum HttpResult {
    Success(int statusCode, String body),
    Redirect(String location),
    Error(int code, String message);
}

public void handle(HttpResult result) {
    switch (result) {
        case Success(var code, var body) -> print("OK: " + body);
        case Redirect(var url) -> print("Follow: " + url);
        case Error(var code, var msg) -> print("Failed " + code + ": " + msg);
    }
}
```

Unlike Java enums (which are essentially singletons of a class), Jux enums are tagged unions with optional payloads. They monomorphize to compact tagged structures with no allocation overhead for primitive payloads.

### 7.8. Generics

```java
public class Box<T> {
    private T value;

    public Box(T value) {
        this.value = value;
    }

    public T get() {
        return value;
    }

    public void set(T value) {
        this.value = value;
    }
}

public class Pair<A, B> {
    public get A first;
    public get B second;

    public Pair(A first, B second) {
        this.first = first;
        this.second = second;
    }
}

// Bounded generics with `extends`
public <T extends Comparable<T>> T max(T a, T b) {
    if (a.compareTo(b) > 0) return a;
    return b;
}

// Multiple bounds with `&`
public <T extends Comparable<T> & Serializable> void sortAndSave(List<T> items) {
    // ...
}

// Generic method on a non-generic class
public class Utils {
    public static <T> List<T> repeat(T value, int times) {
        var result = new List<T>();
        for (var i = 0; i < times; i++) {
            result.add(value);
        }
        return result;
    }
}

// Wildcards (Java-style)
public void copyAll(List<? extends Animal> source, List<? super Animal> dest) {
    for (var item : source) {
        dest.add(item);
    }
}

// Diamond operator
List<String> names = new List<>();        // type inferred
```

Generics are monomorphized — `List<int>` produces a packed array of ints with no boxing. This is invisible at the source level but provides Rust-level performance for generic code.

### 7.9. Lambdas and Function Types

```java
// Lambda expressions
var adder = (int x, int y) -> x + y;
var greeter = (User u) -> {
    print("Hello, " + u.name);
};

// Function types
public (int, int) -> int makeAdder(int base) {
    return (x, y) -> base + x + y;
}

// Higher-order methods on collections
var users = new List<User>();
var names = users.map(u -> u.name);                     // List<String>
var adults = users.filter(u -> u.age >= 18);
var totalAge = users.reduce(0, (acc, u) -> acc + u.age);

// Method references
users.forEach(User::greet);
```

Closures infer their capture mode (shared borrow, exclusive borrow, or move) from the body and from how they are used. The user does not annotate capture mode.

### 7.10. Nullable Types

```java
public String? findName(int id) {           // returns String or null
    if (id < 0) return null;
    return database.lookup(id);
}

public void main() {
    var name = findName(42);
    if (name != null) {
        print(name);                        // smart-cast: name is non-null here
    }

    // Elvis operator for default value
    var displayName = findName(42) ?: "unknown";

    // Safe navigation
    var length = findName(42)?.length();    // returns int?
}
```

Nullability is tracked in the type system. Non-nullable references cannot hold null. This eliminates NullPointerException as a runtime failure mode.

### 7.11. Error Handling

Jux uses checked exceptions, compiled internally to discriminated unions for zero-overhead propagation:

```java
public class FileError extends Exception {
    public FileError(String message) { super(message); }
}

public String readFile(String path) throws FileError {
    if (!exists(path)) {
        throw new FileError("not found: " + path);
    }
    // ...
}

public void main() {
    try {
        var contents = readFile("config.txt");
        print(contents);
    } catch (FileError e) {
        print("Failed: " + e.message);
    }
}
```

Functions that may throw must declare their exception types in `throws`. Unchecked exceptions (runtime panics for arithmetic overflow, array bounds, etc.) exist but are not part of the function signature.

**`Result<T, E>` and the `?` operator.** When exceptions are unavailable (in `jux-core`, in real-time code, or wherever the developer prefers explicit error returns), `Result<T, E>` from `core.result` provides a value-based alternative. The `?` operator propagates errors without `try`/`catch`:

```java
import core.result.Result;

public Result<Config, ConfigError> loadConfig() {
    var contents = readFile("config.txt")?;       // returns early if Err
    var parsed = parseConfig(contents)?;
    return Result.ok(parsed);
}
```

`x?` is shorthand for "if `x` is `Ok(value)`, unwrap it; if `x` is `Err(e)`, return `Err(e)` from the enclosing function." The compiler verifies that the enclosing function's return type can hold the propagated error.

Code can mix the two styles: a function declared `throws E` can call a function returning `Result<T, E>` (using `match` or `?` after a conversion), and vice versa. In profiles that disable exceptions, the compiler can lower `throws` to `Result` automatically, so the same source ports cleanly between server and embedded targets.

### 7.12. Smart Casting

Type tests with `instanceof` bind the narrowed type to a name when present, and the compiler tracks the narrowed type within the resulting scope:

```java
public void process(Animal a) {
    if (a instanceof Dog d) {
        d.bark();                    // d is Dog here, no cast needed
        d.learn("sit");
    } else if (a instanceof Cat c) {
        c.purr();                    // c is Cat here
    }
}
```

Smart-casting also applies after null checks for nullable types:

```java
public void greet(String? name) {
    if (name != null) {
        print("Hello, " + name);     // name is String here, not String?
    }
}
```

This eliminates the typical Java sequence of `if (x instanceof Foo) { Foo f = (Foo) x; ... }` — the cast and binding happen in one step, and the result is statically type-checked.

### 7.13. Static Members

Classes and interfaces may declare `static` fields and methods. Static members belong to the type, not to instances. Java syntax exactly:

```java
public class MathUtils {
    public static const double PI = 3.14159265358979;

    public static double square(double x) {
        return x * x;
    }
}

var d = MathUtils.square(5.0);
var pi = MathUtils.PI;
```

**Static fields.** `static const` (immutable) fields are freely allowed and compile to read-only data. Mutable static fields are global state and must use thread-safe types — the compiler rejects raw mutable statics that are not synchronized:

```java
public class Counter {
    private static AtomicInt instanceCount = new AtomicInt(0);   // OK

    // private static int rawCount = 0;                          // ERROR: mutable static
    //                                                              must be a thread-safe type
    //                                                              (AtomicInt, Mutex<T>, etc.)
}
```

This eliminates a major source of subtle bugs in Java codebases.

**Static initializer blocks.** A `static { ... }` block runs once, on first access to the class, in a thread-safe manner:

```java
public class Config {
    private static final Map<String, String> defaults;

    static {
        defaults = new Map<>();
        defaults.put("host", "localhost");
        defaults.put("port", "8080");
    }
}
```

**Static factory methods.** Pair naturally with private constructors:

```java
public class User {
    private User(String name, int age) { /* ... */ }

    public static User create(String name, int age) {
        if (age < 0) throw new IllegalArgumentException("negative age");
        return new User(name, age);
    }

    public static User anonymous() {
        return new User("Anonymous", 0);
    }
}
```

**Static methods on interfaces.** Permitted, like Java 8+:

```java
public interface Comparable<T> {
    int compareTo(T other);

    static <T extends Comparable<T>> T max(T a, T b) {
        return a.compareTo(b) > 0 ? a : b;
    }
}
```

**Static vs free functions.** Both exist in Jux. Use a free function for general utilities (`sqrt`, `parseInt`, `print`); use a static method when the function is conceptually associated with a specific type (factories, type-related utilities, methods that need access to private fields of the type).

### 7.14. Top-Level Statements

The file conventionally named `main.jux` (configurable in `module.jux`) may contain top-level statements that execute at program start:

```java
// File: main.jux
package com.example.app;

import std.io.print;

print("Starting up");
var x = 42;
print("x = " + x);
```

This is equivalent to wrapping the statements in a `public static void main(String[] args)` body. Other files must contain only declarations.

### 7.15. Loops and Ranges

Jux supports four loop forms. The first three match Java exactly; the fourth (range-based) adds an ergonomic shorthand.

```java
// 1. while loop
while (condition) {
    // ...
}

// 2. do-while loop
do {
    // ...
} while (condition);

// 3. C-style for loop
for (var i = 0; i < 10; i++) {
    print(i);
}

// 4. for-each loop, over any iterable
for (var name : names) {
    print(name);
}
```

**Ranges.** The `..` operator constructs a range expression. Ranges are iterable and integrate with for-each:

```java
for (var i : 0..10) {            // 0, 1, 2, ..., 9   (exclusive end)
    print(i);
}

for (var i : 0..=10) {           // 0, 1, 2, ..., 10  (inclusive end)
    print(i);
}

for (var i : 10..0 step -1) {    // 10, 9, 8, ..., 1
    print(i);
}
```

Ranges work for any integer type and for `char`. They are zero-cost — the compiler generates the same code as a hand-written C-style loop.

**`break` and `continue`** behave as in Java, including support for labeled forms:

```java
outer: for (var i : 0..10) {
    for (var j : 0..10) {
        if (matrix[i][j] == target) {
            break outer;          // exit both loops
        }
    }
}
```

---

## 8. Foreign Function Interface

### 8.1. C Library Interop

C functions are declared inside `native` blocks within `@extern` annotations. The raw declarations are unsafe and only callable from privileged code; safe wrappers expose them as ordinary Jux classes.

```java
package std.ffi.sqlite;

@extern(lib = "sqlite3")
native {
    int sqlite3_open(CString path, out RawHandle* db);
    int sqlite3_close(RawHandle* db);
    int sqlite3_exec(RawHandle* db, CString sql, void* callback, void* arg, out CString errmsg);
}

@extern(lib = "sqlite3")
native opaque struct RawHandle;

public final class Database {
    private RawHandle* handle;

    public Database(String path) throws DbError {
        var h: RawHandle* = null;
        var rc = sqlite3_open(path.toCString(), out h);
        if (rc != 0) {
            throw new DbError("Failed to open: " + path);
        }
        this.handle = h;
    }

    public void execute(String sql) throws DbError {
        var err: CString = null;
        var rc = sqlite3_exec(handle, sql.toCString(), null, null, out err);
        if (rc != 0) {
            throw new DbError("SQL error: " + err.toString());
        }
    }

    drop {
        sqlite3_close(handle);
    }
}
```

Users of `Database` see only a safe Jux class. The raw pointer is `private` and never leaks.

### 8.2. Rust Library Interop

Rust libraries can be consumed in three layers, ordered by ambition:

**Layer 1 — Rust libraries with C ABI exports.** Indistinguishable from C libraries. Use the `@extern` machinery above.

**Layer 2 — Auto-wrapped via `jux-bindgen`.** A tooling-level approach: a `jux-bindgen` tool reads a Rust crate's public API and generates Jux declarations plus a Rust shim crate that exposes everything as `extern "C"`. Same pattern as `uniffi`.

**Layer 3 — First-class imports.** The Jux compiler directly understands Rust crates. Because Jux compiles through Rust (Phase 1 of the backend strategy), the compiler natively reads Rust type signatures and translates them:

```java
import rust.serde_json.Value;
import rust.serde_json.from_str;
import rust.tokio.runtime.Runtime;

public void main() throws ParseError {
    var json = from_str("{ \"name\": \"Alice\" }");
    print(json.get("name"));
}
```

Type translation table:

| Rust type                | Jux type                              |
|--------------------------|---------------------------------------|
| `String`, `&str`         | `String`                              |
| `Vec<T>`                 | `List<T>`                             |
| `HashMap<K, V>`          | `Map<K, V>`                           |
| `HashSet<T>`             | `Set<T>`                              |
| `Option<T>`              | `T?`                                  |
| `Result<T, E>`           | function returning `T throws E`       |
| `Box<T>`, `Rc<T>`, `Arc<T>` | `T` (refcount-managed by Jux)      |
| `&T` and `&mut T`        | borrowed `T` (handled by inference)   |
| `i32`, `u64`, etc.       | `int`, `ulong`, etc.                  |
| `Box<dyn Trait>`         | `Trait` (Jux interface)               |
| `Fn(A) -> B` family      | function type `(A) -> B`              |
| Macros                   | not importable                        |
| Async functions          | require runtime support               |

### 8.3. C++ Library Interop

C++ uses the same C ABI machinery as C, because every C++ compiler supports `extern "C"` linkage. The generated header file is bilingual:

```c
#ifndef MYLANG_ENGINE_H
#define MYLANG_ENGINE_H

#ifdef __cplusplus
extern "C" {
#endif

void* engine_create(void);
void engine_destroy(void* e);
int engine_run(void* e);

#ifdef __cplusplus
}
#endif

#endif
```

Optionally, the `@export(cpp_wrapper = true)` annotation generates an additional C++ header with idiomatic RAII classes wrapping the C functions:

```cpp
namespace mylang {
    class Engine {
        void* handle_;
    public:
        Engine();
        ~Engine();
        Engine(const Engine&) = delete;
        Engine(Engine&& other) noexcept;
        int run();
    };
}
```

This is purely a header-side convenience — the binary contains only C-ABI symbols.

### 8.4. Exporting Jux Functions to Foreign Code

The `@export` annotation marks free functions (or static methods) as part of the C-callable surface:

```java
@export
public int compute_distance(double x1, double y1, double x2, double y2) {
    return sqrt((x2-x1)*(x2-x1) + (y2-y1)*(y2-y1));
}

// Block form for many functions
@export {
    public int json_parse(CString input, out CString error) { /* ... */ }
    public CString json_get_string(JsonHandle* h, CString key) { /* ... */ }
    public void json_destroy(JsonHandle* h) { /* ... */ }
}

// Optional explicit C name
@export(name = "mylang_compute_v2")
public int computeDistance(double x1, double y1, double x2, double y2) { /* ... */ }
```

The compiler:

1. Disables name mangling for `@export` symbols
2. Applies C calling convention
3. Validates that all parameter and return types are C-compatible
4. Emits the function in the generated `.h` header with `extern "C"` linkage guards

Permitted parameter and return types in `@export` signatures: primitives, `CString`, raw pointers `T*`, plain structs whose fields are all C-compatible, and function pointers. Not permitted: classes, generics, sealed types, exceptions, `String`, collection types.

To expose class functionality to C, write `@export` free functions that take handle pointers (the standard `*-sys` pattern from Rust):

```java
public final class Database { /* full Jux API */ }

@export {
    public Database* db_open(CString path) { return new Database(path.toString()); }
    public void db_close(Database* db) { delete db; }
    public int db_execute(Database* db, CString sql) {
        try { db.execute(sql.toString()); return 0; }
        catch (DbError e) { return -1; }
    }
}
```

---

## 9. Standard Library

### 9.1. Layered Structure

The standard library is organized in tiers. Each tier may depend only on tiers below it. This keeps dependencies clean and dead code elimination effective.

**Tier 0 — `core`** (always available, no allocation, no OS):

| Module               | Purpose                                       |
|----------------------|-----------------------------------------------|
| `core.primitives`    | Operations on `int`, `float`, `bool`, etc.    |
| `core.math`          | Pure math: `sqrt`, `sin`, `abs`, constants    |
| `core.array`         | `RingBuffer<T, N>`, array utilities           |
| `core.string`        | `StackString<N>`, string slicing              |
| `core.option`        | `Option<T>` and nullable utilities            |
| `core.result`        | `Result<T, E>` for error returns              |
| `core.ffi`           | `CString`, raw pointer utilities              |
| `core.embedded`      | `Volatile<T>`, register access, `SharedRef<T>`|

**Tier 1 — `std`** (requires allocator; most embedded targets supply one):

| Module               | Purpose                                       |
|----------------------|-----------------------------------------------|
| `std.collections`    | `List`, `Map`, `Set`, `Queue`, `Deque`        |
| `std.string`         | Heap-backed `String` operations and formatting|
| `std.error`          | `Exception` hierarchy                         |
| `std.json`           | JSON parsing and serialization                |

**Tier 2 — `std` with OS** (requires an operating system):

| Module               | Purpose                                       |
|----------------------|-----------------------------------------------|
| `std.io`             | `print`, `readLine`, file I/O                 |
| `std.fs`             | Filesystem operations                         |
| `std.net`            | Networking primitives                         |
| `std.process`        | Process management, environment variables     |
| `std.time`           | Wall-clock time                               |
| `std.concurrent`     | Threads, channels, atomics, mutexes           |
| `std.async`          | Async runtime (future)                        |
| `std.crypto`         | Hashing, random, basic primitives             |
| `std.testing`        | Unit testing framework                        |

### 9.2. Availability Per Profile

| Tier   | jux-full | jux-embedded     | jux-core |
|--------|----------|------------------|----------|
| Tier 0 | Yes      | Yes              | Yes      |
| Tier 1 | Yes      | Yes (with allocator) | No   |
| Tier 2 | Yes      | If OS present    | No       |

A `jux-core` build that imports `std.io` is rejected at compile time with: `"std.io requires an operating system; current profile is jux-core."`

### 9.3. Design Principles

- **Small core.** Anything beyond fundamental primitives goes in higher tiers or separate packages.
- **One way to do common things.** One canonical `List`, one canonical `Map`. No `ArrayList` vs `LinkedList` proliferation.
- **No legacy.** No deprecated APIs, no compatibility shims.
- **Borrow-friendly.** Methods returning references make the borrow obvious; methods returning owned values don't entangle the caller.
- **Iterator protocol.** Every collection implements a uniform iteration interface for `for (var x : coll)` and chainable operations.

### 9.4. Core Interfaces

A small set of interfaces in `core` define the contracts most types interact with. They are available in all profiles.

```java
public interface Iterator<T> {
    bool hasNext();
    T next();
}

public interface Iterable<T> {
    Iterator<T> iterator();
}

public interface Comparable<T> {
    int compareTo(T other);          // negative, zero, or positive
}

public interface Equatable {
    bool equals(Object other);
    int hashCode();
}

public interface Cloneable<T> {
    T clone();
}

public interface Sendable {}         // marker: safe to transfer between threads
public interface Shareable {}        // marker: safe to access from multiple threads
```

**`Iterable<T>` powers `for (var x : coll)`.** Any type implementing `Iterable<T>` works in for-each loops. All standard collections, ranges, and arrays implement it.

**`Equatable` is required for use as a `Map` or `Set` key.** Records and enums implement it automatically based on their fields. Classes must implement it explicitly when needed; the compiler can derive it on request via `@Derive(Equatable)`.

**`Comparable<T>` powers sorting.** `List<T>` has `.sort()` available when `T : Comparable<T>`.

**`Cloneable<T>` exposes explicit duplication.** Used with `var b = a.clone();` to make a deep copy when the borrow checker would otherwise force a move.

**`Sendable`/`Shareable`** are inferred by the compiler from a type's fields. A type is `Sendable` if all its fields are `Sendable`; same for `Shareable`. They cannot be implemented manually — they are facts about the type, not contracts to fulfill.

---

## 10. Concurrency

### 10.1. Threads

```java
import std.concurrent.Thread;

public void main() {
    var t = new Thread(() -> {
        print("Hello from another thread");
    });
    t.start();
    t.join();
}
```

### 10.2. Send/Sync Discipline

Types implement `Sendable` (safe to transfer between threads) and `Shareable` (safe to access from multiple threads simultaneously) via marker interfaces. The compiler infers conformance from a type's fields. This matches Rust's `Send` and `Sync` but uses Java-style interface names.

```java
public class Counter implements Sendable {
    private int count;
    // ...
}
```

A type containing a non-sendable field is not itself `Sendable`. The compiler enforces this when transferring values between threads.

### 10.3. Channels

```java
import std.concurrent.{Channel, Thread};

public void main() {
    var ch = new Channel<int>();

    new Thread(() -> {
        for (var i = 0; i < 10; i++) {
            ch.send(i);
        }
        ch.close();
    }).start();

    while (var value = ch.receive()) {
        print("Received: " + value);
    }
}
```

### 10.4. Mutexes and Atomics

```java
import std.concurrent.{Mutex, AtomicInt};

public class SafeCounter {
    private Mutex<int> count;

    public SafeCounter() {
        this.count = new Mutex<>(0);
    }

    public void increment() {
        count.lock(c -> c + 1);
    }
}
```

The `Mutex<T>` API forces lock acquisition before access — there is no way to access the protected value without going through the mutex. This eliminates "forgot to lock" bugs at compile time.

### 10.5. Async (Future Direction)

Async/await is a planned feature for a later version. The design will draw from Rust's `async fn` + `Future` model but with implicit polling and Java-style `await` syntax. Not part of the v0.1 specification.

---

## 11. Conditional Compilation

Cross-platform code needs to vary by operating system, CPU architecture, build profile, or user-defined feature flags. Jux handles this through `@cfg` annotations and `if cfg(...)` blocks — both are part of the language proper, not a textual preprocessor. The compiler understands them, the IDE highlights them correctly, and unused branches are not compiled at all.

There is no `#if`/`#endif` style preprocessor in Jux.

### 11.1. The `@cfg` Annotation

`@cfg(...)` applies to any declaration: function, class, method, field, import, or module. The annotation's predicate is evaluated at compile time. If false, the declaration does not exist for the current build.

```java
@cfg(os = "linux")
public void openSocket() {
    // Linux-specific implementation using POSIX
}

@cfg(os = "windows")
public void openSocket() {
    // Windows-specific implementation using Winsock
}

@cfg(os = "macos")
public void openSocket() {
    // macOS-specific implementation
}
```

Exactly one version is compiled into the binary. Code that doesn't apply to the current target is invisible to the compiler — it can call platform-specific APIs without breaking other targets.

### 11.2. Predicates

Built-in predicates the compiler understands:

| Predicate         | Values (examples)                                                |
|-------------------|------------------------------------------------------------------|
| `os`              | `"linux"`, `"macos"`, `"windows"`, `"freebsd"`, `"none"`         |
| `arch`            | `"x86_64"`, `"aarch64"`, `"armv7"`, `"armv6m"`, `"riscv32"`, `"avr"`, `"xtensa"` |
| `profile`         | `"full"`, `"embedded"`, `"core"`                                 |
| `endian`          | `"big"`, `"little"`                                              |
| `pointer_width`   | `"32"`, `"64"`                                                   |
| `target`          | Full target triple, e.g. `"thumbv7em-none-eabihf"`               |
| `feature`         | User-defined feature flags from `jux.toml`                       |
| `debug`           | `true` in debug builds                                           |
| `release`         | `true` in release builds                                         |

### 11.3. Combining Predicates

```java
@cfg(os = "linux", arch = "x86_64")           // AND: both must hold
public void fastSyscall() { ... }

@cfg(any(os = "linux", os = "macos"))         // OR
public void unixOnly() { ... }

@cfg(not(os = "windows"))                     // negation
public void notOnWindows() { ... }

@cfg(all(os = "linux", any(arch = "x86_64", arch = "aarch64")))
public void modernLinux() { ... }
```

### 11.4. Where `@cfg` Can Appear

```java
// Function
@cfg(os = "linux")
public void epollCreate() { ... }

// Class
@cfg(os = "windows")
public class WindowsRegistry { ... }

// Method inside a class
public class FileSystem {
    public void open(String path) { /* portable */ }

    @cfg(os = "windows")
    public void setHidden(String path) { /* Windows-only */ }

    @cfg(any(os = "linux", os = "macos"))
    public void chmod(String path, int mode) { /* Unix-only */ }
}

// Field
public class Config {
    public String name;

    @cfg(profile = "full")
    public List<String> history;

    @cfg(profile = "embedded")
    public RingBuffer<String, 32> history;
}

// Import
@cfg(os = "linux")
import std.platform.linux.epoll;

@cfg(os = "windows")
import std.platform.windows.iocp;

// Constant
@cfg(os = "windows")
public const String PATH_SEPARATOR = "\\";

@cfg(not(os = "windows"))
public const String PATH_SEPARATOR = "/";
```

### 11.5. Inline Conditionals: `if cfg(...)`

For varying a few lines inside a function rather than the whole declaration, use `if cfg(...)`. The compiler evaluates the predicate at compile time and discards the dead branch entirely. Both branches must parse, but the dead branch does not need to type-check against missing platform APIs.

```java
public void log(String message) {
    if cfg(debug) {
        var timestamp = currentTime();
        print("[" + timestamp + "] " + message);
    }
    // No output in release builds — the if branch is removed
}

public void process(byte[] data) {
    if cfg(arch = "x86_64") {
        processSimd(data);          // calls SIMD intrinsics
    } else {
        processScalar(data);        // portable fallback
    }
}
```

This is similar to C++'s `if constexpr` — real Jux syntax (parsed, IDE-friendly), but resolved at compile time with the unused branch eliminated before code generation.

### 11.6. Feature Flags

User-defined feature flags are declared in `jux.toml`:

```toml
[module]
name = "com.example.mylib"

[features]
default = ["json"]
json = []
yaml = []
binary_protocol = []
async = ["std.async"]
```

Code uses them via `@cfg(feature = "...")`:

```java
@cfg(feature = "json")
public class JsonParser { /* ... */ }

@cfg(feature = "yaml")
public class YamlParser { /* ... */ }
```

Consumers enable features in their own `jux.toml`:

```toml
[dependencies]
"com.example.mylib" = { version = "1.0", features = ["json", "async"] }
```

The compiler includes only the code for enabled features; everything else is stripped.

### 11.7. Cross-Platform Example

```java
package std.fs;

public class File {
    private FileHandle* handle;

    public File(String path, int mode) throws IOError {
        this.handle = openImpl(path, mode);
    }

    @cfg(any(os = "linux", os = "macos", os = "freebsd"))
    private static FileHandle* openImpl(String path, int mode) throws IOError {
        var fd = posix_open(path.toCString(), mode);
        if (fd < 0) throw new IOError("open failed");
        return new FileHandle(fd);
    }

    @cfg(os = "windows")
    private static FileHandle* openImpl(String path, int mode) throws IOError {
        var h = CreateFileW(path.toUtf16(), accessFlags(mode), 0, null, OPEN_EXISTING, 0, null);
        if (h == INVALID_HANDLE_VALUE) throw new IOError("CreateFile failed");
        return new FileHandle(h);
    }

    public byte[] readAll() throws IOError { /* portable using handle */ }

    drop {
        closeImpl(handle);
    }

    @cfg(any(os = "linux", os = "macos", os = "freebsd"))
    private static void closeImpl(FileHandle* h) { posix_close(h.fd); }

    @cfg(os = "windows")
    private static void closeImpl(FileHandle* h) { CloseHandle(h.windowsHandle); }
}
```

The `File` class has one public API. Internally, two implementations exist; the compiler picks one based on the target OS. The other does not appear in the binary.

---

## 12. Tooling

### 12.1. Compiler: `juxc`

The reference compiler. Command-line interface:

```
juxc build                       # build the current module
juxc run                         # build and run
juxc test                        # run tests
juxc check                       # type-check without codegen
juxc fmt                         # format source files
juxc doc                         # generate documentation
juxc bindgen <c-header>          # generate Jux bindings from a C header
juxc bindgen --rust <crate>      # generate Jux bindings from a Rust crate
```

### 12.2. Build System: `jux`

Project-level tool, comparable to Cargo. Configuration in `jux.toml`:

```toml
[module]
name = "com.example.myapp"
version = "0.1.0"

[dependencies]
"com.mylang.json" = "1.0"
"rust.serde_json" = "1.0"
"c.sqlite3" = { lib = "sqlite3", header = "sqlite3.h" }

[build]
target = "native"
optimization = "release"
```

Commands:

```
jux new <name>                   # create a new project
jux build                        # build the project
jux run                          # build and run
jux test                         # run all tests
jux publish                      # publish to the package registry
jux update                       # update dependencies
```

### 12.3. Package Registry

A central registry (analogous to crates.io or Maven Central) hosts Jux modules. Modules are immutable once published. Semantic versioning is enforced.

### 12.4. Language Server

A reference implementation of LSP for IDE integration: completion, go-to-definition, find-references, refactoring, real-time error reporting, format-on-save. Built on the same compiler frontend.

### 12.5. Documentation Generator

Doc comments use Markdown. The `juxc doc` command produces searchable HTML documentation, including cross-module links and rendered examples that are compiled and tested as part of CI.

### 12.6. Testing Framework

`std.testing` provides a testing framework integrated with `juxc test`. Tests are ordinary functions annotated with `@Test`:

```java
package com.example.math.test;

import std.testing.{Test, assertEqual, assertThrows, BeforeEach, AfterEach};
import com.example.math.{add, divide, MathError};

@Test
public void addsCorrectly() {
    assertEqual(5, add(2, 3));
    assertEqual(0, add(-1, 1));
}

@Test
public void divideThrowsOnZero() {
    assertThrows(MathError, () -> divide(10, 0));
}

// Setup that runs before each @Test in this file
@BeforeEach
public void setup() {
    // ...
}

// Cleanup after each @Test
@AfterEach
public void teardown() {
    // ...
}
```

Test files conventionally live in `test/` directories paralleling the source tree. `juxc test` runs all `@Test`-annotated functions, reports pass/fail, and supports parallel execution by default. Tests honor the same compilation profile as the code under test, so embedded code can be tested under `jux-embedded` or `jux-core` targets.

The framework provides assertions for common cases (`assertEqual`, `assertNotEqual`, `assertNull`, `assertThrows`, `assertTrue`) and a generic `assert(condition, message)` for everything else. Failures include source-location info and a structural diff for non-trivial values.

---

## 13. Compiler Implementation Plan

### 13.1. Phase 1: Bootstrap (Months 1–6)

- Lexer and parser for full Jux grammar (written in Rust, since the Phase 1 backend is Rust)
- AST definition and basic name resolution
- Type checker for the core language (no generics yet)
- Lowering to Rust source code
- Integration with `cargo` for the actual build
- Minimal `std` (io, collections, string)
- Working "hello world" through to native binary

### 13.2. Phase 2: Core Language (Months 7–12)

- Generics with monomorphization (lowers to Rust generics)
- Sealed types and pattern matching
- Borrow inference (initial heuristic version)
- Destructors / `drop` blocks
- C FFI: `@extern` and `@export`
- Module system and `module.jux`
- Build tool (`jux` command)
- Basic LSP

### 13.3. Phase 3: Ergonomics (Months 13–18)

- Nullable types and smart-casting
- Lambda capture inference
- Default and named arguments
- Error handling (try/catch lowering to Result)
- Refcounting + weak references for class types
- Async (initial design, exploratory)
- Documentation generator

### 13.4. Phase 4: Ecosystem (Months 19–24)

- `jux-bindgen` for C and Rust
- First-class `import rust.X` (Layer 3 of Rust interop)
- C++ wrapper generation
- Package registry MVP
- Standard library expansion (net, fs, crypto, json)
- Performance optimization passes
- Formal testing of borrow inference completeness

### 13.5. Phase 5: Self-Hosting (Years 2–3)

- Rewrite the compiler in Jux
- Direct LLVM backend (Phase 3 of the backend strategy)
- Stable 1.0 release
- Long-term support commitment

---

## 14. Open Design Questions

The following are unresolved and require further design work:

1. **Async/await syntax and runtime model.** Whether to adopt a poll-based model (Rust) or thread-of-execution model (Go, Kotlin). Trade-off: integration with Rust async ecosystem vs. simpler mental model.

2. **Const evaluation.** How much of Jux is evaluable at compile time? Rust's `const fn` model is expressive but complex. Java has no real answer. Likely middle ground: simple `const` expressions and pure functions only.

3. **Reflection and runtime type information.** Java's reflection is powerful but costly. Rust has none. Jux likely needs *some* (for serialization, frameworks) but not full Java-level reflection. Where to draw the line?

4. **Operator overloading.** Currently absent from this design. Mathematical types (vectors, matrices, fixed-precision decimals) benefit from it; general code abuses it. Whether to allow it for selected operators only.

5. **Inline assembly.** Probably not in v1.0, but the design should not preclude it for systems work.

6. **Effects beyond exceptions.** Rust has nothing here; algebraic effects are research-grade. Jux likely stays with exceptions but should not architect them out.

7. **Macros.** Compile-time code generation is powerful but complex. Likely deferred indefinitely; user-defined macros are the slipperiest slope in language design.

8. **Specialization within generics.** Whether `List<int>` can have a different (faster) implementation than `List<T>` in general. Rust has experimental specialization; full design is unsolved.

---

## 15. Example: A Complete Small Program

```java
// File: main.jux
package com.example.zoo;

import std.io.print;
import std.collections.List;

public sealed abstract class Animal permits Dog, Cat, Bird {
    protected String name;
    protected int age;

    public Animal(String name, int age = 0) {
        this.name = name;
        this.age = age;
    }

    public abstract void speak();

    public void introduce() {
        print("I am " + name + ", age " + age);
    }

    public String getName() { return name; }
}

public interface Trainable {
    void learn(String command);

    default void learnAll(List<String> commands) {
        for (var cmd : commands) {
            learn(cmd);
        }
    }
}

public final class Dog extends Animal implements Trainable {
    private List<String> tricks;

    public Dog(String name, int age = 0) {
        super(name, age);
        this.tricks = new List<String>();
    }

    @Override
    public void speak() {
        print(name + " says woof");
    }

    @Override
    public void learn(String command) {
        tricks.add(command);
    }
}

public final class Cat extends Animal {
    public Cat(String name, int age = 0) {
        super(name, age);
    }

    @Override
    public void speak() {
        print(name + " says meow");
    }
}

public final class Bird extends Animal {
    public Bird(String name, int age = 0) {
        super(name, age);
    }

    @Override
    public void speak() {
        print(name + " sings");
    }
}

// Top-level entry: this file is named main.jux, so these statements run at start.
var zoo = new List<Animal>();
zoo.add(new Dog("Rex", age: 3));
zoo.add(new Cat("Whiskers"));
zoo.add(new Bird("Tweety", age: 1));

// Teach Rex some tricks (only Dogs are Trainable)
for (var animal : zoo) {
    animal.introduce();
    animal.speak();

    if (animal instanceof Dog d) {
        d.learn("sit");
        d.learn("roll over");
    }
}

print("Zoo has " + zoo.size() + " animals");
```

This program exercises: top-level statements, sealed inheritance hierarchies, interfaces with default methods, abstract classes, record-style constructors with default arguments, polymorphism through a `List<Animal>`, pattern matching with `instanceof`, and the borrow checker quietly enforcing safety throughout.

---

## 16. Embedded and Bare-Metal Targets

Jux is designed to scale from cloud servers to 32KB microcontrollers. The same syntax, the same borrow checker, and the same FFI model apply on every target. What changes between targets is which features are available, controlled by the build profile (§2.4).

### 16.1. The Three Profiles

| Profile | Heap | Refcount | Exceptions | Threads | std tier 2 | Typical target |
|---|---|---|---|---|---|---|
| `jux-full` | Yes | Yes | Yes | Yes | Yes | Linux, macOS, Windows |
| `jux-embedded` | Optional | Optional | Optional | Optional | If OS | ESP32, STM32, Pi Pico |
| `jux-core` | No | No | No | No | No | ATmega, bootloaders, kernels |

The compiler enforces the profile at compile time. Importing a Tier 2 module from a `jux-core` build is a clean error, naming the module and suggesting an alternative.

### 16.2. Allocators

In `jux-embedded` and `jux-core`, code that allocates must say where the memory comes from. Collections accept an optional allocator parameter:

```java
import std.collections.List;
import core.embedded.{StaticAllocator, GlobalHeap};

// Stack-allocated, no heap at all
var samples = new int[64];

// Static memory pool (lives in .bss, no heap manager)
var pool = new StaticAllocator<2048>();
var queue = new List<Event>(pool);

// Global heap (only available if linked)
var dynamic = new List<Event>(GlobalHeap);
```

In `jux-full`, the global heap is the default — `new List<Event>()` works without an allocator argument. The same code compiled for `jux-core` would fail to link unless an allocator is passed.

### 16.3. Hardware Access

Memory-mapped registers are declared with the `@register` annotation and accessed through the `volatile` modifier so the compiler does not optimize away the read or write:

```java
import core.embedded.Volatile;

@register(address = 0x40021000)
public static volatile uint RCC_AHB1ENR;

@register(address = 0x40020000)
public static volatile uint GPIOA_MODER;

@register(address = 0x40020014)
public static volatile uint GPIOA_ODR;

public void enableLED() {
    RCC_AHB1ENR = RCC_AHB1ENR | (1 << 0);                          // enable clock
    GPIOA_MODER = (GPIOA_MODER & ~(0b11 << 10)) | (0b01 << 10);    // pin 5 output
    GPIOA_ODR = GPIOA_ODR | (1 << 5);                              // turn on
}
```

### 16.4. Interrupt Handlers

```java
@interrupt(vector = 30)
public static void onTimerOverflow() {
    Timer.clearFlag();
    eventQueue.push(TimerTick.instance);
}
```

The compiler enforces interrupt-handler discipline: no allocation, no blocking calls, no virtual dispatch on types whose vtables aren't statically resolvable. If your handler violates these rules, you get a compile error pointing to the offending line.

### 16.5. Vendor SDKs

Every MCU vendor ships a C SDK. Jux consumes them through ordinary FFI (§8), with `juxc bindgen` generating the `native` declarations from the vendor's headers automatically:

```toml
# jux.toml
[build]
profile = "embedded"
target = "thumbv7em-none-eabihf"
linker_script = "stm32f407.ld"

[dependencies]
"c.stm32_hal" = { lib = "stm32_hal", headers = "Drivers/STM32F4xx_HAL_Driver/Inc" }
"c.cmsis" = { lib = "cmsis", headers = "Drivers/CMSIS/Include" }
```

```java
@extern(lib = "stm32_hal")
native {
    int HAL_Init();
    void HAL_GPIO_WritePin(GPIO_TypeDef* port, ushort pin, int state);
    void HAL_Delay(uint ms);
}
```

Same model on every platform: STM32, ESP-IDF, Nordic nRF, NXP, Microchip, Raspberry Pi Pico SDK. Anything that links as a C library links into Jux.

### 16.6. Real-Time Guarantees

`jux-core` guarantees:

- No hidden allocations
- No hidden destructor chains (single-ownership means destructors run once, in known order at known points)
- No exception unwinding (`throws` is unavailable; use `Result<T, E>` with the `?` operator)
- No reference-count traffic (no atomic operations on hot paths)
- Iterators and pattern matching compile to tight loops with no indirection

This brings Jux to feature-parity with `no_std` Rust for hard real-time work.

### 16.7. Error Handling Without Exceptions

When exceptions are unavailable, code uses `Result<T, E>` from `core.result`. The `?` operator propagates errors without `try`/`catch` ceremony:

```java
import core.result.Result;

public Result<Config, ConfigError> loadConfig() {
    var contents = readFile("config.txt")?;       // returns early if Err
    var parsed = parseConfig(contents)?;
    return Result.ok(parsed);
}
```

The compiler can also lower `throws` to `Result` automatically when targeting profiles that disable exceptions, so the same source code can be portable between `jux-full` (where it uses real exceptions) and `jux-embedded` (where it lowers to `Result`).

### 16.8. Inline Assembly

For boot code, atomic primitives, or cycle-counting work where the toolchain offers no alternative:

```java
public static int readSP() {
    var sp: int;
    asm("mov %0, sp" : "=r"(sp));
    return sp;
}
```

GCC-style inline asm syntax. Available only when the profile enables it (typically `jux-core` and `jux-embedded`).

### 16.9. Complete Embedded Example — STM32 LED Blinker

```java
// File: main.jux
// Profile: jux-core
// Target: thumbv7em-none-eabihf (Cortex-M4F, STM32F407)

package com.example.blinker;

import core.embedded.{Volatile, delay_ms};

@register(address = 0x40023830)
public static volatile uint RCC_AHB1ENR;

@register(address = 0x40020000)
public static volatile uint GPIOA_MODER;

@register(address = 0x40020014)
public static volatile uint GPIOA_ODR;

public void setup() {
    RCC_AHB1ENR = RCC_AHB1ENR | (1 << 0);
    GPIOA_MODER = (GPIOA_MODER & ~(0b11 << 10)) | (0b01 << 10);
}

public void main() {
    setup();
    while (true) {
        GPIOA_ODR = GPIOA_ODR ^ (1 << 5);
        delay_ms(500);
    }
}
```

This compiles to a self-contained `.elf` file you flash to the board. No runtime, no allocator, no exception machinery. Just direct hardware access with the borrow checker and type system still active. Final binary: a few KB.

---

## 17. Glossary

- **Borrow checker.** Static analysis that prevents data races and use-after-free by tracking which references to a value are alive at each point in the program.
- **Dead code elimination (DCE).** Removing functions and data the program never reaches from the final binary.
- **Link-time optimization (LTO).** Optimization performed across compilation units at link time, including aggressive inlining and dead code elimination.
- **Monomorphization.** A compilation strategy where each instantiation of a generic type or function produces specialized machine code. Faster than erasure, larger source-level binaries (but DCE cuts the unused parts).
- **Profile.** A compile-time selection (`jux-full`, `jux-embedded`, `jux-core`) controlling which language features and runtime services are available.
- **RAII (Resource Acquisition Is Initialization).** Pattern where a resource's lifetime is bound to an object's lifetime; the destructor releases the resource.
- **Refcounting.** Memory management strategy where each allocation tracks how many references point to it; freed when the count reaches zero.
- **Trait / Interface.** A contract describing operations a type must support. Jux uses `interface` (Java's terminology).
- **Type erasure.** Java's strategy where generic type parameters are removed at compile time. Jux does not use this.
- **Weak reference.** A reference that does not prevent its target from being collected. Used to break refcount cycles.

---

## 18. References and Inspiration

- **Java** — Syntax, OOP model, package system, exceptions, generics syntax, sealed types
- **Kotlin** — Nullable types, smart casting, named arguments, primary constructors, top-level functions, `internal` visibility
- **C#** — Asymmetric property visibility, structs vs classes, top-level statements, records
- **Rust** — Ownership and borrowing, monomorphization, traits, sealed-enum-style sum types, RAII via Drop, FFI model, Cargo, `no_std` profile model, `?` operator
- **Swift** — Reference counting + weak references, value-vs-reference type distinction, native compilation strategy, Embedded Swift profile
- **C++** — Templates (as a cautionary example), header-based interop pattern, `freestanding` mode
- **C** — Universal ABI, vendor SDK conventions, linker-section model for dead code elimination
- **Hylo (Val)** — Mutable value semantics without lifetime annotations
- **Carbon** — Familiar-syntax, new-semantics positioning
- **Move** — Linear types in a Java-shaped language

---

*End of document.*
