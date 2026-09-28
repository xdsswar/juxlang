//! When each instance field initializer runs (ERRATA E139, E140).
//!
//! Construction follows Java (JUX-LANG-V1 §7.3.1): each class of the
//! hierarchy, root first, runs its `super(..)`, then its own field
//! initializers and `init` blocks in the order they are written, then the
//! rest of its constructor body.
//!
//! The object is built as one struct that holds every class's fields, so an
//! initializer is either evaluated into that struct, before any of the
//! hierarchy's code has run, or run against the finished object at its own
//! place in that order ("deferred"). Evaluating it early is only right when
//! nothing can tell: so an initializer is deferred when
//!
//! - it uses the object (`this`, `super`, a bare instance field, property or
//!   method name): the object does not exist while the struct is being
//!   built (E139);
//! - it comes after the class's first `init` block: the block runs first;
//! - an ancestor's construction may have an effect (an `init` block, a
//!   constructor body that does more than store its parameters, an
//!   initializer that calls something) and this initializer may have one
//!   too: the two effects have to happen in Java's order;
//!
//! and every initializer after a deferred one is deferred too, so the class's
//! own still run in the order written. When an ancestor's construction
//! reaches the object at all (calls a method on it, hands it out), every
//! initializer of the class is deferred: that code may read one of its
//! fields, and has to find it at its default value.
//!
//! The backend lowers from [`deferred_indices`]; the representation selector
//! and the driver's [`crate::late_fields`] pass read the same answer.

use juxc_ast::{Block, ClassDecl, Expr, FieldDecl, Stmt};

/// What an ancestor's construction does, as far as a subclass can tell.
#[derive(Debug, Clone, Copy, Default)]
pub struct AncestorFacts {
    /// Some ancestor's construction may have an effect.
    pub effects: bool,
    /// Some ancestor's construction reaches the object (a method called on
    /// it, the object handed out), so it may read a subclass's field.
    pub observes: bool,
}

impl AncestorFacts {
    /// The facts a subclass of `parent` inherits: the parent's own and its
    /// ancestors'.
    pub fn of_parent(parent: &ClassDecl, above: AncestorFacts, is_method: &dyn Fn(&str) -> bool) -> AncestorFacts {
        AncestorFacts {
            effects: above.effects || construction_has_effects(parent),
            observes: above.observes || construction_observes(parent, is_method),
        }
    }
}

/// Whether an instance field's initializer uses the object being built:
/// `this`, `super`, or a bare name that is one of its instance members (a
/// field read, a method call).
pub fn uses_object(init: &Expr, is_member: &dyn Fn(&str) -> bool) -> bool {
    let mut found = false;
    juxc_ast::visit::for_each_expr_in(init, &mut |e| match e {
        Expr::This(_) | Expr::Super(_) => found = true,
        Expr::Path(qn) if qn.segments.len() == 1 && is_member(&qn.segments[0].text) => found = true,
        _ => {}
    });
    found
}

/// Whether evaluating `e` may have an effect another piece of code could
/// see: it calls something, builds a user object (whose constructor may do
/// anything), or assigns.
pub fn may_have_effects(e: &Expr) -> bool {
    let mut found = false;
    juxc_ast::visit::for_each_expr_in(e, &mut |x| match x {
        Expr::Call(_) | Expr::IncDec(_) | Expr::Await(..) | Expr::Throw(..) => found = true,
        Expr::NewObject(n) => {
            // A collection the program only builds (`new Vec<T>()`) does
            // nothing else; a class's constructor may.
            let bare = n.class_name.segments.last().map(|s| s.text.as_str()).unwrap_or("");
            if !n.args.is_empty() || n.anonymous_body.is_some() || !is_builtin_collection(bare) {
                found = true;
            }
        }
        _ => {}
    });
    found
}

