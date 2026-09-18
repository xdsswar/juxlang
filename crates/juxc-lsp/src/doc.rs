//! In-memory document store entry.
//!
//! One [`Document`] per open editor buffer, keyed by `Url` in the server's
//! `DashMap`. It holds the live text as a [`Rope`] plus the cached results of
//! the last analysis pass that hover, completion, references and the rest read
//! from, so those requests never re-run the front end.
//!
//! The text and the analysis move at different speeds. Edits arrive as
//! incremental changes (§L.5) and are applied to the rope **immediately**, in
//! order; the analysis of a revision runs on a blocking thread and lands later.
//! [`Document::install`] only accepts an analysis for the revision the buffer
//! still holds, so a slow pass for an older revision can never overwrite the
//! caches of a newer one.

use std::path::PathBuf;
use std::sync::Arc;

use juxc_source::Span;
use juxc_tycheck::{SymbolTable, Ty};
use ropey::Rope;
use tower_lsp::lsp_types::{Position, Range};

use crate::analysis::Analysis;
use crate::position::{offset_to_position, position_to_offset, span_to_range, PositionEncoding};

/// A tracked open document and its last-analysis caches.
pub struct Document {
    /// Live buffer text, edited in place by incremental `didChange`.
    pub rope: Rope,
    /// Client-reported version of [`Self::rope`].
    pub version: i32,
    /// The column unit negotiated at `initialize` (§L.8). Every position this
    /// document reads or writes uses it.
    pub enc: PositionEncoding,
    /// Version of the text the caches below were computed from. Equal to
    /// [`Self::version`] once the latest analysis has landed.
    pub analysed_version: i32,
    /// Per-expression inferred types from the last analysis, for EVERY file of
    /// the compilation (a span carries its file index). Readers filter to
    /// [`Self::file`] through [`Self::type_at`] and [`Self::type_ending_at`].
    pub expr_types: Vec<(Span, Ty)>,
    /// This document's index in the analysis' source list, the `file` its
    /// spans carry. `None` before the first analysis.
    pub file: Option<u32>,
    /// In-scope type names (last segment of each known FQN) offered by
    /// completion alongside the keyword list.
    pub type_names: Vec<String>,
    /// The merged workspace symbol table from the same analysis pass.
    pub symbols: Arc<SymbolTable>,
    /// Source paths indexed parallel to `symbols.decl_unit`'s unit indices, so
    /// a resolved declaration maps to its file.
    pub source_paths: Arc<Vec<PathBuf>>,
    /// The text each analysed source had, parallel to [`Self::source_paths`]:
    /// `Some` for the project's own `.jux` files, `None` for the embedded
    /// standard library and generated stubs. Cross-file features (references,
    /// rename) read the other files from here, so they see exactly the text
    /// the symbol table was built from.
    pub source_texts: Arc<Vec<Option<Arc<str>>>>,
    /// The package this file's location implies under the editor's source
    /// roots (§I.4), used where the file declares none. `None` outside any
    /// root.
    pub inferred_package: Option<String>,
}

impl Document {
    /// A freshly opened buffer with no analysis yet.
    pub fn new(rope: Rope, version: i32, enc: PositionEncoding) -> Self {
        Document {
            rope,
            version,
            enc,
            analysed_version: i32::MIN,
            expr_types: Vec::new(),
            file: None,
            type_names: Vec::new(),
            symbols: Arc::new(SymbolTable::default()),
            source_paths: Arc::new(Vec::new()),
            source_texts: Arc::new(Vec::new()),
            inferred_package: None,
        }
    }

    /// A document with `analysis` installed as its caches, for `rope` at
    /// `version`: what a buffer looks like once its first analysis lands.
    #[cfg(test)]
    pub fn analysed(rope: Rope, version: i32, enc: PositionEncoding, analysis: Analysis) -> Self {
        let mut doc = Document::new(rope, version, enc);
        doc.install(version, analysis);
        doc
    }

    /// Install `analysis`, computed from revision `version`, as the caches.
    /// Returns `false`, changing nothing, when the buffer has moved past that
    /// revision: the newer revision's own analysis is the one that counts.
    pub fn install(&mut self, version: i32, analysis: Analysis) -> bool {
        if version != self.version {
            return false;
        }
        self.analysed_version = version;
        self.expr_types = analysis.expr_types;
        self.file = analysis.open_file;
        self.type_names = analysis.type_names;
        self.symbols = analysis.symbols;
        self.source_paths = analysis.source_paths;
        self.source_texts = analysis.source_texts;
        true
    }

    /// Byte offset of an editor position, in this document's encoding.
    pub fn offset_at(&self, pos: Position) -> usize {
        position_to_offset(&self.rope, pos, self.enc)
    }

    /// Editor position of a byte offset, in this document's encoding.
    pub fn position_at(&self, offset: usize) -> Position {
        offset_to_position(&self.rope, offset, self.enc)
    }

    /// Editor range of a span of this document.
    pub fn range_of(&self, span: Span) -> Range {
        span_to_range(&self.rope, span, self.enc)
    }

    /// True when `span` belongs to this document (the analysis indexes every
    /// file of the compilation, and offsets restart at zero in each).
    pub fn owns(&self, span: &Span) -> bool {
        self.file.map_or(true, |f| span.file == f)
    }

    /// This document's own expression types (the cache holds every file's).
    pub fn own_expr_types(&self) -> Vec<(Span, Ty)> {
        self.expr_types.iter().filter(|(s, _)| self.owns(s)).cloned().collect()
    }

    /// Smallest cached expression type in this document whose span contains
    /// `offset`.
    ///
    /// "Smallest" so that hovering inside a nested expression reports the
    /// innermost type (the identifier) rather than the enclosing call.
    pub fn type_at(&self, offset: usize) -> Option<(Span, &Ty)> {
        self.expr_types
            .iter()
            .filter(|(s, _)| self.owns(s))
            .filter(|(s, _)| (s.start as usize) <= offset && offset < (s.end as usize))
            .min_by_key(|(s, _)| s.len())
            .map(|(s, t)| (*s, t))
    }

    /// The inferred type of the receiver expression that *ends* at `offset`.
    ///
    /// Used for member completion after `<expr>.`: the receiver expression's
    /// span ends at the `.`'s offset. We pick the LARGEST such span so a
    /// chained receiver like `a.b().c` resolves to the whole expression's type
    /// rather than the inner `a`. `None` when no expression ends there.
    pub fn type_ending_at(&self, offset: usize) -> Option<&Ty> {
        self.expr_types
            .iter()
            .filter(|(s, _)| self.owns(s))
            .filter(|(s, _)| s.end as usize == offset)
            .max_by_key(|(s, _)| s.len())
            .map(|(_, t)| t)
    }
}
