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
    /// One or more `.jux` source files, OR a single directory that's
    /// walked recursively for `.jux` files. Multiple sources are
    /// compiled together as one workspace — cross-file `import`s
    /// resolve, and package-private visibility is enforced across
    /// the unit boundary.
    #[arg(required = true, num_args = 1..)]
    inputs: Vec<PathBuf>,

    /// Directory to write the emitted Rust crate into. Defaults to
    /// `target/.rust-build/` next to the first input file's parent.
    #[arg(long)]
    emit_dir: Option<PathBuf>,

    /// Name of the produced binary. When omitted, defaults to the
    /// input's file-stem (single file) or directory name (folder
    /// input). The name flows into the emitted Cargo.toml and
    /// drives the lookup of the resulting `.exe`.
    #[arg(long)]
    name: Option<String>,

    /// After lowering, run `cargo build` on the emitted crate.
    #[arg(long)]
    build: bool,

    /// After building, execute the produced binary, forwarding stdout/
    /// stderr and the exit code. Implies `--build`.
    #[arg(long)]
    run: bool,

    /// Build the emitted program in release mode (forwards `--release`
    /// to the inner `cargo build`). The produced binary lands under
    /// `target/release/` instead of `target/debug/`. Has no effect
    /// without `--build` or `--run`.
    #[arg(long)]
    release: bool,
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    run_juxc(cli).map(|c| c.unwrap_or(ExitCode::SUCCESS))
}

/// Real `main` body — returns `Ok(Some(code))` when we want to forward an
/// exit code (from the user's emitted binary), `Ok(None)` for ordinary
/// success, and `Err` for fatal driver failures.
fn run_juxc(cli: Cli) -> Result<Option<ExitCode>> {
    // Resolve the input list into a flat set of `.jux` file paths.
    // A single directory expands into every `.jux` inside it
    // (recursive), so users can point `juxc` at a project root.
    let files = collect_input_files(&cli.inputs)?;
    if files.is_empty() {
        anyhow::bail!("no `.jux` source files found in the given inputs");
    }

    // Load every file's contents.
    let mut sources: Vec<juxc_source::SourceFile> = Vec::with_capacity(files.len());
    for path in &files {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        sources.push(juxc_source::SourceFile::new(path.clone(), contents));
    }
    // Discover the project root (nearest ancestor with a `jux.toml`) and load
    // its manifest up front: the `[build] profile` drives front-end profile
    // rules (e.g. `jux-core` rejects `async`, E0701), and the metadata feeds
    // the emitted Cargo.toml later. `None` for a loose file outside any project.
    let project_root = files[0].parent().and_then(find_project_root);
    let manifest = project_root.as_deref().and_then(juxc_driver::Manifest::load);
    let profile = manifest.as_ref().map(|m| m.profile).unwrap_or_default();

    let result = juxc_driver::compile_workspace_with(sources, profile)?;

    // Surface diagnostics. We print to stderr so stdout stays clean
    // (important when --run forwards the user program's output).
    print_diagnostics(&result.diagnostics, &result.sources);

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

    // Decide where to write the emitted crate. Default:
    // `target/.rust-build/` next to the FIRST input file's
    // containing directory. (Multi-file workspaces still share a
    // single emit target — there's only ever one output crate.)
    let emit_dir = cli
        .emit_dir
        .unwrap_or_else(|| default_emit_dir(&files[0]));

    // `project_root` / `manifest` were discovered up front (before the compile)
    // so the front end could enforce profile rules; reuse them here for the
    // emitted Cargo.toml metadata.

    // Decide the produced binary's name. Priority:
    // 1. `--name` (explicit one-off override).
    // 2. The manifest's first `[[bin]].name` — the manifest-driven
    //    name (so a `[[bin]] name="myapp"` project emits `myapp.exe`,
    //    not the legacy stem-derived name). §B.2 Phase 1.
    // 3. The input's file-stem / directory name (loose-file default).
    let crate_name = cli
        .name
        .clone()
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|m| m.bins.first())
                .map(|b| b.name.clone())
        })
        .unwrap_or_else(|| default_crate_name(&cli.inputs, &files[0]));

    let artifact = juxc_driver::build_with_manifest(
        &crate_,
        &emit_dir,
        &crate_name,
        cli.release,
        manifest.as_ref(),
    )?;
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

