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
        /// Build the emitted program with optimizations
        /// (forwards `--release` to the inner `cargo build`).
        #[arg(long)]
        release: bool,
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
        /// Build the emitted program with optimizations
        /// (forwards `--release` to the inner `cargo build`).
        #[arg(long)]
        release: bool,
    },
    /// Run tests. (§B.15 — `jux test`.)
    Test,
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        CliCommand::New { name } => cmd_new(&name),
        CliCommand::Test         => cmd_test(),
        CliCommand::Check { file } => {
            run_single_or_project(file, Action::Check, None, false)
        }
        CliCommand::Build { file, emit_dir, release } => {
            run_single_or_project(file, Action::Build, emit_dir, release)
        }
        CliCommand::Run { file, emit_dir, release } => {
            run_single_or_project(file, Action::Run, emit_dir, release)
        }
    }
}

/// `jux new <name>` — scaffold a fresh Jux project per §B.2.1.
/// Creates:
///   - `<name>/jux.toml`  with a minimum-viable `[package]` block.
///   - `<name>/src/main.jux` with a "hello world" stub.
///   - `<name>/.gitignore` with the standard ignore list.
///
/// `<name>` is the directory name (and the package name's last
/// segment). Refuses to overwrite an existing directory.
fn cmd_new(name: &str) -> Result<ExitCode> {
    let target = PathBuf::from(name);
    if target.exists() {
        eprintln!("jux: target directory '{}' already exists", target.display());
        return Ok(ExitCode::from(1));
    }
    let src_dir = target.join("src");
    std::fs::create_dir_all(&src_dir).with_context(|| {
        format!("creating project directory {}", src_dir.display())
    })?;
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    std::fs::write(target.join("jux.toml"), manifest)
        .context("writing jux.toml")?;
    let main_jux = "public void main() {\n    print(\"Hello from Jux!\");\n}\n";
    std::fs::write(src_dir.join("main.jux"), main_jux)
        .context("writing src/main.jux")?;
    let gitignore = "/target/\n";
    std::fs::write(target.join(".gitignore"), gitignore)
        .context("writing .gitignore")?;
    eprintln!("jux: created project at {}", target.display());
    eprintln!("     next: `cd {name} && jux run`");
    Ok(ExitCode::SUCCESS)
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
    release: bool,
) -> Result<ExitCode> {
    match file {
        Some(path) => run_single_file(&path, action, emit_dir, release),
        None => run_project(action, emit_dir, release),
    }
}

/// Minimum-viable subset of `jux.toml`'s `[package]` section per
/// §B.2.1 — enough to identify the project and pick a binary name.
/// Fancier knobs (`[lib]`, `[[bin]]`, `[dependencies]`, profiles,
/// features) land when the dep resolver does; today's project
/// mode just walks `src/` and compiles everything.
struct ProjectManifest {
    name: String,
}

/// Parse `jux.toml` at `path` into a `ProjectManifest`. Bails with
/// a clear error when `[package].name` is missing — the spec
/// requires it.
fn read_manifest(path: &Path) -> Result<ProjectManifest> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let value: toml::Value = raw
        .parse()
        .with_context(|| format!("parsing {} as TOML", path.display()))?;
    let pkg = value
        .get("package")
        .and_then(|v| v.as_table())
        .with_context(|| format!("{}: missing [package] table", path.display()))?;
    let name = pkg
        .get("name")
        .and_then(|v| v.as_str())
        .with_context(|| {
            format!("{}: [package].name is required", path.display())
        })?
        .to_string();
    Ok(ProjectManifest { name })
}

