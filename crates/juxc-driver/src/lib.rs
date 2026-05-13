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

/// Top-level compile result.
///
/// On success, `crate_` is populated and ready to hand to [`build`].
/// On failure (any error-severity diagnostic), `crate_` is `None` and
/// `diagnostics` explains why.
pub struct CompileResult {
    /// Generated Rust crate, or `None` if compilation produced any errors.
    pub crate_: Option<RustCrate>,
    /// All diagnostics from every phase, in pipeline order.
    pub diagnostics: Vec<Diagnostic>,
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
        return Ok(CompileResult { crate_: None, diagnostics });
    }

    // Phase 1+2 per source — lex and parse independently.
    let mut units: Vec<juxc_ast::CompilationUnit> = Vec::with_capacity(sources.len());
    for source in &sources {
        let lex_result = juxc_lex::lex(source);
        diagnostics.extend(lex_result.diagnostics);
        let parsed = juxc_parse::parse(&lex_result.tokens);
        diagnostics.extend(parsed.diagnostics);
        // Resolver runs per-unit; cross-file name resolution happens
        // through the merged symbol table during tycheck.
        let resolved = juxc_resolve::resolve(&parsed.ast);
        diagnostics.extend(resolved.diagnostics);
        units.push(parsed.ast);
    }

    // Phase 6+ — tycheck against the merged workspace. We build one
    // SymbolTable that contains every class/record/enum/interface/
    // function from every unit, then run the per-expression type
    // walker against each unit using that shared table.
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

    Ok(CompileResult { crate_, diagnostics })
}

/// Compile a single source file through every phase that's currently
/// wired up. Returns a [`CompileResult`].
///
/// Returns `Err` only for genuinely fatal conditions (e.g. an internal
/// invariant violation). User-facing errors are reported via
/// [`CompileResult::diagnostics`], not via `Result::Err`.
pub fn compile(source: SourceFile) -> Result<CompileResult> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Phase 1 — lex.
    let lex_result = juxc_lex::lex(&source);
    diagnostics.extend(lex_result.diagnostics);

    // Phase 2 — parse. Takes a token slice; we forward the lexer's
    // `tokens` vector.
    let parsed = juxc_parse::parse(&lex_result.tokens);
    diagnostics.extend(parsed.diagnostics);

    // Phase 4 — resolve names.
    let resolved = juxc_resolve::resolve(&parsed.ast);
    diagnostics.extend(resolved.diagnostics);

    // Phases 6–9 — type checking.
    let typed = juxc_tycheck::typecheck(&parsed.ast);
    diagnostics.extend(typed.diagnostics);

    // If any phase produced an error, skip lowering. The user can still
    // see all the diagnostics that did fire; we just don't waste cycles
    // generating Rust source from an invalid program.
    let has_errors = diagnostics.iter().any(|d| matches!(d.severity, Severity::Error));

    let crate_ = if has_errors {
        None
    } else {
        // Phase 19 — emit Rust source. This is the Phase 1 strategy of the
        // overall language plan (per JUX-LANG-V1 §2.2).
        //
        // Reuse the tycheck-built symbol table AND the per-expression
        // type map (Phase H). The backend consults `expr_types` for
        // its String / generic-field coercion decisions instead of
        // running its own name-based heuristic pre-passes.
        // Pass the original `SourceFile` so the backend can emit
        // `// JUX:file:line:col` markers throughout the generated
        // Rust. Lets rustc errors on the emitted crate map back to
        // the user's `.jux` source (audit Tier 2.2). Existing test
        // suites that call `lower_with_types` directly stay
        // marker-free, preserving their snapshot stability.
        Some(juxc_backend_rust::lower_with_source(
            &parsed.ast,
            &typed.symbols,
            &typed.expr_types,
            Some(&source),
        ))
    };

    Ok(CompileResult { crate_, diagnostics })
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
