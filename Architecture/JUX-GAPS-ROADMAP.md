# Jux Spec — Gap Analysis and Roadmap

**Status:** Living document. Tracks specification gaps in the v0.1 dossier. Updated as addenda land.

**Purpose:** A single place to see what's left to write. Each gap is sized, prioritized, and given a recommended next step. When an addendum lands, the corresponding gap is moved to "Resolved" at the bottom.

---

## How to Read This Document

Each gap has:

- **Priority** — Blocking (must specify before v0.1), Important (matters for v0.2), Nice-to-have (incremental).
- **Size** — Rough estimate of how much spec text and design work it needs (S/M/L/XL).
- **Blocks** — What other work cannot proceed until this is specified.
- **Recommended approach** — Either "write addendum §X.Y" or a brief design sketch.

Gaps are grouped by category, not priority. The summary table at the end orders them by priority for sequencing.

---

## Resolved (Already Addressed)

- ✅ **Inheritance × borrow checker.** Addendum §6.9, §7.4.1.
- ✅ **Async / await.** Addendum §18 (v2, Kotlin-shaped).

---

## Category 1 — Standard Library

### 1.1. Foundational Interfaces

**Priority:** Blocking
**Size:** S (~150 lines)
**Blocks:** Everything in std. Every example in §7.8 (generics) references `Comparable<T>` without defining it.

The dossier mentions `Comparable`, `Iterable`, `Cloneable`, `Hashable` informally but never specifies them. These interfaces are the foundation every other std type builds on.

Required interfaces:

- `Equatable` — structural equality. Defines `equals(other) -> bool`.
- `Hashable extends Equatable` — defines `hashCode() -> int`. Contract: equal values have equal hashes.
- `Comparable<T>` — defines `compareTo(other: T) -> int`. Total order.
- `Cloneable<T>` — defines `clone() -> T`. Deep copy semantics.
- `Iterable<T>` — defines `iterator() -> Iterator<T>`. Used by `for-each`.
- `Iterator<T>` — defines `next() -> T?`. Returns null at end.
- `Displayable` — defines `toString() -> String`. Used by string interpolation.
- `Sized` — defines `size() -> int`. Used by collections.

Open questions to resolve:

- Does `==` dispatch to `equals` (Kotlin) or do reference comparison (Java)? Recommendation: `==` is structural via `Equatable`; `===` is reference identity. Matches Kotlin.
- Are these auto-derivable for records? Recommendation: yes — every `record` auto-implements `Equatable`, `Hashable`, `Displayable`, `Cloneable` from its fields.
- Iterator shape: `next() -> T?` (Rust/Swift) or `hasNext()/next()` (Java)? Recommendation: `next() -> T?` is simpler, integrates with nullability (§7.10), avoids the two-call dance.

**Recommended approach:** Write addendum §19.1 — "Foundational Interfaces." Define each interface with its contract and a worked example.

---

### 1.2. Error Type Hierarchy

**Priority:** Blocking
**Size:** S (~100 lines)
**Blocks:** §7.11 (error handling), §16.7 (Result-based errors), every `throws` clause in every example.

§7.11 shows `class FileError extends Exception` but never defines `Exception`. The hierarchy has to specify:

- `Exception` — base class. Fields: `message: String`, `cause: Exception?`. Constructor variants.
- `RuntimeException extends Exception` — for unchecked errors (arithmetic overflow, array bounds).
- `IOException extends Exception` — for I/O failures.
- `IllegalArgumentException`, `IllegalStateException`, `NullPointerException`, `IndexOutOfBoundsException`, `ArithmeticException` — standard subtypes.
- `CancellationException` — already referenced in §18.1.9.
- `TimeoutException` — already referenced in §18.1.9.

Stack trace policy:

- Captured at `throw` site by default (debug builds, `jux-full` release).
- Stripped in `jux-embedded` and `jux-core` (size cost).
- Available via `e.stackTrace` returning `StackFrame[]`.

Lowering policy (§16.7):

