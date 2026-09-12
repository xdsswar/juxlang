//! The remaining uncovered examples -- language features with no cluster of
//! their own: annotations, generics, lambdas, records, statics, aliases.

mod common;

#[test]
fn annotations() {
    common::expect_output(
        "annotations",
        "annotations",
        &[
            "woof",
        ],
    );
}

/// User-DEFINED annotation types (§A.2): declaring one, applying it with
/// named arguments, letting defaults fill the rest, and the bare form for an
/// annotation with no parameters.
///
/// The program's output is ordinary: an annotation type emits no code of its
/// own, so what this proves is that the whole surface compiles and changes
/// nothing about what the annotated members do.
#[test]
fn annotations_user() {
    common::expect_output(
        "annotations_user",
        "annotations-user",
        &[
            "user 7",
            "count=3",
            "saved ada",
        ],
    );
}

#[test]
fn bounded_generic() {
    common::expect_output(
        "bounded_generic",
        "bounded-generic",
        &[
            "Woof from Rex",
        ],
    );
}

#[test]
fn cast_test() {
    common::expect_output(
        "cast_test",
        "cast-test",
        &[
            "12",
            "12",
            "42",
            "42",
            "100",
            "10",
        ],
    );
}

#[test]
fn concat_typed() {
    common::expect_output(
        "concat_typed",
        "concat-typed",
        &[
            "hello, world!",
        ],
    );
}

#[test]
fn constants() {
    common::expect_output(
        "constants",
        "constants",
        &[
            "100",
            "Hello",
        ],
    );
}

#[test]
fn crm_dashboard() {
    common::expect_output(
        "crm_dashboard",
        "crm-dashboard",
        &[
            "wrote crm.html",
        ],
    );
}

#[test]
fn elvis_reuse() {
    common::expect_output(
        "elvis_reuse",
        "elvis-reuse",
        &[
            "user_42",
            "user_42",
            "user_42",
        ],
    );
}

#[test]
fn encapsulation() {
    common::expect_output(
        "encapsulation",
        "encapsulation",
        &[
            "balance = 150",
        ],
    );
}

#[test]
fn exhaustive_probe() {
    common::expect_output(
        "exhaustive_probe",
        "exhaustive-probe",
        &[
            "warm",
            "fresh",
            "cool",
        ],
    );
}

#[test]
fn extends_generic() {
    common::expect_output(
        "extends_generic",
        "extends-generic",
        &[
            "42",
            "42",
            "hello",
            "hello",
        ],
    );
}

#[test]
fn higher_order() {
    common::expect_output(
        "higher_order",
        "higher-order",
        &[
            "25",
            "13",
            "15",
        ],
    );
}

#[test]
fn lambdas() {
    common::expect_output(
        "lambdas",
        "lambdas",
        &[
            "14",
            "7",
            "7",
            "0",
            "42",
            "100",
        ],
    );
}

#[test]
fn method_ref() {
    common::expect_output(
        "method_ref",
        "method-ref",
        &[
            "hi, Ada",
            "14",
        ],
    );
}

#[test]
fn null_probe() {
    common::expect_output(
        "null_probe",
        "null-probe",
        &[
            "hello",
            "no note",
            "hi, Ada",
            "ace",
            "ace",
            "null",
            "ace2",
            "nick=ace2",
            "hi, user_7",
            "hi, user_42",
        ],
    );
}

#[test]
fn package_demo() {
    common::expect_output(
        "package_demo",
        "package-demo",
        &[
            "Hello, Jux!",
        ],
    );
}

#[test]
fn record_display() {
    common::expect_output(
        "record_display",
        "record-display",
        &[
            "point=Point(x: 3, y: 4)",
            "user=User(name: alice, age: 30)",
        ],
    );
}

#[test]
fn record_methods() {
    common::expect_output(
        "record_methods",
        "record-methods",
        &[
            "m=$150",
            "doubled=300",
            "bigger=$200",
        ],
    );
}

