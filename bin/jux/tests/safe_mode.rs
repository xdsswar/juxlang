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
//! - the regression harness (sweep C) takes the backend bugs gaps 30-40b
//!   fixed, re-introduces each in one function of one project
//!   (`JUX_TEST_BREAK_FAST=<kind>:<function>`, a break kind per bug), and
//!   checks that rustc refuses the fast lowering with that bug's error family
//!   and that the safe level rescues it: the build succeeds, only that
//!   function is compiled in compatibility mode, and the program prints what
//!   it prints unbroken. Every kind but `borrow` corrupts the judgement at
//!   both levels, so only a safe switch that replaces the judgement heals it.

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
    // Sweep C: the programs behind the historical bugs the regression harness
    // below re-introduces, and the forms added with it.
    "result_from",
    "task_await_method",
    "no_entry_point",
    "release_blockers",
    "intersection_bounds",
    "dependent_bounds",
    "generic_supertypes",
    "bounded_params_in_hierarchy",
    "polymorphic_recursion_ordered_keys",
    "polymorphic_recursion_fbounded",
    "collection_alias_channels",
    "var_lambda_types",
    "private_overloads_dispatch",
    "overload_by_type",
    "out_params",
    "numeric_mixed_ops",
    "generic_declarations_print",
    "foreign_errors_as_jux_exceptions",
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

// ---------------------------------------------------------------------------
// The regression harness (sweep C)
// ---------------------------------------------------------------------------

/// One historical backend bug, re-introduced in one function by a break kind
/// (`juxc_backend_rust::BreakKind`).
struct Regression {
    /// `JUX_TEST_BREAK_FAST`'s kind.
    kind: &'static str,
    /// The function it breaks, as the retry loop names it.
    key: &'static str,
    /// The rustc error family the bug produced.
    rustc: &'static str,
    /// Where the bug was fixed.
    history: &'static str,
}

/// Every bug the harness puts back, one per break kind. Each program shape is
/// the reproducer the fix recorded (GAPS.md, ERRATA), reduced to one function
/// of [`REGRESSION_MAIN`].
const REGRESSIONS: &[Regression] = &[
    Regression { kind: "borrow", key: "aliasPush", rustc: "E0502", history: "gap 30 (L12, L14): a borrow alive across an argument" },
    Regression { kind: "move", key: "aliasPush", rustc: "E0382", history: "gap 40 (E144): `w = v` moved the handle `v.len()` read" },
    Regression { kind: "clone", key: "Cell.get", rustc: "E0599", history: "gap 2 (E118, E120): a member copying `T` without `Clone`" },
    Regression { kind: "keybound", key: "put", rustc: "E0277", history: "gap 40b (E145): a generic key with no `Ord`" },
    Regression { kind: "arms", key: "label", rustc: "E0308", history: "gap 35 (E131): `\"ab \" + e` against `\"c\"`" },
    Regression { kind: "numeric", key: "total", rustc: "E0277", history: "gap 39 (E135): `isize + f64` through an intersection bound" },
    Regression { kind: "path", key: "Basket.origin", rustc: "E0433", history: "gap 31 L3, gap 35 (E131): a type named without `crate::`, in a signature too" },
    Regression { kind: "infer", key: "emptyCount", rustc: "E0283", history: "gap 36 L30, gap 40b: a type argument nothing infers" },
    Regression { kind: "overload", key: "sumTwo", rustc: "E0061", history: "gap 39f (E140): a stale overload pick" },
    Regression { kind: "mutability", key: "outLen", rustc: "E0596", history: "gap 30 L8/L9 (E127): a `&mut` lend of a non-`mut` binding" },
];

/// The project the harness breaks: one function per historical bug.
const REGRESSION_MAIN: &str = r#"import shop.Basket;
import rust.std.*;

