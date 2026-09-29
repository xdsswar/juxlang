//! The safe lowering level is itself correct, and a build heals through it
//! (GAPS.md gap 34).
//!
//! When rustc refuses the Rust the fast lowering emitted, the driver lowers
//! the functions it refused again at the safe level and rebuilds
//! (`crates/juxc-driver/src/self_heal.rs`). That is only worth anything if
//! the safe level compiles and means the same thing. So:
//!
//! - `a_corpus_slice_prints_the_same_under_forced_safe_mode` builds a slice of
//!   the example corpus with `JUX_FORCE_SAFE=1` (every function safe, every
//!   class `rc-refcell`) under the borrow self-check, and holds each to the
//!   SAME `tests/expected/examples/<name>.expected` the fast corpus
//!   (`run.rs`) pins. `JUX_SAFE_CORPUS=all` runs the whole corpus instead,
//!   and `JUX_SAFE_CORPUS=a,b` just the examples named.
//! - the heal tests break one function's fast lowering on purpose
//!   (`JUX_TEST_BREAK_FAST`, a test-only hook that opens its body with a
//!   borrow conflict rustc refuses) and check that the build succeeds and
//!   prints the right output, that the cache sends the next build straight to
//!   the safe level, and that under `JUX_SELFCHECK=1` the same heal fails
//!   loudly, naming the function and rustc's original error.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// The slice: at least sixty examples across classes and their
/// representations, generics, closures, async and workers, collections, and
/// foreign (`rust.*`) APIs, plus the borrow-heavy stress programs.
const SLICE: &[&str] = &[
    // Classes, representations, hierarchies, records, operators.
    "animals",
    "access_control",
    "builder_pattern",
    "construction_and_identity",
    "constructor_chains",
    "ctor_calls_method",
    "cr_rep_arc",
    "cr_rep_hierarchy",
    "cr_rep_rc",
    "cr_rep_value",
    "encapsulation",
    "polymorphism",
    "shapes",
    "nested_types",
    "init_blocks",
    "drop_order",
    "observable_props_full",
    "ref_fields",
    "sealed_shapes",
    "record_methods",
    "record_destructuring",
    "op_overload",
    "operator_overloads_by_operand",
    "iface_dispatch",
    "interface_default_inherit",
    "weak_refs",
    "value_struct_elements",
    // Re-entrancy and borrow stress.
    "reentrant_callbacks",
    "reentrant_operands",
    "reentrant_stored_lambda",
    "reentrancy_stress",
    "stress_borrow",
    "stress_employees",
    "stress_zoo",
    "stress_kitchen_sink",
    // Generics.
    "box_generic",
    "bounded_generic",
    "generic_stack_pop",
    "generics_full_matrix",
    "generics_registry",
    "generics_stress",
    "generic_base_poly",
    "generic_method_bounds",
    "wildcards",
    "where_constraints",
    "extends_generic",
    // Closures and function values.
    "lambdas",
    "closure_capture_shapes",
    "closure_value_captures",
    "mut_capture_closure",
    "higher_order",
    "functions_as_values",
    "method_references",
    "fn_type_polymorphic_callbacks",
    "event_callbacks_field",
    "stress_higher_order",
    // Async, generators, workers.
    "async_basic",
    "async_lambdas",
    "async_generators",
    "async_streams",
    "tasks",
    "task_combinators",
    "channels",
    "stress_async",
    "parallel_fan_out",
    "worker_captures",
    "generators",
    // Collections and strings.
    "rust_collections",
    "rust_std_collections",
    "collection_reference_semantics",
    "collection_pass_by_ref",
    "iterable_combinators",
    "iterator_adaptors",
    "deque",
    "map_entries_owned",
    "dynamic_arrays",
    "multidim_arrays",
    "word_count",
    "everyday_string_and_collection_code",
    "nullable_in_collections",
    "value_reuse_after_pass",
    // Foreign APIs through their bindings.
    "closures_into_rust",
    "comparators_into_rust",
    "foreign_iterators",
    "foreign_collection_slots",
    "foreign_value_fields",
    "generic_over_foreign",
    "lent_foreign_closure",
    "foreign_split_collect",
    "import_wildcard_rust_std",
    // Control flow, switches, exceptions.
    "switch_assign_arm",
    "switch_guards",
    "ternary_narrowed_interpolation",
    "exception_cause",
    "try_finally_semantics",
    "result_propagation",
    "crm_demo",
];

