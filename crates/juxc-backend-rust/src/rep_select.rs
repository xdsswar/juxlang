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
