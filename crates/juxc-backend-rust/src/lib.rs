//! Phase 19 — emit Rust source code.
//!
//! This is the **Phase 1 strategy** of the language plan
//! (`JUX-LANG-V1.md` §2.2) and the normative lowering described in
//! `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.9: the Jux compiler emits
//! idiomatic Rust source that `rustc` then compiles to the final binary.
//!
//! ## Mapping
//!
//! The full §C.9.2 mapping table is large; milestone 1 needs only:
//!
//! | Jux                                       | Rust                                |
//! |-------------------------------------------|-------------------------------------|
//! | `public void main() { ... }`              | `fn main() { ... }`                 |
//! | `print(s)`                                | `println!("{}", s)`                 |
//! | `Literal::String("hi")`                   | `"hi"` (escaped for Rust)           |
//! | `Literal::Int(42)`                        | `42i64`                             |
//! | `Literal::Bool(true)`                     | `true`                              |
//! | `Literal::Null`                           | `()` (placeholder — see §C.9.4)     |
//!
//! Many more rows land as the language grows; each new construct goes in
//! the spec's mapping table first, then here.
//!
//! ## Module layout
//!
//! This crate is split into action-focused modules:
//!
//! - [`analysis`]    — pre-pass walkers and lvalue helpers.
//! - [`types`]       — type-position emission and visibility.
//! - [`decls`]       — class/record/enum/interface/function emitters.
//! - [`stmts`]       — block + statement emitters.
//! - [`exprs`]       — expression emitters and operator precedence.
//! - [`patterns`]    — switch and pattern lowering.
//! - [`sizeof_emit`] — `sizeof(...)` type-vs-value form selection.
//! - [`literals`]    — number/string/format literals and the indent helper.
//! - [`interp`]      — interpolated-string lowering.

use std::collections::{HashMap, HashSet};

use juxc_ast::{CompilationUnit, ImportDecl, ImportSpec, QualifiedName, ReturnType, TopLevelDecl};
use juxc_source::{SourceFile, Span};
use juxc_tycheck::{SymbolTable, Ty};

mod analysis;
mod backend_fqn;
mod decls;
mod exprs;
mod interp;
mod literals;
mod patterns;
mod sizeof_emit;
mod stmts;
mod types;
mod writer;

#[cfg(test)]
mod tests;

// Re-export the precedence helpers, ArgRef, and free analysis fns so
// the module-internal `pub(crate)` items are reachable from sibling
// modules through the canonical short paths used by the impl blocks.
pub(crate) use exprs::ArgRef;

use analysis::collect_user_mut_methods;

/// A fully-generated Rust crate ready to be compiled by `cargo`.
///
/// Per `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.9.5, Phase 1 always emits a
/// single binary crate (no library emissions yet). The driver writes
/// `cargo_toml` and each `(path, content)` from `sources` into
/// `target/.rust-build/` and invokes `cargo build`.
pub struct RustCrate {
    /// Contents of the generated `Cargo.toml`.
    pub cargo_toml: String,
    /// Generated source files, each a `(relative-path, contents)` pair.
    /// Paths are relative to the emitted crate root.
    pub sources: Vec<(String, String)>,
}

/// The fixed crate name for the emitted Rust crate. The driver knows to
/// look for the binary under `target/debug/{CRATE_NAME}{exe-suffix}`.
pub const CRATE_NAME: &str = "jux_emitted";

/// Lower a compilation unit to a [`RustCrate`].
///
/// Runs the full tycheck pipeline internally to produce the
/// [`SymbolTable`] and per-expression `Ty` map the emitter needs.
/// Convenient for callers (mostly tests) that don't drive tycheck
/// themselves. Drivers that already have a tycheck result on hand
/// should call [`lower_with_types`] instead to avoid the duplicate
/// work.
pub fn lower(unit: &CompilationUnit) -> RustCrate {
    // The backend doesn't care about tycheck diagnostics here — the
    // driver path runs tycheck first and bails on errors before ever
    // reaching the backend, so by the time `lower` runs the unit is
    // already known to type-check. We just need the symbol table and
    // expr-type map fall out of the same call.
    let typed = juxc_tycheck::typecheck(unit);
    lower_with_types(unit, &typed.symbols, &typed.expr_types)
}

/// Lower a compilation unit with a pre-built [`SymbolTable`] but no
/// per-expression type map.
///
/// Phase G entry point — kept for back-compat with callers (mostly
/// older tests) that only have a `SymbolTable` on hand. Falls back to
/// an empty `expr_types` map; the emitter's helpers treat missing
/// entries as "unknown" and use their conservative defaults. Drivers
/// running the full pipeline should call [`lower_with_types`] instead.
pub fn lower_with_symbols(unit: &CompilationUnit, symbols: &SymbolTable) -> RustCrate {
    lower_with_types(unit, symbols, &HashMap::new())
}

