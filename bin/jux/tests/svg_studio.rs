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
    run_project("svg_studio")
}

fn run_project(project: &str) -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(root.join("examples").join(project))
        .arg("run")
        .output()
        .unwrap_or_else(|e| panic!("spawning jux run for {project}: {e}"));

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

/// `examples/svg_chart` builds its SVG from data with string interpolation,
/// the way a report or dashboard would, and hands it to the same renderer.
#[test]
fn svg_chart_renders_a_generated_document() {
    assert_eq!(run_project("svg_chart"), ["wrote a 5-bar chart"]);
}

/// `examples/skia_java_style` is the same kind of drawing written the way a
/// Java programmer would: `new Paint()`, `PaintStyle.Stroke`, integer
/// coordinates, the surface in a field and the canvas in a variable. The
/// pixel checks prove the drawing reached the board rather than a copy.
#[test]
fn skia_java_style_draws_on_the_shared_surface() {
    assert_eq!(
        run_project("skia_java_style"),
        [
            "disc teal: true",
            "frame red: true",
            "background dark: true",
            "outline style: true",
        ],
    );
}
