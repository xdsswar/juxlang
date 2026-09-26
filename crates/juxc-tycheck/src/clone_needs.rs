//! Which type parameters of a generic declaration go without `Clone + Debug`,
//! and what each member needs back (GAPS.md gap 2, ERRATA E118 and
//! E120).
//!
//! Every generic declaration used to lower each parameter with the literal
//! bound `Clone + std::fmt::Debug + 'static`, so `Cell<File>` over
//! `class Cell<T> { T value; }` did not compile: `std::fs::File` is not
//! `Clone`. The bound cannot simply go, because a body that reads a `T` by
//! value copies it. What this pass computes instead is where the bound MOVES
//! to: from the declaration to the members that need it.
//!
//! The answer lives here, in the checker, and not in the backend, because two
//! phases need exactly the same answer. The backend writes each member's
//! `where T: Clone + std::fmt::Debug` from it, and the checker reports a use of
//! a member whose type argument cannot meet it (`E0457`) before rustc would.
//! One table, read by both, is what makes "the checker accepts it" and "the
//! emitted crate compiles" the same statement.
//!
//! The rules, per declaration shape, are in [`compute`].

use std::collections::{HashMap, HashSet};

use juxc_ast::{
    Block, ClassDecl, CompilationUnit, EnumDecl, Expr, FnDecl, InterfaceDecl, OperatorDecl, OperatorKind,
    RecordDecl, ReturnType, Stmt, TopLevelDecl, TypeParam, TypeRef,
};
use juxc_source::Span;

use crate::symbol_table::SymbolTable;
use crate::ty::Ty;

/// The relaxed parameters of every declaration, and what each member needs
/// back. Built once per program by [`compute`]; stored on the
/// [`SymbolTable`] so the backend reads the very table the checker used.
#[derive(Debug, Clone, Default)]
pub struct CloneNeeds {
    /// Declaration FQN to its relaxed parameters, in declaration order. A
    /// declaration with none is absent.
    relaxed: HashMap<String, Vec<String>>,
    /// Declaration FQN to the parameters that KEEP the baseline, each with the
    /// reason, in declaration order. Only generic declarations of the program
    /// (not the core library, not a foreign stub) are listed.
    baseline: HashMap<String, Vec<(String, String)>>,
    /// A member's span to what it needs, in its owner's vocabulary.
    members: HashMap<Span, MemberNeeds>,
    /// Relaxed records and enums. A value type keeps `Debug` on a relaxed
    /// parameter: its string form prints its components, and that form is
    /// what every interface and container of it relies on.
    value_decls: HashSet<String>,
}

/// What one member (constructor, method, operator) of a relaxed declaration
/// needs from its type arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberNeeds {
    /// FQN of the declaration that declares the member.
    pub owner: String,
    /// The member's name as the program spells it (`get`, `new`,
    /// `operator +`).
    pub name: String,
    /// The relaxed parameters (of `owner`) whose `Clone + Debug` the member
    /// states in its `where` clause, in declaration order. For a method this
    /// includes what every override needs, since a call through the base
    /// reaches whichever runs.
    pub params: Vec<String>,
    /// Whether the body hands `this` on or calls another member of its class
    /// unqualified. A copy of such a member in a subclass needs every relaxed
    /// parameter of that subclass, not only the ones it maps from `owner`.
    pub touches_self: bool,
}

impl CloneNeeds {
    /// The relaxed parameters of `fqn`, in declaration order.
    pub fn relaxed_params(&self, fqn: &str) -> &[String] {
        self.relaxed.get(fqn).map_or(&[], |v| v.as_slice())
    }

    /// Whether parameter `p` of `fqn` is relaxed.
    pub fn is_relaxed(&self, fqn: &str, p: &str) -> bool {
        self.relaxed_params(fqn).iter().any(|r| r == p)
    }

    /// Whether `fqn` is a relaxed record or enum, whose relaxed parameters
    /// still carry `Debug` on its headers.
    pub fn keeps_debug(&self, fqn: &str) -> bool {
        self.value_decls.contains(fqn)
    }

    /// What the member declared at `span` needs, or `None` when it is not a
    /// member of a declaration of the program (a foreign stub's) or was
    /// synthesized (the default constructor of a class that declares none),
    /// which needs EVERY relaxed parameter. A member of a declaration with
    /// nothing relaxed is present with no parameters: whether it hands `this`
    /// on still matters to a subclass that copies it.
    pub fn member(&self, span: Span) -> Option<&MemberNeeds> {
        self.members.get(&span)
    }

    /// Why parameter `p` of `fqn` keeps `Clone + Debug`, when it does.
    pub fn baseline_reason(&self, fqn: &str, p: &str) -> Option<&str> {
        self.baseline.get(fqn)?.iter().find(|(q, _)| q == p).map(|(_, r)| r.as_str())
    }

    /// Every relaxed declaration with its relaxed parameters.
    pub fn relaxed_decls(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.relaxed.iter()
    }
}

/// Compute the table for a whole program.
///
/// **Which declarations.** Every generic class, interface, record and enum the
/// program declares, except:
///
/// - the core library (`jux.std`, `jux.meta`) and foreign stubs, whose members
///   copy their values and which are compiled for every program alike;
/// - a `struct` (a value type, copied whenever it is passed), a `@layout(c)`
///   record, and a class that is an exception or shares a hierarchy with one
///   (it is thrown and caught by value). These lower to plain values, not the
///   shared handle;
/// - a record or enum that implements an interface: its interface impl hands
///   out copies of the value.
///
/// **Which parameters.** Within such a declaration, a parameter is relaxed
/// unless one of these keeps it on the baseline (each is recorded with its
/// reason, see [`CloneNeeds::baseline_reason`]):
///
/// 1. a declared bound passes it on as a type argument (`K extends
///    Comparable<K>`);
/// 2. an instance field, record component or enum payload holds it other than
///    bare (`T`, `T?`) or forwarded bare into a relaxed parameter of another
///    declaration (`Cell<T>`); arrays, function types, `ref`/`weak` slots and
///    anything the program does not declare keep it;
/// 3. an `extends` or `implements` clause (or an interface's `extends`) passes
///    it other than bare into a relaxed parameter;
/// 4. a settable or computed property's type mentions it: the observer
///    machinery compares and copies old and new values;
/// 5. the class's `drop` body, or its `operator string`, `==`, `hash` or `<=>`
///    reads it: those back uses no member call spells (printing, a map key,
///    a `drop` at scope end), so a member `where` clause could not be checked;
/// 6. a subclass or implementer member that overrides or implements a
///    member of a parent or interface needs it while the declaration does not
///    pass it to that parent or interface: the call through the base could not
///    state the bound.
///
/// The rules are a greatest fixpoint: start with every parameter relaxed and
/// drop one whenever a rule fires, until nothing changes.
///
/// **What a member needs** is [`Walk`]'s answer, joined over overrides and
/// implementations (a call through the base reaches any of them) and over the
/// methods it calls on its own object (`m()`, `this.m()`). A constructor
/// also takes what its field initializers, initializer blocks and parent
/// constructor need; in an `extends` hierarchy one that is not
/// [`ctor_is_pure_store`] needs every relaxed parameter, since the backend
/// replays its body against the finished object with copies of its
/// parameters.
pub fn compute(units: &[CompilationUnit], symbols: &SymbolTable, expr_types: &HashMap<Span, Ty>) -> CloneNeeds {
    let mut an = Analyzer::new(units, symbols, expr_types);
    an.seed();
    for _ in 0..64 {
        let before = an.relaxed_count();
        an.shape_fixpoint();
        an.compute_member_needs();
        an.structural_rules();
        an.join_and_close();
        if an.relaxed_count() == before {
            break;
        }
    }
    an.finish()
}

// ============================================================================
// Declarations
// ============================================================================

#[derive(Clone, Copy)]
enum Kind<'a> {
    Class(&'a ClassDecl),
    Iface(&'a InterfaceDecl),
    Record(&'a RecordDecl),
    Enum(&'a EnumDecl),
}

impl<'a> Kind<'a> {
    fn generic_params(&self) -> &'a [TypeParam] {
        match self {
            Kind::Class(c) => &c.generic_params,
            Kind::Iface(i) => &i.generic_params,
            Kind::Record(r) => &r.generic_params,
            Kind::Enum(e) => &e.generic_params,
        }
    }

    fn name(&self) -> &'a str {
        match self {
            Kind::Class(c) => &c.name.text,
            Kind::Iface(i) => &i.name.text,
            Kind::Record(r) => &r.name.text,
            Kind::Enum(e) => &e.name.text,
        }
    }

    fn methods(&self) -> &'a [FnDecl] {
        match self {
            Kind::Class(c) => &c.methods,
            Kind::Iface(i) => &i.methods,
            Kind::Record(r) => &r.methods,
            Kind::Enum(e) => &e.methods,
        }
    }

    fn operators(&self) -> &'a [OperatorDecl] {
        match self {
            Kind::Class(c) => &c.operators,
            Kind::Iface(i) => &i.operators,
            Kind::Record(r) => &r.operators,
            Kind::Enum(e) => &e.operators,
        }
    }

    fn implements(&self) -> &'a [TypeRef] {
        match self {
            Kind::Class(c) => &c.implements,
            Kind::Iface(i) => &i.extends,
            Kind::Record(r) => &r.implements,
            Kind::Enum(e) => &e.implements,
        }
    }
}

struct Decl<'a> {
    pkg: String,
    unit: usize,
    kind: Kind<'a>,
}

/// The type-parameter names of a declaration, const parameters included, so
/// positions line up with a written argument list.
fn param_names(params: &[TypeParam]) -> Vec<String> {
    params.iter().map(|p| p.name.text.clone()).collect()
}

/// Whether `ty`'s written form mentions the type name `p` anywhere (its head,
/// a generic argument, a function shape, a wildcard bound). Conservative by
/// construction: it reads the derived `Debug` of the tree, where every name is
/// an `Ident` with `text: "<name>"`.
pub fn type_ref_mentions(ty: &TypeRef, p: &str) -> bool {
    format!("{ty:?}").contains(&format!("text: \"{p}\""))
}

/// Whether the checker type `ty` mentions the parameter `p`.
pub fn ty_mentions(ty: &Ty, p: &str) -> bool {
    let s = format!("{ty:?}");
    s.contains(&format!("Param(\"{p}\")")) || s.contains(&format!("name: \"{p}\""))
}

/// `ty` is the bare parameter `p` (or `p?`): a slot that holds the value and
/// asks nothing of its type.
pub fn is_bare_param(ty: &TypeRef, p: &str) -> bool {
    ty.generic_args.is_empty()
        && ty.array_shape.is_none()
        && ty.fn_shape.is_none()
        && ty.name.segments.len() == 1
        && ty.name.segments[0].text == p
}

/// The bare parameter name `ty` is, when it is one of `params`.
fn bare_param_of<'p>(ty: &TypeRef, params: &'p [String]) -> Option<&'p String> {
    params.iter().find(|p| is_bare_param(ty, p))
}

/// Replace every bare parameter named in `subst` inside `ty`.
fn substitute(ty: &TypeRef, subst: &HashMap<String, TypeRef>) -> TypeRef {
    if subst.is_empty() {
        return ty.clone();
    }
    if ty.generic_args.is_empty() && ty.fn_shape.is_none() && ty.name.segments.len() == 1 {
        if let Some(to) = subst.get(&ty.name.segments[0].text) {
            let mut out = to.clone();
            out.nullable |= ty.nullable;
            if ty.array_shape.is_some() {
                out.array_shape.clone_from(&ty.array_shape);
            }
            return out;
        }
    }
    let mut out = ty.clone();
    for arg in &mut out.generic_args {
        if let juxc_ast::GenericArg::Type(t) = arg {
            *t = substitute(t, subst);
        }
    }
    if let Some(shape) = &mut out.fn_shape {
        for p in &mut shape.params {
            *p = substitute(p, subst);
        }
        shape.return_type = substitute(&shape.return_type, subst);
    }
    out
}

