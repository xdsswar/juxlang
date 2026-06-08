//! Driver — phase orchestration.
//!
//! The driver is `juxc`'s top-level entry point as a library: feed it a
//! source file and out comes either a generated Rust crate ready to compile,
//! or a collection of diagnostics. This corresponds to
//! `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.11.1.
//!
//! ## Pipeline
//!
//! Per the phase table in `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.1.2:
//!
//! 1. [`juxc_lex::lex`]              — bytes → tokens
//! 2. [`juxc_parse::parse`]          — tokens → AST
//! 3. [`juxc_resolve::resolve`]      — bind names against module scope
//! 4. [`juxc_tycheck::typecheck`]    — verify types, resolve overloads
//! 5. [`juxc_backend_rust::lower_with_symbols`] — emit Rust source, fed
//!    tycheck's [`juxc_tycheck::SymbolTable`] (Phase 1 strategy)
//!
//! Additional phases (MIR build, borrow inference, async lowering,
//! monomorph, DCE, …) land between (4) and (5) as they're implemented.
//!
//! ## Build orchestration
//!
//! Beyond [`compile`], the driver also offers [`build`]: write a generated
//! [`juxc_backend_rust::RustCrate`] to disk and invoke `cargo build` on it.
//! This is the bridge from "compile produced source" to "produce an
//! executable", which is the Phase 1 strategy's final step.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use juxc_backend_rust::{RustCrate, CRATE_NAME};
use juxc_diagnostics::{Diagnostic, Severity};
use juxc_source::SourceFile;

/// Re-export the legacy default crate name so binaries that link
/// against `juxc-driver` (but not `juxc-backend-rust` directly)
/// can still pass it to [`build`]. New code should prefer a
/// caller-derived name (e.g. from the input file's stem).
pub const DEFAULT_CRATE_NAME: &str = CRATE_NAME;

mod source_map;
mod stdlib;

/// Top-level compile result.
///
/// On success, `crate_` is populated and ready to hand to [`build`].
/// On failure (any error-severity diagnostic), `crate_` is `None` and
/// `diagnostics` explains why.
pub struct CompileResult {
    /// Generated Rust crate, or `None` if compilation produced any errors.
    pub crate_: Option<RustCrate>,
    /// All diagnostics from every phase, in pipeline order. Each diagnostic's
    /// `file` field (when set) indexes into [`CompileResult::sources`].
    pub diagnostics: Vec<Diagnostic>,
    /// The full workspace source list (stdlib units first, then user units),
    /// in the same order the diagnostics' `file` indices reference. Consumers
    /// map `diagnostic.file` → `sources[i].path()` + `line_col`.
    pub sources: Vec<SourceFile>,
}

