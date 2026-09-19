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
use crate::infer::infer_expr;
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
pub const BUILTINS: &[&str] = &[
    "print",
    "parallel",
    "block_on",
    "yield_now",
    "Worker",
    "now_ms",
    "assert",
    "spawn",
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
/// Methods every observable property carries (JUX-OBSERVABLE-PROPERTIES §P.4):
/// `target.Prop.bind(source.Prop)`, `bindBidirectional`, and `unbind()`.
///
/// They are called on a PROPERTY, whose value type is whatever the property
/// holds, so the receiver of `celsius.bind(...)` infers as a plain `double`.
/// Every member check that looks at the receiver's TYPE has to let these
/// through when the receiver is a property access. The backend's binding
/// dispatch reads this same list.
pub const PROPERTY_BINDING_METHODS: &[&str] = &["bind", "bindBidirectional", "unbind"];

/// Methods Jux gives a `char` receiver (JUX-LANG-V1 5.2). The backend lowers
/// each to a Rust `char` call; `juxc_backend_rust`'s
/// `primitive_method_tables_agree` test pins the two together.
pub const BUILTIN_CHAR_METHODS: &[&str] = &[
    "codePoint",
    "isAlphabetic",
    "isDigit",
    "isLowercase",
    "isUppercase",
    "isWhitespace",
    "toLowercase",
    "toUppercase",
];

/// Methods Jux gives a floating-point receiver.
pub const BUILTIN_FLOAT_METHODS: &[&str] = &[
    "abs",
    "bits",
    "bitsEqual",
    "ceil",
    "floor",
    "isFinite",
    "isInfinite",
    "isNaN",
    "round",
    "sqrt",
    "toFixed",
    "totalOrder",
];

/// Methods Jux gives an integer receiver, signed or unsigned.
pub const BUILTIN_INT_METHODS: &[&str] = &[
    "checkedAdd",
    "checkedDiv",
    "checkedMul",
    "checkedSub",
    "countOnes",
    "leadingZeros",
    "rotateLeft",
    "rotateRight",
    "saturatingToInt",
    "saturatingAdd",
    "saturatingMul",
    "saturatingSub",
    "toBinary",
    "toHex",
    "toOctal",
    "trailingZeros",
    "toInt",
    "wrappingAdd",
    "wrappingMul",
    "wrappingSub",
];

/// Methods an integer receiver has only when it is SIGNED. Rust has no `abs`
/// on an unsigned integer, and neither does Jux: there is nothing for it to do.
pub const BUILTIN_SIGNED_INT_METHODS: &[&str] = &["abs", "saturatingAbs"];

/// Every method name legal on `prim`, or `None` when the primitive carries no
/// method surface at all (`bool`, `void`).
pub fn builtin_primitive_methods(prim: Primitive) -> Option<Vec<&'static str>> {
    use Primitive as P;
    match prim {
        P::Char => Some(BUILTIN_CHAR_METHODS.to_vec()),
        P::Float | P::Double | P::F32 | P::F64 => Some(BUILTIN_FLOAT_METHODS.to_vec()),
        P::Bool => None,
        other => {
            let mut names = BUILTIN_INT_METHODS.to_vec();
            if is_signed_primitive(other) {
                names.extend_from_slice(BUILTIN_SIGNED_INT_METHODS);
            }
            Some(names)
        }
    }
}

/// Is this integer primitive signed? Mirrors the backend's own test, which
/// reads the leading `u` off the lowered Rust type name.
pub fn is_signed_primitive(prim: Primitive) -> bool {
    use Primitive as P;
    !matches!(
        prim,
        P::Uint | P::Ubyte | P::Ushort | P::Ulong | P::U8 | P::U16 | P::U32 | P::U64
    )
}

const BUILTIN_ARRAY_METHODS: &[&str] = &[
    "push", "pop", "clone", "len", "length", // List<T> spec methods.
    "add", "get", "set", "contains", "indexOf", "isEmpty", "size", "first", "last", "reverse",
    "sort", "clear", "remove", "insert", "join", "map", "filter", "forEach",
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
    "length",
    "len",
    "clone",
    "chars",
    "bytes",
    "to_string",
    // String spec methods.
    "split",
    "trim",
    "contains",
    "startsWith",
    "endsWith",
    "toUpperCase",
    "toLowerCase",
    "replace",
    "indexOf",
    "substring",
    "charAt",
    "isEmpty",
    // §K.7 surface: explicit byte/char length forms + repeat.
    "byteLength",
    "charLength",
    "repeat",
];

/// Field/property names we allow on **any array receiver** without a
/// class lookup. Today just `length`; the typechecker treats it as `Int`.
const BUILTIN_ARRAY_FIELDS: &[&str] = &["length"];

/// Field/property names we allow on a **String receiver**.
const BUILTIN_STRING_FIELDS: &[&str] = &["length"];

// ============================================================================
// Checker
// ============================================================================

/// How close two method names are, for suggesting one when the other does
/// not resolve. 1.0 is identical, 0.0 is nothing in common.
///
/// One name being a PREFIX of the other scores high whatever the lengths:
/// that is the shape of the misses that actually happen -- `sort` for
/// `sort_unstable`, `push` for `push_mut` -- and a length ratio would score
/// exactly those near zero. Otherwise the shared prefix is scaled by the
/// longer name, so unrelated names stay far apart.
fn name_affinity(a: &str, b: &str) -> f32 {
    let la = a.to_ascii_lowercase();
    let lb = b.to_ascii_lowercase();
    if la == lb {
        return 1.0;
    }
    if !la.is_empty() && (lb.starts_with(&la) || la.starts_with(&lb)) {
        return 0.9;
    }
    let shared = la
        .chars()
        .zip(lb.chars())
        .take_while(|(x, y)| x == y)
        .count();
    if shared == 0 {
        return 0.0;
    }
    shared as f32 / la.len().max(lb.len()) as f32
}

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
    /// Per `T[N]` local, where it was first handed to a runtime-sized `T[]`
    /// slot and where to a fixed `T[N]` one. Both at once is E0468
    /// (JUX-LANG-V1 §5.5); decided when the local's block ends.
    pub(crate) fixed_array_slot_uses: std::collections::HashMap<String, (Option<Span>, Option<Span>)>,
    /// Raw-pointer depth of the enclosing function's declared return type
    /// (`int*` is 1), for the pointer check on `return` (§L.6.1a).
    pub(crate) current_return_ptr: u8,
    /// Whether the enclosing function returns a pointer to `void`.
    pub(crate) current_return_void: bool,
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
    /// Record-destructuring reads resolved to component names, keyed by the
    /// field identifier's span (see `juxc_ast::record_destructure_temp`).
    pub(crate) component_names: HashMap<Span, String>,
    /// Constructor-overload selections — `new T(...)` / `super(...)` /
    /// `this(...)` call span → index into the class's constructor
    /// list. Absorbed into `SymbolTable::ctor_selections` after the
    /// walk so the backend can pick `new` vs `new__K` per call site.
    pub(crate) ctor_selections: HashMap<Span, usize>,
    /// Method-overload selections (call span → group index), absorbed
    /// into `SymbolTable::method_selections` after the walk. Mirrors
    /// `ctor_selections`.
    pub(crate) method_selections: HashMap<Span, usize>,
    /// Free-function overload selections (call span → group index),
    /// absorbed into `SymbolTable::function_selections`. The same
    /// mechanism as `method_selections`, for the other kind of callee.
    pub(crate) function_selections: HashMap<Span, usize>,
    /// Binary expressions resolved to a free-function operator (§7.14):
    /// binary span -> (function key, overload index). Absorbed into
    /// `SymbolTable::free_operator_calls` for the backend.
    pub(crate) free_operator_calls: HashMap<Span, (String, usize)>,
    /// Typed `assertThrows<E>(f)` calls (§TS.3): call span -> the FQN of `E`,
    /// absorbed into `SymbolTable::typed_assert_throws`.
    pub(crate) typed_assert_throws: HashMap<Span, String>,
    /// Record patterns (§A.3): pattern span -> the record's FQN, absorbed into
    /// `SymbolTable::record_patterns`.
    pub(crate) record_patterns: HashMap<Span, String>,
    /// Names the block being checked assigns to, anywhere inside it. Read by
    /// [`Self::narrowable`]: an assignment can put a null back, so a binding
    /// the block writes to is never narrowed by a null test (§7.10).
    pub(crate) assigned_in_block: std::collections::HashSet<String>,
    /// `Some(index)` while walking constructor `index`'s body —
    /// `this(...)` delegation is only legal there (and only as the
    /// first statement), and a delegation may not resolve back to
    /// the declaring constructor itself.
    pub(crate) current_ctor: Option<usize>,
    /// The labeled statements enclosing the one being checked, innermost
    /// last: the label and whether it names a loop (`true`) or a block
    /// (`false`). `break name;` needs one of them; `continue name;` needs a
    /// loop (Grammar §A.2.8, E0241). A lambda body starts with none: a jump
    /// cannot leave the closure.
    pub(crate) labels: Vec<(String, bool)>,
    /// Labels that enclose the current lambda from OUTSIDE it: not jump
    /// targets (a jump can't leave the closure), but worth naming when a
    /// `break outer;` inside the lambda reaches for one.
    pub(crate) labels_outside_closure: Vec<String>,
    /// `true` while walking an instance or `static` `init { }` block.
    /// Like `current_ctor.is_some()`, this is a context in which a
    /// `final`/`const` field MAY be assigned (the init runs during
    /// construction / class setup); used by the E0465 field-reassignment
    /// check to stay false-positive-free on legitimate init-block writes.
    pub(crate) in_init_block: bool,
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
    /// The parameter types of the function-type slot the next lambda is
    /// written into (`(double) -> String show = v -> ...`), taken by that
    /// lambda. A parameter declared without a type has the slot's type, both
    /// for checking the body and in `expr_types` at the parameter's name.
    pub(crate) lambda_slot_params: Option<Vec<Ty>>,
    /// True while checking a for-each header's iterable expression —
    /// the one position a `step` range is legal in Phase 1.
    pub(crate) in_foreach_iter: bool,
    /// True while we're walking the body of a `static` method (or
    /// a `static` field initializer once those land). Drives the
    /// `E0425_ThisInStaticContext` diagnostic in `check_expr`.
    pub(crate) in_static: bool,
    /// True while the receiver of an `I.super.m()` call is being checked, so
    /// `check_field_access` accepts `I.super` there and nowhere else (§T.8.3).
    pub(crate) in_interface_super_call: bool,
    /// Names a `while` condition refined for the loop body being checked, with
    /// their declared (nullable) types (§T.6.5). A name leaves the list at the
    /// first body statement that assigns it.
    pub(crate) loop_narrowed: Vec<(String, Ty)>,
    /// Set while checking a direct `x = e;` that ends a loop refinement of
    /// `x`: the assignment's target has the declared `T?` type.
    pub(crate) loop_assign_widen: Option<(String, Ty)>,
    /// True while we're inside an **async context** — the body of an
    /// `async` function/method, or an async lambda. Drives the
    /// `E0700_AwaitRequiresAsyncContext` check: `await` is only legal when
    /// this is set (async addendum §18.1.2). Reset to `false` inside a
    /// constructor body and inside a non-async lambda.
    pub(crate) in_async: bool,
    /// True while checking an expression that legitimately CONSUMES a
    /// future: the operand of `await`, or an argument to the executor
    /// builtins (`spawn`/`block_on`/`parallel`/`withTimeout`/
    /// `Task.*`/`Worker.spawn`). Outside such a slot, a call to an
    /// `async` callee is an unstarted future used as a value —
    /// E0705 (§18.1.2: direct async calls require `await`).
    pub(crate) in_future_slot: bool,
    /// True while we're inside an **unsafe context** — the body of an
    /// `unsafe` function/method, or an `unsafe { … }` block. Drives the
    /// `E0506_UnsafeOpOutsideUnsafe` check: calling an `unsafe` function (a
    /// foreign `unsafe fn` stub) is only legal when this is set (grammar
    /// §A.2.8). Reset to `false` inside a non-unsafe lambda.
    pub(crate) in_unsafe: bool,
    /// The unit being checked is a generated crate stub (`.jux.d`), whose
    /// binding markers are not annotations a program writes (W0241 skips it).
    pub(crate) checking_external_unit: bool,
    /// `var x = new X<>()` declarations whose inferred type carries an
    /// **unresolved** generic argument (nothing at the construction site pinned
    /// it). Flushed at the end of each function/method/constructor body: a
    /// candidate whose name never appears in [`Self::used_names`] is genuinely
    /// uninferable (an unused, type-ambiguous local) and gets E0453 — turning
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

/// Everything the checker hands downstream, all keyed by expression [`Span`]:
/// inferred types, the call-sugar expansion plan (which argument came from
/// where), the selected constructor overload, and the selected method overload.
/// Named because a bare four-tuple of `HashMap`s at a function boundary tells a
/// reader nothing about which is which.
pub(crate) type CheckerMaps = (
    HashMap<Span, Ty>,
    HashMap<Span, Vec<crate::ArgSource>>,
    HashMap<Span, usize>,
    HashMap<Span, usize>,
    HashMap<Span, usize>,
    HashMap<Span, String>,
    HashMap<Span, String>,
    HashMap<Span, String>,
    HashMap<Span, (String, usize)>,
);

impl<'a> Checker<'a> {
    /// Construct a fresh checker. `symbols` is the Phase-A symbol table;
    /// `diagnostics` is the same vec the rest of typecheck appends to.
    pub(crate) fn new(symbols: &'a SymbolTable, diagnostics: &'a mut Vec<Diagnostic>) -> Self {
        Self {
            env: TypeEnv::new(),
            symbols,
            diagnostics,
            current_return: None,
            fixed_array_slot_uses: std::collections::HashMap::new(),
            current_return_ptr: 0,
            current_return_void: false,
            expr_types: HashMap::new(),
            call_expansions: HashMap::new(),
            component_names: HashMap::new(),
            ctor_selections: HashMap::new(),
            method_selections: HashMap::new(),
            function_selections: HashMap::new(),
            free_operator_calls: HashMap::new(),
            typed_assert_throws: HashMap::new(),
            record_patterns: HashMap::new(),
            assigned_in_block: std::collections::HashSet::new(),
            current_ctor: None,
            labels: Vec::new(),
            labels_outside_closure: Vec::new(),
            in_init_block: false,
            checked_escapes: Vec::new(),
            catch_absorb_stack: Vec::new(),
            lambda_depth: 0,
            lambda_slot_params: None,
            in_foreach_iter: false,
            in_static: false,
            in_interface_super_call: false,
            loop_narrowed: Vec::new(),
            loop_assign_widen: None,
            in_async: false,
            in_future_slot: false,
            in_unsafe: false,
            checking_external_unit: false,
            uninferable_news: Vec::new(),
            used_names: std::collections::HashSet::new(),
            poly_bases: crate::symbol_table::polymorphic_base_bare_names(symbols),
            const_param_names: std::collections::HashSet::new(),
        }
    }

    /// Emit `E0453` for every recorded `var x = new X<>()` whose `x` was never
    /// referenced in the just-walked body (so nothing could pin its generic
    /// argument), then clear the per-body tracking sets. Called at the end of
    /// each function/method/constructor walk.
    fn flush_uninferable_news(&mut self) {
        let candidates = std::mem::take(&mut self.uninferable_news);
        for (name, span) in candidates {
            if !self.used_names.contains(&name) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0453_GenericInferenceNoSolution,
                        format!(
                            "cannot infer the type argument of `{name}`: it is never used to \
                             pin it -- write the type explicitly, e.g. `new Vec<String>()`",
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
            let Some(class) = self.symbols.classes.get(&k) else {
                break;
            };
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

    /// Consume the checker, returning every span-keyed map it built.
    pub(crate) fn into_maps(self) -> CheckerMaps {
        (
            self.expr_types,
            self.call_expansions,
            self.ctor_selections,
            self.method_selections,
            self.function_selections,
            self.typed_assert_throws,
            self.record_patterns,
            self.component_names,
            self.free_operator_calls,
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
        self.check_annotation_applications(unit);
        // E0933: a `HashMap`/`HashSet` key type with no `operator hash`.
        crate::hash_keys::check_unit(unit, &self.env, self.symbols, self.diagnostics);
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(fn_decl) => self.check_function(fn_decl),
                TopLevelDecl::Annotation(decl) => self.check_annotation_decl(decl),
                TopLevelDecl::Class(class) => self.check_class(class),
                TopLevelDecl::Record(record) => self.check_record(record),
                TopLevelDecl::Enum(enum_decl) => self.check_enum(enum_decl),
                // A DEFAULT method has a body, and it went unwalked for as
                // long as this arm said interfaces carry only signatures.
                TopLevelDecl::Interface(iface) => self.check_interface(iface),
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
                    let slot_ty = match &c.ty {
                        Some(t) => ty_from_ref(t, &self.env, self.symbols),
                        None => found.clone(),
                    };
                    self.check_const_integer_fits(&c.name.text, &slot_ty, &c.value, c.span);
                    self.check_const_initializer_folds(&c.name.text, &slot_ty, &c.value, c.span);
                }
                // Foreign-function blocks: validate each signature is
                // FFI-compatible (E0508). No bodies to walk.
                TopLevelDecl::ExternBlock(block) => self.check_extern_block(block),
            }
        }
    }

    /// §T.11: a constant whose initializer CALLS something is computed at
    /// compile time, and the program stores the result. When that cannot be
    /// done the constant has no value, so say why here: E0840 when the
    /// evaluation runs out of budget (a loop that never ends), E0842 when it
    /// overflows or divides by zero, E0841 when the call does non-constant work.
    /// An initializer without a call is left to the checks that already cover
    /// it.
    fn check_const_initializer_folds(&mut self, name: &str, slot: &Ty, init: &Expr, span: juxc_source::Span) {
        fn has_call(e: &Expr) -> bool {
            match e {
                Expr::Call(_) => true,
                Expr::Binary(b) => has_call(&b.left) || has_call(&b.right),
                Expr::Unary(u) => has_call(&u.operand),
                Expr::Ternary(t) => has_call(&t.condition) || has_call(&t.then_branch) || has_call(&t.else_branch),
                Expr::Cast(c) => has_call(&c.value),
                _ => false,
            }
        }
        if !has_call(init) {
            return;
        }
        let ctx = crate::const_eval::ConstCtx {
            symbols: self.symbols,
            generic_param_names: &self.const_param_names,
            enclosing_class: None,
        };
        let result = match slot {
            Ty::Primitive(Primitive::Bool) => crate::const_eval::eval_const_bool(init, &ctx).map(|_| ()),
            Ty::String => crate::const_eval::eval_const_string(init, &ctx).map(|_| ()),
            Ty::Primitive(p) if crate::ty::integer_bits(*p).is_some() => {
                crate::const_eval::eval_const_int(init, &ctx).map(|_| ())
            }
            _ => Err(crate::const_eval::ConstEvalError::NonConst(format!(
                "a constant of type {slot} cannot be computed by a call yet; compile-time evaluation covers integers, bools and Strings"
            ))),
        };
        let at = if expr_span(init) == juxc_source::Span::DUMMY { span } else { expr_span(init) };
        let diagnostic = match result {
            Ok(()) | Err(crate::const_eval::ConstEvalError::Generic) => return,
            Err(crate::const_eval::ConstEvalError::LimitExceeded) => Diagnostic::error(
                code::Code::E0840_ConstEvalLimitExceeded,
                format!("constant `{name}` did not finish computing: its evaluation exceeded the compile-time limit (a loop that never ends, or recursion too deep)"),
            ),
            Err(crate::const_eval::ConstEvalError::Panic(msg)) => Diagnostic::error(
                code::Code::E0842_ConstEvalPanic,
                format!("constant `{name}` fails while computing: {msg}"),
            ),
            Err(crate::const_eval::ConstEvalError::NonConst(msg)) => Diagnostic::error(
                code::Code::E0841_NonConstInConstContext,
                format!("constant `{name}` cannot be computed at compile time: {msg}"),
            )
            .with_help("compute it at run time instead: a `static` field set in a `static { }` block"),
        };
        self.diagnostics.push(diagnostic.with_span(at));
    }

    /// Validate every signature in a `@extern … unsafe native { … }` block is
    /// FFI-compatible (Layout-ABI §L.7). Each parameter and the return type must
    /// be a primitive, a raw pointer (`T*`/`void*`), `String` (marshalled to/from
    /// C `const char*`), or `void` (return only). Anything else (classes,
    /// generics, arrays, collections, `throws`) trips **E0508**.
    fn check_extern_block(&mut self, block: &juxc_ast::ExternBlockDecl) {
        for f in &block.fns {
            // A C-variadic function (`...`) needs at least one fixed parameter
            // before the `...` — `int f(...)` is illegal in C and Rust rejects a
            // bare-`...` `extern "C"` signature (§L.4.2).
            if f.is_c_variadic && f.params.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0508_FfiTypeNotAllowed,
                        format!(
                            "foreign function `{}` is variadic (`...`) but has no fixed parameter \
                             before it -- a C variadic function needs at least one named parameter \
                             (e.g. the `printf` format string)",
                            f.name.text,
                        ),
                    )
                    .with_span(f.span),
                );
            }
            if !f.throws.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0508_FfiTypeNotAllowed,
                        format!(
                            "foreign function `{}` cannot declare `throws` -- C has no \
                             exceptions; return an error code or `out` parameter instead",
                            f.name.text,
                        ),
                    )
                    .with_span(f.span),
                );
            }
            for p in &f.params {
                if !self.ffi_type_ok(&p.ty) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0508_FfiTypeNotAllowed,
                            format!(
                                "parameter `{}` of foreign function `{}` has type `{}`, which is \
                                 not allowed at the C boundary -- use a primitive, a raw pointer \
                                 (`T*`), or `String`",
                                p.name.text,
                                f.name.text,
                                type_ref_display(&p.ty),
                            ),
                        )
                        .with_span(p.span),
                    );
                }
            }
            // Return type: `void` is fine; otherwise the same rule as a param.
            if let juxc_ast::ReturnType::Type(t) = &f.return_type {
                if !self.ffi_type_ok(t) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0508_FfiTypeNotAllowed,
                            format!(
                                "return type `{}` of foreign function `{}` is not allowed at the \
                                 C boundary -- use a primitive, a raw pointer (`T*`), `String`, or \
                                 `void`",
                                type_ref_display(t),
                                f.name.text,
                            ),
                        )
                        .with_span(f.span),
                    );
                }
            }
        }
    }

    /// True when `t` is a valid FIELD type of a `@layout(c)` value struct: a
    /// numeric/bool/char primitive, a raw pointer, another `@layout(c)` struct,
    /// or a `@layout(c)` C enum (a plain `#[repr(int)]` integer, the canonical C
    /// "status code in a struct" shape). Stricter than [`Self::ffi_type_ok`] -
    /// `String` is allowed as a marshalled *parameter* but NOT as a `#[repr(C)]`
    /// `Copy` struct field (it is a non-`Copy` fat pointer with no C field
    /// representation).
    fn ffi_struct_field_ok(&self, t: &juxc_ast::TypeRef) -> bool {
        // A function-pointer field is how a C function table is written
        // (`JNIEnv`, a COM vtable): C-compatible when its signature is.
        if t.fn_pointer_shape().is_some() {
            return self.ffi_type_ok(t);
        }
        if t.array_shape.is_some() || t.fn_shape.is_some() || !t.generic_args.is_empty() {
            return false;
        }
        if t.ptr_depth > 0 {
            return true; // a raw pointer field is fine (a `*mut T`)
        }
        if t.nullable {
            return false;
        }
        let name = t
            .name
            .segments
            .last()
            .map(|s| s.text.as_str())
            .unwrap_or("");
        if name == "String" {
            return false;
        }
        if crate::ty::primitive_from_name(name).is_some() {
            return true;
        }
        // A nested `@layout(c)` value struct or a `@layout(c)` C enum (both are
        // C-compatible by-value fields with a portable `#[repr]`).
        match ty_from_ref(t, &self.env, self.symbols) {
            Ty::User { name, .. } => {
                self.symbols
                    .classes
                    .get(&name)
                    .is_some_and(|c| c.is_layout_c)
                    || self.symbols.enums.get(&name).is_some_and(|e| e.is_layout_c)
            }
            _ => false,
        }
    }

    /// True when `t` is allowed at the C FFI boundary (Layout-ABI §L.7): a
    /// primitive, a raw pointer, `String`, OR a `@layout(c)` value struct (a C
    /// aggregate passable by value, §L.1.2). The structural cases are the free
    /// [`ffi_type_ok`]; the value-struct case needs the symbol table.
    fn ffi_type_ok(&self, t: &juxc_ast::TypeRef) -> bool {
        // A function pointer is a C code address (§L.6.4); it crosses the
        // boundary when its own signature does.
        if let Some(shape) = t.fn_pointer_shape() {
            return !t.nullable && self.fn_pointer_signature_offender(shape).is_none();
        }
        if ffi_type_ok(t) {
            return true;
        }
        // A non-pointer, non-generic user type that resolves to a `@layout(c)`
        // value struct or a `@layout(c)` C enum is a C aggregate / C integer
        // enum, both representable at the boundary by value.
        if t.ptr_depth == 0
            && t.array_shape.is_none()
            && t.fn_shape.is_none()
            && t.generic_args.is_empty()
            && !t.nullable
        {
            if let Ty::User { name, .. } = ty_from_ref(t, &self.env, self.symbols) {
                return self
                    .symbols
                    .classes
                    .get(&name)
                    .is_some_and(|c| c.is_layout_c)
                    || self.symbols.enums.get(&name).is_some_and(|e| e.is_layout_c);
            }
        }
        false
    }

    /// Validate an `@export` (§8.4) free function. It becomes a `pub extern "C"
    /// fn` (or, when its signature mentions `String`, a normal fn plus a C-ABI
    /// marshalling wrapper), so its signature must be C-compatible: each parameter
    /// and the return is a primitive, a raw pointer, a `@layout(c)` value struct /
    /// C enum, or `String` (marshalled to/from `const char*`), and the function
    /// may not be generic, `async`, `unsafe`, or `throws`. Violations are **E0508**.
    fn check_export_signature(&mut self, fn_decl: &FnDecl) {
        if !crate::symbol_table::has_annotation(&fn_decl.annotations, "export") {
            return;
        }
        let name = &fn_decl.name.text;
        if !fn_decl.generic_params.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0508_FfiTypeNotAllowed,
                    format!("`@export` function `{name}` may not be generic -- a C entry point has one concrete signature"),
                )
                .with_span(fn_decl.span),
            );
        }
        if fn_decl
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Unsafe))
        {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0508_FfiTypeNotAllowed,
                    format!("`@export` function `{name}` may not be `unsafe`"),
                )
                .with_span(fn_decl.span),
            );
        }
        if matches!(fn_decl.return_type, juxc_ast::ReturnType::AsyncType(_)) {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0508_FfiTypeNotAllowed,
                    format!("`@export` function `{name}` may not be `async`"),
                )
                .with_span(fn_decl.span),
            );
        }
        if !fn_decl.throws.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0508_FfiTypeNotAllowed,
                    format!("`@export` function `{name}` may not declare `throws` -- C has no exceptions"),
                )
                .with_span(fn_decl.span),
            );
        }
        for p in &fn_decl.params {
            if !self.ffi_type_ok(&p.ty) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0508_FfiTypeNotAllowed,
                        format!(
                            "parameter `{}` of `@export` function `{name}` has type `{}`, which is \
                             not C-compatible -- use a primitive, a raw pointer (`T*`), a \
                             `@layout(c)` struct, or `String`",
                            p.name.text,
                            type_ref_display(&p.ty),
                        ),
                    )
                    .with_span(p.span),
                );
            }
        }
        if let juxc_ast::ReturnType::Type(t) = &fn_decl.return_type {
            if !self.ffi_type_ok(t) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0508_FfiTypeNotAllowed,
                        format!(
                            "return type `{}` of `@export` function `{name}` is not C-compatible -- \
                             use a primitive, a raw pointer (`T*`), a `@layout(c)` struct, \
                             `String`, or `void`",
                            type_ref_display(t),
                        ),
                    )
                    .with_span(fn_decl.span),
                );
            }
        }
    }

    /// Walk an enum's operator bodies. Same scope shape as records:
    /// `this` is the enum's type, operator params are declared into
    /// the body's scope. Deleted operators have no body and are
    /// skipped inside `check_operator`.
    fn check_enum(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        // `sealed enum X permits A, B` (ERRATA E33): an enum is sealed by
        // default, so the list may only restate its variants, all of them.
        if !enum_decl.permits.is_empty() {
            let variants: Vec<&str> = enum_decl.variants.iter().map(|v| v.name.text.as_str()).collect();
            let listed: Vec<&str> = enum_decl.permits.iter().map(|p| p.text.as_str()).collect();
            let strangers: Vec<&str> = listed.iter().copied().filter(|p| !variants.contains(p)).collect();
            let missing: Vec<&str> = variants.iter().copied().filter(|v| !listed.contains(v)).collect();
            if !strangers.is_empty() || !missing.is_empty() {
                let mut why = Vec::new();
                if !strangers.is_empty() {
                    why.push(format!("`{}` is not a variant", strangers.join("`, `")));
                }
                if !missing.is_empty() {
                    why.push(format!("variant `{}` is left out", missing.join("`, `")));
                }
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0490_EnumPermitsMismatch,
                        format!(
                            "the `permits` list of enum `{}` must name exactly its variants: {}. An enum is \
                             sealed already, so the list can be dropped",
                            enum_decl.name.text,
                            why.join("; "),
                        ),
                    )
                    .with_span(enum_decl.permits[0].span.join(enum_decl.permits[enum_decl.permits.len() - 1].span)),
                );
            }
        }
        // `@layout(c [, repr = "i32"])` makes a C-compatible enum: a plain
        // integer with one discriminant per variant (Layout-ABI §L.1.3). Such
        // an enum may NOT carry a payload — a C enum is just an integer, it has
        // no associated data — so a variant with a payload is rejected (E0509).
        let is_c_enum = crate::symbol_table::is_layout_c_annotation(&enum_decl.annotations);
        if is_c_enum {
            // A C enum is a plain integer with one concrete repr, so it cannot
            // be generic (`@layout(c) enum E<T>`): type parameters have no place
            // in an integer enum.
            if !enum_decl.generic_params.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0509_LayoutCOnNonAggregate,
                        format!(
                            "`@layout(c) enum {}` may not be generic -- a C enum is a plain integer \
                             with one concrete representation; remove the type parameters",
                            enum_decl.name.text,
                        ),
                    )
                    .with_span(enum_decl.span),
                );
            }
            // A C enum's discriminants become Rust enum discriminants, which
            // must be compile-time constants. The backend const-folds them; a
            // discriminant that does NOT reduce to an integer would be silently
            // dropped (wrong value), so reject it here.
            let disc_ctx = crate::const_eval::ConstCtx {
                symbols: self.symbols,
                generic_param_names: &std::collections::HashSet::new(),
                enclosing_class: None,
            };
            for variant in &enum_decl.variants {
                if !variant.payload.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0509_LayoutCOnNonAggregate,
                            format!(
                                "variant `{}` of `@layout(c) enum {}` carries a payload, which is \
                                 not C-compatible -- a C enum is a plain integer with no associated \
                                 data; drop the payload or remove `@layout(c)`",
                                variant.name.text, enum_decl.name.text,
                            ),
                        )
                        .with_span(variant.span),
                    );
                }
                if let Some(disc) = &variant.discriminant {
                    if crate::const_eval::eval_const_int(disc, &disc_ctx).is_err() {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0509_LayoutCOnNonAggregate,
                                format!(
                                    "discriminant of variant `{}` in `@layout(c) enum {}` must be a \
                                     constant integer -- a C enum's values are fixed at compile time",
                                    variant.name.text,
                                    enum_decl.name.text,
                                ),
                            )
                            .with_span(variant.span),
                        );
                    }
                }
            }
        } else {
            // An explicit discriminant (`Variant = <const>`) is only meaningful
            // for a C-compatible integer enum (§L.1.3). On a regular sum-type
            // enum the value has no representation and would be silently
            // dropped, so reject it and point at `@layout(c)` (E0510).
            for variant in &enum_decl.variants {
                if variant.discriminant.is_some() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0510_DiscriminantOutsideCEnum,
                            format!(
                                "variant `{}` of enum `{}` has an explicit discriminant `= …`, but \
                                 `{}` is not a C enum -- add `@layout(c, repr = \"i32\")` to make it \
                                 a C-compatible integer enum, or remove the `= …`. A value that belongs to \
                                 each variant is a field: give the enum a constructor and write `{}(…)`",
                                variant.name.text,
                                enum_decl.name.text,
                                enum_decl.name.text,
                                variant.name.text,
                            ),
                        )
                        .with_span(variant.span),
                    );
                }
            }
        }
        let name = crate::symbol_table::make_fqn(&self.env.current_package, &enum_decl.name.text);
        self.check_java_style_enum(enum_decl, &name);
        self.env.set_class(&name);
        let ctor_this = Ty::User { name: name.clone(), generic_args: Vec::new() };
        for (idx, ctor) in enum_decl.constructors.iter().enumerate() {
            self.check_constructor(ctor, &ctor_this, idx);
        }
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
                self.env.declare_pointer(&param.name.text, param.ty.ptr_depth);
                self.note_fixed_array_decl(&param.name.text, &param.ty, true);
                if crate::infer::type_ref_is_void_pointer(&param.ty) {
                    self.env.declare_void_base(&param.name.text);
                }
            }
            let saved = self.current_return.take();
            self.current_return_ptr = return_ptr_depth(&method.return_type);
        self.current_return_void = return_is_void_pointer(&method.return_type);
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
        let mut hits = self.symbols.classes.keys().filter(|k| k.ends_with(&suffix));
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
            match self
                .symbols
                .classes
                .get(&cur)
                .and_then(|c| c.extends_fqn.clone())
            {
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
                            "`{fn_name}` may throw the checked exception `{bare}` -- catch it, or declare `throws {bare}` on the signature",
                        ),
                    )
                    .with_span(span)
                    .with_help("checked = extends Exception without passing through RuntimeException (§X.1.3)"),
                );
            }
        }
    }

    /// `a + b` (or `a += b`) where `a` is a value of a user type that declares
    /// no such operator (§O.2.6: dispatch starts from the left operand), and no
    /// free-function operator takes the operands either: `E0484`. Without this
    /// the program reached rustc, which named a Rust trait (`Add`) the program
    /// never mentions.
    ///
    /// Only the arithmetic and bitwise family is checked here; equality has an
    /// identity default and ordering is derived from `<=>`. Foreign types from
    /// a crate stub are left alone: their operators come from Rust.
    fn check_user_operator_defined(&mut self, op: BinaryOp, left: &Expr, right: &Expr, span: Span) {
        use OperatorKind as K;
        let Some(kind) = op_kind_for_binary(op) else { return };
        if !matches!(
            kind,
            K::Plus | K::Minus | K::Mul | K::Div | K::Rem | K::BitAnd | K::BitOr | K::BitXor | K::Shl | K::Shr
        ) {
            return;
        }
        let left_ty = infer_expr(left, &self.env, self.symbols);
        let right_ty = infer_expr(right, &self.env, self.symbols);
        let user_declared = |this: &Self, t: &Ty| match t {
            Ty::User { name, .. } => {
                this.symbols.classes.get(name).is_some_and(|c| !c.is_external)
                    || this.symbols.records.contains_key(name)
                    || this.symbols.enums.get(name).is_some_and(|e| !e.is_external)
            }
            _ => false,
        };
        // `+` with a `String` on either side is concatenation, which takes
        // any value (§S.3).
        if kind == K::Plus && (matches!(left_ty, Ty::String) || matches!(right_ty, Ty::String)) {
            return;
        }
        let symbol = operator_kind_user_spelling(kind);
        let span = [expr_span(left), span].into_iter().find(|sp| *sp != Span::DUMMY).unwrap_or(span);
        if crate::infer::free_operator_for(self.symbols, kind, &left_ty, &right_ty, &self.env).is_some() {
            return;
        }
        if user_declared(self, &left_ty) {
            if self.ty_satisfies_operator(&left_ty, kind) {
                return;
            }
            let Ty::User { name, .. } = &left_ty else { return };
            let bare = name.rsplit('.').next().unwrap_or(name);
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0484_OperatorNotDefined,
                    format!("`{bare}` has no `operator{symbol}`, so `{symbol}` has nothing to call (§O.2.6)"),
                )
                .with_span(span)
                .with_help(format!("declare it on `{bare}`: `public {bare} operator{symbol}({right_ty} other) {{ ... }}`")),
            );
        } else if matches!(left_ty, Ty::Primitive(_)) && user_declared(self, &right_ty) {
            // A primitive on the left has no member operators, so only a
            // free-function operator can take a user type on the right (§7.14).
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0484_OperatorNotDefined,
                    format!("no `operator{symbol}` takes `{left_ty}` and `{right_ty}`, so `{symbol}` has nothing to call (§7.14)"),
                )
                .with_span(span)
                .with_help(format!("declare a free-function operator: `public R operator{symbol}({left_ty} left, {right_ty} right) {{ ... }}`")),
            );
        }
    }

    /// `a..b` / `a..=b` with `a` of a user type (§O.2.4): it calls the type's
    /// `operator..` / `operator..=`, so that operator must exist (`E0484`)
    /// and take `b` (`E0410`). A primitive start keeps the built-in range.
    fn check_user_range_operator(&mut self, r: &juxc_ast::RangeExpr) {
        let start = infer_expr(&r.start, &self.env, self.symbols);
        let Ty::User { name, .. } = &start else { return };
        let declared_here = self.symbols.classes.get(name).is_some_and(|c| !c.is_external)
            || self.symbols.records.contains_key(name)
            || self.symbols.enums.get(name).is_some_and(|e| !e.is_external);
        if !declared_here {
            return;
        }
        let kind = if r.inclusive { OperatorKind::RangeInclusive } else { OperatorKind::Range };
        let symbol = if r.inclusive { "..=" } else { ".." };
        let bare = name.rsplit('.').next().unwrap_or(name).to_string();
        let op = self
            .symbols
            .classes
            .get(name)
            .and_then(|c| c.operators.get(&kind))
            .or_else(|| self.symbols.records.get(name).and_then(|rec| rec.operators.get(&kind)))
            .or_else(|| self.symbols.enums.get(name).and_then(|e| e.operators.get(&kind)))
            .filter(|op| !op.is_deleted)
            .cloned();
        let Some(op) = op else {
            let end = infer_expr(&r.end, &self.env, self.symbols);
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0484_OperatorNotDefined,
                    format!("`{bare}` has no `operator{symbol}`, so `{symbol}` has nothing to call (§O.2.4)"),
                )
                .with_span(r.span)
                .with_help(format!("declare it on `{bare}`: `public Range<{bare}> operator{symbol}({end} end) {{ ... }}`")),
            );
            return;
        };
        if let Some(param) = op.params.first() {
            let want = crate::ty::lower_member_type(&param.ty, name, self.symbols);
            let got = infer_expr(&r.end, &self.env, self.symbols);
            if !matches!(got, Ty::Unknown) && !compatible(&want, &got, self.symbols) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0410_TypeMismatch,
                        format!("`operator{symbol}` on `{bare}` takes {want}, found {got}"),
                    )
                    // A literal can carry no span of its own; the range's does.
                    .with_span(Some(expr_span(&r.end)).filter(|sp| *sp != Span::DUMMY).unwrap_or(r.span)),
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
                // Records AUTO-DERIVE `==`, `hash`, and `string`
                // (§O.3 — the data-class model; `= delete` re-disables);
                // other operator kinds need an explicit declaration.
                let record = self.symbols.records.get(name).or_else(|| {
                    if name.contains('.') {
                        return None;
                    }
                    let suffix = format!(".{name}");
                    let mut hits = self
                        .symbols
                        .records
                        .iter()
                        .filter(|(k, _)| k.ends_with(&suffix));
                    match (hits.next(), hits.next()) {
                        (Some((_, r)), None) => Some(r),
                        _ => None,
                    }
                });
                let record_ok = record
                    .map(|r| {
                        let declared = r.operators.get(&kind).is_some_and(|o| !o.is_deleted);
                        let deleted = r.operators.get(&kind).is_some_and(|o| o.is_deleted);
                        let auto = matches!(kind, K::Eq | K::Hash | K::ToString);
                        declared || (auto && !deleted)
                    })
                    .unwrap_or(false);
                // Enums likewise derive equality/hash, and their
                // Display prints the variant (`string`).
                let enum_ok = (self.symbols.enums.contains_key(name)
                    || (!name.contains('.')
                        && self
                            .symbols
                            .enums
                            .keys()
                            .filter(|k| k.ends_with(&format!(".{name}")))
                            .count()
                            == 1))
                    && matches!(kind, K::Eq | K::Hash | K::ToString);
                class_ok || record_ok || enum_ok
            }
            _ => false,
        }
    }

    /// Enforce `extends` bounds on generic parameters (§T.4, E0446)
    /// against a resolved instantiation — shared by `new` sites
    /// (class params) and method calls (method params). Bounds lower
    /// in the OWNER's scope. Lenient slots: `Unknown` args
    /// (inference gaps), `Param` args (checked at THEIR
    /// instantiation site), const-generic params, and bounds that
    /// don't lower.
    fn enforce_generic_bounds(
        &mut self,
        generic_params: &[TypeParam],
        args: &[Ty],
        owner: &str,
        callee_desc: &str,
        call_span: Span,
    ) {
        for (idx, param) in generic_params.iter().enumerate() {
            if param.is_const() || param.bounds.is_empty() {
                continue;
            }
            let Some(arg) = args.get(idx) else { continue };
            let arg = match arg {
                Ty::Nullable(inner) => inner.as_ref(),
                other => other,
            };
            if matches!(arg, Ty::Unknown | Ty::Param(_)) {
                continue;
            }
            for bound in &param.bounds {
                let bound_ty = crate::ty::lower_member_type(bound, owner, self.symbols);
                if matches!(bound_ty, Ty::Unknown) {
                    continue;
                }
                if !compatible(&bound_ty, arg, self.symbols) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0446_GenericBoundNotSatisfied,
                            format!(
                                "type `{arg}` does not satisfy the bound `{} extends {bound_ty}` declared by {callee_desc}",
                                param.name.text,
                            ),
                        )
                        .with_span(call_span),
                    );
                }
            }
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
            let Some(bound) = subst_args.get(idx) else {
                continue;
            };
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
                    "`?` inside a `try` block isn't supported yet (Phase 1) -- its early return would bypass the try machinery; restructure with a plain match or move the `?` call out of the try",
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
                                    "`?` propagates error type {err}, but the function returns a Result with error type {ret_err} -- convert explicitly before propagating",
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
                                "`?` on a Result needs the enclosing function to return a Result -- it returns {ret}",
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
                                "`?` on a nullable needs the enclosing function to return a nullable -- it returns {ret}",
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
                        format!("`?` needs a Result or nullable operand, found {operand}",),
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
        // §M.14.4 (E0467): a default fills a TRAILING omitted argument, so a
        // plain (non-defaulted, non-varargs) parameter may not follow a
        // defaulted one — its position could never be omitted. A trailing
        // varargs after a default is exempt (it's handled by the E0212
        // last-param rule and can be empty).
        let mut seen_default = false;
        for param in params {
            if param.default.is_some() {
                seen_default = true;
            } else if seen_default && !param.is_varargs {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0467_DefaultParamOrdering,
                        format!(
                            "parameter `{}` has no default value but follows a defaulted \
                             parameter -- move all defaulted parameters to the end of the \
                             list (§M.14.4)",
                            param.name.text,
                        ),
                    )
                    .with_span(param.span),
                );
            }
        }
        for param in params {
            let Some(default) = &param.default else {
                continue;
            };
            // The default flows into the parameter's slot like an argument.
            let slot = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.check_literal_fits(&slot, default, param.name.span);
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
                                "default value of `{}` references parameter `{}` -- \
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

    /// E0477: `@Test` and the test hooks mark a free `void` function with no
    /// parameters (Testing §TS.1); the runner has nothing to pass and nowhere
    /// to put a result, and a method has no receiver to call it on.
    fn check_test_annotation(&mut self, fn_decl: &FnDecl, is_method: bool) {
        const TEST_ANNOTATIONS: [&str; 5] = ["test", "beforeeach", "aftereach", "beforeall", "afterall"];
        let Some(annotation) = fn_decl.annotations.iter().find(|a| {
            a.name
                .segments
                .last()
                .is_some_and(|s| TEST_ANNOTATIONS.iter().any(|t| s.text.eq_ignore_ascii_case(t)))
        }) else {
            return;
        };
        let name = annotation.name.segments.last().map(|s| s.text.clone()).unwrap_or_default();
        let returns_value = match &fn_decl.return_type {
            juxc_ast::ReturnType::Void => false,
            juxc_ast::ReturnType::AsyncType(t) => !(t.name.segments.len() == 1 && t.name.segments[0].text == "void"),
            juxc_ast::ReturnType::Type(_) => true,
        };
        let problem = if is_method {
            "it is a method; tests and hooks are free functions"
        } else if !fn_decl.params.is_empty() {
            "it takes parameters, and the test runner has nothing to pass"
        } else if returns_value {
            "it returns a value, and the test runner has nowhere to put it"
        } else {
            return;
        };
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0477_TestAnnotationMisplaced,
                format!("`@{name}` cannot mark `{}`: {problem} (§TS.1)", fn_decl.name.text),
            )
            .with_span(fn_decl.name.span)
            .with_help("declare it as `void name()` at the top level of the file"),
        );
    }

    /// E0453: `pick(3, "hi")` against `<T> T pick(T a, T b)`. Two arguments
    /// for parameters declared exactly `T` have no type in common, so no `T`
    /// satisfies both (§T.4.2). Only the certain case is reported: a number
    /// against text, a `bool` against either, or a value type against a class.
    /// Two classes may still share a supertype, which inference resolves.
    fn report_generic_conflict(
        &mut self,
        name: &str,
        generic_params: &[TypeParam],
        param_tys: &[&TypeRef],
        arg_tys: &[Ty],
        c: &CallExpr,
    ) {
        #[derive(PartialEq)]
        enum Kind {
            Number,
            Bool,
            Char,
            Text,
            Object,
        }
        let kind = |t: &Ty| match t {
            Ty::Primitive(Primitive::Bool) => Some(Kind::Bool),
            Ty::Primitive(Primitive::Char) => Some(Kind::Char),
            Ty::Primitive(_) => Some(Kind::Number),
            Ty::String => Some(Kind::Text),
            Ty::User { .. } => Some(Kind::Object),
            _ => None,
        };
        for tp in generic_params {
            let mut first: Option<(&Ty, usize)> = None;
            for (i, (declared, arg)) in param_tys.iter().zip(arg_tys.iter()).enumerate() {
                let bare_t = declared.name.segments.len() == 1
                    && declared.name.segments[0].text == tp.name.text
                    && declared.generic_args.is_empty()
                    && declared.array_shape.is_none()
                    && !declared.nullable;
                let Some(k) = (if bare_t { kind(arg) } else { None }) else { continue };
                match first {
                    None => first = Some((arg, i)),
                    Some((prev, _)) if kind(prev) != Some(k) => {
                        let span = expr_span(&c.args[i]);
                        let span = if span.start == span.end { c.span } else { span };
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0453_GenericInferenceNoSolution,
                                format!(
                                    "no `{}` fits this call to `{name}`: one argument is {prev} and another is {arg}, and no type is both (§T.4.2)",
                                    tp.name.text,
                                ),
                            )
                            .with_span(span)
                            .with_help(format!(
                                "pass values of one type, or name the type: `{name}<{}>(...)`",
                                tp.name.text,
                            )),
                        );
                        return;
                    }
                    _ => {}
                }
            }
        }
    }

    /// E0475: an overloaded call that several members accept with none more
    /// specific (§T.3.3), such as `f(1, 2)` against `f(long, int)` and
    /// `f(int, long)`. Resolving it silently to the first declared member hid a
    /// question only the author can answer.
    fn report_ambiguous_overload(&mut self, name: &str, lists: &[&[ParamSig]], c: &CallExpr) {
        let tied = crate::infer::ambiguous_overloads(lists, c, &self.env, self.symbols);
        if tied.len() < 2 {
            return;
        }
        let shapes: Vec<String> = tied
            .iter()
            .map(|&k| {
                let params: Vec<String> = lists[k]
                    .iter()
                    .map(|p| ty_from_ref(&p.ty, &self.env, self.symbols).to_string())
                    .collect();
                format!("`{name}({})`", params.join(", "))
            })
            .collect();
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0475_AmbiguousOverload,
                format!(
                    "call to `{name}` is ambiguous: {} {} accept these arguments, and none is more specific (§T.3.3)",
                    shapes.join(" and "),
                    if shapes.len() == 2 { "both" } else { "all" },
                ),
            )
            .with_span(c.span)
            .with_help("convert an argument with `as` so exactly one of them fits best"),
        );
    }

    fn check_function(&mut self, fn_decl: &FnDecl) {
        self.check_export_signature(fn_decl);
        self.check_test_annotation(fn_decl, false);
        let Some(body) = &fn_decl.body else { return };
        self.check_param_defaults(&fn_decl.params);
        self.env.push_scope();
        // Register the function's own `<T>` so declared types naming it lower
        // to `Ty::Param("T")`, the way a class's and a method's already do.
        // Without it `Vec<T>` typed its element as a USER type called "T",
        // which resolves to no class -- so the backend's "an element of a
        // non-Copy type clones out of the container" rule never fired and
        // `xs[0]` moved out of a borrow (rustc E0507).
        let saved_generics = std::mem::take(&mut self.env.generic_params);
        for tp in &fn_decl.generic_params {
            self.env.add_generic_param_bounded(&tp.name.text, &tp.bounds);
        }
        // Declare each parameter into the new scope so name lookups
        // inside the body resolve.
        self.env.weak_names.clear();
        for param in &fn_decl.params {
            self.check_iface_value_type(&param.ty);
            self.check_fixed_array_size_in_type(&param.ty);
            self.check_fn_pointer_signatures(&param.ty);
            // J4: a free function's own `<T>` params aren't pushed into the
            // env, so pass them explicitly as the in-scope generics.
            self.validate_sig_type(&param.ty, &fn_decl.generic_params);
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
            self.env.declare_pointer(&param.name.text, param.ty.ptr_depth);
            self.note_fixed_array_decl(&param.name.text, &param.ty, true);
            if crate::infer::type_ref_is_void_pointer(&param.ty) {
                self.env.declare_void_base(&param.name.text);
            }
            // `weak` parameter (§M.14.3): validate its class type (E0455) and
            // register it so reads route through `.get()` (E0456).
            if param.is_weak {
                self.check_weak_param(param);
                self.env.weak_names.insert(param.name.text.clone());
            }
        }
        self.validate_sig_return(&fn_decl.return_type, &fn_decl.generic_params);
        // Const-generic params (`int cap<int N>()`) read as values in
        // the body — declare them with their value type.
        self.declare_const_generic_params(&fn_decl.generic_params);
        self.check_iface_return_type(&fn_decl.return_type);
        let saved = self.current_return.take();
        self.current_return_ptr = return_ptr_depth(&fn_decl.return_type);
        self.current_return_void = return_is_void_pointer(&fn_decl.return_type);
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
        self.check_out_params_assigned(&fn_decl.params, body, &fn_decl.name.text);
        self.check_locals_definitely_assigned(body);
        self.check_final_not_reassigned(&fn_decl.params, body);
        self.check_missing_return(
            &fn_decl.return_type,
            body,
            &fn_decl.name.text,
            fn_decl.name.span,
        );
        // §X.1.3: every checked exception the body can raise must be
        // covered by the declared `throws` clause.
        self.enforce_declared_throws(&fn_decl.throws, &fn_decl.name.text);
        self.flush_uninferable_news();
        self.in_unsafe = saved_unsafe;
        self.in_async = saved_async;
        self.current_return = saved;
        self.env.generic_params = saved_generics;
        self.env.weak_names.clear();
        self.env.pop_scope();
    }

    /// True when `e` is an assignable place that can back an `out` argument: a
    /// variable, a field access, or an array element (§M.4.2). Everything else
    /// (a literal, a call result, an arithmetic expression) is rejected (E0942).
    fn is_assignable_place(e: &Expr) -> bool {
        matches!(e, Expr::Path(_) | Expr::Field(_) | Expr::Index(_))
    }

    /// **E0940 (§M.4.2)** — every `out` parameter must be assigned on every path
    /// that returns from / completes the body. Reuses the field
    /// definite-assignment flow engine.
    /// **E0601 (§S.4.6)** -- a local read before it is definitely assigned.
    ///
    /// The local counterpart of the field rule `E0600` enforces, and the same
    /// flow analysis. Before this existed `int x; print(x);` compiled and
    /// printed `0`: the program read a value nobody wrote, with no diagnostic
    /// anywhere.
    fn check_locals_definitely_assigned(&mut self, body: &juxc_ast::Block) {
        for read in crate::definite_assign::locals_read_before_assignment(body) {
            let name = read.name;
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0601_LocalNotDefinitelyAssigned,
                    format!(
                        "`{name}` is read before it is assigned -- give it a value on every path that reaches here, or declare it with an initializer (§S.4.6)",
                    ),
                )
                .with_span(read.span),
            );
        }
    }

    fn check_out_params_assigned(
        &mut self,
        params: &[juxc_ast::Param],
        body: &juxc_ast::Block,
        fn_name: &str,
    ) {
        let required: std::collections::HashSet<String> = params
            .iter()
            .filter(|p| p.is_out)
            .map(|p| p.name.text.clone())
            .collect();
        if required.is_empty() {
            return;
        }
        for name in crate::definite_assign::unassigned_on_some_exit(body, &required) {
            let span = params
                .iter()
                .find(|p| p.name.text == name)
                .map(|p| p.span)
                .unwrap_or(Span::DUMMY);
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0940_OutParamNotDefinitelyAssigned,
                    format!(
                        "`out` parameter `{name}` is not assigned on every path through `{fn_name}` \
                         -- assign it before each `return` and before the body ends (§M.4)",
                    ),
                )
                .with_span(span),
            );
        }
    }

    /// **E0464 (§M.14.2)** — a `final`/`const` binding must not be reassigned.
    /// Seeds the in-scope final set with the function's `final` parameters and
    /// walks the body; a `final` local (`VarDecl.is_final`) adds to the set, a
    /// non-final local re-using a name removes it (Jux permits shadowing — env.rs
    /// notes shadowing diagnostics are deferred — so an inner `var x` legitimately
    /// un-finals an outer `final x`). Both plain (`x = …`) and compound
    /// (`x += …`, and the parser's `++`/`--` desugaring) assignments are caught.
    fn check_final_not_reassigned(&mut self, params: &[juxc_ast::Param], body: &juxc_ast::Block) {
        // `ref`/`weak` bindings are excluded: on those, `x = v` is a
        // store-through / handle operation (§M.13.2), not a binding
        // reassignment, so `final ref` / `final weak` never trip E0464.
        let finals: std::collections::HashSet<String> = params
            .iter()
            .filter(|p| p.is_final && !p.is_shared_ref && !p.is_weak)
            .map(|p| p.name.text.clone())
            .collect();
        // A body with no `final` params can still declare `final` locals, so we
        // always walk — the walker accumulates locals as it descends.
        self.walk_block_final_reassign(body, &finals);
    }

    /// Walk one block for `final`-reassignment (E0464). `incoming` is the set of
    /// names that are final on entry; locals declared in this block extend/shadow
    /// a *clone* of it so the effect is correctly scoped to the block and its
    /// descendants.
    fn walk_block_final_reassign(
        &mut self,
        block: &juxc_ast::Block,
        incoming: &std::collections::HashSet<String>,
    ) {
        let mut finals = incoming.clone();
        for stmt in &block.statements {
            self.walk_stmt_final_reassign(stmt, &mut finals);
        }
    }

    /// Walk one statement for `final`-reassignment (E0464), updating `finals`
    /// with any binding the statement introduces in the CURRENT block scope and
    /// recursing into nested blocks with a scoped clone.
    fn walk_stmt_final_reassign(
        &mut self,
        stmt: &Stmt,
        finals: &mut std::collections::HashSet<String>,
    ) {
        match stmt {
            // A local declaration (re)binds its name in this scope: a `final`
            // (non-`ref`) local makes it final; a plain `var` — or a `final ref`
            // local, whose `=` stores through (§M.13.2) — un-finals the name.
            Stmt::VarDecl(v) => {
                if v.is_final && !v.is_ref {
                    finals.insert(v.name.text.clone());
                } else {
                    finals.remove(&v.name.text);
                }
            }
            // The violation: assigning (plain or compound) to a bare name that is
            // currently a final binding.
            Stmt::Assign(a) => {
                if let Expr::Path(qn) = &a.target {
                    if qn.segments.len() == 1 && finals.contains(&qn.segments[0].text) {
                        let name = &qn.segments[0].text;
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0464_FinalBindingReassigned,
                                format!(
                                    "cannot reassign `{name}`: it is a `final` binding and is \
                                     immutable (§M.14.2). Drop `final`, or bind a new local.",
                                ),
                            )
                            .with_span(a.span),
                        );
                    }
                }
            }
            Stmt::If(i) => {
                self.walk_block_final_reassign(&i.then_block, finals);
                let mut branch = i.else_branch.as_deref();
                while let Some(b) = branch {
                    match b {
                        ElseBranch::If(inner) => {
                            self.walk_block_final_reassign(&inner.then_block, finals);
                            branch = inner.else_branch.as_deref();
                        }
                        ElseBranch::Block(blk) => {
                            self.walk_block_final_reassign(blk, finals);
                            branch = None;
                        }
                    }
                }
            }
            Stmt::While(w) => self.walk_block_final_reassign(&w.body, finals),
            Stmt::DoWhile(d) => self.walk_block_final_reassign(&d.body, finals),
            Stmt::ForEach(fe) => {
                // The loop variable is a fresh, non-final binding scoped to the
                // body; it shadows any same-named outer final.
                let mut inner = finals.clone();
                inner.remove(&fe.var_name.text);
                self.walk_block_final_reassign(&fe.body, &inner);
            }
            Stmt::ForC(fc) => {
                // The init clause's bindings scope over cond/update/body.
                let mut inner = finals.clone();
                if let Some(init) = &fc.init {
                    self.walk_stmt_final_reassign(init, &mut inner);
                }
                if let Some(update) = &fc.update {
                    self.walk_stmt_final_reassign(update, &mut inner);
                }
                self.walk_block_final_reassign(&fc.body, &inner);
            }
            Stmt::Try(t) => {
                self.walk_block_final_reassign(&t.body, finals);
                for c in &t.catches {
                    // The catch binder shadows in the handler body.
                    let mut inner = finals.clone();
                    inner.remove(&c.name.text);
                    self.walk_block_final_reassign(&c.body, &inner);
                }
                if let Some(fin) = &t.finally {
                    self.walk_block_final_reassign(fin, finals);
                }
            }
            Stmt::Block(b) | Stmt::Unsafe(b) => self.walk_block_final_reassign(b, finals),
            Stmt::Labeled { stmt, .. } => self.walk_stmt_final_reassign(stmt, finals),
            // A statement-position `switch`: walk its block arms (an arm's own
            // pattern binders shadow within the arm, but that is rare enough that
            // the conservative whole-set view suffices).
            Stmt::Expr(Expr::Switch(sw)) => {
                for arm in &sw.arms {
                    if let SwitchBody::Block(b) = &arm.body {
                        self.walk_block_final_reassign(b, finals);
                    }
                }
            }
            _ => {}
        }
    }

    /// **E0451** — a non-`void` function/method must `return` (or `throw`) on
    /// every path; report when control can fall off the end. `void` functions
    /// (and bodiless abstract/interface methods, handled by the caller) are
    /// exempt. Async functions return a value to awaiters, so they're checked
    /// too. Conservative — see [`crate::return_check`].
    fn check_missing_return(
        &mut self,
        return_type: &juxc_ast::ReturnType,
        body: &juxc_ast::Block,
        fn_name: &str,
        name_span: Span,
    ) {
        // Only value-returning functions have a return obligation. `void` and
        // `async void` are exempt — the latter parses as `AsyncType` over a
        // synthesized `void` sentinel TypeRef (see juxc-parse `parse_return_type`).
        let is_void = match return_type {
            juxc_ast::ReturnType::Void => true,
            juxc_ast::ReturnType::AsyncType(t) => {
                t.name.segments.last().map(|s| s.text.as_str()) == Some("void")
            }
            juxc_ast::ReturnType::Type(_) => false,
        };
        if is_void {
            return;
        }
        if crate::return_check::body_can_fall_through(body) {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0460_MissingReturn,
                    format!(
                        "`{fn_name}` can finish without returning a value -- every path must \
                         `return` (or `throw`); add a return for the missing path",
                    ),
                )
                .with_span(name_span),
            );
        }
    }

    /// Walk a class declaration — for each constructor and each method,
    /// set up the class context (current_class + generic params + `this`
    /// binding), run the body checker, then tear it down. Abstract
    /// methods (body = None) are skipped.
    fn check_class(&mut self, class: &ClassDecl) {
        // `@layout(c)` is permitted only on a value aggregate (`struct`), not a
        // `class` (Layout-ABI §L.1.2) — a class has an `Rc`/vtable header with no
        // portable C representation. `@layout(c) struct` field types must also be
        // C-compatible (so the emitted `#[repr(C)]` `Copy` struct is valid).
        if crate::symbol_table::is_layout_c_annotation(&class.annotations) {
            if !class.is_struct {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0509_LayoutCOnNonAggregate,
                        format!(
                            "`@layout(c)` is not allowed on class `{}` -- only a `struct` can have \
                             a C-compatible layout (a class is a reference type with no portable C \
                             representation)",
                            class.name.text,
                        ),
                    )
                    .with_span(class.span),
                );
            } else {
                // A C-compatible struct has ONE concrete memory layout, so it
                // cannot be generic (`@layout(c) struct Box<T>`): a type
                // parameter has no fixed size/repr at the C boundary.
                if !class.generic_params.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0509_LayoutCOnNonAggregate,
                            format!(
                                "`@layout(c) struct {}` may not be generic -- a C-compatible type \
                                 has one concrete layout; remove the type parameters",
                                class.name.text,
                            ),
                        )
                        .with_span(class.span),
                    );
                }
                for field in &class.fields {
                    if field.is_static {
                        continue;
                    }
                    if let Some(fty) = &field.ty {
                        if !self.ffi_struct_field_ok(fty) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0509_LayoutCOnNonAggregate,
                                    format!(
                                        "field `{}` of `@layout(c) struct {}` has type `{}`, which \
                                         is not C-compatible -- use a primitive, a raw pointer \
                                         (`T*`), or another `@layout(c)` struct",
                                        field.name.text,
                                        class.name.text,
                                        type_ref_display(fty),
                                    ),
                                )
                                .with_span(field.span),
                            );
                        }
                    }
                }
            }
        }
        // Class context — FQN'd so the visibility / subtype walks
        // that key on `env.current_class` find the right entry in
        // the symbol table.
        let class_name = crate::symbol_table::make_fqn(&self.env.current_package, &class.name.text);
        self.env.set_class(&class_name);
        // Register every generic param so `T` in declared types lowers
        // to `Ty::Param("T")` rather than `Unknown`.
        for tp in &class.generic_params {
            self.env.add_generic_param_bounded(&tp.name.text, &tp.bounds);
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
            // A `static final` number, bool or String is a Rust `const`, so a
            // call in its initializer has to fold at compile time (§T.11.1).
            if field.is_static && field.is_final {
                if let (Some(fty), Some(init)) = (&field.ty, &field.default) {
                    let slot = ty_from_ref(fty, &self.env, self.symbols);
                    if matches!(slot, Ty::Primitive(_) | Ty::String) {
                        self.check_const_initializer_folds(&field.name.text, &slot, init, field.span);
                    }
                }
            }
            if let Some(fty) = &field.ty {
                self.check_iface_value_type(fty);
                self.check_wildcard_storage_type(fty);
                self.check_fixed_array_size_in_type(fty);
                self.check_fn_pointer_signatures(fty);
                // J4: reject an unresolved field type name.
                self.validate_sig_type(fty, &[]);
            }
            // `weak` field validity (§6.5). A weak field is exempt from
            // definite-assignment (§S.4.5 — pass not yet implemented) and
            // defaults to an empty `Weak`; it lowers to
            // `Weak<RefCell<Target_Inner>>`, so its target must be a plain
            // (non-generic, Phase-1) class reference.
            if field.is_weak {
                self.check_weak_field(field);
            }
            // E0410: a `null` initializer requires a nullable (`T?`) field. A
            // bare non-nullable type — including a type parameter `K` — cannot
            // hold `null`; without this the backend emits `None` into a
            // non-`Option` field (invalid Rust, the "juxc bug" path). `weak`
            // fields (default null, initializer already barred above) and raw
            // pointers (`null` is a valid pointer value) are exempt.
            // A field initializer flows into the field like an assignment.
            if let (Some(default), Some(fty)) = (&field.default, &field.ty) {
                let slot = ty_from_ref(fty, &self.env, self.symbols);
                self.check_literal_fits(&slot, default, field.span);
            }
            if !field.is_weak {
                if let (Some(default), Some(fty)) = (&field.default, &field.ty) {
                    if matches!(default, Expr::Literal(juxc_ast::Literal::Null))
                        && !fty.nullable
                        && fty.ptr_depth == 0
                    {
                        let tname = fty
                            .name
                            .segments
                            .last()
                            .map(|s| s.text.as_str())
                            .unwrap_or("T");
                        // For an auto-property backing slot (`__prop_P`), name the
                        // user-visible property `P` and point at its declaration,
                        // never the internal slot name.
                        let (disp_name, disp_span) = field
                            .origin_property
                            .as_ref()
                            .map(|p| (p.text.as_str(), p.span))
                            .unwrap_or((field.name.text.as_str(), field.span));
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0410_TypeMismatch,
                                format!(
                                    "cannot assign `null` to the non-nullable field `{disp_name}` of \
                                     type `{tname}` -- declare it nullable (`{tname}?`)",
                                ),
                            )
                            .with_span(disp_span),
                        );
                    }
                }
            }
        }
        // §P.1.1 PascalCase property convention (W0974) — preferred,
        // never enforced: a lowercase-initial property name compiles
        // unchanged with this suppressible warning.
        for prop in &class.properties {
            // §P.1.3 / §M.7.7: a setter may be as visible as its property or
            // less, never more. `private int Hidden { get; public set; }`
            // would hand every caller a write to a value most of them cannot
            // even read.
            if let Some(setter) = &prop.setter {
                if let Some(setter_vis) = setter.visibility {
                    if visibility_rank(setter_vis) > visibility_rank(prop.visibility) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0972_PropertyAccessorVisibility,
                                format!(
                                    "the setter of property `{}` is `{}`, which is more visible than the \
                                     property itself (`{}`) -- a setter may only narrow its property's \
                                     visibility",
                                    prop.name.text,
                                    visibility_word(setter_vis),
                                    visibility_word(prop.visibility),
                                ),
                            )
                            .with_span(setter.span),
                        );
                    }
                }
            }
            let starts_lower = prop
                .name
                .text
                .chars()
                .next()
                .map(|c| c.is_lowercase())
                .unwrap_or(false);
            if starts_lower {
                self.diagnostics.push(
                    Diagnostic::warning(
                        code::Code::W0974_PropertyNamePascalCase,
                        format!(
                            "property `{}` should be PascalCase (`{}`) -- the preferred visual \
                             signal that a member is a property, not a plain field (§P.1.1)",
                            prop.name.text,
                            uppercase_first(&prop.name.text),
                        ),
                    )
                    .with_span(prop.name.span),
                );
            }
        }
        // §P.2.2 observer<T> lambda shapes (E0975): an observer field's
        // initializer lambda must take 0 (invalidation), 2 (old, now),
        // or 3 (prop, old, now) parameters.
        for field in &class.fields {
            let is_observer = field
                .ty
                .as_ref()
                .map(|t| {
                    t.fn_shape.is_none()
                        && t.name.segments.len() == 1
                        && t.name.segments[0].text == "observer"
                })
                .unwrap_or(false);
            // A field whose initializer is a lambda (an observer, or any
            // function-typed field) runs before the object exists, so the
            // lambda cannot reach it yet (E0981, see `lambda_uses_this`).
            if let Some(juxc_ast::Expr::Lambda(l)) = &field.default {
                if !field.is_static {
                    if let Some(span) = lambda_uses_this(l, class) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0981_FieldLambdaUsesThis,
                                format!(
                                    "the lambda that initializes `{}` uses this object, which does not exist yet \
                                     while its fields are being initialized; this is not supported in this phase",
                                    field.name.text,
                                ),
                            )
                            .with_span(span)
                            .with_help(
                                "make the lambda independent of the object, or store what it needs in a separate object it can capture",
                            ),
                        );
                    }
                }
            }
            if is_observer {
                if let Some(juxc_ast::Expr::Lambda(l)) = &field.default {
                    if !matches!(l.params.len(), 0 | 2 | 3) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0975_ObserverShapeMismatch,
                                format!(
                                    "observer `{}` takes {} parameter{} -- an observer lambda is \
                                     `() -> …` (invalidation), `(old, now) -> …`, or \
                                     `(prop, old, now) -> …` (§P.2.2)",
                                    field.name.text,
                                    l.params.len(),
                                    if l.params.len() == 1 { "" } else { "s" },
                                ),
                            )
                            .with_span(field.name.span),
                        );
                    }
                }
            }
        }
        for (idx, ctor) in class.constructors.iter().enumerate() {
            self.check_constructor(ctor, &this_ty, idx);
        }
        // Field definite-assignment (§S.4.5, E0600): every non-nullable,
        // non-`weak`, initializer-less instance field must be assigned on every
        // path through every constructor (and the instance `init` blocks).
        for v in crate::definite_assign::analyze_class(class) {
            // `v.display` is the user-visible name — the property name for an
            // auto-property backing slot, never the internal `__prop_…`.
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0600_FieldNotDefinitelyAssigned,
                    format!(
                        "field `{}` of class `{}` is not definitely assigned -- give it an \
                         initializer (`= …`) or assign it in every constructor before the \
                         constructor returns (§S.4.5)",
                        v.display, class.name.text,
                    ),
                )
                .with_span(v.span),
            );
        }
        for method in &class.methods {
            self.check_test_annotation(method, true);
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
            // Init blocks run during construction / class setup, so a
            // `final`/`const` field MAY be assigned here (E0465 allowance).
            let saved_init = std::mem::replace(&mut self.in_init_block, true);
            self.check_block(block);
            self.in_init_block = saved_init;
            self.in_async = saved_async;
            self.env.pop_scope();
        }
        // A lambda stored by a field initializer
        // (`(double) -> void cb = x -> print("v " + x);`) is checked the way
        // one stored into a local is: its untyped parameters take the slot's
        // types, so `x` is a `double` in the body and prints as `5.0`, and
        // the body's own mistakes are reported. `this` is in scope, as in an
        // `init` block, since the initializer runs during construction.
        for field in &class.fields {
            let (Some(default @ Expr::Lambda(_)), Some(fty)) = (&field.default, &field.ty) else {
                continue;
            };
            let Ty::Fn { params, .. } = ty_from_ref(fty, &self.env, self.symbols) else {
                continue;
            };
            self.env.push_scope();
            if !field.is_static {
                self.env.declare("this", this_ty.clone());
            }
            self.lambda_slot_params = Some(params);
            self.check_expr(default);
            self.lambda_slot_params = None;
            self.env.pop_scope();
        }
        // Destructor block (§6.6 / §S.5). At most one per class; the
        // body runs with `this` in scope, synchronously.
        if class.drop_blocks.len() > 1 {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0400_DuplicateDeclaration,
                    format!(
                        "class `{}` declares {} `drop` blocks -- a class may have at most one destructor",
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

    /// The name of the enclosing class's parent when that parent cannot be
    /// constructed with no arguments, and `None` otherwise -- no parent, or a
    /// parent that has a usable `super()`.
    ///
    /// A parent that declares NO constructor has the implicit no-argument one,
    /// and a declared constructor counts when its acceptable-argument range
    /// starts at zero, which is what makes an all-defaults constructor
    /// (`Base(int n = 1)`) satisfy the rule too.
    fn enclosing_parent_needing_args(&self) -> Option<String> {
        let class_fqn = self.env.current_class.as_deref()?;
        let class = self.symbols.classes.get(class_fqn)?;
        // A struct's `extends` is already E0423; it has no parent to call.
        if class.is_struct {
            return None;
        }
        let parent_key = class.extends_fqn.clone().or_else(|| {
            class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
        })?;
        let parent = self
            .symbols
            .classes
            .get(&parent_key)
            .or_else(|| self.symbols.find_fqn_by_bare(&parent_key)
                .and_then(|f| self.symbols.classes.get(&f)))?;
        if parent.constructors.is_empty() {
            return None;
        }
        let has_nullary = parent.constructors.iter().any(|c| {
            crate::symbol_table::ctor_arity_range(&c.params).0 == 0
        });
        if has_nullary {
            None
        } else {
            Some(parent_key.rsplit('.').next().unwrap_or(&parent_key).to_string())
        }
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
            // `super(...)` must come first too: the parent is built before
            // anything in the child's body runs (Grammar §A.2.4).
            if let Stmt::SuperCall(_, sspan) = stmt {
                if i != 0 {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0210_ConstructorCallNotFirst,
                            "`super(...)` must be the first statement of the constructor: the parent is built before the rest of the body runs",
                        )
                        .with_span(*sspan),
                    );
                }
            }
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
                            "a constructor that delegates with `this(...)` can't also call `super(...)` -- the delegated-to constructor owns parent initialization",
                        )
                        .with_span(*sspan),
                    );
                }
            }
        }
        // **E0211 -- the implicit `super()` has to exist.** A constructor
        // that neither delegates with `this(...)` nor calls `super(...)`
        // begins with an implicit `super()`, and that is only available when
        // the parent can be built with no arguments. Without this check the
        // program reached rustc, which reported a missing argument to a
        // generated function the source never mentions.
        let delegates = ctor.body.statements.iter().any(|st| match st {
            Stmt::SuperCall(..) => true,
            Stmt::Expr(Expr::Call(call)) => matches!(call.callee.as_ref(), Expr::This(_)),
            _ => false,
        });
        if !delegates {
            if let Some(parent) = self.enclosing_parent_needing_args() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0211_MissingSuperCall,
                        format!(
                            "this constructor must call `super(...)`: every constructor of `{parent}` takes arguments, so there is no implicit `super()` to run",
                        ),
                    )
                    .with_span(ctor.span),
                );
            }
        }
        let saved_ctor = self.current_ctor.replace(ctor_idx);
        self.env.push_scope();
        self.env.declare("this", this_ty.clone());
        for param in &ctor.params {
            // J4: reject an unresolved parameter type name.
            self.validate_sig_type(&param.ty, &[]);
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
            self.env.declare_pointer(&param.name.text, param.ty.ptr_depth);
            self.note_fixed_array_decl(&param.name.text, &param.ty, true);
            if crate::infer::type_ref_is_void_pointer(&param.ty) {
                self.env.declare_void_base(&param.name.text);
            }
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
        // `@export` (C linkage) is only honored on FREE functions in Phase 1.
        // On a method it was silently ignored (no C symbol emitted), so flag it:
        // an instance method has a receiver C can't express, and static-method
        // export is a deferred spec item (JUX-LANG-V1 §8.4 / Layout-ABI §L.3.2).
        if crate::symbol_table::has_annotation(&method.annotations, "export") {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0508_FfiTypeNotAllowed,
                    format!(
                        "`@export` is not supported on method `{}` in this phase -- `@export` gives \
                         C linkage to a FREE function; move it out of the class (an instance method \
                         has a receiver C cannot express, and static-method export is deferred)",
                        method.name.text,
                    ),
                )
                .with_span(method.span),
            );
        }
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
            self.env.add_generic_param_bounded(&tp.name.text, &tp.bounds);
        }
        self.declare_const_generic_params(&method.generic_params);
        self.env.weak_names.clear();
        for param in &method.params {
            self.check_iface_value_type(&param.ty);
            self.check_fixed_array_size_in_type(&param.ty);
            self.check_fn_pointer_signatures(&param.ty);
            // J4: reject an unresolved type name (e.g. the supertype's `T`
            // written in an override instead of the bound `Object`).
            self.validate_sig_type(&param.ty, &[]);
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
            self.env.declare_pointer(&param.name.text, param.ty.ptr_depth);
            self.note_fixed_array_decl(&param.name.text, &param.ty, true);
            if crate::infer::type_ref_is_void_pointer(&param.ty) {
                self.env.declare_void_base(&param.name.text);
            }
            // `weak` parameter (§M.14.3): validate class type (E0455) and
            // register so reads route through `.get()` (E0456).
            if param.is_weak {
                self.check_weak_param(param);
                self.env.weak_names.insert(param.name.text.clone());
            }
        }
        self.validate_sig_return(&method.return_type, &[]);
        self.check_iface_return_type(&method.return_type);
        let saved = self.current_return.take();
        self.current_return_ptr = return_ptr_depth(&method.return_type);
        self.current_return_void = return_is_void_pointer(&method.return_type);
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
        self.check_out_params_assigned(&method.params, body, &method.name.text);
        self.check_locals_definitely_assigned(body);
        self.check_final_not_reassigned(&method.params, body);
        self.check_missing_return(
            &method.return_type,
            body,
            &method.name.text,
            method.name.span,
        );
        // §X.1.3: checked exceptions the method body raises must be
        // covered by its `throws` clause.
        self.enforce_declared_throws(&method.throws, &method.name.text);
        self.flush_uninferable_news();
        self.in_unsafe = saved_unsafe;
        self.in_async = saved_async;
        self.current_return = saved;
        self.in_static = saved_static;
        self.env.weak_names.clear();
        // Method-local generic params would also clear here, but the
        // class's params are still active until check_class finishes.
        // We can't surgically remove just the method's — for Turn 1 we
        // accept the over-broadening (no method-local generics in any
        // existing example).
        self.env.pop_scope();
    }

    /// **E0416 for TYPES** — a type declared without `public` is visible only
    /// inside its own package (§4.4), exactly as in Java.
    ///
    /// Member-level visibility was already enforced (E0414/E0415/E0416), but
    /// the TYPE itself was not: a package-private helper could be named,
    /// imported and instantiated from any other package, so the modifier on the
    /// declaration meant nothing. A modifier the compiler ignores is a comment.
    ///
    /// `internal` is exempt. The spec scopes it to the MODULE, and the checker
    /// has no module identity — a workspace's members compile together into one
    /// symbol table. Treating it as package-private would reject code the spec
    /// allows, which is the worse of the two errors, so it stays visible until
    /// modules are modelled.
    ///
    /// Only single-segment names are checked. A written FQN (`a.b.C`) is
    /// deliberate and rare, and resolving one here would duplicate the import
    /// machinery for no gain.
    fn check_type_visibility(&mut self, tref: &TypeRef) {
        self.check_type_name_visibility(&tref.name, tref.span);
    }

    /// [`Self::check_type_visibility`] for a bare name — `new X(…)` carries a
    /// [`QualifiedName`] rather than a full type reference.
    fn check_type_name_visibility(
        &mut self,
        name: &juxc_ast::QualifiedName,
        span: juxc_source::Span,
    ) {
        if name.segments.len() != 1 {
            return;
        }
        let bare = &name.segments[0].text;
        let Some((fqn, sig)) = self.symbols.resolve_class(bare) else {
            return;
        };
        if !matches!(sig.visibility, juxc_ast::Visibility::Package) {
            return;
        }
        // Same package — including the "no package at all" case, where every
        // declaration shares one root scope.
        if sig.package.as_slice() == self.env.current_package.as_slice() {
            return;
        }
        let declaring = if sig.package.is_empty() {
            "the root package".to_string()
        } else {
            format!("`{}`", sig.package.join("."))
        };
        let here = if self.env.current_package.is_empty() {
            "the root package".to_string()
        } else {
            format!("`{}`", self.env.current_package.join("."))
        };
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0416_PackagePrivateAccess,
                format!(
                    "cannot use package-private type `{fqn}` from {here} -- it is declared in \
                     {declaring} without `public`, so it is visible only inside that \
                     package (§4.4). Mark the declaration `public` to export it",
                ),
            )
            .with_span(span),
        );
    }

    /// **E0417 (J4)** — validate that every bare type name appearing in a
    /// signature position (a parameter, return, or field type) resolves to a
    /// real type. The motivating case: a class that `implements Holder<Object>`
    /// and overrides `void test(T t)` — `T` is the *interface's* type-parameter
    /// name, not a type in scope here, so per Jux's Java-shaped override rule it
    /// must be written as the bound argument `Object`. Catching it here turns a
    /// confusing rustc `E0412 cannot find type T` into a precise diagnostic that
    /// points at the offending name.
    ///
    /// `extra` carries generic parameters that are in scope but NOT registered
    /// in `self.env.generic_params` — needed for free functions, whose own
    /// `<T>` params aren't pushed into the env (methods push theirs before this
    /// runs, so they pass `&[]`).
    ///
    /// Conservative by design: only single-segment names are checked, and a
    /// small allowlist of language intrinsics that lower to emitted helpers
    /// (rather than symbol-table types) is exempt. Everything else defers to the
    /// real resolver [`ty_from_ref`] — a `Ty::Unknown` result is the
    /// unambiguous "this name resolves to nothing" signal.
    fn validate_sig_type(&mut self, tref: &TypeRef, extra: &[TypeParam]) {
        // Function-type shape — recurse into each parameter and the return;
        // `name`/`generic_args` are conventionally empty in this case.
        if let Some(fs) = &tref.fn_shape {
            for p in &fs.params {
                self.validate_sig_type(p, extra);
            }
            self.validate_sig_type(&fs.return_type, extra);
            return;
        }
        // A synthetic const-generic argument (`<float, 256>`) is carried as a
        // TypeRef whose only segment is the literal text — never a type name.
        if tref.const_literal_text().is_some() {
            return;
        }
        // A resolvable head still has to be REACHABLE from here.
        self.check_type_visibility(tref);
        if self.sig_head_unresolved(tref, extra) {
            let bare = &tref.name.segments[0].text;
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0417_UnknownType,
                    format!(
                        "unknown type `{bare}` -- no primitive, in-scope generic parameter, or \
                         class/record/enum/interface of that name is visible here. If this \
                         overrides a member of a generic supertype, name the concrete type \
                         argument it was bound to (e.g. `Object` under `implements \
                         Holder<Object>`), not the supertype's type-parameter name",
                    ),
                )
                .with_span(tref.span),
            );
            return; // bogus head — don't descend into its (also bogus) args
        }
        // Head resolves — validate each concrete generic argument too, so
        // `List<Bogus>` is caught at `Bogus`.
        for ga in &tref.generic_args {
            if let juxc_ast::GenericArg::Type(inner) = ga {
                self.validate_sig_type(inner, extra);
            }
        }
    }

    /// True when the single-segment head name of `tref` resolves to nothing.
    /// Multi-segment names (explicit FQNs) and intrinsic builtin type names are
    /// never flagged. See [`Self::validate_sig_type`].
    fn sig_head_unresolved(&self, tref: &TypeRef, extra: &[TypeParam]) -> bool {
        // Only bare single-segment names are at risk of the `T`-leak.
        if tref.name.segments.len() != 1 {
            return false;
        }
        let bare = tref.name.segments[0].text.as_str();
        // Return-slot / inference keywords and language intrinsics that lower to
        // emitted helpers rather than symbol-table types. These never appear in
        // `symbols.*`, so `ty_from_ref` can't vouch for them.
        const INTRINSIC: &[&str] = &[
            "void",
            "var",
            "Self",
            "observer",
            "Channel",
            "AsyncMutex",
            "Stream",
            "Task",
        ];
        if INTRINSIC.contains(&bare) || bare == juxc_ast::TUPLE_SENTINEL {
            return false;
        }
        // In-scope generic parameters: those registered in the env (class +
        // method level) plus any passed explicitly (free-function level).
        if self.env.generic_params.contains(bare) {
            return false;
        }
        if extra.iter().any(|tp| tp.name.text == bare) {
            return false;
        }
        // Defer to the real resolver on a name-only probe (strip array /
        // nullable / pointer / generic-arg shapes — none of those change whether
        // the HEAD name resolves). `Ty::Unknown` ⇔ "resolves to nothing".
        let probe = TypeRef {
            name: tref.name.clone(),
            generic_args: vec![],
            nullable: false,
            array_shape: None,
            fn_shape: None,
            ptr_depth: 0,
            span: tref.span,
        };
        matches!(ty_from_ref(&probe, &self.env, self.symbols), Ty::Unknown)
    }

    /// Validate a `ReturnType` slot via [`Self::validate_sig_type`]. `void`
    /// (sync or `async void`) carries no user-named type to resolve.
    fn validate_sig_return(&mut self, rt: &ReturnType, extra: &[TypeParam]) {
        match rt {
            ReturnType::Type(t) | ReturnType::AsyncType(t) => self.validate_sig_type(t, extra),
            ReturnType::Void => {}
        }
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
                            "interface `{bare}` can't be used as a dynamic value type -- its \
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
                             argument(s) (e.g. `{bare}<int>`) -- a raw `{bare}` value slot can't \
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
        let Some(seg) = tref.name.segments.last() else {
            return;
        };
        let bare = seg.text.as_str();
        let is_user_generic_class = self.symbols.classes.iter().any(|(k, c)| {
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
                     storage slot (field, local, or return) in this phase -- the container \
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
    /// E0442 for `5 as String` and `"5" as int`: `as` converts between
    /// numbers, and text is not a number. Interpolation makes text of a value,
    /// and parsing makes a number of text.
    fn check_string_cast(&mut self, c: &juxc_ast::CastExpr) {
        if c.ty.array_shape.is_some() || c.ty.nullable || c.ty.ptr_depth > 0 {
            return;
        }
        let target_ty = ty_from_ref(&c.ty, &self.env, self.symbols);
        let src_ty = infer_expr(&c.value, &self.env, self.symbols);
        let help = match (&src_ty, &target_ty) {
            (Ty::Primitive(_), Ty::String) => "make text of a value with interpolation: `$\"${x}\"`",
            (Ty::String, Ty::Primitive(_)) => "read a number out of text by parsing it, such as `s.parse<int>()`",
            _ => return,
        };
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0442_UnrelatedCast,
                format!("cannot cast `{src_ty}` to `{target_ty}`: `as` converts between numbers, and text is not a number"),
            )
            // The target type: a literal operand's span is empty, and a cast
            // over one would point at the start of the file.
            .with_span(c.ty.span)
            .with_help(help),
        );
    }

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
                format!("`{src_ty} => {target_ty}` can never be true: the types are unrelated",),
            )
            .with_span(t.span),
        );
    }

    /// E0489 when `receiver` is an `any` (§T.1.2), which has no members and
    /// no operators: `what` finishes the sentence ("has no field `x`").
    /// Returns whether it fired, so a caller can skip the member lookup that
    /// would otherwise report the same thing a second, vaguer way.
    fn check_any_receiver(&mut self, receiver: &Expr, what: &str, span: Span) -> bool {
        if !matches!(infer_expr(receiver, &self.env, self.symbols), Ty::Any) {
            return false;
        }
        self.diagnostics.push(
            Diagnostic::error(code::Code::E0489_AnyHasNoOperation, format!("an `any` {what}"))
                .with_span(span)
                .with_help("test it with `=>` first and use the value as what it is: `if (v => Dog d) { d.bark(); }` (§T.1.2)"),
        );
        true
    }

    /// E0489 for a binary operator with an `any` operand (§T.1.2). What is
    /// left of an `any` is `===` / `!==` and its text, so `"x" + v` (a
    /// String on the other side makes `+` concatenation) is fine; `==` gets
    /// its own hint, since `===` is what the program can say instead.
    fn check_any_binary(&mut self, b: &juxc_ast::BinaryExpr) {
        if matches!(b.op, BinaryOp::RefEq | BinaryOp::RefNeq) {
            return;
        }
        let left = infer_expr(&b.left, &self.env, self.symbols);
        let right = infer_expr(&b.right, &self.env, self.symbols);
        if !left.is_any() && !right.is_any() {
            return;
        }
        if b.op == BinaryOp::Add && (left.is_string() || right.is_string()) {
            return;
        }
        let span = [b.span, expr_span(&b.left), expr_span(&b.right)]
            .into_iter()
            .find(|sp| *sp != Span::DUMMY)
            .unwrap_or(b.span);
        let (message, help) = if matches!(b.op, BinaryOp::Eq | BinaryOp::NotEq) {
            (
                format!("an `any` has no `{}`", b.op.as_rust_str()),
                "`===` / `!==` compare an `any`: the same object for a class, array or collection, an equal value otherwise (§T.1.2)",
            )
        } else {
            (
                format!("an `any` has no operator `{}`", b.op.as_rust_str()),
                "test it with `=>` first and use the value as what it is: `if (v => int n) { n + 1 }` (§T.1.2)",
            )
        };
        self.diagnostics.push(
            Diagnostic::error(code::Code::E0489_AnyHasNoOperation, message)
                .with_span(span)
                .with_help(help),
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
                self.check_fn_pointer_signatures(t);
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
        let Ty::User { name, .. } = receiver_ty else {
            return;
        };
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
            self.env.declare_pointer(&param.name.text, param.ty.ptr_depth);
            self.note_fixed_array_decl(&param.name.text, &param.ty, true);
            if crate::infer::type_ref_is_void_pointer(&param.ty) {
                self.env.declare_void_base(&param.name.text);
            }
        }
        let saved = self.current_return.take();
        self.current_return_ptr = return_ptr_depth(&op.return_type);
        self.current_return_void = return_is_void_pointer(&op.return_type);
        self.current_return = Some(return_type_to_ty(&op.return_type, &self.env, self.symbols));
        self.check_block(body);
        self.current_return = saved;
        self.env.pop_scope();
    }

    /// Check every application of a user-defined annotation in `unit` against
    /// its declaration (§A.12): the target kind (E0470), required parameters
    /// (E0472), repetition (E0473) and argument types (E0474).
    ///
    /// Built-in annotations (`@Override`, `@Deprecated`, `@Cfg`, the meta
    /// annotations) are not declared with `annotation`, so they are not in the
    /// table and are left to their own checks.
    fn check_annotation_applications(&mut self, unit: &CompilationUnit) {
        // A generated crate stub carries binding markers (`@rust`, `@MutSelf`)
        // that are not annotations a program writes.
        self.checking_external_unit = unit.is_external;
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(f) => self.check_applied_annotations(&f.annotations, "METHOD"),
                TopLevelDecl::Annotation(a) => {
                    self.check_applied_annotations(&a.annotations, "ANNOTATION")
                }
                TopLevelDecl::Class(c) => {
                    self.check_applied_annotations(&c.annotations, "TYPE");
                    for m in &c.methods {
                        self.check_applied_annotations(&m.annotations, "METHOD");
                    }
                    for f in &c.fields {
                        self.check_applied_annotations(&f.annotations, "FIELD");
                    }
                    for p in &c.properties {
                        self.check_applied_annotations(&p.annotations, "FIELD");
                    }
                    for k in &c.constructors {
                        self.check_applied_annotations(&k.annotations, "CONSTRUCTOR");
                    }
                }
                TopLevelDecl::Record(r) => {
                    self.check_applied_annotations(&r.annotations, "TYPE");
                    for m in &r.methods {
                        self.check_applied_annotations(&m.annotations, "METHOD");
                    }
                }
                TopLevelDecl::Enum(e) => {
                    self.check_applied_annotations(&e.annotations, "TYPE");
                    for m in &e.methods {
                        self.check_applied_annotations(&m.annotations, "METHOD");
                    }
                }
                TopLevelDecl::Interface(i) => {
                    self.check_applied_annotations(&i.annotations, "TYPE");
                    for m in &i.methods {
                        self.check_applied_annotations(&m.annotations, "METHOD");
                    }
                }
                _ => {}
            }
        }
    }

    /// The declaration of a user annotation, by the name an application
    /// writes. Annotation names are case-insensitive (JUX-LANG-V1 3.6), and a
    /// same-package or imported declaration is preferred over one elsewhere.
    fn applied_annotation_sig(
        &self,
        written: &juxc_ast::QualifiedName,
    ) -> Option<crate::symbol_table::AnnotationSig> {
        let last = written.segments.last()?.text.as_str();
        let dotted = written
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".");
        if let Some(sig) = self.symbols.annotations.get(&dotted) {
            return Some(sig.clone());
        }
        let pkg = self.env.current_package.join(".");
        let mut candidates: Vec<(&String, &crate::symbol_table::AnnotationSig)> = self
            .symbols
            .annotations
            .iter()
            .filter(|(k, _)| k.rsplit('.').next().is_some_and(|b| b.eq_ignore_ascii_case(last)))
            .collect();
        candidates.sort_by_key(|(k, _)| {
            let imported = self.env.unqualified.values().any(|v| v == *k);
            let same_pkg = k.rsplit_once('.').map(|(p, _)| p).unwrap_or("") == pkg;
            (!(imported || same_pkg), (*k).clone())
        });
        candidates.first().map(|(_, sig)| (*sig).clone())
    }

    /// W0240 for `@Derive`, W0241 for a name that is neither built in nor
    /// declared (§A.12). Called only for annotations no `annotation`
    /// declaration matches.
    fn check_builtin_or_unknown_annotation(&mut self, a: &juxc_ast::Annotation) {
        if self.checking_external_unit {
            return;
        }
        // The §A.1 built-ins, the §TS.1 test annotations, and the meta
        // annotations a declaration uses. Lower case: names are
        // case-insensitive (LANG-V1 §3.6).
        const BUILTIN: &[&str] = &[
            "test", "beforeall", "beforeeach", "aftereach", "afterall", "ignore", "derive", "deprecated",
            "override", "inline", "noinline", "align", "repr", "export", "extern", "native", "nativemodule",
            "cfg", "entry", "register", "interrupt", "plugininterface", "reflectable", "annotationtype",
            "layout", "target", "retention", "repeatable",
        ];
        let Some(last) = a.name.segments.last() else { return };
        let written = last.text.as_str();
        let lower = written.to_ascii_lowercase();
        if lower == "derive" {
            self.diagnostics.push(
                Diagnostic::warning(
                    code::Code::W0240_DeriveNoOp,
                    "`@Derive` does nothing: records, structs and enums get `==`, `hash` and `string` without it (§O.3)",
                )
                .with_span(a.span)
                .with_help("remove the annotation"),
            );
            return;
        }
        if BUILTIN.contains(&lower.as_str()) {
            return;
        }
        let declared: Vec<String> = self
            .symbols
            .annotations
            .keys()
            .map(|k| k.rsplit('.').next().unwrap_or(k).to_string())
            .collect();
        let suggestion = BUILTIN
            .iter()
            .map(|b| b.to_string())
            .chain(declared)
            .map(|candidate| (edit_distance(&lower, &candidate.to_ascii_lowercase()), candidate))
            .filter(|(d, _)| *d <= 2)
            .min_by_key(|(d, _)| *d)
            .map(|(_, c)| c);
        let mut diagnostic = Diagnostic::warning(
            code::Code::W0241_UnknownAnnotation,
            format!("`@{written}` is not a built-in annotation and no `annotation {written}` is declared, so it has no effect"),
        )
        .with_span(a.span);
        diagnostic = match suggestion {
            Some(name) => diagnostic.with_help(format!("did you mean `@{}`?", canonical_annotation_spelling(&name))),
            None => diagnostic.with_help(format!("declare it with `public annotation {written} {{ }}`, or remove it")),
        };
        self.diagnostics.push(diagnostic);
    }

    /// Check one declaration's annotation list, where the declaration is of
    /// the §A.3 target `kind`.
    fn check_applied_annotations(&mut self, annotations: &[juxc_ast::Annotation], kind: &str) {
        let mut seen: Vec<(String, bool)> = Vec::new();
        for a in annotations {
            let Some(sig) = self.applied_annotation_sig(&a.name) else {
                self.check_builtin_or_unknown_annotation(a);
                continue;
            };
            let name = a.name.segments.last().map(|s| s.text.clone()).unwrap_or_default();

            // E0473: once per declaration unless `@Repeatable`.
            let key = name.to_ascii_lowercase();
            if seen.iter().any(|(k, _)| *k == key) && !sig.repeatable {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0473_AnnotationNotRepeatable,
                        format!(
                            "`@{name}` appears more than once here, and is not `@Repeatable` -- \
                             mark its declaration `@Repeatable` to allow that (§A.7)"
                        ),
                    )
                    .with_span(a.span),
                );
            }
            seen.push((key, sig.repeatable));

            // E0470: the declaration kind must be one `@Target` names.
            if !sig.targets.is_empty() && !sig.targets.iter().any(|t| t == kind) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0470_AnnotationTargetMismatch,
                        format!(
                            "`@{name}` cannot be applied to a {} -- its `@Target` allows {} (§A.3)",
                            target_kind_noun(kind),
                            sig.targets.join(", "),
                        ),
                    )
                    .with_span(a.span),
                );
            }

            // Bind each argument to its parameter: by name, else by position.
            // Each bound value keeps a span to report at. A literal carries no
            // span of its own, so a named argument reports at its NAME and a
            // positional one at the annotation.
            let mut given: Vec<Option<(&Expr, juxc_source::Span)>> = vec![None; sig.params.len()];
            let mut position = 0usize;
            let mut misnamed = false;
            for arg in &a.args {
                match arg {
                    juxc_ast::AnnotationArg::Named { name: n, value } => {
                        match sig.params.iter().position(|p| p.name == n.text) {
                            Some(i) => given[i] = Some((value, n.span)),
                            None => {
                                misnamed = true;
                                let known = sig
                                    .params
                                    .iter()
                                    .map(|p| format!("`{}`", p.name))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0448_BadNamedArgument,
                                        format!(
                                            "`@{name}` has no parameter `{}`; it declares {}",
                                            n.text,
                                            if known.is_empty() { "none".to_string() } else { known },
                                        ),
                                    )
                                    .with_span(n.span),
                                );
                            }
                        }
                    }
                    juxc_ast::AnnotationArg::Positional(value) => {
                        if position < given.len() {
                            given[position] = Some((value, a.span));
                        }
                        position += 1;
                    }
                }
            }

            for (param, value) in sig.params.iter().zip(given.iter()) {
                match value {
                    // E0472: a parameter without a default needs a value. Not
                    // reported alongside a misspelled parameter name, which is
                    // almost always the same mistake stated twice.
                    None if !param.has_default && !misnamed => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0472_MissingAnnotationParameter,
                                format!(
                                    "`@{name}` needs a value for `{}`, which has no default (§A.6)",
                                    param.name,
                                ),
                            )
                            .with_span(a.span),
                        );
                    }
                    Some((v, at)) => self.check_annotation_argument_type(&name, param, v, *at),
                    None => {}
                }
            }
        }
    }

    /// E0474: an argument whose type does not fit its parameter.
    ///
    /// Only a value whose type is actually known is compared. An enum constant
    /// is written bare (`method = GET`) and does not type as a local, so it is
    /// left alone rather than guessed at; an ARRAY parameter accepts a brace
    /// list of fitting elements, or a single fitting element.
    fn check_annotation_argument_type(
        &mut self,
        annotation: &str,
        param: &crate::symbol_table::AnnotationParamSig,
        value: &Expr,
        fallback: juxc_source::Span,
    ) {
        let expected = ty_from_ref(&param.ty, &self.env, self.symbols);
        let element = match &expected {
            Ty::Array { element, .. } => Some((**element).clone()),
            _ => None,
        };
        let values: Vec<&Expr> = match value {
            Expr::NewArrayLit(lit) if element.is_some() => lit.elements.iter().collect(),
            other => vec![other],
        };
        let want = element.unwrap_or(expected);
        for v in values {
            let found = infer_expr(v, &self.env, self.symbols);
            if matches!(found, Ty::Unknown) || compatible(&want, &found, self.symbols) {
                continue;
            }
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0474_AnnotationParameterType,
                    format!(
                        "`@{annotation}` parameter `{}` is `{}`, but this is `{}`",
                        param.name,
                        type_ref_display(&param.ty),
                        found,
                    ),
                )
                .with_span(match expr_span(v) {
                    s if s == juxc_source::Span::DUMMY => fallback,
                    s => s,
                }),
            );
        }
    }

    /// Validate an `annotation Name { … }` declaration (§A.2, §A.5).
    ///
    /// The parameter types are restricted to the CONSTANT kinds, because an
    /// annotation's values are baked in at compile time: a primitive, a
    /// `String`, an enum, or an array of one of those. Anything else has no
    /// meaning at the point the value is recorded.
    fn check_annotation_decl(&mut self, decl: &juxc_ast::AnnotationDecl) {
        for param in &decl.params {
            if !self.annotation_param_type_is_constant(&param.ty) {
                self.diagnostics.push(
                    juxc_diagnostics::Diagnostic::error(
                        juxc_diagnostics::code::Code::E0417_UnknownType,
                        format!(
                            "`{}` is not a valid annotation parameter type. An annotation's values are compile-time constants, so a parameter must be a primitive, a `String`, an enum, or an array of one of those",
                            type_ref_display(&param.ty),
                        ),
                    )
                    .with_span(param.ty.span),
                );
            }
        }
    }

    /// Whether `t` is one of the constant kinds an annotation parameter may
    /// have (§A.5). An ARRAY of a constant kind qualifies; an array of
    /// anything else does not.
    fn annotation_param_type_is_constant(&self, t: &juxc_ast::TypeRef) -> bool {
        if t.ptr_depth > 0 || t.nullable {
            return false;
        }
        let Some(last) = t.name.segments.last() else { return false };
        let name = last.text.as_str();
        // A primitive, or `String` (which has its own Ty variant and so is
        // not in the primitive table).
        if crate::ty::primitive_from_name(name).is_some() || name == "String" || name == "string" {
            return true;
        }
        // An enum is a constant kind; a class or interface is not.
        self.symbols
            .enums
            .keys()
            .any(|k| k == name || k.rsplit('.').next() == Some(name))
    }

    /// Walk an interface's default-method bodies.
    ///
    /// Only the methods that HAVE a body: an abstract signature has nothing
    /// to check. `this` is the interface itself, which is what lets a default
    /// body call the interface's own members and have the results typed --
    /// including the abstract ones it is written against.
    fn check_interface(&mut self, iface: &juxc_ast::InterfaceDecl) {
        let has_bodies = iface.methods.iter().any(|m| m.body.is_some());
        if !has_bodies {
            return;
        }
        let name = crate::symbol_table::make_fqn(&self.env.current_package, &iface.name.text);
        self.env.set_class(&name);
        for tp in &iface.generic_params {
            self.env.add_generic_param_bounded(&tp.name.text, &tp.bounds);
        }
        self.declare_const_generic_params(&iface.generic_params);
        let this_ty = Ty::User {
            name: name.clone(),
            generic_args: iface
                .generic_params
                .iter()
                .map(|tp| Ty::Param(tp.name.text.clone()))
                .collect(),
        };
        for method in &iface.methods {
            if method.body.is_some() {
                self.check_method(method, &this_ty);
            }
        }
        self.env.clear_generic_params();
        self.env.clear_class();
    }

    /// Walk a record's body — operator overrides plus methods. Same
    /// scope shape as classes: `this` is the record's `Ty::User`,
    /// operator/method params are declared into the body's scope.
    /// `= delete;` operators have no body and are skipped inside
    /// [`Self::check_operator`].
    fn check_record(&mut self, record: &RecordDecl) {
        let name = crate::symbol_table::make_fqn(&self.env.current_package, &record.name.text);
        self.env.set_class(&name);
        for tp in &record.generic_params {
            self.env.add_generic_param_bounded(&tp.name.text, &tp.bounds);
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
        if let Some(compact) = &record.compact_ctor {
            self.check_compact_constructor(record, compact, &this_ty);
        }
        // Additional constructors (§7.6.1): each begins with `this(...)`,
        // which is what guarantees every path reaches the canonical
        // constructor and sets the components.
        let sigs = self.symbols.records.get(&name).map(|r| r.constructors.clone()).unwrap_or_default();
        for ctor in &record.constructors {
            // A constructor E0493 rejected has no signature and no index.
            let Some(idx) = sigs.iter().position(|c| c.span == ctor.span) else { continue };
            let starts_with_this = matches!(
                ctor.body.statements.first(),
                Some(Stmt::Expr(Expr::Call(call))) if matches!(call.callee.as_ref(), Expr::This(_))
            );
            if !starts_with_this {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0492_RecordConstructorMustDelegate,
                        format!(
                            "this constructor of record `{}` must begin with `this(...)`: a record's state \
                             is its header, so every constructor passes the component values on to the \
                             canonical one",
                            record.name.text,
                        ),
                    )
                    .with_span(ctor.span),
                );
            }
            self.check_constructor(ctor, &this_ty, idx);
        }
        self.env.clear_generic_params();
        self.env.clear_class();
    }

    /// The Java-style enum form (JUX-LANG-V1 §7.7.4, ERRATA E34): per-variant
    /// fields, constructors, and each variant's arguments as a call to one of
    /// them. The rules: the fields are `final` (E0494); a constructor only
    /// gives each field its value, once (E0495), which is what lets the
    /// values be computed once per variant; arguments need a constructor and
    /// a constructor excludes payload variants and generics (E0496).
    fn check_java_style_enum(&mut self, enum_decl: &juxc_ast::EnumDecl, fqn: &str) {
        for field in &enum_decl.fields {
            if !field.is_final {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0494_EnumFieldNotFinal,
                        format!(
                            "field `{}` of enum `{}` must be `final`: a variant is one immutable value, \
                             and its fields are set once, by its constructor",
                            field.name.text, enum_decl.name.text,
                        ),
                    )
                    .with_span(field.span),
                );
            }
        }
        let has_ctor = !enum_decl.constructors.is_empty();
        if has_ctor && !enum_decl.generic_params.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0496_EnumArgumentsWithoutConstructor,
                    format!(
                        "generic enum `{}` cannot declare a constructor: a variant built from arguments is \
                         one value, with one type, so there is nothing for the type parameters to vary",
                        enum_decl.name.text,
                    ),
                )
                .with_span(enum_decl.constructors[0].span),
            );
        }
        for variant in &enum_decl.variants {
            if !has_ctor && !variant.args.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0496_EnumArgumentsWithoutConstructor,
                        format!(
                            "variant `{}` passes arguments, but enum `{}` declares no constructor to take them. \
                             Declare one (`{}(...) {{ ... }}`) with fields for the values, or write a payload \
                             variant with the types it carries (`{}(int code)`)",
                            variant.name.text, enum_decl.name.text, enum_decl.name.text, variant.name.text,
                        ),
                    )
                    .with_span(variant.span),
                );
                for arg in &variant.args {
                    self.check_expr(arg);
                }
            }
            if has_ctor && !variant.payload.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0496_EnumArgumentsWithoutConstructor,
                        format!(
                            "variant `{}` declares a payload, but enum `{}` has a constructor: its variants \
                             pass constructor arguments instead (`{}(value, ...)`). Use one form or the other",
                            variant.name.text, enum_decl.name.text, variant.name.text,
                        ),
                    )
                    .with_span(variant.span),
                );
            }
        }
        if !has_ctor {
            return;
        }
        // Each variant calls a constructor with its arguments.
        let Some(sig) = self.symbols.enums.get(fqn) else { return };
        let ctors = sig.constructors.clone();
        for variant in &enum_decl.variants {
            if !variant.payload.is_empty() {
                continue;
            }
            match self.select_ctor_typed(&ctors, &variant.args) {
                Some(k) => {
                    self.ctor_selections.insert(variant.span, k);
                    self.check_call_args(
                        &format!("{} (enum constructor)", enum_decl.name.text),
                        &ctors[k].params,
                        &variant.args,
                        &vec![None; variant.args.len()],
                        variant.span,
                        Some(fqn),
                        &[],
                        &[],
                    );
                }
                None => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0411_WrongArgCount,
                            format!(
                                "no constructor of enum `{}` accepts the {} argument{} variant `{}` passes",
                                enum_decl.name.text,
                                variant.args.len(),
                                if variant.args.len() == 1 { "" } else { "s" },
                                variant.name.text,
                            ),
                        )
                        .with_span(variant.span),
                    );
                    for arg in &variant.args {
                        self.check_expr(arg);
                    }
                }
            }
        }
        // A constructor gives each field its value, once, and does nothing
        // else: `this.mass = mass;` (or `mass = mass` is NOT this: a bare
        // name on the left is the parameter when one shadows the field).
        let field_names: Vec<&str> = enum_decl.fields.iter().map(|f| f.name.text.as_str()).collect();
        for ctor in &enum_decl.constructors {
            let params: Vec<&str> = ctor.params.iter().map(|p| p.name.text.as_str()).collect();
            let mut assigned: Vec<String> = Vec::new();
            for stmt in &ctor.body.statements {
                let target = match stmt {
                    Stmt::Assign(a) if a.op.is_none() => match &a.target {
                        Expr::Field(f) if matches!(f.object.as_ref(), Expr::This(_)) => Some(f.field.text.clone()),
                        Expr::Path(qn)
                            if qn.segments.len() == 1
                                && field_names.contains(&qn.segments[0].text.as_str())
                                && !params.contains(&qn.segments[0].text.as_str()) =>
                        {
                            Some(qn.segments[0].text.clone())
                        }
                        _ => None,
                    },
                    _ => None,
                };
                let reads_this = matches!(stmt, Stmt::Assign(a) if {
                    let mut found = false;
                    juxc_ast::visit::for_each_expr_in(&a.value, &mut |e| {
                        if matches!(e, Expr::This(_)) {
                            found = true;
                        }
                    });
                    found
                });
                let problem = match &target {
                    None => Some(
                        "an enum constructor only gives each field its value (`this.mass = mass;`): the \
                         values are computed once per variant, from its arguments, so there is no place \
                         for other statements"
                            .to_string(),
                    ),
                    Some(f) if !field_names.contains(&f.as_str()) => {
                        Some(format!("`{f}` is not a field of enum `{}`", enum_decl.name.text))
                    }
                    Some(f) if assigned.contains(f) => Some(format!("field `{f}` is set twice")),
                    Some(_) if reads_this => Some(
                        "a field's value cannot read `this`: the other fields may not have theirs yet"
                            .to_string(),
                    ),
                    Some(_) => None,
                };
                if let Some(problem) = problem {
                    self.diagnostics.push(
                        Diagnostic::error(code::Code::E0495_EnumConstructorShape, problem)
                            .with_span(match stmt {
                                Stmt::Assign(a) => a.span,
                                _ => ctor.span,
                            }),
                    );
                }
                if let Some(f) = target {
                    assigned.push(f);
                }
            }
            let missing: Vec<&str> =
                field_names.iter().copied().filter(|f| !assigned.iter().any(|a| a == f)).collect();
            if !missing.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0495_EnumConstructorShape,
                        format!(
                            "this constructor of enum `{}` leaves `{}` without a value",
                            enum_decl.name.text,
                            missing.join("`, `"),
                        ),
                    )
                    .with_span(ctor.span),
                );
            }
        }
    }

    /// A record's compact constructor (JUX-LANG-V1 §7.6.1): the header's
    /// components are its parameters, which it may reassign; the values they
    /// hold when it ends are what the record stores. The record does not
    /// exist while it runs, so `this` is E0491, and so is a `return`, which
    /// would skip the storing.
    fn check_compact_constructor(&mut self, record: &RecordDecl, ctor: &ConstructorDecl, this_ty: &Ty) {
        let mut lambdas: Vec<Span> = Vec::new();
        let mut misuse: Vec<(Span, &str)> = Vec::new();
        juxc_ast::visit::for_each_node(&ctor.body, &mut |node| match node {
            juxc_ast::visit::Node::Expr(Expr::Lambda(l)) => lambdas.push(l.span),
            juxc_ast::visit::Node::Expr(Expr::This(span)) => misuse.push((
                *span,
                "a compact constructor cannot use `this`: the record does not exist until it ends. \
                 Read and assign the components by name, as parameters",
            )),
            juxc_ast::visit::Node::Stmt(Stmt::Return(_, span)) => misuse.push((
                *span,
                "a compact constructor cannot `return`: the components are stored when its body \
                 ends, and a `return` would skip that",
            )),
            _ => {}
        });
        for (span, message) in misuse {
            // A `return` inside a lambda returns from the lambda.
            let in_lambda = lambdas.iter().any(|l| l.start <= span.start && span.end <= l.end);
            if in_lambda && message.contains("`return`") {
                continue;
            }
            self.diagnostics.push(
                Diagnostic::error(code::Code::E0491_CompactConstructorMisuse, message).with_span(span),
            );
        }
        // Checked as a constructor whose parameters are the components.
        let as_ctor = ConstructorDecl {
            annotations: ctor.annotations.clone(),
            visibility: ctor.visibility,
            params: record
                .components
                .iter()
                .map(|c| juxc_ast::Param {
                    name: c.name.clone(),
                    ty: c.ty.clone(),
                    is_final: false,
                    is_ref: false,
                    is_mut_ref: false,
                    default: None,
                    is_varargs: false,
                    is_out: false,
                    is_shared_ref: false,
                    is_weak: false,
                    span: c.span,
                })
                .collect(),
            throws: Vec::new(),
            body: ctor.body.clone(),
            span: ctor.span,
        };
        self.check_constructor(&as_ctor, this_ty, 0);
    }

    // ------------------------------------------------------------------
    // Statement walker
    // ------------------------------------------------------------------

    /// Walk a block — each statement in source order. Doesn't push a
    /// scope; callers wrap if they need scope nesting (e.g. method
    /// body, for-each loop body).
    fn check_block(&mut self, block: &Block) {
        // Null-test narrowing (§7.10) is dropped by an assignment, so each
        // block carries the set of names it assigns anywhere inside itself.
        // Computed per block rather than per function so a narrowing only
        // answers for the region it actually covers.
        let saved = std::mem::replace(
            &mut self.assigned_in_block,
            crate::assigned::names_assigned_in(block),
        );
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
        self.assigned_in_block = saved;
        // A `T[N]` local this block declared, handed both to a `T[]` slot and
        // to a `T[N]` one: the two need different storage for one array.
        for stmt in &block.statements {
            let Stmt::VarDecl(v) = stmt else { continue };
            if let Some((Some(_), Some(fixed_at))) = self.fixed_array_slot_uses.remove(&v.name.text) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0468_FixedArrayNotShareable,
                        format!(
                            "`{}` is handed to a runtime-sized array slot and also to a fixed-size one here: one array cannot be stored both ways (JUX-LANG-V1 §5.5)",
                            v.name.text,
                        ),
                    )
                    .with_span(fixed_at)
                    .with_help(format!(
                        "declare `{}` with a runtime-sized type (`[]`), or pass `{}.clone()` where a copy is meant",
                        v.name.text, v.name.text,
                    )),
                );
            }
        }
    }

    /// A method reference (Missing-defs §M.8). The receiver is a VALUE
    /// (`alice::greet`, bound: `this` is captured) when it names a binding in
    /// scope, and a TYPE otherwise (`User::greet` unbound or static,
    /// `User::new` a constructor). When the member is overloaded, the
    /// expected function type picks one (`slot`, the parameter types of the
    /// slot the reference flows into); none or several fitting is `E0980`
    /// (§M.8.3). The pick is recorded in `method_selections` under the
    /// reference's span, where the backend reads it.
    fn check_method_ref(&mut self, m: &juxc_ast::MethodRefExpr, slot: Option<Vec<Ty>>) {
        let member = m.member.text.as_str();
        // A value receiver: a single name bound in scope (`this` included).
        let bound_class = if m.receiver.segments.len() == 1 {
            match self.env.lookup(&m.receiver.segments[0].text) {
                Some(Ty::User { name, .. }) => Some(name.clone()),
                Some(_) => return,
                None => None,
            }
        } else {
            None
        };
        let class = match &bound_class {
            Some(c) => c.clone(),
            None => crate::infer::resolve_class_name(&m.receiver, &self.env, self.symbols),
        };
        let Some((class_fqn, class_sig)) = self.symbols.resolve_class(&class) else {
            // An interface, record or foreign type: not checked here.
            return;
        };
        let class_fqn = class_fqn.clone();
        // Each candidate's parameter types AS A FUNCTION VALUE: a bound or
        // static method takes its own parameters, an unbound instance method
        // takes the receiver first, a constructor takes the constructor's.
        let receiver_ty = Ty::User { name: class_fqn.clone(), generic_args: Vec::new() };
        let candidates: Vec<Vec<Ty>> = if member == "new" {
            if bound_class.is_some() {
                return;
            }
            class_sig
                .constructors
                .iter()
                .map(|c| c.params.iter().map(|p| lower_member_type(&p.ty, &class_fqn, self.symbols)).collect())
                .collect()
        } else {
            let group = self.symbols.merged_method_overloads(&class_fqn, member);
            group
                .iter()
                .map(|sig| {
                    let own: Vec<Ty> =
                        sig.params.iter().map(|p| lower_member_type(&p.ty, &class_fqn, self.symbols)).collect();
                    if bound_class.is_none() && !sig.is_static {
                        std::iter::once(receiver_ty.clone()).chain(own).collect()
                    } else {
                        own
                    }
                })
                .collect()
        };
        if candidates.is_empty() {
            if member != "new" {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0413_UnresolvedMethod,
                        format!("no method `{member}` on `{}` to reference", class_fqn.rsplit('.').next().unwrap_or(&class_fqn)),
                    )
                    .with_span(m.member.span),
                );
            }
            return;
        }
        // Constructors are picked in `ctor_selections`, methods in
        // `method_selections`, as for a call.
        let is_ctor = member == "new";
        if candidates.len() == 1 {
            if is_ctor {
                self.ctor_selections.insert(m.span, 0);
            } else {
                self.method_selections.insert(m.span, 0);
            }
            return;
        }
        let shown = format!("{}::{member}", m.receiver.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."));
        let Some(slot) = slot else {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0980_AmbiguousMethodRef,
                    format!("`{shown}` names {} overloads, and nothing here says which one (§M.8.3)", candidates.len()),
                )
                .with_span(m.span)
                .with_help("give the reference a function type (`(int) -> Pt f = Pt::new;`), or write a lambda"),
            );
            return;
        };
        let fitting: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, params)| {
                params.len() == slot.len()
                    && params.iter().zip(slot.iter()).all(|(p, s)| compatible(p, s, self.symbols))
            })
            .map(|(k, _)| k)
            .collect();
        match fitting.as_slice() {
            [k] => {
                if is_ctor {
                    self.ctor_selections.insert(m.span, *k);
                } else {
                    self.method_selections.insert(m.span, *k);
                }
            }
            [] => self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0980_AmbiguousMethodRef,
                    format!(
                        "no overload of `{shown}` takes ({})",
                        slot.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "),
                    ),
                )
                .with_span(m.span),
            ),
            _ => self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0980_AmbiguousMethodRef,
                    format!("more than one overload of `{shown}` fits here (§M.8.3)"),
                )
                .with_span(m.span)
                .with_help("write a lambda that calls the one you mean"),
            ),
        }
    }

    /// `sizeof(T)` for a type the operand can only be (§5.9.4): `void` is
    /// `E0463`, a wildcard argument `E0462`, and a generic type written
    /// without its arguments (`Vec`, a user `Box`) `E0461`.
    fn check_sizeof_type(&mut self, t: &juxc_ast::TypeRef, span: Span) {
        if t.name.segments.len() == 1 && t.name.segments[0].text == "void" && t.array_shape.is_none() {
            self.diagnostics.push(
                Diagnostic::error(code::Code::E0463_SizeofVoid, "`sizeof(void)`: `void` has no values, so it has no size (§5.9.4)")
                    .with_span(span),
            );
            return;
        }
        fn has_wildcard(t: &juxc_ast::TypeRef) -> bool {
            t.generic_args.iter().any(|a| match a {
                juxc_ast::GenericArg::Wildcard(_) => true,
                juxc_ast::GenericArg::Type(inner) => has_wildcard(inner),
            })
        }
        if has_wildcard(t) {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0462_SizeofWildcard,
                    "`sizeof` of a wildcard type: `?` stands for some type, and no size belongs to it (§5.9.4)",
                )
                .with_span(span)
                .with_help("name the type argument"),
            );
            return;
        }
        let ty = ty_from_ref(t, &self.env, self.symbols);
        if let Ty::User { name, generic_args } = &ty {
            let declared = self
                .symbols
                .resolve_class(name)
                .map(|(_, c)| c.generic_params.iter().filter(|p| !p.is_const()).count())
                .unwrap_or(0);
            if declared > 0 && generic_args.is_empty() && t.generic_args.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0461_SizeofUnboundGeneric,
                        format!("`sizeof({name})`: `{name}` is generic, and its size depends on its type arguments (§5.9.4)"),
                    )
                    .with_span(span)
                    .with_help(format!("write the arguments, `sizeof({name}<...>)`")),
                );
            }
        }
    }

    /// Record `name` as declared with a fixed-size array type when `ty` has a
    /// fixed dimension (see [`TypeEnv::declare_fixed_array`]).
    fn note_fixed_array_decl(&mut self, name: &str, ty: &juxc_ast::TypeRef, is_param: bool) {
        let fixed = ty
            .array_shape
            .as_ref()
            .is_some_and(|shape| shape.dims.iter().any(|d| matches!(d, juxc_ast::ArrayDim::Fixed(_))));
        if fixed && ty.ptr_depth == 0 {
            self.env.declare_fixed_array(name, is_param);
            if !is_param {
                self.fixed_array_slot_uses.remove(name);
            }
        }
    }

    /// `value` flows into an array slot whose outermost dimension is
    /// runtime-sized (`slot_dynamic`) or fixed. A declared-`T[N]` local is
    /// noted for the end-of-block check; a declared-`T[N]` parameter or field
    /// going into a runtime-sized slot is E0468 at once, since its storage
    /// was fixed where it was declared (JUX-LANG-V1 §5.5, ERRATA E42).
    fn note_array_slot(&mut self, value: &Expr, slot_dynamic: bool, span: Span) {
        let (what, param) = match value {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                match self.env.fixed_array(name) {
                    Some(false) => {
                        let entry = self.fixed_array_slot_uses.entry(name.to_string()).or_default();
                        if slot_dynamic {
                            entry.0.get_or_insert(span);
                        } else {
                            entry.1.get_or_insert(span);
                        }
                        return;
                    }
                    Some(true) => (format!("parameter `{name}`"), true),
                    None => return,
                }
            }
            Expr::Field(f) => {
                let Ty::User { name: class, .. } = infer_expr(&f.object, &self.env, self.symbols) else {
                    return;
                };
                let fixed = self.symbols.lookup_field(&class, &f.field.text).is_some_and(|(fs, _)| {
                    fs.ty.array_shape.as_ref().is_some_and(|shape| {
                        shape.dims.iter().any(|d| matches!(d, juxc_ast::ArrayDim::Fixed(_)))
                    })
                });
                if !fixed {
                    return;
                }
                (format!("field `{}`", f.field.text), false)
            }
            _ => return,
        };
        if !slot_dynamic {
            return;
        }
        let ty = infer_expr(value, &self.env, self.symbols);
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0468_FixedArrayNotShareable,
                format!(
                    "{what} is a fixed-size array (`{ty}`) and cannot be handed to a runtime-sized array slot as the same array (JUX-LANG-V1 §5.5)"
                ),
            )
            .with_span(span)
            .with_help(if param {
                "declare the parameter with a runtime-sized type (`[]`), or pass a copy with `.clone()`"
            } else {
                "declare the field with a runtime-sized type (`[]`), or pass a copy with `.clone()`"
            }),
        );
    }

    /// E0418 (§7.10): `recv.member` where `recv` is `T?` and no test has
    /// narrowed it. Only a member the `T` inside actually has is reported --
    /// a field or property for a read, a method for a call -- so an
    /// `Option` method on a `T?` (`maybe.unwrap()`) and a name `T` lacks
    /// (reported as unknown elsewhere) are left alone.
    fn check_nullable_receiver(&mut self, f: &juxc_ast::FieldExpr, is_call: bool) {
        if f.safe {
            return;
        }
        let Ty::Nullable(inner) = infer_expr(&f.object, &self.env, self.symbols) else {
            return;
        };
        let Ty::User { name, .. } = *inner else {
            return;
        };
        let member = f.field.text.as_str();
        let belongs_to_inner = if is_call {
            self.symbols.lookup_method(&name, member).is_some()
        } else {
            self.symbols.lookup_field(&name, member).is_some()
                || self.symbols.lookup_property(&name, member).is_some()
        };
        if !belongs_to_inner {
            return;
        }
        fn spelled(e: &Expr) -> Option<String> {
            match e {
                Expr::Path(qn) => Some(qn.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".")),
                Expr::This(_) => Some("this".to_string()),
                Expr::Field(inner) => spelled(&inner.object).map(|o| format!("{o}.{}", inner.field.text)),
                _ => None,
            }
        }
        let shown = spelled(&f.object).unwrap_or_else(|| "this value".to_string());
        let bare = name.rsplit('.').next().unwrap_or(&name);
        let what = if is_call { format!("`{member}()`") } else { format!("`.{member}`") };
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0418_MemberOfNullable,
                format!("`{shown}` may be null here (its type is `{bare}?`), so {what} has no value to reach"),
            )
            .with_span(f.span)
            .with_help(format!(
                "test it first (`if ({shown} != null) {{ ... }}`), reach through the null with `?.`, \
                 or assert it with `!!`"
            )),
        );
    }

    /// E0418 (§7.10) for the operands of a binary operator: `x + 1`,
    /// `x < y`, `flag && x` with `x` a `T?` no test has narrowed. `==`, `!=`,
    /// `===` and `??` take a null by design, and so does `+` with a `String`
    /// on either side, which concatenates and prints a null as `null`.
    fn check_nullable_operands(&mut self, b: &juxc_ast::BinaryExpr) {
        let Some(spelled) = nullable_sensitive_op(b.op) else { return };
        if b.op == BinaryOp::Add {
            let is_text = |e: &Expr| {
                let t = infer_expr(e, &self.env, self.symbols);
                matches!(t, Ty::String) || matches!(&t, Ty::Nullable(inner) if matches!(**inner, Ty::String))
            };
            if is_text(&b.left) || is_text(&b.right) {
                return;
            }
        }
        self.check_nullable_operand(&b.left, spelled, expr_span(&b.left));
        self.check_nullable_operand(&b.right, spelled, expr_span(&b.right));
    }

    /// E0418 (§7.10), for an operator: `operand` is a `T?` no null test has
    /// narrowed, and `op` needs its value, as a member does. Without this
    /// `int? + 1` typed as `int?` and the lowering asked Rust to add to an
    /// `Option`.
    fn check_nullable_operand(&mut self, operand: &Expr, op: &str, span: Span) {
        if self.expr_ptr_depth(operand) > 0 {
            // A raw pointer's null is its own value (§L.6.1).
            return;
        }
        let Ty::Nullable(inner) = infer_expr(operand, &self.env, self.symbols) else {
            return;
        };
        fn spelled(e: &Expr) -> Option<String> {
            match e {
                Expr::Path(qn) => Some(qn.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".")),
                Expr::This(_) => Some("this".to_string()),
                Expr::Field(inner) => spelled(&inner.object).map(|o| format!("{o}.{}", inner.field.text)),
                _ => None,
            }
        }
        let shown = spelled(operand).unwrap_or_else(|| "this operand".to_string());
        // Only a local or parameter narrows (§7.10); a field or property is
        // copied into one first.
        let local = matches!(operand, Expr::Path(qn)
            if qn.segments.len() == 1 && self.env.lookup(&qn.segments[0].text).is_some());
        let help = if local {
            format!(
                "test it first (`if ({shown} != null) {{ ... }}`), give a default with `??`, \
                 or assert it with `!!`"
            )
        } else {
            format!(
                "assert it with `{shown}!!`, give a default with `{shown} ?? ...`, or copy it \
                 into a local and test that (only a local narrows)"
            )
        };
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0418_MemberOfNullable,
                format!("`{shown}` may be null here (its type is `{inner}?`), so `{op}` has no value to work on"),
            )
            .with_span(span)
            .with_help(help),
        );
    }

    /// Every binding `cond` proves non-null when it evaluates to `outcome`,
    /// each with its non-null type (§7.10): the `!= null` conjuncts of an
    /// `&&` chain when true, the `== null` disjuncts of an `||` chain when
    /// false. Bindings the block reassigns are left out, as for any
    /// narrowing.
    fn narrowings(
        &self,
        cond: &Expr,
        outcome: bool,
        assigned: &std::collections::HashSet<String>,
    ) -> Vec<(String, Ty)> {
        let mut names = Vec::new();
        null_tested_names(cond, outcome, &mut names);
        let mut out: Vec<(String, Ty)> = Vec::new();
        for name in names {
            if out.iter().any(|(n, _)| n == name) || assigned.contains(name) {
                continue;
            }
            if let Some(Ty::Nullable(inner)) = self.env.lookup(name) {
                out.push((name.to_string(), (**inner).clone()));
            }
        }
        out
    }

    /// Run `check` with `narrowed` declared non-null in a scope of its own.
    fn check_narrowed(&mut self, narrowed: &[(String, Ty)], check: impl FnOnce(&mut Self)) {
        if narrowed.is_empty() {
            check(self);
            return;
        }
        self.env.push_scope();
        for (name, ty) in narrowed {
            self.env.declare(name, ty.clone());
        }
        check(self);
        self.env.pop_scope();
    }

    /// §S.2.6: the operands of an arithmetic or bitwise operator must meet in one
    /// type. A signed and an unsigned integer that no one type holds (`int` and
    /// `uint`, `long` and `ulong`) do not, and silently picking either side is
    /// how `-1 + len` became a huge unsigned number. An untyped literal takes
    /// the other side's type and never trips this; a comparison compares the
    /// values exactly and is not checked here.
    /// E0476: `a < b < c`. The left comparison is a `bool`, and ordering a
    /// `bool` against a number is meaningless; the author meant `&&`.
    fn check_comparison_chain(&mut self, b: &juxc_ast::BinaryExpr) {
        let relational = |op: BinaryOp| matches!(op, BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge);
        if !relational(b.op) {
            return;
        }
        let Expr::Binary(inner) = &*b.left else { return };
        if !relational(inner.op) {
            return;
        }
        let middle = expr_span(&inner.right);
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0476_ChainedComparison,
                "comparisons do not chain: the first comparison is a `bool`, and a `bool` cannot be \
                 compared with a number",
            )
            .with_span(b.span)
            .with_help(format!(
                "compare each pair and join them with `&&`: `a < b && b < c` (the middle operand at {}..{} appears in both)",
                middle.start, middle.end,
            )),
        );
    }

    fn check_numeric_operands(&mut self, b: &juxc_ast::BinaryExpr) {
        if !matches!(
            b.op,
            BinaryOp::Add
                | BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::Rem
                | BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
        ) {
            return;
        }
        if crate::infer::untyped_int_literal(&b.left) || crate::infer::untyped_int_literal(&b.right) {
            return;
        }
        let (Ty::Primitive(l), Ty::Primitive(r)) = (
            infer_expr(&b.left, &self.env, self.symbols),
            infer_expr(&b.right, &self.env, self.symbols),
        ) else {
            return;
        };
        // §S.2.5: the bitwise operators are for integers. On two `bool`s they
        // would be a non-short-circuiting `&&`/`||`, which reads as a typo of
        // the logical operator and is refused.
        if matches!(b.op, BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor)
            && l == Primitive::Bool
            && r == Primitive::Bool
        {
            let (word, logical) = match b.op {
                BinaryOp::BitAnd => ("&", "&&"),
                BinaryOp::BitOr => ("|", "||"),
                _ => ("^", "!="),
            };
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0410_TypeMismatch,
                    format!("the bitwise operator `{word}` does not apply to `bool` operands (§S.2.5)"),
                )
                .with_span(b.span)
                .with_help(format!("use the logical operator `{logical}`")),
            );
            return;
        }
        if crate::ty::promote_numeric(l, r) != crate::ty::NumericPromotion::NoCommonType {
            return;
        }
        let (ln, rn) = (crate::ty::primitive_name(l), crate::ty::primitive_name(r));
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0410_TypeMismatch,
                format!(
                    "`{ln}` and `{rn}` have no common type: no one integer type holds every value \
                     of both, so either result type could be wrong -- cast one operand to the type \
                     you mean, `({ln})` or `({rn})`"
                ),
            )
            .with_span(b.span),
        );
    }

    /// Walk one statement, emitting diagnostics where types disagree.
    /// (see `match_null_test` below for the null-test shapes)
    /// Check one statement, ending `while`-condition refinements it assigns
    /// (§T.6.5): before it when it assigns inside a nested statement, and
    /// after it when it is the direct `x = e;` whose `e` still reads `x`.
    fn check_stmt(&mut self, stmt: &Stmt) {
        if self.loop_narrowed.is_empty() {
            self.check_stmt_inner(stmt);
            return;
        }
        let assigned = crate::assigned::names_assigned_in_stmt(stmt);
        let direct = crate::assigned::direct_assign_target(stmt);
        let mut after: Vec<(String, Ty)> = Vec::new();
        let mut kept: Vec<(String, Ty)> = Vec::new();
        for (name, declared) in std::mem::take(&mut self.loop_narrowed) {
            if !assigned.contains(&name) {
                kept.push((name, declared));
            } else if direct.as_deref() == Some(name.as_str()) {
                after.push((name, declared));
            } else {
                self.env.declare(&name, declared);
            }
        }
        self.loop_narrowed = kept;
        let prev = std::mem::replace(&mut self.loop_assign_widen, after.first().cloned());
        self.check_stmt_inner(stmt);
        self.loop_assign_widen = prev;
        for (name, declared) in after {
            self.env.declare(&name, declared);
        }
    }

    fn check_stmt_inner(&mut self, stmt: &Stmt) {
        match stmt {
            // `if cfg` is resolved to its branch before this phase runs (the driver's
            // cfg pass); a unit that skipped that pass has nothing to say here.
            Stmt::IfCfg(_) => {}
            Stmt::VarDecl(v) => {
                // A declared interface-typed local lowers to `Rc<dyn Trait>`
                // — reject the non-dispatchable forms before the backend
                // emits a broken slot type.
                if let Some(t) = &v.ty {
                    self.check_local_type_known(t);
                    if let (Some(shape), Some(init)) = (t.array_shape.as_ref(), v.init.as_ref()) {
                        if let Some(outer) = shape.dims.first() {
                            let slot_dynamic = matches!(outer, juxc_ast::ArrayDim::Dynamic);
                            self.note_array_slot(init, slot_dynamic, expr_span(init));
                        }
                    }
                    self.check_iface_value_type(t);
                    self.check_wildcard_storage_type(t);
                    self.check_fixed_array_size_in_type(t);
                    self.check_fn_pointer_signatures(t);
                    self.check_type_visibility(t);
                }
                // If both a declared type and an initializer are
                // present, the two must be compatible. Otherwise the
                // present one wins.
                let declared =
                    v.ty.as_ref()
                        .map(|t| ty_from_ref(t, &self.env, self.symbols));
                if let (Some(Ty::Fn { params, .. }), Some(Expr::Lambda(_) | Expr::MethodRef(_))) = (&declared, &v.init) {
                    self.lambda_slot_params = Some(params.clone());
                }
                let inferred = v.init.as_ref().map(|e| {
                    // Walk the initializer for nested checks (e.g. a
                    // call inside the RHS) before reading its type.
                    self.check_expr(e);
                    infer_expr(e, &self.env, self.symbols)
                });
                let destructure = juxc_ast::record_destructure_arity(&v.name.text);
                let final_ty = match (&declared, &inferred) {
                    (Some(d), Some(i)) if destructure.is_some() => {
                        self.check_record_destructure(v, d, i, destructure.unwrap_or_default())
                    }
                    (Some(d), Some(i)) => {
                        // A raw-pointer slot (`T*`) accepts the `null` literal —
                        // `null` is the sole `T*` literal for any `T` (§L.6.1).
                        // The erased `Ty` drops `ptr_depth`, so `compatible`
                        // would otherwise compare `int` against `<unknown>?` and
                        // wrongly reject `int* p = null;` / `RawHandle* h = null;`.
                        let ptr_null_ok = match (v.ty.as_ref(), v.init.as_ref()) {
                            // §L.6.1a: a pointer on either side is checked as a
                            // pointer (pointee and depth, no widening), which also
                            // lets `null` into any pointer slot.
                            (Some(t), Some(init)) => self.check_pointer_flow(
                                t.ptr_depth,
                                crate::infer::type_ref_is_void_pointer(t),
                                d,
                                init,
                                v.span,
                            ),
                            _ => false,
                        };
                        if let Some(init) = v.init.as_ref() {
                            self.check_literal_fits(d, init, v.span);
                        }
                        if ptr_null_ok {
                            // Accepted as a null pointer; no mismatch to report.
                        } else if v.init.as_ref().is_some_and(|init| function_into_any(d, init)) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0410_TypeMismatch,
                                    format!(
                                        "type mismatch in declaration of `{}`: a function value does not convert to `any`",
                                        v.name.text,
                                    ),
                                )
                                .with_span(v.span)
                                .with_help("a function has no identity and no string form to hold (§T.1.2); give the slot the function's own type"),
                            );
                        } else if !compatible(d, i, self.symbols) {
                            let mut diag = Diagnostic::error(
                                code::Code::E0410_TypeMismatch,
                                format!(
                                    "type mismatch in declaration of `{}`: expected {}, found {}",
                                    v.name.text, d, i,
                                ),
                            )
                            .with_span(v.span);
                            if let Some(help) = fn_kind_mismatch_help(d, i) {
                                diag = diag.with_help(help);
                            }
                            self.diagnostics.push(diag);
                        }
                        d.clone()
                    }
                    (Some(d), None) => d.clone(),
                    (None, Some(i)) => i.clone(),
                    (None, None) => Ty::Unknown,
                };
                // Record a `var x = new X<>()` whose inferred type still has an
                // unresolved generic argument: if `x` is never referenced later
                // (checked at body end) nothing can pin it → E0453.
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
                // Pointer depth: the declared type's, or for `var q = p + 1`
                // the initializer's (computed before `v.name` is bound, so a
                // shadowing `var p = p + 1` reads the outer `p`).
                let ptr_depth = match (&v.ty, &v.init) {
                    (Some(t), _) => t.ptr_depth,
                    (None, Some(init)) => self.expr_ptr_depth(init),
                    (None, None) => 0,
                };
                let void_base = match (&v.ty, &v.init) {
                    (Some(t), _) => crate::infer::type_ref_is_void_pointer(t),
                    (None, Some(init)) => crate::infer::pointer_base_is_void(init, &self.env, self.symbols),
                    (None, None) => false,
                };
                self.env.declare(&v.name.text, final_ty);
                if let Some(t) = &v.ty {
                    self.note_fixed_array_decl(&v.name.text, t, false);
                }
                self.env.declare_pointer(&v.name.text, ptr_depth);
                if void_base {
                    self.env.declare_void_base(&v.name.text);
                }
            }

            Stmt::Assign(a) => {
                // `v += 1` on an `any` (§T.1.2): it has no operators. And
                // `x += 1` on a `T?` target reads the target first (§7.10).
                if let Some(op) = a.op {
                    if self.check_any_receiver(&a.target, &format!("has no operator `{}=`", op.as_rust_str()), a.span) {
                        self.check_expr(&a.value);
                        return;
                    }
                    if let Some(spelled) = nullable_sensitive_op(op) {
                        self.check_nullable_operand(&a.target, &format!("{spelled}="), a.span);
                    }
                }
                // `p += n` / `p -= n`: the step must be an integer (§L.6.1a).
                if matches!(a.op, Some(juxc_ast::BinaryOp::Add) | Some(juxc_ast::BinaryOp::Sub))
                    && self.expr_ptr_depth(&a.target) > 0
                {
                    let step_ty = infer_expr(&a.value, &self.env, self.symbols);
                    let integer = match &step_ty {
                        Ty::Primitive(p) => crate::ty::integer_bits(*p).is_some(),
                        _ => true,
                    };
                    if !integer {
                        self.push_pointer_op(
                            &format!("a pointer steps by an integer count of elements, and this step is a `{step_ty}`"),
                            a.span,
                        );
                        self.check_expr(&a.value);
                        return;
                    }
                }
                // `p += n` / `p -= n` (and `p++` as a statement) step a pointer.
                if !self.in_unsafe
                    && matches!(a.op, Some(juxc_ast::BinaryOp::Add) | Some(juxc_ast::BinaryOp::Sub))
                    && self.expr_ptr_depth(&a.target) > 0
                {
                    self.unsafe_pointer_op("pointer arithmetic", a.span);
                }
                // A `weak` field WRITE (§6.5): `this.parent = p` / `= null`.
                // The target is a write place, not a read, so the bare-read
                // guard (E0456) must NOT fire — check only the receiver. And a
                // weak slot accepts both a target-class value and `null`, so
                // its assignability is checked against a NULLABLE view of the
                // declared type.
                // A PROPERTY target is likewise a write, not a read. Walking
                // it as one made a single illegal write report twice: the read
                // check on the target, and then the setter check below. The
                // setter check is the precise one — it names the accessor and
                // its visibility — so the read walk stops at the receiver here,
                // exactly as it does for a weak field.
                let weak_target = self.assign_target_is_weak_field(&a.target);
                // (`weak_target` alone drives the nullable widening below; the
                // walk decision is the broader "this is a write, not a read".)
                let write_target = weak_target || self.assign_target_is_property(&a.target);
                if write_target {
                    if let Expr::Field(f) = &a.target {
                        self.check_expr(&f.object);
                    }
                } else {
                    self.check_expr(&a.target);
                }
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
                // **E0465: `final`/`const` field reassignment (§5.6).** A final
                // field is assign-once — settable only by its declaration
                // initializer or in a constructor / `init` block of its own
                // object. Any other write (a method, external code, another
                // instance, a static method) is rejected. Covers plain `=`,
                // compound `+=`, and desugared `++`/`--` (all `Stmt::Assign`).
                // A record is immutable (JUX-LANG-V1 §7.6): its components are
                // set once by its constructor, and a changed copy comes from
                // `with(...)`.
                if let Some((record, component)) = self.record_component_write(&a.target) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0465_FinalFieldReassigned,
                            format!(
                                "cannot assign to `{component}`: `{record}` is a record, and a \
                                 record's components cannot change after construction (§7.6)",
                            ),
                        )
                        .with_span(a.span)
                        .with_help(format!(
                            "make a changed copy instead: `x.with({component}: value)`"
                        )),
                    );
                }
                if let Some(field) = self.final_field_assign_violation(&a.target) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0465_FinalFieldReassigned,
                            format!(
                                "cannot assign to `{field}`: it is a `final`/`const` field \
                                 and may only be set in its declaration or a constructor \
                                 (§5.6). Drop `final`/`const`, or move the assignment into \
                                 the constructor.",
                            ),
                        )
                        .with_span(a.span),
                    );
                }
                // `a += b` is `a = a + b` (§O.2.3), so the `+` has to exist. When
                // a free-function operator answers it (§7.14), record the pick
                // under the assignment's span, which the backend's desugared
                // `a + b` carries.
                if let Some(op) = a.op {
                    self.check_user_operator_defined(op, &a.target, &a.value, a.span);
                    if let Some(kind) = op_kind_for_binary(op) {
                        let target_ty = infer_expr(&a.target, &self.env, self.symbols);
                        let value_ty = infer_expr(&a.value, &self.env, self.symbols);
                        if let Some((key, k, _)) =
                            crate::infer::free_operator_for(self.symbols, kind, &target_ty, &value_ty, &self.env)
                        {
                            let bare = key.rsplit('.').next().unwrap_or(&key).to_string();
                            self.free_operator_calls.insert(a.span, (bare, k));
                            self.function_selections.insert(a.span, k);
                        }
                    }
                }
                let mut target_ty = infer_expr(&a.target, &self.env, self.symbols);
                // `cur = cur.next;` ending a `while` refinement (§T.6.5): the
                // slot is the declared `T?`, whatever `cur` read as above.
                if let (Some((name, declared)), Expr::Path(qn)) = (&self.loop_assign_widen, &a.target) {
                    if qn.segments.len() == 1 && qn.segments[0].text == *name {
                        target_ty = declared.clone();
                    }
                }
                // What actually gets STORED. For a plain `=` that is the
                // value; for a compound assignment it is the result of
                // `target op value`, which is a different type whenever the
                // operator is. `s += 1` on a String stores a String -- the
                // operand's own type was never the question, and asking it
                // rejected the compound form of an expression whose spelled-out
                // twin (`s = s + 1`) is accepted.
                let value_ty = match a.op {
                    Some(op) => infer_expr(
                        &Expr::Binary(juxc_ast::BinaryExpr {
                            op,
                            left: Box::new(a.target.clone()),
                            right: Box::new(a.value.clone()),
                            span: a.span,
                        }),
                        &self.env,
                        self.symbols,
                    ),
                    None => infer_expr(&a.value, &self.env, self.symbols),
                };
                let effective_target = if weak_target {
                    Ty::nullable(target_ty.clone())
                } else {
                    target_ty.clone()
                };
                // A raw-pointer target accepts the `null` literal (`this.ptr =
                // null;`, the FFI handle-reset idiom) — `null` is the sole `T*`
                // literal (§L.6.1). The erased `Ty` drops `ptr_depth`, so we read
                // the target's declared `TypeRef` to recognize the pointer slot.
                let ptr_null_ok = (matches!(a.value, Expr::Literal(juxc_ast::Literal::Null))
                    && self.assign_target_is_raw_pointer(&a.target))
                    || (a.op.is_none() && {
                        let target_depth = self.expr_ptr_depth(&a.target);
                        let target_void = crate::infer::pointer_base_is_void(&a.target, &self.env, self.symbols);
                        self.check_pointer_flow(target_depth, target_void, &target_ty, &a.value, a.span)
                    });
                if a.op.is_none() {
                    self.check_literal_fits(&effective_target, &a.value, a.span);
                }
                if !ptr_null_ok && !compatible(&effective_target, &value_ty, self.symbols) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!("cannot assign {value_ty} to {target_ty}"),
                        )
                        .with_span(a.span),
                    );
                }
            }

            Stmt::Return(opt, ret_span) => {
                // Clone the expected return type up front so we can
                // mutably borrow `self` to walk the expression below
                // without a borrow conflict on `current_return`.
                let expected = self.current_return.clone();
                // An array returned where the function returns `T[]` or `T[N]`.
                if let (Some(Ty::Array { kind, .. }), Some(e)) = (&expected, opt) {
                    self.note_array_slot(e, *kind == crate::ty::ArrayKind::Dynamic, expr_span(e));
                }
                // A returned lambda or method reference takes the function
                // type the method returns, as one stored into a typed local
                // does (`return this::greet;` picks the overload, §M.8.3).
                if let (Some(Ty::Fn { params, .. }), Some(Expr::Lambda(_) | Expr::MethodRef(_))) = (&expected, opt) {
                    self.lambda_slot_params = Some(params.clone());
                }
                match (&expected, opt) {
                    // Bare `return;` inside a void function — fine.
                    (Some(t), None) if t.is_void() => {}
                    // Bare `return;` outside any function — fine.
                    (None, None) => {}
                    // Bare `return;` in a value-returning function. Carry the
                    // statement span so the diagnostic reaches the IDE (a
                    // file-less diagnostic is dropped by the LSP).
                    (Some(exp), None) => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0410_TypeMismatch,
                                format!(
                                    "expected return value of type {exp}, found bare `return`",
                                ),
                            )
                            .with_span(*ret_span),
                        );
                    }
                    (_, Some(expr)) => {
                        self.check_expr(expr);
                        let found = infer_expr(expr, &self.env, self.symbols);
                        if let Some(exp) = &expected {
                            self.check_literal_fits(exp, expr, *ret_span);
                            let pointer_checked =
                                self.check_pointer_flow(self.current_return_ptr, self.current_return_void, exp, expr, *ret_span);
                            if !pointer_checked && !compatible(exp, &found, self.symbols) {
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
                let smartcast: Option<(String, Ty)> = if let Expr::TypeTest(t) = &if_stmt.condition
                {
                    self.check_typetest(t, true);
                    t.binder
                        .as_ref()
                        .map(|b| (b.text.clone(), ty_from_ref(&t.ty, &self.env, self.symbols)))
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
                // Null-test narrowing (§7.10). `x != null` proves it in the
                // then-branch, `x == null` proves it in the else -- and, when
                // the then-branch cannot fall through, for everything after
                // the `if` as well. That last form is the guard clause, and
                // it is the one this language could not previously express.
                // A branch narrows unless the BRANCH reassigns the name: the
                // value tested is the value the branch starts with. Code after
                // a guard clause is the rest of the enclosing block, so that
                // form asks the enclosing block.
                let then_assigned = crate::assigned::names_assigned_in(&if_stmt.then_block);
                let narrow_then = self.narrowings(&if_stmt.condition, true, &then_assigned);
                let else_assigned = match if_stmt.else_branch.as_deref() {
                    Some(ElseBranch::Block(b)) => crate::assigned::names_assigned_in(b),
                    _ => self.assigned_in_block.clone(),
                };
                let narrow_else = self.narrowings(&if_stmt.condition, false, &else_assigned);
                let enclosing_assigned = self.assigned_in_block.clone();
                let narrow_after = self.narrowings(&if_stmt.condition, false, &enclosing_assigned);

                self.env.push_scope();
                if let Some((name, ty)) = &smartcast {
                    self.env.declare(name, ty.clone());
                }
                for (name, ty) in &narrow_then {
                    self.env.declare(name, ty.clone());
                }
                self.check_block(&if_stmt.then_block);
                self.env.pop_scope();
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.env.push_scope();
                    for (name, ty) in &narrow_else {
                        self.env.declare(name, ty.clone());
                    }
                    self.check_else_branch(else_branch);
                    self.env.pop_scope();
                }
                // The guard clause. With no `else`, and a then-branch that
                // always leaves, the statements after this `if` are reachable
                // only when the test was false -- so the binding is non-null
                // for the rest of the enclosing block, which is the scope
                // this `declare` lands in.
                if if_stmt.else_branch.is_none()
                    && !crate::return_check::body_can_fall_through(&if_stmt.then_block)
                {
                    for (name, ty) in narrow_after {
                        self.env.declare(&name, ty);
                    }
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
                // `while (x != null)` refines `x` in the body (§T.6.5).
                let nothing_assigned = std::collections::HashSet::new();
                let narrowed = self.narrowings(&w.condition, true, &nothing_assigned);
                self.env.push_scope();
                let prev_loop = self.loop_narrowed.clone();
                for (name, ty) in &narrowed {
                    let declared = self.env.lookup(name).cloned().unwrap_or_else(|| Ty::nullable(ty.clone()));
                    self.env.declare(name, ty.clone());
                    self.loop_narrowed.push((name.clone(), declared));
                }
                self.check_block(&w.body);
                self.loop_narrowed = prev_loop;
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
                self.check_any_receiver(&f.iter, "cannot be iterated", f.span);
                // §18.6.3 async streams: `for await` is only legal in an
                // async context (it awaits `next()` per element), and the
                // iterable must be a Stream<T> — in both directions
                // (plain `for` over a stream has no sync protocol).
                let iter_is_stream = matches!(
                    &iter_ty,
                    Ty::User { name, .. }
                        if name.rsplit('.').next() == Some("Stream")
                            && !self.symbols.classes.contains_key("Stream")
                );
                if f.is_await {
                    if !self.in_async {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0703_ForAwaitRequiresAsyncContext,
                                "`for await` is only permitted inside an async function, method, or lambda -- it awaits the stream's `next()` for every element (§18.6.3)",
                            )
                            .with_span(f.span),
                        );
                    }
                    if !iter_is_stream && !matches!(iter_ty, Ty::Unknown) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0704_ForAwaitRequiresStream,
                                format!(
                                    "`for await` iterates a `Stream<T>`, found {iter_ty} -- use a plain `for` for synchronous iterables (§18.6.3)",
                                ),
                            )
                            .with_span(expr_span(&f.iter)),
                        );
                    }
                } else if iter_is_stream {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0704_ForAwaitRequiresStream,
                            "a `Stream<T>` has no synchronous iteration protocol -- use `for await (var x : …)` (§18.6.3)",
                        )
                        .with_span(expr_span(&f.iter)),
                    );
                }
                // Loop-var binding: explicit annotation wins; else
                // element-of-array if iter is an array; else the
                // stream's element type; else Unknown.
                let var_ty = if let Some(declared) = &f.var_type {
                    ty_from_ref(declared, &self.env, self.symbols)
                } else {
                    match &iter_ty {
                        Ty::Array { element, .. } => (**element).clone(),
                        // Stream<T> (§18.6): the element type is the
                        // single generic arg.
                        Ty::User { generic_args, .. } if iter_is_stream => {
                            generic_args.first().cloned().unwrap_or(Ty::Unknown)
                        }
                        // A `rust.std` sequence collection iterates over its
                        // element type (the first generic arg) — the stub
                        // exposes `iter()`, not the Jux `iterator()` protocol,
                        // so recognize these directly. (`HashMap`/`BTreeMap`
                        // iterate as key/value pairs and aren't covered here.)
                        Ty::User { name, generic_args }
                            if name.starts_with("rust.std")
                                && matches!(
                                    name.rsplit('.').next().unwrap_or(name),
                                    "Vec" | "VecDeque" | "HashSet" | "BTreeSet",
                                ) =>
                        {
                            generic_args.first().cloned().unwrap_or(Ty::Unknown)
                        }
                        // User iterable (§O.6/§K.5): the protocol's
                        // element type — `iterator()`'s Iterator<T>
                        // argument, or the iterator's own `next()`
                        // return with the `?` peeled.
                        Ty::User { name, .. } => {
                            self.iterable_element_type(name).unwrap_or(Ty::Unknown)
                        }
                        // `s.chars()` (CORE-LIB K.7) iterates a String's `char`s.
                        // Left untyped, `c - 'a'` in the body typed as whatever
                        // the other operand was.
                        _ => match &f.iter {
                            Expr::Call(call) if call.args.is_empty() => match call.callee.as_ref() {
                                Expr::Field(field)
                                    if matches!(infer_expr(&field.object, &self.env, self.symbols), Ty::String) =>
                                {
                                    match field.field.text.as_str() {
                                        "chars" => Ty::Primitive(Primitive::Char),
                                        _ => Ty::Unknown,
                                    }
                                }
                                _ => Ty::Unknown,
                            },
                            _ => Ty::Unknown,
                        },
                    }
                };
                // §O.7.3: the iterated value must be `Iterable<T>`. A number or
                // a `bool` never is, and reached Rust as "is not an iterator".
                if !f.is_await && matches!(iter_ty, Ty::Primitive(_)) {
                    let help = if matches!(iter_ty, Ty::Primitive(p) if crate::ty::integer_bits(p).is_some()) {
                        "to count, iterate a range: `for (var i : 0..n)`"
                    } else {
                        "iterate an array, a collection, or a range"
                    };
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0941_ConstraintNotSatisfied,
                            format!("a for-each needs an `Iterable<T>`, and {iter_ty} is not one (§O.7.3)"),
                        )
                        .with_span(f.var_name.span)
                        .with_help(help),
                    );
                }
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
                // `assert(x != null);` (§T.6.2): the rest of the block runs only
                // when the condition held, so the names it proves non-null are
                // declared non-null in the enclosing scope, as a guard clause's
                // are.
                if let Some(cond) = juxc_ast::assert_condition(e) {
                    let nothing_assigned = std::collections::HashSet::new();
                    for (name, ty) in self.narrowings(cond, true, &nothing_assigned) {
                        self.env.declare(&name, ty);
                    }
                }
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
                // S18 (§18.4 async lowering): an ASYNC try body (one
                // containing an await / `for await`) lowers to an
                // `async move` block that captures outer locals BY
                // VALUE — assigning to an outer primitive/String local
                // inside it would silently update the moved-in copy.
                // Reject with E0706 instead of miscompiling.
                if block_has_await_shallow(&t.body) {
                    let mut assigned: Vec<(String, Span)> = Vec::new();
                    let mut declared: std::collections::HashSet<String> =
                        std::collections::HashSet::new();
                    collect_async_try_writes(&t.body, &mut assigned, &mut declared);
                    for (name, span) in assigned {
                        if declared.contains(&name) {
                            continue;
                        }
                        let Some(ty) = self.env.lookup(&name) else {
                            continue;
                        };
                        if matches!(ty, Ty::Primitive(_) | Ty::String) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0706_AsyncTryMutatesOuterLocal,
                                    format!(
                                        "assignment to outer local `{name}` inside an async `try` updates a captured COPY (the async block captures by value) -- accumulate through a shared handle (`AtomicInt`/`AtomicLong`, a class field) or return the value out of the try",
                                    ),
                                )
                                .with_span(span),
                            );
                        }
                    }
                }
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
                    let mut tys: Vec<Ty> = vec![ty_from_ref(&c.ty, &self.env, self.symbols)];
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
                                             `{}` are in a subtype relationship -- keep only \
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

            // A bare `{ … }` is a scope and nothing more: check what is
            // inside, with no change to what is permitted there.
            Stmt::Block(b) => {
                self.env.push_scope();
                self.check_block(b);
                self.env.pop_scope();
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
            Stmt::Break(label, span) | Stmt::Continue(label, span) => {
                let is_break = matches!(stmt, Stmt::Break(..));
                if let Some(label) = label {
                    self.check_jump_label(&label.text, is_break, label.span.join(*span));
                }
            }
            Stmt::Labeled { label, stmt: inner } => {
                let is_loop = matches!(
                    inner.as_ref(),
                    Stmt::While(_) | Stmt::DoWhile(_) | Stmt::ForEach(_) | Stmt::ForC(_)
                );
                self.labels.push((label.text.clone(), is_loop));
                self.check_stmt(inner);
                self.labels.pop();
            }
        }
    }

    /// `var R(a, b) = value;` (§5.4), checked at the temporary the parser
    /// desugared it into (`juxc_ast::record_destructure_temp`). `declared` is
    /// the pattern's type `R`, `value` the initializer's. Returns the type the
    /// temporary takes: the value's own when it IS an `R` (so a generic
    /// record keeps its arguments), `R` otherwise.
    ///
    /// - `R` must be a record, with as many components as the pattern has
    ///   binders (E0439).
    /// - The value must always be an `R`: a declaration has no other branch to
    ///   fall to, so a pattern that can fail to match is E0271. A value typed
    ///   as a supertype, or nullable, can fail.
    fn check_record_destructure(&mut self, v: &juxc_ast::VarDecl, declared: &Ty, value: &Ty, arity: usize) -> Ty {
        let span = v.span;
        let Ty::User { name: record_name, .. } = declared else {
            return declared.clone();
        };
        let shown = crate::ty::nested_type_spelling(record_name.rsplit('.').next().unwrap_or(record_name)).into_owned();
        let Some(record) = self.symbols.records.get(record_name) else {
            if !matches!(declared, Ty::Unknown) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0439_PatternShapeMismatch,
                        format!("`var {shown}(...)` takes a record apart, and `{shown}` is not a record"),
                    )
                    .with_span(span),
                );
            }
            return declared.clone();
        };
        let components = record.components.len();
        if components != arity {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0439_PatternShapeMismatch,
                    format!(
                        "`{shown}` has {components} component{}, and this pattern gives {arity}",
                        if components == 1 { "" } else { "s" },
                    ),
                )
                .with_span(span),
            );
        }
        match value {
            Ty::User { name, .. } if name == record_name => value.clone(),
            Ty::Unknown => declared.clone(),
            other => {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0271_RefutableDestructuring,
                        format!(
                            "`var {shown}(...)` needs a value that is always a `{shown}`, and this one is a `{other}`: the pattern could fail to match, and a declaration has nowhere to go if it does",
                        ),
                    )
                    .with_span(span)
                    .with_help(format!(
                        "test it first with `switch` (`case {shown}(var a, ...) -> ...`) or `if (value => {shown} r)`",
                    )),
                );
                declared.clone()
            }
        }
    }

    /// `break name;` / `continue name;` (Grammar §A.2.8, E0241): the label
    /// must name an enclosing labeled statement, and `continue` must name a
    /// loop, since a labeled block has no next iteration to continue to.
    fn check_jump_label(&mut self, name: &str, is_break: bool, span: Span) {
        let keyword = if is_break { "break" } else { "continue" };
        match self.labels.iter().rev().find(|(l, _)| l == name) {
            None if self.labels_outside_closure.iter().any(|l| l == name) => self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0241_LabelMismatch,
                    format!(
                        "`{keyword} {name};` would have to leave the lambda it is written in, and a jump cannot: `{name}:` is outside the closure"
                    ),
                )
                .with_span(span),
            ),
            None => self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0241_LabelMismatch,
                    format!("`{keyword} {name};` names no enclosing statement: there is no `{name}:` around it"),
                )
                .with_span(span),
            ),
            Some((_, false)) if !is_break => self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0241_LabelMismatch,
                    format!(
                        "`continue {name};` needs a loop, and `{name}` labels a block: use `break {name};` to leave it"
                    ),
                )
                .with_span(span),
            ),
            Some(_) => {}
        }
    }

    /// Recurse through else / else-if chains, mirroring [`Self::check_stmt`]'s
    /// handling for the top-level `if`.
    fn check_else_branch(&mut self, branch: &ElseBranch) {
        match branch {
            ElseBranch::If(if_stmt) => {
                // Same type-test smart-cast handling as a top-level `if`, so
                // `else if (x => Dog d)` binds `d` in its then-branch.
                let smartcast: Option<(String, Ty)> = if let Expr::TypeTest(t) = &if_stmt.condition
                {
                    self.check_typetest(t, true);
                    t.binder
                        .as_ref()
                        .map(|b| (b.text.clone(), ty_from_ref(&t.ty, &self.env, self.symbols)))
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
                let then_assigned = crate::assigned::names_assigned_in(&if_stmt.then_block);
                let narrow_then = self.narrowings(&if_stmt.condition, true, &then_assigned);
                self.env.push_scope();
                if let Some((name, ty)) = &smartcast {
                    self.env.declare(name, ty.clone());
                }
                for (name, ty) in &narrow_then {
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
        // A fully-qualified class receiver (`demo.pkg.Crate.make()`) reads as a
        // chain of fields. Checked as a chain it typed as an unknown local and
        // nothing about the call was checked at all.
        if let Some(reshaped) = crate::infer::reshape_qualified_class_receiver(
            expr,
            &|n| self.env.lookup(n).is_some(),
            &|n| crate::infer::owner_type_fqn(n, &self.env, self.symbols),
            self.symbols,
        ) {
            return self.check_expr(&reshaped);
        }
        // **E0456 (§M.14.3)** — a bare read of a `weak` parameter. Its strong
        // view is reached only through `.get()` (→ `T?`); the legitimate
        // `.get()` receiver is intercepted in `check_call` and never reaches
        // here, and an assignment place is filtered by the assignment checker —
        // so any weak-param `Path` that lands here is a bare read in a value /
        // argument / return position, which would expose the raw `Weak<…>`.
        if let Expr::Path(qn) = expr {
            if qn.segments.len() == 1 && self.env.weak_names.contains(&qn.segments[0].text) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0456_WeakReadNeedsGet,
                        format!(
                            "`weak` parameter `{}` can only be read via `.get()` (which returns \
                             the target type, nullable) -- a bare read would expose the raw weak \
                             handle (§M.14.3)",
                            qn.segments[0].text,
                        ),
                    )
                    .with_span(qn.span),
                );
                return;
            }
        }
        // Record this expression's type up-front. Sub-expressions get
        // recorded when their containing check_expr recurses into them.
        let _ = self.infer_and_record(expr);
        match expr {
            Expr::Literal(_) => {}
            // `typeof(expr)` (§5.9.10) — the operand is type-checked
            // (undefined names etc. still report) but never evaluated.
            Expr::TypeOf(inner, _) => self.check_expr(inner),
            // `x ?: throw E` (§T.6.2): the thrown value is checked exactly as a
            // `throw` statement's is (E0710, checked raises).
            Expr::Throw(inner, span) => self.check_stmt_inner(&Stmt::Throw((**inner).clone(), *span)),
            // `out <place>` (§M.4) — recurse into the place so an undefined
            // variable etc. is still reported. The place/agreement rules are in
            // `check_call_args`; a bare `out` outside a call is meaningless but
            // harmless to walk.
            Expr::Out(inner, _) => self.check_expr(inner),
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
            // Record a bare-name reference so the E0453 "uninferable `new`"
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

            Expr::Field(f) if f.field.text == "length"
                && matches!(infer_expr(&f.object, &self.env, self.symbols), Ty::String) =>
            {
                self.check_expr(&f.object);
                // §S.3.2: "length" of a string is ambiguous -- bytes or
                // characters -- and guessing is where the bugs come from.
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0479_StringLengthAmbiguous,
                        "a String has two lengths: its bytes and its characters",
                    )
                    .with_span(f.span)
                    .with_help("write `s.byteLength` for the UTF-8 byte count, or `s.charLength` for the number of characters"),
                );
            }
            Expr::Field(f) => {
                self.check_expr(&f.object);
                // `v.x` on an `any` (§T.1.2): it has no fields.
                if self.check_any_receiver(&f.object, &format!("has no field `{}`", f.field.text), f.span) {
                    return;
                }
                self.check_nullable_receiver(f, false);
                self.check_field_access(f);
            }

            Expr::Index(i) => {
                self.check_expr(&i.array);
                self.check_expr(&i.index);
                self.check_any_receiver(&i.array, "cannot be indexed", i.span);
                // §S.3.2: a byte index and a character index are different
                // positions in a UTF-8 string, so `s[i]` says too little.
                if matches!(infer_expr(&i.array, &self.env, self.symbols), Ty::String) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0480_StringIndexAmbiguous,
                            "a String has no single index: bytes and characters are different positions in it",
                        )
                        .with_span(i.span)
                        .with_help("index the bytes with `s.bytes()[i]`, or take a character with `s.chars().nth(i)`"),
                    );
                }
                if !self.in_unsafe && self.expr_ptr_depth(&i.array) > 0 {
                    self.unsafe_pointer_op("indexing a raw pointer `p[i]`", i.span);
                }
            }

            Expr::Call(c) => self.check_call(c),

            Expr::NewObject(n) => self.check_new_object(n),

            Expr::NewArray(n) => {
                // Check every dimension's size (outer + inner dims of a
                // multi-dim `new T[a][b]`).
                self.check_expr(&n.size);
                for inner in &n.inner_sizes {
                    self.check_expr(inner);
                }
                // `new T[«size»]` is HEAPABLE: a non-const (or const-generic
                // arithmetic) size lowers to a heap `vec![..; size]`, so any
                // runtime size is allowed here — that's how a runtime-sized
                // buffer (`new int[w * h]`) is allocated (§5.6). Only a fixed
                // array *type* (`int[N] field;`) needs a const size; that's
                // guarded separately by `check_fixed_array_size_in_type`.
                // Const-fold panics / limit overruns still fire (see helper).
                // A size built only from literals has no span of its own --
                // `Expr::Literal` carries none, so `expr_span` is DUMMY and
                // the message prints at line 1. The enclosing `new` is the
                // nearest thing that does have one.
                self.check_const_size_expr_at(&n.size, true, n.span);
                for inner in &n.inner_sizes {
                    self.check_const_size_expr_at(inner, true, n.span);
                }
                // Every element starts at the element type's default value
                // (JUX-LANG-V1 §5.5), so the type needs one. Without this the
                // Rust compiler reported a missing `Default` on a type the
                // program never mentioned by that name.
                let element = ty_from_ref(&n.element_type, &self.env, self.symbols);
                if !crate::defaults::ty_has_default(&element, self.symbols) {
                    let written = type_ref_display(&n.element_type);
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0458_ArrayElementHasNoDefault,
                            format!(
                                "`new {written}[…]` has nothing to start its elements from: \
                                 `{written}` has no default value -- list the elements with \
                                 `new {written}[]{{…}}`, or collect them in a `Vec<{written}>`",
                            ),
                        )
                        .with_span(n.span),
                    );
                }
            }

            Expr::NewArrayLit(n) => {
                for el in &n.elements {
                    self.check_expr(el);
                }
            }

            Expr::Cast(c) => {
                self.check_expr(&c.value);
                self.check_reference_cast(c);
                self.check_string_cast(c);
                // `p as fn(A) -> R` and `f as void*` reinterpret a code address
                // (§L.6.4), which nothing can check.
                let from_fn_pointer =
                    matches!(infer_expr(&c.value, &self.env, self.symbols), Ty::FnPtr { .. });
                if !self.in_unsafe && (c.ty.fn_pointer_shape().is_some() || from_fn_pointer) {
                    self.unsafe_pointer_op("a cast to or from a function pointer", c.span);
                } else if !self.in_unsafe && (c.ty.ptr_depth > 0 || self.expr_ptr_depth(&c.value) > 0) {
                    // §L.5.2 items 3-4: reinterpreting an address.
                    self.unsafe_pointer_op("a cast to or from a raw pointer", c.span);
                }
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
                self.check_user_range_operator(r);
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
                                "`step` ranges are only supported as for-each iterables in Phase 1 -- `for (var i : a..b step s)`",
                            )
                            .with_span(r.span),
                        );
                    }
                }
            }

            Expr::Unary(u) => {
                self.check_expr(&u.operand);
                self.check_any_receiver(&u.operand, "has no operators", u.span);
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
                match u.op {
                    juxc_ast::UnaryOp::AddrOf => self.check_address_of(u),
                    juxc_ast::UnaryOp::Deref => self.check_deref(u),
                    juxc_ast::UnaryOp::Neg => self.check_nullable_operand(&u.operand, "-", u.span),
                    juxc_ast::UnaryOp::Not => self.check_nullable_operand(&u.operand, "!", u.span),
                    juxc_ast::UnaryOp::BitNot => self.check_nullable_operand(&u.operand, "~", u.span),
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
                // §7.10: `&&` reads its right side only when the left was
                // true, `||` only when it was false, so a null test on the
                // left narrows the right.
                let nothing_assigned = std::collections::HashSet::new();
                let proven = match b.op {
                    BinaryOp::And => self.narrowings(&b.left, true, &nothing_assigned),
                    BinaryOp::Or => self.narrowings(&b.left, false, &nothing_assigned),
                    _ => Vec::new(),
                };
                self.check_narrowed(&proven, |this| this.check_expr(&b.right));
                self.check_numeric_operands(b);
                self.check_nullable_operands(b);
                self.check_comparison_chain(b);
                self.check_pointer_operators(b);
                self.check_any_binary(b);
                if !self.in_unsafe
                    && matches!(b.op, juxc_ast::BinaryOp::Add | juxc_ast::BinaryOp::Sub)
                    && (self.expr_ptr_depth(&b.left) > 0 || self.expr_ptr_depth(&b.right) > 0)
                {
                    // Point at an operand: a binary with a literal on one side
                    // joins that literal's empty span and stretches back to
                    // the start of the file.
                    let span = [expr_span(&b.left), expr_span(&b.right), b.span]
                        .into_iter()
                        .find(|sp| *sp != Span::DUMMY)
                        .unwrap_or(b.span);
                    self.unsafe_pointer_op("pointer arithmetic", span);
                }
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
                // `k * v` answered by a free-function operator (§7.14): the
                // backend calls it, so remember which one.
                if let Some(kind) = op_kind_for_binary(b.op) {
                    let left_ty = infer_expr(&b.left, &self.env, self.symbols);
                    let right_ty = infer_expr(&b.right, &self.env, self.symbols);
                    if let Some((key, k, _)) =
                        crate::infer::free_operator_for(self.symbols, kind, &left_ty, &right_ty, &self.env)
                    {
                        let bare = key.rsplit('.').next().unwrap_or(&key).to_string();
                        self.free_operator_calls.insert(b.span, (bare, k));
                        // The call the backend emits carries the binary's span,
                        // so the overload pick is recorded the way a written
                        // call's is.
                        self.function_selections.insert(b.span, k);
                    }
                }
                // An operator a user type does not declare (§O.2.6).
                self.check_user_operator_defined(b.op, &b.left, &b.right, b.span);
                // §O.3.4 — binary operator on a user type whose
                // matching operator was deleted with `= delete;`.
                // The receiver is the LHS; that's what determines
                // dispatch per §O.2.6.
                if let Some(kind) = op_kind_for_binary(b.op) {
                    let receiver_ty = infer_expr(&b.left, &self.env, self.symbols);
                    self.check_op_not_deleted(&receiver_ty, kind, b.span);
                }
            }

            Expr::SizeOf(s) => match &s.type_operand {
                Some(t) => self.check_sizeof_type(t, s.span),
                None => {
                    self.check_expr(&s.operand);
                    // A bare type name (`sizeof(Box)`, §5.9.3 rule 2) that is
                    // not a binding: the unbound-generic rule applies to it too.
                    if let Expr::Path(qn) = s.operand.as_ref() {
                        if qn.segments.len() == 1 && self.env.lookup(&qn.segments[0].text).is_none() {
                            let t = juxc_ast::TypeRef {
                                name: qn.clone(),
                                generic_args: Vec::new(),
                                nullable: false,
                                array_shape: None,
                                fn_shape: None,
                                ptr_depth: 0,
                                span: qn.span,
                            };
                            self.check_sizeof_type(&t, s.span);
                        }
                    }
                }
            },

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
                            if matches!(ty, Ty::Void) {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0512_VoidInInterpolation,
                                        "this interpolated expression returns `void`, so there is no value to put in the string (§S.3.5)",
                                    )
                                    .with_span(expr_span(e))
                                    .with_help("call it on its own line, and interpolate a value instead"),
                                );
                            }
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
                // A tuple or record value is taken apart by tuple and record
                // patterns (§A.3), which get their shape checked and their
                // bindings typed; its exhaustiveness is the product rule. An
                // enum variant with a payload binds its parts the same way
                // (`case Level.Warn(var v)`), so `v` has the payload's type in
                // the guard and body instead of none.
                let scrutinee_ty = infer_expr(&s.scrutinee, &self.env, self.symbols);
                let product = self.is_product_ty(&scrutinee_ty);
                // An `any` is matched by what it holds: `case int n ->`,
                // `case Dog d ->`, `default ->` (§T.1.2). A value pattern would
                // compare, and an `any` has no `==`.
                if scrutinee_ty.is_any() {
                    for arm in &s.arms {
                        if !matches!(arm.pattern, Pattern::TypeBind { .. } | Pattern::Wildcard(_) | Pattern::Bind(_)) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0489_AnyHasNoOperation,
                                    "an `any` is matched by type, not by value",
                                )
                                .with_span(arm.pattern.span())
                                .with_help("write a type pattern, `case int n ->` or `case Dog d ->`, and test the value inside it (§T.1.2)"),
                            );
                        }
                    }
                }
                for arm in &s.arms {
                    let destructures = product
                        || matches!(&arm.pattern, Pattern::Tuple(..))
                        || matches!(&arm.pattern, Pattern::EnumVariant { path, .. }
                            if self.pattern_record_fqn(path).is_some())
                        || matches!(&arm.pattern, Pattern::EnumVariant { args, .. } if !args.is_empty())
                        || matches!(&arm.pattern, Pattern::TypeBind { .. });
                    let mut bindings = Vec::new();
                    if destructures {
                        self.check_pattern_shape(&arm.pattern, &scrutinee_ty, &mut bindings);
                    }
                    self.env.push_scope();
                    for (name, ty) in bindings {
                        self.expr_types.insert(name.span, ty.clone());
                        self.env.declare(&name.text, ty);
                    }
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
                    // arm's scope declared above, so the guard and a
                    // block body both see them.
                    if let Some(g) = &arm.guard {
                        self.check_expr(g);
                        let guard_ty = infer_expr(g, &self.env, self.symbols);
                        if !is_boolish(&guard_ty) && !matches!(guard_ty, Ty::Unknown) {
                            let guard_span = expr_span(g);
                            let span = if guard_span.start == guard_span.end { arm.span } else { guard_span };
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0272_GuardNotBool,
                                    format!("a `when` guard must be a `bool`, found {guard_ty}"),
                                )
                                .with_span(span)
                                .with_help("write the condition the arm needs, such as `when x > 5`"),
                            );
                        }
                    }
                    match &arm.body {
                        SwitchBody::Expr(e) => self.check_expr(e),
                        SwitchBody::Block(b) => {
                            self.env.push_scope();
                            // Checked like any block, not only walked for its
                            // declarations: its expressions need their types
                            // (a collection field read in `case 1 -> { return
                            // vars.len(); }` otherwise had none) and their errors.
                            self.check_block(b);
                            self.env.pop_scope();
                        }
                    }
                    self.env.pop_scope();
                }
                if product {
                    self.check_product_switch_exhaustive(s, &scrutinee_ty);
                    return;
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
                let slot_params = self.lambda_slot_params.take();
                for (i, p) in l.params.iter().enumerate() {
                    let ty = match &p.ty {
                        Some(t) => ty_from_ref(t, &self.env, self.symbols),
                        None => match slot_params.as_ref().and_then(|ps| ps.get(i)) {
                            Some(slot) => {
                                self.expr_types.insert(p.name.span, slot.clone());
                                slot.clone()
                            }
                            None => Ty::Unknown,
                        },
                    };
                    self.env.declare(&p.name.text, ty);
                }
                let saved_return = self.current_return.take();
                // A lambda introduces its OWN async context: an async lambda
                // (`async (x) -> …`) permits `await`; a plain lambda inside an
                // async function does NOT (§18.1.2).
                let saved_async = self.in_async;
                self.in_async = l.is_async;
                // A jump can't leave the closure, so no outer label is visible.
                let saved_labels = std::mem::take(&mut self.labels);
                let hidden = saved_labels.len();
                self.labels_outside_closure.extend(saved_labels.iter().map(|(l, _)| l.clone()));
                match &l.body {
                    juxc_ast::LambdaBody::Expr(e) => self.check_expr(e),
                    juxc_ast::LambdaBody::Block(b) => self.check_block(b),
                }
                let keep = self.labels_outside_closure.len() - hidden;
                self.labels_outside_closure.truncate(keep);
                self.labels = saved_labels;
                self.in_async = saved_async;
                self.current_return = saved_return;
                self.lambda_depth -= 1;
                self.env.pop_scope();
            }
            Expr::Elvis(e) => {
                self.check_expr(&e.value);
                self.check_expr(&e.fallback);
                // **The fallback has to fit the value's inner type (§7.10).**
                // `n ?? "zero"` on an `int?` used to lower to
                // `n.unwrap_or("zero".to_string())` and reach the user as a
                // Rust error about `isize` and `String` -- a type error in
                // their program, reported by a compiler they did not run.
                let value_ty = infer_expr(&e.value, &self.env, self.symbols);
                let Ty::Nullable(inner) = value_ty else {
                    // Not nullable: redundant operator, nothing to check.
                    return;
                };
                let fallback_ty = infer_expr(&e.fallback, &self.env, self.symbols);
                // A nullable fallback is compared on its inner type; either
                // side being null is exactly what the operator is for.
                let fallback_inner = match &fallback_ty {
                    Ty::Nullable(f) => (**f).clone(),
                    other => other.clone(),
                };
                if matches!(fallback_inner, Ty::Void) {
                    // `x ?? f()` where `f` returns nothing: the void is the
                    // complaint, and E0512 already covers it at its own site.
                    return;
                }
                if !compatible(&inner, &fallback_inner, self.symbols) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!(
                                "the fallback of `??` has type `{}`, which does not fit the \
                                 value's type `{}` -- `a ?? b` yields `a` without its null, so \
                                 `b` must be assignable to that (§7.10)",
                                fallback_ty.display(),
                                inner.display(),
                            ),
                        )
                        // A literal fallback carries a dummy span, so fall
                        // back to the operator's own -- pointing at the whole
                        // `a ?? b` beats pointing at the top of the file.
                        // A literal fallback carries an empty span, so fall
                        // back to the operator's own and then to the value's --
                        // pointing at the whole `a ?? b`, or at what produced
                        // the type, beats pointing at the top of the file.
                        .with_span(
                            [expr_span(&e.fallback), expr_span(&e.value), e.span]
                                .into_iter()
                                .find(|sp| sp.end > sp.start)
                                .unwrap_or(e.span),
                        ),
                    );
                }
            }
            Expr::MethodRef(m) => {
                let slot = self.lambda_slot_params.take();
                self.check_method_ref(m, slot);
            }
            Expr::Ternary(t) => {
                self.check_expr(&t.condition);
                let nothing_assigned = std::collections::HashSet::new();
                let when_true = self.narrowings(&t.condition, true, &nothing_assigned);
                let when_false = self.narrowings(&t.condition, false, &nothing_assigned);
                self.check_narrowed(&when_true, |this| this.check_expr(&t.then_branch));
                self.check_narrowed(&when_false, |this| this.check_expr(&t.else_branch));
                // Condition must be `bool`. Branches should
                // unify; Phase 1 keeps the unification check
                // permissive and lets rustc surface a real
                // mismatch on the emitted `if`.
                let cond_ty = infer_expr(&t.condition, &self.env, self.symbols);
                if !compatible(&Ty::Primitive(Primitive::Bool), &cond_ty, self.symbols) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!("ternary condition must be bool, found {cond_ty}",),
                        )
                        .with_span(expr_span(&t.condition)),
                    );
                }
            }
            // `expr!!` — walk the asserted operand; the null-or-not
            // outcome is a runtime property (NullPointerException), not a
            // static one, so no extra diagnostic fires here.
            Expr::NotNullAssert(inner, _) => self.check_expr(inner),
            // `++place` / `place++` (§A `incdec`, value form). Two
            // validations, mirroring the statement-form rules but at
            // expression level:
            //   1. the operand must be an ASSIGNABLE place — a name,
            //      array element, or field (E0200 otherwise; same gate
            //      `make_incdec` applies in the parser, re-checked here
            //      because a place may also arrive via desugaring).
            //   2. the operand's type must be a NUMERIC primitive —
            //      `++` on a `String`/`bool`/class is meaningless
            //      (E0200). `Unknown` stays lenient (an upstream
            //      inference gap must never manufacture a type error).
            Expr::IncDec(i) => {
                self.check_expr(&i.target);
                if self.check_any_receiver(&i.target, "has no operators", i.span) {
                    return;
                }
                if !self.in_unsafe && self.expr_ptr_depth(&i.target) > 0 {
                    self.unsafe_pointer_op("stepping a raw pointer with `++` / `--`", i.span);
                }
                if !Self::is_assignable_place(&i.target) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "`++`/`--` requires an assignable place (a name, array element, or field)",
                        )
                        .with_span(i.span),
                    );
                } else {
                    let ty = infer_expr(&i.target, &self.env, self.symbols);
                    // Reject non-numeric known types; `char`/`bool` are
                    // excluded by `is_numeric`. Unknown is tolerated.
                    if !matches!(ty, Ty::Unknown) && !ty.is_numeric() {
                        let op = if i.is_inc { "`++`" } else { "`--`" };
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0200_UnexpectedToken,
                                format!(
                                    "{op} applies only to numeric values, but the operand has type `{}`",
                                    ty.display(),
                                ),
                            )
                            .with_span(i.span),
                        );
                    }
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
                             lambda -- mark the enclosing function `async` (e.g. `async T f()`)",
                        )
                        .with_span(*span),
                    );
                }
                // The operand's static type is the operand's type (so a
                // `Future<T>` shape unwraps to `T` in inference); formal
                // Future-typing lands when async types are modelled properly.
                // The operand is THE future-consuming position (E0705).
                let prev_slot = self.in_future_slot;
                self.in_future_slot = true;
                self.check_expr(inner);
                self.in_future_slot = prev_slot;
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
            crate::infer::path_resolves_to_class(qn, &self.env, self.symbols).or_else(|| {
                match infer_expr(&f.object, &self.env, self.symbols) {
                    Ty::User { name, .. } => self.resolve_class_fqn(&name),
                    _ => None,
                }
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
                        "cannot assign to read-only property `{prop_name}` of `{class_fqn}` -- it has no `set` accessor (settable only in the constructor)",
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
                        "cannot assign to init-only property `{prop_name}` of `{class_fqn}` after construction -- `init` accessors are settable only during construction",
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
                            "cannot write property `{prop_name}` of `{class_fqn}` {ctx} -- its setter is `{word}`",
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
    fn write_visibility_allowed(&self, vis: juxc_ast::Visibility, declaring_class: &str) -> bool {
        use juxc_ast::Visibility;
        let accessor = self.env.current_class.as_deref();
        // The same ladder [`Self::check_visibility`] applies to reads, so an
        // accessor that can SEE a member can also write it when the setter's
        // own visibility permits. Keeping the two in step matters: a nested
        // class that could read its owner's private property but not write it
        // would be an inconsistency with no rule behind it.
        match vis {
            Visibility::Public => true,
            Visibility::Private => {
                accessor.is_some_and(|a| self.shares_top_level_owner(a, declaring_class))
            }
            Visibility::Protected => {
                accessor.is_some_and(|a| {
                    a == declaring_class
                        || crate::ty::walk_extends_reaches(a, declaring_class, self.symbols)
                        || self.shares_top_level_owner(a, declaring_class)
                }) || self.same_package_as(declaring_class)
            }
            Visibility::Package | Visibility::Internal => self.same_package_as(declaring_class),
        }
    }

    /// Resolve a (possibly bare) class name to its FQN key in the
    /// symbol table. Direct hit first, then a last-segment scan.
    fn resolve_class_fqn(&self, name: &str) -> Option<String> {
        // Qualified names are their own answer; a BARE name is resolved in this
        // unit's context before it can match a no-package class of the same
        // name (see the backend's `resolve_bare_class_fqn` for the bug the
        // other order caused).
        if name.contains('.') && self.symbols.classes.contains_key(name) {
            return Some(name.to_string());
        }
        // Package-aware resolution: a bare name can match several FQNs (two
        // packages each declaring `Foo`, or a user class colliding with an
        // auto-loaded `rust.std` stub). Resolve in THIS unit's context first —
        // (1) the `unqualified` bare→FQN map (same-package siblings + imports /
        // aliases), then (2) the current package prefix — so a bare `Foo` binds
        // to the referrer's package, never another's.
        if let Some(fqn) = self.env.unqualified.get(name) {
            if self.symbols.classes.contains_key(fqn) {
                return Some(fqn.clone());
            }
        }
        if !self.env.current_package.is_empty() {
            let cand = format!("{}.{}", self.env.current_package.join("."), name);
            if self.symbols.classes.contains_key(&cand) {
                return Some(cand);
            }
        }
        if self.symbols.classes.contains_key(name) {
            return Some(name.to_string());
        }
        // Fallback (unqualified cross-package reference): prefer a non-`external`
        // (user) class so user code shadows a stub, then break ties by FQN so
        // the result is deterministic across `HashMap` iteration orders.
        self.symbols
            .classes
            .iter()
            .filter(|(k, _)| k.rsplit('.').next().unwrap_or(k.as_str()) == name)
            .min_by(|a, b| {
                a.1.is_external
                    .cmp(&b.1.is_external)
                    .then_with(|| a.0.cmp(b.0))
            })
            .map(|(k, _)| k.clone())
    }

    /// Validate a `weak` field (§6.5). Phase-1 rules:
    /// - the target type must resolve to a **non-generic class** (weak
    ///   storage is `Weak<RefCell<Target_Inner>>`, which only exists for
    ///   reference-semantics classes) — else **E0455**;
    /// - the field may not carry an initializer (weak fields default to an
    ///   empty `Weak` and are wired by later assignment) — else **E0456**.
    fn check_weak_field(&mut self, field: &juxc_ast::FieldDecl) {
        if field.default.is_some() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0456_WeakReadNeedsGet,
                    format!(
                        "`weak` field `{}` may not have an initializer -- weak fields \
                         default to null and are assigned later (§6.5)",
                        field.name.text,
                    ),
                )
                .with_span(field.span),
            );
        }
        let target_ok = field
            .ty
            .as_ref()
            .is_some_and(|t| self.type_ref_is_weakable_class(t));
        if !target_ok {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0455_WeakOnNonClass,
                    format!(
                        "`weak` field `{}` must have a non-generic class type -- `weak` \
                         breaks a refcount cycle and applies only to class references (§6.5)",
                        field.name.text,
                    ),
                )
                .with_span(field.ty.as_ref().map_or(field.span, |t| t.span)),
            );
        }
    }

    /// **E0455 (§M.14.3)** — a `weak` parameter's declared type must be a
    /// non-generic class (the only weakable target, like a `weak` field). Reuses
    /// [`Self::type_ref_is_weakable_class`].
    fn check_weak_param(&mut self, param: &juxc_ast::Param) {
        if !self.type_ref_is_weakable_class(&param.ty) {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0455_WeakOnNonClass,
                    format!(
                        "`weak` parameter `{}` must have a non-generic class type -- `weak` is a \
                         reference to a class object, reached via `.get()` (§M.14.3)",
                        param.name.text,
                    ),
                )
                .with_span(param.ty.span),
            );
        }
    }

    /// True when `target` is an assignment place that writes a **`weak`**
    /// field (§6.5) — `recv.weakField = …`. Used by the assignment checker to
    /// (a) suppress the bare-weak-read guard on the write place and (b) treat
    /// the slot as nullable for the assignability check (a weak field accepts
    /// a target-class value or `null`).
    fn assign_target_is_weak_field(&self, target: &juxc_ast::Expr) -> bool {
        if let juxc_ast::Expr::Field(f) = target {
            let recv = infer_expr(&f.object, &self.env, self.symbols);
            if let Ty::User { name, .. } = &recv {
                return self
                    .symbols
                    .lookup_field(name, &f.field.text)
                    .is_some_and(|(fs, _)| fs.is_weak);
            }
        }
        false
    }

    /// True when an assignment target names a PROPERTY rather than a field.
    ///
    /// A property write is enforced by [`Self::check_property_write`], which
    /// reports the accessor and its visibility (E0970/E0972). Walking the same
    /// target as a read as well produced a second, vaguer error for one
    /// mistake.
    fn assign_target_is_property(&self, target: &juxc_ast::Expr) -> bool {
        let juxc_ast::Expr::Field(f) = target else {
            return false;
        };
        let recv = infer_expr(&f.object, &self.env, self.symbols);
        let Ty::User { name, .. } = &recv else {
            return false;
        };
        self.symbols
            .lookup_method(name, &f.field.text)
            .is_some_and(|(m, _)| m.is_property)
            || self.symbols.lookup_interface_property(name, &f.field.text).is_some()
    }

    /// `(record, component)` when `target` writes a component of a record
    /// value: `v.x = ...` on a record-typed receiver, or a bare component name
    /// inside one of the record's own methods.
    fn record_component_write(&self, target: &juxc_ast::Expr) -> Option<(String, String)> {
        let bare = |fqn: &str| fqn.rsplit('.').next().unwrap_or(fqn).to_string();
        match target {
            Expr::Field(f) => {
                let Ty::User { name, .. } = infer_expr(&f.object, &self.env, self.symbols) else {
                    return None;
                };
                let (fqn, record) = self
                    .symbols
                    .records
                    .get_key_value(&name)
                    .or_else(|| self.symbols.records.iter().find(|(k, _)| k.rsplit('.').next() == Some(name.as_str())))?;
                record
                    .components
                    .iter()
                    .any(|c| c.name == f.field.text)
                    .then(|| (bare(fqn), f.field.text.clone()))
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = &qn.segments[0].text;
                if self.env.lookup(name).is_some() {
                    return None;
                }
                let current = self.env.current_class.as_deref()?;
                let record = self.symbols.records.get(current)?;
                record
                    .components
                    .iter()
                    .any(|c| &c.name == name)
                    .then(|| (bare(current), name.clone()))
            }
            _ => None,
        }
    }

    /// When `target` is a **disallowed** write to a `final`/`const` field,
    /// return that field's name to flag (E0465); otherwise `None`.
    ///
    /// A `final` field is assign-once: legal only in its declaration initializer
    /// (not an assignment statement, so never seen here) or in a constructor /
    /// `init` block of its OWN object. This resolves both `obj.x` / `this.x`
    /// (`Expr::Field`) and the bare `x = …` (implicit `this.x`, `Expr::Path`)
    /// forms. Exemptions, to stay false-positive-free:
    /// - a bare name that is a local/param is NOT a field write (that path is the
    ///   E0464 local-binding check) — skipped via `env.lookup`;
    /// - `weak` fields use store-through write semantics (§6.5), handled
    ///   separately, so they never trip E0465.
    ///
    /// Allowance is deliberately permissive inside a constructor / init block (it
    /// does not enforce Java's once-only / declaring-class-only rules there —
    /// definite-assignment E0600 governs required init), which avoids false
    /// positives on legitimate construction-time writes. A `static final` field
    /// has no per-instance constructor, so it is settable only in a `static`
    /// init block.
    fn final_field_assign_violation(&self, target: &juxc_ast::Expr) -> Option<String> {
        let (name, is_final, is_weak, is_static, recv_is_this) = match target {
            Expr::Field(f) => {
                // Resolve the declaring class — instance (`obj.x` / `this.x`) or
                // static (`ClassName.x`) — the same way property-write
                // resolution does, so a qualified static-final write is caught.
                let class_fqn = if let Expr::Path(qn) = f.object.as_ref() {
                    crate::infer::path_resolves_to_class(qn, &self.env, self.symbols).or_else(
                        || match infer_expr(&f.object, &self.env, self.symbols) {
                            Ty::User { name, .. } => self.resolve_class_fqn(&name),
                            _ => None,
                        },
                    )
                } else {
                    match infer_expr(&f.object, &self.env, self.symbols) {
                        Ty::User { name, .. } => self.resolve_class_fqn(&name),
                        _ => None,
                    }
                };
                let class_fqn = class_fqn?;
                let (fs, _) = self.symbols.lookup_field(&class_fqn, &f.field.text)?;
                (
                    f.field.text.clone(),
                    fs.is_final,
                    fs.is_weak,
                    fs.is_static,
                    matches!(&*f.object, Expr::This(_)),
                )
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let nm = qn.segments[0].text.clone();
                // A local/param of this name shadows the field: the write is a
                // binding reassignment (E0464), not a field write.
                if self.env.lookup(&nm).is_some() {
                    return None;
                }
                let class = self.env.current_class.clone()?;
                let (fs, _) = self.symbols.lookup_field(&class, &nm)?;
                (nm, fs.is_final, fs.is_weak, fs.is_static, true)
            }
            _ => return None,
        };
        if !is_final || is_weak {
            return None;
        }
        let allowed = if is_static {
            self.in_init_block
        } else {
            (self.current_ctor.is_some() || self.in_init_block) && recv_is_this
        };
        if allowed {
            None
        } else {
            Some(name)
        }
    }

    /// True when an assignment target is a **raw-pointer field** (`obj.ptr`
    /// where `ptr` is declared `T*`). Used to accept `obj.ptr = null` (§L.6.1).
    /// The erased `Ty` drops `ptr_depth`, so we read the field's declared
    /// `TypeRef`. (A bare-local pointer reassign isn't covered: `env` only keeps
    /// the erased `Ty`; the local DECLARATION `T* p = null;` is handled in the
    /// var-decl check.)
    fn assign_target_is_raw_pointer(&self, target: &juxc_ast::Expr) -> bool {
        if let juxc_ast::Expr::Field(f) = target {
            let recv = infer_expr(&f.object, &self.env, self.symbols);
            if let Ty::User { name, .. } = &recv {
                return self
                    .symbols
                    .lookup_field(name, &f.field.text)
                    .is_some_and(|(fs, _)| fs.ty.ptr_depth > 0);
            }
        }
        false
    }

    /// True iff `tref` denotes a plain, non-generic **class** type — the only
    /// valid target of a `weak` field in Phase 1 (§6.5 / E0455). Rejects
    /// nullable, array, function, pointer, and generic-applied forms outright,
    /// and any name that resolves to an interface / record / enum / struct /
    /// type-parameter (only the `classes` table is consulted) or to a class
    /// that is itself generic.
    fn type_ref_is_weakable_class(&self, tref: &juxc_ast::TypeRef) -> bool {
        if tref.nullable
            || tref.array_shape.is_some()
            || tref.fn_shape.is_some()
            || tref.ptr_depth > 0
            || !tref.generic_args.is_empty()
        {
            return false;
        }
        let Some(seg) = tref.name.segments.last() else {
            return false;
        };
        if self.env.generic_params.contains(seg.text.as_str()) {
            return false;
        }
        match self.resolve_class_fqn(&seg.text) {
            Some(fqn) => self
                .symbols
                .classes
                .get(&fqn)
                .is_some_and(|c| c.generic_params.is_empty()),
            None => false,
        }
    }

    /// The enclosing TOP LEVEL type of `name`, for the JLS §6.6.1 `private`
    /// rule.
    ///
    /// A nested type is lifted to a flat name joined with `__` during
    /// parsing (`compilation.rs`: `Config` inside `HttpServer` becomes
    /// `HttpServer__Config`, recursively for deeper levels), which is the
    /// same convention `ty::resolve` walks outward. So the top-level owner
    /// is the segment before the FIRST `__`.
    ///
    /// A top-level type whose own name contains `__` would be
    /// indistinguishable, so the prefix is only believed when it names a
    /// type that actually exists; otherwise the name is already top-level.
    fn top_level_owner<'n>(&self, name: &'n str) -> &'n str {
        match name.split_once("__") {
            Some((outer, _)) if self.symbols.is_type_name(outer) => outer,
            _ => name,
        }
    }

    /// True when two type names live under the same top-level type — either
    /// because they are the same type, or because one nests inside the other,
    /// or because both nest under a common owner.
    fn shares_top_level_owner(&self, a: &str, b: &str) -> bool {
        a == b || self.top_level_owner(a) == self.top_level_owner(b)
    }

    /// True when the access site is in the same package as `declaring_class`.
    ///
    /// Both packages come from `ClassSig::package`, stamped from each unit's
    /// `package foo.bar;` line during `build_workspace`. Top-level code (no
    /// current class) still belongs to its UNIT's package, so a free `main()`
    /// reaches package-private members of its own package.
    fn same_package_as(&self, declaring_class: &str) -> bool {
        let declaring_pkg: &[String] = self
            .symbols
            .classes
            .get(declaring_class)
            .map(|c| c.package.as_slice())
            .unwrap_or(&[]);
        let accessor_pkg: &[String] = self
            .env
            .current_class
            .as_deref()
            .and_then(|name| self.symbols.classes.get(name))
            .map(|c| c.package.as_slice())
            .unwrap_or(&self.env.current_package);
        declaring_pkg == accessor_pkg
    }

    /// JLS §6.6.2.1 — the qualifier rule for `protected` across a package
    /// boundary.
    ///
    /// Inside a subclass `S` of `C` declared in a DIFFERENT package from `C`, a
    /// `protected` INSTANCE member of `C` may only be reached through a
    /// qualifier whose static type is `S` or a subclass of `S`. So `this.m` and
    /// `s.m` for an `s` typed `S` are fine, while `c.m` for a `c` typed `C` is
    /// not — even though `S` inherits `m`.
    ///
    /// `protected` means "code responsible for the implementation of this
    /// object", and a subclass is responsible for instances of ITSELF, not for
    /// an arbitrary sibling subclass that merely shares `C` as an ancestor.
    ///
    /// Deliberately narrow, because it is a TIGHTENING: same-package access is
    /// untouched (there `protected` already grants package access), `this` and
    /// `super` are untouched, and a static member has no receiver to be
    /// responsible for.
    /// E0413 for a method a bounded type parameter's bounds do not provide.
    /// Silent when the parameter has no bound or any bound is not a class or
    /// interface of this program, since then the member surface is unknown.
    fn check_method_on_bounded_param(&mut self, param: &str, method_name: &str, span: Span) {
        let Some(bounds) = self.env.generic_bounds.get(param).cloned() else { return };
        let mut bound_names = Vec::new();
        for bound in &bounds {
            match ty_from_ref(bound, &self.env, self.symbols) {
                Ty::User { name, .. }
                    if self.symbols.classes.contains_key(&name) || self.symbols.interfaces.contains_key(&name) =>
                {
                    bound_names.push(name)
                }
                _ => return,
            }
        }
        if bound_names.is_empty() {
            return;
        }
        let provided = bound_names.iter().any(|b| {
            self.symbols.lookup_method(b, method_name).is_some() || self.interface_provides_method(b, method_name)
        });
        if provided {
            return;
        }
        let shown: Vec<String> = bound_names
            .iter()
            .map(|b| format!("`{}`", b.rsplit('.').next().unwrap_or(b)))
            .collect();
        let hint = self.nearest_method_hint(&bound_names[0], method_name);
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0413_UnresolvedMethod,
                format!(
                    "no method `{method_name}` on type parameter `{param}`, whose bound {} does not declare it{hint}",
                    shown.join(" & "),
                ),
            )
            .with_span(span),
        );
    }

    /// Whether interface `name`, or an interface it extends, declares `method`.
    fn interface_provides_method(&self, name: &str, method: &str) -> bool {
        let mut queue = vec![name.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(current) = queue.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let iface = self.symbols.interfaces.get(&current).or_else(|| {
                self.symbols
                    .interfaces
                    .iter()
                    .find(|(k, _)| k.rsplit('.').next() == Some(current.as_str()))
                    .map(|(_, v)| v)
            });
            let Some(iface) = iface else { continue };
            if iface.methods.contains_key(method) {
                return true;
            }
            for parent in &iface.extends {
                if let Some(seg) = parent.name.segments.last() {
                    queue.push(seg.text.clone());
                }
            }
        }
        false
    }

    /// A `System.out.println(...)`-shaped call on an undeclared `System`:
    /// the span of `System` and the help to give. The parser reads the callee
    /// as the dotted path `System.out.println`, or as field accesses on it.
    fn java_system_out_call(&self, c: &CallExpr) -> Option<(juxc_source::Span, &'static str)> {
        let (head, stream, method) = match c.callee.as_ref() {
            Expr::Path(qn) if qn.segments.len() == 3 => {
                (&qn.segments[0], qn.segments[1].text.as_str(), qn.segments[2].text.as_str())
            }
            Expr::Field(f) => match f.object.as_ref() {
                Expr::Field(inner) => match inner.object.as_ref() {
                    Expr::Path(p) if p.segments.len() == 1 => {
                        (&p.segments[0], inner.field.text.as_str(), f.field.text.as_str())
                    }
                    _ => return None,
                },
                Expr::Path(p) if p.segments.len() == 2 => {
                    (&p.segments[0], p.segments[1].text.as_str(), f.field.text.as_str())
                }
                _ => return None,
            },
            _ => return None,
        };
        if head.text != "System"
            || self.env.lookup("System").is_some()
            // Only a `System` the program declares counts. The scanned Rust
            // std has one (`std::alloc::System`), which is not this.
            || self
                .symbols
                .classes
                .iter()
                .any(|(k, cls)| k.rsplit('.').next() == Some("System") && !cls.is_external)
        {
            return None;
        }
        crate::java_habits::system_out_hint(stream, method).map(|help| (head.span, help))
    }

    /// Whether the type `type_name` declares or was scanned with a method
    /// `method` (classes, interfaces, records and enums).
    fn type_has_method(&self, type_name: &str, method: &str) -> bool {
        self.symbols.classes.get(type_name).is_some_and(|c| c.methods.contains_key(method))
            || self.symbols.interfaces.get(type_name).is_some_and(|i| i.methods.contains_key(method))
            || self.symbols.records.get(type_name).is_some_and(|r| r.methods.contains_key(method))
            || self.symbols.enums.get(type_name).is_some_and(|e| e.methods.contains_key(method))
    }

    /// Whether `name` is something a Jux program READS on the type rather
    /// than calls: a field, a property or a record component. A Java getter
    /// call on one of those gets the "read it directly" help.
    fn type_has_readable(&self, type_name: &str, name: &str) -> bool {
        self.symbols.lookup_field(type_name, name).is_some()
            || self.symbols.classes.get(type_name).is_some_and(|c| c.properties.contains_key(name))
            || self
                .symbols
                .records
                .get(type_name)
                .is_some_and(|r| r.components.iter().any(|comp| comp.name == name))
    }

    /// `" -- did you mean `x`?"` when the type has a method whose name is
    /// close to the one written, else an empty string.
    ///
    /// Candidates come from the type's own recorded surface, so a foreign
    /// type suggests what the rustdoc scan actually found. The threshold is
    /// deliberately generous on a shared PREFIX, because the misses that
    /// matter are a Rust name the user shortened (`sort` for
    /// `sort_unstable`) or a Java name that has a differently-spelled twin.
    /// A local's declared type must name something (E0417): `Zork z = 5;`
    /// used to pass the checker and fail in rustc. Checks the head and every
    /// generic argument and function-type slot, with the same resolver the
    /// signature check uses, and suggests the nearest visible type name.
    fn check_local_type_known(&mut self, tref: &TypeRef) {
        if let Some(fs) = &tref.fn_shape {
            for p in &fs.params {
                self.check_local_type_known(p);
            }
            self.check_local_type_known(&fs.return_type);
            return;
        }
        if tref.const_literal_text().is_some() {
            return;
        }
        if self.sig_head_unresolved(tref, &[]) {
            let bare = tref.name.segments[0].text.clone();
            let hint = self.nearest_type_hint(&bare);
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0417_UnknownType,
                    format!(
                        "unknown type `{bare}`: no class, record, enum, interface or primitive of that name is visible here{hint}"
                    ),
                )
                .with_span(tref.span),
            );
            return;
        }
        for ga in &tref.generic_args {
            if let juxc_ast::GenericArg::Type(inner) = ga {
                self.check_local_type_known(inner);
            }
        }
    }

    /// `" -- did you mean `String`?"` when a visible type name, a primitive,
    /// or a prelude type is a likely misspelling of `wanted`; empty otherwise.
    /// Ranked by edit distance first: `Strng` is one letter from `String` and
    /// two from `Stream`, and a typo is what an unknown type name usually is.
    fn nearest_type_hint(&self, wanted: &str) -> String {
        let mut names: Vec<String> = juxc_lex::PRIMITIVE_TYPE_NAMES.iter().map(|s| s.to_string()).collect();
        names.extend(juxc_lex::grammar_spec::BUILTIN_NAMES.iter().map(|s| s.to_string()));
        let bare = |k: &String| k.rsplit('.').next().unwrap_or(k).to_string();
        names.extend(self.symbols.classes.keys().map(bare));
        names.extend(self.symbols.records.keys().map(bare));
        names.extend(self.symbols.enums.keys().map(bare));
        names.extend(self.symbols.interfaces.keys().map(bare));
        names.sort_unstable();
        names.dedup();
        // At most a third of the name may differ (one letter in a short one).
        let budget = (wanted.chars().count() / 3).max(1);
        let mut scored: Vec<(&str, usize)> = names
            .iter()
            .map(|c| (c.as_str(), edit_distance(&wanted.to_lowercase(), &c.to_lowercase())))
            .filter(|(_, d)| *d <= budget)
            .collect();
        scored.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.len().cmp(&b.0.len())).then(a.0.cmp(b.0)));
        match scored.first() {
            Some((c, _)) => format!(" -- did you mean `{c}`?"),
            None => String::new(),
        }
    }

    fn nearest_method_hint(&self, type_name: &str, wanted: &str) -> String {
        let mut names: Vec<&str> = Vec::new();
        if let Some(cls) = self.symbols.classes.get(type_name) {
            names.extend(cls.methods.keys().map(|k| k.as_str()));
        }
        if let Some(iface) = self.symbols.interfaces.get(type_name) {
            names.extend(iface.methods.keys().map(|k| k.as_str()));
        }
        if let Some(rec) = self.symbols.records.get(type_name) {
            names.extend(rec.methods.keys().map(|k| k.as_str()));
        }
        if let Some(en) = self.symbols.enums.get(type_name) {
            names.extend(en.methods.keys().map(|k| k.as_str()));
        }
        // Up to three, best first, rather than one. Several candidates often
        // score identically -- `sort` prefix-matches `sort_floats`,
        // `sort_unstable` and `sort_unstable_by` alike -- and picking one
        // would be a guess presented as an answer. Shorter names first within
        // a score, and the list is sorted so the output never depends on hash
        // iteration order.
        names.sort_unstable();
        let mut scored: Vec<(&str, f32)> = names
            .into_iter()
            .map(|c| (c, name_affinity(wanted, c)))
            .filter(|(_, s)| *s >= 0.45)
            .collect();
        scored.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then(a.0.len().cmp(&b.0.len()))
                .then(a.0.cmp(b.0))
        });
        scored.dedup_by(|a, b| a.0 == b.0);
        let picks: Vec<String> = scored.iter().take(3).map(|(c, _)| format!("`{c}`")).collect();
        match picks.len() {
            0 => String::new(),
            1 => format!(" -- did you mean {}?", picks[0]),
            _ => format!(" -- did you mean {}?", picks.join(", ")),
        }
    }

    fn check_protected_qualifier(
        &mut self,
        vis: juxc_ast::Visibility,
        declaring_class: &str,
        member_name: &str,
        member_kind: &str,
        qualifier_ty: &str,
        through_this: bool,
        access_span: juxc_source::Span,
    ) {
        use juxc_ast::Visibility;
        if !matches!(vis, Visibility::Protected) || through_this {
            return;
        }
        // Same package: `protected` grants package access, so the qualifier
        // does not matter.
        if self.same_package_as(declaring_class) {
            return;
        }
        let Some(accessor) = self.env.current_class.clone() else {
            return;
        };
        // Only inside a subclass. Anywhere else the ordinary ladder has
        // already rejected the access, and reporting twice helps nobody.
        let in_subclass = accessor != declaring_class
            && crate::ty::walk_extends_reaches(&accessor, declaring_class, self.symbols);
        if !in_subclass {
            return;
        }
        let qualifier = qualifier_ty.rsplit('.').next().unwrap_or(qualifier_ty);
        let accessor_bare = accessor.rsplit('.').next().unwrap_or(&accessor);
        if qualifier == accessor_bare
            || crate::ty::walk_extends_reaches(qualifier, &accessor, self.symbols)
        {
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0415_ProtectedAccess,
                format!(
                    "protected {member_kind} `{member_name}` of `{declaring_class}` cannot be \
                     reached through a `{qualifier}` -- `{accessor_bare}` is in a different \
                     package, so it may only use a qualifier typed `{accessor_bare}` or a \
                     subclass of it (JLS 6.6.2.1)",
                ),
            )
            .with_span(access_span)
            .with_help("access it through `this`, or hold the value at the subclass type"),
        );
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
                // JLS §6.6.1: `private` access is scoped to the body of the
                // enclosing TOP LEVEL class, not to the immediately declaring
                // one. So a nested class and its owner see each other's
                // privates, in both directions, and so do two nested classes
                // under the same owner.
                if accessor.is_some_and(|a| self.shares_top_level_owner(a, declaring_class)) {
                    return;
                }
                code::Code::E0414_PrivateAccess
            }
            Visibility::Protected => {
                if accessor.is_some_and(|a| {
                    a == declaring_class
                        || crate::ty::walk_extends_reaches(a, declaring_class, self.symbols)
                        // Nesting reaches further than `protected` does, so
                        // anything `private` would allow is allowed here too.
                        || self.shares_top_level_owner(a, declaring_class)
                }) {
                    return;
                }
                // JLS §6.6.1: `protected` also grants package access. A peer
                // in the same package that is NOT a subclass still reaches
                // the member — `protected` is strictly wider than
                // package-private, never narrower.
                if self.same_package_as(declaring_class) {
                    return;
                }
                code::Code::E0415_ProtectedAccess
            }
            Visibility::Package | Visibility::Internal => {
                if self.same_package_as(declaring_class) {
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
            Enum {
                name: &'a str,
                variants: Vec<String>,
            },
            Class {
                name: &'a str,
                permits: Vec<String>,
            },
        }
        let scrut_name = match &scrut_ty {
            Ty::User { name, .. } => name.as_str(),
            _ => return,
        };
        // FQN-aware lookup (exact key, then unique suffix) so a
        // locally-inferred bare enum name still gets exhaustiveness
        // (and rustc's E0004 never leaks for it).
        let kind = if let Some((_, e)) = self
            .symbols
            .lookup_enum_in(scrut_name, &self.env.current_package.join("."))
        {
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
        } else if let Some(i) = self.symbols.interfaces.get(scrut_name) {
            // A sealed interface's implementers are exactly its `permits`
            // list, so type patterns naming each of them cover it.
            if i.is_sealed && !i.permits.is_empty() {
                SealedKind::Class {
                    name: scrut_name,
                    permits: i.permits.clone(),
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
            SealedKind::Class { name, permits } => {
                let label = if self.symbols.interfaces.contains_key(*name) {
                    "sealed interface"
                } else {
                    "sealed class"
                };
                (label, permits.clone(), *name)
            }
        };
        let missing: Vec<String> = all.into_iter().filter(|v| !covered.contains(v)).collect();
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

    /// `I.super.m(...)` (§T.8.3): legal only in an instance context of a class
    /// that lists `I` in its own `implements` clause (`E0482`), and only for a
    /// method `I` declares with a default body (`E0483`). A method `I` does not
    /// declare at all is left to the ordinary member lookup, which says so.
    fn check_interface_super_call(&mut self, iface: &str, method: &str, c: &CallExpr) {
        let bare = iface.rsplit('.').next().unwrap_or(iface);
        let misplaced = if self.env.current_class.is_none() {
            Some(format!("`{bare}.super.{method}()` calls a default method on `this`, so it belongs in a class that implements `{bare}`"))
        } else if self.in_static {
            Some(format!("`{bare}.super.{method}()` needs `this`, and a `static` method has none"))
        } else if self.current_ctor.is_some() || self.in_init_block {
            Some(format!("`{bare}.super.{method}()` belongs in an instance method: a constructor or `init` block is still building `this`"))
        } else if crate::infer::direct_superinterface(&self.env, self.symbols, iface).is_none() {
            let class = self.env.current_class.as_deref().unwrap_or("");
            let class = class.rsplit('.').next().unwrap_or(class);
            Some(format!("`{class}` does not implement `{bare}` directly, so `{bare}.super` names no default of its own: add `{bare}` to its `implements` clause"))
        } else {
            None
        };
        if let Some(message) = misplaced {
            self.diagnostics.push(
                Diagnostic::error(code::Code::E0482_InterfaceSuperMisplaced, format!("{message} (§T.8.3)"))
                    .with_span(c.span),
            );
            return;
        }
        let declared = self.symbols.interfaces.get(iface).and_then(|i| i.methods.get(method));
        if let Some(sig) = declared {
            if sig.is_abstract {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0483_InterfaceSuperNoDefault,
                        format!("`{bare}.{method}` has no default body, so `{bare}.super.{method}()` has nothing to call (§T.8.3)"),
                    )
                    .with_span(c.span),
                );
            }
        }
    }

    fn check_field_access(&mut self, f: &FieldExpr) {
        // A record-destructuring read (`__jux_component_N` on the temporary):
        // name the component for the driver's rewrite. The pattern's shape was
        // judged at the temporary's declaration.
        if let Some(index) = juxc_ast::record_component_index(&f.field.text) {
            if let Ty::User { name, .. } = infer_expr(&f.object, &self.env, self.symbols) {
                if let Some(component) = self.symbols.records.get(&name).and_then(|r| r.components.get(index)) {
                    self.component_names.insert(f.field.span, component.name.clone());
                }
            }
            return;
        }
        // `I.super` (§T.8.3) is a call receiver and nothing else. Its call
        // was validated by `check_interface_super_call`; used as a value it
        // has no meaning.
        if let Some((iface, _)) = crate::infer::interface_super_receiver(f, &self.env, self.symbols) {
            if !self.in_interface_super_call {
                let bare = iface.rsplit('.').next().unwrap_or(&iface);
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0482_InterfaceSuperMisplaced,
                        format!("`{bare}.super` is not a value: it only calls one of `{bare}`'s default methods, as `{bare}.super.m(...)` (§T.8.3)"),
                    )
                    .with_span(f.span),
                );
            }
            return;
        }
        // `ClassName.STATIC_FIELD` — recognize the static-access
        // shape before treating the receiver as a value. Visibility
        // applies the same as for instance fields; reading an
        // instance field via `ClassName.x` fires a clean diagnostic
        // so the user isn't told "no field `x`" when there IS one
        // but it lives on instances.
        if let Expr::Path(qn) = f.object.as_ref() {
            if let Some(class_fqn) =
                crate::infer::path_resolves_to_class(qn, &self.env, self.symbols)
            {
                let field_name = f.field.text.as_str();
                if let Some(field) = self
                    .symbols
                    .classes
                    .get(&class_fqn)
                    .and_then(|c| c.fields.get(field_name))
                {
                    if field.is_static {
                        let vis = field.visibility;
                        self.check_visibility(vis, &class_fqn, field_name, "static field", f.span);
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
            if let Some(iface_fqn) =
                crate::infer::path_resolves_to_interface(qn, &self.env, self.symbols)
            {
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
                        format!("no static field `{field_name}` on interface `{iface_fqn}`",),
                    )
                    .with_span(f.span),
                );
                return;
            }
        }
        let receiver_ty = infer_expr(&f.object, &self.env, self.symbols);
        let field_name = f.field.text.as_str();

        // Bare `weak` field READ (§6.5): a weak field's strong view is reached
        // ONLY through `.get()` (→ `T?`). The legitimate `.get()` receiver is
        // intercepted in `check_call` (and returns before reaching here), and
        // a weak-field WRITE place checks only its receiver — so any weak field
        // that lands in this read path is a bare read in a value / argument /
        // return position. Reject it, else the backend would expose the raw
        // `Weak<…>` handle and leak a rustc type error.
        if let Ty::User { name, .. } = &receiver_ty {
            if self
                .symbols
                .lookup_field(name, field_name)
                .is_some_and(|(fs, _)| fs.is_weak)
            {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0456_WeakReadNeedsGet,
                        format!(
                            "`weak` field `{field_name}` can only be read via `.get()` \
                             (which returns the target type, nullable) -- a bare read would \
                             expose the raw weak handle",
                        ),
                    )
                    .with_span(f.span),
                );
                return;
            }
        }

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
                                    "no element `{field_name}` on tuple `{receiver_ty}` -- valid indices are 0..{}",
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
                if BUILTIN_ARRAY_FIELDS.contains(&field_name) {}
                // Unknown field on array — stay quiet today. A future
                // pass may tighten this.
            }
            // Strings: same allowlist treatment.
            Ty::String => if BUILTIN_STRING_FIELDS.contains(&field_name) {},
            // User types: walk the inheritance chain looking for the
            // field. Emit E0412 if not found anywhere. When the field
            // is found, verify visibility against the current
            // accessor context.
            Ty::User { name, .. } => {
                if let Some((field, declaring_class)) = self.symbols.lookup_field(name, field_name)
                {
                    // **Field access through a polymorphic-base reference.**
                    // The receiver lowers to a `Rc<dyn …Kind>` trait object
                    // that can't expose struct fields directly — a generated
                    // `__get_<f>` / `__set_<f>` accessor stands in. Only a
                    // **private** field lacks one, so only private is E0437.
                    // `this` (concrete self) and concrete receivers are
                    // unaffected.
                    //
                    // The allowed set is `!Private` and not an explicit list,
                    // because that is exactly the gate the BACKEND uses when it
                    // decides to emit the accessor (`exprs/field.rs`, and the
                    // field-hook gate in `decls/classes.rs`). Spelled as
                    // `Public | Protected` this check was the stricter of the
                    // two, and an `internal` or package-private field was
                    // rejected here — reported as "private", which it is not —
                    // even though the accessor it needs was being generated.
                    // Whether the *caller* may see the field is a separate
                    // question, answered by the E0414/E0415/E0416 ladder below.
                    let recv_bare = name.rsplit('.').next().unwrap_or(name);
                    if !matches!(f.object.as_ref(), Expr::This(_))
                        && self.poly_bases.contains(recv_bare)
                    {
                        use juxc_ast::Visibility;
                        if !matches!(field.visibility, Visibility::Private) {
                            // An accessor exists; fall through to the ordinary
                            // visibility ladder rather than returning, so a
                            // field the caller genuinely cannot see is still
                            // reported with the right code and wording.
                            let vis = field.visibility;
                            let declaring = declaring_class.to_string();
                            self.check_visibility(vis, &declaring, field_name, "field", f.span);
                            // Same JLS 6.6.2.1 rule as the concrete path. A
                            // polymorphic base is exactly where this bites:
                            // `other.tag` on a base-typed sibling is the
                            // shape the rule exists to reject.
                            self.check_protected_qualifier(
                                vis,
                                &declaring,
                                field_name,
                                "field",
                                name,
                                matches!(f.object.as_ref(), Expr::This(_)),
                                f.span,
                            );
                            return;
                        }
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0437_FieldThroughPolymorphicBase,
                                format!(
                                    "private field `{field_name}` can't be accessed through a \
                                     `{recv_bare}` reference -- `{recv_bare}` is a polymorphic base \
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
                    self.check_visibility(vis, &declaring, field_name, "field", f.span);
                    // JLS 6.6.2.1 — across a package boundary a subclass may
                    // only reach an inherited `protected` member through a
                    // qualifier typed as itself.
                    self.check_protected_qualifier(
                        vis,
                        &declaring,
                        field_name,
                        "field",
                        name,
                        matches!(f.object.as_ref(), Expr::This(_)),
                        f.span,
                    );
                    return;
                }
                // A property — `T Name { get; set; }` or the
                // expression-bodied `T Name -> expr;` — is stored as a method
                // with `is_property = true`. From the user's perspective
                // `obj.Name` is a field-shaped read, so it lands here rather
                // than in the field branch above.
                //
                // It is still a MEMBER, and its modifier still means something:
                // this branch used to return without asking, so a `private`
                // property was readable from anywhere while a `private` field
                // or method one line away was not. The modifier was simply
                // ignored for the one member kind whose whole purpose is
                // controlling access to state.
                if let Some((method, decl)) = self.symbols.lookup_method(name, field_name) {
                    if method.is_property {
                        let vis = method.visibility;
                        let declaring = decl.to_string();
                        self.check_visibility(vis, &declaring, field_name, "property", f.span);
                        return;
                    }
                }
                // A property an interface declares (§M.7.10): a contract read
                // through an interface-typed value, or a default property the
                // class inherits. Interface members are public.
                if self.symbols.lookup_interface_property(name, field_name).is_some() {
                    return;
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
                // §P.3 helper: `obj.observers` reaches for the observer
                // namespace at the OBJECT level, but `.observers` hangs
                // off an observable PROPERTY — `obj.<Property>.observers`.
                // When the receiver's class has observable properties,
                // point the user at the right form instead of a bare
                // "no field" message.
                if field_name == "observers" {
                    let observable: Vec<String> = self
                        .symbols
                        .classes
                        .get(name)
                        .map(|c| {
                            // `{ get; set; }` — settable, non-static,
                            // not read-only / init-only — is observable.
                            c.properties
                                .iter()
                                .filter(|(_, sig)| {
                                    !sig.is_static && !sig.is_read_only && !sig.is_init_only
                                })
                                .map(|(pname, _)| pname.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    if !observable.is_empty() {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0412_UnresolvedField,
                                format!(
                                    "`.observers` is a member of an observable PROPERTY, not of `{name}` itself -- write `<value>.{}.observers` (§P.3)",
                                    observable[0],
                                ),
                            )
                            .with_span(f.span),
                        );
                        return;
                    }
                }
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0412_UnresolvedField,
                        format!(
                            "no field `{field_name}` on type `{}`",
                            crate::ty::nested_type_spelling(name),
                        ),
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
            // `<int N>` reads as an `int`, `<long N>` as a `long`, `<char C>`
            // as a `char`, `<bool B>` as a `bool` (T.11.3).
            let value_ty = cty
                .name
                .segments
                .last()
                .and_then(|s| crate::ty::primitive_from_name(&s.text))
                .map(Ty::Primitive)
                .unwrap_or(Ty::Primitive(Primitive::Int));
            self.env.declare(&p.name.text, value_ty);
            self.const_param_names.insert(p.name.text.clone());
        }
    }

    /// Type-position sibling of [`Self::check_const_size_expr`]: pull
    /// the size out of a `T[«size»]` declared type (field / local /
    /// param / return) and run the same const-arithmetic guard.
    fn check_fixed_array_size_in_type(&mut self, tref: &juxc_ast::TypeRef) {
        // A multi-dimensional type can carry several fixed dimensions
        // (`int[3][4]`); guard each one's size expression.
        if let Some(shape) = &tref.array_shape {
            for dim in &shape.dims {
                if let juxc_ast::ArrayDim::Fixed(size) = dim {
                    let size = size.clone();
                    // A FIXED array TYPE (`int[«size»] field;`) is a genuine
                    // Rust `[T; N]` const position — the size MUST be const.
                    self.check_const_size_expr(&size, false);
                }
            }
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
    ///
    /// `heapable` is `true` at a `new T[«size»]` **expression** site: such an
    /// allocation lowers to a heap `vec![..; size]` whenever the size isn't a
    /// bare compile-time constant (backend `emit_new_array_dim`), and a
    /// `Vec` repeat accepts ANY runtime `usize` length. So a non-const or
    /// const-generic-arithmetic size is fine there — it just heaps. The
    /// const-ness restriction only bites for a FIXED array TYPE (`heapable =
    /// false`), which is a real Rust `[T; N]` and genuinely needs a const `N`.
    /// Const-fold *panics* (overflow / divide-by-zero) and resource-limit
    /// overruns are reported regardless, since they're real errors either way.
    fn check_const_size_expr(&mut self, size: &Expr, heapable: bool) {
        self.check_const_size_expr_at(size, heapable, juxc_source::Span::DUMMY)
    }

    /// An integer constant's initializer is evaluated here, at compile time
    /// (§T.11.6), so an overflow is `E0842` rather than a value Rust rejects
    /// later: `const long OV = long.MAX_VALUE + 1;`, `const i32 X = 2147483647 +
    /// 1;`. The folded value must also fit the constant's own type, which the
    /// 64-bit evaluation alone does not show for a narrower one.
    fn check_const_integer_fits(&mut self, name: &str, slot: &Ty, value: &Expr, fallback: Span) {
        let Ty::Primitive(p) = slot else { return };
        let Some(bits) = crate::ty::integer_bits(*p) else { return };
        let ctx = crate::const_eval::ConstCtx {
            symbols: self.symbols,
            generic_param_names: &self.const_param_names,
            enclosing_class: None,
        };
        let span = match expr_span(value) {
            s if s == Span::DUMMY => fallback,
            s => s,
        };
        let message = match crate::const_eval::eval_const_int(value, &ctx) {
            Err(crate::const_eval::ConstEvalError::Panic(msg)) => msg,
            Ok(v) => {
                let v = i128::from(v);
                let (lo, hi): (i128, i128) = if crate::ty::is_unsigned_primitive(*p) {
                    (0, (1i128 << bits) - 1)
                } else {
                    (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
                };
                // `int` and `uint` hold 64 bits here; a 32-bit target is the
                // runtime rule's to report, like any `int` arithmetic.
                if (lo..=hi).contains(&v) || crate::infer::untyped_int_literal(value) {
                    return;
                }
                format!(
                    "it evaluates to {v}, which does not fit `{}` ({lo} to {hi})",
                    crate::ty::primitive_name(*p),
                )
            }
            Err(_) => return,
        };
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0842_ConstEvalPanic,
                format!("the initializer of constant `{name}` cannot be evaluated: {message}"),
            )
            .with_span(span),
        );
    }

    /// As [`Self::check_const_size_expr`], with a span to fall back on when
    /// the size expression has none of its own.
    fn check_const_size_expr_at(
        &mut self,
        size: &Expr,
        heapable: bool,
        fallback: juxc_source::Span,
    ) {
        let at = |e: &Expr| {
            let s = expr_span(e);
            if s == juxc_source::Span::DUMMY { fallback } else { s }
        };
        let ctx = crate::const_eval::ConstCtx {
            symbols: self.symbols,
            generic_param_names: &self.const_param_names,
            enclosing_class: None,
        };
        match crate::const_eval::eval_const_int(size, &ctx) {
            // Reduces to a concrete value (`5`, `SIZE`, `SIZE + 1`, a const-fn
            // call) — accept; the backend emits the computed literal (§T.11).
            Ok(_) => {}
            // Mentions a GENERIC const param: a bare `[N]` is fine (forwarded
            // as a Rust const-generic arg), but computed `[N + 1]` needs the
            // nightly `generic_const_exprs` we don't enable — keep E0445 for a
            // FIXED array type. At a `new` site it heaps (`vec![..; N + 1]`),
            // so there's nothing to reject.
            Err(crate::const_eval::ConstEvalError::Generic) => {
                // A const parameter sizes an array only when it is an `int` or
                // a `uint` (a Rust array length is a `usize`, T.11.3).
                if let Expr::Path(qn) = size {
                    if qn.segments.len() == 1 {
                        let name = qn.segments[0].text.as_str();
                        if let Some(Ty::Primitive(kind)) = self.env.lookup(name) {
                            if self.const_param_names.contains(name)
                                && !matches!(kind, Primitive::Int | Primitive::Uint)
                            {
                                let kind = crate::ty::primitive_name(*kind);
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0445_ConstGenericUnsupported,
                                        format!(
                                            "`{name}` is a `{kind}` const parameter, and an array size is an `int`: declare it `<int {name}>` to size an array with it"
                                        ),
                                    )
                                    .with_span(at(size)),
                                );
                                return;
                            }
                        }
                    }
                }
                if !heapable && !matches!(size, Expr::Path(qn) if qn.segments.len() == 1) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0445_ConstGenericUnsupported,
                            "const-generic arithmetic in array sizes (e.g. `N + 1`) is not \
                             supported in this phase -- use the bare parameter (`[N]`) or a \
                             literal size",
                        )
                        .with_span(at(size)),
                    );
                }
            }
            // Overflow / divide-by-zero while folding.
            Err(crate::const_eval::ConstEvalError::Panic(msg)) => {
                self.diagnostics.push(
                    Diagnostic::error(code::Code::E0842_ConstEvalPanic, msg)
                        .with_span(at(size)),
                );
            }
            Err(crate::const_eval::ConstEvalError::LimitExceeded) => {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0840_ConstEvalLimitExceeded,
                        "const evaluation of this array size exceeded its resource limits",
                    )
                    .with_span(at(size)),
                );
            }
            // Not const-evaluable. Preserve the prior leniency for a bare
            // literal / name (which the old code accepted unconditionally); only
            // a clearly non-const ARITHMETIC size is a hard error now (it would
            // otherwise leak a rustc "array size must be const" error).
            Err(crate::const_eval::ConstEvalError::NonConst(msg)) => {
                // A runtime size is only an error for a FIXED array TYPE. At a
                // `new T[n]` expression it heaps to `vec![..; n]` (§5.6), which
                // is exactly how a runtime-sized buffer is allocated.
                if !heapable && !matches!(size, Expr::Literal(_) | Expr::Path(_)) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0841_NonConstInConstContext,
                            format!("array size must be a compile-time constant -- {msg}"),
                        )
                        .with_span(at(size)),
                    );
                }
            }
        }
    }

    /// The typed `assertThrows<E>(f)` (JUX-TESTING-ADDENDUM §TS.3): a call that
    /// resolved to the library `assertThrows` with ONE explicit type argument.
    ///
    /// `E` must be `Exception` or a subclass (E0446, the bound the form
    /// declares in prose); a valid call is recorded for the backend, which
    /// lowers it to a `catch (E e)`-style dispatch. Returns whether the call
    /// is the typed form at all, valid or not, so the caller does not also
    /// report the type argument as one a non-generic function cannot take.
    fn check_typed_assert_throws(&mut self, fqn: &str, c: &CallExpr) -> bool {
        if fqn != crate::infer::TYPED_ASSERT_THROWS_FQN || c.explicit_generic_args.is_empty() {
            return false;
        }
        // "not generic, remove the `<…>`" would be wrong advice here: one type
        // argument is exactly what the typed form takes.
        if c.explicit_generic_args.len() > 1 {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0443_ExplicitTypeArgs,
                    format!(
                        "`assertThrows` takes one type argument, the exception it expects, \
                         but {} were supplied (§TS.3)",
                        c.explicit_generic_args.len()
                    ),
                )
                .with_span(c.span),
            );
            return true;
        }
        let written = ty_from_ref(&c.explicit_generic_args[0], &self.env, self.symbols);
        let exception = match &written {
            Ty::User { name, generic_args } if generic_args.is_empty() => self
                .resolve_class_fqn(name)
                .filter(|class| self.extends_chain_reaches(class, crate::infer::EXCEPTION_FQN)),
            _ => None,
        };
        match exception {
            Some(class) => {
                self.typed_assert_throws.insert(c.span, class);
            }
            None => self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0446_GenericBoundNotSatisfied,
                    format!(
                        "`assertThrows<{written}>` needs an exception type, and `{written}` is not \
                         `Exception` or a subclass of it (§TS.3)"
                    ),
                )
                .with_span(c.span),
            ),
        }
        true
    }

    /// Does the class `fqn`'s `extends` chain reach `ancestor` (itself
    /// included)? Bounded, so a malformed cyclic chain cannot hang the check.
    fn extends_chain_reaches(&self, fqn: &str, ancestor: &str) -> bool {
        let mut cur = Some(fqn.to_string());
        for _ in 0..=64 {
            match cur {
                Some(name) if name == ancestor => return true,
                Some(name) => {
                    cur = self.symbols.classes.get(&name).and_then(|c| c.extends_fqn.clone());
                }
                None => return false,
            }
        }
        false
    }

    /// A call through a function pointer: the callee and every argument are
    /// checked as expressions, the call needs `unsafe` (E0506), and the
    /// arguments must match the pointer's parameters in number (E0411) and type
    /// (E0410), the conversions of a native call aside (§8.1.1).
    fn check_fn_pointer_call(&mut self, c: &CallExpr, params: &[Ty], param_ptr_depths: &[u8]) {
        if let Expr::Field(f) = c.callee.as_ref() {
            self.check_expr(&f.object);
        } else {
            self.check_expr(&c.callee);
        }
        // The backend recognises the call by the callee's recorded type; a
        // field callee is not visited as an expression of its own.
        let callee_ty = infer_expr(&c.callee, &self.env, self.symbols);
        let callee_span = expr_span(&c.callee);
        if callee_span != Span::DUMMY {
            self.expr_types.insert(callee_span, callee_ty);
        }
        for arg in &c.args {
            self.check_expr(arg);
        }
        if !self.in_unsafe {
            self.unsafe_pointer_op("calling through a function pointer", c.span);
        }
        if c.args.len() != params.len() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0411_WrongArgCount,
                    format!(
                        "this function pointer takes {} argument{}, but {} {} given",
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        c.args.len(),
                        if c.args.len() == 1 { "was" } else { "were" },
                    ),
                )
                .with_span(c.span),
            );
            return;
        }
        for (i, (arg, param)) in c.args.iter().zip(params).enumerate() {
            // A pointer parameter takes `null`, which the erased `Ty` cannot
            // tell from a mismatch (§L.6.1).
            let pointer_null = param_ptr_depths.get(i).is_some_and(|d| *d > 0)
                && matches!(arg, Expr::Literal(juxc_ast::Literal::Null));
            let found = infer_expr(arg, &self.env, self.symbols);
            if !pointer_null && !compatible(param, &found, self.symbols) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0410_TypeMismatch,
                        format!("this function pointer takes `{param}` here, but the argument is `{found}`"),
                    )
                    .with_span([expr_span(arg), c.span].into_iter().find(|s| *s != Span::DUMMY).unwrap_or(c.span)),
                );
            }
            self.check_literal_fits(param, arg, c.span);
        }
    }

    /// The first type in a function-pointer signature that has no C form
    /// (§L.6.4 allows what an `@export` signature allows), or `None`.
    fn fn_pointer_signature_offender<'t>(
        &self,
        shape: &'t juxc_ast::FnTypeShape,
    ) -> Option<&'t juxc_ast::TypeRef> {
        let is_void = |t: &juxc_ast::TypeRef| {
            t.fn_shape.is_none()
                && t.ptr_depth == 0
                && t.name.segments.len() == 1
                && t.name.segments[0].text == "void"
        };
        shape
            .params
            .iter()
            .find(|p| !self.ffi_type_ok(p))
            .or_else(|| (!is_void(&shape.return_type) && !self.ffi_type_ok(&shape.return_type)).then_some(&shape.return_type))
    }

    /// **E0508** for a function-pointer type written anywhere in `tref` whose
    /// signature is not C-compatible. Runs at every declared type: locals,
    /// parameters, fields and results.
    fn check_fn_pointer_signatures(&mut self, tref: &juxc_ast::TypeRef) {
        let Some(shape) = tref.fn_shape.as_deref() else { return };
        for inner in shape.params.iter().chain(std::iter::once(&shape.return_type)) {
            self.check_fn_pointer_signatures(inner);
        }
        if !shape.is_pointer {
            return;
        }
        if let Some(bad) = self.fn_pointer_signature_offender(shape) {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0508_FfiTypeNotAllowed,
                    format!(
                        "`{}` has no C form, so it cannot appear in the function-pointer type \
                         `{}` -- use a primitive, `String`, a raw pointer, a `@layout(c)` type, \
                         or another function pointer",
                        type_ref_display(bad),
                        type_ref_display(tref),
                    ),
                )
                .with_span([bad.span, tref.span].into_iter().find(|s| *s != Span::DUMMY).unwrap_or(tref.span)),
            );
        }
    }

    /// What may fill a function-pointer slot (§L.6.4): a free function whose
    /// signature corresponds (E0513), or a lambda that captures nothing
    /// (E0514). `null`, another function pointer and a cast are left to the
    /// ordinary type rules.
    fn check_fn_pointer_value(&mut self, slot: &Ty, value: &Expr, fallback: Span) {
        let Ty::FnPtr { params, param_ptr_depths, return_type, return_ptr_depth } = slot else {
            return;
        };
        let span = [expr_span(value), fallback].into_iter().find(|s| *s != Span::DUMMY).unwrap_or(fallback);
        match value {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                if self.env.lookup(name).is_some() {
                    return;
                }
                // The backend builds the C-ABI entry for a function or lambda
                // from the slot's signature, which it finds under the value's
                // own span.
                self.expr_types.insert(qn.span, slot.clone());
                let not_fitting = |why: String| {
                    Diagnostic::error(
                        code::Code::E0513_FunctionDoesNotFitPointer,
                        format!("`{name}` cannot be used as `{slot}`: {why}"),
                    )
                    .with_span(span)
                };
                // A method of the class being checked, named without `this.`.
                let is_method = self
                    .env
                    .current_class
                    .as_deref()
                    .is_some_and(|c| self.symbols.lookup_method(c, name).is_some());
                let function = self
                    .env
                    .unqualified
                    .get(name)
                    .and_then(|fqn| self.symbols.functions.get(fqn).map(|f| (fqn.clone(), f)))
                    .or_else(|| self.symbols.lookup_function(name).map(|(k, f)| (k.to_string(), f)));
                let Some((fqn, function)) = function else {
                    if is_method {
                        self.diagnostics.push(not_fitting(
                            "it is a method, and a method needs an object; only a free function has a \
                             plain code address"
                                .to_string(),
                        ));
                    }
                    return;
                };
                let function = function.clone();
                let group = self
                    .symbols
                    .function_overloads
                    .get(&fqn)
                    .or_else(|| self.symbols.function_overloads.get(name))
                    .map(|g| g.len())
                    .unwrap_or(1);
                let why = if group > 1 {
                    Some("it is overloaded, so the name does not pick one function".to_string())
                } else if !function.generic_params.is_empty() {
                    Some("it is generic, and a code address is one concrete function".to_string())
                } else if !function.throws.is_empty() {
                    Some("it declares `throws`, and an exception cannot cross a C frame".to_string())
                } else if function.params.len() != params.len() {
                    Some(format!(
                        "it takes {} parameter{}, and the pointer takes {}",
                        function.params.len(),
                        if function.params.len() == 1 { "" } else { "s" },
                        params.len(),
                    ))
                } else {
                    // A pointer is not converted (§8.1.1): `int*` in the C
                    // signature points at a C `int`, and the same spelling in a
                    // Jux function points at a pointer-sized one. Only the
                    // fixed-width names mean one thing on both sides.
                    let width_differs = |t: &Ty, depth: u8| {
                        depth > 0
                            && matches!(
                                t,
                                Ty::Primitive(
                                    Primitive::Int
                                        | Primitive::Uint
                                        | Primitive::Long
                                        | Primitive::Ulong
                                        | Primitive::Char
                                )
                            )
                    };
                    let width_note = function
                        .params
                        .iter()
                        .zip(params.iter().zip(param_ptr_depths))
                        .find(|(_, (want, depth))| width_differs(want, **depth))
                        .map(|(p, (want, _))| {
                            format!(
                                "parameter `{}` is a pointer to `{want}`, which is a different width in \
                                 Jux and in C, so no Jux function can take that C pointer -- spell the \
                                 pointee with a fixed width (`i32*`, `i64*`, `byte*`) in both places",
                                p.name,
                            )
                        });
                    let mismatch = width_note.or_else(|| function.params.iter().zip(params.iter().zip(param_ptr_depths)).find_map(
                        |(p, (want, want_depth))| {
                            let have = ty_from_ref(&p.ty, &self.env, self.symbols);
                            (have != *want || p.ty.ptr_depth != *want_depth).then(|| {
                                format!(
                                    "parameter `{}` is `{}`, and the pointer passes `{}{}`",
                                    p.name,
                                    type_ref_display(&p.ty),
                                    want,
                                    "*".repeat(*want_depth as usize),
                                )
                            })
                        },
                    ));
                    mismatch.or_else(|| {
                        let (have, have_depth) = match &function.return_type {
                            ReturnType::Type(t) | ReturnType::AsyncType(t) => {
                                (ty_from_ref(t, &self.env, self.symbols), t.ptr_depth)
                            }
                            ReturnType::Void => (Ty::Void, 0),
                        };
                        (have != **return_type || have_depth != *return_ptr_depth).then(|| {
                            format!(
                                "it returns `{have}{}`, and the pointer returns `{return_type}{}`",
                                "*".repeat(have_depth as usize),
                                "*".repeat(*return_ptr_depth as usize),
                            )
                        })
                    })
                };
                if let Some(why) = why {
                    self.diagnostics.push(not_fitting(why));
                }
            }
            Expr::Lambda(l) => {
                self.expr_types.insert(l.span, slot.clone());
                if l.params.len() != params.len() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0513_FunctionDoesNotFitPointer,
                            format!(
                                "this lambda takes {} parameter{}, and `{slot}` takes {}",
                                l.params.len(),
                                if l.params.len() == 1 { "" } else { "s" },
                                params.len(),
                            ),
                        )
                        .with_span(span),
                    );
                }
                if let Some(captured) = self.lambda_capture(l) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0514_CapturingLambdaAsPointer,
                            format!(
                                "this lambda reads `{captured}` from the code around it, so it cannot \
                                 be a function pointer: a code address has nowhere to keep it -- pass \
                                 the value through a `void*` argument, or use a closure type \
                                 `(…) -> …` instead of `fn(…) -> …`",
                            ),
                        )
                        .with_span(span),
                    );
                }
            }
            _ => {}
        }
    }

    /// The first thing `l` captures from the code around it -- a local, a
    /// parameter, a field read without `this.`, or `this` itself -- or `None`
    /// when the lambda is self-contained. Names the lambda declares for itself
    /// (its parameters, its locals, loop and catch variables, the parameters of
    /// lambdas inside it) are not captures even when they shadow an outer one.
    fn lambda_capture(&self, l: &juxc_ast::LambdaExpr) -> Option<String> {
        use juxc_ast::visit::Node;
        let mut declared: std::collections::HashSet<String> =
            l.params.iter().map(|p| p.name.text.clone()).collect();
        let mut reads: Vec<String> = Vec::new();
        let mut uses_this = false;
        let mut visit = |n: Node<'_>| match n {
            Node::Stmt(Stmt::VarDecl(v)) => {
                declared.insert(v.name.text.clone());
            }
            Node::Stmt(Stmt::ForEach(fe)) => {
                declared.insert(fe.var_name.text.clone());
            }
            Node::Stmt(Stmt::Try(t)) => {
                declared.extend(t.catches.iter().map(|c| c.name.text.clone()));
            }
            Node::Expr(Expr::TryExpr(t)) => {
                declared.extend(t.catches.iter().map(|c| c.name.text.clone()));
            }
            Node::Expr(Expr::Lambda(inner)) => {
                declared.extend(inner.params.iter().map(|p| p.name.text.clone()));
            }
            Node::Expr(Expr::This(_) | Expr::Super(_)) => uses_this = true,
            Node::Expr(Expr::Path(qn)) if qn.segments.len() == 1 => {
                reads.push(qn.segments[0].text.clone());
            }
            _ => {}
        };
        match &l.body {
            juxc_ast::LambdaBody::Expr(e) => juxc_ast::visit::for_each_node_in(e, &mut visit),
            juxc_ast::LambdaBody::Block(b) => juxc_ast::visit::for_each_node(b, &mut visit),
        }
        if uses_this {
            return Some("this".to_string());
        }
        reads.into_iter().find(|name| {
            if declared.contains(name) {
                return false;
            }
            if self.env.lookup(name).is_some() {
                return true;
            }
            // A field of the enclosing class, read without `this.`.
            self.env
                .current_class
                .as_deref()
                .and_then(|c| self.symbols.lookup_field(c, name))
                .is_some_and(|(f, _)| !f.is_static)
        })
    }

    /// **E0202** when an untyped integer literal, possibly negated, flows into
    /// an integer slot it does not fit (§S.2.6: a literal adopts the slot's
    /// type "when the value fits").
    ///
    /// `u32 pid = -1;` or `AttachConsole(-1)` against a `u32` parameter used to
    /// pass the checker and fail in rustc ("cannot apply unary operator `-` to
    /// type `u32`"); `byte b = 300;` likewise. The help names the explicit
    /// cast, which keeps the low bits (§S.2.4) and is what C's `(DWORD)-1`
    /// means.
    pub(crate) fn check_literal_fits(&mut self, slot: &Ty, value: &Expr, fallback: Span) {
        let slot = match slot {
            Ty::Nullable(inner) => inner.as_ref(),
            other => other,
        };
        // A function-pointer slot has its own rules for what may fill it: a
        // fitting free function, or a lambda that captures nothing (§L.6.4).
        if matches!(slot, Ty::FnPtr { .. }) && !matches!(value, Expr::Ternary(_)) {
            self.check_fn_pointer_value(slot, value, fallback);
            return;
        }
        // Each arm of a conditional flows into the same slot, and each element
        // of an array literal into the element type.
        match (slot, value) {
            (_, Expr::Ternary(t)) => {
                self.check_literal_fits(slot, &t.then_branch, fallback);
                self.check_literal_fits(slot, &t.else_branch, fallback);
                return;
            }
            (Ty::Array { element, .. }, Expr::NewArrayLit(lit)) => {
                let element = element.as_ref().clone();
                for item in &lit.elements {
                    self.check_literal_fits(&element, item, lit.span);
                }
                return;
            }
            _ => {}
        }
        let Ty::Primitive(p) = slot else { return };
        let (lit, v): (&juxc_ast::IntLit, i128) = match value {
            Expr::Literal(juxc_ast::Literal::Int(lit)) => (lit, lit.value as i128),
            Expr::Unary(u) if u.op == juxc_ast::UnaryOp::Neg => match u.operand.as_ref() {
                Expr::Literal(juxc_ast::Literal::Int(lit)) => (lit, -(lit.value as i128)),
                _ => return,
            },
            _ => return,
        };
        if lit.kind.is_some() {
            return;
        }
        let (lo, hi): (i128, i128) = match p {
            Primitive::Byte | Primitive::I8 => (i8::MIN as i128, i8::MAX as i128),
            Primitive::Ubyte | Primitive::U8 => (0, u8::MAX as i128),
            Primitive::Short | Primitive::I16 => (i16::MIN as i128, i16::MAX as i128),
            Primitive::Ushort | Primitive::U16 => (0, u16::MAX as i128),
            Primitive::I32 => (i32::MIN as i128, i32::MAX as i128),
            Primitive::U32 => (0, u32::MAX as i128),
            Primitive::Int | Primitive::Long | Primitive::I64 => (i64::MIN as i128, i64::MAX as i128),
            Primitive::Uint | Primitive::Ulong | Primitive::U64 => (0, u64::MAX as i128),
            _ => return,
        };
        if v >= lo && v <= hi {
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0202_NumericLiteralOutOfRange,
                format!("the literal `{v}` does not fit in `{slot}`, whose range is {lo} to {hi}"),
            )
            // A bare literal carries no span of its own; point at the statement.
            .with_span(match expr_span(value) {
                s if s == Span::DUMMY => fallback,
                s => s,
            })
            .with_help(format!(
                "to keep the literal's low bits, cast it explicitly: `{v} as {slot}` (§S.2.4)"
            )),
        );
    }

    /// §L.6.1a: a value flowing into a slot of pointer depth `slot_depth` over
    /// `slot_pointee`. Returns true when a pointer is involved on either side,
    /// in which case the pointer rules have been applied (and any mismatch
    /// reported) and the caller skips its ordinary compatibility check. A
    /// `null` into a pointer slot is accepted here.
    pub(crate) fn check_pointer_flow(
        &mut self,
        slot_depth: u8,
        slot_void: bool,
        slot_pointee: &Ty,
        value: &Expr,
        fallback: Span,
    ) -> bool {
        let is_null = matches!(value, Expr::Literal(juxc_ast::Literal::Null));
        if is_null {
            return slot_depth > 0;
        }
        let value_depth = self.expr_ptr_depth(value);
        if slot_depth == 0 && value_depth == 0 {
            return false;
        }
        let value_pointee = infer_expr(value, &self.env, self.symbols);
        if matches!(slot_pointee, Ty::FnPtr { .. }) || matches!(value_pointee, Ty::FnPtr { .. }) {
            return false;
        }
        // A conditional's span starts at its (spanless) condition literal, so
        // the statement's span is the better place to point.
        let span = match expr_span(value) {
            s if s == Span::DUMMY || matches!(value, Expr::Ternary(_)) => fallback,
            s => s,
        };
        let value_void = value_depth > 0 && crate::infer::pointer_base_is_void(value, &self.env, self.symbols);
        let expected = pointer_type_text(if slot_void { &Ty::Void } else { slot_pointee }, slot_depth);
        let found = pointer_type_text(if value_void { &Ty::Void } else { &value_pointee }, value_depth);
        if slot_depth != value_depth {
            let help = if slot_depth == 0 {
                "a pointer is not an integer: convert with `p as ulong` inside `unsafe`, or read through it with `*p`"
            } else if value_depth == 0 {
                "take the address with `&x`, convert an integer with `n as T*` inside `unsafe`, or use `null`"
            } else {
                "the pointer depths differ: add `&` or `*` to reach the level the slot holds"
            };
            self.push_pointer_mismatch(&expected, &found, help, span);
            return true;
        }
        let verdict = match (slot_void, value_void) {
            (true, true) => PointeeMatch::Same,
            (true, false) | (false, true) => PointeeMatch::Void,
            (false, false) => pointees_match(slot_pointee, &value_pointee),
        };
        match verdict {
            PointeeMatch::Same => {}
            PointeeMatch::Void => self.push_pointer_mismatch(
                &expected,
                &found,
                "a `void*` converts only by a cast: `p as void*`, or `v as T*`, inside `unsafe`",
                span,
            ),
            PointeeMatch::Different => {
                let help = if pointee_is(slot_pointee, value_pointee.clone(), &[Primitive::Int, Primitive::I32])
                    || pointee_is(slot_pointee, value_pointee.clone(), &[Primitive::Uint, Primitive::U32])
                {
                    "a Jux `int` is pointer-sized, so `int*` and `i32*` point at different widths; use the fixed width the memory has"
                } else {
                    "pointer types never convert implicitly; a cast (`p as T*`, inside `unsafe`) reinterprets the memory"
                };
                self.push_pointer_mismatch(&expected, &found, help, span);
            }
        }
        true
    }

    fn push_pointer_mismatch(&mut self, expected: &str, found: &str, help: &str, span: Span) {
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0410_TypeMismatch,
                format!("mismatched pointer types: expected `{expected}`, found `{found}`"),
            )
            .with_span(span)
            .with_help(help),
        );
    }

    /// §L.6.1a: `&e` needs a place, and a class object's field is reached
    /// through `&obj`, not `&obj.field`.
    fn check_address_of(&mut self, u: &juxc_ast::UnaryExpr) {
        let operand = u.operand.as_ref();
        let problem = match operand {
            Expr::Path(_) | Expr::Index(_) | Expr::This(_) => None,
            Expr::Unary(inner) if inner.op == juxc_ast::UnaryOp::Deref => None,
            Expr::Field(f) => {
                let owner = infer_expr(&f.object, &self.env, self.symbols);
                match &owner {
                    Ty::User { name, .. } if self.symbols.classes.get(name).is_some_and(|c| !c.is_struct) => Some(format!(
                        "`&` cannot take the address of a field of a class object; take `&` of the object itself (a `{}*` reaches its fields, §L.6.5)",
                        name.rsplit('.').next().unwrap_or(name),
                    )),
                    _ => None,
                }
            }
            Expr::Literal(_) => Some("`&` needs a place with an address, and a literal has none; store it in a local first".to_string()),
            Expr::Call(_) => Some("`&` needs a place with an address, and a call's result has none; store it in a local first".to_string()),
            Expr::Binary(_) | Expr::Unary(_) | Expr::Ternary(_) | Expr::Cast(_) => {
                Some("`&` needs a place with an address, and a computed value has none; store it in a local first".to_string())
            }
            _ => None,
        };
        if let Some(message) = problem {
            self.diagnostics.push(
                Diagnostic::error(code::Code::E0516_AddressOfNonPlace, message).with_span(u.span),
            );
        }
    }

    /// §L.6.1a: `*e` needs a typed pointer.
    fn check_deref(&mut self, u: &juxc_ast::UnaryExpr) {
        let depth = self.expr_ptr_depth(&u.operand);
        let pointee = infer_expr(&u.operand, &self.env, self.symbols);
        if depth == 0 {
            if matches!(pointee, Ty::Unknown | Ty::Param(_) | Ty::FnPtr { .. }) {
                return;
            }
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0517_DerefOfNonPointer,
                    format!("`*` reads through a pointer, and this value is not one: its type is `{pointee}`"),
                )
                .with_span(u.span),
            );
        } else if depth == 1 && (matches!(pointee, Ty::Void) || crate::infer::pointer_base_is_void(&u.operand, &self.env, self.symbols)) {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0517_DerefOfNonPointer,
                    "a `void*` has no pointee type to read; cast it to a typed pointer first (`p as T*`)",
                )
                .with_span(u.span),
            );
        }
    }

    /// §L.6.1a: the arithmetic and comparisons pointers have, and those they do not.
    fn check_pointer_operators(&mut self, b: &juxc_ast::BinaryExpr) {
        let (ld, rd) = (self.expr_ptr_depth(&b.left), self.expr_ptr_depth(&b.right));
        if ld == 0 && rd == 0 {
            return;
        }
        let span = b.span;
        match b.op {
            BinaryOp::Eq | BinaryOp::NotEq => {
                let null_side = matches!(*b.left, Expr::Literal(juxc_ast::Literal::Null))
                    || matches!(*b.right, Expr::Literal(juxc_ast::Literal::Null));
                if null_side {
                    return;
                }
                let lt = infer_expr(&b.left, &self.env, self.symbols);
                let rt = infer_expr(&b.right, &self.env, self.symbols);
                if matches!(lt, Ty::FnPtr { .. }) || matches!(rt, Ty::FnPtr { .. }) {
                    return;
                }
                if ld != rd || pointees_match(&lt, &rt) != PointeeMatch::Same {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!(
                                "cannot compare `{}` with `{}`: a pointer compares only with a pointer of the same type, or `null`",
                                pointer_type_text(&lt, ld),
                                pointer_type_text(&rt, rd),
                            ),
                        )
                        .with_span(span),
                    );
                }
            }
            BinaryOp::Add | BinaryOp::Sub => {
                if ld > 0 && rd > 0 {
                    if b.op == BinaryOp::Add {
                        self.push_pointer_op("two pointers cannot be added; subtract them for the distance, or step one by an integer", span);
                    } else {
                        let lt = infer_expr(&b.left, &self.env, self.symbols);
                        let rt = infer_expr(&b.right, &self.env, self.symbols);
                        if ld != rd || pointees_match(&lt, &rt) != PointeeMatch::Same {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0410_TypeMismatch,
                                    format!(
                                        "`q - p` needs two pointers of the same type, found `{}` and `{}`",
                                        pointer_type_text(&lt, ld),
                                        pointer_type_text(&rt, rd),
                                    ),
                                )
                                .with_span(span),
                            );
                        }
                    }
                    return;
                }
                let (pointer, step, depth) = if ld > 0 { (&b.left, &b.right, ld) } else { (&b.right, &b.left, rd) };
                if ld == 0 && b.op == BinaryOp::Sub {
                    self.push_pointer_op("an integer minus a pointer has no meaning; write `p - n`", span);
                    return;
                }
                let pointee = infer_expr(pointer, &self.env, self.symbols);
                if depth == 1 && (matches!(pointee, Ty::Void) || crate::infer::pointer_base_is_void(pointer, &self.env, self.symbols)) {
                    self.push_pointer_op("a `void*` has no element size to step by; cast it to a typed pointer first", span);
                    return;
                }
                let step_ty = infer_expr(step, &self.env, self.symbols);
                let integer = match &step_ty {
                    Ty::Primitive(p) => crate::ty::integer_bits(*p).is_some(),
                    Ty::Unknown | Ty::Param(_) => true,
                    _ => false,
                };
                if !integer {
                    self.push_pointer_op(&format!("a pointer steps by an integer count of elements, and this step is a `{step_ty}`"), span);
                }
            }
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor
            | BinaryOp::Shl | BinaryOp::Shr => {
                self.push_pointer_op(
                    "this operator is not defined on pointers; convert to an integer with `p as ulong` inside `unsafe` if the address arithmetic is intended",
                    span,
                );
            }
            _ => {}
        }
    }

    fn push_pointer_op(&mut self, message: &str, span: Span) {
        self.diagnostics.push(
            Diagnostic::error(code::Code::E0518_InvalidPointerOperation, message.to_string()).with_span(span),
        );
    }

    /// E0506 for a raw-pointer operation outside `unsafe` (§L.6.2): the same
    /// rule, and the same wording, as `*p` and `&x`.
    fn unsafe_pointer_op(&mut self, what: &str, span: Span) {
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0506_UnsafeOpOutsideUnsafe,
                format!(
                    "{what} requires an `unsafe` block; wrap it in `unsafe {{ … }}` \
                     or mark the enclosing function `unsafe`",
                ),
            )
            .with_span(span),
        );
    }

    /// How many raw-pointer levels `e` has (`int*` is 1), or 0 for a value
    /// that is not a pointer. See [`crate::infer::pointer_depth`].
    pub(crate) fn expr_ptr_depth(&self, e: &Expr) -> u8 {
        crate::infer::pointer_depth(e, &self.env, self.symbols)
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
                // Const slot got a non-literal arg. It may still be a const
                // EXPRESSION that reduces to a value — a bare const name
                // (`new Ring<float, SIZE>()`). Evaluate it; accept if it folds
                // to a value of the matching kind. (The parser only produces a
                // type or a bare name in arg position, so arithmetic args like
                // `SIZE*2` are a parser follow-up.)
                (Some(cty), None) => {
                    let probe = Expr::Path(arg.name.clone());
                    let ctx = crate::const_eval::ConstCtx {
                        symbols: self.symbols,
                        generic_param_names: &self.const_param_names,
                        enclosing_class: None,
                    };
                    let param_is_bool = cty
                        .name
                        .segments
                        .last()
                        .map(|s| s.text == "bool")
                        .unwrap_or(false);
                    let folds = if param_is_bool {
                        crate::const_eval::eval_const_bool(&probe, &ctx).is_ok()
                    } else {
                        crate::const_eval::eval_const_int(&probe, &ctx).is_ok()
                    };
                    if !folds {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0445_ConstGenericUnsupported,
                                format!(
                                    "`{}` is a const-generic parameter -- its argument must be a \
                                     compile-time constant (`4`, `true`, or a `const` value), not \
                                     a type or a runtime value",
                                    param.name.text,
                                ),
                            )
                            .with_span(arg.span),
                        );
                    }
                }
                // Const slot + literal: the literal's kind must match
                // the param's value type (`true` can't bind `<int N>`).
                (Some(cty), Some(lit)) => {
                    if let Some(why) = const_arg_mismatch(cty, lit) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0445_ConstGenericUnsupported,
                                format!(
                                    "const-generic argument `{lit}` doesn't fit `{}`: {why}",
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
        let Some(Expr::Lambda(l)) = args.first() else {
            return;
        };
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
            let Some(ty) = self.env.lookup(&name).cloned() else {
                continue;
            };
            let Some(why) = self.worker_capture_blocker(&ty, 0) else {
                continue;
            };
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0702_ObjectCapturedBySpawn,
                    format!("`{name}` cannot be captured by a `Worker.spawn` closure: {why}"),
                )
                .with_span(span),
            );
        }
        // **`this`, written or implied.** A method's closure that reads a field
        // or calls a method of its own class captures the object, exactly as
        // `this.total` would. The class then crosses the boundary like any
        // other captured object, so the same blocker applies to it.
        if let Some(span) = self.worker_lambda_this_capture(l) {
            if let Some(class) = self.env.current_class.clone() {
                let bare = class.rsplit('.').next().unwrap_or(&class).to_string();
                if let Some(why) = self.symbols.worker_share_blocker(&bare) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0702_ObjectCapturedBySpawn,
                            format!(
                                "`this` cannot be captured by a `Worker.spawn` closure: {why}. A class whose members are all shareable is upgraded to an atomic handle automatically; move the unshareable member out, or copy what the worker needs into a local first",
                            ),
                        )
                        .with_span(span),
                    );
                }
            }
        }
    }

    /// Where a `Worker.spawn` closure reaches `this`: an explicit `this`, or a
    /// bare name that is a field, property or method of the enclosing class
    /// and not a local or one of the closure's own parameters. `None` when it
    /// does not, or when there is no enclosing class.
    fn worker_lambda_this_capture(&self, l: &juxc_ast::LambdaExpr) -> Option<Span> {
        let class = self.env.current_class.clone()?;
        let params: std::collections::HashSet<&str> =
            l.params.iter().map(|p| p.name.text.as_str()).collect();
        let mut found: Option<Span> = None;
        let mut visit = |e: &Expr| {
            if found.is_some() {
                return;
            }
            match e {
                Expr::This(span) => found = Some(*span),
                Expr::Path(qn) if qn.segments.len() == 1 => {
                    let name = qn.segments[0].text.as_str();
                    if params.contains(name) || self.env.lookup(name).is_some() {
                        return;
                    }
                    let member = self.symbols.lookup_field(&class, name).is_some()
                        || self.symbols.lookup_method(&class, name).is_some()
                        || self
                            .symbols
                            .resolve_class(&class)
                            .is_some_and(|(_, c)| c.properties.contains_key(name));
                    if member {
                        found = Some(qn.span);
                    }
                }
                _ => {}
            }
        };
        match &l.body {
            juxc_ast::LambdaBody::Expr(e) => juxc_ast::visit::for_each_expr_in(e, &mut visit),
            juxc_ast::LambdaBody::Block(b) => juxc_ast::visit::for_each_expr(b, &mut visit),
        }
        found
    }

    /// Why a value of type `ty` cannot be captured by a `Worker.spawn`
    /// closure, or `None` when it is transferable (JUX-ASYNC-ADDENDUM §18.2).
    ///
    /// Transferable: primitives, `String`, tuples and records of transferable
    /// values, the async runtime's own handles, classes (upgraded to an atomic
    /// handle unless a member cannot come along), and collections and arrays of
    /// transferable values, which the worker receives as its own copy. Not
    /// transferable: function values and interface handles (single-threaded
    /// shared references), streams (task-local, §18.6.1), and a collection of
    /// collections, which a one-level copy cannot carry.
    fn worker_capture_blocker(&self, ty: &Ty, depth: usize) -> Option<String> {
        if depth > 8 {
            return None;
        }
        match ty {
            Ty::Nullable(inner) => self.worker_capture_blocker(inner, depth + 1),
            Ty::Fn { .. } => Some(
                "it is a function value, which is a single-threaded shared reference; call it before spawning and capture the result, or write the work inside the worker closure".to_string(),
            ),
            Ty::Array { element, .. } => self.worker_element_blocker(element, depth),
            Ty::User { name, generic_args } => {
                let bare = name.rsplit('.').next().unwrap_or(name);
                if Self::capture_is_thread_safe(ty) {
                    return None;
                }
                if bare == "__tuple" {
                    return generic_args.iter().find_map(|a| self.worker_capture_blocker(a, depth + 1));
                }
                if self.symbols.is_builtin_stream(name) {
                    return Some("it is a stream, which is task-local (§18.6.1); send its elements through a `Channel` instead".to_string());
                }
                if self.symbols.is_interface_name(name) {
                    return Some(format!(
                        "an interface handle (`{bare}`) is a single-threaded shared reference; capture the concrete class instead",
                    ));
                }
                if self.symbols.is_rust_collection(name) {
                    return generic_args.iter().find_map(|a| self.worker_element_blocker(a, depth));
                }
                if let Some((_, record)) = self.symbols.resolve_record(name) {
                    for c in &record.components {
                        let head = c.ty.name.segments.last().map(|s| s.text.as_str()).unwrap_or("");
                        let why = if let Some(why) = self.symbols.typeref_share_blocker(&c.ty) {
                            why.to_string()
                        } else if c.ty.array_shape.is_some() || self.symbols.is_rust_collection(head) {
                            "a collection, which a record carries as a shared handle".to_string()
                        } else {
                            continue;
                        };
                        return Some(format!(
                            "`{bare}.{}` holds {why}; copy the values the worker needs into locals first",
                            c.name,
                        ));
                    }
                    return None;
                }
                if self.symbols.resolve_class(name).is_some_and(|(_, c)| !c.is_external) {
                    return self.symbols.worker_share_blocker(bare).map(|why| {
                        format!(
                            "{why}. A class whose members are all shareable is upgraded to an atomic handle automatically; move the unshareable member out, or pass data in and return results out",
                        )
                    });
                }
                None
            }
            _ => None,
        }
    }

    /// The element type of a captured collection or array. The worker gets a
    /// copy of the collection one level deep, so each element must itself be
    /// transferable, and must not be another collection.
    fn worker_element_blocker(&self, element: &Ty, depth: usize) -> Option<String> {
        let mut inner = element;
        while let Ty::Nullable(t) = inner {
            inner = t;
        }
        let nested = match inner {
            Ty::Array { .. } => true,
            Ty::User { name, .. } => self.symbols.is_rust_collection(name),
            _ => false,
        };
        if nested {
            return Some(
                "it holds collections, and a worker receives a copy of a collection only one level deep; flatten the data or copy it into records first".to_string(),
            );
        }
        self.worker_capture_blocker(element, depth + 1)
            .map(|why| format!("its elements cannot come along: {why}"))
    }

    /// Resolve `e` as a PROPERTY ACCESS (`recv.PropName` /
    /// `Class.PropName`), returning the property's declared type and
    /// its `Class.Prop` label for diagnostics. `None` when `e` isn't a
    /// property access (then the bind call falls through to the
    /// ordinary method checks).
    fn property_access_ty(&mut self, e: &Expr) -> Option<(Ty, String)> {
        let Expr::Field(f) = e else { return None };
        // A property may already have been rewritten to its backing slot:
        // inside a constructor `this.Shown` reads `this.__prop_Shown` (see
        // `juxc_ast::desugar`), and it is still that property.
        let prop_name = f
            .field
            .text
            .strip_prefix("__prop_")
            .unwrap_or(f.field.text.as_str());
        // Same class-resolution ladder as the E0970/E0972 write checks:
        // a static `Class.Prop` path, else the receiver's inferred type.
        let class_fqn: Option<String> = if let Expr::Path(qn) = f.object.as_ref() {
            crate::infer::path_resolves_to_class(qn, &self.env, self.symbols).or_else(|| {
                match infer_expr(&f.object, &self.env, self.symbols) {
                    Ty::User { name, .. } => self.resolve_class_fqn(&name),
                    _ => None,
                }
            })
        } else {
            match infer_expr(&f.object, &self.env, self.symbols) {
                Ty::User { name, .. } => self.resolve_class_fqn(&name),
                _ => None,
            }
        };
        let class_fqn = class_fqn?;
        // Up the `extends` chain, not just the class itself: an INHERITED
        // property is bound and observed exactly like a declared one, and
        // reading only `properties` missed it -- so the E0974 same-type check
        // silently skipped every binding of an inherited property.
        let prop = self
            .symbols
            .lookup_property(&class_fqn, prop_name)
            .map(|(p, _)| p.clone())?;
        let ty = ty_from_ref(&prop.ty, &self.env, self.symbols);
        let bare = class_fqn.rsplit('.').next().unwrap_or(&class_fqn);
        Some((ty, format!("{bare}.{prop_name}")))
    }

    /// E0705 (§18.1.2): a call to an async callee outside a
    /// future-consuming slot is an unstarted future used as a value —
    /// the body never runs. Read-only on `in_future_slot` (no take):
    /// a nested async call inside an exempt position stays exempt,
    /// which trades a rare false negative (rustc still backstops it)
    /// for zero false positives on branchy await operands.
    fn flag_unawaited_async_call(&mut self, callee_desc: &str, is_async: bool, span: Span) {
        if is_async && !self.in_future_slot {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0705_AsyncCallNotAwaited,
                    format!(
                        "`{callee_desc}` is async -- calling it produces an unstarted future and the body never runs; `await` the call (§18.1.2), or pass it to `spawn(...)` to run it as a task",
                    ),
                )
                .with_span(span),
            );
        }
    }

    fn check_call(&mut self, c: &CallExpr) {
        // `System.out.println(x)` out of Java habit: there is no `System`
        // (unless the program declares one), and it used to reach rustc as
        // "cannot find value `System`" (JUX-DIAGNOSTICS-ADDENDUM "Java Habits").
        if let Some((span, help)) = self.java_system_out_call(c) {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0301_NameNotFound,
                    format!("cannot find `System` in this scope -- {help}"),
                )
                .with_span(span),
            );
            for arg in &c.args {
                self.check_expr(arg);
            }
            return;
        }
        if let Expr::Field(f) = c.callee.as_ref() {
            self.check_nullable_receiver(f, true);
        }
        // A call THROUGH a function pointer (§L.6.4): `f(x)` on a local or
        // parameter, `table.name(x)` on a field. It is checked here in full,
        // because the rest of this function would look for a function or a
        // method with that name and find none.
        // `(*p).method()` on a class pointer (§L.6.5): the payload has fields,
        // not methods.
        if let Expr::Field(f) = c.callee.as_ref() {
            if let Expr::Unary(u) = f.object.as_ref() {
                if u.op == juxc_ast::UnaryOp::Deref {
                    if let Ty::User { name, .. } = infer_expr(&f.object, &self.env, self.symbols) {
                        let is_handle_class = self
                            .symbols
                            .classes
                            .get(&name)
                            .is_some_and(|class| !class.is_struct && !class.is_external);
                        if is_handle_class && self.symbols.lookup_method(&name, &f.field.text).is_some() {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0515_MethodThroughClassPointer,
                                    format!(
                                        "`{}` is a method of `{name}`, and a class pointer reaches the \
                                         object's fields, not its methods -- call it on a handle to the \
                                         object instead",
                                        f.field.text,
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
                }
            }
        }
        if let Ty::FnPtr { params, param_ptr_depths, .. } = infer_expr(&c.callee, &self.env, self.symbols) {
            self.check_fn_pointer_call(c, &params, &param_ptr_depths);
            return;
        }
        // §P.4.2/§P.4.3 — `target.X.bind(source.Y)` /
        // `bindBidirectional`: both ends must be properties of the
        // SAME declared type (E0974). Checked here so the mismatch
        // surfaces at the bind site instead of leaking a rustc error
        // from the emitted binding closure.
        if let Expr::Field(opf) = c.callee.as_ref() {
            if matches!(opf.field.text.as_str(), "bind" | "bindBidirectional") && c.args.len() == 1
            {
                if let Some((t_ty, t_label)) = self.property_access_ty(&opf.object) {
                    if let Some((s_ty, s_label)) = self.property_access_ty(&c.args[0]) {
                        if t_ty != s_ty {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0974_BindTypeMismatch,
                                    format!(
                                        "cannot {} `{t_label}` ({t_ty}) to `{s_label}` ({s_ty}) -- bound properties must have the same type (§P.4.3)",
                                        opf.field.text,
                                    ),
                                )
                                .with_span(c.span),
                            );
                        }
                    }
                }
            }
        }
        // §P.2.2 / §P.3.2 — `<prop>.observers.attach(lambda)` with an
        // inline lambda: the lambda must take 0, 2, or 3 parameters
        // (E0975). Other arg shapes (named observer variables) were
        // validated at their declaration.
        if let Expr::Field(opf) = c.callee.as_ref() {
            if matches!(opf.field.text.as_str(), "attach" | "detach") {
                if let Expr::Field(obsf) = &*opf.object {
                    if obsf.field.text == "observers" {
                        if let Some(Expr::Lambda(l)) = c.args.first() {
                            if !matches!(l.params.len(), 0 | 2 | 3) {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0975_ObserverShapeMismatch,
                                        format!(
                                            "this observer lambda takes {} parameter{} -- an \
                                             observer is `() -> …` (invalidation), \
                                             `(old, now) -> …`, or `(prop, old, now) -> …` \
                                             (§P.2.2)",
                                            l.params.len(),
                                            if l.params.len() == 1 { "" } else { "s" },
                                        ),
                                    )
                                    .with_span(c.span),
                                );
                            }
                        }
                    }
                }
            }
        }
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
                let Some(class_name) = self.env.current_class.clone() else {
                    return;
                };
                // A record's additional constructor delegates among the
                // record's constructors, the canonical one at index 0.
                if let Some(record) = self.symbols.records.get(&class_name) {
                    let ctors = record.constructors.clone();
                    let subst_params = record.generic_params.clone();
                    match self.select_ctor_typed(&ctors, &c.args) {
                        Some(k) if k == current_idx => {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0413_UnresolvedMethod,
                                    "`this(...)` resolves to the declaring constructor itself -- a constructor can't delegate to itself",
                                )
                                .with_span(c.span),
                            );
                            for arg in &c.args {
                                self.check_expr(arg);
                            }
                        }
                        Some(k) => {
                            self.ctor_selections.insert(c.span, k);
                            self.check_call_args(
                                &format!("this (={class_name} constructor)"),
                                &ctors[k].params,
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
                                        "no constructor of record `{class_name}` accepts {} argument{}",
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
                let Some(class) = self.symbols.classes.get(&class_name) else {
                    return;
                };
                let selected = self.select_ctor_typed(&class.constructors, &c.args);
                match selected {
                    Some(k) if k == current_idx => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0413_UnresolvedMethod,
                                "`this(...)` resolves to the declaring constructor itself -- a constructor can't delegate to itself",
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
                    // `spawn(asyncFn())` consumes the future (E0705 exempt).
                    let prev_slot = self.in_future_slot;
                    self.in_future_slot = true;
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    self.in_future_slot = prev_slot;
                    return;
                }
                // Built-in functions accept anything — including bare
                // futures (`block_on(f())`, `parallel(a(), b())`,
                // `withTimeout(ms, f())`), so their args are
                // future-consuming positions (E0705 exempt).
                if BUILTINS.contains(&name.as_str()) {
                    let prev_slot = self.in_future_slot;
                    self.in_future_slot = true;
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    self.in_future_slot = prev_slot;
                    return;
                }
                // **Lexical shadowing.** A LOCAL variable / parameter
                // of this name shadows any same-named top-level free
                // function — `f(...)` then calls the local closure, not
                // the global `f`. Without this, a user free function
                // named `f` would hijack a call to a local lambda
                // parameter `f` (e.g. inside the std `assertThrows`),
                // surfacing a bogus arg-count error. A function-typed
                // local falls through to the closure-call path below.
                let shadowed_by_local = self.env.lookup(name.as_str()).is_some();
                // Resolve the callee FQN: an exact bare key (same-package free
                // function), or an imported FQN — a foreign (`rust.libc.getpid`)
                // or cross-package free function brought into scope via
                // `import a.b.f`, keyed in the table by its full path.
                let resolved_fqn = if shadowed_by_local {
                    None
                } else if self.symbols.functions.contains_key(name.as_str()) {
                    Some(name.to_string())
                } else {
                    self.env
                        .unqualified
                        .get(name.as_str())
                        .cloned()
                        .filter(|fqn| self.symbols.functions.contains_key(fqn))
                };
                if let Some(fqn) = resolved_fqn {
                    // An overloaded free function (§T.3.1) resolves to one
                    // member of its group; record which, so the backend emits
                    // the matching `name__ovK`, and check the arguments
                    // against THAT member rather than against member 0.
                    let picked =
                        crate::infer::select_function_overload_typed(self.symbols, &fqn, c, &self.env);
                    if let Some(group) = self.symbols.function_overload_group(&fqn) {
                        let lists: Vec<&[ParamSig]> = group.iter().map(|fs| fs.params.as_slice()).collect();
                        self.report_ambiguous_overload(name, &lists, c);
                    }
                    if let Some((k, _)) = &picked {
                        self.function_selections.insert(c.span, *k); self.function_selections.insert(expr_span(&c.callee), *k);
                    }
                    let fn_sig = match &picked {
                        Some((_, s)) => s,
                        None => self
                            .symbols
                            .functions
                            .get(&fqn)
                            .expect("resolved_fqn is a known function key"),
                    };
                    let params = fn_sig.params.clone();
                    let generic_params = fn_sig.generic_params.clone();
                    let callee_unsafe = fn_sig.is_unsafe;
                    // A C-variadic foreign fn (`printf(String, ...)`) accepts
                    // any number of trailing args beyond its fixed params.
                    let callee_c_variadic = fn_sig.is_c_variadic;
                    let callee_async =
                        matches!(fn_sig.return_type, juxc_ast::ReturnType::AsyncType(_),);
                    // §X.1.3 propagation: the callee's declared
                    // checked throws raise here.
                    let callee_throws = fn_sig.throws.clone();
                    self.record_callee_throws(&callee_throws, c.span);
                    let callee_wheres = fn_sig.wheres.clone();
                    // §A.2.8: calling an `unsafe` fn needs an `unsafe` context.
                    self.require_unsafe_context(callee_unsafe, name, c.span);
                    // §18.1.2: an async call must be awaited (E0705).
                    self.flag_unawaited_async_call(name, callee_async, c.span);
                    // Validate any explicit `<…>` turbofish against the
                    // callee's declared type params (E0443). The typed
                    // `assertThrows<E>` form checks its own type argument, and
                    // is otherwise the plain library call.
                    let typed_assert_throws = self.check_typed_assert_throws(&fqn, c);
                    self.check_explicit_type_args(
                        if typed_assert_throws { &[] } else { &c.explicit_generic_args },
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
                    let (subst_params, subst_args): (Vec<TypeParam>, Vec<Ty>) = if generic_params
                        .is_empty()
                    {
                        (Vec::new(), Vec::new())
                    } else {
                        let param_tys: Vec<&TypeRef> = params.iter().map(|p| &p.ty).collect();
                        let arg_tys: Vec<Ty> = c
                            .args
                            .iter()
                            .map(|a| infer_expr(a, &self.env, self.symbols))
                            .collect();
                        let inferred = infer_generic_args(&generic_params, &param_tys, &arg_tys);
                        self.report_generic_conflict(name, &generic_params, &param_tys, &arg_tys, c);
                        let args: Vec<Ty> = generic_params
                            .iter()
                            .map(|p| inferred.get(&p.name.text).cloned().unwrap_or(Ty::Unknown))
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
                    // §T.4 (E0446): …and the generic params' declared
                    // `extends` bounds. Free functions have no
                    // declaring type — bounds lower in the empty
                    // scope, which resolves package-level names fine.
                    self.enforce_generic_bounds(
                        &subst_params,
                        &subst_args,
                        "",
                        &format!("function `{name}`"),
                        c.span,
                    );
                    if callee_c_variadic && c.args.len() > params.len() {
                        // C-variadic call with extra args: the fixed prefix gets
                        // the normal per-slot type checks; each trailing arg (the
                        // C `...`) is walked for its own errors but is otherwise
                        // unconstrained (its type is the caller's to get right).
                        let fixed = params.len();
                        self.check_call_args(
                            name,
                            &params,
                            &c.args[..fixed],
                            &c.arg_names[..fixed],
                            c.span,
                            None,
                            &subst_params,
                            &subst_args,
                        );
                        for extra in &c.args[fixed..] {
                            self.check_expr(extra);
                        }
                    } else {
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
                    }
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
                // `x.operator hash()` / `x.operator string()` (§O.2.7): every
                // value has both, as its own operator or the built-in one, so
                // there is no method to look up. Only the receiver is checked.
                if crate::infer::named_operator_call_type(c).is_some() {
                    self.check_expr(&field.object);
                    if method_name == "operator hash"
                        && !self.check_any_receiver(&field.object, "has no hash", c.span)
                    {
                        // All but a few values have `operator hash` (§O.2.7): a
                        // function value, an array and a collection have none, nor
                        // does a type holding one (E0933, the hash-key rule).
                        let receiver = self.infer_and_record(&field.object);
                        if let Some(why) = self.symbols.hash_blocker(&receiver) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0933_KeyHasNoHash,
                                    format!("`{receiver}` has no `operator hash`: {why} (§O.2.7)"),
                                )
                                .with_span(field.field.span),
                            );
                        }
                    }
                    return;
                }
                // `v.foo()` on an `any` (§T.1.2): it has no methods.
                if self.check_any_receiver(&field.object, &format!("has no method `{method_name}`"), c.span) {
                    self.check_expr(&field.object);
                    for a in &c.args {
                        self.check_expr(a);
                    }
                    return;
                }
                // `weakField.get()` (§6.5): a zero-arg `.get()` on a weak-field
                // access is the valid weak→strong promotion (typed `T?` by
                // `infer`). It has no backing method, so accept it here — check
                // only the underlying receiver and return, skipping both method
                // resolution (which would emit E0413 on the field's own class)
                // and the bare-weak-read guard (E0456) that the receiver would
                // otherwise trip.
                if method_name == "get" && c.args.is_empty() {
                    if let Expr::Field(inner) = field.object.as_ref() {
                        let inner_recv = infer_expr(&inner.object, &self.env, self.symbols);
                        if let Ty::User { name, .. } = &inner_recv {
                            if self
                                .symbols
                                .lookup_field(name, &inner.field.text)
                                .is_some_and(|(fs, _)| fs.is_weak)
                            {
                                self.check_expr(&inner.object);
                                return;
                            }
                        }
                    }
                    // `weakParam.get()` (§M.14.3): the weak→strong promotion on a
                    // weak parameter. Accept it without resolving a `.get()`
                    // method and WITHOUT walking the receiver path (which would
                    // trip the bare-weak-read gate, E0456).
                    if let Expr::Path(qn) = field.object.as_ref() {
                        if qn.segments.len() == 1
                            && self.env.weak_names.contains(&qn.segments[0].text)
                        {
                            return;
                        }
                    }
                }
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
                //
                // `Worker` is an empty stub class — `spawn` has no method
                // entry in the symbol table. Return early so the method-
                // resolution paths below don't emit a spurious E0413.
                if method_name == "spawn" {
                    if let Expr::Path(qn) = field.object.as_ref() {
                        if qn.segments.len() == 1 && qn.segments[0].text == "Worker" {
                            self.check_spawn_captures(&c.args);
                            let prev_slot = self.in_future_slot;
                            self.in_future_slot = true;
                            for arg in &c.args {
                                self.check_expr(arg);
                            }
                            self.in_future_slot = prev_slot;
                            return;
                        }
                    }
                }
                // `Task.all/race/delay` (§18.1.4) — runtime statics on
                // the emitted helpers; args are task handles (or a
                // millisecond count for delay) — future-consuming
                // positions (E0705 exempt).
                if let Expr::Path(qn) = field.object.as_ref() {
                    if qn.segments.len() == 1
                        && qn.segments[0].text == "Task"
                        && matches!(method_name, "all" | "race" | "any" | "allSettled" | "delay")
                    {
                        let prev_slot = self.in_future_slot;
                        self.in_future_slot = true;
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        self.in_future_slot = prev_slot;
                        return;
                    }
                    // `Stream.of/from/generate` (§18.6.4) — statics on
                    // the emitted JuxStream helper. `generate`'s single
                    // argument must be a zero-parameter lambda (the
                    // pull-driven producer).
                    if qn.segments.len() == 1
                        && qn.segments[0].text == "Stream"
                        && !self.symbols.classes.contains_key("Stream")
                        && matches!(method_name, "of" | "from" | "generate")
                    {
                        if method_name == "generate" {
                            let ok = matches!(
                                c.args.first(),
                                Some(Expr::Lambda(l)) if l.params.is_empty()
                            ) && c.args.len() == 1;
                            if !ok {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0410_TypeMismatch,
                                        "`Stream.generate` takes exactly one zero-parameter async lambda -- `Stream.generate(async () -> …)` (§18.6.4)",
                                    )
                                    .with_span(c.span),
                                );
                            }
                        }
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
                    if let Some(class_fqn) =
                        crate::infer::path_resolves_to_class(qn, &self.env, self.symbols)
                    {
                        let class_method = match crate::infer::select_method_overload_typed(
                            self.symbols,
                            &class_fqn,
                            method_name,
                            c,
                            &self.env,
                        ) {
                            Some((k, picked)) => {
                                self.method_selections.insert(c.span, k); self.method_selections.insert(expr_span(&c.callee), k);
                                Some(picked)
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
                                self.require_unsafe_context(method.is_unsafe, method_name, c.span);
                                // §18.1.2: async static call must be
                                // awaited (E0705).
                                let static_is_async = matches!(
                                    method.return_type,
                                    juxc_ast::ReturnType::AsyncType(_),
                                );
                                self.flag_unawaited_async_call(
                                    method_name,
                                    static_is_async,
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
                                format!("no static method `{method_name}` on class `{class_fqn}`",),
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
                    if let Some(iface_fqn) =
                        crate::infer::path_resolves_to_interface(qn, &self.env, self.symbols)
                    {
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
                // `I.super.m(...)` (§T.8.3): check where it is written and that
                // `m` has a default to run, then let the ordinary interface-method
                // path check the arguments against `I`'s signature.
                let super_call = match field.object.as_ref() {
                    Expr::Field(recv) => {
                        crate::infer::interface_super_receiver(recv, &self.env, self.symbols)
                            .map(|(iface, _)| iface)
                    }
                    _ => None,
                };
                if let Some(iface) = &super_call {
                    self.check_interface_super_call(iface, method_name, c);
                }
                // Walk the receiver sub-expression first.
                self.in_interface_super_call = super_call.is_some();
                self.check_expr(&field.object);
                self.in_interface_super_call = false;
                let receiver_ty = infer_expr(&field.object, &self.env, self.symbols);
                // A NULLABLE receiver is checked against the type it wraps.
                // `?.` and `!!` both reach the same members, so the member
                // question is the same question. Without this the arms below
                // all miss and the call falls through unchecked: every method
                // name passed on a `T?`, and `m.get(k).orElse(0)` type-checked
                // and then failed in rustc, naming a Rust method the
                // programmer never wrote.
                let receiver_ty = match receiver_ty {
                    Ty::Nullable(inner) => *inner,
                    other => other,
                };
                // **A PRIMITIVE receiver.** `int`, `double` and `char` each
                // carry a method surface, and nothing checked a name against
                // it: `x.totallyNotAMethod()` type-checked and then failed in
                // rustc. The names come from the same table the backend lowers
                // from, so the two cannot disagree about what exists.
                // **A property's binding methods.** `celsius.bind(other.celsius)`
                // has a PROPERTY as its receiver, and a property's value type is
                // whatever it holds -- often a primitive, which has no `bind`.
                // The binding itself is checked above (E0974).
                if PROPERTY_BINDING_METHODS.contains(&method_name)
                    && self.property_access_ty(&field.object).is_some()
                {
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    return;
                }
                if let Ty::Primitive(prim) = &receiver_ty {
                    if let Some(names) = builtin_primitive_methods(*prim) {
                        if !names.contains(&method_name) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0413_UnresolvedMethod,
                                    format!(
                                        "no method `{method_name}` on `{}`",
                                        receiver_ty
                                    ),
                                )
                                .with_span(c.span),
                            );
                        }
                    }
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    return;
                }
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
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    if BUILTIN_STRING_METHODS.contains(&method_name) {
                        return;
                    }
                    // **The SCANNED Rust `String` counts too.** Its surface is
                    // discovered from rustdoc, not listed here, so `push_str`
                    // and everything else Rust's `String` carries resolves
                    // without this file naming them. Consulting the scan is
                    // what keeps the check honest as the std moves.
                    if self
                        .symbols
                        .classes
                        .iter()
                        .filter(|(k, _)| k.rsplit('.').next() == Some("String"))
                        .any(|(_, c)| c.methods.contains_key(method_name))
                    {
                        return;
                    }
                    // **Anything
                    // else is a real miss. It used to fall through with no
                    // diagnostic at all and
                    // land on rustc as "no method named `equals` found for
                    // struct `String`" -- a Rust error about a Jux program.
                    //
                    // `equals` and `compareTo` are the ones people reach for
                    // out of Java habit, and they have a real answer: `==` is
                    // value equality and `<=>` gives the ordering, so the hint
                    // names those rather than just refusing.
                    let hint = match method_name {
                        "equals" | "equalsIgnoreCase" => {
                            " -- `==` on a String is value equality (§7.14.3)"
                        }
                        "compareTo" => " -- use `<=>`, or the `<` / `>` operators it derives",
                        "toString" => " -- a String is already one; interpolate it directly",
                        "size" => " -- use `length()`",
                        _ => "",
                    };
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0413_UnresolvedMethod,
                            format!("no method `{method_name}` on `String`{hint}"),
                        )
                        .with_span(field.field.span),
                    );
                    return;
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
                    if bare == "Channel" && matches!(method_name, "send" | "receive" | "close") {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                    // Stream<T> (§18.6) — same builtin treatment: its
                    // methods live on the emitted JuxStream helper.
                    if bare == "Stream"
                        && !self.symbols.classes.contains_key("Stream")
                        && matches!(
                            method_name,
                            "next" | "mapAsync" | "filterAsync" | "take" | "skip" | "chain",
                        )
                    {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                }
                // **A bounded type parameter.** `<T extends Auto>` promises
                // exactly Auto's members, so a name Auto does not have is a
                // mistake the checker can see: `t.getSpee()` used to pass and
                // then fail in rustc as "no method named `getSpee` found for
                // type parameter `T`". An unbounded `T`, or a bound this
                // symbol table does not know, stays unchecked.
                if let Ty::Param(param) = &receiver_ty {
                    self.check_method_on_bounded_param(param, method_name, field.field.span);
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    return;
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
                {
                    let group = self.symbols.merged_method_overloads(&name, method_name);
                    if group.len() > 1 {
                        let lists: Vec<&[ParamSig]> = group.iter().map(|m| m.params.as_slice()).collect();
                        self.report_ambiguous_overload(method_name, &lists, c);
                    }
                }
                if let Some((method, declaring_class)) =
                    self.symbols.lookup_method(&name, method_name)
                {
                    // Overload-group pick (§T.3): the argument count
                    // selects when only one member accepts it; same-
                    // count members select by ARGUMENT TYPES. The
                    // recorded index drives `name__ovK` emission.
                    let method = match crate::infer::select_method_overload_typed(
                        self.symbols,
                        &name,
                        method_name,
                        c,
                        &self.env,
                    ) {
                        Some((k, picked)) => {
                            self.method_selections.insert(c.span, k); self.method_selections.insert(expr_span(&c.callee), k);
                            picked
                        }
                        None => method.clone(),
                    };
                    let method = &method;
                    let params = method.params.clone();
                    let method_generic_params = method.generic_params.clone();
                    let method_vis = method.visibility;
                    let method_throws = method.throws.clone();
                    self.record_callee_throws(&method_throws, c.span);
                    let method_is_static = method.is_static;
                    let method_is_unsafe = method.is_unsafe;
                    let method_is_async =
                        matches!(method.return_type, juxc_ast::ReturnType::AsyncType(_),);
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
                    self.check_visibility(method_vis, &owner_name, method_name, "method", c.span);
                    self.require_unsafe_context(method_is_unsafe, method_name, c.span);
                    // §18.1.2: an async method call must be awaited (E0705).
                    self.flag_unawaited_async_call(method_name, method_is_async, c.span);
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
                    // METHOD generic `extends` bounds (E0446) — check
                    // the inferred/explicit method type args against
                    // their declared bounds. The method's params sit
                    // at the TAIL of the substitution lists (class
                    // params were composed in first).
                    let method_n = method_generic_params.len();
                    if method_n > 0 && subst_args.len() >= method_n {
                        let tail_args = subst_args[subst_args.len() - method_n..].to_vec();
                        self.enforce_generic_bounds(
                            &method_generic_params,
                            &tail_args,
                            &owner_name,
                            &format!("method `{method_name}`"),
                            c.span,
                        );
                    }
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
                // args. The chain DOES matter: an interface extends other
                // interfaces, and a default body written against an
                // inherited signature (`Greeter extends Named` calling
                // `this.name()`) has to find it up there.
                if let Some(iface) = self.symbols.interfaces.get(&name) {
                    let found = iface.methods.get(method_name).cloned().or_else(|| {
                        crate::infer::inherited_interface_method_sig(
                            self.symbols,
                            iface,
                            method_name,
                        )
                    });
                    if let Some(method) = found.as_ref() {
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
                                        "`{method_name}` is a static method on `{shown}`; call it as `{shown}.{method_name}(...)`, not on an instance",
                                        shown = crate::ty::nested_type_spelling(&name),
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
                    // §M.5 synthesized wither: `r.with(field: v, …)`.
                    // Every argument must be NAMED and name a record
                    // component; values type-check against the
                    // component's type. A user-declared `with` method
                    // (below) shadows the synthesized one.
                    if method_name == "with" && !record.methods.contains_key("with") {
                        let components = record.components.clone();
                        for (i, arg) in c.args.iter().enumerate() {
                            self.check_expr(arg);
                            let Some(Some(arg_name)) = c.arg_names.get(i).map(|n| n.as_ref())
                            else {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0448_BadNamedArgument,
                                        format!(
                                            "`with(...)` takes NAMED arguments only -- write `{name}.with(field: value)` (§M.5)"
                                        ),
                                    )
                                    .with_span(c.span),
                                );
                                continue;
                            };
                            if !components.iter().any(|comp| comp.name == arg_name.text) {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0448_BadNamedArgument,
                                        format!(
                                            "`{}` is not a component of record `{name}` -- `with(...)` accepts: {}",
                                            arg_name.text,
                                            components
                                                .iter()
                                                .map(|comp| comp.name.as_str())
                                                .collect::<Vec<_>>()
                                                .join(", "),
                                        ),
                                    )
                                    .with_span(arg_name.span),
                                );
                            }
                        }
                        return;
                    }
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
                // Before emitting E0413: check whether `method_name` is a
                // **function-typed instance field** on the receiver's class.
                // `obj.field(args)` where `field: () -> T` is a valid call
                // through an `Rc<dyn Fn>` — not an unknown method.
                if let Some((fsig, _)) = self.symbols.lookup_field(&name, method_name) {
                    if fsig.ty.fn_shape.is_some() {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                }
                // `clone()` comes from Rust's `Clone` trait, which the scan
                // records on the TYPE (`@RustClone`) rather than as a method, so
                // it never appears in the method table. It is the one way to
                // copy a collection (§6.5.1), so it has to resolve.
                if method_name == "clone"
                    && c.args.is_empty()
                    && self.symbols.type_is_rust_clone(&name)
                {
                    return;
                }
                // A Java habit's help first (JUX-DIAGNOSTICS-ADDENDUM "Java
                // Habits"), else the near names from the receiver's surface.
                let hint = crate::java_habits::member_hint(
                    method_name,
                    c.args.len(),
                    &|m| self.type_has_method(&name, m),
                    &|m| self.type_has_readable(&name, m),
                )
                .unwrap_or_else(|| self.nearest_method_hint(&name, method_name));
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0413_UnresolvedMethod,
                        format!(
                            "no method `{method_name}` on type `{}`{hint}",
                            crate::ty::nested_type_spelling(&name),
                        ),
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
        // Instantiating a type is naming it: the same package rule applies.
        self.check_type_name_visibility(&n.class_name, n.span);
        let class_name = crate::infer::resolve_class_name(&n.class_name, &self.env, self.symbols);
        if class_name.is_empty() {
            return;
        }
        // An anonymous class's methods are checked like any class's, with
        // `this` typed as the type being implemented and the enclosing
        // body's locals still in scope (they are captures). Unchecked, their
        // expressions had no types, so `s.toUpperCase()` on a `String`
        // parameter reached rustc unlowered.
        if let Some(body) = &n.anonymous_body {
            let this_ty = Ty::User { name: class_name.clone(), generic_args: Vec::new() };
            let saved_return = self.current_return.take();
            for method in &body.methods {
                self.check_method(method, &this_ty);
            }
            self.current_return = saved_return;
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
                            "`{class_name}` declares const-generic parameters -- write them \
                             explicitly (`new {class_name}<…>(…)`); const values can't be \
                             inferred from constructor arguments",
                        ),
                    )
                    .with_span(n.span),
                );
            }
            // §T.4 `extends` bound enforcement (E0446): each resolved
            // generic argument — explicit or ctor-inferred — must
            // satisfy its parameter's declared bounds, so the
            // violation never leaks as rustc's E0277. Unknown args
            // (inference gaps) and Param-typed args (checked at THEIR
            // instantiation site) stay lenient.
            let resolved_args: Vec<Ty> = if !explicit_generic_args.is_empty() {
                explicit_generic_args.clone()
            } else {
                crate::infer::infer_ctor_generic_args(&class_name, &n.args, &self.env, self.symbols)
            };
            self.enforce_generic_bounds(
                &class_generic_params,
                &resolved_args,
                &class_name,
                &format!("class `{class_name}`"),
                n.span,
            );
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
            let selected = self
                .select_ctor_typed(&class.constructors, &n.args)
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
            self.check_visibility(ctor_vis, &class_name, "constructor", "constructor", n.span);
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
            // The canonical constructor (index 0, one parameter per
            // component) or an additional one (§7.6.1), picked by argument
            // types as a class's overloads are. Only a record that HAS
            // additional constructors records the pick: with the canonical
            // one alone every call is `new`.
            let selected = self.select_ctor_typed(&record.constructors, &n.args).unwrap_or(0);
            if record.constructors.len() > 1 {
                self.ctor_selections.insert(n.span, selected);
            }
            let params: Vec<ParamSig> =
                record.constructors.get(selected).map(|c| c.params.clone()).unwrap_or_default();
            let ctor_vis =
                record.constructors.get(selected).map(|c| c.visibility).unwrap_or(juxc_ast::Visibility::Public);
            let subst_params = record.generic_params.clone();
            self.check_visibility(ctor_vis, &class_name, "constructor", "constructor", n.span);
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

        let Some(child_name) = self.env.current_class.clone() else {
            return;
        };
        let Some(child) = self.symbols.classes.get(&child_name) else {
            return;
        };
        let Some(extends) = child.extends.as_ref() else {
            return;
        };
        // Prefer the resolved-at-build-time FQN so cross-package
        // `super(...)` calls find the parent class. Fall back to
        // the bare last segment for single-unit / no-package builds.
        let parent_name: String = child.extends_fqn.clone().unwrap_or_else(|| {
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

        let Some(parent) = self.symbols.classes.get(&parent_name) else {
            return;
        };
        // Parent-constructor overload selection — same count rule as
        // `new` sites; the backend reads the recorded index when it
        // builds `__parent: Parent::new__K(args)`.
        let selected = self
            .select_ctor_typed(&parent.constructors, args)
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
        let inferred = infer_generic_args(method_generic_params, &param_tys, &arg_tys);
        for p in method_generic_params {
            subst_args.push(inferred.get(&p.name.text).cloned().unwrap_or(Ty::Unknown));
        }
        subst_params.extend(method_generic_params.iter().cloned());
    }

    /// Constructor-overload pick, count THEN types (§T.3 applied to
    /// §7.3.1 — S19): when several constructors accept the call's
    /// argument count, score each against the inferred argument types
    /// (2 exact / 1 compatible / disqualified on a mismatch) and take
    /// the best, so `new Point(7)` and `new Point("origin")` pick
    /// different constructors. Ties resolve to the first declared
    /// candidate (identical-shape pairs are rejected at the
    /// declaration).
    fn select_ctor_typed(
        &self,
        ctors: &[crate::symbol_table::ConstructorSig],
        args: &[Expr],
    ) -> Option<usize> {
        let count = args.len();
        let candidates: Vec<usize> = ctors
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                let (lo, hi) = crate::symbol_table::ctor_arity_range(&c.params);
                count >= lo && hi.map_or(true, |h| count <= h)
            })
            .map(|(k, _)| k)
            .collect();
        match candidates.len() {
            0 => None,
            1 => Some(candidates[0]),
            _ => {
                let arg_tys: Vec<Ty> = args
                    .iter()
                    .map(|a| infer_expr(a, &self.env, self.symbols))
                    .collect();
                let mut best: Option<(i32, usize)> = None;
                for &k in &candidates {
                    let params = &ctors[k].params;
                    let mut score = 0i32;
                    let mut ok = true;
                    for (i, at) in arg_tys.iter().enumerate() {
                        let Some(p) = params.get(i) else { continue };
                        let pt = ty_from_ref(&p.ty, &self.env, self.symbols);
                        if pt == *at {
                            score += 2;
                        } else if compatible(&pt, at, self.symbols) {
                            score += 1;
                        } else {
                            ok = false;
                            break;
                        }
                    }
                    if !ok {
                        continue;
                    }
                    if best.map_or(true, |(bs, _)| score > bs) {
                        best = Some((score, k));
                    }
                }
                best.map(|(_, k)| k).or(Some(candidates[0]))
            }
        }
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
            if !compatible(&expected, &found, self.symbols) || function_into_any(&expected, arg) {
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
            compatible(&va_array_ty, &found, self.symbols) && matches!(found, Ty::Array { .. })
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
        let fixed_complete = args.len() >= fixed && params[..fixed.min(args.len())].len() == fixed;
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
            plan.push(crate::ArgSource::Variadic {
                element_type,
                indices: variadic,
            });
        }
        self.call_expansions.insert(call_span, plan);
    }

    /// Validate a call's arguments against a resolved callee signature and record
    /// the expansion plan the backend replays (§7.2).
    ///
    /// Handles the three shapes uniformly: positional, named (`f(x: 1)`), and
    /// variadic (`T...`). `declaring_class` scopes visibility checks to the
    /// caller's position in the hierarchy; `subst_params`/`subst_args` carry the
    /// generic substitution in effect, so a parameter typed `T` is checked
    /// against the bound type rather than against `T` itself.
    /// E0478 (§T.2.2): a `Vec<? extends Animal>` holds values of SOME subtype
    /// of `Animal`, so nothing can be written into it -- not even a `Dog`, since
    /// the receiver may be a `Vec<Cat>`. Reading is what the wildcard is for.
    fn check_wildcard_receiver_write(
        &mut self,
        callee_name: &str,
        params: &[ParamSig],
        subst_params: &[TypeParam],
        subst_args: &[Ty],
        call_span: Span,
    ) {
        fn mentions(ty: &TypeRef, name: &str) -> bool {
            if ty.name.segments.len() == 1 && ty.name.segments[0].text == name {
                return true;
            }
            ty.generic_args.iter().any(|arg| match arg {
                juxc_ast::GenericArg::Type(t) => mentions(t, name),
                juxc_ast::GenericArg::Wildcard(w) => match &w.bound {
                    Some(juxc_ast::WildcardBound::Extends(t) | juxc_ast::WildcardBound::Super(t)) => mentions(t, name),
                    None => false,
                },
            })
        }
        for (i, tp) in subst_params.iter().enumerate() {
            let Some(Ty::Wildcard(crate::ty::Wildcard::Extends(bound))) = subst_args.get(i) else {
                continue;
            };
            if !params.iter().any(|p| mentions(&p.ty, &tp.name.text)) {
                continue;
            }
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0478_WildcardIsReadOnly,
                    format!(
                        "`{callee_name}` takes a `{}`, and a `? extends {bound}` receiver holds values of some \
                         unknown subtype -- nothing is known to fit it (§T.2.2)",
                        tp.name.text,
                    ),
                )
                .with_span(call_span)
                .with_help(format!(
                    "read through the wildcard, or declare the parameter as `{bound}` to write into it",
                )),
            );
            return;
        }
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
        self.check_wildcard_receiver_write(callee_name, params, subst_params, subst_args, call_span);
        // An array argument handed to an array parameter (JUX-LANG-V1 §5.5).
        for (i, arg) in args.iter().enumerate() {
            if arg_names.get(i).is_some_and(|n| n.is_some()) {
                continue;
            }
            let Some(param) = params.get(i) else { break };
            if let Some(shape) = param.ty.array_shape.as_ref() {
                if let Some(outer) = shape.dims.first() {
                    let slot_dynamic = matches!(outer, juxc_ast::ArrayDim::Dynamic);
                    self.note_array_slot(arg, slot_dynamic, expr_span(arg));
                }
            }
        }
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
                            "named arguments can't be combined with a variadic call to `{callee_name}` (Phase 1) -- pass everything positionally",
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
                                    "positional argument after a named one in call to `{callee_name}` -- \
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
                        p.default
                            .clone()
                            .expect("missing-default slots reported above"),
                    ),
                })
                .collect();
            self.call_expansions.insert(call_span, plan);
        }
        for (i, arg) in args.iter().enumerate() {
            // A lambda argument's untyped parameters take the parameter's
            // function type (`app("abc", x -> x.length())` makes `x` a
            // String), exactly as a lambda stored into a typed local does. Only
            // a fully concrete slot is used: a `T` still to be inferred from
            // the call says nothing yet.
            if let (Expr::Lambda(_) | Expr::MethodRef(_), Some(param)) = (arg, arg_to_param[i].and_then(|j| params.get(j))) {
                let slot_raw = match declaring_class {
                    Some(class) => lower_member_type(&param.ty, class, self.symbols),
                    None => ty_from_ref(&param.ty, &self.env, self.symbols),
                };
                if let Ty::Fn { params: slot_params, .. } = substitute(&slot_raw, subst_params, subst_args) {
                    if slot_params.iter().all(ty_is_concrete) {
                        self.lambda_slot_params = Some(slot_params);
                    }
                }
            }
            self.check_expr(arg);
            self.lambda_slot_params = None;
            let Some(param) = arg_to_param[i].and_then(|j| params.get(j)) else {
                continue;
            };
            // **`out` argument rules (§M.4).** An `out` arg must line up with an
            // `out` parameter and vice versa (E0943), and the arg must be an
            // assignable place (E0942). The type check below (E0410) handles the
            // place-vs-param type, since `infer_expr(Expr::Out(p)) = type of p`.
            let arg_is_out = matches!(arg, Expr::Out(..));
            if arg_is_out != param.is_out {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0943_OutArgMismatch,
                        if arg_is_out {
                            format!(
                                "`out` argument passed to parameter `{}` of `{callee_name}`, \
                                 which is not an `out` parameter",
                                param.name,
                            )
                        } else {
                            format!(
                                "parameter `{}` of `{callee_name}` is `out` -- the argument must \
                                 be passed as `out <place>`",
                                param.name,
                            )
                        },
                    )
                    .with_span(expr_span(arg)),
                );
            }
            if let Expr::Out(place, _) = arg {
                if !Self::is_assignable_place(place) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0942_OutArgNotPlace,
                            "an `out` argument must be an assignable place -- a variable, a field, \
                             or an array element (§M.4.2)",
                        )
                        .with_span(expr_span(arg)),
                    );
                }
            }
            let expected_raw = match declaring_class {
                Some(class) => lower_member_type(&param.ty, class, self.symbols),
                None => ty_from_ref(&param.ty, &self.env, self.symbols),
            };
            let expected = substitute(&expected_raw, subst_params, subst_args);
            let found = infer_expr(arg, &self.env, self.symbols);
            self.check_literal_fits(&expected, arg, call_span);
            // A foreign SLICE param (`T[]` = Rust `&[T]`) bridges a Jux array
            // or `rust.std` `Vec<T>` argument (both Deref-coerce to `&[T]`);
            // that's accepted here without touching the global `compatible()`.
            // A raw-pointer parameter takes `null`, its only literal (§L.6.1),
            // as a `T*` local does. The erased `Ty` drops `ptr_depth`, so
            // `compatible` alone rejected `new Env(null, 7)` for a `Table*`.
            let pointer_null = !param.is_out
                && !matches!(arg, Expr::Out(..))
                && self.check_pointer_flow(
                    param.ty.ptr_depth,
                    crate::infer::type_ref_is_void_pointer(&param.ty),
                    &expected,
                    arg,
                    call_span,
                );
            if !pointer_null
                && !foreign_arg_bridges(&expected, &found, param, declaring_class, self.symbols)
                && !compatible(&expected, &found, self.symbols)
            {
                let mut diag = Diagnostic::error(
                    code::Code::E0410_TypeMismatch,
                    format!(
                        "argument {} to `{}`: expected {}, found {}",
                        i + 1,
                        callee_name,
                        expected,
                        found,
                    ),
                )
                // A literal argument has no span of its own; the call does.
                .with_span([expr_span(arg), call_span].into_iter().find(|s| *s != Span::DUMMY).unwrap_or(call_span));
                if let Some(help) = fn_kind_mismatch_help(&expected, &found) {
                    diag = diag.with_help(help);
                }
                // A nullable `T?` flowing into a non-nullable slot is the #1
                // foreign-boundary mistake (e.g. a `WindowOptions?` field
                // passed to `new Window(.., WindowOptions)`). Point the user at
                // the fix instead of leaking a rustc `Option<T>` mismatch.
                if matches!(found, Ty::Nullable(_))
                    && !matches!(expected, Ty::Nullable(_) | Ty::Unknown)
                {
                    diag = diag.with_help(
                        "this value may be null; unwrap it with `!!` (panics on null), provide a \
                         fallback with `?:`, guard it with `if (x != null) { … }`, or make the \
                         parameter nullable",
                    );
                }
                self.diagnostics.push(diag);
            }
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// How widely a visibility modifier exposes a member, for comparing two of
/// them: `public` above `protected` above package access above `private`.
/// `internal` (module-wide) sits with `protected`, both wider than a package.
fn visibility_rank(v: juxc_ast::Visibility) -> u8 {
    match v {
        juxc_ast::Visibility::Public => 4,
        juxc_ast::Visibility::Protected | juxc_ast::Visibility::Internal => 3,
        juxc_ast::Visibility::Package => 2,
        juxc_ast::Visibility::Private => 1,
    }
}

/// The source spelling of a visibility, `package` for the unwritten one.
fn visibility_word(v: juxc_ast::Visibility) -> &'static str {
    match v {
        juxc_ast::Visibility::Public => "public",
        juxc_ast::Visibility::Protected => "protected",
        juxc_ast::Visibility::Internal => "internal",
        juxc_ast::Visibility::Package => "package",
        juxc_ast::Visibility::Private => "private",
    }
}

/// Whether a declared return type is a pointer to `void`.
fn return_is_void_pointer(rt: &juxc_ast::ReturnType) -> bool {
    match rt {
        juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t) => crate::infer::type_ref_is_void_pointer(t),
        juxc_ast::ReturnType::Void => false,
    }
}

/// The raw-pointer depth a declared return type carries.
fn return_ptr_depth(rt: &juxc_ast::ReturnType) -> u8 {
    match rt {
        juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t) => t.ptr_depth,
        juxc_ast::ReturnType::Void => 0,
    }
}

/// `int**` for a pointee `int` at depth 2; the bare type at depth 0.
fn pointer_type_text(pointee: &Ty, depth: u8) -> String {
    let base = match pointee {
        Ty::Void => "void".to_string(),
        other => other.to_string(),
    };
    format!("{base}{}", "*".repeat(depth as usize))
}

#[derive(Debug, PartialEq, Eq)]
enum PointeeMatch {
    Same,
    /// One side is `void`, the other is not.
    Void,
    Different,
}

/// Whether two pointees are one type (§L.6.1a): same representation for
/// primitives, same declaration for user types, no widening. An unknown or
/// generic pointee is given the benefit of the doubt.
fn pointees_match(a: &Ty, b: &Ty) -> PointeeMatch {
    let strip = |t: &Ty| match t {
        Ty::Nullable(inner) => (**inner).clone(),
        other => other.clone(),
    };
    let (a, b) = (strip(a), strip(b));
    match (&a, &b) {
        (Ty::Unknown, _) | (_, Ty::Unknown) | (Ty::Param(_), _) | (_, Ty::Param(_)) => PointeeMatch::Same,
        (Ty::Void, Ty::Void) => PointeeMatch::Same,
        (Ty::Void, _) | (_, Ty::Void) => PointeeMatch::Void,
        (Ty::Primitive(x), Ty::Primitive(y)) => {
            if crate::ty::same_representation(*x, *y) {
                PointeeMatch::Same
            } else {
                PointeeMatch::Different
            }
        }
        (Ty::User { name: x, .. }, Ty::User { name: y, .. }) => {
            if x == y || x.rsplit('.').next() == y.rsplit('.').next() {
                PointeeMatch::Same
            } else {
                PointeeMatch::Different
            }
        }
        _ if a == b => PointeeMatch::Same,
        _ => PointeeMatch::Different,
    }
}

/// Whether the pointees are the two primitives in `pair`, in either order.
fn pointee_is(a: &Ty, b: Ty, pair: &[Primitive; 2]) -> bool {
    matches!((a, &b), (Ty::Primitive(x), Ty::Primitive(y))
        if (*x == pair[0] && *y == pair[1]) || (*x == pair[1] && *y == pair[0]))
}

/// Foreign-call argument BRIDGE (§G interop). A `rust.*` / crate callee whose
/// parameter is a Rust **slice** (`&[T]`, spelled `T[]` in the `.jux.d` stub)
/// accepts a Jux array OR the `rust.std` `Vec<T>` as its argument.
///
/// A Jux array is a plain `Vec<T>` and Deref-coerces to `&[T]` once the backend
/// re-adds the borrow at the call slot (`callee_param_is_foreign_slice`, and
/// its by-name twin `external_param_is_slice`). A `rust.std` collection does
/// NOT: since §6.5.1 it is a shared handle, and `&Rc<RefCell<Vec<T>>>`
/// coerces to nothing. The backend therefore LENDS the interior at the call
/// slot (`foreign_arg_handle_lend`) so the crate sees the sequence it asked
/// for. This comment used to claim the coercion did the work by itself; that
/// stopped being true when collections became reference types, and it was a
/// silent rustc E0308 until an example was written that crosses the boundary.
///
/// Deliberately narrow — it fires ONLY for an external callee, ONLY for a
/// slice parameter, and leaves the global [`compatible`] untouched, so normal
/// (non-foreign) call-arg checking is unchanged. `Vec` is the sole owned std
/// container that derefs to a slice, so naming it is a correct discriminator,
/// not a maintenance-prone allowlist.
fn foreign_arg_bridges(
    expected: &Ty,
    found: &Ty,
    param: &ParamSig,
    declaring_class: Option<&str>,
    symbols: &SymbolTable,
) -> bool {
    // External (foreign) callee only — resolve by exact key, else by bare name.
    let Some(cls) = declaring_class else {
        return false;
    };
    let is_external = symbols
        .classes
        .get(cls)
        .or_else(|| {
            let bare = cls.rsplit('.').next().unwrap_or(cls);
            symbols
                .classes
                .iter()
                .find(|(k, _)| k.rsplit('.').next().unwrap_or(k.as_str()) == bare)
                .map(|(_, v)| v)
        })
        .is_some_and(|c| c.is_external);
    if !is_external {
        return false;
    }
    // The parameter must be a SLICE: a `T[]` (array_shape), not a scalar `&T`
    // borrow (`is_ref`). The lowered `expected` is then a `Ty::Array`.
    if param.ty.array_shape.is_none() || param.is_ref {
        return false;
    }
    let Ty::Array { element, .. } = expected else {
        return false;
    };
    match found {
        // A Jux array argument, element-compatible.
        Ty::Array {
            element: found_el, ..
        } => compatible(element, found_el, symbols),
        // The `rust.std` owned `Vec<E>`, element-compatible.
        Ty::User { name, generic_args } if generic_args.len() == 1 => {
            let bare = name.rsplit('.').next().unwrap_or(name);
            bare == "Vec" && compatible(element, &generic_args[0], symbols)
        }
        _ => false,
    }
}

/// Lower a [`ReturnType`] to a [`Ty`]. Duplicated from `infer.rs` so the
/// checker can use it without exporting an internal helper. `async T`
/// unwraps to `T` (no `Future<T>` wrapper in Phase 1).
/// First character uppercased — for the W0974 rename suggestion
/// (`name` → `Name`).
fn uppercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

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

/// True when `t` is a type permitted at the C FFI boundary (Layout-ABI §L.7):
/// a primitive, a raw pointer (`T*` / `void*`, any pointee), or `String`
/// (`String?` included, for the null-return case). Arrays, function types,
/// generic instantiations, nullable primitives, and user/class types are not.
/// Render a `TypeRef` for an FFI diagnostic (`TypeRef` has no `Display`):
/// dotted name, then `?` for nullable, then one `*` per pointer level.
fn type_ref_display(t: &juxc_ast::TypeRef) -> String {
    // A function type has no name; show the signature the user wrote.
    if let Some(shape) = &t.fn_shape {
        let params = shape.params.iter().map(type_ref_display).collect::<Vec<_>>().join(", ");
        let lead = if shape.is_pointer { "fn" } else { "" };
        let mut out = format!("{lead}({params}) -> {}", type_ref_display(&shape.return_type));
        if t.nullable {
            out = format!("({out})?");
        }
        return out;
    }
    let mut s = t
        .name
        .segments
        .iter()
        .map(|x| x.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    // Generic arguments and the array shape are part of the type the user
    // wrote, and they are exactly the shapes the FFI check rejects - without
    // them E0508 reported `int[]` as `int` and `Vec<int>` as `Vec`, naming a
    // type that would have been perfectly legal.
    if !t.generic_args.is_empty() {
        s.push('<');
        for (i, a) in t.generic_args.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            match a {
                juxc_ast::GenericArg::Type(inner) => s.push_str(&type_ref_display(inner)),
                _ => s.push('_'),
            }
        }
        s.push('>');
    }
    if t.nullable {
        s.push('?');
    }
    for _ in 0..t.ptr_depth {
        s.push('*');
    }
    if let Some(shape) = &t.array_shape {
        for _ in 0..shape.dims.len() {
            s.push_str("[]");
        }
    }
    s
}

fn ffi_type_ok(t: &juxc_ast::TypeRef) -> bool {
    // Composite shapes have no stable C representation.
    if t.array_shape.is_some() || t.fn_shape.is_some() || !t.generic_args.is_empty() {
        return false;
    }
    // Any raw pointer is fine; the pointee is opaque at the boundary.
    if t.ptr_depth > 0 {
        return true;
    }
    let name = t
        .name
        .segments
        .last()
        .map(|s| s.text.as_str())
        .unwrap_or("");
    // `String` (and `String?`) marshal to/from C `const char*`.
    if name == "String" {
        return true;
    }
    // A nullable primitive would lower to `Option<T>`, not C-compatible.
    if t.nullable {
        return false;
    }
    // `char` is permitted: Jux `char` is a 4-byte Unicode scalar (a Rust
    // `char`), but at the FFI boundary it maps to a C `char` (`core::ffi::c_char`)
    // and the compiler converts at the call site (see `emit_extern_c_call`).
    crate::ty::primitive_from_name(name).is_some()
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

/// The binary operators that need their operands' VALUES (§7.10), with their
/// spelling: everything but the equality tests, `??`, `in` and `=`.
/// `None` for an operator that takes a null.
fn nullable_sensitive_op(op: BinaryOp) -> Option<&'static str> {
    Some(match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
        BinaryOp::WrapAdd => "+%",
        BinaryOp::WrapSub => "-%",
        BinaryOp::WrapMul => "*%",
        BinaryOp::WrapShl => "<<%",
        BinaryOp::WrapShr => ">>%",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::Cmp => "<=>",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
        _ => return None,
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
        // `++place` / `place++` reads (and writes) the place — count the
        // place itself as a bare-name read so the borrow-share analysis
        // treats it like any other access.
        Expr::IncDec(i) => collect_bare_name_reads(&i.target, sink),
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
        Stmt::Return(Some(e), _) => collect_bare_name_reads(e, sink),
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
        Expr::Out(_, s) => *s,
        Expr::TypeOf(_, s) => *s,
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
        Expr::Throw(_, s) => *s,
        Expr::IncDec(i) => i.span,
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
        Stmt::Return(_, span) => {
            // The statement carries its own span (covering the `return`
            // keyword), so even a bare `return;` reports a real location.
            out.push(*span);
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
        Pattern::EnumVariant { args, .. } | Pattern::Tuple(args, _) => {
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
    let bare = enum_name.rsplit('.').next().unwrap_or(enum_name);
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
            // A nested enum's own body, or a subclass of its owner, names it by
            // its simple name: `case Level.Junior` inside `Employee` for the
            // lifted `Employee__Level`.
            2 if path.segments[0].text == bare
                || path.segments[0].text == enum_name
                || bare.rsplit("__").next() == Some(path.segments[0].text.as_str()) =>
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
            // `case Order.Status.Pending` -- a NESTED enum named through its
            // owner (M.9). Its prefix, lifted the way the type is
            // (`Order__Status`), is the enum.
            n if n >= 3 => {
                let prefix = path.segments[..n - 1]
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("__");
                if prefix == bare || prefix == enum_name {
                    out.insert(path.segments[n - 1].text.clone());
                }
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
/// Does this block make an enclosing `try` ASYNC — i.e. contain an
/// `await` or a `for await` at its own level? Shallow with respect to
/// LAMBDAS: an await inside a nested lambda belongs to that lambda's
/// async context, not this block's. Used by the S18/E0706 guard.
fn block_has_await_shallow(b: &juxc_ast::Block) -> bool {
    b.statements.iter().any(stmt_has_await_shallow)
}

fn stmt_has_await_shallow(s: &Stmt) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e, _) => expr_has_await_shallow(e),
        Stmt::Return(Some(e), _) => expr_has_await_shallow(e),
        Stmt::VarDecl(v) => v.init.as_ref().is_some_and(expr_has_await_shallow),
        Stmt::Assign(a) => expr_has_await_shallow(&a.value) || expr_has_await_shallow(&a.target),
        Stmt::If(i) => {
            expr_has_await_shallow(&i.condition)
                || block_has_await_shallow(&i.then_block)
                || match i.else_branch.as_deref() {
                    Some(ElseBranch::Block(b)) => block_has_await_shallow(b),
                    Some(ElseBranch::If(inner)) => stmt_has_await_shallow(&Stmt::If(inner.clone())),
                    None => false,
                }
        }
        Stmt::While(w) => expr_has_await_shallow(&w.condition) || block_has_await_shallow(&w.body),
        Stmt::DoWhile(d) => {
            block_has_await_shallow(&d.body) || expr_has_await_shallow(&d.condition)
        }
        Stmt::ForEach(f) => {
            f.is_await || expr_has_await_shallow(&f.iter) || block_has_await_shallow(&f.body)
        }
        Stmt::ForC(f) => {
            f.cond.as_ref().is_some_and(expr_has_await_shallow) || block_has_await_shallow(&f.body)
        }
        Stmt::Try(t) => {
            block_has_await_shallow(&t.body)
                || t.catches.iter().any(|c| block_has_await_shallow(&c.body))
                || t.finally.as_ref().is_some_and(block_has_await_shallow)
        }
        Stmt::Unsafe(b) => block_has_await_shallow(b),
        Stmt::Labeled { stmt, .. } => stmt_has_await_shallow(stmt),
        _ => false,
    }
}

fn expr_has_await_shallow(e: &Expr) -> bool {
    match e {
        Expr::Await(..) => true,
        // Lambdas are their own async scope — don't descend.
        Expr::Lambda(_) => false,
        Expr::Binary(b) => expr_has_await_shallow(&b.left) || expr_has_await_shallow(&b.right),
        Expr::Unary(u) => expr_has_await_shallow(&u.operand),
        Expr::Call(c) => {
            expr_has_await_shallow(&c.callee) || c.args.iter().any(expr_has_await_shallow)
        }
        Expr::Field(f) => expr_has_await_shallow(&f.object),
        Expr::Index(i) => expr_has_await_shallow(&i.array) || expr_has_await_shallow(&i.index),
        Expr::Cast(c) => expr_has_await_shallow(&c.value),
        Expr::NotNullAssert(inner, _) => expr_has_await_shallow(inner),
        Expr::Elvis(el) => {
            expr_has_await_shallow(&el.value) || expr_has_await_shallow(&el.fallback)
        }
        _ => false,
    }
}

/// Collect, for the S18/E0706 guard: every bare-local ASSIGNMENT
/// target in the block (recursively, skipping lambdas) plus every
/// local DECLARED inside it (those live within the async block and
/// are fine to mutate).
fn collect_async_try_writes(
    b: &juxc_ast::Block,
    assigned: &mut Vec<(String, Span)>,
    declared: &mut std::collections::HashSet<String>,
) {
    for s in &b.statements {
        collect_async_try_writes_stmt(s, assigned, declared);
    }
}

fn collect_async_try_writes_stmt(
    s: &Stmt,
    assigned: &mut Vec<(String, Span)>,
    declared: &mut std::collections::HashSet<String>,
) {
    match s {
        Stmt::Assign(a) => {
            if let Expr::Path(qn) = &a.target {
                if qn.segments.len() == 1 {
                    assigned.push((qn.segments[0].text.clone(), a.span));
                }
            }
        }
        Stmt::VarDecl(v) => {
            declared.insert(v.name.text.clone());
        }
        Stmt::If(i) => {
            collect_async_try_writes(&i.then_block, assigned, declared);
            let mut cursor = i.else_branch.as_deref();
            while let Some(branch) = cursor {
                match branch {
                    ElseBranch::Block(b) => {
                        collect_async_try_writes(b, assigned, declared);
                        break;
                    }
                    ElseBranch::If(inner) => {
                        collect_async_try_writes(&inner.then_block, assigned, declared);
                        cursor = inner.else_branch.as_deref();
                    }
                }
            }
        }
        Stmt::While(w) => collect_async_try_writes(&w.body, assigned, declared),
        Stmt::DoWhile(d) => collect_async_try_writes(&d.body, assigned, declared),
        Stmt::ForEach(f) => {
            declared.insert(f.var_name.text.clone());
            collect_async_try_writes(&f.body, assigned, declared);
        }
        Stmt::ForC(f) => {
            if let Some(init) = f.init.as_deref() {
                collect_async_try_writes_stmt(init, assigned, declared);
            }
            collect_async_try_writes(&f.body, assigned, declared);
        }
        Stmt::Try(t) => {
            collect_async_try_writes(&t.body, assigned, declared);
            for c in &t.catches {
                declared.insert(c.name.text.clone());
                collect_async_try_writes(&c.body, assigned, declared);
            }
            if let Some(fin) = &t.finally {
                collect_async_try_writes(fin, assigned, declared);
            }
        }
        Stmt::Unsafe(b) => collect_async_try_writes(b, assigned, declared),
        Stmt::Labeled { stmt, .. } => collect_async_try_writes_stmt(stmt, assigned, declared),
        _ => {}
    }
}

/// The declaration a §A.3 target name refers to, for messages.
fn target_kind_noun(kind: &str) -> &'static str {
    match kind {
        "TYPE" => "type",
        "METHOD" => "method or function",
        "FIELD" => "field or property",
        "PARAMETER" => "parameter",
        "CONSTRUCTOR" => "constructor",
        "LOCAL_VARIABLE" => "local variable",
        "MODULE" => "module",
        "ANNOTATION" => "annotation",
        _ => "declaration",
    }
}

/// True when a lambda or a method reference flows into an `any` / `any?`
/// slot. A function value has no identity and no string form, so it does not
/// convert (§T.1.2). Checked apart from [`compatible`] because a lambda's type
/// comes from its slot and infers as unknown on its own.
fn function_into_any(expected: &Ty, value: &Expr) -> bool {
    let slot = match expected {
        Ty::Nullable(inner) => inner.as_ref(),
        other => other,
    };
    matches!(slot, Ty::Any) && matches!(value, Expr::Lambda(_) | Expr::MethodRef(_))
}

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
    // `any` (§T.1.2 / §T.3.6): every value converts, except a nullable one
    // (that needs `any?`, handled by the nullable arm below) and a function
    // value, which has no identity and no string form.
    if matches!(expected, Ty::Any) {
        return !matches!(found, Ty::Nullable(_) | Ty::Fn { .. } | Ty::FnPtr { .. } | Ty::Void);
    }
    // `null` into a function-pointer slot: the pointer's own default, the way
    // a raw pointer takes it (§L.6.4).
    if matches!(expected, Ty::FnPtr { .. })
        && matches!(found, Ty::Nullable(inner) if matches!(inner.as_ref(), Ty::Unknown))
    {
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
            if expected.is_numeric()
                && !matches!(expected, Ty::Primitive(Primitive::Bool | Primitive::Char)) =>
        {
            true
        }
        (Ty::Primitive(_), Ty::Primitive(Primitive::Double))
            if matches!(
                expected,
                Ty::Primitive(
                    Primitive::Float | Primitive::F32 | Primitive::F64 | Primitive::Double,
                ),
            ) =>
        {
            true
        }
        // `uint` -> any numeric. A collection length / index (`usize` ->
        // `uint`) is routinely used as an `int` (`int size() { return
        // list.len(); }`) or wider; permit it without an explicit cast (the
        // backend emits the matching `as <T>`). Excludes bool/char. This is the
        // opted-in uint->int ergonomic coercion; it is an assignment/return/arg
        // rule, NOT the binary-op promotion that stays strict (no silent
        // int+long promotion).
        (Ty::Primitive(_), Ty::Primitive(Primitive::Uint))
            if expected.is_numeric()
                && !matches!(expected, Ty::Primitive(Primitive::Bool | Primitive::Char)) =>
        {
            true
        }
        // Widening (§S.2.7): a value fits a slot of a type it promotes to under
        // §S.2.6 -- `double d = aFloat;`, `i32 x = aShort;`, `double d = anI32;`.
        // No value changes, and the backend writes the cast.
        (Ty::Primitive(to), Ty::Primitive(from)) if crate::ty::numeric_widens(*from, *to) => true,
        // Arrays — recurse on element. Per JUX-LANG-V1 §5.6, a
        // FIXED-size array (`found`) flows into a runtime-sized
        // (`Dynamic`) slot (`expected`) — the size info is simply
        // dropped, so `int[] a = new int[3];` is valid. The reverse
        // direction (dynamic → fixed) loses the compile-time size
        // guarantee and needs an explicit check, so it's rejected.
        (
            Ty::Array {
                element: e1,
                kind: k1,
            },
            Ty::Array {
                element: e2,
                kind: k2,
            },
        ) => {
            let kind_ok = k1 == k2
                || (*k1 == crate::ty::ArrayKind::Dynamic && *k2 == crate::ty::ArrayKind::Fixed);
            kind_ok && compatible(e1, e2, symbols)
        }
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
            Ty::User {
                name: n1,
                generic_args: a1,
            },
            Ty::User {
                name: n2,
                generic_args: a2,
            },
        ) => {
            if n1 == n2 {
                // Same name — length-mismatch is only a problem
                // when neither side is empty; empty generic args
                // on one side typically means "user didn't write
                // the args" — be lenient.
                if a1.is_empty() || a2.is_empty() {
                    return true;
                }
                // **Diamond (§7.8, Grammar §A.2.6).** `new Vec<>()` and a bare
                // `new Vec()` infer nothing from an empty argument list, so
                // every argument comes back unknown -- one per declared
                // parameter, a foreign type's defaulted ones included (`Vec`
                // has an allocator parameter the program never writes). The
                // slot the value flows into supplies the arguments, exactly
                // as Java's diamond takes them from the target type, so an
                // all-unknown side matches whatever the other side names.
                if a2.iter().all(Ty::is_unknown) || a1.iter().all(Ty::is_unknown) {
                    return true;
                }
                if a1.len() != a2.len() {
                    return false;
                }
                // **Generic invariance (§T.4 / §6.9.6).** A same-name generic
                // type's arguments are INVARIANT: `Box<Dog>` is NOT a `Box<Animal>`
                // even though `Dog extends Animal` — the covariant upcast would let
                // a caller `put()` a non-`Dog` through the `Box<Animal>` view.
                // Require each argument pair to be MUTUALLY compatible (each
                // assignable to the other), which is exact-type identity for
                // concrete types while still letting a wildcard argument
                // (`Box<? extends Animal>`) match via the wildcard arms — that
                // is Jux's explicit variance mechanism. One-way `compatible`
                // here was the covariance hole (gap N2).
                return a1
                    .iter()
                    .zip(a2.iter())
                    .all(|(x, y)| compatible(x, y, symbols) && compatible(y, x, symbols));
            }
            // Different names — try the upcast direction: is the
            // found type a subclass of the expected type? Walks
            // the class-extends chain via `is_subtype`.
            is_subtype(found, expected, symbols)
        }
        _ => false,
    }
}

/// Where a field-initializer lambda uses the object it belongs to: a bare
/// name that is one of the class's own instance fields, properties or
/// methods and not a parameter or local of the lambda. (An explicit `this`
/// there is already the resolver's E0301, one diagnostic per mistake.)
/// Those initializers run while the object is being built (before the handle
/// a capture would need exists), which the backend cannot lower yet (E0981).
fn lambda_uses_this(l: &juxc_ast::LambdaExpr, class: &juxc_ast::ClassDecl) -> Option<Span> {
    let mut members: std::collections::HashSet<&str> = class
        .fields
        .iter()
        .filter(|f| !f.is_static)
        .map(|f| f.name.text.as_str())
        .collect();
    members.extend(class.properties.iter().filter(|p| !p.is_static).map(|p| p.name.text.as_str()));
    members.extend(
        class
            .methods
            .iter()
            .filter(|m| !m.modifiers.contains(&juxc_ast::FnModifier::Static))
            .map(|m| m.name.text.as_str()),
    );
    let mut own: std::collections::HashSet<String> = l.params.iter().map(|p| p.name.text.clone()).collect();
    let body = match &l.body {
        juxc_ast::LambdaBody::Block(b) => (**b).clone(),
        juxc_ast::LambdaBody::Expr(e) => Block {
            statements: vec![Stmt::Expr((**e).clone())],
            span: Span::DUMMY,
        },
    };
    juxc_ast::visit::for_each_node(&body, &mut |n| {
        if let juxc_ast::visit::Node::Stmt(Stmt::VarDecl(v)) = n {
            own.insert(v.name.text.clone());
        }
    });
    let mut found = None;
    juxc_ast::visit::for_each_expr(&body, &mut |e| {
        if found.is_some() {
            return;
        }
        match e {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                if members.contains(name) && !own.contains(name) {
                    found = Some(qn.span);
                }
            }
            _ => {}
        }
    });
    found
}

/// The help for a function value where a function POINTER is expected, or
/// the reverse (Layout-ABI §L.6.4). The two look alike and are not
/// interchangeable: `(A) -> R` is a closure that may capture, `fn(A) -> R`
/// the address of a C-callable function with no environment.
fn fn_kind_mismatch_help(expected: &Ty, found: &Ty) -> Option<&'static str> {
    match (expected, found) {
        (Ty::FnPtr { .. }, Ty::Fn { .. }) => Some(
            "`fn(...) -> R` is a function pointer, for C code; a lambda or a `(...) -> R` value is a closure and \
             does not convert. Pass a named function, or declare the slot `(...) -> R`",
        ),
        (Ty::Fn { .. }, Ty::FnPtr { .. }) => Some(
            "a `fn(...)` pointer is not a closure value; wrap it in a lambda, `(x) -> p(x)`, to store it as `(...) -> R`",
        ),
        _ => None,
    }
}

/// Why the literal `lit` cannot be the argument of a const parameter of
/// kind `cty` (T.11.3), or `None` when it can. An integer kind takes an
/// integer literal that fits it; `int` and `uint` take no negative value,
/// since they size arrays and lower to an unsigned `usize`. `bool` takes
/// `true`/`false`, `char` a char literal.
fn const_arg_mismatch(cty: &juxc_ast::TypeRef, lit: &str) -> Option<String> {
    let kind = cty.name.segments.last().map(|s| s.text.as_str()).unwrap_or("int");
    let is_bool = lit == "true" || lit == "false";
    let is_char = lit.starts_with('\'');
    match kind {
        "bool" => (!is_bool).then(|| "it is a `bool` parameter".to_string()),
        "char" => (!is_char).then(|| "it is a `char` parameter; give a char literal like `'x'`".to_string()),
        _ => {
            if is_bool || is_char {
                return Some(format!("it is an `{kind}` parameter; give an integer"));
            }
            let p = crate::ty::primitive_from_name(kind)?;
            let bits = crate::ty::integer_bits(p)?;
            let v = lit.replace('_', "").parse::<i128>().ok()?;
            let sizes_arrays = matches!(p, Primitive::Int | Primitive::Uint);
            let (lo, hi): (i128, i128) = match p {
                Primitive::Int => (0, (1i128 << 63) - 1),
                Primitive::Uint => (0, (1i128 << 64) - 1),
                _ if crate::ty::is_unsigned_primitive(p) => (0, (1i128 << bits) - 1),
                _ => (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1),
            };
            if (lo..=hi).contains(&v) {
                None
            } else if v < 0 && sizes_arrays {
                Some(format!("an `{kind}` const parameter is a size, and a size is never negative"))
            } else {
                Some(format!("it is outside `{kind}` ({lo} to {hi})"))
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

/// `x == null` (when `want_eq`) or `x != null` (when not), in either operand
/// order, with `x` a bare single-segment name. `None` for every other shape --
/// narrowing only claims what it can see plainly.
/// Whether `ty` names a type with nothing left to infer: no type parameter
/// and no unknown anywhere inside it.
fn ty_is_concrete(ty: &Ty) -> bool {
    match ty {
        Ty::Param(_) | Ty::Unknown | Ty::Wildcard(_) => false,
        Ty::Nullable(inner) => ty_is_concrete(inner),
        Ty::Array { element, .. } => ty_is_concrete(element),
        Ty::User { generic_args, .. } => generic_args.iter().all(ty_is_concrete),
        Ty::Fn { params, return_type, .. } | Ty::FnPtr { params, return_type, .. } => {
            params.iter().all(ty_is_concrete) && ty_is_concrete(return_type)
        }
        _ => true,
    }
}

/// The names `cond` proves non-null when it evaluates to `outcome`, in source
/// order: `x != null` itself when true, `x == null` when false, and through
/// `&&` (true) or `||` (false) the names of both sides. Duplicates are kept;
/// the caller drops them.
fn null_tested_names<'e>(cond: &'e Expr, outcome: bool, out: &mut Vec<&'e str>) {
    match cond {
        Expr::Binary(b) if (b.op == BinaryOp::And && outcome) || (b.op == BinaryOp::Or && !outcome) => {
            null_tested_names(&b.left, outcome, out);
            null_tested_names(&b.right, outcome, out);
        }
        _ => out.extend(match_null_test(cond, !outcome)),
    }
}

fn match_null_test(cond: &Expr, want_eq: bool) -> Option<&str> {
    let Expr::Binary(b) = cond else { return None };
    let matches_op = match b.op {
        juxc_ast::BinaryOp::Eq => want_eq,
        juxc_ast::BinaryOp::NotEq => !want_eq,
        _ => return None,
    };
    if !matches_op {
        return None;
    }
    let target = match (&*b.left, &*b.right) {
        (juxc_ast::Expr::Literal(juxc_ast::Literal::Null), other) => other,
        (other, juxc_ast::Expr::Literal(juxc_ast::Literal::Null)) => other,
        _ => return None,
    };
    match target {
        Expr::Path(qn) if qn.segments.len() == 1 => Some(qn.segments[0].text.as_str()),
        _ => None,
    }
}

/// Levenshtein distance between two short ASCII names.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// How an annotation is conventionally written, for a suggestion: `test` is
/// shown as `Test`, `beforeeach` as `BeforeEach`. A declared name keeps its own
/// spelling.
fn canonical_annotation_spelling(name: &str) -> String {
    match name {
        "beforeall" => "BeforeAll".into(),
        "beforeeach" => "BeforeEach".into(),
        "aftereach" => "AfterEach".into(),
        "afterall" => "AfterAll".into(),
        "noinline" => "NoInline".into(),
        "nativemodule" => "NativeModule".into(),
        "plugininterface" => "PluginInterface".into(),
        "annotationtype" => "AnnotationType".into(),
        other if other.chars().all(|c| c.is_ascii_lowercase()) => {
            let mut chars = other.chars();
            chars.next().map(|f| f.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
        }
        other => other.to_string(),
    }
}

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

    /// Like [`run`], but first builds a FOREIGN (`is_external`) stub unit from
    /// `stub_src` into the same workspace — so `@rust`-style external classes
    /// (e.g. a `rust.std.Vec` or a crate type taking a slice param) are in
    /// scope. Returns only the diagnostics from checking the user `src`.
    fn run_with_stub(stub_src: &str, src: &str) -> Vec<Diagnostic> {
        let stub_sf = SourceFile::new("stub.jux.d", stub_src);
        let stub_lex = lex(&stub_sf);
        assert!(
            stub_lex.diagnostics.is_empty(),
            "stub lex: {:?}",
            stub_lex.diagnostics
        );
        let stub_parse = parse(&stub_lex.tokens);
        assert!(
            stub_parse.diagnostics.is_empty(),
            "stub parse: {:?}",
            stub_parse.diagnostics
        );
        let mut stub_ast = stub_parse.ast;
        stub_ast.is_external = true;

        let sf = SourceFile::new("test.jux", src);
        let lex_result = lex(&sf);
        assert!(
            lex_result.diagnostics.is_empty(),
            "lex: {:?}",
            lex_result.diagnostics
        );
        let parse_result = parse(&lex_result.tokens);
        assert!(
            parse_result.diagnostics.is_empty(),
            "parse: {:?}",
            parse_result.diagnostics
        );
        let main_ast = parse_result.ast;

        let mut throwaway = Vec::new();
        let symbols = crate::symbol_table::build_workspace(
            std::slice::from_ref(&stub_ast)
                .iter()
                .cloned()
                .chain(std::iter::once(main_ast.clone()))
                .collect::<Vec<_>>()
                .as_slice(),
            &mut throwaway,
        );
        let mut diags = Vec::new();
        let mut checker = Checker::new(&symbols, &mut diags);
        checker.check_unit(&main_ast);
        diags
    }

    /// A single-package foreign stub: a `Vec<T>` / `HashSet<T>` container plus
    /// a crate class `Sink` with a SLICE parameter (`u32[]` = Rust `&[u32]`).
    /// One `package` per unit, so everything lives in `demo.stub` — the bridge
    /// keys on the type's bare name (`Vec`), not its package.
    const FOREIGN_STUB: &str = r#"
        package demo.stub;
        @rust("std::vec::Vec")
        public class Vec<T> { public Vec(); public void push(T v); }
        @rust("std::collections::HashSet")
        public class HashSet<T> { public HashSet(); public void insert(T v); }
        @rust("demo::Sink")
        public class Sink { public Sink(); public void feed(u32[] data); }
    "#;

    /// A `Vec<u32>` argument bridges to a foreign `u32[]` (slice) parameter —
    /// `&Vec<u32>` Deref-coerces to `&[u32]`, so NO E0410.
    #[test]
    fn vec_bridges_to_foreign_slice_param() {
        let d = run_with_stub(
            FOREIGN_STUB,
            "import demo.stub.Sink; import demo.stub.Vec; \
             public void main() { var s = new Sink(); var v = new demo.stub.Vec<u32>(); s.feed(v); }",
        );
        assert!(
            !has(&d, code::Code::E0410_TypeMismatch),
            "Vec should bridge to a slice: {d:?}"
        );
    }

    /// A plain Jux array argument bridges to a foreign slice parameter — NO E0410.
    #[test]
    fn array_bridges_to_foreign_slice_param() {
        let d = run_with_stub(
            FOREIGN_STUB,
            "import demo.stub.Sink; \
             public void main() { var s = new Sink(); var a = new u32[8]; s.feed(a); }",
        );
        assert!(
            !has(&d, code::Code::E0410_TypeMismatch),
            "array should bridge to a slice: {d:?}"
        );
    }

    /// The bridge is Vec/array-ONLY: a non-`Vec` external container into a
    /// foreign slice parameter still mismatches (E0410). Proves the bridge
    /// didn't loosen foreign-arg checking generally.
    #[test]
    fn non_vec_container_to_foreign_slice_still_e0410() {
        let d = run_with_stub(
            FOREIGN_STUB,
            "import demo.stub.Sink; import demo.stub.HashSet; \
             public void main() { var s = new Sink(); var h = new demo.stub.HashSet<u32>(); s.feed(h); }",
        );
        assert!(
            has(&d, code::Code::E0410_TypeMismatch),
            "HashSet must NOT bridge to a slice: {d:?}"
        );
    }

    /// A nullable `T?` argument flowing into a non-nullable parameter raises
    /// E0410 WITH actionable unwrap guidance (the `Frame.options` mistake).
    #[test]
    fn nullable_arg_to_non_null_param_emits_e0410_with_help() {
        let d = run("public class Opt { public Opt() { } } \
             public void take(Opt o) { } \
             public void main() { Opt? maybe = new Opt(); take(maybe); }");
        let hit = d.iter().find(|x| x.code == code::Code::E0410_TypeMismatch);
        assert!(hit.is_some(), "expected E0410 for T? -> T param: {d:?}");
        assert!(
            hit.unwrap().help.iter().any(|h| h.contains("!!")),
            "expected unwrap help on the diagnostic: {d:?}",
        );
    }

    /// Convenience: did any diagnostic with `code` fire?
    fn has(diags: &[Diagnostic], wanted: code::Code) -> bool {
        diags.iter().any(|d| d.code == wanted)
    }

    /// Convenience: count diagnostics matching `code`.
    fn count(diags: &[Diagnostic], wanted: code::Code) -> usize {
        diags.iter().filter(|d| d.code == wanted).count()
    }

    // --- §L.7 C FFI: `unsafe native` blocks ---

    /// Calling a foreign function outside an `unsafe` context is E0506
    /// (each `unsafe native` fn is implicitly unsafe to call).
    #[test]
    fn foreign_call_outside_unsafe_is_e0506() {
        let d = run("@extern(lib = \"c\") unsafe native { i32 getpid(); } \
             public void main() { i32 p = getpid(); }");
        assert!(has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe), "{d:?}");
    }

    /// The same call inside `unsafe { … }` is clean (no E0506).
    #[test]
    fn foreign_call_inside_unsafe_is_clean() {
        let d = run("@extern(lib = \"c\") unsafe native { i32 getpid(); } \
             public void main() { unsafe { i32 p = getpid(); } }");
        assert!(!has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe), "{d:?}");
    }

    /// A non-FFI parameter type (a user class) in a foreign signature is E0508.
    #[test]
    fn foreign_class_param_is_e0508() {
        let d = run(
            "public class Foo { public int x; public Foo(int x) { this.x = x; } } \
             @extern(lib = \"c\") unsafe native { void takes(Foo f); } \
             public void main() {}",
        );
        assert!(has(&d, code::Code::E0508_FfiTypeNotAllowed), "{d:?}");
    }

    /// Primitives, raw pointers, `String`, and `void` are all FFI-allowed —
    /// no E0508 for the canonical malloc/free/puts shapes.
    #[test]
    fn foreign_ffi_types_are_allowed() {
        let d = run("@extern(lib = \"c\") unsafe native { \
                void* malloc(ulong size); void free(void* p); \
                i32 puts(String s); String getenv(String name); \
             } public void main() {}");
        assert!(!has(&d, code::Code::E0508_FfiTypeNotAllowed), "{d:?}");
    }

    // --- §L.1.2 `@layout(c)` value structs ---

    /// `@layout(c)` on a `class` (not a struct) is E0509.
    #[test]
    fn layout_c_on_class_is_e0509() {
        let d = run(
            "@layout(c) class Bad { public int x; public Bad(int x) { this.x = x; } } \
             public void main() {}",
        );
        assert!(has(&d, code::Code::E0509_LayoutCOnNonAggregate), "{d:?}");
    }

    /// A non-C-compatible field (`String`) in a `@layout(c)` struct is E0509.
    #[test]
    fn layout_c_struct_string_field_is_e0509() {
        let d = run(
            "@layout(c) struct Bad { String name; public Bad(String n) { this.name = n; } } \
             public void main() {}",
        );
        assert!(has(&d, code::Code::E0509_LayoutCOnNonAggregate), "{d:?}");
    }

    /// A clean `@layout(c)` struct (primitive fields) and its use as an FFI
    /// parameter/`out` parameter are accepted - no E0509, no E0508.
    #[test]
    fn layout_c_struct_is_ffi_allowed() {
        let d = run(
            "@layout(c) struct P { i32 x; i32 y; public P(i32 x, i32 y) { this.x = x; this.y = y; } } \
             @extern(lib = \"user32\") unsafe native { i32 GetCursorPos(out P p); } \
             public void main() {}",
        );
        assert!(!has(&d, code::Code::E0509_LayoutCOnNonAggregate), "{d:?}");
        assert!(!has(&d, code::Code::E0508_FfiTypeNotAllowed), "{d:?}");
    }

    // --- §L.1.3 `@layout(c, repr)` C enums ---

    /// A plain integer `@layout(c)` enum (no payloads) is accepted — no E0509.
    #[test]
    fn layout_c_enum_no_payload_ok() {
        let d = run(
            "@layout(c, repr = \"i32\") enum HttpStatus { Ok = 200, NotFound = 404 } \
             public void main() {}",
        );
        assert!(!has(&d, code::Code::E0509_LayoutCOnNonAggregate), "{d:?}");
    }

    /// A `@layout(c)` enum variant carrying a payload is E0509 (a C enum is a
    /// plain integer with no associated data).
    #[test]
    fn layout_c_enum_with_payload_is_e0509() {
        let d = run("@layout(c, repr = \"i32\") enum Bad { Ok, Err(i32) } \
             public void main() {}");
        assert!(has(&d, code::Code::E0509_LayoutCOnNonAggregate), "{d:?}");
    }

    /// An explicit discriminant on a regular (non-`@layout(c)`) enum is E0510 —
    /// the value would otherwise be silently dropped.
    #[test]
    fn discriminant_on_plain_enum_is_e0510() {
        let d = run("enum Y { A = 5, B = 9 } public void main() {}");
        assert!(has(&d, code::Code::E0510_DiscriminantOutsideCEnum), "{d:?}");
    }

    /// A regular enum with NO discriminants is accepted (no E0510).
    #[test]
    fn plain_enum_without_discriminant_ok() {
        let d = run("enum Y { A, B } public void main() {}");
        assert!(
            !has(&d, code::Code::E0510_DiscriminantOutsideCEnum),
            "{d:?}"
        );
    }

    /// A C-variadic foreign call accepts MORE arguments than its fixed params
    /// (no E0411); a variadic with NO fixed param is rejected (E0508).
    #[test]
    fn c_variadic_call_accepts_extra_args() {
        let ok = run(
            "@extern(lib = \"c\") unsafe native { i32 printf(String fmt, ...); } \
             public void main() { unsafe { printf(\"%d %d\", 1, 2); } }",
        );
        assert!(!has(&ok, code::Code::E0411_WrongArgCount), "{ok:?}");

        // A variadic with no fixed parameter is illegal.
        let bad = run("@extern(lib = \"c\") unsafe native { i32 bad(...); } \
             public void main() {}");
        assert!(has(&bad, code::Code::E0508_FfiTypeNotAllowed), "{bad:?}");
    }

    /// A `@layout(c)` C enum is allowed at the FFI boundary (param and return) —
    /// no E0508; a plain (non-C) enum at the boundary is still E0508.
    #[test]
    fn c_enum_is_ffi_allowed_plain_enum_is_not() {
        let ok = run(
            "@layout(c, repr = \"i32\") enum Status { Ok = 0, Err = 1 } \
             @extern(lib = \"c\") unsafe native { Status flip(Status s); } \
             public void main() {}",
        );
        assert!(!has(&ok, code::Code::E0508_FfiTypeNotAllowed), "{ok:?}");

        let bad = run("enum Plain { A, B } \
             @extern(lib = \"c\") unsafe native { Plain flip(Plain s); } \
             public void main() {}");
        assert!(has(&bad, code::Code::E0508_FfiTypeNotAllowed), "{bad:?}");
    }

    /// A `@layout(c)` C enum is a valid FIELD of a `@layout(c)` struct (the
    /// canonical "status code in a struct" C shape) — no E0509. A plain enum or
    /// a `String` field is still rejected.
    #[test]
    fn c_enum_is_a_valid_layout_c_struct_field() {
        let ok = run("@layout(c, repr = \"i32\") enum Kind { A = 1, B = 2 } \
             @layout(c) struct Tagged { Kind kind; i32 value; } \
             public void main() {}");
        assert!(!has(&ok, code::Code::E0509_LayoutCOnNonAggregate), "{ok:?}");

        let bad = run("enum Plain { A, B } \
             @layout(c) struct Bad { Plain kind; } \
             public void main() {}");
        assert!(
            has(&bad, code::Code::E0509_LayoutCOnNonAggregate),
            "{bad:?}"
        );
    }

    /// A generic `@layout(c)` struct or enum has no fixed C layout → E0509.
    #[test]
    fn generic_layout_c_aggregate_is_e0509() {
        let s = run("@layout(c) struct Box<T> { i32 v; } public void main() {}");
        assert!(has(&s, code::Code::E0509_LayoutCOnNonAggregate), "{s:?}");
        let e = run("@layout(c, repr = \"i32\") enum E<T> { A = 1 } public void main() {}");
        assert!(has(&e, code::Code::E0509_LayoutCOnNonAggregate), "{e:?}");
    }

    /// A non-const C-enum discriminant (`A = foo()`) → E0509; a constant
    /// discriminant (literal or const-foldable) is clean.
    #[test]
    fn non_const_c_enum_discriminant_is_e0509() {
        // `g()` does IO, so it is not const-evaluable — the backend would
        // silently drop the value, so it must be rejected.
        let bad = run("i32 g() { print(\"io\"); return 1; } \
             @layout(c, repr = \"i32\") enum M { A = g(), B } public void main() {}");
        assert!(
            has(&bad, code::Code::E0509_LayoutCOnNonAggregate),
            "{bad:?}"
        );
        let ok =
            run("@layout(c, repr = \"i32\") enum N { A = 200, B = 1 + 1 } public void main() {}");
        assert!(!has(&ok, code::Code::E0509_LayoutCOnNonAggregate), "{ok:?}");
    }

    /// `@export` on a method (static or instance) is E0508 in Phase 1 — it is
    /// only honored on free functions.
    #[test]
    fn export_on_method_is_e0508() {
        let st = run(
            "class Foo { @export public static int bar(int x) { return x; } } public void main() {}",
        );
        assert!(has(&st, code::Code::E0508_FfiTypeNotAllowed), "{st:?}");
        let inst =
            run("class Foo { @export public int baz(int x) { return x; } } public void main() {}");
        assert!(has(&inst, code::Code::E0508_FfiTypeNotAllowed), "{inst:?}");
    }

    /// A `@layout(c)` struct with bare fields and no constructor gets an
    /// implicit positional constructor (synthesized in the desugar pass), so
    /// `new P(x, y)` is clean - no E0600 (definite assignment) and no E0411
    /// (arg count).
    #[test]
    fn layout_c_struct_implicit_ctor_is_clean() {
        let d = run("@layout(c) struct P { i32 x; i32 y; } \
             public void main() { P p = new P(1, 2); i32 a = p.x; }");
        assert!(
            !has(&d, code::Code::E0600_FieldNotDefinitelyAssigned),
            "{d:?}"
        );
        assert!(!has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    // --- §8.4 `@export` (Jux → C) ---

    /// A clean `@export` signature (primitive params/return) is accepted.
    #[test]
    fn export_clean_signature_ok() {
        let d = run("@export public int add(int a, int b) { return a + b; } public void main() {}");
        assert!(!has(&d, code::Code::E0508_FfiTypeNotAllowed), "{d:?}");
    }

    /// A `String` parameter / return on an `@export` function is now allowed
    /// (it is marshalled to/from C `const char*` by the wrapper, §L.3.2) — no
    /// E0508. A genuinely non-C type (a class) is still rejected.
    #[test]
    fn export_string_signature_ok() {
        let ok = run("@export String greet(String name) { return name; } public void main() {}");
        assert!(!has(&ok, code::Code::E0508_FfiTypeNotAllowed), "{ok:?}");

        let bad = run(
            "class Box { public int v; public Box(int v) { this.v = v; } } \
             @export public int bad(Box b) { return 0; } public void main() {}",
        );
        assert!(has(&bad, code::Code::E0508_FfiTypeNotAllowed), "{bad:?}");
    }

    // --- §M.14.2 `final` parameter / local reassignment (E0464) ---

    /// Reassigning a `final` parameter is E0464.
    #[test]
    fn final_param_reassign_is_e0464() {
        let d = run("public void f(final int x) { x = 5; }");
        assert!(has(&d, code::Code::E0464_FinalBindingReassigned), "{d:?}");
    }

    /// A compound assignment to a `final` parameter is also E0464.
    #[test]
    fn final_param_compound_assign_is_e0464() {
        let d = run("public void f(final int x) { x += 1; }");
        assert!(has(&d, code::Code::E0464_FinalBindingReassigned), "{d:?}");
    }

    /// Reading a `final` parameter is fine — only reassignment is barred.
    #[test]
    fn final_param_read_is_ok() {
        let d = run("public void f(final int x) { var y = x + 1; }");
        assert!(!has(&d, code::Code::E0464_FinalBindingReassigned), "{d:?}");
    }

    /// A plain (non-final) parameter may be reassigned.
    #[test]
    fn nonfinal_param_reassign_is_ok() {
        let d = run("public void f(int x) { x = 5; }");
        assert!(!has(&d, code::Code::E0464_FinalBindingReassigned), "{d:?}");
    }

    /// Reassigning a `final` LOCAL is also E0464 (§M.14.2 covers locals).
    #[test]
    fn final_local_reassign_is_e0464() {
        let d = run("public void f() { final int y = 1; y = 2; }");
        assert!(has(&d, code::Code::E0464_FinalBindingReassigned), "{d:?}");
    }

    // ---- E0465: final/const FIELD reassignment (§5.6) ------------------------

    /// The user's case: a bare `x = 5` (implicit `this.x`) in a method, where
    /// `x` is a `const`/`final` field, is E0465.
    #[test]
    fn final_field_bare_reassign_in_method_is_e0465() {
        let d = run("public class C { const int x = 10; public void m() { x = 5; } }");
        assert!(has(&d, code::Code::E0465_FinalFieldReassigned), "{d:?}");
    }

    /// `this.x = 5` in a method is also E0465.
    #[test]
    fn final_field_this_reassign_in_method_is_e0465() {
        let d = run("public class C { final int x = 10; public void m() { this.x = 5; } }");
        assert!(has(&d, code::Code::E0465_FinalFieldReassigned), "{d:?}");
    }

    /// External write `c.x = 5` from outside the class is E0465.
    #[test]
    fn final_field_external_reassign_is_e0465() {
        let d = run("public class C { const int x = 10; } \
             public void main() { var c = new C(); c.x = 5; }");
        assert!(has(&d, code::Code::E0465_FinalFieldReassigned), "{d:?}");
    }

    /// Compound `x += 1` and desugared `x++` on a final field are E0465.
    #[test]
    fn final_field_compound_and_incr_are_e0465() {
        let comp = run("public class C { final int x = 0; public void m() { x += 1; } }");
        assert!(
            has(&comp, code::Code::E0465_FinalFieldReassigned),
            "{comp:?}"
        );
        let incr = run("public class C { final int x = 0; public void m() { x++; } }");
        assert!(
            has(&incr, code::Code::E0465_FinalFieldReassigned),
            "{incr:?}"
        );
    }

    /// A `static final` field reassigned (qualified `C.N` or bare) is E0465.
    #[test]
    fn static_final_field_reassign_is_e0465() {
        let q = run("public class C { static final int N = 10; public void m() { C.N = 5; } }");
        assert!(has(&q, code::Code::E0465_FinalFieldReassigned), "{q:?}");
    }

    /// A `final` field assigned in the CONSTRUCTOR (bare or `this.`) is allowed —
    /// no E0465. (The whole point of `final`: set once at construction.)
    #[test]
    fn final_field_assign_in_ctor_is_ok() {
        let bare = run("public class C { final int x; public C(int v) { x = v; } }");
        assert!(
            !has(&bare, code::Code::E0465_FinalFieldReassigned),
            "{bare:?}"
        );
        let this_ = run("public class C { final int x; public C(int v) { this.x = v; } }");
        assert!(
            !has(&this_, code::Code::E0465_FinalFieldReassigned),
            "{this_:?}"
        );
    }

    /// A `final` field assigned in an instance `init { }` block is allowed.
    #[test]
    fn final_field_assign_in_init_block_is_ok() {
        let d = run("public class C { final int x; init { x = 7; } }");
        assert!(!has(&d, code::Code::E0465_FinalFieldReassigned), "{d:?}");
    }

    /// A non-`final` (`var`) field reassigned in a method is fine — no E0465.
    #[test]
    fn non_final_field_reassign_is_ok() {
        let d = run("public class C { int x = 0; public void m() { x = 5; } }");
        assert!(!has(&d, code::Code::E0465_FinalFieldReassigned), "{d:?}");
    }

    // --- §4.4 type-level visibility (E0416 for TYPES) ---

    /// Two units, so a package boundary actually exists.
    fn run_two(a: &str, b: &str) -> Vec<Diagnostic> {
        let units: Vec<juxc_ast::CompilationUnit> = [a, b]
            .iter()
            .map(|src| {
                let sf = SourceFile::new("t.jux", *src);
                let lexed = lex(&sf);
                assert!(lexed.diagnostics.is_empty(), "lex: {:?}", lexed.diagnostics);
                let parsed = parse(&lexed.tokens);
                assert!(
                    parsed.diagnostics.is_empty(),
                    "parse: {:?}",
                    parsed.diagnostics
                );
                parsed.ast
            })
            .collect();
        crate::typecheck_workspace(&units).diagnostics
    }

    /// Two packages each declare `Base`. A class extending one of them is not a
    /// subtype of the other: the extends walk compares whole names, where it used
    /// to compare simple names and accept the upcast.
    #[test]
    fn same_named_base_in_another_package_is_not_an_ancestor() {
        let d = run_two(
            "package zoo; public class Base { } public class Cat extends Base { }",
            "package garage; import zoo.*; public class Base { } public class Car extends Base { } \
             public class U { public void go() { Base mine = new Car(); zoo.Base theirs = new Car(); } }",
        );
        let mismatches: Vec<_> = d.iter().filter(|x| x.code == code::Code::E0410_TypeMismatch).collect();
        assert_eq!(mismatches.len(), 1, "{d:?}");
        assert!(mismatches[0].message.contains("zoo.Base"), "{d:?}");
    }

    /// A type declared without `public` is visible only inside its own package
    /// (§4.4), exactly as in Java. Member visibility was already enforced; the
    /// TYPE was not, so the modifier on the declaration meant nothing.
    #[test]
    fn package_private_type_is_not_visible_across_packages() {
        let d = run_two(
            "package a; class Hidden { public int x = 1; }",
            "package b; import a.*; public class U { public int go() { Hidden h = new Hidden(); return h.x; } }",
        );
        assert!(has(&d, code::Code::E0416_PackagePrivateAccess), "{d:?}");
    }

    /// The same declaration is fine from inside its own package.
    #[test]
    fn package_private_type_is_visible_in_its_own_package() {
        let d = run_two(
            "package a; class Hidden { public int x = 1; }",
            "package a; public class U { public int go() { Hidden h = new Hidden(); return h.x; } }",
        );
        assert!(!has(&d, code::Code::E0416_PackagePrivateAccess), "{d:?}");
    }

    /// `public` exports it, which is the whole point of writing the modifier.
    #[test]
    fn public_type_crosses_a_package_boundary() {
        let d = run_two(
            "package a; public class Shown { public int y = 2; }",
            "package b; import a.*; public class U { public int go() { Shown s = new Shown(); return s.y; } }",
        );
        assert!(!has(&d, code::Code::E0416_PackagePrivateAccess), "{d:?}");
    }

    /// `internal` is MODULE-scoped and the checker has no module identity, so
    /// it is deliberately exempt: rejecting code the spec allows is the worse
    /// of the two errors. This pins that as a decision rather than an oversight.
    #[test]
    fn internal_type_is_exempt_until_modules_are_modelled() {
        let d = run_two(
            "package a; internal class Mod { public int x = 1; }",
            "package b; import a.*; public class U { public int go() { Mod m = new Mod(); return m.x; } }",
        );
        assert!(!has(&d, code::Code::E0416_PackagePrivateAccess), "{d:?}");
    }

    // --- JLS §6.6.1 access control: nesting and `protected` ---

    /// A nested class reads its owner's `private` member.
    ///
    /// JLS §6.6.1 scopes `private` to the body of the enclosing TOP LEVEL
    /// class, not to the immediately declaring one. Jux scoped it to the
    /// declaring class, so a nested helper could not touch the state it exists
    /// to help with, and the only way out was to widen the field: the rule
    /// pushed authors AWAY from encapsulation.
    #[test]
    fn a_nested_class_reads_its_owners_private() {
        let d = run("public class Outer { private int secret = 7;\n\
               public class Inner { public int go(Outer o) { return o.secret; } } }");
        assert!(!has(&d, code::Code::E0414_PrivateAccess), "{d:?}");
    }

    /// And the owner reads the nested class's `private` member — the rule is
    /// symmetric in Java, both bodies being inside the same top-level class.
    #[test]
    fn an_owner_reads_its_nested_classs_private() {
        let d = run(
            "public class Outer { public int go(Inner i) { return i.hidden; }\n\
               public class Inner { private int hidden = 5; } }",
        );
        assert!(!has(&d, code::Code::E0414_PrivateAccess), "{d:?}");
    }

    /// Two nested types under one owner are also within the same top-level
    /// body, so they see each other.
    #[test]
    fn sibling_nested_classes_see_each_others_privates() {
        let d = run("public class Outer {\n\
               public class A { private int x = 1; }\n\
               public class B { public int go(A a) { return a.x; } } }");
        assert!(!has(&d, code::Code::E0414_PrivateAccess), "{d:?}");
    }

    /// The boundary that still holds: two UNRELATED top-level classes.
    #[test]
    fn unrelated_top_level_classes_still_cannot_read_privates() {
        let d = run("public class A { private int x = 1; }\n\
             public class B { public int go(A a) { return a.x; } }");
        assert!(has(&d, code::Code::E0414_PrivateAccess), "{d:?}");
    }

    /// JLS §6.6.1: `protected` ALSO grants package access. A same-package peer
    /// that is not a subclass still reaches the member, because `protected` is
    /// strictly wider than package-private, never narrower. Jux treated it as
    /// subclass-only, making `protected` narrower than writing nothing.
    #[test]
    fn protected_reaches_a_same_package_non_subclass() {
        let d = run_two(
            "package a; public class Base { protected int v = 4; }",
            "package a; public class Peer { public int go() { Base b = new Base(); return b.v; } }",
        );
        assert!(!has(&d, code::Code::E0415_ProtectedAccess), "{d:?}");
    }

    /// Across a package boundary, a non-subclass is still shut out.
    #[test]
    fn protected_does_not_reach_another_package() {
        let d = run_two(
            "package a; public class Base { protected int v = 4; }",
            "package b; import a.*; public class Peer { public int go() { Base b = new Base(); return b.v; } }",
        );
        assert!(has(&d, code::Code::E0415_ProtectedAccess), "{d:?}");
    }

    /// A subclass in another package keeps its access, which is the case
    /// `protected` exists for.
    #[test]
    fn protected_still_reaches_a_subclass_in_another_package() {
        let d = run_two(
            "package a; public class Base { protected int v = 4; }",
            "package b; import a.*; public class Sub extends Base { public int go() { return v; } }",
        );
        assert!(!has(&d, code::Code::E0415_ProtectedAccess), "{d:?}");
    }

    /// Writes follow the reads. A nested class that can READ its owner's
    /// private property must be able to write it too, or the two halves of
    /// the same rule disagree.
    #[test]
    fn a_nested_class_writes_its_owners_private_property() {
        let d = run(
            "public class Outer { private int Level { get; set; } = 1;\n\
               public class Inner { public void go(Outer o) { o.Level = 9; } } }",
        );
        assert!(!has(&d, code::Code::E0414_PrivateAccess), "{d:?}");
        assert!(
            !has(&d, code::Code::E0972_PropertyAccessorVisibility),
            "{d:?}"
        );
    }

    // --- §M.7.7 property visibility (the read side) ---

    /// Reading a `private` property from outside its class is E0414, exactly
    /// as reading a `private` field is.
    ///
    /// The write side (E0970/E0972) was enforced from the start, and the
    /// STATIC property read was checked too, but the INSTANCE property read
    /// returned as soon as it resolved the accessor and never consulted its
    /// visibility. So `private int Level { get; set; }` was readable from
    /// anywhere: the modifier compiled, and meant nothing.
    #[test]
    fn private_property_read_from_outside_is_e0414() {
        let d = run(
            "public class Config { private int Level { get; set; } = 3; }
             public class U { public int go() { Config c = new Config(); return c.Level; } }",
        );
        assert!(has(&d, code::Code::E0414_PrivateAccess), "{d:?}");
    }

    /// A class reads its OWN private property freely — the check is about
    /// crossing the class boundary, not about the modifier existing.
    #[test]
    fn private_property_read_inside_its_own_class_is_ok() {
        let d = run("public class Config { private int Level { get; set; } = 3;
               public int go() { return Level; } }");
        assert!(!has(&d, code::Code::E0414_PrivateAccess), "{d:?}");
    }

    /// A `public` property stays readable from anywhere; the new check must
    /// not fire on the ordinary case.
    #[test]
    fn public_property_read_from_outside_is_ok() {
        let d = run("public class Config { public int Level { get; set; } = 3; }
             public class U { public int go() { Config c = new Config(); return c.Level; } }");
        assert!(!has(&d, code::Code::E0414_PrivateAccess), "{d:?}");
    }

    /// `protected` reaches a subclass (Java rule) and no further.
    #[test]
    fn protected_property_reaches_a_subclass() {
        let d = run(
            "public class Base { protected int Level { get; set; } = 3; }
             public class Sub extends Base { public int go() { return Level; } }",
        );
        assert!(!has(&d, code::Code::E0415_ProtectedAccess), "{d:?}");
    }

    /// The same property from an unrelated class is E0415.
    #[test]
    fn protected_property_from_an_unrelated_class_is_e0415() {
        // In ANOTHER package: per JLS §6.6.1 `protected` also grants package
        // access, so an unrelated peer in the SAME package reaches it legally.
        let d = run_two(
            "package a; public class Base { protected int Level { get; set; } = 3; }",
            "package b; import a.*; public class Other { public int go() { Base b = new Base(); return b.Level; } }",
        );
        assert!(has(&d, code::Code::E0415_ProtectedAccess), "{d:?}");
    }

    /// A WRITE to a private property reports once, not twice.
    ///
    /// An assignment target is a write, so walking it as an expression would
    /// add the read-side E0414 on top of the write-side E0972 and report the
    /// same mistake under two codes. Only the receiver is walked.
    #[test]
    fn a_private_property_write_reports_exactly_one_error() {
        let d = run(
            "public class Config { private int Level { get; set; } = 3; }
             public class U { public void go() { Config c = new Config(); c.Level = 9; } }",
        );
        let reported: Vec<_> = d
            .iter()
            .filter(|x| {
                matches!(
                    x.code,
                    code::Code::E0414_PrivateAccess | code::Code::E0972_PropertyAccessorVisibility
                )
            })
            .collect();
        assert_eq!(
            reported.len(),
            1,
            "one mistake, one diagnostic: {reported:?}"
        );
        assert!(
            has(&d, code::Code::E0972_PropertyAccessorVisibility),
            "{d:?}"
        );
    }

    /// A package-private property does not cross a package boundary.
    #[test]
    fn package_private_property_does_not_cross_packages() {
        let d = run_two(
            "package a; public class Holder { int Level { get; set; } = 3; }",
            "package b; import a.*; public class U { public int go() { Holder h = new Holder(); return h.Level; } }",
        );
        assert!(has(&d, code::Code::E0416_PackagePrivateAccess), "{d:?}");
    }

    /// A `final` LOCAL stays E0464 (binding rule), never E0465 (field rule).
    #[test]
    fn final_local_is_e0464_not_e0465() {
        let d = run("public void f() { final int y = 1; y = 2; }");
        assert!(has(&d, code::Code::E0464_FinalBindingReassigned), "{d:?}");
        assert!(!has(&d, code::Code::E0465_FinalFieldReassigned), "{d:?}");
    }

    /// A non-final local that SHADOWS a `final` param (in an inner scope) may be
    /// reassigned — the shadow un-finals the name for that scope.
    #[test]
    fn shadowing_local_unfinals_param() {
        let d = run("public void f(final int x) { if (true) { var x = 0; x = 1; } }");
        assert!(!has(&d, code::Code::E0464_FinalBindingReassigned), "{d:?}");
    }

    // --- §M.14.3 `weak` parameters (E0455 non-class, E0456 bare read) ---

    /// A `weak` parameter whose type is not a class is E0455.
    #[test]
    fn weak_param_nonclass_is_e0455() {
        let d = run("public void f(weak int x) { }");
        assert!(has(&d, code::Code::E0455_WeakOnNonClass), "{d:?}");
    }

    /// A `weak` parameter of a plain class type is accepted.
    #[test]
    fn weak_param_class_is_ok() {
        let d = run("public class N { } public void f(weak N n) { var x = n.get(); }");
        assert!(!has(&d, code::Code::E0455_WeakOnNonClass), "{d:?}");
    }

    /// Reading a `weak` parameter via `.get()` is fine (→ `T?`); no bare-read error.
    #[test]
    fn weak_param_get_is_ok() {
        let d = run("public class N { } public void f(weak N n) { var x = n.get(); }");
        assert!(!has(&d, code::Code::E0456_WeakReadNeedsGet), "{d:?}");
    }

    /// A BARE read of a `weak` parameter is E0456 (must go through `.get()`).
    #[test]
    fn weak_param_bare_read_is_e0456() {
        let d = run("public class N { } public void f(weak N n) { var x = n; }");
        assert!(has(&d, code::Code::E0456_WeakReadNeedsGet), "{d:?}");
    }

    // --- §M.14.4 default-parameter ordering (E0467) ---

    /// A non-defaulted parameter after a defaulted one is E0467.
    #[test]
    fn default_before_plain_param_is_e0467() {
        let d = run("public void f(int a = 1, int b) { }");
        assert!(has(&d, code::Code::E0467_DefaultParamOrdering), "{d:?}");
    }

    /// Trailing defaults are fine.
    #[test]
    fn trailing_defaults_are_ok() {
        let d = run("public void f(int a, int b = 1, int c = 2) { }");
        assert!(!has(&d, code::Code::E0467_DefaultParamOrdering), "{d:?}");
    }

    /// A trailing varargs after a default is exempt.
    #[test]
    fn default_then_varargs_is_ok() {
        let d = run("public void f(int a = 1, int... xs) { }");
        assert!(!has(&d, code::Code::E0467_DefaultParamOrdering), "{d:?}");
    }

    /// §7.6 — an interface method with no explicit visibility is implicitly
    /// public, so calling it (through the interface type) is NOT E0416.
    #[test]
    fn interface_method_is_public_by_default() {
        let d = run("public interface I { String label(); } \
             public class C implements I { @override public String label() { return \"x\"; } } \
             public void main() { I x = new C(); var s = x.label(); }");
        assert!(!has(&d, code::Code::E0416_PackagePrivateAccess), "{d:?}");
    }

    /// A `default` interface method (no visibility) is likewise public — calling
    /// the inherited default from outside is not E0416.
    #[test]
    fn interface_default_method_is_public() {
        let d = run(
            "public interface I { default String label() { return \"x\"; } } \
             public class C implements I { } \
             public void main() { var c = new C(); var s = c.label(); }",
        );
        assert!(!has(&d, code::Code::E0416_PackagePrivateAccess), "{d:?}");
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
        let d = run(r#"public enum Color { Red, Green, Blue }
               public void main() {
                   var c = Color.Red;
                   switch (c) {
                       case Color.Red -> {}
                       case Color.Green -> {}
                       case Color.Blue -> {}
                   }
               }"#);
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// A `switch` that misses a variant and has no wildcard fires
    /// E0440 naming the missing variant.
    #[test]
    fn switch_over_enum_missing_variant_emits_e0440() {
        let d = run(r#"public enum Color { Red, Green, Blue }
               public void main() {
                   var c = Color.Red;
                   switch (c) {
                       case Color.Red -> {}
                       case Color.Green -> {}
                   }
               }"#);
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
        let d = run(r#"public enum Color { Red, Green, Blue }
               public void main() {
                   var c = Color.Red;
                   switch (c) {
                       case Color.Red -> {}
                       case _ -> {}
                   }
               }"#);
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// Explicit `case var name -> …` bind-pattern is irrefutable:
    /// it catches every remaining variant.
    #[test]
    fn switch_with_bind_arm_is_exhaustive() {
        let d = run(r#"public enum Color { Red, Green, Blue }
               public void main() {
                   var c = Color.Red;
                   switch (c) {
                       case Color.Red -> {}
                       case var other -> {}
                   }
               }"#);
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// Non-enum scrutinees (numeric, string) aren't checked for
    /// exhaustiveness — the wildcard arm remains the user's tool.
    #[test]
    fn switch_over_int_does_not_check_exhaustiveness() {
        let d = run(r#"public void main() {
                   var n = 1;
                   switch (n) {
                       case 0 -> {}
                       case 1 -> {}
                   }
               }"#);
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// `switch` over a sealed-class scrutinee that names every
    /// permitted subclass passes exhaustiveness.
    #[test]
    fn switch_over_sealed_class_with_all_subclasses_is_exhaustive() {
        let d = run(r#"public sealed class Shape permits Circle, Square {}
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
               public void main() {}"#);
        assert!(!has(&d, code::Code::E0440_NotExhaustive), "got: {d:?}");
    }

    /// `switch` over a sealed-class scrutinee that misses a
    /// permitted subclass fires E0440, naming the gap.
    #[test]
    fn switch_over_sealed_class_missing_subclass_emits_e0440() {
        let d = run(r#"public sealed class Shape permits Circle, Square {}
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
               public void main() {}"#);
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
        let d = run(r#"public class Animal { public Animal() {} }
               public class Dog extends Animal {
                   public Dog() {}
               }
               public void main() {
                   var a = new Animal();
                   switch (a) {
                       case Dog -> {}
                   }
               }"#);
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

    /// A `null` initializer on a NON-nullable field is E0410 — including a bare
    /// type parameter `K` (the reported generics bug: it leaked invalid Rust).
    #[test]
    fn null_init_on_non_nullable_field_is_e0410() {
        let gen = run("public class C<K> { private K k = null; } public void main() {}");
        assert!(
            has(&gen, code::Code::E0410_TypeMismatch),
            "generic K: {gen:?}"
        );
        let concrete = run("public class C { private String s = null; } public void main() {}");
        assert!(
            has(&concrete, code::Code::E0410_TypeMismatch),
            "String: {concrete:?}"
        );
    }

    /// A `null` initializer on a NULLABLE field (`K?`) is fine — nullable fields
    /// default to null.
    #[test]
    fn null_init_on_nullable_field_is_ok() {
        let d = run("public class C<K> { private K? k = null; } public void main() {}");
        assert!(!has(&d, code::Code::E0410_TypeMismatch), "K?: {d:?}");
    }

    /// A bare `return;` in a non-`void` function is E0410 (now carrying a span so
    /// the LSP surfaces it); a bare `return;` in a `void` function is fine.
    #[test]
    fn bare_return_in_non_void_is_e0410() {
        let bad = run("public int f() { return ; }");
        assert!(
            has(&bad, code::Code::E0410_TypeMismatch),
            "non-void: {bad:?}"
        );
        let ok = run("public void f() { return ; }");
        assert!(!has(&ok, code::Code::E0410_TypeMismatch), "void: {ok:?}");
    }

    // ---- Async/await context (E0700, §18.1.2) ----

    /// `await` in a plain (non-async) function → E0700.
    #[test]
    fn await_in_plain_function_errors() {
        let d = run(r#"public int g(){ return 1; } public void f(){ var x = await g(); }"#);
        assert!(
            has(&d, code::Code::E0700_AwaitRequiresAsyncContext),
            "got: {d:?}"
        );
    }

    /// `await` inside an `async` function is fine.
    #[test]
    fn await_in_async_function_ok() {
        let d = run(r#"public int g(){ return 1; } public async int f(){ return await g(); }"#);
        assert!(
            !has(&d, code::Code::E0700_AwaitRequiresAsyncContext),
            "got: {d:?}"
        );
    }

    /// Constructors are never async, so `await` in a constructor body → E0700.
    #[test]
    fn await_in_constructor_errors() {
        let d = run(
            r#"public int g(){ return 1; } public class C { public C(){ var x = await g(); } }"#,
        );
        assert!(
            has(&d, code::Code::E0700_AwaitRequiresAsyncContext),
            "got: {d:?}"
        );
    }

    /// `await` inside an `async` method is fine.
    #[test]
    fn await_in_async_method_ok() {
        let d = run(
            r#"public int g(){ return 1; } public class C { public async int m(){ return await g(); } }"#,
        );
        assert!(
            !has(&d, code::Code::E0700_AwaitRequiresAsyncContext),
            "got: {d:?}"
        );
    }

    /// A plain lambda introduces a non-async context even inside an async
    /// function, so `await` in it → E0700.
    #[test]
    fn await_in_plain_lambda_inside_async_errors() {
        let d = run(
            r#"public int g(){ return 1; } public async void f(){ var bad = () -> { return await g(); }; }"#,
        );
        assert!(
            has(&d, code::Code::E0700_AwaitRequiresAsyncContext),
            "got: {d:?}"
        );
    }

    /// An async lambda permits `await`, even inside a plain function.
    #[test]
    fn await_in_async_lambda_ok() {
        let d = run(
            r#"public int g(){ return 1; } public void f(){ var ok = async () -> { return await g(); }; }"#,
        );
        assert!(
            !has(&d, code::Code::E0700_AwaitRequiresAsyncContext),
            "got: {d:?}"
        );
    }

    // ---- unsafe-context enforcement (E0506, §A.2.8 / Layout-ABI §L.5.2) ----

    /// Calling an `unsafe` free function from a plain (non-unsafe) context → E0506.
    #[test]
    fn unsafe_call_outside_unsafe_errors() {
        let d =
            run(r#"public unsafe int risky(){ return 1; } public void f(){ var x = risky(); }"#);
        assert!(
            has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe),
            "got: {d:?}"
        );
    }

    /// The same call wrapped in an `unsafe { … }` block is fine.
    #[test]
    fn unsafe_call_in_unsafe_block_ok() {
        let d = run(
            r#"public unsafe int risky(){ return 1; } public void f(){ unsafe { var x = risky(); } }"#,
        );
        assert!(
            !has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe),
            "got: {d:?}"
        );
    }

    /// An `unsafe` callee invoked from the body of another `unsafe` fn is fine —
    /// the whole body is an unsafe context.
    #[test]
    fn unsafe_call_from_unsafe_fn_ok() {
        let d = run(
            r#"public unsafe int risky(){ return 1; } public unsafe int caller(){ return risky(); }"#,
        );
        assert!(
            !has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe),
            "got: {d:?}"
        );
    }

    /// Calling a SAFE function (no `unsafe` modifier) never trips E0506,
    /// whether or not it's inside an `unsafe` block.
    #[test]
    fn safe_call_never_trips_e0506() {
        let d = run(r#"public int ok(){ return 1; } public void f(){ var x = ok(); }"#);
        assert!(
            !has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe),
            "got: {d:?}"
        );
    }

    /// Address-of `&x` outside an `unsafe` context → E0506 (§A.2.9).
    #[test]
    fn address_of_outside_unsafe_errors() {
        let d = run(r#"public void f(){ int x = 1; int* p = &x; }"#);
        assert!(
            has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe),
            "got: {d:?}"
        );
    }

    /// Raw-pointer deref `*p` outside an `unsafe` context → E0506 (§A.2.9).
    #[test]
    fn deref_outside_unsafe_errors() {
        let d = run(r#"public void f(){ int x = 1; int* p = &x; int y = *p; }"#);
        assert!(
            has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe),
            "got: {d:?}"
        );
    }

    /// The same pointer ops inside an `unsafe { }` block are fine.
    #[test]
    fn pointer_ops_in_unsafe_block_ok() {
        let d = run(r#"public void f(){ int x = 1; unsafe { int* p = &x; *p = 2; } }"#);
        assert!(
            !has(&d, code::Code::E0506_UnsafeOpOutsideUnsafe),
            "got: {d:?}"
        );
    }

    // ---- throw operand must be an Exception (E0710, §X.2.1) ----
    // `run` builds a single unit with no stdlib, so these use a local
    // `Exception` class — `throwable_ok` matches the bare `Exception` segment.

    #[test]
    fn throw_int_errors() {
        let d = run(r#"public class Exception {} public void f(){ throw 5; }"#);
        assert!(
            has(&d, code::Code::E0710_ThrowRequiresException),
            "got: {d:?}"
        );
    }

    #[test]
    fn throw_string_errors() {
        let d = run(r#"public class Exception {} public void f(){ throw "oops"; }"#);
        assert!(
            has(&d, code::Code::E0710_ThrowRequiresException),
            "got: {d:?}"
        );
    }

    #[test]
    fn throw_exception_ok() {
        let d = run(r#"public class Exception {} public void f(){ throw new Exception(); }"#);
        assert!(
            !has(&d, code::Code::E0710_ThrowRequiresException),
            "got: {d:?}"
        );
    }

    #[test]
    fn throw_user_exception_subclass_ok() {
        let d = run(
            r#"public class Exception {} public class MyErr extends Exception {} public void f(){ throw new MyErr(); }"#,
        );
        assert!(
            !has(&d, code::Code::E0710_ThrowRequiresException),
            "got: {d:?}"
        );
    }

    // ---- uninferable empty-diamond `new` (E0453, §T.4.2) ----

    /// `var b = new Box<>()` (generic class, no args) that is never referenced
    /// can't have its type argument pinned → E0453.
    #[test]
    fn unused_uninferable_new_errors() {
        let d =
            run(r#"public class Box<T> { public Box() {} } public void f(){ var b = new Box(); }"#);
        assert!(
            has(&d, code::Code::E0453_GenericInferenceNoSolution),
            "got: {d:?}"
        );
    }

    /// The same construction, but `b` is later used as a receiver — a use could
    /// pin the argument (as the emitted Rust infers), so no E0453.
    #[test]
    fn used_uninferable_new_ok() {
        let d = run(
            r#"public class Box<T> { public Box() {} public void touch(){} } public void f(){ var b = new Box(); b.touch(); }"#,
        );
        assert!(
            !has(&d, code::Code::E0453_GenericInferenceNoSolution),
            "got: {d:?}"
        );
    }

    /// An explicit type argument pins it — never flagged even if unused.
    #[test]
    fn explicit_type_arg_new_ok() {
        let d = run(
            r#"public class Box<T> { public Box() {} } public void f(){ var b = new Box<int>(); }"#,
        );
        assert!(
            !has(&d, code::Code::E0453_GenericInferenceNoSolution),
            "got: {d:?}"
        );
    }

    /// A non-generic class has no argument to infer — never flagged.
    #[test]
    fn non_generic_new_not_flagged() {
        let d = run(
            r#"public class Plain { public Plain() {} } public void f(){ var p = new Plain(); }"#,
        );
        assert!(
            !has(&d, code::Code::E0453_GenericInferenceNoSolution),
            "got: {d:?}"
        );
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
        let d =
            run(r#"public class E1 {} public void f(){ try {} catch (E1 e) {} catch (E1 e2) {} }"#);
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
        let d = run("public class Foo { public int x; } \
             public void main() { var f = new Foo(); print(f.y); }");
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
        let d = run("public class Animal { public int age() { return 5; } } \
             public class Dog extends Animal {} \
             public void main() { var d = new Dog(); print(d.age()); }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// A field defined on a parent class is resolvable from a child.
    #[test]
    fn inherited_field_resolves() {
        let d = run("public class Animal { public int age = 0; } \
             public class Dog extends Animal {} \
             public void main() { var d = new Dog(); print(d.age); }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// E0600: a non-nullable field with no initializer and no constructor that
    /// assigns it is not definitely assigned.
    #[test]
    fn unassigned_field_fires_e0600() {
        let d = run("public class C { public int n; }");
        assert!(
            has(&d, code::Code::E0600_FieldNotDefinitelyAssigned),
            "{d:?}",
        );
    }

    /// A constructor that assigns the field on every path satisfies E0600.
    #[test]
    fn field_assigned_in_ctor_ok() {
        let d = run("public class C { public int n; public C(int n) { this.n = n; } }");
        assert!(
            !has(&d, code::Code::E0600_FieldNotDefinitelyAssigned),
            "{d:?}",
        );
    }

    /// An initializer, a nullable field, and a `weak` field are all exempt.
    #[test]
    fn initializer_nullable_and_weak_fields_exempt_e0600() {
        let d = run("public class P { public int x = 0; } \
             public class C { public int n = 0; public P? p; weak P back; }");
        assert!(
            !has(&d, code::Code::E0600_FieldNotDefinitelyAssigned),
            "{d:?}",
        );
    }

    /// A field assigned on only ONE branch of an `if` is not definitely
    /// assigned (the else path leaves it unset) → E0600.
    #[test]
    fn conditionally_assigned_field_fires_e0600() {
        let d = run("public class C { public int n; \
             public C(bool b) { if (b) { this.n = 1; } } }");
        assert!(
            has(&d, code::Code::E0600_FieldNotDefinitelyAssigned),
            "{d:?}",
        );
    }

    /// `print` accepts any single argument shape.
    #[test]
    fn print_is_builtin_no_arg_check() {
        let d = run(r#"public void main() { print("x"); print(42); print(true); }"#);
        assert!(d.is_empty(), "{d:?}");
    }

    /// `.push` on a dynamic int array is a built-in receiver method —
    /// no error.
    #[test]
    fn array_push_is_builtin() {
        let d = run("public void main() { var xs = new int[]{1, 2, 3}; xs.push(4); }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// `.length` on an int array reads as int — no error.
    #[test]
    fn array_length_is_builtin() {
        let d = run("public void main() { var xs = new int[]{1}; print(xs.length); }");
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
        let d = run("public int add(int a, int b) { return a + b; } \
             public void main() { print(add(1)); }");
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// Top-level call with wrong arg type → E0410. (Note: this exercises
    /// the path even though "Int → String" wouldn't be tolerated.)
    #[test]
    fn top_level_wrong_arg_type_emits_e0410() {
        let d = run(r#"public void greet(String name) { print(name); }
               public void main() { greet(42); }"#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Multiple wrong-type args still emit at least one E0410.
    #[test]
    fn multiple_wrong_args_each_emit_e0410() {
        let d = run(r#"public void f(String a, String b) {}
               public void main() { f(1, 2); }"#);
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
        let d = run(r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
            }
            public void main() {
                var b = new Box<int>("hi");
            }
            "#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Phase E.2 — same ctor, correct type → no diagnostic.
    #[test]
    fn instantiated_ctor_matching_arg_is_ok() {
        let d = run(r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
            }
            public void main() {
                var b = new Box<int>(42);
            }
            "#);
        assert!(d.is_empty(), "{d:?}");
    }

    /// Phase E.2 — raw-type construction (`new Box(...)` with no
    /// turbofish) leaves substitution off, so any arg passes.
    #[test]
    fn raw_ctor_accepts_any_arg() {
        let d = run(r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
            }
            public void main() {
                var a = new Box(42);
                var b = new Box("hi");
            }
            "#);
        assert!(d.is_empty(), "{d:?}");
    }

    /// Phase E.2 — method call on `Box<int>` substitutes the parameter
    /// type, so passing a String to a `set(T v)` is rejected.
    #[test]
    fn instantiated_method_arg_mismatch_emits_e0410() {
        let d = run(r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
                public void set(T v) { this.value = v; }
            }
            public void main() {
                var b = new Box<int>(0);
                b.set("hi");
            }
            "#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Phase E.3 — `super("hi")` against a parent expecting `int`
    /// emits E0410.
    #[test]
    fn super_call_wrong_arg_type_emits_e0410() {
        let d = run(r#"
            public class Animal {
                public Animal(int age) {}
            }
            public class Dog extends Animal {
                public Dog() { super("hi"); }
            }
            "#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Phase E.3 — `super(42)` against `Animal(int)` is fine.
    #[test]
    fn super_call_matching_args_is_ok() {
        let d = run(r#"
            public class Animal {
                public Animal(int age) {}
            }
            public class Dog extends Animal {
                public Dog() { super(42); }
            }
            "#);
        assert!(d.is_empty(), "{d:?}");
    }

    /// Phase E.3 — `super()` (no args) against `Animal(int age)` is a
    /// wrong-arg-count, emits E0411.
    #[test]
    fn super_call_wrong_arg_count_emits_e0411() {
        let d = run(r#"
            public class Animal {
                public Animal(int age) {}
            }
            public class Dog extends Animal {
                public Dog() { super(); }
            }
            "#);
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// Phase E.3 — `super(name)` with substitution through the extends
    /// clause's generic arg. Animal<T> with `Animal(T name)` lets Dog
    /// (extends Animal<String>) pass a String.
    #[test]
    fn super_call_substitutes_extends_generic_arg() {
        let d = run(r#"
            public class Animal<T> {
                public Animal(T name) {}
            }
            public class Dog extends Animal<String> {
                public Dog() { super("rex"); }
            }
            "#);
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
        let d = run(r#"
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
            "#);
        assert!(d.is_empty(), "{d:?}");
    }

    /// Returning the wrong type from an operator body fires E0410 via
    /// the same path methods use — the operator walker sets
    /// `current_return` to the declared return type before walking.
    #[test]
    fn operator_return_type_mismatch_emits_e0410() {
        let d = run(r#"
            public class Path {
                public bool operator==(Path other) {
                    return 42;
                }
            }
            "#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Calling a method with the wrong-typed argument from inside an
    /// operator body still fires the standard E0410 path — proves the
    /// arg-type check path is reachable from within an operator body.
    #[test]
    fn operator_body_call_arg_mismatch_emits_e0410() {
        let d = run(r#"
            public class Path {
                public String value;
                public Path(String v) { this.value = v; }
                public void greet(String name) {}
                public bool operator==(Path other) {
                    this.greet(42);
                    return true;
                }
            }
            "#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// `operator hash()` is zero-arg and its body type-checks cleanly
    /// when the return type matches.
    #[test]
    fn operator_hash_body_typechecks_cleanly() {
        let d = run(r#"
            public class Path {
                public int operator hash() {
                    return 1;
                }
            }
            "#);
        assert!(d.is_empty(), "{d:?}");
    }

    // ----------------------------------------------------------------
    // E0935 — use-of-deleted-operator (§O.3.4)
    // ----------------------------------------------------------------

    /// `$"$t"` on a record whose `operator string()` is deleted fires
    /// E0935 at the interp-string site.
    #[test]
    fn interp_string_on_deleted_string_op_emits_e0935() {
        let d = run(r#"
            public record OpaqueToken(int secret) {
                public String operator string() = delete;
            }
            public void main() {
                var t = new OpaqueToken(42);
                print($"$t");
            }
            "#);
        assert!(has(&d, code::Code::E0935_DeletedOperator), "{d:?}");
    }

    /// `a + b` where `a`'s class deleted `operator+` fires E0935.
    #[test]
    fn arithmetic_on_deleted_op_emits_e0935() {
        let d = run(r#"
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
            "#);
        assert!(has(&d, code::Code::E0935_DeletedOperator), "{d:?}");
    }

    /// `-x` where `x`'s class deleted unary `operator-` fires E0935.
    #[test]
    fn unary_minus_on_deleted_op_emits_e0935() {
        let d = run(r#"
            public class N {
                public int x;
                public N(int x) { this.x = x; }
                public N operator-() = delete;
            }
            public void main() {
                var v = new N(1);
                var w = -v;
            }
            "#);
        assert!(has(&d, code::Code::E0935_DeletedOperator), "{d:?}");
    }

    /// Primitives + non-deleted classes don't fire E0935. Pins that the
    /// check is gated on receiver class + deletion flag.
    #[test]
    fn no_e0935_for_primitives_or_undeleted() {
        let d = run(r#"
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
            "#);
        assert!(
            !d.iter()
                .any(|d| d.code == code::Code::E0935_DeletedOperator),
            "should not emit E0935: {d:?}",
        );
    }

    /// `$"$x"` where x is a primitive (int, String, etc.) doesn't fire
    /// E0935 — primitives don't have an operator-string declaration.
    #[test]
    fn no_e0935_for_primitive_in_interp() {
        let d = run(r#"
            public void main() {
                var x = 42;
                var s = "hi";
                print($"x=$x, s=$s");
            }
            "#);
        assert!(
            !d.iter()
                .any(|d| d.code == code::Code::E0935_DeletedOperator),
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
        let d = run(r#"
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
            "#);
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "covariant assignment should be accepted: {d:?}",
        );
    }

    /// `List<Cat>` is NOT assignable to `List<? extends Dog>` —
    /// Cat isn't a Dog.
    #[test]
    fn extends_wildcard_rejects_non_subtype() {
        let d = run(r#"
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
            "#);
        assert!(
            d.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "List<Cat> shouldn't fit List<? extends Dog>: {d:?}",
        );
    }

    /// `List<Animal>` is assignable to `List<? super Dog>` —
    /// Animal is a supertype of Dog, slot is contravariant (consumer).
    #[test]
    fn super_wildcard_accepts_supertype() {
        let d = run(r#"
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
            "#);
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "contravariant assignment should be accepted: {d:?}",
        );
    }

    /// `List<Cat>` is NOT assignable to `List<? super Dog>` —
    /// Cat is not a supertype of Dog.
    #[test]
    fn super_wildcard_rejects_unrelated() {
        let d = run(r#"
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
            "#);
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
        let d = run(r#"
            public class Account {
                private int balance;
                public Account(int n) { this.balance = n; }
            }
            public void main() {
                var a = new Account(10);
                print(a.balance);
            }
            "#);
        assert!(
            d.iter().any(|d| d.code == code::Code::E0414_PrivateAccess),
            "expected E0414, got: {d:?}",
        );
    }

    /// Reading a private field from inside the same class is OK.
    #[test]
    fn private_field_access_from_same_class_is_ok() {
        let d = run(r#"
            public class Account {
                private int balance;
                public Account(int n) { this.balance = n; }
                public int get() { return this.balance; }
            }
            public void main() {
                var a = new Account(10);
                print(a.get());
            }
            "#);
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0414_PrivateAccess),
            "should not emit E0414 for self-access: {d:?}",
        );
    }

    /// Calling a protected method from an unrelated class in ANOTHER package
    /// fires E0415.
    ///
    /// The package matters: per JLS §6.6.1, `protected` also grants package
    /// access, so an unrelated peer sharing the package reaches it legally.
    /// This test used to assert on two unpackaged classes, which put them in
    /// the same (default) package and therefore asserted the opposite of the
    /// Java rule.
    #[test]
    fn protected_method_from_unrelated_class_in_another_package_emits_e0415() {
        let d = run_two(
            "package a; public class Base { protected void secret() {} }",
            "package b; import a.*; public class Other { public void touch(Base b) { b.secret(); } }",
        );
        assert!(
            d.iter()
                .any(|d| d.code == code::Code::E0415_ProtectedAccess),
            "expected E0415, got: {d:?}",
        );
    }

    /// The same call from a peer in the SAME package is legal (§6.6.1).
    #[test]
    fn protected_method_from_a_same_package_peer_is_ok() {
        let d = run_two(
            "package a; public class Base { protected void secret() {} }",
            "package a; public class Other { public void touch(Base b) { b.secret(); } }",
        );
        assert!(
            !d.iter()
                .any(|d| d.code == code::Code::E0415_ProtectedAccess),
            "same-package access is legal: {d:?}",
        );
    }

    /// Calling a protected method from a subclass is OK.
    #[test]
    fn protected_method_from_subclass_is_ok() {
        let d = run(r#"
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
            "#);
        assert!(
            !d.iter()
                .any(|d| d.code == code::Code::E0415_ProtectedAccess),
            "should not emit E0415 for subclass access: {d:?}",
        );
    }

    /// `new Foo()` against a private constructor fires E0414.
    #[test]
    fn private_constructor_emits_e0414() {
        let d = run(r#"
            public class Singleton {
                private Singleton() {}
            }
            public void main() {
                var s = new Singleton();
                print(s);
            }
            "#);
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
        let d = run(r#"
            public class Math {
                public static final int X = 1;
                public static int dbl(int n) { return n + n; }
            }
            public void main() {
                print(Math.X);
                print(Math.dbl(5));
            }
            "#);
        assert!(d.is_empty(), "expected clean tycheck: {d:?}");
    }

    /// `this` inside a `static` method fires E0425.
    #[test]
    fn this_in_static_method_emits_e0425() {
        let d = run(r#"
            public class C {
                public int x;
                public C() { this.x = 0; }
                public static int f() { return this.x; }
            }
            public void main() { print(C.f()); }
            "#);
        assert!(
            d.iter()
                .any(|d| d.code == code::Code::E0425_ThisInStaticContext),
            "expected E0425: {d:?}",
        );
    }

    /// Reading an instance field through the class name (`C.x`)
    /// fires a clear `E0412` with the "instance field" message.
    #[test]
    fn instance_field_via_classname_emits_e0412() {
        let d = run(r#"
            public class C { public int x; public C() { this.x = 0; } }
            public void main() { print(C.x); }
            "#);
        assert!(
            d.iter()
                .any(|d| d.code == code::Code::E0412_UnresolvedField),
            "expected E0412: {d:?}",
        );
    }

    /// Unbounded `?` accepts anything in the slot.
    #[test]
    fn unbounded_wildcard_accepts_anything() {
        let d = run(r#"
            public class List<T> {
                public T head;
            }
            public void main() {
                var ints = new List<int>();
                List<?> any = ints;
                print(any);
            }
            "#);
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
        let d = run(r#"
            public class P { public String Name { get; set; } }
            public void main() {
                var p = new P();
                p.Name = "Bob";
                print(p.Name);
            }
            "#);
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
        let d = run(r#"
            public class P {
                public int Id { get; }
                public P() { this.Id = 7; }
            }
            public void main() { var p = new P(); p.Id = 1; }
            "#);
        assert!(
            has(&d, code::Code::E0970_PropertyNotWritable),
            "expected E0970: {d:?}"
        );
    }

    /// §P removed the `init` accessor — the read-only-after-construction
    /// shape is `{ get; }` (settable in the ctor), and a post-construction
    /// write to it fires E0970.
    #[test]
    fn write_ctor_settable_readonly_property_after_construction_errors() {
        let d = run(r#"
            public class P {
                public String Code { get; }
                public P(String c) { this.Code = c; }
            }
            public void main() { var p = new P("a"); p.Code = "x"; }
            "#);
        assert!(
            has(&d, code::Code::E0970_PropertyNotWritable),
            "expected E0970: {d:?}"
        );
    }

    /// Writing a `{ get; private set; }` property from outside the
    /// declaring class fires E0972.
    #[test]
    fn write_private_set_property_from_outside_errors() {
        let d = run(r#"
            public class P {
                public String Token { get; private set; }
                public P() { this.Token = "t"; }
            }
            public void main() { var p = new P(); p.Token = "y"; }
            "#);
        assert!(
            has(&d, code::Code::E0972_PropertyAccessorVisibility),
            "expected E0972: {d:?}",
        );
    }

    /// The constructor may set read-only / private-set properties —
    /// the desugarer lowers those to backing-field writes, so no
    /// access-control diagnostic fires. (`init` accessors were removed
    /// by §P; `{ get; }` covers the construction-time-settable shape.)
    #[test]
    fn ctor_may_set_restricted_properties() {
        let d = run(r#"
            public class P {
                public int Id { get; }
                public String Code { get; }
                public String Token { get; private set; }
                public P(String c) { this.Id = 1; this.Code = c; this.Token = "t"; }
            }
            public void main() { var p = new P("a"); print(p.Id); }
            "#);
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

    /// JLS 6.6.2.1: across a package boundary, a subclass may reach an
    /// inherited `protected` member only through a qualifier typed as itself.
    /// A sibling subclass typed as the BASE is not permitted, even though the
    /// accessor inherits the member.
    #[test]
    fn protected_through_a_base_qualifier_across_packages_is_e0415() {
        let d = run_two(
            "package base;
public class C { protected int tag; public C() { this.tag = 0; } }",
            "package sub;
import base.C;
public class S extends C { public int peek(C other) { return other.tag; } }",
        );
        assert!(
            has(&d, code::Code::E0415_ProtectedAccess),
            "expected E0415: {d:?}"
        );
    }

    /// The same access through `this` is exactly what `protected` is for.
    #[test]
    fn protected_through_this_across_packages_is_allowed() {
        let d = run_two(
            "package base;
public class C { protected int tag; public C() { this.tag = 0; } }",
            "package sub;
import base.C;
public class S extends C { public int peek() { return this.tag; } }",
        );
        assert!(
            !has(&d, code::Code::E0415_ProtectedAccess),
            "`this` is always permitted: {d:?}"
        );
    }

    /// A qualifier typed as the SUBCLASS is permitted.
    #[test]
    fn protected_through_a_subclass_qualifier_is_allowed() {
        let d = run_two(
            "package base;
public class C { protected int tag; public C() { this.tag = 0; } }",
            "package sub;
import base.C;
public class S extends C { public int peek(S other) { return other.tag; } }",
        );
        assert!(
            !has(&d, code::Code::E0415_ProtectedAccess),
            "a qualifier typed S is permitted: {d:?}"
        );
    }

    /// Same PACKAGE is unaffected — there `protected` already grants package
    /// access, so the qualifier rule does not apply at all.
    #[test]
    fn protected_through_a_base_qualifier_in_one_package_is_allowed() {
        let d = run_two(
            "package one;
public class C { protected int tag; public C() { this.tag = 0; } }",
            "package one;
public class S extends C { public int peek(C other) { return other.tag; } }",
        );
        assert!(
            !has(&d, code::Code::E0415_ProtectedAccess),
            "same package is unaffected: {d:?}"
        );
    }

    /// Reading a PRIVATE field through a polymorphic-base reference → E0437
    /// (no accessor is generated for private fields).
    #[test]
    fn private_field_through_polymorphic_base_emits_e0437() {
        let d = run(r#"
            public class Animal { private String name; public Animal(String n){ this.name = n; } public String speak(){ return "..."; } }
            public class Dog extends Animal { public Dog(String n){ super(n); } public String speak(){ return "woof"; } }
            public void main() { Animal a = new Dog("Rex"); print(a.speak()); var n = a.name; }
            "#);
        assert!(
            has(&d, code::Code::E0437_FieldThroughPolymorphicBase),
            "expected E0437: {d:?}"
        );
    }

    /// An `internal` field through a polymorphic base is NOT private: the
    /// backend generates the `__get_<f>` accessor for every non-private field,
    /// so refusing it here made this check stricter than the code it guards —
    /// and it said "private field" about a field that is not private.
    #[test]
    fn internal_field_through_polymorphic_base_is_allowed() {
        let d = run(r#"
            public class Animal { internal String name; public Animal(String n){ this.name = n; } public String speak(){ return "..."; } }
            public class Dog extends Animal { public Dog(String n){ super(n); } public String speak(){ return "woof"; } }
            public void main() { Animal a = new Dog("Rex"); print(a.name); }
            "#);
        assert!(
            !has(&d, code::Code::E0437_FieldThroughPolymorphicBase),
            "an internal field has an accessor: {d:?}",
        );
    }

    /// The same for a package-private field (no modifier at all).
    #[test]
    fn package_private_field_through_polymorphic_base_is_allowed() {
        let d = run(r#"
            public class Animal { String name; public Animal(String n){ this.name = n; } public String speak(){ return "..."; } }
            public class Dog extends Animal { public Dog(String n){ super(n); } public String speak(){ return "woof"; } }
            public void main() { Animal a = new Dog("Rex"); print(a.name); }
            "#);
        assert!(
            !has(&d, code::Code::E0437_FieldThroughPolymorphicBase),
            "a package-private field has an accessor: {d:?}",
        );
    }

    /// A `private` NESTED class is ordinary Java. Nested types are lifted to a
    /// flat `Owner__Name` before the top-level visibility check runs, so every
    /// one of them used to arrive there looking top-level and got E0432.
    #[test]
    fn private_nested_class_is_not_a_top_level_visibility_error() {
        let d = run(r#"
            public class Registry {
                private int tag;
                public Registry() { this.tag = 1; }
                private class Counter {
                    private int n;
                    public Counter() { this.n = 0; }
                }
            }
            public void main() { var r = new Registry(); }
            "#);
        assert!(
            !has(&d, code::Code::E0432_InvalidTopLevelVisibility),
            "a private nested class is legal: {d:?}",
        );
    }

    /// A genuinely top-level `private` class is still rejected — the fix must
    /// not have turned the rule off.
    #[test]
    fn private_top_level_class_still_emits_e0432() {
        let d = run("private class Hidden { }
public void main() { }");
        assert!(
            has(&d, code::Code::E0432_InvalidTopLevelVisibility),
            "expected E0432: {d:?}"
        );
    }

    /// Reading a PUBLIC field through a polymorphic-base reference is allowed
    /// (the generated `__get_<f>` accessor handles it) — no E0437.
    #[test]
    fn public_field_through_polymorphic_base_is_allowed() {
        let d = run(r#"
            public class Animal { public String name; public Animal(String n){ this.name = n; } public String speak(){ return "..."; } }
            public class Dog extends Animal { public Dog(String n){ super(n); } public String speak(){ return "woof"; } }
            public void main() { Animal a = new Dog("Rex"); print(a.name); }
            "#);
        assert!(
            !has(&d, code::Code::E0437_FieldThroughPolymorphicBase),
            "public field via accessor: {d:?}"
        );
    }

    /// Accessing a field on `this` (concrete self) or on a concrete subclass
    /// reference must NOT trip E0437.
    #[test]
    fn field_on_this_or_concrete_no_e0437() {
        let d = run(r#"
            public class Animal { public String name; public Animal(String n){ this.name = n; } public String who(){ return this.name; } public String speak(){ return "..."; } }
            public class Dog extends Animal { public Dog(String n){ super(n); } public String speak(){ return "woof"; } }
            public void main() { var d = new Dog("Rex"); print(d.name); }
            "#);
        assert!(
            !has(&d, code::Code::E0437_FieldThroughPolymorphicBase),
            "this/concrete field access must be clean: {d:?}"
        );
    }

    /// A generic virtual method on a polymorphic base → E0438.
    #[test]
    fn generic_virtual_method_on_base_emits_e0438() {
        let d = run(r#"
            public class Base { public <R> R pick(R x){ return x; } }
            public class Sub extends Base {}
            public void main() {}
            "#);
        assert!(
            has(&d, code::Code::E0438_GenericVirtualMethod),
            "expected E0438: {d:?}"
        );
    }

    /// A cast between two unrelated classes can never succeed → E0442.
    #[test]
    fn unrelated_class_cast_emits_e0442() {
        let d = run(r#"
            public abstract class Animal { public abstract String sound(); }
            public class Dog extends Animal { public Dog() {} public String sound() { return "w"; } }
            public class Cat extends Animal { public Cat() {} public String sound() { return "m"; } }
            public void main() { var dog = new Dog(); var c = dog as Cat; }
            "#);
        assert!(
            has(&d, code::Code::E0442_UnrelatedCast),
            "expected E0442: {d:?}"
        );
    }

    /// A type-test binder outside an `if` condition is rejected (E0441).
    #[test]
    fn typetest_binder_outside_if_emits_e0441() {
        let d = run(r#"
            public abstract class Animal { public abstract String s(); }
            public class Dog extends Animal { public Dog() {} public String s() { return "w"; } }
            public void main() { Animal a = new Dog(); var b = a => Dog d; }
            "#);
        assert!(
            has(&d, code::Code::E0441_TypeTestBinderMisplaced),
            "expected E0441: {d:?}"
        );
    }

    /// `if (x => Dog d)` binds `d: Dog` in the then-branch and is clean.
    #[test]
    fn typetest_smartcast_binder_in_if_is_ok() {
        let d = run(r#"
            public abstract class Animal { public abstract String s(); }
            public class Dog extends Animal { public Dog() {} public String s() { return "w"; } public String fetch() { return "f"; } }
            public void main() {
                Animal a = new Dog();
                if (a => Dog d) { print(d.fetch()); }
            }
            "#);
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
        let d = run(r#"
            public abstract class Animal { public abstract String sound(); }
            public interface Named { String label(); }
            public class Dog extends Animal { public Dog() {} public String sound() { return "w"; } }
            public class Tagged extends Animal implements Named { public Tagged() {} public String sound() { return "t"; } public String label() { return "T"; } }
            public void main() {
                Animal a = new Dog(); var d = a as Dog;        // downcast
                Animal t = new Tagged(); var n = t as Named;    // interface sidecast
            }
            "#);
        assert!(
            !has(&d, code::Code::E0442_UnrelatedCast),
            "valid downcast/sidecast: {d:?}"
        );
    }

    /// `super.method()` in a class with no superclass is rejected.
    #[test]
    fn super_without_superclass_is_rejected() {
        let d = run(r#"
            public class Animal { public String speak() { return super.speak(); } }
            public void main() {}
            "#);
        assert!(
            d.iter().any(|x| x.message.contains("super")),
            "expected a `super` diagnostic: {d:?}",
        );
    }

    /// `super.method()` from a real override resolves cleanly (no error).
    #[test]
    fn super_call_from_override_is_ok() {
        let d = run(r#"
            public class Animal { public String speak() { return "generic"; } }
            public class Dog extends Animal {
                public Dog() {}
                public String speak() { return super.speak(); }
            }
            public void main() { var dog = new Dog(); print(dog.speak()); }
            "#);
        assert!(
            !d.iter().any(|x| x.message.contains("super")),
            "valid super.method() should be clean: {d:?}",
        );
    }

    /// A generic method on a NON-extended (leaf) class is not a virtual
    /// dispatch concern → no E0438.
    #[test]
    fn generic_method_on_leaf_no_e0438() {
        let d = run(r#"
            public class Util { public <R> R pick(R x){ return x; } }
            public void main() {}
            "#);
        assert!(
            !has(&d, code::Code::E0438_GenericVirtualMethod),
            "leaf generic method must be clean: {d:?}"
        );
    }

    /// A generic-method interface used as a value-typed local can't be a
    /// trait object (object safety) → E0435.
    #[test]
    fn generic_method_interface_value_local_emits_e0435() {
        let d = run(r#"
            public interface Mapper { <R> R map(R input); }
            public class Id implements Mapper { public <R> R map(R input) { return input; } }
            public void main() { Mapper m = new Id(); }
            "#);
        assert!(
            has(&d, code::Code::E0435_InterfaceNotDynDispatchable),
            "expected E0435 for generic-method interface value: {d:?}",
        );
    }

    /// A raw generic interface (no type argument) as a value type → E0435.
    #[test]
    fn raw_generic_interface_value_param_emits_e0435() {
        let d = run(r#"
            public interface Box<T> { T get(); }
            public void use(Box b) {}
            public void main() {}
            "#);
        assert!(
            has(&d, code::Code::E0435_InterfaceNotDynDispatchable),
            "expected E0435 for raw generic interface value: {d:?}",
        );
    }

    /// A generic interface WITH a concrete type argument is a working trait
    /// object (`dyn Box<int>`) — must NOT trip E0435.
    #[test]
    fn concrete_generic_interface_value_is_ok() {
        let d = run(r#"
            public interface Box<T> { T get(); }
            public void use(Box<int> b) {}
            public void main() {}
            "#);
        assert!(
            !has(&d, code::Code::E0435_InterfaceNotDynDispatchable),
            "Box<int> value type should be allowed: {d:?}",
        );
    }

    /// A plain non-generic interface value type is the common, supported
    /// case — never E0435.
    #[test]
    fn plain_interface_value_field_is_ok() {
        let d = run(r#"
            public interface Shape { double area(); }
            public class Holder { public Shape s; public Holder(Shape s) { this.s = s; } }
            public void main() {}
            "#);
        assert!(
            !has(&d, code::Code::E0435_InterfaceNotDynDispatchable),
            "plain interface value field should be allowed: {d:?}",
        );
    }

    /// A bounded wildcard on a user generic class in a FIELD slot →
    /// E0444 (covariant container storage isn't supported in Phase 1).
    #[test]
    fn wildcard_storage_field_emits_e0444() {
        let d = run(r#"
            public class Animal { public String name; public Animal(String n) { this.name = n; } }
            public class Bag<T> { public T item; public Bag(T item) { this.item = item; } }
            public class Holder {
                public Bag<? extends Animal> contents;
                public Holder(Bag<? extends Animal> c) { this.contents = c; }
            }
            public void main() {}
            "#);
        assert!(
            has(&d, code::Code::E0444_WildcardStorageUnsupported),
            "expected E0444 for wildcard storage field: {d:?}",
        );
    }

    /// A bounded wildcard in PARAMETER position lifts to a function
    /// generic and is sound — must NOT trip E0444.
    #[test]
    fn wildcard_param_does_not_emit_e0444() {
        let d = run(r#"
            public class Animal { public String name; public Animal(String n) { this.name = n; } }
            public class Bag<T> { public T item; public Bag(T item) { this.item = item; } }
            public void describe(Bag<? extends Animal> b) {}
            public void main() {}
            "#);
        assert!(
            !has(&d, code::Code::E0444_WildcardStorageUnsupported),
            "param-position wildcard should be allowed: {d:?}",
        );
    }

    /// A concrete type argument in a storage slot (`Bag<Dog>`) carries no
    /// wildcard — never E0444.
    #[test]
    fn concrete_storage_field_is_ok() {
        let d = run(r#"
            public class Animal { public String name; public Animal(String n) { this.name = n; } }
            public class Dog extends Animal { public Dog(String n) { super(n); } }
            public class Bag<T> { public T item; public Bag(T item) { this.item = item; } }
            public class Holder {
                public Bag<Dog> contents;
                public Holder(Bag<Dog> c) { this.contents = c; }
            }
            public void main() {}
            "#);
        assert!(
            !has(&d, code::Code::E0444_WildcardStorageUnsupported),
            "concrete-arg storage field should be allowed: {d:?}",
        );
    }

    /// A type supplied where a const value is expected
    /// (`new Buf<String>()` against `class Buf<int N>`) → E0445.
    #[test]
    fn type_in_const_slot_emits_e0445() {
        let d = run(r#"
            public class Buf<int N> { public Buf() { } }
            public void main() { var b = new Buf<String>(); }
            "#);
        assert!(
            has(&d, code::Code::E0445_ConstGenericUnsupported),
            "expected E0445 for a type in a const slot: {d:?}",
        );
    }

    /// A literal supplied where a type is expected (`new Box<256>(5)`)
    /// → E0445.
    #[test]
    fn literal_in_type_slot_emits_e0445() {
        let d = run(r#"
            public class Box<T> { public T v; public Box(T v) { this.v = v; } }
            public void main() { var b = new Box<256>(5); }
            "#);
        assert!(
            has(&d, code::Code::E0445_ConstGenericUnsupported),
            "expected E0445 for a literal in a type slot: {d:?}",
        );
    }

    /// A fixed-array size that divides by zero folds to a compile-time panic.
    #[test]
    fn const_eval_div_by_zero_in_array_size_e0842() {
        let d = run("public class C { byte[1 / 0] data; }");
        assert!(has(&d, code::Code::E0842_ConstEvalPanic), "{d:?}");
    }

    /// A fixed-array size that overflows folds to a compile-time panic.
    #[test]
    fn const_eval_overflow_in_array_size_e0842() {
        let d = run("public class C { byte[9223372036854775807 + 1] data; }");
        assert!(has(&d, code::Code::E0842_ConstEvalPanic), "{d:?}");
    }

    /// A const-position call to a non-terminating function exhausts the budget.
    #[test]
    fn const_eval_runaway_recursion_e0840() {
        let d = run("int rec(int x) { return rec(x); } \
             public class C { int[rec(1)] data; }");
        assert!(has(&d, code::Code::E0840_ConstEvalLimitExceeded), "{d:?}");
    }

    /// A non-const arithmetic array size (over a runtime local) is rejected.
    #[test]
    fn runtime_new_array_size_heaps_no_e0841() {
        // §5.6: a runtime-sized `new T[n]` EXPRESSION is a HEAP allocation
        // (`vec![..; n]`), so a non-const size is accepted — that's how a
        // runtime-sized buffer is allocated. (A fixed array *type* `int[n]`
        // still requires a const size; the type-position check is unchanged.)
        let d = run("public void main() { var n = 5; var a = new int[n + 1]; }");
        assert!(!has(&d, code::Code::E0841_NonConstInConstContext), "{d:?}");
    }

    /// REGRESSION: const-generic ARITHMETIC over a generic param stays E0445
    /// (Rust-blocked), never the new const-eval codes.
    #[test]
    fn generic_param_arithmetic_array_size_still_e0445() {
        let d = run("public class S<int N> { byte[N + 1] data; }");
        assert!(has(&d, code::Code::E0445_ConstGenericUnsupported), "{d:?}");
        assert!(!has(&d, code::Code::E0841_NonConstInConstContext), "{d:?}");
        assert!(!has(&d, code::Code::E0842_ConstEvalPanic), "{d:?}");
    }

    /// REGRESSION: a bare generic-param array size `[N]` is still accepted
    /// (no E0445 — it forwards as a Rust const-generic arg).
    #[test]
    fn bare_generic_param_array_size_ok() {
        let d = run("public class B<int N> { int[N] data; }");
        assert!(!has(&d, code::Code::E0445_ConstGenericUnsupported), "{d:?}");
    }

    /// Const arithmetic over CONCRETE consts is accepted (no const-eval error).
    #[test]
    fn concrete_const_arithmetic_array_size_ok() {
        let d = run("const int SIZE = 4; \
             public void main() { var a = new int[SIZE + 1]; }");
        assert!(!has(&d, code::Code::E0445_ConstGenericUnsupported), "{d:?}");
        assert!(!has(&d, code::Code::E0841_NonConstInConstContext), "{d:?}");
        assert!(!has(&d, code::Code::E0842_ConstEvalPanic), "{d:?}");
    }

    /// Const-generic arithmetic in a `new T[N + 1]` EXPRESSION now heaps
    /// (`vec![..; N + 1]`, valid because `N` is a `usize` const-generic in
    /// scope) — so NO E0445. The fixed array *type* `byte[N + 1]` still gets
    /// E0445 (see `generic_param_arithmetic_array_size_still_e0445`).
    #[test]
    fn const_generic_arithmetic_new_array_heaps_no_e0445() {
        let d = run(r#"
            public class S<int N> {
                public S() { }
                public int probe() { var a = new int[N + 1]; return 0; }
            }
            public void main() { }
            "#);
        assert!(
            !has(&d, code::Code::E0445_ConstGenericUnsupported),
            "expected `new int[N + 1]` to heap (no E0445): {d:?}",
        );
    }

    /// A class whose members are all shareable may cross a worker boundary —
    /// §18.2 upgrades its refcount to an atomic one rather than refusing.
    #[test]
    fn shareable_object_captured_by_spawn_is_accepted() {
        let d = run(r#"
            public class Counter { public int n; public Counter() { this.n = 0; } }
            public void main() {
                var c = new Counter();
                var t = Worker.spawn(() -> { return c.n; });
            }
            "#);
        assert!(
            !has(&d, code::Code::E0702_ObjectCapturedBySpawn),
            "a shareable class should cross a worker boundary: {d:?}",
        );
    }

    /// A class holding a single-threaded shared reference cannot come along,
    /// and the diagnostic names the member that blocks it.
    #[test]
    fn unshareable_object_captured_by_spawn_emits_e0702() {
        let d = run(r#"
            public interface Sink { void write(String s); }
            public class Service {
                private Sink out;
                public Service(Sink out) { this.out = out; }
                public void go() { this.out.write("x"); }
            }
            public class Logger implements Sink {
                public void write(String s) { print(s); }
            }
            public void main() {
                var svc = new Service(new Logger());
                var t = Worker.spawn(() -> { svc.go(); return 1; });
            }
            "#);
        assert!(
            has(&d, code::Code::E0702_ObjectCapturedBySpawn),
            "expected E0702 for an unshareable capture: {d:?}",
        );
        assert!(
            d.iter().any(|x| x.message.contains("Service.out")),
            "the diagnostic should name the blocking member: {d:?}",
        );
    }

    /// Primitive / String captures cross threads fine — no E0702.
    #[test]
    fn primitive_captures_in_spawn_are_ok() {
        let d = run(r#"
            public void main() {
                int n = 5;
                String tag = "x";
                var t = Worker.spawn(() -> { return n + tag.length(); });
            }
            "#);
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
        let d = run(r#"
            public class Buf<int N> {
                public int[N] data;
                public Buf() { this.data = new int[N]; }
                public int cap() { return N; }
            }
            public void main() { var b = new Buf<4>(); print(b.cap()); }
            "#);
        assert!(
            !has(&d, code::Code::E0445_ConstGenericUnsupported),
            "core const-generic subset should be clean: {d:?}",
        );
    }

    // --- §A `incdec` — expression-position ++/-- (value form, N3) ---

    /// `var y = x++;` over a numeric local is clean, and `y` is typed as
    /// the operand's type — proven by using `y` in a further numeric
    /// step without any type error.
    #[test]
    fn incdec_value_initializes_numeric_local_cleanly() {
        let d = run("public void main() { int x = 1; var y = x++; y += 1; print(y); }");
        assert!(
            d.is_empty(),
            "`var y = x++` over an int should typecheck cleanly: {d:?}",
        );
    }

    /// Both prefix and postfix value forms on a numeric place are clean
    /// in every common consuming position (call arg, index, initializer).
    #[test]
    fn incdec_value_forms_are_clean_on_numeric() {
        let d = run(r#"public void main() {
                   int x = 1;
                   var arr = new int[]{0, 0};
                   int i = 0;
                   print(x++);
                   print(++x);
                   var a = arr[i++];
                   var b = --arr[i];
                   print(a + b);
               }"#);
        assert!(
            d.is_empty(),
            "numeric inc/dec value forms should be clean: {d:?}"
        );
    }

    /// `++` / `--` on a NON-numeric place (a `String`) is rejected
    /// (E0200) — the operator only applies to numeric values.
    #[test]
    fn incdec_on_non_numeric_is_e0200() {
        let d = run(r#"public void main() { String s = "hi"; print(s++); }"#);
        assert!(
            has(&d, code::Code::E0200_UnexpectedToken),
            "`s++` on a String should fire E0200: {d:?}",
        );
    }
    // ---- Numeric promotion and slots (JUX-SEMANTICS §S.2.6, §S.2.7) ----

    /// A signed and an unsigned integer that no one type holds have no common
    /// type in arithmetic: silently picking the unsigned side made `-1 + len`
    /// a huge number.
    #[test]
    fn signed_unsigned_arithmetic_without_common_type_is_e0410() {
        let d = run("int f(int a, uint b) { return a + (int) b; }
                     long g(long a, ulong b) { var c = a + b; return 0L; }");
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
        let d = run("long f(long a, u32 b) { return a + b; }
                     int g(int a, ushort b) { return a * b; }");
        assert!(!has(&d, code::Code::E0410_TypeMismatch), "a holding signed type is fine: {d:?}");
    }

    /// Comparisons are exact, never an error, and an untyped literal takes the
    /// other side's type.
    #[test]
    fn signed_unsigned_comparison_and_literals_are_accepted() {
        let d = run("bool f(int a, uint b) { return a < b; }
                     uint g(uint b) { return b - 1; }");
        assert!(!has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// A value widens into a slot of a type it promotes to (§S.2.7).
    #[test]
    fn numeric_widening_into_slots_is_accepted() {
        let d = run("double f(float x) { double d = x; return x; }
                     i32 g(short s) { i32 x = s; return s; }
                     double h(i32 n) { double d = n; return d; }");
        assert!(!has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// The arms of a conditional promote: `t ? 1 : 2.5` is a `double`.
    #[test]
    fn ternary_numeric_arms_promote() {
        let d = run("int f(bool t) { int x = t ? 1 : 2.5; return x; }");
        assert!(has(&d, code::Code::E0410_TypeMismatch), "a double into an int slot: {d:?}");
        let d = run("double f(bool t) { double x = t ? 1 : 2.5; return x; }");
        assert!(!has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// An integer constant is evaluated at compile time: an overflow, and a
    /// value outside the constant's own type, are E0842.
    #[test]
    fn integer_constant_overflow_is_e0842() {
        let d = run("const long OV = 9223372036854775807L + 1L;");
        assert!(has(&d, code::Code::E0842_ConstEvalPanic), "{d:?}");
        let d = run("const i32 X = 2147483647 + 1;");
        assert!(has(&d, code::Code::E0842_ConstEvalPanic), "{d:?}");
        let d = run("const i32 X = 2147483646 + 1;");
        assert!(!has(&d, code::Code::E0842_ConstEvalPanic), "{d:?}");
    }
}
