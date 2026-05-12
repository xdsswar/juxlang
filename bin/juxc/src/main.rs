//! `juxc` — the Jux compiler binary.
//!
//! Per `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.11: `juxc` is the file-level
//! compiler. It does **not** read `jux.toml` or resolve dependencies —
//! that's the job of the `jux` project tool, which dispatches to this
//! binary. Tooling (LSP, Bazel/Buck integrations) may also invoke `juxc`
//! directly.
//!
//! ## Flags
//!
//! - **`<input>`** — path to the `.jux` source file.
//! - **`--emit-dir <dir>`** — where the emitted Rust crate is written.
//!   Default: `target/.rust-build/` relative to the input file's parent.
//! - **`--build`** — invoke `cargo build` on the emitted crate after
//!   lowering. Without this, juxc stops after emitting Rust source.
//! - **`--run`** — implies `--build`; spawns the produced binary and
//!   forwards its stdout/stderr/exit-code. This is the smoke-test path
//!   for milestone 1 — `juxc --run examples/hello.jux` should print
//!   "Hello, world!" and exit 0.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "juxc", version, about = "The Jux compiler")]
struct Cli {
    /// The `.jux` source file to compile.
    input: PathBuf,

    /// Directory to write the emitted Rust crate into. Defaults to
    /// `target/.rust-build/` next to the input file's parent.
    #[arg(long)]
    emit_dir: Option<PathBuf>,

    /// After lowering, run `cargo build` on the emitted crate.
    #[arg(long)]
    build: bool,

    /// After building, execute the produced binary, forwarding stdout/
    /// stderr and the exit code. Implies `--build`.
    #[arg(long)]
    run: bool,
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    run_juxc(cli).map(|c| c.unwrap_or(ExitCode::SUCCESS))
}

/// Real `main` body — returns `Ok(Some(code))` when we want to forward an
/// exit code (from the user's emitted binary), `Ok(None)` for ordinary
/// success, and `Err` for fatal driver failures.
fn run_juxc(cli: Cli) -> Result<Option<ExitCode>> {
    // Load and compile the input file through every implemented phase.
    let contents = std::fs::read_to_string(&cli.input)
        .with_context(|| format!("reading {}", cli.input.display()))?;
    let source = juxc_source::SourceFile::new(cli.input.clone(), contents);
    let result = juxc_driver::compile(source)?;

    // Surface diagnostics. We print to stderr so stdout stays clean
    // (important when --run forwards the user program's output).
    print_diagnostics(&result.diagnostics);

    // If any error fired, bail out with a non-success exit code. The
    // emitted crate (if any) is not produced when errors are present.
    let any_error = result
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, juxc_diagnostics::Severity::Error));
    if any_error {
        return Ok(Some(ExitCode::from(1)));
    }

    let Some(crate_) = result.crate_ else {
        // No errors but no crate — only happens if the input compiled to
        // an empty unit. Nothing to do beyond reporting clean.
        eprintln!("juxc: 0 diagnostics (nothing to emit)");
        return Ok(None);
    };

    // If neither --build nor --run was passed, we stop after emitting
    // source. Useful for inspecting lowering without invoking cargo.
    if !cli.build && !cli.run {
        eprintln!("juxc: 0 diagnostics, lowering complete (use --build to compile)");
        return Ok(None);
    }

    // Decide where to write the emitted crate. Default: `target/.rust-build/`
    // next to the input file's containing directory.
    let emit_dir = cli.emit_dir.unwrap_or_else(|| default_emit_dir(&cli.input));

    let artifact = juxc_driver::build(&crate_, &emit_dir)?;
    eprintln!("juxc: built {}", artifact.binary_path.display());

    if cli.run {
        // Spawn the emitted binary, inheriting our stdio so the user
        // sees its output directly. Forward whatever exit code it
        // produces.
        let status = Command::new(&artifact.binary_path)
            .status()
            .with_context(|| format!("running {}", artifact.binary_path.display()))?;
        let code = status.code().unwrap_or(1) as u8;
        return Ok(Some(ExitCode::from(code)));
    }

    Ok(None)
}

/// Default emit directory: `<input parent>/target/.rust-build/`.
///
/// Putting it under `target/` makes Cargo's `.gitignore` rule
/// automatically cover the generated files. The `.rust-build/`
/// suffix matches what the build-system addendum §B.15.4 names
/// for Phase 1 emissions.
fn default_emit_dir(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    parent.join("target").join(".rust-build")
}

/// Pretty-print one diagnostic per line in a stable, line-oriented format.
/// Format: `[E0xxx] error: message at file:line:col`.
fn print_diagnostics(diagnostics: &[juxc_diagnostics::Diagnostic]) {
    for d in diagnostics {
        eprintln!("[{}] {}: {}", d.code, severity_label(d.severity), d.message);
    }
}

/// Human label for a [`juxc_diagnostics::Severity`] level.
fn severity_label(s: juxc_diagnostics::Severity) -> &'static str {
    match s {
        juxc_diagnostics::Severity::Error => "error",
        juxc_diagnostics::Severity::Warning => "warning",
        juxc_diagnostics::Severity::Note => "note",
        juxc_diagnostics::Severity::Help => "help",
    }
}
