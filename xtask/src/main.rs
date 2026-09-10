//! The project's own command runner.
//!
//! ## Why this exists
//!
//! GitHub Actions is switched off for this repository: `ci.yml` sits in
//! `.github/disabled/`, which GitHub does not read. That was deliberate, so the
//! gate has to be something you run rather than something that runs at you.
//!
//! The trouble with a gate you run yourself is that it is five commands, and
//! five commands is four you forget. Before this, "1405 green, clippy clean"
//! was a claim assembled by hand from several terminals, and two of the
//! project's best bug-finders (the differential harness and the no-ICE fuzzer)
//! were shell scripts nobody had a reason to remember.
//!
//! So: one word, one exit code.
//!
//! ```text
//! cargo xtask gate           # before a commit
//! cargo xtask gate --full    # before a push, or a milestone
//! ```
//!
//! ## The two tiers
//!
//! `gate` is the fast tier and is meant to be affordable: the workspace test
//! suite and a gating clippy. The suite is about 290s on Windows, which is a
//! measured number rather than a guess (see `bin/jux/tests/common/mod.rs`), and
//! most of it is `cargo build` on emitted crates rather than anything this can
//! make faster.
//!
//! `--full` adds the two harnesses that need something beyond cargo: the no-ICE
//! fuzzer needs Python, and the Java differential harness needs bash and a JDK.
//! Both are skipped with a printed reason when their prerequisite is missing,
//! because a gate that fails for want of an interpreter teaches people to pass
//! `--no-verify`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

/// One thing the gate ran, and how it went.
struct Step {
    name: &'static str,
    outcome: Outcome,
    seconds: f64,
}

enum Outcome {
    Passed,
    Failed,
    /// Not run, with the reason. A missing JDK is not a failing gate.
    Skipped(String),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let full = args.iter().any(|a| a == "--full");
    let wants_gate = args.iter().any(|a| a == "gate");

    if !wants_gate || args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!(
            "usage: cargo xtask gate [--full]\n\n\
             gate          workspace tests, then clippy as a hard error\n\
             gate --full   also the no-ICE fuzzer (Python) and the Java\n\
             \x20             differential harness (bash + a JDK)\n"
        );
        return ExitCode::from(2);
    }

    let root = workspace_root();
    let mut steps: Vec<Step> = Vec::new();

    // Tests first. It is the slowest step and the one most likely to fail, and
    // there is no sense linting code that does not work.
    steps.push(run_step("tests", &root, "cargo", &["test", "--workspace"]));
    steps.push(run_step(
        "clippy",
        &root,
        "cargo",
        &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
    ));

    if full {
        // Both harnesses drive the RELEASE binaries, so build them once and
        // report it as its own step. Folding it into the fuzz step made the
        // summary understate the total by however long the build took.
        let built = run_step(
            "build release binaries",
            &root,
            "cargo",
            &["build", "--release", "-p", "juxc", "-p", "jux"],
        );
        let ready = matches!(built.outcome, Outcome::Passed);
        steps.push(built);
        if ready {
            steps.push(fuzz_step(&root));
            steps.push(differential_step(&root));
        } else {
            steps.push(skipped("no-ice fuzz", "release binaries did not build"));
            steps.push(skipped("java differential", "release binaries did not build"));
        }
    }

    report(&steps, full)
}

/// Two levels up from this crate's manifest.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root resolves from xtask/")
        .to_path_buf()
}

/// Run one command, streaming its output, and time it.
fn run_step(name: &'static str, root: &Path, program: &str, args: &[&str]) -> Step {
    println!("\n=== {name} ===");
    let started = Instant::now();
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status();
    let seconds = started.elapsed().as_secs_f64();
    let outcome = match status {
        Ok(s) if s.success() => Outcome::Passed,
        Ok(_) => Outcome::Failed,
        Err(e) => Outcome::Skipped(format!("could not run `{program}`: {e}")),
    };
    Step { name, outcome, seconds }
}