/// `jux build`/`run`/`check` without an explicit file: project
/// mode. Reads `./jux.toml`. When the manifest declares a
/// `[workspace]`, every member is built in dependency order
/// (§B.7). Otherwise the single package's `[lib]`/`[[bin]]`
/// targets are built per the manifest (§B.2). The produced
/// binaries are named from `[[bin]].name`.
fn run_project(
    action: Action,
    _emit_dir_override: Option<PathBuf>,
    release: bool,
) -> Result<ExitCode> {
    let cwd = std::env::current_dir().context("getting current directory")?;
    let manifest_path = cwd.join("jux.toml");
    if !manifest_path.exists() {
        eprintln!(
            "jux: no jux.toml found in {} — pass a file or run `jux new <name>` first",
            cwd.display(),
        );
        return Ok(ExitCode::from(1));
    }
    let Some(manifest) = juxc_driver::Manifest::load(&cwd) else {
        eprintln!("jux: failed to load {}", manifest_path.display());
        return Ok(ExitCode::from(1));
    };

    // ---- Workspace mode -------------------------------------------------
    if !manifest.workspace_members.is_empty() {
        return run_workspace(&manifest, action, release);
    }

    // ---- Single-package mode --------------------------------------------
    if manifest.lib.is_none() && manifest.bins.is_empty() {
        eprintln!(
            "jux: manifest declares no [lib] or [[bin]] target, and no src/lib.jux or src/main.jux exists in {}",
            cwd.display(),
        );
        return Ok(ExitCode::from(1));
    }
    let emit_root = cwd.join("target").join(".rust-build");
    let build = juxc_driver::project::build_package(&manifest, &[], &[], &emit_root, release)?;
    print_diagnostics(&build.diagnostics, &build.sources);
    if build.has_errors() {
        return Ok(ExitCode::from(1));
    }
    match action {
        Action::Check => {
            eprintln!("jux: check ok");
            Ok(ExitCode::SUCCESS)
        }
        Action::Build => {
            if let Some(lib) = &build.library {
                eprintln!("jux: built library crate at {}", lib.crate_dir.display());
            }
            for bin in &build.binaries {
                eprintln!("jux: built {}", bin.binary_path.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        Action::Run => {
            let Some(bin) = build.binaries.first() else {
                eprintln!("jux: nothing to run (no [[bin]] target)");
                return Ok(ExitCode::SUCCESS);
            };
            eprintln!("jux: built {}", bin.binary_path.display());
            let status = Command::new(&bin.binary_path).status().with_context(|| {
                format!("running {}", bin.binary_path.display())
            })?;
            let code = status.code().unwrap_or(1) as u8;
            Ok(ExitCode::from(code))
        }
    }
}

/// `jux build`/`run`/`check` for a `[workspace]`-root manifest. Builds
/// every member in dependency order via the driver's project
/// orchestration, then (for `run`) executes the first binary of the last
/// member built.
fn run_workspace(
    root: &juxc_driver::Manifest,
    action: Action,
    release: bool,
) -> Result<ExitCode> {
    if matches!(action, Action::Check) {
        // Check still goes through the full build path (compile only,
        // then report). For brevity we reuse build_workspace and just
        // skip running.
    }
    let ws = juxc_driver::project::build_workspace(root, release)?;
    for (name, build) in &ws.members {
        print_diagnostics(&build.diagnostics, &build.sources);
        if build.has_errors() {
            eprintln!("jux: member `{name}` failed to build");
            return Ok(ExitCode::from(1));
        }
        if let Some(lib) = &build.library {
            eprintln!("jux: [{name}] built library crate at {}", lib.crate_dir.display());
        }
        for bin in &build.binaries {
            eprintln!("jux: [{name}] built {}", bin.binary_path.display());
        }
    }
    if matches!(action, Action::Run) {
        // Run the first binary of the last member (the workspace's
        // "application" sits at the top of the topological order).
        if let Some((_, build)) = ws.members.last() {
            if let Some(bin) = build.binaries.first() {
                let status = Command::new(&bin.binary_path).status().with_context(|| {
                    format!("running {}", bin.binary_path.display())
                })?;
                let code = status.code().unwrap_or(1) as u8;
                return Ok(ExitCode::from(code));
            }
        }
        eprintln!("jux: no runnable [[bin]] target in the workspace");
    }
    Ok(ExitCode::SUCCESS)
}

/// `jux test` — discover `@Test`-annotated free functions
/// across `src/` and `test/`, build a test runner, run it.
/// Returns the runner's exit code so CI sees test failures.
fn cmd_test() -> Result<ExitCode> {
    let cwd = std::env::current_dir().context("getting current directory")?;
    let manifest_path = cwd.join("jux.toml");
    if !manifest_path.exists() {
        eprintln!(
            "jux: no jux.toml found in {} — pass a project directory or run `jux new <name>` first",
            cwd.display(),
        );
        return Ok(ExitCode::from(1));
    }
    let manifest = read_manifest(&manifest_path)?;
    let binary_name = format!(
        "{}_test",
        manifest
            .name
            .rsplit('.')
            .next()
            .unwrap_or(manifest.name.as_str()),
    );
    let mut sources: Vec<juxc_source::SourceFile> = Vec::new();
    let src_dir = cwd.join("src");
    if src_dir.exists() {
        sources.extend(collect_project_sources(&src_dir)?);
    }
    let test_dir = cwd.join("test");
    if test_dir.exists() {
        sources.extend(collect_project_sources(&test_dir)?);
    }
    if sources.is_empty() {
        eprintln!(
            "jux: no .jux sources under src/ or test/ in {}",
            cwd.display(),
        );
        return Ok(ExitCode::from(1));
    }
    let result = juxc_driver::compile_workspace_test(sources)?;
    print_diagnostics(&result.diagnostics, &result.sources);
    let any_error = result
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    if any_error {
        return Ok(ExitCode::from(1));
    }
    let Some(crate_) = result.crate_ else {
        eprintln!("jux: nothing to test");
        return Ok(ExitCode::SUCCESS);
    };
    let emit_dir = cwd.join("target").join(".rust-build-test");
    let artifact = juxc_driver::build(&crate_, &emit_dir, &binary_name, false)?;
    // Run the test binary, inherit stdio so the user sees PASS/FAIL
    // output in real time. Forward the exit code so CI gates work.
    let status = Command::new(&artifact.binary_path)
        .status()
        .with_context(|| format!("running {}", artifact.binary_path.display()))?;
    let code = status.code().unwrap_or(1) as u8;
    Ok(ExitCode::from(code))
}

/// Walk `src_dir` recursively and collect every `.jux` file as a
/// loaded `SourceFile`. Sort order is path-lexicographic so
/// diagnostics line up across runs.
fn collect_project_sources(src_dir: &Path) -> Result<Vec<juxc_source::SourceFile>> {
    let mut paths: Vec<PathBuf> = Vec::new();
    walk_jux_files(src_dir, &mut paths)?;
    paths.sort();
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let contents = std::fs::read_to_string(&p)
            .with_context(|| format!("reading {}", p.display()))?;
        out.push(juxc_source::SourceFile::new(p, contents));
    }
    Ok(out)
}

fn walk_jux_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading dir {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            walk_jux_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jux") {
            out.push(path);
        }
    }
    Ok(())
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
    release: bool,
) -> Result<ExitCode> {
    // Phase 1–N: lex/parse/resolve/typecheck/lower via the shared driver.
    let contents = std::fs::read_to_string(input)
        .with_context(|| format!("reading {}", input.display()))?;
    let source = juxc_source::SourceFile::new(input.to_path_buf(), contents);
    let result = juxc_driver::compile(source)?;

    print_diagnostics(&result.diagnostics, &result.sources);

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
            // The `jux` driver tool always uses the legacy name —
            // `jux.toml`-driven naming is the proper Phase-2 home
            // for this knob. `juxc` itself exposes a `--name`
            // flag for one-off binary-name overrides.
            let artifact = juxc_driver::build(
                &crate_,
                &emit_dir,
                juxc_driver::DEFAULT_CRATE_NAME,
                release,
            )?;
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

/// Pretty-print one diagnostic per line. When the diagnostic carries a `file`
/// index (into `sources`) and a primary span, render
/// `path:line:col: [E0xxx] level: message` so the user can jump straight to
/// the offending file in a multi-file workspace. Otherwise fall back to the
/// bare `[E0xxx] level: message` form.
fn print_diagnostics(diagnostics: &[Diagnostic], sources: &[juxc_source::SourceFile]) {
    for d in diagnostics {
        match (d.file, d.primary_span) {
            (Some(i), Some(span)) if i < sources.len() => {
                let src = &sources[i];
                let (line, col) = src.line_col(span.start as usize);
                eprintln!(
                    "{}:{}:{}: [{}] {}: {}",
                    src.path().display(),
                    line,
                    col,
                    d.code,
                    severity_label(d.severity),
                    d.message,
                );
            }
            _ => {
                eprintln!("[{}] {}: {}", d.code, severity_label(d.severity), d.message);
            }
        }
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
/// section, then exit with status 2 so scripts can detect it. Retained as
/// scaffolding for CLI subcommands still on the roadmap (§B.15).
#[allow(dead_code)]
fn not_yet_implemented<S: Into<String>>(what: S) -> Result<ExitCode> {
    eprintln!("jux: {} is not yet implemented", what.into());
    eprintln!("     (see JUX-BUILD-SYSTEM-ADDENDUM.md §B.15 for the full CLI surface)");
    Ok(ExitCode::from(2))
}
