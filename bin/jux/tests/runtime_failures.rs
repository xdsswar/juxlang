//! A program that fails at run time says so in Jux terms (GAPS.md gap 33).
//!
//! Each example here ends on purpose, the way programs do: an array position
//! out of range, an integer overflow in a debug build, a failed `assert`, a
//! map key that is not there, recursion that never stops. What the program
//! prints is Jux's report, `Exception in thread "main" ...` or `panic: ...`,
//! with the `.jux` line it happened at, and never the Rust runtime's text
//! (`thread 'main' panicked at src/main.rs`, `index out of bounds: the len
//! is`, `RUST_BACKTRACE`). The first four are also pinned word for word by
//! the output corpus (`tests/run.rs`); this test holds every one of them to
//! the leak detector directly, with nothing excused.

mod common;

use std::process::Command;

/// Run `examples/<name>.jux`; its stdout and stderr, and its exit status.
fn run(name: &str) -> (String, Option<i32>) {
    let root = common::workspace_root();
    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("run")
        .arg("--emit-dir")
        .arg(root.join("target").join(format!("it-rtf-{name}")))
        .arg(root.join("examples").join(format!("{name}.jux")))
        .current_dir(&root)
        .env("JUX_SELFCHECK", "1")
        .output()
        .unwrap_or_else(|e| panic!("spawning jux for {name}: {e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .replace("\r\n", "\n");
    let text: String = text.lines().filter(|l| !l.starts_with("jux: ")).map(|l| format!("{l}\n")).collect();
    (text, output.status.code())
}

/// The run ended with status 101, printed `report`, named the `.jux` line,
/// and showed no Rust.
fn assert_jux_failure(name: &str, report: &str) {
    let (text, code) = run(name);
    assert_eq!(code, Some(101), "{name}: an uncaught failure exits 101:\n{text}");
    assert!(text.contains(report), "{name}: expected {report:?} in:\n{text}");
    assert!(text.contains(&format!("{name}.jux:")), "{name}: the report names the .jux line:\n{text}");
    assert!(
        juxc_diagnostics::leak::find_rust_leak(&text).is_none(),
        "{name} shows Rust: {:?}\n{text}",
        juxc_diagnostics::leak::find_rust_leak(&text)
    );
    assert!(!text.contains("main.rs"), "{name}: {text}");
}

#[test]
fn an_index_out_of_bounds_is_a_jux_exception() {
    assert_jux_failure(
        "runtime_index_out_of_bounds",
        "Exception in thread \"main\" jux.std.exceptions.IndexOutOfBoundsException: Index 5 out of bounds for length 3",
    );
}

#[test]
fn an_integer_overflow_is_a_jux_panic() {
    assert_jux_failure("runtime_integer_overflow", "panic: integer overflow in addition");
}

#[test]
fn a_failed_assert_names_no_generated_code() {
    assert_jux_failure("runtime_assert_failed", "panic: assertion failed\n");
}

#[test]
fn a_missing_map_key_is_a_jux_panic() {
    assert_jux_failure("runtime_missing_map_key", "panic: no entry for the key");
}

/// A Rust library's error nothing catches is reported under the Jux
/// exception it surfaces as, never the library's type name (gap 38).
#[test]
fn an_uncaught_library_error_is_a_jux_exception() {
    assert_jux_failure(
        "rust_error_uncaught",
        "Exception in thread \"main\" jux.std.exceptions.NumberFormatException: invalid float literal",
    );
    let (text, _) = run("rust_error_uncaught");
    assert!(!text.contains("ParseFloatError"), "{text}");
}

/// A library's error with no row of its own is reported as the Jux
/// exception its shape says, never as a `LibraryException` when a closer
/// class exists, and never under its Rust type name (ERRATA E1XX-SWEEPB).
#[test]
fn an_uncaught_library_error_is_the_closest_jux_exception() {
    assert_jux_failure(
        "runtime_library_error_by_shape",
        "Exception in thread \"main\" jux.std.exceptions.NoSuchElementException: environment variable not found",
    );
    let (text, _) = run("runtime_library_error_by_shape");
    assert!(!text.contains("VarError") && !text.contains("LibraryException"), "{text}");
}

/// A stack overflow never reaches a panic hook. The prelude reports it
/// itself: a vectored exception handler on Windows, a `SIGSEGV`/`SIGBUS`
/// handler on an alternate signal stack on Linux and macOS (gap 38).
#[test]
fn a_stack_overflow_is_reported_in_jux_terms() {
    let (text, code) = run("runtime_stack_overflow");
    assert!(text.contains("recursing"), "{text}");
    let covered = cfg!(windows)
        || (cfg!(any(target_os = "linux", target_os = "macos"))
            && cfg!(any(target_arch = "x86_64", target_arch = "aarch64")));
    if covered {
        assert_eq!(code, Some(101), "{text}");
        assert!(
            text.contains("panic: stack overflow: a method called itself too many times without finishing
"),
            "{text}"
        );
        assert!(juxc_diagnostics::leak::find_rust_leak(&text).is_none(), "{text}");
    } else {
        assert_ne!(code, Some(0), "{text}");
    }
}

/// The same on a thread a library started (`std::thread::Builder::spawn`
/// running a Jux lambda): the closure records its thread's stack the first
/// time it runs, and on Linux and macOS a thread nobody recorded is judged by
/// its stack pointer at the fault, so the report is Jux's there too.
#[test]
fn a_stack_overflow_on_a_librarys_thread_is_reported_in_jux_terms() {
    let (text, code) = run("runtime_stack_overflow_crate_thread");
    assert!(text.contains("recursing on a library's thread"), "{text}");
    let covered = cfg!(windows)
        || (cfg!(any(target_os = "linux", target_os = "macos"))
            && cfg!(any(target_arch = "x86_64", target_arch = "aarch64")));
    if covered {
        assert_eq!(code, Some(101), "{text}");
        assert!(
            text.contains("panic: stack overflow: a method called itself too many times without finishing\n"),
            "{text}"
        );
        assert!(!text.contains("has overflowed its stack"), "{text}");
        assert!(juxc_diagnostics::leak::find_rust_leak(&text).is_none(), "{text}");
    } else {
        assert_ne!(code, Some(0), "{text}");
    }
}

/// The closure handed to a library starts by recording its thread's stack,
/// once, and nothing else in the program does.
#[test]
fn a_closure_handed_to_a_library_records_its_thread() {
    let root = common::workspace_root();
    let crate_dir = root.join("target").join("it-rtf-crate-thread-emit");
    let built = Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("build")
        .arg("--emit-dir")
        .arg(&crate_dir)
        .arg(root.join("examples").join("runtime_stack_overflow_crate_thread.jux"))
        .current_dir(&root)
        .output()
        .expect("spawning jux build");
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let main = std::fs::read_to_string(crate_dir.join("src").join("main.rs")).expect("the emitted main.rs");
    let calls = main.matches("crate::jux_enter_callback();").count();
    assert_eq!(calls, 1, "one closure is handed to a library:\n{main}");
}

/// The Unix handler is declared by hand (no binding crate), so its layouts
/// are checked against every Unix target this machine can compile for: the
/// emitted crate of the stack-overflow examples must type-check for Linux and
/// macOS on x86_64 and aarch64: the handler with its per-target `ucontext_t`
/// reads, and a closure handed to a library with its thread registration. A
/// target that is not installed is skipped.
#[test]
fn the_unix_stack_overflow_handler_type_checks_for_each_unix_target() {
    let installed = Command::new("rustup").args(["target", "list", "--installed"]).output();
    let installed = match installed {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        _ => return,
    };
    let targets: Vec<&str> = [
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ]
        .into_iter()
        .filter(|t| installed.lines().any(|l| l.trim() == *t))
        .collect();
    if targets.is_empty() {
        return;
    }
    let root = common::workspace_root();
    for example in ["runtime_stack_overflow", "runtime_stack_overflow_crate_thread"] {
        let crate_dir = root.join("target").join(format!("it-rtf-unix-{example}"));
        let built = Command::new(env!("CARGO_BIN_EXE_jux"))
            .arg("build")
            .arg("--emit-dir")
            .arg(&crate_dir)
            .arg(root.join("examples").join(format!("{example}.jux")))
            .current_dir(&root)
            .output()
            .expect("spawning jux build");
        assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
        let mut check = Command::new("cargo");
        check.arg("check").arg("--quiet").current_dir(&crate_dir);
        for t in &targets {
            check.arg("--target").arg(t);
        }
        let out = check.output().expect("spawning cargo check");
        assert!(
            out.status.success(),
            "the emitted crate of {example} does not type-check for {targets:?}:
{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let main = std::fs::read_to_string(crate_dir.join("src").join("main.rs")).expect("the emitted main.rs");
        assert!(main.contains("fn sigaltstack("), "the Unix handler is in the prelude");
        assert!(main.contains("fn stack_pointer("), "the handler reads the faulting stack pointer");
    }
}
