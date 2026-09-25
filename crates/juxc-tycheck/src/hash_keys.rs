//! `E0933`: a type with no `operator hash` used as a hash key.
//!
//! `HashMap<K, V>` and `HashSet<K>` need `K` to have `operator==` and
//! `operator hash` (JUX-OPERATORS-ADDENDUM §O.3.1, §O.2.7). Most types do,
//! and the ones that don't (a function value, an array, a collection, a
//! floating-point value on its own, or a record holding one of those) used to
//! reach rustc as "the trait bounds were not satisfied" on the first
//! `insert`. This pass finds every place such a container type is WRITTEN
//! (field, parameter and return types, local declarations, `new` expressions)
//! and reports the key type once, naming the component that has no hash.
//!
//! Which containers need a hashed key, and what hashes, both come from
//! [`crate::hashing`], the table the backend reads too.

use std::collections::HashSet;

use juxc_ast::visit::{for_each_node, Node};
use juxc_ast::{Block, CompilationUnit, Expr, FnDecl, ReturnType, Stmt, TopLevelDecl, TypeParam, TypeRef};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

use crate::env::TypeEnv;
use crate::hashing::{key_bound_of, KeyBound};
use crate::symbol_table::SymbolTable;
use crate::ty::{ty_from_ref, Ty};

/// Report `E0933` for every hashed-key type in `unit` that has no hash.
///
/// `base` is the unit's environment (package and imports); each declaration
/// adds its own type parameters on top, so `HashSet<T>` reads `T` as the
/// parameter it is.
pub(crate) fn check_unit(unit: &CompilationUnit, base: &TypeEnv, symbols: &SymbolTable, out: &mut Vec<Diagnostic>) {
    let mut pass = Pass { symbols, out, reported: HashSet::new() };
    for item in &unit.items {
        pass.item(item, base, &[]);
    }
}

struct Pass<'a> {
    symbols: &'a SymbolTable,
    out: &'a mut Vec<Diagnostic>,
    /// Spans already reported, so a type written once is reported once.
    reported: HashSet<Span>,
}

