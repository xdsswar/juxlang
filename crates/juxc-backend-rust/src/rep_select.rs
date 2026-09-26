//! The class representation selector's `mutated` property
//! (`Architecture/JUX-CLASS-REPRESENTATION-ADDENDUM.md` §CR.3.2, §CR.4.1;
//! ERRATA E1XX-PHASE8).
//!
//! A class needs the interior-mutable cell (`Rc<JuxCell<C_Inner>>`) exactly
//! when something writes into one of its objects after the object exists.
//! Everything else can share its object through a plain `Rc<C_Inner>`: the
//! fields are read through the refcount and never borrowed, so there is no
//! borrow flag to pay for and no run-time borrow conflict to guard against.
//!
//! The question is asked of the program text, keyed by fully-qualified class
//! name, and every doubt answers "mutated". A class wrongly judged immutable
//! would have its cell taken away while some statement still writes through
//! it; the emitter then asks for a `borrow_mut()` the handle cannot give, and
//! records the class as a missed mutation ([`crate::RustEmitter::cell_write`]),
//! so the lowering is redone with that class given its cell back. The analysis
//! is the selector; that record is the check that it was right, and the
//! corpus runs with it turned into an error (`JUX_SELFCHECK`).

use std::collections::{HashMap, HashSet};

use juxc_ast::visit::{for_each_node, for_each_node_in, Node};
use juxc_ast::{Block, ClassDecl, Expr, Stmt, TopLevelDecl};
use juxc_source::Span;
use juxc_tycheck::{SymbolTable, Ty};

use crate::backend_fqn;

/// Where a written place sits.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    /// The class whose member body is being walked, by FQN, with its own
    /// declaration. `None` for a free function, a record, an enum or an
    /// interface, whose `this` is not a class object.
    class: Option<(&'a str, &'a ClassDecl)>,
    /// Inside a constructor or an instance initializer of `class`: a store to
    /// one of its own fields there is construction, not mutation (§CR.4.1).
    constructing: bool,
}

/// What the walk knows about the program's classes.
struct Index<'a> {
    /// Class FQN → its declaration.
    decls: HashMap<String, &'a ClassDecl>,
    /// Class FQN → the FQN its `extends` resolves to.
    parent: HashMap<String, String>,
    /// Bare class name → every FQN with that bare name.
    by_bare: HashMap<String, Vec<String>>,
    /// Instance field name → every class FQN declaring a field of that name.
    by_field: HashMap<String, Vec<String>>,
}

impl Index<'_> {
    /// The class FQNs a type can name. A name the symbol table knows is that
    /// class; anything else is matched on its bare name, which may name
    /// several classes, and then all of them are meant.
    fn classes_of(&self, ty: &Ty) -> Vec<String> {
        match ty {
            Ty::Nullable(inner) => self.classes_of(inner),
            Ty::User { name, .. } => {
                if self.decls.contains_key(name) {
                    return vec![name.clone()];
                }
                let bare = backend_fqn::fqn_bare(name);
                let bare = bare.rsplit("__").next().unwrap_or(bare);
                let mut out: Vec<String> = self.by_bare.get(bare).cloned().unwrap_or_default();
                // A nested type is keyed `Outer__Inner`; its bare spelling may
                // arrive either way.
                if let Some(v) = self.by_bare.get(backend_fqn::fqn_bare(name)) {
                    out.extend(v.iter().cloned());
                }
                out
            }
            _ => Vec::new(),
        }
    }

    /// Whether a type is a value that lives INSIDE its owner (a record, a
    /// struct, a primitive): a write into it is a write into the owner. A
    /// class, an interface, an array and a collection are handles with their
    /// own storage, so a write into one does not touch the owner.
    fn is_handle(&self, ty: &Ty, symbols: &SymbolTable) -> bool {
        match ty {
            Ty::Nullable(inner) => self.is_handle(inner, symbols),
            Ty::Array { .. } => true,
            Ty::User { name, .. } => {
                if symbols.interfaces.contains_key(name) {
                    return true;
                }
                let classes = self.classes_of(ty);
                // A user class that is not a `struct` is a handle. Anything the
                // walk cannot place is treated as a value, which marks more.
                !classes.is_empty()
                    && classes.iter().all(|c| self.decls.get(c).is_some_and(|d| !d.is_struct))
            }
            _ => false,
        }
    }

    /// `fqn` and every ancestor: a field declared up the chain is written
    /// through the object whichever class declares it.
    fn with_ancestors(&self, fqn: &str) -> Vec<String> {
        let mut out = vec![fqn.to_string()];
        let mut cur = fqn.to_string();
        for _ in 0..64 {
            match self.parent.get(&cur) {
                Some(p) if !out.contains(p) => {
                    out.push(p.clone());
                    cur = p.clone();
                }
                _ => break,
            }
        }
        out
    }

    /// Whether `name` is an instance field of `fqn` or of an ancestor.
    fn has_instance_field(&self, fqn: &str, name: &str) -> bool {
        self.with_ancestors(fqn).iter().any(|c| {
            self.decls
                .get(c)
                .is_some_and(|d| d.fields.iter().any(|f| !f.is_static && f.name.text == name))
        })
    }
}