/// Lower a compilation unit with a pre-built [`SymbolTable`] AND the
/// per-expression `Ty` map tycheck produced.
///
/// Phase H entry point: drivers running the full pipeline already have
/// both a [`SymbolTable`] and an `expr_types` map on their tycheck
/// result, so they pass them in here directly. The emitter consults
/// `expr_types` instead of its old name-based heuristics
/// (`string_field_names`, `generic_field_names`, …) when deciding
/// whether to emit auto-`.clone()` / `.to_string()` coercions on field
/// reads, assignments, and enum variant construction.
///
/// No source-map markers are emitted on this path — callers that
/// want `// JUX:file:line:col` markers in the generated Rust should
/// use [`lower_with_source`]. Keeping this entry marker-free preserves
/// snapshot stability for the existing backend test suite.
pub fn lower_with_types(
    unit: &CompilationUnit,
    symbols: &SymbolTable,
    expr_types: &HashMap<Span, Ty>,
) -> RustCrate {
    lower_with_source(unit, symbols, expr_types, None)
}

/// Like [`lower_with_types`] but also emits `// JUX:file:line:col`
/// source-map markers throughout the generated Rust. When `source` is
/// `Some(file)`, the emitter sprinkles markers before each top-level
/// declaration, each method/operator/constructor signature, and each
/// statement so rustc errors on the emitted Rust map back to source
/// locations in the original `.jux` file. `None` falls back to plain
/// emission identical to [`lower_with_types`].
///
/// This is the audit Tier 2.2 "source map" mechanism — crude (string
/// comments, not real DWARF), but enough that a user can grep up from
/// a rustc error line to the nearest marker and find the offending
/// Jux site.
pub fn lower_with_source(
    unit: &CompilationUnit,
    symbols: &SymbolTable,
    expr_types: &HashMap<Span, Ty>,
    source: Option<&SourceFile>,
) -> RustCrate {
    let mut e = RustEmitter::new(symbols, expr_types.clone());
    e.source = source.cloned();
    // Patch the AUTO-GENERATED banner's `Source:` line with the real
    // file path so clickable terminal output lands the user back in
    // the originating `.jux` file. Workspace mode (multiple sources)
    // leaves the placeholder alone; per-statement `// JUX:` markers
    // already supply per-line precision in that case.
    if let Some(src) = source {
        let line = format!("// Source: {}\n", src.path().display());
        e.w.replace_first("// Source: <jux compilation unit>\n", &line);
    }
    e.emit_compilation_unit(unit);
    e.finish()
}

/// Multi-unit variant of [`lower_with_source`]. Emits a single
/// `RustCrate` whose `src/main.rs` concatenates every unit's
/// lowering. Each unit is wrapped in its own `package` module tree
/// (or emitted flat if it has no `package` decl), and the crate-root
/// `fn main()` shim is emitted once — against whichever unit
/// declares `main()`. Multiple `main()` definitions are a build
/// error that tycheck catches via `E0400_DuplicateDeclaration`.
///
/// `sources` is parallel to `units` (same length); each entry is
/// the original `SourceFile` for its unit, used to anchor source-map
/// markers per-unit. Pass an empty `sources` slice to suppress
/// marker emission entirely.
pub fn lower_workspace(
    units: &[CompilationUnit],
    symbols: &SymbolTable,
    expr_types: &HashMap<Span, Ty>,
    sources: &[SourceFile],
) -> RustCrate {
    let mut e = RustEmitter::new(symbols, expr_types.clone());
    e.workspace_mode = true;
    // Union the per-unit `user_mut_methods` sets so the `&mut self`
    // promotion analysis sees mutating methods declared in OTHER
    // files. Without this, `var c = new Cart(); c.add(item);` in
    // `app.jux` fails to promote `c` to `let mut` because `add`'s
    // mutation flag lives in `cart.jux`'s per-unit set. The
    // per-unit reset in `emit_package_node_body` would clobber the
    // union, so workspace-mode skips that reset (see the gate
    // there).
    for unit in units {
        let unit_set = collect_user_mut_methods(unit);
        e.user_mut_methods.extend(unit_set);
    }
    // Single-source workspaces (e.g. `juxc foo.jux`) get a precise
    // `// Source:` header line so terminal emit-dir click-throughs
    // land in the user's `.jux` file. Multi-source workspaces leave
    // the placeholder alone — the per-statement `// JUX:` markers
    // carry the per-line precision in that case.
    if sources.len() == 1 {
        let line = format!("// Source: {}\n", sources[0].path().display());
        e.w.replace_first("// Source: <jux compilation unit>\n", &line);
    }
    // Build a package tree so sibling packages share their parent
    // `pub mod` wrapper. Without this, two units in `lib.first` and
    // `lib.second` would each emit their own top-level `pub mod lib`
    // and Rust would reject the second as a redefinition.
    let mut tree = PackageNode::default();
    for (i, unit) in units.iter().enumerate() {
        let pkg: Vec<String> = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
            .unwrap_or_default();
        tree.insert(&pkg, i);
    }
    e.emit_package_tree(&tree, units, sources);
    // One crate-root `fn main()` shim that delegates into whichever
    // unit declared `void main()` inside a package.
    e.source = None;
    e.emit_workspace_main_shim(units);
    e.finish()
}

/// Internal node in the per-workspace package tree built by
/// `lower_workspace`. Each node owns the bare path component for its
/// level plus the indices (into the `units` slice) of every unit
/// that lives at exactly this level. Children indexed by their
/// next-segment name.
#[derive(Default)]
struct PackageNode {
    /// Children keyed by their next-segment name. Iteration is
    /// stable per the BTreeMap so emitted output is deterministic.
    children: std::collections::BTreeMap<String, PackageNode>,
    /// Unit indices whose package path ends at this node.
    unit_indices: Vec<usize>,
}

