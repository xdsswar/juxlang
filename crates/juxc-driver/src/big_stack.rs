//! Run compilation on a thread with a large stack.
//!
//! The front end is recursive descent all the way down: nesting in the source
//! becomes nesting on the call stack, in the parser and again in every AST walk
//! after it. Those frames are not small -- measured on Windows, a single level
//! of parenthesised expression costs roughly 110 KB across the full
//! parse -> resolve -> typecheck chain. Against the 8 MB a main thread gets by
//! default, that is a hard ceiling around 60 levels of nesting, and passing it
//! aborts the process with `thread 'main' has overflowed its stack` -- no
//! diagnostic, no file name, no exit code a caller can interpret.
//!
//! Sixty is not a safe budget. Hand-written code rarely goes that deep, but
//! machine-generated sources, long method-chain builders and deeply nested data
//! literals all get there, and "the compiler died" is the worst possible way to
//! find out.
//!
//! So the work runs on a thread with a much larger stack. This is what rustc
//! does for the same reason. The reserve is virtual address space, not
//! committed memory: pages are only backed as they are touched, so a compile
//! that never recurses deeply pays nothing for the headroom. With the reserve
//! in place, the parser's own [`juxc_parse`] depth limit becomes the thing that
//! actually stops pathological input, and it reports an error instead.

use std::thread;

/// Stack reserved for any thread that runs the front end.
///
/// Also used by `juxc-lsp` for its tokio worker threads, which otherwise get
/// tokio's 2 MB default -- smaller than a main thread's, on the very code path
/// that runs while the user is typing.
///
/// 256 MB against ~110 KB per nesting level leaves room for a couple of
/// thousand levels -- comfortably beyond the parser's own depth limit, which is
/// what should reject over-deep input, and with a wide margin for the deeper
/// frames of the backend.
pub const STACK_SIZE: usize = 256 * 1024 * 1024;

/// Run `f` on a thread with [`STACK_SIZE`] of stack and return its value.
///
/// Panics are propagated to the caller by resuming them on the original thread,
/// so `catch_unwind`-based reporting and the process exit code behave exactly as
/// they would if `f` had run inline.
pub fn run<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let handle = thread::Builder::new()
        .name("juxc".to_string())
        .stack_size(STACK_SIZE)
        .spawn(f)
        // A thread that cannot be spawned is an environment failure, not a
        // compile error; there is nothing useful to fall back to.
        .expect("spawning the compilation thread");
    match handle.join() {
        Ok(value) => value,
        // Re-raise on this thread so the panic hook and exit code are unchanged.
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