impl Pass<'_> {
    /// Walk one declaration, with `outer` the type parameters of the
    /// declarations around it.
    fn item(&mut self, item: &TopLevelDecl, base: &TypeEnv, outer: &[&TypeParam]) {
        match item {
            TopLevelDecl::Function(f) => self.function(f, base, outer),
            TopLevelDecl::Class(c) => {
                let params: Vec<&TypeParam> = outer.iter().copied().chain(c.generic_params.iter()).collect();
                let env = scoped_env(base, &params);
                for f in &c.fields {
                    if let Some(ty) = &f.ty {
                        self.type_ref(ty, &env);
                    }
                }
                for p in &c.properties {
                    self.type_ref(&p.ty, &env);
                }
                for ctor in &c.constructors {
                    for p in &ctor.params {
                        self.type_ref(&p.ty, &env);
                    }
                    self.block(&ctor.body, &env);
                }
                for m in &c.methods {
                    self.function(m, base, &params);
                }
                for op in &c.operators {
                    for p in &op.params {
                        self.type_ref(&p.ty, &env);
                    }
                    self.return_type(&op.return_type, &env);
                    if let Some(body) = &op.body {
                        self.block(body, &env);
                    }
                }
                for b in c.init_blocks.iter().chain(&c.static_init_blocks).chain(&c.drop_blocks) {
                    self.block(b, &env);
                }
                for nested in &c.nested_types {
                    self.item(nested, base, &params);
                }
            }
            TopLevelDecl::Record(r) => {
                let params: Vec<&TypeParam> = outer.iter().copied().chain(r.generic_params.iter()).collect();
                let env = scoped_env(base, &params);
                for c in &r.components {
                    self.type_ref(&c.ty, &env);
                }
                for m in &r.methods {
                    self.function(m, base, &params);
                }
            }
            TopLevelDecl::Enum(e) => {
                let params: Vec<&TypeParam> = outer.iter().copied().chain(e.generic_params.iter()).collect();
                let env = scoped_env(base, &params);
                for v in &e.variants {
                    for p in &v.payload {
                        self.type_ref(&p.ty, &env);
                    }
                }
                for m in &e.methods {
                    self.function(m, base, &params);
                }
            }
            TopLevelDecl::Interface(i) => {
                let params: Vec<&TypeParam> = outer.iter().copied().chain(i.generic_params.iter()).collect();
                for m in &i.methods {
                    self.function(m, base, &params);
                }
            }
            _ => {}
        }
    }

    /// A function or method: its signature and its body.
    fn function(&mut self, f: &FnDecl, base: &TypeEnv, outer: &[&TypeParam]) {
        let params: Vec<&TypeParam> = outer.iter().copied().chain(f.generic_params.iter()).collect();
        let env = scoped_env(base, &params);
        for p in &f.params {
            self.type_ref(&p.ty, &env);
        }
        self.return_type(&f.return_type, &env);
        if let Some(body) = &f.body {
            self.block(body, &env);
        }
    }

    fn return_type(&mut self, rt: &ReturnType, env: &TypeEnv) {
        match rt {
            ReturnType::Type(t) | ReturnType::AsyncType(t) => self.type_ref(t, env),
            ReturnType::Void => {}
        }
    }

    /// The types written inside a body: declared locals and `new` expressions.
    fn block(&mut self, block: &Block, env: &TypeEnv) {
        let mut found: Vec<TypeRef> = Vec::new();
        for_each_node(block, &mut |node| match node {
            Node::Stmt(Stmt::VarDecl(v)) => {
                if let Some(ty) = &v.ty {
                    found.push(ty.clone());
                }
            }
            Node::Expr(Expr::NewObject(n)) => {
                // `new HashSet<Pt>()`: the written type is the class name
                // with its type arguments.
                found.push(TypeRef {
                    name: n.class_name.clone(),
                    generic_args: n.generic_args.iter().cloned().map(juxc_ast::GenericArg::Type).collect(),
                    nullable: false,
                    array_shape: None,
                    fn_shape: None,
                    ptr_depth: 0,
                    span: n.class_name.span,
                });
            }
            _ => {}
        });
        for ty in &found {
            self.type_ref(ty, env);
        }
    }

    /// Check `ty` and every type argument inside it.
    fn type_ref(&mut self, ty: &TypeRef, env: &TypeEnv) {
        let head = ty.name.segments.last().map(|s| s.text.as_str()).unwrap_or("");
        if key_bound_of(head) == Some(KeyBound::Hash) {
            if let Some(key) = ty.generic_args.first().and_then(|a| a.as_type()) {
                self.key(head, key, env);
            }
        }
        self.user_generic_instantiation(ty, env);
        for arg in &ty.generic_args {
            if let Some(inner) = arg.as_type() {
                self.type_ref(inner, env);
            }
        }
    }

    /// Report `E0933` for an instantiation of a USER generic whose parameter
    /// the declaration uses as a hashed container's key.
    ///
    /// This module's own rule is that "a type parameter is assumed hashable
    /// here: the backend bounds it (§T.2.1), and the instantiation is what gets
    /// checked". The instantiation half was never checked, so
    ///
    /// ```jux
    /// class Store<K> { public HashMap<K, int> m = new HashMap<K, int>(); }
    /// Store<double> s = new Store<double>();
    /// ```
    ///
    /// reached rustc as "`f64: Eq` / `f64: Hash` not satisfied" on `Store::new`,
    /// while the literal `HashMap<double, int>` one line away gave a clean
    /// E0933. Both say the same thing about the same program, and the one in
    /// Jux's words is the one the programmer can act on.
    ///
    /// The set of parameters checked is exactly the set the backend turns into
    /// an `Eq + Hash` bound: a parameter named, bare, as argument 0 of a
    /// `HashMap` / `HashSet` anywhere in the declaration's own field,
    /// property, constructor-parameter, method-parameter and return types.
    /// Keeping the two rules the same is the point, since a bound the backend
    /// adds that this pass does not know about is a leak by construction.
    fn user_generic_instantiation(&mut self, ty: &TypeRef, env: &TypeEnv) {
        if ty.generic_args.is_empty() {
            return;
        }
        // Resolve the head through the same path every other type reference
        // takes, so an imported or same-package name lands on its FQN and the
        // arguments come back as the types they are.
        let Ty::User { name: owner, generic_args: arg_tys } = ty_from_ref(ty, env, self.symbols)
        else {
            return;
        };
        let (params, declared) = match self.symbols.classes.get(&owner) {
            Some(sig) => (sig.generic_params.clone(), declared_types_of_class(sig)),
            None => match self.symbols.records.get(&owner) {
                Some(sig) => (sig.generic_params.clone(), declared_types_of_record(sig)),
                None => return,
            },
        };
        if params.is_empty() {
            return;
        }
        let keyed = hash_key_param_names(&params, &declared);
        if keyed.is_empty() {
            return;
        }
        let bare_owner = owner.rsplit('.').next().unwrap_or(&owner).to_string();
        for (i, p) in params.iter().enumerate() {
            if !keyed.contains(&p.name.text) {
                continue;
            }
            let Some(arg_ty) = arg_tys.get(i) else { continue };
            let Some(why) = self.symbols.hash_key_blocker(arg_ty) else { continue };
            // The WRITTEN argument's span, so the error points at the type the
            // programmer chose rather than at the class that constrains it.
            let span = ty
                .generic_args
                .get(i)
                .and_then(|a| a.as_type())
                .map(|t| t.span)
                .unwrap_or(ty.name.span);
            if !self.reported.insert(span) {
                continue;
            }
            self.out.push(
                Diagnostic::error(
                    code::Code::E0933_KeyHasNoHash,
                    format!(
                        "`{arg_ty}` cannot be the `{}` of `{bare_owner}`: {why}; `{bare_owner}` \
                         uses `{}` as a `HashMap` key, and a key needs `operator hash` (§O.3.1)",
                        p.name.text, p.name.text,
                    ),
                )
                .with_span(span),
            );
        }
    }

    /// Report `key` when it has no hash.
    fn key(&mut self, container: &str, key: &TypeRef, env: &TypeEnv) {
        let key_ty = ty_from_ref(key, env, self.symbols);
        let Some(why) = self.symbols.hash_key_blocker(&key_ty) else { return };
        if !self.reported.insert(key.span) {
            return;
        }
        self.out.push(
            Diagnostic::error(
                code::Code::E0933_KeyHasNoHash,
                format!(
                    "`{key_ty}` cannot be a `{container}` key: {why}; a key needs `operator hash` (§O.3.1)"
                ),
            )
            .with_span(key.span),
        );
    }
}

