//! End-to-end test for **async streams (§18.6)** — `Stream<T>` with
//! `async T? next()`, `for await`, the of/from/generate constructors,
//! and the lazy consuming combinators.
//!
//! Runs `examples/async_streams.jux`:
//! - `Stream.of` + `for await` summation;
//! - `Stream.from` over an `ArrayList<String>`;
//! - `Stream.generate` with captured `AtomicInt` state, exhaustion via
//!   `return null`, and idempotent-null after exhaustion (`fuse`);
//! - `continue`/`break` in the loop body; labeled `break outer`;
//! - `mapAsync`/`filterAsync`/`take` chain; `skip` + `chain`;
//! - an empty stream (`take(0)`) whose body never runs;
//! - `for await` inside `try`/`finally` with a `break` (async-try
//!   loop-control interplay) accumulating through a shared handle;
//! - two handles to one stream pulling from the same source.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn async_streams_end_to_end() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("async_streams.jux");
    let emit_dir = workspace_root.join("target").join("it-async-streams");

    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("spawn jux");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "jux exited with {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(
        lines.as_slice(),
        [
            "sum 6",
            "name ada",
            "name grace",
            "gen 0",
            "gen 10",
            "gen 20",
            "gen 30",
            "after null",
            "even 2",
            "even 4",
            "controlFlow done",
            "cell 110",
            "cell 120",
            "cell 130",
            "cell 210",
            "labeled done",
            "combo 20",
            "combo 30",
            "tail 3",
            "tail 4",
            "tail 9",
            "empty ok",
            "cleanup",
            "seen 3",
            "shared 1 2",
        ],
        "unexpected output:\n{stdout}",
    );
}

/// The three stream diagnostics must be juxc-level, not rustc leaks:
/// E0703 (`for await` in a sync fn), E0704 (`for await` over a
/// non-stream), E0704 (plain `for` over a stream).
#[test]
fn stream_diagnostics_are_jux_level() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = workspace_root.join("probes").join("probe_stream_diags.jux");

    let output = Command::new(jux)
        .arg("check")
        .arg(&source)
        .output()
        .expect("spawn jux");
    assert!(!output.status.success(), "diagnostics probe must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[E0703]"), "missing E0703:\n{stderr}");
    assert_eq!(
        stderr.matches("[E0704]").count(),
        2,
        "expected E0704 in both directions:\n{stderr}",
    );
    assert!(!stderr.contains("error[E0"), "rustc leak:\n{stderr}");
}
