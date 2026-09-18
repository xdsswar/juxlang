//! Declaration outlines: `textDocument/documentSymbol` for one file.

use juxc_source::Span;
use ropey::Rope;
use tower_lsp::lsp_types::*;

use crate::position::{span_to_range, PositionEncoding};

/// Map a top-level declaration to a hierarchical [`DocumentSymbol`] for the
/// editor outline: the declaration itself, with its members (methods, fields,
/// variants) nested as children. `range` covers the whole declaration;
/// `selection_range` is the name. Returns `None` for an unnameable item.
pub(crate) fn top_level_document_symbol(
    item: &juxc_ast::TopLevelDecl,
    rope: &Rope,
    enc: PositionEncoding,
) -> Option<DocumentSymbol> {
    use juxc_ast::TopLevelDecl as T;
    let sym = |name: &str, kind: SymbolKind, decl: Span, name_span: Span, children: Vec<DocumentSymbol>| {
        #[allow(deprecated)]
        DocumentSymbol {
            name: name.to_string(),
            detail: None,
            kind,
            tags: None,
            deprecated: None,
            range: span_to_range(rope, decl, enc),
            selection_range: span_to_range(rope, name_span, enc),
            children: if children.is_empty() { None } else { Some(children) },
        }
    };
    match item {
        // An annotation type shows in the outline with its parameters, the
        // way an interface shows its methods -- they are what a reader looks
        // up when writing `@Name(...)`.
        T::Annotation(a) => {
            let kids = a
                .params
                .iter()
                .map(|p| {
                    sym(&p.name.text, SymbolKind::FIELD, p.span, p.name.span, Vec::new())
                })
                .collect();
            Some(sym(&a.name.text, SymbolKind::INTERFACE, a.span, a.name.span, kids))
        }
        T::Class(c) => {
            let mut kids = Vec::new();
            for f in &c.fields {
                kids.push(sym(&f.name.text, SymbolKind::FIELD, f.span, f.name.span, Vec::new()));
            }
            for m in &c.methods {
                kids.push(sym(&m.name.text, SymbolKind::METHOD, m.span, m.name.span, Vec::new()));
            }
            // A `struct`-origin class reads as a STRUCT in the outline.
            let kind = if c.is_struct { SymbolKind::STRUCT } else { SymbolKind::CLASS };
            Some(sym(&c.name.text, kind, c.span, c.name.span, kids))
        }
        T::Interface(i) => {
            let kids = i
                .methods
                .iter()
                .map(|m| sym(&m.name.text, SymbolKind::METHOD, m.span, m.name.span, Vec::new()))
                .collect();
            Some(sym(&i.name.text, SymbolKind::INTERFACE, i.span, i.name.span, kids))
        }
        T::Enum(e) => {
            let kids = e
                .variants
                .iter()
                .map(|v| sym(&v.name.text, SymbolKind::ENUM_MEMBER, v.span, v.name.span, Vec::new()))
                .collect();
            Some(sym(&e.name.text, SymbolKind::ENUM, e.span, e.name.span, kids))
        }
        T::Record(r) => Some(sym(&r.name.text, SymbolKind::STRUCT, r.span, r.name.span, Vec::new())),
        T::Function(f) => {
            Some(sym(&f.name.text, SymbolKind::FUNCTION, f.span, f.name.span, Vec::new()))
        }
        T::Const(c) => Some(sym(&c.name.text, SymbolKind::CONSTANT, c.span, c.name.span, Vec::new())),
        T::TypeAlias(a) => {
            Some(sym(&a.name.text, SymbolKind::TYPE_PARAMETER, a.span, a.name.span, Vec::new()))
        }
        // A `@extern unsafe native { … }` block shows as a module-shaped node
        // named after the library, with each foreign function as a child.
        T::ExternBlock(b) => {
            let kids = b
                .fns
                .iter()
                .map(|f| sym(&f.name.text, SymbolKind::FUNCTION, f.span, f.name.span, Vec::new()))
                .collect();
            Some(sym(
                &format!("extern \"{}\"", b.lib),
                SymbolKind::MODULE,
                b.span,
                b.span,
                kids,
            ))
        }
    }
}

/// Where `workspace/symbol` finds declarations: a merged symbol table with the
/// paths and project texts of its units.
pub(crate) struct SymbolSource<'a> {
    /// The merged symbol table.
    pub(crate) symbols: &'a juxc_tycheck::SymbolTable,
    /// Paths parallel to the table's unit indices.
    pub(crate) paths: &'a [std::path::PathBuf],
    /// Project texts parallel to `paths` (`None` for the standard library and
    /// generated stubs, which the search leaves out).
    pub(crate) texts: &'a [Option<std::sync::Arc<str>>],
}