#[test]
fn reentrancy_notnull_local() {
    common::expect_output(
        "reentrancy_notnull_local",
        "reentrancy-notnull-local",
        &[
            "count=1",
        ],
    );
}

#[test]
fn reentrancy_notnull_this() {
    common::expect_output(
        "reentrancy_notnull_this",
        "reentrancy-notnull-this",
        &[
            "count=1",
        ],
    );
}

#[test]
fn rust_std_collections() {
    common::expect_output(
        "rust_std_collections",
        "rust-std-collections",
        &[
            "vec: len=3 first=3 total=12",
            "map: len=2 has ada=true",
            "set: len=1 has 7=true",
        ],
    );
}

#[test]
fn sealed_shapes() {
    common::expect_output(
        "sealed_shapes",
        "sealed-shapes",
        &[
            "circle radius=5, a shape",
            "square side=3, a shape",
        ],
    );
}

#[test]
fn statics() {
    common::expect_output(
        "statics",
        "statics",
        &[
            "3.14159",
            "2147483647",
            "7",
            "42",
        ],
    );
}

#[test]
fn type_aliases() {
    common::expect_output(
        "type_aliases",
        "type-aliases",
        &[
            "42",
            "99",
        ],
    );
}

#[test]
fn interface_default_override() {
    // A class method beats an interface default, including when the class
    // that declares it is an ancestor rather than the concrete type. The
    // last line is the case that must NOT change: an interface default
    // calling another default, with no class override anywhere.
    common::expect_output(
        "interface_default_override",
        "interface-default-override",
        &[
            "[user: <user>]",
            "[user: Ada (staff)]",
            "[user: Bo (intern, mentored by Ada)]",
            "visitor logged in",
        ],
    );
}

#[test]
fn smart_cast_reuse() {
    // Reading a smart-cast binding more than once. Every line here failed
    // to compile at one point, and only because of the SECOND read.
    common::expect_output(
        "smart_cast_reuse",
        "smart-cast-reuse",
        &[
            "hi|hi",
            "len=2 upper=HI",
            "[\"a\", \"b\"]|2",
            "Ada/Ada/Ada",
            "none",
        ],
    );
}

#[test]
fn const_string_use() {
    // A `const String` is stored as a borrow and read as a `String`, and a
    // constant String expression folds at compile time (§T.11.7).
    common::expect_output(
        "const_string_use",
        "const-string-use",
        &[
            "3",
            "jux-lang",
            "JUX",
            "greeting: hello, jux",
            "TOOL=juxc VERSION=0.0.1",
        ],
    );
}

#[test]
fn numeric_literals_and_promotion() {
    // Three silent-wrong-answer bugs in one example: `long`'s smallest
    // literal became 0, a ternary did not promote its arms, and a whole
    // `double` printed without its decimal point.
    common::expect_output(
        "numeric_literals_and_promotion",
        "numeric-literals-promotion",
        &[
            "2147483647",
            "-2147483648",
            "9223372036854775807",
            "-9223372036854775808",
            "1.0",
            "2.5",
            "3",
            "1.0",
            "1.5",
            "0.30000000000000004",
            "-0.5",
            "100.0",
            "7",
        ],
    );
}

#[test]
fn scopes_and_identity() {
    // A bare block statement, a lambda capturing `this`, `===` and `!==` on
    // both a value type and a class, and printing a collection of
    // collections.
    common::expect_output(
        "scopes_and_identity",
        "scopes-and-identity",
        &[
            "2",
            "1",
            "inner",
            "1",
            "1",
            "9",
            "true",
            "true",
            "false",
            "true",
            // `!==` is the negation of `===`: two equal Holders are still
            // two objects, and a String is a value so it never differs.
            "true",
            "false",
            "false",
            "[[1, 2]]",
            "[[1, 2, 3]]",
        ],
    );
}

