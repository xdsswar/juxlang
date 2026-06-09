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

pub mod manifest;
pub mod project;
mod source_map;
mod stdlib;
mod stdlib_embedded;
pub mod stubs;

pub use manifest::Manifest;
pub use project::{ensure_project_stubs, StubSyncReport};

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
    compile_workspace_as(sources, juxc_backend_rust::lower_workspace)
}

/// Generic core of [`compile_workspace`]: runs the full front end over
/// `sources` (with the stdlib auto-prepended) and, on success, lowers the
/// typed units to a [`RustCrate`] using the caller-supplied `lower`
/// function.
///
/// This lets the project/workspace build path choose between
/// [`juxc_backend_rust::lower_workspace`] (binary crate) and
/// [`juxc_backend_rust::lower_workspace_lib`] (library crate) without
/// duplicating the lex/parse/resolve/tycheck plumbing.
pub fn compile_workspace_as<F>(sources: Vec<SourceFile>, lower: F) -> Result<CompileResult>
where
    F: FnOnce(
        &[juxc_ast::CompilationUnit],
        &juxc_tycheck::SymbolTable,
        &std::collections::HashMap<juxc_source::Span, juxc_tycheck::Ty>,
        &[SourceFile],
    ) -> RustCrate,
{
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
    // Auto-load the default `rust.std.*` declaration stubs (`.jux.d`) so Rust
    // std types autocomplete in Jux syntax with no opt-in (JUX-BINDGEN-ADDENDUM
    // §G.3). These units are flagged `external` below so the backend never
    // lowers them — the real Rust std provides the bodies at link time.
    all_sources.extend(stubs::load_std_stub_sources());
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
    // Flag `.jux.d` units external (§G.9.1) so the lowering step skips them.
    stubs::mark_external_units(&mut units, &sources);

    // Phase 6+ — tycheck against the merged workspace. We build one
    // SymbolTable that contains every class/record/enum/interface/
    // function from every unit, then run the per-expression type
    // walker against each unit using that shared table. tycheck tags its
    // own diagnostics with the matching unit/source index.
    let typed = juxc_tycheck::typecheck_workspace(&units);
    diagnostics.extend(typed.diagnostics);

    // `.jux.d` declaration stubs are trusted, signature-only views of foreign
    // APIs — never validated. Drop any diagnostic they produced so the build
    // isn't blocked (and the user isn't spammed) by complaints about std/crate
    // stubs that the real crate already compiles cleanly.
    stubs::drop_external_diagnostics(&mut diagnostics, &sources);

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
        Some(lower(&units, &typed.symbols, &typed.expr_types, &sources))
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
    all_sources.extend(stubs::load_std_stub_sources());
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
    stubs::mark_external_units(&mut units, &sources);
    let typed = juxc_tycheck::typecheck_workspace(&units);
    diagnostics.extend(typed.diagnostics);
    // Trusted foreign-API stubs are never validated — drop their diagnostics.
    stubs::drop_external_diagnostics(&mut diagnostics, &sources);
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
    // Same `rust.std.*` stub auto-load as the compile path — so the LSP
    // (which routes through `check_workspace`) surfaces Rust std types and
    // methods in completion/hover, in Jux syntax (§G.10).
    all_sources.extend(stubs::load_std_stub_sources());
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
    stubs::mark_external_units(&mut units, &sources);

    // Tycheck against the merged workspace. We keep `symbols` and
    // `expr_types` so the LSP can serve hover/completion/goto without
    // re-running the front end. tycheck tags its own diagnostics with the
    // matching unit/source index.
    let typed = juxc_tycheck::typecheck_workspace(&units);
    diagnostics.extend(typed.diagnostics);

    // Trusted foreign-API stubs are never validated (see
    // `stubs::drop_external_diagnostics`): the LSP must not surface false errors
    // about std/crate stubs, while still serving completion/hover from them.
    stubs::drop_external_diagnostics(&mut diagnostics, &sources);

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
    // No-manifest convenience: same behavior as the historical `build`,
    // emitting the default Cargo.toml with no resource metadata.
    build_with_manifest(crate_, crate_dir, crate_name, release, None)
}

