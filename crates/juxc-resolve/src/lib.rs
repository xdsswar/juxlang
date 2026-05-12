//! Phase 4 — name resolution and module linking.
//!
//! Per `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.2.4: every name reference is
//! bound to a definition. The resolver populates a symbol table, validates
//! imports, and enforces visibility rules.
//!
//! ## Scope today
//!
//! For the Hello-world / single-file target we only need to know:
//!
//! 1. The set of **declared** top-level types and functions in this
//!    compilation unit (populated by [`Resolver::collect_top_level`]).
//! 2. The set of **imported** names introduced by `import …;` statements
//!    in this file (populated by [`Resolver::collect_imports`]). For each
//!    import we register the local-bind name only:
//!    - `import foo.Bar;` → registers `Bar`
//!    - `import foo.Bar as B;` → registers `B`
//!    - `import foo.{ A, B as B2 };` → registers `A` and `B2`
//!    - `import foo.*;` → registers **nothing** (we'd need a module → public-
//!      names table to expand the wildcard, which doesn't exist yet).
//! 3. The set of **built-in** names provided by the compiler — currently
//!    just `print`. This stand-in stays until `import std.io.print;`
//!    becomes meaningful (i.e., when stdlib lands).
//!
//! We then walk the AST and verify every **single-segment** identifier in
//! an expression position is one of those. Unknown names produce
//! `E0301_NameNotFound`. Multi-segment paths (`std.io.print`) are accepted
//! without checking — proper module resolution lands with a stdlib.
//!
//! ## What this pass does NOT do (yet)
//!
//! - **No cross-file linking.** Each compilation unit is its own world.
//!   `import com.example.Foo;` doesn't open `com/example/Foo.jux` — it
//!   just adds `Foo` to the local known-names set so downstream phases
//!   (and the backend's `use` emission, when wired) can find it.
//! - **No wildcard expansion.** Wildcards parse and round-trip in the
//!   AST, but the resolver ignores them. With no module table, we don't
//!   know what names would be brought in.
//! - **No visibility enforcement.** `private`/`internal`/`public` modifiers
//!   are recorded by the parser but the resolver doesn't yet reject
//!   cross-module access. That lands when the build system pinpoints
//!   module boundaries.
//! - **No duplicate-import diagnostic.** Two imports introducing the
//!   same local name silently dedupe into the HashSet — there's no
//!   E-code allocated for this case in `JUX-DIAGNOSTICS-ADDENDUM.md`,
//!   so allocating one needs a spec change first.

use std::collections::HashSet;

use juxc_ast::{
    AssignStmt, Block, CallExpr, CompilationUnit, ElseBranch, Expr, FnDecl, ForEachStmt,
    IfStmt, ImportSpec, QualifiedName, Stmt, TopLevelDecl, VarDecl, WhileStmt,
};
use juxc_diagnostics::{code, Diagnostic};

/// Output of [`resolve`]. The diagnostics list is empty when every
/// name resolved cleanly.
pub struct ResolveResult {
    /// Resolution diagnostics (E0301_…).
    pub diagnostics: Vec<Diagnostic>,
}

/// Walk a compilation unit and resolve every name reference. Always
/// returns a [`ResolveResult`]; never panics on user input.
pub fn resolve(unit: &CompilationUnit) -> ResolveResult {
    let mut r = Resolver::new();
    // Imports first — they introduce names that top-level decls and
    // body expressions may both reference.
    r.collect_imports(unit);
    r.collect_top_level(unit);
    r.visit_compilation_unit(unit);
    ResolveResult { diagnostics: r.diagnostics }
}

// ============================================================================
// Resolver state
// ============================================================================

