//! End-to-end test for **git-hosted Jux dependencies** (§B.2.2):
//!
//! ```toml
//! [dependencies]
//! "com.test.greeter" = { git = "<url>" }
//! ```
//!
//! Builds a local git repository holding a Jux library, a consumer app
//! depending on it by git URL, and exercises the full lifecycle:
//!
//! 1. first build fetches the dep into the `JUX_HOME` cache and links it;
//! 2. a new upstream commit does NOT change the build (cache = pin);
//! 3. `jux update` refreshes the cache; the rebuild sees the new code;
//! 4. a `rev = "..."` pin resolves the exact (older) commit.
//!
//! Uses a `file:`-style local repo path — no network — but the same
//! `git clone` machinery a GitHub URL goes through. Requires the `git`
//! CLI on PATH (the same requirement real git deps have).

use std::path::{Path, PathBuf};
use std::process::Command;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=test", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Run `jux <args>` in `cwd` with the cache rooted at `jux_home`;
/// return (success, stdout, stderr).
fn jux(cwd: &Path, jux_home: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(cwd)
        .env("JUX_HOME", jux_home)
        .args(args)
        .output()
        .expect("spawn jux");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn git_dependency_full_lifecycle() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("target")
        .join("it-git-deps");
    let _ = std::fs::remove_dir_all(&root);
    let lib = root.join("lib-repo");
    let app = root.join("app");
    let home = root.join("jux-home");

    // ---- the library repository -------------------------------------
    write(
        &lib.join("jux.toml"),
        "[package]\nname = \"com.test.greeter\"\nversion = \"0.1.0\"\n\n[lib]\npath = \"src/lib.jux\"\n",
    );
    write(&lib.join("src").join("lib.jux"), "// com.test.greeter\n");
    write(
        &lib.join("src/com/test/greeter/greeter.jux"),
        "package com.test.greeter;\n\npublic class Greeter {\n    public String greet(String who) {\n        return \"Hello, \" + who + \"!\";\n    }\n}\n",
    );
    git(&lib, &["init", "-q"]);
    git(&lib, &["add", "-A"]);
    git(&lib, &["commit", "-q", "-m", "v1"]);
    // Forward slashes keep the URL form portable in the TOML.
    let lib_url = lib.to_string_lossy().replace('\\', "/");

    // ---- the consumer app --------------------------------------------
    write(
        &app.join("jux.toml"),
        &format!(
            "[package]\nname = \"com.test.app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"com.test.greeter\" = {{ git = \"{lib_url}\" }}\n",
        ),
    );
    write(
        &app.join("src").join("main.jux"),
        "import com.test.greeter.Greeter;\n\npublic void main() {\n    var g = new Greeter();\n    print(g.greet(\"GitHub\"));\n}\n",
    );

    // 1. First build fetches + links the dep.
    let (ok, stdout, stderr) = jux(&app, &home, &["run"]);
    assert!(ok, "first run failed:\n{stderr}\n{stdout}");
    assert!(stdout.contains("Hello, GitHub!"), "v1 output:\n{stdout}");

    // 2. Upstream moves; the cached checkout is the pin — no change.
    let greeter_v2 = "package com.test.greeter;\n\npublic class Greeter {\n    public String greet(String who) {\n        return \"Howdy, \" + who + \"!\";\n    }\n}\n";
    write(&lib.join("src/com/test/greeter/greeter.jux"), greeter_v2);
    git(&lib, &["add", "-A"]);
    git(&lib, &["commit", "-q", "-m", "v2"]);
    let (ok, stdout, stderr) = jux(&app, &home, &["run"]);
    assert!(ok, "cached run failed:\n{stderr}\n{stdout}");
    assert!(
        stdout.contains("Hello, GitHub!"),
        "cache must pin v1 until `jux update`:\n{stdout}",
    );

    // 3. `jux update` refreshes; the rebuild sees v2.
    let (ok, stdout, stderr) = jux(&app, &home, &["update"]);
    assert!(ok, "update failed:\n{stderr}\n{stdout}");
    assert!(
        stderr.contains("updated `com.test.greeter`"),
        "update should report the dep:\n{stderr}",
    );
    let (ok, stdout, stderr) = jux(&app, &home, &["run"]);
    assert!(ok, "post-update run failed:\n{stderr}\n{stdout}");
    assert!(stdout.contains("Howdy, GitHub!"), "v2 output:\n{stdout}");

    // 4. A rev pin resolves the exact older commit.
    let out = Command::new("git")
        .arg("-C")
        .arg(&lib)
        .args(["rev-list", "--max-count=1", "HEAD~1"])
        .output()
        .expect("rev-list");
    let v1_rev = String::from_utf8_lossy(&out.stdout).trim().to_string();
    write(
        &app.join("jux.toml"),
        &format!(
            "[package]\nname = \"com.test.app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"com.test.greeter\" = {{ git = \"{lib_url}\", rev = \"{v1_rev}\" }}\n",
        ),
    );
    let (ok, stdout, stderr) = jux(&app, &home, &["run"]);
    assert!(ok, "rev-pinned run failed:\n{stderr}\n{stdout}");
    assert!(
        stdout.contains("Hello, GitHub!"),
        "rev pin must resolve v1:\n{stdout}",
    );
}
