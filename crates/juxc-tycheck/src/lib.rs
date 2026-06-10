//! Phases 6–9 — type checking, generic inference, overload resolution.
//!
//! Per `JUX-COMPILER-PIPELINE-ADDENDUM.md` §C.3: this is the largest phase
//! block. The reference algorithms live in `JUX-TYPE-SYSTEM-ADDENDUM.md`.
//!
//! ## Phased rollout
//!
//! The full type checker lands across multiple turns:
//!
//! - **Phase A — symbol table** ✅. [`symbol_table::build`] walks the
//!   compilation unit and produces a [`symbol_table::SymbolTable`]
//!   indexing every top-level declaration (classes, records, enums,
//!   interfaces, functions) plus their members.
//! - **Phase B — local type environment** ✅. [`TypeEnv`] is a scope
//!   stack threaded through every walker.
//! - **Phase C — expression typing** ✅. [`infer::infer_expr`] returns
//!   a bottom-up [`Ty`] without producing diagnostics.
//! - **Phase D — statement type checking** ✅. [`check::Checker`]
//!   walks every function/method/constructor body and emits
//!   `E0410`–`E0413` for return / assign / arg / field / method
//!   mismatches.
//! - **Phase E — method resolution** ✅ (this turn). Three threads:
//!   inheritance-aware method/field lookup via
//!   [`symbol_table::SymbolTable::lookup_method`] /
//!   [`symbol_table::SymbolTable::lookup_field`], generic-parameter
//!   substitution at use sites via [`ty::substitute`], and `super(...)`
//!   constructor arg-checking against the parent's signature.
//! - **Phase F — diagnostic types (type-mismatch, unresolved-name)** ✅
//!   (folded into Phase D). E0410/E0411/E0412/E0413 all live in
//!   [`check::Checker`].
//! - Phase G — generic inference at call sites (`identity(42)` →
//!   `<int>`), bounded-generic method dispatch, smart-cast narrowing,
//!   exhaustiveness on `switch`.
//!
//! Today the `main`-signature check from milestone 1 still runs alongside
//! the new symbol-table build.

use std::collections::HashMap;

use juxc_ast::{CompilationUnit, FnDecl, FnModifier, Param, ReturnType, TopLevelDecl, TypeRef};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

pub mod check;
pub mod env;
pub mod expand;
pub mod infer;
pub mod symbol_table;
pub mod ty;

pub use env::TypeEnv;
pub use infer::{infer_block, infer_expr};

/// The language **build profile** (async addendum §18.1.11). Selects the async
/// runtime — or, for [`Profile::Core`], forbids `async` entirely. Set from the
/// manifest's `[build] profile`; defaults to [`Profile::Full`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// `jux-full` — full event loop + worker pool. Async available.
    #[default]
    Full,
    /// `jux-embedded` — single-threaded executor, no workers. Async available.
    Embedded,
    /// `jux-core` — no async runtime; declaring `async` is `E0701`.
    Core,
}

impl Profile {
    /// Parse a manifest `[build] profile` value (`"full"` / `"embedded"` /
    /// `"core"`, case-insensitive). Unknown values fall back to [`Profile::Full`].
    pub fn from_manifest_str(s: &str) -> Profile {
        match s.trim().to_ascii_lowercase().as_str() {
            "embedded" => Profile::Embedded,
            "core" => Profile::Core,
            _ => Profile::Full,
        }
    }
}

/// Emit `E0701` for every `async` declaration when the profile has no async
/// runtime ([`Profile::Core`]). Per §18.1.11 the core profile has no event
/// loop, so an `async` function/method can't run — the fix is a `Result`/
/// state-machine rewrite (§16.7). A no-op for `Full` / `Embedded`. External
/// (`.jux.d`) units are skipped, and each diagnostic is tagged with its unit
/// index (parallel to the driver's `sources`) so the LSP routes it correctly.
pub fn check_async_profile(units: &[CompilationUnit], profile: Profile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if profile != Profile::Core {
        return out;
    }
    for (idx, unit) in units.iter().enumerate() {
        if unit.is_external {
            continue;
        }
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(f) => flag_async_decl(f, idx, &mut out),
                TopLevelDecl::Class(c) => {
                    c.methods.iter().for_each(|m| flag_async_decl(m, idx, &mut out))
                }
                TopLevelDecl::Interface(i) => {
                    i.methods.iter().for_each(|m| flag_async_decl(m, idx, &mut out))
                }
                TopLevelDecl::Record(r) => {
                    r.methods.iter().for_each(|m| flag_async_decl(m, idx, &mut out))
                }
                // Enums carry only variants + operator overloads (no plain
                // methods), so there's no `async` declaration to flag there.
                _ => {}
            }
        }
    }
    out
}

