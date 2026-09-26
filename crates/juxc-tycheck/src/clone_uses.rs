//! `E0457`: a use of a generic declaration that needs a capability its type
//! argument lacks (GAPS.md gap 2, ERRATA E120).
//!
//! [`crate::clone_needs`] decides where each generic declaration's
//! `Clone + Debug` goes: which parameters keep it for the whole declaration,
//! and which members state it for the parameters they copy. The backend writes
//! exactly those bounds. This pass checks every USE against the same table, so
//! a program the checker accepts is one whose emitted bounds hold, and the
//! refusal is a Jux diagnostic naming the member, the parameter, the argument
//! and what the argument cannot do, instead of a rustc error about a bound the
//! program never wrote.
//!
//! What counts as a use, and what it asks:
//!
//! - a method call, constructor call, property access or operator on a value
//!   of a generic declaration: whatever that member's `where` clause names,
//!   read through the receiver's type arguments (a member inherited from an
//!   ancestor or an interface maps through the `extends` / `implements`
//!   chain);
//! - a read of a field whose type mentions a relaxed parameter: the value is
//!   copied out of the object;
//! - a written or inferred type `D<X>`: a parameter of `D` that keeps the
//!   baseline asks `Clone + Debug` of `X`, and a relaxed parameter of a record
//!   or enum asks `Debug` (its string form prints it);
//! - an `extends B<X>` / `implements I<X>` clause with a concrete `X`: the class
//!   gets a copy of every member it inherits, so each one's needs apply.
//!
//! Capabilities: every Jux class and interface value is a handle that copies by
//! refcount and has a debug form (ERRATA E107), and so is every primitive,
//! `String` and array; a record or enum copies when its components do; a
//! foreign type has what its stub's `@RustClone` / `@RustDebug` markers say
//! (ERRATA E97), for its arguments too; a function value copies but has no
//! debug form. A type parameter in scope is assumed capable: the enclosing
//! member states the bound itself.

use std::collections::{HashMap, HashSet};

use juxc_ast::{
    BinaryOp, Block, CompilationUnit, Expr, FnDecl, OperatorKind, ReturnType, Stmt, TopLevelDecl, TypeParam,
    TypeRef,
};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

use crate::clone_needs::MemberNeeds;
use crate::env::TypeEnv;
use crate::symbol_table::{MethodSig, SymbolTable};
use crate::ty::Ty;

/// Check every use in `units`. Each diagnostic comes with the index of the
/// unit it belongs to.
pub fn check_uses(
    units: &[CompilationUnit],
    symbols: &SymbolTable,
    expr_types: &HashMap<Span, Ty>,
) -> Vec<(usize, Diagnostic)> {
    let mut c = UseChecker {
        symbols,
        expr_types,
        out: Vec::new(),
        seen: HashSet::new(),
        unit: 0,
        class: None,
        generics: Vec::new(),
    };
    for (i, unit) in units.iter().enumerate() {
        let pkg = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
            .unwrap_or_default();
        // A foreign stub and the core library are compiled for every program
        // alike; their uses are not the program's.
        if unit.is_external || pkg == "jux" || pkg.starts_with("jux.") {
            continue;
        }
        c.unit = i;
        for item in &unit.items {
            c.item(item, &pkg);
        }
    }
    c.out
}

/// A type as the program writes it: a class by its simple name, as in the
/// rest of the checker's messages (`File`, not `rust.std.File`).
struct Show<'t>(&'t Ty);

impl std::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Ty::User { name, generic_args } if name != juxc_ast::TUPLE_SENTINEL => {
                let bare = name.rsplit('.').next().unwrap_or(name);
                f.write_str(&crate::ty::nested_type_spelling(bare))?;
                if !generic_args.is_empty() {
                    f.write_str("<")?;
                    for (i, a) in generic_args.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{}", Show(a))?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            }
            Ty::Nullable(inner) => write!(f, "{}?", Show(inner)),
            Ty::Array { element, kind } => match kind {
                crate::ty::ArrayKind::Fixed => write!(f, "{}[N]", Show(element)),
                crate::ty::ArrayKind::Dynamic => write!(f, "{}[]", Show(element)),
            },
            Ty::Fn { params, return_type, .. } => {
                f.write_str("(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", Show(p))?;
                }
                write!(f, ") -> {}", Show(return_type))
            }
            other => write!(f, "{other}"),
        }
    }
}

/// What a type argument can do.
#[derive(Clone, Copy, Debug)]
struct Caps {
    clone: bool,
    debug: bool,
}

impl Caps {
    const ALL: Caps = Caps { clone: true, debug: true };

    fn and(self, o: Caps) -> Caps {
        Caps { clone: self.clone && o.clone, debug: self.debug && o.debug }
    }
}

fn has_marker(annotations: &[juxc_ast::Annotation], marker: &str) -> bool {
    annotations
        .iter()
        .any(|a| a.name.segments.len() == 1 && a.name.segments[0].text.eq_ignore_ascii_case(marker))
}

/// Whether `ty` mentions any type parameter.
fn has_param(ty: &Ty) -> bool {
    match ty {
        Ty::Param(_) => true,
        Ty::Nullable(t) => has_param(t),
        Ty::Array { element, .. } => has_param(element),
        Ty::User { generic_args, .. } => generic_args.iter().any(has_param),
        Ty::Fn { params, return_type, .. } => params.iter().any(has_param) || has_param(return_type),
        _ => false,
    }
}

