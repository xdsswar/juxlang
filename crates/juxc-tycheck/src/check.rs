//! Phase D + F of the type checker — **statement checking + type-mismatch
//! diagnostics**.
//!
//! Phase C ([`crate::infer`]) is silent: it tells you what type an
//! expression has but never produces a diagnostic. Phase D/F walks every
//! function/method/constructor body and consults the inferred types at
//! "expected vs found" sites, emitting `E0410`–`E0413` when the user's
//! program disagrees with itself.
//!
//! ## Diagnostics
//!
//! - **E0410 — TypeMismatch.** Assignment, return, and call-argument
//!   sites all share this code. The message text differentiates
//!   ("expected X, found Y" for arguments, "cannot assign T to U" for
//!   assignments, "expected return value of type X" for returns).
//! - **E0411 — WrongArgCount.** Wrong number of positional args to a
//!   function, method, or constructor.
//! - **E0412 — UnresolvedField.** `obj.field` where `field` doesn't
//!   exist on the receiver's class (walking the `extends` chain).
//! - **E0413 — UnresolvedMethod.** `obj.method(...)` where `method`
//!   doesn't exist on the receiver's class, OR `new T(...)` where no
//!   class/record `T` is in scope.
//!
//! ## Tolerance rules ([`compatible`])
//!
//! 1. `Unknown` on either side → compatible. Inference's silent-fallback
//!    must not cascade into diagnostic noise.
//! 2. `Ty::Param` on either side → compatible. Phase E substitutes
//!    receiver-side generic args into expected parameter types before
//!    they reach this predicate, so a `new Box<int>("hi")` call now
//!    sees `expected = int, found = String` and emits E0410. The
//!    wildcard rule still catches cases substitution doesn't reach —
//!    method-level generics, raw-type receivers, and members inherited
//!    across an `extends` clause whose generic args weren't propagated.
//! 3. Exact equality (`expected == found`) → compatible.
//! 4. **Default-int / default-float widening.** `Ty::Primitive(Int)`
//!    (the type of an unsuffixed integer literal) is compatible with
//!    any numeric primitive. Same story for `Ty::Primitive(Double)`
//!    (the type of an unsuffixed float literal). This is the
//!    *minimum* coercion needed to accept idiomatic code like
//!    `i32 x = 7;` — true numeric promotion (`int + long`) still
//!    requires an explicit `as` cast.
//! 5. Arrays — element types must be compatible AND kinds must match.
//! 6. User types — same name AND compatible generic-args (pairwise).
//!
//! ## Inheritance chain walks
//!
//! Method and field lookup walk `class.extends` recursively. This means
//! a `Dog extends Animal` can call `getName()` (defined on Animal) and
//! we resolve it correctly. The walk is name-based; we don't try to
//! handle multi-parent (Java's single-inheritance model is enough).
//!
//! ## Built-in receivers
//!
//! Some receivers carry methods/fields the type system doesn't (yet)
//! describe via the symbol table:
//!
//! - **Arrays** carry `.length` (Int) plus the methods `push`, `pop`,
//!   `clone` — known mutators/builtins that Vec exposes. We don't
//!   typecheck their args today.
//! - **Strings** carry `.length` (Int) plus a handful of read-only
//!   methods. Same treatment.
//!
//! These allowlists live as constants near the top of the file
//! ([`BUILTIN_ARRAY_METHODS`] / [`BUILTIN_STRING_METHODS`]) so future
//! turns can grow them without rewiring the call-resolution paths.
//!
//! ## Skipped sites
//!
//! - Class field default initializers (`private int x = 5;`) — Phase D
//!   focuses on bodies; default initializers will join later.
//! - Method calls on `Ty::Param` receivers — see rule 2 above; we'd
//!   need bound-aware lookup to do better than "silently accept".

use std::collections::HashMap;

use juxc_ast::{
    BinaryOp, Block, CallExpr, ClassDecl, CompilationUnit, ConstructorDecl, ElseBranch, Expr,
    FieldExpr, FnDecl, InterpSegment, NewObjectExpr, OperatorDecl, OperatorKind, Pattern,
    RecordDecl, ReturnType, Stmt, SwitchBody, SwitchExpr, TopLevelDecl, TypeParam, TypeRef,
    UnaryOp,
};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

use crate::env::TypeEnv;
use crate::infer::{infer_block, infer_expr};
use crate::symbol_table::{ParamSig, SymbolTable};
use crate::ty::{
    compose_extends_substitution, infer_generic_args, is_subtype, lower_member_type, substitute,
    ty_from_ref, Primitive, Ty,
};

// ============================================================================
// Built-in allowlists
// ============================================================================

/// Single-segment names treated as "built-in function — accepts any args,
/// returns Unknown". `print` is the obvious one; `parallel` is the
/// async-runtime concurrent-await builtin (JUX-ASYNC-ADDENDUM-v2),
/// `block_on` is the sync-side driver for awaiting a Future from a
/// non-async context. If/when more built-ins land (`assert`, `panic`,
/// …) they go here.
const BUILTINS: &[&str] = &[
    "print", "parallel", "block_on", "yield_now", "Worker", "now_ms", "assert", "spawn",
    "withTimeout",
    // Stdlib I/O — `File.readText(path)`, `File.writeText(path, body)`.
    // The Jux-level shape is `File.readText(...)`, parsed as a
    // Field call on Path("File"). Registering `File` in BUILTINS
    // lets the resolver accept it; the backend special-cases the
    // method calls into matching `std::fs` operations.
    "File",
];

/// Methods we let through on **any array / List receiver** without
/// checking against a class signature. The backend lowers each to
/// the matching Rust `Vec` operation; the typechecker plays along
/// without requiring a class lookup.
///
/// **Phase-1 stdlib for `List<T>`** — these mirror the spec's
/// `std.collections.List` shape:
///
/// | Jux       | Rust equivalent                           |
/// |-----------|-------------------------------------------|
/// | `.add(x)`     | `.push(x)`                            |
/// | `.get(i)`     | `[i]` (indexed access, panics on OOB) |
/// | `.set(i, x)`  | `[i] = x`                             |
/// | `.contains(x)`| `.contains(&x)`                       |
/// | `.indexOf(x)` | `.iter().position(|e| *e == x).map(...)` |
/// | `.isEmpty()`  | `.is_empty()`                         |
/// | `.size()`     | `.len()` (alias for `.length`)        |
/// | `.first()`    | `[0]` (Phase-1: panics on empty)      |
/// | `.last()`     | `[len-1]`                             |
/// | `.reverse()`  | `.reverse()`                          |
/// | `.sort()`     | `.sort()`                             |
/// | `.clear()`    | `.clear()`                            |
/// | `.remove(i)`  | `.remove(i)`                          |
/// | `.insert(i,x)`| `.insert(i, x)`                       |
/// | `.join(sep)`  | `.join(sep)`                          |
/// | `.map(f)` / `.filter(f)` / `.forEach(f)` | `iter().map/...` |
const BUILTIN_ARRAY_METHODS: &[&str] = &[
    "push", "pop", "clone", "len", "length",
    // List<T> spec methods.
    "add", "get", "set", "contains", "indexOf", "isEmpty", "size",
    "first", "last", "reverse", "sort", "clear", "remove", "insert",
    "join", "map", "filter", "forEach",
];

/// Methods we let through on a **String receiver**. Same idea: the
/// backend understands these, so the typechecker accepts them.
///
/// **Phase-1 stdlib for `String`** — the most common spec methods:
///
/// | Jux               | Rust equivalent                                |
/// |-------------------|------------------------------------------------|
/// | `.length()`       | `.chars().count() as isize`                    |
/// | `.split(sep)`     | `.split(sep).map(String::from).collect::<Vec>` |
/// | `.trim()`         | `.trim().to_string()`                          |
/// | `.contains(s)`    | `.contains(s.as_str())`                        |
/// | `.startsWith(s)`  | `.starts_with(s.as_str())`                     |
/// | `.endsWith(s)`    | `.ends_with(s.as_str())`                       |
/// | `.toUpperCase()`  | `.to_uppercase()`                              |
/// | `.toLowerCase()`  | `.to_lowercase()`                              |
/// | `.replace(a,b)`   | `.replace(a.as_str(), b.as_str())`             |
/// | `.indexOf(s)`     | `.find(s.as_str()).map(...).unwrap_or(-1)`     |
/// | `.substring(s,e)` | `.chars().skip(s).take(e-s).collect()`         |
/// | `.charAt(i)`      | `.chars().nth(i).unwrap()`                     |
/// | `.isEmpty()`      | `.is_empty()`                                  |
const BUILTIN_STRING_METHODS: &[&str] = &[
    "length", "len", "clone", "chars", "bytes", "to_string",
    // String spec methods.
    "split", "trim", "contains", "startsWith", "endsWith",
    "toUpperCase", "toLowerCase", "replace", "indexOf",
    "substring", "charAt", "isEmpty",
    // §K.7 surface: explicit byte/char length forms + repeat.
    "byteLength", "charLength", "repeat",
];

/// Methods we let through on **a Map receiver** without checking
/// against a class signature. Maps to Rust `HashMap` operations:
///
/// | Jux            | Rust equivalent              |
/// |----------------|------------------------------|
/// | `.put(k, v)`   | `.insert(k, v)`              |
/// | `.get(k)`      | `.get(&k).cloned().unwrap()` |
/// | `.contains(k)` | `.contains_key(&k)`          |
/// | `.remove(k)`   | `.remove(&k)`                |
/// | `.size()`      | `.len() as isize`            |
/// | `.isEmpty()`   | `.is_empty()`                |
/// | `.clear()`     | `.clear()`                   |
/// | `.keys()`      | `.keys().cloned().collect()` |
/// | `.values()`    | `.values().cloned().collect()` |
/// Methods we let through on **a HashMap receiver** without
/// checking against a class signature. Maps to Rust `HashMap`
/// operations:
///
/// | Jux            | Rust equivalent              |
/// |----------------|------------------------------|
/// | `.put(k, v)`   | `.insert(k, v)`              |
/// | `.get(k)`      | `.get(&k).cloned().unwrap()` |
/// | `.contains(k)` | `.contains_key(&k)`          |
/// | `.remove(k)`   | `.remove(&k)`                |
/// | `.size()`      | `.len() as isize`            |
/// | `.isEmpty()`   | `.is_empty()`                |
/// | `.clear()`     | `.clear()`                   |
/// | `.keys()`      | `.keys().cloned().collect()` |
/// | `.values()`    | `.values().cloned().collect()` |
const BUILTIN_MAP_METHODS: &[&str] = &[
    "put", "get", "contains", "remove", "size", "isEmpty",
    "clear", "keys", "values",
];

/// Methods we let through on **a HashSet receiver** without
/// checking against a class signature. Maps to Rust `HashSet`
/// operations.
const BUILTIN_SET_METHODS: &[&str] = &[
    "add", "contains", "remove", "size", "isEmpty", "clear",
];

/// Methods we let through on a **Deque receiver** without checking
/// against a class signature. Maps to Rust `VecDeque` operations;
/// the remove/peek forms are nullable (`T?`) — `null` when empty.
const BUILTIN_DEQUE_METHODS: &[&str] = &[
    "addFirst", "addLast", "removeFirst", "removeLast",
    "peekFirst", "peekLast", "contains", "size", "isEmpty", "clear",
];

/// Field/property names we allow on **any array receiver** without a
/// class lookup. Today just `length`; the typechecker treats it as `Int`.
const BUILTIN_ARRAY_FIELDS: &[&str] = &["length"];

/// Field/property names we allow on a **String receiver**.
const BUILTIN_STRING_FIELDS: &[&str] = &["length"];

// ============================================================================
// Checker
// ============================================================================

/// Statement-checker state. Holds an owned `TypeEnv` (pushed/popped as
/// the walker descends), a borrowed [`SymbolTable`] (read-only), a
/// borrowed diagnostic sink (append-only), and the **expected return
/// type** of the function/method currently being walked.
///
/// `current_return` is `None` outside any function body and inside
/// constructors (constructors don't return a value).
pub(crate) struct Checker<'a> {
    /// Per-scope variable bindings. Owned by the checker so it can
    /// push/pop as it descends.
    pub(crate) env: TypeEnv,
    /// Symbol table built by Phase A. Read-only here.
    pub(crate) symbols: &'a SymbolTable,
    /// Diagnostic sink. Append-only — we never read back.
    pub(crate) diagnostics: &'a mut Vec<Diagnostic>,
    /// Expected return type of the function/method we're inside. `None`
    /// outside a function body, and also inside constructor bodies
    /// (constructors don't `return value;`).
    pub(crate) current_return: Option<Ty>,
    /// Per-expression inferred type, keyed by source [`Span`]. Populated
    /// as the checker walks each function/method/constructor body in
    /// [`Self::check_expr`] and friends. The map is moved out via
    /// [`Self::into_expr_types`] when typecheck finishes and exposed to
    /// downstream phases (the Rust backend) through
    /// [`crate::TypeCheckResult::expr_types`].
    ///
    /// Entries with [`Span::DUMMY`] are skipped — they'd collide and
    /// give the wrong type for any expression that happens to carry a
    /// dummy span. The backend's lookup site treats a missing entry as
    /// "fall back to the conservative behavior."
    pub(crate) expr_types: HashMap<Span, Ty>,
    /// Call-sugar expansion plans recorded while checking — one entry
    /// per call that used named arguments and/or omitted
    /// default-valued parameters, keyed by the call's span. The
    /// driver applies these to the AST (`apply_call_expansions`)
    /// before the backend runs, so emission only ever sees plain
    /// positional calls.
    pub(crate) call_expansions: HashMap<Span, Vec<crate::ArgSource>>,
    /// Constructor-overload selections — `new T(...)` / `super(...)` /
    /// `this(...)` call span → index into the class's constructor
    /// list. Absorbed into `SymbolTable::ctor_selections` after the
    /// walk so the backend can pick `new` vs `new__K` per call site.
    pub(crate) ctor_selections: HashMap<Span, usize>,
    /// Method-overload selections (call span → group index), absorbed
    /// into `SymbolTable::method_selections` after the walk. Mirrors
    /// `ctor_selections`.
    pub(crate) method_selections: HashMap<Span, usize>,
    /// `Some(index)` while walking constructor `index`'s body —
    /// `this(...)` delegation is only legal there (and only as the
    /// first statement), and a delegation may not resolve back to
    /// the declaring constructor itself.
    pub(crate) current_ctor: Option<usize>,
    /// CHECKED exceptions the current function body may raise without
    /// an enclosing catch absorbing them — `(exception FQN, site)`
    /// pairs collected while walking. Compared against the declared
    /// `throws` clause at the end of each function/method walk
    /// (§X.1.3, E0711). Cleared per body.
    pub(crate) checked_escapes: Vec<(String, Span)>,
    /// Catch-absorption stack: one frame per enclosing `try` BODY,
    /// holding every type its clauses can catch. A raised checked
    /// exception that is a subtype of any frame entry is absorbed.
    pub(crate) catch_absorb_stack: Vec<Vec<Ty>>,
    /// Depth of lambda bodies being walked — checked-exception
    /// escapes inside a lambda belong to the LAMBDA, not the
    /// enclosing function (Phase 1 doesn't type lambda throws), so
    /// recording is suppressed when > 0.
    pub(crate) lambda_depth: usize,
    /// True while checking a for-each header's iterable expression —
    /// the one position a `step` range is legal in Phase 1.
    pub(crate) in_foreach_iter: bool,
    /// True while we're walking the body of a `static` method (or
    /// a `static` field initializer once those land). Drives the
    /// `E0425_ThisInStaticContext` diagnostic in `check_expr`.
    pub(crate) in_static: bool,
    /// True while we're inside an **async context** — the body of an
    /// `async` function/method, or an async lambda. Drives the
    /// `E0700_AwaitRequiresAsyncContext` check: `await` is only legal when
    /// this is set (async addendum §18.1.2). Reset to `false` inside a
    /// constructor body and inside a non-async lambda.
    pub(crate) in_async: bool,
    /// True while we're inside an **unsafe context** — the body of an
    /// `unsafe` function/method, or an `unsafe { … }` block. Drives the
    /// `E0506_UnsafeOpOutsideUnsafe` check: calling an `unsafe` function (a
    /// foreign `unsafe fn` stub) is only legal when this is set (grammar
    /// §A.2.8). Reset to `false` inside a non-unsafe lambda.
    pub(crate) in_unsafe: bool,
    /// `var x = new X<>()` declarations whose inferred type carries an
    /// **unresolved** generic argument (nothing at the construction site pinned
    /// it). Flushed at the end of each function/method/constructor body: a
    /// candidate whose name never appears in [`Self::used_names`] is genuinely
    /// uninferable (an unused, type-ambiguous local) and gets E0431 — turning
    /// what would be a `rustc` E0282 into a precise Jux error. A candidate that
    /// IS referenced is left alone, since any later use can pin the parameter
    /// (`new Vec<>(); v.push(1)` infers `Vec<int>` in the emitted Rust).
    pub(crate) uninferable_news: Vec<(String, Span)>,
    /// Bare local names referenced anywhere in the current body (collected as
    /// `Expr::Path` leaves are walked). Pairs with [`Self::uninferable_news`].
    pub(crate) used_names: std::collections::HashSet<String>,
    /// Bare names of polymorphic-base classes (Stage-2 virtual dispatch — see
    /// [`crate::symbol_table::polymorphic_base_bare_names`]). Precomputed once
    /// at construction. Drives the `E0437` field-through-base diagnostic: a
    /// data field accessed through a base-typed reference would hit the
    /// `Rc<dyn …Kind>` representation, which can't expose struct fields.
    pub(crate) poly_bases: std::collections::HashSet<String>,
    /// Names of the **const-generic parameters** currently in scope
    /// (the `N` of an enclosing `<int N>` / `<bool B>`). Populated by
    /// [`Self::declare_const_generic_params`]; never popped — a stale
    /// entry can only arise across sibling decls, where the worst case
    /// is an over-eager E0445 on an already-broken size expression.
    /// Drives the fixed-array-size guard: a size expression that
    /// *mentions* a const param must be the bare name (`new int[N]`),
    /// since arithmetic over it (`N + 1`) needs the const-eval
    /// interpreter (spec phase 16) and would otherwise leak rustc's
    /// `generic_const_exprs` error.
    pub(crate) const_param_names: std::collections::HashSet<String>,
}

impl<'a> Checker<'a> {
    /// Construct a fresh checker. `symbols` is the Phase-A symbol table;
    /// `diagnostics` is the same vec the rest of typecheck appends to.
    pub(crate) fn new(symbols: &'a SymbolTable, diagnostics: &'a mut Vec<Diagnostic>) -> Self {
        Self {
            env: TypeEnv::new(),
            symbols,
            diagnostics,
            current_return: None,
            expr_types: HashMap::new(),
            call_expansions: HashMap::new(),
            ctor_selections: HashMap::new(),
            method_selections: HashMap::new(),
            current_ctor: None,
            checked_escapes: Vec::new(),
            catch_absorb_stack: Vec::new(),
            lambda_depth: 0,
            in_foreach_iter: false,
            in_static: false,
            in_async: false,
            in_unsafe: false,
            uninferable_news: Vec::new(),
            used_names: std::collections::HashSet::new(),
            poly_bases: crate::symbol_table::polymorphic_base_bare_names(symbols),
            const_param_names: std::collections::HashSet::new(),
        }
    }