/// How an operator member is named in a message.
pub fn operator_label(kind: OperatorKind) -> &'static str {
    match kind {
        OperatorKind::Eq => "operator ==",
        OperatorKind::Cmp => "operator <=>",
        OperatorKind::Lt => "operator <",
        OperatorKind::Le => "operator <=",
        OperatorKind::Gt => "operator >",
        OperatorKind::Ge => "operator >=",
        OperatorKind::Hash => "operator hash",
        OperatorKind::ToString => "operator string",
        OperatorKind::Plus => "operator +",
        OperatorKind::Minus => "operator -",
        OperatorKind::Mul => "operator *",
        OperatorKind::Div => "operator /",
        OperatorKind::Rem => "operator %",
        OperatorKind::BitAnd => "operator &",
        OperatorKind::BitOr => "operator |",
        OperatorKind::BitXor => "operator ^",
        OperatorKind::BitNot => "operator ~",
        OperatorKind::Shl => "operator <<",
        OperatorKind::Shr => "operator >>",
        OperatorKind::Index => "operator []",
        OperatorKind::IndexSet => "operator []=",
        OperatorKind::Call => "operator ()",
        _ => "operator",
    }
}

/// An operator the program never calls by name: printing, `==` in a
/// collection, a hash key, ordering in a sort. Its impl cannot be made
/// conditional on a bound no use site states.
fn operator_is_implicit(kind: OperatorKind) -> bool {
    matches!(kind, OperatorKind::ToString | OperatorKind::Eq | OperatorKind::Hash | OperatorKind::Cmp)
}

/// A method's instance-ness.
fn is_static_fn(m: &FnDecl) -> bool {
    m.modifiers.iter().any(|mo| matches!(mo, juxc_ast::FnModifier::Static))
}

/// Every block a declaration carries, for the throw/catch scan.
fn for_each_decl_block(item: &TopLevelDecl, f: &mut dyn FnMut(&Block)) {
    let fns = |ms: &[FnDecl], f: &mut dyn FnMut(&Block)| {
        for m in ms {
            if let Some(b) = &m.body {
                f(b);
            }
        }
    };
    let ops = |os: &[OperatorDecl], f: &mut dyn FnMut(&Block)| {
        for o in os {
            if let Some(b) = &o.body {
                f(b);
            }
        }
    };
    match item {
        TopLevelDecl::Function(fd) => {
            if let Some(b) = &fd.body {
                f(b);
            }
        }
        TopLevelDecl::Class(c) => {
            for ctor in &c.constructors {
                f(&ctor.body);
            }
            fns(&c.methods, f);
            ops(&c.operators, f);
            for b in c.init_blocks.iter().chain(&c.static_init_blocks).chain(&c.drop_blocks) {
                f(b);
            }
        }
        TopLevelDecl::Record(r) => {
            if let Some(c) = &r.compact_ctor {
                f(&c.body);
            }
            for ctor in &r.constructors {
                f(&ctor.body);
            }
            fns(&r.methods, f);
            ops(&r.operators, f);
        }
        TopLevelDecl::Enum(e) => {
            for ctor in &e.constructors {
                f(&ctor.body);
            }
            fns(&e.methods, f);
            ops(&e.operators, f);
        }
        TopLevelDecl::Interface(i) => {
            fns(&i.methods, f);
            ops(&i.operators, f);
        }
        _ => {}
    }
}

/// The written (possibly qualified) name of a type, as a dotted string.
fn written_name(ty: &TypeRef) -> String {
    ty.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".")
}

/// A type as the program would write it, for a reason string.
pub fn type_ref_display(ty: &TypeRef) -> String {
    let mut s = String::new();
    if let Some(shape) = &ty.fn_shape {
        let params: Vec<String> = shape.params.iter().map(type_ref_display).collect();
        s.push_str(&format!("({}) -> {}", params.join(", "), type_ref_display(&shape.return_type)));
    } else {
        s.push_str(&written_name(ty));
        if !ty.generic_args.is_empty() {
            let args: Vec<String> = ty
                .generic_args
                .iter()
                .map(|a| match a.as_type() {
                    Some(t) => type_ref_display(t),
                    None => "?".to_string(),
                })
                .collect();
            s.push_str(&format!("<{}>", args.join(", ")));
        }
    }
    if let Some(shape) = &ty.array_shape {
        for _ in &shape.dims {
            s.push_str("[]");
        }
    }
    if ty.nullable {
        s.push('?');
    }
    s
}

// ============================================================================
// The analysis
// ============================================================================

struct Analyzer<'a> {
    symbols: &'a SymbolTable,
    expr_types: &'a HashMap<Span, Ty>,
    decls: HashMap<String, Decl<'a>>,
    /// Declaration FQNs, sorted, for a deterministic walk.
    order: Vec<String>,
    /// Bare declaration name to the FQNs that carry it.
    by_bare: HashMap<String, Vec<String>>,
    /// The package of each unit, dotted.
    unit_pkgs: Vec<String>,
    /// Bare names a `throw` or `catch` anywhere names.
    thrown: HashSet<String>,
    relaxed: HashMap<String, Vec<String>>,
    reasons: HashMap<String, Vec<(String, String)>>,
    /// Own needs of every member, in its owner's vocabulary (no overrides).
    own: HashMap<Span, MemberNeeds>,
    /// Own needs joined over overrides and implementations.
    joined: HashMap<Span, MemberNeeds>,
    /// Each member's calls of methods of its own object.
    self_calls: HashMap<Span, Vec<(String, usize)>>,
}

impl<'a> Analyzer<'a> {
    fn new(units: &'a [CompilationUnit], symbols: &'a SymbolTable, expr_types: &'a HashMap<Span, Ty>) -> Self {
        let mut decls = HashMap::new();
        let mut unit_pkgs = Vec::new();
        let mut thrown = HashSet::new();
        for (i, unit) in units.iter().enumerate() {
            let pkg: String = unit
                .package
                .as_ref()
                .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
                .unwrap_or_default();
            unit_pkgs.push(pkg.clone());
            if unit.is_external {
                continue;
            }
            for item in &unit.items {
                for_each_decl_block(item, &mut |b| collect_thrown(b, &mut thrown));
                let kind = match item {
                    TopLevelDecl::Class(c) => Kind::Class(c),
                    TopLevelDecl::Interface(it) => Kind::Iface(it),
                    TopLevelDecl::Record(r) => Kind::Record(r),
                    TopLevelDecl::Enum(e) => Kind::Enum(e),
                    _ => continue,
                };
                let name = kind.name();
                let fqn = if pkg.is_empty() { name.to_string() } else { format!("{pkg}.{name}") };
                decls.insert(fqn, Decl { pkg: pkg.clone(), unit: i, kind });
            }
        }
        let mut order: Vec<String> = decls.keys().cloned().collect();
        order.sort();
        let mut by_bare: HashMap<String, Vec<String>> = HashMap::new();
        for fqn in &order {
            let bare = fqn.rsplit('.').next().unwrap_or(fqn).to_string();
            by_bare.entry(bare).or_default().push(fqn.clone());
        }
        Analyzer {
            symbols,
            expr_types,
            decls,
            order,
            by_bare,
            unit_pkgs,
            thrown,
            relaxed: HashMap::new(),
            reasons: HashMap::new(),
            own: HashMap::new(),
            joined: HashMap::new(),
            self_calls: HashMap::new(),
        }
    }

    fn relaxed_count(&self) -> usize {
        self.relaxed.values().map(Vec::len).sum()
    }

    fn is_relaxed(&self, fqn: &str, p: &str) -> bool {
        self.relaxed.get(fqn).is_some_and(|v| v.iter().any(|r| r == p))
    }

    /// Move `p` of `fqn` to the baseline, keeping the FIRST reason given.
    fn keep(&mut self, fqn: &str, p: &str, reason: String) {
        if let Some(v) = self.relaxed.get_mut(fqn) {
            v.retain(|r| r != p);
            if v.is_empty() {
                self.relaxed.remove(fqn);
            }
        }
        let list = self.reasons.entry(fqn.to_string()).or_default();
        if !list.iter().any(|(q, _)| q == p) {
            list.push((p.to_string(), reason));
        }
    }

    /// The declaration a written type names, resolved in unit `unit`'s context.
    fn resolve(&self, ty: &TypeRef, unit: usize) -> Option<String> {
        let written = written_name(ty);
        if written.is_empty() {
            return None;
        }
        if self.decls.contains_key(&written) {
            return Some(written);
        }
        if ty.name.segments.len() != 1 {
            return None;
        }
        if let Some(f) = self.symbols.units.get(unit).and_then(|c| c.unqualified.get(&written)) {
            return self.decls.contains_key(f).then(|| f.clone());
        }
        let pkg = self.unit_pkgs.get(unit).cloned().unwrap_or_default();
        let local = if pkg.is_empty() { written.clone() } else { format!("{pkg}.{written}") };
        if self.decls.contains_key(&local) {
            return Some(local);
        }
        match self.by_bare.get(&written).map(Vec::as_slice) {
            Some([one]) => Some(one.clone()),
            _ => None,
        }
    }

    /// The declaration a checker type names.
    fn resolve_ty_name(&self, name: &str) -> Option<String> {
        if self.decls.contains_key(name) {
            return Some(name.to_string());
        }
        let bare = name.rsplit('.').next().unwrap_or(name);
        match self.by_bare.get(bare).map(Vec::as_slice) {
            Some([one]) => Some(one.clone()),
            _ => None,
        }
    }

    // ---- seeding ----------------------------------------------------------

    fn seed(&mut self) {
        let order = self.order.clone();
        for fqn in &order {
            let d = &self.decls[fqn];
            let params = d.kind.generic_params();
            if params.is_empty() {
                continue;
            }
            let names: Vec<String> = params.iter().filter(|p| !p.is_const()).map(|p| p.name.text.clone()).collect();
            if names.is_empty() {
                continue;
            }
            self.relaxed.insert(fqn.clone(), names.clone());
            if let Some(reason) = self.decl_exclusion(fqn) {
                for p in &names {
                    self.keep(fqn, p, reason.clone());
                }
                continue;
            }
            // Rule 1: a bound that passes the parameter on as an argument.
            for p in &names {
                let in_bound = params
                    .iter()
                    .flat_map(|q| q.bounds.iter().map(move |b| (q, b)))
                    .find(|(_, b)| type_ref_mentions(b, p) && !is_bare_param(b, p));
                if let Some((q, b)) = in_bound {
                    self.keep(
                        fqn,
                        p,
                        format!(
                            "the bound `{} extends {}` passes it on as a type argument",
                            q.name.text,
                            type_ref_display(b)
                        ),
                    );
                }
            }
        }
    }