/// Compile a workspace of one-or-more source files together.
///
/// Each file is lexed, parsed, and resolved independently, then the
/// per-unit symbol tables are merged into a single workspace
/// SymbolTable that all units share for tycheck. The backend emits a
/// single Rust crate containing every unit's lowered output —
/// cross-file `import`s and package-private access checks resolve
/// against the merged view.
///
/// `sources` must be non-empty. Passing exactly one source produces
/// the same result as the legacy [`compile`] entry point.
pub fn compile_workspace(sources: Vec<SourceFile>) -> Result<CompileResult> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    if sources.is_empty() {
        return Ok(CompileResult { crate_: None, diagnostics, sources: Vec::new() });
    }

    // Auto-prepend `jux.std/*` sources so user code sees the
    // stdlib's `Map`/`List`/`Throwable`/etc. types without having
    // to import them by full path. Mirrors Java's implicit
    // `java.lang.*` rule. Stdlib sources go first so their
    // package modules exist by the time user units reference
    // their types.
    let mut all_sources = stdlib::load_std_sources();
    all_sources.extend(sources);
    let sources = all_sources;

    // Phase 1+2 per source — lex and parse independently. Each source's
    // diagnostics are tagged with that source's index (a length-delta:
    // record `len()` before, set `.file` on everything appended after) so
    // consumers can map a diagnostic back to the file that produced it.
    let mut units: Vec<juxc_ast::CompilationUnit> = Vec::with_capacity(sources.len());
    for (idx, source) in sources.iter().enumerate() {
        let before = diagnostics.len();
        let lex_result = juxc_lex::lex(source);
        diagnostics.extend(lex_result.diagnostics);
        let parsed = juxc_parse::parse(&lex_result.tokens);
        diagnostics.extend(parsed.diagnostics);
        // Resolver runs per-unit; cross-file name resolution happens
        // through the merged symbol table during tycheck.
        let resolved = juxc_resolve::resolve(&parsed.ast);
        diagnostics.extend(resolved.diagnostics);
        for d in &mut diagnostics[before..] {
            d.file = Some(idx);
        }
        units.push(parsed.ast);
    }

    // Phase 6+ — tycheck against the merged workspace. We build one
    // SymbolTable that contains every class/record/enum/interface/
    // function from every unit, then run the per-expression type
    // walker against each unit using that shared table. tycheck tags its
    // own diagnostics with the matching unit/source index.
    let typed = juxc_tycheck::typecheck_workspace(&units);
    diagnostics.extend(typed.diagnostics);

    let has_errors = diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));

    let crate_ = if has_errors {
        None
    } else {
        // Backend: emit one Rust crate with each unit's output
        // wrapped in its package modules. The first source file's
        // path drives source-map markers for the `main` unit; the
        // others get their own markers via per-unit source refs.
        Some(juxc_backend_rust::lower_workspace(
            &units,
            &typed.symbols,
            &typed.expr_types,
            &sources,
        ))
    };

    Ok(CompileResult { crate_, diagnostics, sources })
}

/// `compile_workspace` variant that emits a `jux test` binary
/// instead of a regular `void main()` shim. The produced crate's
/// `fn main` is the test runner — it discovers every `@Test`-
/// annotated free function, runs each one inside
/// `std::panic::catch_unwind`, and exits non-zero if any test
/// fails. Same lex/parse/resolve/tycheck pipeline as
/// [`compile_workspace`]; only the backend emit differs.
pub fn compile_workspace_test(sources: Vec<SourceFile>) -> Result<CompileResult> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    if sources.is_empty() {
        return Ok(CompileResult { crate_: None, diagnostics, sources: Vec::new() });
    }
    // Stdlib auto-prepend (see `compile_workspace` for the
    // rationale) — same shape for the test runner so `@Test`
    // bodies can use `Map`, `Throwable`, etc.
    let mut all_sources = stdlib::load_std_sources();
    all_sources.extend(sources);
    let sources = all_sources;
    let mut units: Vec<juxc_ast::CompilationUnit> = Vec::with_capacity(sources.len());
    for (idx, source) in sources.iter().enumerate() {
        let before = diagnostics.len();
        let lex_result = juxc_lex::lex(source);
        diagnostics.extend(lex_result.diagnostics);
        let parsed = juxc_parse::parse(&lex_result.tokens);
        diagnostics.extend(parsed.diagnostics);
        let resolved = juxc_resolve::resolve(&parsed.ast);
        diagnostics.extend(resolved.diagnostics);
        for d in &mut diagnostics[before..] {
            d.file = Some(idx);
        }
        units.push(parsed.ast);
    }
    let typed = juxc_tycheck::typecheck_workspace(&units);
    diagnostics.extend(typed.diagnostics);
    let has_errors = diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    let crate_ = if has_errors {
        None
    } else {
        Some(juxc_backend_rust::lower_workspace_test(
            &units,
            &typed.symbols,
            &typed.expr_types,
            &sources,
        ))
    };
    Ok(CompileResult { crate_, diagnostics, sources })
}