    /// Emit `E0431` for every recorded `var x = new X<>()` whose `x` was never
    /// referenced in the just-walked body (so nothing could pin its generic
    /// argument), then clear the per-body tracking sets. Called at the end of
    /// each function/method/constructor walk.
    fn flush_uninferable_news(&mut self) {
        let candidates = std::mem::take(&mut self.uninferable_news);
        for (name, span) in candidates {
            if !self.used_names.contains(&name) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0431_GenericInferenceNoSolution,
                        format!(
                            "cannot infer the type argument of `{name}`: it is never used to \
                             pin it — write the type explicitly, e.g. `new Vec<String>()`",
                        ),
                    )
                    .with_span(span),
                );
            }
        }
        self.used_names.clear();
    }

    /// True when `ty` is a valid `throw` operand: an `Exception` (or subclass),
    /// per §X.2.1. An indeterminate type (`Unknown` / a type parameter) is
    /// accepted so inference gaps don't produce false E0710s; a definite
    /// non-class value (primitive, `String`, array, …) is rejected.
    fn throwable_ok(&self, ty: &Ty) -> bool {
        // Peel any nullable wrapper — `throw maybeEx` is judged on the inner type.
        let mut inner = ty;
        while let Ty::Nullable(i) = inner {
            inner = i;
        }
        let start = match inner {
            Ty::Unknown | Ty::Param(_) => return true,
            Ty::User { name, .. } => name.as_str(),
            _ => return false,
        };
        // Walk the class-extends chain looking for `Exception`. We match on the
        // bare last segment (`*.Exception`) rather than a strict FQN compare so a
        // chain whose `extends_fqn` is still bare (no-package fallback) resolves
        // too — and we resolve each bare extends segment back to a class key so
        // the hop doesn't dead-end. `Throwable`/`Error` never hit `Exception`, so
        // they're correctly rejected (spec §X.2.1 requires `Exception`).
        let mut key = if self.symbols.classes.contains_key(start) {
            Some(start.to_string())
        } else {
            self.symbols.find_fqn_by_bare(start)
        };
        let mut depth = 0;
        while let Some(k) = key {
            if depth > 64 {
                break;
            }
            if k.rsplit('.').next() == Some("Exception") {
                return true;
            }
            let Some(class) = self.symbols.classes.get(&k) else { break };
            key = match &class.extends_fqn {
                Some(fqn) => Some(fqn.clone()),
                None => class
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
                    .and_then(|bare| {
                        if self.symbols.classes.contains_key(&bare) {
                            Some(bare)
                        } else {
                            self.symbols.find_fqn_by_bare(&bare)
                        }
                    }),
            };
            depth += 1;
        }
        false
    }

    /// Is this function/method declared `async`? `async` is encoded either as
    /// an `async T` return type or as the `async` modifier; accept both.
    fn fn_is_async(decl: &FnDecl) -> bool {
        matches!(decl.return_type, ReturnType::AsyncType(_))
            || decl
                .modifiers
                .iter()
                .any(|m| matches!(m, juxc_ast::FnModifier::Async))
    }

    /// Consume the checker, returning both span-keyed maps it built:
    /// the per-expression types AND the call-sugar expansion plans.
    pub(crate) fn into_maps(
        self,
    ) -> (
        HashMap<Span, Ty>,
        HashMap<Span, Vec<crate::ArgSource>>,
        HashMap<Span, usize>,
        HashMap<Span, usize>,
    ) {
        (
            self.expr_types,
            self.call_expansions,
            self.ctor_selections,
            self.method_selections,
        )
    }

    /// Seed the checker's [`TypeEnv`] with the per-unit
    /// name-resolution context produced during workspace
    /// symbol-table construction. Called once per unit by
    /// `typecheck_workspace` before `check_unit`.
    pub(crate) fn seed_unit_context(
        &mut self,
        package: &[String],
        unqualified: &HashMap<String, String>,
    ) {
        self.env.current_package = package.to_vec();
        self.env.unqualified = unqualified.clone();
    }

    /// Infer the type of `expr` against the current env, then record it
    /// keyed by the expression's span. Returns the inferred type so
    /// existing call sites that used `infer_expr(...)` can drop in this
    /// method as a replacement.
    ///
    /// Dummy spans (`Span::DUMMY`) are not recorded — they'd collide
    /// across unrelated expressions and give the backend wrong type
    /// info.
    pub(crate) fn infer_and_record(&mut self, expr: &Expr) -> Ty {
        let ty = infer_expr(expr, &self.env, self.symbols);
        let span = expr_span(expr);
        if span != Span::DUMMY {
            self.expr_types.insert(span, ty.clone());
        }
        ty
    }


    /// Walk every top-level item in `unit`. Functions get checked
    /// directly; classes / records dispatch to `check_class` /
    /// `check_record` which handle members.
    pub(crate) fn check_unit(&mut self, unit: &CompilationUnit) {
        // Record the unit's package onto the env so `set_class` can
        // build the right FQN and `ty_from_ref` falls back to it
        // when the unit's resolver doesn't carry an entry.
        let pkg: Vec<String> = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
            .unwrap_or_default();
        self.env.current_package = pkg;
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(fn_decl) => self.check_function(fn_decl),
                TopLevelDecl::Class(class) => self.check_class(class),
                TopLevelDecl::Record(record) => self.check_record(record),
                TopLevelDecl::Enum(enum_decl) => self.check_enum(enum_decl),
                // Interfaces carry only signatures (body: None) — no
                // bodies to walk.
                TopLevelDecl::Interface(_) => {}
                // Type aliases — nothing body-shaped to check; the
                // target is validated when expanded at use sites.
                TopLevelDecl::TypeAlias(_) => {}
                // Top-level constants — verify the initializer's
                // inferred type fits the declared type. Emits
                // `E0410_TypeMismatch` on a mismatch. The
                // resolver already walked the initializer for
                // name-resolution errors.
                TopLevelDecl::Const(c) => {
                    let found = self.infer_and_record(&c.value);
                    self.check_expr(&c.value);
                    // When the type is written, check the initializer matches.
                    // When it's omitted (inferred), the initializer's type IS
                    // the constant's type — nothing to compare against.
                    if let Some(decl_ty) = &c.ty {
                        let expected = ty_from_ref(decl_ty, &self.env, self.symbols);
                        if !compatible(&expected, &found, self.symbols) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0410_TypeMismatch,
                                    format!(
                                        "constant `{}`: expected {}, found {}",
                                        c.name.text, expected, found,
                                    ),
                                )
                                .with_span(expr_span(&c.value)),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Walk an enum's operator bodies. Same scope shape as records:
    /// `this` is the enum's type, operator params are declared into
    /// the body's scope. Deleted operators have no body and are
    /// skipped inside `check_operator`.
    fn check_enum(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        let name = crate::symbol_table::make_fqn(
            &self.env.current_package,
            &enum_decl.name.text,
        );
        self.env.set_class(&name);
        let this_ty = Ty::User {
            name: name.clone(),
            generic_args: Vec::new(),
        };
        for op in &enum_decl.operators {
            self.check_operator(op, &this_ty);
        }
        // Enum METHODS (§A.2.5) — `this` is the enum value; bodies
        // check like operator bodies (no inheritance, no fields).
        for method in &enum_decl.methods {
            let Some(body) = &method.body else { continue };
            self.env.push_scope();
            self.env.declare("this", this_ty.clone());
            for param in &method.params {
                let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
                self.env.declare(&param.name.text, ty);
            }
            let saved = self.current_return.take();
            self.current_return = Some(return_type_to_ty(
                &method.return_type,
                &self.env,
                self.symbols,
            ));
            self.check_block(body);
            self.current_return = saved;
            self.env.pop_scope();
        }
        self.env.clear_class();
    }

    // ------------------------------------------------------------------
    // Function / method / constructor walkers
    // ------------------------------------------------------------------

    /// Walk a top-level function. Pushes a parameter scope, sets the
    /// expected return type, walks the body, then restores both.
    /// Abstract / native functions (body = None) are skipped.
    /// Validate the default-value expressions of a parameter list at
    /// the declaration (§S.1.3):
    ///
    /// - the default's type must be assignable to the parameter's
    ///   declared type (E0410), and
    /// - Phase 1 forbids a default from referencing ANY parameter of
    ///   the same declaration (E0449) — the expansion pass clones the
    ///   default into call sites, where parameter names don't exist.
    ///   (The spec allows earlier-parameter references; that needs a
    ///   temp-hoisting lowering that lands later.)
    ///
    /// Called BEFORE the parameters are declared into the body scope,
    /// so the default is inferred in the surrounding (caller-like)
    /// environment.
    /// Resolve a (possibly bare) exception-type name to its FQN in
    /// the class table — exact key, then unique `.{name}` suffix.
    fn resolve_exception_fqn(&self, name: &str) -> Option<String> {
        if self.symbols.classes.contains_key(name) {
            return Some(name.to_string());
        }
        if name.contains('.') {
            return None;
        }
        let suffix = format!(".{name}");
        let mut hits = self
            .symbols
            .classes
            .keys()
            .filter(|k| k.ends_with(&suffix));
        match (hits.next(), hits.next()) {
            (Some(k), None) => Some(k.clone()),
            _ => None,
        }
    }

    /// CHECKED test (§X.1.3): the class reaches
    /// `jux.std.exceptions.Exception` on its extends chain WITHOUT
    /// passing through `RuntimeException`. `Error` and `Throwable`
    /// branches (and non-exception classes) are not checked.
    fn is_checked_exception_fqn(&self, fqn: &str) -> bool {
        let mut cur = fqn.to_string();
        let mut depth = 0usize;
        loop {
            if cur == "jux.std.exceptions.RuntimeException" {
                return false;
            }
            if cur == "jux.std.exceptions.Exception" {
                return true;
            }
            if depth > 64 {
                return false;
            }
            depth += 1;
            match self.symbols.classes.get(&cur).and_then(|c| c.extends_fqn.clone()) {
                Some(p) => cur = p,
                None => return false,
            }
        }
    }

    /// Record a checked exception the current body may raise, unless
    /// an enclosing `try`'s catch absorbs it or we're inside a lambda
    /// body (whose throws are its own, Phase 1).
    fn record_checked_raise(&mut self, fqn: &str, span: Span) {
        if self.lambda_depth > 0 {
            return;
        }
        if !self.is_checked_exception_fqn(fqn) {
            return;
        }
        let raised = Ty::User {
            name: fqn.to_string(),
            generic_args: Vec::new(),
        };
        let absorbed = self.catch_absorb_stack.iter().any(|frame| {
            frame
                .iter()
                .any(|caught| is_subtype(&raised, caught, self.symbols))
        });
        if !absorbed {
            self.checked_escapes.push((fqn.to_string(), span));
        }
    }

    /// Record every checked exception a CALLEE declares it throws
    /// (§X.1.3 propagation) — raw dotted names off the signature.
    fn record_callee_throws(&mut self, throws: &[String], span: Span) {
        for name in throws {
            if let Some(fqn) = self.resolve_exception_fqn(name) {
                self.record_checked_raise(&fqn, span);
            }
        }
    }

    /// End-of-body enforcement: every recorded escape must be a
    /// subtype of some type in the declared `throws` clause — E0711
    /// otherwise. Clears the recording state for the next body.
    fn enforce_declared_throws(&mut self, declared: &[juxc_ast::QualifiedName], fn_name: &str) {
        let escapes = std::mem::take(&mut self.checked_escapes);
        if escapes.is_empty() {
            return;
        }
        let declared_tys: Vec<Ty> = declared
            .iter()
            .filter_map(|qn| {
                let name = qn
                    .segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                self.resolve_exception_fqn(&name).map(|fqn| Ty::User {
                    name: fqn,
                    generic_args: Vec::new(),
                })
            })
            .collect();
        let mut reported: Vec<String> = Vec::new();
        for (fqn, span) in escapes {
            let raised = Ty::User {
                name: fqn.clone(),
                generic_args: Vec::new(),
            };
            let covered = declared_tys
                .iter()
                .any(|d| is_subtype(&raised, d, self.symbols));
            if !covered && !reported.contains(&fqn) {
                reported.push(fqn.clone());
                let bare = fqn.rsplit('.').next().unwrap_or(&fqn);
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0711_UncaughtChecked,
                        format!(
                            "`{fn_name}` may throw the checked exception `{bare}` — catch it, or declare `throws {bare}` on the signature",
                        ),
                    )
                    .with_span(span)
                    .with_help("checked = extends Exception without passing through RuntimeException (§X.1.3)"),
                );
            }
        }
    }

    /// True when `ty` can satisfy a `where T has operator KIND`
    /// constraint (§O.5): user classes/records by declaring the
    /// operator; primitives and String through their native operator
    /// families. `Unknown`/`Param` pass (don't cascade).
    fn ty_satisfies_operator(&self, ty: &Ty, kind: OperatorKind) -> bool {
        use OperatorKind as K;
        match ty {
            Ty::Unknown | Ty::Param(_) => true,
            Ty::Primitive(p) => match kind {
                K::Eq | K::Cmp | K::Hash | K::ToString => true,
                K::Plus | K::Minus | K::Mul | K::Div | K::Rem | K::Neg => {
                    !matches!(p, Primitive::Bool)
                }
                K::BitAnd | K::BitOr | K::BitXor | K::BitNot | K::Shl | K::Shr => {
                    !matches!(p, Primitive::Bool | Primitive::Float | Primitive::Double)
                }
                _ => false,
            },
            Ty::String => matches!(kind, K::Eq | K::Cmp | K::Hash | K::ToString | K::Plus),
            Ty::User { name, .. } => {
                let class_ok = self
                    .symbols
                    .classes
                    .get(name)
                    .or_else(|| {
                        self.resolve_class_fqn(name)
                            .and_then(|fqn| self.symbols.classes.get(&fqn))
                    })
                    .map(|c| c.operators.get(&kind).is_some_and(|o| !o.is_deleted))
                    .unwrap_or(false);
                let record_ok = self
                    .symbols
                    .records
                    .get(name)
                    .map(|r| r.operators.get(&kind).is_some_and(|o| !o.is_deleted))
                    .unwrap_or(false);
                class_ok || record_ok
            }
            _ => false,
        }
    }

    /// Enforce a callee's where-constraints (§O.5, E0941) against the
    /// inferred/explicit instantiation. `None` entries (uninferred
    /// slots) pass — they surface elsewhere.
    fn enforce_where_constraints(
        &mut self,
        callee_name: &str,
        wheres: &[(String, OperatorKind)],
        generic_params: &[TypeParam],
        subst_args: &[Ty],
        call_span: Span,
    ) {
        for (param_name, kind) in wheres {
            let Some(idx) = generic_params
                .iter()
                .position(|g| g.name.text == *param_name)
            else {
                continue;
            };
            let Some(bound) = subst_args.get(idx) else { continue };
            if !self.ty_satisfies_operator(bound, *kind) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0941_ConstraintNotSatisfied,
                        format!(
                            "type {bound} does not satisfy `{param_name} has operator {}` required by `{callee_name}`",
                            operator_kind_user_spelling(*kind),
                        ),
                    )
                    .with_span(call_span),
                );
            }
        }
    }

    /// Element type of a user iterable (§K.5): resolve the class's
    /// `iterator()` method, read its declared `Iterator<T>` return,
    /// and yield `T`. `None` when the class doesn't speak the
    /// protocol (the for-each then types its variable Unknown and
    /// rustc reports the real story).
    fn iterable_element_type(&self, class_name: &str) -> Option<Ty> {
        let (method, declaring) = self.symbols.lookup_method(class_name, "iterator")?;
        let ret = match &method.return_type {
            juxc_ast::ReturnType::Type(t) => t,
            _ => return None,
        };
        let _ = declaring;
        // `Iterator<T>` — take the single generic arg as the element.
        let arg = ret.generic_args.first()?;
        let elem_ref = arg.as_type()?;
        Some(ty_from_ref(elem_ref, &self.env, self.symbols))
    }

    /// Validate one `expr?` site (§X.4.1):
    ///
    /// - `Result<T, E>` operand → the enclosing function must return
    ///   `Result<U, F>`; `E` must be compatible with `F` (E0731
    ///   otherwise).
    /// - `T?` operand → the enclosing return must be nullable.
    /// - anything else (or a non-matching return) → E0730.
    /// - Phase 1: `?` inside a `try` body is rejected — its early
    ///   return would bypass the unwinding machinery.
    fn check_error_prop(&mut self, inner: &Expr, span: Span) {
        if !self.catch_absorb_stack.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0730_QuestionIncompatibleReturn,
                    "`?` inside a `try` block isn't supported yet (Phase 1) — its early return would bypass the try machinery; restructure with a plain match or move the `?` call out of the try",
                )
                .with_span(span),
            );
            return;
        }
        let operand = infer_expr(inner, &self.env, self.symbols);
        let ret = self.current_return.clone().unwrap_or(Ty::Unknown);
        let is_result = |t: &Ty| -> Option<(Ty, Ty)> {
            if let Ty::User { name, generic_args } = t {
                if name.rsplit('.').next() == Some("Result") && generic_args.len() == 2 {
                    return Some((generic_args[0].clone(), generic_args[1].clone()));
                }
            }
            None
        };
        match (&operand, is_result(&operand)) {
            (_, Some((_ok, err))) => match is_result(&ret) {
                Some((_, ret_err)) => {
                    if !compatible(&ret_err, &err, self.symbols) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0731_QuestionNeedsConversion,
                                format!(
                                    "`?` propagates error type {err}, but the function returns a Result with error type {ret_err} — convert explicitly before propagating",
                                ),
                            )
                            .with_span(span),
                        );
                    }
                }
                None => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0730_QuestionIncompatibleReturn,
                            format!(
                                "`?` on a Result needs the enclosing function to return a Result — it returns {ret}",
                            ),
                        )
                        .with_span(span),
                    );
                }
            },
            (Ty::Nullable(_), _) => {
                if !matches!(ret, Ty::Nullable(_)) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0730_QuestionIncompatibleReturn,
                            format!(
                                "`?` on a nullable needs the enclosing function to return a nullable — it returns {ret}",
                            ),
                        )
                        .with_span(span),
                    );
                }
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0730_QuestionIncompatibleReturn,
                        format!(
                            "`?` needs a Result or nullable operand, found {operand}",
                        ),
                    )
                    .with_span(span),
                );
            }
        }
    }

    /// Most specific common SUPERCLASS of the given class types —
    /// the multi-catch binder type (§X.3.6). Walks each type's
    /// extends chain (self first) and returns the first entry of the
    /// first chain present in every other chain; `Ty::Unknown` when
    /// the types share no ancestor (or aren't classes), which keeps
    /// the binder usable without cascading errors.
    fn common_class_supertype(&self, tys: &[Ty]) -> Ty {
        let chain = |t: &Ty| -> Vec<String> {
            let mut out = Vec::new();
            if let Ty::User { name, .. } = t {
                let mut cur = Some(name.clone());
                let mut depth = 0usize;
                while let Some(n) = cur {
                    if depth > 64 {
                        break;
                    }
                    depth += 1;
                    cur = self
                        .symbols
                        .classes
                        .get(&n)
                        .and_then(|c| c.extends_fqn.clone());
                    out.push(n);
                }
            }
            out
        };
        let first = chain(&tys[0]);
        let rest: Vec<Vec<String>> = tys[1..].iter().map(chain).collect();
        for cand in &first {
            if rest.iter().all(|ch| ch.contains(cand)) {
                return Ty::User {
                    name: cand.clone(),
                    generic_args: Vec::new(),
                };
            }
        }
        Ty::Unknown
    }

    fn check_param_defaults(&mut self, params: &[juxc_ast::Param]) {
        for param in params {
            let Some(default) = &param.default else { continue };
            for other in params {
                let mut hit_span: Option<Span> = None;
                collect_bare_name_reads(default, &mut |qn| {
                    if qn.segments.len() == 1 && qn.segments[0].text == other.name.text {
                        hit_span.get_or_insert(qn.segments[0].span);
                    }
                });
                if let Some(span) = hit_span {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0449_DefaultArgParamRef,
                            format!(
                                "default value of `{}` references parameter `{}` — \
                                 a default is evaluated at the call site (Phase 1), \
                                 where parameters aren't in scope; compute it inside \
                                 the body instead",
                                param.name.text, other.name.text,
                            ),
                        )
                        .with_span(span),
                    );
                }
            }
            self.check_expr(default);
            let expected = ty_from_ref(&param.ty, &self.env, self.symbols);
            let found = infer_expr(default, &self.env, self.symbols);
            if !compatible(&expected, &found, self.symbols) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0410_TypeMismatch,
                        format!(
                            "default value of `{}`: expected {}, found {}",
                            param.name.text, expected, found,
                        ),
                    )
                    .with_span(match expr_span(default) {
                        s if s == Span::DUMMY => param.span,
                        s => s,
                    }),
                );
            }
        }
    }

    fn check_function(&mut self, fn_decl: &FnDecl) {
        let Some(body) = &fn_decl.body else { return };
        self.check_param_defaults(&fn_decl.params);
        self.env.push_scope();
        // Declare each parameter into the new scope so name lookups
        // inside the body resolve.
        for param in &fn_decl.params {
            self.check_iface_value_type(&param.ty);
            self.check_fixed_array_size_in_type(&param.ty);
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
        }
        // Const-generic params (`int cap<int N>()`) read as values in
        // the body — declare them with their value type.
        self.declare_const_generic_params(&fn_decl.generic_params);
        self.check_iface_return_type(&fn_decl.return_type);
        let saved = self.current_return.take();
        self.current_return = Some(return_type_to_ty(
            &fn_decl.return_type,
            &self.env,
            self.symbols,
        ));
        let saved_async = self.in_async;
        self.in_async = Self::fn_is_async(fn_decl);
        let saved_unsafe = self.in_unsafe;
        self.in_unsafe = fn_decl.modifiers.contains(&juxc_ast::FnModifier::Unsafe);
        self.check_block(body);
        // §X.1.3: every checked exception the body can raise must be
        // covered by the declared `throws` clause.
        self.enforce_declared_throws(&fn_decl.throws, &fn_decl.name.text);
        self.flush_uninferable_news();
        self.in_unsafe = saved_unsafe;
        self.in_async = saved_async;
        self.current_return = saved;
        self.env.pop_scope();
    }

    /// Walk a class declaration — for each constructor and each method,
    /// set up the class context (current_class + generic params + `this`
    /// binding), run the body checker, then tear it down. Abstract
    /// methods (body = None) are skipped.
    fn check_class(&mut self, class: &ClassDecl) {
        // Class context — FQN'd so the visibility / subtype walks
        // that key on `env.current_class` find the right entry in
        // the symbol table.
        let class_name = crate::symbol_table::make_fqn(
            &self.env.current_package,
            &class.name.text,
        );
        self.env.set_class(&class_name);
        // Register every generic param so `T` in declared types lowers
        // to `Ty::Param("T")` rather than `Unknown`.
        for tp in &class.generic_params {
            self.env.add_generic_param(&tp.name.text);
        }
        // Const-generic params (`<int N>`) additionally read as VALUES
        // inside every body (`return N;`) — declare them with their
        // value type so expressions over `N` type-check as ints/bools.
        self.declare_const_generic_params(&class.generic_params);
        // Pre-compute the `this` type: User<class_name, [Param(T)…]>.
        let this_ty = Ty::User {
            name: class_name.clone(),
            generic_args: class
                .generic_params
                .iter()
                .map(|tp| Ty::Param(tp.name.text.clone()))
                .collect(),
        };

        // Field slots are value positions — an interface-typed field lowers
        // to a `Rc<dyn Trait>` struct member, so reject the non-dispatchable
        // forms before the backend emits a broken field type.
        for field in &class.fields {
            if let Some(fty) = &field.ty {
                self.check_iface_value_type(fty);
                self.check_wildcard_storage_type(fty);
                self.check_fixed_array_size_in_type(fty);
            }
        }
        for (idx, ctor) in class.constructors.iter().enumerate() {
            self.check_constructor(ctor, &this_ty, idx);
        }
        for method in &class.methods {
            self.check_method(method, &this_ty);
        }
        for op in &class.operators {
            self.check_operator(op, &this_ty);
        }
        // Initializer blocks (§M.1 / §S.4.1). `this` is in scope (an instance
        // `init` runs during construction; a `static` block has no instance,
        // but declaring `this` is harmless since a well-formed static block
        // won't read it). Neither form is async.
        for block in class.init_blocks.iter().chain(&class.static_init_blocks) {
            self.env.push_scope();
            self.env.declare("this", this_ty.clone());
            let saved_async = self.in_async;
            self.in_async = false;
            self.check_block(block);
            self.in_async = saved_async;
            self.env.pop_scope();
        }
        // Destructor block (§6.6 / §S.5). At most one per class; the
        // body runs with `this` in scope, synchronously.
        if class.drop_blocks.len() > 1 {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0400_DuplicateDeclaration,
                    format!(
                        "class `{}` declares {} `drop` blocks — a class may have at most one destructor",
                        class.name.text,
                        class.drop_blocks.len(),
                    ),
                )
                .with_span(class.drop_blocks[1].span),
            );
        }
        for block in &class.drop_blocks {
            self.env.push_scope();
            self.env.declare("this", this_ty.clone());
            let saved_async = self.in_async;
            self.in_async = false;
            self.check_block(block);
            self.in_async = saved_async;
            self.env.pop_scope();
        }

        self.env.clear_generic_params();
        self.env.clear_class();
    }

    /// Walk a constructor body. Like [`check_function`] but with no
    /// expected return type (constructors don't return values) and with
    /// `this` pre-declared.
    fn check_constructor(&mut self, ctor: &ConstructorDecl, this_ty: &Ty, ctor_idx: usize) {
        self.check_param_defaults(&ctor.params);
        // `this(...)` / `super(...)` position rules (§7.3.1, E0210):
        // a delegation must be the FIRST statement, and a constructor
        // can't both delegate to a sibling AND call `super(...)` —
        // the delegated-to constructor owns parent initialization.
        let mut saw_this_call = false;
        for (i, stmt) in ctor.body.statements.iter().enumerate() {
            if let Stmt::Expr(Expr::Call(call)) = stmt {
                if matches!(call.callee.as_ref(), Expr::This(_)) {
                    saw_this_call = true;
                    if i != 0 {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0210_ConstructorCallNotFirst,
                                "`this(...)` must be the first statement of the constructor",
                            )
                            .with_span(call.span),
                        );
                    }
                }
            }
            if saw_this_call {
                if let Stmt::SuperCall(_, sspan) = stmt {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0210_ConstructorCallNotFirst,
                            "a constructor that delegates with `this(...)` can't also call `super(...)` — the delegated-to constructor owns parent initialization",
                        )
                        .with_span(*sspan),
                    );
                }
            }
        }
        let saved_ctor = self.current_ctor.replace(ctor_idx);
        self.env.push_scope();
        self.env.declare("this", this_ty.clone());
        for param in &ctor.params {
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
        }
        let saved = self.current_return.take();
        self.current_return = None; // constructors don't return values
        // Constructors are never async (§18.1.1) — `await` in a ctor body is
        // therefore an error. Force the async context off across the body.
        let saved_async = self.in_async;
        self.in_async = false;
        self.check_block(&ctor.body);
        // Constructors carry no `throws` clause in Phase 1 — drop the
        // recorded raises rather than enforce them.
        self.checked_escapes.clear();
        self.flush_uninferable_news();
        self.in_async = saved_async;
        self.current_ctor = saved_ctor;
        self.current_return = saved;
        self.env.pop_scope();
    }

    /// Walk an instance method body. Same scope shape as a function
    /// plus a `this` binding. Abstract methods (body = None) are
    /// skipped.
    fn check_method(&mut self, method: &FnDecl, this_ty: &Ty) {
        let Some(body) = &method.body else { return };
        self.check_param_defaults(&method.params);
        let is_static = method
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Static));
        self.env.push_scope();
        // Static methods have no implicit receiver — skip the
        // `this` binding and flip the `in_static` flag so any
        // `this` inside the body fires `E0425_ThisInStaticContext`.
        if !is_static {
            self.env.declare("this", this_ty.clone());
        }
        let saved_static = self.in_static;
        self.in_static = is_static;
        // Method-level generic params extend the class-level set.
        for tp in &method.generic_params {
            self.env.add_generic_param(&tp.name.text);
        }
        self.declare_const_generic_params(&method.generic_params);
        for param in &method.params {
            self.check_iface_value_type(&param.ty);
            self.check_fixed_array_size_in_type(&param.ty);
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
        }
        self.check_iface_return_type(&method.return_type);
        let saved = self.current_return.take();
        self.current_return = Some(return_type_to_ty(
            &method.return_type,
            &self.env,
            self.symbols,
        ));
        let saved_async = self.in_async;
        self.in_async = Self::fn_is_async(method);
        let saved_unsafe = self.in_unsafe;
        self.in_unsafe = method.modifiers.contains(&juxc_ast::FnModifier::Unsafe);
        self.check_block(body);
        // §X.1.3: checked exceptions the method body raises must be
        // covered by its `throws` clause.
        self.enforce_declared_throws(&method.throws, &method.name.text);
        self.flush_uninferable_news();
        self.in_unsafe = saved_unsafe;
        self.in_async = saved_async;
        self.current_return = saved;
        self.in_static = saved_static;
        // Method-local generic params would also clear here, but the
        // class's params are still active until check_class finishes.
        // We can't surgically remove just the method's — for Turn 1 we
        // accept the over-broadening (no method-local generics in any
        // existing example).
        self.env.pop_scope();
    }

    /// Fire `E0435` when an interface type appears in a **value position**
    /// (a variable / parameter / field / return slot — lowered to
    /// `Rc<dyn Trait>`) in a form that can't be made into a working trait
    /// object:
    ///
    /// - a **generic-method** interface (`<R> R map(...)`) — never object-safe,
    ///   so always rejected; and
    /// - a **generic interface used raw** (`Box b;` with no type argument) —
    ///   `dyn Box` needs its argument (`dyn Box<int>`), so the raw form is
    ///   rejected while `Box<int>` is allowed.
    ///
    /// The interface declaration itself stays perfectly valid — only this
    /// dynamic-value use is restricted; it can still be implemented and called
    /// through concrete classes. Catching it here keeps the emitted
    /// `Rc<dyn Trait>` from leaking rustc's `E0038` / `E0107`.
    fn check_iface_value_type(&mut self, tref: &juxc_ast::TypeRef) {
        // Function-typed and pointer slots are never interface trait objects.
        if tref.fn_shape.is_some() || tref.ptr_depth > 0 {
            return;
        }
        let Some(seg) = tref.name.segments.last() else {
            return;
        };
        let bare = seg.text.as_str();
        match crate::symbol_table::interface_dyn_dispatch_support(self.symbols, bare) {
            Some(Err(crate::symbol_table::DynDispatchBlock::GenericMethod(m))) => {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0435_InterfaceNotDynDispatchable,
                        format!(
                            "interface `{bare}` can't be used as a dynamic value type — its \
                             method `{m}` has generic type parameters, which makes the trait \
                             not object-safe; call it through a concrete implementer instead",
                        ),
                    )
                    .with_span(tref.span),
                );
            }
            Some(Err(crate::symbol_table::DynDispatchBlock::GenericInterface(_)))
                if tref.generic_args.is_empty() =>
            {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0435_InterfaceNotDynDispatchable,
                        format!(
                            "generic interface `{bare}` used as a value type needs its type \
                             argument(s) (e.g. `{bare}<int>`) — a raw `{bare}` value slot can't \
                             be lowered to a trait object",
                        ),
                    )
                    .with_span(tref.span),
                );
            }
            _ => {}
        }
    }

    /// Fire **E0444** when a **bounded wildcard** appears as a generic
    /// argument of a **user-defined generic class** used in a *storage*
    /// position — a field, local-variable, or return slot. Such a slot
    /// erases the wildcard to a trait object inside the container
    /// (`Box<? extends Animal>` → `Box<Rc<dyn AnimalKind>>`), but Rust
    /// generics are invariant, so no concrete `Box<Dog>` can populate it
    /// without a structural conversion Phase 1 doesn't synthesize. Catch
    /// it here instead of leaking `rustc`'s `E0308`.
    ///
    /// **Parameter** positions are exempt — the backend lifts a wildcard
    /// param to a synthetic function generic (`fn f<__W: …>(b: Box<__W>)`)
    /// which accepts any concrete subtype soundly. So this is only called
    /// from the field / local / return visitors, never the param ones.
    ///
    /// Scope is narrow on purpose: only **user classes** (not interfaces
    /// — those route through E0435 — and not stdlib collections, which
    /// have their own representation) carrying a *direct* wildcard arg.
    fn check_wildcard_storage_type(&mut self, tref: &juxc_ast::TypeRef) {
        if tref.fn_shape.is_some() || tref.ptr_depth > 0 {
            return;
        }
        // The type must name a user-declared generic class.
        let Some(seg) = tref.name.segments.last() else { return };
        let bare = seg.text.as_str();
        let is_user_generic_class = self
            .symbols
            .classes
            .iter()
            .any(|(k, c)| {
                !c.generic_params.is_empty()
                    && (k == bare || k.rsplit('.').next().unwrap_or(k.as_str()) == bare)
            });
        if !is_user_generic_class {
            return;
        }
        // At least one DIRECT generic arg must be a bounded/unbounded
        // wildcard (`? extends T`, `? super T`, `?`).
        let has_wildcard = tref
            .generic_args
            .iter()
            .any(|a| matches!(a, juxc_ast::GenericArg::Wildcard(_)));
        if !has_wildcard {
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0444_WildcardStorageUnsupported,
                format!(
                    "a bounded wildcard on the user type `{bare}` can't be used as a \
                     storage slot (field, local, or return) in this phase — the container \
                     erases to a trait object that a concrete `{bare}<…>` can't populate; \
                     use a concrete type argument, or take the value as a parameter (where \
                     wildcards lift to a function generic)",
                ),
            )
            .with_span(tref.span),
        );
    }

    /// Validate a **reference cast** (`(T) x` / `x as T`) between user types
    /// (E0442): the source and target must be in a subtype relationship in
    /// either direction (a downcast or an upcast), or the target must be
    /// `any`. An unrelated cast can never succeed and would lower to a
    /// guaranteed-panicking downcast — reject it. Primitive / numeric casts
    /// and casts where either side is an inference hole are left alone.
    fn check_reference_cast(&mut self, c: &juxc_ast::CastExpr) {
        if !is_plain_user_typeref(&c.ty) {
            return;
        }
        let target_ty = ty_from_ref(&c.ty, &self.env, self.symbols);
        if !matches!(target_ty, Ty::User { .. }) {
            return;
        }
        let src_ty = infer_expr(&c.value, &self.env, self.symbols);
        if self.ref_relation_possible(&src_ty, &target_ty) {
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0442_UnrelatedCast,
                format!(
                    "cannot cast `{src_ty}` to `{target_ty}`: the types are unrelated \
                     (neither is a subtype of the other), so the cast can never succeed",
                ),
            )
            .with_span(c.span),
        );
    }

    /// Validate a type-test `x => T [binder]`. Checks the tested value,
    /// rejects a misplaced binder (E0441 — binders are only meaningful as/in
    /// an `if` condition; `allow_binder` is set by the `if`-condition path),
    /// and rejects an impossible test (E0442 — `x` could never be a `T`).
    fn check_typetest(&mut self, t: &juxc_ast::TypeTestExpr, allow_binder: bool) {
        self.check_expr(&t.value);
        if let Some(binder) = &t.binder {
            if !allow_binder {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0441_TypeTestBinderMisplaced,
                        format!(
                            "the type-test binder `{}` is only valid in an `if` condition \
                             (`if (x => T {})`); use the bare test `x => T` here",
                            binder.text, binder.text,
                        ),
                    )
                    .with_span(binder.span),
                );
            }
        }
        if !is_plain_user_typeref(&t.ty) {
            return;
        }
        let target_ty = ty_from_ref(&t.ty, &self.env, self.symbols);
        if !matches!(target_ty, Ty::User { .. }) {
            return;
        }
        let src_ty = infer_expr(&t.value, &self.env, self.symbols);
        if self.ref_relation_possible(&src_ty, &target_ty) {
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0442_UnrelatedCast,
                format!(
                    "`{src_ty} => {target_ty}` can never be true: the types are unrelated",
                ),
            )
            .with_span(t.span),
        );
    }

    /// True iff a reference cast / type-test from `src_ty` to `target_ty`
    /// could ever succeed: they're in a subtype relationship (either
    /// direction), the target is `any`, the source isn't a concrete user type
    /// (inference hole — don't flag), or some class is a subtype of BOTH (an
    /// interface sidecast). Two unrelated classes have no common instance
    /// under single inheritance, so that case returns `false`.
    fn ref_relation_possible(&self, src_ty: &Ty, target_ty: &Ty) -> bool {
        if let Ty::User { name, .. } = target_ty {
            if name == "any" {
                return true;
            }
        }
        if !matches!(src_ty, Ty::User { .. }) {
            return true;
        }
        if crate::ty::is_subtype(src_ty, target_ty, self.symbols)
            || crate::ty::is_subtype(target_ty, src_ty, self.symbols)
        {
            return true;
        }
        self.symbols.classes.keys().any(|fqn| {
            let cty = Ty::User {
                name: fqn.clone(),
                generic_args: Vec::new(),
            };
            crate::ty::is_subtype(&cty, src_ty, self.symbols)
                && crate::ty::is_subtype(&cty, target_ty, self.symbols)
        })
    }

    /// Run [`Self::check_iface_value_type`] on the `TypeRef` inside a
    /// [`ReturnType`], if any (skips `void`).
    fn check_iface_return_type(&mut self, rt: &ReturnType) {
        match rt {
            ReturnType::Type(t) | ReturnType::AsyncType(t) => {
                self.check_iface_value_type(t);
                self.check_wildcard_storage_type(t);
                self.check_fixed_array_size_in_type(t);
            }
            ReturnType::Void => {}
        }
    }

    /// If `receiver_ty` is a user class/record AND the matching
    /// operator on that type is marked `= delete;` (§O.3.4), emit
    /// `E0935_DeletedOperator` anchored at `span`. No-op otherwise.
    ///
    /// Inherited deletion isn't traced — only the receiver's own
    /// class/record is consulted. That matches the rest of operator
    /// resolution in tycheck today (Phase E substitution only fires
    /// on the receiver's own class) and keeps the diagnostic precise.
    fn check_op_not_deleted(&mut self, receiver_ty: &Ty, kind: OperatorKind, span: Span) {
        let Ty::User { name, .. } = receiver_ty else { return };
        let deleted = self
            .symbols
            .classes
            .get(name)
            .and_then(|c| c.operators.get(&kind))
            .map(|op| op.is_deleted)
            .unwrap_or(false)
            || self
                .symbols
                .records
                .get(name)
                .and_then(|r| r.operators.get(&kind))
                .map(|op| op.is_deleted)
                .unwrap_or(false)
            || self
                .symbols
                .enums
                .get(name)
                .and_then(|e| e.operators.get(&kind))
                .map(|op| op.is_deleted)
                .unwrap_or(false);
        if deleted {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0935_DeletedOperator,
                    format!(
                        "operator `{}` is deleted on type `{}`",
                        operator_kind_user_spelling(kind),
                        name,
                    ),
                )
                .with_span(span),
            );
        }
    }

    /// Walk an operator-overload body. Same scope shape as
    /// [`Self::check_method`]: push a fresh scope, declare `this` with
    /// the class's `Ty::User` shape, declare each formal param, set
    /// `current_return` from the operator's declared return type, walk
    /// the body, then tear it back down.
    ///
    /// Per `JUX-OPERATORS-ADDENDUM.md` §O.2 operators have no
    /// modifiers, no method-level generics, and (today) always have a
    /// body — so the bookkeeping is simpler than `check_method`.
    fn check_operator(&mut self, op: &OperatorDecl, this_ty: &Ty) {
        let Some(body) = &op.body else { return };
        self.env.push_scope();
        self.env.declare("this", this_ty.clone());
        for param in &op.params {
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
        }
        let saved = self.current_return.take();
        self.current_return = Some(return_type_to_ty(&op.return_type, &self.env, self.symbols));
        self.check_block(body);
        self.current_return = saved;
        self.env.pop_scope();
    }

    /// Walk a record's body — operator overrides plus methods. Same
    /// scope shape as classes: `this` is the record's `Ty::User`,
    /// operator/method params are declared into the body's scope.
    /// `= delete;` operators have no body and are skipped inside
    /// [`Self::check_operator`].
    fn check_record(&mut self, record: &RecordDecl) {
        let name = crate::symbol_table::make_fqn(
            &self.env.current_package,
            &record.name.text,
        );
        self.env.set_class(&name);
        for tp in &record.generic_params {
            self.env.add_generic_param(&tp.name.text);
        }
        self.declare_const_generic_params(&record.generic_params);
        let this_ty = Ty::User {
            name: name.clone(),
            generic_args: record
                .generic_params
                .iter()
                .map(|tp| Ty::Param(tp.name.text.clone()))
                .collect(),
        };
        for op in &record.operators {
            self.check_operator(op, &this_ty);
        }
        for method in &record.methods {
            self.check_method(method, &this_ty);
        }
        self.env.clear_generic_params();
        self.env.clear_class();
    }

    // ------------------------------------------------------------------
    // Statement walker
    // ------------------------------------------------------------------

    /// Walk a block — each statement in source order. Doesn't push a
    /// scope; callers wrap if they need scope nesting (e.g. method
    /// body, for-each loop body).
    fn check_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
    }

    /// Walk one statement, emitting diagnostics where types disagree.
    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl(v) => {
                // A declared interface-typed local lowers to `Rc<dyn Trait>`
                // — reject the non-dispatchable forms before the backend
                // emits a broken slot type.
                if let Some(t) = &v.ty {
                    self.check_iface_value_type(t);
                    self.check_wildcard_storage_type(t);
                    self.check_fixed_array_size_in_type(t);
                }
                // If both a declared type and an initializer are
                // present, the two must be compatible. Otherwise the
                // present one wins.
                let declared = v.ty.as_ref().map(|t| ty_from_ref(t, &self.env, self.symbols));
                let inferred = v.init.as_ref().map(|e| {
                    // Walk the initializer for nested checks (e.g. a
                    // call inside the RHS) before reading its type.
                    self.check_expr(e);
                    infer_expr(e, &self.env, self.symbols)
                });
                let final_ty = match (&declared, &inferred) {
                    (Some(d), Some(i)) => {
                        if !compatible(d, i, self.symbols) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0410_TypeMismatch,
                                    format!(
                                        "type mismatch in declaration of `{}`: expected {}, found {}",
                                        v.name.text, d, i,
                                    ),
                                )
                                .with_span(v.span),
                            );
                        }
                        d.clone()
                    }
                    (Some(d), None) => d.clone(),
                    (None, Some(i)) => i.clone(),
                    (None, None) => Ty::Unknown,
                };
                // Record a `var x = new X<>()` whose inferred type still has an
                // unresolved generic argument: if `x` is never referenced later
                // (checked at body end) nothing can pin it → E0431.
                if v.ty.is_none()
                    && matches!(
                        &v.init,
                        Some(Expr::NewObject(n)) if n.generic_args.is_empty() && n.args.is_empty()
                    )
                    && matches!(
                        &final_ty,
                        Ty::User { generic_args, .. }
                            if generic_args.iter().any(|a| matches!(a, Ty::Unknown))
                    )
                {
                    self.uninferable_news.push((v.name.text.clone(), v.span));
                }
                self.env.declare(&v.name.text, final_ty);
            }

            Stmt::Assign(a) => {
                // Walk both sides for nested checks first.
                self.check_expr(&a.target);
                self.check_expr(&a.value);
                // **Property write-access enforcement (§M.7.2).** A
                // write to `obj.Prop` / `Class.Prop` where `Prop` is a
                // read-only / init-only / restricted-visibility property
                // is rejected here. The legitimate constructor write was
                // already lowered (by the parser's desugarer) to a
                // direct backing-field write, so any property-named
                // assignment reaching tycheck is a post-construction or
                // out-of-scope write.
                self.check_property_write(&a.target);
                let target_ty = infer_expr(&a.target, &self.env, self.symbols);
                let value_ty = infer_expr(&a.value, &self.env, self.symbols);
                if !compatible(&target_ty, &value_ty, self.symbols) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!("cannot assign {value_ty} to {target_ty}"),
                        )
                        .with_span(a.span),
                    );
                }
            }

            Stmt::Return(opt) => {
                // Clone the expected return type up front so we can
                // mutably borrow `self` to walk the expression below
                // without a borrow conflict on `current_return`.
                let expected = self.current_return.clone();
                match (&expected, opt) {
                    // Bare `return;` inside a void function — fine.
                    (Some(t), None) if t.is_void() => {}
                    // Bare `return;` outside any function — fine.
                    (None, None) => {}
                    // Bare `return;` in a value-returning function.
                    (Some(exp), None) => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0410_TypeMismatch,
                                format!(
                                    "expected return value of type {exp}, found bare `return`",
                                ),
                            ),
                        );
                    }
                    (_, Some(expr)) => {
                        self.check_expr(expr);
                        let found = infer_expr(expr, &self.env, self.symbols);
                        if let Some(exp) = &expected {
                            if !compatible(exp, &found, self.symbols) {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0410_TypeMismatch,
                                        format!(
                                            "return type mismatch: expected {exp}, found {found}",
                                        ),
                                    )
                                    .with_span(expr_span(expr)),
                                );
                            }
                        }
                        // If `expected` is None (top-level statement
                        // outside a function), nothing to check.
                    }
                }
            }

            Stmt::If(if_stmt) => {
                // Type-test smart-cast: `if (x => Dog d)` allows the binder and
                // introduces `d: Dog` into the then-branch only (§T.6.2).
                let smartcast: Option<(String, Ty)> =
                    if let Expr::TypeTest(t) = &if_stmt.condition {
                        self.check_typetest(t, true);
                        t.binder.as_ref().map(|b| {
                            (
                                b.text.clone(),
                                ty_from_ref(&t.ty, &self.env, self.symbols),
                            )
                        })
                    } else {
                        self.check_expr(&if_stmt.condition);
                        let cond_ty =
                            infer_expr(&if_stmt.condition, &self.env, self.symbols);
                        if !is_boolish(&cond_ty) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0410_TypeMismatch,
                                    format!("expected bool condition, found {cond_ty}"),
                                )
                                .with_span(expr_span(&if_stmt.condition)),
                            );
                        }
                        None
                    };
                self.env.push_scope();
                if let Some((name, ty)) = &smartcast {
                    self.env.declare(name, ty.clone());
                }
                self.check_block(&if_stmt.then_block);
                self.env.pop_scope();
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.check_else_branch(else_branch);
                }
            }

            Stmt::While(w) => {
                self.check_expr(&w.condition);
                let cond_ty = infer_expr(&w.condition, &self.env, self.symbols);
                if !is_boolish(&cond_ty) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!("expected bool condition, found {cond_ty}"),
                        )
                        .with_span(expr_span(&w.condition)),
                    );
                }
                self.env.push_scope();
                self.check_block(&w.body);
                self.env.pop_scope();
            }

            Stmt::DoWhile(d) => {
                // Body first (Java: runs at least once), then the
                // condition — same bool requirement as `while`.
                self.env.push_scope();
                self.check_block(&d.body);
                self.env.pop_scope();
                self.check_expr(&d.condition);
                let cond_ty = infer_expr(&d.condition, &self.env, self.symbols);
                if !is_boolish(&cond_ty) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!("expected bool condition, found {cond_ty}"),
                        )
                        .with_span(expr_span(&d.condition)),
                    );
                }
            }

            Stmt::ForEach(f) => {
                let prev_fe = self.in_foreach_iter;
                self.in_foreach_iter = true;
                self.check_expr(&f.iter);
                self.in_foreach_iter = prev_fe;
                let iter_ty = infer_expr(&f.iter, &self.env, self.symbols);
                // Loop-var binding: explicit annotation wins; else
                // element-of-array if iter is an array; else Unknown.
                let var_ty = if let Some(declared) = &f.var_type {
                    ty_from_ref(declared, &self.env, self.symbols)
                } else {
                    match &iter_ty {
                        Ty::Array { element, .. } => (**element).clone(),
                        // User iterable (§O.6/§K.5): the protocol's
                        // element type — `iterator()`'s Iterator<T>
                        // argument, or the iterator's own `next()`
                        // return with the `?` peeled.
                        Ty::User { name, .. } => self
                            .iterable_element_type(name)
                            .unwrap_or(Ty::Unknown),
                        _ => Ty::Unknown,
                    }
                };
                self.env.push_scope();
                self.env.declare(&f.var_name.text, var_ty);
                self.check_block(&f.body);
                self.env.pop_scope();
            }

            Stmt::ForC(f) => {
                // Header scope: init declares the loop var, visible in
                // cond/update/body.
                self.env.push_scope();
                if let Some(init) = f.init.as_deref() {
                    self.check_stmt(init);
                }
                if let Some(cond) = &f.cond {
                    self.check_expr(cond);
                    let cond_ty = infer_expr(cond, &self.env, self.symbols);
                    if !is_boolish(&cond_ty) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0410_TypeMismatch,
                                format!("expected bool condition, found {cond_ty}"),
                            )
                            .with_span(expr_span(cond)),
                        );
                    }
                }
                if let Some(upd) = f.update.as_deref() {
                    self.check_stmt(upd);
                }
                self.env.push_scope();
                self.check_block(&f.body);
                self.env.pop_scope();
                self.env.pop_scope();
            }

            Stmt::Expr(e) => {
                self.check_expr(e);
            }

            Stmt::SuperCall(args, span) => self.check_super_call(args, *span),

            Stmt::Throw(e, span) => {
                // Walk the operand for sub-expression diagnostics, then enforce
                // §X.2.1: the thrown value must be `Exception` or a subclass.
                // Catching it here turns the otherwise-cryptic emitted-Rust
                // trait-bound failure (`panic_any` on a non-exception) into a
                // precise Jux E0710.
                self.check_expr(e);
                let thrown = infer_expr(e, &self.env, self.symbols);
                // §X.1.3: a CHECKED throw must be absorbed by an
                // enclosing catch or declared on the signature.
                if let Ty::User { name, .. } = &thrown {
                    if let Some(fqn) = self.resolve_exception_fqn(name) {
                        self.record_checked_raise(&fqn, *span);
                    }
                }
                if !self.throwable_ok(&thrown) {
                    // Anchor on the operand when it has a real span, else the
                    // whole `throw` statement (literals can carry dummy spans).
                    let es = expr_span(e);
                    let at = if es == Span::DUMMY { *span } else { es };
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0710_ThrowRequiresException,
                            format!(
                                "`throw` requires an `Exception` (or subclass), found {thrown}",
                            ),
                        )
                        .with_span(at),
                    );
                }
            }

            Stmt::Try(t) => {
                // Checked-exception absorption (§X.1.3): every type
                // this try's clauses can catch shields raises inside
                // the BODY (not the catch/finally blocks).
                let mut absorb_frame: Vec<Ty> = Vec::new();
                for c in &t.catches {
                    absorb_frame.push(ty_from_ref(&c.ty, &self.env, self.symbols));
                    for alt in &c.alt_tys {
                        absorb_frame.push(ty_from_ref(alt, &self.env, self.symbols));
                    }
                }
                self.catch_absorb_stack.push(absorb_frame);
                self.check_block(&t.body);
                self.catch_absorb_stack.pop();
                // Caught types so far, to detect an unreachable later clause
                // (§X.3.4): a catch whose type is the same as, or a subtype of,
                // an earlier clause's can never run.
                let mut caught: Vec<Ty> = Vec::new();
                for c in &t.catches {
                    // All listed types of the clause — one for the
                    // ordinary form, several for a multi-catch
                    // (`catch (E1 | E2 e)`, §X.3.6).
                    let mut tys: Vec<Ty> =
                        vec![ty_from_ref(&c.ty, &self.env, self.symbols)];
                    for alt in &c.alt_tys {
                        tys.push(ty_from_ref(alt, &self.env, self.symbols));
                    }
                    // E0721: alternatives must be pairwise UNRELATED —
                    // a subtype alongside its supertype is dead weight
                    // (the supertype alone already catches both).
                    for j in 1..tys.len() {
                        for i in 0..j {
                            if is_subtype(&tys[i], &tys[j], self.symbols)
                                || is_subtype(&tys[j], &tys[i], self.symbols)
                            {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0721_MultiCatchRelated,
                                        format!(
                                            "multi-catch types must be unrelated: `{}` and \
                                             `{}` are in a subtype relationship — keep only \
                                             the broader type",
                                            tys[i], tys[j],
                                        ),
                                    )
                                    .with_span(c.span),
                                );
                            }
                        }
                    }
                    // E0720 (§X.3.4): the clause is unreachable when
                    // EVERY listed type is already covered by an
                    // earlier clause.
                    if tys.iter().all(|ty| {
                        caught
                            .iter()
                            .any(|earlier| is_subtype(ty, earlier, self.symbols))
                    }) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0720_UnreachableCatch,
                                format!(
                                    "unreachable `catch ({})`: an earlier clause already \
                                     catches it",
                                    tys[0],
                                ),
                            )
                            .with_span(c.span)
                            .with_help("reorder catches so more specific types come first"),
                        );
                    }
                    let bind_ty = if tys.len() == 1 {
                        tys[0].clone()
                    } else {
                        // Multi-catch binder: the most specific COMMON
                        // supertype of the listed types (§X.3.6).
                        self.common_class_supertype(&tys)
                    };
                    caught.extend(tys);
                    self.env.push_scope();
                    // Bind the caught name with the computed type so
                    // the body sees `e` as a normal local.
                    self.env.declare(&c.name.text, bind_ty);
                    self.check_block(&c.body);
                    self.env.pop_scope();
                }
                if let Some(fin) = &t.finally {
                    // W0720 (§X.3.5): a `return` inside `finally`
                    // overrides the body's return value AND swallows
                    // any in-flight exception. Lambdas inside the
                    // block open their own return scope and don't
                    // count.
                    let mut spans = Vec::new();
                    collect_returns_in_block(fin, &mut spans);
                    for span in spans {
                        let span = if span == Span::DUMMY { fin.span } else { span };
                        self.diagnostics.push(
                            Diagnostic::warning(
                                code::Code::W0720_ReturnInFinally,
                                "`return` inside `finally` discards the try/catch result and swallows in-flight exceptions",
                            )
                            .with_span(span)
                            .with_help("compute the value in the try body and return after the try statement"),
                        );
                    }
                    self.check_block(fin);
                }
            }

            Stmt::Unsafe(b) => {
                // Inside an `unsafe { … }` block, unsafe operations (calls to
                // `unsafe` fns, raw-pointer ops) are permitted. Set the flag
                // for the duration of the block, then restore.
                let saved_unsafe = self.in_unsafe;
                self.in_unsafe = true;
                self.check_block(b);
                self.in_unsafe = saved_unsafe;
            }
            Stmt::Break(..) | Stmt::Continue(..) => {}
            Stmt::Labeled { stmt, .. } => self.check_stmt(stmt),
        }
    }

    /// Recurse through else / else-if chains, mirroring [`Self::check_stmt`]'s
    /// handling for the top-level `if`.
    fn check_else_branch(&mut self, branch: &ElseBranch) {
        match branch {
            ElseBranch::If(if_stmt) => {
                // Same type-test smart-cast handling as a top-level `if`, so
                // `else if (x => Dog d)` binds `d` in its then-branch.
                let smartcast: Option<(String, Ty)> =
                    if let Expr::TypeTest(t) = &if_stmt.condition {
                        self.check_typetest(t, true);
                        t.binder.as_ref().map(|b| {
                            (b.text.clone(), ty_from_ref(&t.ty, &self.env, self.symbols))
                        })
                    } else {
                        self.check_expr(&if_stmt.condition);
                        let cond_ty = infer_expr(&if_stmt.condition, &self.env, self.symbols);
                        if !is_boolish(&cond_ty) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0410_TypeMismatch,
                                    format!("expected bool condition, found {cond_ty}"),
                                )
                                .with_span(expr_span(&if_stmt.condition)),
                            );
                        }
                        None
                    };
                self.env.push_scope();
                if let Some((name, ty)) = &smartcast {
                    self.env.declare(name, ty.clone());
                }
                self.check_block(&if_stmt.then_block);
                self.env.pop_scope();
                if let Some(nested) = &if_stmt.else_branch {
                    self.check_else_branch(nested);
                }
            }
            ElseBranch::Block(block) => {
                self.env.push_scope();
                self.check_block(block);
                self.env.pop_scope();
            }
        }
    }

    // ------------------------------------------------------------------
    // Expression walker (depth-first, drives field/method/call checks)
    // ------------------------------------------------------------------

    /// Walk an expression for the side effects of its sub-checks
    /// (field-resolution, call-arg checks, etc.) and call recursively
    /// into sub-expressions. The expression's inferred type is fed by
    /// [`infer_expr`] when callers need it — this method returns
    /// nothing.
    ///
    /// Side-effect (Phase H): the inferred type of `expr` is recorded
    /// into [`Self::expr_types`] keyed by its span before dispatching.
    /// Together with the recursive descent below, this guarantees every
    /// expression the checker visits has its type captured for later
    /// consumption by the Rust backend.
    #[allow(clippy::only_used_in_recursion)]
    fn check_expr(&mut self, expr: &Expr) {
        // Record this expression's type up-front. Sub-expressions get
        // recorded when their containing check_expr recurses into them.
        let _ = self.infer_and_record(expr);
        match expr {
            Expr::Literal(_) => {}
            // `expr?` — error propagation (§X.4.1). Validate the
            // operand/return-type pairing (E0730/E0731) and the
            // Phase-1 no-`?`-inside-try restriction.
            Expr::ErrorProp(inner, span) => {
                self.check_expr(inner);
                self.check_error_prop(inner, *span);
            }
            // Tuple literal — walk each element for nested checks.
            Expr::TupleLit(elems, _) => {
                for e in elems {
                    self.check_expr(e);
                }
            }
            // Try-expression (§X.3.3) — same per-clause checks as the
            // statement form (E0720/E0721, binder typing), via the
            // shared statement walker on a synthesized Stmt view.
            Expr::TryExpr(t) => {
                self.check_stmt(&Stmt::Try((**t).clone()));
            }
            // Record a bare-name reference so the E0431 "uninferable `new`"
            // flush can tell whether a `var x = new X<>()` is ever used.
            Expr::Path(qn) => {
                if qn.segments.len() == 1 {
                    self.used_names.insert(qn.segments[0].text.clone());
                }
            }
            // `this` inside a `static` method has no receiver to
            // refer to — fire E0425 once per occurrence.
            Expr::This(span) => {
                if self.in_static {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0425_ThisInStaticContext,
                            "`this` cannot be used inside a `static` method (no receiver)",
                        )
                        .with_span(*span),
                    );
                }
            }

            Expr::Super(span) => {
                // `super` is a receiver, not a value — like `this`, it's
                // illegal in a `static` context (no instance), and only
                // meaningful when the enclosing class has a superclass.
                if self.in_static {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0425_ThisInStaticContext,
                            "`super` cannot be used inside a `static` method (no receiver)",
                        )
                        .with_span(*span),
                    );
                } else if self
                    .env
                    .current_class
                    .as_ref()
                    .and_then(|c| self.symbols.classes.get(c))
                    .and_then(|c| c.extends_fqn.as_ref())
                    .is_none()
                {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0413_UnresolvedMethod,
                            "`super` is only valid inside a class that has a superclass",
                        )
                        .with_span(*span),
                    );
                }
            }

            Expr::Field(f) => {
                self.check_expr(&f.object);
                self.check_field_access(f);
            }

            Expr::Index(i) => {
                self.check_expr(&i.array);
                self.check_expr(&i.index);
            }

            Expr::Call(c) => self.check_call(c),

            Expr::NewObject(n) => self.check_new_object(n),

            Expr::NewArray(n) => {
                self.check_expr(&n.size);
                // A fixed-array size is a Rust const position: when it
                // references a const-generic param it must be the BARE
                // name (`new int[N]`) — arithmetic over it (`N + 1`)
                // needs the const-eval interpreter (spec phase 16) and
                // would leak rustc's `generic_const_exprs` error.
                self.check_const_size_expr(&n.size);
            }

            Expr::NewArrayLit(n) => {
                for el in &n.elements {
                    self.check_expr(el);
                }
            }

            Expr::Cast(c) => {
                self.check_expr(&c.value);
                self.check_reference_cast(c);
            }

            Expr::TypeTest(t) => {
                // Generic position (not an `if` condition — those are handled
                // in `check_stmt`): the bare boolean test is fine; a binder
                // here has nowhere to bind → E0441.
                self.check_typetest(t, false);
            }

            Expr::Range(r) => {
                self.check_expr(&r.start);
                self.check_expr(&r.end);
                // `step` (§M.6.3): integer-typed; Phase 1 supports it
                // only as a for-each iterable (`for (i : a..b step s)`)
                // — the ForEach arm clears this flag around its head.
                if let Some(s) = &r.step {
                    self.check_expr(s);
                    let st = infer_expr(s, &self.env, self.symbols);
                    if !compatible(&Ty::Primitive(Primitive::Int), &st, self.symbols) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0410_TypeMismatch,
                                format!("range `step` must be an int, found {st}"),
                            )
                            .with_span(expr_span(s)),
                        );
                    }
                    if !self.in_foreach_iter {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0410_TypeMismatch,
                                "`step` ranges are only supported as for-each iterables in Phase 1 — `for (var i : a..b step s)`",
                            )
                            .with_span(r.span),
                        );
                    }
                }
            }

            Expr::Unary(u) => {
                self.check_expr(&u.operand);
                // §A.2.9 — the raw-pointer operators `*p` (deref) and `&x`
                // (address-of) are `unsafe`-only. Outside an `unsafe` context
                // they trip E0506 (same rule as calling an `unsafe` fn).
                if matches!(u.op, juxc_ast::UnaryOp::Deref | juxc_ast::UnaryOp::AddrOf)
                    && !self.in_unsafe
                {
                    let what = if matches!(u.op, juxc_ast::UnaryOp::Deref) {
                        "raw-pointer dereference `*`"
                    } else {
                        "address-of `&`"
                    };
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0506_UnsafeOpOutsideUnsafe,
                            format!(
                                "{what} requires an `unsafe` block; wrap it in `unsafe {{ … }}` \
                                 or mark the enclosing function `unsafe`",
                            ),
                        )
                        .with_span(u.span),
                    );
                }
                // §O.3.4 — unary operator on a user type whose
                // matching operator was deleted with `= delete;`.
                if let Some(kind) = op_kind_for_unary(u.op) {
                    let receiver_ty = infer_expr(&u.operand, &self.env, self.symbols);
                    self.check_op_not_deleted(&receiver_ty, kind, u.span);
                }
            }

            Expr::Binary(b) => {
                self.check_expr(&b.left);
                self.check_expr(&b.right);
                // §S.2.1 — the wrapping family (`+%` `-%` `*%` `<<%`
                // `>>%`) is INTEGER-only: wrap-modulo-2^N has no
                // meaning for floats (IEEE saturates to ±Inf), bools,
                // chars, or user types, and the spec reserves the
                // `%`-suffixed forms from overloading. Unknown operand
                // types stay lenient (inference gaps must not flag).
                if matches!(
                    b.op,
                    BinaryOp::WrapAdd
                        | BinaryOp::WrapSub
                        | BinaryOp::WrapMul
                        | BinaryOp::WrapShl
                        | BinaryOp::WrapShr
                ) {
                    for operand in [&b.left, &b.right] {
                        let ty = infer_expr(operand, &self.env, self.symbols);
                        let ok = match &ty {
                            Ty::Primitive(p) => !matches!(
                                p,
                                Primitive::Float
                                    | Primitive::Double
                                    | Primitive::F32
                                    | Primitive::F64
                                    | Primitive::Bool
                                    | Primitive::Char
                            ),
                            Ty::Unknown => true,
                            _ => false,
                        };
                        if !ok {
                            // A literal operand joins DUMMY into the
                            // binary's span (a 0-anchored join, useless
                            // for pointing) — prefer the offending
                            // operand's own span when it's real.
                            let span = [expr_span(operand), expr_span(&b.left), b.span]
                                .into_iter()
                                .find(|s| *s != Span::DUMMY)
                                .unwrap_or(b.span);
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0410_TypeMismatch,
                                    format!(
                                        "wrapping operator `{}` requires integer operands, found {ty}",
                                        b.op.as_rust_str(),
                                    ),
                                )
                                .with_span(span),
                            );
                        }
                    }
                }
                // §O.3.4 — binary operator on a user type whose
                // matching operator was deleted with `= delete;`.
                // The receiver is the LHS; that's what determines
                // dispatch per §O.2.6.
                if let Some(kind) = op_kind_for_binary(b.op) {
                    let receiver_ty = infer_expr(&b.left, &self.env, self.symbols);
                    self.check_op_not_deleted(&receiver_ty, kind, b.span);
                }
            }

            Expr::SizeOf(s) => self.check_expr(&s.operand),

            Expr::InterpString(s) => {
                for seg in &s.segments {
                    match seg {
                        InterpSegment::Expr(e) => {
                            self.check_expr(e);
                            // `$"${x}"` interpolates via `operator
                            // string` (which lowers to Display). When
                            // the type's `string` was deleted, that
                            // dispatch isn't available — flag here so
                            // the user gets a Jux diagnostic instead
                            // of a downstream rustc error.
                            let ty = infer_expr(e, &self.env, self.symbols);
                            self.check_op_not_deleted(&ty, OperatorKind::ToString, s.span);
                        }
                        InterpSegment::Bare(ident) => {
                            // `$"$x"` — `x` is a single identifier;
                            // its type is whatever `env` has for it.
                            // Same dispatch through `operator string`.
                            if let Some(ty) = self.env.lookup(&ident.text).cloned() {
                                self.check_op_not_deleted(&ty, OperatorKind::ToString, s.span);
                            }
                        }
                        InterpSegment::Literal(_) => {}
                    }
                }
            }

            Expr::Switch(s) => {
                self.check_expr(&s.scrutinee);
                for arm in &s.arms {
                    // Or-pattern alternatives must be binding-free
                    // (§A.3): an arm body can't reference a name that
                    // only exists when one alternative matched.
                    if let Pattern::Or(alts, span) = &arm.pattern {
                        if alts.iter().any(pattern_introduces_bindings) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0447_OrPatternBinding,
                                    "or-pattern alternatives can't introduce bindings: \
                                     split into one `case` per alternative, or drop the \
                                     `var` binders and re-test inside the body",
                                )
                                .with_span(*span),
                            );
                        }
                    }
                    // `when <cond>` guard (§A.2.8) — walk it for the
                    // usual diagnostics. Pattern bindings live in the
                    // arm's scope, which infer_block re-declares below
                    // for block bodies; guard expressions over binders
                    // resolve through the resolver pass.
                    if let Some(g) = &arm.guard {
                        self.check_expr(g);
                    }
                    match &arm.body {
                        SwitchBody::Expr(e) => self.check_expr(e),
                        SwitchBody::Block(b) => {
                            self.env.push_scope();
                            // The arm's pattern may introduce
                            // bindings; let infer_block declare them so
                            // body expression checks resolve. (Phase
                            // C's walker already does this for variant
                            // bindings; we just reuse it here for the
                            // statements-only walk.)
                            infer_block(b, &mut self.env, self.symbols);
                            self.env.pop_scope();
                        }
                    }
                }
                // Exhaustiveness check (§T.5.5): when the
                // scrutinee resolves to an enum, every variant
                // must be covered by some arm or there must be a
                // wildcard catchall. Sealed-class scrutinees get
                // the same treatment via the `permits` list (not
                // yet wired; deferred to the next pass).
                self.check_switch_exhaustive(s);
            }
            // Lambda — declare params into a fresh scope, then walk
            // the body for the usual diagnostics. Untyped params
            // declare as `Ty::Unknown` so internal type-mismatches
            // stay quiet at the Jux level (Rust will catch any
            // real shape mismatch on the emitted closure).
            //
            // We clear `current_return` while walking the body so a
            // `return x;` inside the lambda isn't compared against
            // the enclosing function's return type — they're
            // unrelated. The lambda's own return type is currently
            // `Unknown` (Phase 1 doesn't infer it), so suppressing
            // the check is the right call.
            Expr::Lambda(l) => {
                // Checked-exception recording pauses inside lambda
                // bodies — their raises belong to the lambda, not the
                // declaring function (Phase 1).
                self.lambda_depth += 1;
                self.env.push_scope();
                for p in &l.params {
                    let ty = match &p.ty {
                        Some(t) => ty_from_ref(t, &self.env, self.symbols),
                        None => Ty::Unknown,
                    };
                    self.env.declare(&p.name.text, ty);
                }
                let saved_return = self.current_return.take();
                // A lambda introduces its OWN async context: an async lambda
                // (`async (x) -> …`) permits `await`; a plain lambda inside an
                // async function does NOT (§18.1.2).
                let saved_async = self.in_async;
                self.in_async = l.is_async;
                match &l.body {
                    juxc_ast::LambdaBody::Expr(e) => self.check_expr(e),
                    juxc_ast::LambdaBody::Block(b) => self.check_block(b),
                }
                self.in_async = saved_async;
                self.current_return = saved_return;
                self.lambda_depth -= 1;
                self.env.pop_scope();
            }
            Expr::Elvis(e) => {
                // Walk both sides; Phase 1 doesn't yet enforce
                // "value must be nullable" or "fallback type
                // matches inner". The backend lowers to
                // `value.unwrap_or(fallback)` and rustc surfaces
                // any type mismatch.
                self.check_expr(&e.value);
                self.check_expr(&e.fallback);
            }
            Expr::MethodRef(_) => {
                // No sub-expressions to walk; method existence
                // verification lives in a future tycheck pass
                // (overload resolution / method-table lookup).
                // Untyped today — backend emits the closure
                // adapter and Rust catches missing members.
            }
            Expr::Ternary(t) => {
                self.check_expr(&t.condition);
                self.check_expr(&t.then_branch);
                self.check_expr(&t.else_branch);
                // Condition must be `bool`. Branches should
                // unify; Phase 1 keeps the unification check
                // permissive and lets rustc surface a real
                // mismatch on the emitted `if`.
                let cond_ty = infer_expr(&t.condition, &self.env, self.symbols);
                if !compatible(&Ty::Primitive(Primitive::Bool), &cond_ty, self.symbols) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!(
                                "ternary condition must be bool, found {cond_ty}",
                            ),
                        )
                        .with_span(expr_span(&t.condition)),
                    );
                }
            }
            // `expr!!` — walk the asserted operand; the null-or-not
            // outcome is a runtime property (NullPointerException), not a
            // static one, so no extra diagnostic fires here.
            Expr::NotNullAssert(inner, _) => self.check_expr(inner),
            Expr::Await(inner, span) => {
                // `await` is permitted ONLY inside an async context — an
                // `async` function/method or an async lambda (§18.1.2). Outside
                // one (a plain function, a constructor, a non-async lambda) it's
                // `E0700`; catching it here turns what would be rustc's cryptic
                // `.await is only allowed inside async fn` into a precise Jux
                // diagnostic before codegen.
                if !self.in_async {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0700_AwaitRequiresAsyncContext,
                            "`await` is only allowed inside an async function, method, or \
                             lambda — mark the enclosing function `async` (e.g. `async T f()`)",
                        )
                        .with_span(*span),
                    );
                }
                // The operand's static type is the operand's type (so a
                // `Future<T>` shape unwraps to `T` in inference); formal
                // Future-typing lands when async types are modelled properly.
                self.check_expr(inner);
            }
        }
    }

    /// Resolve an `obj.field` access. If the receiver type is a known
    /// class/record AND the field name isn't found anywhere in the
    /// inheritance chain, emit **E0412**. Built-in receivers (arrays,
    /// strings) get an allowlist pass.
    /// Enforce member-visibility rules (Phase 1 — Java-style 4
    /// visibilities). Emits `E0414` / `E0415` / `E0416` when the
    /// current accessor isn't allowed to touch a `private` /
    /// `protected` / package-private member.
    ///
    /// - `Public` — always allowed.
    /// - `Private` — only allowed when the accessor is inside the
    ///   `declaring_class`'s body.
    /// - `Protected` — allowed inside `declaring_class` and any
    ///   transitive subclass (extends-chain walk).
    /// - `Package` / `Internal` — Phase 1 collapses "package" to
    ///   "same compilation unit", and we currently only support a
    ///   single unit at a time, so this rule always passes today.
    ///   The diagnostic exists so callers can rely on its
    ///   activation once multi-unit `package foo.bar;` lands.
    ///
    /// `member_kind` is the human-readable phrase used in the
    /// emitted diagnostic — `"field"`, `"method"`, or
    /// `"constructor"`.
    /// Enforce property write-access rules (§M.7.2) on an assignment
    /// target. Recognizes both instance (`obj.Prop`) and static
    /// (`Class.Prop`) property writes. Fires:
    ///
    /// - **E0970** when the property is read-only (`{ get; }`) or
    ///   `init`-only and the write is post-construction. (The
    ///   constructor write is legal but was already desugared into a
    ///   direct backing-field write, so anything reaching here is an
    ///   illegal post-construction / external write.)
    /// - **E0972** when the property's `set` accessor is more
    ///   restrictive than the access site permits (e.g. a
    ///   `{ get; private set; }` written from outside the class).
    fn check_property_write(&mut self, target: &juxc_ast::Expr) {
        use juxc_ast::Expr;
        let Expr::Field(f) = target else { return };
        if f.safe {
            return;
        }
        let prop_name = f.field.text.as_str();
        // Resolve the declaring class: static (`Class.Prop`) or
        // instance (`obj.Prop`).
        let class_fqn: Option<String> = if let Expr::Path(qn) = f.object.as_ref() {
            crate::infer::path_resolves_to_class(qn, &self.env, self.symbols)
                .or_else(|| match infer_expr(&f.object, &self.env, self.symbols) {
                    Ty::User { name, .. } => self.resolve_class_fqn(&name),
                    _ => None,
                })
        } else {
            match infer_expr(&f.object, &self.env, self.symbols) {
                Ty::User { name, .. } => self.resolve_class_fqn(&name),
                _ => None,
            }
        };
        let Some(class_fqn) = class_fqn else { return };
        let Some(prop) = self
            .symbols
            .classes
            .get(&class_fqn)
            .and_then(|c| c.properties.get(prop_name))
            .cloned()
        else {
            return;
        };
        // Read-only / init-only writes reaching here are illegal.
        if prop.is_read_only {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0970_PropertyNotWritable,
                    format!(
                        "cannot assign to read-only property `{prop_name}` of `{class_fqn}` — it has no `set` accessor (settable only in the constructor)",
                    ),
                )
                .with_span(f.span),
            );
            return;
        }
        if prop.is_init_only {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0970_PropertyNotWritable,
                    format!(
                        "cannot assign to init-only property `{prop_name}` of `{class_fqn}` after construction — `init` accessors are settable only during construction",
                    ),
                )
                .with_span(f.span),
            );
            return;
        }
        // Setter visibility (§M.7.7). Reuse the standard visibility
        // machinery so private / protected / package rules match the
        // rest of the language, but route the diagnostic through the
        // property-specific E0972 code.
        if let Some(set_vis) = prop.setter_visibility {
            if !self.write_visibility_allowed(set_vis, &class_fqn) {
                let word = match set_vis {
                    juxc_ast::Visibility::Private => "private",
                    juxc_ast::Visibility::Protected => "protected",
                    _ => "restricted",
                };
                let ctx = match self.env.current_class.as_deref() {
                    Some(a) => format!("from `{a}`"),
                    None => "from top-level code".to_string(),
                };
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0972_PropertyAccessorVisibility,
                        format!(
                            "cannot write property `{prop_name}` of `{class_fqn}` {ctx} — its setter is `{word}`",
                        ),
                    )
                    .with_span(f.span),
                );
            }
        }
    }

    /// True iff a write through an accessor of visibility `vis` on
    /// `declaring_class` is permitted from the current accessor
    /// context. Mirrors the allow-rules in [`Self::check_visibility`]
    /// without emitting a diagnostic.
    fn write_visibility_allowed(
        &self,
        vis: juxc_ast::Visibility,
        declaring_class: &str,
    ) -> bool {
        use juxc_ast::Visibility;
        let accessor = self.env.current_class.as_deref();
        match vis {
            Visibility::Public => true,
            Visibility::Private => accessor == Some(declaring_class),
            Visibility::Protected => accessor.map_or(false, |a| {
                a == declaring_class
                    || crate::ty::walk_extends_reaches(a, declaring_class, self.symbols)
            }),
            Visibility::Package | Visibility::Internal => {
                let declaring_pkg: &[String] = self
                    .symbols
                    .classes
                    .get(declaring_class)
                    .map(|c| c.package.as_slice())
                    .unwrap_or(&[]);
                // Top-level code (`accessor == None`) and classes the
                // table can't resolve still belong to the UNIT's
                // package — fall back to it so a free `main()` can
                // reach package-private members of its own package.
                let accessor_pkg: &[String] = accessor
                    .and_then(|name| self.symbols.classes.get(name))
                    .map(|c| c.package.as_slice())
                    .unwrap_or(&self.env.current_package);
                declaring_pkg == accessor_pkg
            }
        }
    }

    /// Resolve a (possibly bare) class name to its FQN key in the
    /// symbol table. Direct hit first, then a last-segment scan.
    fn resolve_class_fqn(&self, name: &str) -> Option<String> {
        if self.symbols.classes.contains_key(name) {
            return Some(name.to_string());
        }
        self.symbols
            .classes
            .keys()
            .find(|k| k.rsplit('.').next().unwrap_or(k.as_str()) == name)
            .cloned()
    }

    fn check_visibility(
        &mut self,
        vis: juxc_ast::Visibility,
        declaring_class: &str,
        member_name: &str,
        member_kind: &str,
        access_span: juxc_source::Span,
    ) {
        use juxc_ast::Visibility;
        let accessor = self.env.current_class.as_deref();
        let allowed_code = match vis {
            Visibility::Public => return,
            Visibility::Private => {
                if accessor == Some(declaring_class) {
                    return;
                }
                code::Code::E0414_PrivateAccess
            }
            Visibility::Protected => {
                if accessor.map_or(false, |a| {
                    a == declaring_class
                        || crate::ty::walk_extends_reaches(a, declaring_class, self.symbols)
                }) {
                    return;
                }
                code::Code::E0415_ProtectedAccess
            }
            Visibility::Package | Visibility::Internal => {
                // Compare the declaring class's package against the
                // accessor's. Both come from `ClassSig::package`,
                // which is stamped from each unit's `package foo.bar;`
                // line during `build_workspace`. Same-package access
                // (including the no-package "everything at crate
                // root" case) is allowed.
                let declaring_pkg: &[String] = self
                    .symbols
                    .classes
                    .get(declaring_class)
                    .map(|c| c.package.as_slice())
                    .unwrap_or(&[]);
                // Top-level code (`accessor == None`) still belongs to
                // the UNIT's package — fall back to it so a free
                // `main()` reaches package-private members of its own
                // package.
                let accessor_pkg: &[String] = accessor
                    .and_then(|name| self.symbols.classes.get(name))
                    .map(|c| c.package.as_slice())
                    .unwrap_or(&self.env.current_package);
                if declaring_pkg == accessor_pkg {
                    return;
                }
                code::Code::E0416_PackagePrivateAccess
            }
        };
        let visibility_word = match vis {
            juxc_ast::Visibility::Private => "private",
            juxc_ast::Visibility::Protected => "protected",
            juxc_ast::Visibility::Package | juxc_ast::Visibility::Internal => "package-private",
            juxc_ast::Visibility::Public => "public",
        };
        let context = match accessor {
            Some(a) => format!("from `{a}`"),
            None => "from top-level code".to_string(),
        };
        self.diagnostics.push(
            Diagnostic::error(
                allowed_code,
                format!(
                    "cannot access {visibility_word} {member_kind} `{member_name}` of `{declaring_class}` {context}",
                ),
            )
            .with_span(access_span),
        );
    }

    /// Exhaustiveness check for a `switch` expression. Fires
    /// `E0440_NotExhaustive` when the scrutinee is a sealed shape
    /// (enum, or `sealed class` with a non-empty `permits` list)
    /// AND the arms neither (a) collectively name every alternative
    /// nor (b) include a wildcard / bind catchall.
    ///
    /// **Enum scrutinees** — every variant must be matched. Variant
    /// patterns can write `case EnumName.Variant(...)` (two-segment
    /// path) or just `case Variant(...)` (single-segment, common
    /// when the enum is well-known); both shapes count.
    ///
    /// **Sealed-class scrutinees** — every name in the `permits`
    /// list must appear in some arm. Patterns use `case Subclass`
    /// or `case Subclass(...)` shape per `JUX-LANG-V1.md` §7.5
    /// example. Other arms that don't name a permitted subclass
    /// are ignored for exhaustiveness (they're either wildcards,
    /// which the catchall check above handles, or pattern-typos
    /// rustc / the resolver flags separately).
    ///
    /// **Non-sealed scrutinees** — `switch (n) { case 0 -> ...;
    /// case _ -> ... }` over an integer doesn't have a finite
    /// variant set, so exhaustiveness via enumeration doesn't
    /// apply. The check returns silently; the wildcard arm
    /// remains the user's catchall there.
    fn check_switch_exhaustive(&mut self, s: &SwitchExpr) {
        let scrut_ty = infer_expr(&s.scrutinee, &self.env, self.symbols);
        // Two scrutinee shapes drive exhaustiveness: enums (every
        // variant) and sealed classes (every permitted subclass).
        // Resolve to one of them, or bail.
        enum SealedKind<'a> {
            Enum { name: &'a str, variants: Vec<String> },
            Class { name: &'a str, permits: Vec<String> },
        }
        let scrut_name = match &scrut_ty {
            Ty::User { name, .. } => name.as_str(),
            _ => return,
        };
        // FQN-aware lookup (exact key, then unique suffix) so a
        // locally-inferred bare enum name still gets exhaustiveness
        // (and rustc's E0004 never leaks for it).
        let kind = if let Some((_, e)) = self.symbols.lookup_enum(scrut_name) {
            SealedKind::Enum {
                name: scrut_name,
                variants: e.variants.keys().cloned().collect(),
            }
        } else if let Some(c) = self.symbols.classes.get(scrut_name) {
            if c.is_sealed && !c.permits.is_empty() {
                SealedKind::Class {
                    name: scrut_name,
                    permits: c.permits.clone(),
                }
            } else {
                return;
            }
        } else {
            return;
        };

        // A wildcard arm (`case _ -> …` / `default ->`) trivially
        // covers everything left. Same with a top-level bind
        // pattern (`case var x -> …`) — `x` is irrefutable, so it
        // catches anything.
        let mut covered: std::collections::HashSet<String> = std::collections::HashSet::new();
        for arm in &s.arms {
            // Guarded arms don't count toward exhaustiveness
            // (§T.5.6) — the compiler can't prove the guard's
            // runtime condition, so `case X when c ->` leaves `X`
            // uncovered until an unguarded arm handles it.
            if arm.guard.is_some() {
                continue;
            }
            if pattern_is_catchall(&arm.pattern) {
                return;
            }
            match &kind {
                SealedKind::Enum { name, .. } => {
                    collect_variants_covered(&arm.pattern, name, &mut covered);
                }
                SealedKind::Class { .. } => {
                    collect_sealed_subclasses_covered(&arm.pattern, &mut covered);
                }
            }
        }
        let (scrut_label, all, scrut_name) = match &kind {
            SealedKind::Enum { name, variants } => ("enum", variants.clone(), *name),
            SealedKind::Class { name, permits } => ("sealed class", permits.clone(), *name),
        };
        let missing: Vec<String> =
            all.into_iter().filter(|v| !covered.contains(v)).collect();
        if missing.is_empty() {
            return;
        }
        let names = missing.join(", ");
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0440_NotExhaustive,
                format!(
                    "non-exhaustive `switch` on {scrut_label} `{scrut_name}`: \
                     no arm covers {names}; add explicit `case` arms \
                     for each, or a `case _` wildcard at the end",
                ),
            )
            .with_span(s.span),
        );
    }

    fn check_field_access(&mut self, f: &FieldExpr) {
        // `ClassName.STATIC_FIELD` — recognize the static-access
        // shape before treating the receiver as a value. Visibility
        // applies the same as for instance fields; reading an
        // instance field via `ClassName.x` fires a clean diagnostic
        // so the user isn't told "no field `x`" when there IS one
        // but it lives on instances.
        if let Expr::Path(qn) = f.object.as_ref() {
            if let Some(class_fqn) = crate::infer::path_resolves_to_class(
                qn,
                &self.env,
                self.symbols,
            ) {
                let field_name = f.field.text.as_str();
                if let Some(field) = self
                    .symbols
                    .classes
                    .get(&class_fqn)
                    .and_then(|c| c.fields.get(field_name))
                {
                    if field.is_static {
                        let vis = field.visibility;
                        self.check_visibility(
                            vis,
                            &class_fqn,
                            field_name,
                            "static field",
                            f.span,
                        );
                    } else {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0412_UnresolvedField,
                                format!(
                                    "field `{field_name}` on `{class_fqn}` is an instance field; access it through a receiver, not the class name",
                                ),
                            )
                            .with_span(f.span),
                        );
                    }
                    return;
                }
                // Static property read (`Class.Prop`) — the getter is
                // a static method with `is_property = true`. Allow it
                // and enforce the getter's visibility.
                if let Some(method) = self
                    .symbols
                    .classes
                    .get(&class_fqn)
                    .and_then(|c| c.methods.get(field_name))
                {
                    if method.is_property {
                        self.check_visibility(
                            method.visibility,
                            &class_fqn,
                            field_name,
                            "property",
                            f.span,
                        );
                        return;
                    }
                }
                // No such field — surface E0412 against the class.
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0412_UnresolvedField,
                        format!("no static field `{field_name}` on class `{class_fqn}`"),
                    )
                    .with_span(f.span),
                );
                return;
            }
            // `IfaceName.CONST` — interface fields are implicitly
            // public static final (§3.3), so the receiver is a
            // type name in expression position just like a class
            // static. We resolve them the same way and emit a
            // clean E0412 when the field doesn't exist.
            if let Some(iface_fqn) = crate::infer::path_resolves_to_interface(
                qn,
                &self.env,
                self.symbols,
            ) {
                let field_name = f.field.text.as_str();
                if let Some(_field) = self
                    .symbols
                    .interfaces
                    .get(&iface_fqn)
                    .and_then(|i| i.fields.get(field_name))
                {
                    return;
                }
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0412_UnresolvedField,
                        format!(
                            "no static field `{field_name}` on interface `{iface_fqn}`",
                        ),
                    )
                    .with_span(f.span),
                );
                return;
            }
        }
        let receiver_ty = infer_expr(&f.object, &self.env, self.symbols);
        let field_name = f.field.text.as_str();

        // Tuple element access — `pair.0` (§5.3). Validate the index
        // against the element count so an out-of-range read gets a
        // clean E0412 instead of leaking rustc's E0609.
        // AsyncMutex guard (§18.3): `guard.value` is the protected T —
        // always a legal read/write.
        if let Ty::User { name, .. } = &receiver_ty {
            if name == "__AsyncMutexGuard" && field_name == "value" {
                return;
            }
        }
        if let Ty::User { name, generic_args } = &receiver_ty {
            if name == juxc_ast::TUPLE_SENTINEL {
                match field_name.parse::<usize>() {
                    Ok(idx) if idx < generic_args.len() => {}
                    _ => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0412_UnresolvedField,
                                format!(
                                    "no element `{field_name}` on tuple `{receiver_ty}` — valid indices are 0..{}",
                                    generic_args.len(),
                                ),
                            )
                            .with_span(f.span),
                        );
                    }
                }
                return;
            }
        }
        match &receiver_ty {
            // Arrays: allow .length and friends silently.
            Ty::Array { .. } => {
                if BUILTIN_ARRAY_FIELDS.contains(&field_name) {
                    return;
                }
                // Unknown field on array — stay quiet today. A future
                // pass may tighten this.
            }
            // Strings: same allowlist treatment.
            Ty::String => {
                if BUILTIN_STRING_FIELDS.contains(&field_name) {
                    return;
                }
            }
            // User types: walk the inheritance chain looking for the
            // field. Emit E0412 if not found anywhere. When the field
            // is found, verify visibility against the current
            // accessor context.
            Ty::User { name, .. } => {
                if let Some((field, declaring_class)) =
                    self.symbols.lookup_field(name, field_name)
                {
                    // **Field access through a polymorphic-base reference.**
                    // The receiver lowers to a `Rc<dyn …Kind>` trait object
                    // that can't expose struct fields directly — a generated
                    // `__get_<f>` / `__set_<f>` accessor handles **public /
                    // protected** fields (so those are allowed). A **private**
                    // field has no accessor and is unreachable through a base
                    // reference → E0437. `this` (concrete self) and concrete
                    // receivers are unaffected.
                    let recv_bare = name.rsplit('.').next().unwrap_or(name);
                    if !matches!(f.object.as_ref(), Expr::This(_))
                        && self.poly_bases.contains(recv_bare)
                    {
                        use juxc_ast::Visibility;
                        if matches!(field.visibility, Visibility::Public | Visibility::Protected) {
                            // Allowed — the backend rewrites to the accessor.
                            return;
                        }
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0437_FieldThroughPolymorphicBase,
                                format!(
                                    "private field `{field_name}` can't be accessed through a \
                                     `{recv_bare}` reference — `{recv_bare}` is a polymorphic base \
                                     (a dynamic-dispatch trait object), and a private field has no \
                                     accessor; make it public/protected, add a method, or hold the \
                                     value at its concrete type",
                                ),
                            )
                            .with_span(f.span),
                        );
                        return;
                    }
                    let vis = field.visibility;
                    let declaring = declaring_class.to_string();
                    self.check_visibility(
                        vis,
                        &declaring,
                        field_name,
                        "field",
                        f.span,
                    );
                    return;
                }
                // Expression-bodied property — `T name => expr;` —
                // is stored as a method with `is_property = true`.
                // From the user's perspective `obj.name` is a
                // field-shaped read; allow it here so tycheck
                // doesn't fire E0412.
                if let Some((method, _decl)) =
                    self.symbols.lookup_method(name, field_name)
                {
                    if method.is_property {
                        return;
                    }
                }
                // Records: check components directly. Record
                // components are always public per the spec (records
                // are simple data carriers), so no visibility check.
                if let Some(record) = self.symbols.records.get(name) {
                    if record.components.iter().any(|c| c.name == field_name) {
                        return;
                    }
                }
                // Enum variant access (`Color.Red`) lives on the enum
                // itself, not as a "field" in the symbol sense; the
                // receiver-name lookup against env was already Unknown
                // for these in practice, so we should only get here
                // when the receiver is actually a known class/record
                // type. Even so, suppress if the name is a known enum.
                if self.symbols.enums.contains_key(name) {
                    return;
                }
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0412_UnresolvedField,
                        format!("no field `{field_name}` on type `{name}`"),
                    )
                    .with_span(f.span),
                );
            }
            // Param receivers, Unknown, primitives — silent. We don't
            // know enough to flag a problem.
            _ => {}
        }
    }

    /// Resolve a call expression. Three shapes:
    ///
    /// - Bare path `foo(args)` → top-level function. Built-in `print`
    ///   accepts anything.
    /// - `obj.method(args)` → look up `method` on the receiver's class
    ///   (walking the chain). Built-in receivers (arrays, strings) get
    ///   the allowlist treatment. When the receiver carries concrete
    ///   generic args, each parameter type is substituted before
    ///   arg-type checking, so `new Box<int>(...).set("hi")` flags as
    ///   a mismatch instead of silently passing on the `Ty::Param`
    ///   wildcard.
    /// - Anything else → walk sub-expressions only.
    /// Emit `E0506` when an `unsafe` callee is invoked outside an `unsafe`
    /// context (`unsafe { … }` block or `unsafe` fn body). No-op when the
    /// callee is safe or we're already in an unsafe context. `name` is the
    /// callee for the message; `span` anchors the diagnostic.
    fn require_unsafe_context(&mut self, callee_is_unsafe: bool, name: &str, span: Span) {
        if callee_is_unsafe && !self.in_unsafe {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0506_UnsafeOpOutsideUnsafe,
                    format!(
                        "call to `unsafe` function `{name}` requires an `unsafe` block; \
                         wrap it in `unsafe {{ … }}` or mark the enclosing function `unsafe`",
                    ),
                )
                .with_span(span),
            );
        }
    }

    /// Declare every **const-generic parameter** in `params` as a VALUE
    /// in the current scope — the `N` of `<int N>` reads as an int
    /// (`return N;`, `head < N`), the `B` of `<bool B>` as a bool. A
    /// no-op for ordinary type params (referencing `T` as a value stays
    /// an error).
    fn declare_const_generic_params(&mut self, params: &[TypeParam]) {
        for p in params {
            let Some(cty) = &p.const_ty else { continue };
            let value_ty = match cty.name.segments.last().map(|s| s.text.as_str()) {
                Some("bool") => Ty::Primitive(Primitive::Bool),
                _ => Ty::Primitive(Primitive::Int),
            };
            self.env.declare(&p.name.text, value_ty);
            self.const_param_names.insert(p.name.text.clone());
        }
    }

    /// Type-position sibling of [`Self::check_const_size_expr`]: pull
    /// the size out of a `T[«size»]` declared type (field / local /
    /// param / return) and run the same const-arithmetic guard.
    fn check_fixed_array_size_in_type(&mut self, tref: &juxc_ast::TypeRef) {
        if let Some(juxc_ast::ArrayShape::Fixed(size)) = &tref.array_shape {
            let size = size.clone();
            self.check_const_size_expr(&size);
        }
    }

    /// Guard a **fixed-array size expression** (`new int[«size»]`,
    /// `int[«size»] field;`) against const-generic arithmetic. The size
    /// is a Rust const position: a const param may appear only as the
    /// BARE name (`[T; N]`) — anything computed over it (`N + 1`,
    /// `N * 2`) requires nightly `generic_const_exprs`, so it gets a
    /// clean **E0445** (deferred to the const-eval phase, spec §T.11.4)
    /// instead of a rustc leak. Sizes that don't mention a const param
    /// are left alone — their (pre-existing) validation is rustc's
    /// const-expr check.
    fn check_const_size_expr(&mut self, size: &Expr) {
        // Bare name or plain literal — always fine.
        if matches!(size, Expr::Literal(_) | Expr::Path(_)) {
            return;
        }
        if expr_mentions_name_of(size, &self.const_param_names) {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0445_ConstGenericUnsupported,
                    "const-generic arithmetic in array sizes (e.g. `N + 1`) is not \
                     supported in this phase — use the bare parameter (`[N]`) or a \
                     literal size",
                )
                .with_span(expr_span(size)),
            );
        }
    }

    /// Validate an **explicit call-site type-argument list** against the
    /// callee's declared generic params (spec turbofish `id<int>(5)`).
    /// Emits **E0443** when the callee isn't generic (no params to bind)
    /// or when the count doesn't match — both of which would otherwise
    /// leak `rustc`'s `E0107`. `callee_desc` is woven into the message
    /// (e.g. ``function `id` `` / ``method `pick` ``). A no-op when the
    /// caller wrote no explicit args.
    fn check_explicit_type_args(
        &mut self,
        explicit: &[TypeRef],
        generic_params: &[TypeParam],
        callee_desc: &str,
        span: Span,
    ) {
        if explicit.is_empty() {
            return;
        }
        if generic_params.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0443_ExplicitTypeArgs,
                    format!(
                        "{callee_desc} is not generic, so it takes no type arguments; \
                         remove the `<…>`",
                    ),
                )
                .with_span(span),
            );
            return;
        }
        if explicit.len() != generic_params.len() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0443_ExplicitTypeArgs,
                    format!(
                        "{callee_desc} expects {} type argument{}, but {} {} supplied",
                        generic_params.len(),
                        if generic_params.len() == 1 { "" } else { "s" },
                        explicit.len(),
                        if explicit.len() == 1 { "was" } else { "were" },
                    ),
                )
                .with_span(span),
            );
        }
        // **Slot-kind validation** (E0445): a const param (`<int N>`)
        // must receive a literal value, a type param must receive a
        // type. The synthetic literal `TypeRef` is recognized via
        // `const_literal_text` — without this check it would reach name
        // resolution / the emitted Rust and leak rustc E0747.
        for (param, arg) in generic_params.iter().zip(explicit.iter()) {
            let literal = arg.const_literal_text();
            match (&param.const_ty, literal) {
                // Type slot got a literal (`new Box<256>(…)`).
                (None, Some(lit)) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0445_ConstGenericUnsupported,
                            format!(
                                "`{}` is a type parameter, but `{lit}` is a constant value; \
                                 supply a type here",
                                param.name.text,
                            ),
                        )
                        .with_span(arg.span),
                    );
                }
                // Const slot got a type (`new Buf<String>()`).
                (Some(_), None) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0445_ConstGenericUnsupported,
                            format!(
                                "`{}` is a const-generic parameter — its argument must be a \
                                 compile-time literal (`4`, `true`), not a type or a runtime \
                                 value",
                                param.name.text,
                            ),
                        )
                        .with_span(arg.span),
                    );
                }
                // Const slot + literal: the literal's kind must match
                // the param's value type (`true` can't bind `<int N>`).
                (Some(cty), Some(lit)) => {
                    let param_is_bool = cty
                        .name
                        .segments
                        .last()
                        .map(|s| s.text == "bool")
                        .unwrap_or(false);
                    let lit_is_bool = lit == "true" || lit == "false";
                    if param_is_bool != lit_is_bool {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0445_ConstGenericUnsupported,
                                format!(
                                    "const-generic argument `{lit}` doesn't match the declared \
                                     value type of `{}`",
                                    param.name.text,
                                ),
                            )
                            .with_span(arg.span),
                        );
                    }
                }
                (None, None) => {}
            }
        }
    }

    /// **E0702** — reject class-typed objects captured by a
    /// `Worker.spawn` closure. The closure runs on another OS thread;
    /// Phase-1 objects are `Rc`-backed shared references (`!Send`), so
    /// the capture can never cross the boundary — rustc would reject
    /// the emitted `std::thread::spawn` with E0277. Detection: every
    /// bare name read inside the closure body (minus the closure's own
    /// params) that resolves in the CURRENT env to a class-typed value
    /// (`Ty::User`, possibly under `T?` / `T[]`) is a capture. Locals
    /// declared inside the closure aren't in the env yet, so they're
    /// naturally excluded.
    /// Capture types that legitimately cross task threads even
    /// though they're class-shaped at the Jux level — the async
    /// runtime's own handles (Arc-backed in the emitted helpers).
    fn capture_is_thread_safe(ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::User { name, .. }
                if matches!(
                    name.rsplit('.').next().unwrap_or(name),
                    "Channel" | "Task" | "AsyncMutex" | "AtomicInt" | "AtomicLong"
                )
        )
    }

    fn check_spawn_captures(&mut self, args: &[Expr]) {
        let Some(Expr::Lambda(l)) = args.first() else { return };
        let mut names: Vec<(String, Span)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut sink = |qn: &juxc_ast::QualifiedName| {
            if qn.segments.len() == 1 && seen.insert(qn.segments[0].text.clone()) {
                names.push((qn.segments[0].text.clone(), qn.span));
            }
        };
        match &l.body {
            juxc_ast::LambdaBody::Expr(e) => collect_bare_name_reads(e, &mut sink),
            juxc_ast::LambdaBody::Block(b) => {
                for s in &b.statements {
                    collect_bare_name_reads_stmt(s, &mut sink);
                }
            }
        }
        let params: std::collections::HashSet<&str> =
            l.params.iter().map(|p| p.name.text.as_str()).collect();
        for (name, span) in names {
            if params.contains(name.as_str()) {
                continue;
            }
            fn is_object_ty(ty: &Ty) -> bool {
                match ty {
                    Ty::User { .. } => true,
                    Ty::Nullable(inner) => is_object_ty(inner),
                    Ty::Array { element, .. } => is_object_ty(element),
                    _ => false,
                }
            }
            // Runtime handles (Channel, Task) are Arc-backed and
            // Send — they exist to cross task boundaries.
            if self
                .env
                .lookup(&name)
                .map(Self::capture_is_thread_safe)
                .unwrap_or(false)
            {
                continue;
            }
            if self.env.lookup(&name).map(is_object_ty).unwrap_or(false) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0702_ObjectCapturedBySpawn,
                        format!(
                            "`{name}` is a class object captured by a `Worker.spawn` \
                             closure — objects are single-threaded shared references and \
                             can't cross threads; pass primitive/String data in and \
                             return results out",
                        ),
                    )
                    .with_span(span),
                );
            }
        }
    }

    fn check_call(&mut self, c: &CallExpr) {
        // Always walk args first, regardless of callee shape, so nested
        // checks still fire.
        match c.callee.as_ref() {
            // `this(args)` — constructor delegation (§7.3.1). Only
            // meaningful inside a constructor body (the first-statement
            // rule is enforced by `check_constructor`); resolve the
            // sibling by argument count, reject self-delegation, and
            // run the ordinary per-arg checks against its params.
            Expr::This(span) => {
                let Some(current_idx) = self.current_ctor else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0210_ConstructorCallNotFirst,
                            "`this(...)` is only valid as the first statement of a constructor",
                        )
                        .with_span(*span),
                    );
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    return;
                };
                let Some(class_name) = self.env.current_class.clone() else { return };
                let Some(class) = self.symbols.classes.get(&class_name) else { return };
                let selected =
                    Self::select_ctor_by_count(&class.constructors, c.args.len());
                match selected {
                    Some(k) if k == current_idx => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0413_UnresolvedMethod,
                                "`this(...)` resolves to the declaring constructor itself — a constructor can't delegate to itself",
                            )
                            .with_span(c.span),
                        );
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                    }
                    Some(k) => {
                        self.ctor_selections.insert(c.span, k);
                        let params = class.constructors[k].params.clone();
                        let subst_params = class.generic_params.clone();
                        self.check_call_args(
                            &format!("this (={class_name} constructor)"),
                            &params,
                            &c.args,
                            &c.arg_names,
                            c.span,
                            Some(&class_name),
                            &subst_params,
                            &[],
                        );
                    }
                    None => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0411_WrongArgCount,
                                format!(
                                    "no constructor of `{class_name}` accepts {} argument{}",
                                    c.args.len(),
                                    if c.args.len() == 1 { "" } else { "s" },
                                ),
                            )
                            .with_span(c.span),
                        );
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                    }
                }
                return;
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = &qn.segments[0].text;
                // `assert(cond)` / `assert(cond, message)` (§S.7.2) —
                // the one builtin with a checked shape: 1-2 args, the
                // first must be bool, the optional second a String.
                if name == "assert" {
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    if c.args.is_empty() || c.args.len() > 2 {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0411_WrongArgCount,
                                format!(
                                    "`assert` takes a condition and an optional message, got {} arguments",
                                    c.args.len(),
                                ),
                            )
                            .with_span(c.span),
                        );
                        return;
                    }
                    let cond_ty = infer_expr(&c.args[0], &self.env, self.symbols);
                    if !compatible(&Ty::Primitive(Primitive::Bool), &cond_ty, self.symbols) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0410_TypeMismatch,
                                format!("`assert` condition must be bool, found {cond_ty}"),
                            )
                            .with_span(match expr_span(&c.args[0]) {
                                s if s == Span::DUMMY => c.span,
                                s => s,
                            }),
                        );
                    }
                    if let Some(msg) = c.args.get(1) {
                        let msg_ty = infer_expr(msg, &self.env, self.symbols);
                        if !compatible(&Ty::String, &msg_ty, self.symbols) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0410_TypeMismatch,
                                    format!("`assert` message must be a String, found {msg_ty}"),
                                )
                                .with_span(match expr_span(msg) {
                                    s if s == Span::DUMMY => c.span,
                                    s => s,
                                }),
                            );
                        }
                    }
                    return;
                }
                // `spawn(f)` (§18.1.3): the lambda's body runs on a
                // pool thread — same Send gate as Worker.spawn
                // (E0702: no wrapper-class captures).
                if name == "spawn" {
                    self.check_spawn_captures(&c.args);
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    return;
                }
                // Built-in functions accept anything.
                if BUILTINS.contains(&name.as_str()) {
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    return;
                }
                // Resolve the callee FQN: an exact bare key (same-package free
                // function), or an imported FQN — a foreign (`rust.libc.getpid`)
                // or cross-package free function brought into scope via
                // `import a.b.f`, keyed in the table by its full path.
                let resolved_fqn = if self.symbols.functions.contains_key(name.as_str()) {
                    Some(name.to_string())
                } else {
                    self.env
                        .unqualified
                        .get(name.as_str())
                        .cloned()
                        .filter(|fqn| self.symbols.functions.contains_key(fqn))
                };
                if let Some(fqn) = resolved_fqn {
                    let fn_sig = self
                        .symbols
                        .functions
                        .get(&fqn)
                        .expect("resolved_fqn is a known function key");
                    let params = fn_sig.params.clone();
                    let generic_params = fn_sig.generic_params.clone();
                    let callee_unsafe = fn_sig.is_unsafe;
                    // §X.1.3 propagation: the callee's declared
                    // checked throws raise here.
                    let callee_throws = fn_sig.throws.clone();
                    self.record_callee_throws(&callee_throws, c.span);
                    let callee_wheres = fn_sig.wheres.clone();
                    // §A.2.8: calling an `unsafe` fn needs an `unsafe` context.
                    self.require_unsafe_context(callee_unsafe, name, c.span);
                    // Validate any explicit `<…>` turbofish against the
                    // callee's declared type params (E0443).
                    self.check_explicit_type_args(
                        &c.explicit_generic_args,
                        &generic_params,
                        &format!("function `{name}`"),
                        c.span,
                    );
                    // Generic inference at the call site (spec §T.4):
                    // when the callee declares `<T>` and the user
                    // didn't write an explicit turbofish, recover the
                    // type args from the argument types so that
                    // per-arg checks below can substitute through the
                    // expected types.
                    let (subst_params, subst_args): (Vec<TypeParam>, Vec<Ty>) =
                        if generic_params.is_empty() {
                            (Vec::new(), Vec::new())
                        } else {
                            let param_tys: Vec<&TypeRef> =
                                params.iter().map(|p| &p.ty).collect();
                            let arg_tys: Vec<Ty> = c
                                .args
                                .iter()
                                .map(|a| infer_expr(a, &self.env, self.symbols))
                                .collect();
                            let inferred = infer_generic_args(
                                &generic_params,
                                &param_tys,
                                &arg_tys,
                            );
                            let args: Vec<Ty> = generic_params
                                .iter()
                                .map(|p| {
                                    inferred
                                        .get(&p.name.text)
                                        .cloned()
                                        .unwrap_or(Ty::Unknown)
                                })
                                .collect();
                            (generic_params, args)
                        };
                    // §O.5 (E0941): the instantiation must satisfy the
                    // callee's where-constraints.
                    self.enforce_where_constraints(
                        name,
                        &callee_wheres,
                        &subst_params,
                        &subst_args,
                        c.span,
                    );
                    self.check_call_args(
                        name,
                        &params,
                        &c.args,
                        &c.arg_names,
                        c.span,
                        None,
                        &subst_params,
                        &subst_args,
                    );
                    return;
                }
                // Unknown bare callee — walk args silently. The
                // resolver phase already flagged unresolved names.
                for arg in &c.args {
                    self.check_expr(arg);
                }
            }

            Expr::Field(field) => {
                let method_name = field.field.text.as_str();
                // Record the receiver's inferred type up front so the
                // backend can dispatch builtin/intrinsic methods on ANY
                // receiver shape — including compound expressions like
                // `(0.0 / 0.0).isNaN()` that no other recording path
                // visits. Type-name receivers record Unknown; harmless.
                self.infer_and_record(&field.object);
                // **`Worker.spawn(closure)` thread-capture gate (E0702).**
                // The closure crosses an OS-thread boundary, but Phase-1
                // objects are `Rc`-backed (`!Send`) — a class-typed
                // capture would leak rustc E0277. Checked on the bare
                // shape before resolution; valid spawns fall through.
                if method_name == "spawn" {
                    if let Expr::Path(qn) = field.object.as_ref() {
                        if qn.segments.len() == 1 && qn.segments[0].text == "Worker" {
                            self.check_spawn_captures(&c.args);
                        }
                    }
                }
                // `Task.all/race/delay` (§18.1.4) — runtime statics on
                // the emitted helpers; args are task handles (or a
                // millisecond count for delay).
                if let Expr::Path(qn) = field.object.as_ref() {
                    if qn.segments.len() == 1
                        && qn.segments[0].text == "Task"
                        && matches!(method_name, "all" | "race" | "delay")
                    {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                }
                // `ClassName.staticMethod(args)` — receiver is a
                // type name; resolve and check as a static call
                // before treating the object as a value. Mirrors
                // the static-field path in `check_field_access`.
                if let Expr::Path(qn) = field.object.as_ref() {
                    if let Some(class_fqn) = crate::infer::path_resolves_to_class(
                        qn,
                        &self.env,
                        self.symbols,
                    ) {
                        let class_method = match self.symbols.select_method_overload(
                            &class_fqn,
                            method_name,
                            c.args.len(),
                        ) {
                            Some((k, picked)) => {
                                self.method_selections.insert(c.span, k);
                                Some(picked.clone())
                            }
                            None => self
                                .symbols
                                .classes
                                .get(&class_fqn)
                                .and_then(|c| c.methods.get(method_name))
                                .cloned(),
                        };
                        if let Some(method) = class_method {
                            if method.is_static {
                                let vis = method.visibility;
                                self.check_visibility(
                                    vis,
                                    &class_fqn,
                                    method_name,
                                    "static method",
                                    c.span,
                                );
                                self.require_unsafe_context(
                                    method.is_unsafe,
                                    method_name,
                                    c.span,
                                );
                                self.check_call_args(
                                    method_name,
                                    &method.params,
                                    &c.args,
                                    &c.arg_names,
                                    c.span,
                                    Some(&class_fqn),
                                    &[],
                                    &[],
                                );
                                return;
                            } else {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0413_UnresolvedMethod,
                                        format!(
                                            "method `{method_name}` on `{class_fqn}` is an instance method; call it on an instance, not on the class name",
                                        ),
                                    )
                                    .with_span(c.span),
                                );
                                for arg in &c.args {
                                    self.check_expr(arg);
                                }
                                return;
                            }
                        }
                        // No such method on the class.
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0413_UnresolvedMethod,
                                format!(
                                    "no static method `{method_name}` on class `{class_fqn}`",
                                ),
                            )
                            .with_span(c.span),
                        );
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                    // `IfaceName.staticMethod(...)` — same shape,
                    // routed through the interface table. A static
                    // method dispatches normally; a default or
                    // abstract method called this way is an error
                    // (E0427) so users don't paper over the
                    // wrong-shape issue and silently miscompile.
                    if let Some(iface_fqn) = crate::infer::path_resolves_to_interface(
                        qn,
                        &self.env,
                        self.symbols,
                    ) {
                        let iface_method = self
                            .symbols
                            .interfaces
                            .get(&iface_fqn)
                            .and_then(|i| i.methods.get(method_name))
                            .cloned();
                        if let Some(method) = iface_method {
                            if method.is_static {
                                self.check_visibility(
                                    method.visibility,
                                    &iface_fqn,
                                    method_name,
                                    "static method",
                                    c.span,
                                );
                                self.check_call_args(
                                    method_name,
                                    &method.params,
                                    &c.args,
                                    &c.arg_names,
                                    c.span,
                                    Some(&iface_fqn),
                                    &[],
                                    &[],
                                );
                                return;
                            }
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0427_StaticCalledOnInstance,
                                    format!(
                                        "`{method_name}` on interface `{iface_fqn}` is not static; call it on an instance of an implementing class",
                                    ),
                                )
                                .with_span(c.span),
                            );
                            for arg in &c.args {
                                self.check_expr(arg);
                            }
                            return;
                        }
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0413_UnresolvedMethod,
                                format!(
                                    "no static method `{method_name}` on interface `{iface_fqn}`",
                                ),
                            )
                            .with_span(c.span),
                        );
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                }
                // Walk the receiver sub-expression first.
                self.check_expr(&field.object);
                let receiver_ty = infer_expr(&field.object, &self.env, self.symbols);
                // Built-in receivers: short-circuit.
                if let Ty::Array { .. } = &receiver_ty {
                    if BUILTIN_ARRAY_METHODS.contains(&method_name) {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                }
                if let Ty::String = &receiver_ty {
                    if BUILTIN_STRING_METHODS.contains(&method_name) {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                }
                // Map-typed receivers: short-circuit method-call
                // verification through the stdlib allowlist. Same
                // shape as the array / String path above.
                // HashMap / HashSet method short-circuit. These
                // stdlib types are compiler primitives — tycheck
                // accepts their method names from a small
                // hardcoded list without walking class-method
                // tables. The backend's `emit_map_stdlib_method`
                // / `emit_set_stdlib_method` produce the matching
                // Rust expressions.
                if let Ty::User { name, .. } = &receiver_ty {
                    let bare = name.rsplit('.').next().unwrap_or(name);
                    // Channel<T> (§18.3) is an async-runtime builtin —
                    // its methods live on the emitted JuxChannel
                    // helper, not a Jux class.
                    if bare == "AsyncMutex" && method_name == "lock" {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                    if bare == "Channel"
                        && matches!(method_name, "send" | "receive" | "close")
                    {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                    if bare == "HashMap" && BUILTIN_MAP_METHODS.contains(&method_name) {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                    if bare == "HashSet" && BUILTIN_SET_METHODS.contains(&method_name) {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                    if bare == "Deque" && BUILTIN_DEQUE_METHODS.contains(&method_name) {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                }
                // Skip method-resolution on Param / Unknown / primitive
                // receivers. We don't have the metadata to do better.
                let (name, generic_args) = match &receiver_ty {
                    Ty::User { name, generic_args } => (name.clone(), generic_args.clone()),
                    _ => {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                };
                // Walk the inheritance chain for the method, then
                // compose the substitution through the chain so
                // `extends Animal<int>` propagates `T → int` onto
                // an inherited method's param/return types.
                if let Some((method, declaring_class)) =
                    self.symbols.lookup_method(&name, method_name)
                {
                    // Overload-group pick (§T.3 Phase-1): when the
                    // receiver's class declares several same-name
                    // methods, the argument count selects the member;
                    // the recorded index drives `name__ovK` emission.
                    let method = match self
                        .symbols
                        .select_method_overload(&name, method_name, c.args.len())
                    {
                        Some((k, picked)) => {
                            self.method_selections.insert(c.span, k);
                            picked
                        }
                        None => method,
                    };
                    let params = method.params.clone();
                    let method_generic_params = method.generic_params.clone();
                    let method_vis = method.visibility;
                    let method_throws = method.throws.clone();
                    self.record_callee_throws(&method_throws, c.span);
                    let method_is_static = method.is_static;
                    let method_is_unsafe = method.is_unsafe;
                    // Clone the declaring-class name into an owned
                    // String so it outlives the immutable borrow on
                    // `self.symbols` we'd otherwise need.
                    let owner_name = declaring_class.to_string();
                    // Java rule: a `static` method must be called via
                    // its declaring type, not an instance. `obj.foo()`
                    // where `foo` is static is misleading because the
                    // receiver doesn't participate in dispatch. We
                    // diagnose at the call site rather than letting
                    // the backend miscompile or rustc complain
                    // downstream.
                    if method_is_static {
                        self.diagnostics.push(
                            juxc_diagnostics::Diagnostic::error(
                                code::Code::E0427_StaticCalledOnInstance,
                                format!(
                                    "`{method_name}` is a static method on `{owner_name}`; call it as `{owner_name}.{method_name}(...)`, not on an instance",
                                ),
                            )
                            .with_span(c.span),
                        );
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                    // Visibility check (E0414 / E0415 / E0416) —
                    // run after cloning out the fields we need so
                    // the symbol-table borrow ends before the
                    // diagnostic-pushing helper grabs `&mut self`.
                    self.check_visibility(
                        method_vis,
                        &owner_name,
                        method_name,
                        "method",
                        c.span,
                    );
                    self.require_unsafe_context(method_is_unsafe, method_name, c.span);
                    // Validate any explicit `<…>` turbofish against the
                    // method's own type params (E0443).
                    self.check_explicit_type_args(
                        &c.explicit_generic_args,
                        &method_generic_params,
                        &format!("method `{method_name}`"),
                        c.span,
                    );
                    let (mut subst_params, mut subst_args): (Vec<TypeParam>, Vec<Ty>) =
                        compose_extends_substitution(
                            &name,
                            &generic_args,
                            &owner_name,
                            self.symbols,
                        )
                        .unwrap_or_default();
                    // Method-level generic inference (spec §T.4).
                    self.append_method_generic_inference(
                        &method_generic_params,
                        &params,
                        &c.args,
                        &mut subst_params,
                        &mut subst_args,
                    );
                    self.check_call_args(
                        method_name,
                        &params,
                        &c.args,
                        &c.arg_names,
                        c.span,
                        Some(&owner_name),
                        &subst_params,
                        &subst_args,
                    );
                    return;
                }
                // Interfaces — same lookup (no chain). Substitute the
                // interface's generic params against the receiver's
                // args; the interface IS the declaring scope here so
                // there's no cross-extends complication.
                if let Some(iface) = self.symbols.interfaces.get(&name) {
                    if let Some(method) = iface.methods.get(method_name) {
                        // Same static-via-instance check as the
                        // class path above. Receiver here is a
                        // value typed by an interface, so a static
                        // method on it would still need to be
                        // called as `Iface.foo(...)`.
                        if method.is_static {
                            self.diagnostics.push(
                                juxc_diagnostics::Diagnostic::error(
                                    code::Code::E0427_StaticCalledOnInstance,
                                    format!(
                                        "`{method_name}` is a static method on `{name}`; call it as `{name}.{method_name}(...)`, not on an instance",
                                    ),
                                )
                                .with_span(c.span),
                            );
                            for arg in &c.args {
                                self.check_expr(arg);
                            }
                            return;
                        }
                        let params = method.params.clone();
                        let method_generic_params = method.generic_params.clone();
                        let mut subst_params = iface.generic_params.clone();
                        let mut subst_args = generic_args.clone();
                        self.append_method_generic_inference(
                            &method_generic_params,
                            &params,
                            &c.args,
                            &mut subst_params,
                            &mut subst_args,
                        );
                        self.check_call_args(
                            method_name,
                            &params,
                            &c.args,
                            &c.arg_names,
                            c.span,
                            Some(&name),
                            &subst_params,
                            &subst_args,
                        );
                        return;
                    }
                }
                // Records can declare methods (per grammar §A.2.4).
                // Same lookup shape as interfaces — records have no
                // inheritance chain, so substitution applies for the
                // record's own generic params.
                if let Some(record) = self.symbols.records.get(&name) {
                    if let Some(method) = record.methods.get(method_name) {
                        let params = method.params.clone();
                        let method_generic_params = method.generic_params.clone();
                        let mut subst_params = record.generic_params.clone();
                        let mut subst_args = generic_args.clone();
                        self.append_method_generic_inference(
                            &method_generic_params,
                            &params,
                            &c.args,
                            &mut subst_params,
                            &mut subst_args,
                        );
                        self.check_call_args(
                            method_name,
                            &params,
                            &c.args,
                            &c.arg_names,
                            c.span,
                            Some(&name),
                            &subst_params,
                            &subst_args,
                        );
                        return;
                    }
                }
                // Enum methods (§A.2.5) — same no-chain lookup shape
                // as records.
                if let Some(enum_sig) = self.symbols.enums.get(&name) {
                    if let Some(method) = enum_sig.methods.get(method_name) {
                        let params = method.params.clone();
                        self.check_call_args(
                            method_name,
                            &params,
                            &c.args,
                            &c.arg_names,
                            c.span,
                            Some(&name),
                            &[],
                            &[],
                        );
                        return;
                    }
                }
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0413_UnresolvedMethod,
                        format!("no method `{method_name}` on type `{name}`"),
                    )
                    .with_span(c.span),
                );
            }

            // Some other callee shape (call-of-call, call-of-index,
            // etc.) — walk sub-expressions only.
            _ => {
                self.check_expr(&c.callee);
                for arg in &c.args {
                    self.check_expr(arg);
                }
            }
        }
    }

    /// Resolve a `new T(args)`. Looks up `T` against classes first,
    /// then records (records have a synthesized canonical constructor
    /// matching their components). Emits **E0413** when the class/record
    /// isn't found, **E0411** on arg-count mismatch, and **E0410** for
    /// each per-argument type mismatch.
    ///
    /// When the user wrote explicit generic args (`new Box<int>(42)`),
    /// each parameter type carrying a `Ty::Param("T")` is substituted
    /// through those args before comparison. `new Box(42)` (no
    /// turbofish) leaves substitution off — the wildcard rule in
    /// [`compatible`] then accepts whatever argument the user passed.
    fn check_new_object(&mut self, n: &NewObjectExpr) {
        // Walk arg expressions for nested checks regardless of resolution.
        for arg in &n.args {
            self.check_expr(arg);
        }
        // FQN-aware resolution (same map `ty_from_ref` consults) —
        // the symbol table keys classes by FQN, so the old
        // bare-last-segment lookup silently skipped constructor
        // checking in any file with a `package` declaration.
        let class_name = crate::infer::resolve_class_name(&n.class_name, &self.env, self.symbols);
        if class_name.is_empty() {
            return;
        }

        // Lower the explicit generic args (if any) into `Ty`s. Empty
        // when the user wrote the bare `new Box(...)` form — in that
        // case we'll try inference (spec §T.4) below.
        let explicit_generic_args: Vec<Ty> = n
            .generic_args
            .iter()
            .map(|g| ty_from_ref(g, &self.env, self.symbols))
            .collect();

        // Validate explicit args against the class's declared params —
        // const-vs-type slot kind + literal-kind checks (E0445). A
        // const-generic class also REQUIRES the explicit form: its
        // value can't be inferred from constructor args.
        // External (`.jux.d` stub) classes are exempt: their stubs
        // mirror Rust signatures with DEFAULTED generic params (e.g.
        // `Vec<T, A = Global>`), which Jux call sites legitimately
        // omit — rustc validates those for real.
        if let Some(class) = self
            .symbols
            .classes
            .get(&class_name)
            .filter(|c| !c.is_external)
        {
            let class_generic_params = class.generic_params.clone();
            if !n.generic_args.is_empty() {
                self.check_explicit_type_args(
                    &n.generic_args,
                    &class_generic_params,
                    &format!("class `{class_name}`"),
                    n.span,
                );
            } else if class_generic_params.iter().any(|p| p.is_const()) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0445_ConstGenericUnsupported,
                        format!(
                            "`{class_name}` declares const-generic parameters — write them \
                             explicitly (`new {class_name}<…>(…)`); const values can't be \
                             inferred from constructor arguments",
                        ),
                    )
                    .with_span(n.span),
                );
            }
        }

        if let Some(class) = self.symbols.classes.get(&class_name) {
            // Abstract classes can't be instantiated directly —
            // only concrete subclasses can satisfy the `new`. Fire
            // E0428 with the abstract-class-specific message so
            // users know to extend the class rather than chase the
            // synthesized constructor.
            if class.is_abstract {
                // Anonymous-class form (`new AbstractC() { … overrides }`)
                // creates a synthetic concrete subclass at the use
                // site that supplies the abstract methods, so it's
                // the one legal `new AbstractC(...)` shape — let it
                // through. Plain `new AbstractC(...)` without a body
                // still errors with the usual subclass-required
                // message.
                if n.anonymous_body.is_none() {
                    self.diagnostics.push(
                        juxc_diagnostics::Diagnostic::error(
                            code::Code::E0428_CannotInstantiate,
                            format!(
                                "cannot instantiate `{class_name}`: it's an abstract class. Subclass it with a concrete class and instantiate that instead.",
                            ),
                        )
                        .with_span(n.span),
                    );
                    return;
                }
                // Anonymous-class form against an abstract class
                // skips constructor-arg checking and returns;
                // backend emission handles the synthesis.
                return;
            }
            // Constructor-overload selection by argument count
            // (§7.3.1, Phase-1 rule — ranges validated disjoint at
            // the declaration). A miss falls back to the first
            // declared constructor so arg-count/type errors report
            // against SOMETHING sensible; no class constructors at
            // all means the synthesized zero-arg default.
            let selected = Self::select_ctor_by_count(&class.constructors, n.args.len())
                .unwrap_or(0);
            if !class.constructors.is_empty() {
                self.ctor_selections.insert(n.span, selected);
            }
            let params: Vec<ParamSig> = class
                .constructors
                .get(selected)
                .map(|c| c.params.clone())
                .unwrap_or_default();
            let ctor_vis = class
                .constructors
                .get(selected)
                .map(|c| c.visibility)
                .unwrap_or(juxc_ast::Visibility::Public);
            let subst_params = class.generic_params.clone();
            // Visibility check on the constructor itself (E0414 /
            // E0415 / E0416). A synthetic default constructor on a
            // class with no declared ctors is treated as `public`.
            self.check_visibility(
                ctor_vis,
                &class_name,
                "constructor",
                "constructor",
                n.span,
            );
            let subst_args = self.resolve_ctor_generic_args(
                &subst_params,
                &explicit_generic_args,
                &params,
                &n.args,
            );
            self.check_call_args(
                &class_name,
                &params,
                &n.args,
                &n.arg_names,
                n.span,
                Some(&class_name),
                &subst_params,
                &subst_args,
            );
            return;
        }
        if let Some(record) = self.symbols.records.get(&class_name) {
            // Canonical constructor: one param per component.
            let params: Vec<ParamSig> = record
                .components
                .iter()
                .map(|c| ParamSig {
                    name: c.name.clone(),
                    ty: c.ty.clone(),
                    is_ref: false,
                    default: None,
                    is_varargs: false,
                })
                .collect();
            let subst_params = record.generic_params.clone();
            let subst_args = self.resolve_ctor_generic_args(
                &subst_params,
                &explicit_generic_args,
                &params,
                &n.args,
            );
            self.check_call_args(
                &class_name,
                &params,
                &n.args,
                &n.arg_names,
                n.span,
                Some(&class_name),
                &subst_params,
                &subst_args,
            );
            return;
        }
        // `new` against an interface, enum, or other non-class type:
        // the resolver already finds the name, so we wouldn't be
        // double-counting an E0301 — and the lowered Rust would
        // otherwise reach rustc as a confusing E0782 ("expected a
        // type, found a trait"). Emit E0428 instead so users see a
        // Jux-level explanation.
        //
        // **Exception:** `new Iface() { body }` is the
        // anonymous-class form (spec §1379) — the body's method
        // overrides synthesize a concrete impl at the use site.
        // It's the only legal `new Iface(...)` shape and is
        // explicitly allowed.
        let kind = if self.symbols.interfaces.contains_key(&class_name) {
            Some("interface")
        } else if self.symbols.enums.contains_key(&class_name) {
            Some("enum")
        } else {
            None
        };
        if let Some(kind) = kind {
            if n.anonymous_body.is_some() && kind == "interface" {
                // Skip E0428 — anonymous-class form is valid.
                return;
            }
            self.diagnostics.push(
                juxc_diagnostics::Diagnostic::error(
                    code::Code::E0428_CannotInstantiate,
                    format!(
                        "cannot instantiate `{class_name}`: it's an {kind}, not a class. Implement {kind} `{class_name}` on a class and instantiate that instead.",
                    ),
                )
                .with_span(n.span),
            );
            return;
        }
        // Not a known class, record, interface, or enum. Stay silent
        // if the resolver already flagged the name (it lands in
        // `resolve` as E0301); emitting a parallel E0413 would be
        // double-counting.
    }

    /// Resolve a `super(args)` invocation inside a constructor body
    /// against the parent class's constructor signature. Reuses
    /// [`Self::check_call_args`] so the same E0410 / E0411 codes apply.
    ///
    /// Substitution: when the child writes `extends Animal<int>`, every
    /// `Ty::Param("T")` in Animal's constructor signature is mapped
    /// through that `int` before comparison. A bare `extends Animal`
    /// (no explicit args) leaves substitution off; the wildcard rule
    /// in [`compatible`] then accepts whatever the user passed.
    ///
    /// Stays silent on shapes Phase E can't decide:
    ///
    /// - Outside a class context (`env.current_class` is `None`) — the
    ///   parser already rejects bare `super(...)`, but be defensive.
    /// - The child has no `extends` clause.
    /// - The parent name doesn't resolve to a known class (extends a
    ///   built-in or an unresolved name — the resolver will have
    ///   already complained about the latter).
    fn check_super_call(&mut self, args: &[Expr], call_span: Span) {
        // Walk arg sub-expressions for nested checks even if we can't
        // resolve the parent — keeps E0410/E0413 from earlier passes
        // firing inside the args.
        for arg in args {
            self.check_expr(arg);
        }

        let Some(child_name) = self.env.current_class.clone() else { return };
        let Some(child) = self.symbols.classes.get(&child_name) else { return };
        let Some(extends) = child.extends.as_ref() else { return };
        // Prefer the resolved-at-build-time FQN so cross-package
        // `super(...)` calls find the parent class. Fall back to
        // the bare last segment for single-unit / no-package builds.
        let parent_name: String = child
            .extends_fqn
            .clone()
            .unwrap_or_else(|| {
                extends
                    .name
                    .segments
                    .last()
                    .map(|s| s.text.clone())
                    .unwrap_or_default()
            });

        // Lower the extends-clause generic args. `extends Animal<int>`
        // gives us [Int]; `extends Animal` gives us []. Empty disables
        // substitution per `substitute`'s rules.
        let parent_generic_args: Vec<Ty> = extends
            .generic_args
            .iter()
            .map(|g| match g.as_type() {
                Some(t) => ty_from_ref(t, &self.env, self.symbols),
                None => Ty::Unknown,
            })
            .collect();

        let Some(parent) = self.symbols.classes.get(&parent_name) else { return };
        // Parent-constructor overload selection — same count rule as
        // `new` sites; the backend reads the recorded index when it
        // builds `__parent: Parent::new__K(args)`.
        let selected = Self::select_ctor_by_count(&parent.constructors, args.len())
            .unwrap_or(0);
        if !parent.constructors.is_empty() {
            self.ctor_selections.insert(call_span, selected);
        }
        let params: Vec<ParamSig> = parent
            .constructors
            .get(selected)
            .map(|c| c.params.clone())
            .unwrap_or_default();
        let subst_params = parent.generic_params.clone();

        // Clone the slice off so we can re-borrow `self` mutably for the
        // arg-check walk without overlapping the immutable borrow above.
        let params_owned = params;
        self.check_call_args(
            &format!("super (={parent_name})"),
            &params_owned,
            args,
            &[],
            call_span,
            Some(&parent_name),
            &subst_params,
            &parent_generic_args,
        );
    }

    /// Shared core for argument-count + per-argument type-check. Used
    /// by top-level fn calls, method calls, and constructor calls.
    /// Emits **E0411** for count mismatch and **E0410** per-arg.
    ///
    /// `callee_name` is just for diagnostic phrasing.
    ///
    /// `declaring_class` is the name of the type that owns the
    /// parameter list (for member calls and constructors). It lets the
    /// checker lower a parameter `T value` to `Ty::Param("T")` even
    /// when called from outside the declaring class's body, where the
    /// checker's own env wouldn't have `T` registered. Pass `None` for
    /// top-level function calls — those parameters lower against the
    /// caller's env, where free-function generic params would be in
    /// scope (when free-function generics get wired up).
    ///
    /// `subst_params` / `subst_args` carry an optional generic
    /// substitution that's applied to each expected parameter type
    /// before comparison — see [`crate::ty::substitute`] for the
    /// rules. Pass empty slices when no substitution applies (top-level
    /// function calls, calls on non-generic receivers, calls whose
    /// receiver is a raw type).
    #[allow(clippy::too_many_arguments)]
    /// Resolve the final substitution-arg list for a `new Foo(...)` /
    /// `new MyRecord(...)` site. Explicit `<...>` always wins; when
    /// the user wrote the bare form and the type is generic, infer
    /// from the constructor's parameter types vs the actual arg types
    /// (spec §T.4). Returns an empty vec when the type isn't generic
    /// — `substitute` short-circuits on a 0-length params list, so an
    /// empty subst_args is the natural pass-through.
    fn resolve_ctor_generic_args(
        &self,
        generic_params: &[TypeParam],
        explicit_args: &[Ty],
        ctor_params: &[ParamSig],
        call_args: &[Expr],
    ) -> Vec<Ty> {
        if !explicit_args.is_empty() {
            return explicit_args.to_vec();
        }
        if generic_params.is_empty() {
            return Vec::new();
        }
        let param_tys: Vec<&TypeRef> = ctor_params.iter().map(|p| &p.ty).collect();
        let arg_tys: Vec<Ty> = call_args
            .iter()
            .map(|a| infer_expr(a, &self.env, self.symbols))
            .collect();
        let inferred = infer_generic_args(generic_params, &param_tys, &arg_tys);
        generic_params
            .iter()
            .map(|p| inferred.get(&p.name.text).cloned().unwrap_or(Ty::Unknown))
            .collect()
    }

    /// Append method-level generic inference (spec §T.4) onto an
    /// existing `(subst_params, subst_args)` pair. The class/record/
    /// interface generics are already filled in by the caller; this
    /// extends the substitution table with the method's own generic
    /// params, inferring concrete arguments from the call's actual
    /// arg types. Only the bare-param-name shape is handled — see
    /// [`infer_generic_args`].
    fn append_method_generic_inference(
        &self,
        method_generic_params: &[TypeParam],
        method_params: &[ParamSig],
        call_args: &[Expr],
        subst_params: &mut Vec<TypeParam>,
        subst_args: &mut Vec<Ty>,
    ) {
        if method_generic_params.is_empty() {
            return;
        }
        let param_tys: Vec<&TypeRef> = method_params.iter().map(|p| &p.ty).collect();
        let arg_tys: Vec<Ty> = call_args
            .iter()
            .map(|a| infer_expr(a, &self.env, self.symbols))
            .collect();
        let inferred =
            infer_generic_args(method_generic_params, &param_tys, &arg_tys);
        for p in method_generic_params {
            subst_args.push(
                inferred.get(&p.name.text).cloned().unwrap_or(Ty::Unknown),
            );
        }
        subst_params.extend(method_generic_params.iter().cloned());
    }

    /// Select a constructor overload by **argument count** (Phase-1
    /// rule, §7.3.1): the unique constructor whose acceptable-count
    /// range `[required ..= max]` covers `arg_count`. Declaration-time
    /// validation (`check_constructor_overloads`) guarantees ranges
    /// are pairwise disjoint, so at most one matches. `None` when no
    /// constructor accepts the count (the caller falls back to the
    /// first for error reporting) or when the class declares none.
    fn select_ctor_by_count(
        ctors: &[crate::symbol_table::ConstructorSig],
        arg_count: usize,
    ) -> Option<usize> {
        ctors.iter().position(|c| {
            let (lo, hi) = crate::symbol_table::ctor_arity_range(&c.params);
            arg_count >= lo && hi.map_or(true, |h| arg_count <= h)
        })
    }

    /// Variadic-callee arm of [`Self::check_call_args`]: positional
    /// args fill the fixed prefix; the rest type-check against the
    /// varargs ELEMENT type and pack into a synthesized array literal
    /// via the recorded `Variadic` plan slot. A single trailing array
    /// of the element type forwards as-is (no packing).
    #[allow(clippy::too_many_arguments)]
    fn check_varargs_call(
        &mut self,
        callee_name: &str,
        params: &[ParamSig],
        args: &[Expr],
        call_span: Span,
        declaring_class: Option<&str>,
        subst_params: &[TypeParam],
        subst_args: &[Ty],
    ) {
        let fixed = params.len() - 1;
        if args.len() < fixed {
            let missing: Vec<String> = params[args.len()..fixed]
                .iter()
                .filter(|p| p.default.is_none())
                .map(|p| format!("`{}`", p.name))
                .collect();
            if !missing.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0411_WrongArgCount,
                        format!(
                            "missing required argument{} {} in call to `{}`",
                            if missing.len() == 1 { "" } else { "s" },
                            missing.join(", "),
                            callee_name,
                        ),
                    )
                    .with_span(call_span),
                );
                return;
            }
        }
        // Fixed prefix — same per-slot type checks as the plain path.
        let lower = |param: &ParamSig, this: &Self| -> Ty {
            let raw = match declaring_class {
                Some(class) => lower_member_type(&param.ty, class, this.symbols),
                None => ty_from_ref(&param.ty, &this.env, this.symbols),
            };
            substitute(&raw, subst_params, subst_args)
        };
        for (i, arg) in args.iter().enumerate() {
            self.check_expr(arg);
            if i >= fixed {
                continue;
            }
            let expected = lower(&params[i], self);
            let found = infer_expr(arg, &self.env, self.symbols);
            if !compatible(&expected, &found, self.symbols) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0410_TypeMismatch,
                        format!(
                            "argument {} to `{}`: expected {}, found {}",
                            i + 1,
                            callee_name,
                            expected,
                            found,
                        ),
                    )
                    .with_span(expr_span(arg)),
                );
            }
        }
        let va = &params[fixed];
        let va_array_ty = lower(va, self);
        // Element type: the declared array type minus its shape.
        let mut element_type = va.ty.clone();
        element_type.array_shape = None;
        let element_ty = match &va_array_ty {
            Ty::Array { element, .. } => (**element).clone(),
            other => other.clone(),
        };
        let variadic: Vec<usize> = (fixed..args.len()).collect();
        // Array passthrough: exactly one trailing arg whose type is
        // already `T[]` forwards directly — plain positional call,
        // no plan needed beyond defaults for the fixed prefix.
        let passthrough = variadic.len() == 1 && {
            let found = infer_expr(&args[fixed], &self.env, self.symbols);
            compatible(&va_array_ty, &found, self.symbols)
                && matches!(found, Ty::Array { .. })
        };
        if !passthrough {
            for &i in &variadic {
                let found = infer_expr(&args[i], &self.env, self.symbols);
                if !compatible(&element_ty, &found, self.symbols) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!(
                                "argument {} to `{}`: expected {} (the `{}...` element type), found {}",
                                i + 1,
                                callee_name,
                                element_ty,
                                element_ty,
                                found,
                            ),
                        )
                        .with_span(match expr_span(&args[i]) {
                            s if s == Span::DUMMY => call_span,
                            s => s,
                        }),
                    );
                }
            }
        }
        // Record the plan. Skipped only for the no-op shape: full
        // fixed prefix supplied AND a passthrough array (the call is
        // already plain positional).
        let fixed_complete = args.len() >= fixed
            && params[..fixed.min(args.len())].len() == fixed;
        if passthrough && fixed_complete {
            return;
        }
        let mut plan: Vec<crate::ArgSource> = Vec::with_capacity(params.len());
        for (j, p) in params.iter().enumerate().take(fixed) {
            if j < args.len() {
                plan.push(crate::ArgSource::Explicit(j));
            } else if let Some(d) = &p.default {
                plan.push(crate::ArgSource::Default(d.clone()));
            } else {
                return; // missing-required already reported above
            }
        }
        if passthrough {
            plan.push(crate::ArgSource::Explicit(fixed));
        } else {
            plan.push(crate::ArgSource::Variadic { element_type, indices: variadic });
        }
        self.call_expansions.insert(call_span, plan);
    }

    fn check_call_args(
        &mut self,
        callee_name: &str,
        params: &[ParamSig],
        args: &[Expr],
        arg_names: &[Option<juxc_ast::Ident>],
        call_span: Span,
        declaring_class: Option<&str>,
        subst_params: &[TypeParam],
        subst_args: &[Ty],
    ) {
        // ---- variadic callee (§7.2 / §E.1.2.1) ----
        //
        // The last parameter being `T...` switches the mapping:
        // args fill the fixed prefix left-to-right, every trailing
        // arg packs into a synthesized `T[]` literal (the recorded
        // plan's `Variadic` slot). Passing ONE array of `T` forwards
        // it directly (Java's array-passthrough rule).
        if params.last().is_some_and(|p| p.is_varargs) {
            if arg_names.iter().any(Option::is_some) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0448_BadNamedArgument,
                        format!(
                            "named arguments can't be combined with a variadic call to `{callee_name}` (Phase 1) — pass everything positionally",
                        ),
                    )
                    .with_span(call_span),
                );
                return;
            }
            self.check_varargs_call(
                callee_name,
                params,
                args,
                call_span,
                declaring_class,
                subst_params,
                subst_args,
            );
            return;
        }
        // ---- argument-to-slot mapping (§T.3.2 / §S.1.4) ----
        //
        // Positional args fill parameter slots left-to-right; named
        // args fill the slot their label names; every slot at most
        // once. Slots left empty must carry a default (§S.1.3) — the
        // recorded expansion plan clones the default into the call
        // site, so the backend only ever sees full positional calls.
        let has_named = arg_names.iter().any(Option::is_some);
        // `arg_to_param[i]` = the parameter slot arg `i` lands in.
        let mut arg_to_param: Vec<Option<usize>> = vec![None; args.len()];
        // `param_filled[j]` = the arg index that filled slot `j`.
        let mut param_filled: Vec<Option<usize>> = vec![None; params.len()];
        let mut seen_named = false;
        let mut mapping_broken = false;
        for i in 0..args.len() {
            let label = arg_names.get(i).and_then(|n| n.as_ref());
            match label {
                None => {
                    if seen_named {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0448_BadNamedArgument,
                                format!(
                                    "positional argument after a named one in call to `{callee_name}` — \
                                     once a label appears, every later argument must be labeled",
                                ),
                            )
                            .with_span(match expr_span(&args[i]) {
                                s if s == Span::DUMMY => call_span,
                                s => s,
                            }),
                        );
                        mapping_broken = true;
                        break;
                    }
                    if i < params.len() {
                        arg_to_param[i] = Some(i);
                        param_filled[i] = Some(i);
                    }
                }
                Some(ident) => {
                    seen_named = true;
                    match params.iter().position(|p| p.name == ident.text) {
                        None => {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0448_BadNamedArgument,
                                    format!(
                                        "`{}` has no parameter named `{}`",
                                        callee_name, ident.text,
                                    ),
                                )
                                .with_span(ident.span),
                            );
                            mapping_broken = true;
                        }
                        Some(j) => {
                            if param_filled[j].is_some() {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0448_BadNamedArgument,
                                        format!(
                                            "parameter `{}` of `{}` is supplied more than once",
                                            ident.text, callee_name,
                                        ),
                                    )
                                    .with_span(ident.span),
                                );
                                mapping_broken = true;
                            } else {
                                param_filled[j] = Some(i);
                                arg_to_param[i] = Some(j);
                            }
                        }
                    }
                }
            }
        }
        // Arity: more arguments than parameter slots.
        if args.len() > params.len() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0411_WrongArgCount,
                    format!(
                        "`{}` expects at most {} argument{}, got {}",
                        callee_name,
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        args.len(),
                    ),
                )
                .with_span(call_span),
            );
        }
        let mut missing: Vec<&str> = Vec::new();
        if !mapping_broken {
            for (j, p) in params.iter().enumerate() {
                if param_filled[j].is_none() && p.default.is_none() {
                    missing.push(p.name.as_str());
                }
            }
        }
        if !missing.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0411_WrongArgCount,
                    format!(
                        "missing required argument{} {} in call to `{}`",
                        if missing.len() == 1 { "" } else { "s" },
                        missing
                            .iter()
                            .map(|n| format!("`{n}`"))
                            .collect::<Vec<_>>()
                            .join(", "),
                        callee_name,
                    ),
                )
                .with_span(call_span),
            );
        }
        // Record the expansion plan when the call used sugar (named
        // args and/or omitted defaults) and mapped cleanly. The driver
        // applies it to the AST before the backend runs.
        if !mapping_broken
            && missing.is_empty()
            && args.len() <= params.len()
            && (has_named || args.len() < params.len())
        {
            let plan: Vec<crate::ArgSource> = params
                .iter()
                .enumerate()
                .map(|(j, p)| match param_filled[j] {
                    Some(i) => crate::ArgSource::Explicit(i),
                    None => crate::ArgSource::Default(
                        p.default.clone().expect("missing-default slots reported above"),
                    ),
                })
                .collect();
            self.call_expansions.insert(call_span, plan);
        }
        for (i, arg) in args.iter().enumerate() {
            self.check_expr(arg);
            let Some(param) = arg_to_param[i].and_then(|j| params.get(j)) else { continue };
            let expected_raw = match declaring_class {
                Some(class) => lower_member_type(&param.ty, class, self.symbols),
                None => ty_from_ref(&param.ty, &self.env, self.symbols),
            };
            let expected = substitute(&expected_raw, subst_params, subst_args);
            let found = infer_expr(arg, &self.env, self.symbols);
            if !compatible(&expected, &found, self.symbols) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0410_TypeMismatch,
                        format!(
                            "argument {} to `{}`: expected {}, found {}",
                            i + 1,
                            callee_name,
                            expected,
                            found,
                        ),
                    )
                    .with_span(expr_span(arg)),
                );
            }
        }
    }

}

