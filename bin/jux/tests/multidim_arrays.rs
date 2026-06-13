//! Integration test for multi-dimensional arrays (§5.5).
//!
//! Builds and runs `examples/multidim_arrays.jux`, asserting the exact
//! deterministic output, driving:
//!
//! - `int[][]` two-dimensional dynamic array (lowers to `Vec<Vec<isize>>`),
//! - `int[][][]` three-dimensional dynamic array,
//! - `new int[r][c]` / `new int[2][2][2]` multi-dim construction,
//! - indexed write `m[i][j] = v;` and read `m[i][j]`,
//! - `.length` on both the outer and inner dimensions.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn multidim_arrays_build_write_read_and_length() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("multidim_arrays.jux");
    let emit_dir = workspace_root.join("target").join("it-multidim-arrays");

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
            "2",  // grid.length (outer dim = rows)
            "3",  // grid[0].length (inner dim = cols)
            // grid[i][j] = i*10 + j, row-major:
            "0", "1", "2",     // row 0
            "10", "11", "12",  // row 1
            "42", // cube[1][0][1] after the single write
            "42", // sum of all cube cells (proves zero-init elsewhere)
        ],
        "unexpected output:\n{stdout}",
    );
}