/// Flag one declaration with `E0701` when it is `async` (encoded as an
/// `async T` return or the `async` modifier).
fn flag_async_decl(f: &FnDecl, file_idx: usize, out: &mut Vec<Diagnostic>) {
    let is_async = matches!(f.return_type, ReturnType::AsyncType(_))
        || f.modifiers.iter().any(|m| matches!(m, FnModifier::Async));
    if is_async {
        out.push(
            Diagnostic::error(
                code::Code::E0701_AsyncNotInProfile,
                format!(
                    "`async` is unavailable in the `jux-core` profile: `{}` cannot be async",
                    f.name.text,
                ),
            )
            .with_span(f.span)
            .with_file(file_idx)
            .with_help(
                "the core profile has no async runtime — rewrite with `Result<T, E>` and an \
                 explicit state machine (§16.7), or build under the `full`/`embedded` profile",
            ),
        );
    }
}
pub use symbol_table::SymbolTable;
pub use ty::{ty_from_ref_in_env, ArrayKind, Primitive, Ty};

/// Resolve a field's type the way the symbol table does: the written type if
/// present, otherwise inferred from its (literal) initializer. The backend
/// calls this for fields that omit their type (`const I = 2;`), so its emitted
/// Rust uses the inferred type. Mirrors the resolution recorded in `FieldSig`.
pub fn resolved_field_type(field: &juxc_ast::FieldDecl) -> juxc_ast::TypeRef {
    symbol_table::resolve_decl_type(field.ty.as_ref(), field.default.as_ref(), field.span)
}

/// Resolve a top-level constant's type — the written type, or one inferred
/// from its initializer (`const PI = 3.14;` → `double`).
pub fn resolved_const_type(decl: &juxc_ast::ConstDecl) -> juxc_ast::TypeRef {
    symbol_table::resolve_decl_type(decl.ty.as_ref(), Some(&decl.value), decl.span)
}

/// One parameter slot of a call-sugar expansion plan — where the
/// value for that slot comes from after named arguments are mapped
/// and omitted defaults filled (§T.3.2 / §S.1.3).
#[derive(Debug, Clone)]
pub enum ArgSource {
    /// The value is the call's `args[i]` (positional, or named and
    /// re-ordered into this slot).
    Explicit(usize),
    /// The slot was omitted; the value is this clone of the
    /// parameter's declared default expression, evaluated at the
    /// call site (fresh per call — §S.1.3's no-shared-default rule).
    Default(juxc_ast::Expr),
    /// The slot is a variadic parameter (`T... name`): the listed
    /// call args (in source order) are packed into a synthesized
    /// array literal of the parameter's element type (§E.1.2.1).
    Variadic {
        /// The varargs element type `T` (the declared array type
        /// with the array shape stripped).
        element_type: juxc_ast::TypeRef,
        /// Indices into the call's `args` to pack, in order.
        indices: Vec<usize>,
    },
}

/// Output of [`typecheck`]. Empty `diagnostics` means everything checked.
pub struct TypeCheckResult {
    /// Type-check diagnostics (E0323_… and friends).
    pub diagnostics: Vec<Diagnostic>,
    /// Symbol table built during this pass. Always populated, even on
    /// error — downstream phases (resolver, backend) can read partial
    /// signatures when some declarations were rejected as duplicates.
    pub symbols: SymbolTable,
    /// Per-expression inferred type, keyed by the expression's [`Span`].
    /// Populated by the Phase D checker as it walks every function /
    /// method / constructor body. Downstream phases (notably the Rust
    /// backend) consult this map instead of running their own pre-pass
    /// heuristics. Expressions tycheck didn't visit (e.g. because an
    /// earlier error short-circuited the walk) won't have entries here;
    /// callers should fall back conservatively when a lookup misses.
    pub expr_types: HashMap<Span, Ty>,
    /// Call-sugar expansion plans (named arguments / omitted
    /// defaults), keyed by call span. The driver hands these to
    /// [`expand::apply_call_expansions`] to rewrite the AST into
    /// plain positional calls before the backend runs.
    pub call_expansions: HashMap<Span, Vec<ArgSource>>,
}

impl TypeCheckResult {
    /// Lookup the inferred type of an expression by its source span.
    /// Returns `None` when the type wasn't recorded (e.g. tycheck didn't
    /// visit it because the program had an earlier error, or the
    /// expression carries `Span::DUMMY`).
    pub fn type_of(&self, span: Span) -> Option<&Ty> {
        self.expr_types.get(&span)
    }
}