impl PackageNode {
    fn insert(&mut self, path: &[String], unit_idx: usize) {
        if path.is_empty() {
            self.unit_indices.push(unit_idx);
            return;
        }
        self.children
            .entry(path[0].clone())
            .or_default()
            .insert(&path[1..], unit_idx);
    }
}

// ============================================================================
// Emitter
// ============================================================================

/// Internal emitter state. Accumulates source text into a [`Writer`]
/// (auto-indent + buffer) and produces a [`RustCrate`] when
/// [`RustEmitter::finish`] is called.
///
/// `mutated_in_fn` is the set of local names that are reassigned somewhere
/// in the current function's body. It's recomputed by
/// [`collect_mutated_names`] before each function is emitted, and read by
/// [`Self::emit_var_decl`] to decide between `let` and `let mut`.
struct RustEmitter {
    /// Output buffer with indent tracking. Existing emitters use
    /// [`Writer::push_str`] / [`Writer::push`] for explicit text
    /// appends — same shape as the previous `out: String` field. New /
    /// migrated emitters use the indent-aware [`Writer::line`] /
    /// [`Writer::indent_inc`] / [`Writer::indent_dec`] /
    /// [`Writer::emit_indent`] helpers so the depth bookkeeping lives
    /// in one place.
    w: writer::Writer,
    mutated_in_fn: HashSet<String>,
    /// What the Jux source's `this` keyword lowers to in the current
    /// emission scope:
    /// - `Some("__self")` inside a constructor body (struct-builder pattern).
    /// - `Some("self")` inside an instance method body.
    /// - `None` everywhere else (and `this` would never appear there
    ///   — the resolver flags it earlier).
    this_alias: Option<String>,
    /// Name of the class whose body we're currently emitting (a
    /// constructor, method, or operator). Used to rewrite bare
    /// references to static fields — `a` inside `class Test`'s
    /// method body resolves to `Test.a`, matching Java's
    /// member-access rule. `None` while emitting top-level
    /// functions or `main`. Set in `emit_method` /
    /// `emit_constructor` / `emit_operator_as_method` for the
    /// duration of each body and restored afterwards.
    pub(crate) enclosing_class: Option<String>,
    /// Name of the interface whose default-method body we're
    /// currently emitting. Powers bare-name method-call rewrites
    /// inside `default` bodies — `monthlySalary()` resolves to
    /// `self.monthlySalary()` when `monthlySalary` is declared on
    /// the enclosing interface, matching Java's member-access rule.
    /// `None` outside an interface default-method body. Set in
    /// `emit_interface_decl` only.
    pub(crate) enclosing_interface: Option<String>,
    /// Names of user-defined methods whose bodies write to `this.field`
    /// — i.e. methods that the backend emits with `&mut self`. Computed
    /// in a single pre-pass over the compilation unit before any
    /// function is emitted. The mutation analyzer consults this set
    /// when it sees `receiver.name(...)` calls, so `let p` gets
    /// promoted to `let mut p` whenever the user calls a mutating
    /// method on it. Without a real type table we can't filter by
    /// receiver type — a same-named method on a different class would
    /// also tag the receiver — but the worst case is a redundant `mut`.
    user_mut_methods: HashSet<String>,
    /// True while we're emitting the LHS of an assignment statement —
    /// suppresses the `.clone()` insertion in `emit_field` so we don't
    /// produce nonsense like `self.name.clone() = "x";`.
    emitting_lvalue: bool,
    /// True while we're emitting a `const NAME: T = …;` (or
    /// `pub static NAME: T = …;`) initializer. Rust's const evaluator
    /// can't run `String::from`/`.to_string()`, so the literal must
    /// stay as a bare `&'static str` — `emit_literal` consults this
    /// flag to suppress the Fix-1 `.to_string()` wrap, and the
    /// const-context type emitter maps Jux `String` to
    /// `&'static str` instead of owned `String`. Reads of the const
    /// are still typed by the surrounding context.
    pub(crate) emitting_const_context: bool,
    /// True while we're emitting an argument **inside a `format!` /
    /// `println!` macro slot**. Those macros take their args by
    /// reference (`Display`), so a `&'static str` works as well as
    /// an owned `String`. Setting this flag tells `emit_literal` to
    /// drop the `.to_string()` self-coerce — keeps the emitted Rust
    /// readable (`format!("{}{}", "hi", name)` vs.
    /// `format!("{}{}", "hi".to_string(), name)`) without changing
    /// semantics.
    pub(crate) emitting_format_arg: bool,
    /// True while we're emitting an operand of an **equality or
    /// ordering comparison** (`==`, `!=`, `<`, `<=`, `>`, `>=`).
    /// Rust's `PartialEq`/`PartialOrd` trait methods take `&self`,
    /// so the operands are borrowed — auto-`.clone()` on
    /// `String`/generic field reads is wasted in this position.
    /// Mirrors the `emitting_format_arg` discipline: set in the
    /// comparison emitter, consulted by `emit_field`.
    pub(crate) emitting_comparison_operand: bool,
    /// True while emitting an expression whose result must be
    /// `Option<T>` (most commonly a return value of a `T?`-typed
    /// function). When set, expression emitters that produce
    /// multi-arm value shapes — `switch` today, ternary in the
    /// future — push the `Some(...)` wrap *into each arm body*
    /// instead of around the outer expression. This keeps mixed
    /// `T` / `null` arms unifiable: `case A -> "x"; case B -> null`
    /// becomes `match … { A => Some("x".to_string()), B => None }`
    /// rather than the broken `Some(match … { … })` form.
    pub(crate) emitting_nullable_target: bool,
    /// Names of locals in the current function body whose declared
    /// type is nullable (`T?`). Populated by [`Self::emit_var_decl`]
    /// when it sees a `var_decl.ty` with `nullable = true`, and
    /// consulted by the nullable-wrap helper to decide whether a
    /// path reference is already `Option<T>`-shaped (no extra
    /// `Some(...)` wrap needed) or a plain `T` value flowing into
    /// a `T?` slot (wrap it).
    ///
    /// Reset at function-body boundaries by the entry points
    /// (`emit_fn_body`, `emit_constructor`, …) so a local from
    /// one function doesn't leak into another's emission. Cleared
    /// in tandem with [`Self::mutated_in_fn`].
    pub(crate) nullable_locals: HashSet<String>,
    /// Declared return type of the function / method / operator body
    /// currently being emitted. `None` outside any function body and
    /// inside constructor bodies (constructors return `Self`).
    /// Drives the return-position string-literal coercion: a `return
    /// "literal";` inside a `String`-returning fn lowers to
    /// `return "literal".to_string();` so the `&str` → owned `String`
    /// gap doesn't surface as an E0308 at rustc time.
    pub(crate) current_return_type: Option<ReturnType>,
    /// Optional original [`SourceFile`] for emitting `// JUX:file:line:col`
    /// source-map markers. `None` (the default) skips markers, keeping
    /// the emitted Rust unchanged from the pre-markers shape — this is
    /// what existing test snapshots rely on. The driver enables markers
    /// by calling [`lower_with_source`] with `Some(source)`.
    pub(crate) source: Option<SourceFile>,
    /// Tycheck's symbol table (Phase G). Cloned rather than borrowed
    /// to keep `RustEmitter` lifetime-parameter-free — the table is
    /// built once per compilation unit and held immutably during
    /// emission, so the clone cost is paid once. Replaces what used
    /// to be the backend's own ad-hoc `class_names`/`class_parents`/
    /// `enum_names`/`interface_methods` pre-pass collections.
    ///
    /// Reads at call sites:
    /// - class membership → `symbols.classes.contains_key(name)`
    /// - parent of a class → `symbols.classes[name].extends`
    /// - enum membership → `symbols.enums.contains_key(name)`
    /// - interface method sigs → `symbols.interfaces[name].methods`
    ///   (returns `&HashMap<String, MethodSig>`; iterate via the
    ///   original interface's `methods` Vec when source order matters)
    symbols: SymbolTable,
    /// Per-expression inferred type as produced by tycheck (Phase H),
    /// keyed by the expression's source [`Span`]. Replaces the three
    /// old name-based heuristic shadow tables
    /// (`string_field_names`, `generic_field_names`,
    /// `enum_string_slots`) that the backend used to compute via
    /// `analysis::collect_*` pre-passes — the precise per-expression
    /// types come straight from tycheck now, with no risk of
    /// cross-class field-name collisions.
    ///
    /// **Missing entries.** Some expressions never had their type
    /// recorded — tycheck may not have walked them (e.g. when an
    /// earlier error short-circuited), or they carry `Span::DUMMY`
    /// (synthesized expressions). Call sites that consult this map
    /// (`emit_field`, `emit_assign`, `emit_call`, `emit_simple_ctor_body`)
    /// fall back conservatively on a miss, matching the previous
    /// heuristic behavior so existing programs still emit identical
    /// Rust.
    expr_types: HashMap<Span, Ty>,
    /// When `true`, the emitter is producing a multi-unit workspace
    /// crate. `emit_compilation_unit` skips its own crate-root
    /// `fn main()` shim in this mode — the workspace driver
    /// (`lower_workspace`) emits exactly one shim at the end, after
    /// every unit has been laid down. Single-unit emitters
    /// (`lower_with_source`, `lower`, etc.) leave this `false` so
    /// existing behavior is preserved unchanged.
    workspace_mode: bool,
    /// Index into `symbols.units` for the compilation unit currently
    /// being emitted. Powers import-alias-aware bare-name lookups
    /// in the backend — the unit's [`UnitContext::unqualified`]
    /// map carries `alias_name → FQN` for both bare imports and
    /// grouped `{ X as Y }` aliases. `None` outside workspace
    /// emission (legacy single-file paths don't have an `units`
    /// table to consult).
    pub(crate) current_unit_idx: Option<usize>,
    /// Monotonic counter for anonymous-class instances seen during
    /// emission. Each `new Iface() { … }` site mints a fresh struct
    /// name (`__JuxAnon0`, `__JuxAnon1`, …) at the use site so
    /// distinct anonymous classes don't collide.
    pub(crate) anonymous_class_counter: usize,
}