/// Compile a single source file through every phase that's currently
/// wired up. Returns a [`CompileResult`].
///
/// Returns `Err` only for genuinely fatal conditions (e.g. an internal
/// invariant violation). User-facing errors are reported via
/// [`CompileResult::diagnostics`], not via `Result::Err`.
pub fn compile(source: SourceFile) -> Result<CompileResult> {
    // Route single-source compile through the workspace path so
    // the stdlib auto-loader fires uniformly. The historical
    // single-file shape stays callable but every user now gets
    // `jux.std.*` types in scope without any explicit imports.
    compile_workspace(vec![source])
}

// ============================================================================
// Check-only analysis (tooling / LSP)
// ============================================================================

/// Result of a backend-free [`check`] pass.
///
/// This is what editor tooling (`juxc-lsp`, per `JUX-LSP-SERVER-ADDENDUM.md`)
/// consumes: it stops after type checking, so no Rust source is generated and
/// no `cargo` is invoked. Alongside the diagnostics it returns the merged
/// [`SymbolTable`](juxc_tycheck::SymbolTable) and the per-expression type map,
/// which the language server uses to answer hover / completion / goto-def.
pub struct CheckResult {
    /// Diagnostics from lex, parse, resolve, and tycheck — in pipeline order.
    /// Each diagnostic's `file` field (when set) indexes into
    /// [`CheckResult::sources`].
    pub diagnostics: Vec<Diagnostic>,
    /// Merged workspace symbol table (includes the auto-loaded stdlib).
    pub symbols: juxc_tycheck::SymbolTable,
    /// Per-expression inferred type, keyed by the expression's source span.
    pub expr_types: std::collections::HashMap<juxc_source::Span, juxc_tycheck::Ty>,
    /// The full workspace source list (stdlib units first, then user units),
    /// in the same order the diagnostics' `file` indices reference. The LSP
    /// maps `diagnostic.file` → `sources[i].path()` → `Url` to publish
    /// per-file diagnostics, and resolves spans against the right file's text.
    pub sources: Vec<SourceFile>,
}

/// Run the front end (lex → parse → resolve → tycheck) over `sources`
/// **without** lowering to Rust. Intended for interactive tooling that
/// re-analyses on every edit and must not pay for codegen or `cargo`.
///
/// Like [`compile_workspace`], this auto-prepends the `jux.std.*` sources so
/// user code sees `Map` / `List` / `String` / `Throwable` in scope without an
/// explicit import. Because the stdlib is error-free by construction, every
/// diagnostic returned here originates in `sources`; the language server can
/// therefore attribute them to the open document(s).
///
/// `sources` may be empty, in which case only the stdlib is analysed (and the
/// diagnostics should be empty).
pub fn check_workspace(sources: Vec<SourceFile>) -> CheckResult {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Same stdlib auto-prepend as the compile path: stdlib units go
    // first so their package modules exist when user units reference
    // their types.
    let mut all_sources = stdlib::load_std_sources();
    all_sources.extend(sources);
    let sources = all_sources;

    // Lex + parse + resolve each unit independently. Cross-file name
    // resolution happens through the merged symbol table in tycheck.
    // Each source's diagnostics are tagged with its index (length-delta)
    // so the LSP can publish per-file diagnostics against the right Url.
    let mut units: Vec<juxc_ast::CompilationUnit> = Vec::with_capacity(sources.len());
    for (idx, source) in sources.iter().enumerate() {
        let before = diagnostics.len();
        let lex_result = juxc_lex::lex(source);
        diagnostics.extend(lex_result.diagnostics);
        let parsed = juxc_parse::parse(&lex_result.tokens);
        diagnostics.extend(parsed.diagnostics);
        let resolved = juxc_resolve::resolve(&parsed.ast);
        diagnostics.extend(resolved.diagnostics);
        for d in &mut diagnostics[before..] {
            d.file = Some(idx);
        }
        units.push(parsed.ast);
    }

    // Tycheck against the merged workspace. We keep `symbols` and
    // `expr_types` so the LSP can serve hover/completion/goto without
    // re-running the front end. tycheck tags its own diagnostics with the
    // matching unit/source index.
    let typed = juxc_tycheck::typecheck_workspace(&units);
    diagnostics.extend(typed.diagnostics);

    CheckResult {
        diagnostics,
        symbols: typed.symbols,
        expr_types: typed.expr_types,
        sources,
    }
}

