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
    lower_workspace_with_entry(units, symbols, expr_types, sources, None)
}

/// Binary-crate lowering with an explicit entry-point package preference.
///
/// Identical to [`lower_workspace`] except the emitted `fn main()` shim
/// targets the `main` declared in `entry_package` (the package segments of a
/// `[[bin]] main = "xss.it.Main"` manifest key) rather than the first `main`
/// encountered. `None` reproduces [`lower_workspace`] exactly.
pub fn lower_workspace_with_entry(
    units: &[CompilationUnit],
    symbols: &SymbolTable,
    expr_types: &HashMap<Span, Ty>,
    sources: &[SourceFile],
    entry_package: Option<Vec<String>>,
) -> RustCrate {
    let mut e = RustEmitter::new(symbols, expr_types.clone());
    e.workspace_mode = true;
    e.entry_package = entry_package;
    // Union the per-unit `user_mut_methods` sets so the `&mut self`
    // promotion analysis sees mutating methods declared in OTHER
    // files. Without this, `var c = new Cart(); c.add(item);` in
    // `app.jux` fails to promote `c` to `let mut` because `add`'s
    // mutation flag lives in `cart.jux`'s per-unit set. The
    // per-unit reset in `emit_package_node_body` would clobber the
    // union, so workspace-mode skips that reset (see the gate
    // there).
    for unit in units {
        // Seed each unit's fixed-point with the extern `@MutSelf` set so a
        // method that calls a discovered `&mut self` extern method on a
        // `this`-rooted receiver is itself flagged (and its cross-file callers
        // promote their receiver to `let mut`).
        let unit_set = analysis::collect_user_mut_methods_seeded(unit, &e.extern_mut_methods);
        e.user_mut_methods.extend(unit_set);
    }
    // C6 pre-pass: with the full cross-file `user_mut_methods` set in
    // hand (mutation discovery folds in `@MutSelf` callees), record the
    // by-`&mut` foreign-collection params for every function/method/
    // ctor across ALL units, so a call in `app.jux` to `fill` declared
    // in `util.jux` emits `&mut <arg>` matching the `&mut T` signature.
    e.populate_byref_params(units);
    // C6: methods containing a self-aliasing byref call become
    // receiver-mutating (write-back assigns `self.field`); register
    // them so they emit `&mut self` and their callers promote the
    // receiver to `let mut`. Workspace mode keeps this set (the
    // per-unit reset is gated off there).
    e.mark_self_aliasing_mut_methods(units);
    // Build FQN → ClassDecl map so emit_class_decl can walk
    // parent chains and copy inherited concrete method bodies
    // down. The FQN is `<package>.<class>` (empty package → bare
    // name), matching how the symbol table keys class entries.
    //
    // Phase B (§CR.3.3): only wrap classes that are BOTH wrap-eligible
    // AND provably aliased — non-aliased eligible classes demote to the
    // legacy plain-struct ("Inline") shape via `compute_wrapped_set`.
    e.class_reps = compute_class_reps(units, &e.expr_types);
    // `wrapper_classes` = every newtype-handle class (bare `Rc` OR `Rc<RefCell>`)
    // — the `.0` newtype shape. `refcell_classes` is the interior-mutable subset
    // that the `.0.borrow()` / `RefCell::new` sites gate on.
    e.wrapper_classes = e
        .class_reps
        .keys()
        .cloned()
        .collect();
    e.refcell_classes = e
        .class_reps
        .iter()
        .filter(|(_, r)| **r == ClassRep::RcRefCell)
        .map(|(n, _)| n.clone())
        .collect();
    e.box_classes = e
        .class_reps
        .iter()
        .filter(|(_, r)| **r == ClassRep::Box)
        .map(|(n, _)| n.clone())
        .collect();
    // A polymorphic base must also be a WRAPPER class for `Rc<dyn …Kind>`
    // dispatch (populated Kind trait, accessors, etc.) to be sound — the
    // delegations and accessors assume the interior-mutable `self.0` shape.
    // Non-wrapper poly bases (e.g. exception classes, excluded from wrapping
    // because `Rc<RefCell>` is `!Send`) stay on their legacy value path.
    e.poly_base_classes = compute_polymorphic_base_classes(units)
        .intersection(&e.wrapper_classes)
        .cloned()
        .collect();
    e.bound_position_classes = compute_bound_position_classes(units);
    e.downcast_targets = compute_downcast_targets(units);
    for unit in units {
        // Stub units carry no real class bodies to copy down parent chains.
        if unit.is_external {
            continue;
        }
        let pkg: Vec<String> = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
            .unwrap_or_default();
        let pkg_str = pkg.join(".");
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                let fqn = if pkg_str.is_empty() {
                    cd.name.text.clone()
                } else {
                    format!("{pkg_str}.{}", cd.name.text)
                };
                e.class_asts.insert(fqn, cd.clone());
            }
        }
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
        // External `.jux.d` stub units (JUX-BINDGEN-ADDENDUM §G.9.1) contribute
        // signatures to the symbol table but have no bodies to lower — the real
        // crate provides them at link time. Skip them so the backend emits no
        // (bodyless) Rust for a stub `class`/`struct`/`fn`.
        if unit.is_external {
            continue;
        }
        let pkg: Vec<String> = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
            .unwrap_or_default();
        tree.insert(&pkg, i);
    }
    // Emit one file per Jux compilation unit: packaged units land in their own
    // `src/<pkg>/<file>.rs` (captured in `split_files`), while `main.rs` keeps
    // the prelude, no-package units, `pub mod <top>;` declarations, and the
    // `fn main` shim. (`_lib`/`_test` variants stay single-file for now.)
    e.split_files = Some(Vec::new());
    e.emit_package_tree(&tree, units, sources);
    // One crate-root `fn main()` shim that delegates into whichever
    // unit declared `void main()` inside a package.
    e.source = None;
    e.emit_workspace_main_shim(units);
    e.finish()
}

/// Library variant of [`lower_workspace`].
///
/// Lowers the units exactly as [`lower_workspace`] does (same package-tree
/// wrapping, same wrapper-class analysis) but produces a **library** crate:
/// the emitted Rust lands in `src/lib.rs` and **no** `fn main()` shim is
/// emitted (a library has no entry point). This is the Phase-1 `[lib]`
/// target — its `pub mod <pkg>` tree exposes the package's public items so
/// a dependent crate could `use <libcrate>::<pkg>::Type` against it.
///
/// Used by the project/workspace build path; loose-file compiles continue
/// to use the binary-producing [`lower_workspace`].
pub fn lower_workspace_lib(
    units: &[CompilationUnit],
    symbols: &SymbolTable,
    expr_types: &HashMap<Span, Ty>,
    sources: &[SourceFile],
) -> RustCrate {
    // Reuse the binary lowering, then re-key its single source file from
    // `src/main.rs` to `src/lib.rs`. Because a library has no `main()`,
    // `emit_workspace_main_shim` already emitted nothing, so the content
    // is exactly the package-module tree — valid as a `lib.rs`.
    let mut produced = lower_workspace(units, symbols, expr_types, sources);
    for (path, _content) in &mut produced.sources {
        if path == "src/main.rs" {
            *path = "src/lib.rs".to_string();
        }
    }
    produced
}

/// Same as [`lower_workspace`], but emits a **test-runner main**
/// instead of the regular `void main()` shim.
///
/// Discovers every top-level free function annotated with `@Test`
/// (case-insensitive — see [`feedback_annotations_case_insensitive`])
/// and emits a synthetic `fn main()` at the crate root that:
///
///   - Walks the discovered test functions in source-declaration
///     order.
///   - Runs each one inside `std::panic::catch_unwind`.
///   - Reports PASS / FAIL with the captured panic message.
///   - Exits non-zero when at least one test fails so CI can
///     detect regressions.
///
/// The user's own `void main()` is ignored in test mode — the
/// test runner is the binary's entry point. This matches Cargo's
/// `cargo test` shape where `fn main` from the bin target is
/// replaced by the test harness's main.
pub fn lower_workspace_test(
    units: &[CompilationUnit],
    symbols: &SymbolTable,
    expr_types: &HashMap<Span, Ty>,
    sources: &[SourceFile],
) -> RustCrate {
    let mut e = RustEmitter::new(symbols, expr_types.clone());
    e.workspace_mode = true;
    e.test_mode = true;
    for unit in units {
        // Seed each unit's fixed-point with the extern `@MutSelf` set so a
        // method that calls a discovered `&mut self` extern method on a
        // `this`-rooted receiver is itself flagged (and its cross-file callers
        // promote their receiver to `let mut`).
        let unit_set = analysis::collect_user_mut_methods_seeded(unit, &e.extern_mut_methods);
        e.user_mut_methods.extend(unit_set);
    }
    // C6 pre-pass (see lower_workspace_with_entry).
    e.populate_byref_params(units);
    e.mark_self_aliasing_mut_methods(units);
    // Phase B (§CR.3.3): wrap only wrap-eligible AND aliased classes;
    // non-aliased eligible classes demote to the legacy Inline shape.
    e.class_reps = compute_class_reps(units, &e.expr_types);
    // `wrapper_classes` = every newtype-handle class (bare `Rc` OR `Rc<RefCell>`)
    // — the `.0` newtype shape. `refcell_classes` is the interior-mutable subset
    // that the `.0.borrow()` / `RefCell::new` sites gate on.
    e.wrapper_classes = e
        .class_reps
        .keys()
        .cloned()
        .collect();
    e.refcell_classes = e
        .class_reps
        .iter()
        .filter(|(_, r)| **r == ClassRep::RcRefCell)
        .map(|(n, _)| n.clone())
        .collect();
    e.box_classes = e
        .class_reps
        .iter()
        .filter(|(_, r)| **r == ClassRep::Box)
        .map(|(n, _)| n.clone())
        .collect();
    // A polymorphic base must also be a WRAPPER class for `Rc<dyn …Kind>`
    // dispatch (populated Kind trait, accessors, etc.) to be sound — the
    // delegations and accessors assume the interior-mutable `self.0` shape.
    // Non-wrapper poly bases (e.g. exception classes, excluded from wrapping
    // because `Rc<RefCell>` is `!Send`) stay on their legacy value path.
    e.poly_base_classes = compute_polymorphic_base_classes(units)
        .intersection(&e.wrapper_classes)
        .cloned()
        .collect();
    e.bound_position_classes = compute_bound_position_classes(units);
    e.downcast_targets = compute_downcast_targets(units);
    for unit in units {
        // Stub units carry no real class bodies to copy down parent chains.
        if unit.is_external {
            continue;
        }
        let pkg: Vec<String> = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
            .unwrap_or_default();
        let pkg_str = pkg.join(".");
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                let fqn = if pkg_str.is_empty() {
                    cd.name.text.clone()
                } else {
                    format!("{pkg_str}.{}", cd.name.text)
                };
                e.class_asts.insert(fqn, cd.clone());
            }
        }
    }
    if sources.len() == 1 {
        let line = format!("// Source: {}\n", sources[0].path().display());
        e.w.replace_first("// Source: <jux compilation unit>\n", &line);
    }
    let mut tree = PackageNode::default();
    for (i, unit) in units.iter().enumerate() {
        // Skip external `.jux.d` stub units — they have no bodies to lower.
        if unit.is_external {
            continue;
        }
        let pkg: Vec<String> = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
            .unwrap_or_default();
        tree.insert(&pkg, i);
    }
    e.emit_package_tree(&tree, units, sources);
    e.source = None;
    e.emit_test_runner_main(units);
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

/// One `bind()` / `bindBidirectional()` call deferred out of a
/// constructor body (P6, §P.9) — see
/// `RustEmitter::pending_ctor_binds`. Receiver `None` means the
/// instance under construction (`this`); `Some(e)` is the original
/// receiver expression, replayed verbatim in the public `new` (ctor
/// params are still in scope there, cloned before the `new_inner`
/// hand-off).
pub(crate) struct PendingCtorBind {
    pub(crate) target: (Option<juxc_ast::Expr>, String, String),
    pub(crate) source: (Option<juxc_ast::Expr>, String, String),
    pub(crate) bidirectional: bool,
}

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
    /// Parameter names of the method / constructor / operator whose body is
    /// currently being emitted. A bare identifier that names a parameter (or a
    /// local, tracked via `local_types`) shadows an instance field of the same
    /// name, so the implicit-`this` rewrite (a bare instance-field reference →
    /// `this.field`, Java rule) must NOT fire for it. Set alongside
    /// `this_alias` at each body-emission site and restored afterwards.
    pub(crate) current_fn_params: std::collections::HashSet<String>,
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
    /// Names of EXTERNAL (stub) methods whose real Rust receiver is
    /// `&mut self`, DISCOVERED from the bindgen `@MutSelf` markers in
    /// the symbol table (no hardcoded method-name lists — the set
    /// tracks the library surface automatically). Computed once in
    /// [`Self::new`]; re-unioned into `user_mut_methods` after every
    /// per-unit recompute so `let mut` promotion sees stub mutators.
    extern_mut_methods: HashSet<String>,
    /// **C6 — foreign collection pass-by-`&mut`.** Maps a callee's
    /// identity key to the set of parameter indices that lower to a
    /// Rust `&mut T` (instead of by-value `mut T`) because the
    /// parameter's declared type is an EXTERNAL/foreign non-`Copy`
    /// type (a `.jux.d` stub class, `is_external`) AND the body
    /// MUTATES it (reassign / index-assign / a call to a `@MutSelf`
    /// or known-mutating method on it — all DISCOVERED, never a
    /// hardcoded type-name list). Java container-passing semantics:
    /// the callee's `v.push(x)` is visible to the caller.
    ///
    /// Single source of truth: BOTH the parameter declaration
    /// (`&mut T`) and every matching call site (`&mut <arg>`) read
    /// this same precomputed map, so the two can never diverge (the
    /// #1 borrow-checker hazard). Keys:
    /// - free function: `"fn::" + name`
    /// - method:       `"m::" + class_bare + "::" + method_name`
    /// - constructor:  `"ctor::" + class_bare`
    ///
    /// Computed once over ALL compilation units (so cross-file calls
    /// resolve) in the workspace pre-pass, and per-unit in
    /// `emit_compilation_unit` for the single-file path. Only ever
    /// extended; the predicate is deterministic, so re-running over a
    /// unit is idempotent.
    byref_params: std::collections::HashMap<String, HashSet<usize>>,
    /// **C6 runtime set.** Names of the parameters in the body currently
    /// being emitted that were lowered to `&mut T` (the per-call-site
    /// `&mut` from `byref_params`). Seeded at each body-emission site
    /// (free fn / method / ctor) and restored afterwards, mirroring the
    /// `out_params` discipline. Consulted by the body emitters to:
    /// - reborrow when the param flows onward to another `&mut` slot
    ///   (`&mut *v`),
    /// - clone out of the borrow when it is returned / stored into an
    ///   owned slot (`(*v).clone()`),
    /// - deref on reassignment (`*v = expr;`).
    byref_param_names: HashSet<String>,
    /// True while we're emitting the LHS of an assignment statement —
    /// suppresses the `.clone()` insertion in `emit_field` so we don't
    /// produce nonsense like `self.name.clone() = "x";`.
    emitting_lvalue: bool,
    /// True while we're emitting the place behind an `out` argument
    /// (§M.4) — `setIt(out b.field)`. The place is passed by exclusive
    /// reference (`&mut`), so a wrapper-class field must take the
    /// **mutable** interior borrow (`b.0.borrow_mut().field`) rather than
    /// the default read-path `b.0.borrow()`. The `RefMut` temporary lives
    /// to the end of the enclosing call statement (Rust temporary-lifetime
    /// extension), so the `&mut` into it stays valid for the callee.
    pub(crate) emitting_out_place: bool,
    /// True while emitting a mutating stdlib-collection call whose
    /// arguments were already **hoisted into temps** (§CR.4.1 / gap N1).
    /// The temps already carry the element coercion ladder (nullable
    /// `Some(…)` wrap, wrapper share-`.clone()`), so `emit_collection_arg`
    /// must emit the bare temp reference and NOT re-apply that ladder —
    /// otherwise a nullable element double-wraps to `Some(Some(v))`.
    pub(crate) collection_args_prehoisted: bool,
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
    /// Names of in-scope `ref` bindings (§M.13) — locals and params
    /// whose slot is an `Rc<RefCell<T>>` shared reference to a
    /// value-typed object. Reads clone out (`x.borrow().clone()`),
    /// assignments store through (`*x.borrow_mut() = v`), and passing
    /// to a `ref` parameter shares the handle (`x.clone()`). Reset
    /// per function alongside `nullable_locals`.
    pub(crate) ref_locals: HashSet<String>,
    /// Names of in-scope **raw-pointer** locals/params (declared type has
    /// `ptr_depth > 0`, e.g. `int* p`, `Point* ptr`). The lowered `Ty` erases
    /// `ptr_depth`, so this set is how the emitter recovers pointer-ness at a
    /// use site. Drives the `p == null` / `p != null` peephole to emit the
    /// raw-pointer `is_null()` test instead of the `Option::is_none()` form
    /// (a `*mut T` has no `is_none`). Reset per function alongside
    /// `nullable_locals`.
    pub(crate) pointer_locals: HashSet<String>,
    /// In-scope `weak` parameters (§M.14.3) → the bare name of the target
    /// class. The slot is a `Weak<RefCell<Class_Inner>>`; `param.get()` lowers
    /// to `param.upgrade().map(Class)` (→ `Option<Class>` = `Class?`), and a
    /// class argument passed to a `weak` parameter is downgraded. Reset and
    /// seeded per body alongside `ref_locals`.
    pub(crate) weak_params: std::collections::HashMap<String, String>,
    /// When set, a `ref` field read emits the HANDLE (`….x.clone()`,
    /// an aliasing share) instead of the value clone-out
    /// (`….x.borrow().clone()`) — used when the read flows into a
    /// `ref` parameter slot (§M.13 aliasing pass).
    pub(crate) emitting_ref_handle: bool,
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
    /// Preferred entry-point package for the `fn main()` shim — the package
    /// segments of a `[[bin]] main = "xss.it.Main"` manifest key (`["xss",
    /// "it"]`). When `Some`, [`Self::emit_workspace_main_shim`] selects the
    /// `main` declared in that package over any other; `None` keeps the
    /// legacy "first `main` found" behavior.
    entry_package: Option<Vec<String>>,
    /// `use` statements already emitted in the current `pub mod`
    /// block. Two units that live in the same package can carry
    /// the same `import` clause without realizing it (e.g. both
    /// `jux.std.collections.ArrayList` and `jux.std.collections.
    /// HashMap` import `UnsupportedOperationException`); the
    /// `emit_imports` skip-on-duplicate-line path keys off this
    /// set. Reset when entering / leaving a `pub mod`.
    emitted_uses_in_module: std::collections::HashSet<String>,
    /// Local variable → declared type, scoped. Mirrors tycheck's
    /// `TypeEnv` but lives at the backend layer so receiver-type
    /// lookups for `@Intrinsic` dispatch can find the right class
    /// even when `expr_types` is unreliable (spans collide across
    /// interp-string synthetic sources). Push on function/block
    /// entry, pop on exit, `declare` on each `Stmt::VarDecl`.
    local_types: Vec<std::collections::HashMap<String, juxc_tycheck::Ty>>,
    /// Owned-typed constructor parameters (String / arrays) that are
    /// still READ by a later statement of the constructor body being
    /// emitted. A `this.f = param;` assignment whose RHS is one of
    /// these must `.clone()` — the plain move would poison the later
    /// read (rustc E0382). Maintained per-statement by
    /// `emit_ctor_body_stmts`; empty outside constructor bodies.
    pub(crate) ctor_live_after: std::collections::HashSet<String>,
    /// Pending `__ovK` suffix for the method NAME of the call
    /// currently being emitted — set by `emit_call` from
    /// `SymbolTable::method_selections` (overload pick, §T.3
    /// Phase-1), consumed exactly once by whichever path writes the
    /// member name (field-position callee, bare implicit-this call,
    /// `Class::method` static). `None` for non-overloaded calls.
    pub(crate) pending_method_suffix: Option<String>,
    /// Pending `__ovK` suffix for the method DECLARATION being
    /// emitted — set by the class-decl loops (position of the decl
    /// among same-name siblings), consumed by `emit_method`'s name
    /// write.
    pub(crate) pending_decl_suffix: Option<String>,
    /// True while emitting a STATEMENT-form catch arm's body. A
    /// `return` there must run the try's `finally` first (§X.3.2), so
    /// it parks its value in `__jux_ret` and breaks the dispatch
    /// block; a `throw` parks its payload in `__jux_unhandled` the
    /// same way. Cleared inside nested try closures (their own
    /// machinery owns control flow there).
    pub(crate) in_catch_arm: bool,
    /// True while emitting an enum's `&self` method body. A
    /// `switch (this)` there matches on the borrowed receiver, so
    /// payload binders come out `&T` — the scrutinee emission clones
    /// (`self.clone()`, enums always derive Clone) so binders own
    /// their payloads and `return v;` type-checks for generic `T`.
    pub(crate) in_enum_method: bool,
    /// The bare enum name of the scrutinee for the `switch` currently being
    /// emitted, when it resolves to an enum. Lets bare `case Variant ->`
    /// patterns (which parse as `Pattern::Bind`, Java-style unqualified labels)
    /// emit the qualified `Enum::Variant` Rust pattern instead of a catch-all
    /// binding (rustc `E0170`). `None` outside an enum switch.
    current_switch_enum: Option<String>,
    /// When `true`, the emitter is producing a `jux test` binary:
    /// the workspace shim is a test runner that invokes every
    /// `@Test`-annotated function instead of the user's `main()`.
    /// Set by [`lower_workspace_test`].
    test_mode: bool,
    /// Index into `symbols.units` for the compilation unit currently
    /// being emitted. Powers import-alias-aware bare-name lookups
    /// in the backend — the unit's [`UnitContext::unqualified`]
    /// map carries `alias_name → FQN` for both bare imports and
    /// grouped `{ X as Y }` aliases. `None` outside workspace
    /// emission (legacy single-file paths don't have an `units`
    /// table to consult).
    pub(crate) current_unit_idx: Option<usize>,
    /// When `Some`, the crate is being emitted as **one file per Jux
    /// compilation unit** (the multi-file output). Each packaged unit's body
    /// and each package's `mod.rs` are captured here as `(rel-path, content)`
    /// while `self.w` accumulates only `main.rs` (prelude + no-package units +
    /// `pub mod <top>;` declarations + the `fn main` shim). `None` keeps the
    /// legacy single-`main.rs` (nested `pub mod` blocks) emission.
    pub(crate) split_files: Option<Vec<(String, String)>>,
    /// Monotonic counter for anonymous-class instances seen during
    /// emission. Each `new Iface() { … }` site mints a fresh struct
    /// name (`__JuxAnon0`, `__JuxAnon1`, …) at the use site so
    /// distinct anonymous classes don't collide.
    pub(crate) anonymous_class_counter: usize,
    /// Cloned `ClassDecl` ASTs keyed by FQN, populated upfront in
    /// `lower_workspace`. Lets `emit_class_decl` walk parent
    /// classes by FQN and copy inherited concrete method bodies
    /// down into each concrete subclass — preserving virtual
    /// dispatch (so `Entity.describe()` sees `Player::kind()`
    /// when called on a Player, rather than Entity's abstract
    /// stub via Deref).
    pub(crate) class_asts: std::collections::HashMap<String, juxc_ast::ClassDecl>,
    /// Bare names of classes lowered to the **shared-mutation wrapper
    /// shape** — `pub struct C(Rc<RefCell<C_Inner>>)` — per the
    /// class-representation addendum's Phase A (§CR.4.1 / §CR.6). A
    /// class lands here when it's "simple": no `extends`, no
    /// `sealed permits`, no generics, not abstract, and not a stdlib
    /// intrinsic. Field reads/writes on a value whose type is one of
    /// these names route through `.0.borrow()` / `.0.borrow_mut()`
    /// (see `emit_field` / `emit_assign`), and `this.f` inside such a
    /// class's methods reads via `self.0.borrow()`.
    ///
    /// Populated upfront — same place as `class_asts` — so the
    /// field-access emitters (which only know a receiver's *type
    /// name*, not its decl) can recognize a wrapper receiver. Keyed
    /// by bare class name; cross-package collisions are accepted for
    /// Phase A (the simple-class set rarely collides, and a false
    /// positive just adds a `.borrow()` that wouldn't compile, which
    /// surfaces clearly).
    pub(crate) wrapper_classes: std::collections::HashSet<String>,
    /// Per-class Rust representation (§CR.3.3 + §CR.4.1), keyed by bare class
    /// name. The source of truth for the four-way object lowering; absent ⇒
    /// legacy Inline. [`Self::wrapper_classes`] is derived from this as the
    /// `RcRefCell` slice (the interior-mutability borrow sites read that set).
    pub(crate) class_reps: std::collections::HashMap<String, ClassRep>,
    /// The `RcRefCell` slice of [`Self::class_reps`] — classes that need the
    /// interior-mutability cell. Gates `.borrow()`/`.borrow_mut()`/`RefCell::new`
    /// emission. A class in [`Self::wrapper_classes`] but NOT here is the bare
    /// `Rc<C_Inner>` rep (read-only shared): same `.0` newtype, no borrow.
    pub(crate) refcell_classes: std::collections::HashSet<String>,
    /// The `Box` slice of [`Self::class_reps`] — escaping-but-unaliased classes
    /// lowered to `C(Box<C_Inner>)`: a unique owner (heap, no refcount/cell).
    /// Box methods take `&mut self` when they mutate (inline-style), field
    /// access is direct `.0` (Box derefs), and `===` is `std::ptr::eq`.
    pub(crate) box_classes: std::collections::HashSet<String>,
    /// Bare names of **polymorphic base classes** (non-sealed, non-final,
    /// non-generic classes extended by ≥1 subclass — see
    /// [`compute_polymorphic_base_classes`]). A value slot of one of these
    /// types lowers to `Rc<dyn <Name>Kind>` for Stage-2 virtual dispatch, the
    /// `<Name>Kind` trait is populated with the base's virtual methods, and an
    /// upcast wraps instead of slicing. Empty until the Stage-2 emit paths
    /// consult it; populated alongside [`Self::wrapper_classes`].
    pub(crate) poly_base_classes: std::collections::HashSet<String>,
    /// Bare names of every **class that appears as the head of a generic
    /// bound** anywhere in the unit (`<V extends Container<? extends K>>` →
    /// `Container`). These classes get a *method-carrying* generic marker
    /// trait (`trait ContainerKind<T>: Debug { fn peek(&self) -> T; }`)
    /// instead of the empty `trait XKind: Debug {}` marker, so a bounded
    /// type param `V: ContainerKind<K>` can call the class's public instance
    /// methods (`this.backing.peek()`). Computed once via
    /// [`compute_bound_position_classes`]; consulted by
    /// `emit_class_marker_trait` and `emit_bound_type`.
    pub(crate) bound_position_classes: std::collections::HashSet<String>,
    /// Names of **`int`-typed const-generic parameters** in scope —
    /// the `N` of an enclosing `class RingBuffer<T, int N>` or
    /// `fn cap<int N>()`. A bare read of such a name in *value*
    /// position emits `(N as isize)`: the param declares as Rust
    /// `const N: usize` (fixed array sizes require exactly `usize` on
    /// stable), while Jux `int` is `isize`. Array-size position
    /// (`[T; N]`) wants the raw `usize` — suppressed there via
    /// [`Self::in_array_size_position`]. `bool` const params need no
    /// cast and aren't tracked. Set around class / function body
    /// emission; cleared after.
    pub(crate) const_int_params: std::collections::HashSet<String>,
    /// Names of the **`out` parameters** of the function/method whose body is
    /// being emitted (§M.4). They lower to Rust `&mut T`, so every read of one
    /// in the body emits `(*name)` and every assignment `name = v` emits
    /// `*name = v`. Set/restored around each body, mirroring `const_int_params`.
    pub(crate) out_params: std::collections::HashSet<String>,
    /// Names of the **ordinary type parameters** in scope (the `T` of
    /// an enclosing `class Ring<T, int N>` / `fn id<T>(…)`). Lets the
    /// array-creation emitter recognize a generic element (`new T[N]`)
    /// and lower it via `std::array::from_fn(|_| Default::default())`
    /// — the `[Default::default(); N]` repeat form would additionally
    /// require `T: Copy`, which Jux generics don't carry. Set/restored
    /// alongside [`Self::const_int_params`].
    pub(crate) current_type_params: std::collections::HashSet<String>,
    /// In-scope generic type-param **bounds**, keyed by param name — the
    /// enclosing class's params (and, while emitting a method, that method's
    /// params too). Lets `emit_generic_params_with_clone_bound` expand a bound
    /// that names another in-scope type-param (`<R extends K>`) into that
    /// param's own bounds (`R: Id + Named + Comparable<K> + …`), since Rust has
    /// no `R: K` "param-as-bound" form. Set/restored alongside
    /// [`Self::current_type_params`].
    pub(crate) type_param_bounds:
        std::collections::HashMap<String, Vec<juxc_ast::TypeRef>>,
    /// True while emitting the size expression of a fixed array
    /// (`[T; «here»]`, types.rs `ArrayShape::Fixed`). Suppresses the
    /// `(N as isize)` value-cast for const params — the size slot
    /// needs the raw `usize`.
    pub(crate) in_array_size_position: bool,
    /// Set while emitting the initializer of a DYNAMIC-array slot
    /// (`int[] a = new int[3]`, §5.6). A `new T[N]` then lowers to a
    /// heap `vec![default; N]` instead of a stack `[default; N]` — the
    /// Java-standard array allocation, and the only valid form for a
    /// runtime-sized `N`. Cleared for fixed-array (`int[N] a`) and
    /// `var` slots, which keep the stack-array shape.
    pub(crate) dynamic_array_target: bool,
    /// The FULL array shape of the current initializer's LHS slot, when
    /// it is an array type (`int[][] g = new int[3][4]` → the `int[][]`
    /// shape). A multi-dimensional `new T[a][b]` consults this per
    /// dimension to choose, INDEPENDENTLY at each level, between the
    /// fixed (`[v; N]`) and dynamic (`vec![v; N]`) lowering — so
    /// `int[3][] g = new int[3][4]` lowers the outer dim fixed and the
    /// inner dim dynamic. `None` outside an array-typed slot
    /// initializer (the new-array then keys off `dynamic_array_target`
    /// for the outer dim and treats all inner dims as fixed).
    pub(crate) target_array_shape: Option<juxc_ast::ArrayShape>,
    /// A loop label waiting to be attached — set by the
    /// `Stmt::Labeled` emission arm, consumed by the next loop
    /// emitter (`emit_while` / `emit_do_while` / `emit_for_each` /
    /// `emit_for_c`) right before its loop keyword. Indirection is
    /// needed because `for_c` lowers inside a scope block: the Rust
    /// label must sit on the inner `loop`, not the block (a block
    /// label wouldn't support `continue 'label`).
    pub(crate) pending_loop_label: Option<String>,
    /// Bare names used as **cast / type-test targets** (`(T) e`, `e => T`) in
    /// the program (see [`compute_downcast_targets`]). For each such target the
    /// relevant dyn traits emit a `__jux_as_<T>` runtime-type downcast hook;
    /// bounding to actually-used targets keeps non-downcasting programs'
    /// emitted Rust unchanged.
    pub(crate) downcast_targets: std::collections::HashSet<String>,
    /// True while emitting the body of a constructor / method /
    /// operator that belongs to a **wrapper-shape** class (one in
    /// [`Self::wrapper_classes`]). Drives the interior-mutability
    /// lowering: `this.f` reads via `self.0.borrow().f`, field writes
    /// go through the scoped `self.0.borrow_mut()` temp shape, and the
    /// constructor wraps its `C_Inner { … }` in
    /// `C(Rc::new(RefCell::new(…)))`. `false` everywhere else, so the
    /// legacy plain-struct emitters keep their original behavior.
    pub(crate) emitting_wrapper_class: bool,
    /// `true` while emitting a **value-position type** — a variable /
    /// parameter / field / return slot, as opposed to a trait-impl header
    /// (`impl Trait for C`), a generic bound (`<T: Trait>`), or a `From<>`
    /// header. Drives the interface→`Rc<dyn Trait>` rewrite in
    /// [`Self::emit_type_as_rust`]: an interface name in a value slot becomes
    /// a `Rc<dyn Trait>` trait object (so the slot can hold any implementer
    /// and dispatch dynamically), while the same name in a trait/bound
    /// position keeps its bare spelling. Defaults to `false`; value-position
    /// emitters set it via [`Self::emit_value_type_as_rust`], so any position
    /// that isn't routed simply keeps the legacy bare emission (a feature
    /// gap, never a miscompile of existing code).
    pub(crate) in_value_type_position: bool,
    /// `true` while emitting the BODY of a `try` block that contains a
    /// function-level `return`. The body lowers inside a
    /// `catch_unwind` closure, so a `return` there can't exit the
    /// enclosing fn — the Return emitter threads the value out as
    /// `Some(value)` instead, and `emit_try`'s post-`finally` step
    /// performs the real return (Java ordering: value computed →
    /// `finally` runs → return completes). Cleared inside lambda and
    /// anonymous-class bodies, whose `return`s belong to themselves.
    pub(crate) in_try_closure: bool,
    /// Loop-nesting depth at the CURRENT emission point — incremented
    /// around each loop body (`while` / `do-while` / `for-each` /
    /// C-style `for`). Used by the O2 try/loop-control threading to
    /// tell whether a `break`/`continue` binds a loop OUTSIDE the
    /// enclosing try's `catch_unwind` closure (must thread through the
    /// `__jux_loopctl` flag) or a loop INSIDE it (plain emission).
    pub(crate) loop_emit_depth: usize,
    /// Active `__jux_loopctl` threading channels, one per enclosing
    /// `try` lowering whose body/catches contain loop-escaping
    /// `break`/`continue` (O2). Innermost last. See
    /// [`stmts::TryLoopCtl`] and `RustEmitter::emit_loop_escape`.
    pub(crate) try_loopctl: Vec<stmts::TryLoopCtl>,
    /// §P observable properties: set just before `emit_method` emits a
    /// synthesized property setter (`__set_<X>`) whose property is
    /// observable. Carries `(property name, change-comparable?)` —
    /// `emit_method` takes it and brackets the body with the old/new
    /// capture and the post-body `__obs_<X>_fire` call. Comparable
    /// types fire only when `old != now`; non-comparable (user-class)
    /// types fire on every set. The third slot lists the COMPUTED
    /// properties (§P.1.5) whose getters read this property — each as
    /// `(name, change-comparable?)` — so the setter also recomputes
    /// them and fires their observers on change. The fourth slot is
    /// `true` for a STATIC property setter (P7): the bracket then
    /// reads through `Self::<X>()` and fires the class-scoped
    /// `Self::__obs_<X>_fire`, and no P2 gate wrapper is emitted
    /// (statics have no binding machinery to guard). The fifth slot
    /// is the `__parent` hop DEPTH to the property's storage slice —
    /// 0 for a class's own property, n for an INHERITED setter copy
    /// (Java semantics: a subclass setter fires the ancestor-slice
    /// observers; the P2 gate reads `__bind_<X>` through the hops).
    pub(crate) pending_setter_observer: Option<(String, bool, Vec<(String, bool)>, bool, usize)>,
    /// P6 (§P.9): `bind()` calls on a property of `this` inside a
    /// constructor body. While the body runs, `this` is the bare
    /// inner struct (`__self`) — no wrapper handle exists for the
    /// binding closures to hold weakly — so `emit_bind` records the
    /// call here (emitting a no-op at the original site) and the
    /// public `new` replays it AFTER `Self(Rc::new(RefCell::new(…)))`
    /// wraps the instance. The deferred bind still performs its
    /// initial sync, so the only observable difference from in-place
    /// execution is ordering relative to later ctor statements.
    pub(crate) pending_ctor_binds: Vec<PendingCtorBind>,
    /// §P observer-variable shapes: `observer<T>` fields/locals mapped
    /// to their lambda arity (0 = invalidation, 2 = full, 3 = full +
    /// property reference). Keyed by bare variable/field name; filled
    /// as declarations are emitted, consulted by the
    /// `.observers.attach/detach` routing to pick the storage vec.
    pub(crate) observer_shapes: std::collections::HashMap<String, usize>,
    /// `true` while emitting a class that declares `static { }` blocks
    /// (§S.4.1). Drives the first-use trigger: each constructor body and each
    /// static method body emits a `Self::__static_init();` call at its top so
    /// the (once-guarded) static initializer runs on first observable use.
    pub(crate) emitting_class_has_static_init: bool,
    /// Set for the duration of emitting a method-call **callee**
    /// (`recv.method` in `recv.method(args)`). The outermost `Field`
    /// node of such a callee names a METHOD, which lives on the wrapper
    /// newtype — not inside `C_Inner` — so its `.0.borrow()` rewrite
    /// must be suppressed even when a same-named instance field exists
    /// somewhere up the `extends` chain (e.g. a `legs` field plus a
    /// `legs()` method on `Mammal`). `emit_field` takes-and-clears this
    /// flag on entry so only the outermost field sees it; a nested
    /// field receiver (`obj.field.method()`) still borrows correctly.
    pub(crate) emitting_call_callee: bool,
    /// True while emitting the RECEIVER place of a method call
    /// (`h.item` in `h.item.set(x)`). A plain (non-wrapper-borrow)
    /// field read in this position must NOT take the auto-`.clone()`
    /// — the method needs to borrow the place itself, and a clone
    /// would silently discard any `&mut self` mutation (S7). Set by
    /// `emit_field` around its `f.object` emission when the field is
    /// a call callee; consumed (take-and-clear) at the next
    /// `emit_field` and cleared at `emit_call` entry so it never
    /// leaks into argument or unrelated sub-expressions.
    pub(crate) emitting_method_receiver: bool,
    /// True while emitting statements inside a LAMBDA body. A `try`
    /// with returns emitted there can't type its `__jux_ret` channel
    /// from `current_return_type` (that still describes the enclosing
    /// FUNCTION — the lambda's return type is inferred), so the try
    /// machinery drops the explicit annotations and lets rustc infer
    /// from the threaded `Some(v)` values (S9). Cleared on entry to a
    /// real fn body (`emit_fn_body_at`) so nested anonymous-class
    /// methods type normally.
    pub(crate) in_lambda_body: bool,
    /// Set when the lambda about to be emitted flows into a fn-typed
    /// parameter whose declared return is `void`
    /// (`assertThrows(() -> divide(10, 0))` — §TS.3): an EXPRESSION
    /// body wraps as `{ expr; }` so its value is discarded and the
    /// closure types as `()`. Take-and-cleared by the lambda
    /// emitters.
    pub(crate) lambda_void_target: bool,
}