    /// Why a whole declaration keeps the baseline, if it does.
    fn decl_exclusion(&self, fqn: &str) -> Option<String> {
        let d = &self.decls[fqn];
        if is_core_package(&d.pkg) {
            return Some("it is part of the core library, whose members copy their values".to_string());
        }
        match d.kind {
            Kind::Class(_) => {
                // The whole `extends` component lowers together (§CR.3.5): one
                // plain-value member makes every class in it a plain value.
                for member in self.class_component(fqn) {
                    let Some(Kind::Class(c)) = self.decls.get(&member).map(|m| m.kind) else { continue };
                    let who = if member == fqn {
                        "it".to_string()
                    } else {
                        format!("it shares a hierarchy with `{}`, which", c.name.text)
                    };
                    if c.is_struct {
                        return Some(format!("{who} is a `struct`, a value type copied whenever it is passed"));
                    }
                    if self.class_is_exception(&member) {
                        return Some(format!("{who} is an exception type, thrown and caught by value"));
                    }
                    if let Some(ext) = &c.extends {
                        if self.resolve(ext, self.decls[&member].unit).is_none() {
                            return Some(format!(
                                "{who} extends `{}`, which this program does not declare",
                                type_ref_display(ext)
                            ));
                        }
                    }
                }
                None
            }
            Kind::Iface(_) => None,
            Kind::Record(r) => {
                if self.symbols.records.get(fqn).is_some_and(|s| s.is_layout_c) {
                    return Some("it is a `@layout(c)` record, a plain C value".to_string());
                }
                r.implements.first().map(|i| {
                    format!(
                        "it implements `{}`, and a record's interface impl hands out copies of it",
                        type_ref_display(i)
                    )
                })
            }
            Kind::Enum(e) => e.implements.first().map(|i| {
                format!("it implements `{}`, and an enum's interface impl hands out copies of it", type_ref_display(i))
            }),
        }
    }

