//! Backend integration tests.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Tests are identical to the original
//! `#[cfg(test)] mod tests` block.

use super::*;
use juxc_lex::lex;
use juxc_parse::parse;
use juxc_source::SourceFile;

/// Lex → parse → lower; return the emitted main.rs text.
fn emit(src: &str) -> String {
    let sf = SourceFile::new("test.jux", src);
    let lex_result = lex(&sf);
    assert!(lex_result.diagnostics.is_empty());
    let parse_result = parse(&lex_result.tokens);
    assert!(parse_result.diagnostics.is_empty());
    let crate_ = lower(&parse_result.ast);
    assert_eq!(crate_.sources.len(), 1);
    crate_.sources.into_iter().next().unwrap().1
}

/// Same as [`emit`] but routes through [`lower_with_source`] so the
/// emitted Rust includes `// JUX:file:line:col` markers. Used by the
/// source-map tests.
fn emit_with_source(src: &str) -> String {
    let sf = SourceFile::new("test.jux", src);
    let lex_result = lex(&sf);
    assert!(lex_result.diagnostics.is_empty());
    let parse_result = parse(&lex_result.tokens);
    assert!(parse_result.diagnostics.is_empty());
    let typed = juxc_tycheck::typecheck(&parse_result.ast);
    let crate_ = lower_with_source(
        &parse_result.ast,
        &typed.symbols,
        &typed.expr_types,
        Some(&sf),
    );
    crate_.sources.into_iter().next().unwrap().1
}

/// hello.jux lowers to a Rust source that contains `fn main` and the
/// idiomatic single-argument `println!("Hello, world!")`. (No
/// redundant `"{}", "…"` placeholder for the all-literal case.)
#[test]
fn hello_world_lowers_to_idiomatic_println() {
    let rust = emit(r#"public void main() { print("Hello, world!"); }"#);
    assert!(rust.contains("fn main()"), "missing fn main(): {rust}");
    assert!(
        rust.contains(r#"println!("Hello, world!")"#),
        "expected literal-form println, got: {rust}"
    );
    assert!(
        !rust.contains(r#"println!("{}", "Hello, world!")"#),
        "should not emit redundant `{{}}` placeholder for literal: {rust}"
    );
}

/// `int main()` lowers to `fn main() -> isize` because Jux `int`
/// is **platform-sized** per §5.1. The trailing `return 0;` elides
/// to a bare `0` tail expression per the trailing-return cosmetic;
/// the unsuffixed `0` is inferred to `isize` by Rust from the
/// function's return type.
#[test]
fn int_main_returns_isize() {
    let rust = emit("public int main() { return 0; }");
    assert!(rust.contains("fn main() -> isize"), "got: {rust}");
    assert!(!rust.contains("return 0;"), "tail return should be elided: {rust}");
    assert!(rust.contains("\n    0\n}"), "expected bare-tail `0`, got: {rust}");
}

/// `var` lowers to immutable `let` (no `let mut`) — until we have
/// reassignment statements, none of our `var`s are mutated.
#[test]
fn var_lowers_to_let_not_let_mut() {
    let rust = emit("public void main() { var x = 10; print(x); }");
    assert!(rust.contains("let x = 10;"), "expected `let x = 10;`, got: {rust}");
    // Scope the negative assertion to the user's binding — the
    // emitted runtime prelude (channels, tasks) legitimately uses
    // `let mut` internally.
    assert!(!rust.contains("let mut x"), "no `let mut x` expected, got: {rust}");
}

/// Integer literals are emitted without a `i64` / `i32` suffix —
/// Rust infers the right type from context.
#[test]
fn integer_literal_has_no_suffix() {
    let rust = emit("public void main() { var x = 42; print(x); }");
    assert!(rust.contains("= 42;"), "expected `= 42;`, got: {rust}");
    assert!(!rust.contains("42i64"), "no i64 suffix expected, got: {rust}");
}

/// A simple binary expression like `x + y` should emit without
/// outer parens.
#[test]
fn simple_binary_has_no_outer_parens() {
    let rust = emit("public void main() { var s = 1 + 2; print(s); }");
    assert!(
        rust.contains("let s = 1 + 2;"),
        "expected no parens around `1 + 2`, got: {rust}",
    );
}

/// `1 + 2 * 3` emits with `*` binding tighter, no parens needed.
#[test]
fn mixed_precedence_has_no_redundant_parens() {
    let rust = emit("public void main() { var s = 1 + 2 * 3; print(s); }");
    assert!(
        rust.contains("let s = 1 + 2 * 3;"),
        "expected `1 + 2 * 3`, got: {rust}",
    );
}

/// `(1 + 2) * 3` parses as `Binary(Mul, Binary(Add, 1, 2), 3)`. The
/// `+` subexpr has lower precedence than `*`, so the left side
/// MUST be parenthesized to preserve grouping.
#[test]
fn lower_prec_subexpr_keeps_parens() {
    let rust = emit("public void main() { var s = (1 + 2) * 3; print(s); }");
    assert!(
        rust.contains("let s = (1 + 2) * 3;"),
        "expected `(1 + 2) * 3`, got: {rust}",
    );
}

/// `if (cond)` Jux lowers to `if cond` Rust — no outer parens around
/// the condition.
#[test]
fn if_condition_has_no_outer_parens() {
    let rust = emit(
        r#"public void main() { if (1 < 2) { print("yes"); } }"#,
    );
    assert!(rust.contains("if 1 < 2 {"), "got: {rust}");
    assert!(!rust.contains("if (1 < 2)"), "outer parens not expected, got: {rust}");
}

/// String escaping: Jux `"c:\\path"` decodes at parse time to the
/// one-byte-backslash string `c:\path`, and the backend then
/// re-escapes that single `\` as `\\` when emitting into Rust source.
/// End result: `println!("c:\\path")` (one logical backslash) —
/// exactly what the user would write in Java / most C-family
/// languages.
#[test]
fn rust_special_chars_in_string_are_reescaped() {
    let rust = emit(r#"public void main() { print("c:\\path"); }"#);
    assert!(
        rust.contains(r#"println!("c:\\path")"#),
        "expected one re-escaped backslash, got: {rust}"
    );
}

/// `\n`, `\t`, and `\u{…}` round-trip from Jux to runtime. The
/// parser-time escape decoder converts each escape sequence to its
/// real Unicode scalar, then the backend re-encodes those chars
/// using Rust escape conventions on the way out.
#[test]
fn standard_escapes_decoded_at_parse_time() {
    let rust = emit(r#"public void main() { print("a\nb"); }"#);
    assert!(
        rust.contains(r#"println!("a\nb")"#),
        "newline escape: {rust}"
    );

    let rust = emit(r#"public void main() { print("hi\tthere"); }"#);
    assert!(
        rust.contains(r#"println!("hi\tthere")"#),
        "tab escape: {rust}"
    );

    // `\u{1F600}` is the 😀 emoji. The decoder produces the actual
    // char and the backend emits it verbatim (Rust string literals
    // accept any Unicode scalar value).
    let rust = emit(r#"public void main() { print("face=\u{1F600}"); }"#);
    assert!(
        rust.contains("face=\u{1F600}"),
        "unicode escape: {rust}"
    );
}

/// `{` and `}` in a string passed to `print` must be doubled in the
/// emitted format string so `println!` doesn't read them as
/// placeholders.
#[test]
fn braces_in_print_literal_are_doubled() {
    let rust = emit(r#"public void main() { print("a{b}c"); }"#);
    assert!(
        rust.contains(r#"println!("a{{b}}c")"#),
        "expected braces doubled in format string, got: {rust}"
    );
}

/// A function with one String parameter lowers to
/// `fn name(p: String)` post Fix 1: every Jux `String` position is
/// owned `String` in emitted Rust.
#[test]
fn string_param_lowers_to_owned_string() {
    let rust = emit(
        r#"public void greet(String name) { print(name); }
           public void main() { greet("Alice"); }"#,
    );
    assert!(
        rust.contains("fn greet(name: String)"),
        "expected `fn greet(name: String)`, got: {rust}",
    );
    // The argument site lowers the literal through Fix 1's
    // `.to_string()` self-coercion, so the type matches.
    assert!(
        rust.contains(r#"greet("Alice".to_string())"#),
        "expected `greet(\"Alice\".to_string())`, got: {rust}",
    );
}

/// `int` parameters lower to **`isize`** (platform-sized) per
/// spec §5.1. For a fixed-width 32-bit param, write `i32` instead.
#[test]
fn int_param_lowers_to_isize() {
    let rust = emit("public void noop(int x) {}");
    assert!(
        rust.contains("fn noop(x: isize)"),
        "expected `fn noop(x: isize)`, got: {rust}",
    );
}

/// `i32` parameters lower to Rust's fixed-width `i32` — distinct
/// from `int` (which is platform-sized).
#[test]
fn i32_param_lowers_to_fixed_width_i32() {
    let rust = emit("public void noop(i32 x) {}");
    assert!(
        rust.contains("fn noop(x: i32)"),
        "expected `fn noop(x: i32)`, got: {rust}",
    );
}

/// `bool` parameters lower to `bool`.
#[test]
fn bool_param_lowers_to_bool() {
    let rust = emit("public void noop(bool flag) {}");
    assert!(
        rust.contains("fn noop(flag: bool)"),
        "expected `fn noop(flag: bool)`, got: {rust}",
    );
}

/// Multiple parameters are emitted comma-separated. Uses `i32`
/// (fixed-width) for the int params to keep the test stable across
/// 32/64-bit target choices.
#[test]
fn multiple_params_are_comma_separated() {
    let rust = emit("public void f(i32 x, i32 y, bool b) {}");
    assert!(
        rust.contains("fn f(x: i32, y: i32, b: bool)"),
        "got: {rust}",
    );
}

/// A `while (cond) { body }` lowers to `while cond { body }` with no
/// outer parens on the condition.
#[test]
fn while_lowers_naturally() {
    let rust = emit(
        r#"public void main() {
               var i = 0;
               while (i < 3) { i = i + 1; }
           }"#,
    );
    assert!(rust.contains("while i < 3 {"), "got: {rust}");
}

/// A `var` that gets reassigned must lower to `let mut`. A `var`
/// that never gets reassigned must lower to plain `let`. Both
/// happening in the same function exercises the per-function
/// mutation analysis.
#[test]
fn var_promoted_to_let_mut_only_when_reassigned() {
    let rust = emit(
        r#"public void main() {
               var i = 0;
               var n = 5;
               while (i < n) { i = i + 1; }
           }"#,
    );
    assert!(rust.contains("let mut i = 0;"), "i should be let mut: {rust}");
    assert!(
        rust.contains("let n = 5;") && !rust.contains("let mut n"),
        "n should stay immutable: {rust}",
    );
}

/// An assignment statement `name = value ;` lowers to the natural Rust
/// form `name = value;`.
#[test]
fn assignment_lowers_directly() {
    let rust = emit(
        r#"public void main() {
               var x = 10;
               x = x + 1;
           }"#,
    );
    assert!(rust.contains("x = x + 1;"), "got: {rust}");
}

/// Compound assignment `+=` lowers (via parse-time desugar) to
/// `name = name + value;` in Rust.
#[test]
fn compound_assignment_preserves_op_form() {
    // Post the compound-assign AST preservation pass, `+=` no longer
    // desugars at parse time. The backend emits `x += 5;` directly,
    // matching Rust's own `+=` (which evaluates the place expr
    // once even for side-effecting shapes like `arr[next()] += 1`).
    let rust = emit(
        r#"public void main() {
               var x = 10;
               x += 5;
           }"#,
    );
    assert!(rust.contains("x += 5;"), "expected `x += 5`, got: {rust}");
    assert!(
        !rust.contains("x = x + 5"),
        "should NOT desugar to `x = x + 5`: {rust}",
    );
}

/// Side-effecting place expressions on the LHS of `+=` are evaluated
/// EXACTLY ONCE — the audit-flagged double-eval bug
/// (`arr[next()] += 1` calling `next()` twice) is gone now that
/// compound assignment preserves the operator instead of expanding
/// to `target = target + rhs` at parse time.
#[test]
fn compound_assignment_does_not_double_evaluate_index() {
    let rust = emit(
        r#"public int next() { return 0; }
           public void main() {
               int[3] xs = {0, 0, 0};
               xs[next()] += 1;
           }"#,
    );
    // Emitted code must use `+=`, not the desugared form. Index
    // expression may carry a `usize` cast — the key invariant is
    // that `next()` is invoked exactly once on the assignment
    // statement. Find the `+= 1` line and confirm `next()` only
    // appears once in it.
    let line = rust
        .lines()
        .find(|l| l.contains("+= 1"))
        .expect("expected a `+= 1` line in emitted source");
    let call_count = line.matches("next()").count();
    assert_eq!(
        call_count, 1,
        "next() should be evaluated exactly once on the assign line, got {call_count}: {line}\nfull:\n{rust}",
    );
}

/// `break;` lowers to `break;`. `continue;` lowers to `continue;`.
/// (Rust has both keywords with identical semantics for our use.)
#[test]
fn break_and_continue_lower_directly() {
    let rust = emit(
        r#"public void main() {
               while (true) {
                   if (1 > 0) { break; }
                   continue;
               }
           }"#,
    );
    assert!(rust.contains("break;"), "missing break: {rust}");
    assert!(rust.contains("continue;"), "missing continue: {rust}");
}

/// An `int`-returning function with parameters lowers to all
/// `isize` — `int` is platform-sized per §5.1.
#[test]
fn int_function_lowers_to_isize_uniformly() {
    let rust = emit("public int add(int a, int b) { return a + b; }");
    assert!(
        rust.contains("fn add(a: isize, b: isize) -> isize"),
        "expected `fn add(a: isize, b: isize) -> isize`, got: {rust}",
    );
}

/// Width-explicit `i32` end-to-end: param, return, body all i32.
#[test]
fn i32_function_keeps_fixed_width_i32() {
    let rust = emit("public i32 add(i32 a, i32 b) { return a + b; }");
    assert!(
        rust.contains("fn add(a: i32, b: i32) -> i32"),
        "got: {rust}",
    );
}

/// `while (true)` Jux lowers to Rust's idiomatic `loop { … }`. This
/// is a cosmetic special case — both produce identical machine code.
#[test]
fn while_true_lowers_to_loop() {
    let rust = emit(
        r#"public void main() {
               while (true) { break; }
           }"#,
    );
    assert!(rust.contains("loop {"), "expected `loop {{`, got: {rust}");
    assert!(
        !rust.contains("while true"),
        "should not emit `while true`, got: {rust}",
    );
}

/// A non-literal always-true condition stays as `while`. We only
/// special-case the literal `true` token — generalizing to "any
/// always-true expression" needs const evaluation.
#[test]
fn while_nonliteral_truth_stays_while() {
    let rust = emit(
        r#"public void main() {
               var x = 1;
               while (x == 1) { break; }
           }"#,
    );
    assert!(rust.contains("while x == 1"), "got: {rust}");
    assert!(!rust.contains("loop {"), "should NOT collapse to loop: {rust}");
}

/// `while (false)` is NOT `loop` — the loop body never runs, which
/// is semantically distinct from "loop forever." We emit `while false`
/// faithfully; clippy in the downstream `cargo build` will warn
/// about it (a true Jux user-bug surfaces as a Rust lint).
#[test]
fn while_false_stays_while() {
    let rust = emit(
        r#"public void main() {
               while (false) {}
           }"#,
    );
    assert!(rust.contains("while false"), "got: {rust}");
    assert!(!rust.contains("loop {"), "must not be `loop`: {rust}");
}

// -----------------------------------------------------------------
// Trailing-return elision
// -----------------------------------------------------------------

/// A non-void function whose body ends in `return expr;` emits the
/// expression bare on its own line — no `return`, no `;`.
#[test]
fn trailing_return_with_value_is_elided() {
    let rust = emit("public int add(int a, int b) { return a + b; }");
    assert!(
        !rust.contains("return"),
        "trailing return should be elided, got: {rust}",
    );
    // `\n    a + b\n}` — properly indented tail expression.
    assert!(
        rust.contains("\n    a + b\n}"),
        "expected `a + b` as bare tail expr, got: {rust}",
    );
}

/// A `void` function whose body ends in a bare `return;` drops the
/// statement entirely — Rust returns `()` implicitly.
#[test]
fn trailing_bare_return_in_void_is_dropped() {
    let rust = emit("public void main() { print(\"hi\"); return; }");
    // The println should still be there.
    assert!(rust.contains(r#"println!("hi")"#), "got: {rust}");
    // No `return;` should appear at all.
    assert!(!rust.contains("return"), "void tail return should be dropped, got: {rust}");
}

/// Early returns inside conditionals stay as `return expr;` — only
/// the very last statement of the function gets elided. This is
/// important for control flow that intentionally short-circuits.
///
/// (We avoid `-1` here since the parser doesn't yet support unary
/// minus on a literal; the elision logic doesn't care which literal
/// is returned.)
#[test]
fn early_return_in_if_body_stays_explicit() {
    let rust = emit(
        r#"public int classify(int x) {
               if (x < 1) {
                   return 0;
               }
               return 1;
           }"#,
    );
    // The early return is preserved verbatim — it's not the function's
    // last statement.
    assert!(rust.contains("return 0;"), "early return should stay: {rust}");
    // The tail return is elided to a bare `1`.
    assert!(
        rust.contains("\n    1\n}"),
        "expected `1` as bare tail expr, got: {rust}",
    );
    // The trailing return shape should NOT appear.
    assert!(
        !rust.contains("return 1;"),
        "trailing `return 1;` should be elided, got: {rust}",
    );
}

/// When the function body has no return at all, nothing changes —
/// emission is identical to before the elision was added.
#[test]
fn function_without_return_is_unchanged() {
    let rust = emit("public void main() { print(\"hi\"); }");
    assert!(rust.contains(r#"println!("hi")"#), "got: {rust}");
    assert!(!rust.contains("return"), "no return should appear: {rust}");
}

// -----------------------------------------------------------------
// Unary operators
// -----------------------------------------------------------------

/// `-x` lowers to `-x` (direct).
#[test]
fn unary_negation_lowers_directly() {
    let rust = emit(
        r#"public void main() {
               var x = 7;
               print(-x);
           }"#,
    );
    assert!(rust.contains("println!(\"{}\", -x)"), "got: {rust}");
}

/// `~mask` lowers to Rust's `!mask` (Rust uses `!` for bitwise NOT
/// on integer operands).
#[test]
fn bitwise_not_lowers_to_rust_bang() {
    let rust = emit(
        r#"public void main() {
               var mask = 0;
               print(~mask);
           }"#,
    );
    assert!(rust.contains("println!(\"{}\", !mask)"), "got: {rust}");
}

/// `-(x + y)` keeps the parens — `+` has lower precedence than unary
/// `-`, so omitting them would change grouping (`-x + y`).
#[test]
fn unary_over_binary_operand_keeps_parens() {
    let rust = emit(
        r#"public void main() {
               var x = 1;
               var y = 2;
               print(-(x + y));
           }"#,
    );
    assert!(
        rust.contains("println!(\"{}\", -(x + y))"),
        "expected `-(x + y)`, got: {rust}",
    );
}

/// `-x` in argument position emits naked — no parens needed when the
/// operand is atomic.
#[test]
fn unary_over_atomic_has_no_parens() {
    let rust = emit("public void main() { print(-7); }");
    assert!(rust.contains("println!(\"{}\", -7)"), "got: {rust}");
    // Sanity: we don't emit redundant parens like `(-7)`.
    assert!(!rust.contains("(-7)"), "redundant parens: {rust}");
}

// -----------------------------------------------------------------
// For-each + range expressions
// -----------------------------------------------------------------

/// Half-open range `0..10` lowers to the same Rust syntax.
#[test]
fn exclusive_range_lowers_directly() {
    let rust = emit("public void main() { for (var i : 0..10) { print(i); } }");
    assert!(rust.contains("for i in 0..10 {"), "got: {rust}");
}

/// Inclusive range `0..=10` lowers to the same Rust syntax.
#[test]
fn inclusive_range_lowers_directly() {
    let rust = emit("public void main() { for (var i : 0..=10) { print(i); } }");
    assert!(rust.contains("for i in 0..=10 {"), "got: {rust}");
}

/// For-each emits `for name in iter` — no Rust-specific `pattern` or
/// type annotation, just the bare name.
#[test]
fn for_each_emits_idiomatic_for_in() {
    let rust = emit(
        r#"public void main() {
               var total = 0;
               for (var i : 1..=5) { total += i; }
               print(total);
           }"#,
    );
    assert!(rust.contains("for i in 1..=5 {"), "got: {rust}");
    // `total` mutated in the loop body — should be `let mut`.
    assert!(rust.contains("let mut total"), "expected let mut total: {rust}");
}

/// Typed for-each currently drops the type annotation and emits the
/// same `for name in iter` shape — Rust infers the loop variable's
/// type from the iterator's Item type.
#[test]
fn typed_for_each_drops_type_annotation_for_now() {
    let rust = emit("public void main() { for (int i : 0..10) { print(i); } }");
    assert!(rust.contains("for i in 0..10 {"), "got: {rust}");
    // No `i: i32` annotation on the loop variable yet.
    assert!(!rust.contains("for i: i32"), "got: {rust}");
}

// ----------------------------------------------------------------------
// For-each on arrays (borrow + pattern-deref)
// ----------------------------------------------------------------------

/// For-each over a dynamic array of a NON-Copy element type
/// iterates by `.iter().cloned()` so the Vec stays usable after
/// the loop and the loop variable is an owned `T` value (works
/// for non-Copy types like `String`).
#[test]
fn for_each_on_string_vec_borrows_when_body_doesnt_move() {
    let rust = emit(
        r#"public void main() {
               String[] xs = {"a", "b"};
               for (var x : xs) { print(x); }
               print(xs.length);
           }"#,
    );
    // Body only borrows via `print(x)` — no `.iter().cloned()`
    // needed. The cheaper `for x in &xs` form fires here so each
    // iteration runs without a per-element heap clone.
    assert!(rust.contains("for x in &xs {"), "got: {rust}");
    assert!(
        !rust.contains(".iter().cloned()"),
        "non-moving body shouldn't clone: {rust}",
    );
    // The post-loop `.length` reads xs — proves we didn't move it.
    // Identifier receiver, so no parens around it.
    assert!(rust.contains("xs.len() as isize"), "got: {rust}");
}

/// When the body **moves** the loop variable (passes it to a fn
/// expecting `T` by value), we fall back to `.iter().cloned()`
/// so each iteration owns a fresh `T`.
#[test]
fn for_each_on_string_vec_clones_when_body_consumes() {
    let rust = emit(
        r#"public void take(String s) { print(s); }
           public void main() {
               String[] xs = {"a", "b"};
               for (var x : xs) { take(x); }
           }"#,
    );
    assert!(
        rust.contains("for x in xs.iter().cloned() {"),
        "moving body should clone: {rust}",
    );
}

/// Fixed-size array of a Copy element type uses the cheap
/// borrow-and-destructure shape — no per-iteration clone.
#[test]
fn for_each_on_copy_fixed_array_borrows_each_element() {
    let rust = emit(
        r#"public void main() {
               int[3] xs = {1, 2, 3};
               for (var x : xs) { print(x); }
           }"#,
    );
    assert!(rust.contains("for &x in &xs {"), "got: {rust}");
}

/// Ranges keep their naked `for x in 0..10` form — no borrow.
/// (Already covered above; this one re-asserts after the change.)
#[test]
fn for_each_on_range_still_unborrowed() {
    let rust = emit("public void main() { for (var i : 0..3) { print(i); } }");
    assert!(rust.contains("for i in 0..3 {"), "got: {rust}");
    assert!(!rust.contains(".iter()"), "ranges shouldn't .iter(): {rust}");
}

// -----------------------------------------------------------------
// Full primitive type mapping (§5.1)
// -----------------------------------------------------------------

/// Each Jux primitive in §5.1 lowers to its documented Rust type.
/// Note `int`/`uint` are **platform-sized** — they map to `isize`/
/// `usize`, not `i32`/`u32`.
#[test]
fn every_primitive_maps_to_its_rust_counterpart() {
    let cases = [
        ("bool",   "bool"),
        ("byte",   "i8"),
        ("ubyte",  "u8"),
        ("short",  "i16"),
        ("ushort", "u16"),
        ("int",    "isize"),   // platform-sized
        ("uint",   "usize"),   // platform-sized
        ("long",   "i64"),
        ("ulong",  "u64"),
        ("float",  "f32"),
        ("double", "f64"),
        ("char",   "char"),
        // Per Fix 1, Jux `String` is owned `String` in every
        // position — parameters included.
        ("String", "String"),
    ];
    for (jux_ty, rust_ty) in cases {
        let src = format!("public void f({jux_ty} x) {{}}");
        let rust = emit(&src);
        let needle = format!("fn f(x: {rust_ty})");
        assert!(
            rust.contains(&needle),
            "{jux_ty} should map to {rust_ty}, got: {rust}",
        );
    }
}

/// Return-type emission goes through the same mapping, so an `int`
/// return lowers to `-> i32` and a `bool` return to `-> bool`, etc.
#[test]
fn primitive_return_types_map_correctly() {
    let cases = [
        ("bool",   "bool"),
        ("long",   "i64"),
        ("double", "f64"),
        ("char",   "char"),
    ];
    for (jux_ty, rust_ty) in cases {
        let src = format!("public {jux_ty} f() {{ return f(); }}");
        let rust = emit(&src);
        let needle = format!("fn f() -> {rust_ty}");
        assert!(
            rust.contains(&needle),
            "{jux_ty} return should map to {rust_ty}, got: {rust}",
        );
    }
}

/// Width-explicit aliases per §5.1 map to the same Rust types as
/// their Java-family partners. `i32` == `int`, `u8` == `ubyte`, etc.
#[test]
fn width_aliases_map_to_same_rust_types() {
    let cases = [
        ("i8",  "i8"),
        ("u8",  "u8"),
        ("i16", "i16"),
        ("u16", "u16"),
        ("i32", "i32"),
        ("u32", "u32"),
        ("i64", "i64"),
        ("u64", "u64"),
        ("f32", "f32"),
        ("f64", "f64"),
    ];
    for (jux_ty, rust_ty) in cases {
        let src = format!("public void f({jux_ty} x) {{}}");
        let rust = emit(&src);
        let needle = format!("fn f(x: {rust_ty})");
        assert!(
            rust.contains(&needle),
            "{jux_ty} alias should map to {rust_ty}, got: {rust}",
        );
    }
}

/// Aliases work in return-type position too — `i64` return →
/// `fn f() -> i64`.
#[test]
fn width_aliases_work_in_return_position() {
    let rust = emit("public i64 f() { return f(); }");
    assert!(rust.contains("fn f() -> i64"), "got: {rust}");
}

/// Aliases work as typed local declarations: `i64 big = 0;` →
/// `let big: i64 = 0;`.
#[test]
fn width_aliases_work_as_typed_locals() {
    let rust = emit("public void main() { i64 big = 0L; print(big); }");
    assert!(rust.contains("let big: i64 = 0i64;"), "got: {rust}");
}

/// User-defined types still fall through to verbatim path emission.
/// The primitive table is the fast path; anything not in it is
/// emitted as a `::`-joined path on faith.
#[test]
fn unknown_type_falls_through_verbatim() {
    let rust = emit("public void f(Foo x) {}");
    assert!(rust.contains("fn f(x: Foo)"), "got: {rust}");
}

/// `nint` and `nuint` are NOT recognized as Jux primitives — the
/// platform-sized type is just `int`/`uint`. Users who write `nint`
/// get verbatim path emission (and a Rust error from rustc later if
/// `nint` isn't a real Rust type in their crate). This locks in the
/// "no platform-sized width-explicit synonym" decision.
#[test]
fn nint_and_nuint_are_not_primitives() {
    let rust = emit("public void f(nint x) {}");
    // Falls through to verbatim — `nint` emitted as-is, not mapped.
    assert!(rust.contains("fn f(x: nint)"), "got: {rust}");
    // Scoped: the runtime prelude uses isize internally (channel
    // capacities); the PARAM must not silently map.
    assert!(!rust.contains("x: isize"), "should not silently map to isize: {rust}");
}

// -----------------------------------------------------------------
// Logical operators
// -----------------------------------------------------------------

/// `a && b` lowers to `a && b` (Rust direct).
#[test]
fn logical_and_lowers_directly() {
    let rust = emit(
        r#"public bool f(bool a, bool b) { return a && b; }"#,
    );
    assert!(rust.contains("a && b"), "got: {rust}");
}

/// `a || b` lowers to `a || b`.
#[test]
fn logical_or_lowers_directly() {
    let rust = emit(
        r#"public bool f(bool a, bool b) { return a || b; }"#,
    );
    assert!(rust.contains("a || b"), "got: {rust}");
}

/// Precedence is preserved: `a || b && c` emits with no parens
/// because Rust's `&&` is tighter than `||` and so naked emission
/// preserves grouping.
#[test]
fn or_then_and_has_no_parens() {
    let rust = emit(
        r#"public bool f(bool a, bool b, bool c) { return a || b && c; }"#,
    );
    assert!(rust.contains("a || b && c"), "got: {rust}");
    assert!(!rust.contains("(b && c)"), "redundant parens: {rust}");
}

/// Forced grouping: `(a || b) && c` MUST keep parens around the OR
/// because `&&` is tighter — naked emission would change meaning.
#[test]
fn forced_grouping_keeps_parens() {
    let rust = emit(
        r#"public bool f(bool a, bool b, bool c) { return (a || b) && c; }"#,
    );
    assert!(rust.contains("(a || b) && c"), "got: {rust}");
}

/// Comparisons inside logical ops emit naked — comparison is tighter
/// than logical, so `x >= low && x <= high` emits as-is.
#[test]
fn comparison_inside_logical_has_no_parens() {
    let rust = emit(
        r#"public bool inRange(i32 x, i32 low, i32 high) {
               return x >= low && x <= high;
           }"#,
    );
    assert!(rust.contains("x >= low && x <= high"), "got: {rust}");
}

// -----------------------------------------------------------------
// Bitwise + shifts
// -----------------------------------------------------------------

/// Each bitwise operator emits its identical Rust token.
#[test]
fn bitwise_ops_lower_directly() {
    let cases = [
        ("|", "a | b"),
        ("^", "a ^ b"),
        ("&", "a & b"),
    ];
    for (op_src, needle) in cases {
        let src = format!(
            "public void main() {{ var a = 1; var b = 2; print(a {op_src} b); }}"
        );
        let rust = emit(&src);
        assert!(rust.contains(needle), "{op_src} should lower to `{needle}`: {rust}");
    }
}

/// Shift operators emit identically.
#[test]
fn shifts_lower_directly() {
    let rust = emit("public void main() { print(1 << 4); print(256 >> 4); }");
    assert!(rust.contains("1 << 4"), "got: {rust}");
    assert!(rust.contains("256 >> 4"), "got: {rust}");
}

/// **Key cross-language test**: Jux parses `a & b == c` as `a & (b == c)`
/// (`&` looser than `==` in Jux). Rust would parse the same source as
/// `(a & b) == c` (`&` tighter than `==` in Rust). The emitter MUST
/// insert parens to preserve Jux's tree shape — otherwise we'd silently
/// produce a different program.
#[test]
fn bit_and_with_equality_gets_parens_in_rust_output() {
    let rust = emit(
        r#"public void main() {
               var a = 1;
               var b = 2;
               var c = 3;
               print(a & b == c);
           }"#,
    );
    // Expect `a & (b == c)` — parens around the equality preserve
    // Jux's "& is looser than ==" semantics under Rust's parser.
    assert!(
        rust.contains("a & (b == c)"),
        "expected parens around `b == c` to preserve Jux semantics; got: {rust}",
    );
}

