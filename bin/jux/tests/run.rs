//! Every example's exact output is pinned.
//!
//! `bin/juxc/tests/ui.rs` does this for what the compiler SAYS when it refuses
//! a program. This does it for what a program PRINTS when the compiler accepts
//! one, which until now nothing did with any strength.
//!
//! ## Why exactness, specifically
//!
//! The corpus already ran every example. What it did not do was look hard at
//! the output. `bin/jux/tests/abs.rs` was representative: its whole assertion
//! was that a `7` appeared before a `5` somewhere in stdout, which passes just
//! as happily if the program prints `-7.0` and `5000`. Only 6 of 182 test files
//! used the shared `expect_output` helper; the rest were hand-rolled and loose.
//!
//! That matters because of the bug class this compiler actually produces. Of
//! the eight bugs fixed in `6f10c2d`, four were silent: the program compiled,
//! ran, and printed something else. A whole `double` printed as `1` while the
//! same value inside a collection printed `1.0`. `-9223372036854775808` printed
//! as `0`. An `abs.rs`-shaped assertion cannot see any of them, and a gate that
//! cannot see the bugs you have is not a gate.
//!
//! ## The shape
//!
//! Copied from `ui.rs`, including the discipline in its doc comment, which
//! applies here word for word: blessing without reading is worse than no test,
//! because it looks like a decision. A blessed `.expected` records what the
//! compiler does today, bugs included, unless somebody reads it.
//!
//! ```text
//! cargo test -p jux --test run              # check
//! JUX_BLESS=1 cargo test -p jux --test run  # re-bless, then READ the diff
//! ```
//!
//! Expectations live in `tests/expected/examples/<name>.expected` rather than
//! beside the sources, because `examples/` is a showcase people read and browse
//! from the README, not a fixture directory.
//!
//! ## Parallelism is the point, not a detail
//!
//! Cargo runs test BINARIES sequentially and the tests inside one in parallel.
//! Measured on this suite, an example costs 2.49s when it has a test file to
//! itself and 0.42s inside a cluster runner, so the 158 single-test files are
//! worth about 327s of a 529s suite. This harness is one binary, so it fans the
//! corpus across a pool of worker threads itself. See `tests/common/mod.rs` for
//! the measurements.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Examples this harness does not run, each with the reason.
///
/// Keep it short and keep the reasons here. An entry with no reason is
/// indistinguishable from an oversight, which is the thing a gate exists to
/// prevent. This mirrors `example_coverage.rs`'s `EXCLUDED` on purpose.
const EXCLUDED: &[(&str, &str)] = &[
    // Output is not the same twice, so there is nothing exact to pin. These
    // stay on `expect_contains` in their own tests, which is the right tool
    // for output carrying timings or thread interleaving.
    ("stress_workers", "thread interleaving, output varies per run"),
    ("stress_jux_std", "timings in the output"),
    ("stress_jux_std_parallel", "thread interleaving and timings"),
    ("ffi_strings", "prints a heap address"),
    ("ffi_struct", "prints a heap address"),
    // Needs `target/it-io-time-data/` to exist before it runs, because
    // `File.writeText` does not create parent directories. That is setup, not
    // output, so it stays with `tests/io_and_time.rs`, which does the
    // `create_dir_all` first. Blessed here it recorded a Rust panic instead.
    ("io_and_time", "needs a directory created first; covered by tests/io_and_time.rs"),
];

/// How many examples to compile at once.
///
/// Each case spawns `jux run`, which spawns `cargo`, so this is a pool of
/// process trees rather than of threads doing arithmetic. Cargo does its own
/// internal parallelism, so oversubscribing here makes things slower, not
/// faster.
fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 8))
        .unwrap_or(4)
}

/// Where an example's pinned output lives.
fn expected_path(root: &Path, name: &str) -> PathBuf {
    root.join("tests")
        .join("expected")
        .join("examples")
        .join(format!("{name}.expected"))
}

