//! `examples/keyword_packages` -- packages and a file named after Rust keywords.
//!
//! `demo.box`, `demo.match` and `Crate.jux` are ordinary in Jux and reserved in
//! Rust. The compiler turns dotted names into Rust paths in dozens of places,
//! and one that forgot to escape was enough to break the build, so the example
//! is run end to end rather than inspected. It also covers a fully-qualified
//! static call, `demo.box.Crate.of(7)`, which used to lower as field access.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn keyword_named_packages_and_files_build_and_run() {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .join("examples")
        .join("keyword_packages");

    let out = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(&project)
        .arg("run")
        .output()
        .expect("spawning jux run for keyword_packages");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "keyword_packages failed with {:?}\n{stdout}{stderr}",
        out.status.code(),
    );

    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.starts_with("jux: "))
        .collect();
    assert_eq!(
        lines,
        [
            "small fits: true",
            "large fits: false",
            "imported: 7",
            "fully qualified: 7",
        ],
    );
}