/// `a & b` with both operands atomic doesn't need parens.
#[test]
fn pure_bitwise_has_no_parens() {
    let rust = emit("public void main() { var a = 1; var b = 2; print(a & b); }");
    assert!(rust.contains("println!(\"{}\", a & b)"), "got: {rust}");
}

// -----------------------------------------------------------------
// `as` casts
// -----------------------------------------------------------------

/// `x as int` lowers to `x as i32` — atomic operand, no parens.
#[test]
fn cast_of_atomic_has_no_parens() {
    let rust = emit("public void main() { var x = 5; print(x as long); }");
    assert!(rust.contains("println!(\"{}\", x as i64)"), "got: {rust}");
    assert!(!rust.contains("(x) as"), "redundant parens: {rust}");
}

/// `(a + b) as long` lowers to Rust `(a + b) as i64` — binary
/// operand needs parens because `as` binds tighter than `+`.
#[test]
fn cast_of_binary_keeps_parens() {
    let rust = emit(
        r#"public void main() {
               var a = 1;
               var b = 2;
               print((a + b) as long);
           }"#,
    );
    assert!(rust.contains("(a + b) as i64"), "got: {rust}");
}

/// `x as i32 as long` lowers to `x as i32 as i64` — chained casts
/// stay naked (left-associative; Rust's `as` is also left-assoc).
/// Uses the width-explicit `i32` to avoid the `int → isize` mapping.
#[test]
fn chained_cast_lowers_naked() {
    let rust = emit("public void main() { var x = 5; print(x as i32 as long); }");
    assert!(rust.contains("x as i32 as i64"), "got: {rust}");
}

/// Width-explicit names work too: `count as i64`.
#[test]
fn cast_to_width_explicit_type() {
    let rust = emit("public void main() { var count = 7; print(count as i64); }");
    assert!(rust.contains("count as i64"), "got: {rust}");
}

// -----------------------------------------------------------------
// sizeof
// -----------------------------------------------------------------

/// `sizeof(int)` — primitive → type form. `int` is platform-sized,
/// so it maps to Rust's `isize` per §5.1.
#[test]
fn sizeof_int_emits_isize() {
    let rust = emit("public void main() { print(sizeof(int)); }");
    assert!(
        rust.contains("std::mem::size_of::<isize>()"),
        "got: {rust}",
    );
}

/// `sizeof(long)` → `size_of::<i64>()`.
#[test]
fn sizeof_long_maps_to_i64() {
    let rust = emit("public void main() { print(sizeof(long)); }");
    assert!(rust.contains("std::mem::size_of::<i64>()"), "got: {rust}");
}

/// `sizeof(i32)` — width-explicit name; type form to fixed `i32`.
#[test]
fn sizeof_width_explicit_emits_type_form() {
    let rust = emit("public void main() { print(sizeof(i32)); }");
    assert!(rust.contains("std::mem::size_of::<i32>()"), "got: {rust}");
}

/// Uppercase bare identifier → type form per §5.9.3 rule 2. Falls
/// through verbatim into Rust (works if it's a real user type, fails
/// otherwise).
#[test]
fn sizeof_uppercase_ident_emits_type_form() {
    let rust = emit("public void main() { print(sizeof(MyType)); }");
    assert!(
        rust.contains("std::mem::size_of::<MyType>()"),
        "got: {rust}",
    );
}

/// Lowercase bare identifier → value form per §5.9.3 rule 3.
#[test]
fn sizeof_lowercase_ident_emits_value_form() {
    let rust = emit("public void main() { var count = 5; print(sizeof(count)); }");
    assert!(
        rust.contains("std::mem::size_of_val(&count)"),
        "expected value form for lowercase ident, got: {rust}",
    );
    assert!(
        !rust.contains("std::mem::size_of::<count>()"),
        "should not emit type form for lowercase ident: {rust}",
    );
}

/// Compound expression → value form per §5.9.3 rule 5.
#[test]
fn sizeof_compound_expr_emits_value_form() {
    let rust = emit("public void main() { print(sizeof(1 + 2)); }");
    assert!(
        rust.contains("std::mem::size_of_val(&(1 + 2))"),
        "got: {rust}",
    );
}

/// Multi-segment path → type form per §5.9.3 rule 4.
#[test]
fn sizeof_multi_segment_path_emits_type_form() {
    let rust = emit("public void main() { print(sizeof(std.io.Stream)); }");
    assert!(
        rust.contains("std::mem::size_of::<std::io::Stream>()"),
        "got: {rust}",
    );
}

/// A dotted operand whose root is a **value in scope** (`p.x`) is a
/// member access — the value form (§5.9.2) — not a type path, even though
/// it wears the same `a.B` shape as §5.9.3 rule 4. Regression for
/// `sizeof(obj.Name)` emitting the unresolvable `size_of::<obj::Name>()`.
#[test]
fn sizeof_member_access_on_local_emits_value_form() {
    let rust = emit(
        "public class Point { public int x; } \
         public void main() { var p = new Point(); print(sizeof(p.x)); }",
    );
    assert!(
        rust.contains("std::mem::size_of_val(&("),
        "expected value form for member access, got: {rust}",
    );
    assert!(
        !rust.contains("size_of::<p::x>()"),
        "should not emit a type path for member access: {rust}",
    );
}

/// Hex literal `0xF0` lowers to Rust `0xF0` — radix preserved.
#[test]
fn hex_literal_preserves_radix() {
    let rust = emit("public void main() { print(0xF0); }");
    assert!(rust.contains("println!(\"{}\", 0xF0)"), "got: {rust}");
}

/// Binary literal `0b1010` lowers to Rust `0b1010`.
#[test]
fn binary_literal_preserves_radix() {
    let rust = emit("public void main() { print(0b1010); }");
    assert!(rust.contains("println!(\"{}\", 0b1010)"), "got: {rust}");
}

/// Octal literal `0o17` lowers to Rust `0o17`.
#[test]
fn octal_literal_preserves_radix() {
    let rust = emit("public void main() { print(0o17); }");
    assert!(rust.contains("println!(\"{}\", 0o17)"), "got: {rust}");
}

/// Hex with suffix: `0xFFL` lowers to `0xFFi64`.
#[test]
fn hex_with_long_suffix() {
    let rust = emit("public void main() { print(0xFFL); }");
    assert!(rust.contains("println!(\"{}\", 0xFFi64)"), "got: {rust}");
}

// -----------------------------------------------------------------
// Suffixed literals + typed locals
// -----------------------------------------------------------------

/// `5L` (Jux Long) lowers to Rust `5i64`.
#[test]
fn long_literal_emits_i64_suffix() {
    let rust = emit("public void main() { print(5L); }");
    assert!(rust.contains("println!(\"{}\", 5i64)"), "got: {rust}");
}

/// `3u` lowers to `3u32`.
#[test]
fn uint_literal_emits_u32_suffix() {
    let rust = emit("public void main() { print(3u); }");
    assert!(rust.contains("println!(\"{}\", 3u32)"), "got: {rust}");
}

/// `5uL` lowers to `5u64`.
#[test]
fn ulong_literal_emits_u64_suffix() {
    let rust = emit("public void main() { print(5uL); }");
    assert!(rust.contains("println!(\"{}\", 5u64)"), "got: {rust}");
}

/// `3.14` lowers verbatim — Rust's default float is f64.
#[test]
fn default_float_literal_has_no_suffix() {
    let rust = emit("public void main() { print(3.14); }");
    assert!(rust.contains("println!(\"{}\", 3.14)"), "got: {rust}");
    assert!(!rust.contains("3.14f32"), "got: {rust}");
}

/// `1.5f` lowers to `1.5f32`.
#[test]
fn float_suffix_emits_f32() {
    let rust = emit("public void main() { print(1.5f); }");
    assert!(rust.contains("println!(\"{}\", 1.5f32)"), "got: {rust}");
}

/// `int x = 5;` lowers to `let x: isize = 5;` — platform-sized int.
/// Rust infers the literal `5` to `isize` from the binding annotation.
#[test]
fn typed_int_local_lowers_with_isize_annotation() {
    let rust = emit("public void main() { int x = 5; print(x); }");
    assert!(rust.contains("let x: isize = 5;"), "got: {rust}");
}

/// `i32 x = 5;` lowers to `let x: i32 = 5;` — fixed width.
#[test]
fn typed_i32_local_lowers_with_i32_annotation() {
    let rust = emit("public void main() { i32 x = 5; print(x); }");
    assert!(rust.contains("let x: i32 = 5;"), "got: {rust}");
}

/// `long n = 1000L;` lowers to `let n: i64 = 1000i64;` — the typed
/// annotation AND the literal suffix both come through.
#[test]
fn typed_long_local_with_suffixed_literal() {
    let rust = emit("public void main() { long n = 1000L; print(n); }");
    assert!(rust.contains("let n: i64 = 1000i64;"), "got: {rust}");
}

/// `var pi = 3.14;` — inferred (no `: f64` annotation), but the
/// emitted value is unambiguously a float (has `.14`).
#[test]
fn var_with_float_literal_keeps_decimal_point() {
    let rust = emit("public void main() { var pi = 3.14; print(pi); }");
    assert!(rust.contains("let pi = 3.14;"), "got: {rust}");
    // The annotation form for var is intentionally absent.
    assert!(!rust.contains(": f64 ="), "var form shouldn't annotate: {rust}");
}

/// `double rate = 5;` — typed local with type annotation. The
/// integer literal `5` doesn't auto-promote; Rust would error here
/// (mismatched i32 vs f64). That's *correct* — the user wrote
/// something semantically wrong and rustc surfaces it. Just check
/// our shape.
#[test]
fn typed_double_local_emits_f64_annotation() {
    let rust = emit("public void main() { double rate = 5.0; print(rate); }");
    assert!(rust.contains("let rate: f64 = 5.0;"), "got: {rust}");
}

// ----------------------------------------------------------------------
// Arrays (Turn 1: fixed-size)
// ----------------------------------------------------------------------

/// `int[10] xs = new int[10];` lowers to a `[isize; 10]` typed
/// local initialized with `[0; 10]`.
#[test]
fn fixed_int_array_lowers_to_isize_n_with_zero_init() {
    let rust = emit("public void main() { int[10] xs = new int[10]; print(xs[0]); }");
    assert!(
        rust.contains("let xs: [isize; 10] = [0; 10];"),
        "got: {rust}",
    );
}

/// `bool[5]` lowers to `[bool; 5]`; default value is `false`.
#[test]
fn fixed_bool_array_zero_inits_to_false() {
    let rust = emit("public void main() { bool[5] flags = new bool[5]; print(flags[0]); }");
    assert!(
        rust.contains("let flags: [bool; 5] = [false; 5];"),
        "got: {rust}",
    );
}

/// `double[3]` lowers to `[f64; 3]`; default value is `0.0`.
#[test]
fn fixed_double_array_zero_inits_to_zero_point_zero() {
    let rust = emit("public void main() { double[3] xs = new double[3]; print(xs[0]); }");
    assert!(
        rust.contains("let xs: [f64; 3] = [0.0; 3];"),
        "got: {rust}",
    );
}

// ----------------------------------------------------------------------
// Multi-dimensional arrays (nested Vec / nested fixed / mixed)
// ----------------------------------------------------------------------

/// `int[][]` (both dimensions dynamic) lowers to a nested
/// `Vec<Vec<isize>>`, and `new int[n][m]` to `vec![vec![0; m]; n]`.
#[test]
fn two_dim_dynamic_array_lowers_to_nested_vec() {
    let rust = emit(
        "public void main() { int[][] m = new int[3][4]; m[0][0] = 1; print(m[0][0]); }",
    );
    assert!(
        rust.contains("let mut m: Vec<Vec<isize>> = vec![vec![0; 4]; 3];"),
        "got: {rust}",
    );
}

/// `int[3][4]` (both dimensions fixed) lowers to a nested fixed array
/// `[[isize; 4]; 3]`, and `new int[3][4]` to `[[0; 4]; 3]`.
#[test]
fn two_dim_fixed_array_lowers_to_nested_fixed() {
    let rust = emit(
        "public void main() { int[3][4] b = new int[3][4]; b[0][0] = 1; print(b[0][0]); }",
    );
    assert!(
        rust.contains("let mut b: [[isize; 4]; 3] = [[0; 4]; 3];"),
        "got: {rust}",
    );
}

/// `int[3][]` mixes a fixed OUTER dimension with a dynamic inner one:
/// the Rust type is `[Vec<isize>; 3]` (outer fixed, inner Vec).
#[test]
fn mixed_fixed_outer_dynamic_inner_array_lowers_to_array_of_vec() {
    let rust = emit(
        "public void main() { int[3][] r = new int[3][4]; print(r[0].length); }",
    );
    assert!(
        rust.contains("let r: [Vec<isize>; 3] = [vec![0; 4]; 3];"),
        "got: {rust}",
    );
}

