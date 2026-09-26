//! Secondary labels and code actions (JUX-DIAGNOSTICS-ADDENDUM §D.1.3,
//! §D.2.1, §D.2.3), end to end through the `juxc` binary.
//!
//! The UI cases are rendered in the one-line `line` format, which has no room
//! for a label, so their `.expected` files cannot show one. These run the same
//! cases in `compact` (one `note:` line per label) and `json` (the
//! `secondary_spans` and `code_action` keys) and pin what each diagnostic now
//! points back at: the declaration a duplicate collides with, the `final` a
//! reassignment breaks.

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// `juxc --check <case> --diagnostic-format <format>` over a UI case, with the
/// case's path written as its bare file name.
fn check(case: &str, format: &str) -> String {
    let file = root().join("tests").join("ui").join(case);
    run(&file, format)
}

fn run(file: &PathBuf, format: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg("--check")
        .arg(file)
        .args(["--diagnostic-format", format])
        .output()
        .expect("running juxc");
    let name = file.file_name().unwrap().to_string_lossy().to_string();
    let path = file.display().to_string();
    // Text formats go to stderr, `json` to stdout.
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
        .replace(&path, &name)
        .replace(&path.replace('\\', "/"), &name)
}

#[test]
fn a_duplicate_member_points_at_the_first_declaration() {
    let got = check("duplicate_members.jux", "compact");
    assert!(
        got.contains(
            "duplicate_members.jux:7:13: error[E0401]: field `retries` is declared more than once in class `Config`\n\
             duplicate_members.jux:6:17: note: first declared here\n"
        ),
        "{got}"
    );
    assert!(
        got.contains(
            "duplicate_members.jux:13:5: error[E0403]: variant `Bronze` is declared more than once in enum `Tier`\n\
             duplicate_members.jux:11:5: note: first declared here\n"
        ),
        "{got}"
    );
}

#[test]
fn a_duplicate_local_points_at_the_first_one() {
    let got = check("duplicate_local.jux", "compact");
    assert!(got.contains("duplicate_local.jux:4:9: note: first declared here\n"), "{got}");
}

#[test]
fn a_duplicate_top_level_name_points_at_the_first_one() {
    let got = check("duplicate_declaration.jux", "compact");
    assert!(got.contains("[E0400]"), "{got}");
    assert!(got.contains("note: first declared here"), "{got}");
}

#[test]
fn a_final_reassignment_points_at_the_final_and_offers_to_remove_it() {
    let got = check("final_reassigned.jux", "compact");
    assert!(got.contains("final_reassigned.jux:9:29: note: `n` is declared `final` here\n"), "{got}");
    assert!(got.contains("final_reassigned.jux:4:12: note: `retries` is declared `final` here\n"), "{got}");

    let json = check("final_reassigned.jux", "json");
    let e0464 = json.lines().find(|l| l.contains("\"code\":\"E0464\"")).expect("an E0464 line");
    assert!(e0464.contains("\"secondary_spans\":[{"), "{e0464}");
    // `final ` in `public void takes(final int n)`: the keyword and its space.
    assert!(e0464.contains("\"code_action\":{\"title\":\"Remove `final` from `n`\",\"edits\":[{\"file\":"), "{e0464}");
    assert!(e0464.contains("\"replacement\":\"\""), "{e0464}");
    let e0465 = json.lines().find(|l| l.contains("\"code\":\"E0465\"")).expect("an E0465 line");
    assert!(e0465.contains("\"title\":\"Make `retries` reassignable\""), "{e0465}");
}

#[test]
fn a_final_method_override_points_at_the_final_method() {
    let got = check("inheritance_errors.jux", "compact");
    assert!(got.contains("inheritance_errors.jux:21:12: note: declared `final` here\n"), "{got}");
}

/// The fix deletes exactly the modifier and the space after it.
#[test]
fn the_remove_final_edit_covers_the_keyword_and_its_space() {
    let text = std::fs::read_to_string(root().join("tests").join("ui").join("final_reassigned.jux")).unwrap();
    let json = check("final_reassigned.jux", "json");
    let fixes: Vec<&str> = json.lines().filter(|l| l.contains("\"code_action\"")).collect();
    assert_eq!(fixes.len(), 3, "{json}");
    for line in fixes {
        let start = number_after(line, "\"byte_start\":", line.find("\"code_action\"").unwrap());
        let end = number_after(line, "\"byte_end\":", line.find("\"code_action\"").unwrap());
        assert_eq!(&text[start..end], "final ", "{line}");
    }
}

/// A mistyped import whose leaf names a real type offers to import that one.
#[test]
fn a_mistyped_import_offers_the_real_path() {
    let dir = root().join("target").join("it-secondary-labels");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("imp.jux");
    std::fs::write(&file, "import wrong.place.HashMap;\n\npublic void main() {\n    print(1);\n}\n").unwrap();
    let json = run(&file, "json");
    let line = json.lines().find(|l| l.contains("\"code\":\"E0301\"")).expect("an E0301 line");
    assert!(line.contains("\"title\":\"Import `rust.std.HashMap` instead\""), "{line}");
    assert!(line.contains("\"byte_start\":7,\"byte_end\":26,\"replacement\":\"rust.std.HashMap\""), "{line}");
}

fn number_after(line: &str, key: &str, from: usize) -> usize {
    let at = line[from..].find(key).map(|i| from + i + key.len()).expect("key present");
    line[at..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap()
}
