//! End-to-end test for **varargs** (`T... name`, §7.2 / §E.1.2.1).
//!
//! Runs `examples/varargs.jux`: zero-arg / one-arg / many-arg variadic
//! calls, array passthrough (`sum(nums)` forwards the `int[]` without
//! re-packing), a fixed-prefix + variadic mix on a free function and
//! on a static method.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn varargs_pack_and_passthrough() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("varargs.jux");
    let emit_dir = workspace_root.join("target").join("it-varargs");

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
        ["0", "1", "10", "60", "[info] one", "[info] two", "5", "9"],
        "unexpected output:\n{stdout}",
    );

    // `main(String... args)` entry form: the emitted exe receives real
    // command-line args through the generated shim (program name
    // excluded). Reuses the already-built probe source.
    let args_src = workspace_root.join("target").join("it-varargs-main.jux");
    std::fs::write(
        &args_src,
        "package probe;\n\npublic void main(String... args) {\n    print(\"argc \" + args.length);\n}\n",
    )
    .expect("write main-args probe");
    let args_emit = workspace_root.join("target").join("it-varargs-main");
    let build = Command::new(jux)
        .arg("build")
        .arg("--emit-dir")
        .arg(&args_emit)
        .arg(&args_src)
        .output()
        .expect("spawn jux build");
    let build_out = String::from_utf8_lossy(&build.stdout);
    let build_err = String::from_utf8_lossy(&build.stderr);
    assert!(
        build.status.success(),
        "jux build failed: {:?}\nstderr:\n{build_err}\nstdout:\n{build_out}",
        build.status.code(),
    );
    let exe = args_emit.join("target").join("debug").join(format!(
        "jux_emitted{}",
        std::env::consts::EXE_SUFFIX
    ));
    let run = Command::new(&exe)
        .args(["alpha", "beta", "gamma"])
        .output()
        .expect("run emitted exe");
    let run_out = String::from_utf8_lossy(&run.stdout);
    assert_eq!(run_out.trim(), "argc 3", "unexpected args output:\n{run_out}");
}
