# Jux Spec Addendum — Diagnostic Codes Catalog

**Status:** Proposed insertion. Provides the master index of every diagnostic code referenced in JUX-LANG-V1 and the prior addenda, the JSON schema for machine-readable output, the format for human-readable rendering, and the stability promise.

**Insertion points:**
- New §D.1 ("Diagnostic Format")
- New §D.2 ("JSON Schema")
- New §D.3 ("Code Allocation by Phase")
- New §D.4 ("Master Catalog")
- New §D.5 ("Stability and Lifecycle")

---

## Design Philosophy (Non-Normative)

Compiler diagnostics are the most-read part of any compiler's output. They must be:

- **Stable.** A code (`E0450`) means the same thing forever. Programmers who memorize codes shouldn't be retrained.
- **Machine-readable.** CI systems, IDEs, and editor plugins parse diagnostics. JSON output is mandatory.
- **Human-readable.** The default terminal output is multi-line, color-coded, and context-rich.
- **Catalogued.** Every code has a docs page with description, example, and remediation.
- **Actionable.** When a fix is plausible, the diagnostic suggests it (sometimes with a code action the editor can apply).

Rust set the bar here. Jux follows it.

---

## §D.1 — Diagnostic Format

### D.1.1. Anatomy of a Diagnostic

Every diagnostic carries:

- **`code`** — the stable identifier (e.g., `E0450`).
- **`severity`** — `error`, `warning`, `lint`, or `note`.
- **`message`** — a one-line summary in plain English.
- **`primary_span`** — the source location the diagnostic is "about."
- **`secondary_spans`** — zero or more related locations with their own messages.
- **`hint`** — optional one-line suggested fix.
- **`code_action`** — optional structured edit the editor can apply (replacement text + range).
- **`docs_url`** — the URL to the full documentation page.

### D.1.2. Severity Levels

| Severity | Meaning                                                     | Exit code impact |
|----------|-------------------------------------------------------------|------------------|
| `error`  | The compilation cannot produce correct output.               | Fails the build  |
| `warning`| Likely a problem but compilation continues.                  | Does not fail unless `-Werror` |
| `lint`   | A style or best-practice suggestion. Configurable per project.| Same as warning |
| `note`   | Supplementary context attached to another diagnostic.        | Never fails      |

### D.1.3. Default Terminal Format

```
error[E0450]: ambiguous overload of `log`
  --> src/foo.jux:14:5
   |
14 |     log(value);
   |     ^^^ ambiguous: could be `log(int)` or `log(String)`
   |
note: candidate `log(int)` defined here
  --> src/util.jux:8:5
   |
 8 | public void log(int code) { ... }
   |             ---
note: candidate `log(String)` defined here
  --> src/util.jux:12:5
   |
12 | public void log(String message) { ... }
   |             ---
help: cast the argument to disambiguate
   |
14 |     log(value as int);
   |        +++++++++++

For more information about this error, see https://docs.jux-lang.org/diag/E0450
```

The format is identical to Rust's. Color: red for `error` markers, yellow for `warning`, blue for `note`, green for `help`.

### D.1.4. Compact Format

`--diagnostic-format=compact` produces one line per diagnostic, suitable for older tooling:

```
src/foo.jux:14:5: error[E0450]: ambiguous overload of `log`
src/util.jux:8:5: note: candidate `log(int)` defined here
src/util.jux:12:5: note: candidate `log(String)` defined here
```

### D.1.5. Short Format

`--diagnostic-format=short` produces one line per diagnostic with no notes/hints, only the primary message:

```
src/foo.jux:14:5: error[E0450]: ambiguous overload of `log`
```

---

## §D.2 — JSON Schema

`--diagnostic-format=json` emits one JSON object per line (NDJSON), one object per top-level diagnostic.

### D.2.1. Top-Level Object