- `throws E` lowers to `Result<T, E>` in profiles without exceptions.
- The compiler synthesizes the conversion; user code is portable.

**Recommended approach:** Write addendum §19.2 — "Exception Hierarchy and Result Lowering." Specify the base class, the standard subtypes, and the lowering rules.

---

### 1.3. Collections

**Priority:** Blocking
**Size:** M (~400 lines)
**Blocks:** Almost every code example in the dossier.

Required:

- `List<T>` — already referenced. Mutable, growable, indexed.
- `Map<K, V>` — hash map. `K` must be `Hashable`.
- `Set<T>` — hash set. `T` must be `Hashable`.
- `Deque<T>` — double-ended queue.
- `LinkedList<T>` — for cases where `List<T>` is wrong (rare).
- `SortedMap<K, V>` / `SortedSet<T>` — tree-backed.
- `RingBuffer<T, N>` — already mentioned in §5.5; needs the full API.

Iterator protocol (depends on §1.1):

- Every collection implements `Iterable<T>`.
- `for (var x : collection)` desugars to iterator calls.
- Combinators (`map`, `filter`, `reduce`, `take`, `skip`, `chain`, `zip`, `flatten`, `groupBy`) defined on `Iterable<T>` via default methods.

Mutability question to resolve:

- Are `List`, `Map`, `Set` always mutable, or do we have `ImmutableList<T>` etc.? Recommendation: single mutable type per shape, with `.toImmutable()` returning a frozen view. Matches Kotlin. Avoids the Java `List<T>` vs `ImmutableList<T>` confusion.

**Recommended approach:** Write addendum §19.3 — "Collections." Spec the seven collection types with their full public API. Phase 1 implements each as a thin wrapper over a Rust counterpart (`Vec`, `HashMap`, `HashSet`, `VecDeque`, `BTreeMap`, `BTreeSet`).

---

### 1.4. Strings, I/O, Time

**Priority:** Blocking
**Size:** M (~600 lines combined)
**Blocks:** Practical programs.

**`std.string`:**

- `String` methods beyond the basics: `split`, `trim`, `replace`, `contains`, `startsWith`, `endsWith`, `toLowerCase`, `toUpperCase`, `chars()`, `bytes()`, `format()`.
- `StringBuilder` for incremental construction.
- `Regex` for pattern matching. Backed by a battle-tested engine; PCRE-like syntax.
- Format strings with `%` syntax for scientific/numeric formatting (`%.2f`, `%08x`). Distinct from `$` interpolation (§3.4) which is for general substitution.

**`std.io`:**

- File reading/writing: `readFile`, `writeFile`, `appendFile`, `File.open`.
- Streams: `InputStream`, `OutputStream`, `Reader`, `Writer`. Async-aware (return `T` from `async` methods, not `Future<T>`).
- Standard streams: `stdin`, `stdout`, `stderr`.
- Path manipulation: `Path` class with `join`, `parent`, `extension`, `exists`, `isDir`.
- Directory listing: `listDir`, `walkDir`.

**`std.time`:**

- `Instant` — point in time, monotonic.
- `Duration` — span. Constructors: `seconds(n)`, `milliseconds(n)`, `microseconds(n)`, `minutes(n)`, `hours(n)`, `days(n)`. Already used in §18.
- `Clock` — interface for `now()`. Standard impls: `SystemClock`, `MonotonicClock`, `MockClock` (for testing).
- `LocalDateTime`, `ZonedDateTime`, `LocalDate`, `LocalTime` — calendar types. Borrowed from `java.time`.
- Parsing/formatting via ISO 8601 by default; custom patterns optional.

**Recommended approach:** Three smaller addenda (§19.4, §19.5, §19.6) or one consolidated one. Recommend three — they have different audiences and review cycles.

---

### 1.5. Async Streams

**Priority:** Blocking
**Size:** S (~150 lines)
**Blocks:** Real I/O code. Every networking example needs streams.

§18.6 already calls this out as deferred. Required:

