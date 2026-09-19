//! `tests/lessons`: the Jux teaching lessons, built and run.
//!
//! Each lesson is a small `jux.toml` project with an `expected.txt` beside it.
//! The folders are discovered from the directory, so adding a lesson is adding
//! a folder: there is no list in this file to keep in step.
//!
//! Everything runs from ONE test, which walks every lesson and reports every
//! failure in a single message, so a run shows the whole picture at once
//! instead of stopping at the first broken lesson. The lessons are built on a
//! few worker threads, because each one is a separate cargo build and most of
//! the cost is waiting on them.
//!
//! A lesson that fails because of a compiler bug is listed in
//! `tests/lessons/known-failures.txt` with the reason, which keeps the gate
//! green while the bug is open. The list is checked in both directions: a
//! listed lesson that starts passing fails the test until its line is removed,
//! so a fixed bug cannot hide behind a stale entry.
//!
//! Set `LESSON=Name` (or `Name,Other`) to run only those lessons.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// Where the lessons live, relative to the repository root.
const LESSONS: &str = "tests/lessons";

/// The file naming lessons that are expected to fail, one per line.
const KNOWN_FAILURES: &str = "known-failures.txt";

/// What running one lesson produced.
enum Outcome {
    /// The trimmed stdout matched `expected.txt` exactly.
    Pass,
    /// Anything else, with a message worth reading.
    Fail(String),
}

/// Every lesson folder: a directory holding a `jux.toml`, sorted by name so
/// the report reads in a stable order.
fn discover(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .unwrap_or_else(|e| panic!("reading {}: {e}", root.display()))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("jux.toml").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Parses `known-failures.txt`: `Name: reason` per line, with blank lines and
/// `#` comments ignored. A line without a reason is rejected, because an entry
/// nobody can explain is an entry nobody will remove.
fn known_failures(root: &Path) -> BTreeMap<String, String> {
    let path = root.join(KNOWN_FAILURES);
    let Ok(text) = fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let mut known = BTreeMap::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, reason) = line.split_once(':').unwrap_or_else(|| {
            panic!(
                "{}:{}: expected `Name: reason`, got `{line}`",
                path.display(),
                number + 1
            )
        });
        let reason = reason.trim();
        assert!(
            !reason.is_empty(),
            "{}:{}: `{}` has no reason",
            path.display(),
            number + 1,
            name.trim()
        );
        known.insert(name.trim().to_string(), reason.to_string());
    }
    known
}

/// Normalizes program output for comparison: line endings unified, trailing
/// whitespace dropped from every line, the tool's own `jux:` status lines
/// removed, and leading and trailing blank lines trimmed.
fn normalize(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.starts_with("jux: "))
        .collect();
    lines.join("\n").trim_matches('\n').to_string()
}

/// Copies a lesson's `jux.toml` and `src/` tree into `dest`, replacing what a
/// previous run left there. The build then happens under the workspace
/// `target/`, so running the suite never writes into the lesson folders.
fn stage(lesson: &Path, dest: &Path) {
    let _ = fs::remove_dir_all(dest.join("src"));
    fs::create_dir_all(dest).unwrap_or_else(|e| panic!("creating {}: {e}", dest.display()));
    fs::copy(lesson.join("jux.toml"), dest.join("jux.toml"))
        .unwrap_or_else(|e| panic!("copying {}: {e}", lesson.display()));
    copy_tree(&lesson.join("src"), &dest.join("src"));
}

/// Recursive directory copy; the lesson trees are a handful of small files.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap_or_else(|e| panic!("creating {}: {e}", to.display()));
    for entry in fs::read_dir(from).unwrap_or_else(|e| panic!("reading {}: {e}", from.display())) {
        let entry = entry.expect("directory entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target)
                .unwrap_or_else(|e| panic!("copying {}: {e}", entry.path().display()));
        }
    }
}

