//! `.jux.d` declaration-stub loader and bound-crate stub resolution.
//!
//! Implements the loading half of JUX-BINDGEN-ADDENDUM.md §G: foreign APIs
//! (Rust `std`, Rust crates, C/C++ headers) are surfaced to Jux source and to
//! editor tooling as **Jux-syntax interface stubs** — `.jux.d` files whose
//! bodies are elided to `;`. A stub is loaded into the workspace symbol table
//! exactly like an ordinary `.jux` source (so resolution / type-checking /
//! the LSP serve it for free, §G.10), but the unit is flagged **external**
//! (`CompilationUnit::is_external`) so the backend never lowers it — the real
//! crate provides the implementation at link time (§G.9.2).
//!
//! ## What this module supplies
//!
//! - [`is_stub_path`] — recognise a `.jux.d` file by extension.
//! - [`load_std_stub_sources`] — the default `rust.std.*` stub set, auto-loaded
//!   into every compile (mirrors the [`crate::stdlib`] auto-prepend), so std
//!   collections / `String` / `io` autocomplete in Jux syntax with no opt-in.
//! - [`load_project_stub_sources`] — every `.jux.d` under a project's
//!   `.jux-stubs/` cache directory (§G.11.2).
//! - [`resolve_crate_stub`] — given a `rust.<crate>` (or `c.` / `cpp.`)
//!   dependency, return the cached `.jux.d`, generating it from the crate's
//!   rustdoc JSON via [`juxc_bindgen`] when absent (§G.6.2).
//!
//! ## Why externality is keyed on the extension
//!
//! The parser is source-origin-agnostic — it can't tell a stub from a normal
//! source. So the driver flips `CompilationUnit::is_external` on after parsing,
//! using [`is_stub_path`] against the source's path. Every entry point that
//! builds units ([`crate::compile_workspace_as`], [`crate::check_workspace`],
//! …) calls [`mark_external_units`] after the parse loop.

use std::path::{Path, PathBuf};
use std::process::Command;

use juxc_ast::CompilationUnit;
use juxc_diagnostics::Diagnostic;
use juxc_source::SourceFile;

/// The conventional double extension a declaration stub carries.
const STUB_EXT: &str = ".jux.d";

/// Project-local cache directory for generated / vendored crate stubs
/// (§G.11.2). `rust.<crate>` → `.jux-stubs/rust/<crate>.jux.d`.
pub const PROJECT_STUB_DIRNAME: &str = ".jux-stubs";

/// The crates whose pre-built rustdoc JSON is merged to form the default
/// `rust.std` surface. Order is significant — [`juxc_bindgen::ingest::generate_merged`]
/// is first-definition-wins, and Rust layers `core` ⊂ `alloc` ⊂ `std`, so the
/// more fundamental crate is listed first. `core` is deliberately **excluded**:
/// its rustdoc JSON is ~20k items / tens of MB (operator traits, SIMD, every
/// primitive's intrinsics) — far too heavy to lex on every compile — and the
/// only `core` types a Jux programmer reaches (`Option`, `Result`) are folded
/// away by the bindgen type map (`Option<T>`→`T?`, `Result<T,E>`→`T throws E`).
/// `alloc` + `std` carry the prelude collections (`Vec`, `String`, `Box`,
/// `Rc`/`Arc`, `BTreeMap`, `HashMap`, …).
const STD_MERGE_CRATES: &[&str] = &["alloc", "std"];

/// Crates read ONLY to resolve `Deref` targets, contributing no items.
///
/// `Vec` derefs to `[T]`, and the inherent `impl<T> [T]` blocks that carry
/// `get`/`first`/`last`/`iter`/`contains`/`reverse` live in `core` -- they are
/// in neither `alloc.json` nor `std.json`. Without this every one of those
/// methods is missing from the scanned surface, which is what pushed the
/// backend into hardcoding some of them. Emitting all of `core` instead would
/// multiply the stub for types nobody asked for.
const STD_POOL_CRATES: &[&str] = &["core"];

/// Bump to invalidate previously-cached generated `rust.std` stubs when the
/// bindgen surface or the merge set changes. Embedded in the cache header and
/// checked on load.
const STD_STUB_CACHE_VERSION: u32 = 23;

/// A pre-generated `rust.std` surface, compiled into the binary as the
/// last-resort fallback.
///
/// The generated stub (step 3 below) is built from
/// `<sysroot>/share/doc/rust/json/{alloc,std}.json`, which only exists when the
/// user has installed the **nightly** `rust-docs-json` rustup component. Most
/// people have not, and before this fallback existed the whole `rust.std`
/// surface silently evaporated for them: every `Vec` / `HashMap` / `String`
/// reference failed with an unresolved-type error that named no cause and
/// suggested no fix. Shipping a frozen snapshot means `rust.std` works out of
/// the box on a stock stable toolchain, and a user who *does* install the
/// component transparently gets the fresher, toolchain-exact surface instead.
///
/// **Regenerating:** install the component
/// (`rustup component add rust-docs-json --toolchain nightly`), delete the
/// cached stub under `<user-cache>/juxc/stubs/rust-std.jux.d`, run any compile
/// to regenerate it, then copy it over `crates/juxc-driver/stubs/rust-std.jux.d`.
/// `vendored_std_stub_is_current` fails the build's test suite whenever this
/// snapshot's header falls behind [`STD_STUB_CACHE_VERSION`], so a bindgen
/// change can't leave it quietly stale.
const VENDORED_RUST_STD: &str = include_str!("../stubs/rust-std.jux.d");

