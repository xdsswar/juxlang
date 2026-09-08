//! UI tests: every diagnostic's exact rendered output is pinned.
//!
//! This is the gate Rust leans on hardest. `tests/ui` there is thousands of
//! small programs, each with the compiler's exact stderr checked in beside it;
//! change a message and the test fails, so you re-bless it deliberately and
//! the diff shows every user-visible word that moved.
//!
//! Jux had nothing equivalent. Three assertions in the whole suite looked at a
//! diagnostic's TEXT, so ~140 specced codes could change their wording, their
//! span, or stop firing entirely, and every test would still pass. The example
//! corpus proves what the compiler ACCEPTS; this proves what it says when it
//! refuses.
//!
//! Each case is a pair under `tests/ui/`:
//!
//! - `<name>.jux` — a small program, usually one bad line.
//! - `<name>.expected` — the exact `juxc --check` output for it.
//!
//! To add one: write the `.jux`, run with `JUX_BLESS=1`, read the generated
//! `.expected` and decide whether that is the message a user should get. That
//! reading is the point — a bad message blessed without thought is worse than
//! no test, because it looks like a decision.
//!
//! ```text
//! cargo test -p juxc --test ui              # check
//! JUX_BLESS=1 cargo test -p juxc --test ui  # re-bless every case
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

/// The compiler's output with everything machine-specific taken out: the
/// absolute path becomes the bare file name, and line endings normalize. What
/// remains is the part a user reads.
fn normalize(raw: &str, case: &str) -> String {
    let mut out = String::new();
    for line in raw.replace("\r\n", "\n").lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        // `C:/long/path/to/tests/ui/foo.jux:3:15: …` → `foo.jux:3:15: …`
        let needle = format!("{case}.jux");
        let cleaned = match line.find(&needle) {
            Some(i) => &line[i..],
            None => line,
        };
        out.push_str(cleaned);
        out.push('\n');
    }
    out
}

/// Run one case and return its normalized output.
///
/// A case is either a single `<name>.jux` or a DIRECTORY `<name>/` holding
/// several. `juxc` already expands a directory into every `.jux` inside it,
/// which is how the diagnostics that need more than one file -- a conflicting
/// import, a package cycle, a profile rule from `jux.toml` -- get a case at
/// all. They were the reason those codes had no test.
fn run_case(jux: &Path) -> String {
    let case = jux.file_stem().and_then(|s| s.to_str()).expect("case name");
    let output = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg("--check")
        .arg(jux)
        .output()
        .unwrap_or_else(|e| panic!("spawning juxc --check for {case}: {e}"));
    let blob = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    normalize(&blob, case)
}

#[test]
fn every_ui_case_matches_its_expected_output() {
    let root = workspace_root();
    let dir = root.join("tests").join("ui");
    assert!(dir.is_dir(), "tests/ui is missing");

    let bless = std::env::var("JUX_BLESS").is_ok();
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("reading tests/ui")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("jux")
                || (p.is_dir() && p.join("jux.toml").is_file())
        })
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "tests/ui has no cases");

    let mut failures: Vec<String> = Vec::new();
    for jux in &cases {
        let name = jux.file_stem().unwrap().to_string_lossy().to_string();
        // A directory case keeps its `.expected` beside it, not inside it.
        let got = run_case(jux);
        let expected_path = jux.with_extension("expected");

        if bless {
            std::fs::write(&expected_path, &got).expect("writing .expected");
            continue;
        }

        let want = match std::fs::read_to_string(&expected_path) {
            Ok(s) => s.replace("\r\n", "\n"),
            Err(_) => {
                failures.push(format!(
                    "{name}: no .expected file. Run with JUX_BLESS=1, then READ what it \
                     generated:\n{got}"
                ));
                continue;
            }
        };
        if got != want {
            failures.push(format!(
                "{name}: output changed.\n--- expected ---\n{want}--- got ---\n{got}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} UI case(s) differ. If the new text is BETTER, re-bless with \
         JUX_BLESS=1 and commit the diff — it is the record of what users now see.\n\n{}",
        failures.len(),
        failures.join("\n\n"),
    );
}

/// A case with no `.jux` beside it is a leftover, and an orphan `.expected`
/// silently stops testing anything.
#[test]
fn no_orphan_expected_files() {
    let dir = workspace_root().join("tests").join("ui");
    let mut orphans: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("reading tests/ui").flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("expected") {
            continue;
        }
        // A case is a `<name>.jux` OR a `<name>/` directory of them.
        let stem = p.with_extension("");
        if !p.with_extension("jux").exists() && !stem.is_dir() {
            orphans.push(p.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    assert!(orphans.is_empty(), "expected files with no case: {orphans:?}");
}

/// No em-dash in anything the compiler says.
///
/// House rule, and the UI cases are where it became visible: two of the first
/// fifteen blessed messages used `—` while thirteen used `--`, so the
/// compiler's voice changed line to line. Checking the blessed output catches
/// it for every case at once, without a lint over the source.
#[test]
fn no_em_dash_in_any_diagnostic() {
    let dir = workspace_root().join("tests").join("ui");
    let mut offenders: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("reading tests/ui").flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("expected") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        if text.contains('\u{2014}') {
            offenders.push(p.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "em-dash in diagnostic text (use `--`): {offenders:?}",
    );
}
