# JUX-CODEGEN-FIXES.md

**Status:** Active TODO list for the v0.1 codegen polish pass.
**Target:** The Rust source emitter in `juxc`.
**Goal:** Move generated output from "compiles with rough edges" to "clean, idiomatic Rust."

This document collects the codegen issues identified during the first end-to-end run (the `match_payload.jux` example), prioritized by impact and effort. Each fix is small, bounded, and independent of the others.

---

## Context

The first end-to-end pipeline run produced working but rough output. Source like:

```java
public enum Token {
    Number(int),
    Word(String),
    Stop
}

public String label(Token t) {
    return switch (t) {
        case Token.Number(var n) -> $"num=$n";
        case Token.Word(var w)   -> $"word=$w";
        case Token.Stop          -> $"stop";
    };
}
```

compiled to Rust that built and ran correctly, but had several issues: a workaround for string type unification across match arms, inconsistent indentation, an incomplete `Display` implementation, and a questionable `int → isize` mapping. All are fixable in a single day of focused work.

---

## Fix 1 — Always lower string literals to `String`

**Priority:** High. **Effort:** ~2 hours.

**The problem.** A `switch` expression where some arms produce `&str` (`"stop"`) and other arms produce `String` (`$"num=$n"`) makes Rust reject the match. The current workaround is forcing every arm to use `$"..."` even when no interpolation is needed — a footgun for users.

**The fix.** Lower every Jux string literal to a Rust `String`, not `&str`. Every `"hello"` in Jux becomes `"hello".to_string()` in generated Rust. Match arms unify naturally because every arm produces the same type.

**The cost.** Every string literal does one heap allocation. Negligible for normal code; meaningless for hot loops compared to the allocations that already happen for interpolated strings. Java does this and nobody complains.

**Where to change.** The lowering pass that turns Jux string literal AST nodes into Rust expressions. Probably a single function.

**Pseudocode:**

```rust
fn lower_string_literal(lit: &StringLit) -> RustExpr {
    RustExpr::MethodCall {
        receiver: Box::new(RustExpr::StrLit(lit.value.clone())),
        method: "to_string".to_string(),
        args: vec![],
    }
}
```

**Test case to add:**

```java
public String describe(int code) {
    return switch (code) {
        case 0 -> "zero";
        case 1 -> $"one ($code)";
        case _ -> "other";
    };
}
```

Should compile and run with no manual coercion.

**Future option.** If a `&str` is ever needed for FFI or hot paths, expose it as a separate type (`CString` already covers the FFI case). The default stays `String`.

**Knock-on effect — lock this in now.** Once every string literal is owned `String`, the rest of the codegen MUST commit to the same choice: function parameters typed `String` in Jux lower to Rust `String` (owned), **never `&str`**. The string-interop row in `JUX-LANG-V1.md` (~line 2848 — `String, &str → String`) already endorses this; the fix just makes the codegen consistent with the spec. Future `extern` / FFI work needs a separate `CString`-ish path; do NOT smuggle `&str` back in by overloading `String`.

**Acceptance criteria.**

- [ ] Every Jux string literal in the AST lowers to `"...".to_string()` in emitted Rust.
- [ ] No function parameter or return type in emitted Rust uses `&str` for a Jux `String` value.
- [ ] The mixed-arm switch test case below compiles without manual coercion.
- [ ] All existing examples in `examples/` still compile + run with identical observed output.

---

## Fix 2 — Run `rustfmt` on generated output

**Priority:** High. **Effort:** ~30 minutes.

**The problem.** Generated `.rs` files have inconsistent indentation. The code compiles correctly but looks unprofessional. Example:

```rust
fn label(t: Token) -> String {
    match t {
    Token::Number(n) => format!("num={}", n),    // not indented
    Token::Word(w) => format!("word={}", w),
    Token::Stop => format!("stop"),
}
}
```

**The fix.** After writing the `.rs` file and before invoking `rustc`, run `rustfmt` on the file. `rustfmt` is part of the standard Rust toolchain, so any user with `rustc` already has it.

**Where to change.** The driver that orchestrates compilation, right after `std::fs::write(...)` on the generated file.

**Pseudocode:**

```rust
fn emit_rust_file(path: &Path, source: &str) -> Result<()> {
    std::fs::write(path, source)?;

    let status = std::process::Command::new("rustfmt")
        .arg("--edition=2021")
        .arg(path)
        .status();

    if let Err(e) = status {
        eprintln!("warning: rustfmt failed: {} (continuing anyway)", e);
    }

    Ok(())
}
```

**Caveats.**

