//! Field definite-assignment analysis (§S.4.5, diagnostic **E0600**).
//!
//! Every non-nullable, non-`weak`, initializer-less **instance** field of a
//! class must be definitely assigned by the end of construction — assigned on
//! *every* normal-completion path through each constructor (plus the instance
//! `init` blocks that run before it). A field with a textual initializer is
//! trivially assigned; `weak` and nullable fields default to null and are
//! exempt; `record`s assign all components via their primary constructor and
//! never reach here.
//!
//! The analysis is a small forward dataflow over the constructor body. It is
//! deliberately **conservative about what counts as an assignment** so it never
//! reports a false positive on valid code:
//!
//! - `this.f = …` (and `f = …` for a field name) marks `f` assigned.
//! - a `this(…)` delegating constructor is skipped entirely — the delegated-to
//!   constructor owns field initialization (Java's rule).
//! - an explicit `this.helper(…)` call is treated as assigning *all* required
//!   fields (an init-helper may assign them; we can't see through the call, so
//!   we assume the best rather than flag a false positive).
//! - `if`/`else` merges by **intersection** (a field is assigned after the `if`
//!   only if assigned on both arms); loop bodies may run zero times, so their
//!   assignments do not escape; `return` is a completion path that must satisfy
//!   the requirement; `throw`/`break`/`continue` divert control and impose no
//!   end-of-construction obligation on that path.

use std::collections::{HashMap, HashSet};

use juxc_ast::{Block, CallExpr, ClassDecl, ConstructorDecl, ElseBranch, Expr, IfStmt, Stmt};
use juxc_source::Span;

/// One field that may remain unassigned at the end of construction.
pub(crate) struct DaViolation {
    /// The unassigned field's internal name (the `__prop_…` slot for an
    /// auto-property). Used for the analysis; not for the message.
    pub field: String,
    /// The user-visible name to print in the diagnostic — the property name
    /// for an auto-property backing slot, otherwise the field name.
    pub display: String,
    /// Span of the field (property) declaration (where E0600 is reported).
    pub span: Span,
}

/// Run field definite-assignment for a class and return its E0600 candidates,
/// ordered by field name for deterministic diagnostics.
pub(crate) fn analyze_class(class: &ClassDecl) -> Vec<DaViolation> {
    // 1. The fields that REQUIRE definite assignment: instance, non-`weak`,
    //    non-nullable, with no textual initializer.
    let mut required: HashSet<String> = HashSet::new();
    let mut spans: HashMap<String, Span> = HashMap::new();
    // field-internal-name -> user-visible display name (the property name for
    // an auto-property backing slot, else the field name).
    let mut display: HashMap<String, String> = HashMap::new();
    for f in &class.fields {
        if f.is_static || f.is_weak || f.default.is_some() {
            continue;
        }
        let nullable = f.ty.as_ref().is_some_and(|t| t.nullable);
        if nullable {
            continue;
        }
        required.insert(f.name.text.clone());
        spans.insert(f.name.text.clone(), f.span);
        let disp = f
            .origin_property
            .as_ref()
            .map(|p| p.text.clone())
            .unwrap_or_else(|| f.name.text.clone());
        display.insert(f.name.text.clone(), disp);
    }
    if required.is_empty() {
        return Vec::new();
    }

    // 2. Fields assigned by the instance `init { }` blocks — they run for every
    //    constructor (after `super`, before the constructor body), so anything
    //    they definitely assign counts as pre-assigned everywhere.
    let mut init_assigned: HashSet<String> = HashSet::new();
    for blk in &class.init_blocks {
        let mut da = Da { required: &required, exits: Vec::new() };
        let flow = da.block(blk, HashSet::new());
        init_assigned.extend(flow.assigned);
    }

    // 3. Each constructor must leave every required field assigned on every
    //    normal exit. A class with NO constructor uses the synthetic default
    //    one, which assigns nothing — so any required field not covered by an
    //    init block is unassigned.
    let mut missing: HashSet<String> = HashSet::new();
    if class.constructors.is_empty() {
        for f in &required {
            if !init_assigned.contains(f) {
                missing.insert(f.clone());
            }
        }
    } else {
        for ctor in &class.constructors {
            if ctor_delegates_this(ctor) {
                continue;
            }
            let mut da = Da { required: &required, exits: Vec::new() };
            let flow = da.block(&ctor.body, init_assigned.clone());
            if flow.reachable {
                da.exits.push(flow.assigned);
            }
            for f in &required {
                if da.exits.iter().any(|e| !e.contains(f)) {
                    missing.insert(f.clone());
                }
            }
        }
    }

    let mut out: Vec<DaViolation> = missing
        .into_iter()
        .map(|f| {
            let span = spans[&f];
            let disp = display.get(&f).cloned().unwrap_or_else(|| f.clone());
            DaViolation { field: f, display: disp, span }
        })
        .collect();
    out.sort_by(|a, b| a.field.cmp(&b.field));
    out
}