/// Single-document convenience wrapper over [`check_workspace`].
pub fn check(source: SourceFile) -> CheckResult {
    check_workspace(vec![source])
}

// ============================================================================
// Build orchestration
// ============================================================================

/// Result of [`build`]: where the emitted crate lives and where its binary
/// landed after `cargo build`.
pub struct BuildArtifact {
    /// Directory containing the emitted `Cargo.toml` + `src/`.
    pub crate_dir: PathBuf,
    /// Filesystem path to the compiled native binary. On Windows this
    /// ends in `.exe`; elsewhere it's the bare name.
    pub binary_path: PathBuf,
}

/// Materialize a [`RustCrate`] to disk under `crate_dir` and run
/// `cargo build` on it. Returns the path to the resulting binary.
///
/// `crate_dir` is created (with any missing parents) if it doesn't exist.
/// Existing files in `crate_dir` are overwritten — the driver assumes
/// it owns this directory.
///
/// When `release` is `true`, the inner `cargo build` is invoked with
/// `--release`, and the returned `binary_path` points at
/// `target/release/{name}` instead of `target/debug/{name}`.
///
/// On `cargo build` failure, returns `Err` with the captured stderr from
/// cargo so callers can surface it to the user. The juxc-emitted Rust
/// should always compile cleanly; if it doesn't, that's a juxc bug, not
/// a user error.
pub fn build(
    crate_: &RustCrate,
    crate_dir: &Path,
    crate_name: &str,
    release: bool,
) -> Result<BuildArtifact> {
    // Ensure the destination directory and any intermediate `src/`
    // subdirectories exist before writing.
    fs::create_dir_all(crate_dir.join("src"))
        .with_context(|| format!("creating emitted crate dir {}", crate_dir.display()))?;

    // Regenerate Cargo.toml with the user-requested crate name.
    // The backend emits a default-named Cargo.toml during
    // `lower_workspace`; here we override with the CLI-chosen
    // (or auto-derived) name so `target/debug/{name}.exe` matches
    // the user's expectation.
    //
    // The emitted prelude unconditionally references
    // `futures::channel::oneshot` (for the `Task<T>` / `Worker`
    // helpers) and the async-runtime hooks (`block_on`, `join!`),
    // so the `futures` crate is a hard dependency of every
    // emitted Jux crate now — even ones that never touch async.
    // The compile overhead of `futures` on a fresh build is
    // ~3-4 seconds; subsequent builds reuse the cached artifact
    // and pay nothing. The simplification (no conditional dep
    // detection, no source-scanning) is worth the up-front cost.
    let cargo_toml = juxc_backend_rust::cargo_toml_for_with(crate_name, true);
    fs::write(crate_dir.join("Cargo.toml"), &cargo_toml)
        .with_context(|| format!("writing Cargo.toml to {}", crate_dir.display()))?;

    // Write each source file. The backend uses `src/main.rs` for the
    // single binary right now; future emissions may add library crates
    // and tests, so we handle arbitrary nested paths.
    let mut written_rs: Vec<PathBuf> = Vec::with_capacity(crate_.sources.len());
    for (rel_path, content) in &crate_.sources {
        let full = crate_dir.join(rel_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating parent dir for {}", full.display()))?;
        }
        fs::write(&full, content)
            .with_context(|| format!("writing source file {}", full.display()))?;
        if rel_path.ends_with(".rs") {
            written_rs.push(full);
        }
    }

    // Run `rustfmt` on every emitted `.rs` file (Fix 2). Failures here
    // are advisory — generated code already compiles; rustfmt is
    // purely a readability upgrade. We swallow the error and continue
    // so users without rustfmt on `PATH` aren't blocked.
    run_rustfmt(&written_rs);

    // Run cargo build inside the emitted crate. `--quiet` suppresses
    // cargo's "compiling/finished" lines; we surface anything that
    // actually went wrong via the captured stderr. When `release` is
    // set we also pass `--release` so the emitted program is built
    // with optimizations (and lands under `target/release/`).
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--quiet");
    if release {
        cmd.arg("--release");
    }
    let output = cmd
        .current_dir(crate_dir)
        .output()
        .with_context(|| format!("invoking `cargo build` in {}", crate_dir.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Rewrite emitted-Rust file/line anchors back to original
        // `.jux` locations using the `// JUX:` markers the backend
        // sprinkles into the emission. When markers are absent
        // (caller used `lower_with_types` directly, e.g. tests) the
        // stderr passes through unchanged. The primary emitted file
        // is `src/main.rs`; locate it in the source list and use its
        // contents to build the lookup table.
        let main_rs = crate_
            .sources
            .iter()
            .find(|(p, _)| p == "src/main.rs")
            .map(|(_, c)| c.as_str())
            .unwrap_or("");
        let map = source_map::MarkerMap::from_emitted_source(main_rs);
        let rewritten = source_map::rewrite_rustc_output(&stderr, &map);
        anyhow::bail!(
            "`cargo build` failed for the emitted Rust crate (this is a juxc bug):\n{rewritten}",
        );
    }

    // Compute the binary path. Cargo's default target dir is
    // `target/debug/{name}{exe-suffix}` (or `target/release/...`
    // when `--release` was passed).
    let profile_dir = if release { "release" } else { "debug" };
    let binary_path = crate_dir
        .join("target")
        .join(profile_dir)
        .join(format!("{crate_name}{}", std::env::consts::EXE_SUFFIX));

    Ok(BuildArtifact { crate_dir: crate_dir.to_path_buf(), binary_path })
}

/// Invoke `rustfmt --edition=2021 <file>` on every emitted Rust file
/// for readability. Failures (rustfmt not on PATH, syntax that
/// rustfmt rejects, etc.) are logged to stderr but do NOT fail the
/// build — the generated source still compiles either way, so we
/// shouldn't block users who haven't installed rustfmt.
///
/// Per `JUX-CODEGEN-FIXES.md` Fix 2: rustfmt runs once per emitted
/// file. We don't batch because rustfmt's `--check` mode behaves
/// per-file and we want one failure to be visible rather than
/// masked by another file's success.
fn run_rustfmt(files: &[PathBuf]) {
    if files.is_empty() {
        return;
    }
    for path in files {
        // `--quiet` suppresses rustfmt's own "Formatting…" chatter so
        // a clean run stays silent. We still surface the spawn error
        // (rustfmt not found) once, on the first file only — repeating
        // the same warning for every file would be noisy.
        let status = Command::new("rustfmt")
            .arg("--edition=2021")
            .arg("--quiet")
            .arg(path)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(_) => {
                // Non-zero exit: rustfmt parsed but flagged something.
                // The unformatted file is still compilable, so we just
                // warn and move on. Worth knowing about — points at a
                // codegen bug worth investigating later.
                eprintln!(
                    "warning: rustfmt failed on {} (continuing with unformatted source)",
                    path.display(),
                );
            }
            Err(_) => {
                // Couldn't spawn rustfmt at all — almost always means
                // it's missing from PATH. One advisory line covers the
                // whole batch since the cause is the same for every
                // file; returning early keeps the warning de-duplicated.
                eprintln!("warning: rustfmt not found on PATH; emitted code is unformatted");
                return;
            }
        }
    }
}
