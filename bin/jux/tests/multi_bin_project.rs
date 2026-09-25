//! The canonical multi-target project shape (JUX-BUILD-SYSTEM-ADDENDUM
//! §B.15.2): `src/lib.jux` + `src/main.jux` + `src/bin/server.jux` +
//! `src/bin/migrator.jux`, one `[lib]` and three `[[bin]]` blocks, every binary
//! importing a type from the shared library code.
//!
//! It is the commonest non-trivial shape a project takes, and it did not build.
//! Two independent causes, one test file:
//!
//! 1. The `[lib]` target compiled the WHOLE `src/` tree, entry files included,
//!    so all three `main`s landed in one library crate: E0400, "`main` is
//!    declared more than once at the top level". The lib target is built
//!    unconditionally, so this failed before any binary was reached.
//! 2. A file's package is derived from its directory under `src/`, which told
//!    `src/bin/server.jux` to declare `package bin;` (E0301) -- but an entry
//!    point named by the manifest is a program, not a member of a package.
//!
//! The negative cases are here too, because the fix for either one could have
//! been a blanket exemption that threw away a real error: two `main`s in ONE
//! binary's own source is still E0400, and an ordinary file below `src/` still
//! declares the package its directory implies.
//!
//! Only the first test builds anything. `jux check` compiles every clean target
//! of a package through cargo, so a project with four targets costs four crate
//! builds; each negative case is therefore scaffolded so that NO target of it
//! is clean, and it stops at the diagnostic.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Minimal std-only temp dir, removed on drop. Same shape as
/// `bin_main_entry.rs`: the suite scaffolds throwaway projects often enough to
/// want this, and not often enough to take a dependency for it.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let id = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "jux-multi-bin-{tag}-{}-{id}",
            std::process::id(),
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Write `text` to `<root>/<rel>`, creating the directories it needs.
fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
}

/// Run `jux <args>` in `root` and return (success, stdout + stderr).
fn jux(root: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn jux");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    (output.status.success(), all)
}

/// The shared library type every entry file imports. §3.1 puts a public type in
/// the file named after it, inside its package's directory, so the library's
/// code is here and `src/lib.jux` is only the library root.
const GREETER: &str = "\
package com.example.myapp;

public class Greeter {
    private String who;

    public Greeter(String who) {
        this.who = who;
    }

    public String greet() {
        return \"hello, \" + who;
    }
}
";