/// Run the must-assign-on-every-exit flow analysis over a function body and
/// return the names in `required` that are NOT assigned on some normal-exit
/// path. Reused by the `out`-parameter check (§M.4, E0940): an out param must be
/// assigned before every `return` and before the body ends. Same engine as the
/// field check ([`analyze_class`]); the seed is empty (an out param is never
/// pre-assigned), and a bare `name = …;` already counts as an assignment via
/// [`Da::assign_target_field`] — the backend's `*name` deref is irrelevant here.
pub(crate) fn unassigned_on_some_exit(
    body: &Block,
    required: &HashSet<String>,
) -> Vec<String> {
    if required.is_empty() {
        return Vec::new();
    }
    let mut da = Da { required, exits: Vec::new() };
    let flow = da.block(body, HashSet::new());
    if flow.reachable {
        da.exits.push(flow.assigned);
    }
    let mut missing: Vec<String> = required
        .iter()
        .filter(|n| da.exits.iter().any(|e| !e.contains(*n)))
        .cloned()
        .collect();
    missing.sort();
    missing
}

/// True when the constructor's first statement is a `this(…)` delegation — the
/// delegated-to constructor owns field initialization, so this one is exempt.
fn ctor_delegates_this(ctor: &ConstructorDecl) -> bool {
    matches!(
        ctor.body.statements.first(),
        Some(Stmt::Expr(Expr::Call(c))) if matches!(c.callee.as_ref(), Expr::This(_))
    )
}

/// Result of analyzing a block / statement: which fields are definitely
/// assigned if control completes normally past it, and whether control *can*
/// fall through (a `return`/`throw`/`break`/`continue` makes it unreachable).
struct Flow {
    assigned: HashSet<String>,
    reachable: bool,
}

struct Da<'a> {
    required: &'a HashSet<String>,
    /// Assigned-sets captured at each `return` (normal completion paths).
    exits: Vec<HashSet<String>>,
}