/// Examples `run.rs` does not pin (its `EXCLUDED`), left out of the whole
/// corpus run here for the same reasons.
const NOT_PINNED: &[&str] = &[
    "stress_workers",
    "stress_jux_std",
    "stress_jux_std_parallel",
    "ffi_strings",
    "ffi_struct",
    "io_and_time",
    "jni_java_vm",
    "ffi_unwind_barrier",
    "runtime_stack_overflow",
    "runtime_stack_overflow_crate_thread",
];

fn worker_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get().clamp(2, 8)).unwrap_or(4)
}

/// What `run.rs` records for a run, normalized the same way.
fn record(output: &Output, root: &Path) -> String {
    let root_slash = root.display().to_string().replace('\\', "/");
    let root_back = root.display().to_string().replace('/', "\\");
    let mut out = String::new();
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    for raw in [stdout, stderr] {
        for line in raw.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with("jux: ") {
                continue;
            }
            let cleaned = if line.contains(&root_slash) || line.contains(&root_back) {
                line.replace(&format!("{root_slash}/"), "")
                    .replace(&format!("{root_back}\\"), "")
                    .replace(&root_slash, "")
                    .replace(&root_back, "")
                    .replace('\\', "/")
                    .trim_start_matches('/')
                    .to_string()
            } else {
                line.to_string()
            };
            out.push_str(&cleaned);
            out.push('\n');
        }
    }
    if !output.status.success() {
        out.push_str(&format!("[exit: {}]\n", output.status.code().unwrap_or(-1)));
    }
    out
}

fn run_forced_safe(root: &Path, name: &str) -> String {
    let source = root.join("examples").join(format!("{name}.jux"));
    let emit_dir = root.join("target").join(format!("it-safe-{name}"));
    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .current_dir(root)
        .env("JUX_FORCE_SAFE", "1")
        .env("JUX_SELFCHECK", "1")
        .env_remove("JUX_TEST_BREAK_FAST")
        .output()
        .unwrap_or_else(|e| panic!("spawning jux for {name}: {e}"));
    record(&output, root)
}

/// Every example of the slice prints, fully safe, exactly what the fast
/// corpus pins for it.
#[test]
fn a_corpus_slice_prints_the_same_under_forced_safe_mode() {
    let root = common::workspace_root();
    let chosen = std::env::var("JUX_SAFE_CORPUS").unwrap_or_default();
    let names: Vec<String> = if !chosen.is_empty() && chosen != "all" {
        // A comma-separated list, for looking at a few by hand.
        chosen.split(',').map(|s| s.trim().to_string()).collect()
    } else if chosen == "all" {
        let mut all: Vec<String> = std::fs::read_dir(root.join("examples"))
            .expect("reading examples/")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jux"))
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
            .filter(|n| !NOT_PINNED.contains(&n.as_str()))
            .collect();
        all.sort();
        all
    } else {
        SLICE.iter().map(|s| s.to_string()).collect()
    };
    assert!(names.len() >= 60 || !chosen.is_empty(), "the slice must cover at least 60 examples");
    for name in &names {
        assert!(root.join("examples").join(format!("{name}.jux")).is_file(), "`{name}` is not an example");
    }
    let next = AtomicUsize::new(0);
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..worker_count() {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(name) = names.get(i) else { break };
                let got = run_forced_safe(&root, name);
                let path = root.join("tests").join("expected").join("examples").join(format!("{name}.expected"));
                let want = std::fs::read_to_string(&path).map(|w| w.replace("\r\n", "\n")).unwrap_or_default();
                if got != want {
                    failures.lock().unwrap().push(format!(
                        "{name}: differs under JUX_FORCE_SAFE=1.\n--- expected ---\n{want}--- got ---\n{got}"
                    ));
                }
            });
        }
    });
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    assert!(
        failures.is_empty(),
        "{} of {} example(s) differ at the safe lowering level:\n\n{}",
        failures.len(),
        names.len(),
        failures.join("\n\n")
    );
}

/// A small program whose `Counter.bump` the test hook breaks.
const PROGRAM: &str = "\
class Counter {
    int n;
    Counter() { this.n = 0; }
    void bump(int by) {
        n += by;
    }
    int get() { return n; }
}