impl RustEmitter {
    /// Construct an emitter primed with tycheck's [`SymbolTable`] and
    /// per-expression `Ty` map.
    ///
    /// Both the table and the map are moved in (the lib.rs entry
    /// points clone before calling) so the emitter can hold them
    /// without lifetime parameters spreading across the eleven backend
    /// impl blocks.
    fn new(symbols: &SymbolTable, expr_types: HashMap<Span, Ty>) -> Self {
        let mut w = writer::Writer::new();
        // File header: AUTO-GENERATED banner plus a block of crate-wide
        // `#![allow(...)]` so users never see Rust warnings on code
        // they didn't write. Per JUX-CODEGEN-FIXES.md Fix 3. The
        // per-file `// Source:` line is patched in by
        // [`Self::set_source_in_header`] when a `SourceFile` is
        // attached; workspace emissions leave it as the generic note
        // since a single crate spans multiple `.jux` files.
        w.push_str("// AUTO-GENERATED by juxc. DO NOT EDIT.\n");
        w.push_str("// Source: <jux compilation unit>\n");
        w.push_str("\n");
        w.push_str("#![allow(dead_code)]\n");
        w.push_str("#![allow(unused_variables)]\n");
        w.push_str("#![allow(unused_imports)]\n");
        w.push_str("#![allow(unused_mut)]\n");
        w.push_str("#![allow(unused_parens)]\n");
        w.push_str("#![allow(non_snake_case)]\n");
        w.push_str("#![allow(non_camel_case_types)]\n");
        w.push_str("#![allow(non_upper_case_globals)]\n");
        w.push_str("#![allow(clippy::all)]\n\n");
        // Prelude: a tiny `Display` adapter for `Option<T>` so
        // `print(maybeName)` produces `"value"` or `"null"` rather
        // than failing to compile (Rust's std doesn't impl
        // `Display` for `Option`). Used by the print/format-arg
        // emit paths whenever the value being formatted has
        // nullable shape; non-nullable args pass straight through.
        // Hidden behind `#[allow(dead_code)]` from the block
        // above, so unused-Optional programs don't emit a warning.
        w.push_str("struct JuxOpt<'a, T: std::fmt::Display>(&'a Option<T>);\n");
        w.push_str("impl<'a, T: std::fmt::Display> std::fmt::Display for JuxOpt<'a, T> {\n");
        w.push_str("    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n");
        w.push_str("        match self.0 {\n");
        w.push_str("            Some(v) => std::fmt::Display::fmt(v, f),\n");
        w.push_str("            None => f.write_str(\"null\"),\n");
        w.push_str("        }\n");
        w.push_str("    }\n");
        w.push_str("}\n\n");
        Self {
            w,
            mutated_in_fn: HashSet::new(),
            this_alias: None,
            enclosing_class: None,
            enclosing_interface: None,
            user_mut_methods: HashSet::new(),
            emitting_lvalue: false,
            emitting_const_context: false,
            emitting_format_arg: false,
            emitting_comparison_operand: false,
            emitting_nullable_target: false,
            nullable_locals: HashSet::new(),
            current_return_type: None,
            source: None,
            symbols: symbols.clone(),
            expr_types,
            workspace_mode: false,
            current_unit_idx: None,
            anonymous_class_counter: 0,
        }
    }