/// Internal resolver state.
///
/// Names visible at any point come from four places, checked in order:
///
/// 1. The **innermost lexical scope** in `scopes` (function bodies, `if`
///    branches, nested blocks each push a frame).
/// 2. The **top-level scope** populated by [`Resolver::collect_top_level`]
///    — every function/type declared at file scope.
/// 3. The **imported scope** populated by [`Resolver::collect_imports`]
///    — the local-bind name of every `import …;` declaration. See the
///    module doc for which imports contribute (wildcards don't, yet).
/// 4. The **built-in scope** — compiler-provided names like `print`.
///
/// Lookup short-circuits on the first hit. Shadowing is therefore implicit:
/// a `var x` inside a block hides any outer `x` for the duration of that
/// block, which matches every C-family language. Top-level decls also
/// shadow imports — if a file both declares `class Foo` and writes
/// `import bar.Foo;`, references to `Foo` resolve to the local class.
struct Resolver {
    /// Compiler-provided built-in names that any program can reference.
    /// Currently just `print` (milestone-1 stand-in for `std.io.println`).
    builtins: HashSet<&'static str>,
    /// Top-level user-declared names in this compilation unit.
    user_names: HashSet<String>,
    /// Local-bind names introduced by `import …;` declarations. See
    /// [`Self::collect_imports`] for the mapping rules. Wildcards
    /// contribute nothing here — they parse into the AST but the
    /// resolver doesn't yet have a module → public-names table to
    /// expand them against.
    imported_names: HashSet<String>,
    /// Stack of lexical scopes. The top of the stack is the current scope.
    /// Each entry is the set of names declared at that level.
    scopes: Vec<HashSet<String>>,
    /// Diagnostics accumulated as we walk.
    diagnostics: Vec<Diagnostic>,
}