// ============================================================================
// Helpers
// ============================================================================

/// Lower a [`ReturnType`] to a [`Ty`]. Duplicated from `infer.rs` so the
/// checker can use it without exporting an internal helper. `async T`
/// unwraps to `T` (no `Future<T>` wrapper in Phase 1).
fn return_type_to_ty(rt: &ReturnType, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    match rt {
        ReturnType::Void => Ty::Void,
        ReturnType::Type(t) | ReturnType::AsyncType(t) => ty_from_ref(t, env, symbols),
    }
}

/// True iff a condition expression's type is acceptable in a boolean
/// position — exactly `bool` or `Unknown` (suppression).
fn is_boolish(ty: &Ty) -> bool {
    ty.is_unknown() || ty.is_bool()
}

/// Map a [`BinaryOp`] to the [`OperatorKind`] whose deletion would
/// suppress this op. `None` for ops that aren't user-overloadable
/// (logical `&&` / `||`) or that auto-derive from another operator
/// at the Rust level (`!=` derives from `==`, the four ordering ops
/// auto-derive from `<=>`). Phase-1 simplification: only the
/// "primary" operator is checked — if the user deleted `==` but the
/// program writes `a != b`, the deletion goes uncaught here. A future
/// pass can chase the auto-derive graph.
fn op_kind_for_binary(op: BinaryOp) -> Option<OperatorKind> {
    Some(match op {
        BinaryOp::Eq => OperatorKind::Eq,
        BinaryOp::Add => OperatorKind::Plus,
        BinaryOp::Sub => OperatorKind::Minus,
        BinaryOp::Mul => OperatorKind::Mul,
        BinaryOp::Div => OperatorKind::Div,
        BinaryOp::Rem => OperatorKind::Rem,
        BinaryOp::BitAnd => OperatorKind::BitAnd,
        BinaryOp::BitOr => OperatorKind::BitOr,
        BinaryOp::BitXor => OperatorKind::BitXor,
        BinaryOp::Shl => OperatorKind::Shl,
        BinaryOp::Shr => OperatorKind::Shr,
        // !=, comparison, &&, || — skipped (see fn doc).
        _ => return None,
    })
}

