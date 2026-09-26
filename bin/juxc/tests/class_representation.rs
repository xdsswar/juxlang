//! Which Rust representation the selector gives each class
//! (JUX-CLASS-REPRESENTATION-ADDENDUM §CR.3, §CR.4.1; ERRATA E121).
//!
//! The representation is invisible to a program: `examples/cr_rep_*.jux` pin
//! that each one still prints what Java would. These cases pin the other half,
//! the emitted Rust, because what the selector changes is exactly what the
//! corpus cannot see: which handle a class gets, and how its fields are
//! reached through it.
//!
//! Each case compiles an example (or a short program) with `juxc` and reads
//! the `main.rs` it emitted.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

/// A case directory holding `main.jux` and the vendored `rust.std` snapshot,
/// so the stubs the program sees do not depend on this machine's cache.
fn case_dir(tag: &str, program: &str) -> PathBuf {
    let root = workspace_root();
    let dir = root.join("target").join(format!("it-class-rep-{tag}"));
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
    dir
}

/// A case directory holding a copy of `examples/<name>.jux`.
fn example_case(name: &str) -> PathBuf {
    let source = workspace_root().join("examples").join(format!("{name}.jux"));
    let program = std::fs::read_to_string(&source)
        .unwrap_or_else(|e| panic!("reading {}: {e}", source.display()));
    case_dir(name, &program)
}

