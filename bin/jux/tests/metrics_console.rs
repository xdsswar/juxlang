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

    // `-p app` by name: the workspace has a windowed member too, and a test
    // that relies on which member sorts last is a test that breaks when
    // someone adds one.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jux"));
    cmd.current_dir(&project).arg("run").arg("-p").arg("app");
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
        stdout.contains(&format!("wrote {WIDTH}x{HEIGHT} midnight overview frame")),
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
fn renders_the_paper_palette() {
    // The light theme is half of `Theme`, and before `--theme` existed the
    // only way to reach it was to press a key in the window -- which is to
    // say, nothing checked it.
    let out = common::workspace_root()
        .join("target")
        .join("it-metrics-console-paper.ppm");
    let _ = std::fs::remove_file(&out);

    let stdout = run(&["--theme", "paper", "--snapshot", &out.to_string_lossy()]);
    assert!(
        stdout.contains("paper overview frame"),
        "expected the paper theme to be named back, got: {stdout}"
    );

    let ppm = std::fs::read_to_string(&out).expect("the snapshot file exists");
    let samples: Vec<u16> = ppm
        .split_ascii_whitespace()
        .skip(4)
        .map(|v| v.parse().expect("a decimal sample"))
        .collect();

    // The paper ground is light and the midnight ground is dark, so the mean
    // sample separates them without pinning either palette's exact values.
    let mean: u32 = samples.iter().map(|&s| u32::from(s)).sum::<u32>() / samples.len() as u32;
    assert!(
        mean > 160,
        "the paper frame averages {mean}, which is not a light ground"
    );
}

/// The seven hosts the simulation always builds. A click lands on one of
/// them; which one depends on the layout, and pinning that would make every
/// spacing change a test failure.
const HOSTS: &[&str] = &[
    "web-01", "web-02", "web-03", "db-01", "cache-01", "cache-02", "edge-01",
];

/// The frame the click coordinates below are measured against. A click is
/// only meaningful against a known layout, so these tests fix the size.
const CLICK_SIZE: &str = "1000x620";

/// Run with a click at a canvas coordinate and return the report line.
fn click(x: &str, y: &str) -> String {
    let stdout = run(&["--size", CLICK_SIZE, "--click", x, y]);
    stdout
        .lines()
        .find(|l| l.starts_with("click "))
        .unwrap_or_else(|| panic!("no click line in: {stdout}"))
        .to_string()
}

#[test]
fn renders_every_view() {
    // Three views, each reachable by name and each drawing something. A view
    // that rendered nothing but its background would pass the first check and
    // fail the second.
    for view in ["overview", "hosts", "detail"] {
        let out = common::workspace_root()
            .join("target")
            .join(format!("it-metrics-console-{view}.ppm"));
        let _ = std::fs::remove_file(&out);

        let stdout = run(&["--view", view, "--snapshot", &out.to_string_lossy()]);
        assert!(
            stdout.contains(&format!("{view} frame")),
            "expected the {view} view to be named back, got: {stdout}"
        );

        let ppm = std::fs::read_to_string(&out).expect("the snapshot file exists");
        let samples: Vec<u16> = ppm
            .split_ascii_whitespace()
            .skip(4)
            .map(|v| v.parse().expect("a decimal sample"))
            .collect();
        let distinct: std::collections::BTreeSet<&[u16]> = samples.chunks(3).collect();
        assert!(
            distinct.len() > 8,
            "the {view} view drew {} colours, so nothing but the background \
             reached the buffer",
            distinct.len()
        );
    }
}

#[test]
fn a_click_on_a_host_row_opens_that_host() {
    // The real dispatch: `--click` calls `Console.click`, which is what the
    // window calls. The hit regions come from a render, so this also proves
    // the widgets registered them.
    let line = click("60", "120");
    assert!(line.contains("view detail"), "a host row opens Detail: {line}");
    assert!(
        HOSTS.iter().any(|h| line.contains(&format!("host {h}"))),
        "the click should have selected one of the fleet's hosts: {line}"
    );
}

#[test]
fn a_click_on_a_tab_switches_view() {
    let line = click("258", "28");
    assert!(line.contains("view hosts"), "the second tab is Hosts: {line}");
}

#[test]
fn a_click_on_the_theme_chip_switches_palette() {
    // The palette is reachable by mouse as well as by key, which is what
    // makes it checkable here at all.
    let line = click("390", "28");
    assert!(line.contains("theme paper"), "the chip switches palette: {line}");
    assert!(
        line.contains("view overview"),
        "and changes nothing else: {line}"
    );
}

#[test]
fn a_click_on_nothing_changes_nothing() {
    // Empty canvas, below every panel. A hit map that matched here would be
    // dispatching clicks to whatever used to be under the cursor.
    let line = click("700", "600");
    assert!(line.contains("no change"), "empty space is inert: {line}");
}

#[test]
fn honours_a_requested_size() {
    let out = common::workspace_root()
        .join("target")
        .join("it-metrics-console-small.ppm");
    let _ = std::fs::remove_file(&out);

    let stdout = run(&["--size", "320x200", "--snapshot", &out.to_string_lossy()]);
    assert!(
        stdout.contains("wrote 320x200 midnight"),
        "expected the requested size, got: {stdout}"
    );

    let ppm = std::fs::read_to_string(&out).expect("the snapshot file exists");
    let mut fields = ppm.split_ascii_whitespace().skip(1);
    assert_eq!(fields.next(), Some("320"));
    assert_eq!(fields.next(), Some("200"));
}

/// The windowed member, BUILT and not run.
///
/// Running it opens a window and waits for someone to close it, so this
/// asserts what has value: that the same `demo.render` code compiles behind
/// a real `minifb` window, across the crate boundary. Windows-only for the
/// same reason the three `minifb` examples are -- building `minifb` on Linux
/// wants X11 development headers a runner may not have. The Jux is portable.
#[test]
#[cfg(windows)]
fn the_windowed_front_end_builds() {
    let root = common::workspace_root();
    let output = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(root.join("examples").join("metrics_console"))
        .arg("build")
        .arg("-p")
        .arg("desktop")
        .output()
        .expect("spawning jux build -p desktop");

    assert!(
        output.status.success(),
        "demo.desktop failed to build with {:?}\n{}{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
