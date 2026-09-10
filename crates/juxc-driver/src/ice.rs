//! Internal compiler error reporting.
//!
//! A compiler is allowed to have bugs. It is not allowed to hide them, and it
//! is not allowed to blame the user for them. Until this module existed, a
//! panic anywhere inside `juxc` reached the terminal as Rust's default hook
//! wrote it:
//!
//! ```text
//! thread 'juxc' panicked at crates/juxc-tycheck/src/check.rs:1204:9:
//! index out of bounds: the len is 3 but the index is 7
//! note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
//! ```
//!
//! Everything a person needs is missing from that. It does not say which `.jux`
//! file was being compiled, it does not say which compiler version produced it,
//! it does not say the failure is the compiler's fault rather than the
//! program's, and it does not say where to report it. It also looks exactly
//! like a crash in the user's own code, which is the worst part: someone
//! reading it starts debugging their program.
//!
//! ## How it works
//!
//! Two halves, because the information is split across two places.
//!
//! The panic *location* is only available inside a panic hook, at the site of
//! the panic, on whichever thread panicked. The panic *payload* is only
//! available to whoever catches the unwind. So [`install_hook`] records the
//! location into a slot and stays silent, and [`guard`] catches the unwind on
//! the main thread and renders both together.
//!
//! That split is why [`crate::big_stack::run`] resumes the panic on the calling
//! thread instead of swallowing it. Its doc comment has always said the design
//! was for "`catch_unwind`-based reporting"; this is the reporting.
//!
//! ## `RUST_BACKTRACE`
//!
//! The hook suppresses Rust's default output, which would otherwise print
//! alongside this report and say the same thing twice in a less useful way.
//! When `RUST_BACKTRACE` is set to anything but `0`, the default hook is
//! chained after ours, so asking for a backtrace still gets one. That is the
//! same bargain rustc strikes.

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Mutex, OnceLock};

use anyhow::Result;

/// Exit code for a compile that ended in an internal compiler error.
///
/// 101 is what a panicking Rust process already exits with, so nothing that
/// shells out to `juxc` sees a behaviour change. The difference is that it is
/// now a documented contract rather than an accident of the runtime: a caller
/// can tell "the compiler broke" (101) from "the program did not compile"
/// (`ExitCode::FAILURE`) and from whatever the user's own binary returned under
/// `--run`.
pub const ICE_EXIT_CODE: u8 = 101;

/// Where to send a report. Read from the workspace manifest so a fork does not
/// point people at this repository.
const ISSUES_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues");

/// Set to any non-empty value to make [`guard`] panic on purpose, so the ICE
/// path itself can be tested end to end.
///
/// This is deliberately an environment variable and not a CLI flag: the flag
/// list is a user-facing surface and a self-test hook does not belong in it.
const SELFTEST_VAR: &str = "JUX_ICE_SELFTEST";

/// The `file:line:column` of the most recent panic, recorded by the hook.
///
/// A `Mutex` rather than a thread-local because the panic happens on the
/// large-stack `juxc` thread and the report is rendered on the main one.
static PANIC_SITE: Mutex<Option<String>> = Mutex::new(None);

/// Guards against installing the hook more than once. `jux` calls into driver
/// code that could reasonably want its own guard some day; installing twice
/// would chain our hook to itself and print the report twice.
static HOOK: OnceLock<()> = OnceLock::new();

/// Install the panic hook that records the panic site and suppresses Rust's
/// default report.
///
/// Idempotent. [`guard`] calls it, so a binary that uses `guard` does not need
/// to call this itself.
pub fn install_hook() {
    HOOK.get_or_init(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if let Some(location) = info.location() {
                // A poisoned lock here means a previous panic died while
                // holding it. There is nothing useful to do about that while
                // already panicking, so the site is simply lost and the report
                // says "unknown".
                if let Ok(mut site) = PANIC_SITE.lock() {
                    *site = Some(format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    ));
                }
            }
            if backtrace_requested() {
                default_hook(info);
            }
        }));
    });
}

/// Run `f`, turning any panic inside it into an internal compiler error report
/// on stderr and [`ICE_EXIT_CODE`].
///
/// `tool` is the binary name as the user typed it (`juxc` or `jux`) and
/// `inputs` are the paths that were being compiled, both of which go into the
/// report so an issue arrives with enough to reproduce it.
///
/// Errors returned by `f` are ordinary compile failures and pass through
/// untouched; only a panic is treated as an ICE.
pub fn guard<F>(tool: &str, inputs: &[PathBuf], f: F) -> Result<ExitCode>
where
    F: FnOnce() -> Result<ExitCode>,
{
    install_hook();
    // `AssertUnwindSafe` because the closure owns the parsed command line and
    // nothing observes it after the unwind: this process is about to exit.
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => {
            let site = PANIC_SITE.lock().ok().and_then(|s| s.clone());
            eprint!(
                "{}",
                render_report(tool, &message_of(payload.as_ref()), site.as_deref(), inputs)
            );
            Ok(ExitCode::from(ICE_EXIT_CODE))
        }
    }
}