```json
{
  "code": "E0450",
  "severity": "error",
  "message": "ambiguous overload of `log`",
  "primary_span": {
    "file": "src/foo.jux",
    "byte_start": 142,
    "byte_end": 145,
    "line_start": 14,
    "line_end": 14,
    "column_start": 5,
    "column_end": 8,
    "snippet": "    log(value);",
    "highlight_start": 4,
    "highlight_end": 7,
    "label": "ambiguous: could be `log(int)` or `log(String)`"
  },
  "secondary_spans": [
    {
      "file": "src/util.jux",
      "byte_start": 80,
      "byte_end": 83,
      "line_start": 8,
      "line_end": 8,
      "column_start": 13,
      "column_end": 16,
      "snippet": "public void log(int code) { ... }",
      "highlight_start": 12,
      "highlight_end": 15,
      "label": "candidate `log(int)` defined here",
      "severity": "note"
    },
    {
      "file": "src/util.jux",
      "byte_start": 132,
      "byte_end": 135,
      "line_start": 12,
      "line_end": 12,
      "column_start": 13,
      "column_end": 16,
      "snippet": "public void log(String message) { ... }",
      "highlight_start": 12,
      "highlight_end": 15,
      "label": "candidate `log(String)` defined here",
      "severity": "note"
    }
  ],
  "hint": "cast the argument to disambiguate",
  "code_action": {
    "title": "Cast to int",
    "edits": [
      {
        "file": "src/foo.jux",
        "byte_start": 145,
        "byte_end": 145,
        "replacement": " as int"
      }
    ]
  },
  "docs_url": "https://docs.jux-lang.org/diag/E0450"
}
```

### D.2.2. Span Object Schema

Every span has:

- `file` — path relative to the project root, forward-slash separated.
- `byte_start`, `byte_end` — UTF-8 byte offsets into the file.
- `line_start`, `line_end` — 1-indexed line numbers.
- `column_start`, `column_end` — 1-indexed column numbers (Unicode-character columns, not byte offsets within the line).
- `snippet` — the line(s) containing the span, exactly as in the source.
- `highlight_start`, `highlight_end` — 0-indexed byte offsets within `snippet` for the highlighted region.
- `label` — optional short string explaining the span's role.
- `severity` — for secondary spans, one of `note`, `help`, `error`, `warning`. Defaults to `note`.

### D.2.3. Code Action Schema

When the compiler can suggest an automated fix:

```json
"code_action": {
  "title": "Add the missing override marker",
  "edits": [
    {
      "file": "src/foo.jux",
      "byte_start": 100,
      "byte_end": 100,
      "replacement": "@Override\n    "
    }
  ]
}
```

Each edit is an insertion (start == end), deletion (replacement == ""), or replacement. Multiple edits in one action are applied atomically.

### D.2.4. NDJSON Stream

Diagnostics stream one per line; the consumer (LSP, editor, CI) reads them as they arrive. This lets fast feedback in editors: parse errors appear before type errors, type errors before borrow errors.

A trailing summary line:

```json
{"summary": {"errors": 2, "warnings": 5, "files_compiled": 12, "duration_ms": 1840}}
```

---

## §D.3 — Code Allocation by Phase