/// Run one example and return everything a user would observe, normalized.
///
/// Deliberately not `Result`. An example that does not compile, or that ends in
/// an uncaught exception, is not a harness failure: it is behaviour, and it is
/// behaviour worth pinning. `examples/nullable_arg_mismatch.jux` exists to
/// produce E0410 and `examples/arith_uncaught.jux` exists to die on a divide by
/// zero. Treating those as errors is how a first draft of this harness lost
/// four examples without saying so.
///
/// So the record is stdout, then any stderr worth keeping, then the exit code
/// when it is not zero.
fn run_example(root: &Path, name: &str) -> String {
    let source = root.join("examples").join(format!("{name}.jux"));
    // Reuse the per-example build directory convention. `tests/common/mod.rs`
    // records why these stay per-example rather than being pooled into one.
    let emit_dir = root.join("target").join(format!("it-{name}"));

    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        // From the repository root, because that is where a person stands when
        // they run `jux run`, and examples resolve relative paths against it.
        // Cargo starts a test in its own package directory (`bin/jux`), so
        // without this an example doing file I/O writes somewhere else entirely
        // and dies. `examples/io_and_time.jux` did exactly that.
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("spawning jux for {name}: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");

    let mut record = normalize(&stdout, root);
    record.push_str(&normalize(&stderr, root));
    if !output.status.success() {
        // Pinned because it is the difference between "printed nothing" and
        // "died". `jux run` forwards the program's own code, so an uncaught Jux
        // exception shows up here as 101: it lowers to a Rust panic.
        record.push_str(&format!("[exit: {}]\n", output.status.code().unwrap_or(-1)));
    }
    record
}

/// Trim what a terminal would show to what is worth pinning.
///
/// Out go blank lines, trailing whitespace and CRLF, plus two things that are
/// about this machine rather than about the program: `jux: ` build-progress
/// lines, and the absolute path to the checkout. What is left is what the
/// program said.
fn normalize(raw: &str, root: &Path) -> String {
    let root_slash = root.display().to_string().replace('\\', "/");
    let root_back = root.display().to_string().replace('/', "\\");
    let mut out = String::new();
    for line in raw.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with("jux: ") {
            continue;
        }
        // Separator normalization is confined to lines that actually named the
        // checkout. A program is free to print a backslash of its own, and
        // rewriting those would corrupt the very output being pinned.
        let cleaned = if line.contains(&root_slash) || line.contains(&root_back) {
            line.replace(&root_slash, "")
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
    out
}

/// Output shapes that have been a silent bug before, flagged when blessing so
/// the reading pass has somewhere to start.
///
/// None of these is wrong on its own. `Some(4)` is right if the program prints
/// an optional on purpose. They are listed because each one WAS a bug once, and
/// a person blessing 270 files needs the twenty worth staring at.
const SUSPICIOUS: &[(&str, &str)] = &[
    ("RefCell {", "a handle's Debug leaking instead of what it holds"),
    ("error[E", "a rustc error leaked through, which is a juxc bug"),
    // A Rust panic is never something a Jux program asks for. An uncaught Jux
    // exception is, and reads as `Exception in thread "main" jux.std...`, so
    // the two are told apart by shape. This entry exists because the first
    // blessing pass recorded `io_and_time` panicking on a missing directory and
    // nothing said so.
    ("panicked at", "the emitted Rust panicked, which a Jux program cannot ask for"),
    ("Result::unwrap()", "an unwrap on an Err reached the user as a crash"),
    ("Some(", "an Option rendered instead of unwrapped (s?.length() printed Some(4))"),
    ("Ok(", "a Result rendered instead of unwrapped"),
    ("Rc(", "a handle's Debug leaking"),
    ("\\\"", "an escaped quote, seen when a String inside a generic printed with quotes"),
];

/// The whole corpus, minus the exclusions, sorted.
fn corpus(root: &Path) -> Vec<String> {
    let dir = root.join("examples");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("reading examples/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jux"))
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .filter(|n| !EXCLUDED.iter().any(|(x, _)| x == n))
        .collect();
    names.sort();
    names
}

