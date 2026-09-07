//! The multi-file examples: directory trees and sibling-file programs.
//!
//! None of these had ever been covered. A naive `examples/*.jux` sweep does
//! not see them, which is exactly why they were missed -- and why the
//! coverage gate looks at directories too.

use std::path::PathBuf;
use std::process::Command;

/// Compile and run a set of inputs through `juxc --build --run`.
///
/// These are not single-file programs, so `jux run <file>` cannot express
/// them: a directory tree needs the whole tree, and `examples/multifile`
/// holds four separate mini-programs side by side that each need their own
/// file list.
fn run(tag: &str, inputs: &[&str]) -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_juxc"));
    cmd.arg("--build")
        .arg("--run")
        .arg("--emit-dir")
        .arg(root.join("target").join(format!("it-{tag}")));
    for i in inputs {
        cmd.arg(root.join(i));
    }
    let output = cmd.output().unwrap_or_else(|e| panic!("spawning juxc for {tag}: {e}"));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{tag} exited with {:?}
{stdout}{stderr}",
        output.status.code(),
    );
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("juxc:"))
        .map(str::to_string)
        .collect()
}

#[test]
fn showcase() {
    let got = run("showcase", &["examples/showcase"]);
    assert_eq!(
        got,
        [
            "abs(-9) = 9",
            "max(3, 7) = 7",
            "PI = 3.14159",
            "circle (area 12.56636)",
            "square (area 9)",
            "triangle (area 10)",
            "circle (area 3.14159)",
            "square(10) = 100",
            "plus3(40) = 43",
        ],
    );
}

#[test]
fn stress_bank() {
    let got = run("stress_bank", &["examples/stress_bank"]);
    assert_eq!(
        got,
        [
            "--- Bank stress, CENTS_PER_UNIT=100 ---",
            "opened=3",
            "[Checking#1]",
            "[Savings#2]",
            "[Checking#3]",
            "[Checking#1] bal=518.0 USD",
            "[Savings#2] bal=210.0 USD",
            "[Checking#3] bal=150.0 EUR",
            "[Savings#2] interest: 7.35 USD, 7.35 USD",
            "interest runs total = 2",
            "DEPOSIT to Alice of 25.0 USD",
            "WITHDRAW from Alice of 7.0 USD",
            "[HUGE] WITHDRAW from Carol of 15000.0 EUR",
            "TRANSFER 0.50 USD from Bob to Alice",
            "INTEREST for Bob: 7.0 USD",
            "tx counted=5, huge=1",
            "STAMP-1 0.99 USD",
            "STAMP-2 manual entry",
            "stamp seq=2",
            "biggest balance holder = alice",
            "--- counters ---",
            "Account.opened=3",
            "Savings.interestRunsTotal=2",
            "TxLog.counted=5",
            "TxLog.hugeAmounts=1",
            "Stamp.sequence=2",
        ],
    );
}

#[test]
fn stress_demo() {
    let got = run("stress_demo", &["examples/stress_demo"]);
    assert_eq!(
        got,
        [
            "=== demo.app boot (FIXED_POINT=100) ===",
            "origin=(0, 0), step=(3, 4), trail=(12, 16)",
            "origin.isOrigin=true",
            "red=#FF0000 green=#00FF00",
            "ColorCodes.requested=2",
            "=== Renderer Output ===",
            "- Player Hero @ (1, 1) hp=100",
            "- Enemy Goblin @ (5, 5) hp=30",
            "- Pickup GoldCoin @ (3, 3) hp=1",
            ">>>>>>>> Verbose Frame <<<<<<<<",
            "** Player Hero @ (1, 1) hp=100 **",
            "** Enemy Goblin @ (5, 5) hp=30 **",
            "after tick: player.score=15, Player.totalScore=15",
            "Enemy.totalAttacks=3",
            "slot[the-answer]=42",
            "slot[greeting]=hello",
            "Slot.created=2",
            "world '<untitled>' score=53 grade=A",
            "SPAWN Goblin at (5, 5)",
            "hit Hero -12 hp",
            "HEAVY Hero -30 hp",
            "FATAL Goblin -75 hp",
            "COLLECT GoldCoin (+25)",
            "QUIT",
            "--- counters ---",
            "Entity.spawnCount=3",
            "Player.totalScore=15",
            "Enemy.totalAttacks=3",
            "ColorCodes.requested=2",
            "EventReporter.dispatched=6",
            "EventReporter.quitCount=1",
            "Slot.created=2",
            "World.turns=3",
            "UIFactory.builtCount=2",
        ],
    );
}

#[test]
fn stress_shop() {
    // `[user: <user>]` is the example's OWN documented Phase-1 limitation
    // (see the comment on `Loggable`): a default interface method calling an
    // overridable one does not dispatch to the override. Pinned as it stands,
    // so the day that is fixed this test says so rather than passing quietly.
    let got = run("stress_shop", &["examples/stress_shop"]);
    assert_eq!(
        got,
        [
            "[user: <user>]",
            "[user: <user>]",
            "[user: <user>]",
            "book Effective Jux @ 29.99 USD",
            "apparel T-Shirt (M) @ 15.0 USD",
            "subscription Pro Plan: 9.99 USD/mo x 12",
            "cart(<anonymous>): 3 lines, total 164.87 USD, max 64",
            "cart(<anonymous>): 1 lines, total 29.99 USD, max 64",
            "Person.created = 3",
            "Catalog.describedCount = 3",
            "Cart.allocated = 2",
        ],
    );
}

#[test]
fn mf_alias() {
    let got = run("mf_alias", &["examples/multifile/alias_app.jux", "examples/multifile/alias_lib.jux"]);
    assert_eq!(
        got,
        [
            "7",
            "1",
            "2",
        ],
    );
}

#[test]
fn mf_app() {
    let got = run("mf_app", &["examples/multifile/app.jux", "examples/multifile/greeter.jux"]);
    assert_eq!(
        got,
        [
            "Hello, multifile!",
        ],
    );
}

#[test]
fn mf_foo() {
    let got = run("mf_foo", &["examples/multifile/foo_app.jux", "examples/multifile/lib_foo.jux", "examples/multifile/lib_foo2.jux"]);
    assert_eq!(
        got,
        [
            "first",
            "second",
        ],
    );
}

#[test]
fn mf_polish() {
    // Cross-package inheritance: a qualified-path `new`, a bare-name `new`
    // after an import, and a subclass in ANOTHER package overriding a method.
    // Every one of the three used to fail to compile -- the parent's inner
    // struct, its `Kind` trait, and the subclass named in the base package's
    // upcast impl were all emitted without saying which package they were in.
    let got = run("mf_polish", &[
        "examples/multifile/polish_app.jux",
        "examples/multifile/polish_lib.jux",
    ]);
    assert_eq!(got, ["Animal Rex", "Animal Spot", "Woof, Fido"]);
}