/// Map a [`UnaryOp`] to the [`OperatorKind`] whose deletion would
/// suppress this op. `!x` (logical NOT) isn't overloadable per spec
/// §O.2.5.
fn op_kind_for_unary(op: UnaryOp) -> Option<OperatorKind> {
    Some(match op {
        // Unary `-` maps to the zero-param Neg kind (the parser
        // re-kinds `operator-()` declarations from Minus to Neg).
        UnaryOp::Neg => OperatorKind::Neg,
        UnaryOp::BitNot => OperatorKind::BitNot,
        // `!x`, raw-pointer `*p` / `&x` aren't overloadable (§O.2.5).
        UnaryOp::Not | UnaryOp::Deref | UnaryOp::AddrOf => return None,
    })
}

/// Human-readable spelling of an [`OperatorKind`] for diagnostics.
/// Matches the form the user would have written (`==`, `<=>`, `hash`,
/// `string`, …). Mirrors the same helper in `symbol_table.rs`.
fn operator_kind_user_spelling(kind: OperatorKind) -> &'static str {
    match kind {
        OperatorKind::Eq => "==",
        OperatorKind::In => "in",
        OperatorKind::Cmp => "<=>",
        OperatorKind::Lt => "<",
        OperatorKind::Le => "<=",
        OperatorKind::Gt => ">",
        OperatorKind::Ge => ">=",
        OperatorKind::Hash => "hash",
        OperatorKind::ToString => "string",
        OperatorKind::Plus => "+",
        OperatorKind::Minus => "-",
        OperatorKind::Neg => "- (unary)",
        OperatorKind::Mul => "*",
        OperatorKind::Div => "/",
        OperatorKind::Rem => "%",
        OperatorKind::BitAnd => "&",
        OperatorKind::BitOr => "|",
        OperatorKind::BitXor => "^",
        OperatorKind::BitNot => "~",
        OperatorKind::Shl => "<<",
        OperatorKind::Shr => ">>",
        OperatorKind::Index => "[]",
        OperatorKind::IndexSet => "[]=",
        OperatorKind::Call => "()",
        OperatorKind::Range => "..",
        OperatorKind::RangeInclusive => "..=",
    }
}

