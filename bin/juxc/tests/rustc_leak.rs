//! A build that fails after the front end accepted the program is reported as
//! Jux diagnostics, not as cargo's text (ERRATA E116, gaps 15 and 27).
//!
//! The rustc half is fed canned cargo JSON: getting a real rustc error out of
//! the backend on purpose would need a backend bug to keep, and the point is
//! the translation, not the bug. The target and linkage halves run the real
//! `juxc` binary, because they stop before cargo does.

use std::path::{Path, PathBuf};
use std::process::Command;

use juxc_diagnostics::code::Code;
use juxc_driver::render::{render_json, render_text, DiagnosticFormat};

/// A scratch directory under the workspace's `target/`, emptied first.
fn scratch(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("target")
        .join(format!("it-rustc-leak-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creating scratch dir");
    dir
}

/// One cargo `compiler-message` line, the shape `--message-format=json` writes.
fn compiler_message(code: Option<&str>, message: &str, file: &str, line: u32, col: u32) -> String {
    let code = match code {
        Some(c) => format!("{{\"code\":\"{c}\",\"explanation\":null}}"),
        None => "null".to_string(),
    };
    format!(
        "{{\"reason\":\"compiler-message\",\"message\":{{\"rendered\":\"error: {message}\\n\",\"children\":[],\
         \"code\":{code},\"level\":\"error\",\"message\":\"{message}\",\"spans\":[{{\"file_name\":\"{file}\",\
         \"is_primary\":true,\"line_start\":{line},\"column_start\":{col}}}]}}}}"
    )
}

/// A `.jux` file on disk and the Rust the backend "emitted" for it, with a
/// `// JUX:` marker naming the file's line 3.
fn program(dir: &Path) -> (PathBuf, String) {
    let jux = dir.join("leak.jux");
    std::fs::write(&jux, "public void main() {\n    var a = spawn(() -> 1);\n    print(a.blockingGet());\n}\n").unwrap();
    let path = jux.display().to_string().replace('\\', "/");
    let rust = format!("fn main() {{\n// JUX:{path}:3:5\n    let b = a;\n    use_it(a);\n}}\n");
    (jux, rust)
}

#[test]
fn a_borrow_error_is_an_e0900_ice_at_the_jux_line() {
    let dir = scratch("borrow");
    let (_jux, rust) = program(&dir);
    let json = compiler_message(Some("E0502"), "cannot borrow `x` as mutable because it is also borrowed as immutable", "src\\\\main.rs", 4, 5);
    let failure = juxc_driver::build_failure::from_cargo_messages(&json, &[("src/main.rs", &rust)]).expect("a failure");
    assert_eq!(failure.exit_code, 101, "an ICE exits the way a panicking compiler does");
    let d = &failure.diagnostics[0];
    assert_eq!(d.code, Code::E0900_BackendEmittedInvalidRust);

    let human = render_text(&failure.diagnostics, &failure.sources, DiagnosticFormat::Human, false);
    assert!(human.starts_with("error[E0900]: internal compiler error: object borrowed twice\n"), "{human}");
    assert!(human.contains("leak.jux:3:5"), "{human}");
    assert!(human.contains("print(a.blockingGet());"), "the Jux line is shown, not the Rust one: {human}");
    assert!(human.contains("note: rustc reported error[E0502]"), "{human}");
    assert!(human.contains("this is a bug in the Jux compiler"), "{human}");
    assert!(human.contains("ERRATA E23"), "{human}");
    assert!(human.contains("/issues"), "{human}");

    let json = render_json(&failure.diagnostics, &failure.sources, 0);
    let first = json.lines().next().unwrap();
    assert!(first.starts_with("{\"code\":\"E0900\",\"severity\":\"error\""), "{first}");
    assert!(first.contains("\"line_start\":3"), "{first}");
}

/// LEAKS L27: the one-line formats (`line` is the default off a terminal)
/// carry the rustc error that caused an E0900, not only "does not compile",
/// and keep the Jux location; `--verbose` stays the way to the full report.
#[test]
fn the_one_line_formats_name_the_rustc_error() {
    let dir = scratch("oneline");
    let (_jux, rust) = program(&dir);
    let json = compiler_message(Some("E0599"), "no method named `borrow` found for struct `String` in the current scope", "src\\\\main.rs", 4, 5);
    let failure = juxc_driver::build_failure::from_cargo_messages(&json, &[("src/main.rs", &rust)]).expect("a failure");
    for format in [DiagnosticFormat::Line, DiagnosticFormat::Short, DiagnosticFormat::Compact] {
        let text = render_text(&failure.diagnostics, &failure.sources, format, false);
        let first = text.lines().next().unwrap_or_default();
        assert!(first.contains("leak.jux:3:5"), "{format:?}: {first}");
        assert!(first.contains("internal compiler error: the Rust generated for this code does not compile"), "{format:?}: {first}");
        assert!(
            first.contains("rustc reported error[E0599]: no method named `borrow` found for struct `String`"),
            "{format:?}: {first}"
        );
        assert!(first.contains("`--verbose` shows the full report"), "{format:?}: {first}");
        assert!(!first.contains("main.rs"), "the generated file stays out of the one line: {first}");
    }
    let human = render_text(&failure.diagnostics, &failure.sources, DiagnosticFormat::Human, false);
    assert!(human.contains("note: rustc's error is at src"), "{human}");
}

#[test]
fn each_borrow_family_gets_its_jux_words() {
    for (code, words) in [
        ("E0382", "value used after it was moved"),
        ("E0505", "value used after it was moved"),
        ("E0499", "object borrowed twice"),
        ("E0597", "temporary dropped while in use"),
        ("E0716", "temporary dropped while in use"),
        ("E0308", "the Rust generated for this code does not compile"),
    ] {
        let json = compiler_message(Some(code), "x", "src/main.rs", 1, 1);
        let failure = juxc_driver::build_failure::from_cargo_messages(&json, &[]).unwrap();
        assert_eq!(failure.diagnostics[0].message, format!("internal compiler error: {words}"), "{code}");
    }
}

#[test]
fn a_link_failure_is_e0906_with_one_line() {
    let json = "{\"reason\":\"compiler-message\",\"message\":{\"rendered\":\"error: linking with `cc` failed\\n\",\
                \"children\":[{\"level\":\"note\",\"message\":\"\\\"cc\\\" \\\"-m64\\\" \\\"-lnosuch\\\"\"},\
                {\"level\":\"note\",\"message\":\"/usr/bin/ld: cannot find -lnosuch: No such file or directory\\ncollect2: error: ld returned 1 exit status\\n\"}],\
                \"code\":null,\"level\":\"error\",\"message\":\"linking with `cc` failed: exit status: 1\",\"spans\":[]}}";
    let failure = juxc_driver::build_failure::from_cargo_messages(json, &[]).expect("a failure");
    assert_eq!(failure.exit_code, 1, "a missing library is not a compiler bug");
    let human = render_text(&failure.diagnostics, &failure.sources, DiagnosticFormat::Human, false);
    assert!(human.starts_with("error[E0906]: could not link library `nosuch`\n"), "{human}");
    assert!(human.contains("the linker said: /usr/bin/ld: cannot find -lnosuch"), "{human}");
    assert!(!human.contains("-m64"), "the command line stays behind --verbose: {human}");
    assert!(failure.detail.contains("linking with"), "--verbose still has it");
    let json = render_json(&failure.diagnostics, &failure.sources, 0);
    assert!(json.starts_with("{\"code\":\"E0906\""), "{json}");
}

fn juxc(args: &[&str], dir: &Path, target: Option<&str>) -> (String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_juxc"));
    cmd.args(args).current_dir(dir);
    match target {
        Some(t) => cmd.env("JUX_TARGET", t),
        None => cmd.env_remove("JUX_TARGET"),
    };
    let out = cmd.output().expect("running juxc");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (text, out.status.code().unwrap_or(-1))
}

/// An unknown `--target` is refused before cargo runs, as one diagnostic.
#[test]
fn an_unknown_target_is_e0904_not_a_flood_of_e0463() {
    let dir = scratch("target");
    std::fs::write(dir.join("hello.jux"), "public void main() {\n    print(1);\n}\n").unwrap();
    let (out, code) = juxc(&["--build", "hello.jux"], &dir, Some("nonexistent-triple"));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("[E0904] error: `nonexistent-triple` is not a target Jux can build for"), "{out}");
    assert!(!out.contains("E0463"), "{out}");

    let (json, _) = juxc(&["--build", "--diagnostic-format", "json", "hello.jux"], &dir, Some("nonexistent-triple"));
    assert!(json.contains("{\"code\":\"E0904\",\"severity\":\"error\""), "{json}");
}

/// `linkage = "framework"` is Apple's; anywhere else it is refused up front,
/// and an entry no `@extern` uses is a warning and is not linked.
#[test]
fn a_framework_off_apple_is_e0908_and_an_unused_entry_is_w0906() {
    if cfg!(target_vendor = "apple") {
        return;
    }
    let dir = scratch("framework");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("jux.toml"),
        "[package]\nname = \"frame\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n\
         [ffi.Cocoa]\nlinkage = \"framework\"\n\n[ffi.Metal]\nlinkage = \"framework\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src").join("main.jux"),
        "@extern(lib = \"Cocoa\")\nunsafe native {\n    int NSApplicationLoad();\n}\n\n\
         public void main() {\n    print(1);\n}\n",
    )
    .unwrap();
    let (out, code) = juxc(&["--build", "src/main.jux"], &dir, None);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("[W0906] warning: `[ffi.Metal]` in jux.toml is not linked"), "{out}");
    assert!(out.contains("[E0908] error: `[ffi.Cocoa]` asks for `linkage = \"framework\"`"), "{out}");
}