// move (gap 40): a handle assigned to another local, then read again.
int aliasPush() {
    var v = new Vec<int>();
    v.push(1);
    var w = new Vec<int>();
    w = v;
    w.push(7);
    return (int) v.len();
}

// clone (gap 2, E118/E120): a member of a relaxed generic class that copies
// its `T`.
class Cell<T> {
    T value;
    Cell(T v) { value = v; }
    T get() { return value; }
}

// keybound (gap 40b): a generic function's own parameter as an ordered key.
<T> int put(BTreeMap<T, int> m, T k) {
    m.insert(k, 1);
    return (int) m.len();
}

// arms (gap 35): a `String` switch with a computed arm and a literal arm.
void label(int e) {
    print(switch (e) {
        case 1 -> "ab " + e;
        default -> "c";
    });
}

// numeric (gap 39): members reached through an intersection bound.
interface Aged { int age(); }
interface Scored { double score(); }
class Pupil implements Aged, Scored {
    public int age() { return 12; }
    public double score() { return 0.5; }
}
<T extends Aged & Scored> double total(T t) {
    return t.age() + t.score();
}

// infer (gap 36 L30, 40b): a type argument nothing in the call can infer.
<T> Vec<T> empty() {
    return new Vec<T>();
}
int emptyCount() {
    var e = empty<int>();
    return (int) e.len();
}

// overload (gap 39f): an overloaded method called with two arguments.
class Adder {
    int add(int a) { return a; }
    int add(int a, int b) { return a + b; }
}
int sumTwo() {
    var a = new Adder();
    return a.add(2, 3);
}

// mutability (gap 30 L8/L9): a local lent mutably, through `out`.
bool measure(String s, out int result) {
    result = s.length();
    return true;
}
int outLen() {
    int n = 0;
    measure("abcd", out n);
    return n;
}

void main() {
    print(aliasPush());
    print(Basket.corner());
    var c = new Cell<String>("kept");
    print(c.get());
    var m = new BTreeMap<String, int>();
    print(put(m, "a"));
    label(1);
    label(2);
    print(total(new Pupil()));
    print(emptyCount());
    print(sumTwo());
    print(outLen());
}
"#;

/// A packaged class naming a type of another package (the `path` case).
const REGRESSION_BASKET: &str = "package shop;

import geo.Point;

public class Basket {
    public static int corner() {
        Point p = origin(3, 4);
        return p.x + p.y;
    }

    // A signature naming another package's type: the header emitters'
    // spelling of it is the type-path switch's too (sweep C2).
    static Point origin(int x, int y) {
        return new Point(x, y);
    }
}
";

const REGRESSION_POINT: &str = "package geo;

public record Point(int x, int y) {}
";

/// What the project prints, broken or not.
const REGRESSION_OUTPUT: &str = "2\n7\nkept\n1\nab 1\nc\n12.5\n0\n5\n4\n";

