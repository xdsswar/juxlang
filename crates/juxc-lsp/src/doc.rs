//! In-memory document store entry.
//!
//! One [`Document`] per open editor buffer, keyed by `Url` in the server's
//! `DashMap`. It holds the live text as a [`Rope`] plus the cached results of
//! the last analysis pass that hover/completion read from — so those requests
//! never re-run the front end.

use std::sync::Arc;

use juxc_source::Span;
use juxc_tycheck::{SymbolTable, Ty};
use ropey::Rope;

/// A tracked open document and its last-analysis caches.
pub struct Document {
    /// Live buffer text. The skeleton uses full-document sync, so this is
    /// replaced wholesale on each `didChange`; the rope still gives O(log n)
    /// position lookups for hover/completion.
    pub rope: Rope,
    /// Client-reported version. Echoed back with published diagnostics today;
    /// the incremental-sync path (a later optimization) will read it to drop
    /// stale edits.
    #[allow(dead_code)]
    pub version: i32,
    /// Per-expression inferred types from the last analysis, used by hover.
    /// A `Vec` (not the source `HashMap`) because hover scans for the
    /// smallest span containing the cursor.
    pub expr_types: Vec<(Span, Ty)>,
    /// In-scope type names (last segment of each known FQN) offered by
    /// completion alongside the keyword list.
    pub type_names: Vec<String>,
    /// The merged workspace symbol table from the same analysis pass. Hover and
    /// member completion resolve the hovered/receiver identifier against this.
    pub symbols: Arc<SymbolTable>,
}

impl Document {
    /// Smallest cached expression type whose span contains `offset`.
    ///
    /// "Smallest" so that hovering inside a nested expression reports the
    /// innermost type (the identifier) rather than the enclosing call.
    pub fn type_at(&self, offset: usize) -> Option<(Span, &Ty)> {
        self.expr_types
            .iter()
            .filter(|(s, _)| (s.start as usize) <= offset && offset < (s.end as usize))
            .min_by_key(|(s, _)| s.len())
            .map(|(s, t)| (*s, t))
    }

    /// The inferred type of the receiver expression that *ends* at `offset`.
    ///
    /// Used for member completion after `<expr>.`: the cursor sits just past
    /// the `.`, so the receiver expression's span ends at the `.`'s offset
    /// (i.e. one byte before the cursor). We pick the LARGEST such span so a
    /// chained receiver like `a.b().c` resolves to the whole `a.b().c`
    /// expression's type rather than the inner `a`. Returns `None` when no
    /// expression ends there (the receiver type couldn't be inferred), so the
    /// caller falls back to the flat completion bag.
    pub fn type_ending_at(&self, offset: usize) -> Option<&Ty> {
        self.expr_types
            .iter()
            .filter(|(s, _)| s.end as usize == offset)
            .max_by_key(|(s, _)| s.len())
            .map(|(_, t)| t)
    }
}