/// Metadata-aware variant of [`build`]. When `manifest` is `Some`, the
/// project's `[package]` metadata is woven into the emitted `Cargo.toml`
/// (version, authors, description, …) and — for the version-info / icon
/// subset — a `build.rs` is generated and the icon `.ico` copied into the
/// crate dir so the produced executable carries a Windows resource block.
///
/// When `manifest` is `None`, the emitted manifest is byte-identical to
/// the legacy template and no `build.rs` is written — exactly the path
/// loose `.jux` files and the example corpus take.
pub fn build_with_manifest(
    crate_: &RustCrate,
    crate_dir: &Path,
    crate_name: &str,
    release: bool,
    manifest: Option<&Manifest>,
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
    // Project the manifest's `[package]` metadata into the backend's
    // `CargoMeta` shape. No manifest → empty meta → legacy Cargo.toml.
    let cargo_meta = manifest
        .map(|m| m.package.to_cargo_meta())
        .unwrap_or_default();
    let cargo_toml =
        juxc_backend_rust::cargo_toml_for_with_meta(crate_name, true, &cargo_meta);
    fs::write(crate_dir.join("Cargo.toml"), &cargo_toml)
        .with_context(|| format!("writing Cargo.toml to {}", crate_dir.display()))?;

    // When the metadata calls for a Windows-resource build script, write
    // `build.rs` and copy the icon (if any) next to it. This is a no-op
    // for the no-manifest path (and for manifests that carry only Cargo
    // keys with no resource fields), keeping those crates clean.
    if cargo_meta.needs_build_script() {
        let icon_in_crate = if let Some(m) = manifest {
            copy_icon_into_crate(m, crate_dir)?
        } else {
            None
        };
        let build_rs =
            generate_build_rs(&cargo_meta, crate_name, icon_in_crate.as_deref());
        fs::write(crate_dir.join("build.rs"), &build_rs)
            .with_context(|| format!("writing build.rs to {}", crate_dir.display()))?;
    } else {
        // Defensive: if a previous run for this emit dir wrote a
        // build.rs (e.g. the manifest used to have metadata), remove the
        // stale script so the now-clean crate doesn't try to compile it.
        let stale = crate_dir.join("build.rs");
        if stale.exists() {
            let _ = fs::remove_file(stale);
        }
    }

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

/// Materialize and build a [`RustCrate`] for a specific
/// [`juxc_backend_rust::CrateTarget`] — the manifest-driven build path for
/// the multi-module project model.
///
/// Unlike [`build_with_manifest`] (which always emits a single binary named
/// `crate_name`), this:
///
/// - emits the `Cargo.toml` via
///   [`juxc_backend_rust::cargo_toml_for_target`], so a
///   [`CrateTarget::Bin`](juxc_backend_rust::CrateTarget::Bin) produces a
///   `[[bin]]` literally named from the target (Phase 1 proof: a
///   `[[bin]] name="myapp"` project produces `myapp(.exe)`), and a
///   [`CrateTarget::Lib`](juxc_backend_rust::CrateTarget::Lib) produces a
///   `[lib]` with the requested `crate-type` and **no** binary;
/// - threads `path_deps` into `[dependencies]` so an emitted crate can
///   path-depend on a sibling emitted crate (workspace linking seam);
/// - sets `in_workspace` to omit the per-crate `[workspace]` opt-out when
///   the crate is a member of an emitted Cargo workspace.
///
/// The returned [`BuildArtifact::binary_path`] points at the produced
/// executable for a `Bin` target; for a `Lib` target it points at the
/// emitted crate dir's expected library artifact under `target/<profile>/`
/// (the file name follows Cargo's `lib<name>` convention, but callers
/// usually only need to know the build succeeded).
pub fn build_emitted_crate(
    crate_: &RustCrate,
    crate_dir: &Path,
    target: &juxc_backend_rust::CrateTarget,
    release: bool,
    manifest: Option<&Manifest>,
    path_deps: &[juxc_backend_rust::PathDep],
    in_workspace: bool,
) -> Result<BuildArtifact> {
    fs::create_dir_all(crate_dir.join("src"))
        .with_context(|| format!("creating emitted crate dir {}", crate_dir.display()))?;

    // The emitted prelude unconditionally references
    // `futures::channel::oneshot` (for the `Task<T>` / `Worker` helpers),
    // so `futures` is a hard dependency of every emitted Jux crate — even
    // ones that never use async. This mirrors the legacy
    // `build_with_manifest`, which passes `uses_async = true` to the
    // Cargo.toml emitter unconditionally. The ~3-4s one-time `futures`
    // compile is cached across builds.
    let uses_async = true;

    let cargo_meta = manifest
        .map(|m| m.package.to_cargo_meta())
        .unwrap_or_default();
    let cargo_toml = juxc_backend_rust::cargo_toml_for_target(
        target,
        uses_async,
        &cargo_meta,
        path_deps,
        in_workspace,
    );
    fs::write(crate_dir.join("Cargo.toml"), &cargo_toml)
        .with_context(|| format!("writing Cargo.toml to {}", crate_dir.display()))?;

    // Windows-resource build script (icon / version-info) — same logic as
    // the legacy path. Only fires when the metadata calls for it.
    if cargo_meta.needs_build_script() {
        let icon_in_crate = if let Some(m) = manifest {
            copy_icon_into_crate(m, crate_dir)?
        } else {
            None
        };
        // ProductName for a lib falls back to the lib name; for a bin to
        // the binary name. Use the target's name.
        let target_name = match target {
            juxc_backend_rust::CrateTarget::Bin { name } => name.as_str(),
            juxc_backend_rust::CrateTarget::Lib { name, .. } => name.as_str(),
        };
        let build_rs = generate_build_rs(&cargo_meta, target_name, icon_in_crate.as_deref());
        fs::write(crate_dir.join("build.rs"), &build_rs)
            .with_context(|| format!("writing build.rs to {}", crate_dir.display()))?;
    } else {
        let stale = crate_dir.join("build.rs");
        if stale.exists() {
            let _ = fs::remove_file(stale);
        }
    }

    // Write each emitted source file.
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

    run_rustfmt(&written_rs);

    // Run `cargo build`.
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
        // Map emitted-Rust anchors back to `.jux` sites via markers in the
        // primary emitted file (main.rs for bins, lib.rs for libs).
        let primary = crate_
            .sources
            .iter()
            .find(|(p, _)| p == "src/main.rs" || p == "src/lib.rs")
            .map(|(_, c)| c.as_str())
            .unwrap_or("");
        let map = source_map::MarkerMap::from_emitted_source(primary);
        let rewritten = source_map::rewrite_rustc_output(&stderr, &map);
        anyhow::bail!(
            "`cargo build` failed for the emitted Rust crate (this is a juxc bug):\n{rewritten}",
        );
    }

    // Compute the produced-artifact path.
    let profile_dir = if release { "release" } else { "debug" };
    let out_dir = crate_dir.join("target").join(profile_dir);
    let binary_path = match target {
        juxc_backend_rust::CrateTarget::Bin { name } => {
            out_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
        }
        juxc_backend_rust::CrateTarget::Lib { name, .. } => {
            // Best-effort: the rlib Cargo produces is `lib<name>.rlib`.
            // Callers usually only check that the build succeeded.
            out_dir.join(format!("lib{}.rlib", sanitize_crate(name)))
        }
    };

    Ok(BuildArtifact { crate_dir: crate_dir.to_path_buf(), binary_path })
}