/// Builds and runs one lesson, feeding it `stdin.txt` when the lesson has one,
/// and compares its output with `expected.txt`.
fn run_lesson(workspace: &Path, name: &str) -> Outcome {
    let lesson = workspace.join(LESSONS).join(name);
    let staged = workspace.join("target").join("lessons").join(name);
    stage(&lesson, &staged);

    let expected = match fs::read_to_string(lesson.join("expected.txt")) {
        Ok(text) => normalize(&text),
        Err(e) => return Outcome::Fail(format!("no expected.txt: {e}")),
    };

    let mut child = Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("run")
        .arg("--manifest-path")
        .arg(&staged)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("spawning jux run for {name}: {e}"));

    // An absent `stdin.txt` still closes the pipe, so a lesson that reads
    // input sees end-of-file rather than waiting forever.
    let input = fs::read(lesson.join("stdin.txt")).unwrap_or_default();
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(&input);
    }

    let output = child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("waiting on {name}: {e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Outcome::Fail(format!(
            "exited with {:?}\n{}\n{}",
            output.status.code(),
            stdout.trim_end(),
            stderr.trim_end()
        ));
    }

    let got = normalize(&stdout);
    if got == expected {
        Outcome::Pass
    } else {
        Outcome::Fail(format!(
            "output differs from expected.txt\n--- expected\n{expected}\n--- got\n{got}"
        ))
    }
}

/// The lessons named in `LESSON`, or every lesson when it is unset.
fn selected(all: Vec<String>) -> Vec<String> {
    match std::env::var("LESSON") {
        Ok(filter) if !filter.trim().is_empty() => {
            let wanted: Vec<&str> = filter.split(',').map(str::trim).collect();
            for name in &wanted {
                assert!(
                    all.iter().any(|lesson| lesson == name),
                    "LESSON names `{name}`, which is not a lesson folder"
                );
            }
            all.into_iter()
                .filter(|name| wanted.contains(&name.as_str()))
                .collect()
        }
        _ => all,
    }
}

#[test]
fn lessons() {
    let workspace = common::workspace_root();
    let root = workspace.join(LESSONS);
    let all = discover(&root);
    let known = known_failures(&root);

    // A stale entry names a lesson that no longer exists. Caught before any
    // build runs, since it needs none.
    let mut problems: Vec<String> = known
        .keys()
        .filter(|name| !all.contains(name))
        .map(|name| format!("{name}: listed in {KNOWN_FAILURES} but there is no such lesson"))
        .collect();

    let lessons = selected(all);
    assert!(
        !lessons.is_empty(),
        "no lessons found under {}",
        root.display()
    );

    // A shared queue of names, drained by a few workers. Each lesson has its
    // own staging directory, so the builds never share files.
    let queue = Mutex::new(lessons.clone());
    let results = Mutex::new(BTreeMap::new());
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 6))
        .unwrap_or(2);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let Some(name) = queue.lock().unwrap().pop() else {
                    break;
                };
                let outcome = run_lesson(&workspace, &name);
                results.lock().unwrap().insert(name, outcome);
            });
        }
    });

    let results = results.into_inner().unwrap();
    let mut passed = 0;
    let mut expected_failures = 0;
    for (name, outcome) in &results {
        match (outcome, known.get(name)) {
            (Outcome::Pass, None) => passed += 1,
            (Outcome::Pass, Some(_)) => problems.push(format!(
                "{name}: passes now; remove it from known-failures.txt"
            )),
            (Outcome::Fail(_), Some(_)) => expected_failures += 1,
            (Outcome::Fail(message), None) => problems.push(format!("{name}: {message}")),
        }
    }

    eprintln!(
        "jux lessons: {passed} passed, {expected_failures} known failures, {} problems, of {}",
        problems.len(),
        results.len()
    );
    assert!(
        problems.is_empty(),
        "{} lesson problem(s):\n\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}

/// The normalizer is what makes the comparison exact without being brittle
/// about line endings, so it gets checked on its own.
#[test]
fn normalize_ignores_line_endings_and_status_lines() {
    let raw = "jux: built x.exe\r\nfirst  \r\n\r\nsecond\r\n\r\n";
    assert_eq!(normalize(raw), "first\n\nsecond");
    assert_eq!(normalize("\n\nonly\n"), "only");
}
