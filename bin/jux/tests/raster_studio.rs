//! `examples/raster_studio` — three real crates.io crates in one program.
//!
//! `tiny-skia` draws a vector scene, `image` decodes the PNG that came out of
//! it, and `rayon` renders two more sizes on two threads. It is RUN, not just
//! built, because every line it prints is derived from the pixels: if the
//! drawing or the crate boundary changes, the numbers change.
//!
//! This is the crate-interop regression test. Between them the three crates
//! exercise a re-exported type (`tiny_skia::Path` is defined in
//! `tiny-skia-path`), a renamed package (`tiny-skia` under `tiny_skia`), a
//! crate-local `Result` alias (`ImageResult`), an enum with inherent methods
//! (`DynamicImage`), a `&[u8]` slice parameter, a `-> &[u8]` borrowed return,
//! `u8`/`u32` parameters against Jux `int`, and a closure handed to a Rust
//! function expecting `FnOnce`.

use std::path::PathBuf;
use std::process::Command;

/// Run the example and return its stdout lines.
///
/// The first build downloads and compiles three crates, so this is slow once
/// and cached afterwards.
fn run() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(root.join("examples").join("raster_studio"))
        .arg("run")
        .output()
        .expect("spawning jux run for raster_studio");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "raster_studio failed with {:?}\n{stdout}{stderr}",
        output.status.code(),
    );
    stdout
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with("jux:"))
        .collect()
}

#[test]
fn raster_studio_draws_encodes_and_analyses() {
    let got = run();
    assert_eq!(
        got,
        [
            "canvas 240x160",
            "png bytes present: true",
            "decoded 240x160",
            "opaque pixels: 38400",
            "transparent pixels: 0",
            "dominant channel: red",
            "mean luminance: 44",
            "half-size opaque: 9600",
            "quarter-size opaque: 2400",
            "same drawing, same channel: true",
        ],
    );
}