/// True when a class declaration should lower to the shared-mutation
/// **wrapper shape** (`pub struct C(Rc<RefCell<C_Inner>>)`) rather
/// than the legacy plain-struct shape.
///
/// Phase A scope (per the class-representation addendum): only
/// **simple** classes take the wrapper path. A class is simple when
/// it has no `extends`, no `sealed permits`, no generic parameters,
/// and isn't `abstract`. Inheritance, sealed enums, and generics keep
/// the existing emission for now — those are follow-up passes. The
/// stdlib-intrinsic skip and the sealed-enum branch in
/// `emit_class_decl` run *before* this gate, so a class reaching it
/// is already known to be a normal struct-shaped declaration.
///
/// **Superseded** by [`compute_wrapper_classes`] for the actual
/// emission gate (which now also admits non-sealed `extends`
/// hierarchies). Retained as a single-class predicate for reference.
#[allow(dead_code)]
pub(crate) fn class_decl_uses_wrapper(cd: &juxc_ast::ClassDecl) -> bool {
    cd.extends.is_none()
        && cd.permits.is_empty()
        && cd.generic_params.is_empty()
        && !cd.is_abstract
}

/// True for a `@layout(c) struct` — a C-compatible **value** type (§L.1.2): a
/// `struct` carrying a `@layout` annotation with a positional `c` argument.
/// These lower to a flat `#[repr(C)]` Rust struct (not the `Rc<RefCell>`
/// handle), so they are excluded from the wrapper set. Pure AST check (runs
/// before the symbol table is built); mirrors
/// `juxc_tycheck::symbol_table::is_layout_c_annotation`.
pub(crate) fn is_layout_c_struct(cd: &juxc_ast::ClassDecl) -> bool {
    cd.is_struct
        && cd.annotations.iter().any(|a| {
            let is_layout = a
                .name
                .segments
                .last()
                .map(|s| s.text.eq_ignore_ascii_case("layout"))
                .unwrap_or(false);
            is_layout
                && a.args.iter().any(|arg| {
                    matches!(arg,
                        juxc_ast::AnnotationArg::Positional(juxc_ast::Expr::Path(qn))
                            if qn.segments.last()
                                .map(|s| s.text.eq_ignore_ascii_case("c"))
                                .unwrap_or(false))
                })
        })
}

