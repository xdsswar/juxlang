//! What a Jux aggregate derives when it holds a foreign value
//! (JUX-CLASS-REPRESENTATION-ADDENDUM §CR.5.8, Bindgen §G.6.4.7, ERRATA E97).
//!
//! A class field, a record component or a collection element of a Rust type
//! decides what the enclosing struct can `#[derive]`, and the answer comes from
//! that type's own stub markers. Before it did, the only question asked was
//! whether the type came from a crate other than `std`, on the assumption that
//! every `rust.std` type is `Clone`. 139 of the 296 types in the std stub are
//! not, `std::fs::File` among them, so no class could hold an open file.
//!
//! These cases pin the emitted Rust rather than the program's output, because
//! what changed is which derives are written. Each one is driven by a synthetic
//! stub declaring exactly the marker combination under test, so the assertions
//! do not move when the real std surface does. Portability is the other reason
//! for the synthetic stub: real std has no PORTABLE type without `Debug`, and
//! the whole point of the second half of this rule is what happens to one.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

/// The three marker combinations under test, declared into `rust.std` so a
/// program reaches them through the ordinary auto-import and needs no crate
/// dependency. Their `@rust` paths are never compiled: these cases read the
/// emitted Rust and stop there.
const PROBE_STUB: &str = "\
package rust.std;