/// Bump to invalidate previously-cached generated per-crate (`rust.<crate>`)
/// stubs when the bindgen surface / naming changes. Stamped as the first line of
/// each generated `.jux-stubs/rust/<crate>.jux.d` and checked on load so a stale
/// stub (e.g. a pre-snake_case cache) is regenerated rather than trusted. Started
/// at 1 alongside the snake_case-verbatim naming switch.
const CRATE_STUB_CACHE_VERSION: u32 = 6;

/// The first-line marker a generated crate stub must carry to be trusted.
///
/// Carries both the bindgen cache version and the crate's SOURCE, so a stub
/// is re-generated when either the rules that produced it or the crate it
/// described has changed.
fn crate_cache_header_for(source: &juxc_backend_rust::CrateSource) -> String {
    format!(
        "// juxc crate stub cache-version {CRATE_STUB_CACHE_VERSION} source {}\n",
        crate_source_tag(source),
    )
}

/// True when a generated `rust.*` crate stub on disk carries the current
/// marker for `source`.
///
/// A stub written by an older toolchain (or hand-vendored without a marker)
/// is stale, so it regenerates against the current bindgen rules. So is one
/// generated from a different source: a dependency repointed from crates.io
/// to a local checkout must not keep serving the registry crate's API.
fn crate_stub_cache_is_fresh_for(
    path: &Path,
    source: &juxc_backend_rust::CrateSource,
) -> bool {
    match std::fs::read_to_string(path) {
        Ok(text) => text.starts_with(crate_cache_header_for(source).trim_end()),
        Err(_) => false,
    }
}

/// Does `path` name a `.jux.d` declaration stub?
///
/// `Path::extension` only returns the final component (`d`), so we match on the
/// full file-name suffix to distinguish `foo.jux.d` (a stub) from a stray
/// `foo.d` (which is *not* a Jux stub).
pub fn is_stub_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(STUB_EXT))
}

/// After the per-source parse loop, flag every unit whose source is a `.jux.d`
/// stub as [`CompilationUnit::is_external`]. `sources[i]` corresponds to
/// `units[i]` (the driver builds them in lock-step), so a single positional
/// walk is enough.
pub fn mark_external_units(units: &mut [CompilationUnit], sources: &[SourceFile]) {
    for (unit, source) in units.iter_mut().zip(sources.iter()) {
        if is_stub_path(source.path()) {
            unit.is_external = true;
        }
    }
}

/// Remove every diagnostic that a `.jux.d` declaration stub produced.
///
/// A stub is a *trusted, signature-only* view of a foreign API (Rust `std`, a
/// crate, …). It is loaded into the symbol table so user code, hover, and
/// completion can see those types — but it is **never validated**: the real
/// crate already compiles, so any lex/parse/resolve/tycheck complaint about the
/// stub itself (an unknown referenced type the bindgen surface didn't pull in, a
/// `uint?` that Jux's value-type rules reject, an unresolved const initializer,
/// …) is noise, not a user-actionable error. Suppressing it is what makes
/// "autocomplete over *any* crate" robust: a stub that is 95% well-formed still
/// contributes its 95%, instead of burying the user in errors about std.
///
/// A diagnostic is dropped iff its `file` points at a source whose path is a
/// `.jux.d` stub. Untagged diagnostics (`file == None`) and diagnostics against
/// ordinary `.jux` sources (including the hand-written `jux.std/` tree) are
/// always kept.
///
/// **One exception: a lex or syntax error survives.** The "95% still
/// contributes" argument only holds for complaints about a declaration the
/// parser managed to READ. A malformed token sequence stops the parse, so the
/// stub contributes NOTHING -- and with its diagnostic dropped too, the only
/// thing the user sees is `E0301 unresolved import` against a crate that is
/// right there in `.jux-stubs/`. That is a bindgen bug reported as a user
/// mistake. `Result<T, ()>` rendering as `throws void` was exactly this: one
/// unparsable method took down a whole crate's API, silently.
pub fn drop_external_diagnostics(diagnostics: &mut Vec<Diagnostic>, sources: &[SourceFile]) {
    diagnostics.retain(|d| match d.file {
        Some(idx) => {
            !sources.get(idx).is_some_and(|s| is_stub_path(s.path()))
                || stub_error_is_fatal_to_the_unit(d)
        }
        None => true,
    });
}