/// Compute the **global wrapper-class set** for a workspace.
///
/// Phase A (§CR.4.1 / §CR.5.1) — two families of class take the
/// shared-mutation wrapper shape:
///
///   1. **Leaf simple classes** — no `extends`, `sealed`, generics,
///      or `abstract` (the original Phase-A set).
///   2. **Non-sealed `extends` hierarchies** — every class in a
///      connected inheritance component lowers to the wrapper shape so
///      inherited fields/methods + shared mutation work (§CR.3.5 rolls
///      the whole chain up to one representation). Abstract parents are
///      *included* here (they're still real structs in the chain),
///      unlike the leaf rule which excludes `abstract`.
///
/// A connected hierarchy is wrapped **only if every class in it** is:
///   - non-sealed and not a subclass-of-sealed (sealed parents lower
///     as Rust enums; their subclasses are variants),
///   - non-generic (generic-class lowering is out of scope),
///   - not an intrinsic stdlib class (those suppress struct emission),
///   - **not an exception type** — any class whose `extends` chain
///     reaches `Throwable`. Exceptions are thrown via `panic_any`,
///     which needs `Send`, and `Rc<RefCell<_>>` is `!Send`. To keep
///     correctness over completeness, the whole `Throwable` chain stays
///     on the legacy plain-struct path for now (the safe fallback the
///     phasing notes call out).
///
/// When any class in a component fails a check, the *entire* component
/// stays on the legacy path — a wrapper parent with a plain-struct
/// child (or vice-versa) would break `__parent` embedding and upcasts.
pub(crate) fn compute_wrapper_classes(
    units: &[juxc_ast::CompilationUnit],
) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};

    // Bare-name → (ClassDecl, package) for every class across the
    // workspace (including stdlib units). Bare names are the join key
    // because `extends` clauses in source carry only the bare name.
    //
    // **Bare-name multimap.** The emit-time wrapper gate keys on the
    // class's bare name (`wrapper_classes.contains(name)`), and a
    // program can legally declare the same bare name in two packages
    // (e.g. a user `IOException` in the default package alongside the
    // stdlib `jux.std.exceptions.IOException`). We therefore decide
    // wrappability **per bare name**, conservatively: a bare name is
    // wrappable only if *every* declaration sharing it is wrappable.
    // This also makes a user class that collides with an exception name
    // fall back to legacy — the safe choice, since the user's class may
    // itself be thrown (`!Send` `Rc<RefCell>` would break `panic_any`).
    let mut by_name: HashMap<String, Vec<(&juxc_ast::ClassDecl, String)>> = HashMap::new();
    for unit in units {
        let pkg: Vec<String> = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
            .unwrap_or_default();
        let pkg_str = pkg.join(".");
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                by_name
                    .entry(cd.name.text.clone())
                    .or_default()
                    .push((cd, pkg_str.clone()));
            }
        }
    }

    // The set of bare names extended by *some* declaration of `name`.
    let parents_of = |name: &str| -> Vec<String> {
        by_name
            .get(name)
            .map(|decls| {
                decls
                    .iter()
                    .filter_map(|(cd, _)| {
                        cd.extends
                            .as_ref()
                            .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    // **Thrown/caught class names.** A class that is `throw`n (or named
    // in a `catch`) is panicked through `std::panic::panic_any`, which
    // requires the payload to be `Send`. `Rc<RefCell<_>>` (the wrapper
    // rep) is `!Send`, so any thrown class — even one that does NOT
    // extend `Throwable` — must stay on the legacy plain-struct path.
    // We scan every function/method/constructor body for `throw new
    // X(...)` targets and `catch (X e)` types. This is the safe
    // fallback the phasing notes call out (correctness over
    // completeness): a thrown wrapper would fail to compile.
    let thrown = collect_thrown_class_names(units);

    // Exception detection: a bare name is exception-tainted iff *any*
    // declaration sharing it transitively extends `Throwable` (or is
    // `Throwable` itself), OR the name is thrown/caught anywhere. Walks
    // the union of every declaration's parents so a collision with the
    // stdlib exception chain taints the name.
    let is_exception = |start: &str| -> bool {
        let mut stack = vec![start.to_string()];
        let mut seen: HashSet<String> = HashSet::new();
        while let Some(name) = stack.pop() {
            if name == "Throwable" || thrown.contains(&name) {
                return true;
            }
            if !seen.insert(name.clone()) {
                continue;
            }
            for p in parents_of(&name) {
                stack.push(p);
            }
        }
        false
    };

    // True when *every* declaration sharing the bare `name` is
    // wrappable in isolation: non-sealed, non-generic, non-intrinsic,
    // and not a subclass-of-sealed. A name with no visible declaration
    // (an `extends` target we can't see) is NOT ok — the chain falls
    // back to legacy.
    let name_ok = |name: &str| -> bool {
        let Some(decls) = by_name.get(name) else {
            return false;
        };
        decls.iter().all(|(cd, pkg)| {
            if cd.is_sealed || !cd.permits.is_empty() {
                return false;
            }
            // A `@layout(c) struct` is a C-compatible VALUE type (§L.1.2): it
            // lowers to a flat `#[repr(C)]` struct, never the `Rc<RefCell>`
            // handle. Keep it off the wrapper path (it then flows through the
            // plain-struct emission with by-value construction + direct fields).
            if is_layout_c_struct(cd) {
                return false;
            }
            // Generic classes ARE wrappable now (Phase A GENERICS pass):
            // the generic params + their `T: Clone` bound thread onto the
            // `C_Inner<T>` struct, the `C<T>` newtype, and every `impl<T:
            // Clone>` block (see `emit_wrapper_class_decl`). We no longer
            // exclude on a non-empty `generic_params` list — only sealed,
            // intrinsic, subclass-of-sealed, and exception/Throwable-chain
            // classes stay on the legacy path.
            if is_intrinsic_class(pkg, &cd.name.text) {
                return false;
            }
            // Subclass-of-sealed: parent is a sealed class → enum
            // variant, not an embedded struct.
            if let Some(parent) = cd
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
            {
                let parent_sealed = by_name
                    .get(&parent)
                    .map(|ds| ds.iter().any(|(pcd, _)| pcd.is_sealed))
                    .unwrap_or(true); // unseen parent → treat as unsafe
                if parent_sealed {
                    return false;
                }
            }
            true
        })
    };

    // A bare name is a wrapper iff it AND its whole transitive `extends`
    // closure (ancestors) are wrappable and exception-free. We don't
    // need to walk *descendants*: a child whose parent is excluded gets
    // excluded on its own ancestor walk, and a parent stays wrappable
    // independently (the parent's own struct shape doesn't depend on
    // its children — only the child embeds the parent's inner). This is
    // a slight relaxation of the strict whole-component roll-up, but is
    // sound here because excluded children simply fall back to legacy
    // plain-struct + `From`/`Deref`, which still upcast into a wrapper
    // parent through the parent's own `From`. To stay fully consistent
    // with §CR.3.5 we additionally require every *descendant* to be
    // wrappable too, so a mixed hierarchy never arises.
    //
    // Build child→parents adjacency for the descendant check.
    let mut children_of: HashMap<String, Vec<String>> = HashMap::new();
    for (name, decls) in &by_name {
        for (cd, _) in decls {
            if let Some(parent) = cd
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
            {
                children_of.entry(parent).or_default().push(name.clone());
            }
        }
    }

    // Collect the full connected component (ancestors + descendants)
    // reachable from `start` through the extends graph (both
    // directions), then require every member to be `name_ok` and
    // exception-free.
    let component_wrappable = |start: &str| -> bool {
        let mut stack = vec![start.to_string()];
        let mut seen: HashSet<String> = HashSet::new();
        while let Some(name) = stack.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if !name_ok(&name) || is_exception(&name) {
                return false;
            }
            for p in parents_of(&name) {
                stack.push(p);
            }
            if let Some(kids) = children_of.get(&name) {
                for k in kids {
                    stack.push(k.clone());
                }
            }
        }
        true
    };

    let mut wrapper: HashSet<String> = HashSet::new();
    for name in by_name.keys() {
        if component_wrappable(name) {
            wrapper.insert(name.clone());
        }
    }
    wrapper
}

/// Phase B — the **escape-analysis "fast tier"** selector
/// (`Architecture/JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.3.2 /
/// §CR.3.3 / §CR.4.1).
///
/// Computes the conservative `aliased` property per class over the
/// WHOLE workspace and returns the **bare names** of every class that
/// must STAY wrapped (`Rc<RefCell<…>>`). A class NOT in the returned
/// set has been proven never aliased/escaping and may be demoted to the
/// legacy plain-struct ("Inline") shape — the caller intersects this
/// set with [`compute_wrapper_classes`] so only classes that are BOTH
/// wrap-eligible AND aliased keep the wrapper.
///
/// ## The invariant (correctness over completeness)
///
/// A class MUST stay wrapped if any instance could ever be observed as
/// shared. We only demote when we can PROVE the class is never
/// aliased/stored/passed/returned anywhere in the program. **When in
/// doubt we keep it wrapped** — a wrong demotion silently turns Java
/// shared-mutation into copy semantics (a silent correctness bug),
/// which is far worse than a missed optimization.
///
/// ## What marks a class `aliased`
///
/// An instance of class `C` (identified by its tycheck type via
/// `expr_types`) is aliased when, ANYWHERE in the program, it is:
///
/// 1. **bound to a new name from an existing place** — `var y = <expr>`
///    (or `C y = <expr>`) where `<expr>` is NOT a fresh `new C(...)`
///    (a Path / field / index / call-result / `this` of type `C`) →
///    a second live reference to the same object.
/// 2. **stored into a heap-rooted slot** — assigned to a field of
///    another object (`obj.f = c`), pushed into an array literal, or
///    assigned to a static. (Collection `add`/`put` is an ordinary
///    method call and is therefore covered by rule 3.)
/// 3. **passed as an ARGUMENT** to any function / method / constructor
///    (`foo(c)`, `obj.m(c)`, `new X(c)`) — the callee may retain it.
///    A method CALL *on* the instance (`c.method()`) is a borrow, not a
///    pass, and does NOT alias — only `c`-as-argument counts.
/// 4. **returned from a function** (`return <expr>` whose value has type
///    `C`) — the instance escapes its introducing scope. Inline can't
///    escape, so any escape keeps the class wrapped.
///
/// Additionally, a class is forced aliased (kept wrapped) when it is in
/// an `extends` relationship with an aliased class: per §CR.3.5 a whole
/// connected inheritance component rolls up to one representation, so if
/// ANY member of the component is aliased the entire component stays
/// wrapped.
///
/// Generic classes are handled conservatively by the same rules — any
/// flow of a `C<…>` instance through 1–4 marks the bare name `C`.
///
/// ## Keying
///
/// Results are keyed by **bare name** (the last `.`-segment of the
/// tycheck type's name), matching the scheme [`compute_wrapper_classes`]
/// and the emit-time `wrapper_classes.contains(name)` gate use.
pub(crate) fn compute_aliased_classes(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
) -> HashSet<String> {
    use juxc_ast::{Expr, Stmt};

    // The accumulating set of bare class names proven to be aliased
    // (and therefore kept wrapped). Unioned across every body.
    let mut aliased: HashSet<String> = HashSet::new();

    // Bare class name of an expression's tycheck type, if it is a user
    // class. `Ty::User.name` may be a bare name OR an FQN
    // (`jux.std.collections.ArrayList`); we take the last `.`-segment so
    // the key matches `compute_wrapper_classes`'s bare-name scheme.
    // Nullable / array wrappers around a user type also count — a
    // `C?` or `C[]` element binding still aliases the underlying `C`.
    fn class_name_of_ty(ty: &Ty) -> Option<String> {
        match ty {
            Ty::User { name, .. } => {
                Some(name.rsplit('.').next().unwrap_or(name).to_string())
            }
            Ty::Nullable(inner) => class_name_of_ty(inner),
            _ => None,
        }
    }

    // The bare class name an expression evaluates to, when that type is
    // a user class. Looks the expression's span up in `expr_types`.
    let class_of_expr = |e: &Expr| -> Option<String> {
        expr_types
            .get(&exprs::expr_span_of(e))
            .and_then(class_name_of_ty)
    };

    // True when `e` is a *fresh* allocation — `new C(...)`. Binding a
    // fresh value to a name is NOT aliasing (it's the unique owner);
    // every other place-shaped RHS is.
    fn is_fresh_new(e: &Expr) -> bool {
        matches!(e, Expr::NewObject(_))
    }

    // Mark the class an expression evaluates to (if any) as aliased.
    let mut mark = |e: &Expr, aliased: &mut HashSet<String>| {
        if let Some(c) = class_of_expr(e) {
            aliased.insert(c);
        }
    };

    // Walk an expression for ARGUMENT-passing (rule 3): every call,
    // method-call, constructor, and array-literal element that carries a
    // place of class type aliases that class. The receiver of a method
    // call is a borrow, NOT a pass — so for `recv.m(args)` (a `Call`
    // whose callee is a `Field`) we DON'T mark the receiver, only the
    // args. We still recurse into the receiver expression to catch
    // nested calls (`a.m(x).n(y)`).
    fn walk_expr(
        e: &Expr,
        aliased: &mut HashSet<String>,
        mark: &mut dyn FnMut(&Expr, &mut HashSet<String>),
    ) {
        match e {
            // `typeof` never evaluates its operand — nothing aliases.
            Expr::TypeOf(..) => {}
            // `out <place>` — passed by `&mut`, not aliased into a value. Recurse
            // into the place to catch nested calls.
            Expr::Out(inner, _) => walk_expr(inner, aliased, mark),
            // Tuple literal — each element is a by-value capture into
            // the tuple, same aliasing consequence as a call argument.
            Expr::TupleLit(elems, _) => {
                for el in elems {
                    mark(el, aliased);
                    walk_expr(el, aliased, mark);
                }
            }
            Expr::ErrorProp(inner, _) => walk_expr(inner, aliased, mark),
            Expr::TryExpr(t) => {
                walk_block(&t.body, aliased, mark);
                for c in &t.catches {
                    walk_block(&c.body, aliased, mark);
                }
            }
            Expr::Call(c) => {
                // Each argument is passed by value → the callee may
                // retain it → alias the arg's class (fresh or not; a
                // freshly-`new`'d arg can still be stored by the callee).
                for a in &c.args {
                    mark(a, aliased);
                    walk_expr(a, aliased, mark);
                }
                // Recurse into the callee, but a method-call receiver
                // (`recv.m`) is a borrow, not an alias — so we descend
                // without marking the receiver itself. `expr_span_of`
                // marking only happens through `mark` at arg/store/return
                // sites, never on a bare callee, so plain recursion here
                // is already borrow-safe.
                walk_expr(&c.callee, aliased, mark);
            }
            Expr::NewObject(n) => {
                for a in &n.args {
                    mark(a, aliased);
                    walk_expr(a, aliased, mark);
                }
            }
            Expr::NewArrayLit(n) => {
                // Array-literal elements live in a heap-rooted slot
                // (rule 2) — every class-typed element aliases.
                for el in &n.elements {
                    mark(el, aliased);
                    walk_expr(el, aliased, mark);
                }
            }
            Expr::NewArray(n) => {
                walk_expr(&n.size, aliased, mark);
                for inner in &n.inner_sizes {
                    walk_expr(inner, aliased, mark);
                }
            }
            Expr::Binary(b) => {
                walk_expr(&b.left, aliased, mark);
                walk_expr(&b.right, aliased, mark);
            }
            Expr::Unary(u) => walk_expr(&u.operand, aliased, mark),
            Expr::Range(r) => {
                walk_expr(&r.start, aliased, mark);
                walk_expr(&r.end, aliased, mark);
            }
            Expr::Cast(c) => walk_expr(&c.value, aliased, mark),
            Expr::TypeTest(t) => walk_expr(&t.value, aliased, mark),
            Expr::SizeOf(s) => walk_expr(&s.operand, aliased, mark),
            Expr::Index(i) => {
                walk_expr(&i.array, aliased, mark);
                walk_expr(&i.index, aliased, mark);
            }
            Expr::Field(f) => walk_expr(&f.object, aliased, mark),
            // `++place` / `place++` — walk the place for nested calls.
            Expr::IncDec(i) => walk_expr(&i.target, aliased, mark),
            Expr::InterpString(s) => {
                for seg in &s.segments {
                    if let juxc_ast::InterpSegment::Expr(inner) = seg {
                        walk_expr(inner, aliased, mark);
                    }
                }
            }
            Expr::Elvis(el) => {
                walk_expr(&el.value, aliased, mark);
                walk_expr(&el.fallback, aliased, mark);
            }
            Expr::Ternary(t) => {
                walk_expr(&t.condition, aliased, mark);
                walk_expr(&t.then_branch, aliased, mark);
                walk_expr(&t.else_branch, aliased, mark);
            }
            Expr::Await(inner, _) => walk_expr(inner, aliased, mark),
            Expr::NotNullAssert(inner, _) => walk_expr(inner, aliased, mark),
            Expr::Lambda(l) => {
                // A class captured by a lambda IS an alias: the closure
                // holds its own handle to the object, mutations through
                // either handle must be visible through the other, and
                // the closure may outlive the frame. A non-wrapper
                // capture would `move` the struct by value (silently
                // forking state) and make the closure `FnMut` — which
                // the `Rc<closure>` storage can't even call (rustc
                // E0596). So: mark EVERY class-typed bare name read
                // inside the body as aliased — that's the (superset of
                // the) capture set; lambda params are swept in too,
                // which only over-wraps (the doctrine is "when in
                // doubt, wrap").
                match &l.body {
                    juxc_ast::LambdaBody::Expr(b) => {
                        mark_lambda_captures(b, aliased, mark);
                        walk_expr(b, aliased, mark);
                    }
                    juxc_ast::LambdaBody::Block(blk) => {
                        mark_lambda_captures_block(blk, aliased, mark);
                        walk_block(blk, aliased, mark);
                    }
                }
            }
            Expr::Switch(s) => {
                walk_expr(&s.scrutinee, aliased, mark);
                for arm in &s.arms {
                    if let Some(g) = &arm.guard {
                        walk_expr(g, aliased, mark);
                    }
                    match &arm.body {
                        juxc_ast::SwitchBody::Expr(b) => walk_expr(b, aliased, mark),
                        juxc_ast::SwitchBody::Block(blk) => {
                            walk_block(blk, aliased, mark)
                        }
                    }
                }
            }
            // Leaves — no sub-expressions that can carry a place-pass.
            Expr::Literal(_)
            | Expr::Path(_)
            | Expr::This(_)
            | Expr::Super(_)
            | Expr::MethodRef(_) => {}
        }
    }

    fn walk_block(
        b: &juxc_ast::Block,
        aliased: &mut HashSet<String>,
        mark: &mut dyn FnMut(&Expr, &mut HashSet<String>),
    ) {
        for s in &b.statements {
            walk_stmt(s, aliased, mark);
        }
    }

    fn walk_else(
        branch: Option<&juxc_ast::ElseBranch>,
        aliased: &mut HashSet<String>,
        mark: &mut dyn FnMut(&Expr, &mut HashSet<String>),
    ) {
        match branch {
            Some(juxc_ast::ElseBranch::If(inner)) => {
                walk_expr(&inner.condition, aliased, mark);
                walk_block(&inner.then_block, aliased, mark);
                walk_else(inner.else_branch.as_deref(), aliased, mark);
            }
            Some(juxc_ast::ElseBranch::Block(b)) => walk_block(b, aliased, mark),
            None => {}
        }
    }

    fn walk_stmt(
        s: &Stmt,
        aliased: &mut HashSet<String>,
        mark: &mut dyn FnMut(&Expr, &mut HashSet<String>),
    ) {
        match s {
            Stmt::Expr(e) => walk_expr(e, aliased, mark),
            Stmt::Return(opt, _) => {
                if let Some(e) = opt {
                    // A class-typed return ESCAPES the function but is not, by
                    // itself, ALIASED — a returned-but-otherwise-unaliased class
                    // is the `Box` rep (unique owner, heap, stable identity), not
                    // `Rc`. Escape is collected separately by
                    // [`compute_escaping_classes`]; here we only recurse so
                    // nested calls/aliases inside the returned expression still
                    // mark (rules 1-3).
                    walk_expr(e, aliased, mark);
                }
            }
            Stmt::VarDecl(v) => {
                if let Some(init) = &v.init {
                    // Rule 1: binding a *place* of class type to a new
                    // name creates a second live reference. A fresh
                    // `new C(...)` RHS is the unique owner and does NOT
                    // alias.
                    if !is_fresh_new(init) {
                        mark(init, aliased);
                    }
                    walk_expr(init, aliased, mark);
                }
            }
            Stmt::Assign(a) => {
                // Rule 2: assigning a class-typed value into a heap-rooted
                // slot aliases. A field target (`obj.f = c`) or a static
                // /path target both root the value where it can be read
                // back later. We conservatively mark the RHS class for
                // ANY assignment target that is a Field or Index (heap
                // slot) — and also for a bare Path target, since a Path
                // could name a static field or an outer-scope binding
                // (cheap to be conservative; a local-to-local reassign is
                // already an alias anyway).
                mark(&a.value, aliased);
                walk_expr(&a.value, aliased, mark);
                // Walk the target for nested calls/indexes too.
                walk_expr(&a.target, aliased, mark);
            }
            Stmt::Throw(e, _) => walk_expr(e, aliased, mark),
            Stmt::SuperCall(args, _) => {
                // `super(args)` passes each arg to the parent constructor
                // (rule 3).
                for a in args {
                    mark(a, aliased);
                    walk_expr(a, aliased, mark);
                }
            }
            Stmt::If(i) => {
                walk_expr(&i.condition, aliased, mark);
                walk_block(&i.then_block, aliased, mark);
                walk_else(i.else_branch.as_deref(), aliased, mark);
            }
            Stmt::While(w) => {
                walk_expr(&w.condition, aliased, mark);
                walk_block(&w.body, aliased, mark);
            }
            Stmt::DoWhile(d) => {
                walk_block(&d.body, aliased, mark);
                walk_expr(&d.condition, aliased, mark);
            }
            Stmt::ForEach(f) => {
                walk_expr(&f.iter, aliased, mark);
                walk_block(&f.body, aliased, mark);
            }
            Stmt::ForC(f) => {
                if let Some(cond) = &f.cond {
                    walk_expr(cond, aliased, mark);
                }
                walk_block(&f.body, aliased, mark);
            }
            Stmt::Try(t) => {
                walk_block(&t.body, aliased, mark);
                for c in &t.catches {
                    walk_block(&c.body, aliased, mark);
                }
                if let Some(fin) = &t.finally {
                    walk_block(fin, aliased, mark);
                }
            }
            Stmt::Unsafe(b) => walk_block(b, aliased, mark),
            Stmt::Break(..) | Stmt::Continue(..) => {}
            Stmt::Labeled { stmt, .. } => walk_stmt(stmt, aliased, mark),
        }
    }

    /// Mark every class-typed **bare name** read anywhere inside a
    /// lambda body as aliased — a (superset of the) closure's capture
    /// set. A captured object IS an alias: the closure holds its own
    /// handle, mutation through either handle must be visible through
    /// the other, and the closure may outlive the frame. Without the
    /// wrapper, the lambda would `move` the struct by value (silently
    /// forking state) and a mutating body would make the closure
    /// `FnMut` — which the `Rc<closure>` storage can't call (rustc
    /// E0596). Lambda params and plain locals declared inside the body
    /// are swept up too, which only over-wraps their classes ("when in
    /// doubt, wrap").
    fn mark_lambda_captures(
        e: &Expr,
        aliased: &mut HashSet<String>,
        mark: &mut dyn FnMut(&Expr, &mut HashSet<String>),
    ) {
        match e {
            Expr::Path(qn) => {
                if qn.segments.len() == 1 {
                    mark(e, aliased);
                }
            }
            Expr::Call(c) => {
                mark_lambda_captures(&c.callee, aliased, mark);
                for a in &c.args {
                    mark_lambda_captures(a, aliased, mark);
                }
            }
            Expr::NewObject(n) => {
                for a in &n.args {
                    mark_lambda_captures(a, aliased, mark);
                }
            }
            Expr::NewArrayLit(n) => {
                for el in &n.elements {
                    mark_lambda_captures(el, aliased, mark);
                }
            }
            Expr::NewArray(n) => {
                mark_lambda_captures(&n.size, aliased, mark);
                for inner in &n.inner_sizes {
                    mark_lambda_captures(inner, aliased, mark);
                }
            }
            Expr::Binary(b) => {
                mark_lambda_captures(&b.left, aliased, mark);
                mark_lambda_captures(&b.right, aliased, mark);
            }
            Expr::Unary(u) => mark_lambda_captures(&u.operand, aliased, mark),
            Expr::Range(r) => {
                mark_lambda_captures(&r.start, aliased, mark);
                mark_lambda_captures(&r.end, aliased, mark);
            }
            Expr::Cast(c) => mark_lambda_captures(&c.value, aliased, mark),
            Expr::TypeTest(t) => mark_lambda_captures(&t.value, aliased, mark),
            Expr::Index(i) => {
                mark_lambda_captures(&i.array, aliased, mark);
                mark_lambda_captures(&i.index, aliased, mark);
            }
            Expr::Field(f) => mark_lambda_captures(&f.object, aliased, mark),
            Expr::InterpString(s) => {
                for seg in &s.segments {
                    if let juxc_ast::InterpSegment::Expr(inner) = seg {
                        mark_lambda_captures(inner, aliased, mark);
                    }
                }
            }
            Expr::Elvis(el) => {
                mark_lambda_captures(&el.value, aliased, mark);
                mark_lambda_captures(&el.fallback, aliased, mark);
            }
            Expr::Ternary(t) => {
                mark_lambda_captures(&t.condition, aliased, mark);
                mark_lambda_captures(&t.then_branch, aliased, mark);
                mark_lambda_captures(&t.else_branch, aliased, mark);
            }
            Expr::Await(inner, _) => mark_lambda_captures(inner, aliased, mark),
            Expr::NotNullAssert(inner, _) => mark_lambda_captures(inner, aliased, mark),
            Expr::Lambda(inner) => match &inner.body {
                juxc_ast::LambdaBody::Expr(b) => mark_lambda_captures(b, aliased, mark),
                juxc_ast::LambdaBody::Block(blk) => {
                    mark_lambda_captures_block(blk, aliased, mark)
                }
            },
            _ => {}
        }
    }

    /// Statement-level driver for [`mark_lambda_captures`] — sweeps the
    /// expressions of every statement in a lambda's block body.
    fn mark_lambda_captures_block(
        b: &juxc_ast::Block,
        aliased: &mut HashSet<String>,
        mark: &mut dyn FnMut(&Expr, &mut HashSet<String>),
    ) {
        for s in &b.statements {
            match s {
                Stmt::Expr(e) => mark_lambda_captures(e, aliased, mark),
                Stmt::Return(Some(e), _) => mark_lambda_captures(e, aliased, mark),
                Stmt::Return(None, _) => {}
                Stmt::VarDecl(v) => {
                    if let Some(init) = &v.init {
                        mark_lambda_captures(init, aliased, mark);
                    }
                }
                Stmt::Assign(a) => {
                    mark_lambda_captures(&a.target, aliased, mark);
                    mark_lambda_captures(&a.value, aliased, mark);
                }
                Stmt::Throw(e, _) => mark_lambda_captures(e, aliased, mark),
                Stmt::SuperCall(args, _) => {
                    for a in args {
                        mark_lambda_captures(a, aliased, mark);
                    }
                }
                Stmt::If(i) => {
                    mark_lambda_captures(&i.condition, aliased, mark);
                    mark_lambda_captures_block(&i.then_block, aliased, mark);
                    let mut cursor = i.else_branch.as_deref();
                    while let Some(branch) = cursor {
                        match branch {
                            juxc_ast::ElseBranch::If(inner) => {
                                mark_lambda_captures(&inner.condition, aliased, mark);
                                mark_lambda_captures_block(&inner.then_block, aliased, mark);
                                cursor = inner.else_branch.as_deref();
                            }
                            juxc_ast::ElseBranch::Block(blk) => {
                                mark_lambda_captures_block(blk, aliased, mark);
                                cursor = None;
                            }
                        }
                    }
                }
                Stmt::While(w) => {
                    mark_lambda_captures(&w.condition, aliased, mark);
                    mark_lambda_captures_block(&w.body, aliased, mark);
                }
                Stmt::DoWhile(d) => {
                    mark_lambda_captures_block(&d.body, aliased, mark);
                    mark_lambda_captures(&d.condition, aliased, mark);
                }
                Stmt::ForEach(f) => {
                    mark_lambda_captures(&f.iter, aliased, mark);
                    mark_lambda_captures_block(&f.body, aliased, mark);
                }
                Stmt::ForC(f) => {
                    if let Some(cond) = &f.cond {
                        mark_lambda_captures(cond, aliased, mark);
                    }
                    mark_lambda_captures_block(&f.body, aliased, mark);
                }
                Stmt::Try(t) => {
                    mark_lambda_captures_block(&t.body, aliased, mark);
                    for c in &t.catches {
                        mark_lambda_captures_block(&c.body, aliased, mark);
                    }
                    if let Some(fin) = &t.finally {
                        mark_lambda_captures_block(fin, aliased, mark);
                    }
                }
                Stmt::Unsafe(b) => mark_lambda_captures_block(b, aliased, mark),
                Stmt::Break(..) | Stmt::Continue(..) => {}
                Stmt::Labeled { stmt, .. } => mark_lambda_captures_block(
                    &juxc_ast::Block { statements: vec![(**stmt).clone()], span: Span::DUMMY },
                    aliased,
                    mark,
                ),
            }
        }
    }

    // Drive the walk over every body in the workspace: free functions,
    // class methods, constructors, and operator overloads.
    for unit in units {
        for item in &unit.items {
            match item {
                juxc_ast::TopLevelDecl::Function(f) => {
                    if let Some(body) = &f.body {
                        walk_block(body, &mut aliased, &mut mark);
                    }
                }
                juxc_ast::TopLevelDecl::Class(cd) => {
                    for m in &cd.methods {
                        if let Some(body) = &m.body {
                            walk_block(body, &mut aliased, &mut mark);
                        }
                    }
                    for ctor in &cd.constructors {
                        walk_block(&ctor.body, &mut aliased, &mut mark);
                    }
                    for op in &cd.operators {
                        if let Some(body) = &op.body {
                            walk_block(body, &mut aliased, &mut mark);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Conservative generic-inheritance guard (§CR.3.4 — "conservative
    // when in doubt" on generics; §CR.3.5 roll-up). A class that
    // **extends a generic parent** (`class IntBox extends Container<int>`)
    // is forced aliased so the whole hierarchy stays on the wrapper
    // shape. The legacy Inline `From`-upcast emitter doesn't thread the
    // parent's generic argument onto the `impl From<Child> for Parent`
    // header (it emits `impl From<Label> for Container` instead of
    // `Container<String>`), so demoting a generic-base hierarchy to
    // Inline would emit ill-typed Rust. The wrapper path handles the
    // generic upcast correctly, so we keep these wrapped — a missed
    // optimization, never a correctness bug. (A plain generic *leaf*
    // class like `Box<T>` with no children is unaffected and still
    // demotes when never aliased.)
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                let extends_generic = cd
                    .extends
                    .as_ref()
                    .map(|t| !t.generic_args.is_empty())
                    .unwrap_or(false);
                if extends_generic {
                    aliased.insert(cd.name.text.clone());
                    // Also keep the named generic parent wrapped — the
                    // roll-up below propagates through the component, but
                    // seeding the parent directly handles the case where
                    // the parent is only referenced through this child.
                    if let Some(parent) = cd
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
                    {
                        aliased.insert(parent);
                    }
                }
            }
        }
    }

    // §P observable properties: a class declaring at least one
    // writable `{ get; set; }` property stays wrapped — the observer
    // storage fields, the attach/detach/clear/size helpers, and the
    // setter fire epilogue are all emitted on the wrapper shape only.
    // Demoting such a class to Inline would silently lose its
    // observability (and break `.observers` call sites, which route to
    // the wrapper helper methods).
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                let has_observable = cd
                    .properties
                    .iter()
                    .any(|p| !p.is_static && p.getter.is_some() && p.setter.is_some());
                if has_observable {
                    aliased.insert(cd.name.text.clone());
                }
            }
        }
    }

    // §CR.3.5 inheritance roll-up: a connected `extends` component shares
    // one representation. If ANY class in a component is aliased, the
    // whole component stays wrapped. Build the bidirectional extends
    // adjacency (bare names) and flood `aliased` across it to a
    // fixed point.
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                if let Some(parent) = cd
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
                {
                    let child = cd.name.text.clone();
                    adj.entry(child.clone()).or_default().push(parent.clone());
                    adj.entry(parent).or_default().push(child);
                }
            }
        }
    }
    // Flood-fill: starting from every currently-aliased class, mark every
    // class reachable through the extends graph as aliased too.
    let seeds: Vec<String> = aliased.iter().cloned().collect();
    let mut stack = seeds;
    while let Some(name) = stack.pop() {
        if let Some(neighbors) = adj.get(&name) {
            for n in neighbors {
                if aliased.insert(n.clone()) {
                    stack.push(n.clone());
                }
            }
        }
    }

    aliased
}

/// Intersect the wrap-eligible set with the aliased set: a class is
/// emitted with the `Rc<RefCell>` wrapper **only** when it is both
/// wrap-eligible ([`compute_wrapper_classes`]) AND provably aliased
/// ([`compute_aliased_classes`]). Wrap-eligible-but-non-aliased classes
/// fall through to the legacy plain-struct ("Inline") emission — the
/// Phase B "fast tier" demotion (§CR.3.3).
///
/// Superseded by [`compute_class_reps`] (the four-way selector); retained for
/// reference and the binary `wrapped`/`inline` view some tests may want.
#[allow(dead_code)]
pub(crate) fn compute_wrapped_set(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
) -> HashSet<String> {
    let eligible = compute_wrapper_classes(units);
    let aliased = compute_aliased_classes(units, expr_types);
    let iface_forced = compute_interface_forced_classes(units);
    let poly_forced = compute_polymorphic_forced_classes(units);
    let recursive = compute_recursive_field_classes(units);
    let weak_forced = compute_weak_forced_classes(units);
    // (eligible ∩ aliased) ∪ (eligible ∩ interface-implementer-closure)
    //                     ∪ (eligible ∩ polymorphic-base-hierarchy-closure)
    //                     ∪ (eligible ∩ recursive-field-cycle).
    // Forcing interface implementers AND polymorphic-base hierarchies makes
    // their inherent methods `&self` (interior-mutable wrapper rule), which is
    // what the `&self` `Kind`/interface trait methods require, and keeps the
    // upcast a shared-`Rc` bump (identity-preserving) instead of a slice.
    // Recursive classes (a field-type cycle back to themselves — `Node {
    // Node? peer; }`, or mutual A↔B) MUST stay wrapped regardless of
    // aliasing: the inline demotion would emit an infinite-size struct
    // (rustc E0072); the wrapper's `Rc` is the indirection that breaks the
    // cycle. A component that isn't wrap-eligible (sealed / exception /
    // generic …) is dropped by the `eligible` intersection here — sealed
    // dispatches through `&self` match arms, and genuine conflicts are
    // rejected at tycheck.
    eligible
        .iter()
        .filter(|n| {
            aliased.contains(*n)
                || iface_forced.contains(*n)
                || poly_forced.contains(*n)
                || recursive.contains(*n)
                || weak_forced.contains(*n)
        })
        .cloned()
        .collect()
}

/// The concrete Rust representation chosen for a Jux class
/// (`Architecture/JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.3.3 + §CR.4.1).
/// Single-thread reps only; the cross-thread `Arc` / `Arc<Mutex>` rows are
/// deferred (a class that would need them stays `RcRefCell` today).
///
/// A class absent from the [`compute_class_reps`] map is the legacy plain
/// struct — semantically identical to [`ClassRep::Inline`]; the map only lists
/// wrap-*eligible* classes, so the emitters treat "absent" as Inline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ClassRep {
    /// Plain owned struct — never escapes, never aliased. Zero indirection.
    Inline,
    /// `Box<C_Inner>` — escapes (returned / owned) but never aliased: a unique
    /// owner, so no refcount and no interior-mutability cell.
    Box,
    /// `Rc<C_Inner>` — aliased (shared) but never mutated after construction:
    /// refcount sharing with **no** `RefCell` and therefore no borrow-flag cost.
    Rc,
    /// `Rc<RefCell<C_Inner>>` — aliased AND mutated: full Java shared-mutation
    /// semantics with statement-scoped runtime borrows (§CR.4.1, NORMATIVE).
    RcRefCell,
}

impl ClassRep {
    /// Generality order for the inheritance roll-up (§CR.3.5): a connected
    /// `extends` component takes the **max-rank** rep of its members, so a child
    /// can never be tighter than its parent (which would break upcasts).
    pub(crate) fn rank(self) -> u8 {
        match self {
            ClassRep::Inline => 0,
            ClassRep::Box => 1,
            ClassRep::Rc => 2,
            ClassRep::RcRefCell => 3,
        }
    }
}