- `rustfmt` must be available on the user's PATH. Document this as a toolchain requirement.
- For very large files this adds a second or two. Negligible.
- If `rustfmt` fails for any reason, log a warning but don't fail the build — the unformatted code still compiles.

**Alternative.** Build a proper Rust pretty-printer in `juxc`. More work, no external dependency. For v0.1, just use `rustfmt`.

**Acceptance criteria.**

- [ ] `rustfmt --edition=2021` is invoked on every emitted `.rs` file between write and `rustc` invocation.
- [ ] Failure to find `rustfmt` produces a single warning to stderr and does NOT fail the build.
- [ ] A `--no-rustfmt` flag exists on the `juxc` driver for debugging the raw emitter output.
- [ ] CI verifies that `juxc --emit-rust examples/match_payload.jux | rustfmt --check --edition=2021` succeeds (i.e. output is already idempotent under rustfmt; no second pass changes anything).

---

## Fix 3 — Suppress Rust warnings on generated code

**Priority:** High. **Effort:** ~15 minutes.

**The problem.** When a Jux user writes a function that isn't called, `rustc` emits a `warning: function is never used` message. The user sees a Rust warning about Rust code they didn't write, in a Rust file they didn't create. Confusing noise.

**The fix.** Add a module-level `#![allow(...)]` block to every generated `.rs` file that suppresses warnings users can't act on.

**Where to change.** The emitter's file-header generation.

**Add this header to every generated file:**

```rust
// AUTO-GENERATED by juxc. DO NOT EDIT.
// Source: <path to original .jux file>

#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(clippy::all)]
```

**Reasoning per lint.**

- `dead_code` — user may define functions for later use; Rust's "unused" doesn't translate to Jux.
- `unused_variables` — Jux handles unused-variable rules itself; Rust's warning is duplicate signal at best, wrong at worst.
- `non_snake_case` — Jux allows `camelCase`; the user did nothing wrong.
- `non_camel_case_types` — generated names may not match Rust convention.
- `non_upper_case_globals` — constant naming is a Jux concern, not a Rust one.
- `clippy::all` — clippy lints are about Rust idiom; they don't apply to generated code.

**Future improvement.** Selectively translate the warnings that *are* meaningful (e.g., a Rust "unused mutable variable" that corresponds to a real Jux issue) back to Jux source spans using the existing `// JUX:...` source-map comments. For v0.1, blanket-suppress is fine.

**Scope note on `clippy::all`.** This only silences the default-on clippy categories. If a user manually runs `cargo clippy -- -W clippy::pedantic` against the emitted crate, they will still see warnings. That's acceptable — power users who go looking for clippy lints accept the responsibility. The blanket `clippy::all` covers the default `cargo build` and `cargo clippy` flows, which is what matters.

**Acceptance criteria.**

- [ ] Every emitted `.rs` file begins with the AUTO-GENERATED banner + source path + the eight `#![allow(...)]` attributes listed above.
- [ ] Compiling the entire `examples/` directory produces zero `warning:` lines on stderr.
- [ ] The banner's source path is the absolute path of the originating `.jux` file (for click-through in editors that hyperlink terminal output).

---

## Fix 4 — `Display` implementation includes payload values

**Priority:** Medium. **Effort:** ~3 hours.

**The problem.** The auto-generated `Display` impl shows only the variant name, not the payload contents. `Token::Number(42)` prints as `"Number"` rather than `"Number(42)"`. This contradicts the spec (`JUX-LANG-V1.md` §7.7.2):

> `operator string()` — `"VariantName"` for no-payload, `"VariantName(field: ..., ...)"` for payloads.

**Current generated output:**

```rust
impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Token::Number(..) => write!(f, "Number"),
            Token::Word(..) => write!(f, "Word"),
            Token::Stop => write!(f, "Stop"),
        }
    }
}
```

**Should be:**

```rust
impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Token::Number(field0) => write!(f, "Number({})", field0),
            Token::Word(field0) => write!(f, "Word({})", field0),
            Token::Stop => write!(f, "Stop"),
        }
    }
}
```

**The fix.** In the auto-derive logic for `Display`, destructure each variant's payload and emit field placeholders in the format string.

**Where to change.** The pass that generates auto-derives for enum types.

**Pseudocode:**

