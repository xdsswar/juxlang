//! Cross-references: which declaration each name in a file denotes.
//!
//! References, rename and semantic tokens all need the same answer for an
//! identifier: is it a local (and which one), a member of some type (and which
//! type declares it), or a top-level declaration (and which one)? This module
//! gives that answer from what the analysis already produced (the merged
//! symbol table and the per-expression types) plus a lex + parse of the file
//! for what the analysis does not keep: where each local is declared and how
//! far it reaches.
//!
//! The model is per file ([`FileModel`]); a target found in one file is
//! matched in the others by resolving each same-named identifier there and
//! comparing ([`Target`] is the identity). Resolution never guesses: a name
//! whose receiver type is unknown is simply not claimed.
//!
//! Scoping rules, the ones the checker applies:
//!
//! - A parameter is visible in its function's body; a local from its name to
//!   the end of its block; a `for` header's local to the end of the loop; a
//!   loop or catch variable to the end of its body; a lambda parameter to the
//!   end of the lambda; a type-test binder (`x => Dog d`) to the end of the
//!   enclosing block. The innermost (latest-declared) visible binding wins.
//! - A bare name that is no binding is a member of the enclosing type when that
//!   type (or an ancestor) declares it (implicit `this`), else a top-level
//!   declaration, preferring an explicit import, then the file's own package.
//! - After a `.`: `this.m` and `super.m` are members of the enclosing type and
//!   its parent; `Type.m` is a static member; `expr.m` is a member of the
//!   expression's inferred type.

use std::collections::{HashMap, HashSet};

use juxc_ast::visit::{for_each_expr_in, for_each_node, Node};
use juxc_ast::{Block, Expr, InterpSegment, Stmt, TopLevelDecl};
use juxc_lex::{Keyword, TokenKind};
use juxc_source::Span;
use juxc_tycheck::{SymbolTable, Ty};

use crate::intel;
use crate::text::{receiver_dot_before, word_at};

/// One identifier occurrence in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Occurrence {
    /// The identifier text.
    pub(crate) name: String,
    /// Byte range in the file.
    pub(crate) start: usize,
    /// Exclusive end.
    pub(crate) end: usize,
}

/// What a binding is: drives the semantic-token type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingKind {
    /// A function, method, constructor or lambda parameter.
    Param,
    /// A local variable (including loop, catch and type-test binders).
    Local,
}

/// A name a body declares, with how far it reaches.
#[derive(Debug, Clone)]
pub(crate) struct Binding {
    /// The bound name.
    pub(crate) name: String,
    /// Byte range of the declaring identifier.
    pub(crate) decl: (usize, usize),
    /// Byte range where the name refers to this binding.
    pub(crate) visible: (usize, usize),
    /// Parameter or local.
    pub(crate) kind: BindingKind,
}

/// A declared type and the range its body covers.
#[derive(Debug, Clone)]
struct TypeScope {
    /// Bare name.
    name: String,
    /// Whole declaration.
    span: (usize, usize),
}

/// A lexed and parsed view of one file.
pub(crate) struct FileModel {
    /// The file's text.
    pub(crate) text: String,
    /// Every identifier, in source order: the lexer's, plus the names inside
    /// interpolated strings, which lex as one string token.
    pub(crate) idents: Vec<Occurrence>,
    /// Every binding the file's bodies declare.
    pub(crate) bindings: Vec<Binding>,
    /// Every type declaration, nested ones included.
    types: Vec<TypeScope>,
    /// Generic parameters with the declaration range they belong to.
    type_params: Vec<(String, (usize, usize))>,
    /// Byte ranges of the `package …;` and `import …;` statements.
    headers: Vec<(usize, usize)>,
    /// The declared package, if any.
    pub(crate) package: Option<String>,
    /// Single-type imports, as written (`a.b.C`), grouped ones expanded.
    imports: Vec<String>,
    /// Packages imported whole (`import a.b.*;`).
    wildcards: Vec<String>,
}