/// Does this stub diagnostic mean the whole stub failed to load?
///
/// Lexical (`E01xx`) and syntax (`E02xx`) errors do; everything later in the
/// pipeline is about a declaration that parsed, and is the noise
/// [`drop_external_diagnostics`] exists to suppress.
fn stub_error_is_fatal_to_the_unit(d: &Diagnostic) -> bool {
    d.severity == juxc_diagnostics::Severity::Error
        && matches!(&d.code.as_str()[..3], "E01" | "E02")
}

// ============================================================================
// Default `rust.std.*` stub set (auto-loaded)
// ============================================================================

/// Supply the default `rust.std` declaration stub, auto-loaded into every
/// compile and editor analysis so Rust std types (`Vec`, `HashMap`, `String`,
/// …) autocomplete and hover in Jux syntax with no opt-in (mirrors the
/// [`crate::stdlib`] auto-prepend; JUX-BINDGEN-ADDENDUM §G.3).
///
/// Unlike the hand-written `jux.std/` tree, the `rust.std` stub is **generated
/// from the toolchain the user actually has installed** — there is no curated
/// std `.jux.d` checked into the repo. Resolution order:
///
/// 1. `$JUX_STUBS_DIR` — an explicit directory of `.jux.d` files, loaded
///    verbatim with **no** generation. The override hook for test harnesses
///    (and for vendoring a frozen std surface); it short-circuits everything
///    below.
/// 2. A cached generated stub under the user cache dir, when present and
///    version-current — loaded directly, with **no** subprocess, so the LSP's
///    per-keystroke `check_workspace` stays cheap.
/// 3. Otherwise, locate the installed toolchain's pre-built rustdoc JSON
///    (`<sysroot>/share/doc/rust/json/{alloc,std}.json`), merge it through
///    [`juxc_bindgen`], cache the result, and load it.
/// 4. Failing all of that, fall back to [`VENDORED_RUST_STD`] — a frozen
///    snapshot compiled into the binary, so `rust.std` resolves on a stock
///    stable toolchain with nothing installed.
///
/// Step 4 is what most users get, because step 3 needs the nightly
/// `rust-docs-json` component. The result is never an empty list, so an
/// unresolved `Vec` now means the user really did mistype it.
pub fn load_std_stub_sources() -> Vec<SourceFile> {
    // (1) Explicit override — a directory of `.jux.d` files, loaded as-is.
    if let Ok(dir) = std::env::var("JUX_STUBS_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return collect_stub_sources(&p);
        }
    }
    // (2)+(3) Cached-or-generated `rust.std`, then (4) the vendored snapshot.
    match cached_or_generated_std_stub() {
        Ok(Some(src)) => vec![src],
        _ => vec![vendored_std_stub()],
    }
}

/// The frozen `rust.std` snapshot as a [`SourceFile`]. Its path is synthetic —
/// nothing on disk backs it — but it is stable and self-describing, so a
/// diagnostic or a go-to-definition that lands in the std surface names
/// something the user can recognise.
fn vendored_std_stub() -> SourceFile {
    SourceFile::new(
        PathBuf::from("<vendored>/rust-std.jux.d"),
        VENDORED_RUST_STD.to_string(),
    )
}

/// Return the generated `rust.std` stub as a [`SourceFile`], loading a fresh
/// cache when present or generating (and caching) it from the toolchain's
/// pre-built rustdoc JSON otherwise. `Ok(None)` means the std JSON isn't
/// available (no autocomplete, no error).
fn cached_or_generated_std_stub() -> anyhow::Result<Option<SourceFile>> {
    let cache = std_stub_cache_path();

    // Cache hit (fast path — no subprocess, no JSON parse): accept only when the
    // embedded version marker matches, so a bindgen change invalidates old caches.
    if let Some(cache) = &cache {
        if let Ok(text) = std::fs::read_to_string(cache) {
            if text.starts_with(&std_cache_header()) {
                return Ok(Some(SourceFile::new(cache.clone(), text)));
            }
        }
    }

    // Cold path: find the toolchain's pre-built JSON and merge it.
    let Some(json_dir) = locate_rust_json_dir() else {
        return Ok(None);
    };
    let Some(text) = generate_std_stub_text(&json_dir)? else {
        return Ok(None);
    };

    // Best-effort cache write — a read-only cache dir must not fail the compile.
    if let Some(cache) = &cache {
        if let Some(parent) = cache.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = write_atomic(cache, &text);
    }
    let path = cache.unwrap_or_else(|| PathBuf::from("rust.std.jux.d"));
    Ok(Some(SourceFile::new(path, text)))
}

/// The cache header line carrying the current cache version. A cached stub is
/// only trusted when it starts with exactly this line.
fn std_cache_header() -> String {
    format!("// juxc rust.std stub cache-version {STD_STUB_CACHE_VERSION}\n")
}

/// Path of the cached generated `rust.std` stub:
/// `<user-cache>/juxc/stubs/rust-std.jux.d`. `None` when no cache root resolves
/// (the compiler then regenerates each run rather than caching).
fn std_stub_cache_path() -> Option<PathBuf> {
    user_cache_dir().map(|d| d.join("juxc").join("stubs").join("rust-std.jux.d"))
}

