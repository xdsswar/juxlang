//! Diagnostics — error / warning / note types and the master E-code catalog.
//!
//! ## What this crate owns
//!
//! - The **shape** of a diagnostic — what fields it carries, what severity
//!   levels exist, how labels and help text attach.
//! - The **stable identity** of every error — the [`code::Code`] enum is the
//!   single Rust-side source of truth for E-numbers. Adding a code here is
//!   a spec change; allocate the number in `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4
//!   **first**, then expose it here.
//!
//! ## What this crate does NOT own
//!
//! - Rendering. The terminal/JSON renderer is a separate concern; this crate
//!   just hands you a `Diagnostic` you can pass to a renderer or accumulate
//!   in a `Vec`. The renderer reads the spec for format (`§D.1.3` etc.) and
//!   makes the bytes.
//! - Lifetimes / phase ordering. Any phase can construct a `Diagnostic`; the
//!   driver decides how to surface them.
//!
//! ## Code allocation by compiler phase
//!
//! Per `JUX-DIAGNOSTICS-ADDENDUM.md` §D.3:
//!
//! | Range           | Phase                                  |
//! |-----------------|----------------------------------------|
//! | `E0100–E0199`   | Lexical                                |
//! | `E0200–E0299`   | Syntax                                 |
//! | `E0300–E0399`   | Name resolution / modules              |
//! | `E0400–E0499`   | Type checking                          |
//! | `E0500–E0599`   | Borrow checker                         |
//! | `E0600–E0699`   | Lowering (drop / move / refcount)      |
//! | `E0700–E0799`   | Async / generators                     |
//! | `E0800–E0899`   | Const evaluation                       |
//! | `E0900–E0999`   | Backend / codegen                      |

use juxc_source::Span;

pub mod code;

/// A single diagnostic — error, warning, note, or help.
///
/// Format and JSON schema are normative per `JUX-DIAGNOSTICS-ADDENDUM.md`
/// §D.1–§D.2. This struct is the in-memory representation; rendering to
/// terminal or JSON happens in a downstream sink. The builder methods
/// ([`Diagnostic::with_span`], [`Diagnostic::with_label`],
/// [`Diagnostic::with_help`]) let phases construct diagnostics in a single
/// expression without an intermediate `mut` binding.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Stable E-code identifying this diagnostic. Tooling keys off this.
    pub code: code::Code,
    /// Whether this is an error, warning, note, or help suggestion.
    pub severity: Severity,
    /// One-line summary of the diagnostic, shown next to the code.
    pub message: String,
    /// The primary span the diagnostic points at, if any. Synthesized
    /// diagnostics (no source to point at) leave this `None`.
    pub primary_span: Option<Span>,
    /// Index of the source file this diagnostic belongs to, within the
    /// workspace source list (stdlib units first, then user units — the
    /// same ordering the driver/tycheck use). `None` when the diagnostic
    /// cannot be attributed to a single source (e.g. a cross-unit
    /// symbol-table conflict). [`Span`] itself is file-relative and
    /// carries no file id, so this is the only place file identity lives.
    pub file: Option<usize>,
    /// Secondary spans with explanatory captions. Per the spec these are
    /// the "labels" surfaced under the underline.
    pub labels: Vec<Label>,
    /// `help:` lines — concrete suggestions for fixing the problem.
    pub help: Vec<String>,
    /// `note:` lines — clarifying remarks that aren't actionable themselves.
    pub notes: Vec<String>,
}

/// Severity of a diagnostic. Per `JUX-DIAGNOSTICS-ADDENDUM.md` §D.1.2.
///
/// `Error` halts the build at the next phase boundary (subject to
/// error-recovery policy); `Warning`, `Note`, and `Help` never do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A bug in the user's program. The compilation cannot produce a binary.
    Error,
    /// Suspicious construct the compiler still accepts.
    Warning,
    /// Clarifying information attached to another diagnostic.
    Note,
    /// A concrete, actionable suggestion.
    Help,
}

/// A captioned secondary span attached to a diagnostic.
#[derive(Debug, Clone)]
pub struct Label {
    /// The source range this label highlights.
    pub span: Span,
    /// The caption rendered alongside the highlighted region.
    pub message: String,
}

impl Diagnostic {
    /// Start building an error-severity diagnostic with the given code and
    /// summary message. The result has no span, labels, help, or notes yet —
    /// chain the `with_*` builders to add them.
    pub fn error(code: code::Code, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            primary_span: None,
            file: None,
            labels: Vec::new(),
            help: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Attach the primary span the diagnostic points at.
    pub fn with_span(mut self, span: Span) -> Self {
        self.primary_span = Some(span);
        self
    }

    /// Attach the workspace source index this diagnostic belongs to. The
    /// driver/tycheck tag freshly-produced diagnostics with this so
    /// consumers (CLI, LSP) can map index → path + compute line:col.
    pub fn with_file(mut self, file: usize) -> Self {
        self.file = Some(file);
        self
    }

    /// Add a captioned secondary label at `span`.
    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label { span, message: message.into() });
        self
    }

    /// Add a `help:` suggestion line.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help.push(help.into());
        self
    }
}