/// Every class FQN whose objects are written after construction, or whose
/// lowering needs the cell for another reason (below). The complement, among
/// the wrap-eligible classes, may drop the cell.
///
/// Structural reasons, independent of any write:
///
/// - a `ref` field (§M.13), an `observer` field, or an observable property,
///   settable or computed (§P): each is storage the lowering updates in
///   place (attaching an observer writes its list);
/// - an interface it implements declares a settable property: the interface's
///   setter writes the object;
/// - a constructor that runs against the finished object instead of the
///   struct being built (it calls a method on `this`, or hands `this` to a
///   lambda), because its stores then go through the handle;
/// - a class in an `extends` hierarchy whose constructor does more than store
///   its parameters (ERRATA E21 runs it against the handle).
pub(crate) fn compute_cell_classes(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
    symbols: &SymbolTable,
    unit_offset: usize,
    extern_mut_methods: &HashSet<String>,
) -> HashSet<String> {
    let mut index = Index {
        decls: HashMap::new(),
        parent: HashMap::new(),
        by_bare: HashMap::new(),
        by_field: HashMap::new(),
    };
    // (class FQN, decl) for the structural pass below.
    let mut classes: Vec<(String, &ClassDecl)> = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        if unit.is_external {
            continue;
        }
        let pkg = unit_package(unit);
        let ctx = symbols.units.get(unit_offset + i);
        for item in &unit.items {
            let TopLevelDecl::Class(cd) = item else { continue };
            let fqn = if pkg.is_empty() { cd.name.text.clone() } else { format!("{pkg}.{}", cd.name.text) };
            if let Some(t) = &cd.extends {
                let written = t.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                if let Some(p) = backend_fqn::resolve_class_name(symbols, ctx, &pkg, &written) {
                    index.parent.insert(fqn.clone(), p);
                }
            }
            index.by_bare.entry(cd.name.text.clone()).or_default().push(fqn.clone());
            for f in cd.fields.iter().filter(|f| !f.is_static) {
                index.by_field.entry(f.name.text.clone()).or_default().push(fqn.clone());
            }
            index.decls.insert(fqn.clone(), cd);
            classes.push((fqn, cd));
        }
    }
    let children: HashSet<&String> = index.parent.values().collect();

    let mut user_mut: HashSet<String> = extern_mut_methods.clone();
    for unit in units {
        user_mut.extend(crate::analysis::collect_user_mut_methods_seeded(unit, extern_mut_methods));
    }

    let mut cells: HashSet<String> = HashSet::new();

    // Interfaces that declare a settable property (`{ get; set; }`), closed
    // over interface inheritance, by bare name. A class that meets one with a
    // field hands the interface a setter that writes the object.
    let mut iface_parents: HashMap<String, Vec<String>> = HashMap::new();
    let mut settable_ifaces: HashSet<String> = HashSet::new();
    for unit in units {
        for item in &unit.items {
            let TopLevelDecl::Interface(id) = item else { continue };
            let parents = id
                .extends
                .iter()
                .filter_map(|t| t.name.segments.last().map(|s| s.text.clone()))
                .collect();
            iface_parents.insert(id.name.text.clone(), parents);
            if id.properties.iter().any(|p| p.setter.is_some()) {
                settable_ifaces.insert(id.name.text.clone());
            }
        }
    }
    let mut grew = true;
    while grew {
        grew = false;
        for (name, parents) in &iface_parents {
            if !settable_ifaces.contains(name) && parents.iter().any(|p| settable_ifaces.contains(p)) {
                settable_ifaces.insert(name.clone());
                grew = true;
            }
        }
    }

    // Structural reasons.
    for (fqn, cd) in &classes {
        let in_hierarchy = cd.extends.is_some() || children.contains(fqn);
        let structural = cd.fields.iter().any(|f| {
            !f.is_static
                && (f.is_ref
                    || f.ty.as_ref().is_some_and(|t| {
                        t.fn_shape.is_none()
                            && t.name.segments.len() == 1
                            && t.name.segments[0].text == "observer"
                    }))
        }) || !crate::decls::observers::observable_props(cd).is_empty()
            || !crate::decls::observers::computed_observable_props(cd).is_empty()
            || cd.implements.iter().any(|t| {
                t.name.segments.last().is_some_and(|s| settable_ifaces.contains(&s.text))
            })
            || cd
                .constructors
                .iter()
                .any(|c| crate::RustEmitter::ctor_calls_method_on_this(cd, c))
            || (in_hierarchy
                && cd
                    .constructors
                    .iter()
                    .any(|c| !juxc_tycheck::clone_needs::ctor_is_pure_store(c, cd)));
        if structural {
            cells.insert(fqn.clone());
        }
    }

    // Writes.
    let mut walker = Walker { index: &index, expr_types, symbols, user_mut: &user_mut, out: &mut cells };
    for unit in units {
        if unit.is_external {
            continue;
        }
        let pkg = unit_package(unit);
        for item in &unit.items {
            match item {
                TopLevelDecl::Class(cd) => {
                    let fqn = if pkg.is_empty() { cd.name.text.clone() } else { format!("{pkg}.{}", cd.name.text) };
                    walker.class(&fqn, cd);
                }
                other => walker.other(other),
            }
        }
    }
    cells
}

fn unit_package(unit: &juxc_ast::CompilationUnit) -> String {
    unit.package
        .as_ref()
        .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
        .unwrap_or_default()
}

struct Walker<'a, 'b> {
    index: &'a Index<'a>,
    expr_types: &'a HashMap<Span, Ty>,
    symbols: &'a SymbolTable,
    user_mut: &'a HashSet<String>,
    out: &'b mut HashSet<String>,
}

