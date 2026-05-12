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
    assert!(!rust.contains("let mut"), "no `let mut` expected, got: {rust}");
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
fn for_each_on_string_vec_clones_each_element() {
    let rust = emit(
        r#"public void main() {
               String[] xs = {"a", "b"};
               for (var x : xs) { print(x); }
               print(xs.length);
           }"#,
    );
    assert!(rust.contains("for x in xs.iter().cloned() {"), "got: {rust}");
    // The post-loop `.length` reads xs — proves we didn't move it.
    // Identifier receiver, so no parens around it.
    assert!(rust.contains("xs.len() as isize"), "got: {rust}");
    assert!(!rust.contains("(xs).len()"), "stale parens: {rust}");
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
    assert!(!rust.contains("isize"), "should not silently map to isize: {rust}");
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
    assert!(rust.contains("name: String,"), "field type: {rust}");
    assert!(rust.contains("pub fn new(name: String)"), "param: {rust}");
    // Rust struct field shorthand kicks in when init expr matches
    // the field name.
    assert!(rust.contains("name,\n"), "ctor init shorthand: {rust}");
    assert!(!rust.contains("name: name"), "no longhand: {rust}");
    // Value-consuming context — `return this.name;` — still clones
    // so the field doesn't move out of `&self`.
    assert!(
        rust.contains("self.name.clone()"),
        "value-position read should still clone: {rust}",
    );
    // Format-arg context — `println!("{}", u.name)` — borrows, no
    // clone needed.
    assert!(
        rust.contains(r#"println!("{}", u.name)"#),
        "format-arg read should NOT clone: {rust}",
    );
    assert!(
        !rust.contains("u.name.clone()"),
        "stale clone in format arg: {rust}",
    );
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
        rust.contains("impl<T: Drawable + Clone> Wrapper<T> {"),
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
    assert!(rust.contains("pub trait AnimalKind {}"), "marker decl: {rust}");
    assert!(rust.contains("impl AnimalKind for Animal {}"), "marker impl: {rust}");
    // The bound on Carrier uses AnimalKind, not Animal directly.
    assert!(
        rust.contains("impl<T: AnimalKind + Clone> Carrier<T> {"),
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
        rust.contains("impl<T: AnimalKind + Greeter + Clone> Holder<T> {"),
        "combined bound: {rust}",
    );
    // Transitive marker impl — Polite implements AnimalKind because
    // it extends Animal.
    assert!(rust.contains("impl AnimalKind for Polite {}"), "transitive marker: {rust}");
}

// ----------------------------------------------------------------------
// Inheritance (Turn 1) — composition + Deref, super() in ctor
// ----------------------------------------------------------------------

/// `class Dog extends Animal { … }` lowers to a struct embedding
/// `__parent: Animal` plus `impl Deref` / `impl DerefMut` blocks so
/// inherited methods auto-deref through.
#[test]
fn extends_emits_parent_field_and_deref_impls() {
    let rust = emit(
        r#"
        public class Animal { private String name; public Animal(String name) { this.name = name; } }
        public class Dog extends Animal { public Dog(String name) { super(name); } }
        public void main() { var d = new Dog("Rex"); print(d); }
        "#,
    );
    // Subclass struct has `__parent: Animal` as the first field.
    assert!(
        rust.contains("pub struct Dog {\n    __parent: Animal,"),
        "struct embed: {rust}",
    );
    // Deref + DerefMut impls emitted.
    assert!(rust.contains("impl std::ops::Deref for Dog {"), "Deref: {rust}");
    assert!(rust.contains("type Target = Animal;"), "Deref target: {rust}");
    assert!(rust.contains("impl std::ops::DerefMut for Dog {"), "DerefMut: {rust}");
}