impl Da<'_> {
    fn block(&mut self, b: &Block, mut st: HashSet<String>) -> Flow {
        let mut reachable = true;
        for s in &b.statements {
            if !reachable {
                break; // unreachable code contributes nothing
            }
            let flow = self.stmt(s, st);
            st = flow.assigned;
            reachable = flow.reachable;
        }
        Flow { assigned: st, reachable }
    }

    fn stmt(&mut self, s: &Stmt, st: HashSet<String>) -> Flow {
        match s {
            Stmt::Assign(a) => {
                let mut st = st;
                if let Some(f) = self.assign_target_field(&a.target) {
                    st.insert(f);
                }
                Flow { assigned: st, reachable: true }
            }
            Stmt::Return(_, _) => {
                self.exits.push(st.clone());
                Flow { assigned: st, reachable: false }
            }
            // A throw aborts construction (no instance escapes); break/continue
            // divert within a loop. None impose an end-of-construction duty on
            // their path, so they just mark the linear flow unreachable.
            Stmt::Throw(..) | Stmt::Break(..) | Stmt::Continue(..) => {
                Flow { assigned: st, reachable: false }
            }
            // An explicit `this.helper(...)` call may assign fields; assume it
            // assigns all required ones rather than risk a false positive.
            Stmt::Expr(Expr::Call(c)) if call_is_this_method(c) => {
                let mut st = st;
                st.extend(self.required.iter().cloned());
                Flow { assigned: st, reachable: true }
            }
            Stmt::If(i) => self.if_stmt(i, st),
            // Loop bodies may run zero times — their assignments don't escape.
            // We still walk the body so a `return` inside it is validated.
            Stmt::While(w) => {
                self.block(&w.body, st.clone());
                Flow { assigned: st, reachable: true }
            }
            Stmt::ForEach(fe) => {
                self.block(&fe.body, st.clone());
                Flow { assigned: st, reachable: true }
            }
            Stmt::ForC(fc) => {
                self.block(&fc.body, st.clone());
                Flow { assigned: st, reachable: true }
            }
            // A `do … while` body runs at least once, so its assignments DO
            // escape (modulo an inner break, which we conservatively ignore).
            Stmt::DoWhile(d) => {
                let flow = self.block(&d.body, st.clone());
                Flow { assigned: flow.assigned, reachable: true }
            }
            Stmt::Labeled { stmt, .. } => self.stmt(stmt, st),
            Stmt::Unsafe(b) => self.block(b, st),
            Stmt::Try(t) => {
                // The try body may abort partway, and a catch only runs on
                // failure — neither contributes guaranteed assignments. Only a
                // `finally` (always runs) does. We still walk every sub-block
                // so inner `return`s are validated.
                self.block(&t.body, st.clone());
                for c in &t.catches {
                    self.block(&c.body, st.clone());
                }
                let mut out = st;
                if let Some(fin) = &t.finally {
                    out = self.block(fin, out).assigned;
                }
                Flow { assigned: out, reachable: true }
            }
            // Expr (non-this-call), VarDecl, SuperCall — no field assignment,
            // control falls through.
            _ => Flow { assigned: st, reachable: true },
        }
    }

    fn if_stmt(&mut self, i: &IfStmt, st: HashSet<String>) -> Flow {
        let then_flow = self.block(&i.then_block, st.clone());
        let Some(eb) = &i.else_branch else {
            // No `else`: the false path skips the body, so only the incoming
            // assignments are guaranteed afterward.
            return Flow { assigned: st, reachable: true };
        };
        let else_flow = match eb.as_ref() {
            ElseBranch::Block(b) => self.block(b, st.clone()),
            ElseBranch::If(inner) => self.if_stmt(inner, st.clone()),
        };
        match (then_flow.reachable, else_flow.reachable) {
            // Both arms fall through: a field is assigned only if assigned on
            // BOTH (intersection).
            (true, true) => Flow {
                assigned: then_flow
                    .assigned
                    .intersection(&else_flow.assigned)
                    .cloned()
                    .collect(),
                reachable: true,
            },
            // One arm diverges (e.g. `else { return; }`): control past the `if`
            // came from the other arm, so take its assignments.
            (true, false) => Flow { assigned: then_flow.assigned, reachable: true },
            (false, true) => Flow { assigned: else_flow.assigned, reachable: true },
            (false, false) => Flow { assigned: st, reachable: false },
        }
    }

    /// If `target` assigns a required field — `this.f = …` or a bare `f = …`
    /// naming a required field — return that field's name.
    fn assign_target_field(&self, target: &Expr) -> Option<String> {
        if let Expr::Field(fe) = target {
            if matches!(fe.object.as_ref(), Expr::This(_)) {
                return Some(fe.field.text.clone());
            }
        }
        if let Expr::Path(qn) = target {
            if qn.segments.len() == 1 && self.required.contains(qn.segments[0].text.as_str()) {
                return Some(qn.segments[0].text.clone());
            }
        }
        None
    }
}

