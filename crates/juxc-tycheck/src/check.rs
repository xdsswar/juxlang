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
    "print", "parallel", "block_on", "yield_now", "Worker", "now_ms",
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
            in_static: false,
            in_async: false,
            uninferable_news: Vec::new(),
            used_names: std::collections::HashSet::new(),
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

    /// Consume the checker, returning the per-expression type map it
    /// built up during [`Self::check_unit`]. Called once at the end of
    /// the top-level `typecheck()` driver.
    pub(crate) fn into_expr_types(self) -> HashMap<Span, Ty> {
        self.expr_types
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
        self.env.clear_class();
    }

    // ------------------------------------------------------------------
    // Function / method / constructor walkers
    // ------------------------------------------------------------------

    /// Walk a top-level function. Pushes a parameter scope, sets the
    /// expected return type, walks the body, then restores both.
    /// Abstract / native functions (body = None) are skipped.
    fn check_function(&mut self, fn_decl: &FnDecl) {
        let Some(body) = &fn_decl.body else { return };
        self.env.push_scope();
        // Declare each parameter into the new scope so name lookups
        // inside the body resolve.
        for param in &fn_decl.params {
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
        }
        let saved = self.current_return.take();
        self.current_return = Some(return_type_to_ty(
            &fn_decl.return_type,
            &self.env,
            self.symbols,
        ));
        let saved_async = self.in_async;
        self.in_async = Self::fn_is_async(fn_decl);
        self.check_block(body);
        self.flush_uninferable_news();
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
        // Pre-compute the `this` type: User<class_name, [Param(T)…]>.
        let this_ty = Ty::User {
            name: class_name.clone(),
            generic_args: class
                .generic_params
                .iter()
                .map(|tp| Ty::Param(tp.name.text.clone()))
                .collect(),
        };

        for ctor in &class.constructors {
            self.check_constructor(ctor, &this_ty);
        }
        for method in &class.methods {
            self.check_method(method, &this_ty);
        }
        for op in &class.operators {
            self.check_operator(op, &this_ty);
        }

        self.env.clear_generic_params();
        self.env.clear_class();
    }

    /// Walk a constructor body. Like [`check_function`] but with no
    /// expected return type (constructors don't return values) and with
    /// `this` pre-declared.
    fn check_constructor(&mut self, ctor: &ConstructorDecl, this_ty: &Ty) {
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
        self.flush_uninferable_news();
        self.in_async = saved_async;
        self.current_return = saved;
        self.env.pop_scope();
    }

    /// Walk an instance method body. Same scope shape as a function
    /// plus a `this` binding. Abstract methods (body = None) are
    /// skipped.
    fn check_method(&mut self, method: &FnDecl, this_ty: &Ty) {
        let Some(body) = &method.body else { return };
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
        let saved_async = self.in_async;
        self.in_async = Self::fn_is_async(method);
        self.check_block(body);
        self.flush_uninferable_news();
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
                self.env.push_scope();
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

            Stmt::ForEach(f) => {
                self.check_expr(&f.iter);
                let iter_ty = infer_expr(&f.iter, &self.env, self.symbols);
                // Loop-var binding: explicit annotation wins; else
                // element-of-array if iter is an array; else Unknown.
                let var_ty = if let Some(declared) = &f.var_type {
                    ty_from_ref(declared, &self.env, self.symbols)
                } else {
                    match iter_ty {
                        Ty::Array { element, .. } => *element,
                        _ => Ty::Unknown,
                    }
                };
                self.env.push_scope();
                self.env.declare(&f.var_name.text, var_ty);
                self.check_block(&f.body);
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
                self.check_block(&t.body);
                // Caught types so far, to detect an unreachable later clause
                // (§X.3.4): a catch whose type is the same as, or a subtype of,
                // an earlier clause's can never run.
                let mut caught: Vec<Ty> = Vec::new();
                for c in &t.catches {
                    let ty = ty_from_ref(&c.ty, &self.env, self.symbols);
                    if caught.iter().any(|earlier| is_subtype(&ty, earlier, self.symbols)) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0720_UnreachableCatch,
                                format!(
                                    "unreachable `catch ({})`: an earlier clause already \
                                     catches it",
                                    ty,
                                ),
                            )
                            .with_span(c.span)
                            .with_help("reorder catches so more specific types come first"),
                        );
                    }
                    caught.push(ty);
                    self.env.push_scope();
                    // Bind the caught name with the declared type
                    // so the body sees `e` as a normal local.
                    let bind_ty = ty_from_ref(&c.ty, &self.env, self.symbols);
                    self.env.declare(&c.name.text, bind_ty);
                    self.check_block(&c.body);
                    self.env.pop_scope();
                }
                if let Some(fin) = &t.finally {
                    self.check_block(fin);
                }
            }

            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    /// Recurse through else / else-if chains, mirroring [`Self::check_stmt`]'s
    /// handling for the top-level `if`.
    fn check_else_branch(&mut self, branch: &ElseBranch) {
        match branch {
            ElseBranch::If(if_stmt) => {
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
                self.env.push_scope();
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

            Expr::NewArray(n) => self.check_expr(&n.size),

            Expr::NewArrayLit(n) => {
                for el in &n.elements {
                    self.check_expr(el);
                }
            }

            Expr::Cast(c) => self.check_expr(&c.value),

            Expr::Range(r) => {
                self.check_expr(&r.start);
                self.check_expr(&r.end);
            }

            Expr::Unary(u) => {
                self.check_expr(&u.operand);
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
                let accessor_pkg: &[String] = accessor
                    .and_then(|name| self.symbols.classes.get(name))
                    .map(|c| c.package.as_slice())
                    .unwrap_or(&[]);
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
                let accessor_pkg: &[String] = accessor
                    .and_then(|name| self.symbols.classes.get(name))
                    .map(|c| c.package.as_slice())
                    .unwrap_or(&[]);
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
        let kind = if let Some(e) = self.symbols.enums.get(scrut_name) {
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
    fn check_call(&mut self, c: &CallExpr) {
        // Always walk args first, regardless of callee shape, so nested
        // checks still fire.
        match c.callee.as_ref() {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = &qn.segments[0].text;
                // Built-in functions accept anything.
                if BUILTINS.contains(&name.as_str()) {
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    return;
                }
                if let Some(fn_sig) = self.symbols.functions.get(name) {
                    let params = fn_sig.params.clone();
                    let generic_params = fn_sig.generic_params.clone();
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
                    self.check_call_args(
                        name,
                        &params,
                        &c.args,
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
                        let class_method = self
                            .symbols
                            .classes
                            .get(&class_fqn)
                            .and_then(|c| c.methods.get(method_name))
                            .cloned();
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
                                self.check_call_args(
                                    method_name,
                                    &method.params,
                                    &c.args,
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
                    let params = method.params.clone();
                    let method_generic_params = method.generic_params.clone();
                    let method_vis = method.visibility;
                    let method_is_static = method.is_static;
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
                            c.span,
                            Some(&name),
                            &subst_params,
                            &subst_args,
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
        let class_name = match n.class_name.segments.last() {
            Some(seg) => seg.text.clone(),
            None => return,
        };

        // Lower the explicit generic args (if any) into `Ty`s. Empty
        // when the user wrote the bare `new Box(...)` form — in that
        // case we'll try inference (spec §T.4) below.
        let explicit_generic_args: Vec<Ty> = n
            .generic_args
            .iter()
            .map(|g| ty_from_ref(g, &self.env, self.symbols))
            .collect();

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
            // At most one constructor in Turn 1. If there are none, the
            // synthesized default takes zero args.
            let params: Vec<ParamSig> = class
                .constructors
                .first()
                .map(|c| c.params.clone())
                .unwrap_or_default();
            let ctor_vis = class
                .constructors
                .first()
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
        let params: Vec<ParamSig> = parent
            .constructors
            .first()
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

    fn check_call_args(
        &mut self,
        callee_name: &str,
        params: &[ParamSig],
        args: &[Expr],
        call_span: Span,
        declaring_class: Option<&str>,
        subst_params: &[TypeParam],
        subst_args: &[Ty],
    ) {
        if params.len() != args.len() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0411_WrongArgCount,
                    format!(
                        "`{}` expects {} argument{}, got {}",
                        callee_name,
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        args.len(),
                    ),
                )
                .with_span(call_span),
            );
            // Still check the overlapping prefix for type mismatches so
            // the user gets every problem at once.
        }
        for (i, arg) in args.iter().enumerate() {
            self.check_expr(arg);
            let Some(param) = params.get(i) else { break };
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
        UnaryOp::Neg => OperatorKind::Minus,
        UnaryOp::BitNot => OperatorKind::BitNot,
        UnaryOp::Not => return None,
    })
}

/// Human-readable spelling of an [`OperatorKind`] for diagnostics.
/// Matches the form the user would have written (`==`, `<=>`, `hash`,
/// `string`, …). Mirrors the same helper in `symbol_table.rs`.
fn operator_kind_user_spelling(kind: OperatorKind) -> &'static str {
    match kind {
        OperatorKind::Eq => "==",
        OperatorKind::Cmp => "<=>",
        OperatorKind::Lt => "<",
        OperatorKind::Le => "<=",
        OperatorKind::Gt => ">",
        OperatorKind::Ge => ">=",
        OperatorKind::Hash => "hash",
        OperatorKind::ToString => "string",
        OperatorKind::Plus => "+",
        OperatorKind::Minus => "-",
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
fn expr_span(e: &Expr) -> Span {
    match e {
        Expr::Literal(_) => Span::DUMMY,
        Expr::Path(qn) => qn.span,
        Expr::Call(c) => c.span,
        Expr::Binary(b) => b.span,
        Expr::Unary(u) => u.span,
        Expr::Range(r) => r.span,
        Expr::Cast(c) => c.span,
        Expr::SizeOf(s) => s.span,
        Expr::NewArray(n) => n.span,
        Expr::NewArrayLit(n) => n.span,
        Expr::Index(i) => i.span,
        Expr::Field(f) => f.span,
        Expr::InterpString(s) => s.span,
        Expr::This(s) => *s,
        Expr::NewObject(n) => n.span,
        Expr::Switch(s) => s.span,
        Expr::Lambda(l) => l.span,
        Expr::Elvis(e) => e.span,
        Expr::MethodRef(m) => m.span,
        Expr::Ternary(t) => t.span,
        Expr::Await(_, s) => *s,
    }
}

/// True when `pattern` is irrefutable — covers every value of the
/// scrutinee type. Used by the exhaustiveness check to detect
/// catchall arms (`case _ -> …`, `case name -> …`). Variant
/// patterns are NOT irrefutable, even when their sub-patterns are
/// — they only cover their specific variant.
fn pattern_is_catchall(p: &Pattern) -> bool {
    matches!(p, Pattern::Wildcard(_) | Pattern::Bind(_))
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
}
