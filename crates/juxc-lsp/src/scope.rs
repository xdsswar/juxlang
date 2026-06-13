//! Scope-aware local discovery for completion.
//!
//! Completion's biggest single win is offering the names the user can
//! actually write at the caret: the locals declared **above** it, the
//! enclosing function's parameters, and (inside a class) the implicit-`this`
//! members. The analysis pass doesn't keep the checker's transient scope
//! stack, but the AST has everything — so this module re-parses the open
//! buffer (lex + parse only, the same cost `documentSymbol` already pays)
//! and walks the declarations that *contain* the caret, collecting every
//! binding whose declaration ends before it.
//!
//! Mid-edit resilience: the buffer usually fails to parse exactly at the
//! caret (a partial identifier, a dangling `obj.`). When the first parse
//! doesn't place the caret inside a function body, we patch the
//! `patch_start..caret` slice out (replacing it with `;`, the same trick as
//! `receiver_type_by_reparse`) and parse again — the surrounding scopes are
//! intact, so the locals still resolve while the user types.

use juxc_ast::{
    Block, ClassDecl, ConstructorDecl, ElseBranch, Expr, FnDecl, FnModifier, Param, Stmt,
    TopLevelDecl, VarDecl,
};
use juxc_source::Span;
use juxc_tycheck::Ty;

use crate::intel::render_type;

/// What kind of binding a [`LocalVar`] is — drives the completion item's
/// detail text when the declared type couldn't be rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalKind {
    /// A formal parameter of the enclosing function / method / constructor.
    Param,
    /// A `var` / typed local declared in a block above the caret.
    Local,
    /// A `for (var x : iter)` loop variable (visible only in the loop body).
    ForEachVar,
    /// A `catch (E e)` binding (visible only in the catch block).
    CatchVar,
}

/// One name visible at the caret.
#[derive(Debug)]
pub struct LocalVar {
    /// The binding's name — what completion inserts.
    pub name: String,
    /// Rendered type for the item's detail column: the declared type when
    /// one was written, else the initializer's inferred type from the last
    /// analysis, else `None` (shown without a type).
    pub ty_display: Option<String>,
    /// What kind of binding this is.
    pub kind: LocalKind,
}

/// Everything the scope walk learned about the caret's surroundings.
#[derive(Default)]
pub struct ScopeInfo {
    /// Bindings visible at the caret, innermost shadowing outermost
    /// (deduplicated by name — the inner binding wins).
    pub locals: Vec<LocalVar>,
    /// Bare name of the innermost `class`/`enum`/`record`/`interface`
    /// declaration containing the caret, or `None` at the top level / inside
    /// a free function.
    pub enclosing_class: Option<String>,
    /// True when the enclosing function is `static` — implicit-`this`
    /// completion must then offer static members only.
    pub enclosing_fn_is_static: bool,
    /// True when the caret sits inside a function / method / constructor
    /// body (the only place locals and implicit-`this` members make sense).
    pub in_fn_body: bool,
}

/// Collect the scope picture at `caret`.
///
/// `patch_start..caret` is the slice the mid-edit fallback removes before
/// the second parse — the partial word being completed (or the dangling
/// `.`-through-caret slice of a member access). `expr_types` is the open
/// document's cached per-expression type map, used to type `var x = init`
/// locals by their initializer.
pub fn scope_at(
    text: &str,
    patch_start: usize,
    caret: usize,
    expr_types: &[(Span, Ty)],
) -> ScopeInfo {
    let probe = patch_start.min(text.len());

    // First try the buffer as-is: when the code around the caret is
    // well-formed (completion invoked on a blank line, say) one parse does it.
    if let Some((info, parses_clean)) = walk_parsed(text, probe, expr_types) {
        // The caret landed in a function body — believe it. Otherwise that's
        // the truth only when the parse was clean (top level / type body);
        // with parse errors the enclosing function may simply have failed to
        // parse — fall through to the patched retry before believing it.
        if info.in_fn_body || parses_clean {
            return info;
        }
    }

    // Mid-edit fallback: drop the partial slice, close the statement with a
    // `;`, and parse again. Declarations before `patch_start` keep their
    // spans, so the walk below sees them unchanged.
    if patch_start <= caret && caret <= text.len()
        && text.is_char_boundary(patch_start)
        && text.is_char_boundary(caret)
    {
        let mut patched = String::with_capacity(text.len());
        patched.push_str(&text[..patch_start]);
        patched.push(';');
        patched.push_str(&text[caret..]);
        if let Some((info, _)) = walk_parsed(&patched, probe, expr_types) {
            return info;
        }
    }

    ScopeInfo::default()
}