    /// If a source file is attached (i.e. emission was driven through
    /// [`lower_with_source`]) AND `span` carries a real byte offset,
    /// emit a `// JUX:filename:line:col` comment on its own line at
    /// the current indent. No-op when `source` is `None` (the path
    /// existing tests take) or when the span is [`Span::DUMMY`].
    ///
    /// Markers go on their own line so they don't interfere with
    /// post-line rendering (e.g. inline expressions inside a complex
    /// statement). A line-leading `//` is also harmless inside Rust
    /// blocks at any indent level.
    pub(crate) fn emit_source_marker(&mut self, span: Span) {
        let Some(source) = &self.source else { return };
        if span == Span::DUMMY {
            return;
        }
        let (line, col) = source.line_col(span.start as usize);
        let path = source.path().display().to_string();
        // Strip any leading `./` or directory prefix isn't worth the
        // complexity — keep the rendered path verbatim so users can
        // grep for the exact string rustc would echo back.
        let marker = format!("// JUX:{path}:{line}:{col}");
        self.w.line(&marker);
    }

    /// Walk the AST and emit Rust source for each top-level item.
    ///
    /// Class/enum/interface metadata (names, parents, interface
    /// method signatures) is read directly from `self.symbols` —
    /// the four old `collect_*` pre-passes that built parallel
    /// `HashSet`/`HashMap` shadow tables were retired in Phase G.
    /// The three remaining "is this a String field" / "generic field" /
    /// "enum String slot" heuristic pre-passes retired in Phase H —
    /// the backend now reads precise per-expression types from
    /// `expr_types` instead. Only the receiver-mutation pre-pass
    /// (`collect_user_mut_methods`) survives, since `&mut self`
    /// promotion is name-keyed by construction.
    fn emit_compilation_unit(&mut self, unit: &CompilationUnit) {
        // Pre-pass: collect names of user methods that need `&mut self`.
        // Mutation analysis in `main` (and elsewhere) consults this set
        // so that calling `p.shift(…)` correctly promotes `p` to `let mut`.
        self.user_mut_methods = collect_user_mut_methods(unit);

        // Each unit is wrapped in its OWN package's module path —
        // read from the unit's parsed `package foo.bar;` declaration
        // rather than the workspace-level table. In multi-unit mode
        // the workspace table's `package` is only the FIRST unit's
        // (kept as a back-compat marker), so consulting `unit.package`
        // is the source of truth for per-unit emission.
        let package: Vec<String> = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
            .unwrap_or_default();
        let has_main = unit.items.iter().any(|item| {
            matches!(item, TopLevelDecl::Function(fn_decl) if fn_decl.name.text == "main")
        });

        if package.is_empty() {
            // No package — emit flat at crate root, same as before.
            self.emit_imports(&unit.imports, /*inside_package_mod=*/ false);
            for item in &unit.items {
                self.emit_top_level_decl(item);
            }
            return;
        }

        // `package a.b;` → wrap everything in `pub mod a { pub mod b
        // { … } }`. Imports stay inside the innermost module so
        // their `use` statements are scoped where the body lives.
        for seg in &package {
            self.w.emit_indent();
            self.w.push_str("pub mod ");
            self.w.push_str(seg);
            self.w.push_str(" {\n");
            self.w.indent_inc();
        }
        // Inside a `mod`, plain `use foo::bar;` resolves relative to
        // the current module, not the crate root. Flip the rewrite
        // so `import com.lib.Greeter;` becomes `use crate::com::lib::Greeter;`
        // — that anchors at the crate root and matches the `pub mod`
        // structure the workspace emits per-unit.
        self.emit_imports(&unit.imports, /*inside_package_mod=*/ true);
        for item in &unit.items {
            self.emit_top_level_decl(item);
        }
        for _ in &package {
            self.w.indent_dec();
            self.w.line("}");
        }

        // Rust's binary entry point lives at the crate root. When the
        // user's `void main()` got buried inside the module tree, emit
        // a shim at top-level that forwards into it. Without this, the
        // emitted crate compiles as a library and `--run` has nothing
        // to launch.
        //
        // Workspace mode defers shim emission to `lower_workspace`
        // (which picks one main across every unit), so this branch
        // only runs in the single-unit path.
        if has_main && !self.workspace_mode {
            self.w.newline();
            self.w.line("fn main() {");
            self.w.indent_inc();
            self.w.emit_indent();
            let path = package.join("::");
            self.w.push_str(&path);
            self.w.push_str("::main();\n");
            self.w.indent_dec();
            self.w.line("}");
        }
    }