/// Fan `f` across the corpus and collect whatever it reports.
fn each_example<F>(names: &[String], f: F) -> Vec<String>
where
    F: Fn(&str) -> Option<String> + Sync,
{
    let next = AtomicUsize::new(0);
    let reports: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..worker_count() {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(name) = names.get(i) else { break };
                if let Some(report) = f(name) {
                    reports.lock().expect("report lock").push(report);
                }
            });
        }
    });
    let mut out = reports.into_inner().expect("report lock");
    out.sort();
    out
}

/// Every example prints exactly what it printed last time.
#[test]
fn every_example_matches_its_expected_output() {
    let root = common::workspace_root();
    let names = corpus(&root);
    assert!(!names.is_empty(), "examples/ has no .jux files");

    let bless = std::env::var("JUX_BLESS").is_ok();
    if bless {
        let dir = root.join("tests").join("expected").join("examples");
        std::fs::create_dir_all(&dir).expect("creating tests/expected/examples");
    }

    let failures = each_example(&names, |name| {
        let got = run_example(&root, name);
        let path = expected_path(&root, name);

        if bless {
            std::fs::write(&path, &got).expect("writing .expected");
            // Blessing is where bugs get found, so say which files are worth
            // opening rather than leaving 270 of them to be skimmed.
            let hits: Vec<&str> = SUSPICIOUS
                .iter()
                .filter(|(needle, _)| got.contains(needle))
                .map(|(_, why)| *why)
                .collect();
            return hits
                .first()
                .map(|why| format!("{name}: worth reading, {why}"));
        }

        match std::fs::read_to_string(&path) {
            Ok(want) => {
                let want = want.replace("\r\n", "\n");
                (got != want).then(|| {
                    format!("{name}: output changed.\n--- expected ---\n{want}--- got ---\n{got}")
                })
            }
            Err(_) => Some(format!(
                "{name}: no .expected file. Run with JUX_BLESS=1, then READ what it \
                 generated before committing it:\n{got}"
            )),
        }
    });

    if bless {
        if !failures.is_empty() {
            eprintln!(
                "\nBlessed. {} file(s) contain output that has been a silent bug \
                 before, and are worth opening:\n  {}\n",
                failures.len(),
                failures.join("\n  ")
            );
        }
        return;
    }

    assert!(
        failures.is_empty(),
        "{} example(s) differ. If the new output is CORRECT, re-bless with \
         JUX_BLESS=1 and commit the diff, which is the record of what the \
         language now does.\n\n{}",
        failures.len(),
        failures.join("\n\n"),
    );
}

/// An `.expected` whose example was deleted silently stops testing anything.
///
/// Lifted from `ui.rs`'s `no_orphan_expected_files`, for the same reason: the
/// file keeps passing forever and nobody thinks to delete it.
#[test]
fn no_orphan_expected_files() {
    let root = common::workspace_root();
    let dir = root.join("tests").join("expected").join("examples");
    if !dir.is_dir() {
        return;
    }
    let mut orphans: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("reading tests/expected/examples").flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("expected") {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if !root.join("examples").join(format!("{name}.jux")).is_file() {
            orphans.push(name);
        }
    }
    assert!(
        orphans.is_empty(),
        "expectation file(s) with no example left: {}. Delete them.",
        orphans.join(", ")
    );
}