/** A foreign type that is `Clone` and `Debug`, like `String`. */
@rust(\"probe::Whole\")
@RustClone
@RustDebug
@RustPartialEq
@RustDefault
public class ProbeWhole {
    public static ProbeWhole make();
}

/** A foreign type that is `Debug` and not `Clone`, exactly like `std::fs::File`. */
@rust(\"probe::Printable\")
@RustDebug
public class ProbePrintable {
    public static ProbePrintable make();
}

/** A foreign type with none of the four, like most crate handles. */
@rust(\"probe::Opaque\")
public class ProbeOpaque {
    public static ProbeOpaque make();
}
";

/// A case directory holding `main.jux` and a private stub directory: the
/// vendored `rust.std` snapshot plus [`PROBE_STUB`].
///
/// The snapshot is copied in because `JUX_STUBS_DIR` short-circuits stub
/// generation entirely, so a directory holding only the probe stub would leave
/// `print` and the prelude types undeclared. Pinning it is wanted anyway: the
/// compiler otherwise serves `rust.std` from a cache under the user's cache
/// directory that any other checkout on the machine may have written.
fn case_dir(tag: &str, program: &str) -> PathBuf {
    let root = workspace_root();
    let dir = root.join("target").join(format!("it-derive-markers-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("stubs")).expect("creating the case directory");
    std::fs::write(dir.join("main.jux"), program).expect("writing main.jux");
    let vendored = root
        .join("crates")
        .join("juxc-driver")
        .join("stubs")
        .join("rust-std.jux.d");
    std::fs::copy(&vendored, dir.join("stubs").join("rust-std.jux.d"))
        .expect("copying the vendored rust.std snapshot");
    std::fs::write(dir.join("stubs").join("probe.jux.d"), PROBE_STUB).expect("writing probe.jux.d");
    dir
}

/// Lower the case's program and hand back the emitted `main.rs`.
fn emit(dir: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg(dir.join("main.jux"))
        .env("JUX_STUBS_DIR", dir.join("stubs"))
        .output()
        .expect("running juxc");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(out.status.success(), "juxc refused the program:\n{said}");
    let emitted = dir.join("target").join(".rust-build").join("src").join("main.rs");
    std::fs::read_to_string(&emitted)
        .unwrap_or_else(|e| panic!("reading {}: {e}\njuxc said:\n{said}", emitted.display()))
}

/// The declaration under test, for a failure message: the struct and the
/// attributes above it, with the surrounding hundred lines of prelude left out.
fn declaration_of(rust: &str, name: &str) -> String {
    // A class's inner struct is `pub`, its newtype handle and a record are
    // `pub(crate)` when the declaration is package-private, so both spellings
    // have to be looked for.
    let heads = [format!("pub struct {name}"), format!("pub(crate) struct {name}")];
    for head in &heads {
        if let Some(at) = rust.find(head.as_str()) {
            let from = rust[..at].rfind("\n\n").map(|i| i + 2).unwrap_or(0);
            let to = rust[at..].find("\n}\n").map(|i| at + i + 3).unwrap_or(rust.len());
            return rust[from..to].to_string();
        }
    }
    format!("(no `struct {name}` in the emitted Rust)")
}

/// A type that IS `Clone + Debug` still derives both. The rule only takes away
/// what the held type does not have, so nothing about an ordinary foreign field
/// changed.
#[test]
fn a_clone_and_debug_type_keeps_both_derives() {
    let dir = case_dir(
        "whole",
        "public void main() { print(\"built\"); }\n\
         \n\
         class Wrap {\n\
         \x20   public ProbeWhole w;\n\
         \x20   public Wrap(ProbeWhole w) { this.w = w; }\n\
         }\n",
    );
    let rust = emit(&dir);
    let decl = declaration_of(&rust, "Wrap_Inner");
    assert!(
        decl.contains("#[derive(Clone, Debug)]"),
        "a Clone+Debug field must keep both derives, got:\n{decl}",
    );
    assert!(
        !rust.contains("impl std::fmt::Debug for Wrap_Inner"),
        "a derivable Debug must not be written out by hand:\n{decl}",
    );
}

/// The reported defect. `std::fs::File` is `Debug` and is not `Clone`, so the
/// inner struct keeps `Debug` and loses `Clone`. The wrapper newtype is `Clone`
/// regardless: it shares the instance through an `Rc`, which is `Clone` for
/// every payload.
#[test]
fn a_type_without_clone_loses_only_clone() {
    let dir = case_dir(
        "printable",
        "public void main() { print(\"built\"); }\n\
         \n\
         class Wrap {\n\
         \x20   public ProbePrintable p;\n\
         \x20   public Wrap(ProbePrintable p) { this.p = p; }\n\
         }\n",
    );
    let rust = emit(&dir);
    let decl = declaration_of(&rust, "Wrap_Inner");
    assert!(
        decl.contains("#[derive(Debug)]"),
        "a Debug-but-not-Clone field keeps Debug alone, got:\n{decl}",
    );
    assert!(
        !decl.contains("Clone"),
        "nothing can clone the held value, so the inner cannot derive Clone:\n{decl}",
    );
    let newtype = declaration_of(&rust, "Wrap(");
    assert!(
        newtype.contains("#[derive(Clone, Debug)]"),
        "the handle itself still clones by refcount, got:\n{newtype}",
    );
}

/// A type with NEITHER trait. `Debug` is a dropped derive, not a dropped trait:
/// the impl is written out so an interface's `Debug` supertrait, `throw`
/// formatting and a container of these values all keep working.
#[test]
fn a_type_with_neither_gets_a_written_debug_impl() {
    let dir = case_dir(
        "opaque",
        "public void main() { print(\"built\"); }\n\
         \n\
         class Wrap {\n\
         \x20   public ProbeOpaque o;\n\
         \x20   public Wrap(ProbeOpaque o) { this.o = o; }\n\
         }\n",
    );
    let rust = emit(&dir);
    let decl = declaration_of(&rust, "Wrap_Inner");
    assert!(
        !decl.contains("#[derive("),
        "neither trait is derivable, so no derive attribute is written:\n{decl}",
    );
    assert!(
        rust.contains("impl std::fmt::Debug for Wrap_Inner"),
        "the inner needs a written Debug so the newtype's derived Debug resolves:\n{rust}",
    );
}

/// A record over such a type. Its `Debug` is written out in the shape the derive
/// would have produced, rendering the component through the universal show
/// helper, and the three traits the component has no impl for come off the
/// derive list instead of failing in rustc.
#[test]
fn a_record_component_takes_only_what_the_type_lacks() {
    let dir = case_dir(
        "record",
        "public void main() { print(\"built\"); }\n\
         \n\
         record Holder(ProbeOpaque o);\n",
    );
    let rust = emit(&dir);
    let decl = declaration_of(&rust, "Holder");
    assert!(
        !decl.contains("#[derive("),
        "a component with no Clone, Debug, PartialEq or Default leaves nothing to derive:\n{decl}",
    );
    assert!(
        rust.contains("impl std::fmt::Debug for Holder"),
        "the record's Debug is written out instead:\n{rust}",
    );
    assert!(
        rust.contains("write!(f, \"Holder {{ o: {}\", crate::__jux_show!(self.o))")
            || rust.contains("crate::__jux_show!(self.o)"),
        "the written Debug renders the component through the show helper:\n{rust}",
    );
}

/// A component that HAS all four keeps the full record derive list, so the rule
/// costs an ordinary record nothing.
#[test]
fn a_whole_record_component_keeps_the_full_derive_list() {
    let dir = case_dir(
        "record-whole",
        "public void main() { print(\"built\"); }\n\
         \n\
         record Holder(ProbeWhole w);\n",
    );
    let rust = emit(&dir);
    let decl = declaration_of(&rust, "Holder");
    assert!(
        decl.contains("#[derive(Debug, Clone, PartialEq, Default)]"),
        "a Clone+Debug+PartialEq+Default component keeps all four, got:\n{decl}",
    );
}

/// A generic argument counts as part of the stored type: a `Vec<ProbeOpaque>`
/// field is only as `Debug` as its element, even though `Vec` itself has every
/// impl there is. Its `Clone` survives, because a collection is held behind a
/// shared handle whose clone is a refcount bump (§6.5.1).
#[test]
fn a_generic_argument_counts_as_part_of_the_field_type() {
    let dir = case_dir(
        "element",
        "public void main() { print(\"built\"); }\n\
         \n\
         class Cabinet {\n\
         \x20   public Vec<ProbeOpaque> items;\n\
         \x20   public Cabinet() { this.items = new Vec<ProbeOpaque>(); }\n\
         }\n",
    );
    let rust = emit(&dir);
    let decl = declaration_of(&rust, "Cabinet_Inner");
    assert!(
        decl.contains("#[derive(Clone)]"),
        "the handle still clones by refcount, but its element has no Debug, got:\n{decl}",
    );
    assert!(
        rust.contains("impl std::fmt::Debug for Cabinet_Inner"),
        "the element's missing Debug is covered by a written impl:\n{rust}",
    );
}