impl FileModel {
    /// Lex and parse `text` into a model. Never fails: a file mid-edit yields
    /// whatever part parses.
    pub(crate) fn build(text: &str) -> FileModel {
        let source =
            juxc_source::SourceFile::new(std::path::PathBuf::from("xref.jux"), text.to_string());
        let lexed = juxc_lex::lex(&source);
        let parsed = juxc_parse::parse(&lexed.tokens);

        let mut model = FileModel {
            text: text.to_string(),
            idents: Vec::new(),
            bindings: Vec::new(),
            types: Vec::new(),
            type_params: Vec::new(),
            headers: Vec::new(),
            package: None,
            imports: Vec::new(),
            wildcards: Vec::new(),
        };

        // Identifiers, header statements, and the brace pairs block scoping
        // is measured by.
        let mut braces: Vec<(usize, usize)> = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        let mut header: Option<usize> = None;
        for tok in &lexed.tokens {
            let (start, end) = (tok.span.start as usize, tok.span.end as usize);
            match &tok.kind {
                TokenKind::Ident(name) => {
                    model.idents.push(Occurrence { name: name.clone(), start, end });
                }
                TokenKind::Kw(Keyword::Package | Keyword::Import) => header = Some(start),
                TokenKind::Semicolon => {
                    if let Some(hstart) = header.take() {
                        model.headers.push((hstart, end));
                        let statement = text[hstart..end].to_string();
                        model.read_header(&statement);
                    }
                }
                TokenKind::LBrace => open.push(start),
                TokenKind::RBrace => {
                    if let Some(o) = open.pop() {
                        braces.push((o, end));
                    }
                }
                _ => {}
            }
        }

        let mut collector = Collector { model: &mut model, braces: &braces, for_inits: HashMap::new() };
        collector.items(&parsed.ast.items);
        model.idents.sort_by_key(|o| o.start);
        model.idents.dedup_by_key(|o| o.start);
        model
    }

    /// Record what one `package …;` / `import …;` statement declares:
    /// the package, a single-type import (`a.b.C`, `a.b.C as D`), a grouped
    /// one (`a.b.{C, D as E}`), or a wildcard (`a.b.*`).
    fn read_header(&mut self, statement: &str) {
        let body = statement.trim().trim_end_matches(';').trim();
        if let Some(pkg) = body.strip_prefix("package") {
            self.package = Some(pkg.trim().to_string());
            return;
        }
        let Some(path) = body.strip_prefix("import") else { return };
        let path: String = path.chars().filter(|c| !c.is_whitespace() || *c == ' ').collect();
        let path = path.trim();
        let name_of = |item: &str| item.split(" as ").next().unwrap_or(item).trim().to_string();
        if let Some((base, rest)) = path.split_once('{') {
            let base = base.trim().trim_end_matches('.');
            for item in rest.trim_end_matches('}').split(',') {
                let item = name_of(item);
                if !item.is_empty() {
                    self.imports.push(format!("{base}.{item}"));
                }
            }
        } else if let Some(base) = path.strip_suffix(".*") {
            self.wildcards.push(base.trim().to_string());
        } else {
            self.imports.push(name_of(path));
        }
    }

    /// True when `offset` sits in a `package` or `import` statement.
    pub(crate) fn in_header(&self, offset: usize) -> bool {
        self.headers.iter().any(|&(s, e)| s <= offset && offset < e)
    }

    /// The header statement containing `offset`, as text.
    fn header_text(&self, offset: usize) -> Option<&str> {
        let &(s, e) = self.headers.iter().find(|&&(s, e)| s <= offset && offset < e)?;
        self.text.get(s..e)
    }

    /// The innermost binding of `name` visible at `offset`.
    pub(crate) fn binding_at(&self, name: &str, offset: usize) -> Option<&Binding> {
        self.bindings
            .iter()
            .filter(|b| b.name == name && b.visible.0 <= offset && offset < b.visible.1)
            .max_by_key(|b| b.decl.0)
    }

    /// True when `name` is a generic parameter in scope at `offset`.
    pub(crate) fn is_type_param(&self, name: &str, offset: usize) -> bool {
        self.type_params
            .iter()
            .any(|(n, (s, e))| n == name && *s <= offset && offset < *e)
    }