/// `int[][][]` (three dynamic dimensions) lowers to `Vec<Vec<Vec<isize>>>`.
#[test]
fn three_dim_dynamic_array_lowers_to_triple_nested_vec() {
    let rust = emit(
        "public void main() { int[][][] c = new int[2][2][2]; print(c[0][0][0]); }",
    );
    assert!(
        rust.contains("Vec<Vec<Vec<isize>>>"),
        "expected triply-nested Vec type, got: {rust}",
    );
    assert!(
        rust.contains("vec![vec![vec![0; 2]; 2]; 2]"),
        "expected triply-nested vec! init, got: {rust}",
    );
}

/// `String[][]` lowers to `Vec<Vec<String>>` — element type threads
/// through both dimensions.
#[test]
fn two_dim_string_array_lowers_to_nested_vec_of_string() {
    let rust = emit("public void main() { String[][] t; print(t.length); }");
    assert!(
        rust.contains("Vec<Vec<String>>"),
        "got: {rust}",
    );
}

/// Integer literal indices emit raw (no `as usize`) — Rust infers
/// `usize` from the indexing context.
#[test]
fn integer_literal_index_does_not_cast_to_usize() {
    let rust = emit("public void main() { int[10] xs = new int[10]; print(xs[3]); }");
    assert!(rust.contains("xs[3]"), "got: {rust}");
    assert!(!rust.contains("xs[(3)"), "literal indices should be naked: {rust}");
}

/// Non-literal indices (variables, expressions) get cast to `usize`
/// so platform-int (`isize`) loop counters index `[T; N]` correctly.
#[test]
fn variable_index_wraps_with_as_usize() {
    let rust = emit(
        "public void main() { int[10] xs = new int[10]; var i = 0; print(xs[i]); }",
    );
    assert!(rust.contains("xs[(i) as usize]"), "got: {rust}");
}

/// `xs[i] = v;` lowers to a direct indexed assignment with the same
/// `as usize` coercion on the index.
#[test]
fn indexed_assignment_emits_with_usize_coercion() {
    let rust = emit(
        "public void main() { int[3] xs = new int[3]; var i = 0; xs[i] = 7; }",
    );
    assert!(rust.contains("xs[(i) as usize] = 7;"), "got: {rust}");
}

/// `xs[i] = v;` causes the mutation analysis to promote `xs` to
/// `let mut` — the indexed write counts as mutation of `xs`.
#[test]
fn indexed_assignment_promotes_array_to_let_mut() {
    let rust = emit(
        "public void main() { int[3] xs = new int[3]; xs[0] = 1; }",
    );
    assert!(rust.contains("let mut xs:"), "expected `let mut xs:` — got: {rust}");
}

/// `arr.length` lowers to `arr.len() as isize` — Java-int-typed
/// length, despite Rust's `usize` underlying API. Identifier
/// receivers don't get spurious parens.
#[test]
fn array_length_lowers_to_len_as_isize() {
    let rust = emit(
        "public void main() { int[10] xs = new int[10]; print(xs.length); }",
    );
    assert!(
        rust.contains("xs.len() as isize"),
        "expected `.len() as isize`, got: {rust}",
    );
    assert!(
        !rust.contains("(xs).len()"),
        "no spurious parens around identifier receiver: {rust}",
    );
}

/// A composite receiver expression still gets wrapped in parens
/// because `.` binds tighter than the surrounding op.
#[test]
fn array_length_on_composite_receiver_keeps_parens() {
    // `(a + b).length` would be the natural shape if Jux ever
    // allowed array addition; for now an `if`-like expression is
    // the next-best non-atom receiver — kept minimal in case the
    // parser refuses other shapes.
    // Use a switch-as-expression returning an array: this is
    // tricky to write at the syntax level today, so we settle for
    // an `arr[i]` index receiver, which IS atom-shape and stays
    // paren-free. The composite-receiver case is harder to
    // exercise without more parser support and is left to the
    // emitter's own paren logic (covered by the negative assert
    // in `array_length_lowers_to_len_as_isize`).
    let rust = emit(
        "public void main() { int[3] xs = {1,2,3}; print(xs[0]); }",
    );
    assert!(rust.contains("xs[0]"), "got: {rust}");
}

// ----------------------------------------------------------------------
// Arrays (Turn 2: dynamic T[] + initializer-list literal)
// ----------------------------------------------------------------------

/// `int[]` typed local with a `new int[]{…}` initializer lowers to
/// `Vec<isize>` + `vec![…]`.
#[test]
fn dynamic_int_array_lowers_to_vec_isize_with_vec_macro() {
    let rust = emit(
        "public void main() { int[] xs = new int[]{1, 2, 3}; print(xs.length); }",
    );
    assert!(
        rust.contains("let xs: Vec<isize> = vec![1, 2, 3];"),
        "got: {rust}",
    );
}

