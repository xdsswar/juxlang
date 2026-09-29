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
//! Sweep C added the type and trait families, each where a judgement of the
//! backend's is the cause:
//!
//! | switch | fast | safe | eliminates |
//! |---|---|---|---|
//! | derived bounds | a relaxed parameter's `Clone + Debug` only on the members the checker's table names; a generic function's `Ord` / `Eq + Hash` only for the key uses its body was judged to make | a safe member states `Clone + Debug` for every relaxed parameter every instantiation binds to a surely-`Clone + Debug` type; a safe generic free function's own parameters take `Eq + Hash + Ord` (so `PartialEq + PartialOrd`) where every recorded call binds a type with a total order | `E0277` (trait bound not satisfied), `E0599` (no `clone` / `cmp` / `hash` on a type parameter), `E0369` (`==` on a parameter) |
//! | method names | a method call's overload suffix (`__ovK`) armed by the call and carried through emission to where the name is written | the suffix is read from the checker's pick for the call being written (a stack of calls, innermost on top) | `E0061` (wrong argument count), `E0308` (an argument of the wrong overload), `E0599` (a member that does not exist) |
//! | numeric boundaries | promotion and widening casts from the backend's own typing of each operand (its local map first) | the checker's recorded type of each operand first, so every promotion (`(a as f64) + b`) and widening (`x as i64`) the checker typed is written | `E0308` (mismatched types at a slot), `E0277` (`cannot add f64 to isize`) |
//! | type paths | a program type named relative to the module when it is in the same package or the crate root | every program type the FQN writer or a `new` names is rooted at `crate::` | `E0433` (failed to resolve), `E0412` (cannot find type), `E0425` |
//! | generic calls | a turbofish only where the program wrote type arguments | a generic free function's call is written with the checker's record of its type arguments: the written ones, and each inferred one no parameter type can carry | `E0282`, `E0283` (type annotations needed), and a written argument lost |
//! | `let` bindings | `mut` where the mutation analysis found a write or a `&mut` lend | every local is `mut` (the crate allows `unused_mut`) | `E0596` (borrow as mutable), `E0384` (assign twice) |
//!
//! `String` boundaries are the arms switch above and the literal lowering,
//! which already writes every `String` slot owned.
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
    /// Test-only (`JUX_TEST_BREAK_FAST=[<kind>:]<key>`): re-introduce a
    /// backend bug in the function with this key, so the tests can prove the
    /// driver's retry loop finds the function and the safe level rescues it.
    /// See [`BreakKind`] for what each kind breaks. Without a kind it is
    /// [`BreakKind::Borrow`].
    pub break_fast: Option<String>,
}

/// A backend bug the test hook (`JUX_TEST_BREAK_FAST=<kind>:<key>`) puts back
/// into one function. Each is a judgement of the fast lowering that a real
/// fix in gaps 30-40b corrected, reverted for that function only.
///
/// [`BreakKind::Borrow`] adds a statement to the fast lowering alone, as
/// gap 34's hook always did. Every other kind corrupts the JUDGEMENT at
/// whatever level the function is lowered, so the build only heals when a
/// safe switch really replaces that judgement with its general answer: the
/// regression harness (`bin/jux/tests/safe_mode.rs`) is a proof that the net
/// catches the bug, not that the hook is off at the safe level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreakKind {
    /// A borrow conflict opens the body (fast level only). rustc `E0502`.
    Borrow,
    /// Every read of a local is its last, so a local read twice is moved on
    /// the first read (gap 40: `w = v; w.push(7); v.len()`). rustc `E0382`.
    Move,
    /// The member states no `Clone + Debug` for its declaration's relaxed
    /// parameters (gap 2, ERRATA E118/E120: a member that copies a `T`).
    /// rustc `E0599` (no `clone` on the parameter) or `E0277`.
    Clone,
    /// A generic function's own parameter used as a map or set key gets no
    /// `Ord` / `Eq + Hash` (gap 40b, ERRATA E145). rustc `E0277`.
    KeyBound,
    /// A `String` switch's arms are emitted as each lowers (gap 35, ERRATA
    /// E131: `"ab " + e` against `"c"`). rustc `E0308`.
    Arms,
    /// Operand types are unknown to the promotion, so mixed numeric operands
    /// and a narrower value in a wider slot are left unconverted (gap 39,
    /// ERRATA E135: a member reached through an intersection bound). rustc
    /// `E0277` / `E0308`.
    Numeric,
    /// A program type is named relative to the module, without its crate-root
    /// path (gap 31 L3, gap 35: `crate::Phone`). rustc `E0433` / `E0412`.
    Path,
    /// A generic call's type arguments are left to inference (gap 36 L30,
    /// gap 40b: a call with nothing to infer them from). rustc `E0282`.
    Infer,
    /// An overloaded method call takes a stale overload pick (gap 39f,
    /// ERRATA E140: `add(x, name())` called the one-argument `add`). rustc
    /// `E0061`.
    Overload,
    /// A local that is written or lent mutably is bound without `mut` (gap
    /// 30 L8/L9, ERRATA E127: a `&mut` lend of a non-`mut` binding). rustc
    /// `E0596` / `E0384`.
    Mutability,
}

