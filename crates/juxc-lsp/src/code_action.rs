//! `textDocument/codeAction`: the auto-import quick fix.
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
