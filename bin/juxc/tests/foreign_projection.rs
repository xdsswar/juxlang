//! What a program is told about an associated-type projection in a foreign
//! signature (Bindgen §G.6.4.5 / §G.6.4.6).
//!
//! Rust's `fn get<I>(&self, i: I) -> Option<&I::Output> where I: SliceIndex<[T]>`
//! says what it gives back only once `I` is known. The binder resolves that
//! from the bound's own impls, so `v.get(0)` on a `Vec<string?>` is a
//! `string??` and handing it to a `string?` parameter is a Jux error. When the
//! binder cannot resolve one, the call is `E0469` rather than a value of an
//! unknown type that quietly fits every slot -- which is how this reached rustc
//! in the first place, and §G.1 says a leaked rustc error is always a juxc bug.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

/// A fresh directory under `target/` for one case.
fn case_dir(tag: &str) -> PathBuf {
    let dir = workspace_root().join("target").join(format!("it-projection-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creating the case directory");
    dir
}

/// `juxc --check <dir>`, with the `rust.std` surface pinned to the snapshot
/// this repository vendors.
///
/// Pinning matters: the compiler otherwise serves `rust.std` from a cache under
/// the user's cache directory, which any other checkout on the machine may have
/// written with its own bindgen. The assertions below are about what THIS
/// compiler says, so the stub has to be this repository's.
fn check(dir: &Path) -> String {
    check_with(dir, &[])
}

/// [`check`] in the `human` format, which is the only one that prints the
/// `help:` line (the piped default is one line per diagnostic, ERRATA E72).
fn check_human(dir: &Path) -> String {
    check_with(dir, &["--diagnostic-format", "human", "--color", "never"])
}

fn check_with(dir: &Path, args: &[&str]) -> String {
    let stubs = workspace_root().join("crates").join("juxc-driver").join("stubs");
    let out = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg("--check")
        .arg(dir)
        .args(args)
        .env("JUX_STUBS_DIR", &stubs)
        .output()
        .expect("running juxc --check");
    let blob = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    // Paths are machine-specific; the words are not.
    let raw = dir.display().to_string();
    blob.replace(&raw, "<case>").replace(&raw.replace('\\', "/"), "<case>")
}

/// The reported bug, end to end: `Vec<string?>` makes `get` a `string??`
/// (nullability NESTS for a generic instantiation, §M.15.2), and handing that
/// to a `string?` parameter is a mismatch juxc names in Jux's own words.
///
/// Before the projection was resolved, `get` returned `I.Output?` for every
/// element type there is, the checker read that as an unknown type, and an
/// unknown type fits every slot: this program compiled clean and failed in
/// rustc.
#[test]
fn a_nullable_element_reaching_a_nullable_slot_is_a_jux_error() {
    let dir = case_dir("nested-nullable");
    std::fs::write(
        dir.join("main.jux"),
        "import rust.std.Vec;\n\
         \n\
         public void main() {\n\
         \x20   var v = new Vec<string?>();\n\
         \x20   v.push(\"x\");\n\
         \x20   show(v.get(0));\n\
         }\n\
         \n\
         public void show(string? st) { print($\"Value : $st\"); }\n",
    )
    .expect("writing main.jux");

    let got = check(&dir);
    assert!(
        got.contains("[E0410]") && got.contains("expected String?, found String??"),
        "expected the nested-nullable mismatch, got:\n{got}",
    );
    assert!(
        !got.contains("<unknown>"),
        "the element type must be known now, got:\n{got}",
    );
    let human = check_human(&dir);
    assert!(
        human.contains("`?? null`"),
        "the help should name the operator that collapses a layer, got:\n{human}",
    );
}

/// The same `get`, with the element type known: it types as `string?` and the
/// program compiles.
#[test]
fn a_resolved_projection_types_as_the_element() {
    let dir = case_dir("resolved");
    std::fs::write(
        dir.join("main.jux"),
        "public void main() {\n\
         \x20   var v = new Vec<string>();\n\
         \x20   v.push(\"hello\");\n\
         \x20   string? a = v.get(0);\n\
         \x20   print($\"a = ${a ?? \"none\"}\");\n\
         }\n",
    )
    .expect("writing main.jux");

    let got = check(&dir);
    assert!(got.is_empty(), "expected a clean check, got:\n{got}");
}

/// A projection the binder could NOT resolve is reported at the call (E0469),
/// not passed on as an unknown type.
///
/// The stub here is hand-written, standing for a crate whose rustdoc does not
/// carry the projection's trait: that is the one case §G.6.4.5 cannot resolve,
/// and `rust.std` no longer has one for the test to borrow.
#[test]
fn an_unresolved_projection_is_reported_at_the_call() {
    let dir = case_dir("unresolved");
    std::fs::write(
        dir.join("probe.jux.d"),
        "package rust.probe;\n\
         \n\
         @rust(\"probe::Holder\")\n\
         public class Holder<T> {\n\
         \x20   public Holder();\n\
         \x20   public I.Output? get<I>(I index);\n\
         }\n",
    )
    .expect("writing probe.jux.d");
    std::fs::write(
        dir.join("main.jux"),
        "import rust.probe.Holder;\n\
         \n\
         public void main() {\n\
         \x20   var h = new Holder<int>();\n\
         \x20   print($\"got ${h.get(0)}\");\n\
         }\n",
    )
    .expect("writing main.jux");

    let got = check(&dir);
    assert!(
        got.contains("[E0469]") && got.contains("`I.Output`"),
        "expected E0469 naming the projection, got:\n{got}",
    );
    assert!(
        got.contains("Holder.get"),
        "the diagnostic should name the method, got:\n{got}",
    );
}