/// Flatten the user's input list into a deduplicated, sorted vector
/// of concrete `.jux` file paths.
///
/// - A path naming a `.jux` file is added as-is.
/// - A directory is walked recursively; every `.jux` file inside is
///   collected. Hidden directories (starting with `.`) are skipped so
///   `target/`-style trees don't sneak in.
/// - Any other shape is an error — we don't try to second-guess what
///   the user typed.
///
/// Sort order is path-lexicographic, which keeps diagnostic and
/// emission ordering reproducible across runs.
fn collect_input_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in inputs {
        if path.is_file() {
            out.push(path.clone());
            continue;
        }
        if path.is_dir() {
            walk_dir_for_jux(path, &mut out)?;
            continue;
        }
        anyhow::bail!("input `{}` is not a file or directory", path.display());
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Recursive directory walk used by [`collect_input_files`]. Visits
/// every entry in `dir`; descends into subdirectories whose names
/// don't start with `.` (skipping `target`, `.git`, etc.).
fn walk_dir_for_jux(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("file type for {}", path.display()))?;
        if file_type.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" {
                continue;
            }
            walk_dir_for_jux(&path, out)?;
        } else if file_type.is_file() {
            if path.extension().and_then(|s| s.to_str()) == Some("jux") {
                out.push(path);
            }
        }
    }
    Ok(())
}

/// Pick a default crate name from the user's input paths.
///
/// Rule:
/// - If the user passed a single argument that is a directory,
///   use the directory's name (e.g. `examples/showcase` → `showcase`).
/// - Otherwise, use the first source file's stem
///   (e.g. `app.jux` → `app`).
///
/// The result is sanitized: invalid Cargo-name characters are
/// replaced with `_`, and a leading digit is prefixed with `_`.
/// Empty results fall back to the legacy `"jux_emitted"`.
fn default_crate_name(inputs: &[PathBuf], first_file: &Path) -> String {
    let raw = if inputs.len() == 1 && inputs[0].is_dir() {
        inputs[0]
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    } else {
        first_file
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    };
    let raw = raw.unwrap_or_default();
    let mut out: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        return "jux_emitted".to_string();
    }
    if out.chars().next().map_or(false, |c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Default emit directory: `<project root>/target/.rust-build/`.
///
/// The project root is the nearest ancestor of the input that contains a
/// `jux.toml` (the project manifest, §B.2). This keeps the generated `target/`
/// at the project root — outside `src/` — so running `src/main.jux` doesn't
/// scatter a `target/` inside the source tree. When no `jux.toml` is found
/// (a loose file compiled outside any project), we fall back to the input
/// file's own directory.
///
/// Putting it under `target/` makes the standard ignore rule cover the
/// generated files. The `.rust-build/` suffix matches what the build-system
/// addendum §B.15.4 names for Phase 1 emissions.
fn default_emit_dir(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let root = find_project_root(parent).unwrap_or_else(|| parent.to_path_buf());
    root.join("target").join(".rust-build")
}

/// Walk upward from `start` looking for the nearest directory that contains a
/// `jux.toml`. Returns that directory (the project root), or `None` if none is
/// found before reaching the filesystem root.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join("jux.toml").is_file() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// Pretty-print one diagnostic per line in a stable, line-oriented format.
///
/// When a diagnostic carries a `file` index (into `sources`) and a primary
/// span, we render `path:line:col: [Code] severity: message` so the user can
/// jump straight to the offending file — important in multi-file workspaces
/// where the same message could come from any unit. Diagnostics without file
/// identity or a span fall back to the bare `[Code] severity: message` form.
fn print_diagnostics(
    diagnostics: &[juxc_diagnostics::Diagnostic],
    sources: &[juxc_source::SourceFile],
) {
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

/// Human label for a [`juxc_diagnostics::Severity`] level.
fn severity_label(s: juxc_diagnostics::Severity) -> &'static str {
    match s {
        juxc_diagnostics::Severity::Error => "error",
        juxc_diagnostics::Severity::Warning => "warning",
        juxc_diagnostics::Severity::Note => "note",
        juxc_diagnostics::Severity::Help => "help",
    }
}
