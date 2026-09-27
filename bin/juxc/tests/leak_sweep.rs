//! No pinned output shows Rust (GAPS.md gap 33).
//!
//! A Jux programmer never sees Rust: not a Rust type or path, not rustc's or
//! cargo's words, not a `.rs` location, not Rust's panic text. Every exit the
//! compiler and the emitted program have is guarded by one detector,
//! `juxc_diagnostics::leak`. This test holds the record of what users see to
//! the same rule: every `tests/ui/*.expected` (what the compiler says) and
//! every `tests/expected/examples/*.expected` (what a program prints) is run
//! through the detector, told what the case's own source says so the
//! program's own names and strings are not mistaken for a leak.
//!
//! A hit is fixed where the text is made, never here. The allowlist
//! (`tests/leak-allowlist.txt`) exists for the rare output that must show
//! such text on purpose; each line names the file and the matched text and
//! says why, and a line that no longer matches anything fails the test so
//! the list cannot rot. It is empty.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

/// Every `.jux` text of a case: `<dir>/<name>.jux`, or all of `<dir>/<name>/`.
fn case_sources(dir: &Path, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let file = dir.join(format!("{name}.jux"));
    if let Ok(text) = std::fs::read_to_string(&file) {
        out.push(text);
    }
    let sub = dir.join(name);
    if sub.is_dir() {
        collect(&sub, &mut out);
    }
    out
}

fn collect(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|e| e == "jux" || e == "toml") {
            if let Ok(text) = std::fs::read_to_string(&p) {
                out.push(text);
            }
        }
    }
}

/// The one place an output may quote rustc: the `E0900` one-line formats
/// append rustc's error, which is how a compiler bug gets reported (ERRATA
/// E116, E125). Taken out before the scan.
fn without_documented_rustc_note(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        match (line.find(" (rustc reported "), line.rfind("`--verbose` shows the full report)")) {
            (Some(start), Some(end)) if line.contains("[E0900]") && end > start => {
                out.push_str(&line[..start]);
                out.push_str(&line[end + "`--verbose` shows the full report)".len()..]);
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// `(file, matched text)` pairs the allowlist excuses.
fn allowlist(root: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(root.join("tests").join("leak-allowlist.txt")).unwrap_or_default();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut parts = l.splitn(3, " | ");
            let file = parts.next().unwrap_or("").trim().to_string();
            let matched = parts.next().unwrap_or("").trim().to_string();
            assert!(
                parts.next().is_some_and(|why| !why.trim().is_empty()),
                "tests/leak-allowlist.txt: every entry says why (`file | matched | reason`): {l}"
            );
            (file, matched)
        })
        .collect()
}

/// Run the detector over every `.expected` in `dir`, against the sources the
/// case in `source_dir` holds.
fn sweep(root: &Path, dir: &Path, source_dir: &Path, allowed: &[(String, String)], used: &mut Vec<usize>) -> Vec<String> {
    let mut failures = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "expected"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "{} has no .expected files", dir.display());
    for file in &files {
        let name = file.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let rel = file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(file).unwrap_or_default().replace("\r\n", "\n");
        if text.contains("this diagnostic's text was not written in Jux terms") {
            failures.push(format!("{rel}: pins the leak guard's E0900, so a diagnostic there shows Rust"));
            continue;
        }
        let sources = case_sources(source_dir, &name);
        let texts: Vec<&str> = sources.iter().map(String::as_str).collect();
        let wrote = juxc_diagnostics::leak::contained_in(&texts);
        let scanned = without_documented_rustc_note(&text);
        if let Some(hit) = juxc_diagnostics::leak::find_rust_leak_quoting(&scanned, &wrote) {
            match allowed.iter().position(|(f, m)| *f == rel && *m == hit.matched) {
                Some(i) => used.push(i),
                None => failures.push(format!("{rel}: {hit}")),
            }
        }
    }
    failures
}

#[test]
fn no_pinned_output_shows_rust() {
    let root = workspace_root();
    let allowed = allowlist(&root);
    let mut used = Vec::new();
    let mut failures = sweep(&root, &root.join("tests").join("ui"), &root.join("tests").join("ui"), &allowed, &mut used);
    failures.extend(sweep(
        &root,
        &root.join("tests").join("expected").join("examples"),
        &root.join("examples"),
        &allowed,
        &mut used,
    ));
    let stale: Vec<String> = allowed
        .iter()
        .enumerate()
        .filter(|(i, _)| !used.contains(i))
        .map(|(_, (f, m))| format!("{f} | {m}"))
        .collect();
    assert!(
        stale.is_empty(),
        "tests/leak-allowlist.txt excuses output that no longer shows it; delete these lines:\n{}",
        stale.join("\n")
    );
    assert!(
        failures.is_empty(),
        "{} pinned output(s) show Rust to the user. Fix the text where it is made \
         (the diagnostic, the prelude, the emitter), then hand-edit the .expected:\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The sweep would pass vacuously if the detector found nothing anywhere;
/// it does find what it is for.
#[test]
fn the_sweep_sees_a_planted_leak() {
    let wrote = juxc_diagnostics::leak::contained_in(&[]);
    let planted = "app.jux:3:5: [E0410] error: expected `std::string::String`\n";
    assert!(juxc_diagnostics::leak::find_rust_leak_quoting(planted, &wrote).is_some());
    let e0900 = "app.jux:3:5: [E0900] error: internal compiler error: x (rustc reported error[E0599]: \
                 no method; a compiler bug, `--verbose` shows the full report)\n";
    assert!(juxc_diagnostics::leak::find_rust_leak_quoting(&without_documented_rustc_note(e0900), &wrote).is_none());
}