fn binary_kind(op: BinaryOp) -> Option<OperatorKind> {
    Some(match op {
        BinaryOp::Add => OperatorKind::Plus,
        BinaryOp::Sub => OperatorKind::Minus,
        BinaryOp::Mul => OperatorKind::Mul,
        BinaryOp::Div => OperatorKind::Div,
        BinaryOp::Rem => OperatorKind::Rem,
        BinaryOp::BitAnd => OperatorKind::BitAnd,
        BinaryOp::BitOr => OperatorKind::BitOr,
        BinaryOp::BitXor => OperatorKind::BitXor,
        BinaryOp::Shl => OperatorKind::Shl,
        BinaryOp::Shr => OperatorKind::Shr,
        _ => return None,
    })
}

/// How a member is named in a message.
enum MemberWord {
    /// `Cell<File>.get()`, which returns the bare parameter or not.
    Method { name: String, returns: Vec<String> },
    Ctor,
    Property(String),
    Operator(OperatorKind),
    Field(String),
}

struct UseChecker<'a> {
    symbols: &'a SymbolTable,
    expr_types: &'a HashMap<Span, Ty>,
    out: Vec<(usize, Diagnostic)>,
    seen: HashSet<(Span, String)>,
    unit: usize,
    /// The declaration whose body is being walked.
    class: Option<String>,
    /// The generic parameters of the member being walked.
    generics: Vec<TypeParam>,
}

