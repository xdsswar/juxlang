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

/// Bump to invalidate previously-cached generated `rust.std` stubs when the
/// bindgen surface or the merge set changes. Embedded in the cache header and
/// checked on load.
const STD_STUB_CACHE_VERSION: u32 = 7;

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
pub fn drop_external_diagnostics(diagnostics: &mut Vec<Diagnostic>, sources: &[SourceFile]) {
    diagnostics.retain(|d| match d.file {
        Some(idx) => !sources.get(idx).is_some_and(|s| is_stub_path(s.path())),
        None => true,
    });
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
///
/// Every failure mode degrades gracefully to an empty list: std autocomplete is
/// simply unavailable (e.g. the `rust-docs-json` rustup component isn't
/// installed), never a hard error.
pub fn load_std_stub_sources() -> Vec<SourceFile> {
    // (1) Explicit override — a directory of `.jux.d` files, loaded as-is.
    if let Ok(dir) = std::env::var("JUX_STUBS_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return collect_stub_sources(&p);
        }
    }
    // (2)+(3) Cached-or-generated `rust.std`.
    match cached_or_generated_std_stub() {
        Ok(Some(src)) => vec![src],
        _ => Vec::new(),
    }
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
        let _ = std::fs::write(cache, &text);
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
    let refs: Vec<(&str, &str)> = jsons.iter().map(|(n, j)| (n.as_str(), j.as_str())).collect();
    let stub = juxc_bindgen::ingest::generate_merged(&refs, "rust.std")
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
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|c| SourceFile::new(p, c)))
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
pub fn resolve_crate_stub(
    project_root: &Path,
    kind: &str,
    crate_name: &str,
    version: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let cache = crate_stub_cache_path(project_root, kind, crate_name);
    if cache.is_file() {
        return Ok(cache);
    }
    if kind != "rust" {
        anyhow::bail!(
            "no cached stub for `{kind}.{crate_name}` at {} (C/C++ stub generation \
             is not wired in this phase — vendor a `.jux.d` into `.jux-stubs/{kind}/`)",
            cache.display()
        );
    }

    let json = run_cargo_rustdoc_json(crate_name, version)?;
    let package = format!("rust.{crate_name}");
    let stub = generate_stub_from_rustdoc_json(&json, &package)
        .map_err(|e| anyhow::anyhow!("bindgen failed to ingest rustdoc JSON for `{crate_name}`: {e}"))?;

    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&cache, stub)?;
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
fn run_cargo_rustdoc_json(crate_name: &str, version: Option<&str>) -> anyhow::Result<String> {
    let work = rustdoc_gen_dir(crate_name)
        .ok_or_else(|| anyhow::anyhow!("no cache directory to generate rustdoc JSON in"))?;
    std::fs::create_dir_all(work.join("src"))?;
    // A minimal package whose only purpose is to pull `crate_name` into a
    // resolvable dependency graph for rustdoc.
    let sanitized = crate_name.replace('-', "_");
    let ver = version.unwrap_or("*");
    let cargo_toml = format!(
        "[package]\nname = \"__juxc_doc_{sanitized}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [dependencies]\n{crate_name} = \"{ver}\"\n",
    );
    std::fs::write(work.join("Cargo.toml"), cargo_toml)?;
    std::fs::write(work.join("src").join("lib.rs"), "")?;

    let output = Command::new("cargo")
        .arg("+nightly")
        .arg("rustdoc")
        .arg("-p")
        .arg(crate_name)
        .arg("--")
        .arg("-Z")
        .arg("unstable-options")
        .arg("--output-format")
        .arg("json")
        .current_dir(&work)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to spawn `cargo +nightly rustdoc`: {e}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "`cargo +nightly rustdoc --output-format json` failed for `{crate_name}`:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // rustdoc writes `<crate>.json` (hyphens become underscores in the file).
    let json_name = format!("{sanitized}.json");
    let json_path = work.join("target").join("doc").join(&json_name);
    std::fs::read_to_string(&json_path).map_err(|e| {
        anyhow::anyhow!("rustdoc JSON not found at {}: {e}", json_path.display())
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
        assert_eq!(foreign_dep_kind("rust.serde_json"), Some(("rust", "serde_json")));
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
        assert!(stub.contains("class Pt"), "expected `class Pt`, got:\n{stub}");
        // Jux-syntax constructor from `Pt::new(i32, i32)`, body elided to `;`.
        assert!(stub.contains("public Pt(i32 x, i32 y);"), "ctor missing:\n{stub}");
        // Instance method `sum(&self) -> i32` → `public i32 sum();`.
        assert!(stub.contains("public i32 sum();"), "method missing:\n{stub}");
    }

    /// `resolve_crate_stub` returns a cache-hit path without shelling out to
    /// cargo when the `.jux.d` already exists under `.jux-stubs/`.
    #[test]
    fn resolve_crate_stub_returns_cache_hit() {
        let dir = std::env::temp_dir().join(format!("juxc-stub-cache-{}", std::process::id()));
        let cached = dir.join(PROJECT_STUB_DIRNAME).join("rust").join("serde_json.jux.d");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, "package rust.serde_json;\n").unwrap();

        let got = resolve_crate_stub(&dir, "rust", "serde_json", None).expect("cache hit");
        assert_eq!(got, cached);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
