//! `textDocument/references`, `textDocument/prepareRename` and
//! `textDocument/rename`.
//!
//! All three start from [`target_at`]: resolve the identifier under the cursor
//! to a [`Target`] (see `xref`), then find every identifier across the
//! project that resolves to the same target. A local is looked for in its own
//! file only; a declaration or member in every project file the analysis
//! checked, read from the same text the analysis used (open buffers included),
//! so the ranges match the symbol table.
//!
//! Rename refuses rather than half-renames: the new name must be an
//! identifier that is not a keyword or a built-in type name, the declaration
//! must live in the project (not the standard library or a generated stub),
//! and the new name must not already mean something where the old one is
//! used. A public type renamed in the file named after it (`Widget.jux`, per
//! LANG-V1 §3.1) takes its file along when the client supports file renames.

use std::collections::HashMap;

use ropey::Rope;
use tower_lsp::lsp_types::*;

use crate::doc::Document;
use crate::position::span_to_range;
use crate::text::receiver_dot_before;
use crate::xref::{self, FileFacts, FileModel, Target};

/// One project file to search: its analysis index, URI, and text.
pub(crate) struct ProjectFile {
    /// Index in the analysis' source list.
    pub(crate) unit: u32,
    /// Where it lives.
    pub(crate) uri: Url,
    /// Its text as analysed.
    pub(crate) text: String,
}

/// The project's own files, as the analysis saw them. The open document comes
/// from its live buffer; the rest from the analysis' kept texts.
pub(crate) fn project_files(doc: &Document, uri: &Url) -> Vec<ProjectFile> {
    let mut out = Vec::new();
    for (i, path) in doc.source_paths.iter().enumerate() {
        let unit = i as u32;
        if doc.file == Some(unit) {
            out.push(ProjectFile { unit, uri: uri.clone(), text: doc.rope.to_string() });
        } else if let Some(Some(text)) = doc.source_texts.get(i) {
            if let Ok(file_uri) = Url::from_file_path(path) {
                out.push(ProjectFile { unit, uri: file_uri, text: text.to_string() });
            }
        }
    }
    out
}

/// The target under `pos` in `doc`, and the identifier's byte range.
pub(crate) fn target_at(doc: &Document, pos: Position) -> Option<(Target, (usize, usize))> {
    let text = doc.rope.to_string();
    let offset = doc.offset_at(pos);
    let model = FileModel::build(&text);
    let occ = model.idents.iter().find(|o| o.start <= offset && offset <= o.end)?.clone();
    let facts = FileFacts::new(&doc.symbols, &doc.expr_types, doc.file);
    let target = xref::resolve(&facts, &model, &occ)?;
    Some((target, (occ.start, occ.end)))
}

/// One file's hits: its URI, its text, and the byte ranges that refer to the
/// target.
type FileHits = (Url, String, Vec<(usize, usize)>);

/// Every occurrence of `target`, by file.
fn find_occurrences(doc: &Document, uri: &Url, target: &Target) -> Vec<FileHits> {
    let mut out = Vec::new();
    let files: Vec<ProjectFile> = match target {
        // A local lives in its own file.
        Target::Local { .. } => project_files(doc, uri)
            .into_iter()
            .filter(|f| &f.uri == uri)
            .collect(),
        _ => project_files(doc, uri),
    };
    // A local can be asked about before any analysis: search the buffer.
    let files = if files.is_empty() && matches!(target, Target::Local { .. }) {
        vec![ProjectFile { unit: 0, uri: uri.clone(), text: doc.rope.to_string() }]
    } else {
        files
    };
    for file in files {
        // A file can only mention a declaration or member by its name.
        if !file.text.contains(target.name()) {
            continue;
        }
        let model = FileModel::build(&file.text);
        let facts = FileFacts::new(&doc.symbols, &doc.expr_types, Some(file.unit));
        let hits = xref::occurrences_in(&facts, &model, target);
        if !hits.is_empty() {
            out.push((file.uri, file.text, hits));
        }
    }
    out
}

