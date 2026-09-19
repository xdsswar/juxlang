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
use clap::{Parser, ValueEnum};

/// How diagnostics are rendered (JUX-DIAGNOSTICS-ADDENDUM §D.1, §D.2). See
/// `juxc_driver::render` for each format; without the flag, a terminal gets
/// `human` and anything else gets `line` (ERRATA E72).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum DiagnosticFormat {
    /// The multi-line block with source frames (§D.1.3).
    Human,
    /// One line per diagnostic and per label (§D.1.4).
    Compact,
    /// One line per diagnostic (§D.1.5).
    Short,
    /// `file:line:col: [E0xxx] severity: message`, the historical form.
    Line,
    /// NDJSON on stdout (§D.2).
    Json,
}

impl DiagnosticFormat {
    fn to_render(self) -> juxc_driver::render::DiagnosticFormat {
        use juxc_driver::render::DiagnosticFormat as R;
        match self {
            DiagnosticFormat::Human => R::Human,
            DiagnosticFormat::Compact => R::Compact,
            DiagnosticFormat::Short => R::Short,
            DiagnosticFormat::Line => R::Line,
            DiagnosticFormat::Json => R::Json,
        }
    }
}

/// `--color`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum ColorArg {
    /// Color on a terminal, unless `NO_COLOR` is set.
    Auto,
    /// Always color.
    Always,
    /// Never color.
    Never,
}

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

    /// Run the front end only (lex → parse → resolve → tycheck) and report
    /// diagnostics — emit NO Rust crate and never touch `cargo` or the
    /// filesystem. This is the path editor tooling / CI lint use; combine
    /// with `--diagnostic-format json` for machine-readable output. Exits
    /// non-zero iff an error-severity diagnostic fired.
    #[arg(long)]
    check: bool,

    /// Diagnostic output format (§D.1, §D.2): `human` (source frames),
    /// `compact`, `short`, `line`, or `json` (NDJSON on stdout, then a
    /// `{"summary":…}` line). Default: `human` on a terminal, else `line`.
    #[arg(long, value_enum)]
    diagnostic_format: Option<DiagnosticFormat>,

    /// Color the `human` format: `auto` (a terminal without `NO_COLOR`),
    /// `always`, or `never`.
    #[arg(long, value_enum, default_value_t = ColorArg::Auto)]
    color: ColorArg,
}

fn main() -> Result<ExitCode> {
    // `juxc explain <CODE>` (§D.5.3) takes no inputs, so it is answered
    // before the compile arguments are parsed.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("explain") {
        return Ok(explain(args.get(2).map(String::as_str)));
    }
    let cli = Cli::parse();
    // Kept for the report: `cli` is moved onto the compilation thread below,
    // and an ICE needs to name the files that were being compiled.
    let inputs = cli.inputs.clone();
    // A panic anywhere in the compiler is a bug in the compiler, and it should
    // say so rather than dumping a Rust backtrace that reads like the user's
    // program crashed -- see `juxc_driver::ice`.
    juxc_driver::ice::guard("juxc", &inputs, move || {
        // The front end recurses in step with source nesting and its frames are
        // large; on a default 8 MB main stack that caps out around 60 levels of
        // nested expression and then aborts the process with no diagnostic. Give
        // it room -- see `juxc_driver::big_stack`.
        juxc_driver::big_stack::run(move || {
            juxc_driver::ice::selftest_trip();
            run_juxc(cli).map(|c| c.unwrap_or(ExitCode::SUCCESS))
        })
    })
}