- `Stream<T>` interface: `async T? next()`. Returns null when exhausted.
- `for await (var x : stream)` syntax that desugars to `next()` calls in a loop.
- Stream combinators on async streams: `mapAsync`, `filterAsync`, `take`, `skip`, `chain`.
- Stream constructors: `Stream.of(items...)`, `Stream.from(iterable)`, `Stream.generate(() async -> T?)`.
- Backpressure: implicit. The producer suspends until the consumer calls `next()`.

Worked example to include:

```jux
public async void readLines(Path file) {
    var stream = File.openLines(file);          // returns Stream<String>
    for await (var line : stream) {
        if (line.startsWith("#")) continue;
        process(line);
    }
}
```

**Recommended approach:** Write addendum §18.6 — "Async Streams." Sits inside the concurrency section.

---

### 1.6. Networking, HTTP, JSON

**Priority:** Important
**Size:** L (~600 lines combined)
**Blocks:** Server applications, but not language usability.

Already referenced in async examples but not specified. Phase 1 backs each by a Rust crate:

- `std.net` — TCP/UDP sockets. Backed by `tokio::net`.
- `std.http` — HTTP client and server. Backed by `reqwest` and `axum` (or `hyper`).
- `std.json` — JSON parsing/serialization. Backed by `serde_json`.

The `std.json` design is the critical one: does Jux have derive-style annotations that auto-generate serializers, or does the user write them by hand?

**Recommendation:** Use compile-time annotations. `@Serializable` on a record auto-generates `toJson()` and `fromJson()`. This requires committing to a metaprogramming model first (see §3.4 below).

---

## Category 2 — Type System Polish

### 2.1. Operator Overloading Policy

**Priority:** Important
**Size:** S (decision) + M (spec if yes)
**Blocks:** Math libraries, vector types, custom numeric types.

Three options:

1. **No overloading** (Java). Simplest. Loses ergonomic vector math.
2. **Function-name overloading** (Kotlin). `operator fun plus(other: Vec) = ...`. Reasonable middle ground.
3. **Trait-based overloading** (Rust, Swift). `implements Add<Vec>`. Most flexible.

