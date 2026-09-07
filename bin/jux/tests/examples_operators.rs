//! The `op_*` examples: operator overloading, which had no coverage at all.

mod common;

#[test]
fn op_arithmetic() {
    common::expect_output(
        "op_arithmetic",
        "op-arithmetic",
        &[
            "sum=$200",
            "diff=$100",
            "false",
            "true",
        ],
    );
}

#[test]
fn op_cmp() {
    common::expect_output(
        "op_cmp",
        "op-cmp",
        &[
            "a<b: true",
            "a<=c: true",
            "b>a: true",
            "a>=c: true",
            "a==c: true",
            "a==b: false",
        ],
    );
}

#[test]
fn op_delete() {
    common::expect_output(
        "op_delete",
        "op-delete",
        &[
            "true",
            "m=$150",
        ],
    );
}

#[test]
fn op_enum() {
    common::expect_output(
        "op_enum",
        "op-enum",
        &[
            "c=Color::user",
        ],
    );
}

#[test]
fn op_overload() {
    common::expect_output(
        "op_overload",
        "op-overload",
        &[
            "true",
            "false",
            "a=hello, c=world",
        ],
    );
}
