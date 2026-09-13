//! `examples/nested_enums` -- an enum declared inside a class, used from
//! another package through its owner (`Order.Status.Shipped`), in full
//! (`shop.orders.Order.Status.Delivered`), and as a local and parameter type.
//!
//! JUX-MISSING-DEFS M.9.4 listed qualified access to a nested enum's variants
//! as deferred. It is run end to end because the failures were in three
//! different places: the type position, the `case` pattern, and the value.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn nested_enum_variants_are_reachable_through_their_owner() {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .join("examples")
        .join("nested_enums");

    let out = Command::new(env!("CARGO_BIN_EXE_jux"))
        .current_dir(&project)
        .arg("run")
        .output()
        .expect("spawning jux run for nested_enums");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "nested_enums failed with {:?}\n{stdout}{stderr}",
        out.status.code(),
    );

    let lines: Vec<&str> = stdout.lines().filter(|l| !l.starts_with("jux: ")).collect();
    assert_eq!(
        lines,
        ["pending -> shipped", "shipped? true", "delivered? true"],
    );
}