/// Bare names of classes whose instances **escape** their introducing scope by
/// being **returned** from a function/method (§CR.3.3 `escapes` property). This
/// is the `Box` signal: a class that escapes but is never aliased is a unique
/// owner that can't be Inline (the moved-out address would be unstable), so it
/// lands on `Box<C_Inner>`. (Other escape routes — stored into a field, passed
/// as an argument — are already covered as *aliasing* by
/// [`compute_aliased_classes`], which forces at least `Rc`.)
pub(crate) fn compute_escaping_classes(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
) -> HashSet<String> {
    use juxc_ast::Stmt;

    fn class_bare_of_ty(ty: &Ty) -> Option<String> {
        match ty {
            Ty::User { name, .. } => Some(name.rsplit('.').next().unwrap_or(name).to_string()),
            Ty::Nullable(inner) => class_bare_of_ty(inner),
            _ => None,
        }
    }

    fn walk_block(b: &juxc_ast::Block, et: &HashMap<Span, Ty>, out: &mut HashSet<String>) {
        for s in &b.statements {
            walk_stmt(s, et, out);
        }
    }
    fn walk_else(branch: Option<&juxc_ast::ElseBranch>, et: &HashMap<Span, Ty>, out: &mut HashSet<String>) {
        match branch {
            Some(juxc_ast::ElseBranch::Block(b)) => walk_block(b, et, out),
            Some(juxc_ast::ElseBranch::If(inner)) => {
                walk_block(&inner.then_block, et, out);
                walk_else(inner.else_branch.as_deref(), et, out);
            }
            None => {}
        }
    }
    fn walk_stmt(s: &Stmt, et: &HashMap<Span, Ty>, out: &mut HashSet<String>) {
        match s {
            Stmt::Return(Some(e), _) => {
                if let Some(c) = et.get(&exprs::expr_span_of(e)).and_then(class_bare_of_ty) {
                    out.insert(c);
                }
            }
            Stmt::If(i) => {
                walk_block(&i.then_block, et, out);
                walk_else(i.else_branch.as_deref(), et, out);
            }
            Stmt::While(w) => walk_block(&w.body, et, out),
            Stmt::DoWhile(d) => walk_block(&d.body, et, out),
            Stmt::ForEach(f) => walk_block(&f.body, et, out),
            Stmt::ForC(f) => walk_block(&f.body, et, out),
            Stmt::Try(t) => {
                walk_block(&t.body, et, out);
                for c in &t.catches {
                    walk_block(&c.body, et, out);
                }
                if let Some(fin) = &t.finally {
                    walk_block(fin, et, out);
                }
            }
            Stmt::Unsafe(b) => walk_block(b, et, out),
            Stmt::Labeled { stmt, .. } => walk_stmt(stmt, et, out),
            _ => {}
        }
    }

    let mut escapes = HashSet::new();
    for unit in units {
        for item in &unit.items {
            for_each_body(item, &mut |b| walk_block(b, expr_types, &mut escapes));
        }
    }
    escapes
}

/// Pick a [`ClassRep`] per wrap-eligible class (§CR.3.3 + §CR.4.1).
///
/// Applies the decision table: `forced`/`aliased`+`mutated` → `RcRefCell`;
/// `aliased`+immutable → `Rc` (read-only share, no cell); `escapes`+unaliased →
/// `Box` (unique owner); otherwise Inline (absent). A conservative guard keeps
/// classes with properties or inheritance on `RcRefCell` while their non-field
/// emitters aren't rep-aware. The emitter consults this map; `wrapper_classes`
/// is the derived newtype set and `refcell_classes` its interior-mutable subset.
pub(crate) fn compute_class_reps(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
) -> HashMap<String, ClassRep> {
    // Wrap-eligibility gate: classes excluded here (sealed / generic-excluded /
    // intrinsic / exception …) stay on their legacy plain-struct path and never
    // appear in the rep map.
    let eligible = compute_wrapper_classes(units);
    // `aliased` (rules 1-3 + lambda capture, NO return — §CR.3.3) and `escapes`
    // (return — the `Box` signal) are now distinct.
    let aliased = compute_aliased_classes(units, expr_types);
    let escapes = compute_escaping_classes(units, expr_types);

    // Forced-RcRefCell seeds: these need the interior-mutable wrapper for
    // reasons other than a plain field write (dyn dispatch / interface trait
    // `&self`, recursive-field cycle break, weak-ref endpoints, generic-parent
    // upcast). They must NEVER demote to plain `Rc`/`Box` (§CR.3.3 dyn row / §CR.5).
    let mut forced: HashSet<String> = HashSet::new();
    forced.extend(compute_interface_forced_classes(units));
    forced.extend(compute_polymorphic_forced_classes(units));
    forced.extend(compute_recursive_field_classes(units));
    forced.extend(compute_weak_forced_classes(units));
    // Conservative demotion exclusions (Stage 2/3 scope): the field-READ path is
    // rep-aware, but a class's accessor-impl, inherited-method, super-shim, and
    // upcast emitters still emit `self.0.borrow()` unconditionally. Until those
    // are rep-aware too, keep any class that has a property accessor OR
    // participates in an `extends` relationship (as child or parent) on the
    // `RcRefCell` rep — so the `Rc`/`Box` demotions apply only to leaf,
    // property-free classes (the common immutable-value-object / factory-return
    // wins). "When in doubt, RcRefCell."
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                if !cd.properties.is_empty() {
                    forced.insert(cd.name.text.clone());
                }
                if let Some(parent) = cd
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
                {
                    forced.insert(cd.name.text.clone()); // child
                    forced.insert(parent); // parent
                }
            }
        }
    }

    let mutated = compute_mutated_classes(units, expr_types);

    // §CR.3.3 + §CR.4.1 decision table (first match wins):
    //   forced                          → RcRefCell
    //   aliased &&  mutated             → RcRefCell  (Java shared mutation)
    //   aliased && !mutated             → Rc         (read-only sharing, no cell)
    //  !aliased &&  escapes && !mutated → Box        (unique owner, heap, &self)
    //  !aliased &&  escapes &&  mutated → RcRefCell  (Box would need &mut-self
    //                                                 method receivers — deferred;
    //                                                 keep wrapped, safe)
    //  !aliased && !escapes             → Inline     (absent from the map)
    let mut reps: HashMap<String, ClassRep> = HashMap::new();
    for n in &eligible {
        let rep = if forced.contains(n) {
            ClassRep::RcRefCell
        } else if aliased.contains(n) {
            if mutated.contains(n) {
                ClassRep::RcRefCell
            } else {
                ClassRep::Rc
            }
        } else if escapes.contains(n) {
            if mutated.contains(n) {
                ClassRep::RcRefCell
            } else {
                ClassRep::Box
            }
        } else {
            continue; // Inline — legacy plain struct, no rep entry.
        };
        reps.insert(n.clone(), rep);
    }

    // §CR.3.5 inheritance roll-up: a connected `extends` component takes the
    // MAX-rank rep of its members, so a child is never tighter than its parent
    // (which would break an upcast). The wrapped set already rolled `aliased`
    // through the chain, but `mutated` is per-class — a mutated parent must pull
    // an immutable child up to `RcRefCell` (and vice-versa via the max).
    rollup_class_reps(&mut reps, units);
    reps
}

/// Raise every class in a connected `extends` component to the MAX-rank
/// [`ClassRep`] in that component (§CR.3.5). Fixpoint over the bidirectional
/// extends graph. Only classes already present in `reps` (wrapped) participate;
/// an Inline class never gains a rep here.
fn rollup_class_reps(reps: &mut HashMap<String, ClassRep>, units: &[juxc_ast::CompilationUnit]) {
    // Bidirectional parent↔child adjacency (bare names).
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                if let Some(parent) = cd
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
                {
                    let child = cd.name.text.clone();
                    adj.entry(child.clone()).or_default().push(parent.clone());
                    adj.entry(parent).or_default().push(child);
                }
            }
        }
    }
    // Fixpoint: keep lifting neighbours until no rep changes.
    let mut changed = true;
    while changed {
        changed = false;
        let names: Vec<String> = reps.keys().cloned().collect();
        for name in names {
            let rep = reps[&name];
            if let Some(neighbors) = adj.get(&name) {
                for n in neighbors.clone() {
                    // A neighbour that is wrapped (present) and lower-rank rises.
                    if let Some(nr) = reps.get(&n).copied() {
                        if nr.rank() < rep.rank() {
                            reps.insert(n, rep);
                            changed = true;
                        }
                    }
                }
            }
        }
    }
}

/// Bare names of classes whose instances are **mutated after construction** —
/// the `mutated` selector property (§CR.4.1). A class is mutated when an
/// instance field of one of its instances is written through a binding:
/// `this.f = …` (internal), an external `obj.f = …`, `++obj.f`, `out obj.f`,
/// `&obj.f`, or a mutating method call on a field (`obj.coll.push(x)`). The
/// **owner** whose field is touched is marked (so `a.b.coll.push()` marks
/// `a.b`'s class, not `a`'s).
///
/// SOUNDNESS — this gates the `RcRefCell`→`Rc` demotion. A class wrongly judged
/// "not mutated" loses its `RefCell`, so a later write emits `obj.0.f = v` on a
/// `Rc<C_Inner>` and **fails to compile** (rustc E0594) — a LOUD error, never a
/// silent shared-mutation fork. We therefore OVER-approximate: any uncertainty
/// keeps the class mutated (→ `RcRefCell`).
pub(crate) fn compute_mutated_classes(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
) -> HashSet<String> {
    use juxc_ast::{Expr, Stmt};

    let mut mutated: HashSet<String> = HashSet::new();
    let mut user_mut: HashSet<String> = HashSet::new();
    for unit in units {
        user_mut.extend(crate::analysis::collect_user_mut_methods(unit));
    }

    fn class_bare_of_ty(ty: &Ty) -> Option<String> {
        match ty {
            Ty::User { name, .. } => Some(name.rsplit('.').next().unwrap_or(name).to_string()),
            Ty::Nullable(inner) => class_bare_of_ty(inner),
            _ => None,
        }
    }
    // Mark the OWNER class of a written place: `obj.f` → obj's class; index /
    // not-null chains recurse to their base; a bare local marks nothing (a
    // local reassign isn't field mutation).
    fn mark_owner(place: &Expr, expr_types: &HashMap<Span, Ty>, out: &mut HashSet<String>) {
        match place {
            Expr::Field(f) => {
                if let Some(c) = expr_types
                    .get(&exprs::expr_span_of(&f.object))
                    .and_then(class_bare_of_ty)
                {
                    out.insert(c);
                }
            }
            Expr::Index(i) => mark_owner(&i.array, expr_types, out),
            Expr::NotNullAssert(inner, _) => mark_owner(inner, expr_types, out),
            _ => {}
        }
    }

    fn walk_expr(
        e: &Expr,
        et: &HashMap<Span, Ty>,
        um: &HashSet<String>,
        out: &mut HashSet<String>,
    ) {
        match e {
            Expr::TypeOf(..) => {}
            Expr::IncDec(i) => {
                mark_owner(&i.target, et, out);
                walk_expr(&i.target, et, um, out);
            }
            Expr::Out(inner, _) => {
                mark_owner(inner, et, out);
                walk_expr(inner, et, um, out);
            }
            Expr::Unary(u) => {
                if u.op == juxc_ast::UnaryOp::AddrOf {
                    mark_owner(&u.operand, et, out);
                }
                walk_expr(&u.operand, et, um, out);
            }
            Expr::Call(c) => {
                // A mutating method call mutates its receiver's contents; the
                // owner of the receiver field must allow `&mut` access.
                if let Expr::Field(f) = &*c.callee {
                    if crate::analysis::is_mutating_method(&f.field.text)
                        || um.contains(&f.field.text)
                    {
                        mark_owner(&f.object, et, out);
                    }
                }
                walk_expr(&c.callee, et, um, out);
                for a in &c.args {
                    walk_expr(a, et, um, out);
                }
            }
            Expr::NewObject(n) => {
                for a in &n.args {
                    walk_expr(a, et, um, out);
                }
            }
            Expr::NewArrayLit(n) => {
                for el in &n.elements {
                    walk_expr(el, et, um, out);
                }
            }
            Expr::NewArray(n) => {
                walk_expr(&n.size, et, um, out);
                for inner in &n.inner_sizes {
                    walk_expr(inner, et, um, out);
                }
            }
            Expr::TupleLit(elems, _) => {
                for el in elems {
                    walk_expr(el, et, um, out);
                }
            }
            Expr::ErrorProp(inner, _) => walk_expr(inner, et, um, out),
            Expr::TryExpr(t) => {
                walk_block(&t.body, et, um, out);
                for c in &t.catches {
                    walk_block(&c.body, et, um, out);
                }
            }
            Expr::Binary(b) => {
                walk_expr(&b.left, et, um, out);
                walk_expr(&b.right, et, um, out);
            }
            Expr::Range(r) => {
                walk_expr(&r.start, et, um, out);
                walk_expr(&r.end, et, um, out);
            }
            Expr::Cast(c) => walk_expr(&c.value, et, um, out),
            Expr::TypeTest(t) => walk_expr(&t.value, et, um, out),
            Expr::SizeOf(s) => walk_expr(&s.operand, et, um, out),
            Expr::Index(i) => {
                walk_expr(&i.array, et, um, out);
                walk_expr(&i.index, et, um, out);
            }
            Expr::Field(f) => walk_expr(&f.object, et, um, out),
            Expr::InterpString(s) => {
                for seg in &s.segments {
                    if let juxc_ast::InterpSegment::Expr(inner) = seg {
                        walk_expr(inner, et, um, out);
                    }
                }
            }
            Expr::Elvis(el) => {
                walk_expr(&el.value, et, um, out);
                walk_expr(&el.fallback, et, um, out);
            }
            Expr::Ternary(t) => {
                walk_expr(&t.condition, et, um, out);
                walk_expr(&t.then_branch, et, um, out);
                walk_expr(&t.else_branch, et, um, out);
            }
            Expr::Await(inner, _) => walk_expr(inner, et, um, out),
            Expr::NotNullAssert(inner, _) => walk_expr(inner, et, um, out),
            // A class captured-and-mutated by a lambda is already `aliased`
            // (lambda-capture rule in `compute_aliased_classes`) → RcRefCell, so
            // we don't need to recurse lambda bodies for the `mutated` property.
            _ => {}
        }
    }

    fn walk_block(
        b: &juxc_ast::Block,
        et: &HashMap<Span, Ty>,
        um: &HashSet<String>,
        out: &mut HashSet<String>,
    ) {
        for s in &b.statements {
            walk_stmt(s, et, um, out);
        }
    }

    fn walk_stmt(
        s: &Stmt,
        et: &HashMap<Span, Ty>,
        um: &HashSet<String>,
        out: &mut HashSet<String>,
    ) {
        match s {
            Stmt::Expr(e) => walk_expr(e, et, um, out),
            Stmt::Return(opt, _) => {
                if let Some(e) = opt {
                    walk_expr(e, et, um, out);
                }
            }
            Stmt::VarDecl(v) => {
                if let Some(init) = &v.init {
                    walk_expr(init, et, um, out);
                }
            }
            Stmt::Assign(a) => {
                // The assignment writes into `a.target`'s owner.
                mark_owner(&a.target, et, out);
                walk_expr(&a.value, et, um, out);
                walk_expr(&a.target, et, um, out);
            }
            Stmt::Throw(e, _) => walk_expr(e, et, um, out),
            Stmt::SuperCall(args, _) => {
                for a in args {
                    walk_expr(a, et, um, out);
                }
            }
            Stmt::If(i) => {
                walk_expr(&i.condition, et, um, out);
                walk_block(&i.then_block, et, um, out);
                walk_else(i.else_branch.as_deref(), et, um, out);
            }
            Stmt::While(w) => {
                walk_expr(&w.condition, et, um, out);
                walk_block(&w.body, et, um, out);
            }
            Stmt::DoWhile(d) => {
                walk_block(&d.body, et, um, out);
                walk_expr(&d.condition, et, um, out);
            }
            Stmt::ForEach(f) => {
                walk_expr(&f.iter, et, um, out);
                walk_block(&f.body, et, um, out);
            }
            Stmt::ForC(f) => {
                if let Some(cond) = &f.cond {
                    walk_expr(cond, et, um, out);
                }
                walk_block(&f.body, et, um, out);
            }
            Stmt::Try(t) => {
                walk_block(&t.body, et, um, out);
                for c in &t.catches {
                    walk_block(&c.body, et, um, out);
                }
                if let Some(fin) = &t.finally {
                    walk_block(fin, et, um, out);
                }
            }
            Stmt::Unsafe(b) => walk_block(b, et, um, out),
            Stmt::Break(..) | Stmt::Continue(..) => {}
            Stmt::Labeled { stmt, .. } => walk_stmt(stmt, et, um, out),
        }
    }

    fn walk_else(
        branch: Option<&juxc_ast::ElseBranch>,
        et: &HashMap<Span, Ty>,
        um: &HashSet<String>,
        out: &mut HashSet<String>,
    ) {
        match branch {
            Some(juxc_ast::ElseBranch::Block(b)) => walk_block(b, et, um, out),
            Some(juxc_ast::ElseBranch::If(inner)) => {
                walk_expr(&inner.condition, et, um, out);
                walk_block(&inner.then_block, et, um, out);
                walk_else(inner.else_branch.as_deref(), et, um, out);
            }
            None => {}
        }
    }

    for unit in units {
        for item in &unit.items {
            // Internal mutation, decided WITHOUT relying on `this` being typed in
            // `expr_types`: a class is mutated if any instance method writes
            // `this.f`, is a known mutating method, or it exposes a writable
            // property (a setter writes its backing field).
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                let internal = cd.methods.iter().any(|m| {
                    m.body.as_ref().map(crate::analysis::body_writes_to_this).unwrap_or(false)
                        || user_mut.contains(&m.name.text)
                }) || cd
                    .properties
                    .iter()
                    .any(|p| !p.is_static && p.setter.is_some());
                if internal {
                    mutated.insert(cd.name.text.clone());
                }
            }
            // External mutation (and a redundant internal pass via `this` typing):
            // walk every body for writes / mutating calls and mark the owner.
            for_each_body(item, &mut |b| walk_block(b, expr_types, &user_mut, &mut mutated));
        }
    }
    mutated
}

/// Invoke `f` on every **post-construction** executable
/// [`Block`](juxc_ast::Block) of a top-level decl — method / operator /
/// free-function bodies. **Constructor bodies are deliberately skipped**: a
/// ctor's `this.f = …` is construction, not mutation (§CR.4.1), so a value
/// object built once and never written stays `Rc` (not `RcRefCell`). A genuine
/// post-construction write to that object lives in a method / function / `main`
/// body and is still seen here.
fn for_each_body(item: &juxc_ast::TopLevelDecl, f: &mut dyn FnMut(&juxc_ast::Block)) {
    use juxc_ast::TopLevelDecl;
    match item {
        TopLevelDecl::Function(fd) => {
            if let Some(b) = &fd.body {
                f(b);
            }
        }
        TopLevelDecl::Class(cd) => {
            for m in &cd.methods {
                if let Some(b) = &m.body {
                    f(b);
                }
            }
            for op in &cd.operators {
                if let Some(b) = &op.body {
                    f(b);
                }
            }
            for nt in &cd.nested_types {
                for_each_body(nt, f);
            }
        }
        TopLevelDecl::Record(rd) => {
            for m in &rd.methods {
                if let Some(b) = &m.body {
                    f(b);
                }
            }
        }
        TopLevelDecl::Enum(ed) => {
            for m in &ed.methods {
                if let Some(b) = &m.body {
                    f(b);
                }
            }
        }
        TopLevelDecl::Interface(id) => {
            for m in &id.methods {
                if let Some(b) = &m.body {
                    f(b);
                }
            }
        }
        _ => {}
    }
}

/// Bare names of every class that must be **force-wrapped because it is an
/// endpoint of a `weak` field** (§6.5). A weak field stores a
/// `Weak<RefCell<Target_Inner>>`: the class that **declares** it reaches the
/// field through `self.0.borrow()`, and the **target** class supplies both the
/// `Target_Inner` payload the `Weak` points at and the `.0` that a store
/// downgrades. A weak ref whose endpoints aren't otherwise wrapped (no cycle,
/// alias, interface, or poly-base involvement) would therefore reference a
/// non-existent `_Inner` / missing `.0`; seeding both endpoints here keeps the
/// lowering well-formed. Returns the raw closure; [`compute_wrapped_set`]
/// intersects it with the wrap-eligible set.
/// Derive a valid Rust module identifier from a `.jux` file path's stem, for
/// the per-unit file split. The stem is **lower-cased** so the module name can
/// never collide with a PascalCase type the file defines (`Exception.jux` →
/// module `exception`, which re-exports the type `Exception` flat — a `mod
/// Exception;` would instead shadow the type, since modules and types share one
/// namespace). Non-ident characters become `_`, a leading digit is prefixed
/// with `_`, and a Rust-keyword stem gets a trailing `_` (so the name works as
/// BOTH a file name and a `mod` identifier — `r#`-escaping is no good in a path).
fn module_base_name(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unit")
        .to_ascii_lowercase();
    let mut out = String::new();
    for (i, ch) in stem.chars().enumerate() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if i == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("unit");
    }
    let ident = crate::backend_fqn::to_rust_ident(&out);
    match ident.strip_prefix("r#") {
        Some(stripped) => format!("{stripped}_"),
        None => ident,
    }
}

fn compute_weak_forced_classes(units: &[juxc_ast::CompilationUnit]) -> HashSet<String> {
    let mut forced = HashSet::new();
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                for field in &cd.fields {
                    if !field.is_weak {
                        continue;
                    }
                    // The declaring class.
                    forced.insert(cd.name.text.clone());
                    // The weak field's target class (E0455 guarantees the type
                    // resolves to a plain class).
                    if let Some(seg) = field.ty.as_ref().and_then(|t| t.name.segments.last()) {
                        forced.insert(seg.text.clone());
                    }
                }
            }
        }
    }
    forced
}

/// Bare names of every class that sits on a **field-type cycle** — its own
/// fields (or a chain of other classes' fields) eventually reference the
/// class itself: `class Node { Node? peer; }`, mutual `A { B b; } / B { A
/// a; }`, etc. Field types peel nullable / array / generic-arg layers so
/// `Node?`, `Node[]`, and `Bag<Node>` edges all count. These classes need
/// the wrapper's `Rc` indirection to be finite-sized (rustc E0072).
pub(crate) fn compute_recursive_field_classes(
    units: &[juxc_ast::CompilationUnit],
) -> HashSet<String> {
    // class name → field-referenced class names (one edge set per class).
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
    fn add_type_edges(ty: &juxc_ast::TypeRef, out: &mut HashSet<String>) {
        if let Some(seg) = ty.name.segments.last() {
            out.insert(seg.text.clone());
        }
        for arg in &ty.generic_args {
            if let juxc_ast::GenericArg::Type(t) = arg {
                add_type_edges(t, out);
            }
        }
    }
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                let entry = edges.entry(cd.name.text.clone()).or_default();
                for field in &cd.fields {
                    if let Some(ty) = &field.ty {
                        add_type_edges(ty, entry);
                    }
                }
                // The embedded `__parent` of a subclass is a field too.
                if let Some(parent) = &cd.extends {
                    add_type_edges(parent, entry);
                }
            }
        }
    }
    // A class is recursive iff its edge graph reaches back to itself.
    let mut recursive: HashSet<String> = HashSet::new();
    for start in edges.keys() {
        let mut stack: Vec<&str> = edges[start].iter().map(|s| s.as_str()).collect();
        let mut seen: HashSet<&str> = HashSet::new();
        while let Some(name) = stack.pop() {
            if name == start {
                recursive.insert(start.clone());
                break;
            }
            if !seen.insert(name) {
                continue;
            }
            if let Some(next) = edges.get(name) {
                stack.extend(next.iter().map(|s| s.as_str()));
            }
        }
    }
    recursive
}

/// Bare names of every class that must be **force-wrapped because it — or a
/// member of its connected inheritance component — implements an interface**.
///
/// Stage-1 interface dispatch flips interface trait methods and their
/// `impl Trait for C` delegations to `&self` (step 5), which is only sound
/// when every implementer is a wrapper class (interior-mutable, so a field
/// write goes through `self.0.borrow_mut()` instead of needing `&mut self`).
/// A class with a non-empty `implements` clause is therefore seeded into the
/// forced set, and the seed floods its **whole connected component** through
/// the `extends` graph (both directions): a wrapper child embeds its parent's
/// `_Inner` struct, so a value-class parent under a wrapper child wouldn't
/// compile (§CR.3.5 hierarchy uniformity).
///
/// Returns the raw (un-intersected) closure; [`compute_wrapped_set`]
/// intersects it with the wrap-eligible set, so a sealed/exception component
/// is excluded here and handled on its own path (sealed `&self` match
/// dispatch) or diagnosed at tycheck.
fn compute_interface_forced_classes(
    units: &[juxc_ast::CompilationUnit],
) -> HashSet<String> {
    // Direct `extends` parent (bare) per class, plus the inverse children
    // adjacency, and the seed list of classes that implement an interface.
    let mut parent_of: HashMap<String, Option<String>> = HashMap::new();
    let mut children_of: HashMap<String, Vec<String>> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                let name = cd.name.text.clone();
                let parent = cd
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
                if let Some(p) = &parent {
                    children_of.entry(p.clone()).or_default().push(name.clone());
                }
                parent_of.insert(name.clone(), parent);
                if !cd.implements.is_empty() {
                    stack.push(name);
                }
            }
        }
    }
    // Flood each seed's connected component (ancestors + descendants).
    let mut forced: HashSet<String> = HashSet::new();
    while let Some(name) = stack.pop() {
        if !forced.insert(name.clone()) {
            continue;
        }
        if let Some(Some(parent)) = parent_of.get(&name) {
            stack.push(parent.clone());
        }
        if let Some(kids) = children_of.get(&name) {
            for k in kids {
                stack.push(k.clone());
            }
        }
    }
    forced
}

/// Collect the names of the **`int`-typed const-generic parameters** in a
/// generic-params list — the `N` of `<T, int N>`. These feed
/// [`RustEmitter::const_int_params`] so bare value reads of `N` emit
/// `(N as isize)` (the param declares as Rust `const N: usize`; see
/// `emit_const_generic_param_decl`). `bool` const params need no cast and
/// are excluded.
pub(crate) fn collect_const_int_params(
    params: &[juxc_ast::TypeParam],
) -> HashSet<String> {
    params
        .iter()
        .filter(|p| {
            p.const_ty
                .as_ref()
                .and_then(|t| t.name.segments.last())
                .map(|s| s.text != "bool")
                .unwrap_or(false)
        })
        .map(|p| p.name.text.clone())
        .collect()
}

/// Collect the names of the **ordinary type parameters** in a
/// generic-params list — everything that is NOT a const param. Feeds
/// [`RustEmitter::current_type_params`] (generic-element array
/// recognition in `emit_new_array`).
pub(crate) fn collect_type_param_names(
    params: &[juxc_ast::TypeParam],
) -> HashSet<String> {
    params
        .iter()
        .filter(|p| !p.is_const())
        .map(|p| p.name.text.clone())
        .collect()
}