**Recommendation:** Option 2 (Kotlin's approach). Defined operators are listed in a fixed set (`+`, `-`, `*`, `/`, `%`, `==`, `<`, `[]`, `()`, `..`). Each maps to a method by convention (`plus`, `minus`, `times`, etc.). No surprises, no infinite operator soup.

Decide and document. A two-page addendum suffices.

---

### 2.2. Equality and Hashing Semantics

**Priority:** Blocking (it's already implicit in examples)
**Size:** S
**Blocks:** Use of any type as a `Map` key.

Resolves alongside §1.1 (Foundational Interfaces). The decisions:

- `==` calls `equals()` (structural). `===` is reference identity (only meaningful for class types).
- Records auto-derive `equals` and `hashCode` from fields.
- Classes inherit `Object`-style identity equality unless they implement `Equatable`.
- Structs auto-derive structural equality (they're value types).

Document this in the §19.1 addendum on foundational interfaces.

---

### 2.3. Nested and Inner Classes

**Priority:** Important
**Size:** S
**Blocks:** Idiomatic class organization.

§7 doesn't cover nested types. Need to decide:

- **Static nested classes** (Java's `static class Inner`): just namespacing, no outer reference. Yes, support these.
- **Inner classes** (Java's non-static `class Inner`): hold a reference to the enclosing instance. Footgun (lifetime entanglement). Recommendation: do not support. Force composition.
- **Anonymous classes**: `new Runnable() { ... }`. Replaced by lambdas in modern Java; recommend skipping.
- **Local classes** (declared inside a method): rarely useful. Skip.

**Recommendation:** Support only static nested classes. One paragraph in §7.

---

### 2.4. Reflection and Metadata

**Priority:** Important (for ecosystem, not the language)
**Size:** M
**Blocks:** Serialization libraries, ORMs, test frameworks, dependency injection.

Three positions:

1. **None** (Rust). Generic code uses traits. Reflection-like behavior comes from macros.
2. **Compile-time only** (Swift). Mirrors at compile time, not runtime.
3. **Full runtime reflection** (Java). Heavy but flexible.

**Recommendation:** Option 2. Reflection metadata is generated at compile time only for types annotated `@Reflectable`. The default is no reflection (zero size cost). A `Type<T>` API gives compile-time access to fields, methods, annotations for those that opt in. This pairs with §3.4 (annotations) to support `@Serializable`-style derive macros without needing full runtime reflection.

---

### 2.5. Pattern Matching Extensions

**Priority:** Nice-to-have
**Size:** S-M
**Blocks:** Nothing critical.

§7.5 covers sealed-type matching. What's missing:

- Range patterns: `case 0..10 -> ...`
- Guards beyond `when`: any boolean expression
- Collection patterns: `case [first, ...rest] -> ...`
- String patterns: `case "exit" -> ...` (already implied; needs spec)
- Or-patterns: `case Circle | Square -> ...`

**Recommendation:** Add range patterns and or-patterns now (cheap). Defer collection patterns to v0.2 (interacts with iterator protocol in non-obvious ways).

---

### 2.6. Compile-Time Constants

**Priority:** Important for embedded
**Size:** M
**Blocks:** Const generics like `byte[N]` from §5.5 in non-trivial cases.

§1.2 says "limited const evaluation only." Limited how?

Decisions to make:

- Can `const fn` exist? (functions evaluable at compile time)
- Can const expressions involve recursion? Loops?
- Can const generics be computed (`byte[N + M]`)?

**Recommendation:** Allow const expressions to use:
- Arithmetic on integer literals
- `if` and `match`
- Const function calls (functions marked `const`)
- Bounded recursion (depth limit)

Disallow:
- Heap allocation
- I/O
- Unbounded loops

Roughly Rust's `const fn` model. Powerful enough for `RingBuffer<T, N + 1>`-style typing, restrictive enough to keep compilation tractable.

---

## Category 3 — Toolchain

### 3.1. Build System and Package Manager

**Priority:** Blocking (for any user with multiple files)
**Size:** L
**Blocks:** Multi-file projects, dependency use, the entire ecosystem.

`jux.toml` appears in §2.4 and §16.5 but is never specified. Required:

- Manifest format (full TOML schema).
- Dependency resolution algorithm. Recommend SemVer + lockfile, like Cargo.
- Registry model. Recommend a default registry (`registry.jux-lang.org`) with the option to use Git URLs and local paths.
- Build commands: `juxc build`, `juxc run`, `juxc test`, `juxc doc`, `juxc fmt`.
- Workspace support (multiple packages in one repo).

**Recommendation:** Steal the Cargo design wholesale. It is the most-loved tooling story in modern languages and there's no reason to differ. Write a §20 addendum titled "Build System and Package Manager."

---

### 3.2. Testing Framework

**Priority:** Blocking (no language ships without one in 2026)
**Size:** S
**Blocks:** CI for any Jux project.

Need:

- `@Test` annotation on functions. Discovered by `juxc test`.
- Assertion library: `assert`, `assertEquals`, `assertThrows`, `assertNull`, etc.
- Setup/teardown: `@BeforeEach`, `@AfterEach`, `@BeforeAll`, `@AfterAll`.
- Async tests: `@Test async void ...` works naturally.
- Test reporting: human-readable + JUnit XML for CI integration.
- Property testing: `@Property` with generated inputs (later, not v0.1).

**Recommendation:** Write addendum §21 — "Testing." Mostly mechanical.

---

### 3.3. Tooling and IDE Support

**Priority:** Blocking (for adoption, not for the language itself)
**Size:** XL
**Blocks:** Editor support, refactoring, intellisense.

Not a spec gap — an implementation gap. But the spec should commit to:

- The compiler exposes its parser and type checker as a library.
- A language server (`juxc lsp`) ships with the toolchain. LSP protocol.
- Diagnostic output is JSON-structured for editor consumption.

**Recommendation:** A short §22 addendum committing to LSP-based tooling and a stable parser API. Implementation comes later but the commitment matters.

---

### 3.4. Macros and Annotation Processing

**Priority:** Important
**Size:** L
**Blocks:** Derive-style annotations (`@Serializable`, `@Test`, ORM mappings).

§3.6 introduces annotations but doesn't specify how user-defined annotations get processed. The decision is whether Jux has:

1. **No metaprogramming** (Java pre-annotation-processors). Forces every `@Serializable` impl to be hand-written or runtime-reflective.
2. **Annotation processors** (Java). Compile-time, ad-hoc, ugly to write.
3. **Hygienic macros** (Rust, Swift). Powerful, complex.
4. **Decorator-style with limited capability** (TypeScript, Python). Mostly runtime.

**Recommendation:** Option 3 (Rust-style hygienic macros), but limited to what `derive`-style annotations need. The author of `@Serializable` writes a compiler plugin in Jux that examines the annotated type's fields and generates code. This is enough for serialization, ORM, RPC stubs, and test frameworks without exposing arbitrary AST manipulation.

This is the longest-lead spec gap. Probably v0.2.

---

### 3.5. Documentation Generator Output

**Priority:** Nice-to-have
**Size:** S
**Blocks:** Discoverable APIs.

§3.5 mentions `juxc doc` runs example code blocks. What's missing: the output format. Recommend HTML similar to rustdoc, with JSON metadata for third-party doc tools.

---

## Category 4 — Strategic / Cross-Cutting

### 4.1. FFI Safety Boundaries

**Priority:** Blocking
**Size:** M
**Blocks:** Any C interop in real code.

§8 promises C/C++/Rust interop is direct. But calling C means losing the borrow checker, exception safety, and type guarantees. Two camps:

1. **`unsafe` blocks** (Rust). FFI calls and raw pointer ops require explicit `unsafe { ... }`. Forces the author to acknowledge the risk.
2. **No marker** (Swift, Kotlin). FFI calls look like normal calls. Convenient, hides risk.

§3.2 has no `unsafe` keyword listed. Either:

- Add `unsafe` as a reserved keyword and require it for FFI/raw pointers.
- Commit explicitly to the no-marker approach and document the safety considerations elsewhere.

**Recommendation:** Add `unsafe`. The borrow checker promises memory safety; FFI breaks that promise. An `unsafe` block is the minimum acknowledgment. Cost is one keyword and slightly noisier FFI. Benefit is clear safety boundaries — the same reason Rust succeeded where C++'s "trust the programmer" failed.

Write a small addendum touching §3.2 (add the keyword) and §8 (specify when `unsafe` is required).

---

### 4.2. Memory Layout and ABI

**Priority:** Blocking (for FFI)
**Size:** M
**Blocks:** Real C interop, generated `.h` files (§2.3), shared libraries.

Required:

- Struct field layout: declaration order, with alignment? `#[repr(C)]`-equivalent for FFI types?
- Generic monomorphization: how are symbols mangled? Is the mangling stable across compiler versions?
- Calling conventions for `@export` functions.
- ABI compatibility commitments. Likely none initially, like Rust.

**Recommendation:** Adopt Rust's approach: default layout is unspecified and subject to optimization; types meant for FFI must be marked `@layout(c)` (or similar). Symbol mangling follows a documented but unstable scheme. ABI stability is not promised across compiler versions.

Write a §8.5 addition.

---

### 4.3. Versioning and Stability Policy

**Priority:** Important
**Size:** S
**Blocks:** Long-term ecosystem health.

When `juxc 1.1` ships, can `juxc 1.0` code still compile? Three models:

1. **Strict backward compatibility** (Java, Go). Old code always compiles. Restricts language evolution.
2. **Editions** (Rust). Code declares an edition (`2015`, `2018`, `2021`, `2024`). Each edition is forward-stable; edition migration is opt-in.
3. **No commitment** (most pre-1.0 languages).

**Recommendation:** Edition model. It is the only one that lets the language evolve while keeping old code working. The first edition is `2026`; subsequent editions ship every 2-3 years.

---

### 4.4. Compiler Diagnostics Specification

**Priority:** Nice-to-have
**Size:** S
**Blocks:** Reproducible error message quality.

The dossier shows beautiful error messages (§6.7, §6.9.7, §18.1.6). What's missing:

- Error codes (e.g., `E0042`) so messages are searchable and stable across versions.
- Severity levels (error, warning, lint, hint).
- Machine-readable JSON output (`--diagnostic-format=json`) for editor integration.
- A diagnostics catalog page in the docs.

**Recommendation:** A short §22 addendum. Mostly committing to what's already implicitly desired.

---

## Priority Summary

The order suggested for sequencing addenda:

| # | Addendum | Priority | Size | Why now |
|---|----------|----------|------|---------|
| 1 | §19.1 — Foundational Interfaces | Blocking | S | Unblocks all of std, plus generic constraints already used in examples |
| 2 | §19.2 — Exception Hierarchy | Blocking | S | Every `throws` clause references undefined `Exception` |
| 3 | §3.2 + §8 — `unsafe` and FFI Boundaries | Blocking | M | Decide before any real FFI work |
| 4 | §18.6 — Async Streams | Blocking | S | Needed for real I/O examples |
| 5 | §19.3 — Collections | Blocking | M | Almost every example references `List`, `Map`, `Set` |
| 6 | §19.4-6 — Strings, I/O, Time | Blocking | M | Practical programs |
| 7 | §20 — Build System (`jux.toml`) | Blocking | L | Multi-file projects |
| 8 | §21 — Testing Framework | Blocking | S | CI from day one |
| 9 | §8.5 — Memory Layout and ABI | Blocking | M | FFI correctness |
| 10 | §19.7 — Equality semantics (rolls into §19.1) | Blocking | — | Captured in #1 |
| 11 | §7.x — Nested classes | Important | S | Idiomatic class organization |
| 12 | §x — Operator overloading | Important | S | Math/vector libraries |
| 13 | §19.8 — Reflection (compile-time) | Important | M | Serialization, frameworks |
| 14 | §x — Const evaluation | Important | M | Const generics in embedded |
| 15 | §22 — Diagnostics spec | Important | S | Reproducible tooling |
| 16 | §x — Edition model | Important | S | Long-term ecosystem |
| 17 | §19.9-11 — Networking/HTTP/JSON | Important | L | Servers |
| 18 | §x — Macros / annotation processing | Important | L | Derive-style annotations |
| 19 | §x — Pattern matching extensions | Nice-to-have | S | Ergonomics |
| 20 | §x — Doc generator output | Nice-to-have | S | Discoverability |
| 21 | §x — IDE / LSP commitment | Nice-to-have (for spec) | S | Adoption |

Items 1-9 are the v0.1 critical path. Items 10-18 fill out v0.2. Items 19-21 are polish.

---

## What v0.1 Looks Like When Items 1-9 Land

A user can:

- Write multi-file Jux projects with `jux.toml`.
- Use `List`, `Map`, `Set`, `String`, `Path`, `Instant`, `Duration` confidently.
- Read files, write files, make HTTP-less network calls (TCP/UDP at minimum).
- Throw exceptions with a documented hierarchy, or use `Result<T, E>`.
- Write async code that suspends, spawns, awaits, and streams.
- Run tests with `juxc test`.
- Call C libraries through `unsafe` blocks with documented ABI rules.

That's a usable language. Items 10-18 make it competitive with mature languages; items 19-21 make the experience polished.

---

## What This Document Doesn't Cover

- **Implementation work** — getting the Phase 1 transpiler running, building the runtime, packaging the toolchain. Separate from spec.
- **Ecosystem** — registry hosting, documentation site, learning materials, governance.
- **Performance benchmarks and validation** — needs working compiler first.

These belong in a separate planning document.

---

*Generated alongside the inheritance and async/await addenda. Update as gaps resolve.*
