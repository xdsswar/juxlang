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
use juxc_source::Span;

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
    /// Per-class member name index: class name → set of member names
    /// (static fields, instance fields, methods) declared on that
    /// class. Used to pre-declare member names into a class body's
    /// scope so bare references (Java rule: `foo()` ≡ `this.foo()`)
    /// don't fire E0301. Built by [`Self::collect_top_level`].
    ///
    /// The names are not inherited at this point — inheritance roll-up
    /// is done at the use site by walking [`Self::class_parents`].
    class_members: std::collections::HashMap<String, HashSet<String>>,
    /// Per-class parent-name index: class name → direct `extends`
    /// target name, if any. The chain is walked at the use site so
    /// inherited members surface as bare names too.
    class_parents: std::collections::HashMap<String, String>,
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
        // Async-runtime builtin per JUX-ASYNC-ADDENDUM-v2: `parallel`
        // takes N async expressions and concurrently awaits them
        // all, returning a tuple of their results. Lowers to
        // `async { futures::join!(...) }` at the backend — i.e. a
        // Future, so the user always writes `await parallel(...)`
        // inside async code or `block_on(parallel(...))` from sync
        // code. Phase-1 stand-in for the spec's
        // `std.async.parallel` import.
        builtins.insert("parallel");
        // `block_on(future)` — drive a Future to completion from a
        // synchronous context, returning the future's resolved
        // value. Lowers to `futures::executor::block_on(...)`. Lets
        // users keep a plain `void main()` and reach into async
        // helpers without making the entry point itself async.
        builtins.insert("block_on");
        // `yield_now()` — cooperative suspension point. Lowers to
        // a one-shot Pending future that returns Ready on the
        // second poll. Awaiting this in the middle of an async
        // function gives the executor a chance to make progress
        // on sibling futures inside the same `parallel(...)` /
        // `join!` group — proves the cooperative-interleaving
        // behavior `parallel(...)` advertises.
        builtins.insert("yield_now");
        // `Worker` — opaque host-side handle to the worker thread
        // pool. The only operation today is `Worker.spawn(closure)`,
        // which runs the closure on a real OS thread and returns
        // a `Task<T>` (a Future yielding T). Per JUX-ASYNC-ADDENDUM
        // §18.2 this is the path to TRUE multi-core parallelism —
        // `parallel(...)` is cooperative concurrency on one
        // thread, `Worker.spawn` is real preemptive parallelism on
        // many threads.
        builtins.insert("Worker");
        // `now_ms()` — monotonic clock reading in milliseconds.
        // Lowers to the emitted `__jux_now_ms()` helper which
        // calls `std::time::SystemTime::now()` and returns the
        // milliseconds since the UNIX epoch as `long`. Used to
        // self-measure timing in benchmarks / stress tests
        // (e.g. proving Worker.spawn yields wall-clock speedup).
        builtins.insert("now_ms");
        // `File` — stdlib I/O host per JUX-CORE-LIB-ADDENDUM.
        // Backend recognizes `File.readText(path)` and
        // `File.writeText(path, body)` and lowers to `std::fs::*`.
        builtins.insert("File");
        // `Map` and `List` — stdlib container constructors. The
        // type emitter rewrites `Map<K, V>` → `std::collections::
        // HashMap<K, V>` and `List<T>` → `Vec<T>`; this
        // registration lets the resolver accept the type-position
        // / `new` uses without an explicit `import`.
        builtins.insert("Map");
        builtins.insert("List");
        Self {
            builtins,
            user_names: HashSet::new(),
            imported_names: HashSet::new(),
            scopes: Vec::new(),
            class_members: std::collections::HashMap::new(),
            class_parents: std::collections::HashMap::new(),
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
    ///
    /// **Same-scope shadowing** fires `E0304_DuplicateLocalDeclaration`
    /// per `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4. Outer-scope
    /// shadowing (a nested scope reusing a name) is still allowed;
    /// only collisions within the innermost active scope are
    /// rejected. Globals (no active scope) silently overwrite —
    /// duplicate top-level decls land downstream at tycheck via
    /// `E0400_DuplicateDeclaration`.
    fn declare(&mut self, name: &str) {
        self.declare_at(name, Span::DUMMY);
    }

    /// Like [`Self::declare`] but with a source span used to anchor
    /// the diagnostic on a re-declaration. Use this when the
    /// caller has a `Span` handy (e.g. the offending `Ident`'s).
    fn declare_at(&mut self, name: &str, span: Span) {
        if let Some(top) = self.scopes.last_mut() {
            if top.contains(name) {
                self.diagnostics.push(
                    juxc_diagnostics::Diagnostic::error(
                        code::Code::E0304_DuplicateLocalDeclaration,
                        format!(
                            "`{name}` is already declared in this scope; \
                             rename the local or move it into a nested block",
                        ),
                    )
                    .with_span(span),
                );
                return;
            }
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
            | juxc_ast::Pattern::Literal(_, _)
            | juxc_ast::Pattern::Range { .. } => {}
            juxc_ast::Pattern::Bind(name) => self.declare(&name.text),
            juxc_ast::Pattern::EnumVariant { args, .. } => {
                for sub in args {
                    self.declare_pattern_bindings(sub);
                }
            }
            juxc_ast::Pattern::TypeBind { binder, .. } => {
                // `case Type ident -> ...` introduces `ident` as
                // a binding scoped to the arm's body.
                self.declare(&binder.text);
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
                    // Index members + parent so a method body can
                    // pre-declare class-member names (including the
                    // inherited ones — walked via `class_parents`).
                    let mut members: HashSet<String> = HashSet::new();
                    for f in &class_decl.fields {
                        members.insert(f.name.text.clone());
                    }
                    for m in &class_decl.methods {
                        members.insert(m.name.text.clone());
                    }
                    self.class_members
                        .insert(class_decl.name.text.clone(), members);
                    if let Some(parent) = &class_decl.extends {
                        if let Some(seg) = parent.name.segments.first() {
                            self.class_parents
                                .insert(class_decl.name.text.clone(), seg.text.clone());
                        }
                    }
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
                TopLevelDecl::TypeAlias(alias) => {
                    // Register the alias name so references to it
                    // resolve. Expansion to the target type happens
                    // in tycheck's `ty_from_ref`, but at the resolver
                    // level it's just another top-level name.
                    self.user_names.insert(alias.name.text.clone());
                }
                TopLevelDecl::Const(c) => {
                    // Constants are visible as bare identifiers in
                    // expression position (`var n = PI;`), so the
                    // resolver registers their name like any other.
                    self.user_names.insert(c.name.text.clone());
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
            // Enum declarations may carry operator-override bodies in
            // their body (§O.3.4 customization on the auto-derives).
            // Variant payloads themselves reference type names only,
            // which type-position resolution handles separately.
            TopLevelDecl::Enum(enum_decl) => self.visit_enum_decl(enum_decl),
            // Records carry operator-overload bodies (§O.3.4) — walk
            // them so typos and unresolved names inside surface as
            // E0301. Header components themselves don't reference
            // anything resolvable until type-position resolution lands.
            TopLevelDecl::Record(record_decl) => self.visit_record_decl(record_decl),
            // Interfaces carry method signatures only — no bodies to
            // walk in Turn 1. Default methods (when added) will need
            // a visit pass.
            TopLevelDecl::Interface(_) => {}
            // Type aliases — target is a TypeRef; type-position
            // resolution belongs to tycheck. Nothing to walk here.
            TopLevelDecl::TypeAlias(_) => {}
            // Top-level constants — walk the initializer so
            // unresolved names inside it surface as E0301.
            TopLevelDecl::Const(c) => self.visit_expr(&c.value),
        }
    }

    /// Walk a record's body: operator overrides and methods. `this`
    /// is the implicit receiver inside each body, matching how class
    /// methods/operators are walked. Deleted operators have no body
    /// and are skipped silently.
    fn visit_record_decl(&mut self, record_decl: &juxc_ast::RecordDecl) {
        // Static-field names declared on the record are visible as
        // bare identifiers inside every operator / method body
        // (Java rule, also applies to Java records per JEP 395 §3).
        // Component names ARE visible bare too because each
        // component is a parameter of the canonical ctor; method
        // bodies still need `this.x` to read them, so we don't
        // declare the component names here.
        let static_field_names: Vec<&str> = record_decl
            .static_fields
            .iter()
            .map(|f| f.name.text.as_str())
            .collect();
        for op in &record_decl.operators {
            let Some(body) = &op.body else { continue };
            self.push_scope();
            self.declare("this");
            for name in &static_field_names {
                self.declare(name);
            }
            self.push_scope();
            for param in &op.params {
                self.declare(&param.name.text);
            }
            self.visit_block(body);
            self.pop_scope();
            self.pop_scope();
        }
        for method in &record_decl.methods {
            self.push_scope();
            self.declare("this");
            for name in &static_field_names {
                self.declare(name);
            }
            self.push_scope();
            for param in &method.params {
                self.declare(&param.name.text);
            }
            if let Some(body) = &method.body {
                self.visit_block(body);
            }
            self.pop_scope();
            self.pop_scope();
        }
    }

    /// Same shape as [`Self::visit_record_decl`] but for enums.
    fn visit_enum_decl(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        for op in &enum_decl.operators {
            let Some(body) = &op.body else { continue };
            self.push_scope();
            self.declare("this");
            for param in &op.params {
                self.declare(&param.name.text);
            }
            self.visit_block(body);
            self.pop_scope();
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
        // Static fields and methods (instance and static) of this class
        // are visible as bare names inside every constructor / method /
        // operator body (Java rule: `a` inside `class Test` resolves
        // to `Test.a`; `foo()` resolves to `this.foo()` for an instance
        // method or `Test.foo()` for a static one). We declare them
        // into an OUTER scope (so locals/params can still shadow them
        // in the inner body scope) only to suppress the spurious
        // E0301 the resolver would otherwise raise — tycheck and the
        // backend each do their own enclosing-class lookup on the
        // emission side and pick the right shape.
        //
        // Walks the `extends` chain so inherited static fields and
        // methods are also reachable bare from the subclass body.
        let mut member_names: HashSet<String> = HashSet::new();
        let mut cursor: Option<&str> = Some(class_decl.name.text.as_str());
        while let Some(n) = cursor {
            if let Some(set) = self.class_members.get(n) {
                for m in set {
                    member_names.insert(m.clone());
                }
            }
            cursor = self.class_parents.get(n).map(|s| s.as_str());
        }
        // Helper closure: predeclared class-member names land in an
        // outer scope so a local/param of the same name shadows them
        // in the inner scope without firing
        // `E0304_DuplicateLocalDeclaration`. (Same-scope duplicates
        // are only caught against the innermost frame.) `this` rides
        // along since it shouldn't conflict with a `this` param —
        // the parser already rejects that.
        for ctor in &class_decl.constructors {
            self.push_scope(); // outer: class-member visibility
            self.declare("this");
            for name in &member_names {
                self.declare(name);
            }
            self.push_scope(); // inner: ctor body locals + params
            for param in &ctor.params {
                self.declare(&param.name.text);
            }
            self.visit_block(&ctor.body);
            self.pop_scope();
            self.pop_scope();
        }
        for method in &class_decl.methods {
            self.push_scope();
            self.declare("this");
            for name in &member_names {
                self.declare(name);
            }
            self.push_scope();
            for param in &method.params {
                self.declare(&param.name.text);
            }
            if let Some(body) = &method.body {
                self.visit_block(body);
            }
            self.pop_scope();
            self.pop_scope();
        }
        // Operator overloads (§O.2). Same scope shape as methods —
        // `this` is the implicit receiver and the formal params are
        // declared into the body's scope. Without this walk, typos
        // inside an operator body would slip through silently because
        // the resolver wouldn't see references at all.
        for op in &class_decl.operators {
            self.push_scope();
            self.declare("this");
            for name in &member_names {
                self.declare(name);
            }
            self.push_scope();
            for param in &op.params {
                self.declare(&param.name.text);
            }
            if let Some(body) = &op.body {
                self.visit_block(body);
            }
            self.pop_scope();
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
            Stmt::Throw(e, _) => self.visit_expr(e),
            Stmt::Try(t) => {
                // Try body in its own scope.
                self.push_scope();
                self.visit_block(&t.body);
                self.pop_scope();
                for c in &t.catches {
                    // Each catch gets its own scope with the caught
                    // name bound. We don't validate the catch type
                    // (T) here — that's a tycheck concern.
                    self.push_scope();
                    self.declare(&c.name.text);
                    self.visit_block(&c.body);
                    self.pop_scope();
                }
                if let Some(fin) = &t.finally {
                    self.push_scope();
                    self.visit_block(fin);
                    self.pop_scope();
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
        // Use the span-aware variant so a re-declaration error
        // (`E0304_DuplicateLocalDeclaration`) lands on the new
        // `var name` token, not on the original one.
        self.declare_at(&var.name.text, var.name.span);
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
                // Anonymous-class body: each method body opens its
                // own scope with `this` + the formal params. The
                // body has no access to the enclosing method's
                // locals (the synthetic struct is stateless and
                // its methods don't capture — see spec §1379's
                // anonymous-class rules). Instance initializer
                // blocks (bare `{ … }`) get their own scope too
                // and run sequentially before the instance is
                // returned.
                if let Some(body) = &n.anonymous_body {
                    for init_block in &body.init_blocks {
                        self.push_scope();
                        self.visit_block(init_block);
                        self.pop_scope();
                    }
                    for method in &body.methods {
                        self.push_scope();
                        self.declare("this");
                        self.push_scope();
                        for param in &method.params {
                            self.declare(&param.name.text);
                        }
                        if let Some(body) = &method.body {
                            self.visit_block(body);
                        }
                        self.pop_scope();
                        self.pop_scope();
                    }
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
            Expr::Lambda(l) => {
                // Push a scope, declare each parameter as a known
                // name, then walk the body. Body-position
                // expressions and blocks both go through the
                // existing visitors.
                self.push_scope();
                for p in &l.params {
                    self.declare(&p.name.text);
                }
                match &l.body {
                    juxc_ast::LambdaBody::Expr(e) => self.visit_expr(e),
                    juxc_ast::LambdaBody::Block(b) => self.visit_block(b),
                }
                self.pop_scope();
            }
            Expr::Elvis(e) => {
                // Both the nullable value and the non-null fallback
                // are evaluated in the surrounding scope. The result
                // type is the fallback's type; resolution of either
                // side is independent.
                self.visit_expr(&e.value);
                self.visit_expr(&e.fallback);
            }
            Expr::MethodRef(m) => {
                // `Receiver::member`. Check that the receiver path
                // resolves to a known type. Member existence is
                // a tycheck job — the resolver only knows names
                // at the unit level, not method tables. Until
                // tycheck wires method-name verification for
                // method references, a typo in `member` surfaces
                // at the Rust compile step instead of as an
                // E0413; acceptable trade-off for Phase 1.
                self.check_path(&m.receiver);
            }
            Expr::Ternary(t) => {
                self.visit_expr(&t.condition);
                self.visit_expr(&t.then_branch);
                self.visit_expr(&t.else_branch);
            }
            Expr::Await(inner, _) => {
                // `await expr` just walks its operand — the operand
                // is a normal expression whose names need resolving.
                // Whether we're in an async context is the parser's
                // / tycheck's concern.
                self.visit_expr(inner);
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

    /// Same-scope `var x = …; var x = …;` fires E0304 per the
    /// diagnostics addendum §D.4.
    #[test]
    fn same_scope_duplicate_local_emits_e0304() {
        let src = r#"
            public void main() {
                var x = 1;
                var x = 2;
            }
        "#;
        let diags = resolve(&parse_clean(src)).diagnostics;
        assert!(
            diags
                .iter()
                .any(|d| d.code == code::Code::E0304_DuplicateLocalDeclaration),
            "expected E0304, got: {diags:?}",
        );
    }

    /// Outer-scope shadowing — a nested block declares a `var x`
    /// while an outer `x` is in scope — is allowed (no E0304).
    /// Mirrors Java / Kotlin / Rust: only collisions within one
    /// scope are rejected.
    #[test]
    fn nested_scope_shadowing_is_allowed() {
        let src = r#"
            public void main() {
                var x = 1;
                if (true) {
                    var x = 2;
                    print(x);
                }
                print(x);
            }
        "#;
        let diags = resolve(&parse_clean(src)).diagnostics;
        assert!(
            !diags
                .iter()
                .any(|d| d.code == code::Code::E0304_DuplicateLocalDeclaration),
            "no E0304 for nested-scope shadowing: {diags:?}",
        );
    }

    /// Helper for tests that need to inspect parse output before
    /// resolving. The existing `resolve_count` discards the AST.
    fn parse_clean(src: &str) -> juxc_ast::CompilationUnit {
        let sf = SourceFile::new("test.jux", src);
        let lex_result = juxc_lex::lex(&sf);
        assert!(lex_result.diagnostics.is_empty(), "lex: {:?}", lex_result.diagnostics);
        let parse_result = juxc_parse::parse(&lex_result.tokens);
        assert!(parse_result.diagnostics.is_empty(), "parse: {:?}", parse_result.diagnostics);
        parse_result.ast
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

    // ------------------------------------------------------------------
    // Operator overloads (§O.2) — resolver walk
    // ------------------------------------------------------------------

    /// An operator body that references its parameter and a known
    /// top-level name resolves cleanly. Pins the happy path: `this`
    /// and the formal param both land in the operator-body scope.
    #[test]
    fn operator_body_resolves_this_and_params() {
        let src = r#"
            public class Path {
                public String value;
                public Path(String v) { this.value = v; }
                public bool operator==(Path other) {
                    print(this.value);
                    print(other.value);
                    return true;
                }
            }
        "#;
        assert_eq!(resolve_count(src), 0);
    }

    /// A typo inside an operator body emits E0301 — same as a typo in
    /// any method body. Before the resolver walked operators, this
    /// would slip through.
    #[test]
    fn operator_body_typo_emits_e0301() {
        let src = r#"
            public class Path {
                public bool operator==(Path other) {
                    return wibble == other;
                }
            }
        "#;
        // One E0301: `wibble` isn't bound anywhere.
        assert_eq!(resolve_count(src), 1);
    }

    /// Operator parameters introduce names visible inside the body —
    /// and only inside the body. A reference to the param name from
    /// outside the operator (e.g., from a sibling method) fails to
    /// resolve.
    #[test]
    fn operator_param_is_scoped_to_body() {
        let src = r#"
            public class Path {
                public bool operator==(Path other) {
                    return true;
                }
                public void leak() {
                    print(other);
                }
            }
        "#;
        // One E0301: `other` is the operator's parameter and is NOT
        // visible inside `leak`'s body.
        assert_eq!(resolve_count(src), 1);
    }

    /// `operator hash()` and `operator string()` (zero-param operators)
    /// also get their bodies walked.
    #[test]
    fn zero_param_operators_walk_body() {
        let src = r#"
            public class Path {
                public int operator hash() {
                    return wibble;
                }
            }
        "#;
        assert_eq!(resolve_count(src), 1);
    }
}