/// Real `main` body — returns `Ok(Some(code))` when we want to forward an
/// exit code (from the user's emitted binary), `Ok(None)` for ordinary
/// success, and `Err` for fatal driver failures.
fn run_juxc(cli: Cli) -> Result<Option<ExitCode>> {
    // Resolve the input list into a flat set of `.jux` file paths.
    // A single directory expands into every `.jux` inside it
    // (recursive), so users can point `juxc` at a project root.
    // `--check` (no-emit) additionally loads the project's `.jux-stubs/` foreign
    // declarations so `import rust.<crate>.*` resolves for editor tooling. Emit
    // modes (`--build`/`--run`) do NOT: `juxc` can't generate the dependency
    // manifest or link the crate (that's `jux`'s job), so a bare `juxc --build`
    // on a project with `rust.*` deps fails cleanly at the import rather than
    // emitting Rust that won't link.
    let files = collect_input_files(&cli.inputs, cli.check)?;
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
    // The build `@cfg` is decided against: this target and optimization, and
    // the project's features when there is a project.
    let facts = match &manifest {
        Some(m) => juxc_driver::project::cfg_facts_for(m, cli.release),
        None => juxc_driver::CfgFacts::new(cli.release, profile),
    };

    // Check-only mode (editor tooling / CI lint): run the front end, report
    // diagnostics, and stop — no crate is emitted and `cargo` is never
    // invoked, so this is safe to run on every keystroke. Diagnostics go to
    // stdout in JSON mode (the consumer reads stdout) and stderr otherwise.
    if cli.check {
        let result = juxc_driver::check_workspace_cfg(sources, &facts);
        report(&cli, &result.diagnostics, &result.sources);
        let any_error = result
            .diagnostics
            .iter()
            .any(|d| matches!(d.severity, juxc_diagnostics::Severity::Error));
        return Ok(if any_error { Some(ExitCode::from(1)) } else { None });
    }

    let result = juxc_driver::compile_workspace_cfg(sources, &facts)?;

    // Surface diagnostics. Human form goes to stderr so stdout stays clean
    // (important when --run forwards the user program's output); JSON goes to
    // stdout as the canonical machine-readable stream.
    report(&cli, &result.diagnostics, &result.sources);

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

    // Neither `--build` nor `--run`: write the crate and stop. This is the
    // documented way to read the lowering without invoking cargo, and it used
    // to return before writing anything at all.
    if !cli.build && !cli.run {
        juxc_driver::write_crate_with_manifest(
            &crate_,
            &emit_dir,
            &crate_name,
            manifest.as_ref(),
        )?;
        eprintln!(
            "juxc: 0 diagnostics, lowered to {} (use --build to compile)",
            emit_dir.display(),
        );
        return Ok(None);
    }

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
fn collect_input_files(inputs: &[PathBuf], include_stubs: bool) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in inputs {
        if path.is_file() {
            out.push(path.clone());
            continue;
        }
        if path.is_dir() {
            walk_dir_for_jux(path, &mut out, include_stubs)?;
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
fn walk_dir_for_jux(dir: &Path, out: &mut Vec<PathBuf>, include_stubs: bool) -> Result<()> {
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
            // Skip hidden / build dirs (`.git`, `.idea`, `target`). In
            // stub-loading mode (`--check`), DO descend into `.jux-stubs/`, the
            // generated foreign-declaration tree (`rust.<crate>` / Jux-dep
            // stubs) that makes `import rust.<crate>.*` resolve.
            let is_stub_dir = name == ".jux-stubs";
            if (name.starts_with('.') && !(include_stubs && is_stub_dir))
                || name == "target"
            {
                continue;
            }
            walk_dir_for_jux(&path, out, include_stubs)?;
        } else if file_type.is_file() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            // Always collect `.jux` source. In stub-loading mode also collect
            // `.jux.d` declaration stubs (extension is `d`, so the plain `.jux`
            // test misses them); these carry the extern decls for foreign crates
            // so a bare `juxc --check <projectDir>` (the IntelliJ always-on
            // semantic check) resolves `rust.<crate>` imports — the same decls
            // `jux build` passes explicitly. NOT loaded for emit modes: `juxc`
            // can't link the crate, so a build would emit Rust that fails to
            // compile; failing earlier at the unresolved import is clearer.
            if name.ends_with(".jux") || (include_stubs && name.ends_with(".jux.d")) {
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
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
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

/// Print `diagnostics` in the format the command line asked for. Text formats
/// go to stderr so stdout stays the program's own when `--run` forwards it;
/// JSON goes to stdout, the stream a tool reads (§D.2).
fn report(
    cli: &Cli,
    diagnostics: &[juxc_diagnostics::Diagnostic],
    sources: &[juxc_source::SourceFile],
) {
    use std::io::IsTerminal;
    let terminal = std::io::stderr().is_terminal();
    let format = cli
        .diagnostic_format
        .map(DiagnosticFormat::to_render)
        .unwrap_or_else(|| juxc_driver::render::DiagnosticFormat::default_for(terminal));
    if format == juxc_driver::render::DiagnosticFormat::Json {
        print!("{}", juxc_driver::render::render_json(diagnostics, sources, 0));
        return;
    }
    let color = match cli.color {
        ColorArg::Auto => juxc_driver::render::ColorChoice::Auto,
        ColorArg::Always => juxc_driver::render::ColorChoice::Always,
        ColorArg::Never => juxc_driver::render::ColorChoice::Never,
    }
    .enabled(terminal);
    eprint!("{}", juxc_driver::render::render_text(diagnostics, sources, format, color));
}

/// `juxc explain <CODE>`: print the bundled documentation of one code.
fn explain(code: Option<&str>) -> ExitCode {
    let Some(code) = code else {
        eprintln!("usage: juxc explain <CODE>   (for example `juxc explain E0413`)");
        return ExitCode::from(2);
    };
    match juxc_driver::explain::explain(code) {
        Some(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!(
                "juxc: `{}` is not a diagnostic code this compiler knows",
                juxc_driver::explain::normalize(code),
            );
            ExitCode::from(1)
        }
    }
}
