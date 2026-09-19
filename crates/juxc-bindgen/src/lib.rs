//! `juxc-bindgen` — generates Jux-syntax interface stubs (`.jux.d`) from
//! foreign APIs. Implements JUX-BINDGEN-ADDENDUM.md §G.
//!
//! Pipeline:
//!
//! ```text
//! rustdoc JSON ──ingest──▶ stub IR (model) ──emit──▶ .jux.d text
//! ```
//!
//! - [`ty`] — the [`JuxType`] representation and its Jux-syntax rendering (§G.3).
//! - [`naming`] — snake→camel, module-path→package, keyword escaping (§G.4).
//! - [`model`] — the language-agnostic stub IR (§G.2 / §G.5).
//! - [`emit`] — renders the IR to signature-only `.jux.d` text.
//! - [`ingest`] — builds the IR from a rustdoc-JSON crate (§G.6).
//! - [`source`] — the second source: `alloc`'s inherent slice and `str`
//!   methods, read from the toolchain's library source (§G.6.4.4).
//!
//! The first four modules are pure and independent of the rustdoc schema, so
//! the spec's mapping rules are unit-tested on plain data.

pub mod emit;
pub mod ingest;
pub mod model;
pub mod naming;
pub mod source;
pub mod ty;

pub use model::StubFile;
pub use ty::JuxType;

/// Render a stub file straight to `.jux.d` text. Convenience over
/// [`emit::render`] for callers that already hold a [`StubFile`].
pub fn render_stub(file: &StubFile) -> String {
    emit::render(file)
}