/// Every type a CLASS declaration writes down, which is the set the lowering
/// derives its added bounds from (§T.2.1): field, property, constructor-
/// parameter, method-parameter and method-return types.
///
/// Bodies are deliberately left out. The backend derives the key bounds from
/// the same list, and the two have to agree: a bound it adds from something
/// this does not look at would leak a rustc error, and a diagnostic this raises
/// for something it does not bound would reject a legal program.
fn declared_types_of_class(sig: &crate::symbol_table::ClassSig) -> Vec<TypeRef> {
    let mut out: Vec<TypeRef> = Vec::new();
    out.extend(sig.fields.values().map(|f| f.ty.clone()));
    out.extend(sig.properties.values().map(|p| p.ty.clone()));
    for ctor in &sig.constructors {
        out.extend(ctor.params.iter().map(|p| p.ty.clone()));
    }
    for m in sig.methods.values().chain(sig.method_overloads.values().flatten()) {
        out.extend(m.params.iter().map(|p| p.ty.clone()));
        if let ReturnType::Type(t) | ReturnType::AsyncType(t) = &m.return_type {
            out.push(t.clone());
        }
    }
    out
}

/// [`declared_types_of_class`] for a record: its components plus its methods.
fn declared_types_of_record(sig: &crate::symbol_table::RecordSig) -> Vec<TypeRef> {
    let mut out: Vec<TypeRef> = sig.components.iter().map(|c| c.ty.clone()).collect();
    for ctor in &sig.constructors {
        out.extend(ctor.params.iter().map(|p| p.ty.clone()));
    }
    for m in sig.methods.values() {
        out.extend(m.params.iter().map(|p| p.ty.clone()));
        if let ReturnType::Type(t) | ReturnType::AsyncType(t) = &m.return_type {
            out.push(t.clone());
        }
    }
    out
}

/// Which of `params` the lowering bounds with `Eq + Hash` because one of
/// `declared` uses it, bare, as a `HashMap` / `HashSet` key.
///
/// The walk descends through generic arguments, so `Vec<HashMap<K, int>>` pins
/// `K` exactly as a bare `HashMap<K, int>` does (§T.2.1).
fn hash_key_param_names(params: &[TypeParam], declared: &[TypeRef]) -> HashSet<String> {
    let names: HashSet<&str> = params.iter().map(|p| p.name.text.as_str()).collect();
    let mut out: HashSet<String> = HashSet::new();
    fn walk(ty: &TypeRef, names: &HashSet<&str>, out: &mut HashSet<String>) {
        let head = ty.name.segments.last().map(|s| s.text.as_str()).unwrap_or("");
        if key_bound_of(head) == Some(KeyBound::Hash) {
            if let Some(key) = ty.generic_args.first().and_then(|a| a.as_type()) {
                if key.generic_args.is_empty() && key.name.segments.len() == 1 {
                    let k = key.name.segments[0].text.as_str();
                    if names.contains(k) {
                        out.insert(k.to_string());
                    }
                }
            }
        }
        for arg in &ty.generic_args {
            if let Some(inner) = arg.as_type() {
                walk(inner, names, out);
            }
        }
    }
    for ty in declared {
        walk(ty, &names, &mut out);
    }
    out
}

/// An environment for one declaration: the unit's package and imports, plus
/// the type parameters in scope there.
fn scoped_env(base: &TypeEnv, params: &[&TypeParam]) -> TypeEnv {
    let mut env = TypeEnv::new();
    env.current_package = base.current_package.clone();
    env.unqualified = base.unqualified.clone();
    for p in params {
        env.add_generic_param(&p.name.text);
    }
    env
}