impl UseChecker<'_> {
    // ---- declarations -------------------------------------------------------

    fn item(&mut self, item: &TopLevelDecl, pkg: &str) {
        let fqn = |name: &str| if pkg.is_empty() { name.to_string() } else { format!("{pkg}.{name}") };
        match item {
            TopLevelDecl::Function(f) => {
                self.class = None;
                self.function(f);
            }
            TopLevelDecl::Class(c) => {
                self.class = Some(fqn(&c.name.text));
                self.generics.clear();
                if let Some(ext) = &c.extends {
                    self.written(ext);
                    self.inherits(ext, false);
                }
                for i in &c.implements {
                    self.written(i);
                    self.inherits(i, true);
                }
                for f in &c.fields {
                    if let Some(t) = &f.ty {
                        self.written(t);
                    }
                    if let Some(d) = &f.default {
                        self.expr_tree(d);
                    }
                }
                for ctor in &c.constructors {
                    for p in &ctor.params {
                        self.written(&p.ty);
                    }
                    self.block(&ctor.body);
                }
                for m in &c.methods {
                    self.function(m);
                }
                for op in &c.operators {
                    for p in &op.params {
                        self.written(&p.ty);
                    }
                    if let Some(b) = &op.body {
                        self.block(b);
                    }
                }
                for b in c.init_blocks.iter().chain(&c.static_init_blocks).chain(&c.drop_blocks) {
                    self.block(b);
                }
            }
            TopLevelDecl::Record(r) => {
                self.class = Some(fqn(&r.name.text));
                self.generics.clear();
                for i in &r.implements {
                    self.written(i);
                    self.inherits(i, true);
                }
                for comp in &r.components {
                    self.written(&comp.ty);
                }
                if let Some(c) = &r.compact_ctor {
                    self.block(&c.body);
                }
                for ctor in &r.constructors {
                    for p in &ctor.params {
                        self.written(&p.ty);
                    }
                    self.block(&ctor.body);
                }
                for m in &r.methods {
                    self.function(m);
                }
                for op in &r.operators {
                    if let Some(b) = &op.body {
                        self.block(b);
                    }
                }
            }
            TopLevelDecl::Enum(e) => {
                self.class = Some(fqn(&e.name.text));
                self.generics.clear();
                for i in &e.implements {
                    self.written(i);
                    self.inherits(i, true);
                }
                for v in &e.variants {
                    for p in &v.payload {
                        self.written(&p.ty);
                    }
                }
                for m in &e.methods {
                    self.function(m);
                }
                for op in &e.operators {
                    if let Some(b) = &op.body {
                        self.block(b);
                    }
                }
            }
            TopLevelDecl::Interface(i) => {
                self.class = Some(fqn(&i.name.text));
                self.generics.clear();
                for e in &i.extends {
                    self.written(e);
                }
                for m in &i.methods {
                    self.function(m);
                }
            }
            _ => {}
        }
        self.class = None;
    }

    fn function(&mut self, f: &FnDecl) {
        self.generics = f.generic_params.clone();
        for p in &f.params {
            self.written(&p.ty);
        }
        if let ReturnType::Type(t) | ReturnType::AsyncType(t) = &f.return_type {
            self.written(t);
        }
        if let Some(b) = &f.body {
            self.block(b);
        }
        self.generics.clear();
    }

    // ---- bodies ---------------------------------------------------------------

    fn block(&mut self, b: &Block) {
        let mut skip = Skip::default();
        juxc_ast::visit::for_each_node(b, &mut |n| skip.note(n));
        juxc_ast::visit::for_each_node(b, &mut |n| self.node(n, &skip));
    }

    fn expr_tree(&mut self, e: &Expr) {
        let mut skip = Skip::default();
        juxc_ast::visit::for_each_node_in(e, &mut |n| skip.note(n));
        juxc_ast::visit::for_each_node_in(e, &mut |n| self.node(n, &skip));
    }

    fn node(&mut self, n: juxc_ast::visit::Node<'_>, skip: &Skip) {
        match n {
            juxc_ast::visit::Node::Stmt(st) => match st {
                Stmt::VarDecl(v) => {
                    if let Some(t) = &v.ty {
                        self.written(t);
                    }
                }
                Stmt::ForEach(f) => {
                    if let Some(t) = &f.var_type {
                        self.written(t);
                    }
                }
                Stmt::Try(t) => {
                    for c in &t.catches {
                        self.written(&c.ty);
                    }
                }
                Stmt::Assign(a) => {
                    // A write to a property runs its setter.
                    if let Expr::Field(fe) = &a.target {
                        self.property_write(fe);
                    }
                }
                _ => {}
            },
            juxc_ast::visit::Node::Expr(e) => self.expr(e, skip),
        }
    }

    fn expr(&mut self, e: &Expr, skip: &Skip) {
        match e {
            Expr::Call(c) => {
                for t in &c.explicit_generic_args {
                    self.written(t);
                }
                if let Expr::Field(fe) = &*c.callee {
                    self.method_call(fe, c.span, c.args.len());
                }
            }
            Expr::NewObject(n) => {
                for t in &n.generic_args {
                    self.written(t);
                }
                self.construct(n.span);
            }
            Expr::Binary(b) => {
                if let Some(kind) = binary_kind(b.op) {
                    self.operator(&b.left, kind, b.span);
                }
            }
            Expr::Field(fe) => {
                let span = fe.span;
                if !skip.callees.contains(&span) && !skip.targets.contains(&span) {
                    self.field_read(fe, skip.receivers.contains(&span));
                }
            }
            Expr::Cast(c) => self.written(&c.ty),
            Expr::TypeTest(t) => self.written(&t.ty),
            Expr::NewArray(n) => self.written(&n.element_type),
            Expr::Lambda(l) => {
                for p in &l.params {
                    if let Some(t) = &p.ty {
                        self.written(t);
                    }
                }
            }
            _ => {}
        }
    }

    // ---- types and capabilities ----------------------------------------------

    /// Lower a type the program wrote where the walk is.
    fn lower(&self, ty: &TypeRef) -> Ty {
        let mut env = TypeEnv::new();
        if let Some(ctx) = self.symbols.units.get(self.unit) {
            env.current_package.clone_from(&ctx.package);
            env.unqualified.clone_from(&ctx.unqualified);
        }
        if let Some(c) = &self.class {
            env.current_class = Some(c.clone());
            for tp in self.decl_params(c) {
                env.add_generic_param(&tp.name.text);
            }
        }
        for tp in &self.generics {
            env.add_generic_param(&tp.name.text);
        }
        crate::ty::ty_from_ref(ty, &env, self.symbols)
    }

    /// The FQN a checker type name keys.
    fn decl_fqn(&self, name: &str) -> Option<String> {
        let s = self.symbols;
        let known = |n: &str| {
            s.classes.contains_key(n) || s.records.contains_key(n) || s.enums.contains_key(n) || s.interfaces.contains_key(n)
        };
        if known(name) {
            return Some(name.to_string());
        }
        s.find_fqn_by_bare(name.rsplit('.').next().unwrap_or(name)).filter(|f| known(f))
    }

    fn decl_params(&self, fqn: &str) -> Vec<TypeParam> {
        let s = self.symbols;
        if let Some(c) = s.classes.get(fqn) {
            return c.generic_params.clone();
        }
        if let Some(r) = s.records.get(fqn) {
            return r.generic_params.clone();
        }
        if let Some(e) = s.enums.get(fqn) {
            return e.generic_params.clone();
        }
        if let Some(i) = s.interfaces.get(fqn) {
            return i.generic_params.clone();
        }
        Vec::new()
    }

    /// Whether `fqn` is a Jux (non-foreign) generic declaration.
    fn is_jux_decl(&self, fqn: &str) -> bool {
        let s = self.symbols;
        s.classes.get(fqn).is_some_and(|c| !c.is_external)
            || s.records.contains_key(fqn)
            || s.enums.get(fqn).is_some_and(|e| !e.is_external)
            || s.interfaces.get(fqn).is_some_and(|i| !i.is_external)
    }

    fn caps(&self, ty: &Ty, depth: usize) -> Caps {
        if depth > 8 {
            return Caps::ALL;
        }
        match ty {
            Ty::Nullable(t) => self.caps(t, depth + 1),
            Ty::Array { element, .. } => Caps { clone: true, debug: self.caps(element, depth + 1).debug },
            Ty::Fn { .. } => Caps { clone: true, debug: false },
            Ty::User { name, generic_args } => {
                let args = generic_args.iter().fold(Caps::ALL, |acc, a| acc.and(self.caps(a, depth + 1)));
                if name == juxc_ast::TUPLE_SENTINEL {
                    return args;
                }
                let Some(fqn) = self.decl_fqn(name) else { return Caps::ALL };
                let s = self.symbols;
                if let Some(c) = s.classes.get(&fqn) {
                    if c.is_external {
                        return Caps {
                            clone: has_marker(&c.annotations, "rustclone") && args.clone,
                            debug: has_marker(&c.annotations, "rustdebug") && args.debug,
                        };
                    }
                    // A class is a shared handle: copying it is a refcount
                    // bump and its debug form is its string form.
                    return Caps::ALL;
                }
                if s.interfaces.contains_key(&fqn) {
                    return Caps::ALL;
                }
                if let Some(e) = s.enums.get(&fqn) {
                    if e.is_external {
                        return Caps {
                            clone: has_marker(&e.annotations, "rustclone") && args.clone,
                            debug: has_marker(&e.annotations, "rustdebug") && args.debug,
                        };
                    }
                    let mut clone = true;
                    for v in e.variants.values() {
                        for p in &v.payload {
                            let t = crate::ty::substitute(
                                &crate::ty::lower_member_type(p, &fqn, s),
                                &e.generic_params,
                                generic_args,
                            );
                            clone &= self.caps(&t, depth + 1).clone;
                        }
                    }
                    return Caps { clone, debug: args.debug };
                }
                if let Some(r) = s.records.get(&fqn) {
                    let mut clone = true;
                    for comp in &r.components {
                        let t = crate::ty::substitute(
                            &crate::ty::lower_member_type(&comp.ty, &fqn, s),
                            &r.generic_params,
                            generic_args,
                        );
                        clone &= self.caps(&t, depth + 1).clone;
                    }
                    return Caps { clone, debug: args.debug };
                }
                Caps::ALL
            }
            _ => Caps::ALL,
        }
    }

    // ---- reporting --------------------------------------------------------------

    /// Check that each of `params` (of declaration `owner`, bound to `args`)
    /// has `Clone + Debug`, reporting the first that does not.
    fn require(
        &mut self,
        site: Span,
        recv: &Ty,
        owner: &str,
        params: &[String],
        args: &HashMap<String, Ty>,
        word: &MemberWord,
    ) {
        self.require_caps(site, recv, owner, params, args, word, true);
    }

    /// [`Self::require`], or `Debug` alone when `need_clone` is false (a
    /// record's or enum's relaxed parameter, which its impls keep `Debug` on).
    fn require_caps(
        &mut self,
        site: Span,
        recv: &Ty,
        owner: &str,
        params: &[String],
        args: &HashMap<String, Ty>,
        word: &MemberWord,
        need_clone: bool,
    ) {
        for p in params {
            let Some(arg) = args.get(p) else { continue };
            if has_param(arg) {
                continue;
            }
            let mut caps = self.caps(arg, 0);
            if !need_clone {
                caps.clone = true;
            }
            if caps.clone && caps.debug {
                continue;
            }
            if !self.seen.insert((site, p.clone())) {
                continue;
            }
            let owner_bare = crate::ty::nested_type_spelling(owner.rsplit('.').next().unwrap_or(owner)).into_owned();
            let what = match word {
                MemberWord::Method { name, .. } => format!("`{}.{name}()`", Show(recv)),
                MemberWord::Ctor => format!("the constructor of `{}`", Show(recv)),
                MemberWord::Property(name) => format!("the property `{}.{name}`", Show(recv)),
                MemberWord::Operator(k) => format!("`{}` on `{}`", crate::clone_needs::operator_label(*k), Show(recv)),
                MemberWord::Field(name) => format!("reading `{}.{name}`", Show(recv)),
            };
            let message = if !caps.clone {
                let does = match word {
                    MemberWord::Method { returns, .. } if returns.contains(p) => format!("returns a copy of `{p}`"),
                    MemberWord::Field(_) => format!("copies the `{p}` out of the object"),
                    _ => format!("copies its `{p}`"),
                };
                format!("`{}` cannot be copied, and {what} {does}", Show(arg))
            } else {
                format!("`{}` has no debug form, and {what} needs one for `{p}`", Show(arg))
            };
            let help = if !caps.clone {
                format!(
                    "a member of `{owner_bare}` that only stores or moves its `{p}` works with `{}`; this one needs a type that can be copied",
                    Show(arg)
                )
            } else {
                format!("a function value has no debug form; `{owner_bare}` needs one for `{p}` here")
            };
            let d = Diagnostic::error(code::Code::E0457_TypeArgumentLacksCapability, message)
                .with_span(site)
                .with_help(help);
            self.out.push((self.unit, d));
            return;
        }
    }

    /// Report `ty` (written or inferred at `site`) where one of its
    /// declarations asks of an argument what that argument lacks.
    fn instantiation(&mut self, ty: &Ty, site: Span, depth: usize) {
        if depth > 8 {
            return;
        }
        match ty {
            Ty::Nullable(t) => self.instantiation(t, site, depth + 1),
            Ty::Array { element, .. } => self.instantiation(element, site, depth + 1),
            Ty::Fn { params, return_type, .. } => {
                for p in params {
                    self.instantiation(p, site, depth + 1);
                }
                self.instantiation(return_type, site, depth + 1);
            }
            Ty::User { name, generic_args } => {
                for a in generic_args {
                    self.instantiation(a, site, depth + 1);
                }
                let Some(fqn) = self.decl_fqn(name) else { return };
                // Only a class's struct and an interface's trait carry the
                // bounds in the TYPE; a record or enum type is well-formed
                // with any argument, and its members are checked where they
                // are used (`Self::value_impl`).
                let type_level = self.symbols.classes.get(&fqn).is_some_and(|c| !c.is_external)
                    || self.symbols.interfaces.get(&fqn).is_some_and(|i| !i.is_external);
                if !type_level {
                    return;
                }
                let params = self.decl_params(&fqn);
                let needs = &self.symbols.clone_needs;
                for (tp, arg) in params.iter().zip(generic_args) {
                    if tp.is_const() || has_param(arg) {
                        continue;
                    }
                    let p = &tp.name.text;
                    let caps = self.caps(arg, 0);
                    if needs.is_relaxed(&fqn, p) || (caps.clone && caps.debug) {
                        continue;
                    }
                    if !self.seen.insert((site, format!("{fqn}.{p}"))) {
                        continue;
                    }
                    let bare = crate::ty::nested_type_spelling(fqn.rsplit('.').next().unwrap_or(&fqn)).into_owned();
                    let (lack, cap) = if caps.clone {
                        ("has no debug form", "to have a debug form")
                    } else {
                        ("cannot be copied", "to be copyable")
                    };
                    let why = needs
                        .baseline_reason(&fqn, p)
                        .map(str::to_string)
                        .unwrap_or_else(|| "the declaration copies it".to_string());
                    let d = Diagnostic::error(
                        code::Code::E0457_TypeArgumentLacksCapability,
                        format!(
                            "`{}`: `{}` {lack}, and `{bare}` needs its `{p}` {cap} because {why}",
                            Show(ty),
                            Show(arg)
                        ),
                    )
                    .with_span(site)
                    .with_help(format!(
                        "a generic class asks nothing of a parameter it only stores; `{bare}` uses its `{p}` in a way that copies it"
                    ));
                    self.out.push((self.unit, d));
                }
            }
            _ => {}
        }
    }

    fn written(&mut self, ty: &TypeRef) {
        if ty.generic_args.is_empty() && ty.array_shape.is_none() && ty.fn_shape.is_none() {
            return;
        }
        let lowered = self.lower(ty);
        self.instantiation(&lowered, ty.span, 0);
    }

    // ---- member uses --------------------------------------------------------------

    /// The receiver's declaration and type arguments, when it is a value of a
    /// generic Jux declaration.
    fn receiver(&self, e: &Expr) -> Option<(Ty, String, Vec<Ty>)> {
        let ty = self.expr_types.get(&crate::check::expr_span_pub(e))?;
        let inner = match ty {
            Ty::Nullable(t) => t.as_ref(),
            t => t,
        };
        let Ty::User { name, generic_args } = inner else { return None };
        if generic_args.is_empty() {
            return None;
        }
        let fqn = self.decl_fqn(name)?;
        self.is_jux_decl(&fqn).then(|| (inner.clone(), fqn, generic_args.clone()))
    }

    /// `owner`'s parameters bound for a receiver of `recv<recv_args>`.
    fn owner_args(&self, recv: &str, recv_args: &[Ty], owner: &str) -> Option<HashMap<String, Ty>> {
        let zip = |params: Vec<TypeParam>, args: &[Ty]| -> HashMap<String, Ty> {
            params.iter().zip(args).map(|(p, a)| (p.name.text.clone(), a.clone())).collect()
        };
        if recv == owner {
            return Some(zip(self.decl_params(recv), recv_args));
        }
        let s = self.symbols;
        if s.classes.contains_key(recv) && s.classes.contains_key(owner) {
            let (params, args) = crate::ty::compose_extends_substitution(recv, recv_args, owner, s)?;
            return Some(zip(params, &args));
        }
        if s.interfaces.contains_key(owner) {
            return self.iface_args(recv, recv_args, owner, 0);
        }
        None
    }

    /// The arguments a declaration passes an interface it implements,
    /// directly, through its ancestors, or through another interface.
    fn iface_args(&self, holder: &str, args: &[Ty], iface: &str, depth: usize) -> Option<HashMap<String, Ty>> {
        if depth > 16 {
            return None;
        }
        let s = self.symbols;
        let params = self.decl_params(holder);
        let supers: Vec<TypeRef> = if let Some(c) = s.classes.get(holder) {
            c.implements.clone()
        } else if let Some(r) = s.records.get(holder) {
            r.implements.clone()
        } else if let Some(e) = s.enums.get(holder) {
            e.implements.clone()
        } else if let Some(i) = s.interfaces.get(holder) {
            i.extends.clone()
        } else {
            Vec::new()
        };
        for sup in &supers {
            let lowered = crate::ty::substitute(&crate::ty::lower_member_type(sup, holder, s), &params, args);
            let Ty::User { name, generic_args } = lowered else { continue };
            let Some(fqn) = self.decl_fqn(&name) else { continue };
            if fqn == iface {
                let iparams = self.decl_params(&fqn);
                return Some(iparams.iter().zip(&generic_args).map(|(p, a)| (p.name.text.clone(), a.clone())).collect());
            }
            if let Some(found) = self.iface_args(&fqn, &generic_args, iface, depth + 1) {
                return Some(found);
            }
        }
        // Through the parent class.
        if let Some(c) = s.classes.get(holder) {
            if let Some(parent) = &c.extends_fqn {
                if let Some((_, pargs)) = crate::ty::compose_extends_substitution(holder, args, parent, s) {
                    return self.iface_args(parent, &pargs, iface, depth + 1);
                }
            }
        }
        None
    }

    /// The member's needs, or every relaxed parameter of `owner` when the
    /// table has no entry (a synthesized member).
    fn needs_of(&self, span: Option<Span>, owner: &str) -> MemberNeeds {
        let table = &self.symbols.clone_needs;
        match span.and_then(|s| table.member(s)) {
            Some(n) => n.clone(),
            None => MemberNeeds {
                owner: owner.to_string(),
                name: String::new(),
                params: table.relaxed_params(owner).to_vec(),
                touches_self: true,
            },
        }
    }

    /// Apply a member's needs to a use through a receiver of
    /// `recv_fqn<recv_args>`.
    fn apply(&mut self, site: Span, recv: &Ty, recv_fqn: &str, recv_args: &[Ty], needs: &MemberNeeds, word: &MemberWord) {
        let owner = needs.owner.clone();
        self.value_impl(site, recv, recv_fqn, recv_args, word);
        let Some(args) = self.owner_args(recv_fqn, recv_args, &owner) else { return };
        self.require(site, recv, &owner, &needs.params, &args, word);
        // A copy of the member in the receiver's class hands `this` on: every
        // relaxed parameter of that class is needed there.
        if needs.touches_self && owner != recv_fqn {
            let own = self.symbols.clone_needs.relaxed_params(recv_fqn).to_vec();
            let own_args = self.owner_args(recv_fqn, recv_args, recv_fqn).unwrap_or_default();
            self.require(site, recv, recv_fqn, &own, &own_args, word);
        }
    }

    /// Whether `fqn` is a Jux record or enum: a value type whose members all
    /// sit in impls that carry its parameters' bounds.
    fn is_value_decl(&self, fqn: &str) -> bool {
        self.symbols.records.contains_key(fqn) || self.symbols.enums.get(fqn).is_some_and(|e| !e.is_external)
    }

    /// Every member of a record or enum lives in an impl that bounds its
    /// parameters: `Clone + Debug` for one that keeps the baseline, `Debug`
    /// for a relaxed one (its string form prints it).
    fn value_impl(&mut self, site: Span, recv: &Ty, recv_fqn: &str, recv_args: &[Ty], word: &MemberWord) {
        if !self.is_value_decl(recv_fqn) {
            return;
        }
        let args = self.owner_args(recv_fqn, recv_args, recv_fqn).unwrap_or_default();
        let (relaxed, baseline): (Vec<String>, Vec<String>) = self
            .decl_params(recv_fqn)
            .iter()
            .filter(|p| !p.is_const())
            .map(|p| p.name.text.clone())
            .partition(|p| self.symbols.clone_needs.is_relaxed(recv_fqn, p));
        self.require_caps(site, recv, recv_fqn, &baseline, &args, word, true);
        self.require_caps(site, recv, recv_fqn, &relaxed, &args, word, false);
    }

    /// The needs of a member the table has no entry for: nothing moved to it
    /// for a class or interface (its parameters are checked on the type), the
    /// impl's own bounds for a record or enum.
    fn untabled(&self, fqn: &str) -> Option<MemberNeeds> {
        self.is_value_decl(fqn).then(|| MemberNeeds {
            owner: fqn.to_string(),
            name: String::new(),
            params: Vec::new(),
            touches_self: false,
        })
    }

    /// The signature a call `recv.name(..)` with `argc` arguments reaches.
    fn method_sig(&self, recv_fqn: &str, name: &str, call: Span, argc: usize) -> Option<MethodSig> {
        let s = self.symbols;
        if s.classes.contains_key(recv_fqn) {
            let group = s.merged_method_overloads(recv_fqn, name);
            if !group.is_empty() {
                if group.len() == 1 {
                    return group.into_iter().next();
                }
                if let Some(k) = s.method_selections.get(&call) {
                    return group.get(*k).cloned();
                }
                let same: Vec<MethodSig> = group.into_iter().filter(|m| m.params.len() == argc).collect();
                return (same.len() == 1).then(|| same[0].clone());
            }
            // An interface default the class inherits.
            return self.iface_method(recv_fqn, name, 0);
        }
        if let Some(i) = s.interfaces.get(recv_fqn) {
            return i.methods.get(name).cloned().or_else(|| self.iface_method(recv_fqn, name, 0));
        }
        if let Some(r) = s.records.get(recv_fqn) {
            return r.methods.get(name).cloned();
        }
        if let Some(e) = s.enums.get(recv_fqn) {
            return e.methods.get(name).cloned();
        }
        None
    }

    fn iface_method(&self, holder: &str, name: &str, depth: usize) -> Option<MethodSig> {
        if depth > 16 {
            return None;
        }
        let s = self.symbols;
        let supers: Vec<TypeRef> = if let Some(c) = s.classes.get(holder) {
            if let Some(p) = &c.extends_fqn {
                if let Some(m) = self.iface_method(p, name, depth + 1) {
                    return Some(m);
                }
            }
            c.implements.clone()
        } else if let Some(i) = s.interfaces.get(holder) {
            i.extends.clone()
        } else {
            Vec::new()
        };
        for sup in &supers {
            let Some(fqn) = self.decl_fqn(&sup.name.segments.iter().map(|x| x.text.as_str()).collect::<Vec<_>>().join("."))
            else {
                continue;
            };
            if let Some(i) = s.interfaces.get(&fqn) {
                if let Some(m) = i.methods.get(name) {
                    return Some(m.clone());
                }
                if let Some(m) = self.iface_method(&fqn, name, depth + 1) {
                    return Some(m);
                }
            }
        }
        None
    }

    fn method_call(&mut self, fe: &juxc_ast::FieldExpr, call: Span, argc: usize) {
        let Some((recv, fqn, args)) = self.receiver(&fe.object) else { return };
        let name = fe.field.text.clone();
        // A record component read through its accessor copies the component.
        if let Some(r) = self.symbols.records.get(&fqn) {
            if argc == 0 && !r.methods.contains_key(&name) {
                if let Some(comp) = r.components.iter().find(|c| c.name == name) {
                    let ty = comp.ty.clone();
                    self.component_read(call, &recv, &fqn, &args, &name, &ty);
                    return;
                }
            }
        }
        let Some(sig) = self.method_sig(&fqn, &name, call, argc) else { return };
        let Some(needs) = self.symbols.clone_needs.member(sig.span).cloned().or_else(|| self.untabled(&fqn)) else {
            // Not a member of a relaxed declaration: nothing moved to it.
            return;
        };
        let returns: Vec<String> = match &sig.return_type {
            ReturnType::Type(t) if t.generic_args.is_empty() && t.name.segments.len() == 1 => {
                vec![t.name.segments[0].text.clone()]
            }
            _ => Vec::new(),
        };
        self.apply(call, &recv, &fqn, &args, &needs, &MemberWord::Method { name, returns });
    }

    fn component_read(&mut self, site: Span, recv: &Ty, fqn: &str, args: &[Ty], name: &str, ty: &TypeRef) {
        let relaxed = self.symbols.clone_needs.relaxed_params(fqn).to_vec();
        let needed: Vec<String> =
            relaxed.into_iter().filter(|p| crate::clone_needs::type_ref_mentions(ty, p)).collect();
        if needed.is_empty() {
            return;
        }
        let map = self.owner_args(fqn, args, fqn).unwrap_or_default();
        self.require(site, recv, fqn, &needed, &map, &MemberWord::Field(name.to_string()));
    }

    fn construct(&mut self, span: Span) {
        let Some(ty) = self.expr_types.get(&span).cloned() else { return };
        self.instantiation(&ty, span, 0);
        let Ty::User { name, generic_args } = &ty else { return };
        if generic_args.is_empty() {
            return;
        }
        let Some(fqn) = self.decl_fqn(name) else { return };
        if self.symbols.clone_needs.relaxed_params(&fqn).is_empty() {
            // Nothing relaxed: a class's parameters were checked on the type,
            // and a record's constructor lives in its bounded impl.
            if self.is_value_decl(&fqn) {
                self.value_impl(span, &ty, &fqn, generic_args, &MemberWord::Ctor);
            }
            return;
        }
        let s = self.symbols;
        let pick = s.ctor_selections.get(&span).copied().unwrap_or(0);
        let ctor_span = if let Some(c) = s.classes.get(&fqn) {
            // No declared constructor: the synthesized one is keyed by the
            // class's own span.
            if c.constructors.is_empty() {
                Some(c.span)
            } else {
                c.constructors.get(pick).map(|c| c.span)
            }
        } else if let Some(r) = s.records.get(&fqn) {
            if pick == 0 {
                Some(r.span)
            } else {
                r.constructors.get(pick).map(|c| c.span)
            }
        } else {
            return;
        };
        let needs = self.needs_of(ctor_span, &fqn);
        let needs = MemberNeeds { touches_self: false, ..needs };
        self.apply(span, &ty, &fqn, generic_args, &needs, &MemberWord::Ctor);
    }

    fn operator(&mut self, left: &Expr, kind: OperatorKind, site: Span) {
        let Some((recv, fqn, args)) = self.receiver(left) else { return };
        let s = self.symbols;
        let pick = s.operator_selections.get(&site).copied().unwrap_or(0);
        // The class that declares the operator, walking up the chain.
        let mut cur = Some(fqn.clone());
        let mut span = None;
        for _ in 0..64 {
            let Some(c) = cur.clone() else { break };
            let found = if let Some(cs) = s.classes.get(&c) {
                cs.operator_overloads
                    .get(&kind)
                    .and_then(|g| g.get(pick))
                    .or_else(|| cs.operators.get(&kind))
                    .map(|o| o.span)
            } else if let Some(r) = s.records.get(&c) {
                r.operators.get(&kind).map(|o| o.span)
            } else if let Some(e) = s.enums.get(&c) {
                e.operators.get(&kind).map(|o| o.span)
            } else {
                None
            };
            if found.is_some() {
                span = found;
                break;
            }
            cur = s.classes.get(&c).and_then(|cs| cs.extends_fqn.clone());
        }
        let Some(span) = span else { return };
        let Some(needs) = s.clone_needs.member(span).cloned().or_else(|| self.untabled(&fqn)) else { return };
        self.apply(site, &recv, &fqn, &args, &needs, &MemberWord::Operator(kind));
    }

    fn property_write(&mut self, fe: &juxc_ast::FieldExpr) {
        let Some((recv, fqn, args)) = self.receiver(&fe.object) else { return };
        if !self.symbols.classes.contains_key(&fqn) || !self.class_has_property(&fqn, &fe.field.text) {
            return;
        }
        let setter =
            self.symbols.merged_method_overloads(&fqn, &juxc_ast::desugar_static_setter_name(&fe.field.text));
        let Some(sig) = setter.first() else { return };
        let Some(needs) = self.symbols.clone_needs.member(sig.span).cloned() else { return };
        self.apply(fe.span, &recv, &fqn, &args, &needs, &MemberWord::Property(fe.field.text.clone()));
    }

    fn class_has_property(&self, fqn: &str, name: &str) -> bool {
        let mut cur = Some(fqn.to_string());
        for _ in 0..64 {
            let Some(c) = cur else { return false };
            let Some(cs) = self.symbols.classes.get(&c) else { return false };
            if cs.properties.contains_key(name) {
                return true;
            }
            cur = cs.extends_fqn.clone();
        }
        false
    }

    fn field_read(&mut self, fe: &juxc_ast::FieldExpr, is_receiver: bool) {
        let Some((recv, fqn, args)) = self.receiver(&fe.object) else { return };
        let name = fe.field.text.clone();
        if let Some(r) = self.symbols.records.get(&fqn) {
            if let Some(comp) = r.components.iter().find(|c| c.name == name) {
                if !is_receiver {
                    let ty = comp.ty.clone();
                    self.component_read(fe.span, &recv, &fqn, &args, &name, &ty);
                }
            }
            return;
        }
        if !self.symbols.classes.contains_key(&fqn) {
            return;
        }
        // A property read runs its getter.
        if self.class_has_property(&fqn, &name) {
            let group = self.symbols.merged_method_overloads(&fqn, &name);
            let Some(sig) = group.first() else { return };
            let Some(needs) = self.symbols.clone_needs.member(sig.span).cloned() else { return };
            self.apply(fe.span, &recv, &fqn, &args, &needs, &MemberWord::Property(name));
            return;
        }
        let Some((field, owner)) = self.symbols.lookup_field(&fqn, &name) else { return };
        let owner = owner.to_string();
        let relaxed = self.symbols.clone_needs.relaxed_params(&owner).to_vec();
        let mentioned: Vec<String> =
            relaxed.into_iter().filter(|p| crate::clone_needs::type_ref_mentions(&field.ty, p)).collect();
        if mentioned.is_empty() {
            return;
        }
        let Some(map) = self.owner_args(&fqn, &args, &owner) else { return };
        // Through a polymorphic base the read goes through the `__get_<f>`
        // accessor, which states the bound; on the class itself the value
        // is copied out, and a method called on it is called in place.
        let poly = self.symbols.classes.values().any(|c| c.extends_fqn.as_deref() == Some(fqn.as_str()));
        if poly {
            self.require(fe.span, &recv, &owner, &mentioned, &map, &MemberWord::Field(name));
            return;
        }
        if is_receiver {
            return;
        }
        let owner_params = self.decl_params(&owner);
        let owner_args: Vec<Ty> = owner_params
            .iter()
            .map(|p| map.get(&p.name.text).cloned().unwrap_or(Ty::Param(p.name.text.clone())))
            .collect();
        let value = crate::ty::substitute(
            &crate::ty::lower_member_type(&field.ty, &owner, self.symbols),
            &owner_params,
            &owner_args,
        );
        if has_param(&value) || self.caps(&value, 0).clone {
            return;
        }
        // Name the parameter the copied value comes from.
        let culprit: Vec<String> = mentioned
            .into_iter()
            .filter(|p| map.get(p).is_some_and(|a| !self.caps(a, 0).clone))
            .take(1)
            .collect();
        let only_clone: HashMap<String, Ty> = culprit.iter().filter_map(|p| map.get(p).map(|a| (p.clone(), a.clone()))).collect();
        self.require(fe.span, &recv, &owner, &culprit, &only_clone, &MemberWord::Field(name));
    }

    // ---- extends / implements -----------------------------------------------------

    /// A class copies every member of the class it extends (and forwards
    /// every default of an interface it implements), so each one's needs
    /// apply to the arguments the clause passes.
    fn inherits(&mut self, clause: &TypeRef, is_implements: bool) {
        let lowered = self.lower(clause);
        let Ty::User { name, generic_args } = &lowered else { return };
        if generic_args.is_empty() {
            return;
        }
        let Some(target) = self.decl_fqn(name) else { return };
        if !self.is_jux_decl(&target) {
            return;
        }
        let s = self.symbols;
        // Every declaration up the chain, with its members.
        let mut chain: Vec<String> = vec![target.clone()];
        if !is_implements {
            let mut cur = s.classes.get(&target).and_then(|c| c.extends_fqn.clone());
            while let Some(c) = cur {
                if chain.len() > 64 || chain.contains(&c) {
                    break;
                }
                cur = s.classes.get(&c).and_then(|cs| cs.extends_fqn.clone());
                chain.push(c);
            }
        }
        let mut members: Vec<(String, MethodSig)> = Vec::new();
        for owner in &chain {
            if let Some(c) = s.classes.get(owner) {
                for m in c.methods.values().chain(c.method_overloads.values().flatten()) {
                    if !m.is_static && !m.is_abstract {
                        members.push((owner.clone(), m.clone()));
                    }
                }
            } else if let Some(i) = s.interfaces.get(owner) {
                for m in i.methods.values() {
                    if !m.is_static && !m.is_abstract {
                        members.push((owner.clone(), m.clone()));
                    }
                }
            }
        }
        members.sort_by_key(|a| a.1.span.start);
        let this = self.class.clone().unwrap_or_default();
        let this_bare = this.rsplit('.').next().unwrap_or(&this).to_string();
        for (owner, m) in members {
            let Some(needs) = s.clone_needs.member(m.span).cloned() else { continue };
            let Some(args) = self.owner_args(&target, generic_args, &owner) else { continue };
            for p in &needs.params {
                let Some(arg) = args.get(p) else { continue };
                if has_param(arg) {
                    continue;
                }
                let caps = self.caps(arg, 0);
                if (caps.clone && caps.debug) || !self.seen.insert((clause.span, p.clone())) {
                    continue;
                }
                let owner_bare = owner.rsplit('.').next().unwrap_or(&owner).to_string();
                let lack = if caps.clone { "has no debug form" } else { "cannot be copied" };
                let verb = if is_implements { "implements" } else { "extends" };
                let d = Diagnostic::error(
                    code::Code::E0457_TypeArgumentLacksCapability,
                    format!(
                        "`{this_bare}` {verb} `{}` and so gets `{owner_bare}.{}()`, which copies its `{p}`, but `{}` {lack}",
                        Show(&lowered),
                        needs.name,
                        Show(arg)
                    ),
                )
                .with_span(clause.span)
                .with_help(format!(
                    "a class receives a copy of every member it inherits; pass a type argument that can be copied, or keep `{p}` a parameter of `{this_bare}`"
                ));
                self.out.push((self.unit, d));
            }
        }
    }
}

/// Spans the body walk treats specially.
#[derive(Default)]
struct Skip {
    /// `recv.m` of a call `recv.m(..)`: the callee, not a field read.
    callees: HashSet<Span>,
    /// `recv` of a call `recv.m(..)`: used in place.
    receivers: HashSet<Span>,
    /// Assignment targets: written, not read.
    targets: HashSet<Span>,
}

impl Skip {
    fn note(&mut self, n: juxc_ast::visit::Node<'_>) {
        match n {
            juxc_ast::visit::Node::Expr(Expr::Call(c)) => {
                if let Expr::Field(fe) = &*c.callee {
                    self.callees.insert(fe.span);
                    if let Expr::Field(inner) = &*fe.object {
                        self.receivers.insert(inner.span);
                    }
                }
            }
            juxc_ast::visit::Node::Stmt(Stmt::Assign(a)) => {
                if let Expr::Field(fe) = &a.target {
                    self.targets.insert(fe.span);
                }
            }
            _ => {}
        }
    }
}