    /// Every class connected to `fqn` through `extends`, `fqn` included.
    fn class_component(&self, fqn: &str) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        let mut stack = vec![fqn.to_string()];
        while let Some(cur) = stack.pop() {
            if seen.contains(&cur) {
                continue;
            }
            seen.push(cur.clone());
            if let Some((parent, _)) = self.class_parent(&cur) {
                stack.push(parent);
            }
            for other in &self.order {
                if self.class_parent(other).is_some_and(|(p, _)| p == cur) {
                    stack.push(other.clone());
                }
            }
        }
        seen
    }

    /// Whether class `fqn` is an exception: its chain reaches `Throwable`, or
    /// something throws or catches it by name.
    fn class_is_exception(&self, fqn: &str) -> bool {
        let bare = fqn.rsplit('.').next().unwrap_or(fqn);
        if self.thrown.contains(bare) {
            return true;
        }
        let mut cur = Some(fqn.to_string());
        for _ in 0..64 {
            let Some(c) = cur else { return false };
            if c.rsplit('.').next() == Some("Throwable") {
                return true;
            }
            cur = self.symbols.classes.get(&c).and_then(|s| s.extends_fqn.clone());
        }
        false
    }

    /// The parent class of class `fqn` and the args it passes, when the parent
    /// is a declaration of the program.
    fn class_parent(&self, fqn: &str) -> Option<(String, &'a TypeRef)> {
        let d = self.decls.get(fqn)?;
        let Kind::Class(c) = d.kind else { return None };
        let ext = c.extends.as_ref()?;
        let parent = self.resolve(ext, d.unit)?;
        matches!(self.decls.get(&parent).map(|p| p.kind), Some(Kind::Class(_))).then_some((parent, ext))
    }

    /// How `child`'s parameters reach `target`'s through `via` (a written
    /// `extends` / `implements` type): child param to the target params it is
    /// passed to bare.
    fn forward_map(&self, child: &str, target: &str, via: &TypeRef) -> HashMap<String, Vec<String>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        let Some(cd) = self.decls.get(child) else { return out };
        let Some(td) = self.decls.get(target) else { return out };
        let child_params = param_names(cd.kind.generic_params());
        let target_params = param_names(td.kind.generic_params());
        for (i, arg) in via.generic_args.iter().enumerate() {
            let Some(a) = arg.as_type() else { continue };
            if let (Some(q), Some(tp)) = (bare_param_of(a, &child_params), target_params.get(i)) {
                out.entry(q.clone()).or_default().push(tp.clone());
            }
        }
        out
    }

    // ---- rule 2 to 4: shapes ---------------------------------------------

    fn shape_fixpoint(&mut self) {
        loop {
            let mut changed = false;
            for fqn in self.order.clone() {
                let params = self.relaxed.get(&fqn).cloned().unwrap_or_default();
                for p in params {
                    if let Some(reason) = self.shape_reason(&fqn, &p) {
                        self.keep(&fqn, &p, reason);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn shape_reason(&self, fqn: &str, p: &str) -> Option<String> {
        let d = &self.decls[fqn];
        match d.kind {
            Kind::Class(c) => {
                for f in c.fields.iter().filter(|f| !f.is_static) {
                    let ty = crate::resolved_field_type(f);
                    if (f.is_weak || f.is_ref) && type_ref_mentions(&ty, p) {
                        let what = if f.is_ref { "ref" } else { "weak" };
                        return Some(format!("field `{}` is a `{what}` slot over it", f.name.text));
                    }
                    if let Some(why) = self.slot_reason(&ty, p, d.unit) {
                        return Some(format!("field `{}` holds it as `{}`: {why}", f.name.text, type_ref_display(&ty)));
                    }
                }
                for prop in &c.properties {
                    if prop.is_static || !type_ref_mentions(&prop.ty, p) {
                        continue;
                    }
                    let observable = prop.getter.is_some()
                        && (prop.setter.is_some()
                            || matches!(
                                prop.getter.as_ref().map(|g| &g.body),
                                Some(juxc_ast::AccessorBody::Expr(_)) | Some(juxc_ast::AccessorBody::Block(_))
                            ));
                    if observable {
                        return Some(format!(
                            "property `{}` is observable, and firing its observers compares and copies old and new values",
                            prop.name.text
                        ));
                    }
                }
                if let Some(ext) = &c.extends {
                    if let Some(why) = self.forward_reason(ext, p, d.unit) {
                        return Some(format!("it extends `{}`: {why}", type_ref_display(ext)));
                    }
                }
                for i in &c.implements {
                    if let Some(why) = self.forward_reason(i, p, d.unit) {
                        return Some(format!("it implements `{}`: {why}", type_ref_display(i)));
                    }
                }
                None
            }
            Kind::Iface(it) => {
                for e in &it.extends {
                    if let Some(why) = self.forward_reason(e, p, d.unit) {
                        return Some(format!("it extends `{}`: {why}", type_ref_display(e)));
                    }
                }
                None
            }
            Kind::Record(r) => {
                for comp in &r.components {
                    if let Some(why) = self.slot_reason(&comp.ty, p, d.unit) {
                        return Some(format!(
                            "component `{}` holds it as `{}`: {why}",
                            comp.name.text,
                            type_ref_display(&comp.ty)
                        ));
                    }
                }
                None
            }
            Kind::Enum(e) => {
                for v in &e.variants {
                    for pay in &v.payload {
                        if let Some(why) = self.slot_reason(&pay.ty, p, d.unit) {
                            return Some(format!(
                                "variant `{}` holds it as `{}`: {why}",
                                v.name.text,
                                type_ref_display(&pay.ty)
                            ));
                        }
                    }
                }
                None
            }
        }
    }

    /// Why a slot of type `ty` keeps parameter `p` on the baseline.
    fn slot_reason(&self, ty: &TypeRef, p: &str, unit: usize) -> Option<String> {
        if !type_ref_mentions(ty, p) || is_bare_param(ty, p) {
            return None;
        }
        if ty.array_shape.is_some() {
            return Some("an array copies its elements".to_string());
        }
        if ty.fn_shape.is_some() {
            return Some("a function type is not a slot this analysis follows".to_string());
        }
        self.forward_reason(ty, p, unit)
    }

    /// Why passing `p` inside the written type `ty` keeps it on the baseline:
    /// `None` when every argument that mentions it is `p` itself, handed to a
    /// relaxed parameter of a declaration of the program.
    fn forward_reason(&self, ty: &TypeRef, p: &str, unit: usize) -> Option<String> {
        if !type_ref_mentions(ty, p) {
            return None;
        }
        let head = written_name(ty);
        let Some(target) = self.resolve(ty, unit) else {
            return Some(format!("`{head}` keeps `Clone + Debug` on its parameters"));
        };
        let tparams = param_names(self.decls[&target].kind.generic_params());
        for (i, arg) in ty.generic_args.iter().enumerate() {
            match arg.as_type() {
                Some(a) if !type_ref_mentions(a, p) => {}
                Some(a) if is_bare_param(a, p) => {
                    let Some(tp) = tparams.get(i) else {
                        return Some(format!("`{head}` takes no parameter in that position"));
                    };
                    if !self.is_relaxed(&target, tp) {
                        return Some(format!("`{head}`'s `{tp}` keeps `Clone + Debug`"));
                    }
                }
                _ => return Some(format!("it is nested inside an argument of `{head}`")),
            }
        }
        None
    }

    // ---- member needs ------------------------------------------------------

    fn compute_member_needs(&mut self) {
        self.own.clear();
        self.self_calls.clear();
        // Root classes first: a subclass constructor reads its parent's.
        let mut order = self.order.clone();
        order.sort_by_key(|f| self.class_depth(f));
        // Every declaration is walked, relaxed or not: a member of one with
        // nothing relaxed needs no parameter, but whether it hands `this` on
        // still decides what a subclass's copy of it needs (rule 6).
        for fqn in order {
            let params = self.relaxed.get(&fqn).cloned().unwrap_or_default();
            let d = &self.decls[&fqn];
            match d.kind {
                Kind::Class(c) => self.class_needs(&fqn, c, &params),
                Kind::Iface(it) => {
                    let ctx = self.ctx(&fqn, &params, HashMap::new());
                    let mut out = Vec::new();
                    for m in it.methods.iter().filter(|m| !is_static_fn(m)) {
                        out.push((m.span, method_needs(&ctx, m)));
                    }
                    self.store(&fqn, out);
                }
                Kind::Record(r) => {
                    let fields: HashMap<String, TypeRef> =
                        r.components.iter().map(|c| (c.name.text.clone(), c.ty.clone())).collect();
                    let ctx = self.ctx(&fqn, &params, fields);
                    let mut out = Vec::new();
                    // The canonical constructor moves its components into the
                    // value; only the compact body, which runs first, can ask.
                    let canonical = match &r.compact_ctor {
                        Some(compact) => {
                            let own = r.components.iter().map(|c| (c.name.text.clone(), c.ty.clone())).collect();
                            let mut w = Walk::new(&ctx, own);
                            w.block(&compact.body);
                            w.result("new")
                        }
                        None => Needs::named("new"),
                    };
                    out.push((r.span, canonical));
                    for ctor in &r.constructors {
                        let own = ctor.params.iter().map(|p| (p.name.text.clone(), p.ty.clone())).collect();
                        let mut w = Walk::new(&ctx, own);
                        for p in &ctor.params {
                            w.signature_type(&p.ty, true);
                        }
                        w.block(&ctor.body);
                        out.push((ctor.span, w.result("new")));
                    }
                    for m in r.methods.iter().filter(|m| !is_static_fn(m)) {
                        out.push((m.span, method_needs(&ctx, m)));
                    }
                    for op in &r.operators {
                        out.push((op.span, operator_needs(&ctx, op)));
                    }
                    self.store(&fqn, out);
                }
                Kind::Enum(e) => {
                    let ctx = self.ctx(&fqn, &params, HashMap::new());
                    let mut out = Vec::new();
                    for m in e.methods.iter().filter(|m| !is_static_fn(m)) {
                        out.push((m.span, method_needs(&ctx, m)));
                    }
                    for op in &e.operators {
                        out.push((op.span, operator_needs(&ctx, op)));
                    }
                    self.store(&fqn, out);
                }
            }
        }
    }

    fn store(&mut self, owner: &str, list: Vec<(Span, Needs)>) {
        for (span, n) in list {
            if !n.self_calls.is_empty() {
                self.self_calls.entry(span).or_default().extend(n.self_calls.iter().cloned());
            }
            let params = self.relaxed.get(owner).cloned().unwrap_or_default();
            let entry = MemberNeeds {
                owner: owner.to_string(),
                name: n.name,
                params: params.iter().filter(|p| n.params.contains(*p)).cloned().collect(),
                touches_self: n.touches_self,
            };
            // Two members can share a span (a property's getter and setter are
            // both synthesized at the property); they then share one answer.
            match self.own.get_mut(&span) {
                Some(prev) => {
                    for p in entry.params {
                        if !prev.params.contains(&p) {
                            prev.params.push(p);
                        }
                    }
                    prev.touches_self |= entry.touches_self;
                    let order = self.relaxed.get(owner).cloned().unwrap_or_default();
                    prev.params = order.into_iter().filter(|p| prev.params.contains(p)).collect();
                }
                None => {
                    self.own.insert(span, entry);
                }
            }
        }
    }

    /// The instance slots a class's members read by name: its own fields and
    /// every inherited one, typed in the class's own vocabulary.
    fn class_fields(&self, fqn: &str) -> HashMap<String, TypeRef> {
        let mut out: HashMap<String, TypeRef> = HashMap::new();
        let mut cur = fqn.to_string();
        let mut subst: HashMap<String, TypeRef> = HashMap::new();
        for _ in 0..64 {
            let Some(Kind::Class(c)) = self.decls.get(&cur).map(|d| d.kind) else { break };
            for f in c.fields.iter().filter(|f| !f.is_static) {
                out.entry(f.name.text.clone()).or_insert_with(|| substitute(&crate::resolved_field_type(f), &subst));
            }
            let Some((parent, ext)) = self.class_parent(&cur) else { break };
            let pparams = param_names(self.decls[&parent].kind.generic_params());
            let mut next = HashMap::new();
            for (i, arg) in ext.generic_args.iter().enumerate() {
                if let (Some(a), Some(pp)) = (arg.as_type(), pparams.get(i)) {
                    next.insert(pp.clone(), substitute(a, &subst));
                }
            }
            subst = next;
            cur = parent;
        }
        out
    }

    /// The method names a class's body may call unqualified: its own, every
    /// ancestor's, and every interface default it inherits.
    fn class_method_names(&self, fqn: &str) -> HashSet<String> {
        let mut out = HashSet::new();
        let mut cur = fqn.to_string();
        for _ in 0..64 {
            let Some(d) = self.decls.get(&cur) else { break };
            for m in d.kind.methods() {
                out.insert(m.name.text.clone());
            }
            for (iface, _) in self.implemented_ifaces(&cur) {
                if let Some(id) = self.decls.get(&iface) {
                    for m in id.kind.methods() {
                        out.insert(m.name.text.clone());
                    }
                }
            }
            let Some((parent, _)) = self.class_parent(&cur) else { break };
            cur = parent;
        }
        out
    }

    fn ctx<'b>(&'b self, fqn: &str, params: &'b [String], fields: HashMap<String, TypeRef>) -> Ctx<'b, 'a> {
        let d = &self.decls[fqn];
        let methods = match d.kind {
            Kind::Class(_) => self.class_method_names(fqn),
            k => k.methods().iter().map(|m| m.name.text.clone()).collect(),
        };
        Ctx { an: self, unit: d.unit, params, fields, methods }
    }

    fn class_needs(&mut self, fqn: &str, c: &'a ClassDecl, params: &[String]) {
        let fields = self.class_fields(fqn);
        let ctx = self.ctx(fqn, params, fields);
        let mut out = Vec::new();

        // Field initializers and instance initializer blocks run in every
        // constructor.
        let mut init = Walk::new(&ctx, HashMap::new());
        for f in c.fields.iter().filter(|f| !f.is_static) {
            if let Some(d) = &f.default {
                juxc_ast::visit::for_each_node_in(d, &mut |n| init.node(n));
            }
        }
        for b in &c.init_blocks {
            init.block(b);
        }
        let init = init.result("");

        // In an `extends` hierarchy a constructor body runs again against the
        // finished object (ERRATA E21) unless it only stores its parameters
        // (`ctor_is_pure_store`); a replayed one copies every parameter, and
        // so does a constructor whose ancestors replay theirs.
        let in_hierarchy = c.extends.is_some() || self.class_is_extended(fqn);
        let chain_replays = self.ancestor_chain_replays(fqn);
        let parent_ctor_needs = self.parent_ctor_needs(fqn);
        for ctor in &c.constructors {
            let own = ctor.params.iter().map(|p| (p.name.text.clone(), p.ty.clone())).collect();
            let mut w = Walk::new(&ctx, own);
            let pure = ctor_is_pure_store(ctor, c);
            // Only the fast path builds the literal straight from the
            // parameters, and only a parameter named once is moved rather than
            // cloned for a second use.
            w.stores_move = ctor_is_fast_path(ctor, c) && (!in_hierarchy || pure);
            w.super_moves = pure;
            if !w.stores_move {
                // The general path moves a parameter into a seed it lifts.
                for f in c.fields.iter().filter(|f| !f.is_static) {
                    if let Some(p) = seed_moves_param(ctor, c, &f.name.text) {
                        w.seed_moves.insert(f.name.text.clone(), p.to_string());
                    }
                }
            }
            let mut uses: HashMap<String, usize> = HashMap::new();
            juxc_ast::visit::for_each_expr(&ctor.body, &mut |e| {
                if let Expr::Path(qn) = e {
                    if qn.segments.len() == 1 {
                        *uses.entry(qn.segments[0].text.clone()).or_default() += 1;
                    }
                }
            });
            w.moved_once = uses.into_iter().filter(|(_, k)| *k == 1).map(|(n, _)| n).collect();
            for p in &ctor.params {
                w.signature_type(&p.ty, true);
            }
            w.block(&ctor.body);
            if (in_hierarchy && !pure) || chain_replays {
                w.all();
            }
            let mut n = w.result("new");
            // A pure subclass constructor builds its parent slice with the
            // parent's constructor, whose needs reach it through `extends`.
            if c.extends.is_some() && pure && !chain_replays {
                let argc = ctor
                    .body
                    .statements
                    .iter()
                    .find_map(|st| match st {
                        Stmt::SuperCall(args, _) => Some(args.len()),
                        _ => None,
                    })
                    .unwrap_or(0);
                if let Some(extra) = parent_ctor_needs.get(&argc) {
                    n.params.extend(extra.iter().cloned());
                }
            }
            n.params.extend(init.params.iter().cloned());
            n.touches_self |= init.touches_self;
            out.push((ctor.span, n));
        }
        // A class that declares no constructor gets one that runs the field
        // initializers and builds the parent slice with the parent's
        // no-argument constructor. It is keyed by the class's own span.
        if c.constructors.is_empty() {
            let mut n = Needs::named("new");
            n.params.extend(init.params.iter().cloned());
            n.touches_self = init.touches_self;
            if let Some(extra) = parent_ctor_needs.get(&0) {
                n.params.extend(extra.iter().cloned());
            }
            out.push((c.span, n));
        }
        for m in c.methods.iter().filter(|m| !is_static_fn(m)) {
            out.push((m.span, method_needs(&ctx, m)));
        }
        for op in &c.operators {
            out.push((op.span, operator_needs(&ctx, op)));
        }
        self.store(fqn, out);
    }

    /// Whether some class of the program extends class `fqn`.
    fn class_is_extended(&self, fqn: &str) -> bool {
        self.order.iter().any(|o| self.class_parent(o).is_some_and(|(p, _)| p == fqn))
    }

    /// Whether an ancestor of class `fqn` has a constructor the backend
    /// replays against the finished object (not `ctor_is_pure_store`).
    fn ancestor_chain_replays(&self, fqn: &str) -> bool {
        let mut cur = fqn.to_string();
        for _ in 0..64 {
            let Some((parent, _)) = self.class_parent(&cur) else { return false };
            if let Some(Kind::Class(pc)) = self.decls.get(&parent).map(|d| d.kind) {
                if pc.constructors.iter().any(|k| !ctor_is_pure_store(k, pc)) {
                    return true;
                }
            }
            cur = parent;
        }
        false
    }

    /// The needs of each of the parent's constructors (by arity), in class
    /// `fqn`'s vocabulary: a parent parameter `fqn` passes a bare parameter
    /// of its own carries its need across. The parent's own needs are
    /// already computed: classes are walked root first.
    fn parent_ctor_needs(&self, fqn: &str) -> HashMap<usize, Vec<String>> {
        let mut out: HashMap<usize, Vec<String>> = HashMap::new();
        let Some((parent, ext)) = self.class_parent(fqn) else { return out };
        let Some(Kind::Class(pc)) = self.decls.get(&parent).map(|d| d.kind) else { return out };
        let forward = self.forward_map(fqn, &parent, ext);
        let mapped = |n: &MemberNeeds| -> Vec<String> {
            forward
                .iter()
                .filter(|(_, targets)| targets.iter().any(|t| n.params.contains(t)))
                .map(|(q, _)| q.clone())
                .collect()
        };
        for k in &pc.constructors {
            let needs = self.own.get(&k.span).map(&mapped).unwrap_or_default();
            out.entry(k.params.len()).or_default().extend(needs);
        }
        // The synthesized constructor of a parent that declares none.
        if pc.constructors.is_empty() {
            let needs = self.own.get(&pc.span).map(&mapped).unwrap_or_default();
            out.entry(0).or_default().extend(needs);
        }
        out
    }

    /// How many `extends` hops class `fqn` is from its root.
    fn class_depth(&self, fqn: &str) -> usize {
        let mut depth = 0;
        let mut cur = fqn.to_string();
        while let Some((parent, _)) = self.class_parent(&cur) {
            depth += 1;
            if depth > 64 {
                break;
            }
            cur = parent;
        }
        depth
    }

    // ---- rule 5: drop and the implicit operators --------------------------

    fn structural_rules(&mut self) {
        for fqn in self.order.clone() {
            let Some(params) = self.relaxed.get(&fqn).cloned() else { continue };
            let d = &self.decls[&fqn];
            let mut kept: Vec<(String, String)> = Vec::new();
            if let Kind::Class(c) = d.kind {
                if !c.drop_blocks.is_empty() {
                    let ctx = self.ctx(&fqn, &params, self.class_fields(&fqn));
                    let mut w = Walk::new(&ctx, HashMap::new());
                    for b in &c.drop_blocks {
                        w.block(b);
                    }
                    for p in w.result("drop").params {
                        kept.push((p, "its `drop` body reads it, and `drop` runs where no call names a type".to_string()));
                    }
                }
            }
            for op in d.kind.operators() {
                if !operator_is_implicit(op.kind) || op.is_deleted {
                    continue;
                }
                if let Some(n) = self.own.get(&op.span) {
                    for p in &n.params {
                        kept.push((
                            p.clone(),
                            format!(
                                "its `{}` reads it, and that operator also runs where no call names it (printing, a map key, a sort)",
                                operator_label(op.kind)
                            ),
                        ));
                    }
                }
            }
            for (p, why) in kept {
                if self.is_relaxed(&fqn, &p) {
                    self.keep(&fqn, &p, why);
                }
            }
        }
    }

    // ---- joining over overrides and implementations (rule 6) --------------

    /// The interfaces a declaration implements, transitively (its own
    /// clause, its class ancestors', each interface's `extends`), each with
    /// how the declaration's parameters reach the interface's.
    fn implemented_ifaces(&self, fqn: &str) -> Vec<(String, HashMap<String, Vec<String>>)> {
        let mut out: Vec<(String, HashMap<String, Vec<String>>)> = Vec::new();
        let Some(d) = self.decls.get(fqn) else { return out };
        let own_params = param_names(d.kind.generic_params());
        let identity: HashMap<String, Vec<String>> = own_params.iter().map(|p| (p.clone(), vec![p.clone()])).collect();
        // (holder fqn, holder -> self map) for self and each class ancestor.
        let mut holders: Vec<(String, HashMap<String, Vec<String>>)> = vec![(fqn.to_string(), identity)];
        if matches!(d.kind, Kind::Class(_)) {
            let mut cur = fqn.to_string();
            let mut map: HashMap<String, Vec<String>> = own_params.iter().map(|p| (p.clone(), vec![p.clone()])).collect();
            for _ in 0..64 {
                let Some((parent, ext)) = self.class_parent(&cur) else { break };
                let step = self.forward_map(&cur, &parent, ext);
                map = compose(&map, &step);
                holders.push((parent.clone(), map.clone()));
                cur = parent;
            }
        }
        let mut queue: Vec<(String, &TypeRef, ParamMap)> = Vec::new();
        for (holder, map) in &holders {
            let Some(hd) = self.decls.get(holder) else { continue };
            if matches!(hd.kind, Kind::Iface(_)) && holder == fqn {
                // An interface's own `extends` list is its supertype list.
                for e in hd.kind.implements() {
                    queue.push((holder.clone(), e, map.clone()));
                }
                continue;
            }
            for i in hd.kind.implements() {
                queue.push((holder.clone(), i, map.clone()));
            }
        }
        let mut guard = 0;
        while let Some((holder, via, map)) = queue.pop() {
            guard += 1;
            if guard > 256 {
                break;
            }
            let Some(hd) = self.decls.get(&holder) else { continue };
            let Some(iface) = self.resolve(via, hd.unit) else { continue };
            if !matches!(self.decls.get(&iface).map(|d| d.kind), Some(Kind::Iface(_))) {
                continue;
            }
            if out.iter().any(|(f, _)| f == &iface) {
                continue;
            }
            let step = self.forward_map(&holder, &iface, via);
            let composed = compose(&map, &step);
            let idecl = &self.decls[&iface];
            for e in idecl.kind.implements() {
                queue.push((iface.clone(), e, composed.clone()));
            }
            out.push((iface, composed));
        }
        out
    }

    /// Join over overrides and implementations, then fold each self-call's
    /// needs into its caller, until nothing grows (a caller's needs feed the
    /// member it overrides, and back).
    fn join_and_close(&mut self) {
        for _ in 0..64 {
            self.join_overrides();
            if !self.close_self_calls() {
                break;
            }
        }
    }

    /// The members a call `name(..)` with `argc` arguments on an object of
    /// `fqn` may reach, each with how its owner's parameters map back into
    /// `fqn`'s (owner param to the `fqn` params passed to it bare). `None`
    /// when nothing of the program declares it.
    fn self_call_targets(&self, fqn: &str, name: &str, argc: usize) -> Option<Vec<(Span, ParamMap)>> {
        let d = self.decls.get(fqn)?;
        let identity: HashMap<String, Vec<String>> =
            param_names(d.kind.generic_params()).into_iter().map(|p| (p.clone(), vec![p])).collect();
        let hits = |k: Kind<'a>| -> Vec<Span> {
            k.methods()
                .iter()
                .filter(|m| !is_static_fn(m) && m.name.text == name && m.params.len() == argc)
                .map(|m| m.span)
                .collect()
        };
        let own = hits(d.kind);
        if !own.is_empty() {
            return Some(own.into_iter().map(|s| (s, identity.clone())).collect());
        }
        let invert = |fwd: &HashMap<String, Vec<String>>| -> HashMap<String, Vec<String>> {
            let mut inv: HashMap<String, Vec<String>> = HashMap::new();
            for (q, targets) in fwd {
                for t in targets {
                    inv.entry(t.clone()).or_default().push(q.clone());
                }
            }
            inv
        };
        // Up the class chain.
        let mut cur = fqn.to_string();
        let mut map = identity.clone();
        for _ in 0..64 {
            let Some((parent, ext)) = self.class_parent(&cur) else { break };
            map = compose(&map, &self.forward_map(&cur, &parent, ext));
            let found = hits(self.decls[&parent].kind);
            if !found.is_empty() {
                let inv = invert(&map);
                return Some(found.into_iter().map(|s| (s, inv.clone())).collect());
            }
            cur = parent;
        }
        // An interface default.
        for (iface, fwd) in self.implemented_ifaces(fqn) {
            let found = hits(self.decls[&iface].kind);
            if !found.is_empty() {
                let inv = invert(&fwd);
                return Some(found.into_iter().map(|s| (s, inv.clone())).collect());
            }
        }
        None
    }

    /// Fold the needs of every method a member calls on its own object into
    /// the member's own needs. Returns whether anything grew.
    fn close_self_calls(&mut self) -> bool {
        let mut changed = false;
        let mut spans: Vec<Span> = self.self_calls.keys().copied().collect();
        spans.sort_by_key(|s| (s.file, s.start));
        for span in spans {
            let Some(owner) = self.own.get(&span).map(|n| n.owner.clone()) else { continue };
            let calls = self.self_calls.get(&span).cloned().unwrap_or_default();
            let mut add: Vec<String> = Vec::new();
            let mut touches = false;
            for (name, argc) in calls {
                match self.self_call_targets(&owner, &name, argc) {
                    Some(targets) => {
                        for (callee, back) in targets {
                            let Some(n) = self.joined.get(&callee) else { continue };
                            touches |= n.touches_self;
                            for p in &n.params {
                                for q in back.get(p).into_iter().flatten() {
                                    add.push(q.clone());
                                }
                            }
                        }
                    }
                    None => touches = true,
                }
            }
            let order = self.relaxed.get(&owner).cloned().unwrap_or_default();
            let Some(entry) = self.own.get_mut(&span) else { continue };
            if touches && !entry.touches_self {
                entry.touches_self = true;
                changed = true;
            }
            let wanted: Vec<String> = if entry.touches_self { order.clone() } else { add };
            for p in wanted {
                if order.contains(&p) && !entry.params.contains(&p) {
                    entry.params.push(p);
                    changed = true;
                }
            }
            if changed {
                entry.params = order.iter().filter(|p| entry.params.contains(*p)).cloned().collect();
            }
        }
        changed
    }

    fn join_overrides(&mut self) {
        self.joined = self.own.clone();
        let mut kept: Vec<(String, String, String)> = Vec::new();

        // Class hierarchies: an override's needs join the member it overrides,
        // at every ancestor that declares one.
        for fqn in self.order.clone() {
            let Some(Kind::Class(c)) = self.decls.get(&fqn).map(|d| d.kind) else { continue };
            let own_params = self.relaxed.get(&fqn).cloned().unwrap_or_default();
            let mut map: HashMap<String, Vec<String>> =
                param_names(&c.generic_params).into_iter().map(|p| (p.clone(), vec![p])).collect();
            let mut cur = fqn.clone();
            let mut overridden: HashSet<(String, usize)> = HashSet::new();
            for _ in 0..64 {
                let Some((parent, ext)) = self.class_parent(&cur) else { break };
                let step = self.forward_map(&cur, &parent, ext);
                map = compose(&map, &step);
                let Some(Kind::Class(pc)) = self.decls.get(&parent).map(|d| d.kind) else { break };
                for pm in pc.methods.iter().filter(|m| !is_static_fn(m)) {
                    let key = (pm.name.text.clone(), pm.params.len());
                    let own_m = c.methods.iter().find(|m| !is_static_fn(m) && m.name.text == pm.name.text && m.params.len() == pm.params.len());
                    match own_m {
                        Some(m) => {
                            let Some(n) = self.own.get(&m.span).cloned() else { continue };
                            let mapped = map_params(&n.params, &map);
                            if let Some(target) = self.joined.get_mut(&pm.span) {
                                add_params(target, &mapped);
                            }
                            for q in &n.params {
                                if map.get(q).map_or(true, Vec::is_empty) {
                                    kept.push((
                                        fqn.clone(),
                                        q.clone(),
                                        format!(
                                            "`{}` overrides `{}.{}` and reads it, but `{}` does not pass it to `{}`",
                                            m.name.text,
                                            pc.name.text,
                                            pm.name.text,
                                            c.name.text,
                                            pc.name.text
                                        ),
                                    ));
                                }
                            }
                            if n.touches_self {
                                for q in own_params.iter().filter(|q| map.get(*q).map_or(true, Vec::is_empty)) {
                                    kept.push((
                                        fqn.clone(),
                                        q.clone(),
                                        format!(
                                            "`{}` overrides `{}.{}` and hands `this` on, but `{}` does not pass it to `{}`",
                                            m.name.text, pc.name.text, pm.name.text, c.name.text, pc.name.text
                                        ),
                                    ));
                                }
                            }
                        }
                        // Inherited: a copy of the ancestor's body runs in
                        // this class. One that hands `this` on needs every
                        // relaxed parameter here, which a call through the
                        // ancestor cannot state for one it was not given.
                        None if !overridden.contains(&key)
                            && self.own.get(&pm.span).is_some_and(|n| n.touches_self) =>
                        {
                            for q in own_params.iter().filter(|q| map.get(*q).map_or(true, Vec::is_empty)) {
                                kept.push((
                                    fqn.clone(),
                                    q.clone(),
                                    format!(
                                        "it inherits `{}.{}`, which hands `this` on, and does not pass it to `{}`",
                                        pc.name.text, pm.name.text, pc.name.text
                                    ),
                                ));
                            }
                        }
                        None => {}
                    }
                    overridden.insert(key);
                }
                // An inherited implicit operator runs in this class too.
                for op in pc.operators.iter().filter(|o| operator_is_implicit(o.kind) && !o.is_deleted) {
                    if c.operators.iter().any(|o| o.kind == op.kind) {
                        continue;
                    }
                    if self.own.get(&op.span).is_some_and(|n| n.touches_self) {
                        for q in own_params.iter().filter(|q| map.get(*q).map_or(true, Vec::is_empty)) {
                            kept.push((
                                fqn.clone(),
                                q.clone(),
                                format!(
                                    "it inherits `{}`'s `{}`, which hands `this` on",
                                    pc.name.text,
                                    operator_label(op.kind)
                                ),
                            ));
                        }
                    }
                }
                cur = parent;
            }
        }

        // Interfaces: an implementation's needs join the interface member.
        for fqn in self.order.clone() {
            let Some(d) = self.decls.get(&fqn) else { continue };
            if matches!(d.kind, Kind::Iface(_)) {
                continue;
            }
            let own_params = self.relaxed.get(&fqn).cloned().unwrap_or_default();
            let dname = d.kind.name().to_string();
            for (iface, map) in self.implemented_ifaces(&fqn) {
                let Some(Kind::Iface(it)) = self.decls.get(&iface).map(|d| d.kind) else { continue };
                for im in it.methods.iter().filter(|m| !is_static_fn(m)) {
                    let Some(n) = self.implementation_needs(&fqn, im) else { continue };
                    let mapped = map_params(&n.params, &map);
                    if let Some(target) = self.joined.get_mut(&im.span) {
                        add_params(target, &mapped);
                    }
                    let mut unforwarded: Vec<String> =
                        n.params.iter().filter(|q| map.get(*q).map_or(true, Vec::is_empty)).cloned().collect();
                    if n.touches_self {
                        unforwarded.extend(own_params.iter().filter(|q| map.get(*q).map_or(true, Vec::is_empty)).cloned());
                    }
                    for q in unforwarded {
                        kept.push((
                            fqn.clone(),
                            q,
                            format!(
                                "its implementation of `{}.{}` reads it, but `{dname}` does not pass it to `{}`",
                                it.name.text, im.name.text, it.name.text
                            ),
                        ));
                    }
                }
            }
        }

        for (fqn, p, why) in kept {
            if self.is_relaxed(&fqn, &p) {
                self.keep(&fqn, &p, why);
            }
        }
    }

    /// What the member of `fqn` that implements interface method `im` needs,
    /// in `fqn`'s vocabulary. `None` when nothing of `fqn` implements it (the
    /// interface's default body runs).
    fn implementation_needs(&self, fqn: &str, im: &FnDecl) -> Option<MemberNeeds> {
        let d = self.decls.get(fqn)?;
        let every = || MemberNeeds {
            owner: fqn.to_string(),
            name: im.name.text.clone(),
            params: self.relaxed.get(fqn).cloned().unwrap_or_default(),
            touches_self: true,
        };
        let matches_im = |m: &FnDecl| !is_static_fn(m) && m.name.text == im.name.text && m.params.len() == im.params.len();
        if let Some(m) = d.kind.methods().iter().find(|m| matches_im(m)) {
            return Some(self.own.get(&m.span).cloned().unwrap_or_else(every));
        }
        if crate::symbol_table::operator_contract_kind(&im.name.text).is_some()
            && d.kind.operators().iter().any(|o| Some(o.kind) == crate::symbol_table::operator_contract_kind(&im.name.text))
        {
            return Some(every());
        }
        if let Kind::Class(_) = d.kind {
            // An ancestor's method, copied into this class.
            let mut cur = fqn.to_string();
            let mut map: HashMap<String, Vec<String>> =
                param_names(d.kind.generic_params()).into_iter().map(|p| (p.clone(), vec![p])).collect();
            for _ in 0..64 {
                let Some((parent, ext)) = self.class_parent(&cur) else { break };
                let step = self.forward_map(&cur, &parent, ext);
                map = compose(&map, &step);
                let pd = &self.decls[&parent];
                if let Some(m) = pd.kind.methods().iter().find(|m| matches_im(m) && m.body.is_some()) {
                    let Some(n) = self.own.get(&m.span) else { return Some(every()) };
                    // Back into this class's vocabulary.
                    let mut params = Vec::new();
                    for (q, targets) in &map {
                        if targets.iter().any(|t| n.params.contains(t)) {
                            params.push(q.clone());
                        }
                    }
                    return Some(MemberNeeds {
                        owner: fqn.to_string(),
                        name: im.name.text.clone(),
                        params,
                        touches_self: n.touches_self,
                    });
                }
                cur = parent;
            }
            // A property contract met by a field, or an operator: read it all.
            if im.body.is_none() {
                return Some(every());
            }
        }
        None
    }

    fn finish(self) -> CloneNeeds {
        let mut members = HashMap::new();
        for (span, mut n) in self.joined {
            let order = self.relaxed.get(&n.owner).cloned().unwrap_or_default();
            n.params = order.iter().filter(|p| n.params.contains(*p)).cloned().collect();
            members.insert(span, n);
        }
        let value_decls = self
            .relaxed
            .keys()
            .filter(|f| matches!(self.decls.get(*f).map(|d| d.kind), Some(Kind::Record(_) | Kind::Enum(_))))
            .cloned()
            .collect();
        let mut baseline: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for (fqn, list) in self.reasons {
            let d = &self.decls[&fqn];
            let order = param_names(d.kind.generic_params());
            let mut sorted = list;
            sorted.sort_by_key(|(p, _)| order.iter().position(|q| q == p).unwrap_or(usize::MAX));
            baseline.insert(fqn, sorted);
        }
        CloneNeeds { relaxed: self.relaxed, baseline, members, value_decls }
    }
}

/// A parameter-name map: each parameter to the parameters it is passed to
/// bare.
type ParamMap = HashMap<String, Vec<String>>;

/// `a` then `b`: a parameter's images under `a`, mapped through `b`.
fn compose(a: &HashMap<String, Vec<String>>, b: &HashMap<String, Vec<String>>) -> HashMap<String, Vec<String>> {
    a.iter()
        .map(|(k, vs)| {
            let mut out: Vec<String> = Vec::new();
            for v in vs {
                for w in b.get(v).into_iter().flatten() {
                    if !out.contains(w) {
                        out.push(w.clone());
                    }
                }
            }
            (k.clone(), out)
        })
        .collect()
}

fn map_params(params: &[String], map: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut out = Vec::new();
    for p in params {
        for t in map.get(p).into_iter().flatten() {
            if !out.contains(t) {
                out.push(t.clone());
            }
        }
    }
    out
}

fn add_params(target: &mut MemberNeeds, extra: &[String]) {
    for p in extra {
        if !target.params.contains(p) {
            target.params.push(p.clone());
        }
    }
}

/// A package whose declarations keep the baseline: the core library.
fn is_core_package(pkg: &str) -> bool {
    pkg == "jux" || pkg.starts_with("jux.") || crate::symbol_table::is_library_realm_package(pkg)
}

/// The bare names a `throw` or `catch` in `b` names.
fn collect_thrown(b: &Block, out: &mut HashSet<String>) {
    juxc_ast::visit::for_each_node(b, &mut |n| match n {
        juxc_ast::visit::Node::Stmt(Stmt::Throw(e, _)) => {
            let name = match e {
                Expr::NewObject(n) => n.class_name.segments.last().map(|s| s.text.clone()),
                Expr::Path(qn) => qn.segments.last().map(|s| s.text.clone()),
                _ => None,
            };
            out.extend(name);
        }
        juxc_ast::visit::Node::Stmt(Stmt::Try(t)) => {
            for c in &t.catches {
                for ty in std::iter::once(&c.ty).chain(&c.alt_tys) {
                    out.extend(ty.name.segments.last().map(|s| s.text.clone()));
                }
            }
        }
        _ => {}
    });
}

/// Whether a constructor does nothing but hand its parameters on: an optional
/// leading `super(args)` whose arguments are parameters or literals, then
/// stores of this class's own fields (`this.f = p;`, `f = p;`) from
/// parameters or literals, in a class with no instance initializer blocks.
///
/// Such a body has no observable effect beyond the values it stores, so it
/// makes no difference WHEN it runs. The backend relies on that: a
/// constructor of a class in an `extends` hierarchy normally runs its body
/// again against the finished object, after every field initializer of the
/// hierarchy (JUX-LANG-V1 §7.3.1, ERRATA E21), which copies its parameters
/// (they are used once to build the value and once more by the replay). A
/// pure constructor is left in the builder, where each parameter is moved
/// exactly once, so a hierarchy over a type that cannot be copied can still
/// be constructed. The checker's `where` clauses and the backend's choice of
/// path read this one predicate.
pub fn ctor_is_pure_store(ctor: &juxc_ast::ConstructorDecl, c: &ClassDecl) -> bool {
    if !c.init_blocks.is_empty() {
        return false;
    }
    let params: HashSet<&str> = ctor.params.iter().map(|p| p.name.text.as_str()).collect();
    let own_fields: HashSet<&str> = c.fields.iter().filter(|f| !f.is_static).map(|f| f.name.text.as_str()).collect();
    let plain = |e: &Expr| match e {
        Expr::Literal(_) => true,
        Expr::Path(qn) => qn.segments.len() == 1 && params.contains(qn.segments[0].text.as_str()),
        _ => false,
    };
    ctor.body.statements.iter().enumerate().all(|(i, st)| match st {
        Stmt::SuperCall(args, _) => i == 0 && args.iter().all(plain),
        Stmt::Assign(a) => {
            a.op.is_none()
                && plain(&a.value)
                && match &a.target {
                    Expr::Field(fe) => {
                        matches!(&*fe.object, Expr::This(_)) && own_fields.contains(fe.field.text.as_str())
                    }
                    Expr::Path(qn) => {
                        qn.segments.len() == 1
                            && own_fields.contains(qn.segments[0].text.as_str())
                            && !params.contains(qn.segments[0].text.as_str())
                    }
                    _ => false,
                }
        }
        _ => false,
    })
}

/// Whether a constructor that builds its object through the general
/// `__self` path (an instance initializer block, a body that does more than
/// store fields) MOVES the parameter stored by the store `this.<field> =
/// <value>;`, and which parameter that is.
///
/// The general path lifts the leading stores of fields that have no default
/// value into the struct literal ("seeds", `analysis::extract_ctor_prefix_
/// seeds`). A seed whose value is a parameter the body names exactly once,
/// stored into a field typed as a bare type parameter of the class, is moved
/// there: nothing reads the parameter again. The backend emits exactly this
/// store without a copy, and the checker exempts it from the member's
/// `where` clause. The walk here stops at the first statement that is not a
/// plain field store from an expression reading no instance state, a
/// condition the backend's seed scan implies, so a store this answers for is
/// always one the backend lifted.
pub fn seed_moves_param<'c>(ctor: &'c juxc_ast::ConstructorDecl, c: &ClassDecl, field: &str) -> Option<&'c str> {
    let params: HashSet<&str> = ctor.params.iter().map(|p| p.name.text.as_str()).collect();
    let instance: HashSet<&str> = c
        .fields
        .iter()
        .filter(|f| !f.is_static)
        .map(|f| f.name.text.as_str())
        .chain(c.properties.iter().map(|p| p.name.text.as_str()))
        .filter(|n| !params.contains(n))
        .collect();
    let class_params: HashSet<&str> = c.generic_params.iter().map(|p| p.name.text.as_str()).collect();
    let mut uses: HashMap<&str, usize> = HashMap::new();
    juxc_ast::visit::for_each_expr(&ctor.body, &mut |e| {
        if let Expr::Path(qn) = e {
            if qn.segments.len() == 1 {
                if let Some(p) = params.get(qn.segments[0].text.as_str()) {
                    *uses.entry(p).or_default() += 1;
                }
            }
        }
    });
    let mut taken: HashSet<String> = HashSet::new();
    for st in &ctor.body.statements {
        let Stmt::Assign(a) = st else { return None };
        if a.op.is_some() {
            return None;
        }
        let name = match &a.target {
            Expr::Field(fe) if matches!(&*fe.object, Expr::This(_)) => fe.field.text.clone(),
            Expr::Path(qn) if qn.segments.len() == 1 && instance.contains(qn.segments[0].text.as_str()) => {
                qn.segments[0].text.clone()
            }
            _ => return None,
        };
        let mut reads = false;
        juxc_ast::visit::for_each_expr_in(&a.value, &mut |x| match x {
            Expr::This(_) | Expr::Super(_) => reads = true,
            Expr::Path(qn) if qn.segments.len() == 1 && instance.contains(qn.segments[0].text.as_str()) => reads = true,
            _ => {}
        });
        if reads || !taken.insert(name.clone()) {
            return None;
        }
        let f = c.fields.iter().find(|f| !f.is_static && f.name.text == name)?;
        if name != field {
            continue;
        }
        let ty = f.ty.as_ref()?;
        let bare_param = !ty.nullable
            && ty.generic_args.is_empty()
            && ty.array_shape.is_none()
            && ty.fn_shape.is_none()
            && ty.name.segments.len() == 1
            && class_params.contains(ty.name.segments[0].text.as_str());
        if f.default.is_some() || !bare_param {
            return None;
        }
        let Expr::Path(qn) = &a.value else { return None };
        if qn.segments.len() != 1 {
            return None;
        }
        let p = ctor.params.iter().find(|p| p.name.text == qn.segments[0].text)?;
        if p.ty.nullable || uses.get(p.name.text.as_str()).copied() != Some(1) {
            return None;
        }
        return Some(p.name.text.as_str());
    }
    None
}