#[test]
fn generics_nesting() {
    // Nested generics, found by diffing the same program against Java.
    // A generic class could not be another's type argument; a `String`
    // inside a generic printed with quotes.
    common::expect_output(
        "generics_nesting",
        "generics-nesting",
        &[
            "1",
            "[1]",
            "2",
            "[2]",
            "3",
            "30",
            "22",
            "box(7)",
            // `box(q)`, NOT `box(\"q\")`: a generic function's type parameter
            // carries a `Display` bound when its values are formatted.
            "box(q)",
            "1/a",
            "x/2.5",
            "(1,2)/tag",
            "store:8",
            "empty",
            "has v",
            "has deep",
            "sq=9.0",
            "tri=12.0",
        ],
    );
}

#[test]
fn swap_casts_and_safe_nav() {
    // The bubble sort aborted at run time with "RefCell already borrowed";
    // `(short) 70000` did not compile; `s?.length()` printed `Some(4)`.
    common::expect_output(
        "swap_casts_and_safe_nav",
        "swap-casts-safe-nav",
        &[
            "1,2,3,5,6,7,8,9,",
            "3",
            "-1",
            "44",
            "4464",
            "122",
            "B",
            "b",
            "4",
            "ABCD",
            "null",
            "2",
            "xy",
        ],
    );
}

#[test]
fn indexed_writes() {
    // Every shape of indexed write that can alias the cell being written:
    // the swap at the heart of a sort, a self-referential index, a value that
    // is a call borrowing the same array, a nested collection row, and map
    // writes through a field. Each of these aborted at run time, or failed to
    // compile, before the statement-scoped lowering (§CR.4.1).
    common::expect_output(
        "indexed_writes",
        "indexed-writes",
        &[
            "1,2,3,5,6,7,8,9,",
            "3",
            "-1",
            "77",
            "2",
            "[[10, 10]]",
            "[[10, 10], [1, 2]]",
            "2",
            "1",
            "2",
            "9",
            "ok",
        ],
    );
}

#[test]
fn safe_navigation() {
    // `?.` produces a nullable value even when the member is not (7.10, and
    // ERRATA E5 marks the example normative) -- and produces an ORDINARY one
    // when the receiver cannot be null, which used to fail to compile.
    common::expect_output(
        "safe_navigation",
        "safe-navigation",
        &[
            "4",
            "false",
            "null",
            "true",
            "-1",
            "x",
            "x",
            "1",
            "none",
            "deep",
            "true",
        ],
    );
}

#[test]
fn generic_declarations_print() {
    // Every generic declaration kind has a string form and formats a type
    // parameter without quoting it. Generic records and enums had NO string
    // form, so they could not be another generic's type argument; generic
    // methods, records and enums were missing the `Display` bound, so a
    // `String` payload printed with quotes.
    common::expect_output(
        "generic_declarations_print",
        "generic-declarations-print",
        &[
            "cell(5)",
            // `cell(x)`, NOT `cell("x")`.
            "cell(x)",
            "Cell(value: y)",
            "pair(1, two)",
            "Some(value: hi)",
            "None",
            // A generic record and a generic enum as a type ARGUMENT: these
            // did not compile at all.
            "wrap(Cell(value: in))",
            "wrap(Some(value: deep))",
            // A generic method: `m:s`, not `m:"s"`.
            "m:s",
            "m:7",
        ],
    );
}

#[test]
fn elvis_typing() {
    // `a ?? b` is typed from `a`, the fallback is checked against it, a
    // nullable fallback keeps the null, and `??` on a non-nullable value is
    // the value. `sideEffect()` printing nothing is the assertion that a
    // redundant fallback is never evaluated.
    common::expect_output(
        "elvis_typing",
        "elvis-typing",
        &[
            "unknown",
            "Ada",
            "0",
            "7",
            "true",
            "false",
            "4",
            "-1",
            "plain",
            "plain",
            "deep",
        ],
    );
}


#[test]
fn interface_constant_scope() {
    // A bare interface constant read from the interface's own `default`
    // method, and the same constant as a concat operand.
    common::expect_output(
        "interface_constant_scope",
        "interface-constant-scope",
        &[
            ">> ada",
            ">> ada (3)",
            ">>",
            ">> qualified",
            "3",
        ],
    );
}

