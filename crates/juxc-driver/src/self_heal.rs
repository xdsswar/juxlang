//! Self-healing lowering (GAPS.md gap 34, ERRATA E130).
//!
//! Jux never shows its user Rust (ERRATA E116, E129), and a program the front
//! end accepted must build: every rustc rejection of the emitted crate is a
//! compiler bug (ERRATA E23). This module is where the driver makes good on
//! the second promise when the backend breaks the first.
//!
//! The backend has two lowering levels (`juxc_backend_rust::LoweringPlan`,
//! documented in the backend's `lowering_level` module): **fast**, the normal
//! lowering, and **safe**, the same lowering with every judgement that can
//! produce a borrow, move or inference error turned to its general answer.
//! A build goes:
//!
//! 1. Everything fast, except what the build directory's cache
//!    (`.jux-safe-fns`) says needed the safe level last time.
//! 2. If rustc refuses the crate, each error's primary span is traced through
//!    the `// JUX:` markers to the `.jux` line, and from there to the Jux
//!    function whose source holds it ([`juxc_backend_rust::region_at`]).
//!    Those functions are lowered again at the safe level and the crate is
//!    rebuilt. At most [`MAX_RETRIES`] times.
//! 3. If that does not converge, or an error lies outside every function (a
//!    declaration's own shape), the whole program is lowered safe, once.
//! 4. Only if that fails too does the user see the `E0900` of step 2's first
//!    failure: the fast lowering's error is the one that names the bug.
//!
//! A heal is silent: `--verbose` adds one line saying how many functions were
//! compiled in compatibility mode, and the cache takes the next build straight
//! there. Only a failure rustc reported inside the emitted crate (an `E0900`)
//! heals; a dependency that cannot be fetched (`E0905`) or a link failure
//! (`E0906`) is not the lowering's fault and is reported as before.
//!
//! **The compiler's own tests** set `JUX_SELFCHECK=1`. There a heal is a
//! failure: the build stops with an `E0900` per function that needed the safe
//! level, naming it and quoting the fast lowering's rustc error, and the cache
//! is neither read nor written. The fast path is still to be fixed; the heal
//! is what a user gets meanwhile.
//!
//! **`JUX_FORCE_SAFE=1`** lowers everything at the safe level from the start,
//! which is how the corpus proves the safe level is itself correct
//! (`bin/jux/tests/safe_mode.rs`). **`JUX_TEST_BREAK_FAST=[<kind>:]<key>`**
//! is a test-only hook that puts a historical backend bug back into the named
//! function (`juxc_backend_rust::BreakKind`: a borrow conflict by default, or
//! one of the judgements gaps 30-40b fixed), so the tests can watch a heal
//! happen and prove the safe level rescues each family of rustc error.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use juxc_backend_rust::{FnRegion, LoweringPlan, RustCrate};
use juxc_diagnostics::{code::Code, Diagnostic};
use juxc_source::SourceFile;

use crate::build_failure::BuildFailure;
use crate::source_map::SourceMap;

/// How many times the functions rustc refused are lowered safe before the
/// whole program is.
pub(crate) const MAX_RETRIES: usize = 2;

/// Where the build directory remembers the functions that needed the safe
/// level.
const CACHE_FILE: &str = ".jux-safe-fns";

/// The cache entry that means "the whole program".
const WHOLE_PROGRAM: &str = "*";

/// Whether a variable is set to anything but empty or `0`.
fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// The plan a program is first lowered with: all fast, or all safe under
/// `JUX_FORCE_SAFE=1`, with the test hook when `JUX_TEST_BREAK_FAST` names a
/// function.
pub(crate) fn initial_plan() -> LoweringPlan {
    LoweringPlan {
        safe: BTreeSet::new(),
        all_safe: env_flag("JUX_FORCE_SAFE"),
        break_fast: std::env::var("JUX_TEST_BREAK_FAST").ok().filter(|v| !v.is_empty()),
    }
}