/// True for an explicit `this.method(...)` call (the only call shape we treat
/// as possibly initializing fields — a bare `foo()` is a free function and
/// `super(...)` is a separate statement).
fn call_is_this_method(c: &CallExpr) -> bool {
    matches!(c.callee.as_ref(), Expr::Field(fe) if matches!(fe.object.as_ref(), Expr::This(_)))
}

// ============================================================================
// Local definite-assignment (§S.4.6, E0601)
// ============================================================================

/// One read of a local that reached it before any assignment did.
pub(crate) struct LocalRead {
    pub name: String,
    pub span: Span,
}

/// Every local in `body` that is READ on a path where it has not been
/// assigned, in source order, at most once per local.
///
/// A local declared with an initializer, or declared nullable (which starts as
/// null per §S.4.6), never enters the analysis. What remains is the
/// declaration with nothing to start as, and the question is whether every
/// path reaching a given read has assigned it.
pub(crate) fn locals_read_before_assignment(body: &Block) -> Vec<LocalRead> {
    let mut la = La { pending: HashSet::new(), reported: HashSet::new(), out: Vec::new() };
    la.block(body, HashSet::new());
    la.out
}

struct La {
    /// Declared without an initializer and not nullable: the locals this
    /// analysis is about. A name leaves the set when its scope ends.
    pending: HashSet<String>,
    /// Reported already -- one diagnostic per local, at its first bad read.
    reported: HashSet<String>,
    out: Vec<LocalRead>,
}

impl La {
    /// Walk a block. `st` is the set of pending locals ASSIGNED on every path
    /// reaching this point; the returned flow carries it forward.
    fn block(&mut self, b: &Block, mut st: HashSet<String>) -> Flow {
        let declared_before: HashSet<String> = self.pending.clone();
        let mut reachable = true;
        for stmt in &b.statements {
            if !reachable {
                break;
            }
            let flow = self.stmt(stmt, st);
            st = flow.assigned;
            reachable = flow.reachable;
        }
        // Locals declared inside this block leave scope with it, so a later
        // sibling block declaring the same name starts clean.
        let declared_here: Vec<String> =
            self.pending.difference(&declared_before).cloned().collect();
        for n in declared_here {
            self.pending.remove(&n);
            st.remove(&n);
        }
        Flow { assigned: st, reachable }
    }

