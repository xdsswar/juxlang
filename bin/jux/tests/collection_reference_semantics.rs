//! End-to-end test for JUX-LANG-V1 §6.5.1 — collections are reference types.
//!
//! Every line here was silently wrong before the collection handle existed:
//! `var a = obj.getItems(); a.push(v)` mutated a temporary copy and vanished,
//! with no diagnostic. These are the shapes that must keep aliasing, plus the
//! two rules that fall out of it — `clone()` copies (shallowly), and a
//! `for`-each walks a snapshot so a body may mutate what it iterates.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn collections_alias_like_java_objects() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root
        .join("examples")
        .join("collection_reference_semantics.jux");
    let emit_dir = workspace_root
        .join("target")
        .join("it-collection-ref-semantics");

    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("spawn jux");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "jux exited with {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(
        lines.as_slice(),
        [
            "1",   // a getter hands back the real collection
            "1",   // a second name aliases the first
            "2",   // an argument passes the reference
            "1",   // a collection inside a map is shared too
            "2 3", // clone() copies: the original did not grow
            "42",  // an indexed write lands in the shared collection
            "2 9", // a read and a write on one collection in one statement
            "4",   // a for-each walks a snapshot; the body appended two
        ],
        "unexpected output:\n{stdout}",
    );
}