/// Panic on purpose when [`SELFTEST_VAR`] is set, so the ICE path can be
/// exercised end to end.
///
/// Call this from *inside* the large-stack closure, not from [`guard`]. A panic
/// raised on the main thread would never travel through
/// [`crate::big_stack::run`]'s `resume_unwind`, which is the half of the path
/// most likely to break: the hook that records the location runs on the
/// compilation thread while the report is rendered on the main one. Tripping it
/// anywhere else would test the easy half and leave the real one uncovered.
pub fn selftest_trip() {
    if std::env::var_os(SELFTEST_VAR).is_some_and(|v| !v.is_empty()) {
        panic!("{SELFTEST_VAR} was set");
    }
}

/// Render the report. Separated from [`guard`] so its wording can be tested
/// without arranging a real panic.
///
/// `site` is the `file:line:column` inside the compiler, absent when the panic
/// carried no location.
pub fn render_report(
    tool: &str,
    message: &str,
    site: Option<&str>,
    inputs: &[PathBuf],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "internal compiler error: {tool} panicked and could not continue\n\n"
    ));
    out.push_str(&format!("  message:  {message}\n"));
    out.push_str(&format!("  location: {}\n", site.unwrap_or("unknown")));
    out.push_str(&format!(
        "  version:  {tool} {}\n",
        env!("CARGO_PKG_VERSION")
    ));
    for (i, input) in inputs.iter().enumerate() {
        let label = if i == 0 { "input:   " } else { "         " };
        out.push_str(&format!("  {label} {}\n", display_path(input)));
    }
    out.push('\n');
    out.push_str("This is a bug in the Jux compiler, not in your program. Nothing you wrote\n");
    out.push_str("can be at fault here: a source file the compiler rejects should produce a\n");
    out.push_str("diagnostic, never a crash.\n\n");
    out.push_str(&format!(
        "Please report it at {ISSUES_URL}, with the source that triggered it.\n"
    ));
    if !backtrace_requested() {
        out.push_str("Re-run with RUST_BACKTRACE=1 to include a backtrace in the report.\n");
    }
    out
}

/// Pull a human-readable string out of a panic payload.
///
/// `panic!("...")` with no arguments carries a `&'static str`; with formatting
/// it carries a `String`. Anything else came from `panic_any` and there is
/// nothing sensible to print.
fn message_of(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panicked with a non-string payload".to_string()
    }
}

/// Whether the user asked for a backtrace.
///
/// Matches the runtime's own reading of the variable: set and not `0`.
fn backtrace_requested() -> bool {
    match std::env::var("RUST_BACKTRACE") {
        Ok(value) => value != "0",
        Err(_) => false,
    }
}

/// Render a path for the report, preferring a path relative to the working
/// directory so the line stays short and does not leak a home directory into a
/// public issue.
fn display_path(path: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything an issue needs has to be in the text: what broke, where in
    /// the compiler, which version, which input, and where to report it.
    #[test]
    fn report_carries_what_an_issue_needs() {
        let report = render_report(
            "juxc",
            "index out of bounds",
            Some("crates/juxc-tycheck/src/check.rs:1204:9"),
            &[PathBuf::from("examples/hello.jux")],
        );
        assert!(report.contains("internal compiler error"), "{report}");
        assert!(report.contains("index out of bounds"), "{report}");
        assert!(report.contains("check.rs:1204:9"), "{report}");
        assert!(report.contains(env!("CARGO_PKG_VERSION")), "{report}");
        assert!(report.contains("hello.jux"), "{report}");
        assert!(report.contains(ISSUES_URL), "{report}");
    }

    /// The point of the report is that it does not read as the user's fault.
    #[test]
    fn report_says_whose_fault_it_is() {
        let report = render_report("jux", "boom", None, &[]);
        assert!(report.contains("not in your program"), "{report}");
        assert!(report.contains("location: unknown"), "{report}");
    }

    /// House style: no em-dashes in anything a user reads.
    #[test]
    fn report_has_no_em_dash() {
        let report = render_report("juxc", "boom", Some("a.rs:1:1"), &[PathBuf::from("a.jux")]);
        assert!(!report.contains('\u{2014}'), "{report}");
    }

    /// Every input is listed, not just the first, because a multi-file compile
    /// needs all of them to reproduce.
    #[test]
    fn report_lists_every_input() {
        let report = render_report(
            "juxc",
            "boom",
            None,
            &[PathBuf::from("a.jux"), PathBuf::from("b.jux")],
        );
        assert!(report.contains("a.jux"), "{report}");
        assert!(report.contains("b.jux"), "{report}");
    }

    /// A returned error is an ordinary compile failure and must not be dressed
    /// up as a compiler bug.
    #[test]
    fn guard_passes_errors_through() {
        let result = guard("juxc", &[], || anyhow::bail!("ordinary failure"));
        assert_eq!(result.unwrap_err().to_string(), "ordinary failure");
    }

    /// And a success is left alone.
    #[test]
    fn guard_passes_success_through() {
        let result = guard("juxc", &[], || Ok(ExitCode::SUCCESS));
        assert!(result.is_ok());
    }
}