/// Whether the constructor's body is nothing but stores of this class's own
/// fields from expressions that read no instance state: the shape the backend
/// lowers to the `C_Inner { .. }` literal built straight from the parameters
/// (`analysis::extract_simple_ctor_inits` with no side effects and no `super`
/// call). This is a sufficient condition for that path, never a wider one.
fn ctor_is_fast_path(ctor: &juxc_ast::ConstructorDecl, c: &ClassDecl) -> bool {
    // A class with `init { }` blocks never takes it: they run against a
    // constructed `__self`.
    if !c.init_blocks.is_empty() {
        return false;
    }
    let params: HashSet<&str> = ctor.params.iter().map(|p| p.name.text.as_str()).collect();
    let own_fields: HashSet<&str> = c.fields.iter().filter(|f| !f.is_static).map(|f| f.name.text.as_str()).collect();
    let instance: HashSet<&str> = c
        .fields
        .iter()
        .filter(|f| !f.is_static)
        .map(|f| f.name.text.as_str())
        .chain(c.properties.iter().map(|p| p.name.text.as_str()))
        .filter(|n| !params.contains(n))
        .collect();
    let reads_instance = |e: &Expr| {
        let mut hit = false;
        juxc_ast::visit::for_each_expr_in(e, &mut |x| match x {
            Expr::This(_) | Expr::Super(_) => hit = true,
            Expr::Path(qn) if qn.segments.len() == 1 && instance.contains(qn.segments[0].text.as_str()) => hit = true,
            _ => {}
        });
        hit
    };
    ctor.body.statements.iter().all(|st| {
        let Stmt::Assign(a) = st else { return false };
        if a.op.is_some() || reads_instance(&a.value) {
            return false;
        }
        match &a.target {
            Expr::Field(fe) => matches!(&*fe.object, Expr::This(_)) && own_fields.contains(fe.field.text.as_str()),
            Expr::Path(qn) => {
                qn.segments.len() == 1
                    && own_fields.contains(qn.segments[0].text.as_str())
                    && !params.contains(qn.segments[0].text.as_str())
            }
            _ => false,
        }
    })
}

