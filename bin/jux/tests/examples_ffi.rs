//! The `ffi_*` examples: the C boundary, which had no coverage at all.
//!
//! Three of the five link a Windows system library by name and are gated
//! accordingly; the other two are portable.

mod common;

#[test]
fn ffi_enum() {
    common::expect_output(
        "ffi_enum",
        "ffi-enum",
        &[
            "status=NotFound code=404",
        ],
    );
}

/// An exception reaching a C entry point (Exceptions §X.6.5) is reported with
/// its type and message and ends the process: it never unwinds into C. The
/// abort's exit status is platform specific, so only its absence of success,
/// the output before it, and the report are pinned.
#[test]
fn ffi_unwind_barrier() {
    let (lines, all) = common::run_example_expecting("ffi_unwind_barrier", "ffi-unwind-barrier", false);
    assert_eq!(lines, ["10", "-1", "42"], "output before the abort:\n{all}");
    assert!(
        all.contains(
            "Exception reached the C boundary in `a function pointer`, aborting: \
             jux.std.exceptions.IllegalArgumentException: negative: -3"
        ),
        "the barrier's report:\n{all}"
    );
    assert!(!all.contains("not reached"), "the program stopped at the barrier:\n{all}");
}

#[test]
fn ffi_export() {
    common::expect_output(
        "ffi_export",
        "ffi-export",
        &[
            "add=5 square=25",
            "Hello world (x3)",
        ],
    );
}

/// Links a Windows system library by name, so it is gated to Windows.
#[cfg(windows)]
#[test]
fn ffi_strings() {
    // The process id and the command line differ every run, so only the parts
    // the example is actually demonstrating are asserted: a `String` marshalled
    // out to `lstrlenA` (9 characters), and one marshalled back in.
    common::expect_contains("ffi_strings", "ffi-strings", &["len=9 pid=", "cmd="]);
}

/// Links a Windows system library by name, so it is gated to Windows.
#[cfg(windows)]
#[test]
fn ffi_struct() {
    // `GetCursorPos` fills the struct with the LIVE mouse position, so the
    // coordinates are whatever the pointer happens to be doing. Only the
    // parts a run can repeat are pinned: that the call succeeded, and that
    // the struct round-tripped through C and back.
    common::expect_contains(
        "ffi_struct",
        "ffi-struct",
        &[
            "ok=1 cursor=(",
            "a.x=99 b.x=1",
        ],
    );
}

/// Links a Windows system library by name, so it is gated to Windows.
#[cfg(windows)]
#[test]
fn ffi_variadic() {
    common::expect_output(
        "ffi_variadic",
        "ffi-variadic",
        &[
            "hi world, 2 + 3 = 5",
            "no extra args here",
        ],
    );
}

/// `jni_java_vm`: a real Java VM started from Jux, called through `JNIEnv`'s
/// table of function pointers (Layout-ABI §L.6.4). It needs a JDK, found through
/// `JAVA_HOME`: the linker takes `jvm.lib` / `libjvm.so` from its `lib`, and the
/// loader takes the library from `bin/server`. Without one the test says so and
/// passes, the way the Windows-only examples are simply not built elsewhere.
#[test]
fn jni_java_vm() {
    let Some(jdk) = std::env::var_os("JAVA_HOME").map(std::path::PathBuf::from) else {
        eprintln!("jni_java_vm: skipped, JAVA_HOME is not set");
        return;
    };
    let lib_dir = jdk.join("lib");
    let (import_lib, runtime_dir) = if cfg!(windows) {
        (lib_dir.join("jvm.lib"), jdk.join("bin").join("server"))
    } else {
        (lib_dir.join("server").join("libjvm.so"), lib_dir.join("server"))
    };
    if !import_lib.exists() {
        eprintln!("jni_java_vm: skipped, no {} in JAVA_HOME", import_lib.display());
        return;
    }
    let prepend = |var: &str, dir: &std::path::Path| {
        let mut paths = vec![dir.to_path_buf()];
        if let Some(old) = std::env::var_os(var) {
            paths.extend(std::env::split_paths(&old));
        }
        std::env::join_paths(paths).expect("joinable search path")
    };
    let (link_var, load_var) = if cfg!(windows) { ("LIB", "PATH") } else { ("LIBRARY_PATH", "LD_LIBRARY_PATH") };
    let link_dir = import_lib.parent().expect("library directory").to_path_buf();
    let root = common::workspace_root();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jux"))
        .arg("run")
        .arg("--emit-dir")
        .arg(root.join("target").join("it-jni-java-vm"))
        .arg(root.join("examples").join("jni_java_vm.jux"))
        .env(link_var, prepend(link_var, &link_dir))
        .env(load_var, prepend(load_var, &runtime_dir))
        .output()
        .expect("spawning jux for jni_java_vm");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let all = format!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "jni_java_vm failed:
{all}");
    for needle in [
        "JNI_CreateJavaVM -> 0",
        "JNI version ",
        "Math.abs(-42) = 42",
        "Integer.toHexString(48879) = beef",
        "raised; the handle is null: true",
        "DestroyJavaVM -> 0",
    ] {
        assert!(stdout.contains(needle), "jni_java_vm: missing {needle:?} in:
{all}");
    }
}
