//! Two lowering levels: **fast** and **safe** (GAPS.md gap 34, ERRATA
//! E130).
//!
//! Jux has no borrow checker a program can fail (ERRATA E23), so every rustc
//! rejection of the emitted crate is a compiler bug. The fast level is the
//! lowering the rest of this crate describes: it moves a local on its last
//! read, binds an operand only when a cell guard would be alive across Jux
//! code, gives a class the tightest representation the selector allows, and
//! leaves a `let`'s type to rustc's inference. Each of those is a judgement,
//! and a wrong judgement is a program rustc refuses.
//!
//! The safe level makes none of them. It is the same lowering with a handful
//! of switches turned to their general answer, chosen so that the classes of
//! rustc error the judgements produce cannot arise:
//!
//! | switch | fast | safe | eliminates |
//! |---|---|---|---|
//! | local reads | move on the last read | every read of a non-`Copy` local copies, every lambda capture shares | `E0382`, `E0505`, `E0507` (use after move, move while borrowed, move out of a borrow) |
//! | operands and arguments | bound to a `let` only when a guard would be alive across Jux code | every operand and argument that reads a cell or runs Jux code is bound first, in order | `E0499`, `E0502`, `E0506`, `E0716` (two borrows at once, a borrow outliving its statement), and the run-time "already in use" |
//! | class representation | the selector's tier (inline, box, rc, ...) | `rc-refcell` for the classes the function touches (whole program: every class) | `E0382`/`E0507` on a copied value object, `E0594`/`E0596` (writing through a shared handle) |
//! | `var` declarations | type left to inference | a primitive or `String` local is declared with its checked type | `E0282`, `E0283` (type annotations needed) |
//! | `switch` arms | each arm as it lowers | each value arm of a `String` switch is made an owned `String` | `E0308` ("`match` arms have incompatible types": `String` against `&str`) |
//!
//! The level is chosen per function. The driver (`juxc_driver::self_heal`)
//! builds everything fast; when rustc refuses the crate, it maps each error to
//! the Jux function whose code it is in (the `// JUX:` markers and
//! [`function_regions`]), lowers those functions again at the safe level, and
//! rebuilds. [`LoweringPlan`] is what it asks for, installed around a lowering
//! with [`with_lowering_plan`]; the emitter reads it once, in
//! `RustEmitter::new`, and asks [`crate::RustEmitter::safe_at`] at each switch.
//!
//! A lowering outside [`with_lowering_plan`] (the backend's own tests, a
//! caller that does not heal) is all fast, exactly as before.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};

use juxc_ast::{Block, ClassDecl, CompilationUnit, TopLevelDecl};
use juxc_source::{SourceFile, Span};
use juxc_tycheck::Ty;

/// Which functions to lower at the safe level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoweringPlan {
    /// Function keys ([`FnRegion::key`]) lowered at the safe level.
    pub safe: BTreeSet<String>,
    /// Every function at the safe level (`JUX_FORCE_SAFE=1`, or the driver's
    /// last escalation).
    pub all_safe: bool,
    /// Test-only (`JUX_TEST_BREAK_FAST=<key>`): the function with this key,
    /// while it is lowered at the FAST level, opens with a statement rustc
    /// refuses as a borrow conflict (`E0502`). It lets the tests prove that
    /// the driver's retry loop finds the function and heals it. The safe
    /// level never emits it.
    pub break_fast: Option<String>,
}

impl LoweringPlan {
    /// The plan with nothing at the safe level.
    pub fn is_all_fast(&self) -> bool {
        !self.all_safe && self.safe.is_empty()
    }
}

/// One Jux function (method, constructor, operator, property, initializer
/// block) and where its source is: what an error in the emitted Rust is
/// traced back to, and what the safe level is switched on for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnRegion {
    /// `Owner.name` for a member (`Counter.bump`, `Counter.<init>` for a
    /// constructor or instance initializer, `Counter.<clinit>`, `Counter.<drop>`,
    /// `Vec2.operator Add`), the bare name for a free function. Overloads
    /// share a key, and are lowered at one level together.
    pub key: String,
    /// The class whose member this is, by the fully-qualified name the
    /// representation selector uses (`pkg.Class`), for a top-level class.
    pub owner_class: Option<String>,
    /// The declaration's span (its file index included).
    pub span: Span,
    /// The source path exactly as the `// JUX:` markers spell it.
    pub path: String,
    /// 1-based `(line, column)` of the first and last character.
    pub start: (u32, u32),
    pub end: (u32, u32),
}