/// The OS user-cache root: `%LOCALAPPDATA%` on Windows, then `$XDG_CACHE_HOME`,
/// then `$HOME/.cache`. `None` when none resolve.
fn user_cache_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        if let Ok(d) = std::env::var("LOCALAPPDATA") {
            if !d.is_empty() {
                return Some(PathBuf::from(d));
            }
        }
    }
    if let Ok(d) = std::env::var("XDG_CACHE_HOME") {
        if !d.is_empty() {
            return Some(PathBuf::from(d));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home).join(".cache"));
        }
    }
    None
}

/// Locate the installed toolchain's pre-built rustdoc JSON directory
/// (`<sysroot>/share/doc/rust/json/`), the source the default `rust.std` stub is
/// generated from. Resolution:
///
/// 1. `$JUX_RUST_JSON_DIR` — explicit override (tests / unusual layouts).
/// 2. `rustc +nightly --print sysroot` + `share/doc/rust/json` — rustdoc JSON is
///    a nightly feature, so the nightly toolchain is asked first.
/// 3. `rustc --print sysroot` (whatever the default toolchain is) as a fallback.
///
/// Returns the directory only when it actually exists *and* contains the merge
/// crates' JSON (the `rust-docs-json` rustup component must be installed);
/// `None` otherwise.
fn locate_rust_json_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("JUX_RUST_JSON_DIR") {
        let p = PathBuf::from(dir);
        if json_dir_has_merge_crates(&p) {
            return Some(p);
        }
    }
    for toolchain in [Some("nightly"), None] {
        if let Some(sysroot) = rustc_sysroot(toolchain) {
            let json = sysroot.join("share").join("doc").join("rust").join("json");
            if json_dir_has_merge_crates(&json) {
                return Some(json);
            }
        }
    }
    None
}

/// True when `dir` holds every crate in [`STD_MERGE_CRATES`] as `<crate>.json`.
fn json_dir_has_merge_crates(dir: &Path) -> bool {
    dir.is_dir()
        && STD_MERGE_CRATES
            .iter()
            .all(|c| dir.join(format!("{c}.json")).is_file())
}

/// Ask `rustc` for its sysroot. `toolchain` selects an explicit toolchain via
/// the `+name` shorthand (`Some("nightly")`), or the default (`None`).
fn rustc_sysroot(toolchain: Option<&str>) -> Option<PathBuf> {
    let mut cmd = Command::new("rustc");
    if let Some(tc) = toolchain {
        cmd.arg(format!("+{tc}"));
    }
    let output = cmd.arg("--print").arg("sysroot").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

/// Write `contents` to `path` so a concurrent reader never sees a partial file.
///
/// The stub caches are shared: an editor's language server and a CLI build can
/// regenerate the same path at the same time, and a plain `fs::write` truncates
/// first, so a reader in that window gets a half-written stub and reports
/// nonsense about the standard library. Writing beside the target and renaming
/// makes the swap atomic — a reader sees either the old file or the new one.
///
/// Surfaced as flaky tests first: two driver tests failed on the run right
/// after a cache-version bump (when the multi-megabyte regeneration actually
/// happens) and passed on every run after.
fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, contents)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}
/// Read the [`STD_MERGE_CRATES`] JSON from `json_dir`, merge them into one
/// `rust.std` stub via [`juxc_bindgen`], and return the rendered `.jux.d` text
/// (prefixed with the [`std_cache_header`] version marker). `Ok(None)` when a
/// required JSON file can't be read.
fn generate_std_stub_text(json_dir: &Path) -> anyhow::Result<Option<String>> {
    let mut jsons: Vec<(String, String)> = Vec::with_capacity(STD_MERGE_CRATES.len());
    for &crate_name in STD_MERGE_CRATES {
        let path = json_dir.join(format!("{crate_name}.json"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(None);
        };
        jsons.push((crate_name.to_string(), text));
    }
    let refs: Vec<(&str, &str)> = jsons
        .iter()
        .map(|(n, j)| (n.as_str(), j.as_str()))
        .collect();
    // Deref-only crates are optional: a toolchain without `core.json` still
    // generates, just without the slice surface.
    let mut pool_texts: Vec<(String, String)> = Vec::new();
    for &crate_name in STD_POOL_CRATES {
        if let Ok(text) = std::fs::read_to_string(json_dir.join(format!("{crate_name}.json"))) {
            pool_texts.push((crate_name.to_string(), text));
        }
    }
    let pool_refs: Vec<(&str, &str)> = pool_texts
        .iter()
        .map(|(n, j)| (n.as_str(), j.as_str()))
        .collect();
    let stub = juxc_bindgen::ingest::generate_merged_with_pool(&refs, &pool_refs, "rust.std")
        .map_err(|e| anyhow::anyhow!("bindgen failed to merge std rustdoc JSON: {e}"))?;
    let rendered = juxc_bindgen::render_stub(&stub);
    Ok(Some(format!("{}{rendered}", std_cache_header())))
}

// ============================================================================
// Project stub cache (`.jux-stubs/`)
// ============================================================================

/// Read every `.jux.d` under `<project_root>/.jux-stubs/` (§G.11.2). These are
/// the generated / vendored stubs for the project's bound crates. Empty when
/// the project has no `.jux-stubs/` directory.
pub fn load_project_stub_sources(project_root: &Path) -> Vec<SourceFile> {
    let dir = project_root.join(PROJECT_STUB_DIRNAME);
    if !dir.is_dir() {
        return Vec::new();
    }
    collect_stub_sources(&dir)
}

/// Recursively read every `.jux.d` file under `dir` into [`SourceFile`]s,
/// path-sorted. Hidden subdirectories are skipped.
fn collect_stub_sources(dir: &Path) -> Vec<SourceFile> {
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_stub_files(dir, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            std::fs::read_to_string(&p)
                .ok()
                .map(|c| SourceFile::new(p, c))
        })
        .collect()
}

