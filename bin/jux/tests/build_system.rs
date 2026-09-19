//! Build-system behavior of the `jux` project tool, end to end on temporary
//! projects (JUX-BUILD-SYSTEM-ADDENDUM §B.7, §B.9, §B.15).
//!
//! Every test writes its own project under `target/it-build-system-*`, runs the
//! freshly built `jux` against it, and reads what `jux` printed.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The freshly-built `jux` binary, courtesy of cargo's test harness.
fn jux_binary() -> &'static str {
    env!("CARGO_BIN_EXE_jux")
}

/// Two `..`s up from `bin/jux/` is the workspace root.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

/// A clean scratch directory for one test.
fn scratch(tag: &str) -> PathBuf {
    let base = workspace_root().join("target").join(format!("it-build-system-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creating scratch dir");
    base
}

/// Write `contents` to `dir/rel`, creating parent directories.
fn write(dir: &Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("creating dirs");
    std::fs::write(&path, contents).expect("writing file");
}

/// Run `jux <args>` in `dir`; return (success, stdout, stderr).
fn jux(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(jux_binary())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("running jux");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A workspace root using every §B.7.1 key: a `members` pattern, `exclude`,
/// `default-members`, and `[workspace.package]` / `[workspace.dependencies]`
/// inherited by a member.
fn write_workspace(root: &Path) {
    write(
        root,
        "jux.toml",
        "[workspace]\n\
         members = [\"apps/*\", \"shared\", \"tool\"]\n\
         exclude = [\"apps/experimental\"]\n\
         default-members = [\"apps/main\"]\n\
         \n\
         [workspace.package]\n\
         edition = \"2026\"\n\
         license = \"MIT\"\n\
         \n\
         [workspace.dependencies]\n\
         \"ws.shared\" = { path = \"shared\" }\n",
    );
    // A library every app shares.
    write(
        root,
        "shared/jux.toml",
        "[package]\nname = \"ws.shared\"\nversion = \"0.1.0\"\nedition.workspace = true\n\n[lib]\n",
    );
    // The crate root is package-less; the package's code lives in its tree.
    write(root, "shared/src/lib.jux", "// crate root of ws.shared\n");
    write(
        root,
        "shared/src/ws/shared/Answers.jux",
        "package ws.shared;\n\npublic int answer() { return 42; }\n",
    );
    // The default member: inherits its edition and its dependency.
    write(
        root,
        "apps/main/jux.toml",
        "[package]\nname = \"ws.main\"\nversion = \"0.1.0\"\nedition.workspace = true\nlicense.workspace = true\n\n[dependencies]\n\"ws.shared\".workspace = true\n",
    );
    write(
        root,
        "apps/main/src/main.jux",
        "import ws.shared.answer;\n\npublic void main() { print($\"answer ${answer()}\"); }\n",
    );
    // Excluded by `exclude`: would not even compile, so building it fails loudly.
    write(root, "apps/experimental/jux.toml", "[package]\nname = \"ws.experimental\"\n");
    write(root, "apps/experimental/src/main.jux", "public void main() { this is not jux }\n");
    // A member, but not a default one.
    write(root, "tool/jux.toml", "[package]\nname = \"ws.tool\"\nversion = \"0.1.0\"\n");
    write(root, "tool/src/main.jux", "public void main() { print(\"tool\"); }\n");
}

/// A bare `jux build` builds the default member and the member it depends
/// on, and nothing else: not the non-default `tool`, not the excluded
/// `experimental`. Inheritance lets `apps/main` name `ws.shared` without
/// repeating its path.
#[test]
fn workspace_builds_default_members_and_their_dependencies() {
    let root = scratch("workspace");
    write_workspace(&root);
    let (ok, _out, err) = jux(&root, &["build"]);
    assert!(ok, "workspace build failed:\n{err}");
    assert!(err.contains("[ws.main] built"), "the default member was not built:\n{err}");
    assert!(err.contains("[ws.shared] built"), "its dependency was not built:\n{err}");
    assert!(!err.contains("ws.tool"), "a non-default member was built:\n{err}");
    assert!(!err.contains("experimental"), "an excluded member was touched:\n{err}");
}

/// `-p` still reaches a member outside `default-members`, and `jux metadata`
/// lists the expanded members (pattern resolved, exclusion applied).
#[test]
fn workspace_member_outside_the_defaults_is_still_selectable() {
    let root = scratch("workspace-select");
    write_workspace(&root);
    let (ok, _out, err) = jux(&root, &["run", "-p", "tool"]);
    assert!(ok, "running a non-default member failed:\n{err}");

    let (ok, out, err) = jux(&root, &["metadata"]);
    assert!(ok, "metadata failed:\n{err}");
    let json: serde_json::Value = serde_json::from_str(&out).expect("metadata is JSON");
    let names: Vec<&str> = json["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert!(names.contains(&"ws.main") && names.contains(&"ws.tool"), "got {names:?}");
    assert!(!names.contains(&"ws.experimental"), "excluded member listed: {names:?}");
    let main = json["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "ws.main")
        .unwrap();
    assert_eq!(main["edition"], "2026", "edition was not inherited");
}

/// `--profile <name>` selects a custom `[profile.<name>]` (§B.9): the build
/// lands in cargo's directory for that profile, `cfg(release)` follows the
/// profile's `extends` chain, and an unknown name is an error listing the
/// profiles there are.
#[test]
fn custom_profiles_are_selectable() {
    let root = scratch("profiles");
    write(
        &root,
        "jux.toml",
        "[package]
name = \"prof\"
version = \"0.1.0\"

[profile.fast]
extends = \"release\"
opt-level = 2

[profile.trace]
debug = \"full\"
",
    );
    write(
        &root,
        "src/main.jux",
        "public void main() {
    if cfg(release) {
        print(\"optimized\");
    } else {
        print(\"debug\");
    }
}
",
    );
    let (ok, out, err) = jux(&root, &["run", "--profile", "fast"]);
    assert!(ok, "--profile fast failed:
{err}");
    assert_eq!(out.trim(), "optimized");
    let fast_dir = format!("{}fast{}", std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR);
    assert!(err.contains(&fast_dir), "not built in the profile's own directory:
{err}");

    let (ok, out, err) = jux(&root, &["run", "--profile", "trace"]);
    assert!(ok, "--profile trace failed:
{err}");
    assert_eq!(out.trim(), "debug", "a profile with no extends derives from dev");

    let (ok, _out, err) = jux(&root, &["build", "--profile", "ghost"]);
    assert!(!ok, "an unknown profile must fail");
    assert!(err.contains("no profile `ghost`") && err.contains("fast, trace"), "got:
{err}");
}

/// `--example <name>` builds `examples/<name>.jux` (or the directory
/// `examples/<name>/`) against the package's library code; `--examples`
/// builds them all; an unknown name lists the ones there are.
#[test]
fn examples_build_against_the_library() {
    let root = scratch("examples");
    write(&root, "jux.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write(&root, "src/main.jux", "public void main() { print(\"app\"); }\n");
    write(&root, "src/demo/Greeter.jux", "package demo;\n\npublic String greet(String who) { return \"hello \" + who; }\n");
    write(&root, "examples/hello.jux", "import demo.greet;\n\npublic void main() { print(greet(\"example\")); }\n");
    write(&root, "examples/multi/main.jux", "import demo.greet;\n\npublic void main() { print(greet(\"multi\") + helper()); }\n");
    write(&root, "examples/multi/Help.jux", "public String helper() { return \"!\"; }\n");
    write(&root, "examples/README.md", "not an example\n");

    let (ok, out, err) = jux(&root, &["run", "--example", "hello"]);
    assert!(ok, "--example hello failed:\n{err}");
    assert_eq!(out.trim(), "hello example");
    let (ok, out, err) = jux(&root, &["run", "--example", "multi"]);
    assert!(ok, "--example multi failed:\n{err}");
    assert_eq!(out.trim(), "hello multi!");
    let (ok, _out, err) = jux(&root, &["build", "--examples"]);
    assert!(ok, "--examples failed:\n{err}");
    assert!(err.contains("example `hello`") && err.contains("example `multi`"), "got:\n{err}");
    let (ok, _out, err) = jux(&root, &["run", "--example", "nope"]);
    assert!(!ok);
    assert!(err.contains("available: hello, multi"), "got:\n{err}");
}

/// `jux new` writes the four §B.15.1 files and a project that runs; `--lib`
/// writes a library whose own test passes; `--workspace` a root manifest.
#[test]
fn new_scaffolds_runnable_projects() {
    let base = scratch("new");
    let (ok, _out, err) = jux(&base, &["new", "app"]);
    assert!(ok, "jux new failed:\n{err}");
    for f in ["jux.toml", "src/main.jux", "README.md", ".gitignore"] {
        assert!(base.join("app").join(f).is_file(), "missing {f}");
    }
    let (ok, out, err) = jux(&base.join("app"), &["run"]);
    assert!(ok, "the new project does not run:\n{err}");
    assert_eq!(out.trim(), "Hello, world!");

    let (ok, _out, err) = jux(&base, &["new", "--lib", "my-greeter"]);
    assert!(ok, "jux new --lib failed:\n{err}");
    assert!(base.join("my-greeter/src/my_greeter/MyGreeter.jux").is_file());
    let (ok, out, err) = jux(&base.join("my-greeter"), &["test"]);
    assert!(ok, "the new library's test fails:\n{out}\n{err}");

    let (ok, _out, err) = jux(&base, &["new", "--workspace", "ws"]);
    assert!(ok, "jux new --workspace failed:\n{err}");
    let manifest = std::fs::read_to_string(base.join("ws/jux.toml")).unwrap();
    assert!(manifest.contains("[workspace]"), "{manifest}");

    let (ok, _out, err) = jux(&base, &["new", "app"]);
    assert!(!ok && err.contains("already exists"), "overwrote a project:\n{err}");
}

/// `jux add` / `jux remove` edit `[dependencies]` in place: other text,
/// comments included, stays exactly as written. `jux tree` shows the result.
#[test]
fn add_remove_and_tree_edit_the_manifest_in_place() {
    let root = scratch("deps");
    write(&root, "lib/jux.toml", "[package]\nname = \"deps.lib\"\nversion = \"0.2.0\"\n\n[lib]\n");
    write(&root, "lib/src/lib.jux", "// crate root\n");
    write(
        &root,
        "app/jux.toml",
        "# the app\n[package]\nname = \"deps.app\"   # keep this comment\nversion = \"0.1.0\"\n",
    );
    write(&root, "app/src/main.jux", "public void main() { print(\"x\"); }\n");
    let app = root.join("app");

    let (ok, _out, err) = jux(&app, &["add", "deps.lib", "--path", "../lib"]);
    assert!(ok, "add --path failed:\n{err}");
    let (ok, _out, err) = jux(&app, &["add", "rust.rand@0.9", "--features", "small_rng"]);
    assert!(ok, "add name@version failed:\n{err}");
    let text = std::fs::read_to_string(app.join("jux.toml")).unwrap();
    assert!(text.starts_with("# the app\n[package]\nname = \"deps.app\"   # keep this comment\n"), "reformatted:\n{text}");
    assert!(text.contains("\"deps.lib\" = { path = \"../lib\" }"), "{text}");
    assert!(text.contains("\"rust.rand\" = { version = \"0.9\", features = [\"small_rng\"] }"), "{text}");

    let (ok, out, _err) = jux(&app, &["tree"]);
    assert!(ok);
    assert!(out.starts_with("deps.app v0.1.0"), "{out}");
    assert!(out.contains("deps.lib (path"), "{out}");
    assert!(out.contains("rust.rand v0.9"), "{out}");

    let (ok, _out, err) = jux(&app, &["remove", "rust.rand"]);
    assert!(ok, "remove failed:\n{err}");
    let text = std::fs::read_to_string(app.join("jux.toml")).unwrap();
    assert!(!text.contains("rust.rand"), "{text}");
    let (ok, _out, err) = jux(&app, &["remove", "rust.rand"]);
    assert!(!ok && err.contains("declared: deps.lib"), "{err}");
}

/// `jux clean` removes the build output and nothing else; `jux init` adds a
/// manifest to a directory and refuses to replace one.
#[test]
fn clean_and_init() {
    let root = scratch("clean-init");
    write(&root, "src/main.jux", "public void main() { print(\"x\"); }\n");
    let (ok, _out, err) = jux(&root, &["init"]);
    assert!(ok, "init failed:\n{err}");
    assert!(root.join("jux.toml").is_file());
    let (ok, _out, err) = jux(&root, &["init"]);
    assert!(!ok && err.contains("already has a jux.toml"), "{err}");

    let (ok, _out, err) = jux(&root, &["build"]);
    assert!(ok, "build failed:\n{err}");
    assert!(root.join("target").is_dir());
    let (ok, _out, err) = jux(&root, &["clean"]);
    assert!(ok, "clean failed:\n{err}");
    assert!(!root.join("target").exists(), "target/ is still there");
    assert!(root.join("src/main.jux").is_file(), "clean touched the sources");
}