/// Lex + parse `text` and walk the result. Returns the scope picture plus
/// whether the parse was clean; `None` only when the parse produced no items
/// at all (nothing to walk).
fn walk_parsed(
    text: &str,
    probe: usize,
    expr_types: &[(Span, Ty)],
) -> Option<(ScopeInfo, bool)> {
    let source =
        juxc_source::SourceFile::new(std::path::PathBuf::from("scope.jux"), text.to_string());
    let lexed = juxc_lex::lex(&source);
    let parsed = juxc_parse::parse(&lexed.tokens);
    if parsed.ast.items.is_empty() {
        return None;
    }
    let mut walker = Walker { probe, expr_types, info: ScopeInfo::default() };
    walker.walk_items(&parsed.ast.items);
    walker.info.dedup_by_shadowing();
    Some((walker.info, parsed.diagnostics.is_empty()))
}

impl ScopeInfo {
    /// Keep the LAST occurrence of each name — the walk pushes outer scopes
    /// before inner ones, so the innermost (shadowing) binding survives.
    fn dedup_by_shadowing(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut kept: Vec<LocalVar> = Vec::new();
        for var in self.locals.drain(..).rev() {
            if seen.insert(var.name.clone()) {
                kept.push(var);
            }
        }
        kept.reverse();
        self.locals = kept;
    }
}

/// The AST walk: descend only into declarations whose span contains the
/// probe, collecting bindings declared before it.
struct Walker<'a> {
    /// Byte offset the completion targets (start of the partial word).
    probe: usize,
    /// Cached per-expression types from the last analysis (for `var` locals).
    expr_types: &'a [(Span, Ty)],
    /// Accumulated result.
    info: ScopeInfo,
}

impl<'a> Walker<'a> {
    /// True when `span` contains the probe (end-inclusive: a caret parked at
    /// the very `}` of a block is still "inside" for completion purposes).
    fn contains(&self, span: Span) -> bool {
        (span.start as usize) <= self.probe && self.probe <= (span.end as usize)
    }

    fn walk_items(&mut self, items: &[TopLevelDecl]) {
        for item in items {
            match item {
                TopLevelDecl::Function(f) => self.visit_fn(f),
                TopLevelDecl::Class(c) => self.visit_class(c),
                TopLevelDecl::Enum(e) => {
                    if self.contains(e.span) {
                        self.info.enclosing_class = Some(e.name.text.clone());
                        for m in &e.methods {
                            self.visit_fn(m);
                        }
                    }
                }
                TopLevelDecl::Record(r) => {
                    if self.contains(r.span) {
                        self.info.enclosing_class = Some(r.name.text.clone());
                        for m in &r.methods {
                            self.visit_fn(m);
                        }
                    }
                }
                TopLevelDecl::Interface(i) => {
                    if self.contains(i.span) {
                        self.info.enclosing_class = Some(i.name.text.clone());
                        // Interface methods are normally bodyless; default
                        // methods (with bodies) still walk like any other.
                        for m in &i.methods {
                            self.visit_fn(m);
                        }
                    }
                }
                // Foreign-function blocks are bodyless — no scope to descend into.
                TopLevelDecl::TypeAlias(_)
                | TopLevelDecl::Const(_)
                | TopLevelDecl::ExternBlock(_) => {}
            }
        }
    }

    fn visit_class(&mut self, c: &ClassDecl) {
        if !self.contains(c.span) {
            return;
        }
        self.info.enclosing_class = Some(c.name.text.clone());
        for ctor in &c.constructors {
            self.visit_ctor(ctor);
        }
        for m in &c.methods {
            self.visit_fn(m);
        }
        // Nested types: recurse — an inner declaration containing the probe
        // overwrites `enclosing_class` with the innermost name.
        self.walk_items(&c.nested_types);
    }