    /// Bare name of the innermost type declaration containing `offset`.
    pub(crate) fn enclosing_type(&self, offset: usize) -> Option<&str> {
        self.types
            .iter()
            .filter(|t| t.span.0 <= offset && offset < t.span.1)
            .min_by_key(|t| t.span.1 - t.span.0)
            .map(|t| t.name.as_str())
    }

    /// True when the identifier at `occ` is a named-argument label
    /// (`greet(who: 1)`): it names a parameter of the call, not anything in
    /// scope.
    fn is_argument_label(&self, occ: &Occurrence) -> bool {
        let bytes = self.text.as_bytes();
        let mut after = occ.end;
        while after < bytes.len() && bytes[after].is_ascii_whitespace() {
            after += 1;
        }
        if bytes.get(after) != Some(&b':') || bytes.get(after + 1) == Some(&b':') {
            return false;
        }
        let mut before = occ.start;
        while before > 0 && bytes[before - 1].is_ascii_whitespace() {
            before -= 1;
        }
        matches!(bytes.get(before.wrapping_sub(1)), Some(b'(') | Some(b','))
    }
}

/// A binding found by the body walk, bound once the walk ends: the name, its
/// reach when the construct fixes it (`None` for "to the end of the block"),
/// and its kind.
type PendingBind = (juxc_ast::Ident, Option<(usize, usize)>, BindingKind);

/// Walks a file's declarations, recording scopes and bindings.
struct Collector<'a> {
    model: &'a mut FileModel,
    /// Every matched `{ … }` pair, as byte ranges (open brace to past the close).
    braces: &'a [(usize, usize)],
    /// A `for (int i = 0; …)` header local: name start → end of the loop.
    for_inits: HashMap<usize, usize>,
}