impl Walker<'_, '_> {
    fn class(&mut self, fqn: &str, cd: &ClassDecl) {
        let member = Ctx { class: Some((fqn, cd)), constructing: false };
        let building = Ctx { class: Some((fqn, cd)), constructing: true };
        for m in &cd.methods {
            if let Some(b) = &m.body {
                self.block(b, member);
            }
        }
        for op in &cd.operators {
            if let Some(b) = &op.body {
                self.block(b, member);
            }
        }
        for p in &cd.properties {
            for body in p.getter.iter().map(|g| &g.body).chain(p.setter.iter().map(|s| &s.body)) {
                match body {
                    juxc_ast::AccessorBody::Auto => {}
                    juxc_ast::AccessorBody::Expr(e) => self.expr(e, member),
                    juxc_ast::AccessorBody::Block(b) => self.block(b, member),
                }
            }
            if let Some(init) = &p.initializer {
                self.expr(init, building);
            }
        }
        for b in &cd.drop_blocks {
            self.block(b, member);
        }
        for b in &cd.static_init_blocks {
            self.block(b, Ctx { class: None, constructing: false });
        }
        for c in &cd.constructors {
            self.block(&c.body, building);
        }
        for b in &cd.init_blocks {
            self.block(b, building);
        }
        for f in &cd.fields {
            if let Some(d) = &f.default {
                self.expr(d, if f.is_static { Ctx { class: None, constructing: false } } else { building });
            }
        }
        for nt in &cd.nested_types {
            match nt {
                TopLevelDecl::Class(inner) => {
                    let inner_fqn = format!("{fqn}__{}", inner.name.text);
                    self.class(&inner_fqn, inner);
                }
                other => self.other(other),
            }
        }
    }

    fn other(&mut self, item: &TopLevelDecl) {
        let none = Ctx { class: None, constructing: false };
        match item {
            TopLevelDecl::Function(fd) => {
                if let Some(b) = &fd.body {
                    self.block(b, none);
                }
            }
            TopLevelDecl::Record(rd) => {
                for m in &rd.methods {
                    if let Some(b) = &m.body {
                        self.block(b, none);
                    }
                }
            }
            TopLevelDecl::Enum(ed) => {
                for m in &ed.methods {
                    if let Some(b) = &m.body {
                        self.block(b, none);
                    }
                }
            }
            TopLevelDecl::Interface(id) => {
                for m in &id.methods {
                    if let Some(b) = &m.body {
                        self.block(b, none);
                    }
                }
            }
            TopLevelDecl::Class(_) => {}
            _ => {}
        }
    }

    fn block(&mut self, b: &Block, ctx: Ctx<'_>) {
        let user_mut = self.user_mut;
        for_each_node(b, &mut |n| {
            let mut places: Vec<&Expr> = Vec::new();
            collect_places(n, user_mut, &mut places);
            for p in places {
                self.place(p, ctx);
            }
        });
    }

    fn expr(&mut self, e: &Expr, ctx: Ctx<'_>) {
        let user_mut = self.user_mut;
        for_each_node_in(e, &mut |n| {
            let mut places: Vec<&Expr> = Vec::new();
            collect_places(n, user_mut, &mut places);
            for p in places {
                self.place(p, ctx);
            }
        });
    }

    /// Mark `fqn` as written, with every class its object may also be seen as:
    /// the ancestors that declare the fields it inherits, and the subclasses
    /// whose objects a `fqn`-typed value may really be. (§CR.3.5 gives the
    /// whole hierarchy one representation anyway.)
    fn mark(&mut self, fqn: &str) {
        let mut stack = vec![fqn.to_string()];
        while let Some(c) = stack.pop() {
            if !self.out.insert(c.clone()) && c != fqn {
                continue;
            }
            if let Some(p) = self.index.parent.get(&c) {
                if !self.out.contains(p) {
                    stack.push(p.clone());
                }
            }
            for (child, parent) in &self.index.parent {
                if parent == &c && !self.out.contains(child) {
                    stack.push(child.clone());
                }
            }
        }
    }

    /// Mark whichever objects a store into `place` writes.
    fn place(&mut self, place: &Expr, ctx: Ctx<'_>) {
        match place {
            Expr::Field(f) => {
                if matches!(&*f.object, Expr::This(_)) {
                    if let Some((fqn, _)) = ctx.class {
                        if !ctx.constructing {
                            self.mark(fqn);
                        }
                    } else {
                        self.by_field_name(&f.field.text);
                    }
                    return;
                }
                match self.expr_types.get(&crate::exprs::expr_span_of(&f.object)) {
                    Some(ty) => {
                        let owners = self.index.classes_of(ty);
                        if owners.is_empty() {
                            // An interface, a type parameter, a type the walk
                            // cannot place: any class with such a field.
                            self.by_field_name(&f.field.text);
                        }
                        for o in &owners {
                            self.mark(o);
                        }
                        if !self.index.is_handle(ty, self.symbols) {
                            // A value field: the write lands in whatever holds it.
                            self.place(&f.object, ctx);
                        }
                    }
                    None => {
                        self.by_field_name(&f.field.text);
                        self.place(&f.object, ctx);
                    }
                }
            }
            Expr::Index(i) => self.place(&i.array, ctx),
            Expr::NotNullAssert(inner, _) => self.place(inner, ctx),
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                if let Some((fqn, _)) = ctx.class {
                    if self.index.has_instance_field(fqn, name) && !ctx.constructing {
                        self.mark(fqn);
                    }
                }
            }
            _ => {}
        }
    }

    /// A write whose owner the types do not name: every class with a field of
    /// that name is assumed to be the one written.
    fn by_field_name(&mut self, name: &str) {
        if let Some(owners) = self.index.by_field.get(name).cloned() {
            for o in owners {
                self.mark(&o);
            }
        }
    }
}