```rust
fn generate_display_arm(enum_name: &str, variant: &EnumVariant) -> String {
    match &variant.payload {
        VariantPayload::None => format!(
            "{}::{} => write!(f, \"{}\"),",
            enum_name, variant.name, variant.name
        ),
        VariantPayload::Tuple(field_types) => {
            let field_names: Vec<String> = (0..field_types.len())
                .map(|i| format!("field{}", i))
                .collect();
            let placeholders = vec!["{}"; field_types.len()].join(", ");
            format!(
                "{}::{}({}) => write!(f, \"{}({})\", {}),",
                enum_name, variant.name, field_names.join(", "),
                variant.name, placeholders, field_names.join(", ")
            )
        }
        VariantPayload::Record(fields) => {
            let bindings: Vec<String> = fields.iter()
                .map(|f| f.name.clone()).collect();
            let format_parts: Vec<String> = fields.iter()
                .map(|f| format!("{}: {{}}", f.name)).collect();
            format!(
                "{}::{} {{ {} }} => write!(f, \"{}({})\", {}),",
                enum_name, variant.name, bindings.join(", "),
                variant.name, format_parts.join(", "),
                bindings.join(", ")
            )
        }
    }
}
```

**Test case to add:**

```java
public enum Event {
    Click(int x, int y),
    Scroll(int delta),
    Idle;
}

public void main() {
    print($"${Event.Click(10, 20)}");   // expected: "Click(10, 20)"
    print($"${Event.Scroll(-5)}");       // expected: "Scroll(-5)"
    print($"${Event.Idle}");             // expected: "Idle"
}
```

**Watch out — reserved-word field names.** Jux record-variants permit field names that collide with Rust reserved words (`match`, `type`, `move`, `ref`, `mut`, `box`, `async`, `await`, `dyn`, …). The destructure `{}::{} {{ {} }} =>` line in the pseudocode emits these bindings verbatim and the resulting Rust does not compile.