// ============================================================================
// One member's walk
// ============================================================================

/// What every walk over one declaration shares.
struct Ctx<'b, 'a> {
    an: &'b Analyzer<'a>,
    unit: usize,
    /// The declaration's relaxed parameters.
    params: &'b [String],
    /// Instance slots by name (fields, or a record's components).
    fields: HashMap<String, TypeRef>,
    /// Methods a body may call unqualified.
    methods: HashSet<String>,
}

impl Ctx<'_, '_> {
    /// Whether the written type `ty` is the handle of a relaxed class or
    /// interface over `p`: copying it is a refcount bump, whatever `p` is.
    fn relaxed_handle(&self, ty: &TypeRef, p: &str) -> bool {
        if is_bare_param(ty, p) || ty.array_shape.is_some() || ty.fn_shape.is_some() {
            return false;
        }
        let Some(target) = self.an.resolve(ty, self.unit) else { return false };
        if !matches!(self.an.decls.get(&target).map(|d| d.kind), Some(Kind::Class(_) | Kind::Iface(_))) {
            return false;
        }
        self.an.forward_reason(ty, p, self.unit).is_none()
    }

    /// The same question for a recorded checker type.
    fn ty_is_relaxed_handle(&self, ty: &Ty, p: &str) -> bool {
        match ty {
            Ty::Nullable(inner) => self.ty_is_relaxed_handle(inner, p),
            Ty::User { name, generic_args } if !generic_args.is_empty() => {
                let Some(target) = self.an.resolve_ty_name(name) else { return false };
                let d = &self.an.decls[&target];
                if !matches!(d.kind, Kind::Class(_) | Kind::Iface(_)) {
                    return false;
                }
                let declared = param_names(d.kind.generic_params());
                generic_args.iter().enumerate().all(|(i, a)| match a {
                    a if !ty_mentions(a, p) => true,
                    Ty::Param(n) if n == p => declared.get(i).is_some_and(|q| self.an.is_relaxed(&target, q)),
                    _ => false,
                })
            }
            _ => false,
        }
    }
}