/// Reach into an expression for its span, mirroring the parser's
/// `expr_span`. Synth literals from inference don't carry a span, so
/// `Span::DUMMY` is the fallback.
/// True when `ty` is a plain user-type reference — a single named class /
/// interface with no array / nullable / pointer / function-type markers. Only
/// these can be a reference-cast or type-test target.
fn is_plain_user_typeref(ty: &juxc_ast::TypeRef) -> bool {
    ty.array_shape.is_none() && !ty.nullable && ty.ptr_depth == 0 && ty.fn_shape.is_none()
}

/// Report every bare `Path` leaf in `e` to `sink` — drives the
/// `Worker.spawn` capture scan ([`Checker::check_spawn_captures`]).
/// Field accesses report the root; unmatched expression shapes are
/// skipped (conservative: a missed read only skips the diagnostic and
/// falls back to the rustc error).
fn collect_bare_name_reads(e: &Expr, sink: &mut dyn FnMut(&juxc_ast::QualifiedName)) {
    match e {
        Expr::Path(qn) => sink(qn),
        Expr::Call(c) => {
            collect_bare_name_reads(&c.callee, sink);
            for a in &c.args {
                collect_bare_name_reads(a, sink);
            }
        }
        Expr::Binary(b) => {
            collect_bare_name_reads(&b.left, sink);
            collect_bare_name_reads(&b.right, sink);
        }
        Expr::Unary(u) => collect_bare_name_reads(&u.operand, sink),
        Expr::Cast(c) => collect_bare_name_reads(&c.value, sink),
        Expr::NotNullAssert(inner, _) => collect_bare_name_reads(inner, sink),
        Expr::Field(f) => collect_bare_name_reads(&f.object, sink),
        Expr::Index(ix) => {
            collect_bare_name_reads(&ix.array, sink);
            collect_bare_name_reads(&ix.index, sink);
        }
        Expr::Ternary(t) => {
            collect_bare_name_reads(&t.condition, sink);
            collect_bare_name_reads(&t.then_branch, sink);
            collect_bare_name_reads(&t.else_branch, sink);
        }
        Expr::Elvis(el) => {
            collect_bare_name_reads(&el.value, sink);
            collect_bare_name_reads(&el.fallback, sink);
        }
        Expr::InterpString(s) => {
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Expr(inner) = seg {
                    collect_bare_name_reads(inner, sink);
                }
            }
        }
        Expr::NewObject(n) => {
            for a in &n.args {
                collect_bare_name_reads(a, sink);
            }
        }
        Expr::Await(inner, _) => collect_bare_name_reads(inner, sink),
        Expr::Lambda(l) => match &l.body {
            juxc_ast::LambdaBody::Expr(b) => collect_bare_name_reads(b, sink),
            juxc_ast::LambdaBody::Block(blk) => {
                for s in &blk.statements {
                    collect_bare_name_reads_stmt(s, sink);
                }
            }
        },
        _ => {}
    }
}