    /// Emit the workspace's package tree. Each top-level child of
    /// the root produces one `pub mod {name} { … }` block; nested
    /// children recurse inside it. Units that live exactly at a
    /// given level have their bodies inlined into that level's
    /// scope.
    ///
    /// Root-level (no-package) units are emitted flat at the crate
    /// root, matching the existing single-file behavior.
    pub(crate) fn emit_package_tree(
        &mut self,
        tree: &PackageNode,
        units: &[CompilationUnit],
        sources: &[SourceFile],
    ) {
        // No-package units (the bare crate-root tier) first, so the
        // overall ordering stays close to "input order modulo
        // package grouping".
        for &idx in &tree.unit_indices {
            self.source = sources.get(idx).cloned();
            self.emit_compilation_unit(&units[idx]);
        }
        // Then each top-level package, with its descendants nested.
        for (name, child) in &tree.children {
            self.w.emit_indent();
            self.w.push_str("pub mod ");
            self.w.push_str(name);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            self.emit_package_node_body(child, units, sources);
            self.w.indent_dec();
            self.w.line("}");
        }
    }

    /// Body of a single package node — emit every unit belonging to
    /// this level (inlined, without its own package wrapper),
    /// followed by recursive `pub mod` blocks for each child
    /// sub-package.
    fn emit_package_node_body(
        &mut self,
        node: &PackageNode,
        units: &[CompilationUnit],
        sources: &[SourceFile],
    ) {
        for &idx in &node.unit_indices {
            let unit = &units[idx];
            // In workspace mode the union across all units is
            // pre-computed in `lower_workspace`; per-unit reset
            // here would clobber it and reintroduce the cross-file
            // `&mut` promotion gap. Single-unit emission (the
            // legacy `lower_with_*` entry points that flip back
            // through this code path) keeps the per-unit recompute.
            if !self.workspace_mode {
                self.user_mut_methods = collect_user_mut_methods(unit);
            }
            // Track which unit we're emitting so import-alias
            // aware bare-name lookups (e.g. `Catalog.describe()`
            // where `Catalog` is `CatalogOps`'s alias) can consult
            // the unit's `unqualified` map.
            let prev_unit = self.current_unit_idx.take();
            self.current_unit_idx = Some(idx);
            self.source = sources.get(idx).cloned();
            // Imports inside a packaged unit need `crate::`-rooted
            // paths so they don't resolve relative to the current
            // module nest.
            self.emit_imports(&unit.imports, /*inside_package_mod=*/ true);
            for item in &unit.items {
                self.emit_top_level_decl(item);
            }
            self.current_unit_idx = prev_unit;
        }
        for (name, child) in &node.children {
            self.w.emit_indent();
            self.w.push_str("pub mod ");
            self.w.push_str(name);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            self.emit_package_node_body(child, units, sources);
            self.w.indent_dec();
            self.w.line("}");
        }
    }

