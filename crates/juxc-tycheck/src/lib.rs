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

use juxc_ast::{CompilationUnit, FnDecl, Param, ReturnType, TopLevelDecl, TypeRef};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

pub mod check;
pub mod env;
pub mod infer;
mod nullable_check;
pub mod symbol_table;
pub mod ty;

pub use env::TypeEnv;
pub use infer::{infer_block, infer_expr};
pub use symbol_table::SymbolTable;
pub use ty::{ArrayKind, Primitive, Ty};

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
    let symbols = symbol_table::build_workspace(units, &mut tc.diagnostics);
    for unit in units {
        tc.check_unit(unit);
        // Reject `T?` where T is a non-nullable value-type primitive
        // (`int?`, `bool?`, `double?`, …). Per spec, only reference
        // types — `String`, user classes/records/enums, arrays of
        // references — can carry the nullable marker. Primitives are
        // value types that always have a meaningful default and
        // never hold null.
        nullable_check::check_nullable_primitives(unit, &mut tc.diagnostics);
    }
    let mut all_expr_types = std::collections::HashMap::new();
    for (idx, unit) in units.iter().enumerate() {
        let mut checker = check::Checker::new(&symbols, &mut tc.diagnostics);
        // Seed the checker's TypeEnv with the per-unit context built
        // during workspace symbol-table construction. This is how
        // `ty_from_ref` knows that a bare `Greeter` in app.jux maps
        // to `com.lib.Greeter` (or whatever the import resolved to).
        if let Some(ctx) = symbols.units.get(idx) {
            checker.seed_unit_context(&ctx.package, &ctx.unqualified);
        }
        checker.check_unit(unit);
        all_expr_types.extend(checker.into_expr_types());
    }
    TypeCheckResult {
        diagnostics: tc.diagnostics,
        symbols,
        expr_types: all_expr_types,
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
                // Class declarations don't carry top-level signature
                // rules of their own yet — the only check we do at
                // this milestone is on `main`. Tycheck for class
                // bodies lands once we have a real type table.
                TopLevelDecl::Class(_) => {}
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
        let return_ok = match &fn_decl.return_type {
            ReturnType::Void => true,
            ReturnType::Type(t) => is_int(t),
            // `async void main()` synthesizes an AsyncType whose inner
            // TypeRef's name is the sentinel "void". Treat that the
            // same as `void`. `async int main()` is the int-returning
            // async entry shape.
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
        if !(return_ok && params_ok) {
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

    // TODO(async main): per `JUX-ENTRY-POINTS-ADDENDUM.md` §E.1.3 the form
    // `public async void main()` is permitted at the spec level, but the
    // grammar in §A.2.4 only allows `async T` for some type `T` — `void`
    // isn't a `type` token. Once the spec resolves the disagreement and
    // the parser accepts `async void`, add a test here asserting that
    // tycheck currently rejects it (until async runtime is wired up).
}