    fn stmt(&mut self, s: &Stmt, st: HashSet<String>) -> Flow {
        let mut st = st;
        match s {
            Stmt::VarDecl(v) => {
                match &v.init {
                    Some(init) => {
                        // The initializer is evaluated before the binding
                        // exists, so a self-reference reads the OUTER name if
                        // there is one -- either way this is an ordinary read.
                        self.reads(init, &mut st);
                        st.insert(v.name.text.clone());
                    }
                    None => {
                        // A nullable local starts as null (§S.4.6); only a
                        // non-nullable one has nothing to start as.
                        let nullable = v.ty.as_ref().is_some_and(|t| t.nullable);
                        if !nullable {
                            self.pending.insert(v.name.text.clone());
                        }
                    }
                }
                Flow { assigned: st, reachable: true }
            }
            Stmt::Assign(a) => {
                // The VALUE is read before the store, and so is anything in
                // the target that is not the binding itself (`a[i] = v` reads
                // `i`, and reads `a` too -- writing through a binding needs it
                // assigned already).
                self.reads(&a.value, &mut st);
                match &a.target {
                    Expr::Path(qn) if qn.segments.len() == 1 => {
                        // A compound assignment READS the target first.
                        if a.op.is_some() {
                            self.reads(&a.target, &mut st);
                        }
                        st.insert(qn.segments[0].text.clone());
                    }
                    other => self.reads(other, &mut st),
                }
                Flow { assigned: st, reachable: true }
            }
            Stmt::Expr(e) => {
                self.reads(e, &mut st);
                Flow { assigned: st, reachable: true }
            }
            Stmt::Return(v, _) => {
                if let Some(e) = v {
                    self.reads(e, &mut st);
                }
                Flow { assigned: st, reachable: false }
            }
            Stmt::Throw(e, _) => {
                self.reads(e, &mut st);
                Flow { assigned: st, reachable: false }
            }
            Stmt::Break(..) | Stmt::Continue(..) => Flow { assigned: st, reachable: false },
            Stmt::If(i) => {
                self.reads(&i.condition, &mut st);
                let then_flow = self.block(&i.then_block, st.clone());
                let else_flow = match i.else_branch.as_deref() {
                    Some(ElseBranch::Block(b)) => Some(self.block(b, st.clone())),
                    Some(ElseBranch::If(inner)) => {
                        Some(self.stmt(&Stmt::If(inner.clone()), st.clone()))
                    }
                    None => None,
                };
                match else_flow {
                    // Only what BOTH arms assign survives the join, and an arm
                    // that cannot fall through contributes no constraint --
                    // nothing after the `if` is reachable from it.
                    Some(ef) => match (then_flow.reachable, ef.reachable) {
                        (true, true) => Flow {
                            assigned: then_flow
                                .assigned
                                .intersection(&ef.assigned)
                                .cloned()
                                .collect(),
                            reachable: true,
                        },
                        (true, false) => Flow { assigned: then_flow.assigned, reachable: true },
                        (false, true) => Flow { assigned: ef.assigned, reachable: true },
                        (false, false) => Flow { assigned: st, reachable: false },
                    },
                    // No `else`: the other path assigns nothing.
                    None => Flow { assigned: st, reachable: true },
                }
            }
            // A loop body may run zero times, so its assignments do not escape.
            // It is still walked, for reads inside it.
            Stmt::While(w) => {
                self.reads(&w.condition, &mut st);
                self.block(&w.body, st.clone());
                Flow { assigned: st, reachable: true }
            }
            Stmt::ForEach(fe) => {
                self.reads(&fe.iter, &mut st);
                let mut inner = st.clone();
                inner.insert(fe.var_name.text.clone());
                self.block(&fe.body, inner);
                Flow { assigned: st, reachable: true }
            }
            Stmt::ForC(fc) => {
                let mut inner = st.clone();
                if let Some(init) = fc.init.as_deref() {
                    inner = self.stmt(init, inner).assigned;
                }
                if let Some(c) = &fc.cond {
                    self.reads(c, &mut inner);
                }
                let body = self.block(&fc.body, inner.clone());
                if let Some(u) = fc.update.as_deref() {
                    self.stmt(u, body.assigned);
                }
                Flow { assigned: st, reachable: true }
            }
            // A `do … while` body runs at least once, so its assignments DO
            // escape -- the same carve-out the field pass makes.
            Stmt::DoWhile(d) => {
                let flow = self.block(&d.body, st.clone());
                let mut assigned = flow.assigned;
                self.reads(&d.condition, &mut assigned);
                Flow { assigned, reachable: true }
            }
            Stmt::Labeled { stmt, .. } => self.stmt(stmt, st),
            Stmt::Block(b) | Stmt::Unsafe(b) => self.block(b, st),
            Stmt::Try(t) => {
                // The body can abort partway, and a catch arm runs from an
                // unknown point inside it, so neither one's assignments are
                // guaranteed past the statement. Only `finally` runs to
                // completion on every path that leaves.
                self.block(&t.body, st.clone());
                for c in &t.catches {
                    self.block(&c.body, st.clone());
                }
                match &t.finally {
                    Some(fin) => self.block(fin, st),
                    None => Flow { assigned: st, reachable: true },
                }
            }
            // Anything else (a `super(...)` call, a declaration) assigns
            // nothing and reads nothing this analysis tracks.
            _ => Flow { assigned: st, reachable: true },
        }
    }