/// Walk `dir` recursively, appending every `.jux.d` file path to `out`.
fn collect_stub_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }
            collect_stub_files(&path, out);
        } else if is_stub_path(&path) {
            out.push(path);
        }
    }
}

// ============================================================================
// Bound-crate stub resolution / generation (§G.6, §G.11)
// ============================================================================

/// A `[dependencies]` name that names a foreign crate stub: `rust.<crate>`,
/// `c.<lib>`, or `cpp.<lib>`. Returns the `(kind, crate)` split, or `None` for
/// an ordinary Jux path dependency.
pub fn foreign_dep_kind(name: &str) -> Option<(&'static str, &str)> {
    if let Some(rest) = name.strip_prefix("rust.") {
        Some(("rust", rest))
    } else if let Some(rest) = name.strip_prefix("c.") {
        Some(("c", rest))
    } else if let Some(rest) = name.strip_prefix("cpp.") {
        Some(("cpp", rest))
    } else {
        None
    }
}

/// The cache path a `rust.<crate>` stub lives at: `.jux-stubs/rust/<crate>.jux.d`.
pub fn crate_stub_cache_path(project_root: &Path, kind: &str, crate_name: &str) -> PathBuf {
    project_root
        .join(PROJECT_STUB_DIRNAME)
        .join(kind)
        .join(format!("{crate_name}{STUB_EXT}"))
}

/// Resolve a bound Rust crate to a `.jux.d` stub on disk, returning its path.
///
/// 1. **Cache hit** — if `.jux-stubs/<kind>/<crate>.jux.d` already exists, use it
///    (the spec's keyed-by-version regeneration is a future refinement; today a
///    present stub is taken as fresh).
/// 2. **Generate** — otherwise run `cargo rustdoc … --output-format json` on the
///    crate (in `project_root`), pipe the JSON through
///    [`generate_stub_from_rustdoc_json`], and write the result to the cache.
///
/// Only `kind == "rust"` is wired to live generation here; `c` / `cpp` stubs
/// need a libclang / autocxx front end (§G.7/§G.8) that is out of this phase's
/// scope, so for those we only honour a pre-vendored cache entry.
///
/// Returns `Err` with context when generation is attempted but fails (rustdoc
/// JSON requires a nightly toolchain and network access for the crate's deps),
/// so the caller can downgrade to a diagnostic rather than aborting the build.
/// Where a foreign dependency's crate comes from, as the backend spells it.
///
/// One conversion, used by both the emitted `Cargo.toml` and the throwaway
/// manifest rustdoc runs in -- so the stub always describes the crate the
/// build links. The §B.5.5 source priority (`path > git > registry`) is the
/// manifest's; this only translates the answer it already reached.
pub fn crate_source_of(dep: &crate::manifest::Dependency) -> juxc_backend_rust::CrateSource {
    if let Some(path) = &dep.path {
        return juxc_backend_rust::CrateSource::Path(path.display().to_string());
    }
    if let Some(url) = &dep.git {
        let pin = dep.git_ref.as_ref().map(|r| match r {
            crate::manifest::GitRef::Branch(b) => ("branch".to_string(), b.clone()),
            crate::manifest::GitRef::Tag(t) => ("tag".to_string(), t.clone()),
            crate::manifest::GitRef::Rev(r) => ("rev".to_string(), r.clone()),
        });
        return juxc_backend_rust::CrateSource::Git { url: url.clone(), pin };
    }
    juxc_backend_rust::CrateSource::Registry
}

/// A short, stable tag for a crate source, folded into the stub cache header.
///
/// A stub is cached under the crate's NAME. Without the source in the key,
/// pointing a dependency at a local checkout would keep serving the stub
/// generated from the crates.io release -- an API mismatch that reads as a
/// compiler bug. A changed tag regenerates, exactly as a changed toolchain
/// version does.
fn crate_source_tag(source: &juxc_backend_rust::CrateSource) -> String {
    match source {
        juxc_backend_rust::CrateSource::Registry => "registry".to_string(),
        juxc_backend_rust::CrateSource::Path(p) => format!("path:{p}"),
        juxc_backend_rust::CrateSource::Git { url, pin } => match pin {
            Some((k, v)) => format!("git:{url}#{k}={v}"),
            None => format!("git:{url}"),
        },
    }
}

