//! `examples/metrics_console`: the four-package workspace, actually run.
//!
//! This one is worth more than the window examples it sits beside. Those are
//! only built, because opening a window in a test hangs; this draws into a
//! pixel buffer it owns, so the whole render path -- generic containers, an
//! interface with a default method, an enum with behaviour, all of it across
//! four packages and three dependency edges -- runs headless and the frame it
//! produces can be asserted on.
//!
//! What is asserted is deliberately structural: the frame's dimensions, that
//! every pixel is in range, and that more than one colour reached the buffer.
//! Pinning exact pixels would break on any deliberate design change and say
//! nothing about the compiler, which is what this is here to exercise.

mod common;

use std::process::Command;

/// The frame the example draws by default. Kept here rather than read from
/// the source, so a change to either side has to be made on purpose.
const WIDTH: usize = 900;
const HEIGHT: usize = 560;

/// Run the workspace with the given arguments, returning stdout.
fn run(args: &[&str]) -> String {
    let root = common::workspace_root();
    let project = root.join("examples").join("metrics_console");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jux"));
    cmd.current_dir(&project).arg("run");
    if !args.is_empty() {
        cmd.arg("--");
        cmd.args(args);
    }

    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("spawning jux run for metrics_console: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "metrics_console exited with {:?}\n{stdout}{stderr}",
        output.status.code(),
    );
    stdout
}

#[test]
fn renders_a_frame_to_a_file() {
    let out = common::workspace_root()
        .join("target")
        .join("it-metrics-console.ppm");
    let _ = std::fs::remove_file(&out);

    let stdout = run(&["--snapshot", &out.to_string_lossy()]);
    assert!(
        stdout.contains(&format!("wrote {WIDTH}x{HEIGHT} frame")),
        "expected the snapshot line, got: {stdout}"
    );

    let ppm = std::fs::read_to_string(&out).expect("the snapshot file exists");
    let mut fields = ppm.split_ascii_whitespace();
    assert_eq!(fields.next(), Some("P3"), "plain-text PPM magic");
    assert_eq!(fields.next(), Some(WIDTH.to_string().as_str()));
    assert_eq!(fields.next(), Some(HEIGHT.to_string().as_str()));
    assert_eq!(fields.next(), Some("255"), "8 bits per channel");

    // Three channels per pixel, every one a byte, and more than one colour --
    // a frame that cleared to the background and drew nothing would pass the
    // first two checks and fail this one.
    let samples: Vec<u16> = fields.map(|f| f.parse().expect("a decimal sample")).collect();
    assert_eq!(
        samples.len(),
        WIDTH * HEIGHT * 3,
        "one RGB triple per pixel"
    );
    assert!(samples.iter().all(|&s| s <= 255), "every sample is a byte");

    let distinct: std::collections::BTreeSet<&[u16]> = samples.chunks(3).collect();
    assert!(
        distinct.len() > 8,
        "the frame drew {} distinct colours, so nothing but the background \
         reached the buffer",
        distinct.len()
    );
}

#[test]
fn previews_the_frame_as_text() {
    // With no arguments the console prints the frame as a block of ASCII, so
    // `jux run` on a machine with no display still shows something.
    let stdout = run(&[]);
    let rows: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with("jux:"))
        .collect();

    assert_eq!(rows.len(), 30, "one line per preview row, got: {stdout}");
    for row in &rows {
        assert_eq!(row.chars().count(), 90, "every row is the same width");
        assert!(
            row.chars().all(|c| " .:-=+*#%@".contains(c)),
            "rows use only the ramp: {row}"
        );
    }
    assert!(
        rows.iter().any(|r| r.contains('#') || r.contains('%')),
        "the brightest cells appear somewhere, got: {stdout}"
    );
}

#[test]
fn honours_a_requested_size() {
    let out = common::workspace_root()
        .join("target")
        .join("it-metrics-console-small.ppm");
    let _ = std::fs::remove_file(&out);

    let stdout = run(&["--size", "320x200", "--snapshot", &out.to_string_lossy()]);
    assert!(
        stdout.contains("wrote 320x200 frame"),
        "expected the requested size, got: {stdout}"
    );

    let ppm = std::fs::read_to_string(&out).expect("the snapshot file exists");
    let mut fields = ppm.split_ascii_whitespace().skip(1);
    assert_eq!(fields.next(), Some("320"));
    assert_eq!(fields.next(), Some("200"));
}