Thread every binding through a `to_rust_ident()` helper that emits the `r#match` raw-identifier form when the name collides with a Rust keyword. The helper almost certainly already exists in `juxc` for ordinary identifier lowering — reuse it; do not duplicate the keyword list. Apply the same helper anywhere else this fix emits a binding (the `Tuple` arm's `field0`, `field1`, … are safe because they're synthesized, but the `Record` arm's field names come from user source).

**Acceptance criteria.**

- [ ] All three payload kinds (none / tuple / record) produce a `Display` arm that prints `Variant`, `Variant(v1, v2, …)`, or `Variant(field1: v1, field2: v2, …)` respectively, matching `JUX-LANG-V1.md` §7.7.2.
- [ ] A test enum with a record-variant field named `match` (or any other Rust keyword) compiles and prints correctly.
- [ ] The `Event` test case above produces exactly the three expected output strings.

---

## Fix 5 — Clean up `format!` with no interpolations

**Priority:** Low. **Effort:** ~1 hour.

**The problem.** A Jux string like `$"stop"` (no interpolations) currently lowers to `format!("stop")`. This works but is mildly wasteful — `format!` builds a `String` with overhead that a plain literal doesn't have. Clippy will warn about it.

**The fix.** In the interpolated-string lowering, detect when the string has no interpolation expressions and emit a simpler form.

**Where to change.** The pass that lowers `InterpolatedString` AST nodes.

**Pseudocode:**

```rust
fn lower_interpolated_string(parts: &[StringPart]) -> RustExpr {
    let has_interpolation = parts.iter().any(|p| matches!(p, StringPart::Expr(_)));

    if !has_interpolation {
        let combined: String = parts.iter()
            .filter_map(|p| match p {
                StringPart::Literal(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        // Lower to "combined".to_string() — same as Fix 1's path
        return RustExpr::MethodCall {
            receiver: Box::new(RustExpr::StrLit(combined)),
            method: "to_string".to_string(),
            args: vec![],
        };
    }

    // Has interpolations: use format!
    let format_str: String = parts.iter().map(|p| match p {
        StringPart::Literal(s) => s.replace('{', "{{").replace('}', "}}"),
        StringPart::Expr(_) => "{}".to_string(),
    }).collect();

    let format_args: Vec<RustExpr> = parts.iter()
        .filter_map(|p| match p {
            StringPart::Expr(e) => Some(lower_expr(e)),
            _ => None,
        })
        .collect();

    RustExpr::MacroCall {
        name: "format".to_string(),
        args: std::iter::once(RustExpr::StrLit(format_str))
            .chain(format_args)
            .collect(),
    }
}
```

**Outcome.** `$"stop"` becomes `"stop".to_string()` in Rust, not `format!("stop")`. Cleaner, slightly faster, no Clippy complaint.

**Note.** This change is automatically consistent with Fix 1 — both routes produce the same `"...".to_string()` form for non-interpolated literals. Implement Fix 1 first; the no-interp branch here MUST reuse Fix 1's `lower_string_literal` helper, not duplicate it.

**Acceptance criteria.**

- [ ] An interpolated string with zero `${…}` segments lowers to the same form as a plain literal (Fix 1 path), not to a `format!(…)` call.
- [ ] An interpolated string with one or more `${…}` segments still lowers to `format!(…)`.
- [ ] `cargo clippy --` on the emitted output of `examples/colors.jux` and similar surfaces no `useless_format` warnings.

---

## Expected output after all fixes

After all five fixes are applied, the `match_payload.jux` example should produce:

```rust
// AUTO-GENERATED by juxc. DO NOT EDIT.
// Source: F:\DEV\juxlang\examples\match_payload.jux

#![allow(dead_code, unused_variables, unused_imports, unused_mut)]
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![allow(clippy::all)]

// JUX:F:\DEV\juxlang\examples\match_payload.jux:5:8
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Token {
    Number(isize),
    Word(String),
    Stop,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Token::Number(field0) => write!(f, "Number({})", field0),
            Token::Word(field0) => write!(f, "Word({})", field0),
            Token::Stop => write!(f, "Stop"),
        }
    }
}

// JUX:F:\DEV\juxlang\examples\match_payload.jux:11:8
fn label(t: Token) -> String {
    // JUX:F:\DEV\juxlang\examples\match_payload.jux:16:12
    match t {
        Token::Number(n) => format!("num={}", n),
        Token::Word(w) => format!("word={}", w),
        Token::Stop => "stop".to_string(),
    }
}

// JUX:F:\DEV\juxlang\examples\match_payload.jux:23:8
fn main() {
    // JUX:F:\DEV\juxlang\examples\match_payload.jux:24:5
    println!("{}", label(Token::Number(42)));
    // JUX:F:\DEV\juxlang\examples\match_payload.jux:25:5
    println!("{}", label(Token::Word("hi".to_string())));
    // JUX:F:\DEV\juxlang\examples\match_payload.jux:26:5
    println!("{}", label(Token::Stop));
}
```

Clean. Indented. Idiomatic Rust. Spec-compliant (note `Number(isize)` — `int` is the platform-sized type per `JUX-LANG-V1.md` §5, NOT `i32`). Source-mapped back to the original `.jux` lines for error messages.

---

## Summary table

| # | Fix | Priority | Effort | Visible Impact |
|---|---|---|---|---|
| 1 | String literals → `String` | High | 2h | Eliminates an entire class of match-arm type errors; locks "no `&str` for `String`" rule |
| 2 | Run `rustfmt` on output | High | 30m | Generated code looks professional |
| 3 | `#![allow(...)]` header | High | 15m | No spurious Rust warnings reach the user |
| 4 | `Display` includes payloads | Medium | 3h | Spec compliance (§7.7.2); correct `print($"$enum")` behavior; handles reserved-word field names via `r#…` |
| 5 | `$"stop"` → `"stop".to_string()` | Low | 1h | Minor polish, removes Clippy noise; reuses Fix 1's helper |

**Total:** roughly one short day (~6h45m) of focused work. None of these are architectural changes — each is a small, bounded patch to the codegen.

*The previous "`int → i32`" item was removed: `JUX-LANG-V1.md` §5 already defines `int` as platform-sized, so the current `int → isize` mapping is spec-correct and needs no change. See the review note in this doc's commit history if context is needed later.*

---

## Suggested order of work

1. **Fix 1** (string literals → `String`) — eliminates the largest user-visible papercut. Do this first; Fix 5 depends on its helper.
2. **Fix 5** (clean up `format!`) — pairs naturally with Fix 1; same code path; literally calls Fix 1's helper.
3. **Fix 3** (`#![allow(...)]` header) — fifteen-minute change that removes user confusion.
4. **Fix 2** (`rustfmt` invocation) — small driver change, no codegen impact.
5. **Fix 4** (Display with payloads) — the longest one, but isolated to the auto-derive pass. Needs the `to_rust_ident()` helper threaded through; verify that helper exists before starting.

After this pass, the codegen output is in good shape for whatever the next milestone needs (more language features, the LSP, the std library, etc.).

---

## Architectural note (not a fix, just a thought)

If `juxc` is currently emitting Rust by concatenating strings, each of these fixes touches the same string-manipulation code. Worth considering an intermediate Rust AST representation — emit a structured tree, then pretty-print it — at some point.

The `syn` and `quote` crates do exactly this and could be used directly. Procedural macros use them constantly to generate Rust code; the patterns are well-established.

Not necessary now. But if codegen complexity grows, the IR approach pays off. Worth noting in the gaps roadmap for a future pass.

*End of codegen fixes addendum.*
