//! The `ffi_*` examples: the C boundary, which had no coverage at all.
//!
//! Three of the five link a Windows system library by name and are gated
//! accordingly; the other two are portable.

mod common;

#[test]
fn ffi_enum() {
    common::expect_output(
        "ffi_enum",
        "ffi-enum",
        &[
            "status=NotFound code=404",
        ],
    );
}

#[test]
fn ffi_export() {
    common::expect_output(
        "ffi_export",
        "ffi-export",
        &[
            "add=5 square=25",
            "Hello world (x3)",
        ],
    );
}

/// Links a Windows system library by name, so it is gated to Windows.
#[cfg(windows)]
#[test]
fn ffi_strings() {
    // The process id and the command line differ every run, so only the parts
    // the example is actually demonstrating are asserted: a `String` marshalled
    // out to `lstrlenA` (9 characters), and one marshalled back in.
    common::expect_contains("ffi_strings", "ffi-strings", &["len=9 pid=", "cmd="]);
}

/// Links a Windows system library by name, so it is gated to Windows.
#[cfg(windows)]
#[test]
fn ffi_struct() {
    common::expect_output(
        "ffi_struct",
        "ffi-struct",
        &[
            "ok=1 cursor=(281, 717)",
            "a.x=99 b.x=1",
        ],
    );
}

/// Links a Windows system library by name, so it is gated to Windows.
#[cfg(windows)]
#[test]
fn ffi_variadic() {
    common::expect_output(
        "ffi_variadic",
        "ffi-variadic",
        &[
            "hi world, 2 + 3 = 5",
            "no extra args here",
        ],
    );
}