/// Every exclusion still refers to a real example.
///
/// A stale reason is the one thing nobody thinks to delete, and it reads like a
/// known limitation long after the limitation is gone.
#[test]
fn every_exclusion_still_refers_to_something() {
    let root = common::workspace_root();
    let mut stale: Vec<&str> = Vec::new();
    for (name, _) in EXCLUDED {
        if !root.join("examples").join(format!("{name}.jux")).is_file() {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "EXCLUDED names an example that no longer exists: {}",
        stale.join(", ")
    );
}

/// The compiler says the same thing, in the same order, every time.
///
/// Pinning exact output is what turned this up, and it is worth its own test
/// because the corpus only caught it by being run twice.
/// `examples/reentrancy_notnull_local.jux` earns two W0457 warnings, and they
/// used to come out in either order: the cycle scan walked a `HashMap`, whose
/// hasher is seeded once per process, so two runs of an unchanged program
/// disagreed about which warning came first.
///
/// That is worse than untidy. Diagnostics are the compiler's user interface,
/// any blessed expectation covering two of them flakes at random, and "run it
/// again" is the worst thing a compiler can imply.
///
/// Through `jux`, deliberately. The same case via `juxc --check` came out the
/// same way 20 times running even with the bug present, because the front end
/// reaches the scan with its allocator in the same state each time; through
/// `jux` it was an even coin flip. A regression test that exercises the lucky
/// path would pass forever and prove nothing.
#[test]
fn diagnostics_come_out_in_the_same_order_every_time() {
    let root = common::workspace_root();
    let case = root.join("examples").join("reentrancy_notnull_local.jux");
    assert!(case.is_file(), "the two-warning case is missing");

    let mut first: Option<String> = None;
    // One process per run: a hasher seeded per process is the thing to catch.
    for run in 1..=12 {
        let output = Command::new(env!("CARGO_BIN_EXE_jux"))
            .arg("check")
            .arg(&case)
            .output()
            .expect("spawn jux check");
        let got = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
        .replace("\r\n", "\n");
        match &first {
            None => first = Some(got),
            Some(want) => assert_eq!(
                want, &got,
                "run {run} printed its diagnostics in a different order. Something in \
                 the front end iterates a hash map where the order reaches the user.",
            ),
        }
    }
}

/// Diagnostics come out in the order a reader meets them.
///
/// `juxc` has sorted by file, then byte offset, then code since the phase-order
/// fix. `jux` never did: it printed whatever order resolve and tycheck happened
/// to produce, so the two tools disagreed about the same file, and `jux` is the
/// one people type. On `reentrancy_notnull_local.jux` that meant line 16 before
/// line 8.
///
/// Asserted as a PROPERTY of `jux` alone rather than as agreement between the
/// two binaries. A cross-binary comparison would have to find `juxc` beside
/// `jux` on disk and would quietly pass against a stale copy, which is the kind
/// of test that looks like cover and is not.
#[test]
fn diagnostics_are_printed_in_source_order() {
    let root = common::workspace_root();
    // Two warnings, deliberately not in the order the scan produces them.
    let case = root.join("examples").join("reentrancy_notnull_local.jux");
    assert!(case.is_file(), "the two-warning case is missing");

    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("check")
        .arg(&case)
        .current_dir(&root)
        .output()
        .expect("spawn jux check");
    let blob = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // `…/foo.jux:16:12: [W0457] warning: …` -> (16, 12)
    let positions: Vec<(u32, u32)> = blob
        .lines()
        .filter_map(|line| {
            let rest = line.split(".jux:").nth(1)?;
            let mut parts = rest.split(':');
            let line_no = parts.next()?.parse().ok()?;
            let column = parts.next()?.parse().ok()?;
            Some((line_no, column))
        })
        .collect();

    assert!(
        positions.len() >= 2,
        "expected at least two located diagnostics, got {positions:?} from:\n{blob}"
    );
    let mut sorted = positions.clone();
    sorted.sort_unstable();
    assert_eq!(
        positions, sorted,
        "diagnostics came out in {positions:?}, which is not source order. \
         `jux` and `juxc` must both print through \
         `juxc_driver::diagnostic_order::in_source_order`.",
    );
}
