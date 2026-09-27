//! Borrowed foreign objects: closures a crate lends them to, parameters that
//! carry them, fields lent to `&mut` slots and writes through `&mut`
//! accessors (LEAKS.md L1, L8, L9, L10, L12, L14, L15; GAPS.md gap 30; ERRATA
//! E127).
//!
//! egui is the crate that found these, and it is too heavy to build in a test,
//! so a small crate written here, `fakeui`, has the same shapes: a `Ui` with no
//! `Clone` that containers lend to a closure as `&mut Ui`, a `spacing_mut()`
//! that lends its insides, a text field that writes through `&mut String`, and
//! a picker that writes through `&mut usize` while calling a closure. Its stub
//! is the one bindgen writes for such a crate, markers included.
//!
//! The program is BUILT and RUN against that crate, because what these
//! findings got wrong was behaviour: the rustc errors were the loud half, and
//! the writes that went into copies were the quiet one. It also runs under the
//! borrow self-check (ERRATA E119), so a lowering that keeps a cell borrowed
//! across the crate's call fails here.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

/// The crate the program links against. Every signature is the shape egui has.
const FAKEUI_LIB: &str = r#"
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 { pub x: f32, pub y: f32 }
impl Vec2 { pub fn new(x: f32, y: f32) -> Vec2 { Vec2 { x, y } } }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spacing { pub indent: f32, pub item_spacing: Vec2 }

/// No `Clone`: egui's `Ui` is only ever lent.
#[derive(Debug, Default)]
pub struct Ui { lines: Vec<String>, spacing: Spacing, depth: usize }

impl Ui {
    pub fn root() -> Ui { Ui::default() }
    pub fn label(&mut self, text: String) { let pad = "  ".repeat(self.depth); self.lines.push(format!("{pad}{text}")); }
    pub fn width(&self) -> f32 { 100.0 - 10.0 * self.depth as f32 }
    pub fn horizontal<R>(&mut self, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
        self.depth += 1;
        let r = add_contents(self);
        self.depth -= 1;
        r
    }
    pub fn spacing(&self) -> &Spacing { &self.spacing }
    pub fn spacing_mut(&mut self) -> &mut Spacing { &mut self.spacing }
    pub fn text_edit(&mut self, text: &mut String) { text.push_str(" (edited)"); }
    pub fn dump(&self) -> String { self.lines.join("\n") }
}

pub struct Panel;
impl Panel {
    pub fn new() -> Panel { Panel }
    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R { add_contents(ui) }
}

pub struct Picker;
impl Picker {
    pub fn new() -> Picker { Picker }
    pub fn show_index<T: Into<String>>(self, ui: &mut Ui, selected: &mut usize, len: usize, get: impl Fn(usize) -> T) {
        for i in 0..len { ui.label(get(i).into()); }
        *selected = len - 1;
    }
}
"#;

/// The stub bindgen writes for [`FAKEUI_LIB`], declared into `rust.std` so the
/// program reaches it with no crate dependency of its own.
const FAKEUI_STUB: &str = "\
package rust.std;

