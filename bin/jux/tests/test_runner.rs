//! End-to-end test for the **testing framework (§21 /
//! JUX-TESTING-ADDENDUM)** — `jux test` with assertions, lifecycle
//! hooks, an async test, a deliberate failure, and runtime filtering.
//!
//! Builds a scratch project under `target/it-jux-test-e2e/` with
//! packaged code under test (`src/com/test/mathapp/`), a test file
//! using `jux.std.testing`, and asserts:
//!
//! 1. the failing run exits 1 with exact PASS/FAIL lines and hook
//!    ordering (BeforeAll once → BeforeEach/AfterEach around every
//!    test → AfterAll once);
//! 2. the async `@Test` actually executes (prints from inside);
//! 3. a filtered run (`jux test addsCorrectly`) exits 0 and reports
//!    `filtered out`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn jux_test(cwd: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(cwd)
        .arg("test")
        .args(args)
        .output()
        .expect("spawn jux test");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr),
    )
}

#[test]
fn jux_test_full_lifecycle() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("target")
        .join("it-jux-test-e2e");
    let _ = std::fs::remove_dir_all(&root);

    write(
        &root.join("jux.toml"),
        "[package]\nname = \"com.test.mathapp\"\nversion = \"0.1.0\"\n",
    );
    write(
        &root.join("src/com/test/mathapp/math.jux"),
        "package com.test.mathapp;\n\npublic int add(int a, int b) {\n    return a + b;\n}\n\npublic int divide(int a, int b) {\n    return a / b;\n}\n",
    );
    write(
        &root.join("src/main.jux"),
        "import com.test.mathapp.{add};\n\npublic void main() {\n    print(add(2, 3));\n}\n",
    );
    write(
        &root.join("test/math_test.jux"),
        r#"import com.test.mathapp.{add, divide};
import jux.std.testing.{assertEqual, assertTrue, assertNear, assertThrows};

@BeforeAll
public void beforeAll() {
    print("[hook beforeAll]");
}

@BeforeEach
public void setUp() {
    print("[hook beforeEach]");
}

@AfterEach
public void tearDown() {
    print("[hook afterEach]");
}

@AfterAll
public void afterAll() {
    print("[hook afterAll]");
}

@Test
public void addsCorrectly() {
    assertEqual(5, add(2, 3));
    assertEqual(0, add(-1, 1));
}

@Test
public void divideThrowsOnZero() {
    var e = assertThrows(() -> divide(10, 0));
    assertTrue(e.getMessage().contains("zero"));
}

@Test
public void thisOneFails() {
    assertEqual(42, add(40, 1));
}

@Test
public async void asyncWorks() {
    var x = await asyncDouble(21);
    assertEqual(42, x);
    print("[async ran]");
}

async int asyncDouble(int n) {
    return n * 2;
}

@Test
public void floatsNear() {
    assertNear(0.3, 0.1 + 0.2);
}
"#,
    );

    // 1. Full run: one failure → exit 1, exact lifecycle output.
    let (ok, out) = jux_test(&root, &[]);
    assert!(!ok, "run with a failing test must exit non-zero:\n{out}");
    let lines: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("jux:"))
        .collect();
    assert_eq!(
        lines.as_slice(),
        [
            "running 5 tests",
            "[hook beforeAll]",
            "[hook beforeEach]",
            "[hook afterEach]",
            "PASS addsCorrectly",
            "[hook beforeEach]",
            "[hook afterEach]",
            "PASS divideThrowsOnZero",
            "[hook beforeEach]",
            "[hook afterEach]",
            "FAIL thisOneFails: assertEqual: expected `42`, got `41`",
            "[hook beforeEach]",
            "[async ran]",
            "[hook afterEach]",
            "PASS asyncWorks",
            "[hook beforeEach]",
            "[hook afterEach]",
            "PASS floatsNear",
            "[hook afterAll]",
            "test result: FAILED. 4 passed; 1 failed",
        ],
        "unexpected jux test output:\n{out}",
    );

    // 2. Filtered run: only the passing test → exit 0, filtered count.
    let (ok, out) = jux_test(&root, &["addsCorrectly"]);
    assert!(ok, "filtered run must pass:\n{out}");
    assert!(out.contains("running 1 tests"), "filter count:\n{out}");
    assert!(
        out.contains("test result: ok. 1 passed; 0 failed; 4 filtered out"),
        "filtered summary:\n{out}",
    );
    assert!(
        !out.contains("thisOneFails"),
        "filtered-out test must not run:\n{out}",
    );
}