#[test]
fn static_collection() {
    // A mutable `static` whose payload is a collection handle, and the
    // Java field-initializer order around it.
    common::expect_output(
        "static_collection",
        "static-collection",
        &[
            "base-field",
            "base-ctor",
            "derived-field",
            "derived-ctor",
            "base-field/derived-field",
            "4",
        ],
    );
}

#[test]
fn switch_assign_arm() {
    // An assignment (plain and compound) as an arrow-switch arm body.
    common::expect_output(
        "switch_assign_arm",
        "switch-assign-arm",
        &["high", "mid", "low", "150", "50", "0"],
    );
}

#[test]
fn generic_free_functions() {
    // A free function's own `<T>`: an element read out of a `Vec<T>` has to
    // clone rather than move.
    common::expect_output(
        "generic_free_functions",
        "generic-free-functions",
        &["one", "2", "7", "value=one", "value=42"],
    );
}

#[test]
fn index_target_side_effects() {
    // A store into a local from inside an assignment TARGET, and the S.1
    // evaluation order it exposes.
    common::expect_output(
        "index_target_side_effects",
        "index-target-side-effects",
        &["1", "1", "10", "40", "4", "9", "9", "1"],
    );
}


#[test]
fn free_function_overloading() {
    // Overloaded free functions (§T.3.1): the pick, and the return type
    // coming from the member that was picked rather than from member 0.
    common::expect_output(
        "free_function_overloading",
        "free-function-overloading",
        &[
            "int:1",
            "double:2.5",
            "string:x",
            "two:7",
            "int:9",
            "string:text",
            "4",
            "pair",
            "5",
            "m-int:7",
            "m-string:y",
        ],
    );
}


#[test]
fn null_narrowing() {
    // Narrowing on a null test (§7.10): the then-branch, the else-branch,
    // and the guard clause that narrows the rest of the block.
    common::expect_output(
        "null_narrowing",
        "null-narrowing",
        &[
            "OK",
            "none",
            "OK",
            "-",
            "OK",
            "-",
            "4",
            "-1",
            "1->2",
            "(empty)",
            "1",
            "2",
        ],
    );
}


#[test]
fn runtime_exceptions() {
    // The exceptions the RUNTIME raises (ERRATA E11). Each catches as
    // itself, as `RuntimeException`, and as `Throwable`; a handler naming a
    // different type does not swallow it.
    common::expect_output(
        "runtime_exceptions",
        "runtime-exceptions",
        &[
            "NPE caught",
            "CCE caught",
            "AE caught: / by zero",
            "caught as RuntimeException",
            "caught as Throwable",
            "outer NPE",
        ],
    );
}


#[test]
fn interface_hierarchy() {
    // An interface extending another: the obligations are checked where the
    // class is declared, a default method on the super-interface resolves,
    // and a value flows into every level of the chain.
    common::expect_output(
        "interface_hierarchy",
        "interface-hierarchy",
        &[
            "ada",
            "ada!",
            "hello, ada",
            "ada!",
            "hello, ada",
            "36",
            "grace!",
            "ada!",
        ],
    );
}


#[test]
fn definite_assignment() {
    // The shapes that satisfy §S.4.6: both arms of an if/else, a guard that
    // leaves, a `do`/`while` body, a `finally`, an `out` argument, and a
    // nullable local that needs no initializer at all.
    common::expect_output(
        "definite_assignment",
        "definite-assignment",
        &["1", "5", "1", "2", "null ok", "3", "6", "2", "3"],
    );
}


#[test]
fn enum_auto_helpers() {
    // §7.7.3: `name()` and `ordinal()` on any enum value, `values()` as a
    // static on a payload-free one, and a user declaration replacing one.
    common::expect_output(
        "enum_auto_helpers",
        "enum-auto-helpers",
        &[
            "Bronze",
            "2",
            "Silver@1",
            "Bronze=0",
            "Silver=1",
            "Gold=2",
            "Circle",
            "1",
            "shouted",
            "1",
        ],
    );
}
