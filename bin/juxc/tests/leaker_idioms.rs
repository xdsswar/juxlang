//! The shapes the idiomatic rewrite of the leaker app hit (LEAKS.md L30-L37):
//! a lambda handed to a container from a call whose other arguments read
//! fields, a field lent to a foreign builder that a later call consumes, a
//! lent `Ui` handed to a Jux method, a crate closure that takes `&str` and
//! returns an `Option`, a `static final` computed by a call, a collection lent
//! to a crate in a trailing `return`, and a crate method named like a Jux one.
//!
//! Like `borrowed_foreign.rs`, the program is built and RUN against a small
//! crate with egui's and printpdf's shapes, under the borrow self-check, since
//! half of what went wrong was a write that went into a copy.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

/// The crate the program links against.
const FAKEAPP_LIB: &str = r#"
#[derive(Debug, Default)]
pub struct Ui { lines: Vec<String>, depth: usize }

impl Ui {
    pub fn root() -> Ui { Ui::default() }
    pub fn label(&mut self, text: String) { let pad = "  ".repeat(self.depth); self.lines.push(format!("{pad}{text}")); }
    pub fn horizontal<R>(&mut self, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
        self.depth += 1;
        let r = add_contents(self);
        self.depth -= 1;
        r
    }
    /// egui's `ui.add(TextEdit::singleline(&mut text))`: the widget holds the
    /// caller's string and the call types into it.
    pub fn add(&mut self, w: TextEdit<'_>) { w.text.push_str(" (typed)"); let t = w.text.clone(); self.label(t); }
    /// egui's `ui.add(DragValue::new(&mut value))`.
    pub fn add_drag(&mut self, d: Drag<'_>) { *d.value += d.step; }
    pub fn dump(&self) -> String { self.lines.join("\n") }
}

pub struct TextEdit<'t> { text: &'t mut String, width: f32 }
impl<'t> TextEdit<'t> {
    pub fn singleline(text: &'t mut String) -> TextEdit<'t> { TextEdit { text, width: 0.0 } }
    pub fn desired_width(mut self, width: f32) -> Self { self.width = width; self }
}

pub struct Drag<'a> { value: &'a mut i64, step: i64 }
impl<'a> Drag<'a> {
    pub fn new(value: &'a mut i64) -> Drag<'a> { Drag { value, step: 1 } }
    pub fn step(mut self, step: i64) -> Self { self.step = step; self }
}

pub struct Panel;
impl Panel {
    pub fn new() -> Panel { Panel }
    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R { add_contents(ui) }
}

/// egui's `DragValue::custom_parser(impl Fn(&str) -> Option<f64>)`.
pub struct Parser;
impl Parser {
    pub fn new() -> Parser { Parser }
    pub fn parse_with(&self, text: String, parse: impl Fn(&str) -> Option<f64>) -> f64 { parse(&text).unwrap_or(-1.0) }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Color { Rgb(f32, f32, f32) }

/// eframe's `run_ui_native(.., impl FnMut(&mut Ui, ..))`: a crate FUNCTION
/// that lends its closure a `Ui`.
pub struct Frame;
pub fn run_app(f: impl FnOnce(&mut Ui, &mut Frame)) -> String { let mut ui = Ui::root(); f(&mut ui, &mut Frame); ui.dump() }

/// printpdf's `PdfDocument::save(&self, .., warnings: &mut Vec<PdfWarnMsg>)`.
pub struct Doc;
impl Doc {
    pub fn new() -> Doc { Doc }
    pub fn save(&self, warnings: &mut Vec<String>) -> Vec<u8> { warnings.push("checked".to_string()); vec![1, 2, 3] }
}
"#;

/// The stub bindgen writes for [`FAKEAPP_LIB`], declared into `rust.std` so
/// the program reaches it with no crate dependency of its own.
const FAKEAPP_STUB: &str = "\
package rust.std;

@rust(\"fakeapp::Ui\")
@RustDebug
@RustDefault
public class Ui {
    public static Ui root();
    @MutSelf public void label(String text);
    @MutSelf @RustClosureRefs(\"0\") public R horizontal<R>((Ui) -> R add_contents);
    @MutSelf public void add(TextEdit w);
    @MutSelf public void add_drag(Drag d);
    public String dump();
}

@rust(\"fakeapp::TextEdit\")
public class TextEdit {
    public static TextEdit singleline(&mut String text);
    public TextEdit desired_width(float width);
}

@rust(\"fakeapp::Drag\")
public class Drag {
    public Drag(&mut long value);
    public Drag step(long step);
}

@rust(\"fakeapp::Panel\")
public class Panel {
    public Panel();
    @RustClosureRefs(\"1\") public R show<R>(&mut Ui ui, (Ui) -> R add_contents);
}

@rust(\"fakeapp::Parser\")
public class Parser {
    public Parser();
    @RustClosureRefs(\"1\") @RustClosureShared(\"1:0\") public double parse_with(String text, (String) -> double? parse);
}

@rust(\"fakeapp::Color\")
@RustClone
@RustDebug
@RustPartialEq
public enum Color {
    Rgb(float, float, float)
}

@rust(\"fakeapp::run_app\")
@RustClosureRefs(\"0\") public String run_app((Ui, Frame) -> void f);

@rust(\"fakeapp::Frame\")
public class Frame {
}

@rust(\"fakeapp::Doc\")
public class Doc {
    public Doc();
    public Vec<ubyte> save(&mut Vec<String> warnings);
}
";

/// The program, one shape per finding, each named where it is exercised.
const PROGRAM: &str = r#"
import rust.std.Ui;
import rust.std.TextEdit;
import rust.std.Drag;
import rust.std.Panel;
import rust.std.Parser;
import rust.std.Color;
import rust.std.Doc;
import rust.std.run_app;

class Palette {
    public static Color rgb(int r, int g, int b) {
        return Color.Rgb((float) r, (float) g, (float) b);
    }
    // L31: a `static final` of a crate's enum computed by a call.
    public static final Color INK = Palette.rgb(1, 2, 3);
}

class Line {
    public long qty = 2;
}

class Form {
    public String name = "ann";
    public Line line = new Line();
    public String status = "ok";

    public void show(Ui ui) {
        // L30: the lambda reads a field, so the call's arguments are hoisted;
        // the lambda itself must stay at its slot for its `Ui` to be known.
        new Panel().show(ui, (panel) -> {
            panel.label($"form for ${name}");
            // L33: a field lent to a builder that `add` consumes, in place.
            panel.add(TextEdit.singleline(name).desired_width(100.0f));
            // L33: a field of another object lent to a crate constructor.
            panel.add_drag(new Drag(line.qty).step(3));
        });
    }

    // L34: the `Ui` a lambda is lent, handed to a Jux method by a call whose
    // other argument reads a field, and used again after.
    public void rows(Ui ui) {
        ui.horizontal((row) -> {
            tile(row, $"status ${status}");
            tile(row, $"name ${name}");
            row.label("after");
        });
    }

    private void tile(Ui ui, String text) {
        ui.label(text);
    }

    // L36, L37: a collection lent to the crate's `save` in the trailing
    // `return`, from a Jux method also called `save`.
    public Vec<ubyte> save() {
        var doc = new Doc();
        var warnings = new Vec<String>();
        return doc.save(warnings);
    }
}

public double? number(String text) {
    try {
        return text.parse<double>();
    } catch (Exception e) {
        return null;
    }
}

public void main() {
    var ui = Ui.root();
    var form = new Form();
    form.show(ui);
    form.rows(ui);
    print(ui.dump());
    print($"name=${form.name} qty=${form.line.qty}");
    // L35: the crate lends the closure a `&str` and wants an `Option` back.
    var parser = new Parser();
    print(parser.parse_with("12.5", (text) -> {
        var n = number(text);
        return n == null ? null : n!! * 2.0;
    }));
    print(parser.parse_with("x", (text) -> {
        var n = number(text);
        return n == null ? null : n!! * 2.0;
    }));
    print(Palette.INK == Color.Rgb(1.0f, 2.0f, 3.0f));
    print(form.save().len());
    // L38: the `Ui` a crate function's closure is lent, handed to a method of
    // a local object. `Frame` is not imported, as in a `run_ui_native` call.
    var other = new Form();
    print(run_app((app, frame) -> other.rows(app)));
}
"#;

const EXPECTED: &str = "\
form for ann
ann (typed)
  status ok
  name ann (typed)
  after
name=ann (typed) qty=5
25.0
-1.0
true
3
  status ok
  name ann
  after
";

/// A case directory: `main.jux`, the stubs (vendored `rust.std` plus
/// [`FAKEAPP_STUB`]) and the `fakeapp` crate.
fn case_dir(tag: &str) -> PathBuf {
    let root = workspace_root();
    let dir = root.join("target").join(format!("it-leaker-idioms-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("stubs")).expect("creating the case directory");
    std::fs::create_dir_all(dir.join("fakeapp").join("src")).expect("creating the fakeapp crate");
    std::fs::write(dir.join("main.jux"), PROGRAM).expect("writing main.jux");
    let vendored = root.join("crates").join("juxc-driver").join("stubs").join("rust-std.jux.d");
    std::fs::copy(&vendored, dir.join("stubs").join("rust-std.jux.d"))
        .expect("copying the vendored rust.std snapshot");
    std::fs::write(dir.join("stubs").join("fakeapp.jux.d"), FAKEAPP_STUB).expect("writing fakeapp.jux.d");
    std::fs::write(
        dir.join("fakeapp").join("Cargo.toml"),
        "[package]\nname = \"fakeapp\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n",
    )
    .expect("writing fakeapp/Cargo.toml");
    std::fs::write(dir.join("fakeapp").join("src").join("lib.rs"), FAKEAPP_LIB).expect("writing fakeapp/src/lib.rs");
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
    let manifest = crate_dir.join("Cargo.toml");
    let toml = std::fs::read_to_string(&manifest).expect("reading the emitted Cargo.toml");
    let fakeapp = dir.join("fakeapp").to_string_lossy().replace('\\', "/");
    let toml = toml.replacen("[dependencies]\n", &format!("[dependencies]\nfakeapp = {{ path = \"{fakeapp}\" }}\n"), 1);
    std::fs::write(&manifest, toml).expect("adding the fakeapp dependency");
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
fn the_lowering_lends_in_place_and_keeps_closures_at_their_slot() {
    let dir = case_dir("emit");
    let (_, rust) = emit(&dir);
    let flat = flat(&rust);
    // L30: no closure bound to an argument temporary.
    assert!(
        !rust.lines().any(|l| l.contains("let __jux_arg") && l.contains("move |")),
        "a lambda argument must be built at its slot:\n{rust}",
    );
    // L33: fields are lent mutably through their owner's cell, not copied.
    assert!(
        flat.contains("TextEdit::singleline(&mut __jux_this.0.borrow_mut().name)"),
        "the field must be lent to the builder in place:\n{rust}",
    );
    assert!(
        flat.contains("Drag::new( &mut __jux_this.0.borrow_mut().line.0.borrow_mut().qty")
            || flat.contains("Drag::new(&mut __jux_this.0.borrow_mut().line.0.borrow_mut().qty"),
        "the other object's field must be lent in place:\n{rust}",
    );
    // L34: the lent `Ui` is reborrowed into a temporary, never moved.
    assert!(
        !rust.lines().any(|l| l.contains("__jux_arg") && l.trim_end().ends_with("= row;")),
        "`row` must not be moved into an argument temporary:\n{rust}",
    );
    // L31: the call-initialized constant is computed at run time.
    assert!(!flat.contains("const INK"), "`INK` cannot be a Rust `const`:\n{rust}");
}
