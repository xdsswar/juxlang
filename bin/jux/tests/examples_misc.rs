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
    // A bare block statement, a lambda capturing `this`, `===` on a value
    // type, and printing a collection of collections.
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
            "[[1, 2]]",
            "[[1, 2, 3]]",
        ],
    );
}
