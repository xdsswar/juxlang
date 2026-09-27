//! What a program may pass to a crate, and the Rust each call lowers to
//! (Bindgen §G.3.6, §G.3.7, ERRATA E128, LEAKS L2-L21).
//!
//! The stub below is the one bindgen writes for
//! `crates/juxc-bindgen/tests/fixtures/leaks-src/lib.rs`, with its paths
//! pointed at a crate named `probe`. Nothing is compiled by rustc: each case
//! reads the emitted `main.rs`, or the checker's verdict, and stops there.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

const PROBE_STUB: &str = r#"
package rust.probe;

@rust("probe::AsKey")
@RustBlanket("Hash + Debug")
public interface AsKey {
}

@rust("probe::Buffer")
@RustImplementedBy("String")
public interface Buffer {
    public String text();
}

@rust("probe::Color")
@RustClone
@RustDebug
@RustPartialEq
public class Color {
    public ubyte r;
    public static final Color RED;
    public Color(ubyte r);
}

@rust("probe::IntoAtom")
@RustBlanket("Into<Label>")
public interface IntoAtom {
}

@rust("probe::Label")
@RustClone
@RustDebug
@RustPartialEq
@RustFrom("Rich,String")
@RustHash
public class Label implements AsKey, IntoAtom {
}

@rust("probe::Mm")
@RustClone
@RustDebug
@RustPartialEq
@RustDefault
@RustTuple
public class Mm {
    public float _0;
    @RustTuple public Mm(float _0);
    @RustDefault public Mm();
}

@rust("probe::Op")
@RustClone
@RustDebug
@RustStructVariants("Fill:col;Move:x,y")
public enum Op {
    Fill(Color), Move(Mm, Mm), End
}

@rust("probe::Panel")
@RustDefault
public class Panel {
    @RustDefault public Panel();
    @MutSelf public void label(@RustImpl Label text);
    @MutSelf public void atom(@RustImpl IntoAtom a);
    @MutSelf public void edit(@RustImpl &mut Buffer b);
    @MutSelf public void key(@RustImpl AsKey k);
    @RustRefOut @RustDerefOut public Style style();
    @MutSelf public void set_style(@RustArc Style s);
    @RustClosureRefs("0") @RustClosureShared("0:0") public int read((Style) -> int f);
}

@rust("probe::Rich")
@RustClone
@RustDebug
@RustPartialEq
@RustTuple
public class Rich {
    public String _0;
    @RustTuple public Rich(String _0);
}

@rust("probe::Style")
@RustClone
@RustDebug
@RustDefault
public class Style {
    public bool bold;
    @RustDefault public Style();
    @MutSelf public void set_bold();
}

@rust("probe::Text")
@RustClone
@RustDebug
public enum Text {
    Plain(String), Shared(@RustArc Rich)
}
"#;

const IMPORTS: &str = "import rust.probe.*;\n";

fn case_dir(tag: &str, program: &str) -> PathBuf {
    let root = workspace_root();
    let dir = root.join("target").join(format!("it-type-mapping-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("stubs")).expect("creating the case directory");
    std::fs::write(dir.join("main.jux"), format!("{IMPORTS}{program}")).expect("writing main.jux");
    let vendored = root.join("crates").join("juxc-driver").join("stubs").join("rust-std.jux.d");
    std::fs::copy(&vendored, dir.join("stubs").join("rust-std.jux.d")).expect("copying rust.std");
    std::fs::write(dir.join("stubs").join("probe.jux.d"), PROBE_STUB).expect("writing probe.jux.d");
    dir
}

fn run(dir: &Path) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg(dir.join("main.jux"))
        .env("JUX_STUBS_DIR", dir.join("stubs"))
        .output()
        .expect("running juxc");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), said)
}

fn emit(tag: &str, program: &str) -> String {
    let dir = case_dir(tag, program);
    let (ok, said) = run(&dir);
    assert!(ok, "juxc refused the program:\n{said}");
    let emitted = dir.join("target").join(".rust-build").join("src").join("main.rs");
    let rust = std::fs::read_to_string(&emitted).unwrap_or_else(|e| panic!("reading {}: {e}\n{said}", emitted.display()));
    // The program's own `main`, without the prelude around it.
    let from = rust.find("pub fn __jux_user_main()").expect("the entry is emitted");
    let to = rust[from..].find("\n}\n").map_or(rust.len(), |i| from + i + 3);
    rust[from..to].to_string()
}

fn refused(tag: &str, program: &str) -> String {
    let dir = case_dir(tag, program);
    let (ok, said) = run(&dir);
    assert!(!ok, "juxc accepted a program it must refuse:\n{said}");
    said
}