/// The byte range of the target's declaring identifier, and its file.
fn declaration_of(doc: &Document, uri: &Url, target: &Target) -> Option<(Url, (usize, usize))> {
    if let Target::Local { decl, .. } = target {
        return Some((uri.clone(), *decl));
    }
    let (unit, start) = xref::declaration_site(&doc.symbols, target)?;
    let file = project_files(doc, uri).into_iter().find(|f| f.unit == unit as u32)?;
    let model = FileModel::build(&file.text);
    let occ = model
        .idents
        .iter()
        .find(|o| o.start >= start as usize && o.name == target.name())?;
    Some((file.uri, (occ.start, occ.end)))
}

/// `textDocument/references` for `pos` in `doc`.
pub(crate) fn references(doc: &Document, uri: &Url, pos: Position, include_declaration: bool) -> Vec<Location> {
    let Some((target, _)) = target_at(doc, pos) else { return Vec::new() };
    let decl = if include_declaration { None } else { declaration_of(doc, uri, &target) };
    let mut out = Vec::new();
    for (file_uri, text, hits) in find_occurrences(doc, uri, &target) {
        let rope = Rope::from_str(&text);
        for (s, e) in hits {
            if decl.as_ref().is_some_and(|(du, dr)| du == &file_uri && *dr == (s, e)) {
                continue;
            }
            let span = juxc_source::Span::new(s as u32, e as u32);
            out.push(Location::new(file_uri.clone(), span_to_range(&rope, span, doc.enc)));
        }
    }
    out
}

/// True when the target's declaration is a project source: something the user
/// owns and may rename. The standard library and generated stubs are not.
fn renamable(doc: &Document, target: &Target) -> bool {
    match xref::declaration_site(&doc.symbols, target) {
        None => true, // a local
        Some((unit, _)) => {
            doc.file == Some(unit as u32)
                || matches!(doc.source_texts.get(unit), Some(Some(_)))
        }
    }
}

/// `textDocument/prepareRename`: the name's range and text when it can be
/// renamed, an error saying why not otherwise.
pub(crate) fn prepare_rename(doc: &Document, pos: Position) -> Result<Option<PrepareRenameResponse>, String> {
    let Some((target, (s, e))) = target_at(doc, pos) else {
        return Ok(None);
    };
    if !renamable(doc, &target) {
        return Err(format!(
            "`{}` is declared outside this project and cannot be renamed",
            target.name()
        ));
    }
    let range = doc.range_of(juxc_source::Span::new(s as u32, e as u32));
    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
        range,
        placeholder: target.name().to_string(),
    }))
}

/// True when `name` can be written as a Jux identifier and is not reserved.
pub(crate) fn valid_new_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c == '_' || c.is_ascii_alphabetic());
    if !first_ok || !chars.all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        return Err(format!("`{name}` is not a valid identifier"));
    }
    if juxc_lex::Keyword::lookup(name).is_some() || matches!(name, "true" | "false" | "null") {
        return Err(format!("`{name}` is a keyword"));
    }
    if juxc_lex::PRIMITIVE_TYPE_NAMES.contains(&name) {
        return Err(format!("`{name}` is a built-in type name"));
    }
    Ok(())
}