impl FnRegion {
    /// Whether the `.jux` position `line:col` falls inside this function.
    pub fn contains(&self, line: u32, col: u32) -> bool {
        (line, col) >= self.start && (line, col) <= self.end
    }
}

/// The innermost function whose source holds `path:line:col`, as a `// JUX:`
/// marker names it. `None` for code that belongs to no function (a field
/// initializer, a declaration's own shape).
pub fn region_at<'r>(regions: &'r [FnRegion], path: &str, line: u32, col: u32) -> Option<&'r FnRegion> {
    regions
        .iter()
        .filter(|r| r.path == path && r.contains(line, col))
        .min_by_key(|r| r.span.end.saturating_sub(r.span.start))
}

/// Every Jux function of the program's own units (library stubs have no
/// bodies), for tracing an error back and for switching the level on.
/// `sources` is parallel to `units`, as the lowering entry points take it.
pub fn function_regions(units: &[CompilationUnit], sources: &[SourceFile]) -> Vec<FnRegion> {
    let mut out = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        if unit.is_external {
            continue;
        }
        let Some(source) = sources.get(i) else { continue };
        let pkg: String = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
            .unwrap_or_default();
        let mut c = Collector { source, path: source.path().display().to_string(), out: &mut out };
        for item in &unit.items {
            c.item(item, None, &pkg);
        }
    }
    out
}

struct Collector<'a> {
    source: &'a SourceFile,
    path: String,
    out: &'a mut Vec<FnRegion>,
}

impl Collector<'_> {
    fn push(&mut self, key: String, owner_class: Option<&str>, span: Span) {
        if span == Span::DUMMY || span.end <= span.start {
            return;
        }
        let start = self.source.line_col(span.start as usize);
        let end = self.source.line_col(span.end.saturating_sub(1) as usize);
        self.out.push(FnRegion {
            key,
            owner_class: owner_class.map(str::to_string),
            span,
            path: self.path.clone(),
            start,
            end,
        });
    }

    fn block(&mut self, key: String, owner_class: Option<&str>, b: &Block) {
        self.push(key, owner_class, b.span);
    }

    /// `outer` is the enclosing type's display name for a nested type.
    fn item(&mut self, item: &TopLevelDecl, outer: Option<&str>, pkg: &str) {
        let qualify = |name: &str| match outer {
            Some(o) => format!("{o}.{name}"),
            None => name.to_string(),
        };
        match item {
            TopLevelDecl::Function(f) => {
                if f.body.is_some() {
                    self.push(qualify(&f.name.text), None, f.span);
                }
            }
            TopLevelDecl::Class(cd) => {
                let owner = qualify(&cd.name.text);
                // The selector's key for a top-level class; a nested one is
                // keyed another way and is left to the whole-program switch.
                let fqn = outer.is_none().then(|| {
                    if pkg.is_empty() {
                        cd.name.text.clone()
                    } else {
                        format!("{pkg}.{}", cd.name.text)
                    }
                });
                self.class(cd, &owner, fqn.as_deref(), pkg);
            }
            TopLevelDecl::Record(rd) => {
                let owner = qualify(&rd.name.text);
                for m in rd.methods.iter().filter(|m| m.body.is_some()) {
                    self.push(format!("{owner}.{}", m.name.text), None, m.span);
                }
                for o in rd.operators.iter().filter(|o| o.body.is_some()) {
                    self.push(format!("{owner}.operator {:?}", o.kind), None, o.span);
                }
            }
            TopLevelDecl::Enum(ed) => {
                let owner = qualify(&ed.name.text);
                for m in ed.methods.iter().filter(|m| m.body.is_some()) {
                    self.push(format!("{owner}.{}", m.name.text), None, m.span);
                }
                for o in ed.operators.iter().filter(|o| o.body.is_some()) {
                    self.push(format!("{owner}.operator {:?}", o.kind), None, o.span);
                }
            }
            TopLevelDecl::Interface(id) => {
                let owner = qualify(&id.name.text);
                for m in id.methods.iter().filter(|m| m.body.is_some()) {
                    self.push(format!("{owner}.{}", m.name.text), None, m.span);
                }
            }
            _ => {}
        }
    }

    fn class(&mut self, cd: &ClassDecl, owner: &str, fqn: Option<&str>, pkg: &str) {
        for m in cd.methods.iter().filter(|m| m.body.is_some()) {
            self.push(format!("{owner}.{}", m.name.text), fqn, m.span);
        }
        for k in &cd.constructors {
            self.push(format!("{owner}.<init>"), fqn, k.span);
        }
        for o in cd.operators.iter().filter(|o| o.body.is_some()) {
            self.push(format!("{owner}.operator {:?}", o.kind), fqn, o.span);
        }
        for p in &cd.properties {
            self.push(format!("{owner}.{}", p.name.text), fqn, p.span);
        }
        for b in &cd.init_blocks {
            self.block(format!("{owner}.<init>"), fqn, b);
        }
        for b in &cd.static_init_blocks {
            self.block(format!("{owner}.<clinit>"), fqn, b);
        }
        for b in &cd.drop_blocks {
            self.block(format!("{owner}.<drop>"), fqn, b);
        }
        for nested in &cd.nested_types {
            self.item(nested, Some(owner), pkg);
        }
    }
}