/// Sanitize a name for use in a Cargo library file-name lookup: Cargo
/// lower-cases nothing but replaces `-` with `_` in the produced rlib name.
fn sanitize_crate(name: &str) -> String {
    name.replace('-', "_")
}

/// Copy the manifest's `icon` (a `.ico` resolved against the project
/// root) into the emitted crate dir as `app.ico`, returning the
/// in-crate file name (`"app.ico"`) the generated `build.rs` should
/// reference. Returns `Ok(None)` when the manifest carries no icon.
///
/// A missing icon *file* (manifest names one but it isn't on disk) is a
/// soft failure: we warn and proceed without an icon rather than failing
/// the whole build over a resource asset. The build script gates its
/// `set_icon` on the file's presence too, so a stray reference can't
/// break compilation.
fn copy_icon_into_crate(manifest: &Manifest, crate_dir: &Path) -> Result<Option<String>> {
    let Some(src) = &manifest.package.icon else {
        return Ok(None);
    };
    if !src.is_file() {
        eprintln!(
            "juxc: warning: icon `{}` not found; building without an executable icon",
            src.display()
        );
        return Ok(None);
    }
    let dest_name = "app.ico";
    let dest = crate_dir.join(dest_name);
    fs::copy(src, &dest).with_context(|| {
        format!("copying icon {} -> {}", src.display(), dest.display())
    })?;
    Ok(Some(dest_name.to_string()))
}

