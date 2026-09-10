//! Every example must be named by some test.
//!
//! Coverage used to be opt-in: one hand-written test binary per example, and
//! nothing at all to notice a new example that nobody wired up. That is how
//! five examples went stale unremarked, and how `closures_into_rust` sat
//! broken for months -- it had never built, and no test existed to say so.
//!
//! This runs no builds and costs milliseconds: it reads the example directory,
//! reads the test sources, and reports anything in the first that is not named
//! by the second. It is deliberately dumb about HOW an example is covered --
//! asserting that a test mentions it is enough to make the omission visible,
//! and the test itself asserts what the example should do.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Examples that are deliberately not covered, and why.
///
/// Keep this list short and keep the reasons here: an entry with no reason is
/// indistinguishable from an oversight, which is the thing this test exists to
/// prevent.
const EXCLUDED: &[(&str, &str)] = &[
    // Hand-written Rust, not Jux -- reference lowerings kept beside the
    // examples they explain. There is nothing for `jux` to compile.
    ("_sealed", "hand-written Rust reference lowering"),
    ("_seal_pat", "hand-written Rust reference lowering"),
    ("_stress_workers", "hand-written Rust reference lowering"),
    // A library with no entry point: there is no program to run.
    ("stress_alias", "library only, no entry point"),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

/// Every `.rs` under the given test directories, concatenated.
fn all_test_sources(root: &Path) -> String {
    let mut dirs: Vec<PathBuf> = vec![
        root.join("bin").join("jux").join("tests"),
        root.join("bin").join("juxc").join("tests"),
    ];
    // Any crate may gate an example too (`juxc-driver` does).
    if let Ok(crates) = std::fs::read_dir(root.join("crates")) {
        for entry in crates.flatten() {
            let t = entry.path().join("tests");
            if t.is_dir() {
                dirs.push(t);
            }
        }
    }

    let mut text = String::new();
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    text.push_str(&s);
                    text.push('\n');
                }
            }
        }
    }
    text
}

/// Is `name` named by a test?
///
/// Matched as a quoted STRING -- a file reference (`"name.jux"`), a path join
/// (`join("name")`), or the bare name a cluster runner passes its helper
/// (`expect_output("name", ...)`). Never as a bare word: several tests mention
/// an example in prose, and counting that as coverage would hide exactly the
/// gaps this is looking for.
fn is_covered(root: &Path, tests: &str, name: &str) -> bool {
    // The strongest form of coverage, and the one that needs no mention at all:
    // `bin/jux/tests/run.rs` discovers the corpus at runtime and pins each
    // example's exact output, so a blessed expectation IS the test. This is
    // what let the per-example test files go: they asserted less, and they
    // asserted it six times slower.
    if root
        .join("tests")
        .join("expected")
        .join("examples")
        .join(format!("{name}.expected"))
        .is_file()
    {
        return true;
    }
    tests.contains(&format!("\"{name}.jux\""))
        || tests.contains(&format!("join(\"{name}\")"))
        || tests.contains(&format!("\"{name}\""))
        || tests.contains(&format!("/{name}.jux\""))
        || tests.contains(&format!("/{name}\""))
        // A directory named as a PATH SEGMENT: `examples/multifile/app.jux`
        // covers the `multifile` directory as well as the file.
        || tests.contains(&format!("/{name}/"))
}

#[test]
fn every_example_is_named_by_a_test() {
    let root = workspace_root();
    let tests = all_test_sources(&root);
    let excluded: BTreeSet<&str> = EXCLUDED.iter().map(|(n, _)| *n).collect();

    let mut missing: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(root.join("examples")).expect("examples/ exists");
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let is_jux = path.extension().and_then(|e| e.to_str()) == Some("jux");
        // A directory is a multi-file project; anything else on disk (a stray
        // `target/`, an editor file) is not an example.
        let is_project = path.is_dir() && path.join("jux.toml").exists()
            || path.is_dir() && path.join("app.jux").exists()
            || path.is_dir() && path.join("lib.jux").exists()
            || path.is_dir()
                && std::fs::read_dir(&path).is_ok_and(|d| {
                    d.flatten()
                        .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jux"))
                });
        if !is_jux && !is_project {
            continue;
        }
        if excluded.contains(name.as_str()) {
            continue;
        }
        if !is_covered(&root, &tests, &name) {
            missing.push(name);
        }
    }
    missing.sort();

    assert!(
        missing.is_empty(),
        "{} example(s) have no test naming them.\n\
         Add one, or add an entry to EXCLUDED in this file WITH a reason:\n  {}",
        missing.len(),
        missing.join("\n  "),
    );
}

/// The exclusion list must not rot either: an entry for an example that no
/// longer exists is a stale reason nobody will think to remove.
#[test]
fn every_exclusion_still_refers_to_something() {
    let root = workspace_root();
    let examples = root.join("examples");
    for (name, reason) in EXCLUDED {
        let jux = examples.join(format!("{name}.jux"));
        let dir = examples.join(name);
        assert!(
            jux.exists() || dir.exists(),
            "EXCLUDED names `{name}` ({reason}), which no longer exists",
        );
    }
}