    /// Record a read of every pending local named in `e` that `st` has not
    /// assigned -- and ADD the ones `e` assigns through `out`.
    ///
    /// `out place` is an assignment: §M.4 requires the callee to set it on
    /// every path and `E0940` enforces that, so by the time the call's result
    /// is used the binding holds a value. Treating it as a read reported the
    /// canonical `int n; if (p.tryParse(s, out n))` as an error.
    fn reads(&mut self, e: &Expr, st: &mut HashSet<String>) {
        match e {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = &qn.segments[0].text;
                if self.pending.contains(name)
                    && !st.contains(name)
                    && self.reported.insert(name.clone())
                {
                    self.out.push(LocalRead { name: name.clone(), span: qn.span });
                }
            }
            Expr::Path(_) | Expr::Literal(_) | Expr::This(_) | Expr::Super(_) => {}
            // A lambda body runs at some other time, against bindings that may
            // well be assigned by then. Not this analysis's question.
            Expr::Lambda(_) | Expr::MethodRef(_) => {}
            Expr::Out(inner, _) => {
                if let Expr::Path(qn) = inner.as_ref() {
                    if qn.segments.len() == 1 {
                        st.insert(qn.segments[0].text.clone());
                        return;
                    }
                }
                self.reads(inner, st);
            }
            Expr::TypeOf(inner, _) => self.reads(inner, st),
            Expr::Await(inner, _) => self.reads(inner, st),
            Expr::NotNullAssert(inner, _) => self.reads(inner, st),
            Expr::ErrorProp(inner, _) => self.reads(inner, st),
            Expr::Unary(u) => self.reads(&u.operand, st),
            Expr::Cast(c) => self.reads(&c.value, st),
            Expr::SizeOf(so) => self.reads(&so.operand, st),
            Expr::TypeTest(t) => self.reads(&t.value, st),
            Expr::Field(fe) => self.reads(&fe.object, st),
            Expr::Binary(b) => {
                self.reads(&b.left, st);
                self.reads(&b.right, st);
            }
            Expr::Range(r) => {
                self.reads(&r.start, st);
                self.reads(&r.end, st);
            }
            Expr::Index(i) => {
                self.reads(&i.array, st);
                self.reads(&i.index, st);
            }
            Expr::Elvis(el) => {
                self.reads(&el.value, st);
                self.reads(&el.fallback, st);
            }
            Expr::Ternary(t) => {
                self.reads(&t.condition, st);
                self.reads(&t.then_branch, st);
                self.reads(&t.else_branch, st);
            }
            Expr::IncDec(i) => self.reads(&i.target, st),
            Expr::Call(c) => {
                self.reads(&c.callee, st);
                for a in &c.args {
                    self.reads(a, st);
                }
            }
            Expr::NewObject(n) => {
                for a in &n.args {
                    self.reads(a, st);
                }
            }
            Expr::NewArray(n) => {
                self.reads(&n.size, st);
                for i in &n.inner_sizes {
                    self.reads(i, st);
                }
            }
            Expr::NewArrayLit(n) => {
                for el in &n.elements {
                    self.reads(el, st);
                }
            }
            Expr::TupleLit(els, _) => {
                for el in els {
                    self.reads(el, st);
                }
            }
            Expr::InterpString(is) => {
                for seg in &is.segments {
                    if let juxc_ast::InterpSegment::Expr(inner) = seg {
                        self.reads(inner, st);
                    }
                }
            }
            Expr::TryExpr(_) | Expr::Switch(_) => {
                // Both carry blocks whose flow this walker does not model.
                // Silence beats a false positive here.
            }
        }
    }
}