    /// Emit a single crate-root `fn main()` shim that delegates into
    /// whichever unit declared `void main()` (and lives inside a
    /// `package`).
    ///
    /// - Zero units with a packaged `main()` → nothing emitted; the
    ///   caller is presumably building a library, or the main lives
    ///   at the crate root and doesn't need a shim.
    /// - Exactly one packaged `main()` → emit
    ///   `fn main() { path::to::main(); }`.
    /// - Multiple `main()`s — tycheck's `E0400_DuplicateDeclaration`
    ///   already fired during symbol-table merging, so the backend
    ///   path here is unreachable for clean compiles. Emit a shim
    ///   for the first occurrence anyway so partially-erroring
    ///   builds still produce SOMETHING valid.
    pub(crate) fn emit_workspace_main_shim(&mut self, units: &[CompilationUnit]) {
        for unit in units {
            let has_main = unit.items.iter().any(|item| {
                matches!(item, TopLevelDecl::Function(fn_decl) if fn_decl.name.text == "main")
            });
            if !has_main {
                continue;
            }
            let pkg: Vec<&str> = unit
                .package
                .as_ref()
                .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect())
                .unwrap_or_default();
            if pkg.is_empty() {
                // Unit's `main()` is already at the crate root —
                // nothing to forward through, the user's `fn main()`
                // emission IS the binary entry point.
                return;
            }
            self.w.newline();
            self.w.line("fn main() {");
            self.w.indent_inc();
            self.w.emit_indent();
            self.w.push_str(&pkg.join("::"));
            self.w.push_str("::main();\n");
            self.w.indent_dec();
            self.w.line("}");
            return;
        }
    }

    /// Emit one Rust `use` statement per Jux `import` declaration.
    ///
    /// Mapping (per `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.9 — Jux `.`
    /// lowers to Rust `::`):
    ///
    /// | Jux                                  | Rust                                  |
    /// |--------------------------------------|---------------------------------------|
    /// | `import com.example.Foo;`            | `use com::example::Foo;`              |
    /// | `import com.example.*;`              | `use com::example::*;`                |
    /// | `import com.example.Foo as Bar;`     | `use com::example::Foo as Bar;`       |
    /// | `import com.example.{A, B as B2};`   | `use com::example::{A, B as B2};`     |
    ///
    /// A trailing blank line separates the import block from the body
    /// when at least one `use` was emitted, matching idiomatic Rust.
    ///
    /// Defensive cases:
    /// - **Empty path** (parser recovery) — skip silently, no `use::;`.
    /// - **Empty grouped import** (parser-recovery shape; parser already
    ///   rejected this with `E0200`) — skip the whole declaration.
    /// - **Wildcard + alias** (parser already rejected) — the alias is
    ///   dropped; emit just the wildcard form so the result is at least
    ///   valid Rust.
    fn emit_imports(&mut self, imports: &[ImportDecl], inside_package_mod: bool) {
        let mut emitted_any = false;
        for import in imports {
            if let Some(line) = render_use(&import.spec, inside_package_mod) {
                self.w.line(&line);
                emitted_any = true;
            }
        }
        // Blank line after the use block — keeps the emitted source
        // readable and matches `cargo fmt` defaults.
        if emitted_any {
            self.w.newline();
        }
    }

    fn emit_top_level_decl(&mut self, item: &TopLevelDecl) {
        // Source-map marker (when `source` is set) anchored at the
        // declaration's start. Lets rustc diagnostics on the
        // generated Rust scan upward to find the nearest `.jux`
        // location for the offending decl.
        let span = match item {
            TopLevelDecl::Function(d) => d.span,
            TopLevelDecl::Class(d) => d.span,
            TopLevelDecl::Enum(d) => d.span,
            TopLevelDecl::Record(d) => d.span,
            TopLevelDecl::Interface(d) => d.span,
            TopLevelDecl::TypeAlias(d) => d.span,
            TopLevelDecl::Const(d) => d.span,
        };
        self.emit_source_marker(span);
        // Pre-decl Rust attribute emission for built-in
        // annotations (`@Deprecated`, `@Cfg`). User-defined
        // annotations are still Phase-2 work.
        let anns: &[juxc_ast::Annotation] = match item {
            TopLevelDecl::Function(d) => &d.annotations,
            TopLevelDecl::Class(d) => &d.annotations,
            TopLevelDecl::Enum(d) => &d.annotations,
            TopLevelDecl::Record(d) => &d.annotations,
            TopLevelDecl::Interface(d) => &d.annotations,
            TopLevelDecl::TypeAlias(d) => &d.annotations,
            TopLevelDecl::Const(d) => &d.annotations,
        };
        let anns_owned: Vec<juxc_ast::Annotation> = anns.to_vec();
        self.emit_annotation_attrs(&anns_owned);
        match item {
            TopLevelDecl::Function(fn_decl) => self.emit_fn_decl(fn_decl),
            TopLevelDecl::Class(class_decl) => self.emit_class_decl(class_decl),
            TopLevelDecl::Enum(enum_decl) => self.emit_enum_decl(enum_decl),
            TopLevelDecl::Record(record_decl) => self.emit_record_decl(record_decl),
            TopLevelDecl::Interface(interface_decl) => {
                self.emit_interface_decl(interface_decl);
            }
            TopLevelDecl::TypeAlias(alias) => self.emit_type_alias_decl(alias),
            TopLevelDecl::Const(c) => self.emit_const_decl(c),
        }
    }

    /// Emit a Jux `const T NAME = expr;` (or its `final` synonym)
    /// as a Rust `pub const NAME: T = expr;`. Same visibility
    /// rule as `emit_fn_decl` (honor declared visibility only
    /// inside a `pub mod` wrapper).
    pub(crate) fn emit_const_decl(&mut self, decl: &juxc_ast::ConstDecl) {
        self.w.emit_indent();
        if !self.symbols.package.is_empty() || self.workspace_mode {
            self.emit_visibility(decl.visibility);
        }
        self.w.push_str("const ");
        self.w.push_str(&decl.name.text);
        self.w.push_str(": ");
        // Const-context emission. Jux `String` → Rust `&'static str`
        // here, since `String::new`/`.to_string` aren't const fns.
        // The matching `emit_literal` path drops its `.to_string()`
        // wrap when this flag is set, so the value text and type
        // line up.
        self.emitting_const_context = true;
        self.emit_type_as_rust(&decl.ty);
        self.w.push_str(" = ");
        self.emit_expr(&decl.value);
        self.emitting_const_context = false;
        self.w.push_str(";\n");
        self.w.newline();
    }

    /// Emit a Jux `type Foo<...>? = TargetTy;` as a Rust
    /// `pub type Foo<...>? = TargetTy;`. Visibility follows the
    /// user's declared modifier (mirroring `emit_fn_decl`'s
    /// inside-module-mod rule); the target lowers through the
    /// normal type-emission path so primitive/generic/wildcard
    /// shapes pick up their usual mappings.
    pub(crate) fn emit_type_alias_decl(&mut self, alias: &juxc_ast::TypeAliasDecl) {
        self.w.emit_indent();
        if !self.symbols.package.is_empty() || self.workspace_mode {
            self.emit_visibility(alias.visibility);
        }
        self.w.push_str("type ");
        self.w.push_str(&alias.name.text);
        self.emit_generic_params(&alias.generic_params);
        self.w.push_str(" = ");
        self.emit_type_as_rust(&alias.target);
        self.w.push_str(";\n");
        self.w.newline();
    }

    /// Wrap up: build the Cargo.toml and bundle with the emitted source.
    fn finish(self) -> RustCrate {
        RustCrate {
            cargo_toml: cargo_toml_for(CRATE_NAME),
            sources: vec![("src/main.rs".to_string(), self.w.into_string())],
        }
    }
}

