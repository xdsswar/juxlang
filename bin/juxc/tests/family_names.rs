//! Two types of one simple name in a crate family (LEAKS L29, gap 37), and a
//! crate's deprecated method (LEAKS L39).
//!
//! `eframe::Frame` and `egui::Frame` are both `Frame`. The family stub keeps
//! the host's as `rust.eframe.Frame` and declares the member's in the nested
//! package its Rust path names, `rust.eframe.egui.Frame`; every signature
//! names the exact one. The fixture is a two-crate family in that shape
//! (`crates/juxc-bindgen/tests/fixtures/family-src`), whose stubs are
//! generated here from its checked-in rustdoc JSON exactly as the driver
//! generates a dependency's. The program is then checked, and built and RUN
//! against the real crates.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

fn fixtures() -> PathBuf {
    workspace_root().join("crates").join("juxc-bindgen").join("tests").join("fixtures")
}

/// The family stub and its nested packages, rendered as the driver writes
/// them: `(file name, text)`.
fn family_stub_files() -> Vec<(String, String)> {
    let host = std::fs::read_to_string(fixtures().join("g37app.rustdoc.json")).expect("host JSON");
    let member = std::fs::read_to_string(fixtures().join("g37ui.rustdoc.json")).expect("member JSON");
    let jsons = [("g37app", host.as_str()), ("g37ui", member.as_str())];
    let fam = juxc_bindgen::family::FamilyPaths::read("g37app", &jsons).expect("the family reads");
    let mut stub = juxc_bindgen::ingest::generate_family(
        &jsons,
        "rust.g37app",
        &fam,
        &std::collections::HashMap::new(),
    )
    .expect("the family ingests");
    juxc_bindgen::ingest::rewrite_reexported_paths(&mut stub, "g37app", &host).expect("re-exports read");
    let mut files = vec![("g37app.jux.d".to_string(), juxc_bindgen::render_stub(&stub))];
    for nested in &stub.nested {
        files.push((format!("{}.jux.d", nested.package), juxc_bindgen::render_stub(nested)));
    }
    files
}