/// The places a node writes: an assignment's target, `++`/`--`, an `out`
/// argument, `&x`, and the receiver of a call that mutates it (a known
/// mutating container method, a user method that writes `this`, or a foreign
/// method whose stub says it takes `&mut self`).
fn collect_places<'a>(n: Node<'a>, user_mut: &HashSet<String>, out: &mut Vec<&'a Expr>) {
    match n {
        Node::Stmt(Stmt::Assign(a)) => out.push(&a.target),
        Node::Expr(Expr::IncDec(i)) => out.push(&i.target),
        Node::Expr(Expr::Out(inner, _)) => out.push(inner),
        Node::Expr(Expr::Unary(u)) if u.op == juxc_ast::UnaryOp::AddrOf => out.push(&u.operand),
        Node::Expr(Expr::Call(c)) => {
            if let Expr::Field(f) = &*c.callee {
                if crate::analysis::is_mutating_method(&f.field.text) || user_mut.contains(&f.field.text) {
                    out.push(&f.object);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// §CR.3.2's `aliased` and `escapes`: classes whose objects stay where they
// were made.
// ---------------------------------------------------------------------------

/// How the objects of a CONTAINED class move (see [`compute_contained_classes`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Containment {
    /// Every object lives and dies in the local it was made into (§CR.2.1).
    Local,
    /// Some objects are returned from the function that made them, and move
    /// into the caller's local (§CR.2.2): they escape, and are still never
    /// aliased.
    Returned,
}

/// The classes whose objects can be held by value without anything the
/// program does telling the difference (§CR.3.3's `Inline` and `Box` rows).
///
/// The question is decided by a whitelist of positions, not by a list of the
/// ways an object could be observed: an object of a contained class `C`
/// appears ONLY
///
/// - as the initializer of a local variable, when it is fresh there
///   (`var p = new C(..)`, or the result of a function returning `C`; never
///   `var q = p`),
/// - as a whole expression statement (`new C(..);`),
/// - as the receiver of a field read, a field write or a method call
///   (`p.x`, `p.x = 1`, `p.m()`), `this` included, and
/// - as the value of a `return` (the object escapes, into the caller's local),
///
/// never inside a lambda or an anonymous class, and `C` is named by no field,
/// parameter, record component, enum payload, type bound, alias or constant,
/// and by no expression type other than exactly `C`. So an object is never
/// passed, stored, compared, printed, hashed, captured, put in an array or a
/// collection, or given a second name: at every point exactly one binding
/// reaches it, and a copy of it would be indistinguishable from it. Java's
/// sharing (§CR.4.1) and identity (§CR.4) cannot be observed, which is the
/// condition §CR.2.1 and §CR.2.2 state for the value representations.
///
/// A receiver may only be asked for one of the class's own fields or methods
/// (the universal `operator hash` / `operator string` read its identity); a
/// local's initializer only counts when the local is `var` or declared as the
/// class itself; a `return` only when the function is declared to return the
/// class itself; and a closure inside the class's own members may not name
/// one of its instance members (it would reach `this`).
///
/// A class is a candidate at all only when its lowering has nothing a value
/// cannot carry: no type parameters, no `extends`, no subclass, no
/// `implements`, not abstract, no properties, no `drop` body (a copy would run
/// it twice), no annotation (the registry may hold its objects), no `async`
/// or generator method (its future holds the receiver), no `ref`, `weak` or
/// `observer` field.
pub(crate) fn compute_contained_classes(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
) -> HashMap<String, Containment> {
    // Candidates by bare name → their FQNs. A type is matched on its bare
    // name, so two same-named classes are ruled out together.
    let mut candidates: HashMap<String, Vec<String>> = HashMap::new();
    let mut members: HashMap<String, HashSet<String>> = HashMap::new();
    let mut instance_members: HashMap<String, HashSet<String>> = HashMap::new();
    let mut extended: HashSet<String> = HashSet::new();
    for unit in units {
        for item in &unit.items {
            if let TopLevelDecl::Class(cd) = item {
                if let Some(seg) = cd.extends.as_ref().and_then(|t| t.name.segments.last()) {
                    extended.insert(simple_name(&seg.text));
                }
            }
        }
    }
    for unit in units {
        if unit.is_external {
            continue;
        }
        let pkg = unit_package(unit);
        for item in &unit.items {
            let TopLevelDecl::Class(cd) = item else { continue };
            let plain = !cd.is_struct
                && !cd.is_abstract
                && cd.generic_params.is_empty()
                && cd.extends.is_none()
                && !extended.contains(&simple_name(&cd.name.text))
                && cd.implements.is_empty()
                && cd.properties.is_empty()
                && cd.drop_blocks.is_empty()
                && cd.annotations.is_empty()
                && cd.nested_types.is_empty()
                && !cd.methods.iter().any(suspends)
                && !cd.fields.iter().any(|f| {
                    f.is_ref
                        || f.is_weak
                        || f.ty.as_ref().is_some_and(|t| t.name.segments.len() == 1 && t.name.segments[0].text == "observer")
                });
            if plain {
                let fqn = if pkg.is_empty() { cd.name.text.clone() } else { format!("{pkg}.{}", cd.name.text) };
                let simple = simple_name(&cd.name.text);
                candidates.entry(simple.clone()).or_default().push(fqn);
                let own = members.entry(simple.clone()).or_default();
                let inst = instance_members.entry(simple).or_default();
                for f in &cd.fields {
                    own.insert(f.name.text.clone());
                    if !f.is_static {
                        inst.insert(f.name.text.clone());
                    }
                }
                for m in &cd.methods {
                    own.insert(m.name.text.clone());
                    if !m.modifiers.contains(&juxc_ast::FnModifier::Static) {
                        inst.insert(m.name.text.clone());
                    }
                }
            }
        }
    }
    let mut scan = Containments {
        candidates: &candidates,
        members: &members,
        instance_members: &instance_members,
        ruled_out: HashSet::new(),
        returned: HashSet::new(),
    };
    for unit in units {
        for item in &unit.items {
            scan.decl(item, expr_types);
        }
    }
    let mut out = HashMap::new();
    for (bare, fqns) in &candidates {
        if scan.ruled_out.contains(bare) {
            continue;
        }
        let how = if scan.returned.contains(bare) { Containment::Returned } else { Containment::Local };
        for f in fqns {
            out.insert(f.clone(), how);
        }
    }
    out
}

/// The body being judged: the class whose member it is, and the exact class
/// name its declared return type is.
#[derive(Clone, Copy)]
struct Frame<'a> {
    class: Option<&'a str>,
    returns: Option<&'a str>,
}

/// Whether a method's body runs later than its call: an `async` method or a
/// generator (`yield`) keeps its receiver in the future or iterator it
/// returns, which a value lowering would copy there.
fn suspends(m: &juxc_ast::FnDecl) -> bool {
    if matches!(m.return_type, juxc_ast::ReturnType::AsyncType(_)) || m.modifiers.contains(&juxc_ast::FnModifier::Async) {
        return true;
    }
    let mut yields = false;
    if let Some(b) = &m.body {
        for_each_node(b, &mut |n| yields |= matches!(n, Node::Stmt(Stmt::Yield(..))));
    }
    yields
}

struct Containments<'a> {
    candidates: &'a HashMap<String, Vec<String>>,
    /// Candidate bare name → the names of its fields and methods: what a
    /// receiver of that class may be asked for. Anything else (the universal
    /// `operator hash`, `operator string`) reads the object's identity.
    members: &'a HashMap<String, HashSet<String>>,
    /// Candidate bare name → its instance fields and methods, which a bare
    /// name inside a closure in its own members reaches through `this`.
    instance_members: &'a HashMap<String, HashSet<String>>,
    /// Bare names of candidates some position rules out.
    ruled_out: HashSet<String>,
    /// Bare names of candidates some function returns.
    returned: HashSet<String>,
}

impl Containments<'_> {
    /// Rule out every candidate a written type mentions anywhere in it.
    fn type_ref(&mut self, t: &juxc_ast::TypeRef) {
        let mut names = Vec::new();
        type_ref_names(t, &mut names);
        for n in names {
            if self.candidates.contains_key(&n) {
                self.ruled_out.insert(n);
            }
        }
    }

    /// A declared return type: exactly `C` marks `C` as returned and is the
    /// body's frame; anything else that mentions a candidate rules it out.
    fn return_type(&mut self, r: &juxc_ast::ReturnType) -> Option<String> {
        let t = match r {
            juxc_ast::ReturnType::Void => return None,
            juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t) => t,
        };
        let async_ = matches!(r, juxc_ast::ReturnType::AsyncType(_));
        match exact_name(t) {
            Some(n) if !async_ && self.candidates.contains_key(&n) => {
                self.returned.insert(n.clone());
                Some(n)
            }
            _ => {
                self.type_ref(t);
                None
            }
        }
    }

    fn function(&mut self, f: &juxc_ast::FnDecl, class: Option<&str>, et: &HashMap<Span, Ty>) {
        let returns = self.return_type(&f.return_type);
        for p in &f.params {
            self.type_ref(&p.ty);
        }
        for tp in &f.generic_params {
            for b in &tp.bounds {
                self.type_ref(b);
            }
        }
        if let Some(b) = &f.body {
            self.body(b, Frame { class, returns: returns.as_deref() }, et);
        }
    }

    fn decl(&mut self, item: &TopLevelDecl, et: &HashMap<Span, Ty>) {
        let plain = Frame { class: None, returns: None };
        match item {
            TopLevelDecl::Function(f) => self.function(f, None, et),
            TopLevelDecl::Class(cd) => {
                let simple = simple_name(&cd.name.text);
                let own = Some(simple.as_str());
                let member = Frame { class: own, returns: None };
                for tp in &cd.generic_params {
                    for b in &tp.bounds {
                        self.type_ref(b);
                    }
                }
                for t in cd.extends.iter().chain(cd.implements.iter()) {
                    self.type_ref(t);
                }
                for f in &cd.fields {
                    if let Some(t) = &f.ty {
                        self.type_ref(t);
                    }
                    if let Some(d) = &f.default {
                        self.expr_body(d, member, et);
                    }
                }
                for p in &cd.properties {
                    self.type_ref(&p.ty);
                }
                for m in &cd.methods {
                    self.function(m, own, et);
                }
                for c in &cd.constructors {
                    for p in &c.params {
                        self.type_ref(&p.ty);
                    }
                    self.body(&c.body, member, et);
                }
                for op in &cd.operators {
                    let returns = self.return_type(&op.return_type);
                    for p in &op.params {
                        self.type_ref(&p.ty);
                    }
                    if let Some(b) = &op.body {
                        self.body(b, Frame { class: own, returns: returns.as_deref() }, et);
                    }
                }
                for b in cd.init_blocks.iter().chain(&cd.drop_blocks) {
                    self.body(b, member, et);
                }
                for b in &cd.static_init_blocks {
                    self.body(b, plain, et);
                }
                for nt in &cd.nested_types {
                    self.decl(nt, et);
                }
            }
            TopLevelDecl::Record(rd) => {
                for c in &rd.components {
                    self.type_ref(&c.ty);
                }
                for m in &rd.methods {
                    self.function(m, None, et);
                }
            }
            TopLevelDecl::Enum(ed) => {
                for v in &ed.variants {
                    for p in &v.payload {
                        self.type_ref(&p.ty);
                    }
                    for a in &v.args {
                        self.expr_body(a, plain, et);
                    }
                }
                for f in &ed.fields {
                    if let Some(t) = &f.ty {
                        self.type_ref(t);
                    }
                }
                for m in &ed.methods {
                    self.function(m, None, et);
                }
            }
            TopLevelDecl::Interface(id) => {
                for f in &id.fields {
                    if let Some(t) = &f.ty {
                        self.type_ref(t);
                    }
                }
                for m in &id.methods {
                    self.function(m, None, et);
                }
            }
            TopLevelDecl::TypeAlias(a) => self.type_ref(&a.target),
            TopLevelDecl::Const(c) => {
                if let Some(t) = &c.ty {
                    self.type_ref(t);
                }
            }
            _ => {}
        }
    }

    fn body(&mut self, b: &Block, frame: Frame<'_>, et: &HashMap<Span, Ty>) {
        let mut places = Positions::default();
        for_each_node(b, &mut |n| places.note(n));
        let mut seen: Vec<Seen> = Vec::new();
        for_each_node(b, &mut |n| note_seen(n, et, &mut seen, &mut places));
        self.judge(&places, &seen, frame);
    }

    fn expr_body(&mut self, e: &Expr, frame: Frame<'_>, et: &HashMap<Span, Ty>) {
        let mut places = Positions::default();
        for_each_node_in(e, &mut |n| places.note(n));
        let mut seen: Vec<Seen> = Vec::new();
        for_each_node_in(e, &mut |n| note_seen(n, et, &mut seen, &mut places));
        // A field or variant initializer is no local's initializer, and no
        // statement: a candidate object made here is stored.
        self.judge(&places, &seen, frame);
    }

    fn judge(&mut self, places: &Positions, seen: &[Seen], frame: Frame<'_>) {
        // A closure in a candidate's own member that names one of its
        // instance members reaches `this` without writing it.
        if let Some(c) = frame.class {
            if let Some(inst) = self.instance_members.get(c) {
                if places.closure_names.iter().any(|n| inst.contains(n)) {
                    self.ruled_out.insert(c.to_string());
                }
            }
        }
        for s in seen {
            if s.is_this {
                // `this` is an object of the class being walked: it may only
                // be a receiver, and never inside a closure.
                if let Some(c) = frame.class {
                    if self.candidates.contains_key(c) && !self.receives(places, s.span, c) {
                        self.ruled_out.insert(c.to_string());
                    }
                }
                continue;
            }
            let Some(ty) = &s.ty else { continue };
            let mut names = Vec::new();
            ty_names(ty, &mut names);
            for n in names {
                if !self.candidates.contains_key(&n) {
                    continue;
                }
                let exact = matches!(ty, Ty::User { name, generic_args } if generic_args.is_empty() && bare_of(name) == n);
                let allowed = exact
                    && (self.receives(places, s.span, &n)
                        || places.local_init.get(&s.span).is_some_and(|declared| {
                            declared.as_deref().map_or(true, |d| d == n)
                        })
                        || places.statement.contains(&s.span)
                        || (places.returned.contains(&s.span) && frame.returns == Some(n.as_str())));
                if !allowed || places.inside_closure.contains(&s.span) {
                    self.ruled_out.insert(n);
                }
            }
        }
    }

    /// Whether the expression at `span` is the receiver of one of candidate
    /// `c`'s own members, outside any closure.
    fn receives(&self, places: &Positions, span: Span, c: &str) -> bool {
        !places.inside_closure.contains(&span)
            && places
                .receiver
                .get(&span)
                .is_some_and(|member| self.members.get(c).is_some_and(|m| m.contains(member)))
    }
}