/// Type-check a compilation unit. Always returns a [`TypeCheckResult`];
/// never panics.
pub fn typecheck(unit: &CompilationUnit) -> TypeCheckResult {
    typecheck_workspace(std::slice::from_ref(unit))
}

/// Multi-unit variant of [`typecheck`]. Every unit contributes its
/// top-level declarations to one shared [`SymbolTable`], then each
/// unit is walked against that merged view so cross-file references
/// resolve.
///
/// Diagnostics from every unit are concatenated in input order. The
/// returned `symbols` is the merged workspace table; the
/// `expr_types` map is the union of every unit's inferred-expression
/// types (keyed by span, so cross-unit overlap is impossible).
///
/// Phase-1 simplification: classes/records/enums/interfaces/functions
/// share one flat namespace across the workspace. A duplicate name in
/// two units fires `E0400_DuplicateDeclaration` against the second
/// occurrence.
pub fn typecheck_workspace(units: &[CompilationUnit]) -> TypeCheckResult {
    let mut tc = TypeChecker::new();
    // `build_workspace` may emit cross-unit symbol-table diagnostics
    // (e.g. a duplicate declaration spanning two units) that can't be
    // cleanly attributed to a single source; those stay `file: None`.
    let symbols = symbol_table::build_workspace(units, &mut tc.diagnostics);
    // The unit index here is the SAME index the driver uses for its
    // `sources` list (stdlib units first, then user units, in order).
    // We tag each unit's diagnostics with that index via a length-delta:
    // record `len()` before the unit's checks, set `.file` on everything
    // appended after.
    for (idx, unit) in units.iter().enumerate() {
        let before = tc.diagnostics.len();
        tc.check_unit(unit);
        // NOTE: nullable PRIMITIVES (`int?`, `bool?`, `<int?>` generic
        // args, …) are legal — the spec's `T?` ≡ `Option<T>` mapping
        // carries no reference-type restriction (JUX-LANG-V1's
        // `int? readByte();`, type-system §T.2's `List<Dog?>`). The
        // old `check_nullable_primitives` rejection pre-pass
        // contradicted that and was removed: `int?` lowers to
        // `Option<isize>` — `None` is a stack discriminant, so a null
        // primitive costs NO allocation (no Java-style `Integer`
        // boxing).
        for d in &mut tc.diagnostics[before..] {
            d.file = Some(idx);
        }
    }
    let mut all_expr_types = std::collections::HashMap::new();
    let mut all_call_expansions = std::collections::HashMap::new();
    let mut all_ctor_selections = std::collections::HashMap::new();
    let mut all_method_selections = std::collections::HashMap::new();
    for (idx, unit) in units.iter().enumerate() {
        let before = tc.diagnostics.len();
        let mut checker = check::Checker::new(&symbols, &mut tc.diagnostics);
        // Seed the checker's TypeEnv with the per-unit context built
        // during workspace symbol-table construction. This is how
        // `ty_from_ref` knows that a bare `Greeter` in app.jux maps
        // to `com.lib.Greeter` (or whatever the import resolved to).
        if let Some(ctx) = symbols.units.get(idx) {
            checker.seed_unit_context(&ctx.package, &ctx.unqualified);
        }
        checker.check_unit(unit);
        let (expr_types, call_expansions, ctor_selections, method_selections) =
            checker.into_maps();
        all_expr_types.extend(expr_types);
        all_call_expansions.extend(call_expansions);
        all_ctor_selections.extend(ctor_selections);
        all_method_selections.extend(method_selections);
        for d in &mut tc.diagnostics[before..] {
            d.file = Some(idx);
        }
    }
    // The checker borrows `symbols` immutably during the walk, so the
    // constructor-overload selections are absorbed into the table
    // afterward — the backend reads them from `symbols` directly.
    let mut symbols = symbols;
    symbols.ctor_selections = all_ctor_selections;
    symbols.method_selections = all_method_selections;
    TypeCheckResult {
        diagnostics: tc.diagnostics,
        symbols,
        expr_types: all_expr_types,
        call_expansions: all_call_expansions,
    }
}

// ============================================================================
// Type-checker state
// ============================================================================

/// Internal type-checker state. Currently only collects diagnostics —
/// no symbol table yet, no type environment, no inference. Those land as
/// we expand the milestone surface.
struct TypeChecker {
    diagnostics: Vec<Diagnostic>,
}

impl TypeChecker {
    fn new() -> Self {
        Self { diagnostics: Vec::new() }
    }