fn regression_project() -> PathBuf {
    let dir = common::workspace_root().join("target").join("it-heal-regressions");
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, text) in [
        ("jux.toml", "[package]\nname = \"probe.regressions\"\nversion = \"0.1.0\"\nedition = \"2026\"\n"),
        ("src/main.jux", REGRESSION_MAIN),
        ("src/shop/Basket.jux", REGRESSION_BASKET),
        ("src/geo/Point.jux", REGRESSION_POINT),
    ] {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn jux_run_project(dir: &Path, envs: &[(&str, &str)]) -> Output {
    // A heal is cached in the build directory; every run here starts cold.
    let _ = std::fs::remove_file(dir.join("target").join(".rust-build").join("bin-regressions").join(".jux-safe-fns"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jux"));
    cmd.arg("run").arg("--verbose").current_dir(dir);
    cmd.env_remove("JUX_SELFCHECK").env_remove("JUX_FORCE_SAFE").env_remove("JUX_TEST_BREAK_FAST");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("spawning jux")
}

/// Every historical bug, put back into its function, is caught by the net:
/// rustc refuses the fast lowering with the bug's own error family, the retry
/// loop traces it to that one function, and the safe level compiles it to a
/// program that prints what the unbroken one prints. Under `JUX_SELFCHECK=1`
/// the same heal is the `E0900` naming the function and the rustc code, which
/// the driver reports only after the safe lowering built.
#[test]
fn the_safe_level_rescues_every_historical_backend_bug() {
    let dir = regression_project();

    // Unbroken, the program builds fast.
    let sound = jux_run_project(&dir, &[("JUX_SELFCHECK", "1")]);
    let (out, err) = text(&sound);
    assert!(sound.status.success(), "the unbroken program must build fast:\n{out}{err}");
    assert_eq!(out, REGRESSION_OUTPUT, "{err}");

    let mut failures: Vec<String> = Vec::new();
    let mut table = String::from("| kind | function | rustc | fixed in | rescued |\n|---|---|---|---|---|\n");
    for r in REGRESSIONS {
        let spec = format!("{}:{}", r.kind, r.key);
        let mut rescued = true;

        // The fast lowering with the bug is refused, with the bug's family,
        // and traced to the function.
        let checked = jux_run_project(&dir, &[("JUX_TEST_BREAK_FAST", &spec), ("JUX_SELFCHECK", "1")]);
        let (out, err) = text(&checked);
        let named = format!("`{}` compiled only in compatibility mode", r.key);
        let code = format!("rustc reported error[{}]", r.rustc);
        if checked.status.code() != Some(101) || !err.contains(&named) || !err.contains(&code) {
            rescued = false;
            failures.push(format!("{spec}: under the self-check expected `{named}` and `{code}`:\n{out}{err}"));
        }

        // Without the self-check the build heals, lowering that function
        // alone at the safe level, and the program is right.
        let healed = jux_run_project(&dir, &[("JUX_TEST_BREAK_FAST", &spec)]);
        let (out, err) = text(&healed);
        let retry = format!("retrying `{}` in compatibility mode", r.key);
        if !healed.status.success()
            || out != REGRESSION_OUTPUT
            || !err.contains(&retry)
            || !err.contains("note: 1 function compiled in compatibility mode")
        {
            rescued = false;
            failures.push(format!("{spec}: the heal did not rescue it:\n{out}{err}"));
        }
        table.push_str(&format!(
            "| {} | `{}` | {} | {} | {} |\n",
            r.kind,
            r.key,
            r.rustc,
            r.history,
            if rescued { "yes" } else { "NO" }
        ));
    }
    eprintln!("{table}");
    let families: std::collections::BTreeSet<&str> = REGRESSIONS.iter().map(|r| r.rustc).collect();
    assert!(families.len() >= 8, "the harness must cover at least 8 rustc error families, has {families:?}");
    assert!(failures.is_empty(), "{} regression(s) not rescued:\n\n{}", failures.len(), failures.join("\n\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The hook's spelling: `<kind>:<key>`, or a bare key for the borrow break a
/// hook without a kind always meant.
#[test]
fn a_break_names_its_kind_or_defaults_to_borrow() {
    use juxc_driver::BreakKind;
    assert_eq!(BreakKind::parse("Counter.bump"), (BreakKind::Borrow, "Counter.bump"));
    assert_eq!(BreakKind::parse("clone:Cell.get"), (BreakKind::Clone, "Cell.get"));
    assert_eq!(BreakKind::parse("nosuch:f"), (BreakKind::Borrow, "nosuch:f"));
    for k in BreakKind::ALL {
        assert_eq!(BreakKind::parse(&format!("{}:x", k.name())), (k, "x"));
    }
    assert!(REGRESSIONS.iter().all(|r| BreakKind::ALL.iter().any(|k| k.name() == r.kind)));
    assert_eq!(REGRESSIONS.len(), BreakKind::ALL.len(), "one historical bug per break kind");
}