/// One expression the judge looks at.
struct Seen {
    span: Span,
    ty: Option<Ty>,
    is_this: bool,
}

fn note_seen(n: Node<'_>, et: &HashMap<Span, Ty>, seen: &mut Vec<Seen>, places: &mut Positions) {
    let Node::Expr(e) = n else { return };
    let span = crate::exprs::expr_span_of(e);
    seen.push(Seen { span, ty: et.get(&span).cloned(), is_this: matches!(e, Expr::This(_)) });
    // An anonymous subclass of a class is an object of that class made in a
    // place no local holds.
    if let Expr::NewObject(no) = e {
        if no.anonymous_body.is_some() {
            if let Some(seg) = no.class_name.segments.last() {
                seen.push(Seen {
                    span: no.span,
                    ty: Some(Ty::User { name: seg.text.clone(), generic_args: Vec::new() }),
                    is_this: false,
                });
                places.inside_closure.insert(no.span);
            }
        }
    }
}

/// The positions a contained object may take, by the span of the expression
/// in them.
#[derive(Default)]
struct Positions {
    /// A receiver, with the member it is asked for.
    receiver: HashMap<Span, String>,
    /// A local's initializer, with the class name the local is declared as
    /// (`None` for `var`).
    local_init: HashMap<Span, Option<String>>,
    statement: HashSet<Span>,
    returned: HashSet<Span>,
    /// Every expression inside a lambda or an anonymous class body.
    inside_closure: HashSet<Span>,
    /// Every single-segment name written inside a lambda or an anonymous
    /// class body.
    closure_names: HashSet<String>,
}