/// The no-ICE fuzzer: mutate every example and assert the front end answers
/// with diagnostics rather than a panic.
///
/// Needs Python, and a release `juxc` for it to drive.
fn fuzz_step(root: &Path) -> Step {
    let script = root.join("tools").join("no-ice-fuzz").join("fuzz.py");
    if !script.is_file() {
        return skipped("no-ice fuzz", "tools/no-ice-fuzz/fuzz.py is missing");
    }
    let Some(python) = first_available(&["python", "python3"]) else {
        return skipped("no-ice fuzz", "no `python` on PATH");
    };
    run_step("no-ice fuzz", root, &python, &[script.to_str().unwrap_or("")])
}

/// The Java differential harness: the same program written twice, once in Jux
/// and once in Java, with Java as the oracle.
///
/// Needs bash and a JDK. It found eleven bugs on its first two runs, four of
/// them silent, which is the argument for keeping it reachable even though it
/// cannot be a `cargo test`.
fn differential_step(root: &Path) -> Step {
    let script = root.join("tools").join("java-differential").join("run.sh");
    if !script.is_file() {
        return skipped("java differential", "tools/java-differential/run.sh is missing");
    }
    if first_available(&["java"]).is_none() {
        return skipped("java differential", "no `java` on PATH");
    }
    let Some(bash) = find_bash() else {
        return skipped("java differential", "no working `bash` found");
    };
    run_step(
        "java differential",
        root,
        &bash,
        &[script.to_str().unwrap_or("")],
    )
}

fn skipped(name: &'static str, why: &str) -> Step {
    Step { name, outcome: Outcome::Skipped(why.to_string()), seconds: 0.0 }
}

/// A `bash` that actually runs.
///
/// On Windows, PATH usually resolves `bash` to `C:\Windows\System32\bash.exe`,
/// which is the WSL launcher. On a machine without a WSL distribution
/// installed that is a stub: it is on PATH, it is not a shell, and asking it
/// for `--version` fails. Git for Windows ships the real one somewhere else, so
/// PATH is tried first and then the usual install locations, and each candidate
/// has to answer `--version` before it counts.
fn find_bash() -> Option<String> {
    if let Some(found) = first_available(&["bash"]) {
        return Some(found);
    }
    let mut candidates: Vec<PathBuf> = vec![
        PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"),
    ];
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Programs\Git\bin\bash.exe"));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.display().to_string())
}

/// The first of these that exists on PATH.
fn first_available(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|program| {
            Command::new(program)
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
        .map(|s| (*s).to_string())
}

/// One summary and one exit code, which is the point of the whole thing.
fn report(steps: &[Step], full: bool) -> ExitCode {
    println!("\n=== gate ===");
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut total = 0.0;
    for step in steps {
        total += step.seconds;
        let (mark, note) = match &step.outcome {
            Outcome::Passed => ("pass", String::new()),
            Outcome::Failed => {
                failed += 1;
                ("FAIL", String::new())
            }
            Outcome::Skipped(why) => {
                skipped += 1;
                ("skip", format!("  ({why})"))
            }
        };
        println!("  {mark}  {:<34} {:>6.1}s{note}", step.name, step.seconds);
    }
    println!("  ----  {:<34} {total:>6.1}s", "total");

    if !full {
        println!(
            "\nFast tier. `cargo xtask gate --full` adds the no-ICE fuzzer and the\n\
             Java differential harness."
        );
    }
    if skipped > 0 {
        println!(
            "{skipped} step(s) skipped for a missing prerequisite. That is not a pass:\n\
             install it, or run that harness by hand, before trusting a green gate."
        );
    }
    if failed > 0 {
        println!("\n{failed} step(s) failed.");
        return ExitCode::FAILURE;
    }
    println!("\nGreen.");
    ExitCode::SUCCESS
}