@rust(\"fakeui::Vec2\")
@RustClone
@RustDebug
@RustPartialEq
@RustDefault
public class Vec2 {
    public float x;
    public float y;
    public Vec2(float x, float y);
}

@rust(\"fakeui::Spacing\")
@RustClone
@RustDebug
@RustPartialEq
@RustDefault
public class Spacing {
    public float indent;
    public Vec2 item_spacing;
}

@rust(\"fakeui::Ui\")
@RustDebug
@RustDefault
public class Ui {
    public static Ui root();
    @MutSelf public void label(String text);
    public float width();
    @MutSelf @RustClosureRefs(\"0\") public R horizontal<R>((Ui) -> R add_contents);
    @RustRefOut public Spacing spacing();
    @MutSelf @RustRefOut public Spacing spacing_mut();
    @MutSelf public void text_edit(&mut String text);
    public String dump();
}

@rust(\"fakeui::Panel\")
public class Panel {
    public Panel();
    @RustClosureRefs(\"1\") public R show<R>(&mut Ui ui, (Ui) -> R add_contents);
}

@rust(\"fakeui::Picker\")
public class Picker {
    public Picker();
    public void show_index<T>(&mut Ui ui, &mut uint selected, uint len, (uint) -> T get);
}
";

/// The program, one shape per finding, each named where it is exercised.
const PROGRAM: &str = r#"
import rust.std.Ui;
import rust.std.Panel;
import rust.std.Picker;
import rust.std.Vec2;

class Counter {
    public int n = 0;
    public void bump() { n = n + 1; }
}

class Gui {
    // L9: `&self` methods only, and handed on. Before, this was `Ui` by value.
    public static void caption(Ui ui, String text) {
        line(ui, $"== ${text} ==");
    }
    public static void line(Ui ui, String text) {
        ui.label(text);
    }
    public static float width(Ui ui) {
        return ui.width();
    }
    // A local naming the borrow is the same borrow.
    public static void aliased(Ui ui, String text) {
        var u = ui;
        u.label(text);
    }
}

class Form {
    public String name = "ann";
    public uint choice = 0;
    public Vec<String> options = new Vec<String>();
    public float gap = 0.0f;

    // L8: a field lent to a `&mut` slot, edited in place.
    public void edit(Ui ui) {
        ui.text_edit(this.name);
    }
    // L8 + L12: a field lent while a closure runs; the closure reads a
    // collection that another argument also reads.
    public void pick(Ui ui) {
        new Picker().show_index(ui, this.choice, options.len(), (i) -> options[i]);
    }
    // L14: arguments read fields, twice with the same `ui`.
    public void fields(Ui ui) {
        Gui.line(ui, this.name);
        Gui.line(ui, $"${this.options.len()} options");
    }
    // L10: a write through a `&mut` accessor, direct and through a local.
    public void layout(Ui ui) {
        ui.spacing_mut().item_spacing = new Vec2(3.0f, this.gap);
        var s = ui.spacing_mut();
        s.indent = 12.0f;
    }
}

// L12: the closure captures a local that another argument also reads, and
// nothing reads it afterwards.
public uint pickLocal(Ui ui) {
    var names = new Vec<String>();
    names.push("x");
    names.push("y");
    names.push("z");
    uint s = 0;
    new Picker().show_index(ui, s, names.len(), (i) -> names[i]);
    return s;
}

// L15: a String read in one arm of a ternary, then used again.
public String wrap(String text, int width) {
    String out = "";
    String line = "";
    for (var word : text.split(" ")) {
        String candidate = line.length() == 0 ? word : line + " " + word;
        if (candidate.length() > width) {
            out = out + line + "|";
            line = word;
        } else {
            line = candidate;
        }
    }
    return out + line;
}

public void main() {
    var ui = Ui.root();
    var counter = new Counter();
    var form = new Form();
    form.options.push("red");
    form.options.push("green");
    form.gap = 4.0f;
    var seen = new Vec<String>();
    float inner = 0.0f;
    // L1: the lambda gets the `&mut Ui` itself, and passes it on.
    new Panel().show(ui, (panel) -> {
        Gui.caption(panel, "panel");
        panel.label("top");
        panel.horizontal((row) -> {
            row.label("in a row");
            Gui.line(row, "helper in a row");
            Gui.aliased(row, "through an alias");
            inner = Gui.width(row);
            counter.bump();
            seen.push("row");
        });
        form.edit(panel);
        form.pick(panel);
        form.fields(panel);
        form.layout(panel);
        counter.bump();
    });
    uint picked = pickLocal(ui);
    print(ui.dump());
    print($"counter=${counter.n} seen=${seen.len()} inner=${inner}");
    print($"name=${form.name} choice=${form.choice} picked=${picked}");
    print($"spacing=${ui.spacing().item_spacing.y} indent=${ui.spacing().indent}");
    print(wrap("the quick brown fox jumps over the lazy dog", 12));
}
"#;

const EXPECTED: &str = "\
== panel ==
top
  in a row
  helper in a row
  through an alias
red
green
ann (edited)
2 options
x
y
z
counter=2 seen=1 inner=90.0
name=ann (edited) choice=1 picked=2
spacing=4.0 indent=12.0
the quick|brown fox|jumps over|the lazy dog
";

/// A case directory: `main.jux`, the stubs (vendored `rust.std` plus
/// [`FAKEUI_STUB`]) and the `fakeui` crate.
fn case_dir(tag: &str) -> PathBuf {
    let root = workspace_root();
    let dir = root.join("target").join(format!("it-borrowed-foreign-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("stubs")).expect("creating the case directory");
    std::fs::create_dir_all(dir.join("fakeui").join("src")).expect("creating the fakeui crate");
    std::fs::write(dir.join("main.jux"), PROGRAM).expect("writing main.jux");
    let vendored = root.join("crates").join("juxc-driver").join("stubs").join("rust-std.jux.d");
    std::fs::copy(&vendored, dir.join("stubs").join("rust-std.jux.d"))
        .expect("copying the vendored rust.std snapshot");
    std::fs::write(dir.join("stubs").join("fakeui.jux.d"), FAKEUI_STUB).expect("writing fakeui.jux.d");
    std::fs::write(
        dir.join("fakeui").join("Cargo.toml"),
        "[package]\nname = \"fakeui\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n",
    )
    .expect("writing fakeui/Cargo.toml");
    std::fs::write(dir.join("fakeui").join("src").join("lib.rs"), FAKEUI_LIB).expect("writing fakeui/src/lib.rs");
    dir
}

/// Lower the program under the borrow self-check and hand back the emitted
/// crate's directory and `main.rs`.
fn emit(dir: &Path) -> (PathBuf, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg(dir.join("main.jux"))
        .env("JUX_STUBS_DIR", dir.join("stubs"))
        .env("JUX_SELFCHECK", "1")
        .output()
        .expect("running juxc");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "juxc refused the program:\n{said}");
    let crate_dir = dir.join("target").join(".rust-build");
    let rust = std::fs::read_to_string(crate_dir.join("src").join("main.rs"))
        .unwrap_or_else(|e| panic!("reading the emitted main.rs: {e}\njuxc said:\n{said}"));
    (crate_dir, rust)
}

/// The emitted Rust with whitespace collapsed, so a check does not depend on
/// where rustfmt breaks a line.
fn flat(rust: &str) -> String {
    rust.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_program_runs_against_the_crate_and_every_write_lands() {
    let dir = case_dir("run");
    let (crate_dir, _) = emit(&dir);
    // The emitted crate depends on `fakeui` by path, as a `jux.toml`
    // dependency on a local crate would make it.
    let manifest = crate_dir.join("Cargo.toml");
    let toml = std::fs::read_to_string(&manifest).expect("reading the emitted Cargo.toml");
    let fakeui = dir.join("fakeui").to_string_lossy().replace('\\', "/");
    let toml = toml.replacen("[dependencies]\n", &format!("[dependencies]\nfakeui = {{ path = \"{fakeui}\" }}\n"), 1);
    std::fs::write(&manifest, toml).expect("adding the fakeui dependency");
    let out = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .env_remove("RUSTFLAGS")
        .output()
        .expect("running cargo");
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    assert!(
        out.status.success(),
        "the emitted crate did not build or run:\n{}\n{stdout}",
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn the_lowering_lends_rather_than_copies() {
    let dir = case_dir("emit");
    let (_, rust) = emit(&dir);
    let flat = flat(&rust);
    // L1: no clone of a lent foreign object at the top of a closure body.
    for name in ["panel", "row"] {
        assert!(
            !flat.contains(&format!("let {name} = {name}.clone();")),
            "the closure parameter `{name}` must be the lent `&mut Ui`, not a clone:\n{rust}",
        );
    }
    // L9: a `Ui` parameter is always a borrow, whatever the body calls: an
    // exclusive one where the body writes through it or lends it to a slot
    // that does, a shared one where it only reads, never a by-value `Ui`.
    for sig in [
        "pub fn caption(ui: &mut fakeui::Ui, text: String)",
        "pub fn line(ui: &mut fakeui::Ui, text: String)",
        "pub fn width(ui: &fakeui::Ui) -> f32",
    ] {
        assert!(flat.contains(sig), "expected `{sig}` in:\n{rust}");
    }
    // L8: the field is lent through its owner's cell.
    assert!(
        flat.contains("ui.text_edit(&mut self.0.borrow_mut().name)"),
        "the field must be lent in place:\n{rust}",
    );
    // L10: the accessor's `&mut` is written through, not copied.
    assert!(
        !flat.contains("spacing_mut()).clone()") && !flat.contains("spacing_mut().clone()"),
        "a write through `spacing_mut()` must not go into a copy:\n{rust}",
    );
    // L14: a borrowed `ui` is reborrowed, never moved into a temporary.
    assert!(
        !rust.lines().any(|l| l.contains("__jux_arg") && l.trim_end().ends_with("= ui;")),
        "`ui` must not be moved into an argument temporary:\n{rust}",
    );
}