/// Cap on one answer: a picker shows a screenful, and an empty query over a
/// large project should not ship the whole symbol table.
const MAX_WORKSPACE_SYMBOLS: usize = 512;

/// True when every character of `query` appears in `name`, in order, ignoring
/// case: `wdg` finds `Widget`, the way editor pickers match.
fn fuzzy_match(name: &str, query: &str) -> bool {
    let mut chars = name.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .all(|q| chars.by_ref().any(|c| c == q))
}

/// `workspace/symbol`: the project's types, functions, constants and members
/// whose name matches `query`, located in their declaring files. Prefix
/// matches first, then the rest, each alphabetically.
pub(crate) fn workspace_symbols(src: &SymbolSource<'_>, query: &str, enc: PositionEncoding) -> Vec<SymbolInformation> {
    use std::collections::HashMap;
    let symbols = src.symbols;
    // (name, kind, unit, span, container)
    let mut found: Vec<(String, SymbolKind, usize, Span, Option<String>)> = Vec::new();
    let bare = |k: &str| k.rsplit('.').next().unwrap_or(k).to_string();
    let package = |k: &str| k.rsplit_once('.').map(|(p, _)| p.to_string());
    let unit_of = |k: &str| symbols.decl_unit.get(k).copied();
    let mut top = |key: &String, kind: SymbolKind, span: Span| {
        if let Some(unit) = unit_of(key) {
            found.push((bare(key), kind, unit, span, package(key)));
        }
    };
    for (k, c) in &symbols.classes {
        top(k, if c.is_struct { SymbolKind::STRUCT } else { SymbolKind::CLASS }, c.span);
    }
    for (k, i) in &symbols.interfaces {
        top(k, SymbolKind::INTERFACE, i.span);
    }
    for (k, e) in &symbols.enums {
        top(k, SymbolKind::ENUM, e.span);
    }
    for (k, r) in &symbols.records {
        top(k, SymbolKind::STRUCT, r.span);
    }
    for (k, f) in &symbols.functions {
        top(k, SymbolKind::FUNCTION, f.span);
    }
    for (k, c) in &symbols.consts {
        top(k, SymbolKind::CONSTANT, c.span);
    }
    // Members, with their type as the container.
    let mut member = |owner: &String, name: &String, kind: SymbolKind, span: Span| {
        if let Some(unit) = unit_of(owner) {
            found.push((name.clone(), kind, unit, span, Some(bare(owner))));
        }
    };
    for (owner, c) in &symbols.classes {
        for (n, m) in &c.methods {
            member(owner, n, if m.is_property { SymbolKind::PROPERTY } else { SymbolKind::METHOD }, m.span);
        }
        for (n, f) in &c.fields {
            member(owner, n, SymbolKind::FIELD, f.span);
        }
        for (n, p) in &c.properties {
            member(owner, n, SymbolKind::PROPERTY, p.span);
        }
    }
    for (owner, i) in &symbols.interfaces {
        for (n, m) in &i.methods {
            member(owner, n, SymbolKind::METHOD, m.span);
        }
    }
    for (owner, r) in &symbols.records {
        for (n, m) in &r.methods {
            member(owner, n, SymbolKind::METHOD, m.span);
        }
    }
    for (owner, e) in &symbols.enums {
        for (n, v) in &e.variants {
            member(owner, n, SymbolKind::ENUM_MEMBER, v.span);
        }
        for (n, m) in &e.methods {
            member(owner, n, SymbolKind::METHOD, m.span);
        }
    }

    let query_lower = query.to_lowercase();
    found.retain(|(name, _, unit, _, _)| {
        matches!(src.texts.get(*unit), Some(Some(_))) && fuzzy_match(name, query)
    });
    found.sort_by(|a, b| {
        let pa = !a.0.to_lowercase().starts_with(&query_lower);
        let pb = !b.0.to_lowercase().starts_with(&query_lower);
        (pa, &a.0, &a.4).cmp(&(pb, &b.0, &b.4))
    });
    found.truncate(MAX_WORKSPACE_SYMBOLS);

    let mut ropes: HashMap<usize, Rope> = HashMap::new();
    found
        .into_iter()
        .filter_map(|(name, kind, unit, span, container)| {
            let path = src.paths.get(unit)?;
            let uri = Url::from_file_path(path).ok()?;
            let text = src.texts.get(unit)?.as_ref()?;
            let rope = ropes.entry(unit).or_insert_with(|| Rope::from_str(text));
            #[allow(deprecated)]
            Some(SymbolInformation {
                name,
                kind,
                tags: None,
                deprecated: None,
                location: Location::new(uri, span_to_range(rope, span, enc)),
                container_name: container,
            })
        })
        .collect()
}