/// Statement-level driver for [`collect_bare_name_reads`].
fn collect_bare_name_reads_stmt(s: &Stmt, sink: &mut dyn FnMut(&juxc_ast::QualifiedName)) {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e, _) => collect_bare_name_reads(e, sink),
        Stmt::Return(Some(e)) => collect_bare_name_reads(e, sink),
        Stmt::VarDecl(v) => {
            if let Some(init) = &v.init {
                collect_bare_name_reads(init, sink);
            }
        }
        Stmt::Assign(a) => {
            collect_bare_name_reads(&a.target, sink);
            collect_bare_name_reads(&a.value, sink);
        }
        Stmt::If(i) => {
            collect_bare_name_reads(&i.condition, sink);
            for st in &i.then_block.statements {
                collect_bare_name_reads_stmt(st, sink);
            }
            let mut cursor = i.else_branch.as_deref();
            while let Some(branch) = cursor {
                match branch {
                    juxc_ast::ElseBranch::If(inner) => {
                        collect_bare_name_reads(&inner.condition, sink);
                        for st in &inner.then_block.statements {
                            collect_bare_name_reads_stmt(st, sink);
                        }
                        cursor = inner.else_branch.as_deref();
                    }
                    juxc_ast::ElseBranch::Block(blk) => {
                        for st in &blk.statements {
                            collect_bare_name_reads_stmt(st, sink);
                        }
                        cursor = None;
                    }
                }
            }
        }
        Stmt::While(w) => {
            collect_bare_name_reads(&w.condition, sink);
            for st in &w.body.statements {
                collect_bare_name_reads_stmt(st, sink);
            }
        }
        Stmt::ForEach(f) => {
            collect_bare_name_reads(&f.iter, sink);
            for st in &f.body.statements {
                collect_bare_name_reads_stmt(st, sink);
            }
        }
        Stmt::ForC(f) => {
            if let Some(cond) = &f.cond {
                collect_bare_name_reads(cond, sink);
            }
            for st in &f.body.statements {
                collect_bare_name_reads_stmt(st, sink);
            }
        }
        _ => {}
    }
}

/// True when `e` contains a bare single-segment `Path` whose name is in
/// `names`. Drives the const-generic array-size guard
/// ([`Checker::check_const_size_expr`]) — only the expression shapes a
/// size expression can realistically take are walked; an unmatched
/// shape conservatively reports `false` (no diagnostic, rustc's
/// const-expr check remains the backstop).
fn expr_mentions_name_of(e: &Expr, names: &std::collections::HashSet<String>) -> bool {
    match e {
        Expr::Path(qn) => {
            qn.segments.len() == 1 && names.contains(&qn.segments[0].text)
        }
        Expr::Binary(b) => {
            expr_mentions_name_of(&b.left, names) || expr_mentions_name_of(&b.right, names)
        }
        Expr::Unary(u) => expr_mentions_name_of(&u.operand, names),
        Expr::Cast(c) => expr_mentions_name_of(&c.value, names),
        Expr::Call(c) => c.args.iter().any(|a| expr_mentions_name_of(a, names)),
        Expr::Field(f) => expr_mentions_name_of(&f.object, names),
        Expr::Index(ix) => {
            expr_mentions_name_of(&ix.array, names)
                || expr_mentions_name_of(&ix.index, names)
        }
        _ => false,
    }
}

/// Public-in-crate alias for [`expr_span`] — used by the expansion
/// pass to anchor synthesized array literals.
pub(crate) fn expr_span_pub(e: &Expr) -> Span {
    expr_span(e)
}

fn expr_span(e: &Expr) -> Span {
    match e {
        Expr::Literal(_) => Span::DUMMY,
        Expr::TupleLit(_, s) => *s,
        Expr::TryExpr(t) => t.span,
        Expr::ErrorProp(_, s) => *s,
        Expr::Path(qn) => qn.span,
        Expr::Call(c) => c.span,
        Expr::Binary(b) => b.span,
        Expr::Unary(u) => u.span,
        Expr::Range(r) => r.span,
        Expr::Cast(c) => c.span,
        Expr::TypeTest(t) => t.span,
        Expr::SizeOf(s) => s.span,
        Expr::NewArray(n) => n.span,
        Expr::NewArrayLit(n) => n.span,
        Expr::Index(i) => i.span,
        Expr::Field(f) => f.span,
        Expr::InterpString(s) => s.span,
        Expr::This(s) => *s,
        Expr::Super(s) => *s,
        Expr::NewObject(n) => n.span,
        Expr::Switch(s) => s.span,
        Expr::Lambda(l) => l.span,
        Expr::Elvis(e) => e.span,
        Expr::MethodRef(m) => m.span,
        Expr::Ternary(t) => t.span,
        Expr::Await(_, s) => *s,
        Expr::NotNullAssert(_, s) => *s,
    }
}

/// True when `pattern` is irrefutable — covers every value of the
/// scrutinee type. Used by the exhaustiveness check to detect
/// catchall arms (`case _ -> …`, `case name -> …`). Variant
/// patterns are NOT irrefutable, even when their sub-patterns are
/// — they only cover their specific variant.
/// True when `pattern` introduces at least one binding — a `var name`
/// bind, a payload binder inside a variant pattern, or a type-test
/// binder. Used to enforce the §A.3 rule that or-pattern alternatives
/// are binding-free.
/// Collect the spans of every `return` statement lexically inside
/// `block`, NOT crossing into lambda bodies (those return from the
/// lambda, §X.3.5 doesn't apply to them). Drives W0720.
fn collect_returns_in_block(block: &Block, out: &mut Vec<Span>) {
    for stmt in &block.statements {
        collect_returns_in_stmt(stmt, out);
    }
}

fn collect_returns_in_if(i: &juxc_ast::IfStmt, out: &mut Vec<Span>) {
    collect_returns_in_block(&i.then_block, out);
    if let Some(else_branch) = &i.else_branch {
        match &**else_branch {
            ElseBranch::If(elif) => collect_returns_in_if(elif, out),
            ElseBranch::Block(b) => collect_returns_in_block(b, out),
        }
    }
}

