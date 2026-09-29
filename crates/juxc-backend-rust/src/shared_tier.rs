//! The SHARED collection tier of the representation selector (ERRATA
//! E147).
//!
//! A collection or array is a shared handle (JUX-LANG-V1 §6.5.1, §6.5.2). On
//! one thread that handle is `Rc<JuxCell<..>>`, which costs a plain refcount
//! and a borrow flag. A collection that a worker can reach needs a handle that
//! can cross threads: `JuxSync`, an `Arc` of the prelude's reentrant lock
//! around a `RefCell`, which answers the same `borrow` / `borrow_mut` surface,
//! so every rule the emitter follows for the single-threaded handle holds for
//! it unchanged. Because the two are one Jux collection written two ways, the
//! choice has to be the same wherever a value can flow, and that is what the
//! selector decides.
//!
//! **What reaches a worker.** A worker reaches an object of a worker-shared
//! class (JUX-ASYNC-ADDENDUM §18.2) and everything that object holds or hands
//! over: the type of each of its fields and properties, each parameter and
//! result of its methods and constructors, walked through type arguments,
//! nullable and array types, and the components of a record held there. A
//! worker also hands its result back (`await Worker.spawn(..)`), so the result
//! type of every `Worker.spawn` counts too.
//!
//! **Granularity.** A collection moves between names freely -- an argument, a
//! return, a field store, a generic parameter, an element of another
//! collection -- so the tier is decided per collection TYPE CONSTRUCTOR
//! (`Vec`, `HashMap`, ..., keyed by its Rust path): if any `Vec` can reach a
//! worker, every `Vec` in the program takes the shared tier. Arrays are
//! decided together for the same reason (a `T[]` parameter meets arrays of
//! every element type). That is coarser than a per-value flow analysis, and in
//! exchange there is no place where the two handles could meet: a caller's
//! list stored into the object is the SAME list, a getter hands back the SAME
//! list, and a write through either name is seen through the other, exactly
//! as in Java. A program with no worker-shared object and no worker result
//! holding a collection keeps the single-threaded handle everywhere, and pays
//! nothing.

use std::collections::HashSet;

use juxc_ast::{CompilationUnit, ReturnType, TopLevelDecl, TypeRef};
use juxc_tycheck::Ty;

use crate::RustEmitter;

impl RustEmitter {
    /// Decide the shared tier: fill [`RustEmitter::sync_coll_paths`] and
    /// [`RustEmitter::sync_arrays`]. Runs once `sync_class_fqns` is known.
    pub(crate) fn compute_shared_tiers(&mut self, units: &[CompilationUnit]) {
        self.sync_coll_paths.clear();
        self.sync_arrays = false;
        let saved_unit = self.current_unit_idx;
        let saved_class = self.enclosing_class.clone();
        let mut colls: HashSet<String> = HashSet::new();
        let mut arrays = false;
        // Records by bare name, so a record held by a shared object has its
        // components walked (its storage crosses with the object).
        let mut records: Vec<(usize, &juxc_ast::RecordDecl)> = Vec::new();
        for (idx, unit) in units.iter().enumerate() {
            for item in &unit.items {
                if let TopLevelDecl::Record(r) = item {
                    records.push((idx, r));
                }
            }
        }
        let mut seen_records: HashSet<String> = HashSet::new();
        let mut pending: Vec<(usize, TypeRef)> = Vec::new();
        if !self.sync_class_fqns.is_empty() {
            for (idx, unit) in units.iter().enumerate() {
                let pkg = unit
                    .package
                    .as_ref()
                    .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
                    .unwrap_or_default();
                for item in &unit.items {
                    let TopLevelDecl::Class(cd) = item else { continue };
                    let fqn = if pkg.is_empty() { cd.name.text.clone() } else { format!("{pkg}.{}", cd.name.text) };
                    if !self.sync_class_fqns.contains(&fqn) {
                        continue;
                    }
                    for f in &cd.fields {
                        if let Some(t) = &f.ty {
                            pending.push((idx, t.clone()));
                        }
                    }
                    for p in &cd.properties {
                        pending.push((idx, p.ty.clone()));
                    }
                    for m in &cd.methods {
                        for p in &m.params {
                            pending.push((idx, p.ty.clone()));
                        }
                        if let ReturnType::Type(t) | ReturnType::AsyncType(t) = &m.return_type {
                            pending.push((idx, t.clone()));
                        }
                    }
                    for c in &cd.constructors {
                        for p in &c.params {
                            pending.push((idx, p.ty.clone()));
                        }
                    }
                }
            }
        }
        while let Some((idx, t)) = pending.pop() {
            self.current_unit_idx = Some(idx);
            self.enclosing_class = None;
            let mut stack = vec![t];
            while let Some(t) = stack.pop() {
                if t.array_shape.is_some() {
                    arrays = true;
                }
                if let Some(path) = self.external_class_real_path(&t.name).filter(|_| self.collection_is_handle(&t.name)) {
                    colls.insert(path);
                }
                for a in &t.generic_args {
                    match a {
                        juxc_ast::GenericArg::Type(inner) => stack.push(inner.clone()),
                        juxc_ast::GenericArg::Wildcard(w) => {
                            if let Some(juxc_ast::WildcardBound::Extends(b) | juxc_ast::WildcardBound::Super(b)) = &w.bound {
                                stack.push(b.clone());
                            }
                        }
                    }
                }
                // A record's storage crosses with the object that holds it.
                if t.name.segments.len() == 1 {
                    let bare = t.name.segments[0].text.as_str();
                    for (ridx, r) in &records {
                        if r.name.text == bare && seen_records.insert(format!("{ridx}.{bare}")) {
                            for c in &r.components {
                                pending.push((*ridx, c.ty.clone()));
                            }
                        }
                    }
                }
            }
        }
        self.current_unit_idx = saved_unit;
        self.enclosing_class = saved_class;
        // What a worker hands back crosses too.
        let mut results: Vec<Ty> = Vec::new();
        for unit in units {
            crate::worker::for_each_worker_spawn_call(unit, &mut |c| {
                if let Some(t) = self.expr_types.get(&c.span) {
                    results.push(t.clone());
                }
            });
        }
        for t in results {
            self.shared_tier_walk_ty(&t, &mut colls, &mut arrays);
        }
        self.sync_coll_paths = colls;
        self.sync_arrays = arrays;
    }

