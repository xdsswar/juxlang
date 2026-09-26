//! `textDocument/codeAction`: the compiler's own fixes, and the auto-import
//! quick fix.
//!
//! A diagnostic that carries a §D.2.3 `code_action` brings it back in its
//! `data` slot (see `diagnostics::to_lsp`), and it is offered as it stands.
//!
//! For every resolution diagnostic (`E03xx`) the editor passes in, take the
//! identifier it points at; if that bare name is a known workspace type whose
//! package is not yet imported, offer an `import pkg.Name;` action.

use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::doc::Document;
use crate::imports::{current_package, import_edit};
use crate::text::word_at;
use crate::workspace::Workspace;

/// The quick fixes for `diagnostics` in `doc` (the document at `uri`).
pub(crate) fn code_actions(
    doc: &Document,
    ws: &Workspace,
    uri: &Url,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let text = doc.rope.to_string();
    let mut actions: Vec<CodeActionOrCommand> = Vec::new();
    let mut offered: HashSet<String> = HashSet::new();
    let cur_pkg = current_package(&text).or_else(|| doc.inferred_package.clone());

    for diag in diagnostics {
        if let Some(action) = compiler_fix(diag) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
        // Only resolution-phase diagnostics (`E03xx`) name an unresolved type.
        let is_resolution = matches!(
            &diag.code,
            Some(NumberOrString::String(c)) if c.starts_with("E03")
        );
        if !is_resolution {
            continue;
        }
        // The identifier the diagnostic points at: the word at its start.
        let start = doc.offset_at(diag.range.start);
        let Some(word) = word_at(&text, start) else { continue };
        let Some(pkgs) = ws.type_packages.get(&word.text) else { continue };
        for pkg in pkgs {
            // A same-package type needs no import; never offer to import a
            // type in this file's own package (or one this file declares).
            if Some(pkg.as_str()) == cur_pkg.as_deref() {
                continue;
            }
            let fqn = format!("{pkg}.{}", word.text);
            if !offered.insert(fqn.clone()) {
                continue;
            }
            let Some(edit) = import_edit(&doc.rope, &fqn) else { continue };
            let mut changes = HashMap::new();
            changes.insert(uri.clone(), vec![edit]);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Import `{fqn}`"),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
                is_preferred: Some(true),
                ..Default::default()
            }));
        }
    }
    actions
}

/// The fix the compiler attached to `diag`, read back out of its `data`.
/// Anything malformed is no fix at all rather than a partial one.
fn compiler_fix(diag: &Diagnostic) -> Option<CodeAction> {
    let action = diag.data.as_ref()?.get("code_action")?;
    let title = action.get("title")?.as_str()?.to_string();
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for edit in action.get("edits")?.as_array()? {
        let uri = Url::parse(edit.get("uri")?.as_str()?).ok()?;
        let range: Range = serde_json::from_value(edit.get("range")?.clone()).ok()?;
        let new_text = edit.get("newText")?.as_str()?.to_string();
        changes.entry(uri).or_default().push(TextEdit { range, new_text });
    }
    if changes.is_empty() {
        return None;
    }
    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
        is_preferred: Some(true),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A compiler fix comes back out of `data` as a quick fix that edits the
    /// ranges the compiler named, in the file it named.
    #[test]
    fn a_compiler_fix_in_data_becomes_a_quick_fix() {
        let diag = Diagnostic {
            range: Range::new(Position::new(1, 0), Position::new(1, 5)),
            code: Some(NumberOrString::String("E0464".to_string())),
            message: "cannot reassign".to_string(),
            data: Some(serde_json::json!({ "code_action": {
                "title": "Remove `final`",
                "edits": [{
                    "uri": "file:///t.jux",
                    "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 6 } },
                    "newText": ""
                }]
            }})),
            ..Default::default()
        };
        let action = compiler_fix(&diag).expect("the fix is offered");
        assert_eq!(action.title, "Remove `final`");
        let changes = action.edit.unwrap().changes.unwrap();
        let edits = &changes[&Url::parse("file:///t.jux").unwrap()];
        assert_eq!(edits[0].range.end, Position::new(0, 6));
        assert_eq!(edits[0].new_text, "");
    }

    /// No `data`, no compiler fix: the auto-import path is the only one left.
    #[test]
    fn a_diagnostic_without_data_has_no_compiler_fix() {
        assert!(compiler_fix(&Diagnostic::default()).is_none());
    }
}