impl Resolver {
    /// Construct a resolver with the standard set of built-in names
    /// pre-registered. The scope stack starts empty; a fresh frame is
    /// pushed when we enter a function body.
    fn new() -> Self {
        let mut builtins = HashSet::new();
        // The milestone-1 built-in. Eventually this will be supplanted by
        // `import std.io.print;` and removed from here.
        builtins.insert("print");
        Self {
            builtins,
            user_names: HashSet::new(),
            imported_names: HashSet::new(),
            scopes: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Push a fresh scope onto the stack. Pair with [`Self::pop_scope`].
    fn push_scope(&mut self) {
        self.scopes.push(HashSet::new());
    }

    /// Pop the most recently pushed scope. Names declared inside become
    /// invisible to subsequent lookups.
    fn pop_scope(&mut self) {
        self.scopes.pop().expect("scope stack underflow");
    }

    /// Declare `name` in the innermost current scope. If there's no
    /// active scope (we're at the global level), declare it in
    /// `user_names` instead — this lets [`Self::collect_top_level`] reuse
    /// the same code path.
    fn declare(&mut self, name: &str) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string());
        } else {
            self.user_names.insert(name.to_string());
        }
    }

    /// Walk a pattern and declare every name it introduces into the
    /// current scope. Called once per `case` arm so `var x` and nested
    /// sub-pattern binders appear in scope for the arm's body.
    fn declare_pattern_bindings(&mut self, pattern: &juxc_ast::Pattern) {
        match pattern {
            juxc_ast::Pattern::Wildcard(_)
            | juxc_ast::Pattern::Literal(_, _) => {}
            juxc_ast::Pattern::Bind(name) => self.declare(&name.text),
            juxc_ast::Pattern::EnumVariant { args, .. } => {
                for sub in args {
                    self.declare_pattern_bindings(sub);
                }
            }
        }
    }

    /// First pass: register the local-bind name of every `import` in
    /// this compilation unit into [`Self::imported_names`].
    ///
    /// Mapping rules (mirroring the module-level docs):
    ///
    /// - `ImportSpec::Path { name, wildcard: false, alias: Some(a) }` →
    ///   `a.text` (the explicit alias wins).
    /// - `ImportSpec::Path { name, wildcard: false, alias: None }` →
    ///   `name.segments.last().text` (the last path segment is the
    ///   imported symbol's local name).
    /// - `ImportSpec::Path { wildcard: true, .. }` → **skipped**. A
    ///   wildcard would need a module → public-names table to expand
    ///   against; until that exists, any name a wildcard would have
    ///   brought in still fires `E0301_NameNotFound`.
    /// - `ImportSpec::Items { items, .. }` → for each item, the alias if
    ///   present else the item's name.
    ///
    /// Defensive against parser-recovery shapes: an empty path (zero
    /// segments) is silently skipped, since the parser already emitted
    /// the relevant `E0200` and we have no local name to register.
    fn collect_imports(&mut self, unit: &CompilationUnit) {
        for import in &unit.imports {
            match &import.spec {
                ImportSpec::Path { name, wildcard, alias } => {
                    if *wildcard {
                        // Wildcards parse but don't contribute names —
                        // see the comment in the module doc. When a
                        // module table lands, this branch expands.
                        continue;
                    }
                    let local = if let Some(a) = alias {
                        a.text.clone()
                    } else if let Some(last) = name.segments.last() {
                        last.text.clone()
                    } else {
                        // Parser-recovery empty path. Skip silently.
                        continue;
                    };
                    self.imported_names.insert(local);
                }
                ImportSpec::Items { items, .. } => {
                    for item in items {
                        let local = item
                            .alias
                            .as_ref()
                            .map(|a| a.text.clone())
                            .unwrap_or_else(|| item.name.text.clone());
                        self.imported_names.insert(local);
                    }
                }
            }
        }
    }

    /// Second pass: collect every top-level declared name into the
    /// global scope. Done before the resolution walk so forward
    /// references work (declaration order shouldn't matter for
    /// top-level decls).
    fn collect_top_level(&mut self, unit: &CompilationUnit) {
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(fn_decl) => {
                    self.user_names.insert(fn_decl.name.text.clone());
                }
                TopLevelDecl::Class(class_decl) => {
                    // Register the class name so `new Foo(…)` resolves
                    // against the known set. Methods and fields aren't
                    // top-level names — they're looked up through their
                    // receiver, not as bare identifiers.
                    self.user_names.insert(class_decl.name.text.clone());
                }
                TopLevelDecl::Enum(enum_decl) => {
                    // Register the enum name so `Color.Red` and the
                    // type-position `Color` reference both resolve.
                    // Variant names aren't free identifiers (they're
                    // accessed via the enum name), so we don't add them.
                    self.user_names.insert(enum_decl.name.text.clone());
                }
                TopLevelDecl::Record(record_decl) => {
                    // Register the record name so `new Vector3(…)` and
                    // `Vector3 v` both resolve.
                    self.user_names.insert(record_decl.name.text.clone());
                }
                TopLevelDecl::Interface(interface_decl) => {
                    // Register the interface name so `class C implements I`
                    // and `<T extends I>` (when that lands) both resolve
                    // through the symbol table.
                    self.user_names.insert(interface_decl.name.text.clone());
                }
            }
        }
    }

    /// Look `name` up across all visible scopes:
    /// innermost-block → … → outermost-block → top-level → imports →
    /// built-ins. Returns `true` on the first hit; `false` if nothing
    /// matches.
    ///
    /// Imports sit after top-level decls so a locally declared `class
    /// Foo` shadows an `import bar.Foo;` — matches Java's "compilation
    /// unit's own declarations win" rule.
    fn is_known(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if scope.contains(name) {
                return true;
            }
        }
        self.user_names.contains(name)
            || self.imported_names.contains(name)
            || self.builtins.contains(name)
    }

    // ----------------------------------------------------------------------
    // AST walk
    //
    // Plain visitor style — no transforms, only diagnostic emission.
    // ----------------------------------------------------------------------

    fn visit_compilation_unit(&mut self, unit: &CompilationUnit) {
        for item in &unit.items {
            self.visit_top_level_decl(item);
        }
    }

    fn visit_top_level_decl(&mut self, item: &TopLevelDecl) {
        match item {
            TopLevelDecl::Function(fn_decl) => self.visit_fn_decl(fn_decl),
            TopLevelDecl::Class(class_decl) => self.visit_class_decl(class_decl),
            // Enum declarations have no expressions to resolve at this
            // milestone — variants reference primitive/String types
            // only, which aren't in the symbol table. When methods on
            // enums land they'll need a `visit_enum_decl` like the
            // class one.
            TopLevelDecl::Enum(_) => {}
            // Records similarly carry no expressions in the Turn-1
            // header-only form. Body methods (when added) will need
            // a visit pass equivalent to `visit_class_decl`.
            TopLevelDecl::Record(_) => {}
            // Interfaces carry method signatures only — no bodies to
            // walk in Turn 1. Default methods (when added) will need
            // a visit pass.
            TopLevelDecl::Interface(_) => {}
        }
    }

    /// Walk a class declaration's members, opening a fresh scope per
    /// constructor and method body. Within those bodies, `this` is
    /// always available; field names are NOT auto-bound (Turn 1 requires
    /// `this.field` for member access — bare-field shorthand is a Turn-2
    /// extension that needs a real type table).
    fn visit_class_decl(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // Fields may carry default-initializer expressions; resolve any
        // identifiers they reference in the top-level scope (no `this`
        // available — initializers run before the constructor body).
        for field in &class_decl.fields {
            if let Some(init) = &field.default {
                self.visit_expr(init);
            }
        }
        for ctor in &class_decl.constructors {
            self.push_scope();
            // `this` is the implicit receiver — register it as a known
            // name so reads of `this` resolve. `this.field` member
            // access then walks through the Field/Path resolution path
            // (which is suppressed at the root of a field chain anyway).
            self.declare("this");
            for param in &ctor.params {
                self.declare(&param.name.text);
            }
            self.visit_block(&ctor.body);
            self.pop_scope();
        }
        for method in &class_decl.methods {
            self.push_scope();
            self.declare("this");
            for param in &method.params {
                self.declare(&param.name.text);
            }
            if let Some(body) = &method.body {
                self.visit_block(body);
            }
            self.pop_scope();
        }
    }

    fn visit_fn_decl(&mut self, fn_decl: &FnDecl) {
        // Function body opens a new scope. Parameters would be declared
        // into that scope here once the parser carries their names in a
        // useful shape; for now hello.jux's main() has none.
        self.push_scope();
        for param in &fn_decl.params {
            self.declare(&param.name.text);
        }
        if let Some(body) = &fn_decl.body {
            self.visit_block(body);
        }
        self.pop_scope();
    }

    /// Walk the statements of a block in source order. The caller decides
    /// whether to wrap this in `push_scope` / `pop_scope` — function
    /// bodies open their scope in [`Self::visit_fn_decl`], and `if`/`else`
    /// arms open theirs in [`Self::visit_if`]. This keeps "what counts as
    /// a scope boundary" explicit at the call site.
    fn visit_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.visit_stmt(stmt);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(e) => self.visit_expr(e),
            Stmt::Return(Some(e)) => self.visit_expr(e),
            Stmt::Return(None) => {}
            Stmt::VarDecl(var) => self.visit_var_decl(var),
            Stmt::If(if_stmt) => self.visit_if(if_stmt),
            Stmt::While(w) => self.visit_while(w),
            Stmt::ForEach(f) => self.visit_for_each(f),
            Stmt::Assign(a) => self.visit_assign(a),
            // break/continue introduce no names and reference none.
            // Validating they're inside a loop is a future check.
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::SuperCall(args, _) => {
                // Walk the super-call arguments — they live in the
                // enclosing constructor's scope, so all the usual name
                // resolution applies.
                for arg in args {
                    self.visit_expr(arg);
                }
            }
        }
    }

    /// Walk a `for (var name : iter) { body }` loop.
    ///
    /// The iterator is resolved in the **outer** scope (so an enclosing
    /// `var i` is visible when the user writes `for (var j : i.range())`).
    /// The body opens a fresh scope into which the loop variable is
    /// declared — keeps `j` invisible after the loop ends.
    fn visit_for_each(&mut self, f: &ForEachStmt) {
        self.visit_expr(&f.iter);
        self.push_scope();
        self.declare(&f.var_name.text);
        self.visit_block(&f.body);
        self.pop_scope();
    }

    fn visit_while(&mut self, w: &WhileStmt) {
        // Condition lives in the enclosing scope (so a `var i` declared
        // before the loop is visible). The body opens a fresh scope so
        // bindings inside the loop don't leak out.
        self.visit_expr(&w.condition);
        self.push_scope();
        self.visit_block(&w.body);
        self.pop_scope();
    }

    /// Walk an `target = value;` assignment.
    ///
    /// The **right-hand side is resolved first** — important for things
    /// like `i = i - 1` so the rhs reads the existing binding before any
    /// staleness check on the target. Then we walk the target expression
    /// to resolve any names it contains (for `arr[i] = v`, both `arr`
    /// and `i` need to resolve).
    fn visit_assign(&mut self, a: &AssignStmt) {
        self.visit_expr(&a.value);
        self.visit_expr(&a.target);
    }

    fn visit_var_decl(&mut self, var: &VarDecl) {
        // Walk the initializer **before** declaring the name. This way
        // `var x = x + 1` correctly fails to resolve the rhs `x`, instead
        // of silently shadowing.
        if let Some(init) = &var.init {
            self.visit_expr(init);
        }
        self.declare(&var.name.text);
    }

    fn visit_if(&mut self, if_stmt: &IfStmt) {
        self.visit_expr(&if_stmt.condition);
        // Each arm gets its own scope so `var` declarations inside the
        // `if` aren't visible after the `if` ends.
        self.push_scope();
        self.visit_block(&if_stmt.then_block);
        self.pop_scope();
        if let Some(else_branch) = &if_stmt.else_branch {
            match else_branch.as_ref() {
                ElseBranch::If(inner) => self.visit_if(inner),
                ElseBranch::Block(block) => {
                    self.push_scope();
                    self.visit_block(block);
                    self.pop_scope();
                }
            }
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(_) => {}
            Expr::Path(qn) => self.check_path(qn),
            Expr::Call(c) => self.visit_call(c),
            Expr::Binary(b) => {
                self.visit_expr(&b.left);
                self.visit_expr(&b.right);
            }
            Expr::Unary(u) => self.visit_expr(&u.operand),
            Expr::Range(r) => {
                self.visit_expr(&r.start);
                self.visit_expr(&r.end);
            }
            Expr::Cast(c) => {
                self.visit_expr(&c.value);
                // Target type resolution lands when we have a type table.
            }
            Expr::SizeOf(_) => {
                // `sizeof(...)` operand is type-or-value (§5.9). We
                // skip resolution here — both type names and variable
                // names live in different namespaces and the
                // disambiguation rule (§5.9.3) is purely syntactic.
                // The backend handles classification at lowering time.
            }
            Expr::NewArray(n) => {
                // The element type's name doesn't need resolution
                // (primitive types aren't in the symbol table; user
                // types will be checked once we have a type table).
                // The size expression does.
                self.visit_expr(&n.size);
            }
            Expr::NewArrayLit(n) => {
                // Element type isn't resolved (see NewArray above) —
                // but each initializer expression is.
                for elem in &n.elements {
                    self.visit_expr(elem);
                }
            }
            Expr::Index(idx) => {
                // `arr[i]` — both the array expression and the index
                // need to resolve.
                self.visit_expr(&idx.array);
                self.visit_expr(&idx.index);
            }
            Expr::InterpString(s) => {
                // Walk each interpolation segment. Literal text needs
                // no resolution. `Bare($name)` and `Expr(${expr})` may
                // reference locals/parameters/top-level names — those
                // get the normal resolve treatment.
                for seg in &s.segments {
                    match seg {
                        juxc_ast::InterpSegment::Literal(_) => {}
                        juxc_ast::InterpSegment::Bare(ident) => {
                            // Equivalent to a single-segment Path read.
                            if !self.is_known(&ident.text) {
                                self.diagnostics.push(
                                    juxc_diagnostics::Diagnostic::error(
                                        code::Code::E0301_NameNotFound,
                                        format!(
                                            "cannot find `{}` in this scope",
                                            ident.text,
                                        ),
                                    )
                                    .with_span(ident.span),
                                );
                            }
                        }
                        juxc_ast::InterpSegment::Expr(inner) => self.visit_expr(inner),
                    }
                }
            }
            Expr::This(_) => {
                // `this` is bound at the head of each constructor /
                // method scope by `visit_class_decl`. If we ever see
                // it outside a class body, it'll come up as unresolved
                // and the user gets a clear E0301.
                if !self.is_known("this") {
                    self.diagnostics.push(
                        juxc_diagnostics::Diagnostic::error(
                            code::Code::E0301_NameNotFound,
                            "`this` is only available inside a class constructor or method",
                        ),
                    );
                }
            }
            Expr::NewObject(n) => {
                // Walk the class-name path (single segment → check it
                // resolves; multi-segment → defer to the import table
                // which doesn't exist yet, so just walk silently).
                self.check_path(&n.class_name);
                for arg in &n.args {
                    self.visit_expr(arg);
                }
            }
            Expr::Switch(s) => {
                // Walk the scrutinee in the surrounding scope, then
                // each arm in a fresh scope into which pattern bindings
                // are declared. Bindings introduced by `var name` and
                // by nested sub-patterns of a variant pattern are
                // visible across the arm's body.
                self.visit_expr(&s.scrutinee);
                for arm in &s.arms {
                    self.push_scope();
                    self.declare_pattern_bindings(&arm.pattern);
                    match &arm.body {
                        juxc_ast::SwitchBody::Expr(e) => self.visit_expr(e),
                        juxc_ast::SwitchBody::Block(b) => self.visit_block(b),
                    }
                    self.pop_scope();
                }
            }
            Expr::Field(f) => {
                // `obj.field` — Java-style member access.
                //
                // The object may be a local-or-parameter (`arr.length`)
                // or the package-path prefix of an imported name
                // (`std.io.print`). Without an import table we can't
                // reliably tell the two apart, so we **suppress
                // single-Path resolution at the root of a field chain**:
                // a bare ident as the root of `x.y[.z…]` is treated as
                // an unknown-but-acceptable package head, matching the
                // existing "multi-segment paths pass" behavior.
                //
                // Deeper structure (calls, indices, parens) still gets
                // walked so its sub-expressions are resolved.
                //
                // Trade-off: a typo'd local like `xx.length` (where
                // `xx` isn't bound) won't be caught at resolve time —
                // it surfaces as a Rust compile error instead. That's
                // identical to today's multi-segment handling; better
                // diagnostics land with imports + a type table.
                if !matches!(&*f.object, Expr::Path(_) | Expr::Field(_)) {
                    self.visit_expr(&f.object);
                }
            }
        }
    }

    fn visit_call(&mut self, call: &CallExpr) {
        self.visit_expr(&call.callee);
        for arg in &call.args {
            self.visit_expr(arg);
        }
    }

    /// Check a `Path` expression. Single-segment paths must resolve to a
    /// built-in or top-level user name. Multi-segment paths are accepted
    /// without checking until module resolution lands.
    fn check_path(&mut self, qn: &QualifiedName) {
        // Empty segments come from parser recovery; nothing to check.
        if qn.segments.is_empty() {
            return;
        }
        if qn.segments.len() == 1 {
            let name = &qn.segments[0].text;
            if !self.is_known(name) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0301_NameNotFound,
                        format!("cannot find `{name}` in this scope"),
                    )
                    .with_span(qn.segments[0].span),
                );
            }
        }
        // Multi-segment: TODO once imports/modules exist.
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use juxc_lex::lex;
    use juxc_parse::parse;
    use juxc_source::SourceFile;

    /// Drive lex → parse → resolve and return the diagnostic count from
    /// the resolve step only.
    fn resolve_count(src: &str) -> usize {
        let sf = SourceFile::new("test.jux", src);
        let lex_result = lex(&sf);
        assert!(lex_result.diagnostics.is_empty(), "lex errors: {:?}", lex_result.diagnostics);
        let parse_result = parse(&lex_result.tokens);
        assert!(parse_result.diagnostics.is_empty(), "parse errors: {:?}", parse_result.diagnostics);
        resolve(&parse_result.ast).diagnostics.len()
    }

    /// `print` is built-in: a call to it must resolve cleanly.
    #[test]
    fn print_is_builtin() {
        assert_eq!(resolve_count(r#"public void main() { print("hi"); }"#), 0);
    }

    /// An unknown identifier emits exactly one E0301.
    #[test]
    fn unknown_name_is_e0301() {
        assert_eq!(resolve_count(r#"public void main() { wibble("hi"); }"#), 1);
    }

    /// A user-declared top-level function resolves when called by another.
    #[test]
    fn user_top_level_function_resolves() {
        let src = r#"
            public void main() { helper(); }
            public void helper() { print("hi"); }
        "#;
        assert_eq!(resolve_count(src), 0);
    }

    /// Multi-segment paths pass without checking (TODO until imports land).
    #[test]
    fn multi_segment_path_passes() {
        assert_eq!(resolve_count(r#"public void main() { std.io.print("hi"); }"#), 0);
    }

    /// A `var` declares a name in the current block scope; subsequent
    /// uses resolve cleanly.
    #[test]
    fn var_declares_local_in_scope() {
        let src = r#"
            public void main() {
                var x = 10;
                print(x);
            }
        "#;
        assert_eq!(resolve_count(src), 0);
    }

    /// `var x = x + 1` is a classic gotcha: the initializer's `x` must
    /// resolve to an *outer* `x`, not the one being declared. With no
    /// outer `x`, this should fail.
    #[test]
    fn var_initializer_cannot_see_its_own_name() {
        let src = r#"
            public void main() {
                var x = x + 1;
            }
        "#;
        assert_eq!(resolve_count(src), 1);
    }

    /// A `var` inside an `if` block goes out of scope when the block ends.
    #[test]
    fn var_in_if_block_does_not_escape() {
        let src = r#"
            public void main() {
                if (true) {
                    var x = 1;
                }
                print(x);
            }
        "#;
        assert_eq!(resolve_count(src), 1);
    }

    /// Identifiers inside binary expressions are walked.
    #[test]
    fn binary_expr_operands_are_resolved() {
        // `a` and `b` are undeclared — expect 2 diagnostics.
        let src = r#"
            public void main() {
                print(a + b);
            }
        "#;
        assert_eq!(resolve_count(src), 2);
    }

    /// Assigning to a previously-declared `var` resolves cleanly. The
    /// rhs uses the existing binding; the target is a known name.
    #[test]
    fn assignment_to_declared_var_resolves() {
        let src = r#"
            public void main() {
                var i = 0;
                i = i + 1;
            }
        "#;
        assert_eq!(resolve_count(src), 0);
    }

    /// Assigning to an undeclared name emits E0301 against the target.
    #[test]
    fn assignment_to_unknown_name_is_e0301() {
        let src = r#"
            public void main() {
                wibble = 1;
            }
        "#;
        // One diagnostic — only the target. The rhs `1` is a literal,
        // nothing to resolve there.
        assert_eq!(resolve_count(src), 1);
    }

    /// The while condition and body both get walked.
    #[test]
    fn while_walks_condition_and_body() {
        // `unknownA` in condition, `unknownB` in body — 2 diagnostics.
        let src = r#"
            public void main() {
                while (unknownA > 0) { print(unknownB); }
            }
        "#;
        assert_eq!(resolve_count(src), 2);
    }

    /// for-each declares the loop variable into a body-local scope:
    /// references inside resolve, references outside do not.
    #[test]
    fn for_each_var_is_visible_in_body_only() {
        let src = r#"
            public void main() {
                for (var i : 0..10) {
                    print(i);
                }
            }
        "#;
        assert_eq!(resolve_count(src), 0);
    }

    /// Using the loop variable after the loop exits is `E0301`.
    #[test]
    fn for_each_var_is_invisible_after_the_loop() {
        let src = r#"
            public void main() {
                for (var i : 0..10) {}
                print(i);
            }
        "#;
        assert_eq!(resolve_count(src), 1);
    }

    /// Both range bounds get walked. Unknown names in start or end emit
    /// E0301 just like anywhere else.
    #[test]
    fn range_bounds_are_walked() {
        let src = r#"
            public void main() {
                for (var i : nope..wat) { }
            }
        "#;
        assert_eq!(resolve_count(src), 2);
    }

    // ------------------------------------------------------------------
    // Imports — Phase 1 (single-file local-bind registration)
    // ------------------------------------------------------------------

    /// `import com.example.Foo;` makes `Foo` resolvable as a bare name.
    #[test]
    fn bare_import_introduces_last_segment() {
        let src = r#"
            import com.example.Foo;
            public void main() {
                var f = new Foo();
            }
        "#;
        assert_eq!(resolve_count(src), 0);
    }

    /// `import com.example.Foo as Bar;` makes `Bar` resolvable. The
    /// original `Foo` is **not** introduced.
    #[test]
    fn aliased_import_introduces_alias_only() {
        let src = r#"
            import com.example.Foo as Bar;
            public void main() {
                var b = new Bar();
                var f = new Foo();
            }
        "#;
        // One E0301: `Foo` isn't in scope (only `Bar` is).
        assert_eq!(resolve_count(src), 1);
    }

    /// Grouped imports register every item independently.
    #[test]
    fn grouped_import_introduces_each_item() {
        let src = r#"
            import com.example.{ A, B as B2, C };
            public void main() {
                var a = new A();
                var b = new B2();
                var c = new C();
            }
        "#;
        assert_eq!(resolve_count(src), 0);
    }

    /// A grouped import's renamed item shadows its original name —
    /// just like the singleton-aliased form.
    #[test]
    fn grouped_aliased_item_hides_original() {
        let src = r#"
            import com.example.{ B as B2 };
            public void main() {
                var b = new B();
            }
        "#;
        // One E0301: `B` wasn't introduced — only `B2` was.
        assert_eq!(resolve_count(src), 1);
    }

    /// Wildcard imports contribute no names yet — every name a wildcard
    /// would have brought in still fails to resolve.
    #[test]
    fn wildcard_import_introduces_nothing_today() {
        let src = r#"
            import com.example.*;
            public void main() {
                var f = new Foo();
            }
        "#;
        // `Foo` would resolve once wildcards expand, but for now it's
        // E0301 like any other unimported name.
        assert_eq!(resolve_count(src), 1);
    }

    /// A locally declared class **shadows** an import with the same
    /// name — references resolve to the local declaration.
    #[test]
    fn local_decl_shadows_import_silently() {
        // Two contributors to `Foo`: a local class and an import. Both
        // are valid sources, the resolver should accept the reference
        // either way. (Detecting the conflict needs an E-code we don't
        // have allocated yet — see module doc.)
        let src = r#"
            import bar.Foo;
            public class Foo {}
            public void main() {
                var f = new Foo();
            }
        "#;
        assert_eq!(resolve_count(src), 0);
    }

    /// Imports may be referenced from top-level decls (class fields,
    /// method signatures) not just bodies — same lookup path.
    #[test]
    fn import_visible_in_top_level_decls() {
        // Today the resolver only walks expression-position names — type
        // references in field declarations aren't resolved through
        // `is_known`. This test pins the bare-body behavior; full
        // type-position resolution lands with the type-name resolver.
        let src = r#"
            import com.example.Foo;
            public void main() {
                var x = new Foo();
            }
        "#;
        assert_eq!(resolve_count(src), 0);
    }
}