/// Lower the case's program and hand back the emitted `main.rs`.
fn emit(dir: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg(dir.join("main.jux"))
        .env("JUX_STUBS_DIR", dir.join("stubs"))
        // The representation fallback is a finding under the self-check: the
        // selector alone must get these programs right.
        .env("JUX_SELFCHECK", "1")
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

/// What a class's newtype handle holds: the text between `struct Name(` and
/// its closing `);`, visibility included. `None` when the class is not a
/// newtype handle (an Inline class is a plain struct).
fn handle_of(rust: &str, class: &str) -> Option<String> {
    let head = format!("struct {class}(");
    let at = rust.find(&head)? + head.len();
    let end = rust[at..].find(");")? + at;
    Some(rust[at..end].to_string())
}

fn assert_handle(rust: &str, class: &str, want: &str) {
    let got = handle_of(rust, class)
        .unwrap_or_else(|| panic!("no newtype handle for `{class}` in the emitted Rust:\n{rust}"));
    assert!(
        got.ends_with(want),
        "`{class}` should be held as `{want}`, the emitted handle is `{got}`",
    );
}

/// Tier `Rc`: a class nothing writes after construction drops its cell, even
/// when it is aliased and stored in a collection; a class that is written
/// keeps it.
#[test]
fn an_immutable_class_is_a_plain_rc_and_a_mutable_one_keeps_its_cell() {
    let rust = emit(&example_case("cr_rep_rc"));
    assert_handle(&rust, "Point", "std::rc::Rc<Point_Inner>");
    assert_handle(&rust, "Palette", "std::rc::Rc<Palette_Inner>");
    assert_handle(&rust, "Counter", "std::rc::Rc<crate::JuxCell<Counter_Inner>>");
    // A `Point`'s fields are read straight through the refcount...
    assert!(rust.contains("self.0.x"), "Point reads its field with no borrow:\n{rust}");
    assert!(!rust.contains("self.0.borrow().x"), "Point has no cell to borrow:\n{rust}");
    // ...and are constructed without one.
    assert!(
        rust.contains("Self(std::rc::Rc::new(Self::new_inner(x, y)))"),
        "Point is built straight into an Rc:\n{rust}",
    );
    // A `Counter` is written through its cell.
    assert!(rust.contains("self.0.borrow_mut()"), "Counter writes through its cell:\n{rust}");
}

/// Writes the selector has to see: through a field typed as the class, through
/// `this` in a method, through a bare field name, and into a value struct held
/// in a field. Each class below is written exactly one of those ways and must
/// keep its cell. `Untouched` is written by none, and since its one object
/// never leaves its local either, it is not even a handle.
#[test]
fn every_kind_of_write_keeps_the_cell() {
    let rust = emit(&case_dir(
        "writes",
        "struct Spot { public int x; public Spot(int x) { this.x = x; } }\n\
         class ByField { public int n = 0; }\n\
         class ByThis { private int n = 0; public void set(int v) { this.n = v; } public int get() { return n; } }\n\
         class ByBareName { private int n = 0; public void inc() { n++; } public int get() { return n; } }\n\
         class ByValueField { public Spot spot = new Spot(1); }\n\
         class Untouched { public int n = 7; }\n\
         public void main() {\n\
         \x20   var a = new ByField(); a.n = 4; print(a.n);\n\
         \x20   var b = new ByThis(); b.set(5); print(b.get());\n\
         \x20   var c = new ByBareName(); c.inc(); print(c.get());\n\
         \x20   var d = new ByValueField(); d.spot.x = 9; print(d.spot.x);\n\
         \x20   var e = new Untouched(); print(e.n);\n\
         }\n",
    ));
    for class in ["ByField", "ByThis", "ByBareName", "ByValueField"] {
        assert_handle(&rust, class, &format!("std::rc::Rc<crate::JuxCell<{class}_Inner>>"));
    }
    assert_inline(&rust, "Untouched");
}

/// An Inline class is a plain struct: no `_Inner`, no handle.
fn assert_inline(rust: &str, class: &str) {
    assert!(
        handle_of(rust, class).is_none() && !rust.contains(&format!("{class}_Inner")),
        "`{class}` should be a plain struct, the emitted Rust has a handle for it:\n{rust}",
    );
    assert!(
        rust.contains(&format!("// JUX-REP: inline\n#[derive(Clone)]\npub(crate) struct {class} {{")),
        "`{class}` should be marked and emitted as a plain struct:\n{rust}",
    );
}

/// Tier Inline / `Box`: a class whose objects stay in the local they were
/// made into is a value; one that is also returned is a `Box`; one that is
/// passed anywhere stays a shared handle.
#[test]
fn contained_classes_are_values_and_a_passed_one_is_not() {
    let rust = emit(&example_case("cr_rep_value"));
    assert_inline(&rust, "Money");
    assert_handle(&rust, "Receipt", "std::boxed::Box<Receipt_Inner>");
    assert!(
        rust.contains("Self(std::boxed::Box::new(Self::new_inner(item, cents)))"),
        "Receipt is built straight into a Box:\n{rust}",
    );
    assert_handle(&rust, "Ledger", "std::rc::Rc<Ledger_Inner>");
}

/// Tier `Arc` / `Arc<Mutex>`: a class a worker takes across gets an atomic
/// handle, with no lock when nothing writes it.
#[test]
fn a_worker_shared_class_is_atomic_and_locks_only_when_written() {
    let rust = emit(&example_case("cr_rep_arc"));
    assert_handle(&rust, "Config", "crate::JuxArc<Config_Inner>");
    assert_handle(&rust, "Tally", "crate::JuxSync<Tally_Inner>");
    assert!(
        rust.contains("Self(crate::JuxArc::new(Self::new_inner(name, step)))"),
        "Config is built straight into an Arc:\n{rust}",
    );
    assert!(rust.contains("// JUX-REP: arc\n"), "Config is marked `arc`:\n{rust}");
    assert!(rust.contains("// JUX-REP: arc-mutex\n"), "Tally is marked `arc-mutex`:\n{rust}");
}

/// Hierarchies and generics (§CR.3.4, §CR.3.5): a hierarchy takes the most
/// general representation any member needs, and a generic class one for all
/// its arguments.
#[test]
fn a_hierarchy_and_a_generic_class_each_take_one_representation() {
    let rust = emit(&example_case("cr_rep_hierarchy"));
    // Nothing writes a shape: the whole hierarchy is a plain `Rc`.
    for class in ["Circle", "Square"] {
        assert_handle(&rust, class, &format!("std::rc::Rc<{class}_Inner>"));
    }
    assert_eq!(rep_label(&rust, "Shape_Inner").as_deref(), Some("rc"), "Shape is `rc`:\n{rust}");
    // `Truck` is written, and pulls `Vehicle` up with it.
    assert_handle(&rust, "Truck", "std::rc::Rc<crate::JuxCell<Truck_Inner>>");
    assert_eq!(
        rep_label(&rust, "Vehicle_Inner").as_deref(),
        Some("rc-refcell"),
        "Vehicle is raised to `rc-refcell`:\n{rust}",
    );
    // `Slot<int>` is written; `Slot<String>` shares its representation.
    assert_eq!(
        rep_label(&rust, "Slot_Inner").as_deref(),
        Some("rc-refcell"),
        "Slot is `rc-refcell` for every argument:\n{rust}",
    );
}

/// The `// JUX-REP:` label written above the struct named `name` (§CR.9
/// Phase D): the nearest one before its declaration.
fn rep_label(rust: &str, name: &str) -> Option<String> {
    let at = rust.find(&format!("struct {name}"))?;
    let before = &rust[..at];
    let mark = before.rfind("// JUX-REP: ")? + "// JUX-REP: ".len();
    Some(before[mark..].lines().next()?.trim().to_string())
}