/// An entry file that greets as `who`.
fn entry(who: &str) -> String {
    format!(
        "\
import com.example.myapp.Greeter;

public void main() {{
    Greeter g = new Greeter(\"{who}\");
    print(g.greet());
}}
"
    )
}

/// Scaffold §B.15.2's tree verbatim.
fn scaffold(root: &Path) {
    write(
        root,
        "jux.toml",
        "\
[package]
name = \"com.example.myapp\"

[lib]
name = \"myapp\"

[[bin]]
name = \"myapp\"
path = \"src/main.jux\"

[[bin]]
name = \"myapp-server\"
path = \"src/bin/server.jux\"

[[bin]]
name = \"myapp-migrator\"
path = \"src/bin/migrator.jux\"
",
    );
    write(root, "src/lib.jux", "// Library root for `com.example.myapp`.\n");
    write(root, "src/com/example/myapp/Greeter.jux", GREETER);
    write(root, "src/main.jux", &entry("main"));
    write(root, "src/bin/server.jux", &entry("server"));
    write(root, "src/bin/migrator.jux", &entry("migrator"));
    // §B.1.2's parallel `test/` tree, so `jux test` has something to run and
    // the run proves the shared library code is in scope for it.
    write(
        root,
        "test/com/example/myapp/GreeterTest.jux",
        "\
package com.example.myapp;

import jux.std.testing.*;

@Test
void greetsByName() {
    Greeter g = new Greeter(\"test\");
    assertEqual(\"hello, test\", g.greet());
}
",
    );
}

/// The whole shape, end to end: `jux check` is clean, `jux build` produces the
/// library and all three binaries, and `jux run --bin <name>` runs the one
/// asked for. All in one test so the emitted crates are built once: each target
/// is its own cargo crate, and this shape has four of them.
#[test]
fn lib_plus_three_bins_checks_builds_and_runs_each() {
    let dir = TempDir::new("all");
    let root = dir.path();
    scaffold(root);

    let (ok, out) = jux(root, &["check"]);
    assert!(ok, "check failed on the §B.15.2 shape:\n{out}");
    assert!(out.contains("check ok"), "check said something else:\n{out}");

    let (ok, out) = jux(root, &["build"]);
    assert!(ok, "build failed on the §B.15.2 shape:\n{out}");
    assert!(out.contains("built library crate"), "no [lib] target built:\n{out}");
    for name in ["myapp", "myapp-server", "myapp-migrator"] {
        assert!(out.contains(name), "binary `{name}` not built:\n{out}");
    }

    // Each binary runs its OWN entry point, not whichever one came first.
    for (name, who) in [
        ("myapp", "main"),
        ("myapp-server", "server"),
        ("myapp-migrator", "migrator"),
    ] {
        let (ok, out) = jux(root, &["run", "--bin", name]);
        assert!(ok, "`run --bin {name}` failed:\n{out}");
        assert!(
            out.contains(&format!("hello, {who}")),
            "`--bin {name}` ran the wrong entry:\n{out}",
        );
    }

    // `jux test` builds ONE crate for the whole package, so it too must not
    // take every binary's entry file: the shape could be built and run but not
    // tested (E0400 on each `main`).
    let (ok, out) = jux(root, &["test"]);
    assert!(ok, "test failed on the §B.15.2 shape:\n{out}");
    assert!(out.contains("1 passed; 0 failed"), "the package's test did not run:\n{out}");
}

/// A diagnostic in an entry file must name that file. Each target compiles a
/// different source list (the lib drops every entry, a bin drops the others'),
/// so a diagnostic's file index means nothing outside the target that produced
/// it: an error on line 5 of `src/bin/server.jux` was reported against
/// `src/com/example/myapp/Greeter.jux:6`, a file with nothing wrong in it.
///
/// Two bins, no `[lib]`, and BOTH entries wrong: every target fails, so the
/// test costs no cargo build, and each entry's own error is checked against the
/// source list of a different target.
#[test]
fn an_error_in_an_entry_file_is_reported_against_that_file() {
    let dir = TempDir::new("attrib");
    let root = dir.path();
    write(
        root,
        "jux.toml",
        "\
[package]
name = \"com.example.myapp\"

[[bin]]
name = \"myapp\"
path = \"src/main.jux\"

[[bin]]
name = \"myapp-server\"
path = \"src/bin/server.jux\"
",
    );
    write(root, "src/com/example/myapp/Greeter.jux", GREETER);
    // `greet()` returns a String; both entries assign it to an int.
    for (rel, name) in [("src/main.jux", "fromMain"), ("src/bin/server.jux", "fromServer")] {
        write(
            root,
            rel,
            &format!(
                "\
import com.example.myapp.Greeter;

public void main() {{
    Greeter g = new Greeter(\"x\");
    int {name} = g.greet();
    print({name});
}}
"
            ),
        );
    }

    let (ok, out) = jux(root, &["check"]);
    assert!(!ok, "a type error unexpectedly checked clean:\n{out}");
    for (file, name) in [("main.jux", "fromMain"), ("server.jux", "fromServer")] {
        let line = out
            .lines()
            .find(|l| l.contains("[E0410]") && l.contains(name))
            .unwrap_or_else(|| panic!("no E0410 for `{name}` in:\n{out}"));
        assert!(line.contains(&format!("{file}:5:")), "E0410 blamed the wrong file:\n{line}");
    }
    assert!(
        !out.contains("Greeter.jux"),
        "the shared library file was blamed for an entry's error:\n{out}",
    );
}

/// Dropping the other entries from a target's compile must not weaken the
/// duplicate-entry rule WITHIN one binary: two top-level `main`s in a single
/// entry file are still E0400 (§E.1.3 reports E0320 for the file as well).
#[test]
fn two_entry_points_in_one_binary_are_still_rejected() {
    let dir = TempDir::new("dup");
    let root = dir.path();
    write(
        root,
        "jux.toml",
        "\
[package]
name = \"com.example.myapp\"

[[bin]]
name = \"myapp-server\"
path = \"src/bin/server.jux\"
",
    );
    write(
        root,
        "src/bin/server.jux",
        "\
public void main() {
    print(\"once\");
}

public void main() {
    print(\"twice\");
}
",
    );

    let (ok, out) = jux(root, &["check"]);
    assert!(!ok, "two `main`s in one binary unexpectedly checked clean:\n{out}");
    assert!(out.contains("[E0400]"), "expected E0400, got:\n{out}");
    assert!(
        out.contains("declared more than once"),
        "expected the duplicate-`main` message, got:\n{out}",
    );
}

/// The entry exemption is per declared file, not per directory: an ordinary
/// source below `src/` still declares the package its location implies
/// (§B.1.1), while the entry beside it stays package-less.
#[test]
fn a_non_entry_file_below_src_still_declares_its_package() {
    let dir = TempDir::new("pkg");
    let root = dir.path();
    write(
        root,
        "jux.toml",
        "\
[package]
name = \"com.example.myapp\"

[[bin]]
name = \"myapp-server\"
path = \"src/bin/server.jux\"
",
    );
    write(root, "src/bin/server.jux", "public void main() { print(\"ok\"); }\n");
    write(
        root,
        "src/tools/Helper.jux",
        "\
public class Helper {
    public int two() {
        return 2;
    }
}
",
    );

    let (ok, out) = jux(root, &["check"]);
    assert!(!ok, "a package-less file below `src/` unexpectedly checked clean:\n{out}");
    assert!(out.contains("[E0301]"), "expected E0301, got:\n{out}");
    assert!(
        out.contains("must declare `package tools;`"),
        "expected the derived package in the message, got:\n{out}",
    );
    assert!(
        !out.contains("package bin;"),
        "the declared entry file was asked for a package anyway:\n{out}",
    );
}