/// Lower a checked program so that it can be lowered again: the returned
/// crate carries a [`juxc_backend_rust::Relower`] holding the program, and
/// its functions' regions.
pub(crate) fn lower_healable<F>(
    units: Vec<juxc_ast::CompilationUnit>,
    symbols: juxc_tycheck::SymbolTable,
    expr_types: std::collections::HashMap<juxc_source::Span, juxc_tycheck::Ty>,
    sources: &[SourceFile],
    lower: F,
) -> RustCrate
where
    F: Fn(
            &[juxc_ast::CompilationUnit],
            &juxc_tycheck::SymbolTable,
            &std::collections::HashMap<juxc_source::Span, juxc_tycheck::Ty>,
            &[SourceFile],
        ) -> RustCrate
        + 'static,
{
    let regions = juxc_backend_rust::function_regions(&units, sources);
    let program = std::rc::Rc::new((units, symbols, expr_types, sources.to_vec(), lower));
    let relower = juxc_backend_rust::Relower(std::rc::Rc::new(move |plan: &LoweringPlan| {
        let (units, symbols, expr_types, sources, lower) = &*program;
        let mut produced =
            juxc_backend_rust::with_lowering_plan(plan, &regions, || lower(units, symbols, expr_types, sources));
        produced.fn_regions = regions.clone();
        produced
    }));
    let mut produced = relower.lower(&initial_plan());
    produced.relower = Some(relower);
    produced
}

/// One error rustc reported in the emitted crate, with where it points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RustcError {
    /// `E0502`, or `None` for an error without a code.
    pub code: Option<String>,
    /// rustc's first line.
    pub message: String,
    /// The primary span: crate-relative file, 1-based line and column.
    pub file: String,
    pub line: u32,
    pub col: u32,
}

/// The errors in cargo's JSON messages (`--message-format=json`) that have a
/// primary span in the emitted crate.
pub(crate) fn rustc_errors(messages: &str) -> Vec<RustcError> {
    let mut out = Vec::new();
    for line in messages.lines() {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if json.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(message) = json.get("message") else { continue };
        if !message.get("level").and_then(|l| l.as_str()).is_some_and(|l| l.starts_with("error")) {
            continue;
        }
        let text = message.get("message").and_then(|m| m.as_str()).unwrap_or("");
        let Some((file, line, col)) = crate::build_failure::primary_span(message) else { continue };
        let code = message.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str()).map(str::to_string);
        let error = RustcError { code, message: text.lines().next().unwrap_or("").to_string(), file, line, col };
        if !out.contains(&error) {
            out.push(error);
        }
    }
    out
}

/// A rustc error traced back to Jux: the function it is in (`None` when it is
/// in no function), and the `.jux` position the markers give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Traced {
    pub error: RustcError,
    pub function: Option<String>,
    pub jux: Option<(String, u32, u32)>,
}

/// Trace each error to the Jux function whose emitted code it is in.
pub(crate) fn trace_errors(errors: &[RustcError], map: &SourceMap, regions: &[FnRegion]) -> Vec<Traced> {
    errors
        .iter()
        .map(|e| {
            let jux = map.lookup(&e.file, e.line).map(|m| (m.jux_path.clone(), m.jux_line, m.jux_col));
            let function = jux
                .as_ref()
                .and_then(|(path, line, col)| juxc_backend_rust::region_at(regions, path, *line, *col))
                .map(|r| r.key.clone());
            Traced { error: e.clone(), function, jux }
        })
        .collect()
}

/// What to lower next after a failed build: the functions to add to the safe
/// set, or the whole program, or nothing more to try (`None`).
pub(crate) fn next_plan(plan: &LoweringPlan, traced: &[Traced], retries: usize) -> Option<LoweringPlan> {
    if plan.all_safe {
        return None;
    }
    let all_mapped = traced.iter().all(|t| t.function.is_some());
    let fresh: BTreeSet<String> =
        traced.iter().filter_map(|t| t.function.clone()).filter(|k| !plan.safe.contains(k)).collect();
    let mut next = plan.clone();
    if all_mapped && !fresh.is_empty() && retries < MAX_RETRIES {
        next.safe.extend(fresh);
    } else {
        next.all_safe = true;
    }
    Some(next)
}

/// The functions the cache in `crate_dir` says need the safe level, if it
/// was written by this compiler.
fn load_cache(crate_dir: &Path) -> Option<(BTreeSet<String>, bool)> {
    let text = std::fs::read_to_string(crate_dir.join(CACHE_FILE)).ok()?;
    let mut lines = text.lines();
    if lines.next()? != cache_header() {
        return None;
    }
    let mut safe = BTreeSet::new();
    let mut all = false;
    for line in lines.map(str::trim).filter(|l| !l.is_empty()) {
        if line == WHOLE_PROGRAM {
            all = true;
        } else {
            safe.insert(line.to_string());
        }
    }
    Some((safe, all))
}