    fn shared_tier_walk_ty(&self, t: &Ty, colls: &mut HashSet<String>, arrays: &mut bool) {
        match t {
            Ty::Nullable(inner) => self.shared_tier_walk_ty(inner, colls, arrays),
            Ty::Array { element, .. } => {
                *arrays = true;
                self.shared_tier_walk_ty(element, colls, arrays);
            }
            Ty::User { name, generic_args } => {
                if let Some(path) = self.collection_tier_key(name) {
                    colls.insert(path);
                }
                for a in generic_args {
                    self.shared_tier_walk_ty(a, colls, arrays);
                }
            }
            _ => {}
        }
    }

    /// The Rust path that keys a collection's tier, for a type NAME the
    /// checker resolved; `None` when the name is not a collection handle.
    pub(crate) fn collection_tier_key(&self, name: &str) -> Option<String> {
        if !self.collection_name_is_handle(name) {
            return None;
        }
        let bare = name.rsplit('.').next().unwrap_or(name);
        let sig = self
            .lookup_class_by_bare_or_fqn(name)
            .or_else(|| self.lookup_class_by_bare_or_fqn(bare))?;
        sig.rust_path.clone()
    }

    /// Whether the collection type `name` (a checked type's name) takes the
    /// shared tier.
    pub(crate) fn collection_name_is_sync(&self, name: &str) -> bool {
        !self.sync_coll_paths.is_empty()
            && self.collection_tier_key(name).is_some_and(|k| self.sync_coll_paths.contains(&k))
    }

    /// Whether the collection type written `qn` takes the shared tier.
    pub(crate) fn collection_qn_is_sync(&self, qn: &juxc_ast::QualifiedName) -> bool {
        !self.sync_coll_paths.is_empty()
            && self.external_class_real_path(qn).is_some_and(|k| self.sync_coll_paths.contains(&k))
    }

    /// The handle type's opening for a collection written `qn`.
    pub(crate) fn coll_handle_open(&self, qn: &juxc_ast::QualifiedName) -> &'static str {
        if self.collection_qn_is_sync(qn) {
            "crate::JuxSync<"
        } else {
            "crate::JuxArr<"
        }
    }

    /// The constructor, as a function path, that makes the handle for a value
    /// of the checked type `ty` (a collection or an array, possibly
    /// nullable): `crate::jux_arr` or `crate::JuxSync::new`.
    pub(crate) fn handle_ctor_for_ty(&self, ty: Option<&Ty>) -> &'static str {
        let sync = match ty {
            Some(Ty::Nullable(inner)) => return self.handle_ctor_for_ty(Some(inner)),
            Some(Ty::Array { element, .. }) => {
                let elem = match element.as_ref() {
                    Ty::User { name, .. } => name.rsplit('.').next().unwrap_or(name).to_string(),
                    _ => String::new(),
                };
                self.array_handle_is_sync(&elem)
            }
            Some(Ty::User { name, .. }) => self.collection_name_is_sync(name),
            _ => false,
        };
        if sync {
            "crate::JuxSync::new"
        } else {
            "crate::jux_arr"
        }
    }

    /// Whether the value of `e` is a collection or array handle in the shared
    /// tier.
    pub(crate) fn expr_handle_is_sync(&self, e: &juxc_ast::Expr) -> bool {
        self.expr_is_collection_handle(e)
            && self.handle_ctor_for_ty(self.narrowed_receiver_ty_of(e).as_ref()) != "crate::jux_arr"
    }

    /// [`Self::handle_ctor_for_ty`] for a collection type written `qn`.
    pub(crate) fn coll_ctor_for_qn(&self, qn: &juxc_ast::QualifiedName) -> &'static str {
        if self.collection_qn_is_sync(qn) {
            "crate::JuxSync::new"
        } else {
            "crate::jux_arr"
        }
    }

    /// The constructor for a `Vec` the runtime builds itself (`Task.all`, a
    /// parallel fan-out).
    pub(crate) fn std_vec_ctor(&self) -> &'static str {
        if self.sync_coll_paths.contains("std::vec::Vec") {
            "crate::JuxSync::new"
        } else {
            "crate::jux_arr"
        }
    }

    /// The argument an entry shim passes to `main(String[] args)`: the
    /// command line without the program name (§E.1.3), in the array tier.
    pub(crate) fn entry_args_expr(&self) -> String {
        let (open, close) = self.array_handle_new("String");
        format!("{open}std::env::args().skip(1).collect::<Vec<String>>(){close}")
    }
}
