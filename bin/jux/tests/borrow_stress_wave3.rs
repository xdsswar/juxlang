//! End-to-end regression runners for **borrow-stress wave 3**
//! (`jux-gaps.md` S1–S15) — the adversarial probes that drove the
//! borrow-checker release-hardening pass. Each test pins the exact
//! output of one promoted `examples/stress_NN_*.jux` probe:
//!
//! - S1  observer re-entrant self-set chains to quiescence (§P.3.6)
//! - S3  collection-typed auto-property mutates the live backing field
//! - S4  property compound assign routes get-op-set and fires observers
//! - S5  stdlib forEach with a self-mutating lambda (snapshot, no panic)
//! - S7  field-path receivers keep in-place mutation (no silent clones)
//! - S8  catch binder stored into a nullable base-typed field upcasts
//! - S9  mid-body returns inside a valued lambda try (inferred channel)
//! - S10 async try share-clones wrapper captures (use-after-try works)
//! - S11 statement-position `super.method(args)` parses and dispatches
//! - S12 plain `g[i] = …self-referential…` index-assign hoists its RHS
//! - S13 nullable thread_local static write wraps in `Some(…)`
//! - S15 getter returning an own collection field clones (no move-out)
//! - plus the three probes that always passed (S2 / S6 / S14), kept as
//!   regression anchors for observer-sibling writes, nested lambda
//!   captures, and fluent mutating chains.

use std::path::PathBuf;
use std::process::Command;

/// Run one promoted stress example; assert success and return the
/// trimmed, non-empty stdout lines.
fn run_stress(example: &str, emit_tag: &str) -> Vec<String> {
    let jux = env!("CARGO_BIN_EXE_jux");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let source = root.join("examples").join(example);
    let emit_dir = root.join("target").join(emit_tag);

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
        "{example}: jux exited with {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn s01_observer_self_set_chains_to_quiescence() {
    let lines = run_stress("stress_01_observer_self_set.jux", "it-w3-s01");
    assert_eq!(lines, ["5"]);
}

#[test]
fn s02_observer_writes_sibling_collection_field() {
    let lines = run_stress("stress_02_observer_sibling_field.jux", "it-w3-s02");
    assert_eq!(lines, ["2", "7", "9"]);
}

#[test]
fn s03_collection_property_mutates_live_backing_field() {
    let lines = run_stress("stress_03_collection_property_mutate.jux", "it-w3-s03");
    assert_eq!(lines, ["2", "3", "4"]);
}

#[test]
fn s04_property_compound_assign_fires_observer() {
    let lines = run_stress("stress_04_property_compound_assign.jux", "it-w3-s04");
    assert_eq!(lines, ["fire: 0 -> 1", "1"]);
}

#[test]
fn s05_foreach_lambda_mutating_iterated_object() {
    let lines = run_stress("stress_05_foreach_lambda_mutates.jux", "it-w3-s05");
    assert_eq!(lines, ["6", "3"]);
}

#[test]
fn s06_nested_lambda_capture_shares_object() {
    let lines = run_stress("stress_06_nested_lambda_capture.jux", "it-w3-s06");
    assert_eq!(lines, ["2"]);
}

#[test]
fn s07_field_path_receiver_mutations_persist() {
    let lines = run_stress("stress_07_arg_hoist_reach.jux", "it-w3-s07");
    assert_eq!(lines, ["3", "3"]);
}

#[test]
fn s08_catch_binder_into_nullable_base_field() {
    let lines = run_stress("stress_08_catch_binder_alias.jux", "it-w3-s08");
    assert_eq!(lines, ["x", "outer: x", "x"]);
}

#[test]
fn s09_lambda_try_mid_body_returns() {
    let lines = run_stress("stress_09_try_expr_break.jux", "it-w3-s09");
    assert_eq!(lines, ["10", "20", "done"]);
}

#[test]
fn s10_async_try_share_clones_wrapper_captures() {
    let lines = run_stress("stress_10_async_wrapped_across_await.jux", "it-w3-s10");
    assert_eq!(lines, ["3"]);
}

#[test]
fn s11_super_method_statement_dispatch() {
    let lines = run_stress("stress_11_super_mutating_arg.jux", "it-w3-s11");
    assert_eq!(lines, ["11"]);
    let lines = run_stress("stress_11b_super_stmt.jux", "it-w3-s11b");
    assert_eq!(lines, ["base", "derived"]);
}

#[test]
fn s12_operator_self_referential_operands() {
    let lines = run_stress("stress_12_operator_self_ref.jux", "it-w3-s12");
    assert_eq!(lines, ["12", "11"]);
}

#[test]
fn s13_nullable_threadlocal_static_some_wrap() {
    let lines = run_stress("stress_13_threadlocal_static_self.jux", "it-w3-s13");
    assert_eq!(lines, ["1", "5"]);
}

#[test]
fn s14_fluent_mutating_chain_on_wrapped_field() {
    let lines = run_stress("stress_14_fluent_chain_wrapped.jux", "it-w3-s14");
    assert_eq!(lines, ["abc", "abc"]);
}

#[test]
fn s15_foreach_over_getter_with_structural_mutation() {
    let lines = run_stress("stress_15_foreach_getter_mutation.jux", "it-w3-s15");
    assert_eq!(lines, ["1", "2", "3", "0"]);
}