/// Generate the contents of `build.rs` for the emitted crate.
///
/// The script is gated to Windows targets via `CARGO_CFG_TARGET_OS`, so
/// it's a complete no-op when cross-compiling (or building) for any other
/// platform — the `winresource` dependency is build-only and harmless
/// elsewhere. On Windows it builds a `WindowsResource`, sets the
/// version-info fields (CompanyName / FileDescription / ProductName /
/// LegalCopyright + File/Product version derived from the package
/// version), optionally attaches the copied icon, and `.compile()`s the
/// resource into the executable.
///
/// `company` defaults to the joined `authors` when not given explicitly,
/// matching the spec's "defaults to authors" note. All interpolated
/// strings are escaped for a Rust string literal so quotes/backslashes in
/// metadata can't break the generated source.
fn generate_build_rs(
    meta: &juxc_backend_rust::CargoMeta,
    crate_name: &str,
    icon_in_crate: Option<&str>,
) -> String {
    // ProductName falls back to the crate name; CompanyName falls back to
    // the joined authors (per spec) and then to an empty string.
    let product_name = crate_name.to_string();
    let company = meta
        .company
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| meta.authors.join(", "));
    let description = meta.description.clone().unwrap_or_default();
    let copyright = meta.copyright.clone().unwrap_or_default();
    let version = meta.version.clone().unwrap_or_else(|| "0.0.0".to_string());

    // Build the chain of `.set(...)` calls. We only set fields that have
    // content so we don't stamp empty strings into the resource.
    let mut sets = String::new();
    if !company.is_empty() {
        sets.push_str(&format!(
            "        res.set(\"CompanyName\", \"{}\");\n",
            escape_rs(&company)
        ));
    }
    if !description.is_empty() {
        sets.push_str(&format!(
            "        res.set(\"FileDescription\", \"{}\");\n",
            escape_rs(&description)
        ));
    }
    sets.push_str(&format!(
        "        res.set(\"ProductName\", \"{}\");\n",
        escape_rs(&product_name)
    ));
    if !copyright.is_empty() {
        sets.push_str(&format!(
            "        res.set(\"LegalCopyright\", \"{}\");\n",
            escape_rs(&copyright)
        ));
    }
    // FileVersion / ProductVersion. winresource derives these from
    // CARGO_PKG_VERSION automatically, but we set them explicitly so the
    // produced resource is independent of how cargo is invoked.
    sets.push_str(&format!(
        "        res.set(\"FileVersion\", \"{v}\");\n        res.set(\"ProductVersion\", \"{v}\");\n",
        v = escape_rs(&version)
    ));

    // Icon block — only emitted when an icon was copied in. We re-check
    // the file at build time so a deleted asset degrades to no-icon
    // rather than a hard compile error.
    let icon_block = match icon_in_crate {
        Some(name) => format!(
            "        if std::path::Path::new(\"{n}\").exists() {{\n\
             \x20           res.set_icon(\"{n}\");\n\
             \x20       }}\n",
            n = escape_rs(name)
        ),
        None => String::new(),
    };

    // Assemble the full source. Note the doc comment marking this as
    // generated, and the Windows gate.
    format!(
        "// Generated by juxc — do not edit.\n\
         //\n\
         // Embeds Windows version-info + icon resources into the produced\n\
         // executable from the project's `jux.toml` `[package]` metadata.\n\
         // No-op on non-Windows targets.\n\
         fn main() {{\n\
         \x20   if std::env::var(\"CARGO_CFG_TARGET_OS\").as_deref() == Ok(\"windows\") {{\n\
         \x20       let mut res = winresource::WindowsResource::new();\n\
         {sets}\
         {icon_block}\
         \x20       if let Err(e) = res.compile() {{\n\
         \x20           println!(\"cargo:warning=winresource compile failed: {{e}}\");\n\
         \x20       }}\n\
         \x20   }}\n\
         }}\n",
    )
}

