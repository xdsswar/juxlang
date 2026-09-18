//! Imports: reading a file's package and imports, and the edit that adds one.
//!
//! Shared by completion (a type accepted from another package brings its
//! `import`) and the auto-import quick fix.

use ropey::Rope;
use tower_lsp::lsp_types::*;

use crate::workspace::Workspace;

/// True when `text` already imports `fqn` (`import a.b.C;`, possibly with extra
/// whitespace or a trailing `as` alias on the same line). Used to dedupe the
/// auto-import edit so re-running the action is a no-op.
pub(crate) fn already_imports(text: &str, fqn: &str) -> bool {
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("import") else { continue };
        let rest = rest.trim().trim_end_matches(';').trim();
        // Match the exact path, or `a.b.C as X`.
        if rest == fqn || rest.starts_with(&format!("{fqn} ")) {
            return true;
        }
    }
    false
}

/// Build the [`TextEdit`] that inserts `import <fqn>;` at the right place:
/// immediately after the `package …;` line when present, else at the very top
/// of the file. The edit is a zero-width insertion (start == end) of a full
/// line. Returns `None` when `text` already imports `fqn`.
/// The dotted package declared at the top of `text` (`package a.b.c;` → `a.b.c`),
/// or `None` for a package-less file. Used to suppress auto-import suggestions
/// for a type that lives in the **same** package as the file being edited — a
/// same-package type needs no import (and a file must never offer to import the
/// very type it is declaring).
pub(crate) fn current_package(text: &str) -> Option<String> {
    let mut in_block_comment = false;
    for line in text.lines() {
        let mut t = line.trim_start();

        // Step over comments, including a BLOCK comment spanning lines. A file
        // that opens with `/** … */` -- a licence header or a type doc -- is
        // ordinary, and treating that first line as code ended the scan before
        // the `package` line was ever seen. The package then read as "none",
        // which made every same-package type look like it needed an import,
        // including the file's own.
        loop {
            if in_block_comment {
                match t.find("*/") {
                    Some(i) => {
                        in_block_comment = false;
                        t = t[i + 2..].trim_start();
                    }
                    None => {
                        t = "";
                        break;
                    }
                }
            } else if t.starts_with("/*") {
                in_block_comment = true;
                t = &t[2..];
            } else {
                break;
            }
        }
        if t.is_empty() || t.starts_with("//") {
            continue;
        }

        if let Some(rest) = t.strip_prefix("package") {
            // Require a word boundary after `package`.
            if rest.starts_with(char::is_whitespace) {
                let pkg = rest.trim().trim_end_matches(';').trim();
                if !pkg.is_empty() {
                    return Some(pkg.to_string());
                }
            }
        }
        // Real code before any `package` means the file has none.
        break;
    }
    None
}
pub(crate) fn import_edit(rope: &Rope, fqn: &str) -> Option<TextEdit> {
    let text = rope.to_string();
    if already_imports(&text, fqn) {
        return None;
    }
    // Locate the `package …;` declaration line, if any.
    let package_line = text
        .lines()
        .position(|line| line.trim_start().starts_with("package"));

    // Decide where the import goes and whether it needs a leading blank line.
    // Convention (Java-style): keep exactly one blank line between the
    // `package` declaration and the import block.
    let (insert_line, new_text) = match package_line {
        Some(p) => {
            // Is the line immediately after the package already blank? A
            // missing next line (package is the file's last line) counts as
            // "not blank" so we still insert the separating blank.
            let next_blank = text
                .lines()
                .nth(p + 1)
                .is_some_and(|l| l.trim().is_empty());
            if next_blank {
                // The separating blank already exists — drop the import just
                // below it (joining any existing import block).
                (p + 2, format!("import {fqn};\n"))
            } else {
                // No blank yet — insert one, then the import, so we get
                // `package …;` / <blank> / `import …;`.
                (p + 1, format!("\nimport {fqn};\n"))
            }
        }
        // Package-less file: imports go at the very top, no leading blank.
        None => (0, format!("import {fqn};\n")),
    };
    let pos = Position::new(insert_line as u32, 0);
    Some(TextEdit {
        range: Range::new(pos, pos),
        new_text,
    })
}

/// The auto-import edit for accepting workspace type `name`, when its
/// declaring package is unambiguous and differs from the open file's own.
pub(crate) fn auto_import_for(
    name: &str,
    ws: &Workspace,
    cur_pkg: Option<&str>,
    rope: &Rope,
) -> Option<TextEdit> {
    ws.type_packages
        .get(name)
        .map(|pkgs| {
            pkgs.iter()
                .filter(|p| Some(p.as_str()) != cur_pkg)
                .collect::<Vec<_>>()
        })
        .filter(|pkgs| pkgs.len() == 1)
        .and_then(|pkgs| import_edit(rope, &format!("{}.{name}", pkgs[0])))
}