/// Render one [`ImportSpec`] as a Rust `use ...;` statement, or
/// `None` if the spec is too degenerate to lower (empty path or empty
/// group — both parser-recovery shapes).
///
/// Pure, side-effect-free, no emitter state read or written; this is
/// why it lives as a free function next to [`cargo_toml_for`] rather
/// than as a method on `RustEmitter`.
fn render_use(spec: &ImportSpec, inside_package_mod: bool) -> Option<String> {
    // When the emitted `use` lands inside a `pub mod a::b { … }`
    // wrapper, an unqualified `use com::lib::Greeter;` would resolve
    // relative to `a::b` rather than the crate root. Prefix
    // `crate::` so the path always anchors at the root regardless
    // of how deep the surrounding module nesting is. Flat emission
    // (no package decl) keeps the bare path — that matches the
    // historical single-file behavior the existing test corpus
    // expects.
    let root = if inside_package_mod { "crate::" } else { "" };
    match spec {
        ImportSpec::Path { name, wildcard, alias } => {
            let path = render_qualified(name)?;
            let mut out = format!("use {root}{path}");
            if *wildcard {
                out.push_str("::*");
            } else if let Some(a) = alias {
                out.push_str(&format!(" as {}", a.text));
            }
            out.push(';');
            Some(out)
        }
        ImportSpec::Items { prefix, items } => {
            if items.is_empty() {
                return None;
            }
            let path = render_qualified(prefix)?;
            let body = items
                .iter()
                .map(|it| match &it.alias {
                    Some(a) => format!("{} as {}", it.name.text, a.text),
                    None => it.name.text.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!("use {root}{path}::{{{body}}};"))
        }
    }
}

/// Render a Jux dotted [`QualifiedName`] as a Rust path with `::`
/// separators. Returns `None` for the empty-segments parser-recovery
/// shape — callers skip emitting in that case.
fn render_qualified(name: &QualifiedName) -> Option<String> {
    if name.segments.is_empty() {
        return None;
    }
    Some(
        name.segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("::"),
    )
}

/// Build the `Cargo.toml` for the emitted crate. Pure-text generation; no
/// `toml` crate dependency.
///
/// The `[workspace]` table at the bottom is **required**: the emitted
/// crate ends up under `target/.rust-build/`, which on many setups is
/// inside the user's outer Cargo workspace. Without this empty workspace
/// declaration, cargo refuses to build it ("current package believes
/// it's in a workspace when it's not"). The empty table opts the emitted
/// crate out of any enclosing workspace.
pub fn cargo_toml_for(name: &str) -> String {
    format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         [[bin]]\n\
         name = \"{name}\"\n\
         path = \"src/main.rs\"\n\
         \n\
         [workspace]\n",
    )
}