/// A case directory: `main.jux`, the stubs (vendored `rust.std` plus the
/// family's), and the two crates.
fn case_dir(tag: &str, program: &str) -> PathBuf {
    let root = workspace_root();
    let dir = root.join("target").join(format!("it-family-names-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("stubs")).expect("creating the case directory");
    std::fs::write(dir.join("main.jux"), program).expect("writing main.jux");
    let vendored = root.join("crates").join("juxc-driver").join("stubs").join("rust-std.jux.d");
    std::fs::copy(&vendored, dir.join("stubs").join("rust-std.jux.d"))
        .expect("copying the vendored rust.std snapshot");
    for (name, text) in family_stub_files() {
        std::fs::write(dir.join("stubs").join(name), text).expect("writing a family stub");
    }
    for (krate, deps) in [("g37ui", ""), ("g37app", "[dependencies]\ng37ui = { path = \"../g37ui\" }\n")] {
        let src = dir.join(krate).join("src");
        std::fs::create_dir_all(&src).expect("creating a fixture crate");
        std::fs::write(
            dir.join(krate).join("Cargo.toml"),
            format!(
                "[package]\nname = \"{krate}\"\nversion = \"0.2.0\"\nedition = \"2021\"\npublish = false\n\n\
                 [workspace]\n\n{deps}"
            ),
        )
        .expect("writing a fixture Cargo.toml");
        let lib = std::fs::read_to_string(fixtures().join("family-src").join(krate).join("lib.rs"))
            .expect("reading a fixture crate");
        std::fs::write(src.join("lib.rs"), lib).expect("writing a fixture crate");
    }
    dir
}

/// `juxc` on the case, with its stubs: `(succeeded, what it said)`.
fn juxc(dir: &Path, check_only: bool) -> (bool, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_juxc"));
    if check_only {
        cmd.arg("--check");
    }
    let out = cmd
        .arg(dir.join("main.jux"))
        .env("JUX_STUBS_DIR", dir.join("stubs"))
        .env("JUX_SELFCHECK", "1")
        .output()
        .expect("running juxc");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
        .replace("\r\n", "\n");
    (out.status.success(), said)
}

/// Lower the program, link the emitted crate against the fixture family and
/// run it: its standard output.
fn build_and_run(dir: &Path) -> String {
    let (ok, said) = juxc(dir, false);
    assert!(ok, "juxc refused the program:\n{said}");
    let crate_dir = dir.join("target").join(".rust-build");
    let manifest = crate_dir.join("Cargo.toml");
    let toml = std::fs::read_to_string(&manifest).expect("reading the emitted Cargo.toml");
    let app = dir.join("g37app").to_string_lossy().replace('\\', "/");
    let toml = toml.replacen("[dependencies]\n", &format!("[dependencies]\ng37app = {{ path = \"{app}\" }}\n"), 1);
    std::fs::write(&manifest, toml).expect("adding the g37app dependency");
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
    stdout
}

/// The member's `Frame`, imported by its nested name, fills a panel; the
/// host's `Frame` is still what `run` lends its closure.
#[test]
fn both_frames_are_nameable_and_each_signature_takes_its_own() {
    let dir = case_dir(
        "run",
        r#"
import rust.g37app.Ui;
import rust.g37app.Panel;
import rust.g37app.Color;
import rust.g37app.run;
import rust.g37app.default_panel_frame;
import rust.g37app.g37ui.Frame;

public void main() {
    print(run((ui, window) -> {
        window.set_title("dark");
        Panel.left("nav").frame(new Frame().fill(Color.DARK).inner_margin((byte) 8)).show(ui, (nav) -> {
            nav.label("Overview");
        });
        ui.drop_zone(default_panel_frame(), (zone) -> zone.label("drop here"));
    }));
}
"#,
    );
    assert_eq!(
        build_and_run(&dir),
        "panel nav fill=27,27,27 margin=8\nOverview\nzone fill=0,0,0 margin=4\ndrop here\ntitle=dark\n",
    );
}

/// Written by its fully-qualified name, the member's `Frame` is that type even
/// where the host's `Frame` is imported.
#[test]
fn a_qualified_name_means_its_type_beside_an_import_of_the_other() {
    let dir = case_dir(
        "fqn",
        r#"
import rust.g37app.Frame;
import rust.g37app.Ui;
import rust.g37app.Panel;
import rust.g37app.Color;
import rust.g37app.run;

public void main() {
    print(run((ui, window) -> {
        Panel.left("side").frame(new rust.g37app.g37ui.Frame().fill(Color.rgb((ubyte) 1, (ubyte) 2, (ubyte) 3))).show(ui, (side) -> side.label("x"));
    }));
}
"#,
    );
    assert_eq!(build_and_run(&dir), "panel side fill=1,2,3 margin=0\nx\ntitle=\n");
}

/// Importing both by their simple name is `E0303`, naming both.
#[test]
fn importing_both_frames_is_e0303() {
    let dir = case_dir(
        "both",
        r#"
import rust.g37app.Frame;
import rust.g37app.g37ui.Frame;

public void main() {
    print(1);
}
"#,
    );
    let (ok, said) = juxc(&dir, true);
    assert!(!ok, "importing both `Frame`s must be refused:\n{said}");
    assert!(said.contains("[E0303]"), "{said}");
    assert!(said.contains("rust.g37app.Frame") && said.contains("rust.g37app.g37ui.Frame"), "{said}");
}

/// Two wildcards that both bring a `Frame` are fine until `Frame` is used,
/// then `E0303` names both; the other names they bring are the same types.
#[test]
fn two_wildcards_with_a_frame_each_are_e0303_only_at_a_use() {
    let unused = case_dir(
        "wild-unused",
        r#"
import rust.g37app.*;
import rust.g37app.g37ui.*;

public void main() {
    print(run((ui, window) -> ui.label("fine")));
}
"#,
    );
    let (ok, said) = juxc(&unused, true);
    assert!(ok, "an ambiguous name nobody uses is not an error, and `Ui` is one type:\n{said}");

    let used = case_dir(
        "wild-used",
        r#"
import rust.g37app.*;
import rust.g37app.g37ui.*;

public void main() {
    Frame f = new Frame();
    print(1);
}
"#,
    );
    let (ok, said) = juxc(&used, true);
    assert!(!ok, "a use of the ambiguous `Frame` must be refused:\n{said}");
    assert!(said.contains("[E0303]"), "{said}");
    assert!(said.contains("rust.g37app.Frame") && said.contains("rust.g37app.g37ui.Frame"), "{said}");
}

/// A deprecated crate method is a Jux warning with the crate's note (L39).
#[test]
fn a_deprecated_crate_method_is_w0491_with_the_crates_note() {
    let dir = case_dir(
        "deprecated",
        r#"
import rust.g37app.Panel;
import rust.g37app.run;

public void main() {
    print(run((ui, window) -> {
        Panel.left("old").show_inside(ui, (inner) -> inner.label("still works"));
    }));
}
"#,
    );
    let (ok, said) = juxc(&dir, true);
    assert!(ok, "a deprecated method still compiles:\n{said}");
    assert!(said.contains("[W0491]"), "{said}");
    assert!(said.contains("Renamed to `show`"), "{said}");
}