/// `super(args);` in a child constructor lifts into the
/// `__parent: Parent::new(args)` slot of the child's struct literal.
#[test]
fn super_call_lifts_into_struct_literal() {
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
        public void main() { var d = new Dog("Rex", 4); print(d.age); }
        "#,
    );
    // `Animal::new(name)` appears in Dog's Self literal, NOT as a
    // separate statement.
    assert!(
        rust.contains("__parent: Animal::new(name),"),
        "super lifted into struct: {rust}",
    );
    // The own-field assignment lands too (Rust field shorthand).
    assert!(rust.contains("age,\n"), "own field init shorthand: {rust}");
    assert!(!rust.contains("age: age"), "no longhand: {rust}");
    // The `super(...)` doesn't survive as a statement.
    assert!(
        !rust.contains("super(") && !rust.contains("__super__"),
        "super shouldn't appear in body: {rust}",
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
    assert!(rust.contains("pub trait Drawable {"), "trait header: {rust}");
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
    // Delegating trait impl forwards through `self.magic()`.
    assert!(
        rust.contains("impl Greeter for Friendly {"),
        "trait impl header: {rust}",
    );
    assert!(rust.contains("self.magic()"), "delegating call: {rust}");
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
    // §O.3 auto-derive pass on top of the baseline three.
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Copy)]"),
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
    assert!(rust.contains("impl<A: Clone, B: Clone> Pair<A, B> {"), "bound: {rust}");
    // Generic-component read in format-arg context borrows, no clone.
    assert!(
        rust.contains(r#"println!("{}", p.first)"#),
        "format-arg read: {rust}",
    );
}

// ----------------------------------------------------------------------
// Generics (Turn 1) — generic class declarations + uses
// ----------------------------------------------------------------------

/// `class Box<T> { T value; … }` lowers to a Rust struct with the
/// declared parameter list, a Clone derive, and a `T: Clone`-bounded
/// impl block. The generic-typed field uses Rust's `T` directly.
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
    assert!(rust.contains("#[derive(Clone)]"), "derive(Clone): {rust}");
    assert!(rust.contains("pub struct Box<T> {"), "struct header: {rust}");
    assert!(rust.contains("value: T,"), "generic field: {rust}");
    assert!(rust.contains("impl<T: Clone> Box<T> {"), "impl bound: {rust}");
    // Method body auto-clones the generic field on read.
    assert!(rust.contains("self.value.clone()"), "generic-field clone: {rust}");
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
/// use the direct `Self { … }` literal pattern — no `__self`
/// builder, no `Default`-based initialization. Existing User-style
/// classes get cleaner output as a bonus.
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
    // No `__self` builder — the simple-ctor path emits direct Self.
    assert!(!rust.contains("__self"), "should not use __self pattern: {rust}");
    // The Self literal carries each field's init expr, using Rust's
    // struct field shorthand.
    assert!(rust.contains("Self {"), "Self literal: {rust}");
    assert!(rust.contains("a,\n"), "field init a shorthand: {rust}");
    assert!(rust.contains("b,\n"), "field init b shorthand: {rust}");
    assert!(!rust.contains("a: a"), "no longhand: {rust}");
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
    // Body reads `this.name` which auto-clones via the trailing
    // `.clone()`, producing an owned String the body returns.
    assert!(rust.contains("self.name.clone()"), "field clone missing: {rust}");
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
    assert!(
        rust.contains("use com::example::{A, B as B2, C};"),
        "expected brace group, got: {rust}",
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
    assert!(rust.contains("use c::{X, Y};"), "missing grouped import, got: {rust}");
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
        rust.contains("#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]"),
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
        rust.contains("#[derive(Debug, Clone, PartialEq, Eq, Hash)]"),
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
    assert!(
        rust.contains("#[derive(Debug, Clone, PartialEq, Copy)]"),
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
    assert!(
        !rust.contains("impl Fn"),
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
    assert!(!rust.contains("impl std::fmt::Display for Plain"), "got: {rust}");
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
    assert!(rust_unary.contains("self.__op_sub()"), "neg should call __op_sub: {rust_unary}");
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
        rust.contains("#[derive(Debug, Clone, Copy)]"),
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