fn is_builtin_collection(bare: &str) -> bool {
    matches!(bare, "Vec" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet" | "VecDeque" | "StringBuilder")
}

/// Whether a class's own construction may have an effect a subclass's
/// initializer could be ordered against: an `init` block, a constructor
/// body that does more than store its parameters, an initializer that may
/// have an effect.
pub fn construction_has_effects(cd: &ClassDecl) -> bool {
    !cd.init_blocks.is_empty()
        || cd.constructors.iter().any(|c| !crate::clone_needs::ctor_is_pure_store(c, cd))
        || cd.fields.iter().any(|f| !f.is_static && f.default.as_ref().is_some_and(may_have_effects))
}

/// Whether a class's own construction reaches the object beyond reading
/// and writing its own fields: a method called on it (bare or through
/// `this`), `super.m()`, or `this` itself handed out (to a call, a
/// variable, a lambda, an anonymous class).
pub fn construction_observes(cd: &ClassDecl, is_method: &dyn Fn(&str) -> bool) -> bool {
    let mut count = ObjectUses::default();
    for f in cd.fields.iter().filter(|f| !f.is_static) {
        if let Some(d) = &f.default {
            juxc_ast::visit::for_each_node_in(d, &mut |n| count.visit(n, is_method));
        }
    }
    let mut blocks: Vec<&Block> = cd.init_blocks.iter().collect();
    blocks.extend(cd.constructors.iter().map(|c| &c.body));
    for b in blocks {
        for st in &b.statements {
            // `super(..)`'s arguments run before the object is this class's.
            if matches!(st, Stmt::SuperCall(..)) {
                continue;
            }
            let one = Block { statements: vec![st.clone()], span: b.span };
            juxc_ast::visit::for_each_node(&one, &mut |n| count.visit(n, is_method));
        }
    }
    count.observes()
}

/// Tallies for [`construction_observes`].
#[derive(Default)]
struct ObjectUses {
    /// Every `this`.
    this_total: usize,
    /// Each `this` that is only the object of a field read or store.
    this_as_field_object: usize,
    /// A method reached through the object, or `super`.
    reached: bool,
}

impl ObjectUses {
    fn visit(&mut self, n: juxc_ast::visit::Node<'_>, is_method: &dyn Fn(&str) -> bool) {
        let juxc_ast::visit::Node::Expr(e) = n else { return };
        match e {
            Expr::This(_) => self.this_total += 1,
            Expr::Super(_) => self.reached = true,
            Expr::Field(f) if matches!(f.object.as_ref(), Expr::This(_)) => self.this_as_field_object += 1,
            Expr::Call(c) => match c.callee.as_ref() {
                // `this.m(..)`: its callee was counted as a field access.
                Expr::Field(f) if matches!(f.object.as_ref(), Expr::This(_)) => self.reached = true,
                Expr::Path(qn) if qn.segments.len() == 1 && is_method(&qn.segments[0].text) => self.reached = true,
                _ => {}
            },
            _ => {}
        }
    }

    fn observes(&self) -> bool {
        self.reached || self.this_total > self.this_as_field_object
    }
}

/// The instance fields (indices into `fields`) whose initializers run
/// against the finished object (see the module docs). `first_init_block` is
/// where the class's first `init` block starts in its source, if it has one.
pub fn deferred_indices(
    fields: &[FieldDecl],
    first_init_block: Option<juxc_source::Span>,
    ancestor: AncestorFacts,
    is_member: &dyn Fn(&str) -> bool,
) -> Vec<usize> {
    let with_init: Vec<usize> = fields
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.is_static && f.default.is_some())
        .map(|(i, _)| i)
        .collect();
    if ancestor.observes {
        return with_init;
    }
    let first = with_init.iter().position(|&i| {
        let f = &fields[i];
        let Some(d) = f.default.as_ref() else { return false };
        uses_object(d, is_member)
            || first_init_block.is_some_and(|b| b.file == f.span.file && b.start < f.span.start)
            || (ancestor.effects && may_have_effects(d))
    });
    match first {
        Some(k) => with_init[k..].to_vec(),
        None => Vec::new(),
    }
}

/// Whether `name` is an instance member (field, property or method) of the
/// class `fqn` declared by `cd`, its own or inherited.
pub fn is_instance_member(cd: &ClassDecl, fqn: &str, symbols: &crate::SymbolTable, name: &str) -> bool {
    cd.fields.iter().any(|f| !f.is_static && f.name.text == name) || is_instance_method(cd, fqn, symbols, name)
        || symbols.lookup_field(fqn, name).is_some_and(|(f, _)| !f.is_static)
}

/// Whether `name` is an instance method or property of the class `fqn`,
/// its own or inherited: code a bare use of the name runs.
pub fn is_instance_method(cd: &ClassDecl, fqn: &str, symbols: &crate::SymbolTable, name: &str) -> bool {
    cd.properties.iter().any(|p| p.name.text == name)
        || cd.methods.iter().any(|m| m.name.text == name && !m.modifiers.contains(&juxc_ast::FnModifier::Static))
        || symbols.lookup_property(fqn, name).is_some_and(|(p, _)| !p.is_static)
        || symbols.lookup_method(fqn, name).is_some_and(|(m, _)| !m.is_static)
}

/// [`AncestorFacts`] for the class `fqn`, walking its ancestors through
/// `decl_of` (a class's declaration) and `parent_of` (its parent's FQN).
pub fn ancestor_facts_of<'a>(
    fqn: &str,
    decl_of: &dyn Fn(&str) -> Option<&'a ClassDecl>,
    parent_of: &dyn Fn(&str) -> Option<String>,
    symbols: &crate::SymbolTable,
    depth: usize,
) -> AncestorFacts {
    if depth > 64 {
        return AncestorFacts::default();
    }
    let Some(parent) = parent_of(fqn) else { return AncestorFacts::default() };
    let Some(pd) = decl_of(&parent) else { return AncestorFacts::default() };
    let above = ancestor_facts_of(&parent, decl_of, parent_of, symbols, depth + 1);
    AncestorFacts::of_parent(pd, above, &|n| is_instance_method(pd, &parent, symbols, n))
}

/// [`deferred_indices`] for the class `fqn`, with its ancestors' facts.
pub fn deferred_of<'a>(
    fqn: &str,
    decl_of: &dyn Fn(&str) -> Option<&'a ClassDecl>,
    parent_of: &dyn Fn(&str) -> Option<String>,
    symbols: &crate::SymbolTable,
) -> Vec<usize> {
    let Some(cd) = decl_of(fqn) else { return Vec::new() };
    let facts = ancestor_facts_of(fqn, decl_of, parent_of, symbols, 0);
    deferred_indices(&cd.fields, first_init_block(cd), facts, &|n| is_instance_member(cd, fqn, symbols, n))
}

/// Where a class's first `init` block starts, for [`deferred_indices`].
pub fn first_init_block(cd: &ClassDecl) -> Option<juxc_source::Span> {
    cd.init_blocks.iter().map(|b| b.span).min_by_key(|s| s.start)
}
