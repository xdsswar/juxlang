# Jux Spec Addendum §21 — Testing

**Status:** Normative. Fills roadmap §3.2 ("Testing Framework") and
supersedes the JUX-LANG-V1 §12.6 sketch where they differ (divergences
called out inline). Phase-1 implementation notes are marked as such —
they describe today's behavior, not the final design.

Tests are ordinary Jux functions: no special file format, no test
classes, no inheritance from a framework type. An annotation marks them,
`jux test` finds and runs them, and assertion functions from
`jux.std.testing` report failures by throwing.

---

## §TS.1 — Annotations

| Annotation    | Placement                          | Meaning |
|---------------|------------------------------------|---------|
| `@Test`       | free function, `void` / `async void`, no parameters | A test. |
| `@BeforeEach` | same                               | Runs before every `@Test` in the same file. |
| `@AfterEach`  | same                               | Runs after every `@Test` in the same file — **including failed ones**. |
| `@BeforeAll`  | same                               | Runs once per file, before its first executed test. |
| `@AfterAll`   | same                               | Runs once per file, after its last executed test. |

- Annotation names are **case-insensitive** (`@test` ≡ `@Test`), like
  every built-in annotation.
- The annotations are **compiler builtins — no import is required**.
  *(Divergence from the V1 §12.6 example, which imported `Test` from
  `std.testing`: Phase 1 has no annotation declarations to import, and
  none are needed.)*
- An annotated function with parameters, a non-`void` return, or a
  non-free position (method) is a compile error.
- Hooks are synchronous in Phase 1; tests may be `async`.

## §TS.2 — Discovery and Execution

`jux test` compiles every `.jux` source under the project's `src/` and
`test/` directories (plus resolved `[dependencies]`, §B.2.2 — tests see
the same dependency set the build does) into a test-runner binary, runs
it, and forwards its exit code.

- The **unit of grouping is the file** (compilation unit). Hooks apply
  to the `@Test`s of their own file only.
- Tests run **sequentially, in source order** within a file; files run
  in path order. *(Parallel execution is the documented end state —
  §TS.9.)*
- `test/` files are exempt from the `src/` package-directory layout
  rule; declaring a package in them remains recommended.
- A test's display name is its package-qualified function name.

## §TS.3 — Assertions: `jux.std.testing`

Pure-Jux free functions; import with `import jux.std.testing.*;`.
A failed assertion **raises an assertion failure** carrying a
descriptive message (§TS.4).

```jux
void assertEqual<T>(T expected, T actual)
    where T has operator==(T) -> bool, T has operator string() -> String;
void assertNotEqual<T>(T expected, T actual)
    where T has operator==(T) -> bool, T has operator string() -> String;
void assertTrue(bool condition, String message = "assertTrue failed");
void assertFalse(bool condition, String message = "assertFalse failed");
void assertNull<T>(T? value);
void assertNotNull<T>(T? value);
void assertNear(double expected, double actual, double epsilon = 1e-9);
Exception assertThrows(() -> void f);
```

- `assertEqual`/`assertNotEqual` require the value's `operator==` and
  `operator string` (satisfied by primitives, `String`, enums, records,
  and any class declaring the operators). For types without them, use
  `assertTrue(a == b, "...")`.
- **Floats:** `assertEqual` on `double` compares exactly; `assertNear`
  is the sanctioned approximate comparison.
- **`assertThrows` (divergence from V1 §12.6):** Jux has no class
  literals, so the sketched `assertThrows(MathError, () -> …)` is not
  expressible. The Phase-1 form runs `f`, **fails if nothing was
  thrown**, and returns the caught `Exception` for inspection:

  ```jux
  var e = assertThrows(() -> divide(10, 0));
  assertTrue(e.getMessage().contains("zero"));
  ```

  A typed `assertThrows<E>(() -> void f)` is reserved for a later
  phase.
- The §S.7.2 builtin `assert(condition, message)` remains available
  everywhere. Under `jux test` it is **always checked** (it lowers to a
  release-elided debug assertion in ordinary builds).

## §TS.4 — Assertion Failures Are Panics

A failed assertion is a **runtime panic** (ERRATA E1 mechanics), not a
thrown exception. The consequence is the design goal: a
`catch (Exception e)` in the code under test — or inside `assertThrows`
itself — can **never swallow a test failure**; only the test runner's
own boundary observes it and reports the message.

*(Phase-1 note: the spec's `throw` statement requires an `Exception`
subclass (§X.2.1), so an `Error`-derived `AssertionError` class is not
throwable from Jux source; the panic mechanism delivers the same
"uncatchable by user code" semantics directly. The `AssertionError`
name stays reserved in `jux.std.testing` for a future typed form.)*

## §TS.5 — Lifecycle Ordering

Per file: `BeforeAll` → (`BeforeEach` → test → `AfterEach`)× → `AfterAll`.

- Multiple hooks of one kind run in declaration order.
- `BeforeAll`/`AfterAll` run only if at least one test of the file is
  executed (they respect filtering, §TS.8).
- A `BeforeEach` failure fails the test (the test body does not run);
  `AfterEach` still runs.
- An `AfterEach`/`AfterAll` failure fails the test/run but never masks
  the test's own result message.

## §TS.6 — Async Tests

A test declared `async void` is driven to completion by the runner
(blocking on its future). Awaits inside it behave exactly as in an
`async main`.

## §TS.7 — Output and Exit Codes

```text
running N tests
  PASS pkg.testName
  FAIL pkg.other: assertEqual: expected `5`, got `4`

test result: FAILED. M passed; K failed
```

Exit code `0` when every executed test passed; `1` on any failure or
compile error. The FAIL message is the thrown `AssertionError`'s
message, a panic string, or `<exception-class>: <message>` for other
escaped exceptions.

## §TS.8 — Filtering

`jux test <pattern>` runs only tests whose display name **contains**
`<pattern>` (plain substring, evaluated at runtime — no recompilation).
The summary appends `; N filtered out`. `jux test --release` builds the
runner with optimizations.

## §TS.9 — Deferred (post-v0.1)

Parallel execution (the documented default end-state), JUnit-XML
reporting, `@Property` generative tests, `@Ignore`, per-assertion
source locations in failure messages, typed `assertThrows<E>`.