/// The result of one walk.
struct Needs {
    name: String,
    params: HashSet<String>,
    touches_self: bool,
    /// Methods of the same object the body calls (`m(..)`, `this.m(..)`),
    /// by name and argument count: their needs join this member's
    /// (`Analyzer::close_self_calls`).
    self_calls: Vec<(String, usize)>,
}

impl Needs {
    fn named(name: &str) -> Self {
        Needs { name: name.to_string(), params: HashSet::new(), touches_self: false, self_calls: Vec::new() }
    }
}

fn method_needs(ctx: &Ctx<'_, '_>, m: &FnDecl) -> Needs {
    let own = m.params.iter().map(|p| (p.name.text.clone(), p.ty.clone())).collect();
    let mut w = Walk::new(ctx, own);
    for p in &m.params {
        w.signature_type(&p.ty, true);
    }
    match &m.return_type {
        ReturnType::Void => {}
        ReturnType::Type(t) | ReturnType::AsyncType(t) => w.signature_type(t, false),
    }
    // A method's own type parameter bounded by a class parameter (`<U
    // extends T>`) expands to that parameter's bounds.
    for gp in &m.generic_params {
        for b in &gp.bounds {
            w.strict_type(b);
        }
    }
    // A member with no body (abstract, or an interface's) asks only what its
    // signature asks; what its implementations need joins it afterwards
    // (`Analyzer::join_overrides`).
    if let Some(b) = &m.body {
        w.block(b);
    }
    w.result(&m.name.text)
}

fn operator_needs(ctx: &Ctx<'_, '_>, op: &OperatorDecl) -> Needs {
    let own = op.params.iter().map(|p| (p.name.text.clone(), p.ty.clone())).collect();
    let mut w = Walk::new(ctx, own);
    for p in &op.params {
        w.signature_type(&p.ty, true);
    }
    match &op.return_type {
        ReturnType::Void => {}
        ReturnType::Type(t) | ReturnType::AsyncType(t) => w.signature_type(t, false),
    }
    match &op.body {
        Some(b) => w.block(b),
        None if op.is_deleted => {}
        None => w.all(),
    }
    w.result(operator_label(op.kind))
}

/// One member's walk: the relaxed parameters it has been seen to need.
///
/// A member needs parameter `T` when `T` reaches it as a VALUE or in a type
/// the member cannot write without the bound:
///
/// - a parameter or return type mentioning `T`, other than the bare `T` / `T?`
///   of a parameter (holding a value asks nothing of its type) and a relaxed
///   handle over `T` (`Cell<T>`, see below);
/// - the checker's recorded type of any expression in the body, a slot or
///   parameter the body names, or the declared type of a local, a loop binder
///   or a lambda parameter, under the same exemptions minus the bare one
///   (reading a `T` by value is what `Clone` is for);
/// - a catch clause, a cast, a type test, a `new`, a `new T[n]` or an explicit
///   call type argument mentioning `T` at all;
/// - a method called on a receiver whose type mentions `T`: the callee may
///   state a `where` clause of its own.
///
/// A handle of a relaxed class or interface over `T` passes through like a
/// bare `T` slot does: its type is well-formed without the bound and copying
/// it is a refcount bump.
///
/// In a CONSTRUCTOR on the fast path (nothing but field stores), the
/// same-named store `this.f = f;` of a parameter the body names nowhere else
/// lowers to the `C_Inner { f }` shorthand, which moves the value. Any other
/// spelling, and a method's store, clones the parameter first.
///
/// A member needs EVERY relaxed parameter (and is marked as touching `this`)
/// when its body names `this` other than to reach a field, uses `super`, calls
/// a method of the class unqualified, or takes a method reference: the callee
/// may state a `where` clause of its own, and passing `this` on hands the
/// value to a context whose bounds this walk does not see. So does an
/// abstract member.
struct Walk<'c, 'b, 'a> {
    ctx: &'c Ctx<'b, 'a>,
    /// The member's own parameters and their declared types.
    own_params: HashMap<String, TypeRef>,
    /// Whether `this.f = p;` moves `p` rather than cloning it.
    stores_move: bool,
    /// Field to parameter: the seeds the general constructor path moves a
    /// parameter into (`seed_moves_param`).
    seed_moves: HashMap<String, String>,
    /// Whether `super(p)` moves `p` into the parent's builder (a pure
    /// constructor, `ctor_is_pure_store`) rather than running the parent's
    /// body against the finished object.
    super_moves: bool,
    /// Names the constructor body mentions exactly once.
    moved_once: HashSet<String>,
    /// Spans the plain-store exemption has cleared.
    exempt: HashSet<Span>,
    needs: HashSet<String>,
    touches_self: bool,
    self_calls: Vec<(String, usize)>,
}

impl<'c, 'b, 'a> Walk<'c, 'b, 'a> {
    fn new(ctx: &'c Ctx<'b, 'a>, own_params: HashMap<String, TypeRef>) -> Self {
        Walk {
            ctx,
            own_params,
            stores_move: false,
            seed_moves: HashMap::new(),
            super_moves: false,
            moved_once: HashSet::new(),
            exempt: HashSet::new(),
            needs: HashSet::new(),
            touches_self: false,
            self_calls: Vec::new(),
        }
    }

    fn result(self, name: &str) -> Needs {
        Needs {
            name: name.to_string(),
            params: self.needs,
            touches_self: self.touches_self,
            self_calls: self.self_calls,
        }
    }

    fn all(&mut self) {
        self.needs.extend(self.ctx.params.iter().cloned());
        self.touches_self = true;
    }

    /// A type the member spells in a position that asks for the bound
    /// whatever its shape (a cast, a `new`, a type argument).
    fn strict_type(&mut self, ty: &TypeRef) {
        for p in self.ctx.params {
            if type_ref_mentions(ty, p) {
                self.needs.insert(p.clone());
            }
        }
    }

    /// The type of a value the member reads: a bare parameter needs the
    /// bound, a relaxed handle over it does not.
    fn value_type(&mut self, ty: &TypeRef) {
        for p in self.ctx.params {
            if type_ref_mentions(ty, p) && !self.ctx.relaxed_handle(ty, p) {
                self.needs.insert(p.clone());
            }
        }
    }

    /// A signature type: a bare parameter in a parameter slot is free, and so
    /// is a relaxed handle anywhere.
    fn signature_type(&mut self, ty: &TypeRef, param_slot: bool) {
        for p in self.ctx.params {
            if param_slot && is_bare_param(ty, p) {
                continue;
            }
            if type_ref_mentions(ty, p) && !self.ctx.relaxed_handle(ty, p) {
                self.needs.insert(p.clone());
            }
        }
    }

    fn block(&mut self, b: &Block) {
        juxc_ast::visit::for_each_node(b, &mut |n| self.node(n));
    }

    /// Whether a store target is `this.f` or a bare `f` no parameter shadows.
    fn store_target_field(&self, target: &Expr) -> bool {
        match target {
            Expr::Field(fe) => matches!(&*fe.object, Expr::This(_)) && self.ctx.fields.contains_key(&fe.field.text),
            Expr::Path(qn) => {
                qn.segments.len() == 1
                    && self.ctx.fields.contains_key(&qn.segments[0].text)
                    && !self.own_params.contains_key(&qn.segments[0].text)
            }
            _ => false,
        }
    }

    /// The declared type of a name the body reads bare: a parameter of the
    /// member, else an instance slot.
    fn named_type(&self, name: &str) -> Option<TypeRef> {
        match self.own_params.get(name) {
            Some(t) => Some(t.clone()),
            None => self.ctx.fields.get(name).cloned(),
        }
    }

