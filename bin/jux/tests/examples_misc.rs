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
