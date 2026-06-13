//! Foreign-item IR — the language-agnostic shape of a stub.
//!
//! `ingest` builds these from rustdoc JSON (§G.6.3 kind selection); `emit`
//! renders them as `.jux.d` text (§G.2). Keeping the IR separate from both
//! ends means the type mapping (§G.3), naming (§G.4), and member mapping
//! (§G.5) are all exercised on plain data, independent of the rustdoc schema.

use crate::ty::JuxType;

/// Jux visibility modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vis {
    Public,
    Protected,
    Private,
    /// No modifier (package-private).
    Package,
}

impl Vis {
    /// The keyword plus trailing space, or empty for package-private.
    pub fn prefix(self) -> &'static str {
        match self {
            Vis::Public => "public ",
            Vis::Protected => "protected ",
            Vis::Private => "private ",
            Vis::Package => "",
        }
    }
}

/// Which kind of type declaration a stub renders to (§G.6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Class,
    Interface,
    Struct,
    Record,
    Enum,
}

/// A generated stub file: one package, many top-level declarations.
#[derive(Debug, Clone, Default)]
pub struct StubFile {
    /// Dotted package path, or empty.
    pub package: String,
    /// `// bindgen-version: N` style provenance header lines (optional).
    pub header: Vec<String>,
    pub items: Vec<StubItem>,
}

/// A top-level stub declaration.
#[derive(Debug, Clone)]
pub enum StubItem {
    Type(StubType),
    Function(StubFn),
    Const(StubConst),
}

/// A class / interface / struct / record / enum stub.
#[derive(Debug, Clone)]
pub struct StubType {
    pub kind: TypeKind,
    pub name: String,
    /// Generic parameter names (bounds elided in this slice).
    pub generics: Vec<String>,
    pub fields: Vec<StubField>,
    pub constructors: Vec<StubCtor>,
    pub methods: Vec<StubFn>,
    /// Enum variants (empty for non-enums).
    pub variants: Vec<StubVariant>,
    /// First line of the foreign doc comment, if any.
    pub doc: Option<String>,
    /// The **real** fully-qualified Rust path of this type
    /// (`std::collections::HashSet`, `serde_json::Value`), from the rustdoc
    /// item summary. Emitted as a `@rust("…")` annotation so the backend can
    /// lower a reference to this external type to its true Rust path (§G.9.2) —
    /// the Jux-facing package (`rust.std`) is flat for autocomplete and does not
    /// reflect the real module path. `None` when unavailable.
    pub rust_path: Option<String>,
}

impl StubType {
    /// A bare type shell of the given kind and name.
    pub fn new(kind: TypeKind, name: impl Into<String>) -> StubType {
        StubType {
            kind,
            name: name.into(),
            generics: Vec::new(),
            fields: Vec::new(),
            constructors: Vec::new(),
            methods: Vec::new(),
            variants: Vec::new(),
            doc: None,
            rust_path: None,
        }
    }
}

/// A field declaration (no initializer — stubs are signature-only).
#[derive(Debug, Clone)]
pub struct StubField {
    pub visibility: Vis,
    pub name: String,
    pub ty: JuxType,
}

/// A constructor stub (`new`-style; method name = class name, §G.5.1).
#[derive(Debug, Clone)]
pub struct StubCtor {
    pub visibility: Vis,
    pub name: String,
    pub params: Vec<StubParam>,
}

/// A method / free-function stub.
#[derive(Debug, Clone)]
pub struct StubFn {
    pub visibility: Vis,
    pub is_static: bool,
    /// Interface `default` method (§G.6.4).
    pub is_default: bool,
    pub name: String,
    pub generics: Vec<String>,
    pub params: Vec<StubParam>,
    pub ret: JuxType,
    /// `throws E`, set when the Rust return was `Result<_, E>` (§G.5.4).
    pub throws: Option<JuxType>,
    /// The Rust function is `unsafe`. Rendered as the Jux `unsafe` modifier
    /// (§A.2.4) so the front end records it and the type checker requires an
    /// `unsafe` context at every call site (§A.2.8).
    pub is_unsafe: bool,
    /// The Rust receiver is `&mut self` — the method MUTATES the value it is
    /// called on. Rendered as a `@MutSelf` annotation on the stub method so
    /// the compiler can DISCOVER receiver mutability from the real library
    /// signatures (driving `let mut` promotion and the wrapper-field
    /// `borrow_mut()` upgrade) instead of relying on hardcoded method-name
    /// lists that drift when the library changes.
    pub is_mut_self: bool,
    /// The **real** fully-qualified Rust path of a free function
    /// (`humantime::parse_duration`), from the rustdoc summary. Rendered as a
    /// `@rust("…")` annotation so the backend lowers the import to
    /// `use humantime::parse_duration as parseDuration;` — bridging the
    /// snake_case Rust name to the camelCase Jux stub name. `None` for methods
    /// (dispatched on the type) and when unavailable.
    pub rust_path: Option<String>,
    pub doc: Option<String>,
}

/// A single parameter (`ty name`).
#[derive(Debug, Clone)]
pub struct StubParam {
    pub name: String,
    pub ty: JuxType,
    /// The Rust parameter was a borrow (`&T` / `&mut T`, a non-slice
    /// `BorrowedRef`). The Jux type still drops the `&` (§G.3.4), but the flag is
    /// retained so codegen can re-attach the call-site borrow when invoking the
    /// foreign method (§G.9.2): a Rust `contains_key(&Q)` needs `&arg`, not
    /// `arg`. Surfaced in the `.jux.d` as a leading `&` marker that the parser
    /// consumes back into a per-parameter flag.
    pub by_ref: bool,
}

/// An enum variant. `payload` empty = unit variant; `discriminant` set for
/// C-like enums (§7.7.1 Pattern B/C). Note: no `case` keyword (Jux style).
#[derive(Debug, Clone)]
pub struct StubVariant {
    pub name: String,
    pub payload: Vec<JuxType>,
    pub discriminant: Option<i64>,
}

/// A `public const` stub (§G.5.6).
#[derive(Debug, Clone)]
pub struct StubConst {
    pub name: String,
    pub ty: JuxType,
    /// Literal value text, when the const is known at generation time.
    pub value: Option<String>,
}
