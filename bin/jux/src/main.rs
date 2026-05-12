//! `jux` — the Jux project tool.
//!
//! Per `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.11 and §B.15: `jux` is the
//! Cargo-equivalent project tool. It reads `jux.toml`, resolves
//! dependencies, and dispatches `juxc` invocations. Day-to-day use is
//! `jux`; `juxc` is the file-level compiler invoked by `jux`, by the
//! language server, and by foreign build systems.
//!
//! ## Project mode vs single-file mode
//!
//! The spec's "real" `jux` reads `jux.toml`, walks the package, and
//! dispatches per-file compilation. That **project mode** is not yet
//! implemented — invoking `jux run`/`build`/`check` without a file path
//! prints a "not yet implemented" message and exits non-zero.
//!
//! For early bootstrap we also accept **single-file mode**: `jux run
//! <file.jux>`, `jux build <file.jux>`, `jux check <file.jux>`. These
//! dispatch through the same `juxc-driver` library that `juxc` uses, so
//! the IDE workflow advertised by the spec (`jux run examples/hello.jux`)
//! works today.
//!
//! ## Commands implemented this round
//!
//! - `jux run <file>` — compile, build, execute. Forwards stdio + exit code.
//! - `jux build <file>` — compile + cargo build, don't execute.
//! - `jux check <file>` — lex/parse/resolve/typecheck only, no codegen.
//! - `jux new <name>` — stubbed (per spec §B.15.1).
//! - `jux test` — stubbed.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use juxc_diagnostics::{Diagnostic, Severity};

#[derive(Parser, Debug)]
#[command(name = "jux", version, about = "The Jux project tool")]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    /// Create a new Jux project. (§B.15.1 — `jux new <name>`.)
    New {
        /// Name of the project to scaffold.
        name: String,
    },
    /// Type-check the project (or a single file) without producing a binary.
    /// (§B.15 — `jux check`.)
    Check {
        /// Optional `.jux` file. When omitted, runs in project mode
        /// (currently not yet implemented).
        file: Option<PathBuf>,
    },
    /// Build the project (or a single file). (§B.15 — `jux build`.)
    Build {
        /// Optional `.jux` file.
        file: Option<PathBuf>,
        /// Where to emit the generated Rust crate. Defaults to
        /// `<input-parent>/target/.rust-build/`.
        #[arg(long)]
        emit_dir: Option<PathBuf>,
    },
    /// Build and run the project (or a single file). (§B.15 — `jux run`.)
    Run {
        /// Optional `.jux` file. When omitted, runs in project mode
        /// (currently not yet implemented).
        file: Option<PathBuf>,
        /// Where to emit the generated Rust crate. Defaults to
        /// `<input-parent>/target/.rust-build/`.
        #[arg(long)]
        emit_dir: Option<PathBuf>,
    },
    /// Run tests. (§B.15 — `jux test`.)
    Test,
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        CliCommand::New { name } => not_yet_implemented(format!("jux new {name}")),
        CliCommand::Test         => not_yet_implemented("jux test"),
        CliCommand::Check { file } => {
            run_single_or_project(file, Action::Check, None)
        }
        CliCommand::Build { file, emit_dir } => {
            run_single_or_project(file, Action::Build, emit_dir)
        }
        CliCommand::Run { file, emit_dir } => {
            run_single_or_project(file, Action::Run, emit_dir)
        }
    }
}

/// What kind of pipeline pass to perform in single-file mode.
#[derive(Debug, Clone, Copy)]
enum Action {
    /// Lex/parse/resolve/typecheck only — no Rust emission.
    Check,
    /// Compile + cargo build, don't execute.
    Build,
    /// Compile + cargo build + execute the emitted binary.
    Run,
}

/// Dispatch table: route to single-file mode if `file` is `Some`, else
/// to the project-mode placeholder. `emit_dir` (if any) overrides the
/// default emit directory in single-file mode.
fn run_single_or_project(
    file: Option<PathBuf>,
    action: Action,
    emit_dir: Option<PathBuf>,
) -> Result<ExitCode> {
    match file {
        Some(path) => run_single_file(&path, action, emit_dir),
        None => not_yet_implemented(match action {
            Action::Check => "jux check (project mode)",
            Action::Build => "jux build (project mode)",
            Action::Run => "jux run (project mode)",
        }),
    }
}

/// Compile one `.jux` file through the driver and (optionally) build/run
/// the emitted Rust crate.
///
/// The exit code returned reflects what the user cares about:
/// - Non-zero from juxc diagnostics → 1.
/// - Non-zero from the emitted binary (`Run` only) → forwarded as-is.
/// - Everything else → 0.
fn run_single_file(
    input: &Path,
    action: Action,
    emit_dir_override: Option<PathBuf>,
) -> Result<ExitCode> {
    // Phase 1–N: lex/parse/resolve/typecheck/lower via the shared driver.
    let contents = std::fs::read_to_string(input)
        .with_context(|| format!("reading {}", input.display()))?;
    let source = juxc_source::SourceFile::new(input.to_path_buf(), contents);
    let result = juxc_driver::compile(source)?;

    print_diagnostics(&result.diagnostics);

    let any_error = result.diagnostics.iter().any(|d| matches!(d.severity, Severity::Error));
    if any_error {
        return Ok(ExitCode::from(1));
    }

    match action {
        Action::Check => {
            // No errors and we don't need a binary: the user got what they asked for.
            eprintln!("jux: check ok");
            Ok(ExitCode::SUCCESS)
        }
        Action::Build | Action::Run => {
            // We need the emitted crate. The check path skips lowering when
            // there are errors, but here we expect Some(crate_).
            let Some(crate_) = result.crate_ else {
                // No errors but no crate — only happens for an empty compilation unit.
                eprintln!("jux: nothing to build");
                return Ok(ExitCode::SUCCESS);
            };

            // Caller-provided emit dir wins; otherwise fall back to
            // `<input parent>/target/.rust-build/`, matching juxc's default.
            let emit_dir = emit_dir_override.unwrap_or_else(|| default_emit_dir(input));
            let artifact = juxc_driver::build(&crate_, &emit_dir)?;
            eprintln!("jux: built {}", artifact.binary_path.display());

            if matches!(action, Action::Run) {
                // Spawn the emitted binary with inherited stdio so the
                // user sees its output verbatim. Forward the exit code so
                // CI / IDEs see a non-zero exit when the user's program
                // exits non-zero.
                let status = Command::new(&artifact.binary_path).status().with_context(|| {
                    format!("running {}", artifact.binary_path.display())
                })?;
                let code = status.code().unwrap_or(1) as u8;
                Ok(ExitCode::from(code))
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }
    }
}

/// `<input parent>/target/.rust-build/` — same default `juxc` uses.
fn default_emit_dir(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    parent.join("target").join(".rust-build")
}

/// Pretty-print one diagnostic per line: `[E0xxx] level: message`.
fn print_diagnostics(diagnostics: &[Diagnostic]) {
    for d in diagnostics {
        eprintln!("[{}] {}: {}", d.code, severity_label(d.severity), d.message);
    }
}

fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
        Severity::Help => "help",
    }
}

/// Print a "not yet implemented" message pointing at the relevant spec
/// section, then exit with status 2 so scripts can detect it.
fn not_yet_implemented<S: Into<String>>(what: S) -> Result<ExitCode> {
    eprintln!("jux: {} is not yet implemented", what.into());
    eprintln!("     (see JUX-BUILD-SYSTEM-ADDENDUM.md §B.15 for the full CLI surface)");
    Ok(ExitCode::from(2))
}