fn cache_header() -> String {
    format!("# functions compiled in compatibility mode, juxc {}", env!("CARGO_PKG_VERSION"))
}

fn save_cache(crate_dir: &Path, plan: &LoweringPlan) {
    let path = crate_dir.join(CACHE_FILE);
    if plan.safe.is_empty() && !plan.all_safe {
        let _ = std::fs::remove_file(path);
        return;
    }
    let mut text = cache_header();
    text.push('\n');
    if plan.all_safe {
        text.push_str(WHOLE_PROGRAM);
        text.push('\n');
    }
    for key in &plan.safe {
        text.push_str(key);
        text.push('\n');
    }
    let _ = std::fs::write(path, text);
}

/// Build `crate_`, healing it as the module docs describe. `write` puts a
/// crate on disk (and runs the pre-build checks) and returns its `.rs` files;
/// `cargo` builds what is on disk, `Ok(None)` on success and cargo's
/// `(stdout, stderr)` on failure; `failure` turns a failed build into the
/// error the user sees.
pub(crate) fn build(
    crate_: &RustCrate,
    crate_dir: &Path,
    write: &mut dyn FnMut(&RustCrate) -> Result<Vec<PathBuf>>,
    cargo: &mut dyn FnMut() -> Result<Option<(String, String)>>,
    failure: &dyn Fn(&str, &str, &SourceMap) -> anyhow::Error,
) -> Result<()> {
    let selfcheck = crate::borrow_selfcheck::enabled();
    let start = crate_.plan.clone();
    let mut plan = start.clone();
    let mut healed: Option<RustCrate> = None;
    if let (Some(relower), false) = (&crate_.relower, selfcheck) {
        if let Some((safe, all)) = load_cache(crate_dir) {
            let mut cached = plan.clone();
            cached.safe.extend(safe);
            cached.all_safe |= all;
            if cached != plan {
                plan = cached;
                healed = Some(relower.lower(&plan));
            }
        }
    }
    // The fast lowering's failure, and what it traced to.
    let mut first: Option<(anyhow::Error, Vec<Traced>)> = None;
    let mut retries = 0;
    loop {
        let current = healed.as_ref().unwrap_or(crate_);
        let rs_files = write(current)?;
        let Some((stdout, stderr)) = cargo()? else { break };
        let map = SourceMap::from_disk(crate_dir, &rs_files);
        let err = failure(&stdout, &stderr, &map);
        let lowering_fault = err
            .downcast_ref::<BuildFailure>()
            .is_some_and(|f| f.exit_code == crate::ice::ICE_EXIT_CODE && f.diagnostics.iter().any(|d| d.code == Code::E0900_BackendEmittedInvalidRust));
        let Some(relower) = crate_.relower.as_ref().filter(|_| lowering_fault) else {
            return Err(err);
        };
        let traced = trace_errors(&rustc_errors(&stdout), &map, &current.fn_regions);
        let next = next_plan(&plan, &traced, retries);
        if first.is_none() {
            first = Some((err, traced));
        }
        let Some(next) = next else {
            return Err(first.map(|(e, _)| e).expect("recorded above"));
        };
        if crate::render::verbose() {
            if next.all_safe {
                eprintln!("note: retrying the whole program in compatibility mode");
            } else {
                let fresh: Vec<String> = next.safe.difference(&plan.safe).map(|k| format!("`{k}`")).collect();
                eprintln!("note: retrying {} in compatibility mode", fresh.join(", "));
            }
        }
        if !next.all_safe {
            retries += 1;
        }
        plan = next;
        healed = Some(relower.lower(&plan));
    }
    if plan == start {
        return Ok(());
    }
    if selfcheck {
        if let Some((_, traced)) = &first {
            return Err(selfcheck_failure(traced).into());
        }
    } else {
        save_cache(crate_dir, &plan);
    }
    if crate::render::verbose() && !start.all_safe {
        let count = if plan.all_safe { "all".to_string() } else { plan.safe.len().to_string() };
        let noun = if plan.safe.len() == 1 && !plan.all_safe { "function" } else { "functions" };
        eprintln!("note: {count} {noun} compiled in compatibility mode");
    }
    Ok(())
}