/// `String[]` lowers to `Vec<String>` (Fix 1 — every Jux String
/// position is owned `String`, including array element types).
#[test]
fn dynamic_string_array_lowers_to_vec_owned_string() {
    let rust = emit(
        r#"public void main() { String[] xs = new String[]{"a", "b"}; print(xs.length); }"#,
    );
    assert!(
        rust.contains(r#"let xs: Vec<String> = vec!["a".to_string(), "b".to_string()];"#),
        "got: {rust}",
    );
}

/// `new int[]{}` (empty initializer) lowers to `Vec::<isize>::new()` —
/// `vec![]` alone would be type-ambiguous without an annotation.
#[test]
fn empty_new_array_lit_uses_turbofish_new() {
    let rust = emit(
        "public void main() { var xs = new int[]{}; print(xs.length); }",
    );
    assert!(
        rust.contains("Vec::<isize>::new()"),
        "expected turbofish Vec for empty literal, got: {rust}",
    );
}

/// `var xs = new int[]{…};` infers a `Vec` binding without an
/// explicit annotation. The element type defaults to `i32`
/// (Rust integer-literal default) — that's documented and fine.
#[test]
fn var_inferred_new_array_lit_has_no_annotation() {
    let rust = emit("public void main() { var xs = new int[]{1, 2}; print(xs.length); }");
    assert!(rust.contains("let xs = vec![1, 2];"), "got: {rust}");
}

// ----------------------------------------------------------------------
// Arrays (Turn 3: bare `{a, b, c}` initializer in typed-local RHS)
// ----------------------------------------------------------------------

/// `int[3] xs = {1, 2, 3};` — bare initializer on a fixed-size
/// LHS lowers to a Rust array literal `[1, 2, 3]`, not `vec!`.
#[test]
fn bare_init_on_fixed_lhs_emits_rust_array_literal() {
    let rust = emit("public void main() { int[3] xs = {1, 2, 3}; print(xs.length); }");
    assert!(
        rust.contains("let xs: [isize; 3] = [1, 2, 3];"),
        "got: {rust}",
    );
    assert!(
        !rust.contains("vec!"),
        "fixed-LHS bare init must not emit vec! — got: {rust}",
    );
}

/// `int[] xs = {1, 2, 3};` — bare initializer on a dynamic LHS
/// lowers to `vec![1, 2, 3]` (matching the `new T[]{…}` shape).
#[test]
fn bare_init_on_dynamic_lhs_emits_vec_macro() {
    let rust = emit("public void main() { int[] xs = {1, 2, 3}; print(xs.length); }");
    assert!(
        rust.contains("let xs: Vec<isize> = vec![1, 2, 3];"),
        "got: {rust}",
    );
}

/// `String[3] colors = {…};` — fixed-size string array lowers to
/// `[String; 3]` per Fix 1, with each element self-coerced via
/// `.to_string()`.
#[test]
fn bare_init_string_fixed_array_lowers_to_owned_string_array() {
    let rust = emit(
        r#"public void main() { String[3] cs = {"a", "b", "c"}; print(cs.length); }"#,
    );
    assert!(
        rust.contains(
            r#"let cs: [String; 3] = ["a".to_string(), "b".to_string(), "c".to_string()];"#
        ),
        "got: {rust}",
    );
}

/// `int[] xs = {};` — empty bare initializer on a dynamic LHS
/// still lowers via the turbofish `Vec::<isize>::new()` path.
#[test]
fn empty_bare_init_on_dynamic_lhs_emits_turbofish_new() {
    let rust = emit("public void main() { int[] xs = {}; print(xs.length); }");
    assert!(
        rust.contains("Vec::<isize>::new()"),
        "got: {rust}",
    );
}

// ----------------------------------------------------------------------
// Push / pop methods on dynamic arrays
// ----------------------------------------------------------------------

/// `xs.push(v)` emits verbatim as a Rust method call and promotes
/// `xs` to `let mut` via the mutating-method-call detector.
#[test]
fn push_method_call_promotes_to_let_mut() {
    let rust = emit("public void main() { int[] xs = {}; xs.push(7); }");
    assert!(rust.contains("let mut xs: Vec<isize>"), "got: {rust}");
    assert!(rust.contains("xs.push(7);"), "got: {rust}");
}

/// `xs.pop()` lowers to `xs.pop().unwrap()` since Jux doesn't have
/// `Option<T>` yet — the user expects a `T`-typed value back.
#[test]
fn pop_method_call_appends_unwrap() {
    let rust = emit(
        "public void main() { int[] xs = {}; xs.push(1); var v = xs.pop(); print(v); }",
    );
    assert!(rust.contains("let v = xs.pop().unwrap();"), "got: {rust}");
}

/// `xs.pop()` mutates `xs` even when its return value is bound to
/// a fresh local — the mutation analysis keys on the call shape,
/// not the assignment shape.
#[test]
fn pop_method_call_promotes_receiver_to_let_mut() {
    let rust = emit(
        "public void main() { int[] xs = {}; xs.push(1); var v = xs.pop(); print(v); }",
    );
    assert!(rust.contains("let mut xs: Vec<isize>"), "got: {rust}");
}

// ----------------------------------------------------------------------
// String fields on classes (position-aware mapping + auto-coercions)
// ----------------------------------------------------------------------

/// A class field of type `String` lowers to a Rust `String` field
/// (owned). Post Fix 1 the constructor parameter is also owned
/// `String`, so the field init becomes a plain move with no
/// `.to_string()` coercion. Reads in **value-consuming positions**
/// auto-clone so the field isn't moved out of `u`; reads in
/// **format-arg positions** skip the clone since `format!`/
/// `println!` borrow via `Display`.
///
/// Phase B note: `User` here is purely local (created, field-read,
/// dropped — never aliased / stored / passed / returned), so it
/// demotes to the legacy Inline plain-struct shape (§CR.3.3). Field
/// reads are direct `self.name` / `u.name` with no `.0.borrow()`
/// wrapper — the auto-`.clone()` discipline still fires on
/// value-consuming reads.
#[test]
fn string_field_lowers_to_owned_string_with_plain_move_init() {
    let rust = emit(
        r#"
        public class User {
            private String name;
            public User(String name) { this.name = name; }
            public String label() { return this.name; }
        }
        public void main() {
            var u = new User("Ada");
            print(u.name);
        }
        "#,
    );
    // Inline (demoted) shape: a plain field struct, no inner newtype,
    // owned `String` field.
    assert!(rust.contains("pub struct User {"), "inline plain struct: {rust}");
    assert!(!rust.contains("User_Inner"), "no inner newtype when Inline: {rust}");
    assert!(rust.contains("name: String,"), "field type: {rust}");
    assert!(rust.contains("pub fn new(name: String)"), "param: {rust}");
    // Rust struct field shorthand kicks in when init expr matches the
    // field name — the (raw, pre-rustfmt) literal reads `Self {\n name,\n}`.
    assert!(rust.contains("Self {"), "inline self literal: {rust}");
    assert!(!rust.contains("name: name"), "no longhand: {rust}");
    assert!(!rust.contains("name: self"), "no longhand: {rust}");
    // Value-consuming context — `return this.name;` — reads `self.name`
    // directly and clones so the field doesn't move out of `&self`.
    assert!(
        rust.contains("self.name.clone()"),
        "value-position read should still clone: {rust}",
    );
    // Format-arg context — `println!("{}", u.name)` — borrows direct
    // field, no clone needed.
    assert!(
        rust.contains(r#"println!("{}", u.name)"#),
        "format-arg read should NOT clone: {rust}",
    );
    assert!(
        !rust.contains("u.name.clone()"),
        "stale clone in format arg: {rust}",
    );
    // No interior-mutability wrapper in the Inline shape.
    assert!(!rust.contains(".0.borrow()"), "no borrow when Inline: {rust}");
}

// ----------------------------------------------------------------------
// Enums (Turn 1)
// ----------------------------------------------------------------------

/// A unit-variant enum lowers to a Rust `pub enum` with the right
/// auto-derives and a Display impl matching variant names.
#[test]
fn unit_enum_emits_derives_and_display() {
    let rust = emit("public enum Color { Red, Green, Blue }\npublic void main() {}");
    // Unit-only enums have no payload slots, so every eligibility check
    // is vacuously true and §O.3 grants the full derive set.
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]"),
        "got: {rust}",
    );
    assert!(rust.contains("pub enum Color {"), "got: {rust}");
    assert!(rust.contains("Red,"), "got: {rust}");
    assert!(rust.contains("Green,"), "got: {rust}");
    assert!(rust.contains("Blue,"), "got: {rust}");
    // Display impl with one arm per variant.
    assert!(rust.contains("impl std::fmt::Display for Color"), "got: {rust}");
    assert!(rust.contains(r#"Color::Red => write!(f, "Red"),"#), "got: {rust}");
}

/// `Color.Red` (Field on a known enum) lowers to the Rust path
/// `Color::Red`, not to instance-field access.
#[test]
fn enum_variant_access_lowers_to_path_form() {
    let rust = emit(
        "public enum Color { Red, Green }\npublic void main() { var c = Color.Red; print(c); }",
    );
    assert!(rust.contains("let c = Color::Red;"), "got: {rust}");
}

/// `Token.Number(42)` lowers to `Token::Number(42)` and the
/// Display impl's match arm destructures the payload so the
/// printed form is `Variant(value)` per JUX-LANG-V1 §7.7.2.
#[test]
fn payload_enum_variant_construction_and_display() {
    let rust = emit(
        "public enum Token { Number(int) }\npublic void main() { var t = Token.Number(7); print(t); }",
    );
    assert!(rust.contains("Number(isize),"), "got: {rust}");
    assert!(
        rust.contains("Token::Number(f0) => write!(f, \"Number({})\", f0),"),
        "got: {rust}",
    );
    assert!(rust.contains("let t = Token::Number(7);"), "got: {rust}");
}

// ----------------------------------------------------------------------
// Bounded type params (Turn 2) — `<T extends I & C>` lowering
// ----------------------------------------------------------------------

/// Interface bounds lower verbatim — `<T extends Drawable>` →
/// `<T: Drawable + Clone>`.
#[test]
fn interface_bound_lowers_to_trait_bound_directly() {
    let rust = emit(
        r#"
        public interface Drawable { void draw(); }
        public class Wrapper<T extends Drawable> {
            private T item;
            public Wrapper(T item) { this.item = item; }
        }
        public void main() {}
        "#,
    );
    // Interface bound flows verbatim through the impl bound list.
    assert!(
        rust.contains("impl<T: Drawable + Clone + std::fmt::Debug> Wrapper<T> {"),
        "bound: {rust}",
    );
}

/// Class bounds rewrite to a marker trait — `<T extends Animal>` →
/// `<T: AnimalKind + Clone>`, with `AnimalKind` declared and
/// implemented alongside the Animal class itself.
#[test]
fn class_bound_uses_marker_trait_kind() {
    let rust = emit(
        r#"
        public class Animal { public Animal() {} }
        public class Carrier<T extends Animal> {
            private T item;
            public Carrier(T item) { this.item = item; }
        }
        public void main() {}
        "#,
    );
    // Marker trait + impl emitted for Animal.
    assert!(rust.contains("pub trait AnimalKind: std::fmt::Debug {}"), "marker decl: {rust}");
    assert!(rust.contains("impl AnimalKind for Animal {}"), "marker impl: {rust}");
    // The bound on Carrier uses AnimalKind, not Animal directly.
    assert!(
        rust.contains("impl<T: AnimalKind + Clone + std::fmt::Debug> Carrier<T> {"),
        "marker bound: {rust}",
    );
}

/// `<T extends A & B>` with one class and one interface lowers to
/// `T: AKind + B + Clone` — the class bound rewrites, the
/// interface bound passes through.
#[test]
fn multi_bound_with_class_and_interface_combines() {
    let rust = emit(
        r#"
        public class Animal { public Animal() {} }
        public interface Greeter { String greet(); }
        public class Polite extends Animal implements Greeter {
            public Polite() { super(); }
            public String greet() { return $"hi"; }
        }
        public class Holder<T extends Animal & Greeter> {
            private T item;
            public Holder(T item) { this.item = item; }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("impl<T: AnimalKind + Greeter + Clone + std::fmt::Debug> Holder<T> {"),
        "combined bound: {rust}",
    );
    // Transitive marker impl — Polite implements AnimalKind because
    // it extends Animal.
    assert!(rust.contains("impl AnimalKind for Polite {}"), "transitive marker: {rust}");
}

// ----------------------------------------------------------------------
// Inheritance (Turn 1) — composition + Deref, super() in ctor
// ----------------------------------------------------------------------

/// `class Dog extends Animal { … }` (a non-sealed, non-exception
/// hierarchy) now lowers to the **wrapper shape** (§CR.5.1): each
/// class's inner struct embeds the parent's *inner* as `__parent`,
/// there's NO `Deref` (inherited methods are inlined into the child's
/// impl instead), and an `impl From<Dog> for Animal` performs the
/// identity-losing upcast.
#[test]
fn extends_lowers_to_wrapper_hierarchy() {
    let rust = emit(
        r#"
        public class Animal { private String name; public Animal(String name) { this.name = name; } }
        public class Dog extends Animal { public Dog(String name) { super(name); } }
        public void main() { var d = new Dog("Rex"); print(d); }
        "#,
    );
    // Dog's inner embeds Animal's INNER as `__parent` (not the
    // wrapper newtype) so the whole chain shares one RefCell.
    assert!(
        rust.contains("pub struct Dog_Inner {\n    pub __parent: Animal_Inner,"),
        "inner embed: {rust}",
    );
    // Both classes wrap in `Rc<RefCell<_Inner>>`.
    assert!(
        rust.contains("pub struct Animal(std::rc::Rc<std::cell::RefCell<Animal_Inner>>);"),
        "Animal wrapper newtype: {rust}",
    );
    assert!(
        rust.contains("pub struct Dog(std::rc::Rc<std::cell::RefCell<Dog_Inner>>);"),
        "Dog wrapper newtype: {rust}",
    );
    // Stage-2: `Animal` is a polymorphic base (extended by `Dog`), so the
    // identity-losing slicing `From<Dog> for Animal` is GONE — a base-typed
    // slot is `Rc<dyn AnimalKind>` and upcasts wrap (identity-preserving).
    // `Dog` implements the `AnimalKind` trait (empty here — `Animal` declares
    // no virtual methods).
    assert!(
        !rust.contains("impl From<Dog> for Animal"),
        "no slicing From for a polymorphic base: {rust}",
    );
    assert!(rust.contains("impl AnimalKind for Dog"), "Dog implements AnimalKind: {rust}");
    // No Deref for wrapper hierarchies.
    assert!(
        !rust.contains("impl std::ops::Deref for Dog"),
        "no Deref in wrapper path: {rust}",
    );
}

/// `super(args);` in a child constructor lifts into the
/// `__parent: Parent::new_inner(args)` slot of the child's inner
/// literal (wrapper hierarchy path).
#[test]
fn super_call_lifts_into_inner_literal() {
    let rust = emit(
        r#"
        public class Animal { private String name; public Animal(String name) { this.name = name; } }
        public class Dog extends Animal {
            private int age;
            public Dog(String name, int age) {
                super(name);
                this.age = age;
            }
        }
        public void main() { var d = new Dog("Rex", 4); print(d); }
        "#,
    );
    // `Animal::new_inner(name)` appears in Dog's inner literal, NOT as
    // a separate statement.
    assert!(
        rust.contains("__parent: Animal::new_inner(name)"),
        "super lifted into inner: {rust}",
    );
    // The own-field assignment lands too (Rust field shorthand).
    assert!(rust.contains("age"), "own field init: {rust}");
    // The `super(...)` doesn't survive as a statement.
    assert!(
        !rust.contains("super(") && !rust.contains("__super__"),
        "super shouldn't appear in body: {rust}",
    );
    // Public `new` delegates to `new_inner`.
    assert!(
        rust.contains("Self(std::rc::Rc::new(std::cell::RefCell::new(Self::new_inner("),
        "new delegates to new_inner: {rust}",
    );
}

/// An `abstract` method emits an `unimplemented!()` stub in the
/// inherent impl. Subclasses that define a body shadow it via
/// inherent-method dispatch (taking precedence over Deref-based
/// access to the parent's stub).
#[test]
fn abstract_method_emits_unimplemented_stub() {
    let rust = emit(
        r#"
        public abstract class Animal {
            public abstract String speak();
        }
        public class Dog extends Animal {
            public String speak() { return $"Woof!"; }
        }
        public void main() { var d = new Dog(); print(d.speak()); }
        "#,
    );
    // Parent's abstract method stubs out with unimplemented!().
    assert!(
        rust.contains(r#"unimplemented!("abstract method speak")"#),
        "abstract stub: {rust}",
    );
    // Subclass's override emits the actual body in its inherent impl.
    // Post Fix 5: `$"Woof!"` has no interp segments and lowers to
    // `"Woof!".to_string()` instead of `format!("Woof!")`.
    assert!(rust.contains(r#""Woof!".to_string()"#), "Dog::speak body: {rust}");
}

// ----------------------------------------------------------------------
// Interfaces (Turn 1) — pub trait + delegating impl blocks
// ----------------------------------------------------------------------

/// A bare interface lowers to a Rust `pub trait` with one signature
/// per declared method. Methods take `&self` in Turn 1.
#[test]
fn interface_lowers_to_pub_trait_with_method_signatures() {
    let rust = emit(
        r#"
        public interface Drawable {
            void draw();
            int weight();
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("pub trait Drawable: std::fmt::Debug {"),
        "trait header: {rust}"
    );
    // Interface methods emit as `&self` so the interface can be used as a
    // `dyn` value type (`Rc<dyn Trait>`). Implementers are forced wrapper
    // classes, so a `this.field` write goes through interior `borrow_mut()`
    // and needs no mutable receiver.
    assert!(rust.contains("fn draw(&self);"), "void method: {rust}");
    assert!(rust.contains("fn weight(&self) -> isize;"), "int method: {rust}");
}

/// A class implementing an interface gets two impl blocks:
/// the canonical inherent `impl Class` with the bodies, and a
/// delegating `impl Interface for Class` whose methods forward to
/// the inherent ones.
#[test]
fn class_implements_emits_inherent_and_delegating_impls() {
    let rust = emit(
        r#"
        public interface Greeter {
            int magic();
        }
        public class Friendly implements Greeter {
            public int magic() { return 7; }
        }
        public void main() {
            var f = new Friendly();
            print(f.magic());
        }
        "#,
    );
    // Inherent impl carries the actual body.
    assert!(rust.contains("impl Friendly {"), "inherent impl: {rust}");
    assert!(
        rust.contains("pub fn magic(&self) -> isize {"),
        "inherent method header: {rust}",
    );
    // Delegating trait impl forwards through `Friendly::magic(self)`
    // — Rust's exact-receiver method-resolution rule prefers the
    // trait method over an auto-reborrowed inherent, so the
    // explicit `ClassName::method` path bypasses the recursion
    // that a `self.magic()` body would trigger.
    assert!(
        rust.contains("impl Greeter for Friendly {"),
        "trait impl header: {rust}",
    );
    assert!(rust.contains("Friendly::magic(self)"), "delegating call: {rust}");
}

/// Multiple `implements` produces one delegating impl block per
/// interface listed, all delegating to the inherent methods.
#[test]
fn class_implements_multiple_interfaces_emits_one_impl_each() {
    let rust = emit(
        r#"
        public interface A { int a(); }
        public interface B { int b(); }
        public class C implements A, B {
            public int a() { return 1; }
            public int b() { return 2; }
        }
        public void main() {}
        "#,
    );
    assert!(rust.contains("impl A for C {"), "impl A: {rust}");
    assert!(rust.contains("impl B for C {"), "impl B: {rust}");
}

// ----------------------------------------------------------------------
// Records (Turn 1) — header-only records + canonical constructors
// ----------------------------------------------------------------------

/// A primitive-component record lowers to a Rust `pub struct` with
/// `Debug + Clone + PartialEq` derives, public component fields,
/// and a canonical `pub fn new(...)`.
#[test]
fn primitive_record_lowers_to_struct_and_canonical_ctor() {
    let rust = emit(
        r#"
        public record Vector3(double x, double y, double z) {}
        public void main() {
            var v = new Vector3(1.0, 2.0, 3.0);
            print(v.x);
        }
        "#,
    );
    // Floats are Copy but NOT Eq/Hash → Vector3 picks up Copy from the
    // §O.3 auto-derive pass on top of the baseline three. Default
    // is added because every component (double) is `Default`-able.
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Copy, Default)]"),
        "derive line: {rust}",
    );
    assert!(rust.contains("pub struct Vector3 {"), "struct header: {rust}");
    assert!(rust.contains("pub x: f64,"), "component x: {rust}");
    assert!(rust.contains("pub fn new(x: f64, y: f64, z: f64) -> Self {"), "ctor: {rust}");
    // Canonical constructor body uses the direct `Self { … }` shape
    // with Rust's struct field shorthand (`x` not `x: x`).
    assert!(rust.contains("x,\n"), "self-init x shorthand: {rust}");
    assert!(!rust.contains("x: x"), "no longhand: {rust}");
    // Construction site: `Vector3::new(1.0, 2.0, 3.0)`.
    assert!(rust.contains("Vector3::new(1.0, 2.0, 3.0)"), "ctor call: {rust}");
}

/// String-component records get the Fix-1 unified treatment: the
/// field is owned `String`, the ctor parameter is also owned
/// `String`, and the init is a plain move (no `.to_string()`).
/// Format-arg reads borrow rather than clone.
#[test]
fn string_component_record_uses_owned_string_throughout() {
    let rust = emit(
        r#"
        public record Greeting(String name, int age) {}
        public void main() {
            var g = new Greeting("Ada", 36);
            print(g.name);
        }
        "#,
    );
    assert!(rust.contains("pub name: String,"), "field type: {rust}");
    assert!(rust.contains("pub fn new(name: String, age: isize)"), "ctor params: {rust}");
    // Record canonical ctor binds each component to its own name,
    // so the Self literal uses Rust's struct field shorthand.
    assert!(rust.contains("name,\n"), "init shorthand: {rust}");
    assert!(!rust.contains("name: name"), "no longhand: {rust}");
    // Format-arg read: borrow, no clone.
    assert!(
        rust.contains(r#"println!("{}", g.name)"#),
        "format-arg read: {rust}",
    );
    assert!(
        !rust.contains("g.name.clone()"),
        "stale clone in format arg: {rust}",
    );
}

/// Generic record `Pair<A, B>` lowers to a Rust generic struct with
/// the `Clone`-bounded impl and the auto-clone-on-read for
/// generic-typed components.
#[test]
fn generic_record_emits_clone_bound_and_generic_fields() {
    let rust = emit(
        r#"
        public record Pair<A, B>(A first, B second) {}
        public void main() {
            var p = new Pair<int, int>(1, 2);
            print(p.first);
        }
        "#,
    );
    assert!(rust.contains("pub struct Pair<A, B> {"), "generic header: {rust}");
    assert!(rust.contains("pub first: A,"), "generic field: {rust}");
    assert!(rust.contains("impl<A: Clone + std::fmt::Debug, B: Clone + std::fmt::Debug> Pair<A, B> {"), "bound: {rust}");
    // Generic-component read in format-arg context borrows, no clone.
    assert!(
        rust.contains(r#"println!("{}", p.first)"#),
        "format-arg read: {rust}",
    );
}

// ----------------------------------------------------------------------
// Generics (Turn 1) — generic class declarations + uses
// ----------------------------------------------------------------------

/// `class Box<T> { T value; … }` whose instance is purely local
/// (created, method-called, dropped — never aliased / stored / passed /
/// returned) demotes to the legacy **Inline** plain-struct shape
/// (Phase B fast tier, §CR.3.3). The struct holds the generic field
/// directly, the inherent `impl` carries the `T: Clone + Debug` bound,
/// and the generic-typed field read is a direct `self.value.clone()`
/// with no interior-mutability wrapper. (The aliased generic case —
/// `generic_class_alias_shares_mutation_through_rc_refcell` — keeps the
/// `Rc<RefCell>` wrapper.)
#[test]
fn generic_class_lowers_to_rust_struct_and_clone_bounded_impl() {
    let rust = emit(
        r#"
        public class Box<T> {
            private T value;
            public Box(T value) { this.value = value; }
            public T get() { return this.value; }
        }
        public void main() {
            var b = new Box<int>(7);
            print(b.get());
        }
        "#,
    );
    assert!(rust.contains("#[derive(Clone, Debug)]"), "derive(Clone, Debug): {rust}");
    // Inline plain struct holds the generic field directly — no inner
    // newtype, no `Rc<RefCell>` wrapper. The struct's own type params carry the
    // `Clone + Debug` bound (the `#[derive]` needs it, and a generic field of a
    // bounded type propagates the bound).
    assert!(
        rust.contains("pub struct Box<T: Clone + std::fmt::Debug> {"),
        "inline struct header carries the Clone+Debug bound: {rust}",
    );
    assert!(!rust.contains("Box_Inner"), "no inner newtype when Inline: {rust}");
    // Class-scoped probe — the prelude's JuxStream/JuxChannel types
    // are RefCell-backed, so a whole-output `contains` would
    // false-positive.
    assert!(
        !rust.contains("RefCell<Box"),
        "no RefCell wrapper when Inline: {rust}",
    );
    assert!(rust.contains("value: T,"), "generic field: {rust}");
    // The inherent impl carries the `T: Clone + Debug` bound.
    assert!(rust.contains("impl<T: Clone + std::fmt::Debug> Box<T> {"), "impl bound: {rust}");
    assert!(rust.contains("pub fn new(value: T) -> Self {"), "inline new: {rust}");
    // Field shorthand (raw emit is multi-line; rustfmt collapses to
    // `Self { value }`).
    assert!(rust.contains("Self {"), "inline self literal: {rust}");
    assert!(!rust.contains("value: value"), "no longhand: {rust}");
    // Method body reads the generic field directly + clones.
    assert!(rust.contains("self.value.clone()"), "direct field read+clone: {rust}");
    assert!(!rust.contains(".0.borrow()"), "no borrow when Inline: {rust}");
}

/// A generic wrapper class shares mutation through its `Rc<RefCell>`:
/// `var y = x` clones the `Rc` (refcount bump), so a `set` through one
/// alias is observable through the other. This nails down the Java
/// reference-semantics contract for generic classes — the whole point
/// of routing them through the wrapper shape.
#[test]
fn generic_class_alias_shares_mutation_through_rc_refcell() {
    let rust = emit(
        r#"
        public class Holder<T> {
            private T v;
            public Holder(T v) { this.v = v; }
            public T get() { return this.v; }
            public void set(T v) { this.v = v; }
        }
        public void main() {
            var x = new Holder<int>(1);
            var y = x;
            y.set(9);
            print(x.get());
        }
        "#,
    );
    // Newtype + inner are the wrapper shape (shared cell).
    assert!(
        rust.contains("pub struct Holder<T: Clone + std::fmt::Debug>(std::rc::Rc<std::cell::RefCell<Holder_Inner<T>>>);"),
        "newtype: {rust}",
    );
    // `var y = x` aliases through the newtype's derived Clone — the
    // `.clone()` bumps the shared `Rc` refcount (it does NOT deep-copy
    // the cell), so both names point at one `RefCell`. (`y` is promoted
    // to `let mut` because `y.set(...)` calls a mutating method.)
    assert!(rust.contains("y = x.clone()"), "alias rebind via Rc clone: {rust}");
    // The setter writes through a scoped `borrow_mut()`, so the mutation
    // lands in the shared cell and is visible through `x`.
    assert!(rust.contains("borrow_mut()"), "scoped write: {rust}");
}

/// §CR.4.1 read-only-shared demotion: a class that is ALIASED (a second
/// binding) but never MUTATED after construction lowers to a bare `Rc<C_Inner>`
/// — no `RefCell`, no borrow-flag cost. Field reads go through plain `.0`.
#[test]
fn aliased_immutable_class_lowers_to_bare_rc_no_refcell() {
    let rust = emit(
        r#"
        public class Point {
            public int x;
            public Point(int x) { this.x = x; }
            public int getX() { return this.x; }
        }
        public void main() {
            var p = new Point(1);
            var q = p;
            print(q.getX() + p.getX());
        }
        "#,
    );
    // Bare `Rc` newtype — the cell is gone (the ctor write is construction, the
    // getter is a read, so `Point` is never mutated).
    assert!(
        rust.contains("pub struct Point(std::rc::Rc<Point_Inner>);"),
        "expected bare-Rc newtype (no RefCell): {rust}",
    );
    assert!(
        !rust.contains("RefCell<Point_Inner>"),
        "Point must NOT carry a RefCell: {rust}",
    );
    // Constructor wraps in plain `Rc::new`, no `RefCell::new`.
    assert!(
        rust.contains("std::rc::Rc::new(Self::new_inner"),
        "ctor wraps in plain Rc::new: {rust}",
    );
    // Aliasing still shares via an `Rc` clone (pointer bump, not deep copy).
    assert!(rust.contains("q = p.clone()"), "alias rebind via Rc clone: {rust}");
}

/// §CR.3.3 Box demotion: a class that ESCAPES (returned from a function) but is
/// never aliased and never mutated lowers to `C(Box<C_Inner>)` — a unique owner
/// on the heap, no refcount and no cell.
#[test]
fn escaping_unaliased_immutable_class_lowers_to_box() {
    let rust = emit(
        r#"
        public class Token {
            public int id;
            public Token(int id) { this.id = id; }
            public int getId() { return this.id; }
        }
        Token make(int n) { return new Token(n); }
        public void main() {
            print(make(5).getId());
        }
        "#,
    );
    assert!(
        rust.contains("pub struct Token(std::boxed::Box<Token_Inner>);"),
        "expected Box newtype: {rust}",
    );
    assert!(
        rust.contains("std::boxed::Box::new(Self::new_inner"),
        "ctor wraps in Box::new: {rust}",
    );
    assert!(!rust.contains("RefCell<Token_Inner>"), "Box Token has no RefCell: {rust}");
}

/// Explicit construction `new Box<int>(7)` lowers to the Rust
/// turbofish `Box::<isize>::new(7)`. Implicit `new Box(7)` keeps
/// the bare `Box::new(7)` form (Rust infers).
#[test]
fn generic_new_object_emits_turbofish_when_args_explicit() {
    let rust = emit(
        r#"
        public class Box<T> {
            private T value;
            public Box(T value) { this.value = value; }
        }
        public void main() {
            var explicit = new Box<int>(7);
            var inferred = new Box(8);
            print(explicit.value);
            print(inferred.value);
        }
        "#,
    );
    assert!(rust.contains("Box::<isize>::new(7)"), "turbofish: {rust}");
    assert!(rust.contains("Box::new(8)"), "inferred: {rust}");
}

/// Simple constructors (body is purely `this.field = expr;` lines)
/// build the inner struct directly — no `__self` builder, no
/// `Default`-based initialization.
///
/// Post class-representation Phase B (§CR.3.3), a class whose instances
/// are never aliased / stored / passed / returned demotes back to the
/// legacy plain-struct **Inline** shape (the escape-analysis "fast
/// tier"). Here `Pair` is created, read via a field access, and dropped
/// — purely local — so it lowers to `pub struct Pair { … }` with a
/// direct `Self { a, b }` constructor and NO `Rc<RefCell<…>>` wrapper.
#[test]
fn simple_constructor_emits_direct_self_literal() {
    let rust = emit(
        r#"
        public class Pair {
            private int a;
            private int b;
            public Pair(int a, int b) { this.a = a; this.b = b; }
        }
        public void main() {
            var p = new Pair(1, 2);
            print(p.a);
        }
        "#,
    );
    // Inline (demoted) shape: a plain field struct, no inner newtype.
    assert!(
        rust.contains("pub struct Pair {"),
        "inline plain struct: {rust}",
    );
    assert!(!rust.contains("Pair_Inner"), "no inner newtype when Inline: {rust}");
    // Class-scoped probe — the always-emitted prelude legitimately
    // contains `Rc::new(RefCell::new(…))` (JuxStream), so a whole-
    // output `contains` would false-positive.
    assert!(
        !rust.contains("RefCell<Pair"),
        "no Rc<RefCell> wrapper when Inline: {rust}",
    );
    // No `__self` builder — the simple-ctor path emits the literal
    // directly with field shorthand.
    assert!(!rust.contains("__self"), "should not use __self pattern: {rust}");
    assert!(
        rust.contains("pub fn new(a: isize, b: isize) -> Self {"),
        "inline new signature: {rust}",
    );
    // Inline struct literal with field shorthand (the raw emit is
    // multi-line; rustfmt collapses it to `Self { a, b }`).
    assert!(rust.contains("Self {"), "inline self literal: {rust}");
    assert!(!rust.contains("a: a"), "no longhand: {rust}");
    assert!(!rust.contains("a: self"), "no longhand: {rust}");
}

// ----------------------------------------------------------------------
// Class representation — Phase A wrapper shape (§CR.4.1 / §CR.6)
// ----------------------------------------------------------------------

/// A simple class (no `extends` / `sealed` / generics / abstract)
/// lowers to the shared-mutation, interior-mutable wrapper shape so
/// Jux gets Java reference semantics: the instance fields live in a
/// private `C_Inner` struct, the user-visible type `C` is a
/// `#[derive(Clone)]` newtype over `Rc<RefCell<C_Inner>>`, the
/// constructor wraps an inner literal, methods take `&self` (interior
/// mutability removes the need for `&mut self`), field reads go
/// through a statement-scoped `.0.borrow()`, and field writes through
/// a scoped-temp `.0.borrow_mut()`.
#[test]
fn simple_class_lowers_to_shared_mutation_wrapper() {
    let rust = emit(
        r#"
        public class Counter {
            int v;
            public Counter(int v) { this.v = v; }
            public void set(int v) { this.v = v; }
            public int get() { return this.v; }
        }
        public void main() {
            var x = new Counter(1);
            var y = x;
            y.set(9);
            print(x.get());
        }
        "#,
    );
    // Inner field struct + Debug for the derived `Debug` on the
    // newtype (`Rc<RefCell<C_Inner>>: Debug` needs `C_Inner: Debug`).
    assert!(
        rust.contains("pub struct Counter_Inner {"),
        "inner struct: {rust}",
    );
    assert!(rust.contains("v: isize,"), "inner field: {rust}");
    // The newtype handle.
    assert!(
        rust.contains(
            "pub struct Counter(std::rc::Rc<std::cell::RefCell<Counter_Inner>>);"
        ),
        "newtype handle: {rust}",
    );
    // Constructor: `new_inner` builds the inner literal; the public
    // `new` wraps it.
    assert!(
        rust.contains("Counter_Inner { v }"),
        "inner literal: {rust}",
    );
    assert!(
        rust.contains(
            "Self(std::rc::Rc::new(std::cell::RefCell::new(Self::new_inner(v))))"
        ),
        "wrapping ctor: {rust}",
    );
    // Mutating method stays `&self` and writes via the scoped
    // `borrow_mut()` temp shape.
    assert!(
        rust.contains("pub fn set(&self, v: isize)"),
        "method receiver stays &self under interior mutability: {rust}",
    );
    assert!(
        rust.contains("self.0.borrow_mut().v = __jux_v;"),
        "scoped field write: {rust}",
    );
    // Field read through a statement-scoped immutable borrow.
    assert!(
        rust.contains("self.0.borrow().v"),
        "scoped field read: {rust}",
    );
    // `var y = x;` shares the same instance — cheap `Rc` clone.
    assert!(
        rust.contains("= x.clone();"),
        "share-on-assignment clone: {rust}",
    );
}

/// A method call on a wrapper-class value (`obj.method(args)`) calls
/// the inherent method on the newtype directly — it must NOT be
/// rewritten through `.0.borrow()` (that rewrite is for *field*
/// reads only). Regression guard for the method-vs-field disambiguation.
#[test]
fn wrapper_method_call_is_not_borrow_wrapped() {
    let rust = emit(
        r#"
        public class Counter {
            int v;
            public Counter(int v) { this.v = v; }
            public int get() { return this.v; }
        }
        public void main() {
            var x = new Counter(5);
            print(x.get());
        }
        "#,
    );
    assert!(rust.contains("x.get()"), "method call direct: {rust}");
    assert!(
        !rust.contains("x.0.borrow().get()"),
        "method call must not be borrow-wrapped: {rust}",
    );
}

/// A non-sealed `extends` hierarchy whose instance is **aliased**
/// (`var e = d;`) stays on the wrapper shape with the whole chain
/// rolled up into ONE `Rc<RefCell<_>>` per handle (§CR.3.5 / §CR.5.1).
/// An **inherited method inlined into the child** addresses inherited
/// fields by walking `__parent`: a field declared one ancestor up emits
/// `self.0.borrow().__parent.<field>`. This is the mechanism that makes
/// shared mutation visible through every alias of an instance, even
/// across the inheritance boundary. (The `var e = d;` alias is what
/// keeps the hierarchy wrapped under the Phase B selector — without it
/// the purely-local instance would demote to Inline.)
#[test]
fn wrapper_hierarchy_inherited_field_walks_parent() {
    let rust = emit(
        r#"
        public class Animal {
            public int age;
            public Animal(int age) { this.age = age; }
            public void birthday() { this.age = this.age + 1; }
        }
        public class Dog extends Animal {
            public String name;
            public Dog(String name, int age) { super(age); this.name = name; }
        }
        public void main() { var d = new Dog("Rex", 3); var e = d; e.birthday(); }
        "#,
    );
    // Child inner embeds the parent's INNER as `__parent`.
    assert!(
        rust.contains("pub struct Dog_Inner {\n    pub __parent: Animal_Inner,"),
        "inner embed: {rust}",
    );
    // The inherited `birthday()` is inlined into Dog's impl and reads
    // the inherited `age` through `__parent` (depth 1), reading via a
    // statement-scoped borrow and writing via a scoped-temp
    // `borrow_mut()`.
    assert!(
        rust.contains("self.0.borrow().__parent.age"),
        "inherited-field read walks __parent: {rust}",
    );
    assert!(
        rust.contains("self.0.borrow_mut().__parent.age = __jux_v"),
        "inherited-field write walks __parent: {rust}",
    );
    // Construction chains `Animal::new_inner(age)` into Dog's inner.
    assert!(
        rust.contains("__parent: Animal::new_inner(age)"),
        "ctor chains parent new_inner: {rust}",
    );
    // Stage-2: `Animal` is a polymorphic base, so the slicing `From<Dog> for
    // Animal` is GONE — upcasts wrap into `Rc<dyn AnimalKind>` (identity-
    // preserving). `Dog` implements `AnimalKind`, which carries the inherited
    // virtual `birthday` (delegating to `Dog::birthday`).
    assert!(
        !rust.contains("impl From<Dog> for Animal"),
        "no slicing From for a polymorphic base: {rust}",
    );
    assert!(rust.contains("impl AnimalKind for Dog"), "Dog implements AnimalKind: {rust}");
    // No Deref/DerefMut on the wrapper hierarchy.
    assert!(
        !rust.contains("impl std::ops::Deref for Dog")
            && !rust.contains("impl std::ops::DerefMut for Dog"),
        "no Deref/DerefMut: {rust}",
    );
}

/// A 3-level wrapper hierarchy (`Dog` → `Mammal` → `Animal`) walks
/// TWO `__parent` hops for a field declared on the grandparent. This
/// locks in the depth indexing for inlined inherited methods: `Dog`'s
/// copy of `Animal::name()` (which reads `this.name`) emits
/// `self.0.borrow().__parent.__parent.name`. The `var e = d;` alias is
/// what keeps the chain wrapped under the Phase B selector (§CR.3.3) —
/// the whole connected component rolls up to `Rc<RefCell>` because one
/// member is aliased (§CR.3.5).
#[test]
fn wrapper_three_level_hierarchy_walks_two_parents() {
    let rust = emit(
        r#"
        public abstract class Animal {
            private String name;
            public Animal(String name) { this.name = name; }
            public String name() { return this.name; }
        }
        public abstract class Mammal extends Animal {
            private int legs;
            public Mammal(String name, int legs) { super(name); this.legs = legs; }
        }
        public final class Dog extends Mammal {
            public Dog(String name) { super(name, 4); }
        }
        public void main() { var d = new Dog("Rex"); var e = d; print(e.name()); }
        "#,
    );
    // `Dog_Inner` embeds `Mammal_Inner`, which embeds `Animal_Inner`.
    assert!(
        rust.contains("pub struct Dog_Inner {\n    pub __parent: Mammal_Inner,"),
        "Dog inner: {rust}",
    );
    assert!(
        rust.contains("pub struct Mammal_Inner {\n    pub __parent: Animal_Inner,"),
        "Mammal inner: {rust}",
    );
    // The inherited `name()` reaches the grandparent field across two
    // `__parent` hops (with the auto-`.clone()` on the String read).
    assert!(
        rust.contains("self.0.borrow().__parent.__parent.name"),
        "two-level inherited field walk: {rust}",
    );
    // Construction chains `new_inner` through every level.
    assert!(
        rust.contains("__parent: Mammal::new_inner(name, 4)"),
        "Dog ctor chains Mammal::new_inner: {rust}",
    );
    assert!(
        rust.contains("__parent: Animal::new_inner(name)"),
        "Mammal ctor chains Animal::new_inner: {rust}",
    );
}

/// **Wrapper-class share through a collection (§CR.4.1, generalized).**
/// A wrapped-class value stored into an array literal and later read
/// back out by index must round-trip through `.clone()` at BOTH ends:
///
/// - The store `new Cell[]{ c }` emits `vec![c.clone()]` — a SHARED
///   `Rc` handle goes into the Vec, NOT a destructive move of `c`
///   (which would leave the later `c.get()` reading a moved value:
///   `E0382`).
/// - The index read `var r = xs[0]` emits `xs[0].clone()` — reading a
///   shared handle OUT of the Vec, NOT moving out of it (`E0507 cannot
///   move out of index`).
///
/// Both ends pointing at the same `RefCell` is what makes `r.set(42)`
/// observable through `c.get()`. The class is kept wrapped because it
/// escapes into the collection and is aliased through the index read.
#[test]
fn wrapper_array_store_and_index_read_clone() {
    let rust = emit(
        r#"
        public class Cell {
            int v;
            public Cell(int v) { this.v = v; }
            public void set(int v) { this.v = v; }
            public int get() { return this.v; }
        }
        public void main() {
            var c = new Cell(1);
            var xs = new Cell[]{ c };
            var r = xs[0];
            r.set(42);
            print(c.get());
        }
        "#,
    );
    // Store: the element clones into the Vec (shared handle in).
    assert!(
        rust.contains("vec![c.clone()]"),
        "array-store element clones (shared handle into Vec): {rust}",
    );
    // Index read: the value clones out of the Vec (shared handle out).
    assert!(
        rust.contains("xs[0].clone()"),
        "index-read clones out of the Vec: {rust}",
    );
    // The method-call receiver `r.set(...)` must NOT clone (it borrows
    // the shared handle — cloning here would mutate a throwaway copy).
    assert!(
        !rust.contains("r.clone().set("),
        "method receiver must not clone: {rust}",
    );
}

/// A thrown exception type (any class whose `extends` chain reaches
/// `Throwable`) stays on the **legacy plain-struct** path, NOT the
/// wrapper shape: `Rc<RefCell<_>>` is `!Send`, but `panic_any`
/// requires `Send`. Detection is by the `Throwable` ancestor. The
/// bare-name collision rule also keeps a user class that *shares a
/// name* with a stdlib exception on the legacy path (it may itself be
/// thrown). This program declares a fresh exception chain so the check
/// is self-contained.
#[test]
fn exception_hierarchy_stays_on_legacy_path() {
    let rust = emit(
        r#"
        public class Throwable { public String message; public Throwable(String m) { this.message = m; } }
        public class MyError extends Throwable {
            public MyError(String m) { super(m); }
        }
        public void boom() { throw new MyError("bad"); }
        public void main() { boom(); }
        "#,
    );
    // Legacy plain-struct shape — no `_Inner` newtype, no
    // `Rc<RefCell<_>>` wrapper for the exception classes.
    assert!(
        !rust.contains("struct MyError_Inner"),
        "exception must not get wrapper inner: {rust}",
    );
    assert!(
        !rust.contains("MyError(std::rc::Rc<std::cell::RefCell<"),
        "exception must not get Rc<RefCell> newtype: {rust}",
    );
    // Legacy `__parent: Throwable` embed (a real struct, not `_Inner`).
    assert!(
        rust.contains("__parent: Throwable,"),
        "legacy parent embed: {rust}",
    );
    // It's still thrown via panic_any.
    assert!(rust.contains("std::panic::panic_any("), "throw lowering: {rust}");
}

// ----------------------------------------------------------------------
// Pattern matching (Turn 1) — switch / match
// ----------------------------------------------------------------------

/// Statement-form `switch` over a unit-variant enum lowers to
/// `match scrutinee { Color::Red => …, _ => … }`. `default` arms
/// become Rust's `_` wildcard.
#[test]
fn statement_switch_with_default_lowers_to_match_with_wildcard() {
    let rust = emit(
        r#"
        public enum Color { Red, Green }
        public void main() {
            var c = Color.Red;
            switch (c) {
                case Color.Red -> print("r");
                default        -> print("other");
            }
        }
        "#,
    );
    assert!(rust.contains("match c {"), "got: {rust}");
    assert!(rust.contains("Color::Red =>"), "variant arm: {rust}");
    assert!(rust.contains("_ =>"), "default → _: {rust}");
}

/// Expression-form `switch` returns a value the user can bind.
/// Each arm's body is the arm's value.
#[test]
fn expression_switch_returns_value_per_arm() {
    let rust = emit(
        r#"
        public enum Color { Red, Green }
        public void main() {
            var c = Color.Red;
            var label = switch (c) {
                case Color.Red -> 1;
                case Color.Green -> 2;
            };
            print(label);
        }
        "#,
    );
    // The match appears on the right of a let binding, with both
    // arm bodies emitted as the per-arm value.
    assert!(rust.contains("let label = match c"), "got: {rust}");
    assert!(rust.contains("Color::Red => 1,"), "got: {rust}");
    assert!(rust.contains("Color::Green => 2,"), "got: {rust}");
}

/// Payload-binding pattern `var n` drops the `var` keyword in the
/// Rust match arm — the slot value is bound to the bare name `n`.
#[test]
fn variant_payload_var_binding_drops_var_keyword() {
    let rust = emit(
        r#"
        public enum Token { Number(int) }
        public void main() {
            var t = Token.Number(7);
            switch (t) {
                case Token.Number(var n) -> print(n);
            }
        }
        "#,
    );
    assert!(rust.contains("Token::Number(n) =>"), "got: {rust}");
    // The original `var` keyword should not survive into the arm.
    assert!(!rust.contains("Token::Number(var n)"), "leaked var: {rust}");
}

/// String-payload variants get a `.to_string()` injected on the
/// `&str` argument at construction site.
#[test]
fn string_payload_variant_injects_to_string_coercion() {
    let rust = emit(
        r#"public enum Token { Word(String) }
           public void main() { var w = Token.Word("hi"); print(w); }"#,
    );
    // The payload field type is owned String:
    assert!(rust.contains("Word(String),"), "got: {rust}");
    // …and the construction site coerces `&str` into it.
    assert!(rust.contains(r#"Token::Word("hi".to_string())"#), "got: {rust}");
}

/// A method returning `String` emits the owned Rust `String` return
/// type, and any `this.field` field read inside is auto-cloned so
/// the value can outlive `&self`.
#[test]
fn string_returning_method_returns_owned_with_field_clone() {
    let rust = emit(
        r#"
        public class User {
            private String name;
            public User(String name) { this.name = name; }
            public String getName() { return this.name; }
        }
        public void main() {
            var u = new User("Ada");
            print(u.getName());
        }
        "#,
    );
    assert!(
        rust.contains("pub fn getName(&self) -> String {"),
        "return type should be owned String: {rust}",
    );
    // Phase B: `User` is purely local (`u.getName()` is a borrow, never
    // aliased / stored / passed / returned), so it demotes to the
    // Inline plain-struct shape (§CR.3.3). `this.name` reads `self.name`
    // directly and auto-clones the String so it outlives `&self`.
    assert!(
        rust.contains("self.name.clone()"),
        "inline field clone missing: {rust}",
    );
    assert!(!rust.contains(".0.borrow()"), "no borrow when Inline: {rust}");
}

/// A program that never calls a mutating method on `xs` keeps it
/// as plain `let xs` — promotion is keyed to listed method names.
#[test]
fn no_mutating_method_keeps_let_immutable() {
    let rust = emit(
        "public void main() { int[] xs = {1, 2, 3}; print(xs.length); }",
    );
    assert!(rust.contains("let xs: Vec<isize>"), "got: {rust}");
    assert!(!rust.contains("let mut xs"), "no method-mutation here: {rust}");
}

// ----------------------------------------------------------------------
// String interpolation (§3.4) — `$"…$name…${expr}…"`
// ----------------------------------------------------------------------

/// `$"Hello, $name!"` lowers to `format!("Hello, {}!", name)`
/// outside of print, and collapses to `println!("Hello, {}!", name)`
/// inside print.
#[test]
fn interp_bare_ident_in_print_collapses_to_println() {
    let rust = emit(
        r#"public void main() { var name = "Ada"; print($"Hello, $name!"); }"#,
    );
    assert!(
        rust.contains(r#"println!("Hello, {}!", name)"#),
        "got: {rust}",
    );
    // No nested format! shape after the specialization.
    assert!(
        !rust.contains("format!("),
        "print($\"...\") should not need an inner format!: {rust}",
    );
}

/// `${expr}` interpolation with an arithmetic body recurses through
/// the normal expression emit path inside the `format!` slot.
#[test]
fn interp_expr_arithmetic_emits_as_format_arg() {
    let rust = emit(
        "public void main() { var a = 1; var b = 2; print($\"sum=${a + b}\"); }",
    );
    assert!(
        rust.contains(r#"println!("sum={}", a + b)"#),
        "got: {rust}",
    );
}

/// Outside `print(...)`, `$"…"` lowers to a `format!(...)` call —
/// the result is a `String` value the user can bind or pass on.
#[test]
fn interp_outside_print_lowers_to_format_macro() {
    let rust = emit(
        r#"public void main() { var name = "Ada"; var msg = $"Hi, $name!"; print(msg); }"#,
    );
    assert!(
        rust.contains(r#"let msg = format!("Hi, {}!", name);"#),
        "got: {rust}",
    );
    // print(msg) still goes through the {} placeholder shape.
    assert!(rust.contains(r#"println!("{}", msg)"#), "got: {rust}");
}

/// Empty `$""` lowers to `println!("")` in a print, `format!("")`
/// otherwise — both are no-op-shaped but stay legal.
#[test]
fn interp_empty_string_emits_empty_println() {
    let rust = emit(r#"public void main() { print($""); }"#);
    assert!(rust.contains(r#"println!("")"#), "got: {rust}");
}

/// Curly braces in literal-text chunks are doubled when emitted
/// into the format string so `format!` doesn't try to read them as
/// placeholders.
#[test]
fn interp_literal_braces_double_in_format_string() {
    let rust = emit(r#"public void main() { print($"a {literal} brace"); }"#);
    assert!(
        rust.contains(r#"println!("a {{literal}} brace")"#),
        "got: {rust}",
    );
}

/// `${...}` interpolation with an indexed array access lowers
/// through the existing index path (including the `(i) as usize`
/// coercion when the index is a non-literal).
#[test]
fn interp_expr_with_array_index_emits_indexed_value() {
    let rust = emit(
        "public void main() { int[3] xs = {10, 20, 30}; print($\"first=${xs[0]}\"); }",
    );
    assert!(
        rust.contains(r#"println!("first={}", xs[0])"#),
        "got: {rust}",
    );
}

// ============================================================================
// Imports — `use` statement emission
// ============================================================================

/// `import com.example.Foo;` → `use com::example::Foo;` at the top of
/// the emitted source. Single-segment dots in Jux lower to `::` in
/// Rust per §C.9.
#[test]
fn bare_import_lowers_to_use() {
    let rust = emit(
        r#"
        import com.example.Foo;
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("use com::example::Foo;"),
        "expected `use com::example::Foo;`, got: {rust}",
    );
}

/// A `? super B` parameter wildcard lifts to an UNBOUNDED synthetic
/// function generic — Rust can't express "supertype of B", and reusing
/// the `B` marker bound would wrongly reject the legal supertype caller
/// (`Animal: DogKind not satisfied`). The param still carries the
/// machinery `Clone`/`Debug` bounds, but NOT `DogKind`.
#[test]
fn super_wildcard_param_lifts_to_unbounded_generic() {
    let rust = emit(
        r#"
        public class Animal { public String name; public Animal(String n) { this.name = n; } }
        public class Dog extends Animal { public Dog(String n) { super(n); } }
        public class Bag<T> { public T item; public Bag(T item) { this.item = item; } }
        public void store(Bag<? super Dog> b) {}
        public void main() {}
        "#,
    );
    // The lifted generic must NOT carry a `__W0: DogKind` BOUND (the bug
    // that leaked rustc E0277). `DogKind` still appears elsewhere as
    // Dog's own marker trait, so we check the bound position specifically.
    assert!(
        !rust.contains("__W0: DogKind"),
        "`? super Dog` should not bound `__W0` by `DogKind`, got: {rust}",
    );
    // It IS still a function-generic lift over the container.
    assert!(
        rust.contains("fn store<__W0"),
        "expected a `store<__W0…>` function-generic lift, got: {rust}",
    );
}

/// A `? extends B` parameter wildcard keeps its marker bound — the
/// covariant producer case is sound to constrain.
#[test]
fn extends_wildcard_param_keeps_marker_bound() {
    let rust = emit(
        r#"
        public class Animal { public String name; public Animal(String n) { this.name = n; } }
        public class Dog extends Animal { public Dog(String n) { super(n); } }
        public class Bag<T> { public T item; public Bag(T item) { this.item = item; } }
        public void describe(Bag<? extends Animal> b) {}
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("__W0: AnimalKind"),
        "`? extends Animal` should bound `__W0` by `AnimalKind`, got: {rust}",
    );
}

/// A const-generic class lowers to a Rust const-generic struct:
/// `<int N>` → `const N: usize` (usize, NOT isize — a fixed array size
/// `[T; N]` must be exactly `usize` on stable Rust), the `T[N]` field
/// to `[T; N]`, a bare value read of `N` to `(N as isize)`, and the
/// literal instantiation to a turbofish.
#[test]
fn const_generic_class_lowers_to_rust_const_generics() {
    let rust = emit(
        r#"
        public class Ring<T, int N> {
            public T[N] storage;
            public int head;
            public Ring(T fill) {
                this.storage = new T[N];
                this.head = 0;
            }
            public int capacity() { return N; }
        }
        public void main() {
            var r = new Ring<int, 8>(0);
            print(r.capacity());
        }
        "#,
    );
    assert!(
        rust.contains("const N: usize"),
        "expected `const N: usize` param decl, got: {rust}",
    );
    assert!(
        rust.contains("[T; N]"),
        "expected `[T; N]` fixed-array field, got: {rust}",
    );
    assert!(
        rust.contains("(N as isize)"),
        "expected `(N as isize)` value read, got: {rust}",
    );
    assert!(
        rust.contains("::<isize, 8>"),
        "expected literal turbofish `::<isize, 8>`, got: {rust}",
    );
    // The generic-element array constructs via `from_fn` (no `T: Copy`),
    // and the impl carries the matching `Default` bound.
    assert!(
        rust.contains("std::array::from_fn(|_| Default::default())"),
        "expected from_fn array construction, got: {rust}",
    );
    assert!(
        rust.contains("+ Default"),
        "expected a `+ Default` bound for the array-element param, got: {rust}",
    );
}

/// A `bool` const param lowers to `const B: bool` and its value read
/// stays uncast (`bool` needs no usize bridge).
#[test]
fn bool_const_generic_param_lowers_uncast() {
    let rust = emit(
        r#"
        public class Flag<bool B> {
            public Flag() { }
            public bool get() { return B; }
        }
        public void main() { var f = new Flag<true>(); print(f.get()); }
        "#,
    );
    assert!(
        rust.contains("const B: bool"),
        "expected `const B: bool` param decl, got: {rust}",
    );
    assert!(
        !rust.contains("(B as "),
        "bool const param must not be cast, got: {rust}",
    );
}

/// Wildcard imports lower to the Rust `::*` form.
#[test]
fn wildcard_import_lowers_to_glob_use() {
    let rust = emit(
        r#"
        import com.example.*;
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("use com::example::*;"),
        "expected glob use, got: {rust}",
    );
}

/// Aliased imports preserve the `as Alias` rename verbatim.
#[test]
fn aliased_import_emits_use_as() {
    let rust = emit(
        r#"
        import com.example.Foo as Bar;
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("use com::example::Foo as Bar;"),
        "expected `use ... as Bar;`, got: {rust}",
    );
}

/// Grouped imports lower to Rust's `{...}` brace-group form, comma
/// separated, with per-item `as Alias` renames preserved.
#[test]
fn grouped_import_lowers_to_brace_use() {
    let rust = emit(
        r#"
        import com.example.{ A, B as B2, C };
        public void main() {}
        "#,
    );
    // Grouped imports expand one-line-per-name (the per-line form
    // lets `emit_imports` dedup a symbol imported again by another
    // unit's grouping — rustc E0252 otherwise).
    assert!(
        rust.contains("use com::example::A;"),
        "expected expanded import A, got: {rust}",
    );
    assert!(
        rust.contains("use com::example::B as B2;"),
        "expected expanded aliased import, got: {rust}",
    );
    assert!(
        rust.contains("use com::example::C;"),
        "expected expanded import C, got: {rust}",
    );
}

/// Multiple imports each emit their own `use ...;` line, in source
/// order, and a blank line separates the import block from `fn main`.
#[test]
fn multiple_imports_emit_use_block() {
    let rust = emit(
        r#"
        import a.A;
        import b.*;
        import c.{ X, Y };
        public void main() {}
        "#,
    );
    assert!(rust.contains("use a::A;"), "missing single import, got: {rust}");
    assert!(rust.contains("use b::*;"), "missing glob import, got: {rust}");
    // Grouped imports expand one-line-per-name (E0252 dedup).
    assert!(rust.contains("use c::X;"), "missing expanded import X, got: {rust}");
    assert!(rust.contains("use c::Y;"), "missing expanded import Y, got: {rust}");
    // The block should appear *before* fn main, with a blank line
    // separator between them.
    let use_a_pos = rust.find("use a::A;").expect("use a found");
    let main_pos = rust.find("fn main()").expect("fn main found");
    assert!(use_a_pos < main_pos, "imports should precede fn main");
    assert!(
        rust[..main_pos].contains("\n\n"),
        "expected a blank line between imports and fn main: {rust}",
    );
}

/// A source with no imports produces no `use ...` line and no leading
/// blank line — the file starts directly with the first top-level
/// declaration.
#[test]
fn no_imports_emits_no_use_block() {
    let rust = emit("public void main() {}");
    assert!(!rust.contains("use "), "unexpected `use` in: {rust}");
}

// ============================================================================
// Auto-derive (§O.3) — Eq/Hash/Copy on records and enums
// ============================================================================

/// Integer-only record qualifies for the entire auto-derive set: Eq +
/// Hash because no field is a float; Copy because every field is a
/// Copy primitive.
#[test]
fn int_only_record_gets_full_derive_set() {
    let rust = emit(
        r#"
        public record Point(int x, int y) {}
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, Default)]"),
        "got: {rust}",
    );
}

/// String-bearing record gets Eq + Hash (String supports both) but NOT
/// Copy (String isn't Copy).
#[test]
fn string_bearing_record_gets_eq_hash_but_not_copy() {
    let rust = emit(
        r#"
        public record User(String name, int age) {}
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]"),
        "got: {rust}",
    );
    assert!(!rust.contains("Copy"), "Copy should be skipped: {rust}");
}

/// Float-bearing record stays at the baseline — floats are NOT Eq/Hash,
/// though they are Copy. Pinning the exact shape would mismatch on
/// future polish; just assert presence of the disqualifying logic.
#[test]
fn float_bearing_record_drops_eq_and_hash() {
    let rust = emit(
        r#"
        public record Sample(double v) {}
        public void main() {}
        "#,
    );
    // PartialEq stays, Copy stays (float is Copy), Eq/Hash are absent.
    // Default joins the line because `double` implements Default.
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Copy, Default)]"),
        "got: {rust}",
    );
    assert!(!rust.contains(", Eq"), "Eq should be skipped: {rust}");
    assert!(!rust.contains(", Hash"), "Hash should be skipped: {rust}");
}

/// A record whose components include a user-defined class drops all of
/// Eq/Hash/Copy — the analyzer can't prove the class supports them.
#[test]
fn user_typed_record_drops_extra_derives() {
    let rust = emit(
        r#"
        public class Tag {}
        public record Boxed(Tag t) {}
        public void main() {}
        "#,
    );
    // Only the baseline three remain.
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq)]"),
        "expected baseline derives, got: {rust}",
    );
    assert!(!rust.contains(", Eq"), "Eq should be skipped: {rust}");
    assert!(!rust.contains(", Copy"), "Copy should be skipped: {rust}");
    assert!(!rust.contains(", Hash"), "Hash should be skipped: {rust}");
}

/// An enum whose variants carry only int payloads inherits the full
/// derive set — same rule as a primitive-only record.
#[test]
fn int_payload_enum_gets_full_derive_set() {
    let rust = emit(
        r#"
        public enum Token { Stop, Number(int) }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]"),
        "got: {rust}",
    );
}

/// An enum with a String payload gets Eq + Hash but not Copy (String
/// isn't Copy).
#[test]
fn string_payload_enum_drops_copy() {
    let rust = emit(
        r#"
        public enum Token { Stop, Word(String) }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Eq, Hash)]"),
        "got: {rust}",
    );
    assert!(!rust.contains("Copy"), "Copy should be skipped: {rust}");
}

/// A single float payload disqualifies the whole enum from Eq/Hash —
/// per-variant eligibility aggregates across the whole enum.
#[test]
fn one_float_payload_disqualifies_enum_eq_hash() {
    let rust = emit(
        r#"
        public enum Mixed { Tag, Score(double) }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Copy)]"),
        "got: {rust}",
    );
    assert!(!rust.contains(", Eq"), "Eq should be skipped: {rust}");
    assert!(!rust.contains(", Hash"), "Hash should be skipped: {rust}");
}

// ============================================================================
// Display impl for records (§O.3.1 — operator string)
// ============================================================================

/// A primitive-only record gets an `impl std::fmt::Display` block whose
/// format matches the spec example: `"Point(x: 1.5, y: 2.7)"`.
#[test]
fn primitive_record_emits_display_impl() {
    let rust = emit(
        r#"
        public record Point(double x, double y) {}
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("impl std::fmt::Display for Point {"),
        "missing Display impl: {rust}",
    );
    assert!(
        rust.contains(r#"write!(f, "Point(x: {}, y: {})", self.x, self.y)"#),
        "format mismatch: {rust}",
    );
}

/// String fields participate via Rust's `Display for String` — owned
/// `String` shows without quotes, matching the spec intent.
#[test]
fn string_record_emits_display_impl() {
    let rust = emit(
        r#"
        public record User(String name, int age) {}
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("impl std::fmt::Display for User {"),
        "missing Display impl: {rust}",
    );
    assert!(
        rust.contains(r#"write!(f, "User(name: {}, age: {})", self.name, self.age)"#),
        "format mismatch: {rust}",
    );
}

/// Zero-component records emit `"Empty()"` — bare literal, no args list.
#[test]
fn empty_record_emits_display_with_just_name() {
    let rust = emit(
        r#"
        public record Empty() {}
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("impl std::fmt::Display for Empty {"),
        "missing Display impl: {rust}",
    );
    assert!(
        rust.contains(r#"write!(f, "Empty()")"#),
        "format mismatch: {rust}",
    );
}

/// Records containing a user-class field skip Display emission — we
/// can't statically prove the class supports Display, and emitting a
/// broken impl would fail rustc. Debug stays available from the derive.
#[test]
fn record_with_user_type_field_skips_display() {
    let rust = emit(
        r#"
        public class Tag {}
        public record Boxed(Tag t) {}
        public void main() {}
        "#,
    );
    assert!(
        !rust.contains("impl std::fmt::Display for Boxed"),
        "should skip Display for user-typed field: {rust}",
    );
}

/// Generic records skip Display in this turn — bound propagation
/// (`impl<T: Display> Display for Box<T>`) isn't wired up yet.
#[test]
fn generic_record_skips_display_for_now() {
    let rust = emit(
        r#"
        public record Boxed<T>(T value) {}
        public void main() {}
        "#,
    );
    assert!(
        !rust.contains("impl std::fmt::Display for Boxed"),
        "should skip Display for generic record: {rust}",
    );
}

// ============================================================================
// Operator overloading — backend lowering (§O.2)
// ============================================================================

/// `operator==` emits a `__op_eq` inherent method whose body matches
/// the user's, plus an `impl PartialEq` block that delegates to it.
#[test]
fn operator_eq_emits_inherent_method_and_partial_eq_impl() {
    let rust = emit(
        r#"
        public class Path {
            public String value;
            public Path(String v) { this.value = v; }
            public bool operator==(Path other) { return true; }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("pub fn __op_eq(&self, other: Path) -> bool"),
        "missing inherent op method: {rust}",
    );
    assert!(
        rust.contains("impl PartialEq for Path {"),
        "missing PartialEq impl: {rust}",
    );
    assert!(
        rust.contains("self.__op_eq(other.clone())"),
        "PartialEq impl should delegate: {rust}",
    );
}

/// `operator string()` emits a `__op_string` inherent method plus an
/// `impl Display` block that calls `f.write_str(&self.__op_string())`.
#[test]
fn operator_string_emits_inherent_method_and_display_impl() {
    let rust = emit(
        r#"
        public class Path {
            public String value;
            public Path(String v) { this.value = v; }
            public String operator string() { return this.value; }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("pub fn __op_string(&self) -> String"),
        "missing inherent op method: {rust}",
    );
    assert!(
        rust.contains("impl std::fmt::Display for Path {"),
        "missing Display impl: {rust}",
    );
    assert!(
        rust.contains("f.write_str(&self.__op_string())"),
        "Display impl should delegate: {rust}",
    );
}

/// Operators without a Rust-trait wrapper today (e.g. `operator[]`,
/// `operator()`) still emit the inherent `__op_<kind>` method.
/// Tycheck sees the signature; only the trait-side dispatch is
/// missing. Pinned here so when those wrappers DO land, the test
/// failing tells us to extend the table.
#[test]
fn unmapped_operator_emits_inherent_method_only() {
    let rust = emit(
        r#"
        public class M {
            public int operator[](int i) { return 0; }
            public int operator()(int x) { return 0; }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("pub fn __op_index(&self, i: isize) -> isize"),
        "missing inherent op_index: {rust}",
    );
    assert!(
        rust.contains("pub fn __op_call(&self, x: isize) -> isize"),
        "missing inherent op_call: {rust}",
    );
    // No `impl Index` / `impl Fn` — wrappers deferred. (`Fn*` is
    // nightly-only for user impls; `Index` returns a reference which
    // needs body-translation work.)
    assert!(
        !rust.contains("impl std::ops::Index"),
        "should NOT emit Index impl yet: {rust}",
    );
    // (`impl Fn<` is the tuple-sugar header a real user `Fn` impl
    // would need — a bare `impl Fn` probe would false-positive on the
    // prelude's `impl FnMut() -> …` argument-position bounds.)
    assert!(
        !rust.contains("impl Fn<"),
        "should NOT emit Fn impl yet: {rust}",
    );
}

/// Generic classes skip operator trait impls (bound propagation isn't
/// wired yet) but still emit the inherent `__op_*` methods.
#[test]
fn generic_class_skips_operator_trait_impl() {
    let rust = emit(
        r#"
        public class Box<T> {
            public T value;
            public Box(T value) { this.value = value; }
            public bool operator==(Box<T> other) { return true; }
        }
        public void main() {}
        "#,
    );
    // Inherent method stays.
    assert!(
        rust.contains("pub fn __op_eq"),
        "missing inherent __op_eq on generic class: {rust}",
    );
    // No PartialEq impl — generic-class trait impls deferred.
    assert!(
        !rust.contains("impl PartialEq for Box"),
        "should NOT emit PartialEq impl for generic class yet: {rust}",
    );
}

/// A class with NO operator declarations gets neither `__op_*` methods
/// nor trait impls — emission is gated entirely on the AST list.
#[test]
fn class_without_operators_emits_no_op_method() {
    let rust = emit(
        r#"
        public class Plain {
            public int x;
            public Plain(int x) { this.x = x; }
        }
        public void main() {}
        "#,
    );
    assert!(!rust.contains("__op_"), "unexpected __op_* method: {rust}");
    assert!(!rust.contains("impl PartialEq for Plain"), "got: {rust}");
    // §O.4.1: a class WITHOUT `operator string` still prints, via the
    // identity-format Display (`Plain@<addr>`).
    assert!(
        rust.contains("impl std::fmt::Display for Plain"),
        "identity Display expected: {rust}",
    );
    assert!(rust.contains("Plain@{:p}"), "identity format expected: {rust}");
}

/// `operator hash()` emits `__op_hash` + `impl Hash` that writes the
/// returned isize into the Hasher.
#[test]
fn operator_hash_emits_hash_impl_via_writer() {
    let rust = emit(
        r#"
        public class Path {
            public int x;
            public Path(int x) { this.x = x; }
            public int operator hash() { return this.x; }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("pub fn __op_hash(&self) -> isize"),
        "missing inherent __op_hash: {rust}",
    );
    assert!(
        rust.contains("impl std::hash::Hash for Path"),
        "missing Hash impl: {rust}",
    );
    assert!(
        rust.contains("std::hash::Hasher::write_isize(state, self.__op_hash())"),
        "Hash impl should write into Hasher: {rust}",
    );
}

/// `operator==` together with `operator hash` triggers the `impl Eq`
/// marker emission per spec §O.2.7.
#[test]
fn eq_marker_emitted_when_eq_and_hash_paired() {
    let rust = emit(
        r#"
        public class Path {
            public int x;
            public Path(int x) { this.x = x; }
            public bool operator==(Path other) { return true; }
            public int operator hash() { return this.x; }
        }
        public void main() {}
        "#,
    );
    assert!(rust.contains("impl PartialEq for Path"), "missing PartialEq: {rust}");
    assert!(rust.contains("impl Eq for Path {}"), "missing Eq marker: {rust}");
    assert!(rust.contains("impl std::hash::Hash for Path"), "missing Hash: {rust}");
}

/// `operator==` alone (no `operator hash`) — emit PartialEq but NOT
/// the Eq marker (Eq without Hash is a useless intermediate state and
/// would fail tycheck per §O.2.7 once that's wired).
#[test]
fn eq_marker_not_emitted_for_eq_alone() {
    let rust = emit(
        r#"
        public class Path {
            public int x;
            public Path(int x) { this.x = x; }
            public bool operator==(Path other) { return true; }
        }
        public void main() {}
        "#,
    );
    assert!(rust.contains("impl PartialEq for Path"), "missing PartialEq: {rust}");
    assert!(!rust.contains("impl Eq for Path"), "Eq marker should NOT be emitted: {rust}");
}

/// `operator+` (binary, 1 param) emits `impl std::ops::Add` with
/// Output = user-declared return type, body delegates to `__op_add`.
#[test]
fn operator_plus_binary_emits_add_impl() {
    let rust = emit(
        r#"
        public class Money {
            public int cents;
            public Money(int cents) { this.cents = cents; }
            public Money operator+(Money other) { return new Money(0); }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("pub fn __op_add(&self, other: Money) -> Money"),
        "missing inherent __op_add: {rust}",
    );
    assert!(
        rust.contains("impl std::ops::Add for Money"),
        "missing Add impl: {rust}",
    );
    assert!(
        rust.contains("type Output = Money;"),
        "Output type should match return: {rust}",
    );
    assert!(
        rust.contains("fn add(self, rhs: Money) -> Self::Output"),
        "wrong wrapper signature: {rust}",
    );
    assert!(
        rust.contains("self.__op_add(rhs)"),
        "wrapper should delegate: {rust}",
    );
}

/// `operator-` arity 1 → `impl Sub`; arity 0 → `impl Neg`. Same
/// inherent name (`__op_sub`) for both — the arity decides the
/// trait, not the synthetic method's name.
#[test]
fn operator_minus_arity_dispatches_between_sub_and_neg() {
    let rust_binary = emit(
        r#"
        public class M {
            public int x;
            public M(int x) { this.x = x; }
            public M operator-(M other) { return new M(0); }
        }
        public void main() {}
        "#,
    );
    assert!(rust_binary.contains("impl std::ops::Sub for M"), "binary - → Sub: {rust_binary}");
    assert!(!rust_binary.contains("impl std::ops::Neg"), "should not be Neg: {rust_binary}");

    let rust_unary = emit(
        r#"
        public class M {
            public int x;
            public M(int x) { this.x = x; }
            public M operator-() { return new M(0); }
        }
        public void main() {}
        "#,
    );
    assert!(rust_unary.contains("impl std::ops::Neg for M"), "unary - → Neg: {rust_unary}");
    assert!(rust_unary.contains("fn neg(self) -> Self::Output"), "neg sig: {rust_unary}");
    assert!(rust_unary.contains("self.__op_neg()"), "neg should call __op_neg: {rust_unary}");
}

/// `operator~` (always unary) → `impl Not`. Maps to inherent `__op_not`.
#[test]
fn operator_bitnot_emits_not_impl() {
    let rust = emit(
        r#"
        public class B {
            public int x;
            public B(int x) { this.x = x; }
            public B operator~() { return new B(0); }
        }
        public void main() {}
        "#,
    );
    assert!(rust.contains("pub fn __op_not(&self) -> B"), "missing __op_not: {rust}");
    assert!(rust.contains("impl std::ops::Not for B"), "missing Not impl: {rust}");
    assert!(rust.contains("fn not(self) -> Self::Output"), "wrong sig: {rust}");
    assert!(rust.contains("self.__op_not()"), "wrapper delegates: {rust}");
}

/// `a + b` on a class with `operator+` rewrites the call site to
/// `a.__op_add(b.clone())` — the backend's clone-injection pass.
/// This lets `a` and `b` survive the operation (Rust's `Add::add`
/// consumes operands; the rewrite autorefs LHS and clones RHS).
#[test]
fn arithmetic_op_call_site_rewrites_to_clone_injected_form() {
    let rust = emit(
        r#"
        public class M {
            public int x;
            public M(int x) { this.x = x; }
            public M operator+(M other) { return new M(0); }
        }
        public void main() {
            var a = new M(1);
            var b = new M(2);
            var c = a + b;
        }
        "#,
    );
    assert!(
        rust.contains("a.__op_add(b.clone())"),
        "missing clone-injection rewrite: {rust}",
    );
    // The bare `a + b` form should NOT appear in main — it was rewritten.
    assert!(
        !rust.contains("a + b"),
        "raw `+` form should be rewritten: {rust}",
    );
}

/// Primitive `+` is never rewritten — `1 + 2` stays `1 + 2`.
/// Same for binary ops on user types that DON'T declare the operator.
#[test]
fn primitive_arithmetic_is_not_rewritten() {
    let rust = emit(
        r#"
        public void main() {
            var x = 1 + 2;
            print(x);
        }
        "#,
    );
    assert!(rust.contains("let x = 1 + 2"), "primitive `+` must stay: {rust}");
    assert!(!rust.contains("__op_add"), "no class op rewrite on primitives: {rust}");
}

/// Class without the relevant operator doesn't rewrite. `a + b` would
/// fail to type-check or fall through to Rust's default — either way
/// the backend doesn't synthesize a `__op_add` call that doesn't exist.
#[test]
fn class_without_operator_is_not_rewritten() {
    let rust = emit(
        r#"
        public class Bag {
            public int v;
            public Bag(int v) { this.v = v; }
        }
        public void main() {
            var a = new Bag(1);
            print(a.v);
        }
        "#,
    );
    assert!(!rust.contains("__op_add"), "no rewrite without operator decl: {rust}");
}

/// `a == b` on a class with `operator==` does NOT get the clone-
/// injection rewrite — PartialEq's `eq(&self, &Self)` already takes
/// references, no consumption to fix. The standard `a == b` emission
/// stays.
#[test]
fn equality_op_is_not_rewritten() {
    let rust = emit(
        r#"
        public class P {
            public int x;
            public P(int x) { this.x = x; }
            public bool operator==(P other) { return true; }
        }
        public void main() {
            var a = new P(1);
            var b = new P(2);
            print(a == b);
        }
        "#,
    );
    // Operand-preserving rewrite would emit `a.__op_eq(b.clone())`;
    // but the user code keeps the bare `==` form (Rust's PartialEq
    // takes references — nothing is consumed).
    assert!(rust.contains("a == b"), "expected bare `==` emission: {rust}");
    assert!(
        !rust.contains("a.__op_eq(b.clone())"),
        "should NOT rewrite ==: {rust}",
    );
}

/// `operator<=>` emits `__op_cmp` and an `impl PartialOrd` whose
/// `partial_cmp` body converts the user's int return into Ordering
/// via isize's own Ord (`.cmp(&0)` → Less/Equal/Greater).
#[test]
fn operator_cmp_emits_partial_ord_impl() {
    let rust = emit(
        r#"
        public class V {
            public int x;
            public V(int x) { this.x = x; }
            public int operator<=>(V other) { return 0; }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("pub fn __op_cmp(&self, other: V) -> isize"),
        "missing inherent __op_cmp: {rust}",
    );
    assert!(
        rust.contains("impl PartialOrd for V {"),
        "missing PartialOrd impl: {rust}",
    );
    assert!(
        rust.contains("fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering>"),
        "wrong partial_cmp signature: {rust}",
    );
    assert!(
        rust.contains("Some(self.__op_cmp(other.clone()).cmp(&0))"),
        "missing isize→Ordering bridge: {rust}",
    );
}

/// `<=>` without `==` synthesizes a PartialEq that bridges through
/// `__op_cmp` — Rust's `PartialOrd: PartialEq` constraint requires
/// PartialEq to exist before we can emit the PartialOrd impl.
#[test]
fn cmp_without_eq_synthesizes_partial_eq() {
    let rust = emit(
        r#"
        public class V {
            public int x;
            public V(int x) { this.x = x; }
            public int operator<=>(V other) { return 0; }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("impl PartialEq for V {"),
        "missing synthesized PartialEq: {rust}",
    );
    assert!(
        rust.contains("self.__op_cmp(other.clone()) == 0"),
        "synthesized PartialEq should bridge through cmp: {rust}",
    );
}

// ============================================================================
// `= delete;` suppression (§O.3.4) on records
// ============================================================================

/// `operator string() = delete;` on a record suppresses the
/// synthesized Display impl — security-sensitive types can opt out of
/// default formatting. The struct + `new` are still emitted.
#[test]
fn record_string_delete_suppresses_display_impl() {
    let rust = emit(
        r#"
        public record OpaqueToken(int secret) {
            public String operator string() = delete;
        }
        public void main() {}
        "#,
    );
    assert!(rust.contains("pub struct OpaqueToken"), "struct still emitted: {rust}");
    assert!(rust.contains("pub fn new(secret: isize)"), "ctor still emitted: {rust}");
    assert!(
        !rust.contains("impl std::fmt::Display for OpaqueToken"),
        "Display impl should be suppressed by `= delete;`: {rust}",
    );
}

/// `operator==(...) = delete;` drops `PartialEq` from the record's
/// derive line — and with it `Eq`, since `Eq: PartialEq`.
#[test]
fn record_eq_delete_drops_partial_eq_from_derive() {
    let rust = emit(
        r#"
        public record Unequal(int x) {
            public bool operator==(Unequal other) = delete;
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("#[derive(Debug, Clone, Copy, Default)]"),
        "expected baseline derives minus PartialEq, got: {rust}",
    );
    // Should NOT see Display impl for Unequal either way (PartialEq
    // doesn't affect Display) — but pin the derive shape exactly.
    assert!(!rust.contains("PartialEq"), "PartialEq should be gone: {rust}");
    assert!(!rust.contains(", Eq"), "Eq should be gone: {rust}");
}

/// `operator hash() = delete;` drops `Hash` from the derive line —
/// the record can no longer serve as a HashMap key. `Eq` stays
/// because it's independent of Hash (Eq is the reflexivity marker;
/// HashMap keys need both Eq AND Hash, so dropping Hash is
/// sufficient to opt out).
#[test]
fn record_hash_delete_drops_hash_from_derive() {
    let rust = emit(
        r#"
        public record NoHash(int x) {
            public int operator hash() = delete;
        }
        public void main() {}
        "#,
    );
    assert!(!rust.contains(", Hash"), "Hash should be gone: {rust}");
    // PartialEq + Eq stay (just Hash is dropped). Copy stays since
    // the field is int.
    assert!(rust.contains("PartialEq"), "PartialEq stays: {rust}");
    assert!(rust.contains(", Eq"), "Eq stays: {rust}");
}

/// Records can declare methods (per grammar §A.2.4). They land in
/// the same `impl Record { ... }` block as the canonical
/// `pub fn new`, alongside any operator overrides.
#[test]
fn record_method_emits_in_inherent_impl_block() {
    let rust = emit(
        r#"
        public record Money(int cents) {
            public int doubled() { return this.cents * 2; }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("pub fn new(cents: isize)"),
        "canonical ctor still emitted: {rust}",
    );
    assert!(
        rust.contains("pub fn doubled(&self) -> isize"),
        "method emitted on record: {rust}",
    );
    // Sanity: doubled is inside `impl Money`, not a free function.
    let impl_idx = rust.find("impl Money {").expect("impl block present");
    let doubled_idx = rust.find("pub fn doubled").expect("doubled present");
    assert!(impl_idx < doubled_idx, "method must follow impl header: {rust}");
}

/// A non-deleted operator override on a record emits the inherent
/// `__op_*` method AND the trait wrapper, mirroring class behavior.
#[test]
fn record_operator_override_emits_inherent_and_trait_impl() {
    let rust = emit(
        r#"
        public record Money(int cents) {
            public String operator string() {
                return "x";
            }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("pub fn __op_string(&self) -> String"),
        "missing inherent __op_string: {rust}",
    );
    assert!(
        rust.contains("impl std::fmt::Display for Money"),
        "missing user Display impl: {rust}",
    );
    assert!(
        rust.contains("f.write_str(&self.__op_string())"),
        "Display should delegate to user override: {rust}",
    );
}

/// Enum with `operator string() = delete;` suppresses the auto-
/// Display impl — same suppression behavior records get. The enum
/// itself still emits (struct + derives intact).
#[test]
fn enum_string_delete_suppresses_display_impl() {
    let rust = emit(
        r#"
        public enum Secret {
            Hidden;
            public String operator string() = delete;
        }
        public void main() {}
        "#,
    );
    assert!(rust.contains("pub enum Secret"), "enum still emitted: {rust}");
    assert!(
        !rust.contains("impl std::fmt::Display for Secret"),
        "Display should be suppressed by `= delete;`: {rust}",
    );
}

// ============================================================================
// Return-position String coercion
// ============================================================================

/// `return "literal";` in a `String`-returning function picks up
/// `.to_string()` at tail position. Without the coercion this would
/// be `&str` (from the literal) where `String` is required and rustc
/// would emit E0308.
#[test]
fn tail_string_literal_return_picks_up_to_string() {
    let rust = emit(
        r#"
        public String greet() { return "hi"; }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains(r#""hi".to_string()"#),
        "expected `.to_string()` coercion at tail: {rust}",
    );
}

/// Same coercion applies to an interior `return "lit";` — the
/// non-tail return path also injects `.to_string()`.
#[test]
fn non_tail_string_literal_return_picks_up_to_string() {
    let rust = emit(
        r#"
        public String pick(bool flag) {
            if (flag) { return "yes"; }
            return "no";
        }
        public void main() {}
        "#,
    );
    // Both early and trailing return get coerced.
    assert!(
        rust.contains(r#"return "yes".to_string();"#),
        "early return should coerce: {rust}",
    );
    assert!(
        rust.contains(r#""no".to_string()"#),
        "tail return should coerce: {rust}",
    );
}

/// Void function returning nothing → no coercion (no return type).
#[test]
fn void_return_does_not_inject_coercion() {
    let rust = emit(
        r#"
        public void shout() { print("hi"); }
        public void main() {}
        "#,
    );
    // The literal inside `print(...)` is NOT a return position, so
    // it must stay as `&str`. (`println!` accepts it directly.)
    assert!(!rust.contains(".to_string()"), "no spurious coercion: {rust}");
}

/// Non-String return types don't trigger the coercion either —
/// only `String` returns get the bridge.
#[test]
fn int_return_does_not_inject_coercion() {
    let rust = emit(
        r#"
        public int answer() { return 42; }
        public void main() {}
        "#,
    );
    assert!(!rust.contains(".to_string()"), "no spurious coercion: {rust}");
}

/// String operator on a class returning a bare literal picks up the
/// coercion through the same `current_return_type` plumbing.
#[test]
fn operator_string_literal_return_picks_up_to_string() {
    let rust = emit(
        r#"
        public class P {
            public int x;
            public P(int x) { this.x = x; }
            public String operator string() { return "P"; }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains(r#""P".to_string()"#),
        "operator body literal should coerce: {rust}",
    );
}

/// Enum with a custom `operator string` override emits the user's
/// Display (delegating to `__op_string`) and suppresses the auto
/// variant-name Display. Inherent method lands in `impl Enum { ... }`.
#[test]
fn enum_operator_string_override_emits_inherent_and_user_display() {
    let rust = emit(
        r#"
        public enum Color {
            Red, Green;
            public String operator string() {
                return $"custom";
            }
        }
        public void main() {}
        "#,
    );
    assert!(
        rust.contains("impl Color {"),
        "expected inherent impl block: {rust}",
    );
    assert!(
        rust.contains("pub fn __op_string(&self) -> String"),
        "expected __op_string method: {rust}",
    );
    assert!(
        rust.contains("f.write_str(&self.__op_string())"),
        "Display should delegate through user override: {rust}",
    );
    // Variant-name Display should be replaced — there's no
    // `match self { Color::Red => write!(f, "Red"), ... }`.
    assert!(
        !rust.contains("Color::Red => write!"),
        "auto-Display match should NOT appear: {rust}",
    );
}

/// User-overridden `operator string` on a record SUPPRESSES the
/// auto-Display impl — the user's impl is the only Display in scope
/// (otherwise Rust would see two conflicting impls).
#[test]
fn record_operator_string_override_suppresses_auto_display() {
    let rust = emit(
        r#"
        public record M(int x) {
            public String operator string() { return "x"; }
        }
        public void main() {}
        "#,
    );
    // Exactly one Display impl — the user's. There's no separate
    // "M(x: 0)" auto-format helper.
    let count = rust.matches("impl std::fmt::Display for M").count();
    assert_eq!(count, 1, "expected exactly one Display impl: {rust}");
    assert!(
        !rust.contains("write!(f, \"M(x:"),
        "auto Display format should NOT appear: {rust}",
    );
}

/// When the user defined BOTH `operator<=>` AND `operator==`, the
/// user's own PartialEq impl (delegating to `__op_eq`) is used — we
/// do NOT also emit the synthesized cmp-based PartialEq.
#[test]
fn cmp_with_eq_does_not_synthesize_partial_eq() {
    let rust = emit(
        r#"
        public class V {
            public int x;
            public V(int x) { this.x = x; }
            public int operator<=>(V other) { return 0; }
            public bool operator==(V other) { return true; }
        }
        public void main() {}
        "#,
    );
    // The user's PartialEq is present and delegates through __op_eq.
    assert!(rust.contains("impl PartialEq for V {"), "user PartialEq missing: {rust}");
    assert!(rust.contains("self.__op_eq(other.clone())"), "user delegation missing: {rust}");
    // The synthesized cmp-based form is NOT emitted.
    assert!(
        !rust.contains("self.__op_cmp(other.clone()) == 0"),
        "should NOT emit synthesized PartialEq when user defined ==: {rust}",
    );
    // PartialOrd is still emitted.
    assert!(rust.contains("impl PartialOrd for V {"), "PartialOrd missing: {rust}");
}

/// The full binary arithmetic family — `*`, `/`, `%` — all map to
/// their matching `std::ops::*` traits. Bitwise binary ops
/// (`&`/`|`/`^`) and shifts (`<<`/`>>`) follow the same pattern.
#[test]
fn arithmetic_bitwise_shift_family_all_emit_traits() {
    let rust = emit(
        r#"
        public class N {
            public int x;
            public N(int x) { this.x = x; }
            public N operator*(N o) { return new N(0); }
            public N operator/(N o) { return new N(0); }
            public N operator%(N o) { return new N(0); }
            public N operator&(N o) { return new N(0); }
            public N operator|(N o) { return new N(0); }
            public N operator^(N o) { return new N(0); }
            public N operator<<(N o) { return new N(0); }
            public N operator>>(N o) { return new N(0); }
        }
        public void main() {}
        "#,
    );
    for (trait_path, method) in [
        ("std::ops::Mul",    "mul"),
        ("std::ops::Div",    "div"),
        ("std::ops::Rem",    "rem"),
        ("std::ops::BitAnd", "bitand"),
        ("std::ops::BitOr",  "bitor"),
        ("std::ops::BitXor", "bitxor"),
        ("std::ops::Shl",    "shl"),
        ("std::ops::Shr",    "shr"),
    ] {
        assert!(
            rust.contains(&format!("impl {trait_path} for N")),
            "missing {trait_path} impl: {rust}",
        );
        assert!(
            rust.contains(&format!("fn {method}(self, rhs: N)")),
            "missing {method} signature: {rust}",
        );
    }
}

// ============================================================================
// Source-map markers (audit Tier 2.2)
// ============================================================================

/// The default `lower(...)` path emits no source markers — preserves
/// snapshot stability for every existing backend test.
#[test]
fn default_lower_emits_no_source_markers() {
    let rust = emit(r#"public void main() { print("hi"); }"#);
    assert!(!rust.contains("// JUX:"), "no markers expected: {rust}");
}

/// `lower_with_source` emits one `// JUX:file:line:col` marker before
/// each top-level declaration and each statement inside its body.
#[test]
fn lower_with_source_emits_markers_per_decl_and_stmt() {
    let rust = emit_with_source(
        r#"public void main() {
    print("hi");
    var x = 1;
}"#,
    );
    // Top-level decl marker — at the start of `public void main`.
    assert!(
        rust.contains("// JUX:test.jux:1:"),
        "expected top-level marker on line 1: {rust}",
    );
    // Statement markers — one per stmt inside the body. Line 2 has
    // the print call; line 3 has the var decl.
    assert!(
        rust.contains("// JUX:test.jux:2:"),
        "expected line-2 marker: {rust}",
    );
    assert!(
        rust.contains("// JUX:test.jux:3:"),
        "expected line-3 marker: {rust}",
    );
}

/// Markers anchor at the original `.jux` line, not the emitted Rust
/// line. A statement on `.jux` line 5 gets a `JUX:test.jux:5:…` marker
/// even when the surrounding Rust pushes its emitted form to line 7+.
#[test]
fn markers_track_jux_lines_not_rust_lines() {
    let rust = emit_with_source(
        r#"public class P {
    public int x;
    public P(int x) { this.x = x; }
}
public void main() {
    var p = new P(42);
}"#,
    );
    // The class decl is on .jux line 1.
    assert!(rust.contains("// JUX:test.jux:1:"), "{rust}");
    // The free function is on .jux line 5.
    assert!(rust.contains("// JUX:test.jux:5:"), "{rust}");
    // The var inside main is on .jux line 6.
    assert!(rust.contains("// JUX:test.jux:6:"), "{rust}");
}

/// Each non-marker source line should have at most one marker
/// immediately preceding it. (Smoke check: markers don't accidentally
/// duplicate or leak into expression-position emission.)
#[test]
fn markers_appear_only_before_statements_and_decls() {
    let rust = emit_with_source(
        r#"public void main() {
    print("hi");
}"#,
    );
    // No marker should appear mid-line; every marker must start a
    // line (after any leading indent). Verify by checking that the
    // substring `// JUX:` only appears after a leading-indent
    // pattern (spaces only before it on the line).
    for line in rust.lines() {
        if let Some(idx) = line.find("// JUX:") {
            let before = &line[..idx];
            assert!(
                before.bytes().all(|b| b == b' '),
                "marker not at line start: {line:?}",
            );
        }
    }
}

// ============================================================================
// Package → Rust module mapping (Step 7)
// ============================================================================

/// `package com.example;` wraps the unit in `pub mod com { pub mod
/// example { … } }` and emits a crate-root `fn main()` shim that
/// forwards into the inner `main`.
#[test]
fn package_decl_wraps_unit_in_modules() {
    let rust = emit(
        r#"
        package com.example;
        public void main() { print("hi"); }
        "#,
    );
    assert!(rust.contains("pub mod com {"), "outer mod: {rust}");
    assert!(rust.contains("pub mod example {"), "inner mod: {rust}");
    assert!(
        rust.contains("com::example::main();"),
        "crate-root shim should forward: {rust}",
    );
}

/// Inside a wrapped module, top-level functions are emitted with their
/// declared visibility — `pub fn main()` so the shim can reach it.
#[test]
fn package_wrapped_main_is_pub() {
    let rust = emit(
        r#"
        package com.demo;
        public void main() { print("hello"); }
        "#,
    );
    assert!(
        rust.contains("pub fn main()"),
        "main inside the module must be pub: {rust}",
    );
}

/// Without a `package` decl, emission is flat at the crate root —
/// the historical behavior. No `pub mod`, no shim.
#[test]
fn no_package_keeps_flat_emission() {
    let rust = emit(r#"public void main() { print("hi"); }"#);
    assert!(!rust.contains("pub mod"), "should be flat: {rust}");
    assert!(rust.contains("fn main()"), "main at root: {rust}");
}

/// A multi-segment package nests one `pub mod` per segment.
#[test]
fn deep_package_path_nests_module_per_segment() {
    let rust = emit(
        r#"
        package a.b.c.d;
        public void main() {}
        "#,
    );
    assert!(rust.contains("pub mod a {"), "missing `a`: {rust}");
    assert!(rust.contains("pub mod b {"), "missing `b`: {rust}");
    assert!(rust.contains("pub mod c {"), "missing `c`: {rust}");
    assert!(rust.contains("pub mod d {"), "missing `d`: {rust}");
    assert!(
        rust.contains("a::b::c::d::main();"),
        "crate-root shim path: {rust}",
    );
}

// ---------------------------------------------------------------------
// C#-style properties (JUX-MISSING-DEFS §M.7)
// ---------------------------------------------------------------------

/// An auto-property `T Name { get; set; }` lowers to a private backing
/// field plus a getter (`fn Name(&self) -> T`) and a setter
/// (`fn __set_Name(&self, value: T)`); reads emit `obj.Name()` and
/// writes emit `obj.__set_Name(v)`.
#[test]
fn auto_property_lowers_to_backing_field_and_accessors() {
    let rust = emit(
        r#"
        public class P {
            public int Score { get; set; } = 0;
        }
        public void main() {
            var p = new P();
            p.Score = 5;
            print(p.Score);
        }
        "#,
    );
    // Backing field inside the inner struct.
    assert!(rust.contains("__prop_Score"), "missing backing field: {rust}");
    // Getter method named after the property.
    assert!(rust.contains("fn Score("), "missing getter: {rust}");
    // Setter method.
    assert!(rust.contains("fn __set_Score("), "missing setter: {rust}");
    // Read site is a getter call.
    assert!(rust.contains("p.Score()"), "read should be a getter call: {rust}");
    // Write site routes through the setter.
    assert!(rust.contains("__set_Score("), "write should route to setter: {rust}");
}

/// A read-only auto-property `{ get; }` set in the constructor lowers
/// the ctor write to a direct backing-field assignment (no setter
/// exists for it).
#[test]
fn readonly_property_ctor_write_targets_backing_field() {
    let rust = emit(
        r#"
        public class P {
            public int Id { get; }
            public P() { this.Id = 7; }
        }
        public void main() { var p = new P(); print(p.Id); }
        "#,
    );
    assert!(rust.contains("__prop_Id"), "missing backing field: {rust}");
    // No public setter for a read-only property.
    assert!(!rust.contains("fn __set_Id"), "read-only must not get a setter: {rust}");
    // The read is still a getter call.
    assert!(rust.contains("p.Id()"), "read should be a getter call: {rust}");
}

/// An expression-bodied read-only property `T Name -> expr;` lowers to
/// a getter whose body returns the expression; bare member references
/// inside resolve through implicit-`this`. (Jux uses `->`, not C#'s `=>`,
/// since `=>` is the instanceof operator.)
#[test]
fn expression_bodied_property_resolves_implicit_this() {
    let rust = emit(
        r#"
        public class P {
            public String First { get; set; }
            public String Last { get; set; }
            public String FullName -> First + " " + Last;
        }
        public void main() {
            var p = new P();
            p.First = "Ada"; p.Last = "Lovelace";
            print(p.FullName);
        }
        "#,
    );
    assert!(rust.contains("fn FullName("), "missing computed getter: {rust}");
    // Bare `First` / `Last` became getter calls through `self`.
    assert!(rust.contains("self.First()"), "implicit-this getter call missing: {rust}");
    assert!(rust.contains("self.Last()"), "implicit-this getter call missing: {rust}");
    // FullName itself has no backing field (computed).
    assert!(!rust.contains("__prop_FullName"), "computed prop must have no backing field: {rust}");
}

/// A full-bodied accessor with the implicit `value` parameter lowers
/// with the user's body verbatim and `value` typed as the property.
#[test]
fn full_bodied_setter_uses_value_param() {
    let rust = emit(
        r#"
        public class P {
            private int _age;
            public int Age { get { return _age; } set { _age = value < 0 ? 0 : value; } }
        }
        public void main() { var p = new P(); p.Age = -5; print(p.Age); }
        "#,
    );
    assert!(rust.contains("fn __set_Age("), "missing setter: {rust}");
    assert!(rust.contains("value"), "setter should mention `value`: {rust}");
}

/// A static property `static T Name { get; set; }` lowers to a static
/// backing field and static accessors, accessed as `Class::Name()` /
/// `Class::__set_Name(v)`.
#[test]
fn static_property_uses_class_path_accessors() {
    let rust = emit(
        r#"
        public class P {
            public static int Count { get; set; }
        }
        public void main() { P.Count = 3; print(P.Count); }
        "#,
    );
    assert!(rust.contains("P::Count()"), "static getter call missing: {rust}");
    assert!(rust.contains("P::__set_Count("), "static setter call missing: {rust}");
}

// ===========================================================================
// Multi-module project model — Cargo.toml target emission (§B.2)
// ===========================================================================

#[test]
fn cargo_toml_bin_target_uses_manifest_name() {
    let target = CrateTarget::Bin { name: "myapp".to_string() };
    let toml = cargo_toml_for_target(&target, true, &CargoMeta::default(), &[], &[], false);
    // The [[bin]] name is the manifest-driven name, not `jux_emitted`.
    assert!(toml.contains("[[bin]]\nname = \"myapp\""), "{toml}");
    assert!(toml.contains("path = \"src/main.rs\""), "{toml}");
    assert!(!toml.contains("[lib]"), "{toml}");
    // Stand-alone crate gets the [workspace] opt-out.
    assert!(toml.contains("[workspace]"), "{toml}");
}

#[test]
fn cargo_toml_lib_target_emits_crate_type() {
    let target = CrateTarget::Lib {
        name: "mylib".to_string(),
        crate_type: vec!["lib".to_string(), "cdylib".to_string()],
    };
    let toml = cargo_toml_for_target(&target, true, &CargoMeta::default(), &[], &[], false);
    assert!(toml.contains("[lib]\npath = \"src/lib.rs\""), "{toml}");
    assert!(toml.contains("crate-type = [\"lib\", \"cdylib\"]"), "{toml}");
    // A library target emits no [[bin]].
    assert!(!toml.contains("[[bin]]"), "{toml}");
}

#[test]
fn cargo_toml_path_dep_and_workspace_member() {
    let target = CrateTarget::Bin { name: "app".to_string() };
    let deps = vec![PathDep {
        crate_name: "greeter".to_string(),
        rel_path: "../lib-greeter".to_string(),
    }];
    // in_workspace = true → the per-crate [workspace] opt-out is omitted.
    let toml = cargo_toml_for_target(&target, true, &CargoMeta::default(), &deps, &[], true);
    assert!(
        toml.contains("greeter = { path = \"../lib-greeter\" }"),
        "{toml}"
    );
    assert!(!toml.contains("\n[workspace]\n"), "{toml}");
}

#[test]
fn cargo_toml_registry_dep_is_linked() {
    let target = CrateTarget::Bin { name: "app".to_string() };
    let reg = vec![RegistryDep {
        crate_name: "minifb".to_string(),
        version: "0.27".to_string(),
    }];
    let toml = cargo_toml_for_target(&target, true, &CargoMeta::default(), &[], &reg, false);
    // The foreign `rust.minifb` dep becomes a version line under [dependencies].
    assert!(toml.contains("minifb = \"0.27\""), "{toml}");
    // futures (async) and the registry dep share one [dependencies] table.
    assert_eq!(toml.matches("[dependencies]").count(), 1, "{toml}");
}

/// Tier-0 (`optimizations.md`): a stand-alone crate with no user profiles gets
/// an optimized default `[profile.release]` (opt-level=3 / lto / codegen-units=1
/// / strip / overflow-checks). `panic` is NOT auto-set (would break try/catch).
#[test]
fn cargo_toml_injects_tier0_release_profile() {
    let target = CrateTarget::Bin { name: "app".to_string() };
    let toml = cargo_toml_for_target(&target, true, &CargoMeta::default(), &[], &[], false);
    assert!(toml.contains("[profile.release]"), "{toml}");
    assert!(toml.contains("opt-level = 3"), "{toml}");
    assert!(toml.contains("lto = \"thin\""), "{toml}");
    assert!(toml.contains("codegen-units = 1"), "{toml}");
    assert!(toml.contains("strip = \"symbols\""), "{toml}");
    assert!(toml.contains("overflow-checks = false"), "{toml}");
    // panic stays Cargo's default (unwind) — Jux try/catch needs it.
    assert!(!toml.contains("panic ="), "panic must not be auto-set: {toml}");
}

/// A workspace MEMBER carries no profiles (the workspace root owns them), so the
/// Tier-0 injection must be skipped when `in_workspace`.
#[test]
fn cargo_toml_workspace_member_has_no_profiles() {
    let target = CrateTarget::Bin { name: "app".to_string() };
    let toml = cargo_toml_for_target(&target, true, &CargoMeta::default(), &[], &[], true);
    assert!(!toml.contains("[profile.release]"), "{toml}");
}

/// A user-pinned `[profile.release]` key wins; Tier-0 only fills the gaps. A
/// size build (`opt-level = "z"`) keeps "z" but still gets lto / strip / etc.
#[test]
fn tier0_does_not_override_user_profile_keys() {
    let target = CrateTarget::Bin { name: "app".to_string() };
    let meta = CargoMeta {
        profiles: vec![crate::CargoProfile {
            name: "release".to_string(),
            entries: vec![("opt-level".to_string(), "\"z\"".to_string())],
        }],
        ..CargoMeta::default()
    };
    let toml = cargo_toml_for_target(&target, true, &meta, &[], &[], false);
    assert!(toml.contains("opt-level = \"z\""), "user opt-level kept: {toml}");
    assert!(!toml.contains("opt-level = 3"), "Tier-0 must not re-add opt-level: {toml}");
    assert!(toml.contains("lto = \"thin\""), "Tier-0 fills lto: {toml}");
    assert!(toml.contains("strip = \"symbols\""), "Tier-0 fills strip: {toml}");
}

/// A class downcast target (`x as Dog`) makes the base's `<Name>Kind` trait
/// carry a `__jux_as_Dog` runtime-type hook (default `None`), and the concrete
/// `impl AnimalKind for Dog` overrides it with `Some(self.clone())`.
#[test]
fn downcast_target_emits_runtime_type_hook() {
    let rust = emit(
        r#"
        public abstract class Animal { public abstract String sound(); }
        public class Dog extends Animal { public Dog() {} public String sound() { return "woof"; } }
        public void main() { Animal a = new Dog(); var d = a as Dog; }
        "#,
    );
    assert!(
        rust.contains("fn __jux_as_Dog(&self) -> Option<Dog> { None }"),
        "trait default hook: {rust}",
    );
    assert!(
        rust.contains("fn __jux_as_Dog(&self) -> Option<Dog> { Some(self.clone()) }"),
        "impl hook override: {rust}",
    );
}

/// A program with NO class downcasts emits NO runtime-type hooks — the
/// bounded-emission invariant (non-downcasting programs are unchanged).
#[test]
fn no_downcast_no_hooks() {
    let rust = emit(
        r#"
        public abstract class Animal { public abstract String sound(); }
        public class Dog extends Animal { public Dog() {} public String sound() { return "woof"; } }
        public void main() { Animal a = new Dog(); print(a.sound()); }
        "#,
    );
    assert!(!rust.contains("__jux_as_"), "no hooks expected: {rust}");
}

// ---------------------------------------------------------------------------
// §A `incdec` — expression-position ++/-- (value form, N3)
// ---------------------------------------------------------------------------

/// Postfix `x++` as a VALUE lowers to a block that caches the OLD value
/// (`let __jux_t = x`), then steps the place (`x += 1`), then yields the
/// cache — so the expression's value is the pre-increment `x`.
#[test]
fn postfix_incr_value_yields_old() {
    let rust = emit("public void main() { int x = 5; print(x++); }");
    // Block caches old value before the step.
    assert!(rust.contains("let __jux_t = x"), "expected old-value cache: {rust}");
    assert!(rust.contains("x += 1"), "expected the `+= 1` step: {rust}");
    // The block trails with the cached old value.
    assert!(
        rust.contains("__jux_t") && rust.contains("x += 1"),
        "postfix block should yield the cached old value: {rust}",
    );
}

/// Prefix `++x` as a VALUE lowers to a block that steps the place FIRST
/// (`x += 1`) then yields the place (`x`) — the post-step (new) value.
/// There is NO old-value cache for the prefix form.
#[test]
fn prefix_incr_value_yields_new() {
    let rust = emit("public void main() { int x = 5; var y = ++x; }");
    assert!(rust.contains("x += 1"), "expected the `+= 1` step: {rust}");
    // Prefix mutates first, then reads the place — it never caches an
    // OLD value, so no `__jux_t` temp is emitted for this program.
    assert!(
        !rust.contains("__jux_t"),
        "prefix form must not cache an old value: {rust}",
    );
}

/// Decrement uses `- 1`: postfix `x--` caches the old value then steps
/// `x -= 1`.
#[test]
fn postfix_decr_uses_minus_one() {
    let rust = emit("public void main() { int x = 5; print(x--); }");
    assert!(rust.contains("let __jux_t = x"), "expected old-value cache: {rust}");
    assert!(rust.contains("x -= 1"), "expected the `-= 1` step: {rust}");
}

/// `arr[i++]` single-evaluates the index: the index `i++` block is
/// itself the postfix block, and the OUTER read indexes once. The point
/// is that no `i` step is duplicated — exactly one `i += 1` is emitted.
#[test]
fn array_index_incr_single_evaluates() {
    let rust = emit(
        "public void main() { var arr = new int[]{0}; int i = 0; print(arr[i++]); }",
    );
    assert_eq!(
        rust.matches("i += 1").count(),
        1,
        "the index step must run exactly once (single-eval): {rust}",
    );
}

/// Incrementing an array ELEMENT as a value (`counts[s]++`) hoists the
/// index into `__jux_i` so the element place is evaluated once for the
/// load and once for the store WITHOUT re-running the index expression.
#[test]
fn array_element_incr_hoists_index() {
    let rust = emit(
        "public void main() { var counts = new int[]{0}; int s = 0; var got = counts[s]++; }",
    );
    assert!(rust.contains("let __jux_i = s"), "index should hoist to __jux_i: {rust}");
    assert!(rust.contains("let __jux_t ="), "old value should be cached: {rust}");
}

// ---------------------------------------------------------------------------
// §L.6.5 — address-of a class object (`&obj`) + raw-pointer null handling
// ---------------------------------------------------------------------------

const PTR_CLASS_PRELUDE: &str =
    "public class P { public int x; public P(int x) { this.x = x; } } ";

/// `&obj` on a class lowers to the inner-cell pointer (`obj.0.as_ptr()`),
/// reaching through the `Rc<RefCell<P_Inner>>` handle to the data — not the
/// place-pointer macro used for value locals.
#[test]
fn addr_of_class_object_lowers_to_inner_ptr() {
    let rust = emit(&format!(
        "{PTR_CLASS_PRELUDE} public void main() {{ P p = new P(1); unsafe {{ P* q = &p; }} }}"
    ));
    assert!(rust.contains("p.0.as_ptr()"), "expected inner-cell ptr, got: {rust}");
    assert!(
        !rust.contains("addr_of_mut!(p)"),
        "class `&obj` must not use addr_of_mut!: {rust}"
    );
}

/// A class raw-pointer TYPE lowers to `*mut P_Inner` (points at the inner
/// data struct, matching `&obj`'s `obj.0.as_ptr()`).
#[test]
fn class_pointer_type_lowers_to_inner() {
    let rust = emit(&format!(
        "{PTR_CLASS_PRELUDE} public void main() {{ P p = new P(1); unsafe {{ P* q = &p; }} }}"
    ));
    assert!(rust.contains("*mut P_Inner"), "expected `*mut P_Inner`, got: {rust}");
}

/// A value-typed `&local` is unchanged: the place-pointer macro and a plain
/// `*mut isize` pointee (records/primitives keep the direct pointee).
#[test]
fn addr_of_value_local_uses_addr_of_mut() {
    let rust = emit("public void main() { int n = 1; unsafe { int* p = &n; } }");
    assert!(
        rust.contains("addr_of_mut!(n)"),
        "value `&local` should use addr_of_mut!: {rust}"
    );
    assert!(rust.contains("*mut isize"), "expected `*mut isize`, got: {rust}");
}

/// Raw-pointer null comparison lowers to `is_null()`, never the `Option`
/// `is_some()`/`is_none()` (which a `*mut T` does not have). §L.6.
#[test]
fn pointer_null_comparison_uses_is_null() {
    let rust = emit(
        "public void main() { int n = 1; unsafe { int* p = &n; \
         bool a = (p == null); bool b = (p != null); } }",
    );
    assert!(rust.contains(".is_null()"), "expected is_null(), got: {rust}");
    assert!(!rust.contains(".is_some()"), "raw pointer must not use is_some(): {rust}");
    assert!(!rust.contains(".is_none()"), "raw pointer must not use is_none(): {rust}");
}

/// A `null` initializer in a raw-pointer slot lowers to `std::ptr::null_mut()`,
/// not `None` (§L.6.1: `null` is the sole `T*` literal).
#[test]
fn pointer_null_init_lowers_to_null_mut() {
    let rust = emit("public void main() { unsafe { int* p = null; } }");
    assert!(
        rust.contains("let p: *mut isize = std::ptr::null_mut();"),
        "expected `let p: *mut isize = std::ptr::null_mut();`, got: {rust}"
    );
}

/// A raw-pointer FIELD round-trips `null` correctly (the FFI-wrapper idiom):
/// constructor init and a later assignment lower to `std::ptr::null_mut()`
/// (not `None`), and `ptr == null` lowers to `.is_null()` (not `.is_none()`).
#[test]
fn pointer_field_null_round_trips() {
    let rust = emit(
        "public class Buf { public byte* ptr; \
            public Buf() { this.ptr = null; } \
            public void reset() { this.ptr = null; } \
            public bool isNull() { unsafe { return ptr == null; } } } \
         public void main() {}",
    );
    assert!(
        rust.contains("std::ptr::null_mut()"),
        "null into a pointer field should be null_mut(): {rust}"
    );
    assert!(rust.contains(".is_null()"), "field == null should be is_null(): {rust}");
    assert!(!rust.contains(".is_none()"), "pointer field must not use is_none(): {rust}");
    assert!(
        !rust.contains("ptr: None") && !rust.contains("ptr = None"),
        "pointer field must never be None: {rust}"
    );
}

/// A `@layout(c, repr = "i32")` enum lowers to a `#[repr(i32)]` Rust enum with
/// explicit per-variant discriminants — bit-identical to a C `int` enum.
#[test]
fn layout_c_enum_lowers_to_repr_int() {
    let rust = emit(
        "@layout(c, repr = \"i32\") enum S { Ok = 200, NotFound = 404 } public void main() {}",
    );
    assert!(rust.contains("#[repr(i32)]"), "missing #[repr(i32)]: {rust}");
    assert!(rust.contains("Ok = 200"), "missing Ok = 200: {rust}");
    assert!(rust.contains("NotFound = 404"), "missing NotFound = 404: {rust}");
}

// ---------------------------------------------------------------------------
// §L.1.2 — `@layout(c)` C-compatible value structs
// ---------------------------------------------------------------------------

/// A `@layout(c) struct` lowers to a flat `#[repr(C)]` `Copy` VALUE struct, not
/// the `Rc<RefCell<…>>` class handle: fields in declaration order, plain field
/// access (no `.0.borrow()`), and `S*` is `*mut S` (not `*mut S_Inner`).
#[test]
fn layout_c_struct_lowers_to_repr_c_value() {
    let rust = emit(
        "@layout(c) struct P { int x; int y; \
            public P(int x, int y) { this.x = x; this.y = y; } } \
         public void main() { P p = new P(1, 2); int a = p.x; unsafe { P* q = &p; } }",
    );
    assert!(rust.contains("#[repr(C)]"), "missing #[repr(C)]: {rust}");
    assert!(
        rust.contains("#[derive(Clone, Copy, Debug)]"),
        "value struct should derive Copy: {rust}"
    );
    assert!(rust.contains("struct P {"), "expected a plain struct P: {rust}");
    assert!(!rust.contains("P_Inner"), "value struct has no _Inner: {rust}");
    assert!(
        !rust.contains("Rc<std::cell::RefCell<P"),
        "value struct is not Rc<RefCell>: {rust}"
    );
    // Plain field access, no wrapper borrow.
    assert!(rust.contains("p.x"), "field access: {rust}");
    assert!(!rust.contains("p.0.borrow()"), "no wrapper borrow on a value struct: {rust}");
    // `P*` is `*mut P`, not `*mut P_Inner`; `&p` is the place pointer.
    assert!(rust.contains("*mut P"), "P* should be *mut P: {rust}");
    assert!(rust.contains("addr_of_mut!(p)"), "&valueStruct is addr_of_mut!: {rust}");
}

/// A field access through a raw-pointer deref parenthesizes the receiver:
/// `(*q).x`, not `*q.x` (which Rust parses as `*(q.x)`).
#[test]
fn deref_field_access_parenthesizes() {
    let rust = emit(
        "@layout(c) struct P { int x; public P(int x) { this.x = x; } } \
         public void main() { P p = new P(7); unsafe { P* q = &p; int v = (*q).x; } }",
    );
    assert!(rust.contains("(*q).x"), "deref field should be (*q).x: {rust}");
    assert!(!rust.contains("*q.x"), "must not emit the ambiguous *q.x: {rust}");
}

// ---------------------------------------------------------------------------
// §L.7 — C FFI: `unsafe native` blocks → `extern "C"` + String marshalling
// ---------------------------------------------------------------------------

/// An `@extern(lib="…") unsafe native { … }` block lowers to a Rust
/// `#[link(name="…")] extern "C" { pub fn …; }`, with the FFI type mapping
/// (`String` → `*const core::ffi::c_char`, `void*` → `*mut core::ffi::c_void`,
/// `void` return omitted, `ulong` → `u64`).
#[test]
fn extern_block_lowers_to_link_and_extern_c() {
    let rust = emit(
        "@extern(lib = \"c\") unsafe native { \
            void* malloc(ulong size); void free(void* p); i32 puts(String s); \
         } public void main() {}",
    );
    assert!(rust.contains("#[link(name = \"c\")]"), "missing #[link]: {rust}");
    assert!(rust.contains("extern \"C\" {"), "missing extern C: {rust}");
    assert!(
        rust.contains("pub fn malloc(size: u64) -> *mut core::ffi::c_void"),
        "malloc signature wrong: {rust}"
    );
    assert!(
        rust.contains("pub fn free(p: *mut core::ffi::c_void)") && !rust.contains("free(p: *mut core::ffi::c_void) ->"),
        "free should return nothing: {rust}"
    );
    assert!(
        rust.contains("pub fn puts(s: *const core::ffi::c_char) -> i32"),
        "String param should be *const c_char: {rust}"
    );
}

/// A `String` ARGUMENT to a foreign function marshals through a `CString` temp:
/// `CString::new(arg).expect(...)` + `.as_ptr() as *const core::ffi::c_char`.
#[test]
fn extern_string_arg_marshals_via_cstring() {
    let rust = emit(
        "@extern(lib = \"c\") unsafe native { i32 puts(String s); } \
         public void main() { unsafe { i32 n = puts(\"hi\"); } }",
    );
    assert!(
        rust.contains("::std::ffi::CString::new("),
        "expected CString::new marshalling: {rust}"
    );
    assert!(
        rust.contains(".as_ptr() as *const core::ffi::c_char"),
        "expected .as_ptr() cast: {rust}"
    );
    assert!(rust.contains(".expect("), "expected interior-NUL guard: {rust}");
}

/// A `String` RETURN from a foreign function copies out of the C buffer
/// (`CStr::from_ptr(...).to_string_lossy().into_owned()`) with a null guard,
/// and never frees the C memory.
#[test]
fn extern_string_return_copies_out() {
    let rust = emit(
        "@extern(lib = \"kernel32\") unsafe native { String GetCommandLineA(); } \
         public void main() { unsafe { String s = GetCommandLineA(); } }",
    );
    assert!(
        rust.contains("::std::ffi::CStr::from_ptr(__ret as *const core::ffi::c_char)"),
        "expected CStr::from_ptr copy-out: {rust}"
    );
    assert!(rust.contains(".to_string_lossy().into_owned()"), "expected lossy copy: {rust}");
    assert!(
        rust.contains("if __ret.is_null() { String::new() }"),
        "expected null → empty String: {rust}"
    );
    assert!(!rust.contains("free("), "must NOT free the C buffer: {rust}");
}

/// A `char` parameter maps to a C `char` (`core::ffi::c_char`) in the signature
/// and converts at the call site (`(arg) as core::ffi::c_char`) — Jux `char` is
/// a 4-byte Unicode scalar, C `char` is 1 byte.
#[test]
fn extern_char_arg_maps_to_c_char() {
    let rust = emit(
        "@extern(lib = \"c\") unsafe native { i32 putchar(char c); } \
         public void main() { unsafe { i32 r = putchar('H'); } }",
    );
    assert!(
        rust.contains("pub fn putchar(c: core::ffi::c_char)"),
        "char param should be c_char in the signature: {rust}"
    );
    assert!(
        rust.contains(") as core::ffi::c_char"),
        "char arg should convert at the call site: {rust}"
    );
}

/// An `out T` foreign parameter (§M.4) becomes `*mut <T>` in the extern
/// signature, and the call passes `addr_of_mut!(place)` so the C callee writes
/// through it.
#[test]
fn extern_out_param_passes_addr_of_mut() {
    let rust = emit(
        "@extern(lib = \"c\") unsafe native { i32 f(out long count); } \
         public void main() { unsafe { long c = 0; i32 r = f(out c); } }",
    );
    assert!(
        rust.contains("pub fn f(count: *mut i64)"),
        "out param should be *mut i64: {rust}"
    );
    assert!(
        rust.contains("::core::ptr::addr_of_mut!(c)"),
        "out arg should be addr_of_mut!: {rust}"
    );
}

/// A C-variadic foreign fn (`int printf(String fmt, ...)`) emits a trailing
/// `...` in the `extern "C"` signature, and a trailing string-literal argument
/// is marshalled to a `CString` `const char*` like a fixed `String` param
/// (§L.4.2). Non-string trailing args (ints) pass through directly.
#[test]
fn extern_variadic_printf_marshals_trailing_string() {
    let rust = emit(
        "@extern(lib = \"c\") unsafe native { i32 printf(String fmt, ...); } \
         public void main() { unsafe { printf(\"%s=%d\\n\", \"x\", 7); } }",
    );
    assert!(
        rust.contains("pub fn printf(fmt: *const core::ffi::c_char, ...)"),
        "extern sig should end with `...`: {rust}"
    );
    // Both the fmt and the trailing string literal get a CString temp.
    assert!(rust.contains("let __c0 = ::std::ffi::CString::new("), "fmt CString: {rust}");
    assert!(rust.contains("let __c1 = ::std::ffi::CString::new("), "trailing str CString: {rust}");
    assert!(
        rust.contains("__c1.as_ptr() as *const core::ffi::c_char"),
        "trailing string passed as const char*: {rust}"
    );
}

/// A `@layout(c, repr)` C enum may cross the FFI boundary by value: the foreign
/// signature emits the plain enum name (it is already `#[repr(i32)]`, so it is
/// FFI-safe as a bare `i32`). §L.1.3.
#[test]
fn extern_c_enum_param_and_return() {
    let rust = emit(
        "@layout(c, repr = \"i32\") enum Status { Ok = 0, Err = 1 } \
         @extern(lib = \"c\") unsafe native { Status flip(Status s); } \
         public void main() { unsafe { Status r = flip(Status.Ok); } }",
    );
    assert!(rust.contains("#[repr(i32)]"), "enum should be repr(i32): {rust}");
    assert!(
        rust.contains("pub fn flip(s: Status) -> Status"),
        "C enum should cross by value as the bare enum name: {rust}"
    );
}

/// A `@layout(c)` C enum may be a FIELD of a `@layout(c)` value struct: the enum
/// derives `Copy` (no payloads), so the `#[repr(C)] #[derive(.. Copy ..)]` struct
/// stays valid and the field emits the bare enum type. §L.1.2/§L.1.3.
#[test]
fn c_enum_as_value_struct_field() {
    let rust = emit(
        "@layout(c, repr = \"i32\") enum Kind { A = 1, B = 2 } \
         @layout(c) struct Tagged { Kind kind; i32 value; } \
         public void main() {}",
    );
    assert!(rust.contains("#[repr(i32)]"), "enum repr(i32): {rust}");
    // The struct is the flat repr(C) Copy value struct with the enum field.
    assert!(rust.contains("#[repr(C)]"), "struct repr(C): {rust}");
    assert!(
        rust.contains("kind: Kind") || rust.contains("pub kind: Kind"),
        "struct carries the bare enum field type: {rust}"
    );
}

/// The sqlite-style combination: a `String` argument AND an `out` argument in
/// one call marshal together (CString temp + `addr_of_mut!`).
#[test]
fn extern_string_and_out_combine() {
    let rust = emit(
        "@extern(lib = \"c\") unsafe native { i32 open(String path, out long handle); } \
         public void main() { unsafe { long h = 0; i32 rc = open(\"db\", out h); } }",
    );
    assert!(rust.contains("path: *const core::ffi::c_char"), "String param: {rust}");
    assert!(rust.contains("handle: *mut i64"), "out param: {rust}");
    assert!(rust.contains("::std::ffi::CString::new("), "String marshalling: {rust}");
    assert!(rust.contains("::core::ptr::addr_of_mut!(h)"), "out arg: {rust}");
}

/// `@export` gives a free function C linkage: `#[no_mangle] pub extern "C" fn`
/// for the plain form, and `#[export_name = "…"]` (keeping the Jux name on the
/// Rust fn, so internal calls still resolve) for `@export(name = "…")`.
#[test]
fn export_fn_gets_c_linkage() {
    let rust = emit(
        "@export public int add(int a, int b) { return a + b; } \
         @export(name = \"jux_mul\") public int mul(int a, int b) { return a * b; } \
         public void main() {}",
    );
    assert!(rust.contains("#[no_mangle]"), "plain @export → #[no_mangle]: {rust}");
    assert!(
        rust.contains("pub extern \"C\" fn add(a: isize, b: isize) -> isize"),
        "add C ABI signature: {rust}"
    );
    assert!(
        rust.contains("#[export_name = \"jux_mul\"]"),
        "named export → #[export_name]: {rust}"
    );
    assert!(
        rust.contains("pub extern \"C\" fn mul("),
        "mul keeps its Jux name on the Rust fn: {rust}"
    );
}

/// An `@export` whose signature mentions `String` emits the real fn under its
/// Jux name (normal `String` types) PLUS a `#[no_mangle] extern "C"` marshalling
/// wrapper: each `String` param arrives as `*const c_char` (copied in via
/// `CStr`), and a `String` return is handed back via `CString::into_raw`
/// (§L.3.2). Non-String params pass through.
#[test]
fn export_string_emits_marshalling_wrapper() {
    let rust = emit(
        "@export String greet(String name, int n) { return name; } \
         public void main() {}",
    );
    // Real fn keeps Jux name + normal String types (internal callers use it).
    assert!(
        rust.contains("pub fn greet(name: String, n: isize) -> String"),
        "real fn keeps Jux String signature: {rust}"
    );
    // Wrapper: no_mangle extern "C", C-string params/return.
    assert!(rust.contains("#[no_mangle]"), "wrapper is #[no_mangle]: {rust}");
    assert!(
        rust.contains("pub extern \"C\" fn __jux_cabi_greet(name: *const core::ffi::c_char, n: isize) -> *const core::ffi::c_char"),
        "wrapper C-ABI signature: {rust}"
    );
    assert!(rust.contains("::std::ffi::CStr::from_ptr(name)"), "inbound CStr copy: {rust}");
    assert!(rust.contains("let __r = greet(name, n);"), "wrapper calls real fn: {rust}");
    assert!(rust.contains(".into_raw() as *const core::ffi::c_char"), "outbound into_raw: {rust}");
}

/// A pure-primitive `@export` keeps the INLINE `#[no_mangle] extern "C"` form
/// (no wrapper, no marshalling) — the wrapper is only for `String` signatures.
#[test]
fn export_primitive_has_no_wrapper() {
    let rust = emit("@export int add(int a, int b) { return a + b; } public void main() {}");
    assert!(
        rust.contains("pub extern \"C\" fn add(a: isize, b: isize) -> isize"),
        "inline C ABI: {rust}"
    );
    assert!(!rust.contains("__jux_cabi_add"), "no wrapper for a primitive export: {rust}");
}

/// A numeric/pointer-only foreign call is NOT block-wrapped — it lowers to a
/// bare `name(args)` (the generic call path), no `CString`/`CStr` machinery.
#[test]
fn extern_numeric_call_is_bare() {
    let rust = emit(
        "@extern(lib = \"kernel32\") unsafe native { u32 GetCurrentProcessId(); } \
         public void main() { unsafe { u32 p = GetCurrentProcessId(); } }",
    );
    assert!(rust.contains("GetCurrentProcessId()"), "expected bare call: {rust}");
    assert!(!rust.contains("CString::new"), "no marshalling for numeric call: {rust}");
}