void main() {
    var c = new Counter();
    c.bump(2);
    c.bump(3);
    print($\"count = ${c.get()}\");
}
";

/// A fresh directory under `target/` with the program in it.
fn scratch(tag: &str) -> (PathBuf, PathBuf) {
    let dir = common::workspace_root().join("target").join(format!("it-heal-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("heal.jux");
    std::fs::write(&source, PROGRAM).unwrap();
    (dir, source)
}

fn jux_run(dir: &Path, source: &Path, envs: &[(&str, &str)], verbose: bool) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jux"));
    cmd.arg("run");
    if verbose {
        cmd.arg("--verbose");
    }
    cmd.arg("--emit-dir").arg(dir.join("out")).arg(source);
    cmd.env_remove("JUX_SELFCHECK").env_remove("JUX_FORCE_SAFE").env_remove("JUX_TEST_BREAK_FAST");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("spawning jux")
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n"),
        String::from_utf8_lossy(&o.stderr).replace("\r\n", "\n"),
    )
}

/// The fast lowering of `Counter.bump` does not compile; the build heals by
/// lowering it safe, silently, and the program prints the right answer. The
/// next build reads the cache and goes straight to the safe level.
#[test]
fn a_broken_function_heals_by_retry_and_the_cache_remembers_it() {
    let (dir, source) = scratch("retry");
    let broken = [("JUX_TEST_BREAK_FAST", "Counter.bump")];

    let quiet = jux_run(&dir, &source, &broken, false);
    let (out, err) = text(&quiet);
    assert!(quiet.status.success(), "the heal failed:\n{out}{err}");
    assert!(out.contains("count = 5"), "{out}");
    assert!(!err.contains("compatibility mode"), "a heal is silent without --verbose:\n{err}");
    let cache = std::fs::read_to_string(dir.join("out").join(".jux-safe-fns")).expect("the heal is cached");
    assert!(cache.lines().any(|l| l == "Counter.bump"), "{cache}");

    // With the cache, the first build is already safe: no retry, and the
    // note still says what was compiled in compatibility mode.
    let again = jux_run(&dir, &source, &broken, true);
    let (out, err) = text(&again);
    assert!(again.status.success(), "{out}{err}");
    assert!(out.contains("count = 5"), "{out}");
    assert!(err.contains("note: 1 function compiled in compatibility mode"), "{err}");
    assert!(!err.contains("retrying"), "the cache should skip the retry:\n{err}");

    // From a clean build directory, --verbose shows the retry.
    let _ = std::fs::remove_file(dir.join("out").join(".jux-safe-fns"));
    let fresh = jux_run(&dir, &source, &broken, true);
    let (_, err) = text(&fresh);
    assert!(fresh.status.success(), "{err}");
    assert!(err.contains("retrying `Counter.bump` in compatibility mode"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Under `JUX_SELFCHECK=1` (the compiler's own test runs) the same heal is an
/// internal compiler error naming the function and rustc's original error.
#[test]
fn under_the_selfcheck_a_heal_fails_loudly() {
    let (dir, source) = scratch("selfcheck");
    let run = jux_run(&dir, &source, &[("JUX_TEST_BREAK_FAST", "Counter.bump"), ("JUX_SELFCHECK", "1")], false);
    let (out, err) = text(&run);
    assert_eq!(run.status.code(), Some(101), "{out}{err}");
    assert!(err.contains("[E0900]"), "{err}");
    assert!(err.contains("`Counter.bump` compiled only in compatibility mode"), "{err}");
    assert!(err.contains("rustc reported error[E0502]"), "{err}");
    assert!(err.contains("heal.jux:5:9"), "at the function's first statement:\n{err}");
    assert!(!out.contains("count = 5"), "the program must not run:\n{out}");
    assert!(!dir.join("out").join(".jux-safe-fns").exists(), "the self-check writes no cache");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A program with nothing broken builds fast and leaves no cache.
#[test]
fn a_sound_program_stays_fast() {
    let (dir, source) = scratch("fast");
    let run = jux_run(&dir, &source, &[("JUX_SELFCHECK", "1")], true);
    let (out, err) = text(&run);
    assert!(run.status.success(), "{out}{err}");
    assert!(out.contains("count = 5"), "{out}");
    assert!(!err.contains("compatibility mode"), "{err}");
    assert!(!dir.join("out").join(".jux-safe-fns").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
