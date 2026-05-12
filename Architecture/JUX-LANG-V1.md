# Jux Language v1 — Architecture & Design Dossier

**Version:** 1.0 (consolidated design)
**Status:** Specification draft, integrating the inheritance/borrow and async addenda
**File extension:** `.jux`

This document consolidates the original Jux design dossier with the inheritance × borrow addendum and the async/await addendum (v2). Section numbering is unified. Gaps and remaining open questions are listed in §19.

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
| Async model                 | `async T` (Kotlin-style) | n/a | `async fn` returning `Future` | `async`/`await` | `co_await` |
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

| Profile | Target | Heap | Refcount | Exceptions | Threads | Async | Typical use |
|---|---|---|---|---|---|---|---|
| `jux-full` | Desktop, server | Yes | Yes | Yes | Yes | Yes | Apps, services, tools |
| `jux-embedded` | MCU with OS or RTOS | Optional | Optional | Optional | Optional | Single-threaded | ESP32, STM32, Pi Pico |
| `jux-core` | Bare metal, kernels | No | No | No | No | No | Bootloaders, ATmega-class MCUs |

Profile is set in `jux.toml`:

```toml
[build]
profile = "embedded"
target = "thumbv7em-none-eabihf"
```

The compiler enforces the profile at compile time. Using a feature not available in the current profile is a compile error with a clear message naming the missing feature and what to use instead.