/// Map each non-const type param to its declared bounds — feeds
/// [`RustEmitter::type_param_bounds`] so a bound naming another in-scope param
/// (`<R extends K>`) can be expanded to that param's own bounds at emission.
pub(crate) fn collect_type_param_bounds(
    params: &[juxc_ast::TypeParam],
) -> HashMap<String, Vec<juxc_ast::TypeRef>> {
    params
        .iter()
        .filter(|p| !p.is_const())
        .map(|p| (p.name.text.clone(), p.bounds.clone()))
        .collect()
}

/// Recursively collect every identifier that appears in a [`TypeRef`] —
/// its dotted name segments plus the names inside each generic argument
/// (recursing through nested generics and wildcard bounds). Used to decide
/// which class type params are actually *referenced* by a field type.
pub(crate) fn collect_typeref_names(ty: &juxc_ast::TypeRef, out: &mut HashSet<String>) {
    for seg in &ty.name.segments {
        out.insert(seg.text.clone());
    }
    for arg in &ty.generic_args {
        match arg {
            // A concrete type argument — recurse into it.
            juxc_ast::GenericArg::Type(t) => collect_typeref_names(t, out),
            // A wildcard (`?`, `? extends T`, `? super T`) — its bound (if any)
            // can still name a param, so recurse into the bound type.
            juxc_ast::GenericArg::Wildcard(w) => match &w.bound {
                Some(juxc_ast::WildcardBound::Extends(t))
                | Some(juxc_ast::WildcardBound::Super(t)) => collect_typeref_names(t, out),
                None => {}
            },
        }
    }
}

/// Names of a class's declared **type parameters that are NOT referenced by
/// any instance field** (gap: rustc E0392 "type parameter never used").
///
/// Rust requires every declared generic param to appear in the struct body;
/// a Jux param used only in a method bound (`Registry<K, V, N>` where `K`
/// only constrains `V` and method type params) leaves `K` "phantom". For
/// each such param the backend emits a `__phantom_<name>:
/// std::marker::PhantomData<<name>>` field and initializes it in every
/// constructor path. **Const generics are excluded** — `const N: usize`
/// is a value param, doesn't need PhantomData, and `PhantomData<N>` would
/// be ill-formed.
///
/// "Referenced" means the param name appears anywhere in an instance field's
/// type (including nested generic args / wildcard bounds — see
/// [`collect_typeref_names`]). Static fields don't count: they're not part
/// of the struct layout.
pub(crate) fn unused_class_type_params(class_decl: &juxc_ast::ClassDecl) -> Vec<String> {
    if class_decl.generic_params.is_empty() {
        return Vec::new();
    }
    // Every name mentioned by any instance field's type.
    let mut used: HashSet<String> = HashSet::new();
    for field in &class_decl.fields {
        if field.is_static {
            continue;
        }
        let fty = juxc_tycheck::resolved_field_type(field);
        collect_typeref_names(&fty, &mut used);
    }
    // The parent slice (`extends Parent<T>`) also carries type params into
    // the struct via the `__parent` field — count those as used too.
    if let Some(parent) = &class_decl.extends {
        collect_typeref_names(parent, &mut used);
    }
    class_decl
        .generic_params
        .iter()
        .filter(|p| !p.is_const() && !used.contains(p.name.text.as_str()))
        .map(|p| p.name.text.clone())
        .collect()
}

/// Bare names of every **class that appears as the head of a generic bound**
/// across the whole program — `<V extends Container<? extends K>>` contributes
/// `Container`. Scans every generic-param list reachable in the units: class /
/// interface / function decls, and each class/interface method's own type
/// params (a method like `<R extends K>` only names params, never a class, but
/// scanning them is cheap and future-proof).
///
/// These classes get a method-carrying generic marker trait (see
/// [`RustEmitter::bound_position_classes`] and `emit_class_marker_trait`):
/// a bounded param `V: ContainerKind<K>` must be able to call the class's
/// public instance methods, which an empty marker can't express.
pub(crate) fn compute_bound_position_classes(
    units: &[juxc_ast::CompilationUnit],
) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
    // Record the head class name of every bound in a param list.
    fn scan_params(params: &[juxc_ast::TypeParam], out: &mut HashSet<String>) {
        for p in params {
            for bound in &p.bounds {
                // The bound's head segment, when it has generic args, can only
                // be a class (interfaces flow through unchanged; bare param
                // names carry no args). We record EVERY non-trivial head and
                // let the marker-synthesis side filter to actual classes — the
                // set is a superset hint, never authoritative on its own.
                if let Some(seg) = bound.name.segments.last() {
                    out.insert(seg.text.clone());
                }
            }
        }
    }
    for unit in units {
        for item in &unit.items {
            match item {
                juxc_ast::TopLevelDecl::Class(cd) => {
                    scan_params(&cd.generic_params, &mut out);
                    for m in &cd.methods {
                        scan_params(&m.generic_params, &mut out);
                    }
                }
                juxc_ast::TopLevelDecl::Interface(id) => {
                    scan_params(&id.generic_params, &mut out);
                    for m in &id.methods {
                        scan_params(&m.generic_params, &mut out);
                    }
                }
                juxc_ast::TopLevelDecl::Function(f) => {
                    scan_params(&f.generic_params, &mut out);
                }
                _ => {}
            }
        }
    }
    out
}

/// Bare names of every **polymorphic base class** — a class that is extended
/// by ≥1 other class and is itself non-sealed, non-final, and non-generic.
///
/// Stage-2 virtual dispatch lowers a polymorphic base's value slots to
/// `Rc<dyn <Name>Kind>`: a base-typed reference can hold any subclass instance
/// and dispatches dynamically through the populated `<Name>Kind` trait. The
/// exclusions each have their own path or are deferred: a **sealed** base uses
/// enum + match dispatch (already works); a **final** base can't be extended
/// (so it's never a base); a **generic** base would need an object-unsafe
/// `dyn Kind<T>` (deferred with a diagnostic).
pub(crate) fn compute_polymorphic_base_classes(
    units: &[juxc_ast::CompilationUnit],
) -> HashSet<String> {
    // `candidate` = classes eligible to be a base (non-sealed / non-final /
    // non-generic). `extended` = bare names that appear as some class's
    // `extends` target. A polymorphic base is the intersection.
    let mut candidate: HashSet<String> = HashSet::new();
    let mut extended: HashSet<String> = HashSet::new();
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                if !cd.is_sealed
                    && cd.permits.is_empty()
                    && !cd.is_final
                    && cd.generic_params.is_empty()
                {
                    candidate.insert(cd.name.text.clone());
                }
                if let Some(parent) = cd
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
                {
                    extended.insert(parent);
                }
            }
        }
    }
    candidate.intersection(&extended).cloned().collect()
}

/// Force-wrap closure for polymorphic-base hierarchies: every class in the
/// connected `extends` component reachable from a polymorphic base
/// ([`compute_polymorphic_base_classes`]), flooded in both directions.
///
/// Both the base AND all its subclasses must be wrapper classes so the upcast
/// coercion `std::rc::Rc::new(sub.clone()) as Rc<dyn BaseKind>` works — `sub`
/// derives `Clone`/`Debug` and the `.clone()` is a cheap `Rc` bump sharing the
/// inner `RefCell` (preserving identity) — and so the `__parent` embedding
/// stays uniform across the hierarchy (§CR.3.5). [`compute_wrapped_set`]
/// intersects this with the eligible set, so a component that includes a
/// sealed / exception / generic member collapses back to legacy.
fn compute_polymorphic_forced_classes(
    units: &[juxc_ast::CompilationUnit],
) -> HashSet<String> {
    let bases = compute_polymorphic_base_classes(units);
    let mut parent_of: HashMap<String, Option<String>> = HashMap::new();
    let mut children_of: HashMap<String, Vec<String>> = HashMap::new();
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                let name = cd.name.text.clone();
                let parent = cd
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
                if let Some(p) = &parent {
                    children_of.entry(p.clone()).or_default().push(name.clone());
                }
                parent_of.insert(name.clone(), parent);
            }
        }
    }
    let mut stack: Vec<String> = bases.into_iter().collect();
    let mut forced: HashSet<String> = HashSet::new();
    while let Some(name) = stack.pop() {
        if !forced.insert(name.clone()) {
            continue;
        }
        if let Some(Some(parent)) = parent_of.get(&name) {
            stack.push(parent.clone());
        }
        if let Some(kids) = children_of.get(&name) {
            for k in kids {
                stack.push(k.clone());
            }
        }
    }
    forced
}

/// Collect the bare names used as **cast / type-test targets** — the `T` in a
/// `(T) e` / `e as T` cast or (later) an `e => T` type-test — anywhere in the
/// program. "Finish polymorphism" emits a runtime-type `__jux_as_<T>` downcast
/// hook per collected target; bounding emission to actually-used targets keeps
/// the output of programs that never downcast unchanged. Mirrors the
/// `compute_aliased_classes` body walker.
fn compute_downcast_targets(units: &[juxc_ast::CompilationUnit]) -> HashSet<String> {
    use juxc_ast::TopLevelDecl;
    let mut out: HashSet<String> = HashSet::new();
    for unit in units {
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(f) => {
                    if let Some(b) = &f.body {
                        cast_targets_block(b, &mut out);
                    }
                }
                TopLevelDecl::Class(c) => {
                    for m in &c.methods {
                        if let Some(b) = &m.body {
                            cast_targets_block(b, &mut out);
                        }
                    }
                    for ctor in &c.constructors {
                        cast_targets_block(&ctor.body, &mut out);
                    }
                    for b in c.init_blocks.iter().chain(&c.static_init_blocks) {
                        cast_targets_block(b, &mut out);
                    }
                    for op in &c.operators {
                        if let Some(b) = &op.body {
                            cast_targets_block(b, &mut out);
                        }
                    }
                }
                TopLevelDecl::Interface(i) => {
                    for m in &i.methods {
                        if let Some(b) = &m.body {
                            cast_targets_block(b, &mut out);
                        }
                    }
                }
                TopLevelDecl::Record(r) => {
                    for m in &r.methods {
                        if let Some(b) = &m.body {
                            cast_targets_block(b, &mut out);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn cast_targets_block(b: &juxc_ast::Block, out: &mut HashSet<String>) {
    for s in &b.statements {
        cast_targets_stmt(s, out);
    }
}

fn cast_targets_stmt(s: &juxc_ast::Stmt, out: &mut HashSet<String>) {
    use juxc_ast::Stmt;
    match s {
        Stmt::Expr(e) => cast_targets_expr(e, out),
        Stmt::Return(Some(e), _) => cast_targets_expr(e, out),
        Stmt::Return(None, _) | Stmt::Break(..) | Stmt::Continue(..) => {}
        Stmt::Labeled { stmt, .. } => cast_targets_stmt(stmt, out),
        Stmt::VarDecl(v) => {
            if let Some(e) = &v.init {
                cast_targets_expr(e, out);
            }
        }
        Stmt::If(i) => {
            cast_targets_expr(&i.condition, out);
            cast_targets_block(&i.then_block, out);
            cast_targets_else(i.else_branch.as_deref(), out);
        }
        Stmt::While(w) => {
            cast_targets_expr(&w.condition, out);
            cast_targets_block(&w.body, out);
        }
        Stmt::DoWhile(d) => {
            cast_targets_block(&d.body, out);
            cast_targets_expr(&d.condition, out);
        }
        Stmt::ForEach(f) => {
            cast_targets_expr(&f.iter, out);
            cast_targets_block(&f.body, out);
        }
        Stmt::ForC(f) => {
            if let Some(i) = &f.init {
                cast_targets_stmt(i, out);
            }
            if let Some(c) = &f.cond {
                cast_targets_expr(c, out);
            }
            if let Some(u) = &f.update {
                cast_targets_stmt(u, out);
            }
            cast_targets_block(&f.body, out);
        }
        Stmt::Assign(a) => {
            cast_targets_expr(&a.target, out);
            cast_targets_expr(&a.value, out);
        }
        Stmt::SuperCall(args, _) => {
            for e in args {
                cast_targets_expr(e, out);
            }
        }
        Stmt::Throw(e, _) => cast_targets_expr(e, out),
        Stmt::Try(t) => {
            cast_targets_block(&t.body, out);
            for c in &t.catches {
                cast_targets_block(&c.body, out);
            }
            if let Some(f) = &t.finally {
                cast_targets_block(f, out);
            }
        }
        Stmt::Unsafe(b) => cast_targets_block(b, out),
    }
}

fn cast_targets_else(b: Option<&juxc_ast::ElseBranch>, out: &mut HashSet<String>) {
    match b {
        Some(juxc_ast::ElseBranch::If(i)) => {
            cast_targets_expr(&i.condition, out);
            cast_targets_block(&i.then_block, out);
            cast_targets_else(i.else_branch.as_deref(), out);
        }
        Some(juxc_ast::ElseBranch::Block(b)) => cast_targets_block(b, out),
        None => {}
    }
}

fn cast_targets_expr(e: &juxc_ast::Expr, out: &mut HashSet<String>) {
    use juxc_ast::Expr;
    match e {
        Expr::TypeOf(..) => {}
        Expr::Out(inner, _) => cast_targets_expr(inner, out),
        Expr::TupleLit(elems, _) => {
            for el in elems {
                cast_targets_expr(el, out);
            }
        }
        Expr::ErrorProp(inner, _) => cast_targets_expr(inner, out),
        Expr::TryExpr(t) => {
            cast_targets_block(&t.body, out);
            for c in &t.catches {
                cast_targets_block(&c.body, out);
            }
        }
        Expr::Cast(c) => {
            if let Some(seg) = c.ty.name.segments.last() {
                out.insert(seg.text.clone());
            }
            cast_targets_expr(&c.value, out);
        }
        Expr::TypeTest(t) => {
            if let Some(seg) = t.ty.name.segments.last() {
                out.insert(seg.text.clone());
            }
            cast_targets_expr(&t.value, out);
        }
        Expr::Call(c) => {
            cast_targets_expr(&c.callee, out);
            for a in &c.args {
                cast_targets_expr(a, out);
            }
        }
        Expr::NewObject(n) => {
            for a in &n.args {
                cast_targets_expr(a, out);
            }
            // Anonymous-class body: walk its init blocks and method bodies.
            if let Some(body) = &n.anonymous_body {
                for b in &body.init_blocks {
                    cast_targets_block(b, out);
                }
                for m in &body.methods {
                    if let Some(b) = &m.body {
                        cast_targets_block(b, out);
                    }
                }
            }
        }
        Expr::NewArrayLit(n) => {
            for el in &n.elements {
                cast_targets_expr(el, out);
            }
        }
        Expr::NewArray(n) => {
            cast_targets_expr(&n.size, out);
            for inner in &n.inner_sizes {
                cast_targets_expr(inner, out);
            }
        }
        Expr::Binary(b) => {
            cast_targets_expr(&b.left, out);
            cast_targets_expr(&b.right, out);
        }
        Expr::Unary(u) => cast_targets_expr(&u.operand, out),
        Expr::Range(r) => {
            cast_targets_expr(&r.start, out);
            cast_targets_expr(&r.end, out);
        }
        Expr::SizeOf(s) => cast_targets_expr(&s.operand, out),
        Expr::Index(i) => {
            cast_targets_expr(&i.array, out);
            cast_targets_expr(&i.index, out);
        }
        Expr::Field(f) => cast_targets_expr(&f.object, out),
        // `++place` / `place++` — walk the place for cast targets.
        Expr::IncDec(i) => cast_targets_expr(&i.target, out),
        Expr::InterpString(s) => {
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Expr(inner) = seg {
                    cast_targets_expr(inner, out);
                }
            }
        }
        Expr::Elvis(el) => {
            cast_targets_expr(&el.value, out);
            cast_targets_expr(&el.fallback, out);
        }
        Expr::Ternary(t) => {
            cast_targets_expr(&t.condition, out);
            cast_targets_expr(&t.then_branch, out);
            cast_targets_expr(&t.else_branch, out);
        }
        Expr::Await(inner, _) => cast_targets_expr(inner, out),
        Expr::NotNullAssert(inner, _) => cast_targets_expr(inner, out),
        Expr::Lambda(l) => match &l.body {
            juxc_ast::LambdaBody::Expr(b) => cast_targets_expr(b, out),
            juxc_ast::LambdaBody::Block(blk) => cast_targets_block(blk, out),
        },
        Expr::Switch(s) => {
            cast_targets_expr(&s.scrutinee, out);
            for arm in &s.arms {
                if let Some(g) = &arm.guard {
                    cast_targets_expr(g, out);
                }
                match &arm.body {
                    juxc_ast::SwitchBody::Expr(b) => cast_targets_expr(b, out),
                    juxc_ast::SwitchBody::Block(blk) => cast_targets_block(blk, out),
                }
            }
        }
        Expr::Literal(_)
        | Expr::Path(_)
        | Expr::This(_)
        | Expr::Super(_)
        | Expr::MethodRef(_) => {}
    }
}

/// Collect the **bare names of every class that is `throw`n or named
/// in a `catch`** across a workspace.
///
/// Thrown classes become `panic_any` payloads, which must be `Send`;
/// the wrapper rep (`Rc<RefCell<_>>`) is `!Send`, so these classes
/// (and, via the caller's chain walk, their hierarchies) must stay on
/// the legacy plain-struct path. Detection sources:
///
///   - `throw new X(...)` → the `NewObjectExpr`'s class name.
///   - `throw <bare path>` → that name (covers `throw cachedError;`).
///   - `catch (X e)` → the catch clause's declared type.
///
/// The walker descends through every nested block (if / while / for /
/// try) of every top-level function, class method, and constructor.
fn collect_thrown_class_names(
    units: &[juxc_ast::CompilationUnit],
) -> std::collections::HashSet<String> {
    use juxc_ast::{Stmt, TopLevelDecl};
    let mut out = std::collections::HashSet::new();

    fn bare_of(qn: &juxc_ast::QualifiedName) -> Option<String> {
        qn.segments.last().map(|s| s.text.clone())
    }
    fn ty_bare(t: &juxc_ast::TypeRef) -> Option<String> {
        t.name.segments.last().map(|s| s.text.clone())
    }

    fn walk_block(block: &juxc_ast::Block, out: &mut std::collections::HashSet<String>) {
        for stmt in &block.statements {
            walk_stmt(stmt, out);
        }
    }

    fn walk_else(
        branch: Option<&juxc_ast::ElseBranch>,
        out: &mut std::collections::HashSet<String>,
    ) {
        match branch {
            Some(juxc_ast::ElseBranch::If(inner)) => {
                walk_block(&inner.then_block, out);
                walk_else(inner.else_branch.as_deref(), out);
            }
            Some(juxc_ast::ElseBranch::Block(b)) => walk_block(b, out),
            None => {}
        }
    }

    fn walk_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<String>) {
        match stmt {
            Stmt::Throw(expr, _) => match expr {
                juxc_ast::Expr::NewObject(n) => {
                    if let Some(b) = bare_of(&n.class_name) {
                        out.insert(b);
                    }
                }
                juxc_ast::Expr::Path(qn) => {
                    if let Some(b) = bare_of(qn) {
                        out.insert(b);
                    }
                }
                _ => {}
            },
            Stmt::Try(t) => {
                walk_block(&t.body, out);
                for c in &t.catches {
                    if let Some(b) = ty_bare(&c.ty) {
                        out.insert(b);
                    }
                    walk_block(&c.body, out);
                }
                if let Some(fin) = &t.finally {
                    walk_block(fin, out);
                }
            }
            Stmt::If(s) => {
                walk_block(&s.then_block, out);
                walk_else(s.else_branch.as_deref(), out);
            }
            Stmt::While(s) => walk_block(&s.body, out),
            Stmt::DoWhile(s) => walk_block(&s.body, out),
            Stmt::ForEach(s) => walk_block(&s.body, out),
            _ => {}
        }
    }

    for unit in units {
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(f) => {
                    if let Some(body) = &f.body {
                        walk_block(body, &mut out);
                    }
                }
                TopLevelDecl::Class(cd) => {
                    for m in &cd.methods {
                        if let Some(body) = &m.body {
                            walk_block(body, &mut out);
                        }
                    }
                    for ctor in &cd.constructors {
                        walk_block(&ctor.body, &mut out);
                    }
                }
                _ => {}
            }
        }
    }

    out
}

/// Collect the **bare names of every class that some other class
/// extends** across a set of units. A class that is a parent in any
/// hierarchy must NOT take the Phase-A wrapper shape: the whole
/// inheritance chain has to roll up to a single representation
/// (§CR.3.5), and the `__parent`-embedding / `Deref` machinery the
/// child classes use assumes the legacy plain-struct parent shape.
/// Mixing a wrapper parent with a plain-struct child would break
/// `Deref`, upcasts, and (for thrown exception hierarchies) the
/// `Send` bound `panic_any` needs. Excluding extended classes keeps
/// Phase A to genuinely-leaf simple classes.
///
/// **Superseded** by [`compute_wrapper_classes`], which now performs a
/// whole-component analysis (parents AND children roll up together).
/// Kept for reference / potential reuse.
#[allow(dead_code)]
pub(crate) fn collect_extended_class_names(
    units: &[juxc_ast::CompilationUnit],
) -> std::collections::HashSet<String> {
    let mut extended = std::collections::HashSet::new();
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                if let Some(parent) = &cd.extends {
                    if let Some(seg) = parent.name.segments.last() {
                        extended.insert(seg.text.clone());
                    }
                }
            }
        }
    }
    extended
}

