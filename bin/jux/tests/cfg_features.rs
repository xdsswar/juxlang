//! End-to-end test for **conditional compilation with feature flags**
//! (JUX-LANG-V1 §11, JUX-BUILD-SYSTEM-ADDENDUM §B.8).
//!
//! A two-member workspace under `target/it-jux-cfg-features/`: `codec`, a
//! library whose `[features]` default to `json`, and `app`, which depends on
//! it asking for `yaml`. Asserts that
//!
//! 1. a plain `jux run` builds the library with `json` (its default) and
//!    `yaml` (requested by `app`), and the app can call the `yaml`-only
//!    function -- the library's crate and the app agree on the features;
//! 2. `--features` turns on a feature a member declares, for `@cfg` on a
//!    declaration and for `if cfg` in a body alike;
//! 3. `--no-default-features` does not switch `json` off while `app`'s
//!    dependency entry still asks for the defaults (feature unification).

use std::path::{Path, PathBuf};
use std::process::Command;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn jux_run(cwd: &Path, args: &[&str]) -> (bool, Vec<String>) {
    let out = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(cwd)
        .arg("run")
        .args(args)
        .output()
        .expect("spawn jux run");
    let text = String::from_utf8_lossy(&out.stdout).into_owned()
        + &String::from_utf8_lossy(&out.stderr);
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("jux:"))
        .map(str::to_string)
        .collect();
    (out.status.success(), lines)
}

#[test]
fn features_select_code_across_a_workspace() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("target")
        .join("it-jux-cfg-features");
    let _ = std::fs::remove_dir_all(&root);

    write(&root.join("jux.toml"), "[workspace]\nmembers = [\"codec\", \"app\"]\n");
    write(
        &root.join("codec/jux.toml"),
        r#"[package]
name = "com.test.codec"
version = "0.1.0"

[lib]
name = "codec"

[features]
default = ["json"]
json = []
yaml = []
toml = []
"#,
    );
    write(
        &root.join("codec/src/com/test/codec/codec.jux"),
        r#"package com.test.codec;

@cfg(feature = "json")
public String json() {
    return "json";
}

@cfg(feature = "yaml")
public String yaml() {
    return "yaml";
}

public String formats() {
    var out = "formats:";
    if cfg(feature = "json") {
        out = out + " json";
    }
    if cfg(feature = "yaml") {
        out = out + " yaml";
    }
    if cfg(feature = "toml") {
        out = out + " toml";
    }
    return out;
}
"#,
    );
    write(
        &root.join("app/jux.toml"),
        r#"[package]
name = "com.test.app"
version = "0.1.0"

[dependencies]
"com.test.codec" = { path = "../codec", features = ["yaml"] }

[features]
verbose = []
"#,
    );
    write(
        &root.join("app/src/main.jux"),
        r#"import com.test.codec.{formats, yaml};

public void main() {
    print(formats());
    print(yaml());
    if cfg(feature = "verbose") {
        print("verbose");
    } else {
        print("quiet");
    }
}
"#,
    );

    // 1. Defaults plus what the dependent asks for.
    let (ok, lines) = jux_run(&root, &[]);
    assert!(ok, "plain run failed:\n{}", lines.join("\n"));
    assert_eq!(lines, ["formats: json yaml", "yaml", "quiet"]);

    // 2. `--features` reaches both a member's declarations and the app's body.
    let (ok, lines) = jux_run(&root, &["--features", "verbose,toml"]);
    assert!(ok, "run with features failed:\n{}", lines.join("\n"));
    assert_eq!(lines, ["formats: json yaml toml", "yaml", "verbose"]);

    // 3. The app's dependency entry still asks for the codec's defaults.
    let (ok, lines) = jux_run(&root, &["--no-default-features"]);
    assert!(ok, "run without default features failed:\n{}", lines.join("\n"));
    assert_eq!(lines, ["formats: json yaml", "yaml", "quiet"]);
}