Section 16 covers embedded and bare-metal targets in detail.

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
- A file may contain multiple type declarations, but **at most one of them may be `public`** (Java's rule). The public type's name must match the filename: `Foo.jux` may contain `public class Foo` plus any number of package-private (no modifier) or `internal` types. Free functions, constants, and type aliases at the file's top level may be `public` without restriction; the rule applies only to type declarations (`class`, `interface`, `struct`, `record`, `enum`).

### 3.2. Keywords (Reserved)

```
abstract     annotation   as           async        await
break        case         catch        class        const
continue     default      do           drop         else
enum         extends      final        finally      for
if           implements   import       init         interface
internal     move         native       new          package
permits      private      protected    public       record
return       sealed       sizeof       static       struct
super        switch       this         throw        throws
try          type         var          void         while
yield
```

The keywords `async` and `await` are reserved at the lexical level even when not all features are available in the current profile. `annotation` declares user-defined annotation types (see `JUX-ANNOTATIONS-ADDENDUM.md`). `break` and `continue` are loop-flow keywords (§A.2.8). `as` is the cast operator (§A.5) and the import-alias keyword (§4.2). `sizeof` is the compile-time size query (§5.9).

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
| Character   | `'a'`, `'\n'`, `'é'`               |
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
| `@extern(lib = "...", header = "...")` | Declares a foreign library binding                 | §8.1       |
| `@export`, `@export(name = "...")` | Marks a function as part of the C-callable surface     | §8.4       |
| `@export(cpp_wrapper = true)`      | Additionally generates a C++ wrapper header            | §8.3       |
| `@register(address = ...)`         | Memory-mapped hardware register                        | §16.3      |
| `@interrupt(vector = ...)`         | Interrupt service routine                              | §16.4      |
| `@entry`, `@entry(symbol = ..., convention = ...)` | Marks a function as the program entry point | `JUX-ENTRY-POINTS-ADDENDUM.md` |
| `@inline`, `@noinline`             | Hints to the compiler about inlining                   | —          |
| `@align(N)`                        | Forces alignment of a type or field                    | —          |
| `@async-init`                      | Module supports top-level await in initializer         | §10.1.7    |

Annotations may take parameters. Parameters use named-argument syntax: `@cfg(os = "linux", arch = "x86_64")`. Block form is supported for annotations that apply to several declarations at once: `@export { ... }`.

**Annotation names are case-insensitive.** `@Override`, `@override`, `@OVERRIDE`, and `@OvErRiDe` all refer to the same annotation. The compiler matches annotation references against declarations by lower-casing both sides before comparison. This applies uniformly to built-in annotations and user-defined ones.

```java
@Override          public void foo() { ... }   // canonical PascalCase
@override          public void foo() { ... }   // lowercase — same thing
@OVERRIDE          public void foo() { ... }   // SCREAMING — same thing
@cfg(os = "linux") public void bar() { ... }   // already lowercase, of course
@CFG(OS = "linux") public void bar() { ... }   // also valid, also same
```

The compiler echoes back whichever spelling the user wrote in error messages and `juxc fmt` does **not** normalize the casing — your code looks the way you wrote it. Convention is **PascalCase** (`@Override`, `@Deprecated`, `@Test`, `@Reflectable`) and lower-case for declarative annotations (`@cfg`, `@extern`, `@export`, `@register`, `@interrupt`, `@inline`, `@align`); both conventions are equally valid.

Annotation **parameter names** (e.g., the `os` in `@cfg(os = "linux")`) are also case-insensitive. The same lowercase-then-compare rule applies.

Two user-declared annotations that differ only in case are a duplicate-name error (`E0307`). You cannot declare both `@MyTag` and `@mytag` in the same scope — they're the same name.

User-defined annotations are declared with the `annotation` keyword (see `JUX-ANNOTATIONS-ADDENDUM.md`). The legacy form — an interface marked `@AnnotationType` — is still accepted for back-compat but `juxc fmt --modernize` rewrites it. New code should use `annotation`. The complete catalog of built-in annotations, full target/retention semantics, parameter rules, and compile-time processor support live in the annotations addendum.

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

Jux has two parallel naming styles for the integer/float primitives:

- **Java-family names** — `byte`, `short`, `int`, `long`, … `float`, `double`. Primary names; most readable for general code.
- **Width-explicit names** — `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`. Used when the bit width is load-bearing (embedded code, FFI, protocols, fixed array sizes).

Most pairs are **exact aliases** (same width, same Rust type). The `int`/`uint` pair is the deliberate exception — `int` is platform-sized, while `i32` is always 32-bit, so they are not aliases.

| Java-family | Width-explicit | Size               | Range                              |
|-------------|----------------|--------------------|------------------------------------|
| `bool`      | —              | 1 byte             | `true`, `false`                    |
| `byte`      | `i8`           | 8-bit              | -128 to 127 (signed)               |
| `ubyte`     | `u8`           | 8-bit              | 0 to 255 (unsigned)                |
| `short`     | `i16`          | 16-bit             | signed                             |
| `ushort`    | `u16`          | 16-bit             | unsigned                           |
| **`int`**   | — (none)       | **pointer-sized**  | signed; 32-bit on 32-bit targets, 64-bit on 64-bit targets |
| **`uint`**  | — (none)       | **pointer-sized**  | unsigned                           |
| —           | `i32`          | 32-bit             | signed (use when 32-bit is required) |
| —           | `u32`          | 32-bit             | unsigned (use when 32-bit is required) |
| `long`      | `i64`          | 64-bit             | signed                             |
| `ulong`     | `u64`          | 64-bit             | unsigned                           |
| `float`     | `f32`          | 32-bit             | IEEE 754 single                    |
| `double`    | `f64`          | 64-bit             | IEEE 754 double                    |
| `char`      | —              | 32-bit             | Unicode scalar value               |
| `void`      | —              | —                  | absence of value (return type only)|

### `int` is platform-dependent

**`int` is the platform's native signed integer**: 32 bits on a 32-bit target, 64 bits on a 64-bit target. This is the "natural" integer for the current build target — pick it when you want efficient arithmetic and don't care about exact wire format.

When you need an **exact width** — protocol/FFI/MCU register code, serialization, fixed array sizes, etc. — use the width-explicit name (`i32`, `i64`, etc.). Those never change size across targets.

There is **no width-explicit synonym for `int`/`uint`**. The platform-sized type already has a name: `int` (or `uint`). Width-explicit names exist precisely to express "exactly N bits, always" — that's meaningless for a platform-sized type, so the table simply doesn't list one.

The Java-family `int` predates the explicit-width style, so reading `int x = 0;` carries the "I don't care about size" intent the spec endorses. If you DO care, the explicit name spells it out.

### Default for unsuffixed integer literals

An unsuffixed integer literal like `42` defaults to `i32` (matching the Rust toolchain's literal defaulting). Code that needs `int` semantics should annotate the binding explicitly (`int x = 42;`) or use an `as` cast. This avoids surprise overflow when porting a `var x = 2_000_000_000;` from a 64-bit build environment to a 32-bit target.

### Value semantics

Primitives are value types. They are copied on assignment and passed by value. No boxing required for use as generic type parameters (this differs from Java; see §7.8 on generics).

### 5.2. Reference Types

**`class`** — heap-allocated reference type with identity. Shared on assignment. Supports inheritance, polymorphism, virtual methods. Default visibility for fields: `private`. Public classes are extendable by default; use `final` (or its synonym `const`) to forbid extension (§7.4).

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
| `() async -> R`     | Async function type (may suspend)               | full, embedded |
| `Task<T>`           | Handle to running async computation             | full, embedded |
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
StackString<32> name = "sensor_01";          // up to 31 chars + null
```

### 5.6. Type Inference and Immutable Bindings

Variable declarations are **Java-style** — type comes before the identifier, or use `var` to infer:

```java
// With explicit type — Java-style: Type identifier = expr;
int count = 42;
String name = "Alice";
List<User> users = new List<User>();
fn(int) -> int doubler = x -> x * 2;
StackString<32> sensorId = "sensor_01";

// With type inference — `var` keyword
var count = 42;                  // inferred as int
var name = "Alice";              // inferred as String
var users = new List<User>();    // inferred as List<User>
```

**There is no colon-annotation form.** Kotlin/TypeScript/Swift's `var name: Type = expr` syntax is **not** accepted; use the Java type-first form instead. The compiler rejects `var x: int = 5` with `E0144` and suggests `int x = 5` or `var x = 5`.

To declare an **immutable binding** (a value that cannot be reassigned), prefix with either `const` or `final`. **The two keywords are synonyms** — use whichever you prefer; the compiler treats them identically:

```java
const var pi = 3.14159;          // immutable; same as 'final var pi = 3.14159'
final var name = "Alice";        // immutable
const String greeting = "Hi!";   // immutable, explicit type
final int retries = 3;           // immutable, explicit type
```

Programmers coming from C/C++/Rust gravitate to `const`. Programmers coming from Java/Kotlin gravitate to `final`. Both forms are accepted, both produce identical behavior, and the compiler echoes back whichever spelling you wrote in error messages.

When the initializer is a compile-time-constant expression, the binding is additionally usable in compile-time contexts (const-generic args, `case` patterns, static field initializers). This is determined by the *expression*, not the keyword.

**`const` ≡ `final` everywhere.** The synonym extends uniformly across every position the keyword can appear:

| Position                         | `final` form              | `const` form (same meaning) |
|----------------------------------|---------------------------|----------------------------|
| Local variable                   | `final var x = 5;`        | `const var x = 5;`         |
| Function parameter               | `final int n`             | `const int n`              |
| Class field                      | `private final int n;`    | `private const int n;`     |
| Class declaration (no inheritance) | `public final class Foo` | `public const class Foo`   |
| Method declaration (no override)  | `public final void run()` | `public const void run()`  |

The compiler accepts either spelling, treats them identically, and echoes whichever you wrote in error messages. Programmers from C/C++/Rust gravitate to `const`; programmers from Java/Kotlin gravitate to `final`. Pick whichever reads more naturally.

Type inference is local — it never crosses function boundaries. Public function signatures must always have explicit types.

### 5.7. Type Aliases

```java
public type UserId = long;
public type Callback<T> = (T) -> void;
public type StringMap = Map<String, String>;
```

Type aliases are transparent — a `UserId` is interchangeable with a `long`. For nominal types that share an underlying representation but should not be interchangeable, use a wrapper struct or record.

### 5.8. Casts: Two Equivalent Forms

Both Java/C-style prefix casts and Rust/Kotlin-style postfix casts are accepted, with **identical semantics**:

```java
// Java/C/C++/C# style — prefix
var n = (int) someLong;
var d = (Dog) someAnimal;
var p = (byte*) buffer;          // (in unsafe { } only)

// Rust/Kotlin/Jux style — postfix
var n = someLong as int;
var d = someAnimal as Dog;
var p = buffer as byte*;         // (in unsafe { } only)
```

Both produce the same conversion, the same runtime check (when applicable), and the same exception on failure. Use whichever reads more naturally — programmers from C/Java/C# typically prefer `(int) x`, programmers from Rust/Kotlin prefer `x as int`. The compiler treats them identically and never warns about which form you chose.

The conversion rules are documented in `JUX-GRAMMAR-ADDENDUM.md` §A.5 and are the same regardless of which form you write. Both forms are also available inside `unsafe { }` blocks for raw-pointer casts.

### 5.9. Compile-Time Type Queries

Jux provides one compile-time type query: `sizeof`.

#### 5.9.1. `sizeof(T)` — size of a type

`sizeof(T)` returns the size in bytes of the type `T` as a `uint`. It is a **compile-time constant** — evaluated by the compiler, not at runtime. Usable in any context that accepts an integer constant expression (array sizes, const initializers, embedded register layouts).

```java
print(sizeof(byte));     // 1
print(sizeof(int));      // 4 or 8 — platform-dependent (int is platform-sized per §5.1)
print(sizeof(i32));      // 4 — always
print(sizeof(long));     // 8
print(sizeof(double));   // 8
print(sizeof(bool));     // 1
print(sizeof(Point));    // depends on Point's layout
```

#### 5.9.2. `sizeof(expr)` — size of a value's type

`sizeof(expr)` returns the size in bytes of the **type of `expr`**, also as a `uint`. The expression is **not evaluated** — only its type contributes to the result, so `sizeof(arr[i])` is safe even if `i` is out of bounds, and `sizeof(some_function_call())` does not call the function.

```java
var x = 5;
print(sizeof(x));        // same as sizeof(int) — x's declared type

var arr = new int[10];
print(sizeof(arr[0]));   // sizeof(int) — element type, never indexes arr
```

#### 5.9.3. Disambiguation rule

A `sizeof(...)` body may syntactically be either a type or a value-expression. Disambiguation uses a purely **syntactic** rule (no semantic analysis required):

1. **Primitive names** — `bool`, `byte`, `ubyte`, `short`, `ushort`, `int`, `uint`, `long`, `ulong`, `float`, `double`, `char` and width-explicit aliases (`i8`/`u8`/…/`f64`) — are always treated as **types**.
2. **Uppercase-leading single identifiers** are treated as **types** (Java convention: types are PascalCase).
3. **Lowercase-leading single identifiers** are treated as **values** (Java convention: variables are camelCase).
4. **Multi-segment paths** (`foo.Bar`, `std.io.Stream`) are treated as **types**.
5. **Compound expressions** (anything with operators, indexing, function calls) are treated as **values**.

If the convention doesn't fit your code (a lowercase-named type, or an uppercase-named variable), prefix with an explicit cast or use the value form via a temporary: `sizeof(someVarOfThatType)`.

#### 5.9.4. Operand restrictions

| Operand shape                                    | Status                                   |
|--------------------------------------------------|------------------------------------------|
| Primitive (`byte`, `int`, `f64`, …)              | OK                                       |
| Fully-applied generic (`List<int>`)              | OK                                       |
| User type (PascalCase)                           | OK                                       |
| Local variable (camelCase)                       | OK                                       |
| Compound expression (`arr[3]`, `x + y`)          | OK                                       |
| Unbound generic (`List`, `T` outside mono)       | `E0461 sizeof of unbound generic type`   |
| Open wildcard (`?`, `? extends T`)               | `E0462 sizeof of wildcard type`          |
| `void`                                            | `E0463 sizeof of void`                   |

#### 5.9.5. Return type

`sizeof(...)` returns `uint` (platform-sized unsigned per §5.1). The value is non-negative and fits in the platform's address space.

#### 5.9.6. Lowering (Phase 1)

- **Type form**: `sizeof(T)` lowers to `std::mem::size_of::<T_rust>()` where `T_rust` is the Rust spelling of `T`.
- **Value form**: `sizeof(expr)` lowers to `std::mem::size_of_val(&(expr_rust))`.

Both are `const fn` in current Rust, so the constant-expression property propagates into the emitted code.

#### 5.9.7. Grammar

```
primary     = … | sizeof-expr
sizeof-expr = 'sizeof' '(' (type | expression) ')'
```

The parser parses the operand as a generic expression; the type-vs-value classification (§5.9.3) is applied during lowering. `sizeof` joins the reserved keywords list in §3.2.

#### 5.9.8. Future siblings

Future revisions may add `alignof(T)` (alignment of T) and `typeof(expr)` (the type of an expression, useful in generic code) here as §5.9.9 and §5.9.10. Both follow the same parens-required, compile-time-constant style.

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
counter.increment();                    // OK
counter = new Counter();                // OK: var binding can be reassigned

final var locked = new Counter();
locked.increment();                     // OK: `final` only blocks reassignment, not mutation
locked = new Counter();                 // ERROR: cannot reassign a `final` binding
```

`final` (and its synonym `const`, per §5.6) on a binding means **non-reassignable, that's it** — exactly like Java's `final`, Kotlin's `val`, and C#'s `readonly`. Mutating method calls through a `final` reference are permitted; the borrow checker still tracks aliasing internally to ensure no two references mutate concurrently. There is no Rust-style "shared-access-only" overload of the keyword. To express "this value cannot be mutated," use a `record` (auto-immutable per §7.6) or a class with read-only `{ get; }` properties (per §M.7).

The same inference applies to free functions: parameters that are mutated through their bindings require exclusive access from the caller.

```java
public void resetCounter(Counter c) {
    c.increment();                      // c needs exclusive access — inferred
}
```

This keeps method declarations identical to Java. The safety guarantees are unchanged; only the bookkeeping moves from the syntax to the compiler.

**Virtual methods.** A method is *virtual* (its target chosen at runtime, not compile time) when it can be overridden — per §7.4.1, that means it's on a non-`final`/non-`const` class, has visibility `public` or `protected`, and is not itself marked `final`/`const`. **Interface methods are also virtual**: a call through an interface reference dispatches to the implementing type's method at runtime.

Mutation inference for virtual methods takes the union across all reachable overrides. If any override of a virtual method mutates `this`, every call to that method through a base-class or interface reference is treated as mutating, regardless of which override actually runs at the call site. Sealed hierarchies (§7.5) tighten this — only permitted subclasses contribute to the union. For `final` classes (the default), no override exists, so the per-method inference is exact. See §7.4.1.

Whole-program analysis is used to *narrow* the union when the receiver type is known precisely (e.g., after `=>` smart-casts, §7.12).

#### Worked Example — Virtual Dispatch Through a Class Hierarchy

```java
public class Shape {
    public void render() {
        // base-class default — non-mutating
    }
}

public final class Circle extends Shape {
    private double radius;
    public Circle(double radius) { this.radius = radius; }

    @Override public void render() {
        // non-mutating — only reads radius
        draw_circle(radius);
    }
}

public final class AnimatedRect extends Shape {
    private int frame;
    public AnimatedRect() { this.frame = 0; }

    @Override public void render() {
        frame = frame + 1;          // mutates `this`
        draw_rect(frame);
    }
}

public void redrawAll(List<Shape> shapes) {
    for (var s : shapes) {
        s.render();                 // virtual call — target chosen at runtime
                                    // mutation summary is OR over reachable overrides:
                                    // AnimatedRect mutates → call requires exclusive access
    }
}
```

If `redrawAll` is called with a `List<Shape>` that the borrow checker knows contains only `Circle`s (e.g., the static element type is `Circle` after a smart-cast), the call simplifies to a non-mutating exact dispatch. Otherwise it's treated as mutating.

#### Worked Example — Virtual Dispatch Through an Interface

```java
public interface Drawable {
    void render();                    // abstract — every implementer must provide
    default void renderTwice() {       // default method — overridable
        render();
        render();
    }
}

public final class Sprite implements Drawable {
    private Texture tex;
    public Sprite(Texture tex) { this.tex = tex; }

    @Override public void render() {
        gpu.blit(tex);
    }
}

public final class TextLabel implements Drawable {
    private String text;
    private int cursor;
    public TextLabel(String text) { this.text = text; this.cursor = 0; }

    @Override public void render() {
        cursor = cursor + 1;          // mutates — used for blink animation
        gpu.drawText(text, cursor);
    }
}

public void renderEach(List<Drawable> items) {
    for (var d : items) {
        d.render();                   // virtual call through Drawable
                                      // dispatches to Sprite::render OR TextLabel::render
                                      // at runtime
    }
}
```

Interface dispatch goes through the per-`(class, interface)` interface-table (per `JUX-LAYOUT-ABI-ADDENDUM.md` §L.2.3) — one indirect call to find the slot, then the method call. Same mechanism as virtual dispatch through a base class; the runtime cost is one extra pointer hop.

**Sealed interfaces** give exact analysis (the set of implementers is closed at compile time):

```java
public sealed interface Token permits Identifier, Number, Punctuation {
    String name { get; }
}

public final record Identifier(String value) implements Token {
    public String name => "ident";
}
public final record Number(double value) implements Token {
    public String name => "number";
}
public final record Punctuation(char ch) implements Token {
    public String name => "punct";
}

public void describe(Token t) {
    print(t.name);                    // virtual but exact — only 3 possible targets
}
```

For sealed types, the compiler knows the complete set of implementers, so the mutation union is exact and the optimizer can devirtualize.

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
users.add(alice.copy());         // or write whatever copy method your type defines
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

**The borrow checker and refcounting are complementary.** Refcounting guarantees memory is valid for as long as any reference exists. The borrow checker guarantees that during any mutation, no other reference is reading or writing. The two together provide the same end-user invariants Rust gives — no use-after-free, no data races — without exposing lifetimes. In `jux-embedded` and `jux-core`, where refcounting is off, the borrow checker takes on the additional duty of preventing dangling references; this is why those profiles enforce single-ownership move semantics on classes.

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

### 6.9. Borrowing Across Inheritance

This section specifies how the borrow checker treats values whose static type is a class participating in a hierarchy. The rules are designed so that no upcast, no virtual dispatch, and no `super` call can produce a borrow-check failure that requires the user to think about inheritance.

#### 6.9.1. Class Borrows Are Whole-Object

A reference to a class instance borrows the **whole object**, not individual fields. This is the central rule.

```java
public class Animal {
    protected String name;
    protected int age;
}

public final class Dog extends Animal {
    private List<String> tricks;
}

public void example(Dog d) {
    var t = d.tricks;        // borrows the whole `d` (shared)
    d.age = d.age + 1;       // ERROR: cannot mutate `d` while `t` borrows it
}
```

For class instances, the borrow checker tracks borrows at object granularity. There is no concept of "borrowing only the `tricks` field" of a class. To hold an independent reference to a field, copy or clone the value out:

```java
public void example(Dog d) {
    var trickCount = d.tricks.size();   // copies the int out
    d.age = d.age + 1;                  // OK: no live borrow on `d`
}
```

This is stricter than Rust on classes and looser than Rust on structs. The trade-off is deliberate: classes participate in inheritance and dynamic dispatch, where field-level reasoning would force lifetime annotations to recover precision. Structs, which never participate in inheritance, retain Rust-style field-disjoint borrows.

| Type kind | Borrow granularity | Field-level disjoint borrows |
|-----------|-------------------|------------------------------|
| `struct`  | per-field          | yes                          |
| `record`  | per-field          | yes (read-only)              |
| `class`   | whole-object       | no                           |
| `enum`    | per-variant payload| yes (after match)            |

Note: Reading a primitive field of a class produces a *momentary* borrow that ends with the read. The pattern `count = count + 1` evaluates `count` (read borrow ends), then assigns (write borrow taken) — a single statement with sequential, non-overlapping borrows. The whole-object rule applies to *named bindings* that hold a reference into the class across multiple statements, not to expression-internal reads.

#### 6.9.2. Upcasting Preserves the Borrow

Because borrows are whole-object, upcasting a borrowed reference does not change what is borrowed. The reference simply has a more general static type.

```java
public void describe(Animal a) {
    a.introduce();
}

public void example(Dog d) {
    describe(d);                        // shared borrow of the whole object
    d.age = d.age + 1;                  // OK after describe returns
}
```

Two consequences:

- **No upcast-while-borrowed-subtype-field problem.** That problem only exists in field-level borrow systems. Whole-object borrowing eliminates it.
- **Upcasting cannot strip a borrow.** Passing a `Dog` as `Animal` does not "convert" exclusive access into shared access, or vice versa. The mode is fixed by the call site, not by the static type.

#### 6.9.3. Virtual Dispatch and the Mutation Union

A call through a virtual method is treated as mutating if **any** override of that method (in any subclass reachable from the static type of the receiver) mutates `this`. The mutation summary of a virtual method is the transitive union of:

- The method body of the declaring class (if not abstract)
- Every override in every subclass that is reachable from the receiver type

For sealed types (§7.5), the set of reachable subclasses is closed and exact. For non-sealed extendable classes, the set is open: the compiler must include all overrides linked into the program (whole-program analysis after monomorphization, §2.5).

```java
public class Shape {
    public void render() { ... }       // does not mutate
}

public final class AnimatedShape extends Shape {
    private int frame;

    @Override
    public void render() {
        frame = frame + 1;                  // mutates this
        super.render();
    }
}

public void redraw(Shape s) {
    s.render();                             // treated as mutating: AnimatedShape::render mutates
}
```

If this is too conservative for your use case, declare a more specific receiver type (the borrow checker uses the precise type) or mark the class `final` (no overrides exist). Sealed hierarchies give exact mutation summaries with no whole-program cost.

#### 6.9.4. `super` Calls

A `super` call resolves statically to the named superclass method. Its mutation summary is exact — it is the inferred summary of the named method, not a virtual union.

```java
public final class Dog extends Animal {
    @Override
    public void speak() {
        super.introduce();          // exact: Animal::introduce, non-mutating
        print("woof");
    }
}
```

Because `super` is statically resolved, it does not contribute to the virtual union of any method.

#### 6.9.5. `protected` Fields and Subclass Mutation

Subclasses may read and write `protected` fields of their superclass. The borrow checker treats every such access as a borrow of the receiver, with the same whole-object rule as §6.9.1. A subclass cannot hold a long-lived borrow on `super.field` independent of `this`.

The momentary-borrow exception from §6.9.1 still applies: reading a primitive `protected` field, performing arithmetic, and writing back is a sequence of non-overlapping borrows and compiles cleanly.

```java
public class Animal {
    protected int age;
}

public final class Dog extends Animal {
    public void birthday() {
        age = age + 1;              // OK: read (momentary) then write (momentary)
    }

    public void birthdayWithReceipt() {
        var prev = age;             // OK: copies primitive, no live borrow
        age = prev + 1;
        return prev;
    }
}
```

The borrow conflict only arises when a *reference-typed* field is named and held across a mutation:

```java
public final class Dog extends Animal {
    public void example() {
        var first = tricks.first();  // shared borrow of `this` (via tricks)
        tricks.add("sit");           // ERROR: cannot mutate while `first` is alive
    }
}
```

#### 6.9.6. Generics and Variance Across Inheritance

Generic types are invariant in their type parameters (§7.8). Variance is expressed at use sites via wildcards:

- `List<Dog>` is **not** assignable to `List<Animal>`. (The list could be mutated.)
- `List<? extends Animal>` accepts `List<Dog>`. (Read-only with respect to `T`.)
- `List<? super Dog>` accepts `List<Animal>`. (Write-only with respect to `T`.)

The borrow checker treats wildcards as opaque: a `List<? extends Animal>` exposes only the read API, so its inferred mutation summary is the union of read-only methods. This composes with §6.9.3 cleanly because variance and mutability are decided independently.

#### 6.9.7. Sealed Hierarchies Give Exact Analysis

Within a sealed hierarchy (§7.5), the compiler knows every subclass at compile time. This produces:

- Exact virtual mutation summaries (no over-approximation)
- Exhaustive pattern matching with full type narrowing
- Devirtualization opportunities for the optimizer

When borrow inference fails on a non-sealed (extendable) hierarchy, sealing it is often the cheapest fix. The error message points this out:

```
Error: cannot mutate `s` here
  --> render.jux:18:5
   |
16 |     for (var s : shapes) {
17 |         s.render();
   |         --------- treated as mutating: at least one override of `Shape::render` mutates
18 |         shapes.add(makeNewShape());
   |         ^^^^^^ cannot mutate `shapes` while iteration borrows it
   |
Hint: if you control the Shape hierarchy, consider sealing it. Sealed hierarchies
give exact mutation analysis. See §7.5.
```

#### 6.9.8. Embedded and Bare-Metal Profiles

In `jux-embedded` and `jux-core`, refcounting is off (§6.5). Class instances follow single-ownership move semantics. The whole-object borrow rule still applies — there is no field-level borrow into a class — and inheritance still works, but every reference to a class instance is either the unique owner or an exclusive/shared borrow of that owner. This is the most restrictive setting and produces the smallest code; it is also the setting that maps most directly to Rust's borrow model.

A class hierarchy used in `jux-core` typically wants:

- All non-leaf classes `final` (no virtual dispatch overhead)
- Or a sealed hierarchy with virtual dispatch through a static vtable
- Sharing expressed explicitly via `SharedRef<T>` (refcount on demand)

#### 6.9.9. Summary Table

| Situation | Rule | Where enforced |
|-----------|------|----------------|
| Borrow a field of a class instance | Borrows the whole instance (long-lived bindings only) | §6.9.1 |
| Read a primitive field of a class | Momentary borrow, ends with the read | §6.9.1, §6.9.5 |
| Borrow a field of a struct/record  | Borrows that field only    | §6.1, §6.9.1 |
| Upcast a borrowed class reference  | Borrow mode is unchanged   | §6.9.2 |
| Call a virtual method              | Mutating iff any reachable override mutates | §6.9.3 |
| Call `super.method`                | Exact summary of named method | §6.9.4 |
| Mutate a `protected` superclass field from a subclass | Treated as mutating `this` | §6.9.5 |
| Pass `List<Dog>` where `List<Animal>` expected | Rejected (invariance) | §7.8 |
| Pass `List<Dog>` where `List<? extends Animal>` expected | Accepted, read-only API | §7.8 |
| Pattern match on a sealed type     | Exact narrowing, exhaustive | §7.5 |

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

// Async function — see §10.1
public async String fetchUser(int id) {
    var response = await http.get($"/users/$id");
    return await response.text();
}
```

The variadic parameter is bound to a `String[]` inside the function body. Variadic must be the last parameter; only one variadic parameter per function.

### 7.3. Class Declarations

```java
public class User {                         // final by default
    // Fields and properties (C#-style; see §M.7)
    private String passwordHash;
    public String name { get; private set; }     // public read, private write
    public int age    { get; private set; }      // public read, private write
    public String email { get; set; }            // public read and write
    public String fullId => $"user-${name}";     // expression-bodied read-only

    // Primary constructor — name matches the class name (Java style)
    public User(String name, int age, String email = "") {
        this.name = name;
        this.age = age;
        this.email = email;
        this.passwordHash = "";
    }

    // Secondary constructor — delegates with `this(...)`
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

#### 7.3.1. Constructors

Constructors are declared with the **class name** as the method name, matching Java. The earlier `public new(...)` form is dropped — `new` is the **construction operator** used at call sites (`new User("Alice", 30)`), not part of the constructor declaration.

**Overloading.** Multiple constructors are distinguished by parameter list:

```java
public class User {
    private String name;
    private int age;
    private String email;

    public User(String name) {
        this(name, 0, "");
    }

    public User(String name, int age) {
        this(name, age, "");
    }

    public User(String name, int age, String email) {
        this.name = name;
        this.age = age;
        this.email = email;
    }
}
```

`this(...)` delegates to another constructor in the same class. It must be the **first statement** in the constructor body. A constructor body may contain *either* a `this(...)` delegation *or* a `super(...)` parent call as its first statement — not both.

**Parent construction.** `super(...)` invokes a parent class constructor (see §7.4):

```java
public class Dog extends Animal {
    private List<String> tricks;

    public Dog(String name) {
        super(name);                          // calls Animal(String)
        this.tricks = new List<>();
    }
}
```

`super(...)` must be the first statement of the constructor body. If a class has a non-trivial parent constructor (no zero-arg form) and the subclass does not invoke `super(...)` explicitly, the compiler reports `E0312` (missing super call).

**Default constructor.** If no constructor is declared, the compiler synthesizes an implicit zero-argument constructor that calls `super()` (when a parent exists) and initializes fields to their declared defaults. Declaring *any* constructor removes the implicit one.

**Visibility.** Constructors carry their own visibility modifier and follow the same rules as methods. Private constructors enable factory and singleton patterns:

```java
public class Singleton {
    private static Singleton instance;

    private Singleton() { }

    public static Singleton getInstance() {
        if (instance == null) {
            instance = new Singleton();
        }
        return instance;
    }
}
```

**Init blocks.** For initialization logic shared across multiple constructors, see `JUX-MISSING-DEFS-ADDENDUM.md` §M.1 (`init { ... }` blocks).

**Record constructors.** Records get an implicit primary constructor from their declaration plus optional compact-form validation; see §7.6.

 Mark a class `final`/`const` to forbid extension; otherwise, a subclass elsewhere in the same module (or, for `public` classes, in any consuming module) can `extends` it.

```java
public class Foo { }                  // extendable (default)
public final class Bar { }            // not extendable
public const class Baz { }            // identical to `final` — not extendable
```

For methods: `public const void render() { ... }` ≡ `public final void render() { ... }` — both forbid override (per §7.4.1).

**Nested types.** A class may declare other types inside it — nested classes, structs, records, enums, and interfaces. All nested types are **static** by definition (they have no implicit reference to the enclosing instance). Per `JUX-MISSING-DEFS-ADDENDUM.md` §M.9, inner classes (Java's non-static nested with implicit outer reference), anonymous classes, and local classes are **not** supported — lambdas and explicit composition cover those cases more cleanly.

```java
public class HttpServer {
    public final class Config {              // nested type — namespaced under HttpServer
        public int port { get; set; }
        public int maxConnections { get; set; }
    }

    public final class Request {              // another nested type
        public String path;
        public Map<String, String> headers;
    }

    private Config config;

    public HttpServer(Config config) { this.config = config; }
}

// Usage from outside:
var cfg = new HttpServer.Config();
cfg.port = 8080;
var server = new HttpServer(cfg);
```

Outer-class private members are accessible from a nested type and vice-versa, exactly like Java. Visibility of a nested type is the more restrictive of its declared visibility and the enclosing type's.

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

public class Dog extends Animal implements Trainable {  // final by default
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

A class may extend exactly one class and implement any number of interfaces. The `final` modifier (default) prevents further inheritance. The `open` modifier permits it. To extend a class, the parent must be marked `open` or `abstract`.

#### 7.4.1. Method Overridability — The Rule

A method **can be overridden** by a subclass when **all** of these hold:

1. The declaring class is **not** `final` or `const` — that is, it permits inheritance. By default, every class permits inheritance unless explicitly sealed with `final`/`const`. (Abstract classes always permit inheritance — they cannot be `final`.)
2. The method's visibility is `public` or `protected` — `private` methods are not part of inheritance, and package-private methods are overridable only by subclasses in the same package.
3. The method is **not** marked `final` or `const`. (`const` ≡ `final` per §5.6.)

In short: **on an extensible class, every public/protected method is overridable unless explicitly marked `final` or `const`.** That's the default — Java's rule, applied uniformly. Mark `final class` (or `const class`) when you want to forbid extension; mark `final void method()` when you want to forbid override.

| Method on class C       | Visibility   | Modifier         | Overridable in subclass S extends C? |
|-------------------------|--------------|------------------|--------------------------------------|
| `final class C`         | any          | any              | No — C admits no subclasses          |
| `class C` (default)     | `public`     | (none)           | **Yes** — default                    |
| `class C` (default)     | `public`     | `final`/`const`  | No — explicitly sealed at the method |
| `class C` (default)     | `protected`  | (none)           | **Yes** — default                    |
| `class C` (default)     | `protected`  | `final`/`const`  | No                                   |
| `class C` (default)     | (no modifier — package-private) | (none) | Yes, but only from same-package subclass |
| `class C` (default)     | `private`    | (any)            | No — private methods are not inherited |
| `abstract class C`      | any          | `abstract`        | **Required** — concrete subclass must override |

```jux
public class Animal {
    public void greet() { print("Hello"); }       // overridable (default)
    public final void breathe() { ... }           // NOT overridable — final
    public const void heartbeat() { ... }         // NOT overridable — const (synonym)
    protected void hunt() { ... }                 // overridable (default)
    private void digest() { ... }                 // NOT inherited at all
}

public class Dog extends Animal {
    @Override public void greet() { print("Woof"); }    // OK — was overridable
    @Override public void breathe() { ... }              // ERROR — was final
    @Override public void heartbeat() { ... }            // ERROR — was const
    @Override protected void hunt() { ... }              // OK
    @Override private void digest() { ... }              // ERROR — private isn't inherited
}
```

The `@Override` annotation is recommended on every override. The compiler emits a warning (`W0470`) for an override missing it, and an error (`E0471`) if the method below claims `@Override` but doesn't actually override anything.

#### 7.4.2. Virtual Methods and Mutation Inference

A method participates in **dynamic dispatch** (the call's target is selected at runtime) when it is overridable per §7.4.1. Methods on `final` classes, methods marked `final` or `const`, `private` methods, and `static` methods are **statically resolved** — the compiler picks the target at compile time.

Mutation is inferred per method body (§6.3). For virtual methods, the inferred mutability of a call site is the **union** of the mutabilities of every reachable override (§6.9.3). To make this analysis tractable and produce stable error messages:

- **Mark classes `final` (or `const`) when you don't intend them to be subclassed.** A `final` class has exact mutation analysis at every call, no widening. This is the Java best-practice; Jux makes it cheap by allowing either spelling.
- **Prefer sealed hierarchies for controlled polymorphism.** A `sealed` class or interface has a closed set of permitted subtypes, so the mutation union is exact and stable across compilation units. Use `sealed class C permits A, B, C` (or `sealed interface I permits ...`) when the family of subtypes is fixed at design time.
- **Be careful with publicly-extendable classes.** A public non-`final` class accepts overrides defined in any module that imports it. The mutation union over such a class includes every override linked into the final binary, so adding a new subclass in a downstream module can change a base method's inferred mutability and trigger a borrow error in code that previously compiled. The compiler reports such transitions clearly:

```
Error: introducing `LoggingShape extends Shape` changes the mutability of `Shape::render`
  --> logging_shape.jux:8:5
   |
 8 |     @Override
 9 |     public void render() {
10 |         lastRender = clock.now();    // mutates this
   |         ^^^^^^^^^^ this override mutates; previous overrides did not
   |
Affected call sites:
  render.jux:42:9 — `s.render()` where `s: Shape`
    was: shared borrow
    now: exclusive borrow
    consequence: 3 lines below now fail to compile
```

This is the same class of breakage that Java introduced with non-final-by-default — but here the compiler catches it at the point of introduction, not as a runtime fragile-base-class bug. Sealing the base class prevents the issue entirely.

##### Worked Example

```java
public sealed class Shape permits Circle, Square, AnimatedRectangle {
    public void render() { /* default: non-mutating */ }
}

public final class Circle extends Shape {
    private double radius;
    @Override public void render() { /* non-mutating */ }
}

public final class Square extends Shape {
    private double side;
    @Override public void render() { /* non-mutating */ }
}

public final class AnimatedRectangle extends Shape {
    private int frame;
    @Override public void render() { frame = frame + 1; }   // mutates
}

// Mutation summary of Shape::render is "may mutate" (because of AnimatedRectangle).
// Call sites through Shape see exclusive-borrow requirements:

public void redraw(Shape s) {
    s.render();                     // exclusive borrow of s
}

public void redrawCircle(Circle c) {
    c.render();                     // shared borrow — exact analysis on the leaf type
}
```

The cost of polymorphism in the borrow system is conservative widening at call sites whose receiver is the base class. Sealed hierarchies make that widening exact. `final` leaves recover full precision when the receiver type is precise.

#### 7.4.3. Default Methods on Interfaces

Interfaces may declare **abstract methods** (no body — implementers must provide one) and **default methods** (with a body — implementers inherit the implementation, may override). Java 8+ syntax exactly. Default methods are the way to share workflow code across many implementers without resorting to abstract base classes.

The pattern is most powerful when defaults call into abstract methods — the implementer fills in the small variation points, the interface provides the high-level workflow.

##### Worked Example — A Generic Repository<T>

```java
public interface Repository<T> {
    // -------- abstract methods (implementers MUST provide) --------
    T? findById(int id);
    void save(T item);
    void deleteById(int id);
    Iterable<T> all();

    // -------- default methods (implementers MAY override) --------

    default bool exists(int id) {
        return findById(id) != null;
    }

    default int count() {
        var n = 0;
        for (var _ : all()) n = n + 1;
        return n;
    }

    default T findOrThrow(int id) throws NotFoundError {
        var item = findById(id);
        if (item == null) throw new NotFoundError($"id $id not in $this");
        return item;
    }

    default void saveAll(Iterable<T> items) {
        for (var item : items) save(item);
    }

    default void deleteAll(Iterable<int> ids) {
        for (var id : ids) deleteById(id);
    }

    default Iterable<T> findWhere((T) -> bool pred) {
        return all().filter(pred);
    }

    default int countWhere((T) -> bool pred) {
        var n = 0;
        for (var item : all()) if (pred(item)) n = n + 1;
        return n;
    }

    // Default methods can be expression-bodied properties too
    default bool isEmpty => count() == 0;
}
```

A minimal implementer writes only the four abstract methods:

```java
public final class UserRepo implements Repository<User> {
    private Map<int, User> storage = new Map<>();

    @Override public User? findById(int id) {
        return storage.contains(id) ? storage.get(id) : null;
    }

    @Override public void save(User u) {
        storage.put(u.id, u);
    }

    @Override public void deleteById(int id) {
        storage.remove(id);
    }

    @Override public Iterable<User> all() {
        return storage.values();
    }

    // Everything else (exists, count, findOrThrow, saveAll, deleteAll,
    // findWhere, countWhere, isEmpty) comes for free from the interface.
}
```

Usage immediately gets the full API:

```java
var repo = new UserRepo();
repo.save(new User(1, "Alice"));
repo.save(new User(2, "Bob"));

print(repo.exists(1));                          // true
print(repo.count());                            // 2
print(repo.isEmpty);                            // false
var alice = repo.findOrThrow(1);
var adults = repo.findWhere(u -> u.age >= 18);
```

##### Overriding a Default for a Better Implementation

An implementer with a smarter approach overrides any default it wants:

```java
public final class CachedUserRepo implements Repository<User> {
    private Map<int, User> storage = new Map<>();
    private int cachedCount = 0;

    @Override public User? findById(int id) { ... }
    @Override public void save(User u) {
        if (!storage.contains(u.id)) cachedCount = cachedCount + 1;
        storage.put(u.id, u);
    }
    @Override public void deleteById(int id) {
        if (storage.contains(id)) cachedCount = cachedCount - 1;
        storage.remove(id);
    }
    @Override public Iterable<User> all() { return storage.values(); }

    // Override the default — O(1) instead of O(N)
    @Override public int count() { return cachedCount; }

    // isEmpty's default body is `count() == 0`, so it automatically picks
    // up the optimized count() with no further override.
}
```

##### Diamond Conflict — When Two Interfaces Collide

When a class implements two interfaces that both supply the same default method, the compiler **requires** the class to override (`E0431`):

```java
public interface Walker {
    default void move() { print("walking"); }
}

public interface Swimmer {
    default void move() { print("swimming"); }
}

// ERROR E0431: conflicting defaults — class must override `move`
public class Penguin implements Walker, Swimmer { }

// Fix — explicit choice via explicit delegation:
public class Penguin implements Walker, Swimmer {
    @Override public void move() {
        Walker.super.move();              // explicitly delegate to one default
        // or Swimmer.super.move();
        // or write your own body
    }
}
```

The `InterfaceName.super.method()` form (Java syntax) explicitly invokes a specific inherited default. Without it, the compiler will not silently pick one — explicit override is required.

##### Template-Method Pattern (Default Calls Abstract)

The most powerful pattern: defaults call into abstract methods, so the workflow is fixed but the variation points are filled by each implementer.

```java
public interface JsonSerializable {
    void writeFields(JsonWriter w);            // abstract — implementer provides

    default String toJson() {                   // workflow — comes for free
        var w = new JsonWriter();
        w.startObject();
        writeFields(w);
        w.endObject();
        return w.toString();
    }
}

public final class User implements JsonSerializable {
    public int id; public String name;

    @Override public void writeFields(JsonWriter w) {
        w.field("id", id);
        w.field("name", name);
    }
    // toJson() is provided automatically.
}

print(new User(1, "Alice").toJson());          // {"id":1,"name":"Alice"}
```

##### Interface Inheritance with Defaults

Interfaces can extend other interfaces; defaults flow down:

```java
public interface Reader {
    int? readByte();                            // abstract
}

public interface CountingReader extends Reader {
    int bytesRead { get; }                      // property contract

    // Inherits readByte() abstract from Reader.
    // Adds a default that uses readByte:
    default byte[] readN(int n) {
        var result = new byte[n];
        for (var i = 0; i < n; i++) {
            var b = readByte();
            if (b == null) break;
            result[i] = b as byte;
        }
        return result;
    }
}
```

##### Rules Summary

- Default methods are virtual (overridable per §7.4.1) unless marked `final`/`const`.
- Default methods can call other default methods, and can call abstract methods (template-method pattern).
- Two interfaces supplying the same default name + signature trigger a diamond conflict; the implementing class must override (`E0431`).
- Default methods participate in mutation inference (§6.3) like any virtual method — the union over reachable overrides determines whether a call site is mutating.
- Sealed interfaces (`sealed interface I permits A, B`) with default methods give the compiler an exact set of implementers, enabling devirtualization and exact mutation analysis.

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

**Switch uses `->` only — no fallthrough.** Every case body is self-contained; no `break` is needed. Cases may also bundle multiple patterns or use blocks for multi-statement bodies:

```java
public void handle(HttpStatus s) {
    switch (s) {
        case OK, CREATED, ACCEPTED -> log.info("success");           // multi-pattern
        case NOT_FOUND -> log.warn("missing");
        case SERVER_ERROR -> {                                         // block body
            log.error("server failure");
            metrics.incrementErrorCount();
        }
        default -> log.warn("unhandled status");
    }
}
```

This matches Java 14+ arrow-switch syntax exactly. The legacy C/Java `case X:` colon form with implicit fallthrough is **not** supported — `case` always uses `->`.

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

#### 7.6.1. Record Constructors

A record's header declares its **primary constructor** (`record Vector3(double x, double y, double z)`). The compiler synthesizes a canonical constructor that assigns each header parameter to the corresponding field. No body is required for that.

For validation logic, records support the **compact-form constructor** (matching Java 14+):

```java
public record Point(double x, double y) {
    public Point {                            // compact constructor — no parameter list
        if (Double.isNaN(x) || Double.isNaN(y)) {
            throw new IllegalArgumentException("NaN coordinates");
        }
        // parameters are then implicitly assigned to fields
    }
}
```

The compact form runs *before* the implicit field assignments. It may reassign the parameters (e.g., to normalize them); the (possibly modified) values are then written to the fields.

**Additional constructors** delegate to the primary one:

```java
public record Point(double x, double y) {
    public Point() {                          // additional zero-arg constructor
        this(0.0, 0.0);
    }
}
```

Any constructor of a record must end with the fields fully initialized. The compiler enforces this — secondary constructors that don't reach the primary one via `this(...)` are an error.

### 7.7. Enums (Sum Types — Best of C, Java, Rust)

Jux enums are **tagged unions** that subsume all three traditional enum styles:

- **C-style** (just integer constants — for FFI, simple state codes).
- **Java-style** (named variants with methods).
- **Rust-style** (sum types with payloads — algebraic data types).

All three are written with the same `enum` construct. The compiler picks the most efficient representation for each.

#### 7.7.1. Three Patterns, One Construct

Variant declarations are **comma-separated** inside the enum body. No `case` keyword in the declaration — that's only for `switch` patterns. (Java-shape syntax for the no-payload form, Rust-shape for the payload form, the same construct for both.)

**Pattern A — No-payload enum (C-style + Java-style):**

```java
public enum Direction {
    North, South, East, West;

    // Java-style: methods on enums work
    public Direction opposite() {
        return switch (this) {
            case North -> South;
            case South -> North;
            case East  -> West;
            case West  -> East;
        };
    }
}

// Pattern matching is exhaustive — compiler verifies
var d = Direction.North;
print(d.opposite());                // South

// Java-style helpers auto-provided (see §7.7.3)
print(d.name());                    // "North"
print(d.ordinal());                 // 0
print(Direction.values());          // [North, South, East, West]
print(Direction.fromName("east"));  // East (case-insensitive lookup)
```

A semicolon (`;`) separates the variant list from any methods that follow. If the enum has no methods, the trailing semicolon is optional.

**Pattern B — C-compatible enum for FFI:**

```java
@layout(c, repr = "i32")
public enum HttpStatus {
    Ok = 200,
    NotFound = 404,
    ServerError = 500
}

// Bit-identical to a C `int` enum at the FFI boundary.
// Discriminator values are explicit.
```

The `@layout(c, repr = "...")` annotation pins the underlying integer type (`i8`, `i16`, `i32`, `u8`, etc.). The enum's variants become integer constants in the named representation. No payloads allowed in this form. Ideal for hardware codes, protocol enums, FFI.

**Pattern C — Sum types with payloads (Rust-style ADTs):**

```java
public sealed enum HttpResponse {
    Ok(int status, String body),
    Redirect(String location),
    Error(int code, String message);

    public bool isSuccess() {
        return switch (this) {
            case Ok(_, _) -> true;
            case _ -> false;
        };
    }
}

public void handle(HttpResponse r) {
    switch (r) {
        case Ok(var status, var body) -> print($"$status: $body");
        case Redirect(var url) -> print($"-> $url");
        case Error(var code, var msg) -> print($"$code: $msg");
    }
}
```

Variants may carry heterogeneous payloads. The compiler emits a tagged union (per `JUX-LAYOUT-ABI-ADDENDUM.md` §L.2.4). Pattern matching destructures the active variant. Exhaustiveness is verified.

#### 7.7.2. Auto-Derived for Every Enum

Every enum auto-provides (per `JUX-OPERATORS-ADDENDUM.md` §O.3):

- `operator==(Self)` — variant + payload structural equality.
- `operator hash()` — combined hash of variant tag + payload hashes.
- `operator string()` — `"VariantName"` for no-payload, `"VariantName(field: ..., ...)"` for payloads.
- Implicit copy on assignment.

#### 7.7.3. Auto-Derived Helpers (Java-style API)

Every enum gets these auto-generated methods, available on instances and statically on the type:

| Method                              | Available on            | Returns                                       |
|-------------------------------------|--------------------------|-----------------------------------------------|
| `value.name()`                      | any enum instance        | `String` — the variant's declared name        |
| `value.ordinal()`                   | any enum instance        | `int` — zero-based declaration index          |
| `Self.values()`                     | no-payload enums only    | `List<Self>` — every variant in declaration order |
| `Self.fromName(String)`             | no-payload enums only    | `Self?` — null on miss; case-insensitive       |
| `Self.fromOrdinal(int)`             | no-payload enums only    | `Self?` — null if out-of-range                 |
| `Self.cases()`                      | any enum                 | `List<EnumCase<Self>>` — variant descriptors  |

`Self.values()` is restricted to no-payload enums because variants with payloads can't be enumerated without invented payload data. For payload-carrying enums, `Self.cases()` returns descriptors (variant name, ordinal, payload type signature) — useful for reflection.

`fromName` is **case-insensitive** by default (matches `"North"`, `"north"`, `"NORTH"`). For strict matching, use `fromNameStrict(String)`.

```java
public enum Color { Red, Green, Blue }

var c = Color.fromName("RED");      // Color.Red
var d = Color.fromOrdinal(1);        // Color.Green
var all = Color.values();            // [Red, Green, Blue]
print(c.name());                     // "Red"
print(c.ordinal());                  // 0
```

#### 7.7.4. Methods on Enums

Enums (any flavor) may declare methods, fields (`const` only — no instance state separate from variants), and static functions:

```java
public enum Planet {
    Mercury(3.303e23, 2.4397e6),
    Venus(4.869e24, 6.0518e6),
    Earth(5.976e24, 6.37814e6);
    // ... (semicolon separates variants from the methods that follow)

    private final double mass;            // payload field
    private final double radius;

    public Planet(double mass, double radius) {
        this.mass = mass;
        this.radius = radius;
    }

    public double surfaceGravity() {
        const double G = 6.67300e-11;
        return G * mass / (radius * radius);
    }
}

print(Planet.Earth.surfaceGravity());    // 9.802...
```

This is the same shape as Java's enum-with-fields, but with all the extra power of payload-carrying variants.

#### 7.7.5. Sealed by Default

Enums are **sealed** — the compiler knows every variant. This enables:

- **Exhaustive pattern matching** without `default` clauses.
- **Exact mutation analysis** for borrow inference (per JUX-INHERITANCE-BORROW-ADDENDUM §6.9.7).
- **Niche optimization** in layout (per JUX-LAYOUT-ABI §L.1.6).

You cannot extend an enum from outside its declaration. To add new variants, edit the enum source. To allow open extension, use `sealed interface` + `record` implementations instead (V1 §7.5).

#### 7.7.6. Comparison

| Feature                          | C enum | Java enum | Jux enum |
|----------------------------------|--------|-----------|----------|
| Named variants                   | ✓      | ✓         | ✓        |
| Methods                          | ✗      | ✓         | ✓        |
| Payloads on variants             | ✗      | ✗         | ✓        |
| Pattern matching with destructuring | ✗   | partial   | ✓        |
| Exhaustiveness check             | ✗      | ✓         | ✓        |
| `name()`, `ordinal()`, `values()` | ✗     | ✓         | ✓        |
| FFI bit-compatible               | native | ✗         | via `@layout(c)` |
| Auto-derived `==`, hash, string  | n/a    | identity  | ✓ structural |
| Sealed (closed set of variants)  | ✓      | ✓         | ✓        |

Jux enums dominate every column. The same `enum` keyword does it all — pick the pattern that fits your case, the compiler picks the representation that fits the pattern.

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
    public A first  { get; init; }
    public B second { get; init; }

    public Pair(A first, B second) {
        this.first = first;
        this.second = second;
    }
}

// Structural constraints with `where`
public <T> T max(T a, T b) where T has operator<=>(T) -> int {
    return a <=> b > 0 ? a : b;
}

// Multiple constraints
public <T> void sortAndSave(List<T> items)
    where T has operator<=>(T) -> int,
          T has serialize() -> bytes[] {
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

// Async lambda
var loader = async (int id) -> {
    return await fetchUser(id);
};

// Function types
public (int, int) -> int makeAdder(int base) {
    return (x, y) -> base + x + y;
}

// Async function type
public () async -> String taskFactory();

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

    // Elvis / null-coalescing for default value. `?:` and `??` are
    // aliases — same operator, same AST. Pick whichever reads
    // better at the call site.
    var displayName = findName(42) ?: "unknown";
    var altName     = findName(42) ?? "unknown";

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

Type tests with the `=>` operator (read "is an instance of") bind the narrowed type to a name when present, and the compiler tracks the narrowed type within the resulting scope:

```java
public void process(Animal a) {
    if (a => Dog d) {
        d.bark();                    // d is Dog here, no cast needed
        d.learn("sit");
    } else if (a => Cat c) {
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

This eliminates the typical Java sequence of `if (x instanceof Foo) { Foo f = (Foo) x; ... }` — Jux replaces it with the `=>` test which combines the test, cast, and binding in one step (`if (x => Foo f) { ... }`) and the result is statically type-checked.

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
public interface Container<T> {
    T at(int index);

    static <T> Container<T> singleton(T item) {
        return new SingletonContainer<>(item);
    }
}
```

**Static vs free functions.** Both exist in Jux. Use a free function for general utilities (`sqrt`, `parseInt`, `print`); use a static method when the function is conceptually associated with a specific type (factories, type-related utilities, methods that need access to private fields of the type).

### 7.14. Operator Overloading

Jux supports C++-style operator overloading: a user-defined type may declare its own meaning for arithmetic, comparison, indexing, and a fixed set of other operators. The syntax uses the `operator` keyword in the same shape as C++:

```java
public class Vec3 {
    public double x;
    public double y;
    public double z;

    public Vec3(double x, double y, double z) {
        this.x = x; this.y = y; this.z = z;
    }

    // Member operator: this + other
    public Vec3 operator+ (Vec3 other) {
        return new Vec3(x + other.x, y + other.y, z + other.z);
    }

    // Member operator: this == other
    public bool operator== (Vec3 other) {
        return x == other.x && y == other.y && z == other.z;
    }

    // Unary minus: -this
    public Vec3 operator- () {
        return new Vec3(-x, -y, -z);
    }
}
```

Operators may also be declared as free functions, which is preferred when the left-hand operand is a primitive or when the operator should be commutative across two distinct types:

```java
// Free-function form: scalar * vector
public Vec3 operator* (double scalar, Vec3 v) {
    return new Vec3(v.x * scalar, v.y * scalar, v.z * scalar);
}

// Free-function form: vector * scalar
public Vec3 operator* (Vec3 v, double scalar) {
    return scalar * v;
}
```

The compiler resolves operator calls by looking first at the left operand's member operators, then at free-function operators in scope. Ambiguity is a compile error; explicit qualification resolves it.

#### 7.14.1. Overloadable Operators

| Operator | Form | Method shape |
|----------|------|--------------|
| `+`, `-`, `*`, `/`, `%` | binary | `T operator+(U other)` |
| `+`, `-` | unary | `T operator+()`, `T operator-()` |
| `==`, `!=` | binary, returns `bool` | `bool operator==(U other)` |
| `<`, `<=`, `>`, `>=` | binary, returns `bool` | `bool operator<(U other)` |
| `<=>` | binary, returns `int` | `int operator<=>(U other)` — auto-derives `<`, `<=`, `>`, `>=` |
| `&`, `\|`, `^`, `~`, `<<`, `>>` | bitwise | `T operator&(U other)`, `T operator~()`, etc. |
| `[]` | indexed read | `T operator[](I index)` |
| `[]=` | indexed write | `void operator[]=(I index, T value)` |
| `()` | call | `R operator()(A... args)` — turns the type into a callable |
| `..`, `..=` | range | `Range<T> operator..(T end)`, `Range<T> operator..=(T end)` |
| `in` | containment | `bool operator in(T element)` — note: defined on the *container* |

**Symmetry rules.** Overloading `==` automatically defines `!=` as its negation; you cannot overload `!=` separately. Overloading `<=>` (the three-way comparison from C++20) auto-derives `<`, `<=`, `>`, `>=` from its sign. Overloading any individual `<`, `<=`, `>`, `>=` requires defining all four explicitly — the compiler does not derive partial sets.

**Compound assignment.** `a += b`, `a -= b`, `a *= b`, etc. desugar automatically to `a = a + b`, `a = a - b`, etc., using the corresponding binary operator. Compound assignment cannot be overloaded separately. This eliminates the C++ trap where `a += b` and `a = a + b` can have observably different behavior.

#### 7.14.2. What Cannot Be Overloaded

The following C++-overloadable operators are **not** overloadable in Jux. The reasons are listed; in each case the constraint protects readability or correctness.

| Operator | Why not |
|----------|---------|
| `&&`, `\|\|` | Would lose short-circuit evaluation — `a && b` would always evaluate `b`. C++ allows this and it is a known footgun. |
| `?:` (ternary `c ? a : b`) | Bound to nullable/result semantics; overriding would break null-handling guarantees. |
| `?:` / `??` (Elvis `a ?: b` / `a ?? b`) | Same — bound to nullable semantics. The two spellings are interchangeable. |
| `,` | The comma operator overload was a C++ mistake. Sequencing is not a value-producing operation worth overloading. |
| `=` | Plain assignment. Jux's ownership and refcount semantics define what assignment means; user override would violate borrow-checker invariants. |
| `.`, `?.` | Member access. Overload would essentially be macro magic; defeats `goto-definition`. |
| `=>` | Type-test ("is an instance of"). Language-defined; bound to the runtime type system. |
| `->` | Not a value-level operator. It's syntax — lambda body separator, switch-case body, function-type arrow. There is nothing to overload. |
| `::` | Method reference (`User::greet`, `User::new`). Resolves names at compile time; doesn't compute on values. |
| `===`, `!==` | Always reference identity — bypasses any user `operator==` (per §7.14.3). |
| `?` (postfix) | Error propagation through `Result`/nullable. Language-defined control flow. |
| `!!` | Non-null assertion. Bound to nullable handling. |
| `as` | Cast operator — bound to the type-conversion table (§5.8). |
| `new`, `drop` | Constructor and destructor are declared by the type's primary `new` block and `drop` block; not separately overloadable. |
| `await`, `spawn`, `move`, `yield` | Async / move / generator semantics are language-defined. |

#### 7.14.3. Equality, Identity, and Hashing

Equality is the most common case and gets specific rules:

- `a == b` calls `a.operator==(b)` if defined; otherwise falls back to the auto-derived structural equality of records/structs/enums or to reference identity for classes that don't override `==`.
- `a === b` is **always** reference identity. It bypasses overriding entirely. For value types (struct, record, primitive) it is identical to `==`.
- Overriding `operator==` on a class requires also providing `operator hash` consistent with the equality definition. The compiler enforces this — defining `==` without `hash` is `E0931`.

There is **no** `Equatable`, `Hashable`, or `Cloneable` interface in the language. Capabilities are declared via `operator` overrides; nothing else is needed. See `JUX-OPERATORS-ADDENDUM.md` for the canonical design.

```java
public class User {
    private String name;
    private int age;

    public bool operator== (User other) {
        return name == other.name && age == other.age;
    }

    public int operator hash() {
        return name.operator hash() * 31 + age;
    }
}

var alice1 = new User("Alice", 30);
var alice2 = new User("Alice", 30);

print(alice1 == alice2);     // true  — structural equality
print(alice1 === alice2);    // false — different instances
```

Records, structs, and enums auto-derive `==` and `hash` from their fields and never need explicit overriding for equality. Operator overriding on `==` is for classes whose equality definition cannot be derived structurally (e.g., a class that ignores cache fields, normalizes Unicode, or compares by ID rather than by all fields).

#### 7.14.4. Comparison and `<=>`

For ordered types, prefer the three-way comparison operator `<=>`. It returns a negative `int` if `this < other`, zero if equal, and a positive `int` if `this > other`. From a single `<=>` definition the compiler derives `<`, `<=`, `>`, `>=`:

```java
public class Version {
    public int major;
    public int minor;
    public int patch;

    public int operator<=> (Version other) {
        if (major != other.major) return major - other.major;
        if (minor != other.minor) return minor - other.minor;
        return patch - other.patch;
    }
}

var a = new Version(1, 2, 3);
var b = new Version(1, 3, 0);
print(a < b);        // true   — derived from <=>
print(a <= b);       // true
print(a > b);        // false
```

A type that defines `<=>` should not also define `<`, `<=`, `>`, `>=` separately — it is a compile error to mix the two styles. A type that defines `<` etc. separately must define all four; partial overloading is rejected.

#### 7.14.5. Operators and the Borrow Checker

Overloaded operators are inferred for mutation just like ordinary methods (§6.3). An operator that mutates `this` requires exclusive access to the receiver at every call site. For binary operators that take `other` by reference, the caller's borrow on `other` is shared.

```java
public class Counter {
    private int count;

    // Mutating compound op — but compound assignment desugars, so this is rare.
    // Better: define `+` to return a new value, let `+=` desugar.
    public Counter operator+ (int delta) {
        return new Counter(count + delta);    // pure, returns new Counter
    }
}
```

The whole-object borrow rule (§6.9.1) applies: an operator method on a class instance borrows the entire receiver. This is the same as any other method.

#### 7.14.6. Operators on Generics

Operator overloads can be generic and can appear in interfaces:

```java
public interface Addable<T> {
    T operator+ (T other);
}

public class Money implements Addable<Money> {
    private long cents;
    private String currency;

    public Money operator+ (Money other) {
        if (currency != other.currency) throw new ArithmeticException("currency mismatch");
        return new Money(cents + other.cents, currency);
    }
}

public <T extends Addable<T>> T sum(List<T> items, T zero) {
    var total = zero;
    for (var item : items) total = total + item;     // uses operator+ via the bound
    return total;
}
```

This makes operators usable in generic algorithms without resorting to function arguments, the way `std::accumulate` does in C++ and `Iterator::sum` does in Rust.

#### 7.14.7. Worked Example — A Complex Number Type

```java
public record Complex(double re, double im) {

    public Complex operator+ (Complex other) {
        return new Complex(re + other.re, im + other.im);
    }

    public Complex operator- (Complex other) {
        return new Complex(re - other.re, im - other.im);
    }

    public Complex operator- () {
        return new Complex(-re, -im);
    }

    public Complex operator* (Complex other) {
        return new Complex(
            re * other.re - im * other.im,
            re * other.im + im * other.re
        );
    }

    public bool operator== (Complex other) {
        return re == other.re && im == other.im;
    }

    public double abs() {
        return sqrt(re * re + im * im);
    }
}

// Free functions for primitive-on-the-left cases:
public Complex operator+ (double a, Complex b) { return new Complex(a + b.re, b.im); }
public Complex operator* (double a, Complex b) { return new Complex(a * b.re, a * b.im); }

public void main() {
    var a = new Complex(1.0, 2.0);
    var b = new Complex(3.0, -1.0);
    var c = a + b * 2.0 - 1.0;          // calls operator*, operator-, operator+
    var d = -a;                          // calls unary operator-
    var e = (a == new Complex(1.0, 2.0));
    print(c);
    print(d);
    print(e);
}
```

Records auto-derive `==` and `hashCode` from fields, so `Complex.operator==` here is redundant — included for illustration. In practice, a record only needs operator overloads for arithmetic/comparison; equality and hashing come for free.

### 7.15. Top-Level Statements

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

### 7.16. Loops and Ranges

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

### 7.17. Procedural Programming Style

Jux supports straight procedural code as a first-class style — no class wrapping required. Per design goal #6 (V1 §1.1), procedural and OOP coexist freely; pick whichever fits each piece of code.

The procedural toolkit is:

- **Free functions** declared at module scope with `public R name(...)`.
- **`struct`** for plain data (V1 §5.2, §7.6) — stack-allocated, copied on assignment, public fields by default, zero overhead vs. C structs.
- **`record`** when you also want auto-derived equality, hashing, and immutability.
- **`enum`** with C-style or sum-type variants for state codes and tagged data (V1 §7.7).
- **Module-level constants** with `public const T NAME = ...;` (or `public final T NAME`, synonymous per §5.6).
- **Top-level statements** in the entry-point file (V1 §7.15).
- **Free-function operator overloads** for math/numeric types (V1 §7.14).

#### 7.17.1. Worked Example — Particle Physics

```java
package com.example.physics;

// Module-level constants
public const double G = 9.81;
public const double AIR_DENSITY = 1.225;

// Plain data — value types, no identity, no inheritance
public struct Particle {
    double x;
    double y;
    double vx;
    double vy;
    double mass;
}

public struct ForceField {
    double gx;
    double gy;
}

// Free functions — no class needed
public Particle step(Particle p, ForceField f, double dt) {
    return new Particle(
        p.x + p.vx * dt,
        p.y + p.vy * dt,
        p.vx + f.gx * dt,
        p.vy + f.gy * dt,
        p.mass
    );
}

public double kineticEnergy(Particle p) {
    return 0.5 * p.mass * (p.vx * p.vx + p.vy * p.vy);
}

public ForceField gravity() {
    return new ForceField(0.0, -G);
}

// Top-level statements (in main.jux only — see §7.15)
var p = new Particle(0.0, 100.0, 5.0, 0.0, 1.0);
var g = gravity();

for (var t : 0..100) {
    p = step(p, g, 0.01);
    print($"t=$t  x=${p.x}  y=${p.y}  KE=${kineticEnergy(p)}");
}
```

Zero classes, zero objects, zero `this`. This compiles to native code with the same performance as a hand-written C program — structs are stack-allocated, function calls are direct (no virtual dispatch), and the borrow checker keeps the whole thing memory-safe.

#### 7.17.2. When Procedural Beats OOP

- **Pure data transformations.** Image processing, DSP, math libraries, parsers, compilers. Functions transform data; objects don't help.
- **Numeric / scientific code.** Free-function operator overloads (`Vec3 operator*(double s, Vec3 v)`) make math read like math.
- **Embedded systems.** No allocation, no virtual dispatch, no refcount traffic — predictable behavior on tight memory.
- **Stateless utilities.** String formatting, hashing, encoding — zero state, no reason to wrap in a class.

#### 7.17.3. Mixing Styles

The same file can mix procedural and OOP freely:

```java
package com.example.app;

// Procedural: pure functions on plain data
public struct Color { double r; double g; double b; }
public Color blend(Color a, Color b, double t) {
    return new Color(
        a.r * (1.0 - t) + b.r * t,
        a.g * (1.0 - t) + b.g * t,
        a.b * (1.0 - t) + b.b * t
    );
}

// OOP: stateful object with identity
public class Renderer {
    private GpuContext ctx;
    private List<Surface> surfaces = new List<>();

    public Renderer(GpuContext ctx) { this.ctx = ctx; }

    public void drawColored(Surface s, Color c) {
        ctx.bind(s);
        ctx.setFillColor(c);          // c is plain data, passed by value
        ctx.fill();
    }
}
```

Pick the style that fits. Free functions when you have a stateless transformation; classes when you have identity and state. Both compile to the same machine code class — no overhead from choosing one over the other.

#### 7.17.4. Comparison with C

| Capability             | C                    | Jux procedural             |
|------------------------|----------------------|----------------------------|
| Plain structs          | ✓                    | ✓ (`struct`)               |
| Free functions         | ✓                    | ✓                          |
| Module-level constants | ✓ (`#define`, `const`)| ✓ (`const`/`final`)       |
| No GC                  | ✓                    | ✓                          |
| Pointer access         | ✓ (everywhere)       | ✓ (in `unsafe { }`)        |
| Memory safety          | ✗                    | ✓ (borrow checker)          |
| Generics               | ✗ (or macros)        | ✓ (monomorphized)          |
| Sum types              | ✗ (or unions+tag)    | ✓ (`enum` with payloads)   |
| Build system           | manual / make / CMake| `jux.toml` (Cargo-shaped)   |
| Cross-compilation      | manual               | `juxc build --target ...`   |
| Reading legacy C code  | native               | via `unsafe @extern`        |

You get C's procedural ergonomics with modern tooling, safety, and the ADTs/generics C never had. For 90% of low-level code, this is the right style.

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
        RawHandle* h = null;
        var rc = sqlite3_open(path.toCString(), out h);
        if (rc != 0) {
            throw new DbError("Failed to open: " + path);
        }
        this.handle = h;
    }

    public void execute(String sql) throws DbError {
        CString err = null;
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
    public void db_close(Database* db) { /* refcount drop, destructor runs */ }
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
| `std.async`          | Async runtime, `spawn`, `Task<T>`, channels   |
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

### 9.4. Core Interfaces (Operator-First Design)

`core` contains exactly **one** foundational nominal interface (`Iterable<T>`) plus inferred markers (`Sendable`, `Shareable`). Every other capability is expressed by **operator overrides** declared with the `operator` keyword. There is no `Equatable`, `Hashable`, `Comparable`, `Cloneable`, `Displayable`, or `Sized` interface — those are removed by `JUX-OPERATORS-ADDENDUM.md`.

```java
package core.iter;

public interface Iterator<T> {
    T? next();                                        // returns null at end
}

public interface Iterable<T> {
    Iterator<T> iterator();
    // Default-method combinators: map, filter, reduce, take, skip, zip, chain, ...
}

package core.markers;

public interface Sendable {}                           // inferred marker
public interface Shareable {}                          // inferred marker
```

**`Iterable<T>` powers `for (var x : coll)`.** Any type implementing `Iterable<T>` works in for-each loops. All standard collections, ranges, and arrays implement it. This is the only foundational interface a user type ever needs to `implements`.

**Equality, ordering, hashing, formatting** are operator overrides:

- `operator==(Other)` returning `bool` — equality (auto-derived for records/structs/enums; identity-default for classes).
- `operator<=>(Other)` returning `int` — ordering. Auto-derives `<`, `<=`, `>`, `>=` from sign.
- `operator hash()` returning `int` — hashing for `Map`/`Set` keys.
- `operator string()` returning `String` — used by `$"..."` interpolation and `print(x)`.

Records, structs, and enums auto-derive all four from fields. Classes default to identity (`==` is `===`, `hash` is identity, `string` is type-and-address). To opt a class into structural behavior, write the operators you want — the compiler enforces the `operator==` + `operator hash` pairing.

**Cloning** is **not** a language-level concept. Value types copy implicitly on assignment; classes that want a copy method give it whatever name fits (`snapshot()`, `forked()`, `copy()`).

**`Sendable`/`Shareable`** are inferred by the compiler from a type's fields. A type is `Sendable` if all its fields are `Sendable`; same for `Shareable`. They cannot be implemented manually — they are facts about the type.

See `JUX-OPERATORS-ADDENDUM.md` for the full operator-first design.

---

## 10. Concurrency

Jux's concurrency model has three layers, each opt-in:

1. **Async / await** (§10.1) — single-threaded cooperative scheduling within the event loop. The default for I/O-bound work.
2. **Workers** (§10.2) — preemptive multithreading for CPU-bound or parallel work. Explicit, with compiler-checked transferability.
3. **Synchronization primitives** (§10.3) — channels, mutexes, atomics. Available in both async and sync forms.

The design is Kotlin-shaped at the surface (async functions return `T`, not `Future<T>`) with TypeScript-shaped ergonomics (familiar `async`/`await` keywords) and Rust-shaped soundness (transferability checked at compile time, hidden behind friendly diagnostics).

### 10.1. Async and Await

#### 10.1.1. Declaring Async Functions

A function declared `async` may suspend execution at `await` points. **Its return type is the value it yields, not a wrapper.** No `Future`, no `Task`, no `Promise` appears in the signature.

```java
public async String fetchUser(int id) {
    var response = await http.get($"/users/$id");
    return await response.text();
}

public async List<Post> getUserPosts(int id) {
    return await db.query("posts where user = ?", id);
}

public async UserPage loadPage(int id) {
    var user  = await fetchUser(id);
    var posts = await getUserPosts(id);
    return new UserPage(user, posts);
}
```

The `async` modifier appears before the return type. It declares a property of the function — that it may suspend — without changing what the function produces.

`async` is permitted on:

- Free functions
- Class methods (including overrides)
- Interface methods (including default methods)
- Lambdas (`async (x) -> { ... }`)

Constructors are not allowed to be async. Use a static async factory method instead:

```java
public class Database {
    private Connection conn;

    private Database(Connection conn) {
        this.conn = conn;
    }

    public static async Database connect(String url) {
        var conn = await Connection.open(url);
        return new Database(conn);
    }
}
```

#### 10.1.2. Awaiting

`await` suspends the enclosing async function until a computation completes. It applies to two kinds of expressions:

- **A call to an `async` function.** `await fetchUser(1)` runs the function until it returns; the result is the function's declared `T`.
- **A `Task<T>` value.** `await task` waits for an explicitly-spawned task (§10.1.3) and yields its `T`.

The two cases unify in the type system: `await` accepts anything that "produces a `T` after possibly suspending."

```java
public async String example() {
    var direct = await fetchUser(1);                  // direct call to async function
    var task   = spawn(() -> fetchUser(2));           // Task<String>
    var fromTask = await task;                        // unwrap the task
    return direct + ", " + fromTask;
}
```

`await` is permitted only:

- Inside an `async` function or method
- Inside an async lambda
- At the top level of `main` (if `main` is declared `async`)
- At module initialization in modules marked `@async-init`

**Calling an async function without `await` from inside an async context is a compile error.** This is the central improvement over JS/TS:

```java
public async void example() {
    fetchUser(1);                         // ERROR: must await
}
```

```
Error: async function `fetchUser` called without `await`
  --> example.jux:14:5
   |
14 |     fetchUser(1);
   |     ^^^^^^^^^^^^ this is an async call but its result is being discarded
   |
Hint: either await the result:
   var user = await fetchUser(1);
or run it concurrently:
   var task = spawn(() -> fetchUser(1));
```

There is no silently-dropped Promise.

#### 10.1.3. Spawning Concurrent Work

To run async work alongside other work, use `spawn`. This is the only way to obtain a `Task<T>`:

```java
import std.async.{spawn, Task};

public async UserPage loadPageFast(int id) {
    var userTask  = spawn(() -> fetchUser(id));       // Task<String>
    var postsTask = spawn(() -> getUserPosts(id));    // Task<List<Post>>
    return new UserPage(
        await userTask,
        await postsTask
    );
}
```

`spawn(f)` schedules `f` on the event loop and returns a `Task<T>` immediately. The lambda passed to `spawn` may itself be async. Awaiting the returned task suspends the caller until the spawned work completes.

If a task is dropped without being awaited, it continues to run to completion. Errors in unawaited tasks are reported via the runtime's unhandled-rejection hook (§10.1.8) — the same mechanism Node.js uses for unhandled promise rejections.

#### 10.1.4. The Task Type

`Task<T>` is a refcounted handle to a running computation, defined in `std.async`:

```java
public class Task<T> {
    public T blockingGet() throws ExecutionException;   // available only in jux-full sync contexts

    public void cancel();
    public bool isCancelled();
    public bool isResolved();
}
```

The `await` keyword (§10.1.2) is the canonical way to consume a `Task<T>`. Combinators (`map`, `flatMap`, etc.) are deliberately omitted — with first-class `await`, `var x = await task; transform(x)` is the idiomatic shape.

Static constructors and helpers:

```java
public static <T> Task<T>          Task.completed(T value);
public static <T> Task<T>          Task.failed(Exception error);
public static    Task<void>        Task.delay(Duration d);

public static <T> Task<List<T>>                          Task.all(List<Task<T>> tasks);
public static <T> Task<T>                                Task.race(List<Task<T>> tasks);
public static <T> Task<T>                                Task.any(List<Task<T>> tasks);
public static <T> Task<List<Result<T, Exception>>>       Task.allSettled(List<Task<T>> tasks);
```

`Task.all(tasks)` resolves when every task resolves; rejects on the first failure. `Task.race` resolves with the first to settle. `Task.any` resolves with the first success and rejects only if all fail. Same semantics as the matching `Promise.*` calls in JavaScript.

A common pattern — fan out, await all — gets a built-in shorthand:

```java
import std.async.parallel;

public async List<UserPage> loadAll(List<int> ids) {
    return await parallel(ids, id -> loadPage(id));
}
```

`parallel(items, f)` is equivalent to `Task.all(items.map(it -> spawn(() -> f(it))))` and handles the common case of "do this thing for every item in parallel."

#### 10.1.5. Async Methods on Classes and Interfaces

Async methods are ordinary methods that may suspend. They participate in dispatch, override, and inheritance like any other method:

```java
public interface DataSource {
    async List<Record> fetch(Query q);
}

public final class HttpDataSource implements DataSource {
    private HttpClient client;

    @Override
    public async List<Record> fetch(Query q) {
        var response = await client.get(q.toUrl());
        return parseRecords(await response.text());
    }
}
```

There is no "async fn in traits is unstable" story. Async interface methods, default methods, and overrides all work the same way as their sync counterparts:

```java
public interface Cache {
    async String? get(String key);
    async void put(String key, String value);

    default async String getOrCompute(String key, () async -> String computer) {
        var existing = await get(key);
        if (existing != null) return existing;
        var computed = await computer();
        await put(key, computed);
        return computed;
    }
}
```

The `() async -> String` lambda type denotes "a lambda that may suspend and produces a `String`."

#### 10.1.6. The Borrow Rule Across `await`

> **No exclusive (mutating) borrow on observable state may be active across an `await` point.**

Shared borrows are fine. Class references are fine (refcounting keeps them alive). Owned values are fine. Freshly-acquired guards from `AsyncMutex<T>` (§10.3) are fine — they are exclusive but unobserved by other code. The only thing forbidden is being mid-mutation on a value other code can also observe across a suspension.

This rule exists because at an `await`, control returns to the event loop and other code may run, including code that observes the same object. Allowing exclusive borrows across awaits would let two suspended functions interleave their mutations on the same object — the data-race-via-cooperative-scheduling failure that Node.js codebases hit informally and that JS programmers learn to avoid by convention. Jux makes the convention a compile-time check.

```java
public async void good(User u) {
    var name = u.name;                    // shared access
    await sleep(milliseconds(100));
    print(name);                          // OK
}

public async void bad(User u) {
    var nameField = u.mutableName();      // exclusive borrow on u (whole-object, §6.9.1)
    await sleep(milliseconds(100));       // ERROR: exclusive borrow held across await
    nameField.set("new");
}
```

Diagnostic:

```
Error: cannot await while holding an exclusive borrow on `u`
  --> handler.jux:42:5
   |
40 |     var nameField = u.mutableName();
   |                     - borrows `u` exclusively here
41 |     await sleep(milliseconds(100));
   |     ^^^^^ cannot suspend while `u` is mutably borrowed
   |
Hint: end the borrow before the await:
   var newName = "new";
   await sleep(milliseconds(100));
   u.mutableName().set(newName);
```

For struct values (which support field-level borrows per §6.9.1), the same rule applies field by field. Most async code never encounters this rule because most functions don't hold exclusive borrows across statement boundaries to begin with.

#### 10.1.7. Top-Level Await

The entry point may be declared `async`:

```java
public async void main() {
    var config = await loadConfig();
    await runServer(config);
}
```

The runtime starts the event loop, runs `main`, and exits when `main` returns (and the loop drains). No explicit "start the runtime" call is needed.

Module initializers may use `await` at top level when the module is marked `@async-init`:

```java
@async-init
package com.example.app;

import std.io.readFile;

public const String CONFIG = await readFile("/etc/config.json");
```

Modules with async initializers load asynchronously; their dependents wait for them implicitly. This matches ECMAScript modules with top-level `await`.

#### 10.1.8. Errors and Unhandled Rejections

A `throw` inside an async function causes the awaiter to receive the exception. `try`/`catch` works seamlessly across `await`:

```java
public async int example() throws ParseError {
    try {
        var s = await readFile("data.txt");
        return parse(s);
    } catch (IOException e) {
        throw new ParseError("read failed: " + e.message);
    }
}
```

A spawned task that fails with no awaiter triggers the runtime's unhandled-rejection hook. The default hook logs the error and aborts the process; embedders can override it via `Runtime.onUnhandledRejection`.

Because direct async calls require `await` (§10.1.2), unhandled rejections can only come from spawned tasks. This is a much smaller surface area than JavaScript, where any call that returns a Promise can be silently dropped.

#### 10.1.9. Cancellation

Calling `task.cancel()` requests cancellation. At the next `await` inside the task, a `CancellationException` is thrown:

```java
public async String longOperation() {
    for (var i = 0; i < 1000; i++) {
        await sleep(milliseconds(10));    // checks for cancellation
        // ... work ...
    }
    return "done";
}

public async void driver() {
    var task = spawn(() -> longOperation());
    await sleep(milliseconds(50));
    task.cancel();
    try {
        await task;
    } catch (CancellationException e) {
        print("cancelled");
    }
}
```

Cancellation is **cooperative**: a function that never awaits cannot be cancelled. The runtime guarantees no other delivery mechanism — no thread interrupts, no exception injection at arbitrary points. This is essential to keep §10.1.6's exclusivity rule sound.

Timeouts are built on cancellation:

```java
import std.async.{withTimeout, TimeoutException};

public async String fetchWithTimeout(String url) {
    try {
        return await withTimeout(seconds(5), async () -> await http.get(url));
    } catch (TimeoutException e) {
        return "fallback";
    }
}
```

`withTimeout(d, f)` runs the async closure `f` and returns its result if it completes within `d`, or cancels it and throws `TimeoutException` otherwise.

#### 10.1.10. Calling Async from Sync Code

Calling an async function from a sync function is a compile error in async contexts (§10.1.2). From a fully-sync context (no `async` modifier on the enclosing function), you must explicitly bridge:

```java
public void legacyMain() {                // not async
    var task = spawn(() -> loadPage(42));
    var page = task.blockingGet();        // blocks the calling thread
    print(page.user);
}
```

`blockingGet()` runs the event loop until the task completes and returns the result (or throws). In `jux-full` it spins up the runtime if not already running. In `jux-embedded` and `jux-core` it is unavailable — those profiles require an async `main`.

#### 10.1.11. Profiles

| Profile | Async runtime | Notes |
|---|---|---|
| `jux-full` | Full event loop, work-stealing executor with worker threads | Multithreading via `Worker.spawn` (§10.2); main is single-threaded |
| `jux-embedded` | Single-threaded executor, no workers | Suitable for ESP32 / Cortex-M with FreeRTOS; ~4 KB executor |
| `jux-core` | Async unavailable | Use `Result<T, E>` and explicit state machines; see §16.7 |

In `jux-core`, declaring an `async` function is a compile error pointing to §16.7.

When `throws E` appears on an `async` function in a profile that disables exceptions, the compiler lowers the function to return `Task<Result<T, E>>` and rewrites `?` and `try`/`catch` accordingly. Source code is portable across profiles; the lowering is invisible to users.

### 10.2. Workers (Multithreading)

Single-threaded async covers the bulk of server and UI workloads. CPU-bound work and parallelism use **workers** — explicitly opted into:

```java
import std.async.Worker;

public async int parallelSum(int[] data) {
    var mid = data.length / 2;
    var left  = Worker.spawn(() -> sumRange(data, 0, mid));
    var right = Worker.spawn(() -> sumRange(data, mid, data.length));
    return await left + await right;
}
```

`Worker.spawn(f)` runs `f` on a thread from the worker pool and returns a `Task<T>`. Values captured by `f` must be **transferable** — a property the compiler verifies without exposing `Send` or `Sync` as user-facing terms.

Transferable types are:

- All primitive types and tuples of transferable types
- `struct` and `record` types whose fields are all transferable
- `class` types whose refcount can be made atomic; the compiler synthesizes atomic refcount operations only for class types that actually cross a worker boundary
- `String`, `List<T>`, `Map<K, V>` (their internal sharing is thread-safe by construction)

When the compiler rejects a capture, the diagnostic names the offending value and points to the alternative (e.g., "wrap in `AtomicShared<T>`" or "send by value"). The terms `Send` and `Sync` never appear in error messages or in the user-visible type system.

```
Error: cannot capture `db` across a worker boundary
  --> parallel.jux:14:5
   |
14 |     var task = Worker.spawn(() -> db.query("..."));
   |                                   ^^ value of type `Database` holds a non-thread-safe handle
   |
Hint: Database is single-threaded by design. Either:
  - move the database operation onto the main thread and send results, or
  - use `ConcurrentDatabase`, which is thread-safe.
```

### 10.3. Channels and Synchronization

#### 10.3.1. Async Channels

For inter-task communication:

```java
import std.async.Channel;

public async void producerConsumer() {
    var ch = new Channel<int>(capacity: 16);

    spawn(async () -> {
        for (var i = 0; i < 100; i++) {
            await ch.send(i);
        }
        ch.close();
    });

    while (true) {
        var item = await ch.receive();
        if (item == null) break;          // closed
        print(item);
    }
}
```

`Channel<T>` is bounded and async; `send` suspends when full, `receive` suspends when empty. Closing causes pending receives to resolve to `null`. This mirrors Go channels and Kotlin's `Channel<T>`.

#### 10.3.2. Async Mutex

For mutual exclusion across awaits, `AsyncMutex<T>`:

```java
import std.async.AsyncMutex;

public class Counter {
    private AsyncMutex<int> count = new AsyncMutex<>(0);

    public async int increment() {
        var guard = await count.lock();   // suspends until acquired
        guard.value = guard.value + 1;
        return guard.value;
    }                                      // guard drops here, releasing the lock
}
```

The guard is the only handle that grants access to the protected value. Holding a guard across an await is permitted (this is the entire point of `AsyncMutex` over a regular mutex). The borrow rule from §10.1.6 is satisfied because the guard mediates *exclusive access*: while held, no other code can observe the protected value, so the "observable state" condition of the rule does not apply.

#### 10.3.3. Synchronous Synchronization

For non-async multithreaded code (worker pools, FFI callbacks), `std.concurrent` provides synchronous primitives:

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

The sync `Mutex<T>` API forces lock acquisition before access — there is no way to access the protected value without going through the mutex. This eliminates "forgot to lock" bugs at compile time.

`AtomicInt`, `AtomicLong`, `AtomicRef<T>` provide lock-free primitives for simple cases.

### 10.4. Worked Example — Concurrent HTTP Aggregator

```java
import std.async.{spawn, parallel, withTimeout};
import std.http.HttpClient;
import std.time.seconds;

public record UserSummary(int id, String name, int postCount) {}

public class UserService {
    private HttpClient http;

    public UserService(HttpClient http) {
        this.http = http;
    }

    public async UserSummary fetchSummary(int userId) {
        // Fan out two requests in parallel
        var userTask  = spawn(() -> http.getJson($"/users/$userId"));
        var postsTask = spawn(() -> http.getJson($"/users/$userId/posts"));

        // Bound the whole thing to 5 seconds
        var user  = await withTimeout(seconds(5), async () -> await userTask);
        var posts = await withTimeout(seconds(5), async () -> await postsTask);

        return new UserSummary(
            id: userId,
            name: user.getString("name"),
            postCount: posts.getList().size()
        );
    }

    public async List<UserSummary> fetchAll(List<int> userIds) {
        return await parallel(userIds, id -> fetchSummary(id));
    }
}

public async void main() {
    var service = new UserService(new HttpClient());
    var summaries = await service.fetchAll([1, 2, 3, 4, 5]);
    for (var s : summaries) {
        print($"${s.name}: ${s.postCount} posts");
    }
}
```

No `Future<T>` in any signature. No `Send` bounds. No pinning. No runtime to choose. No `'static`. The shape reads like Kotlin coroutines or modern Swift concurrency, with Java syntax.

### 10.5. Comparison with Other Languages

| Concept | JavaScript / TS | Kotlin coroutines | Jux | Rust |
|---------|-----------------|-------------------|-----|------|
| Async signature | `async function f(): Promise<T>` | `suspend fun f(): T` | `async T f()` | `async fn f() -> T` |
| What `f()` returns to caller | `Promise<T>` | `T` (after suspension) | `T` (after suspension) | `impl Future<Output=T>` |
| Awaiting | `await x` | `f()` (implicit at call site) | `await f()` | `f().await` |
| Concurrent handle | `Promise<T>` (every call) | `Deferred<T>` (only via `async {}`) | `Task<T>` (only via `spawn`) | future you `tokio::spawn` |
| Forgotten `await` | silent bug | impossible (no Promise to forget) | compile error | clippy lint |
| Runtime | event loop (built in) | dispatcher (built in) | event loop (built in) | choose tokio / async-std |
| Threading default | single + Workers | single + Dispatchers | single + Workers | full multithreading |
| `Send` / `Sync` | n/a | n/a (mostly hidden) | hidden ("transferable") | user-facing |
| Pinning | none | none | none | `Pin<&mut Self>` |
| Cancellation | `AbortController` | structured (built in) | `Task.cancel()` | drop the future |
| Top-level await | yes | n/a | yes | no |
| Borrow across await | n/a | n/a | shared OK, exclusive forbidden (§10.1.6) | enforced via Send + lifetimes |

### 10.6. Deferred from this Specification

- **Async iterators / streams** (`for await (var x of stream)`). Underlying type: `Stream<T>` with `async T? next()`.
- **Async generators.** Same.
- **Cooperative fairness guarantees.** The runtime currently makes none beyond "no task starves indefinitely." Workloads that need stronger guarantees should yield explicitly with `Task.yield()`.
- **Structured concurrency primitives** (scoped tasks, supervision trees). The cancellation primitives in §10.1.9 are the foundation.

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

The full `jux.toml` schema is unspecified in this document and is one of the v0.1 blocking gaps (§19).

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

// Async tests are ordinary @Test functions declared async
@Test
public async void fetchesAsynchronously() {
    var result = await api.getUser(1);
    assertEqual("Alice", result.name);
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
- Borrow inference (initial heuristic version, including the §6.9 inheritance rules)
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
- Async / await (Kotlin-shaped, §10)
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

The following are unresolved and require further design work. (Items resolved by addenda are removed; see §19 for the active gap list.)

1. **Const evaluation.** How much of Jux is evaluable at compile time? Rust's `const fn` model is expressive but complex. Java has no real answer. Likely middle ground: simple `const` expressions and pure functions only.

2. **Reflection and runtime type information.** Java's reflection is powerful but costly. Rust has none. Jux likely needs *some* (for serialization, frameworks) but not full Java-level reflection. Where to draw the line?

3. **Inline assembly.** Probably not in v1.0 by default, but the design should not preclude it for systems work. (See §16.8 for the tentative form.)

4. **Effects beyond exceptions.** Rust has nothing here; algebraic effects are research-grade. Jux likely stays with exceptions but should not architect them out.

5. **Macros.** Compile-time code generation is powerful but complex. Likely deferred indefinitely; user-defined macros are the slipperiest slope in language design.

6. **Specialization within generics.** Whether `List<int>` can have a different (faster) implementation than `List<T>` in general. Rust has experimental specialization; full design is unsolved.

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

    if (animal => Dog d) {
        d.learn("sit");
        d.learn("roll over");
    }
}

print("Zoo has " + zoo.size() + " animals");
```

This program exercises: top-level statements, sealed inheritance hierarchies (with the new exact mutation analysis from §6.9.7), interfaces with default methods, abstract classes, record-style constructors with default arguments, polymorphism through a `List<Animal>`, type-test pattern matching with `=>`, and the borrow checker quietly enforcing safety throughout.

---

## 16. Embedded and Bare-Metal Targets

Jux is designed to scale from cloud servers to 32KB microcontrollers. The same syntax, the same borrow checker, and the same FFI model apply on every target. What changes between targets is which features are available, controlled by the build profile (§2.4).

### 16.1. The Three Profiles

| Profile | Heap | Refcount | Exceptions | Threads | Async | Typical target |
|---|---|---|---|---|---|---|
| `jux-full` | Yes | Yes | Yes | Yes | Yes | Linux, macOS, Windows |
| `jux-embedded` | Optional | Optional | Optional | Optional | Single-threaded | ESP32, STM32, Pi Pico |
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
- No async machinery (`async`/`await` rejected at compile time)
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

The compiler can also lower `throws` to `Result` automatically when targeting profiles that disable exceptions, so the same source code can be portable between `jux-full` (where it uses real exceptions) and `jux-embedded` (where it lowers to `Result`). When this lowering is combined with `async`, the function returns `Task<Result<T, E>>` (§10.1.11).

### 16.8. Inline Assembly

For boot code, atomic primitives, or cycle-counting work where the toolchain offers no alternative:

```java
public static int readSP() {
    int sp;
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

This compiles to a self-contained `.elf` file you flash to the board. No runtime, no allocator, no exception machinery, no async state machines. Just direct hardware access with the borrow checker and type system still active. Final binary: a few KB.

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
- **Whole-object borrow.** The rule that a borrow of any field of a class instance is treated as a borrow of the entire instance (§6.9.1). Distinguishes class semantics from struct semantics.
- **Mutation union.** The set of "may mutate" outcomes computed across all reachable overrides of a virtual method. Determines whether a call site requires shared or exclusive access (§6.9.3).
- **Transferable type.** A type whose values can be safely captured into a worker closure. The user-facing equivalent of Rust's `Send`. Inferred by the compiler (§10.2).
- **Cooperative cancellation.** A cancellation model in which a task can only be cancelled at points where it explicitly yields (i.e., at `await`). Contrasts with preemptive cancellation, which Jux does not provide (§10.1.9).

---

## 18. References and Inspiration

- **Java** — Syntax, OOP model, package system, exceptions, generics syntax, sealed types
- **Kotlin** — Nullable types, smart casting, named arguments, primary constructors, top-level functions, `internal` visibility, coroutines (async-returns-T model)
- **C#** — Asymmetric property visibility, structs vs classes, top-level statements, records
- **Rust** — Ownership and borrowing, monomorphization, traits, sealed-enum-style sum types, RAII via Drop, FFI model, Cargo, `no_std` profile model, `?` operator
- **Swift** — Reference counting + weak references, value-vs-reference type distinction, native compilation strategy, Embedded Swift profile, Law of Exclusivity (basis for §6.9 whole-object borrow)
- **TypeScript** — Async/await syntax shape, top-level await
- **JavaScript** — Promise.all/race/any/allSettled semantics, event-loop model
- **C++** — Templates (as a cautionary example), header-based interop pattern, `freestanding` mode
- **C** — Universal ABI, vendor SDK conventions, linker-section model for dead code elimination
- **Hylo (Val)** — Mutable value semantics without lifetime annotations
- **Carbon** — Familiar-syntax, new-semantics positioning
- **Move** — Linear types in a Java-shaped language
- **Go** — Channel model (mirrored in §10.3.1)

---

## 19. Gaps and Open Questions

This section catalogues what is *not yet specified* in the v1 dossier — the work remaining before a v0.1 implementation is feasible. Items are grouped by category and prioritized.

### 19.1. Internal Inconsistencies to Resolve

These are bugs or contradictions inside the existing spec text. They should be addressed before any other gap, because they undermine the stated rules.

**Open-class cross-module breakage policy (§7.4.1).** §7.4.1 documents that introducing a new override in a downstream module can flip the inferred mutability of a base method, breaking previously-compiling code in third modules. The diagnostic is shown at the override site, but the breakage is real and at-a-distance. Two ways forward:

- **Restrict `open` to module-internal.** Cross-module extension requires sealing or interface implementation. This is what Kotlin effectively enforces and eliminates the whole-program-analysis cost of §6.9.3 outright.
- **Accept the breakage.** Document it loudly and rely on the diagnostic.

The current spec is silent on which way to go. Decision needed.

**Single-ownership classes + virtual dispatch in `jux-core` (§6.9.8).** Move semantics on classes with virtual dispatch is a known sharp-edge zone (slicing, vtable identity through moves). The spec currently allows the combination but doesn't show a worked example. Either provide one or restrict virtual dispatch to sealed hierarchies under `jux-core`.

**Mutable static field thread safety (§7.13) in single-threaded profiles.** §7.13 mandates `AtomicInt`/`Mutex<T>` for mutable statics. In `jux-core` (no threads), this is unnecessary overhead. Either relax the rule per profile or accept the overhead with a justification.

### 19.2. Standard Library Foundations (Blocking)

These pieces are referenced throughout the spec but never defined. Code in §15, §10.4, §16 cannot compile until they exist.

**Foundational interfaces.** *Resolved by `JUX-OPERATORS-ADDENDUM.md`.* The interfaces `Equatable`, `Hashable`, `Comparable`, `Cloneable`, `Displayable`, `Sized` **do not exist**. Equality, ordering, hashing, and formatting are operator overrides (`operator==`, `operator<=>`, `operator hash`, `operator string`). Records/structs/enums auto-derive these. `Iterator<T>` and `Iterable<T>` are the only nominal foundational interfaces; `Sendable`/`Shareable` are inferred markers. See §9.4 for the updated text.

**Exception hierarchy.** Every `throws` clause references undefined types. Needed: `Exception` base class, standard subtypes (`RuntimeException`, `IOException`, `IllegalArgumentException`, `IllegalStateException`, `NullPointerException`, `IndexOutOfBoundsException`, `ArithmeticException`, `CancellationException`, `TimeoutException`, `ExecutionException`), stack-trace policy per profile, the lowering rules to `Result<T, E>`.

**Collections.** `List<T>`, `Map<K, V>`, `Set<T>`, `Deque<T>`, `Queue<T>`, `RingBuffer<T, N>` are used throughout. Need full public APIs, iterator integration, mutability story (single mutable type with `.toImmutable()` is the recommendation), and Phase 1 implementation strategy (likely thin wrappers over Rust counterparts).

**Strings, I/O, time.** `std.string` (operations beyond `toString`, `StringBuilder`, `Regex`, `%`-format strings), `std.io` (`File`, `Path`, streams, stdin/out/err), `std.time` (`Instant`, `Duration` constructors used in §10, `Clock`, calendar types).

**Async streams.** §10.6 explicitly defers this. The async examples don't have a streaming primitive yet, which means line-by-line file reading, server-sent events, and chunked HTTP responses have no documented shape. Need: `Stream<T>` interface, `for await (var x : stream)` syntax, combinators (`mapAsync`, `filterAsync`, `take`).

### 19.3. Async Specification Loose Ends

**`AsyncMutex` borrow-rule carve-out (§10.1.6 / §10.3.2).** The text now reads consistently — guards mediate exclusive access to *otherwise unobservable* state, so they don't violate the rule. This needs a formal restatement of the rule alongside the lowering description, not just an English paragraph.

**Atomic refcount strategy (§10.2).** "The compiler synthesizes atomic refcount operations only for class types that actually cross a worker boundary" — does this mean two compiled versions per class, or analysis-driven atomic-when-needed via flow types? The choice has compile-time and binary-size implications. Spec must commit.

**Async × throws × Result lowering interaction (§10.1.11, §16.7).** A single sentence covers this now. A real spec needs to show, for each profile, the exact return-type lowering for `public async T fn() throws E`, where `?` is permitted, and how `try`/`catch` lowers across `await` in each form.

**`spawn` keyword vs std function (§3.2).** *Resolved by `JUX-MISSING-DEFS-ADDENDUM.md` §M.12.1.* `spawn` is a library function in `std.async`, not a keyword. Removed from the reserved-keyword list.

**Async on `jux-embedded` code-size impact (§10.1.11).** Async on small MCUs is expensive — every async function compiles to a state machine. Worth a paragraph about the size cost and a recommendation to prefer sealed hierarchies for async dispatch on embedded.

### 19.4. Type-System Polish (Important for v0.1)

**Const evaluation.** §14.1 says "limited const evaluation only" without specifying limits. Needed for `RingBuffer<T, N + 1>`-style types and embedded constants. Recommended bounds: arithmetic on integer literals, `if`/`match`, const function calls, bounded recursion. No heap, no I/O.

**Reflection.** §14.2 open. Recommendation: compile-time only via `@Reflectable` opt-in, paired with a `Type<T>` API. Pairs with derive-style annotations (§19.5).

**Nested classes.** Not covered in §7. Recommendation: support only `static` nested classes (namespacing). No inner classes (lifetime entanglement), no anonymous classes (lambdas suffice), no local classes.

**Pattern-matching extensions.** §7.5 covers sealed types. Missing: range patterns (`case 0..10 ->`), or-patterns (`case Circle | Square ->`), collection patterns (`case [first, ...rest] ->`), guard expressions beyond `when`. First two are cheap; collection patterns interact with iterators non-trivially.

### 19.5. Toolchain (Blocking)

**`jux.toml` schema.** Used in §2.4, §11.6, §16.5 but never specified. Needed: full TOML schema, dependency-resolution algorithm (recommend SemVer + lockfile, à la Cargo), workspace support, registry model.

**Unsafe boundaries.** FFI calls (§8) and raw-pointer ops break the borrow checker's promises but currently don't require explicit acknowledgment. Recommendation: add `unsafe` as a reserved keyword, require `unsafe { }` around FFI calls and raw-pointer dereferences. Aligns with Rust's hard-won lesson.

**Memory layout and ABI.** §8.4 lists permitted FFI types but doesn't specify struct layout, generic monomorphization symbol mangling, or ABI stability. Recommendation: Rust's model — default layout unspecified, `@layout(c)` for FFI types, mangling documented but unstable across compiler versions.

**Macros / annotation processing.** §3.6 introduces annotations, but without a processing model, derive-style (`@Serializable`, `@Test`-with-runtime-dispatch) is impossible. Recommendation: Rust-style hygienic macros, scoped to derive use cases. Likely v0.2.

**Edition model.** Not specified. Recommendation: Rust's edition system (`2026`, `2029`, …) — old code keeps compiling, edition migration is opt-in.

**Diagnostic codes.** Error messages throughout the spec are beautiful; they need stable codes (`E0042`) and a JSON output mode for editors.

### 19.6. Networking and Application Stack (Important)

§9.1 lists `std.net`, `std.json`, `std.crypto`, but they're unspecified. `std.http` is referenced in §10.4 but not in §9. Phase 1 should back each with a Rust crate (`tokio::net`, `serde_json`, `reqwest`/`axum`, RustCrypto) — but the public Jux APIs must be specified independently of those backings, since Phase 3 strips Rust from the stack.

The most consequential decision here is `std.json`'s shape: hand-written serializers, or `@Serializable` derive? The latter requires the macro/annotation-processing model from §19.5.

### 19.7. Priority Order for Sequencing

When writing v0.1, the order I'd attack these:

1. §19.1 — internal inconsistencies (cheap, unblocks correctness)
2. Foundational interfaces + exception hierarchy (§19.2 first two items)
3. `unsafe` boundaries + ABI/layout decisions (§19.5)
4. Collections + strings + I/O + time (§19.2)
5. Async streams (§19.2, last item)
6. `jux.toml` schema + testing framework polish (§19.5)
7. Const evaluation decisions (§19.4)
8. Reflection + macro model decisions (§19.4, §19.5)
9. Networking/HTTP/JSON (§19.6)

Items 1–6 constitute a usable v0.1; items 7–9 round out v0.2; pattern-matching extensions and edition migration are v1.0 polish.

### 19.8. Scope This Document Does Not Address

- **Implementation work** — Phase 1 transpiler bringup, runtime, packaging, distribution.
- **Ecosystem** — registry hosting, governance, learning materials, community building.
- **Performance benchmarks** — needs a working compiler.
- **Security model for the package registry** — signing, revocation, vulnerability reporting.

These belong in separate planning documents.

---

*End of v1 dossier. Update §19 as gaps resolve.*