impl Positions {
    fn note(&mut self, n: Node<'_>) {
        match n {
            Node::Expr(Expr::Field(f)) => {
                self.receiver.insert(crate::exprs::expr_span_of(&f.object), f.field.text.clone());
            }
            Node::Stmt(Stmt::VarDecl(v)) => {
                // Only a FRESH object may start a local: a `new`, or what a
                // call returns. `var b = a;` would give the object a second
                // name.
                let fresh = |e: &Expr| {
                    matches!(e, Expr::Call(_)) || matches!(e, Expr::NewObject(no) if no.anonymous_body.is_none())
                };
                if let Some(init) = v.init.as_ref().filter(|e| fresh(e)) {
                    let declared = match &v.ty {
                        None => Some(None),
                        Some(t) => exact_name(t).map(Some),
                    };
                    if let Some(declared) = declared {
                        self.local_init.insert(crate::exprs::expr_span_of(init), declared);
                    }
                }
            }
            Node::Stmt(Stmt::Expr(e)) => {
                self.statement.insert(crate::exprs::expr_span_of(e));
            }
            Node::Stmt(Stmt::Return(Some(e), _)) => {
                self.returned.insert(crate::exprs::expr_span_of(e));
            }
            Node::Expr(Expr::Lambda(l)) => match &l.body {
                juxc_ast::LambdaBody::Expr(b) => for_each_node_in(b, &mut |m| self.note_closure(m)),
                juxc_ast::LambdaBody::Block(b) => for_each_node(b, &mut |m| self.note_closure(m)),
            },
            Node::Expr(Expr::NewObject(no)) => {
                if let Some(body) = &no.anonymous_body {
                    for b in &body.init_blocks {
                        for_each_node(b, &mut |m| self.note_closure(m));
                    }
                    for m in &body.methods {
                        if let Some(b) = &m.body {
                            for_each_node(b, &mut |m| self.note_closure(m));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn note_closure(&mut self, m: Node<'_>) {
        if let Node::Expr(e) = m {
            self.inside_closure.insert(crate::exprs::expr_span_of(e));
            if let Expr::Path(qn) = e {
                if let [only] = qn.segments.as_slice() {
                    self.closure_names.insert(only.text.clone());
                }
            }
        }
    }
}

fn bare_of(name: &str) -> String {
    simple_name(backend_fqn::fqn_bare(name))
}

/// A class's name as a written type names it: a nested class is lifted to
/// `Outer__Inner` and written `Inner` (or `Outer.Inner`), so candidates,
/// written types and checked types all meet on the last component. Two
/// classes that share it are decided together.
fn simple_name(name: &str) -> String {
    name.rsplit("__").next().unwrap_or(name).to_string()
}

/// The bare name a written type is, when it is exactly one plain class name:
/// no type arguments, not nullable, not an array, not a function or pointer.
fn exact_name(t: &juxc_ast::TypeRef) -> Option<String> {
    if t.nullable || t.array_shape.is_some() || t.fn_shape.is_some() || t.ptr_depth > 0 || !t.generic_args.is_empty() {
        return None;
    }
    t.name.segments.last().map(|s| simple_name(&s.text))
}

/// Every bare type name a written type mentions, its arguments and function
/// shape included.
fn type_ref_names(t: &juxc_ast::TypeRef, out: &mut Vec<String>) {
    if let Some(s) = t.name.segments.last() {
        out.push(simple_name(&s.text));
    }
    for a in &t.generic_args {
        if let Some(inner) = a.as_type() {
            type_ref_names(inner, out);
        }
    }
    if let Some(shape) = &t.fn_shape {
        for p in &shape.params {
            type_ref_names(p, out);
        }
        type_ref_names(&shape.return_type, out);
    }
}

/// Every bare class name a checked type mentions.
fn ty_names(t: &Ty, out: &mut Vec<String>) {
    match t {
        Ty::User { name, generic_args } => {
            out.push(bare_of(name));
            for a in generic_args {
                ty_names(a, out);
            }
        }
        Ty::Nullable(inner) => ty_names(inner, out),
        Ty::Array { element, .. } => ty_names(element, out),
        Ty::Wildcard(juxc_tycheck::ty::Wildcard::Extends(b) | juxc_tycheck::ty::Wildcard::Super(b)) => ty_names(b, out),
        Ty::Fn { params, return_type, .. } | Ty::FnPtr { params, return_type, .. } => {
            for p in params {
                ty_names(p, out);
            }
            ty_names(return_type, out);
        }
        _ => {}
    }
}

/// Whether every instance field of class `fqn` holds a Jux value or a Jux
/// class, never a foreign type, an array, a collection or a function: the
/// condition under which a lock-free `Arc` over its inner struct is `Sync`
/// (a worker-shared class's fields are themselves worker-shared, so each
/// class it holds carries an atomic handle too).
pub(crate) fn fields_are_jux_values(units: &[juxc_ast::CompilationUnit], symbols: &SymbolTable, fqn: &str) -> bool {
    const PRIMITIVES: &[&str] =
        &["int", "long", "short", "byte", "double", "float", "bool", "boolean", "char", "String"];
    for unit in units.iter().filter(|u| !u.is_external) {
        let pkg = unit_package(unit);
        for item in &unit.items {
            let TopLevelDecl::Class(cd) = item else { continue };
            let this = if pkg.is_empty() { cd.name.text.clone() } else { format!("{pkg}.{}", cd.name.text) };
            if this != fqn {
                continue;
            }
            return cd.fields.iter().filter(|f| !f.is_static).all(|f| {
                let Some(t) = &f.ty else { return false };
                if t.array_shape.is_some()
                    || t.fn_shape.is_some()
                    || t.ptr_depth > 0
                    || !t.generic_args.is_empty()
                    || f.is_ref
                    || f.is_weak
                {
                    return false;
                }
                let Some(head) = t.name.segments.last().map(|s| s.text.as_str()) else { return false };
                if PRIMITIVES.contains(&head) {
                    return true;
                }
                let jux = |name: &str| {
                    symbols.classes.get(name).is_some_and(|c| !c.is_external)
                        || symbols.records.contains_key(name)
                        || symbols.enums.get(name).is_some_and(|e| !e.is_external)
                };
                let written = t.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                let in_pkg = if pkg.is_empty() { written.clone() } else { format!("{pkg}.{written}") };
                jux(&written) || jux(&in_pkg)
            });
        }
    }
    false
}

// ---------------------------------------------------------------------------
// §CR.7: the representations the selector must never commit to.
// ---------------------------------------------------------------------------

/// A value representation (Inline or `Box`) the program cannot have (§CR.7,
/// ERRATA E1XX-PHASE8). The selector escalates past each of these on its own
/// (the whitelist of [`compute_contained_classes`] rules every one of them
/// out), so a violation means the selector was wrong about a class, and it is
/// reported rather than lowered into a program that would behave differently
/// from Java.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepViolation {
    /// `E0953`, `E0954` or `E0955`.
    pub code: &'static str,
    /// The class, by fully-qualified name.
    pub class: String,
    /// What it was selected as.
    pub rep: &'static str,
}

/// Check the value representations in `reps` against §CR.7:
///
/// - `E0953`: an Inline or `Box` class whose objects are compared with `===`
///   or `!==`: identity needs a stable address that a copied value lacks;
/// - `E0954`: an Inline or `Box` class that a `weak` field or parameter
///   points at: a `Weak` needs a refcount;
/// - `E0955`: an Inline or `Box` class whose fields, followed through other
///   classes' fields, contain itself: a cycle needs a refcount (and Inline
///   would be infinitely large).
pub(crate) fn verify_selection(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
    reps: &HashMap<String, crate::ClassRep>,
) -> Vec<RepViolation> {
    let value = |r: &crate::ClassRep| match r {
        crate::ClassRep::Inline => Some("inline"),
        crate::ClassRep::Box => Some("box"),
        _ => None,
    };
    // Simple name → (FQN, rep) of each value class.
    let mut by_simple: HashMap<String, Vec<(String, &'static str)>> = HashMap::new();
    for (fqn, r) in reps {
        if let Some(label) = value(r) {
            by_simple.entry(bare_of(fqn)).or_default().push((fqn.clone(), label));
        }
    }
    let mut out: Vec<RepViolation> = Vec::new();
    if by_simple.is_empty() {
        return out;
    }
    let report = |code: &'static str, simple: &str, out: &mut Vec<RepViolation>| {
        for (fqn, rep) in by_simple.get(simple).cloned().unwrap_or_default() {
            let v = RepViolation { code, class: fqn, rep };
            if !out.contains(&v) {
                out.push(v);
            }
        }
    };

    // E0953: identity comparisons.
    let mut compared: HashSet<String> = HashSet::new();
    let mut note = |e: &Expr| {
        if let Expr::Binary(b) = e {
            if matches!(b.op, juxc_ast::BinaryOp::RefEq | juxc_ast::BinaryOp::RefNeq) {
                for side in [&b.left, &b.right] {
                    if let Some(Ty::User { name, .. }) =
                        expr_types.get(&crate::exprs::expr_span_of(side)).map(strip_nullable_ty)
                    {
                        compared.insert(bare_of(name));
                    }
                }
            }
        }
    };
    for unit in units {
        for item in &unit.items {
            for_each_member_body(item, &mut |b| for_each_expr_in_block(b, &mut note));
        }
    }
    for simple in &compared {
        report("E0953", simple, &mut out);
    }

    // E0954: weak references.
    let mut weak_targets: HashSet<String> = HashSet::new();
    for unit in units {
        for item in &unit.items {
            match item {
                TopLevelDecl::Class(cd) => {
                    for f in cd.fields.iter().filter(|f| f.is_weak) {
                        if let Some(t) = f.ty.as_ref().and_then(exact_name) {
                            weak_targets.insert(t);
                        }
                    }
                    let params = cd
                        .methods
                        .iter()
                        .flat_map(|m| m.params.iter())
                        .chain(cd.constructors.iter().flat_map(|c| c.params.iter()));
                    for p in params.filter(|p| p.is_weak) {
                        if let Some(t) = exact_name(&p.ty) {
                            weak_targets.insert(t);
                        }
                    }
                }
                TopLevelDecl::Function(f) => {
                    for p in f.params.iter().filter(|p| p.is_weak) {
                        if let Some(t) = exact_name(&p.ty) {
                            weak_targets.insert(t);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for simple in &weak_targets {
        report("E0954", simple, &mut out);
    }

    // E0955: classes that contain themselves.
    for simple in crate::compute_recursive_field_classes(units) {
        report("E0955", &simple_name(&simple), &mut out);
    }
    out.sort_by(|a, b| (a.code, &a.class).cmp(&(b.code, &b.class)));
    out
}

fn strip_nullable_ty(t: &Ty) -> &Ty {
    match t {
        Ty::Nullable(inner) => strip_nullable_ty(inner),
        other => other,
    }
}

fn for_each_expr_in_block(b: &Block, f: &mut dyn FnMut(&Expr)) {
    juxc_ast::visit::for_each_expr(b, f);
}

/// Every executable body of a declaration.
fn for_each_member_body(item: &TopLevelDecl, f: &mut dyn FnMut(&Block)) {
    match item {
        TopLevelDecl::Function(fd) => {
            if let Some(b) = &fd.body {
                f(b);
            }
        }
        TopLevelDecl::Class(cd) => {
            for m in &cd.methods {
                if let Some(b) = &m.body {
                    f(b);
                }
            }
            for op in &cd.operators {
                if let Some(b) = &op.body {
                    f(b);
                }
            }
            for c in &cd.constructors {
                f(&c.body);
            }
            for b in cd.init_blocks.iter().chain(&cd.static_init_blocks).chain(&cd.drop_blocks) {
                f(b);
            }
        }
        TopLevelDecl::Record(rd) => {
            for m in &rd.methods {
                if let Some(b) = &m.body {
                    f(b);
                }
            }
        }
        TopLevelDecl::Enum(ed) => {
            for m in &ed.methods {
                if let Some(b) = &m.body {
                    f(b);
                }
            }
        }
        TopLevelDecl::Interface(id) => {
            for m in &id.methods {
                if let Some(b) = &m.body {
                    f(b);
                }
            }
        }
        _ => {}
    }
}