pub fn resolve_crate_stub(
    project_root: &Path,
    kind: &str,
    crate_name: &str,
    version: Option<&str>,
    source: &juxc_backend_rust::CrateSource,
) -> anyhow::Result<PathBuf> {
    let cache = crate_stub_cache_path(project_root, kind, crate_name);
    if cache.is_file() {
        // Vendored c/cpp stubs are authored by hand and carry no version marker,
        // so trust them as-is. A generated `rust.*` stub is trusted only when its
        // cache-version marker matches; a stale one (pre-snake_case naming) falls
        // through to regeneration.
        if kind != "rust" || crate_stub_cache_is_fresh_for(&cache, source) {
            return Ok(cache);
        }
    }
    if kind != "rust" {
        anyhow::bail!(
            "no cached stub for `{kind}.{crate_name}` at {} (C/C++ stub generation \
             is not wired in this phase -- vendor a `.jux.d` into `.jux-stubs/{kind}/`)",
            cache.display()
        );
    }

    let json = run_cargo_rustdoc_json(crate_name, version, source)?;
    let package = format!("rust.{crate_name}");
    let body = generate_stub_from_rustdoc_json(&json, &package).map_err(|e| {
        anyhow::anyhow!("bindgen failed to ingest rustdoc JSON for `{crate_name}`: {e}")
    })?;
    // Prepend the cache-version marker so a future toolchain can detect a stale
    // stub (the leading `//` line is an ordinary Jux comment the parser ignores).
    let stub = format!("{}{body}", crate_cache_header_for(source));

    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomic(&cache, &stub)?;
    Ok(cache)
}

/// Render a `.jux.d` stub from a rustdoc-JSON string for `package`. Thin
/// wrapper over [`juxc_bindgen::generate_from_json`] + [`juxc_bindgen::render_stub`]
/// so callers (and tests) have a one-call path from JSON text to stub text.
pub fn generate_stub_from_rustdoc_json(
    json: &str,
    package: &str,
) -> Result<String, serde_json::Error> {
    let stub = juxc_bindgen::ingest::generate_from_json(json, package)?;
    Ok(juxc_bindgen::render_stub(&stub))
}

/// Invoke `cargo rustdoc` to produce a crate's public-API JSON.
///
/// rustdoc JSON is a nightly-only, `-Z unstable-options` feature, so we ask the
/// `nightly` toolchain explicitly via `cargo +nightly`. A Jux project has no
/// `Cargo.toml`, so rustdoc can't run there; instead we materialise a throwaway
/// Cargo project (under the user cache) that **depends on** `crate_name`, run
/// `cargo +nightly rustdoc -p <crate>` inside it, and read the emitted
/// `target/doc/<crate>.json`. `version` is the manifest requirement
/// (`"0.27"`, …) — `*` when unspecified.
fn run_cargo_rustdoc_json(
    crate_name: &str,
    version: Option<&str>,
    source: &juxc_backend_rust::CrateSource,
) -> anyhow::Result<String> {
    let work = rustdoc_gen_dir(crate_name)
        .ok_or_else(|| anyhow::anyhow!("no cache directory to generate rustdoc JSON in"))?;
    std::fs::create_dir_all(work.join("src"))?;
    // A minimal package whose only purpose is to pull `crate_name` into a
    // resolvable dependency graph for rustdoc.
    let sanitized = crate_name.replace('-', "_");
    // The SAME dependency line the emitted crate gets, so the stub
    // describes the crate the build links rather than a same-named
    // crates.io release that happens to exist.
    let dep_line = juxc_backend_rust::registry_dep_line_for(
        crate_name,
        version.unwrap_or("*"),
        source,
    );
    let cargo_toml = format!(
        "[package]\nname = \"__juxc_doc_{sanitized}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [dependencies]\n{dep_line}",
    );
    std::fs::write(work.join("Cargo.toml"), cargo_toml)?;
    std::fs::write(work.join("src").join("lib.rs"), "")?;

    // Pin the target directory instead of letting cargo choose it. A user
    // with CARGO_TARGET_DIR set in their environment -- a common way to share
    // one build cache across projects -- would otherwise have the JSON written
    // somewhere else entirely, and the read below would fail with a bare
    // "cannot find the path specified" naming a directory cargo never used.
    let doc_target = work.join("target");
    let output = Command::new("cargo")
        .arg("+nightly")
        .arg("rustdoc")
        .arg("--target-dir")
        .arg(&doc_target)
        .arg("-p")
        .arg(crate_name)
        .arg("--")
        .arg("-Z")
        .arg("unstable-options")
        .arg("--output-format")
        .arg("json")
        .current_dir(&work)
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "could not run `cargo`: {e}. Jux generates a crate's API stub \
                 from rustdoc JSON, so `cargo` must be on PATH."
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // rustup says this when the toolchain is absent; it is the single
        // most common reason a first `rust.<crate>` dependency fails, and the
        // fix is one command.
        if stderr.contains("is not installed") || stderr.contains("no such toolchain") {
            anyhow::bail!(
                "the `nightly` toolchain is required to read a crate's API \
                 (rustdoc JSON is a nightly feature) and is not installed.\n\
                 Install it with:\n    rustup toolchain install nightly\n\
                 Then add the docs component:\n    \
                 rustup component add rust-docs-json --toolchain nightly"
            );
        }
        anyhow::bail!(
            "`cargo +nightly rustdoc --output-format json` failed for `{crate_name}`:\n{stderr}"
        );
    }
    // rustdoc writes `<crate>.json` (hyphens become underscores in the file).
    let json_name = format!("{sanitized}.json");
    let json_path = doc_target.join("doc").join(&json_name);
    std::fs::read_to_string(&json_path).map_err(|e| {
        anyhow::anyhow!(
            "rustdoc reported success but wrote no JSON for `{crate_name}` \
             (looked at {}): {e}.\n\
             This usually means the nightly toolchain is missing its docs \
             component:\n    rustup component add rust-docs-json --toolchain nightly",
            json_path.display(),
        )
    })
}