    /// Walk the compilation unit, performing milestone-1 checks.
    fn check_unit(&mut self, unit: &CompilationUnit) {
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(fn_decl) => {
                    if fn_decl.name.text == "main" {
                        self.check_main_signature(fn_decl);
                    }
                }
                // A class member named `main` with an entry-shaped signature
                // must be `static` (§E.1.2.2) — it has no receiver for the
                // runtime to call it on. A non-static one is an ordinary
                // method; flag the likely mistake as E0326.
                TopLevelDecl::Class(class) => {
                    for m in &class.methods {
                        if m.name.text == "main"
                            && is_entry_shaped(m)
                            && !m
                                .modifiers
                                .iter()
                                .any(|md| matches!(md, juxc_ast::FnModifier::Static))
                        {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0326_ClassMainNotStatic,
                                    format!(
                                        "class member `main` must be `static` to be an entry \
                                         point (in class `{}`)",
                                        class.name.text,
                                    ),
                                )
                                .with_span(m.span)
                                .with_help("add `static` to the `main` method"),
                            );
                        }
                    }
                }
                // Same story for enums — no top-level signature rules
                // until methods on enums land.
                TopLevelDecl::Enum(_) => {}
                // Records carry no top-level signature rules today.
                TopLevelDecl::Record(_) => {}
                // Interfaces neither — signatures only, no rules to
                // check at this milestone.
                TopLevelDecl::Interface(_) => {}
                // Type aliases — no body to walk; the target is a
                // syntactic TypeRef checked at use sites via
                // alias expansion in `ty_from_ref`.
                TopLevelDecl::TypeAlias(_) => {}
                // Top-level constants — value-vs-type check lives
                // in `check::Checker::check_unit` (it has the
                // expression-inference machinery).
                TopLevelDecl::Const(_) => {}
            }
        }
    }

    /// Per `JUX-ENTRY-POINTS-ADDENDUM.md` §E.1.2 / §E.1.3, the entry
    /// function's signature must be one of:
    ///
    /// - `public void main()`
    /// - `public void main(String[] args)`
    /// - `public int main()`
    /// - `public int main(String[] args)`
    /// - `public async void main()` — async entry per §E.1.3; the
    ///   backend auto-wraps with `futures::executor::block_on`.
    /// - `public async int main()` — same shape but with an exit code.
    ///
    /// Each may carry a `throws` clause; we don't restrict that. Visibility
    /// is *not* part of the check — the spec doesn't require any specific
    /// modifier on `main`, just that one of these shapes matches.
    fn check_main_signature(&mut self, fn_decl: &FnDecl) {
        if !is_entry_shaped(fn_decl) {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0323_MainSignatureMismatch,
                    "main's signature does not match an accepted form",
                )
                .with_span(fn_decl.span)
                .with_help(
                    "accepted forms: `void main()`, `void main(String[] args)`, \
                     `int main()`, `int main(String[] args)`",
                ),
            );
        }
    }
}

// ============================================================================
// Type-shape helpers
//
// These are intentionally narrow — they check exact name shapes against the
// type-system addendum's primitive names, not against a full type table.
// When real type checking lands, these go away.
// ============================================================================

/// Does `fn_decl` have an **entry-shaped** signature — one of `void`/`int`
/// (or their `async` forms) returning, taking no args or a single `String[]`?
/// This is the name-agnostic shape match shared by the free-`main` check and
/// the class-`main`-must-be-static check (§E.1.2).
fn is_entry_shaped(fn_decl: &FnDecl) -> bool {
    let return_ok = match &fn_decl.return_type {
        ReturnType::Void => true,
        ReturnType::Type(t) => is_int(t),
        // `async void main()` synthesizes an AsyncType whose inner TypeRef's
        // name is the sentinel "void"; treat it like `void`. `async int main()`
        // is the int-returning async entry shape.
        ReturnType::AsyncType(t) => {
            let is_void_sentinel = t.name.segments.len() == 1
                && t.name.segments[0].text == "void"
                && t.generic_args.is_empty()
                && !t.nullable;
            is_void_sentinel || is_int(t)
        }
    };
    let params_ok = match fn_decl.params.as_slice() {
        [] => true,
        [single] => is_string_array(single),
        _ => false,
    };
    return_ok && params_ok
}

/// Is `t` the type `int` (single segment, name "int", non-nullable, no generics)?
fn is_int(t: &TypeRef) -> bool {
    t.name.segments.len() == 1
        && t.name.segments[0].text == "int"
        && t.generic_args.is_empty()
        && !t.nullable
}