    fn visit_ctor(&mut self, ctor: &ConstructorDecl) {
        if !self.contains(ctor.span) || !self.contains(ctor.body.span) {
            return;
        }
        self.info.in_fn_body = true;
        self.info.enclosing_fn_is_static = false;
        self.push_params(&ctor.params);
        self.walk_block(&ctor.body);
    }

    fn visit_fn(&mut self, f: &FnDecl) {
        let Some(body) = &f.body else { return };
        if !self.contains(f.span) || !self.contains(body.span) {
            return;
        }
        self.info.in_fn_body = true;
        self.info.enclosing_fn_is_static = f.modifiers.contains(&FnModifier::Static);
        self.push_params(&f.params);
        self.walk_block(body);
    }

    fn push_params(&mut self, params: &[Param]) {
        for p in params {
            self.info.locals.push(LocalVar {
                name: p.name.text.clone(),
                ty_display: Some(render_type(&p.ty)),
                kind: LocalKind::Param,
            });
        }
    }

    fn walk_block(&mut self, block: &Block) {
        if !self.contains(block.span) {
            return;
        }
        for stmt in &block.statements {
            self.walk_stmt(stmt);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl(v) => self.push_var(v),
            Stmt::If(s) => {
                self.walk_block(&s.then_block);
                let mut else_branch = s.else_branch.as_deref();
                while let Some(eb) = else_branch {
                    match eb {
                        ElseBranch::If(nested) => {
                            self.walk_block(&nested.then_block);
                            else_branch = nested.else_branch.as_deref();
                        }
                        ElseBranch::Block(b) => {
                            self.walk_block(b);
                            else_branch = None;
                        }
                    }
                }
            }
            Stmt::While(w) => self.walk_block(&w.body),
            Stmt::DoWhile(d) => self.walk_block(&d.body),
            Stmt::ForEach(f) => {
                // The loop variable is visible only inside the body.
                if self.contains(f.body.span) {
                    self.info.locals.push(LocalVar {
                        name: f.var_name.text.clone(),
                        ty_display: f.var_type.as_ref().map(render_type),
                        kind: LocalKind::ForEachVar,
                    });
                }
                self.walk_block(&f.body);
            }
            Stmt::ForC(f) => {
                if self.contains(f.span) {
                    // The header's `int i = 0` is visible in the condition,
                    // update, and body — i.e. anywhere inside the loop's span
                    // past its declaration. The VarDecl arm's `end <= probe`
                    // check handles exactly that.
                    if let Some(init) = &f.init {
                        self.walk_stmt(init);
                    }
                    self.walk_block(&f.body);
                }
            }
            Stmt::Labeled { stmt, .. } => self.walk_stmt(stmt),
            Stmt::Try(t) => {
                self.walk_block(&t.body);
                for c in &t.catches {
                    if self.contains(c.body.span) {
                        self.info.locals.push(LocalVar {
                            name: c.name.text.clone(),
                            ty_display: Some(render_type(&c.ty)),
                            kind: LocalKind::CatchVar,
                        });
                    }
                    self.walk_block(&c.body);
                }
                if let Some(fin) = &t.finally {
                    self.walk_block(fin);
                }
            }
            Stmt::Unsafe(b) => self.walk_block(b),
            // No bindings and no blocks to descend into (lambda bodies and
            // switch-expression arms are a later refinement).
            Stmt::Expr(_)
            | Stmt::Return(_)
            | Stmt::Assign(_)
            | Stmt::Break(..)
            | Stmt::Continue(..)
            | Stmt::SuperCall(..)
            | Stmt::Throw(..) => {}
        }
    }

    /// Record a local whose declaration ends before the probe (a local is
    /// not in scope on its own declaration line's initializer).
    fn push_var(&mut self, v: &VarDecl) {
        if (v.span.end as usize) > self.probe {
            return;
        }
        let ty_display = v
            .ty
            .as_ref()
            .map(render_type)
            .or_else(|| v.init.as_ref().and_then(|e| self.ty_of(e)));
        self.info.locals.push(LocalVar {
            name: v.name.text.clone(),
            ty_display,
            kind: LocalKind::Local,
        });
    }