fn assert_has(text: &str, want: &str) {
    assert!(text.contains(want), "expected `{want}` in the emitted Rust:\n{text}");
}

/// L2/L3/L4/L13: a String fills an `impl Into<Label>`, an `impl IntoAtom`
/// (blanket over `Into<Label>`), an `impl AsKey` (blanket over `Hash +
/// Debug`) and a `&mut dyn Buffer` (implemented for `String`), and each is
/// passed as it is.
#[test]
fn text_fills_every_generic_slot_that_takes_it() {
    let rust = emit(
        "text",
        "public void main() {\n\
         \x20   var p = new Panel();\n\
         \x20   String s = \"hello\";\n\
         \x20   p.label(s);\n\
         \x20   p.atom(\"atom\");\n\
         \x20   p.key(\"nav\");\n\
         \x20   String buf = \"edit me\";\n\
         \x20   p.edit(buf);\n\
         \x20   p.label(new Rich(\"rich\"));\n\
         }\n",
    );
    assert_has(&rust, ".label(s");
    assert_has(&rust, ".atom(\"atom\"");
    assert_has(&rust, ".key(\"nav\"");
    assert_has(&rust, ".edit(&mut buf)");
    assert_has(&rust, ".label(probe::Rich(\"rich\"");
    assert!(!rust.contains("Box<dyn"), "a generic slot is never a boxed trait object:\n{rust}");
    assert!(!rust.contains("crate::rust::"), "no path through the stub package:\n{rust}");
}

/// A value that converts into nothing the slot takes is a Jux error.
#[test]
fn a_value_the_slot_cannot_take_is_refused_in_jux_terms() {
    let said = refused(
        "refused",
        "public void main() {\n\
         \x20   var p = new Panel();\n\
         \x20   p.label(new Mm(1.0f));\n\
         }\n",
    );
    assert!(said.contains("E0410"), "{said}");
    assert!(!said.contains("E0900"), "{said}");
}

/// L11/L16/L17/L19: constants, tuple structs, named-field variants and erased
/// `Arc`s lower to the Rust the crate takes.
#[test]
fn constants_tuples_variants_and_shared_pointers_lower() {
    let rust = emit(
        "shapes",
        "public void main() {\n\
         \x20   var p = new Panel();\n\
         \x20   var red = Color.RED;\n\
         \x20   var w = new Mm(210.0f);\n\
         \x20   float x = w._0;\n\
         \x20   var fill = Op.Fill(red);\n\
         \x20   var mv = Op.Move(w, new Mm(3.0f));\n\
         \x20   var t = Text.Shared(new Rich(\"big\"));\n\
         \x20   var style = p.style();\n\
         \x20   style.bold = true;\n\
         \x20   p.set_style(style);\n\
         \x20   print(x);\n\
         }\n",
    );
    assert_has(&rust, "probe::Color::RED");
    assert_has(&rust, "probe::Mm(210.0f32)");
    assert_has(&rust, "(w).0");
    assert_has(&rust, "Op::Fill { col: ");
    assert_has(&rust, "Op::Move {");
    assert_has(&rust, "x: w,");
    assert_has(&rust, "Text::Shared((probe::Rich(");
    assert_has(&rust, ").into())");
    assert_has(&rust, "<probe::Style as ::std::clone::Clone>::clone(&*(");
}

/// Gap 30's follow-up: a lambda may not write through an argument the crate
/// lends it read-only, and hears so from the checker.
#[test]
fn writing_through_a_read_only_lent_argument_is_a_jux_error() {
    let said = refused(
        "shared",
        "public void main() {\n\
         \x20   var p = new Panel();\n\
         \x20   var n = p.read((s) -> {\n\
         \x20       s.bold = true;\n\
         \x20       return 1;\n\
         \x20   });\n\
         \x20   print(n);\n\
         }\n",
    );
    assert!(said.contains("E0488"), "{said}");
    let said = refused(
        "shared-call",
        "public void main() {\n\
         \x20   var p = new Panel();\n\
         \x20   var n = p.read((s) -> {\n\
         \x20       s.set_bold();\n\
         \x20       return 1;\n\
         \x20   });\n\
         \x20   print(n);\n\
         }\n",
    );
    assert!(said.contains("E0488"), "{said}");
    // Reading is fine.
    emit(
        "shared-read",
        "public void main() {\n\
         \x20   var p = new Panel();\n\
         \x20   var n = p.read((s) -> s.bold ? 1 : 0);\n\
         \x20   print(n);\n\
         }\n",
    );
}
