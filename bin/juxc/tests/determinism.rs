//! The same input lowers to byte-identical Rust, every time.
//!
//! Rust guarantees this and tests for it, and the reason is not tidiness: a
//! compiler that reorders its own output has a `HashMap` iteration somewhere
//! in a place that decides program structure. That is a real bug wearing a
//! cosmetic disguise — it means the emitted code depends on hash seeds, and
//! so, eventually, does whether it compiles.
//!
//! A sample is checked here rather than the whole corpus, which takes several
//! minutes; the full sweep is one shell loop over `examples/*.jux` and was run
//! when this landed (259 examples, zero differences).

use std::path::PathBuf;
use std::process::Command;

/// Examples chosen to cover the features most likely to iterate a map:
/// generics and their bounds, interface dispatch, collections, enums with
/// payloads, and a program with many declarations.
const SAMPLE: &[&str] = &[
    "generics_nesting",
    "stress_zoo",
    "stress_kitchen_sink",
    "indexed_writes",
    "generic_declarations_print",
    "safe_navigation",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

/// Lower `example` into `out` and return every emitted file as (relative
/// path, contents), sorted, so two runs compare as a whole tree.
fn lower(example: &str, out: &PathBuf) -> Vec<(String, String)> {
    let root = workspace_root();
    let src = root.join("examples").join(format!("{example}.jux"));
    let _ = std::fs::remove_dir_all(out);

    let output = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg(&src)
        .arg("--emit-dir")
        .arg(out)
        .output()
        .unwrap_or_else(|e| panic!("spawning juxc for {example}: {e}"));
    assert!(
        output.status.success(),
        "{example} failed to lower:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let mut files = Vec::new();
    let mut stack = vec![out.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(text) = std::fs::read_to_string(&p) {
                let rel = p.strip_prefix(out).unwrap_or(&p).to_string_lossy().to_string();
                files.push((rel.replace('\\', "/"), text));
            }
        }
    }
    files.sort();
    assert!(!files.is_empty(), "{example} emitted nothing into {out:?}");
    files
}

#[test]
fn lowering_is_deterministic() {
    let tmp = std::env::temp_dir().join("jux-determinism");
    for example in SAMPLE {
        let a = lower(example, &tmp.join("a"));
        let b = lower(example, &tmp.join("b"));

        assert_eq!(
            a.len(),
            b.len(),
            "{example}: two runs emitted a different set of files",
        );
        for ((pa, ca), (pb, cb)) in a.iter().zip(b.iter()) {
            assert_eq!(pa, pb, "{example}: file list differs between runs");
            assert_eq!(
                ca, cb,
                "{example}: `{pa}` differs between two runs of the same input. \
                 Something in the lowering iterates a hash map where order reaches \
                 the output.",
            );
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