Each compiler phase (per `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.1.2) owns a contiguous range of error codes. Codes never cross phases.

| Range          | Phase | Owner module          | Description                              |
|----------------|-------|-----------------------|------------------------------------------|
| `E0100–E0199`  | 1     | `juxc::lex`           | Lexical errors                           |
| `E0200–E0299`  | 2     | `juxc::parse`         | Syntax errors                            |
| `E0300–E0399`  | 4     | `juxc::resolve`       | Name resolution and module errors         |
| `E0400–E0499`  | 6–9   | `juxc::tycheck`       | Type checking and inference               |
| `E0500–E0599`  | 11    | `juxc::borrow`        | Borrow checker                            |
| `E0600–E0699`  | 12–13 | `juxc::lower`         | Drop, move, refcount lowering            |
| `E0700–E0799`  | 15    | `juxc::async`         | Async / generator lowering                |
| `E0800–E0899`  | 16    | `juxc::const-eval`    | Const evaluation                          |
| `E0900–E0999`  | 17–19 | `juxc::backend`       | Monomorphization, codegen, link errors    |
| `W0100–W0999`  | any   | various               | Warnings (one digit higher than the corresponding error range) |

Cross-phase codes (e.g., overflow detected in const eval and runtime) reuse the same code and clarify in the message.

---

## §D.4 — Master Catalog

The complete catalog of error and warning codes referenced across JUX-LANG-V1 and all addenda. Each entry includes a one-line description and, where useful, the addendum and section that introduced it.

The catalog contains two kinds of entries: codes **implemented** in the compiler (the `juxc_diagnostics::Code` enum mirrors this catalog) and codes **reserved** for checks that are specced but not yet implemented — the latter are marked *(reserved)*. Where the implementation and an older draft of this table disagreed, the implementation's meaning is canonical (codes already emitted to users cannot be reassigned; see §D.5.1).

### Lexical (`E0100–E0199`)

| Code     | Description                                         | Source                        |
|----------|-----------------------------------------------------|-------------------------------|
| `E0100`  | Invalid character in source                          | Grammar §A.1                   |
| `E0101`  | Unterminated string literal                          | Grammar §A.1.5                 |
| `E0102`  | Invalid digit separator placement                    | Grammar §A.1.4                 |
| `E0103`  | Invalid Unicode escape (surrogate code point)         | Grammar §A.1.5                 |
| `E0104`  | Block comment never terminated                       | Grammar §A.1.2                 |
| `E0105`  | Numeric literal out of range for its declared type   | Grammar §A.1.4                 |
| `E0150`  | Invalid `@cfg(...)` predicate syntax                 | Pipeline §C.2.5                |

### Syntax (`E0200–E0299`)

| Code     | Description                                         | Source                        |
|----------|-----------------------------------------------------|-------------------------------|
| `E0200`  | Unexpected token                                     | Generic parse error            |
| `E0210`  | `super(...)` or `this(...)` not first statement      | Grammar §A.2.4                 |
| `E0211`  | Constructor missing required `super(...)` call      | Grammar §A.2.4                 |
| `E0212`  | Varargs (`T...`) parameter is not the last parameter | Entry Points §E (varargs) |
| `E0220`  | Sealed type with `permits` clause needed *(reserved)* | Grammar §A.2.5                 |
| `E0240`  | `try` block without `catch` or `finally` *(reserved)* | Grammar §A.2.8                 |
| `E0241`  | Label-targeted `break`/`continue` mismatched *(reserved)* | Grammar §A.2.8              |
| `E0260`  | `if`-expression missing `else` branch *(reserved)*   | Grammar §A.2.9                 |
| `E0261`  | `switch` expression must be exhaustive *(reserved; statement form is `E0440`)* | Type system §T.5 |
| `E0270`  | Or-pattern alternatives bind incompatible names *(reserved)* | Grammar §A.3            |
| `E0271`  | Local-variable destructuring requires irrefutable pattern *(reserved)* | Grammar §A.3  |
| `E0272`  | Pattern guard expression must have type `bool` *(reserved)* | Grammar §A.3             |

### Resolution (`E0300–E0399`)

| Code     | Description                                         | Source                        |
|----------|-----------------------------------------------------|-------------------------------|
| `E0301`  | Name not found in scope                              | Build system §B.4.1           |
| `E0302`  | Cyclic module import *(reserved)*                    | Build system §B.4.6           |
| `E0303`  | Multiple resolution candidates for name *(reserved)* | Build system §B.4.1           |
| `E0304`  | Duplicate local declaration in the same scope        | JUX-LANG-V1 §6.1 / Semantics §S.1.4 |
| `E0307`  | Duplicate annotation name (case-insensitive collision) *(reserved)* | JUX-LANG-V1 §3.6 / Annotations §A.13 |
| `E0320`  | Entry file has both top-level statements and a `main` function | Entry Points §E.6     |
| `E0321`  | Multiple functions carry `@entry` in the same binary *(reserved)* | Entry Points §E.6   |
| `E0322`  | `@entry(convention = ...)` unsupported on current target *(reserved)* | Entry Points §E.6 |
| `E0323`  | `main`'s signature does not match any accepted form  | Entry Points §E.6              |
| `E0324`  | `@entry` function's signature incompatible with its symbol's ABI *(reserved)* | Entry Points §E.6 |
| `E0325`  | `freestanding = true` but no `@entry` function declared *(reserved)* | Entry Points §E.6 |
| `E0326`  | A class member named `main` with an entry-shaped signature is not `static` | Entry Points §E.1.2.2 |

### Type Checking (`E0400–E0499`)

| Code     | Description                                         | Source                        |
|----------|-----------------------------------------------------|-------------------------------|
| `E0400`  | Duplicate top-level declaration (class, record, enum, interface, or function) | Single-namespace rule |
| `E0401`  | Duplicate field in the same class body              | —                              |
| `E0402`  | Duplicate method in the same class body (lifted once overload resolution lands) | Type system §T.3 |
| `E0403`  | Duplicate variant in the same enum body             | —                              |
| `E0410`  | Type mismatch — assignments, returns, call arguments; also mixed-type arithmetic without explicit `as` and nullable-primitive types | Semantics §S.2.6 / ERRATA E5 |
| `E0411`  | Wrong number of positional call arguments           | —                              |
| `E0412`  | `obj.field` doesn't exist on the receiver (inheritance chain walked) | —             |
| `E0413`  | `obj.method(...)` / `new T(...)` target doesn't resolve | —                          |
| `E0414`  | Access to a `private` member from outside the declaring class | —                    |
| `E0415`  | Access to a `protected` member from outside the extends-chain | ERRATA E4            |
| `E0416`  | Access to a package-private / `internal` member from outside its package | —         |
| `E0420`  | `class C extends F` where `F` is `final`            | JUX-LANG-V1 §7.4               |
| `E0421`  | Override of a `final` method                        | JUX-LANG-V1 §7.4.1             |
| `E0422`  | Sealed class extended outside its `permits` list    | JUX-LANG-V1 §7.4               |
| `E0423`  | `extends` target is not a class                     | classes-rules §1.2             |
| `E0424`  | `implements` target is not an interface             | classes-rules §3               |
| `E0425`  | `this` referenced in a static context               | —                              |
| `E0426`  | `@Override` on a method that overrides nothing      | Annotations                    |
| `E0427`  | Static method called via an instance receiver       | —                              |
| `E0428`  | `new X(...)` where `X` is not instantiable (interface, enum, alias) | —             |
| `E0429`  | Interface abstract method(s) not implemented        | classes-rules §3               |
| `E0430`  | Conflicting default methods (diamond) — class must override explicitly | Type system §T.8.2 |
| `E0431`  | Invalid method-modifier combination (see collision note below) | classes-rules §1.4  |
| `E0432`  | Invalid visibility on a top-level type (`private` / `protected`) | classes-rules §1.1 / §3.1 |
| `E0433`  | Override narrows visibility relative to the overridden method | classes-rules §1.4   |
| `E0434`  | Cyclic `extends` chain                              | classes-rules §1.2             |
| `E0435`  | Interface not usable as a dyn-dispatched value type (generic interface / generic method) | Interface dispatch, stage 1 |
| `E0436`  | Exception-hierarchy class also `implements` an interface (deferred combination) | Interface dispatch, stage 1 |
| `E0437`  | Data field accessed through a polymorphic-base reference | Polymorphism, stage 2     |
| `E0438`  | Generic virtual method on a polymorphic base class  | Polymorphism, stage 2          |
| `E0440`  | Switch is not exhaustive                             | Type system §T.5.5            |
| `E0441`  | Type-test smart-cast binder (`x => T name`) used outside an `if` condition | Polymorphism |
| `E0442`  | Reference cast / type-test between unrelated types  | Polymorphism                   |
| `E0443`  | Malformed explicit call-site type-argument list (`id<int>(5)`) | Generics (Gap 5)      |
| `E0444`  | Bounded wildcard as a storage type over a user generic class (Phase-1 limitation) | Generics (Gap 4) |
| `E0445`  | Const-generic form outside the Phase-1 core subset  | Type system §T.11.3 / Grammar §A.2.6 |
| `E0447`  | Or-pattern alternative introduces bindings (`case A(var x) \| B ->`) | Grammar §A.3 |
| `E0448`  | Malformed named-argument list (unknown name, duplicate slot, positional after named) | Grammar §A.2.9 / Type system §T.3.2 |
| `E0449`  | Default-value expression references another parameter (Phase-1 limitation; §S.1.3 full form deferred) | Semantics §S.1.3 |
| `E0450`  | Ambiguous overload (Phase 1: overlapping constructor arity ranges) | Type system §T.3 |
| `E0450`  | Ambiguous overload *(reserved)*                      | Type system §T.3.3            |
| `E0451`  | No overload candidate produces required return type *(reserved)* | Type system §T.3.4 |
| `E0452`  | No matching operator overload *(reserved)*          | Type system §T.3.5            |
| `E0453`  | Generic type inference is ambiguous *(reserved)*     | Type system §T.4.2            |
| `E0470`  | Annotation applied outside its `@Target` set *(reserved)* | Annotations §A.13        |
| `E0471`  | Runtime annotation read requires reflection *(reserved)* | Annotations §A.13         |
| `E0472`  | Missing required annotation parameter *(reserved)*   | Annotations §A.13              |
| `E0473`  | Annotation is not `@Repeatable` but appears more than once *(reserved)* | Annotations §A.13 |
| `E0474`  | Wrong type for annotation parameter *(reserved)*     | Annotations §A.13              |

> **Known collision (implementation bug, to be fixed in the compiler):** the
> `juxc_diagnostics::Code` enum currently carries a second variant that also
> prints `E0431` — "generic type inference has no solution" (a bare
> `new X<>()` whose type argument can't be inferred, Type system §T.4.2).
> Two meanings cannot share one number; the inference-failure check should be
> renumbered to **`E0446`** (next free slot) in a follow-up compiler change.
> `E0446` is reserved here for that purpose and must not be allocated to
> anything else.

### Borrow Checker (`E0500–E0599`)

| Code     | Description                                                | Source                         |
|----------|------------------------------------------------------------|--------------------------------|
| `E0500`  | Cannot use value: borrowed                                  | Type system §T.7               |
| `E0501`  | Borrow outlives source                                       | Type system §T.7               |
| `E0502`  | Cannot mutate while borrowed (whole-object rule)            | Inheritance §6.9.1, §6.9.7    |
| `E0503`  | Cannot move while borrowed                                  | Type system §T.7               |
| `E0504`  | Use after move                                              | Lowering §C.6.1                |
| `E0505`  | Cannot hold exclusive borrow across `await`                  | Async §18.1.6                 |
| `E0506`  | `unsafe` operation outside `unsafe` block                    | Layout-ABI §L.5.2              |

### Lowering (`E0600–E0699`)

| Code     | Description                                                | Source                         |
|----------|------------------------------------------------------------|--------------------------------|
| `E0600`  | Field not definitely assigned                               | Semantics §S.4.5               |
| `E0610`  | `drop` block in `jux-core` may not throw                    | Semantics §S.5.3               |
| `E0611`  | Use of moved-from binding                                   | Lowering §C.6.1                |
| `E0612`  | Conditional move requires re-initialization on join          | Lowering §C.6.1                |

### Async / Generators (`E0700–E0799`)

| Code     | Description                                                | Source                         |
|----------|------------------------------------------------------------|--------------------------------|
| `E0700`  | `await` requires async context                              | Async §18.1.2                 |
| `E0701`  | `async` not available in current profile                     | Async §18.1.11                |
| `E0702`  | Class object captured by a `Worker.spawn` closure (Phase-1 objects are `!Send`) | Async §18.2 |
| `E0710`  | `throw` requires `Exception` or subtype                      | Exceptions §X.2.1              |
| `E0720`  | Unreachable `catch` clause                                   | Exceptions §X.3.4              |
| `E0721`  | Multi-catch types must be unrelated                         | Exceptions §X.3.6              |
| `E0730`  | `?` operator's enclosing function has incompatible return    | Exceptions §X.4.1              |
| `E0731`  | `?` requires explicit error-type conversion                 | Exceptions §X.4.3              |

### Const Evaluation (`E0800–E0899`)

| Code     | Description                                                | Source                         |
|----------|------------------------------------------------------------|--------------------------------|
| `E0800`  | `unsafe` operation in const context                         | Type system §T.11              |
| `E0810`  | Operation requires `unsafe` block                            | Layout-ABI §L.5.2              |
| `E0820`  | Inferred closure capture violates lifetime                  | Type system §T.9.4            |
| `E0830`  | `protected` member access through unrelated type            | Type system §T.10.3           |
| `E0840`  | Const evaluation exceeded resource limits                    | Type system §T.11.4           |
| `E0841`  | Non-const operation in const context                         | Type system §T.11.6           |
| `E0842`  | Const evaluation panicked at compile time                    | Type system §T.11.6           |
| `E0850`  | Heap-requiring construct in `jux-core`                       | Pipeline §C.2.6                |

### Backend / Codegen (`E0900–E0999`)

| Code     | Description                                                | Source                         |
|----------|------------------------------------------------------------|--------------------------------|
| `E0900`  | Backend cannot lower construct                              | Pipeline §C.9.4                |
| `E0905`  | Cannot resolve dependency                                   | Build system §B.5.4            |
| `E0908`  | Dynamic linkage unavailable in `core` profile                | Build system §B.14.6           |
| `E0910`  | `init` block escapes `this`                                  | Missing-defs §M.1.3            |
| `E0930`  | Conflicting operator declarations (`<=>` plus an individual ordering operator); also: auto-derive cannot satisfy required interface | Operators §O.2.1 / §O.5.1 |
| `E0931`  | `operator==` defined without `operator hash`                 | Operators §O.2.7               |
| `E0935`  | Call to a `delete`d operator                                 | Operators §O.3.4               |
| `E0940`  | Out-parameter not assigned on every path                    | Missing-defs §M.4.2            |
| `E0941`  | No matching operator definition for required capability     | Operators §O.5.1               |
| `E0950`  | Orphan operator overload                                    | Runtime/ABI §R.3.3             |
| `E0951`  | Duplicate operator overload across modules                  | Runtime/ABI §R.3.3             |
| `E0952`  | Orphan free-function operator definition                    | Runtime/ABI §R.3.6             |
| `E0961`  | Mutable static requires thread-safe wrapper                  | Missing-defs §M.12.3           |
| `E0970`  | Write to a read-only or `init`-only property outside its settable window | Missing-defs §M.7.2 |
| `E0972`  | Property accessor visibility violation (e.g. `private set` written from outside) | Missing-defs §M.7.2 / §M.7.7 |
| `E0980`  | Method reference is ambiguous                               | Missing-defs §M.8.3            |
| `E0991`  | Inner classes not supported                                 | Missing-defs §M.9.2            |
| `E0992`  | Anonymous classes not supported                             | Missing-defs §M.9.2            |
| `E0993`  | Local classes not supported                                 | Missing-defs §M.9.2            |

### Warnings (`W0100–W0999`)

| Code     | Description                                                | Source                         |
|----------|------------------------------------------------------------|--------------------------------|
| `W0001`  | Doc comment in non-attaching position                       | Grammar §A.1.2                 |
| `W0210`  | Module declares no exported symbols                         | Build system §B.3.4            |
| `W0301`  | Equality chained with reference identity                    | Grammar §A.4                   |
| `W0530`  | Cyclic class initialization within a module                 | Semantics §S.4.2               |
| `W0720`  | `return` inside `finally` discards exception                | Exceptions §X.3.5              |
| `W0820`  | `unsafe` block missing `// SAFETY:` justification           | Layout-ABI §L.5.5              |
| `W0960`  | Mutable static in single-threaded profile                    | Missing-defs §M.12.3           |

### Notes (`N0xxx`)

Notes don't have stable codes; they are auxiliary spans attached to a primary error. Some categories of recurring notes get tag-style codes (`N1001` for "candidate defined here", `N1002` for "borrowed at this point") for the catalog page, but they aren't surfaced to users — only the primary error code is what programmers look up.

---

## §D.5 — Stability and Lifecycle

### D.5.1. Code Allocation Discipline

- **A code, once published, cannot be reassigned.** `E0450` means "ambiguous overload" forever.
- **A code can be marked `deprecated`** when a check is removed (because the underlying language rule changed). The code then reports `note: code E0450 is deprecated; this check no longer applies` if a stale tool emits it. The number is not reused.
- **New codes** are allocated at the end of each phase's range. The compiler maintainer keeps a registry; PRs that introduce diagnostics also update this catalog.

### D.5.2. Documentation Per Code

Every code gets a docs page at `https://docs.jux-lang.org/diag/E####` containing:

- Title (one line, matches the diagnostic message).
- "What this means" section.
- Minimal example code that triggers the error.
- "Why this is rejected" section.
- "How to fix it" section with corrected code.

The docs page is auto-generated from a `diag.toml` file in the compiler repository:

```toml
[E0450]
title = "Ambiguous overload"
since = "0.1.0"
phase = "tycheck"
description = """
Multiple overloads of a function or method are applicable to a call,
and the resolution rules in §T.3 cannot pick a unique most-specific candidate.
"""

[[E0450.example]]
title = "Untyped literal"
code = '''
public void log(int code) { ... }
public void log(String message) { ... }
log(value);    // ambiguous if value's type is too general
'''

[[E0450.fix]]
title = "Cast the argument"
code = '''
log(value as int);
'''
```

### D.5.3. The `juxc explain` Subcommand

```
juxc explain E0450
```

Prints the docs page for the named code in the terminal. This works offline (the doc text is bundled with the compiler).

### D.5.4. Lint Configuration

Lint codes (`L####`) are configurable per project:

```toml
# in jux.toml
[lints]
warnings-as-errors = false             # default: false
all = "warn"                            # default level for unlisted lints
unused-import = "deny"
shadowed-name = "warn"
unsafe-without-justification = "deny"
```

Levels: `allow` (silent), `warn` (emit warning), `deny` (emit error). The level can be overridden per-file via attribute:

```jux
@lint(allow = "shadowed-name")
public class Builder { ... }
```

Lints that ship in v0.1 are listed alongside the catalog; new lints are added with each compiler release at minor-version bumps.

### D.5.5. The Stability Promise

| Item                              | Stability                                  |
|-----------------------------------|--------------------------------------------|
| Code numbers (`E####`, `W####`)    | Stable forever — no reassignment           |
| Code message text                  | Stable for one major version; may improve  |
| Diagnostic JSON schema             | Stable for one major version; additive change OK |
| Span byte offsets                  | Stable per-source-version                   |
| Hint code-action edits             | Best-effort; may improve with each release  |
| Docs URLs                          | Stable forever; old codes redirect to current docs |

CI scripts that grep on `error[E####]` continue to work across compiler versions.

### D.5.6. Editor Integration

LSP (per `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.11.4) emits diagnostics in the JSON schema above, with one exception: the `docs_url` is converted to a clickable link in the editor. Some editors also display the code-action as a quick-fix.

---

## Summary

This addendum:

- Specifies the **diagnostic format** (terminal, compact, short, JSON).
- Locks the **JSON schema** for machine consumption.
- Allocates **code ranges by compiler phase**.
- Catalogs **every diagnostic code** referenced across the spec, with its source and meaning.
- Establishes the **stability promise** so codes can be safely memorized and tooled around.

Every prior addendum's `E####` reference now resolves to a concrete catalog entry. The codes form a stable contract between the compiler, its tooling, and its users.

---

*End of diagnostics addendum. The Tier-1 set is complete: build system, runtime/ABI, diagnostics. The Phase-1 implementer now has every concrete plumbing detail spec'd.*