/// True when `(package, class_name)` names one of the stdlib classes
/// whose struct emission is *suppressed* in `emit_class_decl` (the
/// compiler owns the real implementation, lowering it onto a Rust std
/// container or runtime helper). Such names must NOT be registered as
/// wrapper classes — they never emit a `C_Inner` newtype, so routing
/// a field access through `.0.borrow()` would dangle. Mirrors the
/// early-return guards at the top of `emit_class_decl`.
pub(crate) fn is_intrinsic_class(pkg: &str, name: &str) -> bool {
    match (pkg, name) {
        ("jux.std.collections", "ArrayList" | "HashMap" | "HashSet" | "Deque") => true,
        ("jux.std.io", "File" | "Path" | "Console") => true,
        ("jux.std.concurrent", "Worker" | "Task" | "AtomicInt" | "AtomicLong") => true,
        ("jux.std.time", "Clock" | "Instant") => true,
        _ => false,
    }
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
        // Observable-property observer handle (§P.2/§P.3): one
        // attached observer of a `{ get; set; }` property. NAMED
        // observer variables attach weakly (§P.2.3 — the owner's
        // field keeps the closure alive; when the owner drops, the
        // weak ref dies and the property prunes it on next fire).
        // INLINE lambdas attach strongly — nothing else holds them,
        // so a weak attach would be dead on arrival.
        w.push_str("enum JuxObserver<F: ?Sized> {\n");
        w.push_str("    Weak(std::rc::Weak<F>),\n");
        w.push_str("    Strong(std::rc::Rc<F>),\n");
        w.push_str("    // A Strong ADAPTER whose real observer lives elsewhere\n");
        w.push_str("    // behind a weak ref (the §P.2.2 shape-2 wrapper): the\n");
        w.push_str("    // dead flag flips when the adapter detects its inner\n");
        w.push_str("    // observer died, so the fire loop prunes it (P5).\n");
        w.push_str("    StrongGuarded(std::rc::Rc<F>, std::rc::Rc<std::cell::Cell<bool>>),\n");
        w.push_str("}\n");
        w.push_str("impl<F: ?Sized> JuxObserver<F> {\n");
        w.push_str("    fn upgrade(&self) -> Option<std::rc::Rc<F>> {\n");
        w.push_str("        match self {\n");
        w.push_str("            JuxObserver::Weak(w) => w.upgrade(),\n");
        w.push_str("            JuxObserver::Strong(s) => Some(s.clone()),\n");
        w.push_str("            JuxObserver::StrongGuarded(s, dead) => {\n");
        w.push_str("                if dead.get() { None } else { Some(s.clone()) }\n");
        w.push_str("            }\n");
        w.push_str("        }\n");
        w.push_str("    }\n");
        w.push_str("    fn is_for(&self, target: &std::rc::Rc<F>) -> bool {\n");
        w.push_str("        self.upgrade().map_or(false, |f| std::rc::Rc::ptr_eq(&f, target))\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        w.push_str("impl<F: ?Sized> Clone for JuxObserver<F> {\n");
        w.push_str("    fn clone(&self) -> Self {\n");
        w.push_str("        match self {\n");
        w.push_str("            JuxObserver::Weak(w) => JuxObserver::Weak(w.clone()),\n");
        w.push_str("            JuxObserver::Strong(s) => JuxObserver::Strong(s.clone()),\n");
        w.push_str("            JuxObserver::StrongGuarded(s, d) => {\n");
        w.push_str("                JuxObserver::StrongGuarded(s.clone(), d.clone())\n");
        w.push_str("            }\n");
        w.push_str("        }\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        w.push_str("impl<F: ?Sized> std::fmt::Debug for JuxObserver<F> {\n");
        w.push_str("    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n");
        w.push_str("        f.write_str(\"observer\")\n");
        w.push_str("    }\n");
        w.push_str("}\n\n");
        // Memory-ordering adapter (§S.6.2): maps the Jux stdlib
        // `MemoryOrder` enum onto Rust's `atomic::Ordering` for the
        // explicit-order overloads of `AtomicInt`/`AtomicLong`.
        // Always emitted (dead_code-allowed) like the other helpers.
        w.push_str("fn __jux_order(o: crate::jux::std::concurrent::MemoryOrder) -> std::sync::atomic::Ordering {\n");
        w.push_str("    match o {\n");
        w.push_str("        crate::jux::std::concurrent::MemoryOrder::Relaxed => std::sync::atomic::Ordering::Relaxed,\n");
        w.push_str("        crate::jux::std::concurrent::MemoryOrder::Acquire => std::sync::atomic::Ordering::Acquire,\n");
        w.push_str("        crate::jux::std::concurrent::MemoryOrder::Release => std::sync::atomic::Ordering::Release,\n");
        w.push_str("        crate::jux::std::concurrent::MemoryOrder::AcqRel => std::sync::atomic::Ordering::AcqRel,\n");
        w.push_str("        crate::jux::std::concurrent::MemoryOrder::SeqCst => std::sync::atomic::Ordering::SeqCst,\n");
        w.push_str("    }\n");
        w.push_str("}\n\n");
        // Checked integer division/remainder (ERRATA.md E1 Java-parity
        // carve-out): integer `a / 0` and `a % 0` throw a catchable
        // `ArithmeticException("/ by zero")` instead of raw-panicking
        // with rustc's `&str` payload (which no `catch` arm could
        // downcast). Overflow (`MIN / -1`) keeps the overflow-row
        // policy: panic in debug, wrap in release — same as `+`/`-`/`*`.
        // Free fns wrap the trait so call sites emit
        // `crate::__jux_idiv(a, b)` with no trait import needed.
        w.push_str("trait JuxIntDiv {\n");
        w.push_str("    fn jux_div(self, rhs: Self) -> Self;\n");
        w.push_str("    fn jux_rem(self, rhs: Self) -> Self;\n");
        w.push_str("}\n");
        w.push_str("macro_rules! jux_int_div_impl {\n");
        w.push_str("    ($($t:ty),*) => {$(\n");
        w.push_str("        impl JuxIntDiv for $t {\n");
        w.push_str("            fn jux_div(self, rhs: Self) -> Self {\n");
        w.push_str("                if rhs == 0 {\n");
        w.push_str("                    std::panic::panic_any(crate::jux::std::exceptions::ArithmeticException::new(String::from(\"/ by zero\")));\n");
        w.push_str("                }\n");
        w.push_str("                if cfg!(debug_assertions) { self / rhs } else { self.wrapping_div(rhs) }\n");
        w.push_str("            }\n");
        w.push_str("            fn jux_rem(self, rhs: Self) -> Self {\n");
        w.push_str("                if rhs == 0 {\n");
        w.push_str("                    std::panic::panic_any(crate::jux::std::exceptions::ArithmeticException::new(String::from(\"/ by zero\")));\n");
        w.push_str("                }\n");
        w.push_str("                if cfg!(debug_assertions) { self % rhs } else { self.wrapping_rem(rhs) }\n");
        w.push_str("            }\n");
        w.push_str("        }\n");
        w.push_str("    )*};\n");
        w.push_str("}\n");
        w.push_str("jux_int_div_impl!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);\n");
        w.push_str("fn __jux_idiv<T: JuxIntDiv>(a: T, b: T) -> T { a.jux_div(b) }\n");
        w.push_str("fn __jux_irem<T: JuxIntDiv>(a: T, b: T) -> T { a.jux_rem(b) }\n\n");
        // Async-runtime helper: `__jux_yield_now()` returns a one-
        // shot yielding Future. On first poll it registers a
        // wake-up and returns `Poll::Pending`; on second poll it
        // returns `Poll::Ready(())`. Awaiting this in the middle
        // of an async function surrenders the polling slot back to
        // the executor — the scheduler then advances any sibling
        // futures inside the same `futures::join!` group before
        // returning to this task.
        //
        // The helper is always emitted (gated behind
        // `#[allow(dead_code)]` from the attribute block above) so
        // programs that never call `yield_now()` don't pay any
        // runtime cost. Inlining is left to rustc — the body is
        // small and the call site is hot in cooperative
        // workloads.
        w.push_str("struct __JuxYieldNow(bool);\n");
        w.push_str("impl std::future::Future for __JuxYieldNow {\n");
        w.push_str(
            "    type Output = ();\n",
        );
        w.push_str(
            "    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {\n",
        );
        w.push_str("        if self.0 {\n");
        w.push_str("            std::task::Poll::Ready(())\n");
        w.push_str("        } else {\n");
        w.push_str("            self.0 = true;\n");
        w.push_str("            cx.waker().wake_by_ref();\n");
        w.push_str("            std::task::Poll::Pending\n");
        w.push_str("        }\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        w.push_str("fn __jux_yield_now() -> __JuxYieldNow { __JuxYieldNow(false) }\n\n");
        // Task runtime — per JUX-ASYNC-ADDENDUM v2 §18.1.3/§18.1.4.
        // `spawn(f)` schedules the lambda's body on a global
        // ThreadPool and returns a `JuxTask<T>` handle immediately:
        //
        //   - `await task`     — JuxTask IS a Future (delegates to
        //     the inner RemoteHandle), so the ordinary await
        //     emission works unchanged.
        //   - `task.blockingGet()` — drive to completion from sync
        //     code (consumes the handle).
        //   - `task.cancel()`  — drop the handle; RemoteHandle
        //     cancels the remote computation on drop (consumes).
        //
        // Spawned bodies run on pool threads, so captures must be
        // Send — tycheck's E0702 capture scan enforces the Jux-level
        // rule (no wrapper-class objects).
        w.push_str("struct JuxTask<T>(Option<futures::future::RemoteHandle<T>>);\n");
        w.push_str("impl<T: 'static> JuxTask<T> {\n");
        w.push_str("    #[allow(non_snake_case)]\n");
        w.push_str("    fn blockingGet(mut self) -> T {\n");
        w.push_str("        futures::executor::block_on(self.0.take().expect(\"task already consumed\"))\n");
        w.push_str("    }\n");
        w.push_str("    fn cancel(mut self) {\n");
        w.push_str("        // Dropping the RemoteHandle cancels the remote\n");
        w.push_str("        // computation (the Drop impl would FORGET it).\n");
        w.push_str("        if let Some(h) = self.0.take() {\n");
        w.push_str("            std::mem::drop(h);\n");
        w.push_str("        }\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        // Per section 18.1.3 an UNAWAITED task runs to completion - but
        // RemoteHandle CANCELS its computation when dropped. The
        // Drop impl forgets the handle instead (detaching the
        // task); explicit `cancel()` drops the handle for real.
        w.push_str("impl<T> Drop for JuxTask<T> {\n");
        w.push_str("    fn drop(&mut self) {\n");
        w.push_str("        if let Some(h) = self.0.take() {\n");
        w.push_str("            h.forget();\n");
        w.push_str("        }\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        w.push_str("impl<T: 'static> std::future::Future for JuxTask<T> {\n");
        w.push_str("    type Output = T;\n");
        w.push_str("    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<T> {\n");
        w.push_str("        let h = self.0.as_mut().expect(\"awaiting a cancelled task\");\n");
        w.push_str("        std::pin::Pin::new(h).poll(cx)\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        w.push_str("static __JUX_TASK_POOL: std::sync::LazyLock<futures::executor::ThreadPool> =\n");
        w.push_str("    std::sync::LazyLock::new(|| futures::executor::ThreadPool::new().expect(\"task pool\"));\n");
        w.push_str("fn __jux_spawn<T: Send + 'static>(\n");
        w.push_str("    fut: impl std::future::Future<Output = T> + Send + 'static,\n");
        w.push_str(") -> JuxTask<T> {\n");
        w.push_str("    JuxTask(Some(\n");
        w.push_str("        futures::task::SpawnExt::spawn_with_handle(&mut &*__JUX_TASK_POOL, fut)\n");
        w.push_str("            .expect(\"spawn\"),\n");
        w.push_str("    ))\n");
        w.push_str("}\n\n");
        // Channel runtime — JUX-ASYNC v2 §18.3. A bounded async
        // channel: `send` suspends when full, `receive` suspends
        // when empty and resolves `null` once closed and drained.
        // The handle is Arc-shared and Clone, so it crosses task
        // boundaries (the spawn emission clone-rebinds captures).
        w.push_str("struct JuxChannelInner<T> {\n");
        w.push_str("    tx: std::sync::Mutex<Option<futures::channel::mpsc::Sender<T>>>,\n");
        w.push_str("    rx: futures::lock::Mutex<futures::channel::mpsc::Receiver<T>>,\n");
        w.push_str("}\n");
        w.push_str("struct JuxChannel<T> {\n");
        w.push_str("    inner: std::sync::Arc<JuxChannelInner<T>>,\n");
        w.push_str("}\n");
        w.push_str("impl<T> Clone for JuxChannel<T> {\n");
        w.push_str("    fn clone(&self) -> Self {\n");
        w.push_str("        JuxChannel { inner: self.inner.clone() }\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        w.push_str("impl<T> JuxChannel<T> {\n");
        w.push_str("    fn new(capacity: isize) -> Self {\n");
        w.push_str("        let (tx, rx) = futures::channel::mpsc::channel(capacity.max(1) as usize);\n");
        w.push_str("        JuxChannel {\n");
        w.push_str("            inner: std::sync::Arc::new(JuxChannelInner {\n");
        w.push_str("                tx: std::sync::Mutex::new(Some(tx)),\n");
        w.push_str("                rx: futures::lock::Mutex::new(rx),\n");
        w.push_str("            }),\n");
        w.push_str("        }\n");
        w.push_str("    }\n");
        w.push_str("    async fn send(&self, v: T) {\n");
        w.push_str("        let tx = self.inner.tx.lock().unwrap().clone();\n");
        w.push_str("        if let Some(mut tx) = tx {\n");
        w.push_str("            let _ = futures::sink::SinkExt::send(&mut tx, v).await;\n");
        w.push_str("        }\n");
        w.push_str("    }\n");
        w.push_str("    async fn receive(&self) -> Option<T> {\n");
        w.push_str("        let mut rx = self.inner.rx.lock().await;\n");
        w.push_str("        futures::stream::StreamExt::next(&mut *rx).await\n");
        w.push_str("    }\n");
        w.push_str("    fn close(&self) {\n");
        w.push_str("        *self.inner.tx.lock().unwrap() = None;\n");
        w.push_str("    }\n");
        w.push_str("}\n\n");
        // Async-stream runtime — §18.6. A `Stream<T>` handle is an
        // `Rc<RefCell<…>>` share over a boxed task-local stream
        // (`LocalBoxStream` — streams never cross threads; the E0702
        // capture gate rejects them in `spawn`). `next()` pulls one
        // element (None = exhausted, idempotent). Combinators CONSUME
        // the receiver: `take_inner` swaps the boxed stream out and
        // leaves `empty()` behind, so the elements flow through the
        // returned stream only (§18.6.5 — a stream is a one-shot
        // sequence and the combinator is its new front).
        w.push_str("struct JuxStream<T> {\n");
        w.push_str("    inner: std::rc::Rc<std::cell::RefCell<futures::stream::LocalBoxStream<'static, T>>>,\n");
        w.push_str("}\n");
        w.push_str("impl<T> Clone for JuxStream<T> {\n");
        w.push_str("    fn clone(&self) -> Self {\n");
        w.push_str("        JuxStream { inner: self.inner.clone() }\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        w.push_str("impl<T: Clone + 'static> JuxStream<T> {\n");
        w.push_str("    fn from_stream(s: futures::stream::LocalBoxStream<'static, T>) -> Self {\n");
        w.push_str("        // `fuse` makes exhaustion idempotent (§18.6.2): every\n");
        w.push_str("        // `next()` after the first None resolves None instead of\n");
        w.push_str("        // re-polling the source (`unfold` panics if re-polled).\n");
        w.push_str("        let fused = ::std::boxed::Box::pin(futures::stream::StreamExt::fuse(s));\n");
        w.push_str("        JuxStream { inner: std::rc::Rc::new(std::cell::RefCell::new(fused)) }\n");
        w.push_str("    }\n");
        w.push_str("    fn of(items: Vec<T>) -> Self {\n");
        w.push_str("        Self::from_stream(::std::boxed::Box::pin(futures::stream::iter(items)))\n");
        w.push_str("    }\n");
        w.push_str("    fn from(items: Vec<T>) -> Self {\n");
        w.push_str("        Self::of(items)\n");
        w.push_str("    }\n");
        w.push_str("    fn generate(\n");
        w.push_str("        f: impl FnMut() -> futures::future::LocalBoxFuture<'static, Option<T>> + 'static,\n");
        w.push_str("    ) -> Self {\n");
        w.push_str("        Self::from_stream(::std::boxed::Box::pin(futures::stream::unfold(f, |mut f| async move {\n");
        w.push_str("            f().await.map(|v| (v, f))\n");
        w.push_str("        })))\n");
        w.push_str("    }\n");
        w.push_str("    async fn next(&self) -> Option<T> {\n");
        w.push_str("        let mut s = self.inner.borrow_mut();\n");
        w.push_str("        futures::stream::StreamExt::next(&mut *s).await\n");
        w.push_str("    }\n");
        w.push_str("    fn take_inner(&self) -> futures::stream::LocalBoxStream<'static, T> {\n");
        w.push_str("        std::mem::replace(\n");
        w.push_str("            &mut *self.inner.borrow_mut(),\n");
        w.push_str("            ::std::boxed::Box::pin(futures::stream::empty()),\n");
        w.push_str("        )\n");
        w.push_str("    }\n");
        w.push_str("    #[allow(non_snake_case)]\n");
        w.push_str("    fn mapAsync<U: Clone + 'static>(&self, f: std::rc::Rc<dyn Fn(T) -> U>) -> JuxStream<U> {\n");
        w.push_str("        JuxStream::from_stream(::std::boxed::Box::pin(futures::stream::StreamExt::map(\n");
        w.push_str("            self.take_inner(),\n");
        w.push_str("            move |v| f(v),\n");
        w.push_str("        )))\n");
        w.push_str("    }\n");
        w.push_str("    #[allow(non_snake_case)]\n");
        w.push_str("    fn filterAsync(&self, f: std::rc::Rc<dyn Fn(T) -> bool>) -> JuxStream<T> {\n");
        w.push_str("        Self::from_stream(::std::boxed::Box::pin(futures::stream::StreamExt::filter(\n");
        w.push_str("            self.take_inner(),\n");
        w.push_str("            move |v: &T| futures::future::ready(f(v.clone())),\n");
        w.push_str("        )))\n");
        w.push_str("    }\n");
        w.push_str("    fn take(&self, n: isize) -> JuxStream<T> {\n");
        w.push_str("        Self::from_stream(::std::boxed::Box::pin(futures::stream::StreamExt::take(self.take_inner(), n.max(0) as usize)))\n");
        w.push_str("    }\n");
        w.push_str("    fn skip(&self, n: isize) -> JuxStream<T> {\n");
        w.push_str("        Self::from_stream(::std::boxed::Box::pin(futures::stream::StreamExt::skip(self.take_inner(), n.max(0) as usize)))\n");
        w.push_str("    }\n");
        w.push_str("    fn chain(&self, other: JuxStream<T>) -> JuxStream<T> {\n");
        w.push_str("        Self::from_stream(::std::boxed::Box::pin(futures::stream::StreamExt::chain(self.take_inner(), other.take_inner())))\n");
        w.push_str("    }\n");
        w.push_str("}\n\n");
        // AsyncMutex runtime — §18.3. `await m.lock()` suspends until
        // acquired; the returned guard is the only handle to the
        // protected value (`guard.value` reads/writes deref it) and
        // releases on scope exit. Holding a guard across an await is
        // the type's entire point.
        w.push_str("struct JuxAsyncMutex<T> {\n");
        w.push_str("    inner: std::sync::Arc<futures::lock::Mutex<T>>,\n");
        w.push_str("}\n");
        w.push_str("impl<T> Clone for JuxAsyncMutex<T> {\n");
        w.push_str("    fn clone(&self) -> Self {\n");
        w.push_str("        JuxAsyncMutex { inner: self.inner.clone() }\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        w.push_str("impl<T> JuxAsyncMutex<T> {\n");
        w.push_str("    fn new(v: T) -> Self {\n");
        w.push_str("        JuxAsyncMutex { inner: std::sync::Arc::new(futures::lock::Mutex::new(v)) }\n");
        w.push_str("    }\n");
        w.push_str("    async fn lock(&self) -> futures::lock::MutexGuard<'_, T> {\n");
        w.push_str("        self.inner.lock().await\n");
        w.push_str("    }\n");
        w.push_str("}\n\n");
        // Worker pool — per JUX-ASYNC-ADDENDUM §18.2. `Worker.spawn(f)`
        // runs `f` on a real OS thread from the system's thread
        // pool and returns a `Task<T>` (a Future yielding the
        // closure's return value). Cooperatively driven via the
        // futures executor like any other Future, so
        // `await task` integrates with the existing async lowering
        // without special-casing.
        //
        // Channel choice: `futures::channel::oneshot` provides a
        // single-producer single-consumer cross-thread channel
        // whose `Receiver<T>` is itself a Future. The spawned
        // thread sends the result through the tx end; the awaiting
        // task polls the rx end. If the thread panics, the tx is
        // dropped without a send, and polling the rx returns
        // `Err(Canceled)` — the helper's poll wrapper escalates
        // that to a panic so the failure isn't swallowed silently.
        //
        // Send/'static bounds on the closure mirror Rust's
        // `std::thread::spawn` — the spec hides these behind the
        // "transferable" terminology, but the constraints are
        // identical at the runtime layer.
        w.push_str("pub struct Task<T>(futures::channel::oneshot::Receiver<T>);\n");
        w.push_str("impl<T> std::future::Future for Task<T> {\n");
        w.push_str("    type Output = T;\n");
        w.push_str(
            "    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<T> {\n",
        );
        w.push_str(
            "        match std::future::Future::poll(std::pin::Pin::new(&mut self.0), cx) {\n",
        );
        w.push_str("            std::task::Poll::Ready(Ok(v))  => std::task::Poll::Ready(v),\n");
        w.push_str(
            "            std::task::Poll::Ready(Err(_)) => panic!(\"worker task aborted before completion\"),\n",
        );
        w.push_str("            std::task::Poll::Pending       => std::task::Poll::Pending,\n");
        w.push_str("        }\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        // `join()` — block the calling thread until the worker completes.
        // Uses futures::executor::block_on so no additional runtime dep needed.
        w.push_str("impl<T> Task<T> { pub fn join(self) -> T { futures::executor::block_on(self) } }\n");
        w.push_str("pub struct Worker;\n");
        w.push_str("impl Worker {\n");
        w.push_str(
            "    pub fn spawn<T: Send + 'static, F: FnOnce() -> T + Send + 'static>(f: F) -> Task<T> {\n",
        );
        w.push_str("        let (tx, rx) = futures::channel::oneshot::channel();\n");
        w.push_str("        std::thread::spawn(move || { let _ = tx.send(f()); });\n");
        w.push_str("        Task(rx)\n");
        w.push_str("    }\n");
        w.push_str("}\n");
        // `now_ms()` helper — wall-clock reading in milliseconds
        // since the UNIX epoch. Lives next to the worker pool
        // because benchmarks pairing the two (timing a parallel
        // workload) are the main reason it exists. The duration
        // computation can panic only if the system clock is
        // before 1970, which we treat as "return 0" to keep the
        // helper total.
        w.push_str("fn __jux_now_ms() -> i64 {\n");
        w.push_str(
            "    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)\n",
        );
        w.push_str("}\n\n");
        // Receiver-mutability discovery: every external (stub) method
        // carrying the bindgen `@MutSelf` marker mutates its receiver
        // (the real Rust signature is `&mut self`). Seeding the
        // mutation analysis from the symbol table keeps `let mut`
        // promotion correct as the library surface evolves — no
        // hardcoded method-name lists.
        let extern_mut_methods: HashSet<String> = symbols
            .classes
            .values()
            .filter(|c| c.is_external)
            .flat_map(|c| {
                c.methods.iter().filter_map(|(name, m)| {
                    if m.annotations
                        .iter()
                        .any(crate::exprs::field::annotation_is_mut_self)
                    {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
            })
            .collect();
        Self {
            w,
            mutated_in_fn: HashSet::new(),
            this_alias: None,
            enclosing_class: None,
            current_fn_params: std::collections::HashSet::new(),
            enclosing_interface: None,
            user_mut_methods: extern_mut_methods.clone(),
            extern_mut_methods,
            byref_params: std::collections::HashMap::new(),
            byref_param_names: HashSet::new(),
            emitting_lvalue: false,
            emitting_out_place: false,
            collection_args_prehoisted: false,
            emitting_const_context: false,
            emitting_format_arg: false,
            emitting_comparison_operand: false,
            emitting_nullable_target: false,
            nullable_locals: HashSet::new(),
            ref_locals: HashSet::new(),
            pointer_locals: HashSet::new(),
            weak_params: std::collections::HashMap::new(),
            emitting_ref_handle: false,
            current_return_type: None,
            source: None,
            symbols: symbols.clone(),
            expr_types,
            workspace_mode: false,
            entry_package: None,
            emitted_uses_in_module: std::collections::HashSet::new(),
            local_types: vec![std::collections::HashMap::new()],
            ctor_live_after: std::collections::HashSet::new(),
            pending_method_suffix: None,
            pending_decl_suffix: None,
            in_catch_arm: false,
            in_enum_method: false,
            current_switch_enum: None,
            test_mode: false,
            current_unit_idx: None,
            split_files: None,
            anonymous_class_counter: 0,
            class_asts: std::collections::HashMap::new(),
            wrapper_classes: std::collections::HashSet::new(),
            class_reps: std::collections::HashMap::new(),
            refcell_classes: std::collections::HashSet::new(),
            box_classes: std::collections::HashSet::new(),
            poly_base_classes: std::collections::HashSet::new(),
            bound_position_classes: std::collections::HashSet::new(),
            const_int_params: std::collections::HashSet::new(),
            out_params: std::collections::HashSet::new(),
            current_type_params: std::collections::HashSet::new(),
            type_param_bounds: std::collections::HashMap::new(),
            in_array_size_position: false,
            dynamic_array_target: false,
            target_array_shape: None,
            pending_loop_label: None,
            downcast_targets: std::collections::HashSet::new(),
            emitting_wrapper_class: false,
            in_value_type_position: false,
            in_try_closure: false,
            loop_emit_depth: 0,
            try_loopctl: Vec::new(),
            pending_setter_observer: None,
            pending_ctor_binds: Vec::new(),
            observer_shapes: std::collections::HashMap::new(),
            emitting_class_has_static_init: false,
            emitting_call_callee: false,
            emitting_method_receiver: false,
            in_lambda_body: false,
            lambda_void_target: false,
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
        // Seed the fixed-point with the extern `@MutSelf` set so transitive
        // callers (`pairWith` → `self.backing.peek()`) are folded in too —
        // otherwise the method emits `&mut self` while the call site fails to
        // promote its receiver to `let mut` (E0596).
        self.user_mut_methods = crate::analysis::collect_user_mut_methods_seeded(
            unit,
            &self.extern_mut_methods,
        );
        // Pre-pass (C6): record by-`&mut` foreign-collection params for
        // this unit. In workspace mode the entry point already populated
        // the map from EVERY unit (so cross-file calls resolve); this
        // per-unit call covers the single-file path and is idempotent.
        let unit_slice = std::slice::from_ref(unit);
        self.populate_byref_params(unit_slice);
        // C6: re-mark methods containing a self-aliasing byref call (the
        // per-unit `collect_user_mut_methods` above clobbered the set).
        self.mark_self_aliasing_mut_methods(unit_slice);
        // Pre-pass: stash this unit's class ASTs by FQN so
        // `emit_class_decl` can walk parents and copy inherited
        // concrete method bodies. Workspace mode does the same
        // collection upfront for every unit; the single-unit path
        // here covers `lower_with_source` and friends.
        if !self.workspace_mode {
            let pkg: Vec<String> = unit
                .package
                .as_ref()
                .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
                .unwrap_or_default();
            let pkg_str = pkg.join(".");
            // Merge this unit's wrapper classes into the running set.
            // Single-unit mode emits each unit independently, so we
            // union rather than overwrite (`lower_with_source` may feed
            // several units one at a time). Phase B (§CR.3.3): only the
            // wrap-eligible AND aliased classes wrap — non-aliased
            // eligible classes demote to the legacy Inline shape.
            for (n, rep) in compute_class_reps(std::slice::from_ref(unit), &self.expr_types) {
                self.wrapper_classes.insert(n.clone());
                if rep == ClassRep::RcRefCell {
                    self.refcell_classes.insert(n.clone());
                }
                if rep == ClassRep::Box {
                    self.box_classes.insert(n.clone());
                }
                self.class_reps.insert(n, rep);
            }
            for b in compute_polymorphic_base_classes(std::slice::from_ref(unit)) {
                // Only wrapper poly bases support `Rc<dyn …Kind>` dispatch.
                if self.wrapper_classes.contains(&b) {
                    self.poly_base_classes.insert(b);
                }
            }
            for t in compute_downcast_targets(std::slice::from_ref(unit)) {
                self.downcast_targets.insert(t);
            }
            for item in &unit.items {
                if let TopLevelDecl::Class(cd) = item {
                    let fqn = if pkg_str.is_empty() {
                        cd.name.text.clone()
                    } else {
                        format!("{pkg_str}.{}", cd.name.text)
                    };
                    self.class_asts.insert(fqn, cd.clone());
                }
            }
        }

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
            // Detect whether the user's main is `async T main()` —
            // when so, the inner function was renamed to
            // `__jux_async_main` by `emit_fn_decl`, and the shim
            // must drive it through `futures::executor::block_on`.
            let main_decl = unit.items.iter().find_map(|item| match item {
                TopLevelDecl::Function(fn_decl) if fn_decl.name.text == "main" => Some(fn_decl),
                _ => None,
            });
            let async_main = main_decl.is_some_and(|f| {
                matches!(f.return_type, juxc_ast::ReturnType::AsyncType(_))
            });
            // `main(String[] args)` / `main(String... args)` — the
            // shim feeds `std::env::args().skip(1)` (program name
            // excluded, like Java). A sync param-taking main was
            // renamed to `__jux_args_main` by `emit_fn_decl`.
            let takes_args = main_decl.is_some_and(|f| !f.params.is_empty());
            let args_expr = if takes_args { "std::env::args().skip(1).collect::<Vec<String>>()" } else { "" };
            self.w.newline();
            self.w.line("fn main() {");
            self.w.indent_inc();
            self.w.emit_indent();
            let path = package.join("::");
            if async_main {
                self.w.push_str("futures::executor::block_on(");
                self.w.push_str(&path);
                self.w.push_str("::__jux_async_main(");
                self.w.push_str(args_expr);
                self.w.push_str("));\n");
            } else if takes_args {
                self.w.push_str(&path);
                self.w.push_str("::__jux_args_main(");
                self.w.push_str(args_expr);
                self.w.push_str(");\n");
            } else {
                self.w.push_str(&path);
                self.w.push_str("::main();\n");
            }
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
        // package grouping". These stay inlined at the crate root
        // (in `main.rs`) in both single-file and split modes.
        for &idx in &tree.unit_indices {
            self.source = sources.get(idx).cloned();
            self.emit_compilation_unit(&units[idx]);
        }
        // **Split mode:** each top-level package becomes a file tree under
        // `src/<name>/`, declared in `main.rs` with `pub mod <name>;`. The
        // bodies + per-package `mod.rs` files are captured into `split_files`.
        if self.split_files.is_some() {
            for (name, child) in &tree.children {
                self.w.emit_indent();
                self.w.push_str("pub mod ");
                self.w.push_str(name);
                self.w.push_str(";\n");
                self.emit_package_files(child, units, sources, &[name.clone()]);
            }
            return;
        }
        // **Single-file mode (legacy):** each top-level package is a nested
        // `pub mod <name> { … }` block in the one `main.rs`. Per-module dedupe
        // of `use` lines: each `pub mod` opens a fresh Rust namespace, so the
        // emitted-uses set is saved before recursing and restored after.
        for (name, child) in &tree.children {
            self.w.emit_indent();
            self.w.push_str("pub mod ");
            self.w.push_str(name);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            let saved_uses = std::mem::take(&mut self.emitted_uses_in_module);
            self.emit_package_node_body(child, units, sources);
            self.emitted_uses_in_module = saved_uses;
            self.w.indent_dec();
            self.w.line("}");
        }
    }

    /// Split-mode generator for one package node at `pkg_path` (e.g.
    /// `["shop", "cart"]`). For each unit at this node, emit its body into a
    /// fresh file `src/<pkg_path>/<base>.rs` (no `pub mod` wrapper — the file
    /// IS the module); synthesize `src/<pkg_path>/mod.rs` declaring each
    /// unit-file (`mod <base>; pub use <base>::*;` — the re-export flattens
    /// `crate::shop::cart::cart::Cart` back to `crate::shop::cart::Cart`) and
    /// each sub-package (`pub mod <child>;`); then recurse into children.
    fn emit_package_files(
        &mut self,
        node: &PackageNode,
        units: &[CompilationUnit],
        sources: &[SourceFile],
        pkg_path: &[String],
    ) {
        let dir = pkg_path.join("/");
        let mut mod_rs = String::from(
            "// AUTO-GENERATED by juxc. DO NOT EDIT.\n\n",
        );
        let mut used_bases: std::collections::HashSet<String> = std::collections::HashSet::new();
        for &idx in &node.unit_indices {
            let unit = &units[idx];
            // Unique module base name from the `.jux` file stem.
            let mut base = sources
                .get(idx)
                .map(|s| module_base_name(&s.path().display().to_string()))
                .unwrap_or_else(|| format!("unit{idx}"));
            while !used_bases.insert(base.clone()) {
                base = format!("{base}_{idx}");
            }
            // Emit the unit body into a fresh writer (own file scope).
            let saved_w = std::mem::replace(&mut self.w, writer::Writer::new());
            let saved_uses = std::mem::take(&mut self.emitted_uses_in_module);
            self.w.push_str("// AUTO-GENERATED by juxc. DO NOT EDIT.\n");
            if let Some(src) = sources.get(idx) {
                self.w.push_str(&format!("// Source: {}\n", src.path().display()));
            }
            // Bring every SAME-PACKAGE sibling into scope. The parent `mod.rs`
            // re-exports each unit-file flat (`pub use <base>::*;`), so
            // `use super::*;` resolves a bare reference to a sibling type
            // (`Iterator` → `crate::<pkg>::Iterator`) — which in the old
            // single-file output worked because all siblings shared one
            // `pub mod` scope. The explicit glob also shadows Rust's prelude
            // (so a Jux `Iterator` wins over `std::iter::Iterator`).
            self.w.push_str("#[allow(unused_imports)]\nuse super::*;\n\n");
            if !self.workspace_mode {
                self.user_mut_methods = crate::analysis::collect_user_mut_methods_seeded(
                    unit,
                    &self.extern_mut_methods,
                );
            }
            let prev_unit = self.current_unit_idx.take();
            self.current_unit_idx = Some(idx);
            self.source = sources.get(idx).cloned();
            self.emit_imports(&unit.imports, /*inside_package_mod=*/ true);
            for item in &unit.items {
                self.emit_top_level_decl(item);
            }
            self.current_unit_idx = prev_unit;
            self.emitted_uses_in_module = saved_uses;
            let body = std::mem::replace(&mut self.w, saved_w).into_string();
            if let Some(files) = &mut self.split_files {
                files.push((format!("src/{dir}/{base}.rs"), body));
            }
            mod_rs.push_str(&format!("mod {base};\npub use {base}::*;\n"));
        }
        // Declare sub-packages, then recurse to emit their files.
        for name in node.children.keys() {
            mod_rs.push_str(&format!("pub mod {name};\n"));
        }
        if let Some(files) = &mut self.split_files {
            files.push((format!("src/{dir}/mod.rs"), mod_rs));
        }
        for (name, child) in &node.children {
            let mut child_path = pkg_path.to_vec();
            child_path.push(name.clone());
            self.emit_package_files(child, units, sources, &child_path);
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
                self.user_mut_methods = crate::analysis::collect_user_mut_methods_seeded(
                    unit,
                    &self.extern_mut_methods,
                );
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
            let saved_uses = std::mem::take(&mut self.emitted_uses_in_module);
            self.emit_package_node_body(child, units, sources);
            self.emitted_uses_in_module = saved_uses;
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
        // Every unit that declares a free `main`. For a clean compile there's
        // exactly one (a second is E0400); a `[[bin]] main = "…"` key lets a
        // multi-`main` project pick which one is THE entry.
        let has_main = |u: &&CompilationUnit| {
            u.items.iter().any(|item| {
                matches!(item, TopLevelDecl::Function(f) if f.name.text == "main")
            })
        };
        let unit_pkg = |u: &CompilationUnit| -> Vec<String> {
            u.package
                .as_ref()
                .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
                .unwrap_or_default()
        };
        // Prefer the unit whose package matches the manifest's entry package;
        // otherwise the first `main`-bearing unit (legacy behavior).
        let chosen = self
            .entry_package
            .as_ref()
            .and_then(|want| units.iter().filter(has_main).find(|u| &unit_pkg(u) == want))
            .or_else(|| units.iter().find(has_main));
        let Some(unit) = chosen else { return };
        {
            // Locate the user's `main` and remember its async-ness —
            // async mains are emitted under `__jux_async_main` and
            // need `futures::executor::block_on(...)` to drive.
            let main_fn = unit.items.iter().find_map(|item| match item {
                TopLevelDecl::Function(fn_decl) if fn_decl.name.text == "main" => Some(fn_decl),
                _ => None,
            });
            let Some(main_fn) = main_fn else { return };
            let is_async_main =
                matches!(main_fn.return_type, juxc_ast::ReturnType::AsyncType(_));
            // Param-taking main (`String[]` / `String...`, §E.1.2) —
            // the shim feeds `std::env::args().skip(1)`. Sync forms
            // were renamed to `__jux_args_main` by `emit_fn_decl`.
            let takes_args = !main_fn.params.is_empty();
            let args_expr = if takes_args { "std::env::args().skip(1).collect::<Vec<String>>()" } else { "" };
            let pkg: Vec<&str> = unit
                .package
                .as_ref()
                .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect())
                .unwrap_or_default();
            if pkg.is_empty() {
                // Crate-root main. A SYNC main already serves as the binary
                // entry point (`fn main` / `pub fn main` emitted in place), so
                // nothing to add. An ASYNC main was renamed to
                // `__jux_async_main` (emit_fn_decl), so emit the sync shim that
                // drives it — it sits at the crate root, no module path needed.
                if is_async_main {
                    self.w.newline();
                    self.w.line("fn main() {");
                    self.w.indent_inc();
                    self.w.emit_indent();
                    self.w.push_str("futures::executor::block_on(__jux_async_main(");
                    self.w.push_str(args_expr);
                    self.w.push_str("));\n");
                    self.w.indent_dec();
                    self.w.line("}");
                } else if takes_args {
                    // Crate-root sync `main(args)` was renamed to
                    // `__jux_args_main`; this shim is the real entry.
                    self.w.newline();
                    self.w.line("fn main() {");
                    self.w.indent_inc();
                    self.w.emit_indent();
                    self.w.push_str("__jux_args_main(");
                    self.w.push_str(args_expr);
                    self.w.push_str(");\n");
                    self.w.indent_dec();
                    self.w.line("}");
                }
                return;
            }
            self.w.newline();
            self.w.line("fn main() {");
            self.w.indent_inc();
            self.w.emit_indent();
            let path = pkg.join("::");
            if is_async_main {
                // Reach into the user's package and drive their async
                // main via the futures executor. The user's main was
                // renamed to `__jux_async_main` by `emit_fn_decl`.
                self.w.push_str("futures::executor::block_on(");
                self.w.push_str(&path);
                self.w.push_str("::__jux_async_main(");
                self.w.push_str(args_expr);
                self.w.push_str("));\n");
            } else if takes_args {
                self.w.push_str(&path);
                self.w.push_str("::__jux_args_main(");
                self.w.push_str(args_expr);
                self.w.push_str(");\n");
            } else {
                self.w.push_str(&path);
                self.w.push_str("::main();\n");
            }
            self.w.indent_dec();
            self.w.line("}");
            return;
        }
    }

    /// Emit the test-runner `fn main()` for `jux test`
    /// (JUX-TESTING-ADDENDUM §TS.2/§TS.5–§TS.8). Walks every unit's
    /// top-level functions and collects `@Test`s plus the per-file
    /// lifecycle hooks (`@BeforeAll`/`@BeforeEach`/`@AfterEach`/
    /// `@AfterAll`, all case-insensitive). Each test runs inside
    /// `std::panic::catch_unwind` so a failed assertion (a panic,
    /// §TS.4) fails the test instead of aborting the runner; async
    /// tests are driven with `block_on`. A runtime argv substring
    /// filter selects tests without recompiling. Output:
    ///
    /// ```text
    /// running 3 tests
    ///   PASS pkg.test_one
    ///   FAIL pkg.test_three: assertEqual: expected `42`, got `41`
    ///
    /// test result: FAILED. 2 passed; 1 failed
    /// ```
    pub(crate) fn emit_test_runner_main(&mut self, units: &[CompilationUnit]) {
        // Case-insensitive annotation probe.
        let has_ann = |f: &juxc_ast::FnDecl, name: &str| {
            f.annotations.iter().any(|a| {
                a.name
                    .segments
                    .last()
                    .is_some_and(|seg| seg.text.eq_ignore_ascii_case(name))
            })
        };
        // Per-unit discovery: tests + hooks, file = lifecycle scope
        // (§TS.5). `call` is the Rust path (`pkg::fn`), `display` the
        // package-qualified Jux name (`pkg.fn`).
        struct UnitPlan {
            tests: Vec<(String, String, bool)>, // (call, display, is_async)
            before_each: Vec<(String, bool)>,
            after_each: Vec<(String, bool)>,
            before_all: Vec<(String, bool)>,
            after_all: Vec<(String, bool)>,
        }
        let mut plans: Vec<UnitPlan> = Vec::new();
        for unit in units {
            let pkg: Vec<&str> = unit
                .package
                .as_ref()
                .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect())
                .unwrap_or_default();
            let rust_prefix: String =
                pkg.iter().map(|s| format!("{s}::")).collect();
            let jux_prefix: String = pkg.iter().map(|s| format!("{s}.")).collect();
            let mut plan = UnitPlan {
                tests: Vec::new(),
                before_each: Vec::new(),
                after_each: Vec::new(),
                before_all: Vec::new(),
                after_all: Vec::new(),
            };
            for item in &unit.items {
                let TopLevelDecl::Function(fn_decl) = item else {
                    continue;
                };
                let is_async = matches!(
                    fn_decl.return_type,
                    juxc_ast::ReturnType::AsyncType(_),
                );
                let call = format!("{rust_prefix}{}", fn_decl.name.text);
                if has_ann(fn_decl, "Test") {
                    let display = format!("{jux_prefix}{}", fn_decl.name.text);
                    plan.tests.push((call, display, is_async));
                } else if has_ann(fn_decl, "BeforeEach") {
                    plan.before_each.push((call, is_async));
                } else if has_ann(fn_decl, "AfterEach") {
                    plan.after_each.push((call, is_async));
                } else if has_ann(fn_decl, "BeforeAll") {
                    plan.before_all.push((call, is_async));
                } else if has_ann(fn_decl, "AfterAll") {
                    plan.after_all.push((call, is_async));
                }
            }
            if !plan.tests.is_empty() {
                plans.push(plan);
            }
        }
        // Render one hook/test invocation, driving async fns through
        // the executor.
        let call_text = |call: &str, is_async: bool| -> String {
            if is_async {
                format!("futures::executor::block_on({call}());")
            } else {
                format!("{call}();")
            }
        };

        // Payload → message helper: assertion failures and runtime
        // panics carry String/&str; an escaped typed exception renders
        // `<Class>: <message>` through one downcast arm per known
        // exception class (same enumeration the uncaught-exception
        // reporter uses).
        self.w.newline();
        self.w.line(
            "fn __jux_panic_msg(p: &::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send>) -> String {",
        );
        self.w.indent_inc();
        self.w
            .line("if let Some(s) = p.downcast_ref::<String>() { return s.clone(); }");
        self.w.line(
            "if let Some(s) = p.downcast_ref::<&'static str>() { return s.to_string(); }",
        );
        for fqn in self.throwable_class_fqns() {
            let path = match backend_fqn::fqn_package(&fqn) {
                Some(pkg) => format!(
                    "crate::{}::{}",
                    pkg.split('.').collect::<Vec<_>>().join("::"),
                    backend_fqn::fqn_bare(&fqn),
                ),
                None => backend_fqn::fqn_bare(&fqn).to_string(),
            };
            self.w.line(&format!(
                "if let Some(e) = p.downcast_ref::<{path}>() {{ return format!(\"{fqn}: {{}}\", e.getMessage()); }}",
            ));
        }
        self.w.line("String::from(\"<panic>\")");
        self.w.indent_dec();
        self.w.line("}");

        let total: usize = plans.iter().map(|p| p.tests.len()).sum();
        self.w.newline();
        self.w.line("fn main() {");
        self.w.indent_inc();
        // Suppress the default panic hook's "thread 'main' panicked
        // at …" noise — failures surface as FAIL lines. Fully-
        // qualified `Box` so a user `class Box` can't shadow std's.
        self.w
            .line("std::panic::set_hook(::std::boxed::Box::new(|_| {}));");
        // Runtime filtering (§TS.8): argv substrings select tests.
        self.w.line(
            "let __jux_filter: Vec<String> = std::env::args().skip(1).collect();",
        );
        self.w.line(
            "let __jux_match = |n: &str| __jux_filter.is_empty() || __jux_filter.iter().any(|f| n.contains(f.as_str()));",
        );
        self.w.emit_indent();
        self.w.push_str("let __jux_names: [&str; ");
        self.w.push_str(&total.to_string());
        self.w.push_str("] = [");
        for (i, (_, display, _)) in
            plans.iter().flat_map(|p| p.tests.iter()).enumerate().map(|(i, t)| (i, t))
        {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push('"');
            self.w.push_str(display);
            self.w.push('"');
        }
        self.w.push_str("];\n");
        self.w.line(
            "let __jux_total = __jux_names.iter().filter(|n| __jux_match(n)).count();",
        );
        self.w
            .line("println!(\"running {} tests\", __jux_total);");
        self.w.line("let mut __jux_passed: i64 = 0;");
        self.w.line("let mut __jux_failed: i64 = 0;");

        for (ui, plan) in plans.iter().enumerate() {
            // Lazy once-per-file BeforeAll (§TS.5) — runs inside the
            // first executed test's catch_unwind, so its failure
            // fails that test.
            self.w
                .line(&format!("let mut __jux_unit{ui}_started = false;"));
            for (call, display, is_async) in &plan.tests {
                self.w
                    .line(&format!("if __jux_match(\"{display}\") {{"));
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str(
                    "let __jux_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {\n",
                );
                self.w.indent_inc();
                self.w.line(&format!(
                    "if !__jux_unit{ui}_started {{ __jux_unit{ui}_started = true;"
                ));
                self.w.indent_inc();
                for (h, ha) in &plan.before_all {
                    self.w.line(&call_text(h, *ha));
                }
                self.w.indent_dec();
                self.w.line("}");
                for (h, ha) in &plan.before_each {
                    self.w.line(&call_text(h, *ha));
                }
                self.w.line(&call_text(call, *is_async));
                self.w.indent_dec();
                self.w.line("}));");
                // AfterEach runs in its OWN catch_unwind — even when
                // the test failed (§TS.5).
                self.w.emit_indent();
                self.w.push_str(
                    "let __jux_after = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {\n",
                );
                self.w.indent_inc();
                for (h, ha) in &plan.after_each {
                    self.w.line(&call_text(h, *ha));
                }
                self.w.indent_dec();
                self.w.line("}));");
                self.w.line("match (&__jux_result, &__jux_after) {");
                self.w.indent_inc();
                self.w.line(&format!(
                    "(Ok(_), Ok(_)) => {{ println!(\"  PASS {display}\"); __jux_passed += 1; }}"
                ));
                self.w.line(&format!(
                    "(Err(p), _) => {{ println!(\"  FAIL {display}: {{}}\", __jux_panic_msg(p)); __jux_failed += 1; }}"
                ));
                self.w.line(&format!(
                    "(Ok(_), Err(p)) => {{ println!(\"  FAIL {display}: <afterEach> {{}}\", __jux_panic_msg(p)); __jux_failed += 1; }}"
                ));
                self.w.indent_dec();
                self.w.line("}");
                self.w.indent_dec();
                self.w.line("}");
            }
            if !plan.after_all.is_empty() {
                // Per-file AfterAll — only when the file ran (§TS.5);
                // a failure counts against the run without masking
                // any test's own result.
                self.w
                    .line(&format!("if __jux_unit{ui}_started {{"));
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str(
                    "let __jux_aa = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {\n",
                );
                self.w.indent_inc();
                for (h, ha) in &plan.after_all {
                    self.w.line(&call_text(h, *ha));
                }
                self.w.indent_dec();
                self.w.line("}));");
                self.w.line(
                    "if let Err(p) = &__jux_aa { println!(\"  FAIL <afterAll>: {}\", __jux_panic_msg(p)); __jux_failed += 1; }",
                );
                self.w.indent_dec();
                self.w.line("}");
            }
        }
        // Summary + exit code (§TS.7); `; N filtered out` when a
        // filter was active.
        self.w.line("println!();");
        self.w.line(
            "let __jux_filtered = __jux_names.len() - __jux_total;",
        );
        self.w.line(
            "if __jux_filtered > 0 { println!(\"test result: {}. {} passed; {} failed; {} filtered out\", if __jux_failed == 0 { \"ok\" } else { \"FAILED\" }, __jux_passed, __jux_failed, __jux_filtered); } else { println!(\"test result: {}. {} passed; {} failed\", if __jux_failed == 0 { \"ok\" } else { \"FAILED\" }, __jux_passed, __jux_failed); }",
        );
        self.w.line("if __jux_failed > 0 { std::process::exit(1); }");
        self.w.indent_dec();
        self.w.line("}");
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
    /// If `spec` imports a single external stub type whose real Rust path is
    /// known (§G.9.2), return the `use <real_path>[ as Alias];` line. The real
    /// path is absolute (`std::…`), so no `crate::` prefix applies. Returns
    /// `None` for wildcard/grouped imports, non-external types, or types without
    /// a recorded `@rust` path — those fall through to the ordinary `render_use`.
    fn external_use_line(&self, spec: &ImportSpec) -> Option<String> {
        let ImportSpec::Path { name, wildcard, alias } = spec else {
            return None;
        };
        if *wildcard || name.segments.is_empty() {
            return None;
        }
        let segs: Vec<&str> = name.segments.iter().map(|s| s.text.as_str()).collect();
        let fqn = segs.join(".");

        // Preferred path: an external *type* records its real Rust path on
        // `ClassSig::rust_path` (from the stub's `@rust("…")` annotation), so
        // `rust.std.HashSet` lowers to `use std::collections::HashSet;`.
        if let Some(sig) = self.symbols.classes.get(&fqn) {
            if sig.is_external {
                if let Some(real) = sig.rust_path.as_ref() {
                    // The real path's last segment equals the Jux type name
                    // (bindgen keeps the Rust type name), so a plain `use`
                    // binds it under that name.
                    return Some(match alias {
                        Some(a) => format!("use {real} as {};", a.text),
                        None => format!("use {real};"),
                    });
                }
            }
        }

        // A foreign free FUNCTION with an `@rust("real::path")` annotation
        // (`FunctionSig::rust_path`): the Rust name is snake_case
        // (`parse_duration`) but the Jux stub name is camelCase
        // (`parseDuration`), so we bind the real path UNDER the Jux name with an
        // alias — `use humantime::parse_duration as parseDuration;`. The bare
        // call `parseDuration(...)` then resolves to the foreign function.
        if let Some(sig) = self.symbols.functions.get(&fqn) {
            if let Some(real) = sig.rust_path.as_ref() {
                // The Jux-facing name is the import's alias if present, else the
                // last segment of the imported path (the camelCase stub name).
                let jux_name = match alias {
                    Some(a) => a.text.as_str(),
                    None => segs.last().copied().unwrap_or(""),
                };
                return Some(format!("use {real} as {jux_name};"));
            }
        }

        // Fallback for foreign *non-type* symbols (free functions, consts) that
        // carry no `@rust` annotation: a `rust.<crate>.…` import on a foreign
        // crate other than `std`. The crate's `.jux.d` package mirrors the crate
        // root one-to-one, so dropping the reserved `rust.` prefix yields the
        // real Rust path (`rust.libc.getpid` → `use libc::getpid;`). `std` is
        // excluded — its stub flattens nested modules, so a bare strip would be
        // wrong (`rust.std.spawn` is really `std::thread::spawn`), and std types
        // are already covered by the `@rust`-annotated branch above.
        if matches!(segs.first(), Some(&"rust")) && segs.len() >= 3 && segs[1] != "std" {
            let real = segs[1..].join("::");
            return Some(match alias {
                Some(a) => format!("use {real} as {};", a.text),
                None => format!("use {real};"),
            });
        }
        None
    }

    fn emit_imports(&mut self, imports: &[ImportDecl], inside_package_mod: bool) {
        let mut emitted_any = false;
        for import in imports {
            // §G.9.2: an `import` of an external stub type (a Rust-std / crate
            // `.jux.d` type) lowers to a `use` of its REAL Rust path, recorded on
            // `ClassSig::rust_path` from the `@rust("…")` annotation — so the bare
            // type name resolves to the foreign symbol (`use
            // std::collections::HashSet;`) rather than the non-existent
            // `rust::std::HashSet`.
            if let Some(line) = self.external_use_line(&import.spec) {
                if self.emitted_uses_in_module.insert(line.clone()) {
                    self.w.line(&line);
                    emitted_any = true;
                }
                continue;
            }
            // **Intrinsic-class import suppression.** `import
            // jux.std.collections.ArrayList;` names a class whose
            // struct emission is suppressed (it lowers to Rust's `Vec`
            // via the builtin dispatch tables), so the `use` would
            // reference a symbol that doesn't exist (rustc E0432).
            // Usage doesn't need the import — the dispatch is
            // name-driven — so the whole declaration is dropped.
            // Wildcard imports of the package still emit (the module
            // exists and carries the non-intrinsic items).
            if let ImportSpec::Path { name, wildcard: false, .. } = &import.spec {
                if name.segments.len() >= 2 {
                    let pkg = name.segments[..name.segments.len() - 1]
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .join(".");
                    let last = &name.segments[name.segments.len() - 1].text;
                    if is_intrinsic_class(&pkg, last) {
                        continue;
                    }
                }
            }
            if let Some(rendered) = render_use(&import.spec, inside_package_mod) {
                // Per-module, PER-LINE dedupe: when two units in the
                // same module import the same name — even through
                // different groupings (`{add}` vs `{add, divide}`,
                // which `render_use` expands one-per-line) — we'd
                // otherwise emit `use foo;` twice and Rust rejects it
                // as E0252. The emitted-uses set is reset whenever we
                // enter or leave a `pub mod`.
                for line in rendered.split('\n') {
                    if !self.emitted_uses_in_module.insert(line.to_string()) {
                        continue;
                    }
                    self.w.line(line);
                    emitted_any = true;
                }
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
            TopLevelDecl::ExternBlock(d) => d.span,
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
            TopLevelDecl::ExternBlock(d) => &d.annotations,
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
            TopLevelDecl::ExternBlock(block) => self.emit_extern_block(block),
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
        self.emit_type_as_rust(&juxc_tycheck::resolved_const_type(decl));
        self.w.push_str(" = ");
        // An int/bool initializer that const-folds (`doubled(1024)`, `SIZE * 2`)
        // emits the computed literal — a Rust `const` can't call the emitted
        // (non-`const`) function, so we evaluate it ourselves (§T.11). Other
        // initializers (String, double, …) emit verbatim as before.
        if let Some(v) = self.try_const_int(&decl.value) {
            self.w.push_str(&v.to_string());
        } else if let Some(b) = self.try_const_bool(&decl.value) {
            self.w.push_str(if b { "true" } else { "false" });
        } else {
            self.emit_expr(&decl.value);
        }
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

    /// FQNs of every concrete, non-generic exception class — classes
    /// whose `extends` chain reaches the exception root (`Exception`
    /// itself included) — for the uncaught-exception reporter appended
    /// by [`Self::finish`]. Filtered out:
    ///
    /// - **Generic classes** (`class E<T> extends Exception`): a
    ///   `downcast_ref` arm needs a concrete type, and instantiations
    ///   can't be enumerated here.
    /// - **Wrapper-shape classes**: they can't be thrown in the first
    ///   place (their `Rc` payload isn't `Send`, which `panic_any`
    ///   requires) and their emitted struct has no direct
    ///   `getMessage`.
    ///
    /// Sorted for deterministic emission.
    fn throwable_class_fqns(&self) -> Vec<String> {
        let root: Option<String> =
            if self.symbols.classes.contains_key("jux.std.exceptions.Exception") {
                Some("jux.std.exceptions.Exception".to_string())
            } else {
                self.symbols.find_fqn_by_bare("Exception")
            };
        let Some(root) = root else {
            return Vec::new();
        };
        let mut out: Vec<String> = self
            .symbols
            .classes
            .iter()
            .filter(|(fqn, sig)| {
                if !sig.generic_params.is_empty() {
                    return false;
                }
                if self.wrapper_classes.contains(backend_fqn::fqn_bare(fqn)) {
                    return false;
                }
                if **fqn == root {
                    return true;
                }
                // Walk the extends chain up toward the root (same
                // bounded walk as `catch_subclass_fqns`).
                let mut cur = sig.extends_fqn.clone();
                let mut depth = 0usize;
                while let Some(p) = cur {
                    if depth > 64 {
                        return false;
                    }
                    depth += 1;
                    if p == root {
                        return true;
                    }
                    cur = self.symbols.classes.get(&p).and_then(|c| c.extends_fqn.clone());
                }
                false
            })
            .map(|(fqn, _)| fqn.clone())
            .collect();
        out.sort();
        out
    }

    /// Wrap up: build the Cargo.toml and bundle with the emitted source.
    ///
    /// **Async detection.** If the emitted text contains `async fn` (or
    /// the unit-return `async fn name(...) -> ()` shape), the user's
    /// program uses async, and the emitted crate needs an executor +
    /// `join!`/`join_all` helpers. `futures` (mature, std-friendly,
    /// zero-runtime startup cost) is added as a dependency so a Jux
    /// `main()` can call `futures::executor::block_on(...)` and
    /// `futures::future::join_all(...)`. The detection is a simple
    /// substring scan — no false positives in well-formed emitted
    /// Rust (juxc never produces a literal `async fn` in any other
    /// context).
    fn finish(mut self) -> RustCrate {
        let split = self.split_files.take();
        // Computed before `self.w` is moved out below (the borrow
        // checker won't allow a method call on a partially-moved
        // `self`).
        let throwable_fqns = self.throwable_class_fqns();
        let mut source = self.w.into_string();
        // In split mode, `async fn` / `panic_any` / `catch_unwind` may live in
        // any per-unit body file, so the Cargo-feature and panic-hook decisions
        // must scan EVERY file, not just `main.rs`.
        let split_text: String = split
            .as_ref()
            .map(|fs| fs.iter().map(|(_, c)| c.as_str()).collect())
            .unwrap_or_default();
        let uses_async =
            source.contains("async fn ") || split_text.contains("async fn ");
        // **Silent panic hook for try/throw programs.** When the
        // user's code throws and catches typed exceptions, the
        // emitted `panic_any` triggers the default Rust panic
        // hook on every throw — printing "thread 'main' panicked
        // at … Box<dyn Any>" to stderr even when `catch_unwind`
        // immediately recovers. This noise drowns out real output
        // for any program that uses try/catch as an error-flow
        // primitive (which Jux programs do by design, since the
        // language doesn't have Result<T, E> yet).
        //
        // Detection: a substring scan for `panic_any` or
        // `catch_unwind` in the emitted source. When present, the
        // entry point gains a quiet panic hook plus an
        // uncaught-exception reporter (see below).
        let uses_panics = source.contains("panic_any")
            || source.contains("catch_unwind")
            || split_text.contains("panic_any")
            || split_text.contains("catch_unwind");
        if uses_panics {
            // Two reporting layers, installed by renaming the real
            // entry point to `__jux_user_main` and appending a fresh
            // `fn main()` wrapper:
            //
            // - The HOOK prints &str / String payloads — RUNTIME
            //   PANICS (assert failures §S.7.2, index bounds), which
            //   no Jux `catch` clause can match, so printing at throw
            //   time is printing at the top. It stays quiet for typed
            //   payloads: the hook fires on every throw, caught or
            //   not, and can't know whether a `catch` downstream will
            //   absorb the exception. (Fully-qualified
            //   `::std::boxed::Box` so a user-declared `class Box`
            //   doesn't shadow std's at the `set_hook` call site.)
            // - The `catch_unwind` around `__jux_user_main` reports
            //   typed payloads that actually ESCAPED — a
            //   thrown-but-uncaught Jux exception — with the
            //   Java-style `Exception in thread "main" <fqn>:
            //   <message>` line, one downcast arm per known
            //   non-generic exception class (generic exception
            //   instantiations can't be enumerated; they exit with
            //   code 101 unreported). Exit code 101 matches the bare
            //   panic exit so scripts see no difference.
            //
            // Rename only the COLUMN-0 entry point. A packaged user
            // `main` (`pub mod arithex { pub fn main() … }`) is
            // indented in single-file mode and lives in a split file
            // in split mode, so anchoring on the preceding newline
            // leaves it alone — the shim's `arithex::main()` call
            // keeps resolving. Both the bare-shim (`fn main`) and the
            // crate-root user-main (`pub fn main`) shapes qualify.
            let mut renamed = false;
            for (from, to) in [
                ("\nfn main() {\n", "\nfn __jux_user_main() {\n"),
                ("\npub fn main() {\n", "\npub fn __jux_user_main() {\n"),
            ] {
                if source.contains(from) {
                    source = source.replace(from, to);
                    renamed = true;
                    break;
                }
            }
            // When no recognized entry shape exists (e.g. a
            // value-returning `fn main() -> isize` shim, or a library
            // crate with no main at all), append nothing — a second
            // `fn main` would not compile.
            if renamed {
                let mut wrapper = String::from(concat!(
                    "\nfn main() {\n",
                    "    std::panic::set_hook(::std::boxed::Box::new(|__jux_info| {\n",
                    "        let __jux_p = __jux_info.payload();\n",
                    "        if let Some(__jux_s) = __jux_p.downcast_ref::<&str>() {\n",
                    "            eprintln!(\"panic: {__jux_s}\");\n",
                    "        } else if let Some(__jux_s) = __jux_p.downcast_ref::<String>() {\n",
                    "            eprintln!(\"panic: {__jux_s}\");\n",
                    "        }\n",
                    "    }));\n",
                    "    if let Err(__jux_p) = std::panic::catch_unwind(::std::panic::AssertUnwindSafe(__jux_user_main)) {\n",
                ));
                for fqn in &throwable_fqns {
                    let path = match backend_fqn::fqn_package(&fqn) {
                        Some(pkg) => format!(
                            "crate::{}::{}",
                            pkg.split('.').collect::<Vec<_>>().join("::"),
                            backend_fqn::fqn_bare(&fqn),
                        ),
                        // No-package classes sit at the crate root;
                        // the wrapper also lives in `main.rs`, so the
                        // bare name resolves in both single-file and
                        // split modes (split keeps root-tier units in
                        // main.rs).
                        None => backend_fqn::fqn_bare(&fqn).to_string(),
                    };
                    wrapper.push_str(&format!(
                        concat!(
                            "        if let Some(__jux_e) = __jux_p.downcast_ref::<{path}>() {{\n",
                            "            eprintln!(\"Exception in thread \\\"main\\\" {fqn}: {{}}\", __jux_e.getMessage());\n",
                            "        }}\n",
                        ),
                        path = path,
                        fqn = fqn,
                    ));
                }
                wrapper.push_str(concat!(
                    "        std::process::exit(101);\n",
                    "    }\n",
                    "}\n",
                ));
                source.push_str(&wrapper);
            }
        }
        // `main.rs` first, then every per-unit body + `mod.rs` (split mode); the
        // hook splice above only touched `main.rs`, which is where `fn main`
        // lives. Single-file mode leaves `split` as `None` → just `main.rs`.
        let mut sources = vec![("src/main.rs".to_string(), source)];
        if let Some(files) = split {
            sources.extend(files);
        }
        RustCrate {
            cargo_toml: cargo_toml_for_with(CRATE_NAME, uses_async),
            sources,
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
            // Grouped imports expand to ONE `use` line per name (not
            // `use p::{a, b};`): the per-module dedupe is line-keyed,
            // so two root-tier units importing the same symbol through
            // DIFFERENT groupings (`{add}` vs `{add, divide}`) must
            // land on identical lines or rustc rejects the second as
            // E0252.
            let lines = items
                .iter()
                .map(|it| match &it.alias {
                    Some(a) => {
                        format!("use {root}{path}::{} as {};", it.name.text, a.text)
                    }
                    None => format!("use {root}{path}::{};", it.name.text),
                })
                .collect::<Vec<_>>()
                .join("\n");
            Some(lines)
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
    cargo_toml_for_with(name, false)
}

/// Variant of [`cargo_toml_for`] that includes async-runtime
/// dependencies when `uses_async` is true.
///
/// The runtime is `futures` (BSD-licensed, no `Send` requirement
/// on user futures, zero startup overhead). It provides:
///
/// - `futures::executor::block_on` — drive a future to completion
///   from a synchronous context (e.g. `fn main()`).
/// - `futures::future::join_all` — fan out a `Vec<F>` of futures
///   and await them all *concurrently* (single-threaded
///   cooperative interleaving). This is the Phase-1 stand-in for
///   the spec's `parallel(...)` builtin.
/// - `futures::join!` — variadic concurrent-await over a fixed
///   set of futures.
///
/// Pinned to a `1` semver range so behavior is reproducible.
pub fn cargo_toml_for_with(name: &str, uses_async: bool) -> String {
    // Delegate to the metadata-aware emitter with an empty `CargoMeta`.
    // An empty meta carries no version override (so the historical
    // `version = "0.0.0"` default is used), no resource fields, and no
    // icon — which means no `build = "build.rs"` line and no
    // `[build-dependencies]` are emitted. The output is byte-for-byte
    // identical to the legacy template, so the no-manifest path
    // (loose `.jux` files, the example corpus) is unchanged.
    cargo_toml_for_with_meta(name, uses_async, &CargoMeta::default())
}

/// Binary-resource / package metadata threaded from a project's
/// `jux.toml` `[package]` table into the emitted `Cargo.toml`.
///
/// Every field is optional. When all are absent (the [`CargoMeta::default`]
/// shape), [`cargo_toml_for_with_meta`] emits exactly the legacy template:
/// `version = "0.0.0"`, no resource keys, no build script. As soon as any
/// version-info field *or* an icon is present, the emitter additionally
/// wires `build = "build.rs"` plus a `[build-dependencies]` entry for the
/// Windows-resource compiler, and the driver writes the matching
/// `build.rs`/`app.ico` into the crate dir.
#[derive(Debug, Default, Clone)]
pub struct CargoMeta {
    /// SemVer string for `[package] version`. When `None`, the emitter
    /// falls back to `"0.0.0"` (the historical default).
    pub version: Option<String>,
    /// `authors = [...]` list. Empty → key omitted.
    pub authors: Vec<String>,
    /// One-line `description`. Doubles as the `FileDescription`
    /// version-info resource in the generated build script.
    pub description: Option<String>,
    /// SPDX `license` identifier.
    pub license: Option<String>,
    /// Project `homepage` URL.
    pub homepage: Option<String>,
    /// Source `repository` URL.
    pub repository: Option<String>,
    /// `CompanyName` version-info resource (Windows). Not a Cargo key;
    /// only flows into `build.rs`. Defaults to the joined authors there.
    pub company: Option<String>,
    /// `LegalCopyright` version-info resource (Windows).
    pub copyright: Option<String>,
    /// Whether an executable icon was supplied. The icon file itself is
    /// copied into the crate dir by the driver as `app.ico`; this flag
    /// only tells the emitter to wire the build script + dependency.
    pub has_icon: bool,
    /// Cargo build profiles to emit (`[profile.dev]`, `[profile.release]`,
    /// …) — the lowered form of the manifest's `[profile.*]` tables
    /// (§B.9). Empty → no profile tables emitted (Cargo's built-in
    /// defaults apply). The driver does the Jux→Cargo key translation;
    /// the emitter just renders each block verbatim.
    pub profiles: Vec<CargoProfile>,
    /// Native-library link config from the manifest's `[ffi.*]` tables
    /// (§B.14), emitted as `cargo:rustc-link-*` directives in the generated
    /// `build.rs`. Empty → no FFI linking. Drives `needs_build_script` on every
    /// platform (linking is not Windows-gated like the resource block).
    pub ffi: Vec<FfiLink>,
}

/// One native library to link for the C FFI (§B.14), lowered from an
/// `[ffi.<name>]` manifest table. Rendered into the generated `build.rs` as
/// `cargo:rustc-link-search` + `cargo:rustc-link-lib` directives.
#[derive(Debug, Clone, Default)]
pub struct FfiLink {
    /// The library name to link (drops any `lib` prefix / extension).
    pub lib: String,
    /// Explicit Cargo link kind (`"static"` / `"framework"`) or `None` for the
    /// default dynamic linkage.
    pub kind: Option<String>,
    /// `cargo:rustc-link-search=native=<path>` directories.
    pub search_paths: Vec<String>,
    /// Transitive libraries to also link (`cargo:rustc-link-lib=<name>`).
    pub extra_libs: Vec<String>,
}

/// One Cargo `[profile.<name>]` table to emit into a generated `Cargo.toml`.
///
/// `entries` are already Cargo-spelled `key` → rendered-TOML-`value` pairs
/// (e.g. `("opt-level", "3")`, `("lto", "\"thin\"")`, `("strip", "true")`),
/// so the emitter can write each line without knowing TOML value types. The
/// driver ([`crate`]'s consumer) owns the spec→Cargo translation and value
/// rendering (quoting strings, leaving integers/bools bare).
#[derive(Debug, Clone, Default)]
pub struct CargoProfile {
    /// Profile name: a Cargo built-in (`dev`/`release`/`test`/`bench`) or a
    /// custom name. Custom names must carry an `inherits` entry (the driver
    /// guarantees this).
    pub name: String,
    /// `(cargo_key, rendered_value)` pairs, already valid TOML on the value
    /// side. Emitted in order, one `key = value` line each.
    pub entries: Vec<(String, String)>,
}

impl CargoMeta {
    /// True when any field that requires a Windows-resource build script
    /// is set: an icon, or any version-info resource (version, company,
    /// copyright, description). Authors/license/homepage/repository alone
    /// are plain Cargo manifest keys and do **not** force a `build.rs`.
    pub fn needs_build_script(&self) -> bool {
        self.has_icon
            || self.version.is_some()
            || self.company.is_some()
            || self.copyright.is_some()
            || self.description.is_some()
            || !self.authors.is_empty()
            // FFI link directives also require a build.rs (on every platform).
            || !self.ffi.is_empty()
    }
}

/// Metadata-aware variant of [`cargo_toml_for_with`].
///
/// Emits the `[package]` table using `meta.version` (default `"0.0.0"`)
/// and adds `authors`/`description`/`license`/`homepage`/`repository`
/// keys when present. When [`CargoMeta::needs_build_script`] holds, it
/// also emits `build = "build.rs"` and a `[build-dependencies]` block
/// pulling in the Windows-resource compiler crate — `winresource`
/// (winres's maintained fork). The driver is responsible for writing the
/// matching `build.rs` and copying the icon next to it.
///
/// `escape_toml` is used on every interpolated value so quotes/backslashes
/// in user metadata can't corrupt the emitted manifest.
pub fn cargo_toml_for_with_meta(name: &str, uses_async: bool, meta: &CargoMeta) -> String {
    let version = meta.version.as_deref().unwrap_or("0.0.0");
    let mut pkg = format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"{version}\"\n\
         edition = \"2021\"\n\
         publish = false\n",
        version = escape_toml(version),
    );
    if !meta.authors.is_empty() {
        let list = meta
            .authors
            .iter()
            .map(|a| format!("\"{}\"", escape_toml(a)))
            .collect::<Vec<_>>()
            .join(", ");
        pkg.push_str(&format!("authors = [{list}]\n"));
    }
    if let Some(desc) = &meta.description {
        pkg.push_str(&format!("description = \"{}\"\n", escape_toml(desc)));
    }
    if let Some(lic) = &meta.license {
        pkg.push_str(&format!("license = \"{}\"\n", escape_toml(lic)));
    }
    if let Some(hp) = &meta.homepage {
        pkg.push_str(&format!("homepage = \"{}\"\n", escape_toml(hp)));
    }
    if let Some(repo) = &meta.repository {
        pkg.push_str(&format!("repository = \"{}\"\n", escape_toml(repo)));
    }
    // A Windows-resource build script is wired in only when there's
    // actually metadata or an icon to embed; loose-file builds keep the
    // clean, dependency-free manifest.
    if meta.needs_build_script() {
        pkg.push_str("build = \"build.rs\"\n");
    }

    let deps = if uses_async {
        "[dependencies]\nfutures = { version = \"0.3\", features = [\"thread-pool\"] }\n\n"
    } else {
        ""
    };

    // The Windows-resource compiler dependency. `winresource` is the
    // maintained fork of `winres`; it is a build-dependency only and a
    // no-op when the target OS isn't Windows (our generated `build.rs`
    // additionally gates on `CARGO_CFG_TARGET_OS`).
    let build_deps = if meta.needs_build_script() {
        "[build-dependencies]\nwinresource = \"0.1\"\n\n"
    } else {
        ""
    };

    let profiles = render_profiles(&meta.profiles);
    format!(
        "{pkg}\
         \n\
         {deps}\
         {build_deps}\
         [[bin]]\n\
         name = \"{name}\"\n\
         path = \"src/main.rs\"\n\
         \n\
         [workspace]\n\
         {profiles}",
    )
}

/// Describes the target shape of an emitted crate: a single binary, or a
/// library with a chosen `crate-type`. Drives [`cargo_toml_for_target`].
///
/// This is the Phase-1 manifest-driven extension: a `[[bin]] name="myapp"`
/// project emits a `[[bin]]` whose `name` is literally `myapp`, and a
/// `[lib]` project emits `[lib]` with the requested `crate-type`.
#[derive(Debug, Clone)]
pub enum CrateTarget {
    /// A single executable. The crate's source root is `src/main.rs` and
    /// the produced binary is named `name`.
    Bin {
        /// Binary (and produced-file) name.
        name: String,
    },
    /// A library. The crate's source root is `src/lib.rs`. `crate_type` is
    /// the Cargo `crate-type` list (e.g. `["lib"]`, `["cdylib"]`); empty
    /// means the Cargo default (`["lib"]`).
    Lib {
        /// Library crate name (the `[package] name`).
        name: String,
        /// Cargo `crate-type` list. Empty → omitted (Cargo default `lib`).
        crate_type: Vec<String>,
    },
}

/// A single path-dependency line for the emitted `[dependencies]` table:
/// a crate name and the relative path to the sibling emitted crate.
#[derive(Debug, Clone)]
pub struct PathDep {
    /// The Rust crate name the dependency was emitted as.
    pub crate_name: String,
    /// Relative path (from this crate's dir) to the dependency crate dir.
    pub rel_path: String,
}

/// A single registry-dependency line for the emitted `[dependencies]` table:
/// a published crate name and a version requirement (`serde_json = "1.0"`).
///
/// These come from a package's foreign (`rust.<crate>`) `[dependencies]` and
/// are what actually *link* the bound crate into the emitted Rust binary —
/// the `.jux.d` stub only puts the crate's API in scope at type-check time.
#[derive(Debug, Clone)]
pub struct RegistryDep {
    /// The published crate name as written after `rust.` (`serde_json`).
    pub crate_name: String,
    /// The version requirement string (`"1.0"`, `"0.27"`); `"*"` when the
    /// manifest left it unspecified.
    pub version: String,
}

/// Build the `Cargo.toml` for an emitted crate of a given [`CrateTarget`],
/// with optional package metadata and path-dependencies.
///
/// This is the manifest-driven successor to [`cargo_toml_for_with_meta`]:
///
/// - [`CrateTarget::Bin`] emits a `[[bin]]` whose `name` is the requested
///   binary name and whose `path` is `src/main.rs`.
/// - [`CrateTarget::Lib`] emits a `[lib]` block (`path = "src/lib.rs"`,
///   plus `crate-type` when non-empty). No `[[bin]]` is emitted.
///
/// `path_deps` produces `name = { path = "..." }` lines under
/// `[dependencies]`, used by workspace path-dependencies so an emitted
/// `app` crate links against an emitted `greeter` crate.
///
/// `in_workspace` controls the trailing `[workspace]` opt-out table:
/// stand-alone emitted crates need the empty `[workspace]` to escape any
/// enclosing Cargo workspace, but a member of an *emitted* Cargo workspace
/// must NOT carry its own `[workspace]` (cargo rejects nested workspaces).
#[allow(clippy::too_many_arguments)]
pub fn cargo_toml_for_target(
    target: &CrateTarget,
    uses_async: bool,
    meta: &CargoMeta,
    path_deps: &[PathDep],
    registry_deps: &[RegistryDep],
    in_workspace: bool,
) -> String {
    let pkg_name = match target {
        CrateTarget::Bin { name } => name,
        CrateTarget::Lib { name, .. } => name,
    };
    let version = meta.version.as_deref().unwrap_or("0.0.0");
    let mut pkg = format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"{version}\"\n\
         edition = \"2021\"\n\
         publish = false\n",
        name = escape_toml(pkg_name),
        version = escape_toml(version),
    );
    if !meta.authors.is_empty() {
        let list = meta
            .authors
            .iter()
            .map(|a| format!("\"{}\"", escape_toml(a)))
            .collect::<Vec<_>>()
            .join(", ");
        pkg.push_str(&format!("authors = [{list}]\n"));
    }
    if let Some(desc) = &meta.description {
        pkg.push_str(&format!("description = \"{}\"\n", escape_toml(desc)));
    }
    if let Some(lic) = &meta.license {
        pkg.push_str(&format!("license = \"{}\"\n", escape_toml(lic)));
    }
    if let Some(hp) = &meta.homepage {
        pkg.push_str(&format!("homepage = \"{}\"\n", escape_toml(hp)));
    }
    if let Some(repo) = &meta.repository {
        pkg.push_str(&format!("repository = \"{}\"\n", escape_toml(repo)));
    }
    if meta.needs_build_script() {
        pkg.push_str("build = \"build.rs\"\n");
    }

    // [dependencies] — futures (when async is used), foreign registry crates
    // (`rust.<crate>` deps, which actually link the bound crate in), and any
    // sibling path deps.
    let mut deps = String::new();
    if uses_async || !path_deps.is_empty() || !registry_deps.is_empty() {
        deps.push_str("[dependencies]\n");
        if uses_async {
            deps.push_str("futures = { version = \"0.3\", features = [\"thread-pool\"] }\n");
        }
        for d in registry_deps {
            deps.push_str(&format!(
                "{} = \"{}\"\n",
                d.crate_name,
                escape_toml(&d.version),
            ));
        }
        for d in path_deps {
            deps.push_str(&format!(
                "{} = {{ path = \"{}\" }}\n",
                d.crate_name,
                escape_toml(&d.rel_path),
            ));
        }
        deps.push('\n');
    }

    let build_deps = if meta.needs_build_script() {
        "[build-dependencies]\nwinresource = \"0.1\"\n\n"
    } else {
        ""
    };

    // The target block: [[bin]] or [lib].
    let target_block = match target {
        CrateTarget::Bin { name } => format!(
            "[[bin]]\nname = \"{}\"\npath = \"src/main.rs\"\n\n",
            escape_toml(name),
        ),
        CrateTarget::Lib { crate_type, .. } => {
            let mut b = String::from("[lib]\npath = \"src/lib.rs\"\n");
            if !crate_type.is_empty() {
                let list = crate_type
                    .iter()
                    .map(|t| format!("\"{}\"", escape_toml(t)))
                    .collect::<Vec<_>>()
                    .join(", ");
                b.push_str(&format!("crate-type = [{list}]\n"));
            }
            b.push('\n');
            b
        }
    };

    // A stand-alone emitted crate needs the empty `[workspace]` opt-out so
    // cargo doesn't try to attach it to an enclosing workspace. A member of
    // an emitted workspace must omit it.
    let workspace_tail = if in_workspace { "" } else { "[workspace]\n" };

    // `[profile.*]` tables (§B.9). Cargo honors these only in a package
    // that is its own workspace root, so they're emitted for stand-alone
    // crates (the common single-package path). Members of an emitted
    // workspace carry their profiles in the workspace-root manifest instead,
    // so `in_workspace` crates skip them here.
    let profiles = if in_workspace {
        String::new()
    } else {
        render_profiles(&meta.profiles)
    };

    format!("{pkg}\n{deps}{build_deps}{target_block}{workspace_tail}{profiles}")
}

/// Escape a string for safe inclusion inside a double-quoted TOML basic
/// string: backslashes and double-quotes are the only characters that can
/// break out of the literal. Newlines/control characters aren't expected
/// in manifest metadata, so we keep this minimal.
fn escape_toml(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Tier-0 release-profile defaults (`optimizations.md`): the "cheap, immediate,
/// large" build-speed win. Emitted into every STAND-ALONE crate's
/// `[profile.release]` for each key the user hasn't pinned, so a `--release`
/// build is optimized out of the box (debug builds ignore the table).
///
/// `panic` is deliberately NOT set here: Jux `try/catch` lowers to
/// `panic_any` + `catch_unwind`, and `panic = "abort"` would break catch
/// recovery. A user who wants `abort` opts in via their own `[profile.release]`.
fn tier0_release_defaults() -> [(&'static str, &'static str); 5] {
    [
        ("opt-level", "3"),
        ("lto", "\"thin\""),
        ("codegen-units", "1"),
        ("strip", "\"symbols\""),
        ("overflow-checks", "false"),
    ]
}

/// Ensure `profiles` carries a `release` profile pre-filled with the Tier-0
/// defaults, WITHOUT overriding any key the user already set (per-key override
/// wins). Returns the effective profile list to render.
fn with_tier0_release_defaults(profiles: &[CargoProfile]) -> Vec<CargoProfile> {
    let mut out = profiles.to_vec();
    let release = match out.iter_mut().find(|p| p.name == "release") {
        Some(p) => p,
        None => {
            out.push(CargoProfile {
                name: "release".to_string(),
                entries: Vec::new(),
            });
            out.last_mut().unwrap()
        }
    };
    for (k, v) in tier0_release_defaults() {
        if !release.entries.iter().any(|(ek, _)| ek == k) {
            release.entries.push((k.to_string(), v.to_string()));
        }
    }
    out
}

/// Render the `[profile.<name>]` tables (§B.9) for a generated `Cargo.toml`.
///
/// The Tier-0 release defaults are injected first (see
/// [`with_tier0_release_defaults`]), so a stand-alone crate always gets an
/// optimized `[profile.release]`. Each [`CargoProfile`] becomes a
/// `[profile.<name>]` header followed by its already-rendered `key = value`
/// entries. A trailing blank line separates the profile block from whatever
/// follows. Only ever called for stand-alone crates — workspace members carry
/// profiles in the workspace root manifest, so the caller skips this for them.
fn render_profiles(profiles: &[CargoProfile]) -> String {
    let profiles = with_tier0_release_defaults(profiles);
    // Lead with a blank line so the profile block reads cleanly after the
    // preceding table (`[workspace]` / `[[bin]]`).
    let mut out = String::from("\n");
    for p in &profiles {
        out.push_str(&format!("[profile.{}]\n", p.name));
        for (k, v) in &p.entries {
            out.push_str(&format!("{k} = {v}\n"));
        }
        out.push('\n');
    }
    out
}