thread_local! {
    /// The plan for the lowering running on this thread, with the program's
    /// regions it is resolved against.
    static ACTIVE: RefCell<Option<(LoweringPlan, Vec<FnRegion>)>> = const { RefCell::new(None) };
}

/// Run `lower` (any lowering entry point) under `plan`. The previous plan, if
/// any, is restored afterwards, so a nested lowering cannot leak one.
pub fn with_lowering_plan<R>(plan: &LoweringPlan, regions: &[FnRegion], lower: impl FnOnce() -> R) -> R {
    let prev = ACTIVE.with(|a| a.replace(Some((plan.clone(), regions.to_vec()))));
    struct Restore(Option<(LoweringPlan, Vec<FnRegion>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            ACTIVE.with(|a| *a.borrow_mut() = prev);
        }
    }
    let _restore = Restore(prev);
    lower()
}

/// The plan the lowering on this thread runs under; all fast outside
/// [`with_lowering_plan`].
pub(crate) fn current_plan() -> LoweringPlan {
    ACTIVE.with(|a| a.borrow().as_ref().map(|(p, _)| p.clone()).unwrap_or_default())
}

/// The level each part of the program is lowered at, resolved from the
/// active plan to spans. Held by the emitter.
#[derive(Debug, Clone, Default)]
pub(crate) struct ActiveLevel {
    all_safe: bool,
    /// Spans of the functions at the safe level.
    safe: Vec<Span>,
    /// Owners of those functions, as the selector keys classes.
    safe_owners: HashSet<String>,
    /// Spans of the function the test hook breaks.
    broken: Vec<Span>,
}

impl ActiveLevel {
    /// The level the active plan asks for; all fast when there is none.
    pub(crate) fn from_thread() -> Self {
        ACTIVE.with(|a| {
            let a = a.borrow();
            let Some((plan, regions)) = a.as_ref() else { return ActiveLevel::default() };
            let mut level = ActiveLevel { all_safe: plan.all_safe, ..ActiveLevel::default() };
            for r in regions {
                if plan.safe.contains(&r.key) {
                    level.safe.push(r.span);
                    if let Some(owner) = &r.owner_class {
                        level.safe_owners.insert(owner.clone());
                    }
                }
                if plan.break_fast.as_deref() == Some(r.key.as_str()) {
                    level.broken.push(r.span);
                }
            }
            level
        })
    }

    fn within(spans: &[Span], span: Span) -> bool {
        spans.iter().any(|r| r.file == span.file && span.start >= r.start && span.start < r.end)
    }