impl Collector<'_> {
    /// End of the innermost `{ … }` containing `offset`, or the file end.
    fn block_end(&self, offset: usize) -> usize {
        self.braces
            .iter()
            .filter(|&&(s, e)| s < offset && offset < e)
            .min_by_key(|&&(s, e)| e - s)
            .map(|&(_, e)| e)
            .unwrap_or(self.model.text.len())
    }

    fn range(span: Span) -> (usize, usize) {
        (span.start as usize, span.end as usize)
    }

    fn items(&mut self, items: &[TopLevelDecl]) {
        for item in items {
            match item {
                TopLevelDecl::Function(f) => self.function(f),
                TopLevelDecl::Class(c) => {
                    self.type_scope(&c.name.text, c.span);
                    self.type_params(&c.generic_params, c.span);
                    for ctor in &c.constructors {
                        self.params(&ctor.params, ctor.body.span);
                        self.body(&ctor.body);
                    }
                    for m in &c.methods {
                        self.function(m);
                    }
                    self.items(&c.nested_types);
                }
                TopLevelDecl::Enum(e) => {
                    self.type_scope(&e.name.text, e.span);
                    for m in &e.methods {
                        self.function(m);
                    }
                }
                TopLevelDecl::Record(r) => {
                    self.type_scope(&r.name.text, r.span);
                    self.type_params(&r.generic_params, r.span);
                    for m in &r.methods {
                        self.function(m);
                    }
                }
                TopLevelDecl::Interface(i) => {
                    self.type_scope(&i.name.text, i.span);
                    self.type_params(&i.generic_params, i.span);
                    for m in &i.methods {
                        self.function(m);
                    }
                }
                _ => {}
            }
        }
    }

    fn type_scope(&mut self, name: &str, span: Span) {
        self.model.types.push(TypeScope { name: name.to_string(), span: Self::range(span) });
    }

    fn type_params(&mut self, params: &[juxc_ast::TypeParam], scope: Span) {
        for p in params {
            self.model.type_params.push((p.name.text.clone(), Self::range(scope)));
        }
    }

    fn function(&mut self, f: &juxc_ast::FnDecl) {
        self.type_params(&f.generic_params, f.span);
        if let Some(body) = &f.body {
            self.params(&f.params, body.span);
            self.body(body);
        } else {
            // A bodyless declaration still names its parameters.
            self.params(&f.params, f.span);
        }
    }

    fn params(&mut self, params: &[juxc_ast::Param], scope: Span) {
        for p in params {
            self.bind(&p.name, (p.name.span.start as usize, scope.end as usize), BindingKind::Param);
        }
    }

    fn bind(&mut self, name: &juxc_ast::Ident, visible: (usize, usize), kind: BindingKind) {
        self.model.bindings.push(Binding {
            name: name.text.clone(),
            decl: Self::range(name.span),
            visible,
            kind,
        });
    }

    fn body(&mut self, block: &Block) {
        // Collect first, then bind: the walk borrows the block, the binds need
        // `self` mutably.
        let mut binds: Vec<PendingBind> = Vec::new();
        let mut interp: Vec<Occurrence> = Vec::new();
        for_each_node(block, &mut |node| match node {
            Node::Stmt(Stmt::VarDecl(v)) => binds.push((v.name.clone(), None, BindingKind::Local)),
            Node::Stmt(Stmt::ForC(f)) => {
                if let Some(Stmt::VarDecl(v)) = f.init.as_deref() {
                    self.for_inits.insert(v.name.span.start as usize, f.span.end as usize);
                }
            }
            Node::Stmt(Stmt::ForEach(f)) => binds.push((
                f.var_name.clone(),
                Some((f.var_name.span.start as usize, f.span.end as usize)),
                BindingKind::Local,
            )),
            Node::Stmt(Stmt::Try(t)) => {
                for c in &t.catches {
                    binds.push((
                        c.name.clone(),
                        Some((c.name.span.start as usize, c.body.span.end as usize)),
                        BindingKind::Local,
                    ));
                }
            }
            Node::Expr(Expr::Lambda(l)) => {
                for p in &l.params {
                    binds.push((
                        p.name.clone(),
                        Some((p.name.span.start as usize, l.span.end as usize)),
                        BindingKind::Param,
                    ));
                }
            }
            Node::Expr(Expr::TypeTest(t)) => {
                if let Some(b) = &t.binder {
                    binds.push((b.clone(), None, BindingKind::Local));
                }
            }
            Node::Expr(Expr::InterpString(s)) => {
                for seg in &s.segments {
                    match seg {
                        InterpSegment::Bare(id) => interp.push(occurrence(id)),
                        InterpSegment::Expr(e) => for_each_expr_in(e, &mut |inner| match inner {
                            Expr::Path(qn) => interp.extend(qn.segments.iter().map(occurrence)),
                            Expr::Field(f) => interp.push(occurrence(&f.field)),
                            _ => {}
                        }),
                        _ => {}
                    }
                }
            }
            _ => {}
        });
        for (name, visible, kind) in binds {
            let start = name.span.start as usize;
            let visible = visible.unwrap_or_else(|| match self.for_inits.get(&start) {
                Some(&end) => (start, end),
                None => (start, self.block_end(start)),
            });
            self.bind(&name, visible, kind);
        }
        self.model.idents.extend(interp);
    }
}

fn occurrence(id: &juxc_ast::Ident) -> Occurrence {
    Occurrence { name: id.text.clone(), start: id.span.start as usize, end: id.span.end as usize }
}

/// What a top-level declaration is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclKind {
    /// A class (including a `struct`).
    Class,
    /// An interface.
    Interface,
    /// An enum.
    Enum,
    /// A record.
    Record,
    /// A type alias.
    Alias,
    /// A free function.
    Function,
    /// A top-level constant.
    Const,
}

impl DeclKind {
    /// True for the kinds that declare a type.
    pub(crate) fn is_type(self) -> bool {
        matches!(self, DeclKind::Class | DeclKind::Interface | DeclKind::Enum | DeclKind::Record)
    }
}

/// The identity of what a name refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// A binding of the file it was found in.
    Local {
        /// The bound name.
        name: String,
        /// Its declaring identifier.
        decl: (usize, usize),
    },
    /// A top-level declaration.
    Decl {
        /// The symbol-table key.
        fqn: String,
        /// Declaring unit.
        unit: usize,
        /// Start of the declaration.
        start: u32,
        /// What it declares.
        kind: DeclKind,
    },
    /// A member (method, field, property, variant) of a type.
    Member {
        /// Symbol-table key of the declaring type.
        owner: String,
        /// Member name.
        name: String,
    },
}