/// Escape a string for inclusion inside a double-quoted Rust string
/// literal in generated `build.rs` source: backslashes and double-quotes.
fn escape_rs(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
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

// ============================================================================
// `.jux.d` stub tests (JUX-BINDGEN-ADDENDUM.md §G)
// ============================================================================

#[cfg(test)]
mod stub_tests {
    use juxc_diagnostics::Severity;
    use juxc_source::SourceFile;

    /// Serializes tests that mutate the process-global `JUX_STUBS_DIR` env var:
    /// Rust runs tests in parallel threads in one process, so two tests touching
    /// the same env var would otherwise race (one's `set_var` leaking into the
    /// other's stub loading). Each such test holds this lock for its duration.
    static STUB_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A signature-only `.jux.d` declaration stub: a `Widget` class whose
    /// methods/ctor end in `;` (no bodies). This is exactly what
    /// `juxc bindgen` emits and what the resolver must ingest as `external`.
    const WIDGET_STUB: &str = "package rust.demo;\n\
        public class Widget {\n\
            public Widget(int w, int h);\n\
            public int area();\n\
        }\n";

    /// User code that imports and constructs the stubbed `Widget` and calls a
    /// method on it. After Phase 5 this must type-check with zero diagnostics
    /// — no \"missing body\", no \"unresolved\".
    const USER_MAIN: &str = "import rust.demo.Widget;\n\
        public void main() {\n\
            var w = new Widget(2, 3);\n\
            print(w.area());\n\
        }\n";

    /// `.jux.d` units type-check as `external`: the stub's bodyless methods /
    /// constructor don't trip a missing-body error, and user code that imports
    /// + uses the stub resolves cleanly.
    #[test]
    fn external_stub_resolves_with_no_diagnostics() {
        let stub = SourceFile::new("rust/demo.jux.d", WIDGET_STUB);
        let main = SourceFile::new("main.jux", USER_MAIN);
        let result = crate::check_workspace(vec![stub, main]);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "expected zero errors resolving a `.jux.d` stub, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// Codegen must NOT emit a `Widget` struct from the stub — the real Rust
    /// crate provides it. We compile the same workspace and assert the emitted
    /// Rust never declares a `Widget` type (the stub is `external`, skipped at
    /// lowering), while the user's `main` IS emitted.
    #[test]
    fn external_stub_is_not_lowered_to_codegen() {
        let stub = SourceFile::new("rust/demo.jux.d", WIDGET_STUB);
        let main = SourceFile::new("main.jux", USER_MAIN);
        let result = crate::compile_workspace(vec![stub, main]).expect("compile");
        let crate_ = result.crate_.expect("workspace should compile to a crate");
        let all_rust: String = crate_
            .sources
            .iter()
            .map(|(_, c)| c.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // No `Widget` struct/impl emitted from the stub.
        assert!(
            !all_rust.contains("struct Widget") && !all_rust.contains("impl Widget"),
            "stub Widget must not be lowered into the emitted Rust:\n{all_rust}"
        );
        // The user's main WAS emitted (sanity: lowering happened for non-stub).
        assert!(all_rust.contains("fn main"), "user main should still be emitted");
    }

    /// A `.jux.d` std stub supplied via `$JUX_STUBS_DIR` is loaded verbatim,
    /// contributes its types to the symbol table, and resolves in Jux syntax —
    /// while its own (signature-only) declarations never raise errors. This is
    /// the override hook the test harness and `vendoring` use; the production
    /// path instead *generates* `rust.std` from the installed toolchain (see
    /// [`stubs::load_std_stub_sources`]), which a separate, toolchain-gated test
    /// exercises.
    #[test]
    fn std_stub_dir_loads_clean_and_resolves() {
        let _env = STUB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A minimal hand-rolled stub standing in for generated `rust.std`.
        let dir = std::env::temp_dir().join(format!(
            "juxc-stubdir-{}-{}",
            std::process::id(),
            "vec"
        ));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("std.jux.d"),
            "package rust.std;\n\
             public class Vec<T> {\n\
                 public Vec();\n\
                 public void push(T value);\n\
                 public int len();\n\
             }\n",
        )
        .unwrap();
        std::env::set_var("JUX_STUBS_DIR", &dir);

        let main = SourceFile::new(
            "main.jux",
            "import rust.std.Vec;\n\
             public void main() {\n\
                 var v = new Vec();\n\
                 v.push(1);\n\
                 print(v.len());\n\
             }\n",
        );
        let result = crate::check_workspace(vec![main]);
        std::env::remove_var("JUX_STUBS_DIR");
        let _ = std::fs::remove_dir_all(&dir);

        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "std stub must load clean and resolve `Vec`, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(
            result.symbols.classes.contains_key("rust.std.Vec"),
            "rust.std.Vec should be in the symbol table"
        );
    }

    /// Production path: `rust.std` is generated from the **installed toolchain's**
    /// pre-built rustdoc JSON (no curated `.jux.d` in the repo). Gated on the
    /// `rust-docs-json` component being present — skipped otherwise so CI without
    /// it stays green. Proves the headline collections (`Vec`, `HashMap`,
    /// `String`) surface in Jux syntax and that the generated stub is error-free.
    #[test]
    fn generated_rust_std_from_toolchain_resolves_collections() {
        let _env = STUB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Make sure no override is active; force regeneration into a temp cache.
        std::env::remove_var("JUX_STUBS_DIR");
        let stub = crate::stubs::load_std_stub_sources();
        let Some(stub) = stub.into_iter().next() else {
            eprintln!("rust-docs-json not installed — skipping generated-std test");
            return;
        };
        let text = stub.contents();
        for ty in ["class Vec", "class HashMap", "class String"] {
            assert!(text.contains(ty), "generated rust.std missing `{ty}`");
        }

        let main = SourceFile::new(
            "main.jux",
            "import rust.std.HashMap;\n\
             public void main() {\n\
                 var m = new HashMap();\n\
             }\n",
        );
        let result = crate::check_workspace(vec![main]);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "generated rust.std must resolve cleanly, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
