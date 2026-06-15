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
    /// The unassigned field's name.
    pub field: String,
    /// Span of the field declaration (where E0600 is reported).
    pub span: Span,
}

/// Run field definite-assignment for a class and return its E0600 candidates,
/// ordered by field name for deterministic diagnostics.
pub(crate) fn analyze_class(class: &ClassDecl) -> Vec<DaViolation> {
    // 1. The fields that REQUIRE definite assignment: instance, non-`weak`,
    //    non-nullable, with no textual initializer.
    let mut required: HashSet<String> = HashSet::new();
    let mut spans: HashMap<String, Span> = HashMap::new();
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
            DaViolation { field: f, span }
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