impl BreakKind {
    /// Every kind, in the order the harness covers them.
    pub const ALL: [BreakKind; 10] = [
        BreakKind::Borrow,
        BreakKind::Move,
        BreakKind::Clone,
        BreakKind::KeyBound,
        BreakKind::Arms,
        BreakKind::Numeric,
        BreakKind::Path,
        BreakKind::Infer,
        BreakKind::Overload,
        BreakKind::Mutability,
    ];

    /// The name `JUX_TEST_BREAK_FAST` spells it with.
    pub fn name(self) -> &'static str {
        match self {
            BreakKind::Borrow => "borrow",
            BreakKind::Move => "move",
            BreakKind::Clone => "clone",
            BreakKind::KeyBound => "keybound",
            BreakKind::Arms => "arms",
            BreakKind::Numeric => "numeric",
            BreakKind::Path => "path",
            BreakKind::Infer => "infer",
            BreakKind::Overload => "overload",
            BreakKind::Mutability => "mutability",
        }
    }

    /// `[<kind>:]<key>`, split. A value without a known kind before its first
    /// `:` is a key alone, broken as [`BreakKind::Borrow`].
    pub fn parse(spec: &str) -> (BreakKind, &str) {
        if let Some((kind, key)) = spec.split_once(':') {
            if let Some(k) = BreakKind::ALL.iter().find(|k| k.name() == kind) {
                return (*k, key);
            }
        }
        (BreakKind::Borrow, spec)
    }
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
    /// Spans of the function the test hook breaks, and how.
    broken: Vec<(Span, BreakKind)>,
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
                if let Some((kind, key)) = plan.break_fast.as_deref().map(BreakKind::parse) {
                    if key == r.key {
                        level.broken.push((r.span, kind));
                    }
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
        !self.safe_at(span) && self.broken_by(BreakKind::Borrow, span)
    }

    /// Whether the test hook reverts the fix `kind` names for code at `span`,
    /// at whatever level it is lowered (see [`BreakKind`]).
    pub(crate) fn broken_by(&self, kind: BreakKind, span: Span) -> bool {
        self.broken
            .iter()
            .any(|(r, k)| *k == kind && r.file == span.file && span.start >= r.start && span.start < r.end)
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

    /// Whether the code being emitted now is at the safe level: the whole
    /// program is, or the innermost function body being emitted is. For the
    /// switches asked where no expression is at hand (a type path, a `let`).
    pub(crate) fn safe_here(&self) -> bool {
        self.level.all_safe || self.body_span.is_some_and(|s| self.level.safe_at(s))
    }

    /// Whether the test hook reverts the fix `kind` names for code at `span`
    /// (see [`BreakKind`]).
    pub(crate) fn broken_by(&self, kind: BreakKind, span: Span) -> bool {
        self.level.broken_by(kind, span)
    }

    /// Whether a value of checked type `ty` surely lowers to a Rust type that
    /// is `Clone + Debug`: a primitive, a `String`, and a type the program
    /// declares (every one is `Clone` and has a `Debug` form) over arguments
    /// that are too. A parameter, a foreign type and anything unknown are not
    /// sure. What the safe level's bound switch asks of every instantiation
    /// before it states a bound, so the bound refuses no caller.
    pub(crate) fn surely_clone_debug(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Primitive(_) | Ty::String => true,
            Ty::Nullable(inner) => self.surely_clone_debug(inner),
            Ty::Array { element, .. } => self.surely_clone_debug(element),
            Ty::User { name, generic_args } => {
                let own = self.symbols.classes.get(name).is_some_and(|c| !c.is_external)
                    || self.symbols.records.contains_key(name)
                    || self.symbols.enums.contains_key(name)
                    || self.symbols.interfaces.get(name).is_some_and(|i| !i.is_external);
                own && generic_args.iter().all(|a| self.surely_clone_debug(a))
            }
            _ => false,
        }
    }

    /// Whether a value of checked type `ty` surely lowers to a Rust type with
    /// a total order and a hash (`PartialEq + Eq + Hash + PartialOrd + Ord`):
    /// an integer, `bool`, `char`, a `String`, or a nullable of one. Floats
    /// and program types are not: a class compares by identity through its
    /// own operators, not these traits.
    pub(crate) fn surely_ord_hash(ty: &Ty) -> bool {
        use juxc_tycheck::ty::Primitive as P;
        match ty {
            Ty::String => true,
            Ty::Primitive(p) => !matches!(p, P::Float | P::Double | P::F32 | P::F64),
            Ty::Nullable(inner) => Self::surely_ord_hash(inner),
            _ => false,
        }
    }

    /// Safe level, generic bounds of a free function: each of its own type
    /// parameters that every call the program makes binds to a type with a
    /// total order and a hash (`Self::surely_ord_hash`) is given the whole
    /// derived set, `Eq + Hash + Ord` (and so `PartialEq + PartialOrd`),
    /// whatever the body's uses were judged to need. Added to the key sets the
    /// header emitter reads; returns `added` with the parameters this put
    /// there, for `drop_key_bound_params` to take back out. A function with no
    /// recorded call gains nothing: there is no instantiation to check the
    /// bound against.
    pub(crate) fn add_safe_derived_bounds(&mut self, f: &juxc_ast::FnDecl, mut added: Vec<String>) -> Vec<String> {
        if f.generic_params.is_empty() || !self.safe_at(f.span) {
            return added;
        }
        let pkg = self.current_package_path();
        let fqn = if pkg.is_empty() { f.name.text.clone() } else { format!("{pkg}.{}", f.name.text) };
        let Some(calls) = self.symbols.instantiations.calls.get(&format!("fn:{fqn}")).filter(|c| !c.is_empty()) else {
            return added;
        };
        let wanted: Vec<String> = f
            .generic_params
            .iter()
            .enumerate()
            .filter(|(i, p)| !p.is_const() && calls.iter().all(|args| args.get(*i).is_some_and(Self::surely_ord_hash)))
            .map(|(_, p)| p.name.text.clone())
            .collect();
        for p in wanted {
            let fresh_hash = self.hash_key_params.insert(p.clone());
            let fresh_ord = self.ord_key_params.insert(p.clone());
            if (fresh_hash || fresh_ord) && !added.contains(&p) {
                added.push(p);
            }
        }
        added
    }

    /// Safe level, generic calls: the turbofish a call of a generic free
    /// function is written with, one entry per type parameter, from the
    /// checker's record of the call (`SymbolTable::call_type_args`), not from
    /// the call's syntax. Arguments the program wrote are written; of the
    /// inferred ones, a parameter that appears in none of the callee's
    /// parameter types has nothing for rustc to infer it from, so it is
    /// written as the type the checker bound it to, and every other one is
    /// `_`. `None` (no turbofish from this switch) when the call is not at the
    /// safe level, is not of a generic free function the checker recorded, is
    /// overloaded, or needs no written argument.
    pub(crate) fn safe_turbofish(&self, call: &juxc_ast::CallExpr) -> Option<Vec<Option<juxc_ast::TypeRef>>> {
        if !self.safe_at(call.span) || self.symbols.function_selections.contains_key(&call.span) {
            return None;
        }
        let (recorded, written) = self.symbols.call_type_args.get(&call.span)?;
        let written = *written;
        let juxc_ast::Expr::Path(qn) = call.callee.as_ref() else { return None };
        let name = qn.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
        let (_, sig) = self.lookup_function_here(&name)?;
        if sig.generic_params.len() != recorded.len() {
            return None;
        }
        let mut any = false;
        let out: Vec<Option<juxc_ast::TypeRef>> = sig
            .generic_params
            .iter()
            .zip(recorded)
            .map(|(p, ty)| {
                let from_args = !written
                    && sig.params.iter().any(|a| juxc_tycheck::clone_needs::type_ref_mentions(&a.ty, &p.name.text));
                if from_args || p.is_const() || matches!(ty, Ty::Unknown) {
                    return None;
                }
                let t = crate::types::ty_to_type_ref(ty, call.span);
                any |= t.is_some();
                t
            })
            .collect();
        any.then_some(out)
    }

    /// The overload suffix (`__ovK`) of the method name being written, taken
    /// with the call it was armed for. At the fast level it is the suffix the
    /// call armed and emission carried to here; at the safe level (sweep C)
    /// it is read again from the checker's pick for that call, so no state
    /// carried through the arguments' emission can change which overload is
    /// called (gap 39f's stale pick, ERRATA E140).
    pub(crate) fn take_method_suffix(&mut self) -> Option<String> {
        let carried = self.pending_method_suffix.take();
        match self.method_call_stack.last().copied() {
            Some(call) if self.safe_at(call) => self
                .symbols
                .method_selections
                .get(&call)
                .filter(|k| **k > 0)
                .map(|k| format!("__ov{k}")),
            _ => carried,
        }
    }

    /// [`Self::broken_by`] for the function body being emitted now.
    pub(crate) fn broken_here(&self, kind: BreakKind) -> bool {
        self.body_span.is_some_and(|s| self.level.broken_by(kind, s))
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