    /// Whether code at `span` is lowered at the safe level.
    pub(crate) fn safe_at(&self, span: Span) -> bool {
        self.all_safe || (!self.safe.is_empty() && Self::within(&self.safe, span))
    }

    /// Whether the body at `span` gets the test hook's borrow conflict.
    pub(crate) fn broken_at(&self, span: Span) -> bool {
        !self.broken.is_empty() && !self.safe_at(span) && Self::within(&self.broken, span)
    }

    /// Whether any code at all is lowered at the safe level.
    pub(crate) fn any_safe(&self) -> bool {
        self.all_safe || !self.safe.is_empty()
    }
}

/// The classes the safe level gives the most general representation,
/// `rc-refcell`, out of `eligible` (the classes the selector chooses for):
/// every one when the whole program is safe, else the owners of the safe
/// functions and every class a type inside one of them names.
pub(crate) fn forced_cell_classes(eligible: &HashSet<String>, expr_types: &HashMap<Span, Ty>) -> HashSet<String> {
    let level = ActiveLevel::from_thread();
    if !level.any_safe() {
        return HashSet::new();
    }
    if level.all_safe {
        return eligible.clone();
    }
    let mut out: HashSet<String> = level.safe_owners.iter().filter(|c| eligible.contains(*c)).cloned().collect();
    fn names(ty: &Ty, out: &mut Vec<String>) {
        match ty {
            Ty::User { name, generic_args } => {
                out.push(name.clone());
                for a in generic_args {
                    names(a, out);
                }
            }
            Ty::Nullable(inner) => names(inner, out),
            Ty::Array { element, .. } => names(element, out),
            _ => {}
        }
    }
    for (span, ty) in expr_types {
        if ActiveLevel::within(&level.safe, *span) {
            let mut found = Vec::new();
            names(ty, &mut found);
            out.extend(found.into_iter().filter(|n| eligible.contains(n)));
        }
    }
    out
}

/// The statement the test hook opens a broken function with: rustc's `E0502`
/// (a vector borrowed while a push borrows it mutably). Test-only; see
/// [`LoweringPlan::break_fast`].
pub(crate) const BROKEN_FAST_STATEMENT: &str = "{ let mut __jux_broken = vec![0]; let __jux_first = &__jux_broken[0]; \
     __jux_broken.push(1); let _ = *__jux_first; }";

impl crate::RustEmitter {
    /// Whether the code at `span` is lowered at the safe level (see the
    /// module docs for what that switches).
    pub(crate) fn safe_at(&self, span: Span) -> bool {
        self.level.safe_at(span)
    }

    /// Safe level, local reads: every read of a local in `body` copies
    /// rather than moves (`crate::lastuse`'s answer with every read counted
    /// as read again), and every lambda capture shares.
    pub(crate) fn apply_safe_last_use(&mut self, body: &Block) {
        if !self.safe_at(body.span) {
            return;
        }
        self.non_final_uses = crate::lastuse::all_local_uses(body);
        self.captures_read_again = crate::lastuse::all_captures(body, &self.current_fn_params);
    }

    /// The test hook: open a broken function's body with a statement rustc
    /// refuses. Nothing unless `JUX_TEST_BREAK_FAST` named this function and
    /// it is lowered fast.
    pub(crate) fn emit_test_break(&mut self, body: &Block) {
        if self.level.broken_at(body.span) {
            if let Some(first) = body.statements.first() {
                self.emit_source_marker(crate::stmts::stmt_span(first));
            } else {
                self.emit_source_marker(body.span);
            }
            self.w.line(BROKEN_FAST_STATEMENT);
        }
    }

    /// Safe level, `switch` arms: a value arm of a switch whose type is
    /// `String` is made an owned `String`, whatever it lowered to (`format!`
    /// gives one, a literal a `&str`). `true` when it emitted `e`.
    pub(crate) fn emit_safe_string_arm(&mut self, switch_span: Span, e: &juxc_ast::Expr) -> bool {
        // Only an arm that is itself a string: an arm that throws, or calls a
        // `never` function, has no value to convert.
        let arm_is_string = matches!(e, juxc_ast::Expr::Literal(juxc_ast::Literal::String(_)))
            || matches!(self.expr_types.get(&crate::exprs::expr_span_of(e)), Some(Ty::String));
        if !self.safe_at(switch_span) || !arm_is_string || !matches!(self.expr_types.get(&switch_span), Some(Ty::String)) {
            return false;
        }
        self.w.push_str("::std::string::String::from(");
        self.emit_expr(e);
        self.w.push(')');
        true
    }