fn collect_returns_in_stmt(stmt: &Stmt, out: &mut Vec<Span>) {
    match stmt {
        Stmt::Return(e) => {
            // Best span available: the returned expression's, else the
            // statement has no own span — fall back to DUMMY (the
            // caller anchors on the finally block if needed).
            out.push(e.as_ref().map(expr_span).unwrap_or(Span::DUMMY));
        }
        Stmt::If(i) => collect_returns_in_if(i, out),
        Stmt::While(w) => collect_returns_in_block(&w.body, out),
        Stmt::DoWhile(d) => collect_returns_in_block(&d.body, out),
        Stmt::ForEach(f) => collect_returns_in_block(&f.body, out),
        Stmt::ForC(f) => collect_returns_in_block(&f.body, out),
        Stmt::Labeled { stmt, .. } => collect_returns_in_stmt(stmt, out),
        Stmt::Try(t) => {
            collect_returns_in_block(&t.body, out);
            for c in &t.catches {
                collect_returns_in_block(&c.body, out);
            }
            if let Some(f) = &t.finally {
                collect_returns_in_block(f, out);
            }
        }
        Stmt::Unsafe(b) => collect_returns_in_block(b, out),
        _ => {}
    }
}

fn pattern_introduces_bindings(p: &Pattern) -> bool {
    match p {
        Pattern::Bind(_) | Pattern::TypeBind { .. } => true,
        Pattern::EnumVariant { args, .. } => {
            args.iter().any(pattern_introduces_bindings)
        }
        Pattern::Or(alts, _) => alts.iter().any(pattern_introduces_bindings),
        Pattern::Wildcard(_) | Pattern::Literal(_, _) | Pattern::Range { .. } => false,
    }
}

fn pattern_is_catchall(p: &Pattern) -> bool {
    match p {
        Pattern::Wildcard(_) | Pattern::Bind(_) => true,
        // `case A | _ ->` — any irrefutable alternative makes the
        // whole or-pattern irrefutable.
        Pattern::Or(alts, _) => alts.iter().any(pattern_is_catchall),
        _ => false,
    }
}

/// Walk a pattern and record every variant of `enum_name` it
/// matches. Accepts both the qualified `case EnumName.Variant`
/// form AND the bare `case Variant` form (common when the
/// surrounding `switch` makes the enum unambiguous). Nested
/// sub-patterns (`Token.Number(var n)`) don't recurse for
/// exhaustiveness — the variant either matches or it doesn't, the
/// inner shape is bookkeeping.
fn collect_variants_covered(
    pattern: &Pattern,
    enum_name: &str,
    out: &mut std::collections::HashSet<String>,
) {
    // `enum_name` is the scrutinee's FQN (e.g. `shop.catalog.Item`),
    // but the pattern usually quotes only the bare class name
    // (`Item.Book`). Compare against the last segment so a
    // cross-package switch still matches its variants.
    let bare = enum_name
        .rsplit('.')
        .next()
        .unwrap_or(enum_name);
    // Or-pattern coverage is the union of its alternatives
    // (`case A | B ->` covers both A and B).
    if let Pattern::Or(alts, _) = pattern {
        for alt in alts {
            collect_variants_covered(alt, enum_name, out);
        }
        return;
    }
    if let Pattern::EnumVariant { path, .. } = pattern {
        match path.segments.len() {
            // `case EnumName.Variant(...)` — qualified form.
            2 if path.segments[0].text == bare
                || path.segments[0].text == enum_name =>
            {
                out.insert(path.segments[1].text.clone());
            }
            // `case Variant(...)` — bare form. The scrutinee's
            // known to be `enum_name` from the type-check above,
            // so a single-segment path here can only mean a
            // variant of that enum. The resolver still flags
            // misspellings via the regular name-resolution
            // diagnostics.
            1 => {
                out.insert(path.segments[0].text.clone());
            }
            _ => {}
        }
    }
}

/// Walk a pattern and record every sealed-class subclass it
/// matches. Sealed-class patterns are written as `case Subclass`
/// or `case Subclass(...)` (single-segment path naming a
/// permitted subclass), per `JUX-LANG-V1.md` §7.5. Other
/// pattern shapes (literals, two-segment paths) contribute
/// nothing — they're either wildcards (which the catchall check
/// already short-circuited) or pattern-typos to be flagged by
/// the resolver.
fn collect_sealed_subclasses_covered(
    pattern: &Pattern,
    out: &mut std::collections::HashSet<String>,
) {
    match pattern {
        Pattern::EnumVariant { path, .. } if path.segments.len() == 1 => {
            out.insert(path.segments[0].text.clone());
        }
        // `case Sub ident -> ...` also covers Sub — the binder
        // captures the matched value while still narrowing the
        // arm to exactly the named subclass.
        Pattern::TypeBind { type_name, .. } => {
            out.insert(type_name.text.clone());
        }
        // Or-pattern coverage is the union of its alternatives.
        Pattern::Or(alts, _) => {
            for alt in alts {
                collect_sealed_subclasses_covered(alt, out);
            }
        }
        _ => {}
    }
}

