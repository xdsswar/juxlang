//! `examples/apps`: realistic multi-file Jux projects, built and run.
//!
//! The single-file examples test one feature each. These test the thing a user
//! actually does: several packages, a manifest, dependencies between members,
//! crates from crates.io, files and program arguments, all at once. Bugs that
//! only appear when features meet live here.
//!
//! Each app is a folder holding a `jux.toml` (a package or a workspace) and an
//! `expected.txt` with the exact output of `jux run`. Two optional files shape
//! the run:
//!
//! - `run-args.txt`: extra arguments for `jux run`, one per line, inserted
//!   right after `run` (so `-p`, `app`, `--`, `add`, `milk` selects a member
//!   and passes the program its arguments). Blank lines are ignored.
//! - `stdin.txt`: piped to the program. Without it the pipe is closed, so a
//!   program that reads input sees end-of-file rather than waiting forever.
//!
//! An app that has a `test/` directory anywhere in it is also run through
//! `jux test`, which must pass.
//!
//! Everything runs from one test that reports every failure together. Set
//! `APP=name` (or `name,other`) to run only those apps.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// Every app folder: a directory under `examples/apps` holding a `jux.toml`,
/// sorted by name so the report reads in a stable order.
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

/// Recursive copy of an app, leaving out build output and the files that only
/// the runner reads. The build then happens in the staged copy, so running
/// the suite never writes into `examples/`.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap_or_else(|e| panic!("creating {}: {e}", to.display()));
    for entry in fs::read_dir(from).unwrap_or_else(|e| panic!("reading {}: {e}", from.display())) {
        let entry = entry.expect("directory entry");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name == ".jux-stubs" {
            continue;
        }
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target)
                .unwrap_or_else(|e| panic!("copying {}: {e}", entry.path().display()));
        }
    }
}

/// Whether the app holds a `test/` directory at any depth.
fn has_tests(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else { return false };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_dir()
            && entry.file_name() != "target"
            && (entry.file_name() == "test" || has_tests(&path))
    })
}

/// The non-blank lines of an optional file.
fn lines_of(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Runs `jux <args>` in `dir`, feeding `input`, and returns the outcome.
fn jux(dir: &Path, args: &[String], input: &[u8]) -> Result<String, String> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_jux"))
        .args(["--manifest-path"])
        .arg(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning jux: {e}"))?;
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(input);
    }
    let output = child.wait_with_output().map_err(|e| format!("waiting on jux: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "`jux {}` exited with {:?}\n{}\n{}",
            args.join(" "),
            output.status.code(),
            stdout.trim_end(),
            String::from_utf8_lossy(&output.stderr).trim_end()
        ))
    }
}

/// Builds and runs one app and compares its output with `expected.txt`; then
/// runs its tests when it has any.
fn run_app(workspace: &Path, name: &str) -> Result<(), String> {
    let app = workspace.join("examples").join("apps").join(name);
    let staged = workspace.join("target").join("apps").join(name);
    let _ = fs::remove_dir_all(&staged);
    copy_tree(&app, &staged);

    let expected = fs::read_to_string(app.join("expected.txt"))
        .map(|t| normalize(&t))
        .map_err(|e| format!("no expected.txt: {e}"))?;

    let mut args = vec!["run".to_string()];
    args.extend(lines_of(&app.join("run-args.txt")));
    let input = fs::read(app.join("stdin.txt")).unwrap_or_default();
    let got = normalize(&jux(&staged, &args, &input)?);
    if got != expected {
        return Err(format!(
            "output differs from expected.txt\n--- expected\n{expected}\n--- got\n{got}"
        ));
    }

    if has_tests(&app) {
        jux(&staged, &["test".to_string()], &[])?;
    }
    Ok(())
}

/// The apps named in `APP`, or every app when it is unset.
fn selected(all: Vec<String>) -> Vec<String> {
    match std::env::var("APP") {
        Ok(filter) if !filter.trim().is_empty() => {
            let wanted: Vec<&str> = filter.split(',').map(str::trim).collect();
            for name in &wanted {
                assert!(
                    all.iter().any(|app| app == name),
                    "APP names `{name}`, which is not an app folder"
                );
            }
            all.into_iter().filter(|n| wanted.contains(&n.as_str())).collect()
        }
        _ => all,
    }
}

#[test]
fn apps() {
    let workspace = common::workspace_root();
    let root = workspace.join("examples").join("apps");
    let apps = selected(discover(&root));
    assert!(!apps.is_empty(), "no apps found under {}", root.display());

    // A shared queue drained by a few workers: each app is its own cargo
    // build, and most of the cost is waiting on them.
    let queue = Mutex::new(apps.clone());
    let results = Mutex::new(BTreeMap::new());
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 4))
        .unwrap_or(2);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let Some(name) = queue.lock().unwrap().pop() else { break };
                let outcome = run_app(&workspace, &name);
                results.lock().unwrap().insert(name, outcome);
            });
        }
    });

    let problems: Vec<String> = results
        .into_inner()
        .unwrap()
        .into_iter()
        .filter_map(|(name, outcome)| outcome.err().map(|e| format!("{name}: {e}")))
        .collect();
    assert!(
        problems.is_empty(),
        "{} app problem(s):\n\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}