/// The throwaway-Cargo-project directory used to rustdoc one foreign crate:
/// `<user-cache>/juxc/rustdoc-gen/<crate>/`. Reused across runs so cargo's own
/// caching makes regeneration cheap. `None` when no cache root resolves.
fn rustdoc_gen_dir(crate_name: &str) -> Option<PathBuf> {
    user_cache_dir().map(|d| {
        d.join("juxc")
            .join("rustdoc-gen")
            .join(crate_name.replace('-', "_"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;


    /// Where a bound crate comes from, and the §B.5.5 priority between the
    /// three spellings.
    #[test]
    fn crate_source_follows_manifest_priority() {
        use juxc_backend_rust::CrateSource;

        let registry = crate::manifest::Dependency {
            name: "rust.serde_json".to_string(),
            path: None,
            version: Some("1.0".to_string()),
            git: None,
            git_ref: None,
        };
        assert_eq!(crate_source_of(&registry), CrateSource::Registry);

        let git = crate::manifest::Dependency {
            name: "rust.mine".to_string(),
            path: None,
            version: None,
            git: Some("https://example.com/mine".to_string()),
            git_ref: Some(crate::manifest::GitRef::Tag("v1".to_string())),
        };
        assert_eq!(
            crate_source_of(&git),
            CrateSource::Git {
                url: "https://example.com/mine".to_string(),
                pin: Some(("tag".to_string(), "v1".to_string())),
            },
        );

        // path wins over git, per §B.5.5.
        let both = crate::manifest::Dependency {
            name: "rust.mine".to_string(),
            path: Some(std::path::PathBuf::from("/crates/mine")),
            version: None,
            git: Some("https://example.com/mine".to_string()),
            git_ref: None,
        };
        assert!(matches!(crate_source_of(&both), CrateSource::Path(_)));
    }

    /// The cache marker separates stubs generated from different sources.
    ///
    /// Without this, repointing `rust.mine` from crates.io at a local
    /// checkout would keep serving the registry crate's API from the cache,
    /// and the editor would describe methods the linked crate does not have.
    #[test]
    fn stub_cache_marker_distinguishes_sources() {
        use juxc_backend_rust::CrateSource;

        let registry = crate_cache_header_for(&CrateSource::Registry);
        let local = crate_cache_header_for(&CrateSource::Path("/crates/mine".to_string()));
        let git = crate_cache_header_for(&CrateSource::Git {
            url: "https://example.com/mine".to_string(),
            pin: None,
        });
        assert_ne!(registry, local);
        assert_ne!(registry, git);
        assert_ne!(local, git);

        // A stub written for one source is stale for another.
        let dir = std::env::temp_dir().join(format!("juxc-stub-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mine.jux.d");
        std::fs::write(&path, format!("{local}package rust.mine;\n")).unwrap();

        assert!(crate_stub_cache_is_fresh_for(
            &path,
            &CrateSource::Path("/crates/mine".to_string())
        ));
        assert!(!crate_stub_cache_is_fresh_for(&path, &CrateSource::Registry));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The dependency line a local crate gets is a path table, not a version.
    #[test]
    fn rustdoc_manifest_names_the_same_crate_the_build_links() {
        use juxc_backend_rust::CrateSource;
        let line = juxc_backend_rust::registry_dep_line_for(
            "mylocal",
            "*",
            &CrateSource::Path("/crates/mylocal".to_string()),
        );
        assert_eq!(line, "mylocal = { path = \"/crates/mylocal\" }\n");
    }

    #[test]
    fn stub_path_recognised_by_double_extension() {
        assert!(is_stub_path(Path::new("rust/std.jux.d")));
        assert!(is_stub_path(Path::new("/abs/path/serde_json.jux.d")));
        // Plain `.jux` and stray `.d` are NOT stubs.
        assert!(!is_stub_path(Path::new("main.jux")));
        assert!(!is_stub_path(Path::new("build.d")));
    }

    #[test]
    fn foreign_dep_kinds_split() {
        assert_eq!(
            foreign_dep_kind("rust.serde_json"),
            Some(("rust", "serde_json"))
        );
        assert_eq!(foreign_dep_kind("c.sqlite3"), Some(("c", "sqlite3")));
        assert_eq!(foreign_dep_kind("cpp.myengine"), Some(("cpp", "myengine")));
        // An ordinary Jux path dependency is not foreign.
        assert_eq!(foreign_dep_kind("greeter"), None);
        assert_eq!(foreign_dep_kind("com.example.lib"), None);
    }

    /// End-to-end bindgen path on a CHECKED-IN real rustdoc-JSON fixture
    /// (`cargo +nightly rustdoc --output-format json` on a tiny crate): the
    /// JSON ingests through `juxc_bindgen` and renders a Jux-syntax `.jux.d`
    /// stub — a `class Pt` with a Jux constructor, fields, and a method whose
    /// body is `;`. This is the exact code path `resolve_crate_stub` runs after
    /// shelling out to rustdoc, verified without needing nightly at test time.
    #[test]
    fn bindgen_renders_jux_stub_from_rustdoc_fixture() {
        let json = include_str!("../tests/fixtures/minicrate.rustdoc.json");
        let stub = generate_stub_from_rustdoc_json(json, "rust.minicrate")
            .expect("bindgen ingests the rustdoc fixture");
        assert!(stub.contains("package rust.minicrate;"), "stub:\n{stub}");
        assert!(
            stub.contains("class Pt"),
            "expected `class Pt`, got:\n{stub}"
        );
        // Jux-syntax constructor from `Pt::new(i32, i32)`, body elided to `;`.
        assert!(
            stub.contains("public Pt(i32 x, i32 y);"),
            "ctor missing:\n{stub}"
        );
        // Instance method `sum(&self) -> i32` → `public i32 sum();`.
        assert!(
            stub.contains("public i32 sum();"),
            "method missing:\n{stub}"
        );
    }

    /// `resolve_crate_stub` returns a cache-hit path without shelling out to
    /// cargo when the `.jux.d` already exists under `.jux-stubs/`.
    #[test]
    fn resolve_crate_stub_returns_cache_hit() {
        let dir = std::env::temp_dir().join(format!("juxc-stub-cache-{}", std::process::id()));
        let cached = dir
            .join(PROJECT_STUB_DIRNAME)
            .join("rust")
            .join("serde_json.jux.d");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        // WITH the cache marker: a stub that lacks one is stale by
        // definition, so a bare package line here would send the call below
        // to `cargo rustdoc` and the test would not be testing a cache hit.
        let source = juxc_backend_rust::CrateSource::Registry;
        std::fs::write(
            &cached,
            format!("{}package rust.serde_json;\n", crate_cache_header_for(&source)),
        )
        .unwrap();

        let got = resolve_crate_stub(&dir, "rust", "serde_json", None, &source)
            .expect("cache hit");
        assert_eq!(got, cached);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The vendored `rust.std` snapshot must stay in lockstep with
    /// [`STD_STUB_CACHE_VERSION`]. Bumping that constant means the bindgen
    /// surface changed, which makes the frozen copy wrong for everyone who
    /// lacks the nightly `rust-docs-json` component — and they are the majority,
    /// so the staleness would be invisible in normal development. Failing here
    /// is the reminder to regenerate it (see [`VENDORED_RUST_STD`]).
    #[test]
    fn vendored_std_stub_is_current() {
        // Compare the header LINE, not a prefix of the file: the snapshot is a
        // checked-in text file, so git may hand it back with CRLF endings on a
        // Windows checkout and a raw prefix test would fail on the invisible
        // `` while reporting two identical-looking strings.
        let want = std_cache_header();
        let want = want.trim_end();
        let got = VENDORED_RUST_STD.lines().next().unwrap_or("").trim_end();
        assert_eq!(
            got, want,
            "crates/juxc-driver/stubs/rust-std.jux.d is stale. Regenerate it -- see the VENDORED_RUST_STD doc comment.",
        );
    }

    /// The snapshot has to actually carry the prelude collections, otherwise the
    /// fallback resolves to an empty surface and we are back to the silent
    /// evaporation it exists to prevent.
    #[test]
    fn vendored_std_stub_carries_the_prelude_types() {
        let src = vendored_std_stub();
        for ty in [
            "class Vec",
            "class HashMap",
            "class VecDeque",
            "class HashSet",
        ] {
            assert!(
                src.contents().contains(ty),
                "vendored rust.std is missing `{ty}`"
            );
        }
        assert!(
            src.contents().contains("package rust.std;"),
            "missing package header"
        );
    }
}