    /// The inferred type of `expr` from the cached analysis, rendered for
    /// display. Exact-span match: the cache and this walk parse the same
    /// buffer, so a well-formed initializer's span is identical in both.
    fn ty_of(&self, expr: &Expr) -> Option<String> {
        let span = expr.span();
        if span == Span::DUMMY {
            return None;
        }
        self.expr_types
            .iter()
            .find(|(s, _)| s.start == span.start && s.end == span.end)
            .map(|(_, t)| t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Params + locals declared above the caret are collected; the enclosing
    /// class and method staticness are reported.
    #[test]
    fn scope_at_collects_params_and_locals_before_cursor() {
        let src = "public class Greeter {\n\
                       public String greet(String who) {\n\
                           var greeting = \"hi\";\n\
                           gr\n\
                       }\n\
                   }\n";
        let caret = src.rfind("gr\n").unwrap() + 2;
        let word_start = caret - 2;
        let info = scope_at(src, word_start, caret, &[]);
        assert!(info.in_fn_body, "caret is inside greet()'s body");
        assert_eq!(info.enclosing_class.as_deref(), Some("Greeter"));
        assert!(!info.enclosing_fn_is_static);
        let names: Vec<&str> = info.locals.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"who"), "param `who` in scope, got {names:?}");
        assert!(names.contains(&"greeting"), "local `greeting` in scope, got {names:?}");
        // The param carries its declared type.
        let who = info.locals.iter().find(|l| l.name == "who").unwrap();
        assert_eq!(who.ty_display.as_deref(), Some("String"));
        assert_eq!(who.kind, LocalKind::Param);
    }

    /// A local declared in a sibling block (already closed) is NOT in scope;
    /// one declared in a containing block is.
    #[test]
    fn scope_at_excludes_out_of_scope_block_locals() {
        let src = "public void run() {\n\
                       var outer = 1;\n\
                       if (true) { var inner = 2; }\n\
                       ou\n\
                   }\n";
        let caret = src.rfind("ou\n").unwrap() + 2;
        let info = scope_at(src, caret - 2, caret, &[]);
        let names: Vec<&str> = info.locals.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"outer"), "containing-block local in scope: {names:?}");
        assert!(!names.contains(&"inner"), "sibling-block local leaked: {names:?}");
    }

    /// A local declared BELOW the caret is not offered (no use-before-decl).
    #[test]
    fn scope_at_excludes_locals_declared_after_cursor() {
        let src = "public void run() {\n\
                       var before = 1;\n\
                       be\n\
                       var after = 2;\n\
                   }\n";
        let caret = src.rfind("be\n").unwrap() + 2;
        let info = scope_at(src, caret - 2, caret, &[]);
        let names: Vec<&str> = info.locals.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"before"), "{names:?}");
        assert!(!names.contains(&"after"), "later local leaked: {names:?}");
    }

    /// Static methods report `enclosing_fn_is_static` so implicit-`this`
    /// completion can stick to statics.
    #[test]
    fn scope_at_reports_enclosing_class_and_staticness() {
        let src = "public class Util {\n\
                       public static int twice(int n) {\n\
                           n\n\
                       }\n\
                   }\n";
        let caret = src.rfind("n\n").unwrap() + 1;
        let info = scope_at(src, caret - 1, caret, &[]);
        assert!(info.in_fn_body);
        assert_eq!(info.enclosing_class.as_deref(), Some("Util"));
        assert!(info.enclosing_fn_is_static);
    }

    /// For-each loop variables and catch bindings are visible in their
    /// blocks (and only there).
    #[test]
    fn scope_at_collects_foreach_and_catch_vars() {
        let src = "public void run() {\n\
                       for (var item : 0..10) {\n\
                           it\n\
                       }\n\
                   }\n";
        let caret = src.rfind("it\n").unwrap() + 2;
        let info = scope_at(src, caret - 2, caret, &[]);
        let names: Vec<&str> = info.locals.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"item"), "loop variable in scope: {names:?}");

        let src2 = "public void run() {\n\
                        try { print(1); } catch (Exception e) {\n\
                            e\n\
                        }\n\
                    }\n";
        let caret2 = src2.rfind("e\n").unwrap() + 1;
        let info2 = scope_at(src2, caret2 - 1, caret2, &[]);
        let names2: Vec<&str> = info2.locals.iter().map(|l| l.name.as_str()).collect();
        assert!(names2.contains(&"e"), "catch binding in scope: {names2:?}");
    }
}
