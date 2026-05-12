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
use juxc_source::Span;
use juxc_tycheck::{SymbolTable, Ty};

mod analysis;
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
pub fn lower_with_types(
    unit: &CompilationUnit,
    symbols: &SymbolTable,
    expr_types: &HashMap<Span, Ty>,
) -> RustCrate {
    let mut e = RustEmitter::new(symbols, expr_types.clone());
    e.emit_compilation_unit(unit);
    e.finish()
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
    /// Declared return type of the function / method / operator body
    /// currently being emitted. `None` outside any function body and
    /// inside constructor bodies (constructors return `Self`).
    /// Drives the return-position string-literal coercion: a `return
    /// "literal";` inside a `String`-returning fn lowers to
    /// `return "literal".to_string();` so the `&str` → owned `String`
    /// gap doesn't surface as an E0308 at rustc time.
    pub(crate) current_return_type: Option<ReturnType>,
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
        // Header comment so anyone who opens the file knows what produced it.
        w.push_str("// Auto-generated by juxc. Do not edit by hand.\n");
        w.push_str("// Source: emitted from a Jux compilation unit per §C.9 of the\n");
        w.push_str("// compiler pipeline addendum.\n\n");
        Self {
            w,
            mutated_in_fn: HashSet::new(),
            this_alias: None,
            user_mut_methods: HashSet::new(),
            emitting_lvalue: false,
            current_return_type: None,
            symbols: symbols.clone(),
            expr_types,
        }
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
        // The package declaration is still ignored — emitting one
        // generated Rust file per `package com.x.y` would require a
        // module-tree layout. Today the emitted crate is a flat single
        // binary; the package name shows up only in diagnostics.
        self.emit_imports(&unit.imports);
        for item in &unit.items {
            self.emit_top_level_decl(item);
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
    fn emit_imports(&mut self, imports: &[ImportDecl]) {
        let mut emitted_any = false;
        for import in imports {
            if let Some(line) = render_use(&import.spec) {
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
        match item {
            TopLevelDecl::Function(fn_decl) => self.emit_fn_decl(fn_decl),
            TopLevelDecl::Class(class_decl) => self.emit_class_decl(class_decl),
            TopLevelDecl::Enum(enum_decl) => self.emit_enum_decl(enum_decl),
            TopLevelDecl::Record(record_decl) => self.emit_record_decl(record_decl),
            TopLevelDecl::Interface(interface_decl) => {
                self.emit_interface_decl(interface_decl);
            }
        }
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
fn render_use(spec: &ImportSpec) -> Option<String> {
    match spec {
        ImportSpec::Path { name, wildcard, alias } => {
            let path = render_qualified(name)?;
            let mut out = format!("use {path}");
            if *wildcard {
                // Wildcard + alias was a parser shape error; if we got
                // here anyway, drop the alias and emit just the `*`
                // form. Better invalid-input recovery than emitting
                // `use foo::* as Bar;` which is not valid Rust.
                out.push_str("::*");
            } else if let Some(a) = alias {
                // `use com::example::Foo as Bar;`.
                out.push_str(&format!(" as {}", a.text));
            }
            out.push(';');
            Some(out)
        }
        ImportSpec::Items { prefix, items } => {
            if items.is_empty() {
                // Empty group is a parser-recovery shape — produce no
                // `use ::{};` since that would be invalid Rust.
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
            Some(format!("use {path}::{{{body}}};"))
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
fn cargo_toml_for(name: &str) -> String {
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