    /// Whether the value of `e` has a type mentioning `p`, by its recorded
    /// type or, for a name or `this.f`, its declared one.
    fn value_mentions(&self, e: &Expr, p: &str) -> bool {
        let recorded =
            self.ctx.an.expr_types.get(&crate::check::expr_span_pub(e)).is_some_and(|t| ty_mentions(t, p));
        let declared = match e {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                self.named_type(&qn.segments[0].text).is_some_and(|t| type_ref_mentions(&t, p))
            }
            Expr::Field(fe) => self.ctx.fields.get(&fe.field.text).is_some_and(|t| type_ref_mentions(t, p)),
            Expr::This(_) => true,
            _ => false,
        };
        recorded || declared
    }

    fn node(&mut self, n: juxc_ast::visit::Node<'_>) {
        match n {
            juxc_ast::visit::Node::Stmt(st) => self.stmt(st),
            juxc_ast::visit::Node::Expr(e) => self.expr(e),
        }
    }

    fn stmt(&mut self, st: &Stmt) {
        match st {
            Stmt::Assign(a) if a.op.is_none() => {
                // Only the same-named store (`this.f = f;`) lowers to the
                // `C_Inner { f }` shorthand; any other spelling is emitted
                // through the value path, which clones a parameter.
                let target_name = match &a.target {
                    Expr::Field(fe) => Some(fe.field.text.as_str()),
                    Expr::Path(qn) if qn.segments.len() == 1 => Some(qn.segments[0].text.as_str()),
                    _ => None,
                };
                let value_is_param = matches!(&a.value, Expr::Path(qn)
                    if qn.segments.len() == 1
                        && target_name == Some(qn.segments[0].text.as_str())
                        && self.own_params.contains_key(&qn.segments[0].text)
                        && self.moved_once.contains(&qn.segments[0].text));
                let seeded = target_name.is_some_and(|f| {
                    matches!(&a.value, Expr::Path(qn)
                        if qn.segments.len() == 1 && self.seed_moves.get(f) == Some(&qn.segments[0].text))
                });
                if (self.stores_move && value_is_param || seeded) && self.store_target_field(&a.target) {
                    self.exempt.insert(crate::check::expr_span_pub(&a.target));
                    self.exempt.insert(crate::check::expr_span_pub(&a.value));
                    if let Expr::Field(fe) = &a.target {
                        self.exempt.insert(crate::check::expr_span_pub(&fe.object));
                    }
                }
            }
            Stmt::VarDecl(v) => {
                if let Some(t) = &v.ty {
                    self.value_type(t);
                }
            }
            Stmt::ForEach(f) => {
                if let Some(t) = &f.var_type {
                    self.value_type(t);
                }
            }
            Stmt::Try(t) => {
                for c in &t.catches {
                    self.strict_type(&c.ty);
                    for alt in &c.alt_tys {
                        self.strict_type(alt);
                    }
                }
            }
            Stmt::SuperCall(args, _) => {
                if self.super_moves {
                    for a in args {
                        if let Expr::Path(qn) = a {
                            if qn.segments.len() == 1 && self.moved_once.contains(&qn.segments[0].text) {
                                self.exempt.insert(crate::check::expr_span_pub(a));
                            }
                        }
                    }
                } else {
                    self.all();
                }
            }
            _ => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        let span = crate::check::expr_span_pub(e);
        if self.exempt.contains(&span) {
            return;
        }
        if let Some(ty) = self.ctx.an.expr_types.get(&span) {
            for p in self.ctx.params {
                if ty_mentions(ty, p) && !self.ctx.ty_is_relaxed_handle(ty, p) {
                    self.needs.insert(p.clone());
                }
            }
        }
        match e {
            Expr::This(_) | Expr::Super(_) | Expr::MethodRef(_) => self.all(),
            Expr::Field(fe) => {
                if matches!(&*fe.object, Expr::This(_)) && self.ctx.fields.contains_key(&fe.field.text) {
                    self.exempt.insert(crate::check::expr_span_pub(&fe.object));
                }
                if let Some(t) = self.ctx.fields.get(&fe.field.text).cloned() {
                    self.value_type(&t);
                }
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                if let Some(t) = self.named_type(&qn.segments[0].text) {
                    self.value_type(&t);
                }
            }
            Expr::Call(c) => {
                for t in &c.explicit_generic_args {
                    self.strict_type(t);
                }
                match &*c.callee {
                    // A call of a method of the same object needs what that
                    // method needs, whichever override runs; resolved once
                    // every member's own needs are known.
                    Expr::Path(qn)
                        if qn.segments.len() == 1
                            && self.ctx.methods.contains(&qn.segments[0].text)
                            && !self.own_params.contains_key(&qn.segments[0].text) =>
                    {
                        self.self_calls.push((qn.segments[0].text.clone(), c.args.len()));
                    }
                    Expr::Field(fe) if matches!(&*fe.object, Expr::This(_)) && self.ctx.methods.contains(&fe.field.text) => {
                        self.self_calls.push((fe.field.text.clone(), c.args.len()));
                        self.exempt.insert(fe.span);
                        self.exempt.insert(crate::check::expr_span_pub(&fe.object));
                    }
                    // A method on a receiver over `T` may state a `where`
                    // clause of its own.
                    Expr::Field(fe) => {
                        for p in self.ctx.params {
                            if self.value_mentions(&fe.object, p) {
                                self.needs.insert(p.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Expr::Cast(c) => self.strict_type(&c.ty),
            Expr::TypeTest(t) => self.strict_type(&t.ty),
            Expr::NewArray(n) => self.strict_type(&n.element_type),
            Expr::NewObject(n) => {
                for t in &n.generic_args {
                    self.strict_type(t);
                }
                if let Some(head) = n.class_name.segments.last() {
                    if self.ctx.params.contains(&head.text) {
                        self.needs.insert(head.text.clone());
                    }
                }
            }
            Expr::Lambda(l) => {
                for p in &l.params {
                    if let Some(t) = &p.ty {
                        self.value_type(t);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use juxc_lex::lex;
    use juxc_parse::parse;
    use juxc_source::SourceFile;

    /// Typecheck one unit and return its table with the diagnostics.
    fn table(src: &str) -> (CloneNeeds, Vec<juxc_diagnostics::Diagnostic>) {
        let sf = SourceFile::new("test.jux", src);
        let lex_result = lex(&sf);
        assert!(lex_result.diagnostics.is_empty(), "lex errors: {:?}", lex_result.diagnostics);
        let parse_result = parse(&lex_result.tokens);
        assert!(parse_result.diagnostics.is_empty(), "parse errors: {:?}", parse_result.diagnostics);
        let result = crate::typecheck_workspace(&[parse_result.ast]);
        (result.symbols.clone_needs, result.diagnostics)
    }

    fn needs_named<'t>(t: &'t CloneNeeds, owner: &str, name: &str) -> &'t MemberNeeds {
        t.members
            .values()
            .find(|n| n.owner == owner && n.name == name)
            .unwrap_or_else(|| panic!("no member {owner}.{name}: {t:#?}"))
    }

    /// Every shape is relaxed when it only stores its parameter, and each
    /// member names exactly what it copies.
    #[test]
    fn every_shape_relaxes_a_stored_parameter() {
        let (t, diags) = table(
            r#"
            class Holder<T> {
                public T item;
                public Holder(T item) { this.item = item; }
                public T get() { return item; }
                public String kind() { return "holder"; }
            }
            class Counted<T> extends Holder<T> {
                public Counted(T item) { super(item); }
            }
            interface Store<T> { T take(); int size(); }
            class One<T> implements Store<T> {
                private T v;
                public One(T v) { this.v = v; }
                public T take() { return v; }
                public int size() { return 1; }
            }
            record Named<T>(String name, T value) {}
            enum Slot<T> { Empty, Full(T value) }
            public void main() {}
            "#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        for fqn in ["Holder", "Counted", "Store", "One", "Named", "Slot"] {
            assert_eq!(t.relaxed_params(fqn), ["T".to_string()], "{fqn}: {t:#?}");
        }
        assert!(t.keeps_debug("Named") && t.keeps_debug("Slot") && !t.keeps_debug("Holder"));
        assert_eq!(needs_named(&t, "Holder", "get").params, ["T".to_string()]);
        assert!(needs_named(&t, "Holder", "kind").params.is_empty());
        assert_eq!(needs_named(&t, "Store", "take").params, ["T".to_string()]);
        assert!(needs_named(&t, "Store", "size").params.is_empty());
    }

    /// A virtual member needs what every override needs; a call of a method
    /// on the same object needs what that method needs.
    #[test]
    fn overrides_and_self_calls_join() {
        let (t, _) = table(
            r#"
            class Base<T> {
                public T item;
                public Base(T item) { this.item = item; }
                public String show() { return "base"; }
                public String twice() { return show() + show(); }
            }
            class Child<T> extends Base<T> {
                public Child(T item) { super(item); }
                @Override
                public String show() { return $"${item}"; }
            }
            public void main() {}
            "#,
        );
        assert_eq!(needs_named(&t, "Base", "show").params, ["T".to_string()], "{t:#?}");
        assert_eq!(needs_named(&t, "Base", "twice").params, ["T".to_string()], "{t:#?}");
        assert!(!needs_named(&t, "Base", "twice").touches_self);
    }

    /// Each rule that keeps a parameter records why.
    #[test]
    fn a_kept_parameter_says_why() {
        let (t, _) = table(
            r#"
            class Arr<T> { private T[] xs; public Arr(T[] xs) { this.xs = xs; } }
            class Obs<T> { public T Val { get; set; } }
            class Drops<T> { private T r; public Drops(T r) { this.r = r; } drop { print(r); } }
            class Shows<T> {
                private T v;
                public Shows(T v) { this.v = v; }
                public String operator string() { return $"${v}"; }
            }
            class Bound<K extends Comparable<K>> { private K k; public Bound(K k) { this.k = k; } }
            public void main() {}
            "#,
        );
        let why = |fqn: &str, p: &str| t.baseline_reason(fqn, p).unwrap_or_default().to_string();
        assert!(why("Arr", "T").contains("an array copies its elements"), "{}", why("Arr", "T"));
        assert!(why("Obs", "T").contains("property `Val` is observable"), "{}", why("Obs", "T"));
        assert!(why("Drops", "T").contains("`drop` body"), "{}", why("Drops", "T"));
        assert!(why("Shows", "T").contains("operator string"), "{}", why("Shows", "T"));
        assert!(why("Bound", "K").contains("passes it on as a type argument"), "{}", why("Bound", "K"));
    }

    /// A child's parameter that an override needs but that the child does
    /// not pass to its parent keeps the bound: the call through the parent
    /// cannot state it.
    #[test]
    fn an_unforwarded_parameter_an_override_needs_keeps_the_bound() {
        let (t, _) = table(
            r#"
            class Base<T> {
                public T item;
                public Base(T item) { this.item = item; }
                public String show() { return "base"; }
            }
            class Extra<T, U> extends Base<T> {
                private U u;
                public Extra(T item, U u) {
                    super(item);
                    this.u = u;
                }
                @Override
                public String show() { return $"${u}"; }
            }
            public void main() {}
            "#,
        );
        assert_eq!(t.relaxed_params("Extra"), ["T".to_string()], "{t:#?}");
        assert!(t.baseline_reason("Extra", "U").unwrap_or_default().contains("does not pass it to `Base`"));
    }

    /// The pure constructor predicate: parameters handed on, nothing else.
    #[test]
    fn pure_store_constructors() {
        let src = r#"
            class Base<T> {
                public T item;
                public int n;
                public Base(T item) { this.item = item; this.n = 0; }
                public Base(T item, int k) { this.item = item; this.n = k + 1; }
            }
            public void main() {}
            "#;
        let sf = SourceFile::new("test.jux", src);
        let parse_result = parse(&lex(&sf).tokens);
        let juxc_ast::TopLevelDecl::Class(c) = &parse_result.ast.items[0] else { panic!("class") };
        assert!(ctor_is_pure_store(&c.constructors[0], c));
        assert!(!ctor_is_pure_store(&c.constructors[1], c), "`k + 1` computes");
    }
}