/// Why renaming `target` to `new_name` would change the program's meaning,
/// if it would.
fn conflict(doc: &Document, uri: &Url, target: &Target, new_name: &str) -> Option<String> {
    match target {
        Target::Local { decl, .. } => {
            // Where the local is visible, `new_name` must not already mean
            // something (another local, a member, a type): the renamed local
            // would capture it.
            let text = doc.rope.to_string();
            let model = FileModel::build(&text);
            let binding = model.bindings.iter().find(|b| b.decl == *decl)?;
            let (from, to) = binding.visible;
            if model.bindings.iter().any(|b| {
                b.name == new_name && b.decl != *decl && b.visible.0 < to && from < b.visible.1
            }) {
                return Some(format!("a variable named `{new_name}` is already in scope"));
            }
            let _ = uri;
            model
                .idents
                .iter()
                .filter(|o| o.name == new_name && from <= o.start && o.start < to)
                .find(|o| receiver_dot_before(&model.text, o.start).is_none())
                .map(|o| {
                    let line = doc.position_at(o.start).line + 1;
                    format!("`{new_name}` already refers to something else on line {line}")
                })
        }
        Target::Decl { fqn, .. } => {
            let renamed = match fqn.rsplit_once('.') {
                Some((pkg, _)) => format!("{pkg}.{new_name}"),
                None => new_name.to_string(),
            };
            let symbols = &doc.symbols;
            let taken = symbols.classes.contains_key(&renamed)
                || symbols.interfaces.contains_key(&renamed)
                || symbols.enums.contains_key(&renamed)
                || symbols.records.contains_key(&renamed)
                || symbols.aliases.contains_key(&renamed)
                || symbols.functions.contains_key(&renamed)
                || symbols.consts.contains_key(&renamed);
            taken.then(|| format!("`{renamed}` is already declared"))
        }
        Target::Member { owner, .. } => {
            let ty = juxc_tycheck::Ty::User { name: owner.clone(), generic_args: vec![] };
            crate::intel::member_owner(&doc.symbols, &ty, new_name)
                .map(|o| format!("`{}` already has a member named `{new_name}`", bare(&o)))
        }
    }
}

fn bare(fqn: &str) -> &str {
    fqn.rsplit('.').next().unwrap_or(fqn)
}

/// `textDocument/rename`: the edit renaming the target at `pos` to `new_name`.
/// `file_renames` says whether the client accepts a file rename in the edit.
pub(crate) fn rename(
    doc: &Document,
    uri: &Url,
    pos: Position,
    new_name: &str,
    file_renames: bool,
) -> Result<Option<WorkspaceEdit>, String> {
    let Some((target, _)) = target_at(doc, pos) else {
        return Ok(None);
    };
    if !renamable(doc, &target) {
        return Err(format!(
            "`{}` is declared outside this project and cannot be renamed",
            target.name()
        ));
    }
    valid_new_name(new_name)?;
    if new_name == target.name() {
        return Ok(Some(WorkspaceEdit::default()));
    }
    if let Some(why) = conflict(doc, uri, &target, new_name) {
        return Err(format!("cannot rename to `{new_name}`: {why}"));
    }

    let mut edits: Vec<(Url, Vec<TextEdit>)> = Vec::new();
    for (file_uri, text, hits) in find_occurrences(doc, uri, &target) {
        let rope = Rope::from_str(&text);
        let file_edits = hits
            .into_iter()
            .map(|(s, e)| TextEdit {
                range: span_to_range(&rope, juxc_source::Span::new(s as u32, e as u32), doc.enc),
                new_text: new_name.to_string(),
            })
            .collect();
        edits.push((file_uri, file_edits));
    }

    // A type declared in the file named after it moves with it (§3.1).
    let file_move = match (&target, file_renames) {
        (Target::Decl { kind, .. }, true) if kind.is_type() => {
            declaration_of(doc, uri, &target).and_then(|(decl_uri, _)| {
                let path = decl_uri.to_file_path().ok()?;
                (path.file_stem()?.to_str()? == target.name()).then(|| {
                    let new_path = path.with_file_name(format!("{new_name}.jux"));
                    Url::from_file_path(new_path).ok().map(|new_uri| (decl_uri, new_uri))
                })?
            })
        }
        _ => None,
    };

    match file_move {
        None => {
            let changes: HashMap<Url, Vec<TextEdit>> = edits.into_iter().collect();
            Ok(Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }))
        }
        Some((old_uri, new_uri)) => {
            // Text edits first (against the old names), then the file move.
            let mut ops: Vec<DocumentChangeOperation> = edits
                .into_iter()
                .map(|(file_uri, file_edits)| {
                    DocumentChangeOperation::Edit(TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier { uri: file_uri, version: None },
                        edits: file_edits.into_iter().map(OneOf::Left).collect(),
                    })
                })
                .collect();
            ops.push(DocumentChangeOperation::Op(ResourceOp::Rename(RenameFile {
                old_uri,
                new_uri,
                options: None,
                annotation_id: None,
            })));
            Ok(Some(WorkspaceEdit {
                document_changes: Some(DocumentChanges::Operations(ops)),
                ..Default::default()
            }))
        }
    }
}