impl Target {
    /// The name the target is written with.
    pub(crate) fn name(&self) -> &str {
        match self {
            Target::Local { name, .. } | Target::Member { name, .. } => name,
            Target::Decl { fqn, .. } => fqn.rsplit('.').next().unwrap_or(fqn),
        }
    }
}

/// The analysis facts resolution needs for one file.
pub(crate) struct FileFacts<'a> {
    /// The merged symbol table.
    pub(crate) symbols: &'a SymbolTable,
    /// The file's expression types by END offset (largest expression wins, so
    /// a chained receiver resolves to the whole chain).
    pub(crate) types_by_end: HashMap<usize, &'a Ty>,
}

impl<'a> FileFacts<'a> {
    /// Facts for analysed file `file` from all of `expr_types`.
    pub(crate) fn new(symbols: &'a SymbolTable, expr_types: &'a [(Span, Ty)], file: Option<u32>) -> Self {
        let mut best: HashMap<usize, (u32, &'a Ty)> = HashMap::new();
        for (span, ty) in expr_types {
            if file.is_some_and(|f| span.file != f) {
                continue;
            }
            let entry = best.entry(span.end as usize).or_insert((span.len(), ty));
            if span.len() > entry.0 {
                *entry = (span.len(), ty);
            }
        }
        FileFacts { symbols, types_by_end: best.into_iter().map(|(k, (_, t))| (k, t)).collect() }
    }
}

/// Resolve the identifier `occ` of `model` to its target.
pub(crate) fn resolve(facts: &FileFacts<'_>, model: &FileModel, occ: &Occurrence) -> Option<Target> {
    let symbols = facts.symbols;
    let text = &model.text;
    let name = occ.name.as_str();

    // `package`/`import` statements: only an import's final segment names a
    // declaration; the rest are package names.
    if model.in_header(occ.start) {
        let header = model.header_text(occ.start)?;
        if !header.trim_start().starts_with("import") {
            return None;
        }
        // The identifier is an imported name only when it is the last segment
        // of one of this file's imports (package segments name no symbol).
        let fqn = model.imports.iter().find(|i| i.rsplit('.').next() == Some(name))?;
        let after = &text[occ.end..];
        let last_segment = !after.trim_start().starts_with('.');
        return if last_segment { decl_target(symbols, fqn) } else { None };
    }

    if model.is_argument_label(occ) {
        return None;
    }

    if let Some(dot) = receiver_dot_before(text, occ.start) {
        let owner_ty = receiver_type(facts, model, dot)?;
        let owner = intel::member_owner(symbols, &owner_ty, name)
            .or_else(|| variant_owner(symbols, &owner_ty, name))?;
        return Some(Target::Member { owner, name: name.to_string() });
    }

    if let Some(b) = model.binding_at(name, occ.start) {
        return Some(Target::Local { name: name.to_string(), decl: b.decl });
    }
    if model.is_type_param(name, occ.start) {
        return None;
    }
    // Implicit `this`: a member of the enclosing type (or an ancestor). Also
    // what a member's own declaration name resolves through.
    if let Some(enclosing) = model.enclosing_type(occ.start) {
        if let Some(key) = type_key(symbols, enclosing, model.package.as_deref()) {
            let ty = Ty::User { name: key, generic_args: vec![] };
            let owner = intel::member_owner(symbols, &ty, name).or_else(|| variant_owner(symbols, &ty, name));
            if let Some(owner) = owner {
                return Some(Target::Member { owner, name: name.to_string() });
            }
        }
    }
    let fqn = pick_decl(symbols, model, name)?;
    decl_target(symbols, &fqn)
}

/// The enum key when `ty` is an enum with a variant named `name`. Variants
/// are members of their enum for references and rename, but the member lookup
/// used for methods and fields does not list them.
fn variant_owner(symbols: &SymbolTable, ty: &Ty, name: &str) -> Option<String> {
    let Ty::User { name: key, .. } = ty else { return None };
    let (key, en) = symbols.enums.get_key_value(key.as_str()).or_else(|| {
        let bare = key.rsplit('.').next().unwrap_or(key);
        symbols.enums.iter().find(|(k, _)| k.rsplit('.').next() == Some(bare))
    })?;
    en.variants.contains_key(name).then(|| key.clone())
}

/// The type of the receiver whose expression ends at `dot`.
fn receiver_type(facts: &FileFacts<'_>, model: &FileModel, dot: usize) -> Option<Ty> {
    let symbols = facts.symbols;
    if let Some(word) = word_at(&model.text, dot) {
        let chained = receiver_dot_before(&model.text, word.start).is_some();
        if !chained {
            match word.text.as_str() {
                "this" | "super" => {
                    let enclosing = model.enclosing_type(dot)?;
                    let key = type_key(symbols, enclosing, model.package.as_deref())?;
                    if word.text == "this" {
                        return Some(Ty::User { name: key, generic_args: vec![] });
                    }
                    let parent = symbols.classes.get(&key)?.extends_fqn.clone()?;
                    return Some(Ty::User { name: parent, generic_args: vec![] });
                }
                _ => {
                    // `Type.member`: a static receiver, unless a binding of
                    // that name shadows the type.
                    if model.binding_at(&word.text, word.start).is_none() {
                        if let Some(key) = type_key(symbols, &word.text, model.package.as_deref()) {
                            // The type itself, not whatever the checker
                            // recorded for the bare type-name expression.
                            return Some(Ty::User { name: key, generic_args: vec![] });
                        }
                    }
                }
            }
        }
    }
    facts.types_by_end.get(&dot).map(|t| (*t).clone())
}

/// The symbol-table key of the type `bare` names, seen from package `pkg`:
/// an exact key, the same-package declaration, or a nested type's mangled
/// key.
pub(crate) fn type_key(symbols: &SymbolTable, bare: &str, pkg: Option<&str>) -> Option<String> {
    let is_type = |k: &str| {
        symbols.classes.contains_key(k)
            || symbols.enums.contains_key(k)
            || symbols.records.contains_key(k)
            || symbols.interfaces.contains_key(k)
    };
    if is_type(bare) {
        return Some(bare.to_string());
    }
    if let Some(k) = symbols.find_fqn_by_bare_in(bare, pkg.unwrap_or("")) {
        return Some(k);
    }
    // A nested type is keyed `Outer__Inner`.
    let nested = format!("__{bare}");
    symbols
        .classes
        .keys()
        .chain(symbols.enums.keys())
        .chain(symbols.records.keys())
        .chain(symbols.interfaces.keys())
        .find(|k| k.ends_with(&nested))
        .cloned()
}

/// The FQN a bare top-level name refers to from `model`'s file: an explicit
/// import, then the file's own package, then a unique declaration anywhere.
fn pick_decl(symbols: &SymbolTable, model: &FileModel, name: &str) -> Option<String> {
    let candidates = decl_keys(symbols, name);
    if candidates.is_empty() {
        return None;
    }
    if let Some(imported) = model.imports.iter().find(|i| i.rsplit('.').next() == Some(name)) {
        if candidates.contains(imported) {
            return Some(imported.clone());
        }
    }
    let own = match model.package.as_deref() {
        Some(p) if !p.is_empty() => format!("{p}.{name}"),
        _ => name.to_string(),
    };
    if candidates.contains(&own) {
        return Some(own);
    }
    if let Some(k) = model
        .wildcards
        .iter()
        .map(|w| format!("{w}.{name}"))
        .find(|k| candidates.contains(k))
    {
        return Some(k);
    }
    if candidates.len() == 1 {
        return candidates.into_iter().next();
    }
    // Several, none imported or local: the checker's own preference.
    symbols.definition_of(name).and_then(|(unit, span)| {
        candidates.into_iter().find(|k| {
            symbols.decl_unit.get(k) == Some(&unit) && decl_span(symbols, k).map(|s| s.start) == Some(span.start)
        })
    })
}

/// Every top-level key whose last segment is `name`.
fn decl_keys(symbols: &SymbolTable, name: &str) -> Vec<String> {
    let hit = |k: &String| k.rsplit('.').next() == Some(name) || k == name;
    let mut out: Vec<String> = symbols
        .classes
        .keys()
        .chain(symbols.interfaces.keys())
        .chain(symbols.enums.keys())
        .chain(symbols.records.keys())
        .chain(symbols.aliases.keys())
        .chain(symbols.functions.keys())
        .chain(symbols.consts.keys())
        .filter(|k| hit(k))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The declaration target for FQN `fqn`.
fn decl_target(symbols: &SymbolTable, fqn: &str) -> Option<Target> {
    let kind = if let Some(c) = symbols.classes.get(fqn) {
        let _ = c;
        DeclKind::Class
    } else if symbols.interfaces.contains_key(fqn) {
        DeclKind::Interface
    } else if symbols.enums.contains_key(fqn) {
        DeclKind::Enum
    } else if symbols.records.contains_key(fqn) {
        DeclKind::Record
    } else if symbols.aliases.contains_key(fqn) {
        DeclKind::Alias
    } else if symbols.functions.contains_key(fqn) {
        DeclKind::Function
    } else if symbols.consts.contains_key(fqn) {
        DeclKind::Const
    } else {
        return None;
    };
    let unit = *symbols.decl_unit.get(fqn)?;
    let start = decl_span(symbols, fqn)?.start;
    Some(Target::Decl { fqn: fqn.to_string(), unit, start, kind })
}

/// The declaration span recorded for top-level key `fqn`.
pub(crate) fn decl_span(symbols: &SymbolTable, fqn: &str) -> Option<Span> {
    symbols
        .classes
        .get(fqn)
        .map(|s| s.span)
        .or_else(|| symbols.records.get(fqn).map(|s| s.span))
        .or_else(|| symbols.enums.get(fqn).map(|s| s.span))
        .or_else(|| symbols.interfaces.get(fqn).map(|s| s.span))
        .or_else(|| symbols.functions.get(fqn).map(|s| s.span))
        .or_else(|| symbols.aliases.get(fqn).map(|s| s.span))
        .or_else(|| symbols.consts.get(fqn).map(|s| s.span))
}

/// Where the target is declared: its declaring unit and the byte offset its
/// declaration starts at (the name is the first occurrence of the target's
/// name from there). `None` for a local (it lives in its own file).
pub(crate) fn declaration_site(symbols: &SymbolTable, target: &Target) -> Option<(usize, u32)> {
    match target {
        Target::Local { .. } => None,
        Target::Decl { unit, start, .. } => Some((*unit, *start)),
        Target::Member { owner, name } => {
            let span = intel::member_decl_span(symbols, owner, name).or_else(|| {
                // Record components and properties have no member span of
                // their own; the owner's declaration is where they are written.
                decl_span(symbols, owner)
            })?;
            Some((*symbols.decl_unit.get(owner)?, span.start))
        }
    }
}

/// Every occurrence in `model` that refers to `target`. `file_facts` are the
/// analysis facts for this file.
pub(crate) fn occurrences_in(
    facts: &FileFacts<'_>,
    model: &FileModel,
    target: &Target,
) -> Vec<(usize, usize)> {
    let name = target.name();
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    for occ in model.idents.iter().filter(|o| o.name == name) {
        let hit = match target {
            // A local: same file, same declaring identifier.
            Target::Local { decl, .. } => {
                receiver_dot_before(&model.text, occ.start).is_none()
                    && !model.is_argument_label(occ)
                    && model.binding_at(name, occ.start).map(|b| b.decl) == Some(*decl)
            }
            _ => resolve(facts, model, occ).as_ref() == Some(target),
        };
        if hit && seen.insert(occ.start) {
            out.push((occ.start, occ.end));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(src: &str) -> FileModel {
        FileModel::build(src)
    }

    fn occ_at(m: &FileModel, needle: &str, nth: usize) -> Occurrence {
        let start = m.text.match_indices(needle).nth(nth).expect("needle present").0;
        m.idents.iter().find(|o| o.start == start).cloned().expect("an identifier starts there")
    }

    /// Parameters reach their whole body; a local reaches from its name to the
    /// end of its block; an inner local shadows an outer one of the same name.
    #[test]
    fn locals_follow_block_scoping_and_shadowing() {
        let src = "public void run(int n) {\n    int x = n;\n    if (n > 0) {\n        int x = 2;\n        print(x);\n    }\n    print(x);\n}\n";
        let m = model(src);
        let outer = m.binding_at("x", src.rfind("print(x)").unwrap()).unwrap();
        let inner = m.binding_at("x", src.find("print(x)").unwrap()).unwrap();
        assert_ne!(outer.decl, inner.decl, "the inner `x` shadows inside its block only");
        assert_eq!(outer.decl.0, src.find("x = n").unwrap());
        let n = m.binding_at("n", src.find("n > 0").unwrap()).unwrap();
        assert_eq!(n.kind, BindingKind::Param);
        assert!(m.binding_at("x", src.find("int x").unwrap() - 1).is_none(), "not before its declaration");
    }

    /// Loop, catch and lambda binders reach exactly their construct.
    #[test]
    fn loop_catch_and_lambda_binders_are_scoped_to_their_construct() {
        let src = "public void run() {\n    for (var item : items) { print(item); }\n    try { go(); } catch (Exception e) { print(e); }\n    var f = (int k) -> k + 1;\n    print(item);\n}\n";
        let m = model(src);
        assert!(m.binding_at("item", src.find("print(item)").unwrap()).is_some());
        assert!(m.binding_at("item", src.rfind("print(item)").unwrap()).is_none(), "out of the loop");
        assert!(m.binding_at("e", src.find("print(e)").unwrap()).is_some());
        assert!(m.binding_at("k", src.find("k + 1").unwrap()).is_some());
    }

    /// Names inside `${…}` holes are identifiers too; they are one string
    /// token to the lexer.
    #[test]
    fn interpolation_holes_contribute_identifiers() {
        let src = "public void run(int total) {\n    print($\"sum=${total} and $total\");\n}\n";
        let m = model(src);
        let hits = m.idents.iter().filter(|o| o.name == "total").count();
        assert_eq!(hits, 3, "the parameter, the `${{total}}` hole and the bare `$total`");
    }

    /// `package` and `import` statements are recorded; a named-argument label
    /// is recognised and resolves to nothing.
    #[test]
    fn headers_and_argument_labels() {
        let src = "package app.ui;\nimport lib.Widget;\nimport kit.{Gear, Bolt as B};\nimport util.*;\npublic void run(int who) { greet(who: who); }\n";
        let m = model(src);
        assert_eq!(m.package.as_deref(), Some("app.ui"));
        assert_eq!(m.imports, vec!["lib.Widget".to_string(), "kit.Gear".to_string(), "kit.Bolt".to_string()]);
        assert_eq!(m.wildcards, vec!["util".to_string()]);
        assert!(m.in_header(src.find("Widget").unwrap()));
        let label = occ_at(&m, "who:", 0);
        assert!(m.is_argument_label(&label));
        let value = occ_at(&m, "who)", 0);
        assert!(!m.is_argument_label(&value));
    }

    /// A `for (int i …)` header local reaches the end of its loop, not the end
    /// of the enclosing block.
    #[test]
    fn for_header_local_ends_with_the_loop() {
        let src = "public void run() {\n    for (int i = 0; i < 3; i = i + 1) { print(i); }\n    print(i);\n}\n";
        let m = model(src);
        assert!(m.binding_at("i", src.find("print(i)").unwrap()).is_some());
        assert!(m.binding_at("i", src.rfind("print(i)").unwrap()).is_none());
    }

    /// Type-test binders are locals of the enclosing block, and generic
    /// parameters are in scope over their declaration.
    #[test]
    fn type_test_binders_and_type_params() {
        let src = "public class Box<T> {\n    public void m(Object o) {\n        if (o => Box d) { print(d); }\n    }\n}\n";
        let m = model(src);
        assert!(m.binding_at("d", src.find("print(d)").unwrap()).is_some());
        assert!(m.is_type_param("T", src.find("public void").unwrap()));
        assert_eq!(m.enclosing_type(src.find("print").unwrap()), Some("Box"));
    }
}