/// Does parameter `p` look like `String[] args`?
///
/// Milestone-1 placeholder: we accept anything for the parameter for now.
/// Array-type parsing (`Type[]`) isn't implemented in the parser yet —
/// once it is, this becomes a proper check.
fn is_string_array(_p: &Param) -> bool {
    // TODO: enforce `String[]` once array-type parsing lands. For
    // milestone 1 hello.jux has no `main(...)` params at all, so this
    // path is unreachable from the canary test.
    true
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

    /// Drive lex → parse → typecheck and return the typecheck diagnostic
    /// count.
    fn check_count(src: &str) -> usize {
        let sf = SourceFile::new("test.jux", src);
        let lex_result = lex(&sf);
        assert!(lex_result.diagnostics.is_empty(), "lex errors: {:?}", lex_result.diagnostics);
        let parse_result = parse(&lex_result.tokens);
        assert!(parse_result.diagnostics.is_empty(), "parse errors: {:?}", parse_result.diagnostics);
        typecheck(&parse_result.ast).diagnostics.len()
    }

    /// `void main()` is the canonical entry form.
    #[test]
    fn void_main_no_args_is_accepted() {
        assert_eq!(check_count("public void main() {}"), 0);
    }

    /// A non-main function with any shape is left alone.
    #[test]
    fn non_main_functions_are_not_checked() {
        assert_eq!(check_count("public bool helper() {}"), 0);
    }

    /// `bool main()` is not in the accepted set — emits E0323.
    #[test]
    fn bool_main_is_e0323() {
        assert_eq!(check_count("public bool main() {}"), 1);
    }

    /// A `static` class `main` with an entry shape is a valid entry point.
    #[test]
    fn class_static_main_is_accepted() {
        assert_eq!(check_count("public class App { public static void main() {} }"), 0);
    }

    /// A non-`static` class `main` with an entry shape fires E0326.
    #[test]
    fn class_non_static_main_is_e0326() {
        assert_eq!(check_count("public class App { public void main() {} }"), 1);
    }

    /// A class method named `main` that ISN'T entry-shaped (two params — no
    /// accepted form takes two) is an ordinary method: no E0326.
    #[test]
    fn class_non_entry_main_is_not_flagged() {
        assert_eq!(check_count("public class App { public void main(int x, int y) {} }"), 0);
    }

    // ---- async-in-profile (E0701, §18.1.11) ----

    fn parse_unit(src: &str) -> CompilationUnit {
        let sf = SourceFile::new("t.jux", src);
        let lexed = lex(&sf);
        parse(&lexed.tokens).ast
    }

    /// An `async` declaration under the `jux-core` profile is E0701.
    #[test]
    fn async_in_core_profile_is_e0701() {
        let unit = parse_unit("public async int f(){ return 1; }");
        let d = check_async_profile(std::slice::from_ref(&unit), Profile::Core);
        assert!(
            d.iter().any(|x| x.code == code::Code::E0701_AsyncNotInProfile),
            "got: {d:?}",
        );
    }

    /// The same declaration under `full` / `embedded` is fine.
    #[test]
    fn async_in_full_and_embedded_profile_ok() {
        let unit = parse_unit("public async int f(){ return 1; }");
        assert!(check_async_profile(std::slice::from_ref(&unit), Profile::Full).is_empty());
        assert!(check_async_profile(std::slice::from_ref(&unit), Profile::Embedded).is_empty());
    }

    /// A non-async declaration is never flagged, even in `core`.
    #[test]
    fn sync_decl_in_core_profile_ok() {
        let unit = parse_unit("public int f(){ return 1; }");
        assert!(check_async_profile(std::slice::from_ref(&unit), Profile::Core).is_empty());
    }

    /// The profile string parser is case-insensitive and defaults to `full`.
    #[test]
    fn profile_parses_from_manifest_str() {
        assert_eq!(Profile::from_manifest_str("core"), Profile::Core);
        assert_eq!(Profile::from_manifest_str("Embedded"), Profile::Embedded);
        assert_eq!(Profile::from_manifest_str("FULL"), Profile::Full);
        assert_eq!(Profile::from_manifest_str("nonsense"), Profile::Full);
    }

    // TODO(async main): per `JUX-ENTRY-POINTS-ADDENDUM.md` §E.1.3 the form
    // `public async void main()` is permitted at the spec level, but the
    // grammar in §A.2.4 only allows `async T` for some type `T` — `void`
    // isn't a `type` token. Once the spec resolves the disagreement and
    // the parser accepts `async void`, add a test here asserting that
    // tycheck currently rejects it (until async runtime is wired up).
}
