//! `examples/svg_studio` — Skia, driven from Jux.
//!
//! Renders an SVG document through Skia's SVG DOM, draws over the result with
//! Skia's own vector API, encodes a PNG, and reads pixels back. It is RUN, not
//! just built: the pixel checks are what prove the document was rasterised
//! rather than merely parsed.
//!
//! This is the C++-library interop test. `skia-safe` presents its whole
//! surface as `pub type Paint = Handle<SkPaint>` aliases over generic handle
//! types, with renamed re-exports, `impl Into<T>` parameters, `Self` returns
//! and borrowed `&Canvas` arguments — every one of which the stub generator
//! has to understand for a single call to type-check.
//!
//! Windows-gated for the same reason the window examples are: the prebuilt
//! Skia binary is fetched per target, and the feature set this example asks
//! for is published for `x86_64-pc-windows-msvc`. The Jux side is portable.

#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

fn run() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(root.join("examples").join("svg_studio"))
        .arg("run")
        .output()
        .expect("spawning jux run for svg_studio");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "svg_studio failed with {:?}\n{stdout}{stderr}",
        output.status.code(),
    );
    stdout
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with("jux:"))
        .collect()
}

#[test]
fn svg_studio_rasterises_a_document_and_reads_it_back() {
    let got = run();
    assert_eq!(
        got,
        [
            "surface 200x120",
            "svg rendered",
            "frame stroked",
            "snapshot 200x120",
            "png bytes: true",
            "png signature: true",
            "background dark: true",
            "disc is teal: true",
            "bar is pale: true",
            "disc leads with: green",
        ],
    );
}