    /// Safe level, `var` declarations: the declaration with its checked type
    /// written out, when it is a primitive or `String` (nullable or not) and
    /// the declaration has none. `None` leaves the declaration as it is.
    pub(crate) fn safe_typed_var(&self, var: &juxc_ast::VarDecl) -> Option<juxc_ast::VarDecl> {
        if var.ty.is_some() || var.is_ref || !self.safe_at(var.span) {
            return None;
        }
        let init = var.init.as_ref()?;
        // A pointer's checked type is its pointee's (the type erases the
        // `*`), so a pointer local keeps its inferred type.
        if matches!(init, juxc_ast::Expr::Literal(juxc_ast::Literal::Null)) || self.pointer_depth(init) > 0 {
            return None;
        }
        let ty = self.expr_types.get(&crate::exprs::expr_span_of(init))?;
        let (inner, nullable) = match ty {
            Ty::Nullable(inner) => (inner.as_ref(), true),
            other => (other, false),
        };
        let name = match inner {
            Ty::Primitive(p) => juxc_tycheck::ty::primitive_name(*p).to_string(),
            Ty::String => "String".to_string(),
            _ => return None,
        };
        let span = var.name.span;
        let mut typed = var.clone();
        typed.ty = Some(juxc_ast::TypeRef {
            name: juxc_ast::QualifiedName { segments: vec![juxc_ast::Ident { text: name, span }], span },
            generic_args: Vec::new(),
            nullable,
            array_shape: None,
            fn_shape: None,
            ptr_depth: 0,
            span,
        });
        Some(typed)
    }

    /// Safe level, call arguments: bind every argument first whenever one of
    /// them reads a cell or runs Jux code, not only when the fast rule sees
    /// a conflict. Not for a call to an overloaded function or method, whose
    /// member the binding form does not name, nor for one with named
    /// arguments, which has its own evaluation order: those keep the fast
    /// rule.
    pub(crate) fn safe_call_hoist(&self, call: &juxc_ast::CallExpr) -> bool {
        self.safe_at(call.span)
            && call.eval_order.is_empty()
            && !self.symbols.method_selections.contains_key(&call.span)
            && !self.symbols.function_selections.contains_key(&call.span)
            && call.args.iter().any(|a| self.operand_leaves_guard(a) || self.operand_may_run_jux_code(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(key: &str, start: (u32, u32), end: (u32, u32), len: u32) -> FnRegion {
        FnRegion {
            key: key.to_string(),
            owner_class: None,
            span: Span::new(0, len),
            path: "a.jux".to_string(),
            start,
            end,
        }
    }

    #[test]
    fn the_innermost_region_holds_a_position() {
        let regions = vec![
            region("Outer.run", (2, 5), (20, 5), 400),
            region("Outer.Inner.go", (5, 9), (8, 9), 60),
        ];
        assert_eq!(region_at(&regions, "a.jux", 6, 13).map(|r| r.key.as_str()), Some("Outer.Inner.go"));
        assert_eq!(region_at(&regions, "a.jux", 12, 1).map(|r| r.key.as_str()), Some("Outer.run"));
        assert!(region_at(&regions, "a.jux", 1, 1).is_none());
        assert!(region_at(&regions, "b.jux", 6, 13).is_none());
    }

    #[test]
    fn a_plan_outside_the_guard_is_all_fast() {
        assert!(!ActiveLevel::from_thread().any_safe());
        let plan = LoweringPlan { all_safe: true, ..LoweringPlan::default() };
        with_lowering_plan(&plan, &[], || assert!(ActiveLevel::from_thread().safe_at(Span::new(3, 4))));
        assert!(!ActiveLevel::from_thread().any_safe(), "the plan is dropped with its guard");
    }
}