/// Type-compatibility predicate. See module docs for the full rule
/// table; the short version:
///
/// - `Unknown` or `Ty::Param` on either side → true (don't cascade).
/// - Exact equality → true.
/// - Unsuffixed-int literal (`Primitive::Int`) widens silently to any
///   numeric primitive on the **expected** side; same story for
///   unsuffixed-float literal (`Primitive::Double`).
/// - Arrays compare element-wise + kind.
/// - User types compare by name + pairwise generic-args.
/// - Everything else: false.
pub(crate) fn compatible(expected: &Ty, found: &Ty, symbols: &SymbolTable) -> bool {
    // Wildcards / suppression escape hatches.
    if expected.is_unknown() || found.is_unknown() {
        return true;
    }
    if matches!(expected, Ty::Param(_)) || matches!(found, Ty::Param(_)) {
        return true;
    }
    // PECS variance — `expected` carries the wildcard, `found` is the
    // concrete actual. `found` carrying a wildcard would mean a
    // raw-type producer flowing into a slot that doesn't accept one;
    // we accept it permissively for Phase 1 (raw-types are already a
    // lenient escape hatch).
    if let Ty::Wildcard(w) = expected {
        return match w {
            crate::ty::Wildcard::Unbounded => true,
            crate::ty::Wildcard::Extends(bound) => is_subtype(found, bound, symbols),
            crate::ty::Wildcard::Super(bound) => is_subtype(bound, found, symbols),
        };
    }
    if matches!(found, Ty::Wildcard(_)) {
        // Raw producer flowing into a non-wildcard slot — stay
        // permissive. A future pass may tighten this with E04xx.
        return true;
    }
    // Exact match.
    if expected == found {
        return true;
    }
    // Nullable widening (one-way): a `T` fits into a `T?` slot,
    // and `null` (typed as an Unknown-inner Nullable) fits into
    // any `T?` slot. The reverse direction (`T?` into `T`) needs
    // an explicit unwrap (`!!`, `?:` / `??`, or `if (x != null)`
    // smart-cast); reject it here so tycheck catches the missing
    // check before the backend turns it into a Rust error.
    if let Ty::Nullable(inner_expected) = expected {
        // `null` literal: `found` is `Ty::Nullable(Unknown)` (set
        // by `infer_literal`). Always fits a `T?` slot.
        if let Ty::Nullable(inner_found) = found {
            if matches!(inner_found.as_ref(), Ty::Unknown) {
                return true;
            }
            return compatible(inner_expected, inner_found, symbols);
        }
        // Plain `T` flows into `T?` — widening.
        return compatible(inner_expected, found, symbols);
    }
    match (expected, found) {
        // Default-int / default-float widening — only when the FOUND
        // side is the unsuffixed-literal default. Going the other
        // direction (`int x = 7L;` for instance) is rejected.
        (Ty::Primitive(_), Ty::Primitive(Primitive::Int))
            if expected.is_numeric() && !matches!(expected, Ty::Primitive(Primitive::Bool | Primitive::Char)) =>
        {
            true
        }
        (Ty::Primitive(_), Ty::Primitive(Primitive::Double))
            if matches!(
                expected,
                Ty::Primitive(
                    Primitive::Float
                        | Primitive::F32
                        | Primitive::F64
                        | Primitive::Double,
                ),
            ) =>
        {
            true
        }
        // Arrays — recurse on element and require matching kind.
        (
            Ty::Array { element: e1, kind: k1 },
            Ty::Array { element: e2, kind: k2 },
        ) => k1 == k2 && compatible(e1, e2, symbols),
        // User types — same name AND pairwise compatible generic
        // args, OR `found` is a subclass of `expected` (Java
        // upcasting). The backend pairs this rule with sealed-class
        // enum lowering + auto-`From<Sub>` impls so the upcast
        // actually carries the subclass's identity through the
        // boundary; non-sealed hierarchies still see strict
        // same-name matching today, and the backend rejects the
        // upcast at emit time when the parent isn't sealed (so the
        // diagnostic is at least loud rather than silently
        // mis-lowered).
        (
            Ty::User { name: n1, generic_args: a1 },
            Ty::User { name: n2, generic_args: a2 },
        ) => {
            if n1 == n2 {
                // Same name — length-mismatch is only a problem
                // when neither side is empty; empty generic args
                // on one side typically means "user didn't write
                // the args" — be lenient.
                if a1.is_empty() || a2.is_empty() {
                    return true;
                }
                if a1.len() != a2.len() {
                    return false;
                }
                return a1
                    .iter()
                    .zip(a2.iter())
                    .all(|(x, y)| compatible(x, y, symbols));
            }
            // Different names — try the upcast direction: is the
            // found type a subclass of the expected type? Walks
            // the class-extends chain via `is_subtype`.
            is_subtype(found, expected, symbols)
        }
        _ => false,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_table::build;
    use juxc_lex::lex;
    use juxc_parse::parse;
    use juxc_source::SourceFile;

    /// Drive lex → parse → symbol-table build → check, returning every
    /// diagnostic emitted across symbol-table + check passes. Caller
    /// filters by code when they want to assert a specific shape.
    fn run(src: &str) -> Vec<Diagnostic> {
        let sf = SourceFile::new("test.jux", src);
        let lex_result = lex(&sf);
        assert!(
            lex_result.diagnostics.is_empty(),
            "lex errors: {:?}",
            lex_result.diagnostics,
        );
        let parse_result = parse(&lex_result.tokens);
        assert!(
            parse_result.diagnostics.is_empty(),
            "parse errors: {:?}",
            parse_result.diagnostics,
        );
        let mut diags = Vec::new();
        let symbols = build(&parse_result.ast, &mut diags);
        let mut checker = Checker::new(&symbols, &mut diags);
        checker.check_unit(&parse_result.ast);
        diags
    }

    /// Convenience: did any diagnostic with `code` fire?
    fn has(diags: &[Diagnostic], wanted: code::Code) -> bool {
        diags.iter().any(|d| d.code == wanted)
    }

    /// Convenience: count diagnostics matching `code`.
    fn count(diags: &[Diagnostic], wanted: code::Code) -> usize {
        diags.iter().filter(|d| d.code == wanted).count()
    }

    /// Bare `return;` in a void function is fine.
    #[test]
    fn void_return_in_void_function_is_ok() {
        let d = run("public void main() { return; }");
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    /// `return 42;` in an int function is fine.
    #[test]
    fn int_return_in_int_function_is_ok() {
        let d = run("public int main() { return 42; }");
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    /// `return "hi";` in an int function → E0410.
    #[test]
    fn string_return_in_int_function_emits_e0410() {
        let d = run(r#"public int main() { return "hi"; }"#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Bare `return;` in a value-returning function → E0410.
    #[test]
    fn bare_return_in_int_function_emits_e0410() {
        let d = run("public int main() { return; }");
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// `if (1) {}` — non-bool condition → E0410.
    #[test]
    fn non_bool_if_condition_emits_e0410() {
        let d = run("public void main() { if (1) {} }");
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// `if (true) {}` is fine.
    #[test]
    fn bool_if_condition_is_ok() {
        let d = run("public void main() { if (true) {} }");
        assert!(d.is_empty(), "{d:?}");
    }

    // ---- Exhaustiveness on switch over enum (E0440) ----

    /// A `switch` over an enum that names every variant compiles
    /// without a wildcard arm — every case is covered.
    #[test]
    fn switch_over_enum_with_all_variants_is_exhaustive() {
        let d = run(
            r#"public enum Color { Red, Green, Blue }
               public void main() {
                   var c = Color.Red;
                   switch (c) {
                       case Color.Red -> {}
                       case Color.Green -> {}
                       case Color.Blue -> {}
                   }
               }"#,
        );
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// A `switch` that misses a variant and has no wildcard fires
    /// E0440 naming the missing variant.
    #[test]
    fn switch_over_enum_missing_variant_emits_e0440() {
        let d = run(
            r#"public enum Color { Red, Green, Blue }
               public void main() {
                   var c = Color.Red;
                   switch (c) {
                       case Color.Red -> {}
                       case Color.Green -> {}
                   }
               }"#,
        );
        assert!(has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
        let msg = d
            .iter()
            .find(|x| x.code == code::Code::E0440_NotExhaustive)
            .map(|x| x.message.as_str())
            .unwrap_or("");
        assert!(msg.contains("Blue"), "diagnostic should name `Blue`: {msg}");
    }

    /// A wildcard `case _` arm catches every remaining variant —
    /// no E0440 even when explicit variants are missing.
    #[test]
    fn switch_with_wildcard_arm_is_exhaustive() {
        let d = run(
            r#"public enum Color { Red, Green, Blue }
               public void main() {
                   var c = Color.Red;
                   switch (c) {
                       case Color.Red -> {}
                       case _ -> {}
                   }
               }"#,
        );
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// Explicit `case var name -> …` bind-pattern is irrefutable:
    /// it catches every remaining variant.
    #[test]
    fn switch_with_bind_arm_is_exhaustive() {
        let d = run(
            r#"public enum Color { Red, Green, Blue }
               public void main() {
                   var c = Color.Red;
                   switch (c) {
                       case Color.Red -> {}
                       case var other -> {}
                   }
               }"#,
        );
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// Non-enum scrutinees (numeric, string) aren't checked for
    /// exhaustiveness — the wildcard arm remains the user's tool.
    #[test]
    fn switch_over_int_does_not_check_exhaustiveness() {
        let d = run(
            r#"public void main() {
                   var n = 1;
                   switch (n) {
                       case 0 -> {}
                       case 1 -> {}
                   }
               }"#,
        );
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// `switch` over a sealed-class scrutinee that names every
    /// permitted subclass passes exhaustiveness.
    #[test]
    fn switch_over_sealed_class_with_all_subclasses_is_exhaustive() {
        let d = run(
            r#"public sealed class Shape permits Circle, Square {}
               public class Circle extends Shape {
                   public Circle() {}
               }
               public class Square extends Shape {
                   public Square() {}
               }
               public void describe(Shape s) {
                   switch (s) {
                       case Circle -> {}
                       case Square -> {}
                   }
               }
               public void main() {}"#,
        );
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// `switch` over a sealed-class scrutinee that misses a
    /// permitted subclass fires E0440, naming the gap.
    #[test]
    fn switch_over_sealed_class_missing_subclass_emits_e0440() {
        let d = run(
            r#"public sealed class Shape permits Circle, Square {}
               public class Circle extends Shape {
                   public Circle() {}
               }
               public class Square extends Shape {
                   public Square() {}
               }
               public void describe(Shape s) {
                   switch (s) {
                       case Circle -> {}
                   }
               }
               public void main() {}"#,
        );
        assert!(has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
        let msg = d
            .iter()
            .find(|x| x.code == code::Code::E0440_NotExhaustive)
            .map(|x| x.message.as_str())
            .unwrap_or("");
        assert!(msg.contains("Square"), "should name `Square`: {msg}");
        assert!(msg.contains("sealed class"), "label: {msg}");
    }

    /// A non-sealed class scrutinee doesn't trigger the check —
    /// open inheritance means more subclasses can land later, so
    /// the wildcard arm stays the canonical fallback.
    #[test]
    fn switch_over_non_sealed_class_does_not_check_exhaustiveness() {
        let d = run(
            r#"public class Animal { public Animal() {} }
               public class Dog extends Animal {
                   public Dog() {}
               }
               public void main() {
                   var a = new Animal();
                   switch (a) {
                       case Dog -> {}
                   }
               }"#,
        );
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    // ---- Nullable type widening (Ty::Nullable + compatible) ----

    /// `String? x = "Ada";` — non-nullable `String` widens into
    /// the `String?` declared type without a diagnostic.
    #[test]
    fn non_null_value_widens_into_nullable_slot() {
        let d = run(r#"public void main() { String? x = "Ada"; }"#);
        assert!(d.is_empty(), "got: {d:?}");
    }

    /// `String? x = null;` — the `null` literal fits any nullable
    /// slot. No diagnostic.
    #[test]
    fn null_literal_fits_any_nullable_slot() {
        let d = run(r#"public void main() { String? x = null; }"#);
        assert!(d.is_empty(), "got: {d:?}");
    }

    /// `String x = null;` — `null` doesn't fit a NON-nullable
    /// slot. Fires `E0410_TypeMismatch`.
    #[test]
    fn null_does_not_fit_non_nullable_slot() {
        let d = run(r#"public void main() { String x = null; }"#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "got: {d:?}");
    }

    // ---- Async/await context (E0700, §18.1.2) ----

    /// `await` in a plain (non-async) function → E0700.
    #[test]
    fn await_in_plain_function_errors() {
        let d = run(r#"public int g(){ return 1; } public void f(){ var x = await g(); }"#);
        assert!(has(&d, code::Code::E0700_AwaitRequiresAsyncContext), "got: {d:?}");
    }

    /// `await` inside an `async` function is fine.
    #[test]
    fn await_in_async_function_ok() {
        let d = run(r#"public int g(){ return 1; } public async int f(){ return await g(); }"#);
        assert!(!has(&d, code::Code::E0700_AwaitRequiresAsyncContext), "got: {d:?}");
    }

    /// Constructors are never async, so `await` in a constructor body → E0700.
    #[test]
    fn await_in_constructor_errors() {
        let d = run(
            r#"public int g(){ return 1; } public class C { public C(){ var x = await g(); } }"#,
        );
        assert!(has(&d, code::Code::E0700_AwaitRequiresAsyncContext), "got: {d:?}");
    }

    /// `await` inside an `async` method is fine.
    #[test]
    fn await_in_async_method_ok() {
        let d = run(
            r#"public int g(){ return 1; } public class C { public async int m(){ return await g(); } }"#,
        );
        assert!(!has(&d, code::Code::E0700_AwaitRequiresAsyncContext), "got: {d:?}");
    }

    /// A plain lambda introduces a non-async context even inside an async
    /// function, so `await` in it → E0700.
    #[test]
    fn await_in_plain_lambda_inside_async_errors() {
        let d = run(
            r#"public int g(){ return 1; } public async void f(){ var bad = () -> { return await g(); }; }"#,
        );
        assert!(has(&d, code::Code::E0700_AwaitRequiresAsyncContext), "got: {d:?}");
    }

    /// An async lambda permits `await`, even inside a plain function.
    #[test]
    fn await_in_async_lambda_ok() {
        let d = run(
            r#"public int g(){ return 1; } public void f(){ var ok = async () -> { return await g(); }; }"#,
        );
        assert!(!has(&d, code::Code::E0700_AwaitRequiresAsyncContext), "got: {d:?}");
    }

    // ---- unsafe-context enforcement (E0506, §A.2.8 / Layout-ABI §L.5.2) ----

    /// Calling an `unsafe` free function from a plain (non-unsafe) context → E0506.
    #[test]
    fn unsafe_call_outside_unsafe_errors() {
        let d = run(r#"public unsafe int risky(){ return 1; } public void f(){ var x = risky(); }"#);
        assert!(has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe), "got: {d:?}");
    }

    /// The same call wrapped in an `unsafe { … }` block is fine.
    #[test]
    fn unsafe_call_in_unsafe_block_ok() {
        let d = run(
            r#"public unsafe int risky(){ return 1; } public void f(){ unsafe { var x = risky(); } }"#,
        );
        assert!(!has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe), "got: {d:?}");
    }

    /// An `unsafe` callee invoked from the body of another `unsafe` fn is fine —
    /// the whole body is an unsafe context.
    #[test]
    fn unsafe_call_from_unsafe_fn_ok() {
        let d = run(
            r#"public unsafe int risky(){ return 1; } public unsafe int caller(){ return risky(); }"#,
        );
        assert!(!has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe), "got: {d:?}");
    }

    /// Calling a SAFE function (no `unsafe` modifier) never trips E0506,
    /// whether or not it's inside an `unsafe` block.
    #[test]
    fn safe_call_never_trips_e0506() {
        let d = run(r#"public int ok(){ return 1; } public void f(){ var x = ok(); }"#);
        assert!(!has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe), "got: {d:?}");
    }

    /// Address-of `&x` outside an `unsafe` context → E0506 (§A.2.9).
    #[test]
    fn address_of_outside_unsafe_errors() {
        let d = run(r#"public void f(){ int x = 1; int* p = &x; }"#);
        assert!(has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe), "got: {d:?}");
    }

    /// Raw-pointer deref `*p` outside an `unsafe` context → E0506 (§A.2.9).
    #[test]
    fn deref_outside_unsafe_errors() {
        let d = run(r#"public void f(){ int x = 1; int* p = &x; int y = *p; }"#);
        assert!(has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe), "got: {d:?}");
    }

    /// The same pointer ops inside an `unsafe { }` block are fine.
    #[test]
    fn pointer_ops_in_unsafe_block_ok() {
        let d = run(r#"public void f(){ int x = 1; unsafe { int* p = &x; *p = 2; } }"#);
        assert!(!has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe), "got: {d:?}");
    }

    // ---- throw operand must be an Exception (E0710, §X.2.1) ----
    // `run` builds a single unit with no stdlib, so these use a local
    // `Exception` class — `throwable_ok` matches the bare `Exception` segment.

    #[test]
    fn throw_int_errors() {
        let d = run(r#"public class Exception {} public void f(){ throw 5; }"#);
        assert!(has(&d, code::Code::E0710_ThrowRequiresException), "got: {d:?}");
    }

    #[test]
    fn throw_string_errors() {
        let d = run(r#"public class Exception {} public void f(){ throw "oops"; }"#);
        assert!(has(&d, code::Code::E0710_ThrowRequiresException), "got: {d:?}");
    }

    #[test]
    fn throw_exception_ok() {
        let d = run(r#"public class Exception {} public void f(){ throw new Exception(); }"#);
        assert!(!has(&d, code::Code::E0710_ThrowRequiresException), "got: {d:?}");
    }

    #[test]
    fn throw_user_exception_subclass_ok() {
        let d = run(
            r#"public class Exception {} public class MyErr extends Exception {} public void f(){ throw new MyErr(); }"#,
        );
        assert!(!has(&d, code::Code::E0710_ThrowRequiresException), "got: {d:?}");
    }

    // ---- uninferable empty-diamond `new` (E0431, §T.4.2) ----

    /// `var b = new Box<>()` (generic class, no args) that is never referenced
    /// can't have its type argument pinned → E0431.
    #[test]
    fn unused_uninferable_new_errors() {
        let d = run(r#"public class Box<T> { public Box() {} } public void f(){ var b = new Box(); }"#);
        assert!(has(&d, code::Code::E0431_GenericInferenceNoSolution), "got: {d:?}");
    }

    /// The same construction, but `b` is later used as a receiver — a use could
    /// pin the argument (as the emitted Rust infers), so no E0431.
    #[test]
    fn used_uninferable_new_ok() {
        let d = run(
            r#"public class Box<T> { public Box() {} public void touch(){} } public void f(){ var b = new Box(); b.touch(); }"#,
        );
        assert!(!has(&d, code::Code::E0431_GenericInferenceNoSolution), "got: {d:?}");
    }

    /// An explicit type argument pins it — never flagged even if unused.
    #[test]
    fn explicit_type_arg_new_ok() {
        let d = run(r#"public class Box<T> { public Box() {} } public void f(){ var b = new Box<int>(); }"#);
        assert!(!has(&d, code::Code::E0431_GenericInferenceNoSolution), "got: {d:?}");
    }

    /// A non-generic class has no argument to infer — never flagged.
    #[test]
    fn non_generic_new_not_flagged() {
        let d = run(r#"public class Plain { public Plain() {} } public void f(){ var p = new Plain(); }"#);
        assert!(!has(&d, code::Code::E0431_GenericInferenceNoSolution), "got: {d:?}");
    }

    // ---- unreachable catch (E0720, §X.3.4) ----

    /// `catch (Base)` before `catch (Derived)` makes the Derived clause
    /// unreachable.
    #[test]
    fn unreachable_catch_after_supertype_errors() {
        let d = run(
            r#"public class Base {} public class Derived extends Base {} public void f(){ try {} catch (Base e) {} catch (Derived e2) {} }"#,
        );
        assert!(has(&d, code::Code::E0720_UnreachableCatch), "got: {d:?}");
    }

    /// Specific-before-broad ordering is reachable — no E0720.
    #[test]
    fn ordered_catches_specific_first_ok() {
        let d = run(
            r#"public class Base {} public class Derived extends Base {} public void f(){ try {} catch (Derived e) {} catch (Base e2) {} }"#,
        );
        assert!(!has(&d, code::Code::E0720_UnreachableCatch), "got: {d:?}");
    }

    /// Catching the exact same type twice — the second is unreachable.
    #[test]
    fn duplicate_catch_type_errors() {
        let d = run(r#"public class E1 {} public void f(){ try {} catch (E1 e) {} catch (E1 e2) {} }"#);
        assert!(has(&d, code::Code::E0720_UnreachableCatch), "got: {d:?}");
    }

    /// Assigning a String to an int local → E0410.
    #[test]
    fn assign_string_to_int_emits_e0410() {
        let d = run(r#"public void main() { var x = 1; x = "hi"; }"#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Assigning an int to an int local is fine.
    #[test]
    fn assign_int_to_int_is_ok() {
        let d = run("public void main() { var x = 1; x = 2; }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// `new Foo(1)` against a zero-arg synthesized constructor → E0411.
    #[test]
    fn wrong_arg_count_to_synth_ctor_emits_e0411() {
        let d = run("public class Foo {} public void main() { var f = new Foo(1); }");
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// `new Foo()` against a 1-arg constructor → E0411.
    #[test]
    fn missing_ctor_arg_emits_e0411() {
        let d = run(
            "public class Foo { public Foo(int x) {} } public void main() { var f = new Foo(); }",
        );
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// `new Foo("hi")` against a 1-int ctor → E0410.
    #[test]
    fn wrong_ctor_arg_type_emits_e0410() {
        let d = run(
            r#"public class Foo { public Foo(int x) {} } public void main() { var f = new Foo("hi"); }"#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Reading a field that doesn't exist → E0412.
    #[test]
    fn unresolved_field_emits_e0412() {
        let d = run(
            "public class Foo { public int x; } \
             public void main() { var f = new Foo(); print(f.y); }",
        );
        assert!(has(&d, code::Code::E0412_UnresolvedField), "{d:?}");
    }

    /// Calling a method that doesn't exist → E0413.
    #[test]
    fn unresolved_method_emits_e0413() {
        let d = run(
            "public class Foo { public int x; public int sum() { return this.x; } } \
             public void main() { new Foo().notThere(); }",
        );
        assert!(has(&d, code::Code::E0413_UnresolvedMethod), "{d:?}");
    }

    /// A method defined on a parent class is resolvable from a child.
    #[test]
    fn inherited_method_resolves() {
        let d = run(
            "public class Animal { public int age() { return 5; } } \
             public class Dog extends Animal {} \
             public void main() { var d = new Dog(); print(d.age()); }",
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// A field defined on a parent class is resolvable from a child.
    #[test]
    fn inherited_field_resolves() {
        let d = run(
            "public class Animal { public int age; } \
             public class Dog extends Animal {} \
             public void main() { var d = new Dog(); print(d.age); }",
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// `print` accepts any single argument shape.
    #[test]
    fn print_is_builtin_no_arg_check() {
        let d = run(
            r#"public void main() { print("x"); print(42); print(true); }"#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// `.push` on a dynamic int array is a built-in receiver method —
    /// no error.
    #[test]
    fn array_push_is_builtin() {
        let d = run(
            "public void main() { var xs = new int[]{1, 2, 3}; xs.push(4); }",
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// `.length` on an int array reads as int — no error.
    #[test]
    fn array_length_is_builtin() {
        let d = run(
            "public void main() { var xs = new int[]{1}; print(xs.length); }",
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Unsuffixed int literal is compatible with typed int locals.
    #[test]
    fn unsuffixed_int_widens_to_i32() {
        let d = run("public void main() { i32 always32 = 7; print(always32); }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// While-loop with bool condition is fine.
    #[test]
    fn bool_while_is_ok() {
        let d = run("public void main() { while (true) { break; } }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// While-loop with non-bool condition → E0410.
    #[test]
    fn non_bool_while_emits_e0410() {
        let d = run("public void main() { while (1) { break; } }");
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Top-level call with wrong number of args → E0411.
    #[test]
    fn top_level_wrong_arg_count_emits_e0411() {
        let d = run(
            "public int add(int a, int b) { return a + b; } \
             public void main() { print(add(1)); }",
        );
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// Top-level call with wrong arg type → E0410. (Note: this exercises
    /// the path even though "Int → String" wouldn't be tolerated.)
    #[test]
    fn top_level_wrong_arg_type_emits_e0410() {
        let d = run(
            r#"public void greet(String name) { print(name); }
               public void main() { greet(42); }"#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Multiple wrong-type args still emit at least one E0410.
    #[test]
    fn multiple_wrong_args_each_emit_e0410() {
        let d = run(
            r#"public void f(String a, String b) {}
               public void main() { f(1, 2); }"#,
        );
        assert!(count(&d, code::Code::E0410_TypeMismatch) >= 2, "{d:?}");
    }

    // ----------------------------------------------------------------
    // Phase E
    // ----------------------------------------------------------------

    /// Phase E.2 — `new Box<int>("hi")` against `Box(T)` substitutes
    /// `T → int` and rejects the String. Before Phase E the param type
    /// was left as `Ty::Param("T")` and the wildcard rule accepted it.
    #[test]
    fn instantiated_ctor_arg_mismatch_emits_e0410() {
        let d = run(
            r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
            }
            public void main() {
                var b = new Box<int>("hi");
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Phase E.2 — same ctor, correct type → no diagnostic.
    #[test]
    fn instantiated_ctor_matching_arg_is_ok() {
        let d = run(
            r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
            }
            public void main() {
                var b = new Box<int>(42);
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Phase E.2 — raw-type construction (`new Box(...)` with no
    /// turbofish) leaves substitution off, so any arg passes.
    #[test]
    fn raw_ctor_accepts_any_arg() {
        let d = run(
            r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
            }
            public void main() {
                var a = new Box(42);
                var b = new Box("hi");
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Phase E.2 — method call on `Box<int>` substitutes the parameter
    /// type, so passing a String to a `set(T v)` is rejected.
    #[test]
    fn instantiated_method_arg_mismatch_emits_e0410() {
        let d = run(
            r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
                public void set(T v) { this.value = v; }
            }
            public void main() {
                var b = new Box<int>(0);
                b.set("hi");
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Phase E.3 — `super("hi")` against a parent expecting `int`
    /// emits E0410.
    #[test]
    fn super_call_wrong_arg_type_emits_e0410() {
        let d = run(
            r#"
            public class Animal {
                public Animal(int age) {}
            }
            public class Dog extends Animal {
                public Dog() { super("hi"); }
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Phase E.3 — `super(42)` against `Animal(int)` is fine.
    #[test]
    fn super_call_matching_args_is_ok() {
        let d = run(
            r#"
            public class Animal {
                public Animal(int age) {}
            }
            public class Dog extends Animal {
                public Dog() { super(42); }
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Phase E.3 — `super()` (no args) against `Animal(int age)` is a
    /// wrong-arg-count, emits E0411.
    #[test]
    fn super_call_wrong_arg_count_emits_e0411() {
        let d = run(
            r#"
            public class Animal {
                public Animal(int age) {}
            }
            public class Dog extends Animal {
                public Dog() { super(); }
            }
            "#,
        );
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// Phase E.3 — `super(name)` with substitution through the extends
    /// clause's generic arg. Animal<T> with `Animal(T name)` lets Dog
    /// (extends Animal<String>) pass a String.
    #[test]
    fn super_call_substitutes_extends_generic_arg() {
        let d = run(
            r#"
            public class Animal<T> {
                public Animal(T name) {}
            }
            public class Dog extends Animal<String> {
                public Dog() { super("rex"); }
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    // ----------------------------------------------------------------
    // Operator body checks (§O.2)
    // ----------------------------------------------------------------

    /// A well-formed `operator==` body type-checks cleanly: `this` and
    /// the formal parameter are in scope, the return type matches.
    /// Also defines the paired `operator hash` — without it the §O.2.7
    /// pairing rule would fire `E0931`.
    #[test]
    fn operator_eq_body_typechecks_cleanly() {
        let d = run(
            r#"
            public class Path {
                public String value;
                public Path(String v) { this.value = v; }
                public bool operator==(Path other) {
                    return true;
                }
                public int operator hash() {
                    return 0;
                }
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Returning the wrong type from an operator body fires E0410 via
    /// the same path methods use — the operator walker sets
    /// `current_return` to the declared return type before walking.
    #[test]
    fn operator_return_type_mismatch_emits_e0410() {
        let d = run(
            r#"
            public class Path {
                public bool operator==(Path other) {
                    return 42;
                }
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Calling a method with the wrong-typed argument from inside an
    /// operator body still fires the standard E0410 path — proves the
    /// arg-type check path is reachable from within an operator body.
    #[test]
    fn operator_body_call_arg_mismatch_emits_e0410() {
        let d = run(
            r#"
            public class Path {
                public String value;
                public Path(String v) { this.value = v; }
                public void greet(String name) {}
                public bool operator==(Path other) {
                    this.greet(42);
                    return true;
                }
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// `operator hash()` is zero-arg and its body type-checks cleanly
    /// when the return type matches.
    #[test]
    fn operator_hash_body_typechecks_cleanly() {
        let d = run(
            r#"
            public class Path {
                public int operator hash() {
                    return 1;
                }
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    // ----------------------------------------------------------------
    // E0935 — use-of-deleted-operator (§O.3.4)
    // ----------------------------------------------------------------

    /// `$"$t"` on a record whose `operator string()` is deleted fires
    /// E0935 at the interp-string site.
    #[test]
    fn interp_string_on_deleted_string_op_emits_e0935() {
        let d = run(
            r#"
            public record OpaqueToken(int secret) {
                public String operator string() = delete;
            }
            public void main() {
                var t = new OpaqueToken(42);
                print($"$t");
            }
            "#,
        );
        assert!(has(&d, code::Code::E0935_DeletedOperator), "{d:?}");
    }

    /// `a + b` where `a`'s class deleted `operator+` fires E0935.
    #[test]
    fn arithmetic_on_deleted_op_emits_e0935() {
        let d = run(
            r#"
            public class M {
                public int x;
                public M(int x) { this.x = x; }
                public M operator+(M other) = delete;
            }
            public void main() {
                var a = new M(1);
                var b = new M(2);
                var c = a + b;
            }
            "#,
        );
        assert!(has(&d, code::Code::E0935_DeletedOperator), "{d:?}");
    }

    /// `-x` where `x`'s class deleted unary `operator-` fires E0935.
    #[test]
    fn unary_minus_on_deleted_op_emits_e0935() {
        let d = run(
            r#"
            public class N {
                public int x;
                public N(int x) { this.x = x; }
                public N operator-() = delete;
            }
            public void main() {
                var v = new N(1);
                var w = -v;
            }
            "#,
        );
        assert!(has(&d, code::Code::E0935_DeletedOperator), "{d:?}");
    }

    /// Primitives + non-deleted classes don't fire E0935. Pins that the
    /// check is gated on receiver class + deletion flag.
    #[test]
    fn no_e0935_for_primitives_or_undeleted() {
        let d = run(
            r#"
            public class M {
                public int x;
                public M(int x) { this.x = x; }
                public bool operator==(M other) { return true; }
            }
            public void main() {
                var a = 1 + 2;
                var b = new M(1);
                var c = new M(2);
                var eq = b == c;
            }
            "#,
        );
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0935_DeletedOperator),
            "should not emit E0935: {d:?}",
        );
    }

    /// `$"$x"` where x is a primitive (int, String, etc.) doesn't fire
    /// E0935 — primitives don't have an operator-string declaration.
    #[test]
    fn no_e0935_for_primitive_in_interp() {
        let d = run(
            r#"
            public void main() {
                var x = 42;
                var s = "hi";
                print($"x=$x, s=$s");
            }
            "#,
        );
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0935_DeletedOperator),
            "should not emit E0935 for primitive: {d:?}",
        );
    }

    // ----------------------------------------------------------------
    // PECS variance — `compatible` with bounded wildcards
    // ----------------------------------------------------------------

    /// `List<Dog>` is assignable to `List<? extends Animal>` —
    /// Dog is-a Animal, slot is covariant (producer).
    #[test]
    fn extends_wildcard_accepts_subtype() {
        let d = run(
            r#"
            public class Animal {}
            public class Dog extends Animal {}
            public class List<T> {
                public T head;
            }
            public void main() {
                var dogs = new List<Dog>();
                List<? extends Animal> animals = dogs;
                print(animals);
            }
            "#,
        );
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "covariant assignment should be accepted: {d:?}",
        );
    }

    /// `List<Cat>` is NOT assignable to `List<? extends Dog>` —
    /// Cat isn't a Dog.
    #[test]
    fn extends_wildcard_rejects_non_subtype() {
        let d = run(
            r#"
            public class Animal {}
            public class Dog extends Animal {}
            public class Cat extends Animal {}
            public class List<T> {
                public T head;
            }
            public void main() {
                var cats = new List<Cat>();
                List<? extends Dog> dogs = cats;
                print(dogs);
            }
            "#,
        );
        assert!(
            d.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "List<Cat> shouldn't fit List<? extends Dog>: {d:?}",
        );
    }

    /// `List<Animal>` is assignable to `List<? super Dog>` —
    /// Animal is a supertype of Dog, slot is contravariant (consumer).
    #[test]
    fn super_wildcard_accepts_supertype() {
        let d = run(
            r#"
            public class Animal {}
            public class Dog extends Animal {}
            public class List<T> {
                public T head;
            }
            public void main() {
                var animals = new List<Animal>();
                List<? super Dog> dogs = animals;
                print(dogs);
            }
            "#,
        );
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "contravariant assignment should be accepted: {d:?}",
        );
    }

    /// `List<Cat>` is NOT assignable to `List<? super Dog>` —
    /// Cat is not a supertype of Dog.
    #[test]
    fn super_wildcard_rejects_unrelated() {
        let d = run(
            r#"
            public class Animal {}
            public class Dog extends Animal {}
            public class Cat extends Animal {}
            public class List<T> {
                public T head;
            }
            public void main() {
                var cats = new List<Cat>();
                List<? super Dog> sink = cats;
                print(sink);
            }
            "#,
        );
        assert!(
            d.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "List<Cat> shouldn't fit List<? super Dog>: {d:?}",
        );
    }

    // ----------------------------------------------------------------
    // Encapsulation — E0414 / E0415 access checks
    // ----------------------------------------------------------------

    /// Reading a private field from top-level code fires E0414.
    #[test]
    fn private_field_access_from_outside_emits_e0414() {
        let d = run(
            r#"
            public class Account {
                private int balance;
                public Account(int n) { this.balance = n; }
            }
            public void main() {
                var a = new Account(10);
                print(a.balance);
            }
            "#,
        );
        assert!(
            d.iter().any(|d| d.code == code::Code::E0414_PrivateAccess),
            "expected E0414, got: {d:?}",
        );
    }

    /// Reading a private field from inside the same class is OK.
    #[test]
    fn private_field_access_from_same_class_is_ok() {
        let d = run(
            r#"
            public class Account {
                private int balance;
                public Account(int n) { this.balance = n; }
                public int get() { return this.balance; }
            }
            public void main() {
                var a = new Account(10);
                print(a.get());
            }
            "#,
        );
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0414_PrivateAccess),
            "should not emit E0414 for self-access: {d:?}",
        );
    }

    /// Calling a protected method from an unrelated class fires E0415.
    #[test]
    fn protected_method_from_unrelated_class_emits_e0415() {
        let d = run(
            r#"
            public class Base {
                protected void secret() {}
            }
            public class Other {
                public void touch(Base b) { b.secret(); }
            }
            public void main() {
                var o = new Other();
                o.touch(new Base());
            }
            "#,
        );
        assert!(
            d.iter().any(|d| d.code == code::Code::E0415_ProtectedAccess),
            "expected E0415, got: {d:?}",
        );
    }

    /// Calling a protected method from a subclass is OK.
    #[test]
    fn protected_method_from_subclass_is_ok() {
        let d = run(
            r#"
            public class Base {
                protected void secret() {}
            }
            public class Sub extends Base {
                public void touch() { this.secret(); }
            }
            public void main() {
                var s = new Sub();
                s.touch();
            }
            "#,
        );
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0415_ProtectedAccess),
            "should not emit E0415 for subclass access: {d:?}",
        );
    }

    /// `new Foo()` against a private constructor fires E0414.
    #[test]
    fn private_constructor_emits_e0414() {
        let d = run(
            r#"
            public class Singleton {
                private Singleton() {}
            }
            public void main() {
                var s = new Singleton();
                print(s);
            }
            "#,
        );
        assert!(
            d.iter().any(|d| d.code == code::Code::E0414_PrivateAccess),
            "expected E0414 on private ctor, got: {d:?}",
        );
    }

    // ----------------------------------------------------------------
    // Static members (call/field resolution + this-in-static)
    // ----------------------------------------------------------------

    /// `Math.PI` and `Math.max(1, 2)` type-check cleanly.
    #[test]
    fn static_member_access_typechecks() {
        let d = run(
            r#"
            public class Math {
                public static final int X = 1;
                public static int dbl(int n) { return n + n; }
            }
            public void main() {
                print(Math.X);
                print(Math.dbl(5));
            }
            "#,
        );
        assert!(d.is_empty(), "expected clean tycheck: {d:?}");
    }

    /// `this` inside a `static` method fires E0425.
    #[test]
    fn this_in_static_method_emits_e0425() {
        let d = run(
            r#"
            public class C {
                public int x;
                public C() { this.x = 0; }
                public static int f() { return this.x; }
            }
            public void main() { print(C.f()); }
            "#,
        );
        assert!(
            d.iter().any(|d| d.code == code::Code::E0425_ThisInStaticContext),
            "expected E0425: {d:?}",
        );
    }

    /// Reading an instance field through the class name (`C.x`)
    /// fires a clear `E0412` with the "instance field" message.
    #[test]
    fn instance_field_via_classname_emits_e0412() {
        let d = run(
            r#"
            public class C { public int x; public C() { this.x = 0; } }
            public void main() { print(C.x); }
            "#,
        );
        assert!(
            d.iter().any(|d| d.code == code::Code::E0412_UnresolvedField),
            "expected E0412: {d:?}",
        );
    }

    /// Unbounded `?` accepts anything in the slot.
    #[test]
    fn unbounded_wildcard_accepts_anything() {
        let d = run(
            r#"
            public class List<T> {
                public T head;
            }
            public void main() {
                var ints = new List<int>();
                List<?> any = ints;
                print(any);
            }
            "#,
        );
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "List<?> should accept anything: {d:?}",
        );
    }

    // ----------------------------------------------------------------
    // C#-style property access control (JUX-MISSING-DEFS §M.7.2)
    // ----------------------------------------------------------------

    /// A read-write auto-property may be freely read and written.
    #[test]
    fn property_read_write_is_ok() {
        let d = run(
            r#"
            public class P { public String Name { get; set; } }
            public void main() {
                var p = new P();
                p.Name = "Bob";
                print(p.Name);
            }
            "#,
        );
        assert!(
            !has(&d, code::Code::E0970_PropertyNotWritable)
                && !has(&d, code::Code::E0972_PropertyAccessorVisibility),
            "read/write property should be clean: {d:?}",
        );
    }

    /// Writing a read-only property (`{ get; }`) outside the
    /// constructor fires E0970.
    #[test]
    fn write_readonly_property_outside_ctor_errors() {
        let d = run(
            r#"
            public class P {
                public int Id { get; }
                public P() { this.Id = 7; }
            }
            public void main() { var p = new P(); p.Id = 1; }
            "#,
        );
        assert!(has(&d, code::Code::E0970_PropertyNotWritable), "expected E0970: {d:?}");
    }

    /// Writing an init-only property after construction fires E0970.
    #[test]
    fn write_init_property_after_construction_errors() {
        let d = run(
            r#"
            public class P {
                public String Code { get; init; }
                public P(String c) { this.Code = c; }
            }
            public void main() { var p = new P("a"); p.Code = "x"; }
            "#,
        );
        assert!(has(&d, code::Code::E0970_PropertyNotWritable), "expected E0970: {d:?}");
    }

    /// Writing a `{ get; private set; }` property from outside the
    /// declaring class fires E0972.
    #[test]
    fn write_private_set_property_from_outside_errors() {
        let d = run(
            r#"
            public class P {
                public String Token { get; private set; }
                public P() { this.Token = "t"; }
            }
            public void main() { var p = new P(); p.Token = "y"; }
            "#,
        );
        assert!(
            has(&d, code::Code::E0972_PropertyAccessorVisibility),
            "expected E0972: {d:?}",
        );
    }

    /// The constructor may set read-only / init / private-set
    /// properties — the desugarer lowers those to backing-field writes,
    /// so no access-control diagnostic fires.
    #[test]
    fn ctor_may_set_restricted_properties() {
        let d = run(
            r#"
            public class P {
                public int Id { get; }
                public String Code { get; init; }
                public String Token { get; private set; }
                public P(String c) { this.Id = 1; this.Code = c; this.Token = "t"; }
            }
            public void main() { var p = new P("a"); print(p.Id); }
            "#,
        );
        assert!(
            !has(&d, code::Code::E0970_PropertyNotWritable)
                && !has(&d, code::Code::E0972_PropertyAccessorVisibility),
            "ctor writes to restricted props should be clean: {d:?}",
        );
    }

    // ----------------------------------------------------------------
    // E0435 — interface used as a non-dispatchable value type
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // Stage-2 deferred-case diagnostics (E0437 / E0438)
    // ----------------------------------------------------------------

    /// Reading a PRIVATE field through a polymorphic-base reference → E0437
    /// (no accessor is generated for private fields).
    #[test]
    fn private_field_through_polymorphic_base_emits_e0437() {
        let d = run(
            r#"
            public class Animal { private String name; public Animal(String n){ this.name = n; } public String speak(){ return "..."; } }
            public class Dog extends Animal { public Dog(String n){ super(n); } public String speak(){ return "woof"; } }
            public void main() { Animal a = new Dog("Rex"); print(a.speak()); var n = a.name; }
            "#,
        );
        assert!(has(&d, code::Code::E0437_FieldThroughPolymorphicBase), "expected E0437: {d:?}");
    }

    /// Reading a PUBLIC field through a polymorphic-base reference is allowed
    /// (the generated `__get_<f>` accessor handles it) — no E0437.
    #[test]
    fn public_field_through_polymorphic_base_is_allowed() {
        let d = run(
            r#"
            public class Animal { public String name; public Animal(String n){ this.name = n; } public String speak(){ return "..."; } }
            public class Dog extends Animal { public Dog(String n){ super(n); } public String speak(){ return "woof"; } }
            public void main() { Animal a = new Dog("Rex"); print(a.name); }
            "#,
        );
        assert!(!has(&d, code::Code::E0437_FieldThroughPolymorphicBase), "public field via accessor: {d:?}");
    }

    /// Accessing a field on `this` (concrete self) or on a concrete subclass
    /// reference must NOT trip E0437.
    #[test]
    fn field_on_this_or_concrete_no_e0437() {
        let d = run(
            r#"
            public class Animal { public String name; public Animal(String n){ this.name = n; } public String who(){ return this.name; } public String speak(){ return "..."; } }
            public class Dog extends Animal { public Dog(String n){ super(n); } public String speak(){ return "woof"; } }
            public void main() { var d = new Dog("Rex"); print(d.name); }
            "#,
        );
        assert!(!has(&d, code::Code::E0437_FieldThroughPolymorphicBase), "this/concrete field access must be clean: {d:?}");
    }

    /// A generic virtual method on a polymorphic base → E0438.
    #[test]
    fn generic_virtual_method_on_base_emits_e0438() {
        let d = run(
            r#"
            public class Base { public <R> R pick(R x){ return x; } }
            public class Sub extends Base {}
            public void main() {}
            "#,
        );
        assert!(has(&d, code::Code::E0438_GenericVirtualMethod), "expected E0438: {d:?}");
    }

    /// A cast between two unrelated classes can never succeed → E0442.
    #[test]
    fn unrelated_class_cast_emits_e0442() {
        let d = run(
            r#"
            public abstract class Animal { public abstract String sound(); }
            public class Dog extends Animal { public Dog() {} public String sound() { return "w"; } }
            public class Cat extends Animal { public Cat() {} public String sound() { return "m"; } }
            public void main() { var dog = new Dog(); var c = dog as Cat; }
            "#,
        );
        assert!(has(&d, code::Code::E0442_UnrelatedCast), "expected E0442: {d:?}");
    }

    /// A type-test binder outside an `if` condition is rejected (E0441).
    #[test]
    fn typetest_binder_outside_if_emits_e0441() {
        let d = run(
            r#"
            public abstract class Animal { public abstract String s(); }
            public class Dog extends Animal { public Dog() {} public String s() { return "w"; } }
            public void main() { Animal a = new Dog(); var b = a => Dog d; }
            "#,
        );
        assert!(has(&d, code::Code::E0441_TypeTestBinderMisplaced), "expected E0441: {d:?}");
    }

    /// `if (x => Dog d)` binds `d: Dog` in the then-branch and is clean.
    #[test]
    fn typetest_smartcast_binder_in_if_is_ok() {
        let d = run(
            r#"
            public abstract class Animal { public abstract String s(); }
            public class Dog extends Animal { public Dog() {} public String s() { return "w"; } public String fetch() { return "f"; } }
            public void main() {
                Animal a = new Dog();
                if (a => Dog d) { print(d.fetch()); }
            }
            "#,
        );
        assert!(
            !has(&d, code::Code::E0441_TypeTestBinderMisplaced)
                && !has(&d, code::Code::E0413_UnresolvedMethod)
                && !has(&d, code::Code::E0442_UnrelatedCast),
            "valid smart-cast should be clean: {d:?}",
        );
    }

    /// A downcast to a subclass and an interface sidecast are valid (no E0442).
    #[test]
    fn downcast_and_interface_sidecast_are_ok() {
        let d = run(
            r#"
            public abstract class Animal { public abstract String sound(); }
            public interface Named { String label(); }
            public class Dog extends Animal { public Dog() {} public String sound() { return "w"; } }
            public class Tagged extends Animal implements Named { public Tagged() {} public String sound() { return "t"; } public String label() { return "T"; } }
            public void main() {
                Animal a = new Dog(); var d = a as Dog;        // downcast
                Animal t = new Tagged(); var n = t as Named;    // interface sidecast
            }
            "#,
        );
        assert!(!has(&d, code::Code::E0442_UnrelatedCast), "valid downcast/sidecast: {d:?}");
    }

    /// `super.method()` in a class with no superclass is rejected.
    #[test]
    fn super_without_superclass_is_rejected() {
        let d = run(
            r#"
            public class Animal { public String speak() { return super.speak(); } }
            public void main() {}
            "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("super")),
            "expected a `super` diagnostic: {d:?}",
        );
    }

    /// `super.method()` from a real override resolves cleanly (no error).
    #[test]
    fn super_call_from_override_is_ok() {
        let d = run(
            r#"
            public class Animal { public String speak() { return "generic"; } }
            public class Dog extends Animal {
                public Dog() {}
                public String speak() { return super.speak(); }
            }
            public void main() { var dog = new Dog(); print(dog.speak()); }
            "#,
        );
        assert!(
            !d.iter().any(|x| x.message.contains("super")),
            "valid super.method() should be clean: {d:?}",
        );
    }

    /// A generic method on a NON-extended (leaf) class is not a virtual
    /// dispatch concern → no E0438.
    #[test]
    fn generic_method_on_leaf_no_e0438() {
        let d = run(
            r#"
            public class Util { public <R> R pick(R x){ return x; } }
            public void main() {}
            "#,
        );
        assert!(!has(&d, code::Code::E0438_GenericVirtualMethod), "leaf generic method must be clean: {d:?}");
    }

    /// A generic-method interface used as a value-typed local can't be a
    /// trait object (object safety) → E0435.
    #[test]
    fn generic_method_interface_value_local_emits_e0435() {
        let d = run(
            r#"
            public interface Mapper { <R> R map(R input); }
            public class Id implements Mapper { public <R> R map(R input) { return input; } }
            public void main() { Mapper m = new Id(); }
            "#,
        );
        assert!(
            has(&d, code::Code::E0435_InterfaceNotDynDispatchable),
            "expected E0435 for generic-method interface value: {d:?}",
        );
    }

    /// A raw generic interface (no type argument) as a value type → E0435.
    #[test]
    fn raw_generic_interface_value_param_emits_e0435() {
        let d = run(
            r#"
            public interface Box<T> { T get(); }
            public void use(Box b) {}
            public void main() {}
            "#,
        );
        assert!(
            has(&d, code::Code::E0435_InterfaceNotDynDispatchable),
            "expected E0435 for raw generic interface value: {d:?}",
        );
    }

    /// A generic interface WITH a concrete type argument is a working trait
    /// object (`dyn Box<int>`) — must NOT trip E0435.
    #[test]
    fn concrete_generic_interface_value_is_ok() {
        let d = run(
            r#"
            public interface Box<T> { T get(); }
            public void use(Box<int> b) {}
            public void main() {}
            "#,
        );
        assert!(
            !has(&d, code::Code::E0435_InterfaceNotDynDispatchable),
            "Box<int> value type should be allowed: {d:?}",
        );
    }

    /// A plain non-generic interface value type is the common, supported
    /// case — never E0435.
    #[test]
    fn plain_interface_value_field_is_ok() {
        let d = run(
            r#"
            public interface Shape { double area(); }
            public class Holder { public Shape s; public Holder(Shape s) { this.s = s; } }
            public void main() {}
            "#,
        );
        assert!(
            !has(&d, code::Code::E0435_InterfaceNotDynDispatchable),
            "plain interface value field should be allowed: {d:?}",
        );
    }

    /// A bounded wildcard on a user generic class in a FIELD slot →
    /// E0444 (covariant container storage isn't supported in Phase 1).
    #[test]
    fn wildcard_storage_field_emits_e0444() {
        let d = run(
            r#"
            public class Animal { public String name; public Animal(String n) { this.name = n; } }
            public class Bag<T> { public T item; public Bag(T item) { this.item = item; } }
            public class Holder {
                public Bag<? extends Animal> contents;
                public Holder(Bag<? extends Animal> c) { this.contents = c; }
            }
            public void main() {}
            "#,
        );
        assert!(
            has(&d, code::Code::E0444_WildcardStorageUnsupported),
            "expected E0444 for wildcard storage field: {d:?}",
        );
    }

    /// A bounded wildcard in PARAMETER position lifts to a function
    /// generic and is sound — must NOT trip E0444.
    #[test]
    fn wildcard_param_does_not_emit_e0444() {
        let d = run(
            r#"
            public class Animal { public String name; public Animal(String n) { this.name = n; } }
            public class Bag<T> { public T item; public Bag(T item) { this.item = item; } }
            public void describe(Bag<? extends Animal> b) {}
            public void main() {}
            "#,
        );
        assert!(
            !has(&d, code::Code::E0444_WildcardStorageUnsupported),
            "param-position wildcard should be allowed: {d:?}",
        );
    }

    /// A concrete type argument in a storage slot (`Bag<Dog>`) carries no
    /// wildcard — never E0444.
    #[test]
    fn concrete_storage_field_is_ok() {
        let d = run(
            r#"
            public class Animal { public String name; public Animal(String n) { this.name = n; } }
            public class Dog extends Animal { public Dog(String n) { super(n); } }
            public class Bag<T> { public T item; public Bag(T item) { this.item = item; } }
            public class Holder {
                public Bag<Dog> contents;
                public Holder(Bag<Dog> c) { this.contents = c; }
            }
            public void main() {}
            "#,
        );
        assert!(
            !has(&d, code::Code::E0444_WildcardStorageUnsupported),
            "concrete-arg storage field should be allowed: {d:?}",
        );
    }

    /// A type supplied where a const value is expected
    /// (`new Buf<String>()` against `class Buf<int N>`) → E0445.
    #[test]
    fn type_in_const_slot_emits_e0445() {
        let d = run(
            r#"
            public class Buf<int N> { public Buf() { } }
            public void main() { var b = new Buf<String>(); }
            "#,
        );
        assert!(
            has(&d, code::Code::E0445_ConstGenericUnsupported),
            "expected E0445 for a type in a const slot: {d:?}",
        );
    }

    /// A literal supplied where a type is expected (`new Box<256>(5)`)
    /// → E0445.
    #[test]
    fn literal_in_type_slot_emits_e0445() {
        let d = run(
            r#"
            public class Box<T> { public T v; public Box(T v) { this.v = v; } }
            public void main() { var b = new Box<256>(5); }
            "#,
        );
        assert!(
            has(&d, code::Code::E0445_ConstGenericUnsupported),
            "expected E0445 for a literal in a type slot: {d:?}",
        );
    }

    /// Const-generic arithmetic in an array size (`new int[N + 1]`)
    /// → E0445 (needs the const-eval interpreter, deferred).
    #[test]
    fn const_arithmetic_array_size_emits_e0445() {
        let d = run(
            r#"
            public class S<int N> {
                public S() { }
                public int probe() { var a = new int[N + 1]; return 0; }
            }
            public void main() { }
            "#,
        );
        assert!(
            has(&d, code::Code::E0445_ConstGenericUnsupported),
            "expected E0445 for const arithmetic in an array size: {d:?}",
        );
    }

    /// A class object captured by a `Worker.spawn` closure → E0702
    /// (Rc-backed objects are !Send; rustc E0277 would leak).
    #[test]
    fn object_captured_by_spawn_emits_e0702() {
        let d = run(
            r#"
            public class Counter { public int n; public Counter() { this.n = 0; } }
            public void main() {
                var c = new Counter();
                var t = Worker.spawn(() -> { return c.n; });
            }
            "#,
        );
        assert!(
            has(&d, code::Code::E0702_ObjectCapturedBySpawn),
            "expected E0702 for object capture in spawn: {d:?}",
        );
    }

    /// Primitive / String captures cross threads fine — no E0702.
    #[test]
    fn primitive_captures_in_spawn_are_ok() {
        let d = run(
            r#"
            public void main() {
                int n = 5;
                String tag = "x";
                var t = Worker.spawn(() -> { return n + tag.length(); });
            }
            "#,
        );
        assert!(
            !has(&d, code::Code::E0702_ObjectCapturedBySpawn),
            "primitive captures must not fire E0702: {d:?}",
        );
    }

    /// The supported core — `<int N>` declared, used bare as an array
    /// size and as an int value, instantiated with a literal — carries
    /// no E0445.
    #[test]
    fn const_generic_core_subset_is_clean() {
        let d = run(
            r#"
            public class Buf<int N> {
                public int[N] data;
                public Buf() { this.data = new int[N]; }
                public int cap() { return N; }
            }
            public void main() { var b = new Buf<4>(); print(b.cap()); }
            "#,
        );
        assert!(
            !has(&d, code::Code::E0445_ConstGenericUnsupported),
            "core const-generic subset should be clean: {d:?}",
        );
    }
}