/// Under `JUX_SELFCHECK=1`, a heal is an `E0900` per function the fast
/// lowering got wrong, at the `.jux` line of its rustc error.
pub(crate) fn selfcheck_failure(traced: &[Traced]) -> BuildFailure {
    let mut failure =
        BuildFailure { diagnostics: Vec::new(), sources: Vec::new(), detail: String::new(), exit_code: crate::ice::ICE_EXIT_CODE };
    let mut seen: Vec<(Option<String>, Option<String>)> = Vec::new();
    for t in traced {
        let key = (t.function.clone(), t.error.code.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let who = match &t.function {
            Some(f) => format!("`{f}`"),
            None => "the program".to_string(),
        };
        let mut d = Diagnostic::error(
            Code::E0900_BackendEmittedInvalidRust,
            format!("internal compiler error: {who} compiled only in compatibility mode"),
        );
        let rustc = t.error.code.as_deref().map(|c| format!("error[{c}]")).unwrap_or_else(|| "error".to_string());
        d.notes.push(format!("rustc reported {rustc}: {}", t.error.message));
        if let Some((path, line, col)) = &t.jux {
            match crate::build_failure::locate(path, *line, *col, &mut failure.sources) {
                Some((span, index)) => d = d.with_span(span).with_file(index),
                None => d.notes.push(format!("the failing code was generated for {path}:{line}:{col}")),
            }
        }
        d.notes.push(
            "the fast lowering of this code does not compile, and the safe lowering (GAPS.md gap 34) had to \
             stand in for it; a user's build would have succeeded, but the fast lowering is a compiler bug"
                .to_string(),
        );
        failure.diagnostics.push(d.with_help(format!(
            "`JUX_SELFCHECK=1` treats every fallback as a failure; fix the fast lowering, or report it at {}",
            crate::ice::ISSUES_URL
        )));
    }
    failure
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN_RS: &str = "\
fn main() {
    // JUX:app.jux:3:5
    let x = 1;
}
impl Counter {
    pub fn bump(&self) {
        // JUX:app.jux:9:9
        let v = vec![0];
        // JUX:app.jux:10:9
        v.push(1);
    }
}
struct Shape {}
";

    fn regions() -> Vec<FnRegion> {
        let region = |key: &str, start: (u32, u32), end: (u32, u32), span: (u32, u32)| FnRegion {
            key: key.to_string(),
            owner_class: None,
            span: juxc_source::Span::new(span.0, span.1),
            path: "app.jux".to_string(),
            start,
            end,
        };
        vec![
            region("main", (2, 1), (4, 1), (10, 60)),
            region("Counter", (7, 1), (12, 1), (100, 300)),
            region("Counter.bump", (8, 5), (11, 5), (120, 250)),
        ]
    }

    fn cargo_error(code: &str, file: &str, line: u32, col: u32) -> String {
        serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "message": format!("an error numbered {code}\nsecond line"),
                "code": {"code": code},
                "level": "error",
                "spans": [
                    {"file_name": "src/other.rs", "line_start": 1, "column_start": 1, "is_primary": false},
                    {"file_name": file, "line_start": line, "column_start": col, "is_primary": true},
                ],
                "rendered": "rendered",
            }
        })
        .to_string()
    }

    /// cargo's JSON stream gives each error with its primary span; notes,
    /// warnings and the "aborting" summary have none and are dropped.
    #[test]
    fn rustc_json_gives_each_error_and_its_primary_span() {
        let stream = [
            cargo_error("E0502", "src\\main.rs", 10, 9),
            r#"{"reason":"compiler-message","message":{"message":"unused","code":null,"level":"warning","spans":[]}}"#.to_string(),
            r#"{"reason":"compiler-message","message":{"message":"aborting due to 1 previous error","code":null,"level":"error","spans":[]}}"#.to_string(),
            r#"{"reason":"build-finished","success":false}"#.to_string(),
            "not json".to_string(),
        ]
        .join("\n");
        let errors = rustc_errors(&stream);
        assert_eq!(
            errors,
            vec![RustcError {
                code: Some("E0502".to_string()),
                message: "an error numbered E0502".to_string(),
                file: "src\\main.rs".to_string(),
                line: 10,
                col: 9,
            }]
        );
    }

    /// An error inside a method's emitted body maps to that method, the
    /// innermost function holding its `.jux` line; one above every marker, or
    /// in code no function holds, maps to none.
    #[test]
    fn an_error_maps_to_the_function_that_emitted_it() {
        let map = SourceMap::from_sources(&[("src/main.rs", MAIN_RS)]);
        let errors = rustc_errors(
            &[
                cargo_error("E0502", "src/main.rs", 10, 9),
                cargo_error("E0382", "src\\main.rs", 3, 5),
                cargo_error("E0277", "src/main.rs", 13, 1),
            ]
            .join("\n"),
        );
        let traced = trace_errors(&errors, &map, &regions());
        let functions: Vec<Option<&str>> = traced.iter().map(|t| t.function.as_deref()).collect();
        assert_eq!(functions, vec![Some("Counter.bump"), Some("main"), Some("Counter.bump")]);
        assert_eq!(traced[0].jux, Some(("app.jux".to_string(), 10, 9)));
        // Line 13 (`struct Shape`) is after the last marker, which is inside
        // `bump`: the markers alone cannot tell, so it goes with the method.
        let outside = trace_errors(
            &[RustcError { code: None, message: String::new(), file: "src/main.rs".into(), line: 1, col: 1 }],
            &map,
            &regions(),
        );
        assert_eq!(outside[0].function, None, "above every marker");
        assert_eq!(outside[0].jux, None);
    }

    /// Two retries on the functions rustc named, then the whole program
    /// once, then nothing more; an error no function holds skips straight to
    /// the whole program.
    #[test]
    fn the_retry_plan_widens_then_gives_up() {
        let traced = |f: Option<&str>| Traced {
            error: RustcError { code: None, message: String::new(), file: String::new(), line: 1, col: 1 },
            function: f.map(str::to_string),
            jux: None,
        };
        let fast = LoweringPlan::default();
        let one = next_plan(&fast, &[traced(Some("A.f"))], 0).expect("retry");
        assert_eq!(one.safe.iter().collect::<Vec<_>>(), vec!["A.f"]);
        assert!(!one.all_safe);
        let two = next_plan(&one, &[traced(Some("B.g"))], 1).expect("retry");
        assert_eq!(two.safe.len(), 2);
        let whole = next_plan(&two, &[traced(Some("C.h"))], 2).expect("escalate");
        assert!(whole.all_safe, "after two retries the whole program");
        assert!(next_plan(&whole, &[traced(Some("C.h"))], 2).is_none(), "nothing left to try");
        let same = next_plan(&one, &[traced(Some("A.f"))], 1).expect("escalate");
        assert!(same.all_safe, "a function already safe that still fails escalates");
        let unmapped = next_plan(&fast, &[traced(Some("A.f")), traced(None)], 0).expect("escalate");
        assert!(unmapped.all_safe, "an error in no function escalates at once");
    }

    /// The cache survives a round trip, and one from another compiler (or a
    /// damaged one) is ignored.
    #[test]
    fn the_cache_round_trips_and_ignores_a_foreign_one() {
        let dir = std::env::temp_dir().join(format!("jux-safe-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut plan = LoweringPlan::default();
        plan.safe.insert("Counter.bump".to_string());
        save_cache(&dir, &plan);
        assert_eq!(load_cache(&dir), Some((plan.safe.clone(), false)));
        plan.all_safe = true;
        save_cache(&dir, &plan);
        assert_eq!(load_cache(&dir).map(|(_, all)| all), Some(true));
        std::fs::write(dir.join(CACHE_FILE), "# another compiler\nX.y\n").unwrap();
        assert_eq!(load_cache(&dir), None);
        save_cache(&dir, &LoweringPlan::default());
        assert!(!dir.join(CACHE_FILE).exists(), "an all-fast plan leaves no cache");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The self-check's report names the function and quotes the fast
    /// lowering's error, in the one note the leak guard allows to.
    #[test]
    fn the_selfcheck_report_names_the_function_and_the_error() {
        let failure = selfcheck_failure(&[Traced {
            error: RustcError {
                code: Some("E0502".to_string()),
                message: "cannot borrow `v` as mutable".to_string(),
                file: "src/main.rs".to_string(),
                line: 10,
                col: 9,
            },
            function: Some("Counter.bump".to_string()),
            jux: None,
        }]);
        assert_eq!(failure.exit_code, crate::ice::ICE_EXIT_CODE);
        let d = &failure.diagnostics[0];
        assert!(d.message.contains("`Counter.bump` compiled only in compatibility mode"), "{}", d.message);
        assert!(d.notes.iter().any(|n| n == "rustc reported error[E0502]: cannot borrow `v` as mutable"), "{:?}", d.notes);
        assert!(juxc_diagnostics::leak::leak_in_diagnostic(d, &|_| false).is_none(), "{d:?}");
    }
}
