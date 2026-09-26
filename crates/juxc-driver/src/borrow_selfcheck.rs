//! The compiler's check on its own borrow discipline (ERRATA E1XX-PHASE7,
//! gap 29).
//!
//! Jux has no borrow checker a program can fail (ERRATA E23). A class object
//! is a shared handle, `Rc<JuxCell<C_Inner>>`, and the backend keeps every
//! borrow of its cell statement-scoped (JUX-CLASS-REPRESENTATION §CR.4.1) so
//! the cell's run-time check can never fire. Where the backend misses a hoist,
//! the program stops with "object of type C was already in use", which is a
//! compiler bug the user found for us.
//!
//! This pass looks for those misses in the emitted Rust, before anything runs.
//! It walks every function the backend generated for Jux code, follows Rust's
//! temporary-scope rules (edition 2021) to know which cell guards are alive at
//! each point, and reports a guard still alive when:
//!
//! - the same object's cell is borrowed again, mutably on either side
//!   (`self.0.borrow_mut().n = self.0.borrow().n + ...` in one statement);
//! - a Jux method that may mutate is called ON the guarded object
//!   (`a.0.borrow().n + a.inc()`), or is handed the object as an argument
//!   (`h.bump(self.clone())` under a guard on `self`);
//! - Jux code held by the object runs: a method on a Jux object read out of
//!   one of its fields (`self.0.borrow().child.clone().poke()`), or a
//!   function value stored in one (`(self.0.borrow().hook.clone())()`).
//!
//! What it does not claim: aliases (`b = a`) and calls through unrelated
//! objects are not followed, so a clean report is not a proof. What it
//! reports it is sure of: each shape above is a statement that fails at run
//! time whenever the call really does touch the object, and none can be told
//! apart from one that does without running it.
//!
//! It runs under `JUX_SELFCHECK=1`, which the example corpus sets. A hit is an
//! `E0900` internal compiler error at the `.jux` line, "the compiler would
//! emit a borrow conflict here": users never see one unless the backend is
//! wrong, and then they see it at build time instead of as a crash.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use juxc_diagnostics::{code::Code, Diagnostic};
use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Block, Expr, ExprMethodCall, Stmt};

use crate::build_failure::BuildFailure;
use crate::source_map::SourceMap;

/// Whether the self-check runs: `JUX_SELFCHECK` set to anything but `0`.
pub(crate) fn enabled() -> bool {
    std::env::var("JUX_SELFCHECK").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// One borrow conflict the emitted Rust would run into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hit {
    /// The crate-relative file it is in.
    pub file: String,
    /// The 1-based line of the emitted Rust.
    pub line: u32,
    /// What conflicts with what, in Jux words.
    pub what: String,
}

/// Check the emitted crate on disk and turn any hit into an `E0900`.
pub(crate) fn check_crate(crate_dir: &Path, rs_files: &[PathBuf]) -> Result<(), BuildFailure> {
    let mut sources: Vec<(String, String)> = Vec::new();
    for full in rs_files {
        let Ok(text) = std::fs::read_to_string(full) else { continue };
        let rel = full
            .strip_prefix(crate_dir)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| full.to_string_lossy().replace('\\', "/"));
        sources.push((rel, text));
    }
    let pairs: Vec<(&str, &str)> = sources.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let hits = check_sources(&pairs);
    if hits.is_empty() {
        return Ok(());
    }
    let map = SourceMap::from_sources(&pairs);
    let mut failure = BuildFailure {
        diagnostics: Vec::new(),
        sources: Vec::new(),
        detail: String::new(),
        exit_code: crate::ice::ICE_EXIT_CODE,
    };
    let mut seen: Vec<(String, u32, u32)> = Vec::new();
    for hit in hits {
        let mut d = Diagnostic::error(
            Code::E0900_BackendEmittedInvalidRust,
            "internal compiler error: the compiler would emit a borrow conflict here",
        );
        d.notes.push(format!("{} (at {}:{} of the generated crate)", hit.what, hit.file, hit.line));
        if let Some(entry) = map.lookup(&hit.file, hit.line) {
            let key = (entry.jux_path.clone(), entry.jux_line, entry.jux_col);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            match crate::build_failure::locate(&entry.jux_path, entry.jux_line, entry.jux_col, &mut failure.sources) {
                Some((span, index)) => d = d.with_span(span).with_file(index),
                None => d.notes.push(format!(
                    "the code was generated for {}:{}:{}",
                    entry.jux_path, entry.jux_line, entry.jux_col
                )),
            }
        }
        d.notes.push(
            "this is a bug in the Jux compiler, not in your program: Jux has no borrow checker a \
             program can fail (ERRATA E23), and the program would stop here with \"already in use\""
                .to_string(),
        );
        failure.detail.push_str(&format!("{}:{}: {}\n", hit.file, hit.line, hit.what));
        failure.diagnostics.push(d.with_help(format!(
            "please report it at {}, with the source that triggered it",
            crate::ice::ISSUES_URL
        )));
    }
    Err(failure)
}

/// Check emitted `(crate-relative path, contents)` pairs. A file that does not
/// parse is skipped: rustc will say why, and that is `E0900` already.
pub(crate) fn check_sources(sources: &[(&str, &str)]) -> Vec<Hit> {
    let mut parsed: Vec<(&str, syn::File, Vec<u32>)> = Vec::new();
    for (path, text) in sources {
        let Ok(file) = syn::parse_file(text) else { continue };
        let markers: Vec<u32> = text
            .lines()
            .enumerate()
            .filter(|(_, l)| l.trim_start().starts_with("// JUX:"))
            .map(|(i, _)| i as u32 + 1)
            .collect();
        parsed.push((path, file, markers));
    }
    // Pass 1: the functions generated for Jux code, and which may mutate.
    let mut fns = FnCollector::default();
    for (_, file, markers) in &parsed {
        fns.markers = markers.clone();
        fns.visit_file(file);
    }
    let mutating = fns.mutating_names();
    // Names the backend also gives to Rust's own plumbing (a record's `clone`,
    // a class's `fmt`) are not calls into Jux code worth following.
    const PLUMBING: &[&str] = &["clone", "borrow", "borrow_mut", "fmt", "deref", "deref_mut", "into", "as_ref", "to_string"];
    let jux_fns: HashSet<String> = fns
        .fns
        .iter()
        .map(|f| f.name.clone())
        .filter(|n| !PLUMBING.contains(&n.as_str()))
        .collect();
    // Pass 2: walk every Jux function's body.
    let mut hits = Vec::new();
    for (path, file, markers) in &parsed {
        let mut bodies = BodyCollector { markers, bodies: Vec::new() };
        bodies.visit_file(file);
        for body in bodies.bodies {
            let mut w = Walker::new(path, &jux_fns, &mutating);
            w.walk_fn_body(body);
            hits.extend(w.hits);
        }
    }
    hits.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    hits.dedup();
    hits
}

fn line_of(span: Span) -> u32 {
    span.start().line as u32
}

/// True when a `// JUX:` marker sits inside `[start, end]`: the function
/// holds code generated for a Jux statement, not runtime support.
fn has_marker(markers: &[u32], start: u32, end: u32) -> bool {
    markers.iter().any(|m| *m >= start && *m <= end)
}

/// A function generated for Jux code.
struct FnFacts {
    name: String,
    /// Takes a mutable borrow of some cell itself.
    borrows_mut: bool,
    /// Calls a function value: arbitrary Jux code.
    calls_closure: bool,
    /// The Jux functions and methods it calls, by name.
    calls: HashSet<String>,
}

#[derive(Default)]
struct FnCollector {
    markers: Vec<u32>,
    fns: Vec<FnFacts>,
}

impl FnCollector {
    fn add(&mut self, name: &str, span: Span, block: &Block) {
        if !has_marker(&self.markers, span.start().line as u32, span.end().line as u32) {
            return;
        }
        let mut facts = CallFacts::default();
        facts.visit_block(block);
        self.fns.push(FnFacts {
            name: name.to_string(),
            borrows_mut: facts.borrows_mut,
            calls_closure: facts.calls_closure,
            calls: facts.calls,
        });
    }

    /// Names of the Jux functions that may take a mutable borrow, directly or
    /// through what they call. By name, so every overload and every override
    /// of a name counts: over-approximating "may mutate" is the safe side.
    fn mutating_names(&self) -> HashSet<String> {
        let mut out: HashSet<String> = self
            .fns
            .iter()
            .filter(|f| f.borrows_mut || f.calls_closure)
            .map(|f| f.name.clone())
            .collect();
        loop {
            let before = out.len();
            for f in &self.fns {
                if !out.contains(&f.name) && f.calls.iter().any(|c| out.contains(c)) {
                    out.insert(f.name.clone());
                }
            }
            if out.len() == before {
                return out;
            }
        }
    }
}

impl<'ast> Visit<'ast> for FnCollector {
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.add(&f.sig.ident.to_string(), f.span(), &f.block);
        syn::visit::visit_impl_item_fn(self, f);
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.add(&f.sig.ident.to_string(), f.span(), &f.block);
        syn::visit::visit_item_fn(self, f);
    }
    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        if let Some(block) = &f.default {
            self.add(&f.sig.ident.to_string(), f.span(), block);
        }
        syn::visit::visit_trait_item_fn(self, f);
    }
}

/// What a function body does, for [`FnFacts`].
#[derive(Default)]
struct CallFacts {
    borrows_mut: bool,
    calls_closure: bool,
    calls: HashSet<String>,
}

impl<'ast> Visit<'ast> for CallFacts {
    fn visit_expr_method_call(&mut self, m: &'ast ExprMethodCall) {
        let name = m.method.to_string();
        if name == "borrow_mut" {
            self.borrows_mut = true;
        }
        self.calls.insert(name);
        syn::visit::visit_expr_method_call(self, m);
    }
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        match path_call_name(&c.func) {
            Some(name) => {
                self.calls.insert(name);
            }
            None => self.calls_closure = true,
        }
        syn::visit::visit_expr_call(self, c);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if let Some(args) = macro_args(m) {
            for a in &args {
                self.visit_expr(a);
            }
        }
    }
}

/// The name a path call names (`f`, `C::new`, `crate::m::f`), or `None` for a
/// call of a function value (`(self.0.borrow().hook.clone())()`, `f.0()`).
/// A single-segment path is a local as often as a function; the caller tells
/// them apart against the set of generated functions.
fn path_call_name(func: &Expr) -> Option<String> {
    match func {
        Expr::Path(p) if p.qself.is_none() => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// The argument expressions of a formatting or assertion macro, which syn
/// leaves as tokens. `None` for any other macro.
fn macro_args(m: &syn::Macro) -> Option<Punctuated<Expr, syn::Token![,]>> {
    let name = m.path.segments.last()?.ident.to_string();
    const FORMATTING: &[&str] = &[
        "println", "print", "eprintln", "eprint", "format", "write", "writeln", "panic", "assert",
        "assert_eq", "assert_ne", "format_args", "vec",
    ];
    if !FORMATTING.contains(&name.as_str()) {
        return None;
    }
    m.parse_body_with(Punctuated::<Expr, syn::Token![,]>::parse_terminated).ok()
}

/// Every function body generated for Jux code, closures excluded (a closure
/// is walked where it is written, as a body of its own).
struct BodyCollector<'a, 'ast> {
    markers: &'a [u32],
    bodies: Vec<&'ast Block>,
}

impl<'ast> Visit<'ast> for BodyCollector<'_, 'ast> {
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        if has_marker(self.markers, f.span().start().line as u32, f.span().end().line as u32) {
            self.bodies.push(&f.block);
        }
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        if has_marker(self.markers, f.span().start().line as u32, f.span().end().line as u32) {
            self.bodies.push(&f.block);
        }
    }
    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        if let Some(block) = &f.default {
            if has_marker(self.markers, f.span().start().line as u32, f.span().end().line as u32) {
                self.bodies.push(block);
            }
        }
    }
}

/// The object an expression names, as text: `self`, `a`, `self.child` for a
/// field read through `self`'s cell. Clones, references, parentheses and the
/// cell's own `.0` / `.borrow()` do not change which object it is. `None` for
/// anything else (a call result, an index).
fn object_of(e: &Expr) -> Option<String> {
    match e {
        Expr::Paren(p) => object_of(&p.expr),
        Expr::Group(g) => object_of(&g.expr),
        Expr::Reference(r) => object_of(&r.expr),
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Deref(_)) => object_of(&u.expr),
        Expr::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
            Some(p.path.segments[0].ident.to_string())
        }
        Expr::Field(f) => {
            let base = object_of(&f.base)?;
            match &f.member {
                syn::Member::Unnamed(i) if i.index == 0 => Some(base),
                syn::Member::Unnamed(i) => Some(format!("{base}.{}", i.index)),
                syn::Member::Named(n) => Some(format!("{base}.{n}")),
            }
        }
        Expr::MethodCall(m)
            if m.args.is_empty() && matches!(m.method.to_string().as_str(), "clone" | "borrow" | "borrow_mut") =>
        {
            object_of(&m.receiver)
        }
        _ => None,
    }
}

/// True when `e` is a function value read out of an object's field and
/// called: `(self.0.borrow().hook.clone())`.
fn is_field_read(e: &Expr) -> bool {
    match e {
        Expr::Paren(p) => is_field_read(&p.expr),
        Expr::MethodCall(m) if m.method == "clone" && m.args.is_empty() => is_field_read(&m.receiver),
        Expr::Field(f) => matches!(f.member, syn::Member::Named(_)),
        _ => false,
    }
}

/// True when the receiver of a method call is a Jux object read out of a
/// field and cloned: `self.0.borrow().child.clone()`. The clone is what the
/// backend writes for a handle; a collection's own method runs in place.
fn is_cloned_field_handle(e: &Expr) -> bool {
    match e {
        Expr::Paren(p) => is_cloned_field_handle(&p.expr),
        Expr::MethodCall(m) if m.method == "clone" && m.args.is_empty() => is_field_read(&m.receiver),
        _ => false,
    }
}

/// A cell guard alive at the current point of the walk.
struct Guard {
    object: String,
    exclusive: bool,
    /// The temporary scope it dies with.
    scope: usize,
}

struct Walker<'a> {
    file: &'a str,
    jux_fns: &'a HashSet<String>,
    mutating: &'a HashSet<String>,
    live: Vec<Guard>,
    /// Scope ids: `next_scope` hands out fresh ones; `temp_scope` is where a
    /// temporary created now dies.
    next_scope: usize,
    temp_scope: usize,
    hits: Vec<Hit>,
    /// Locals bound to a function value: calling one runs Jux code.
    closure_locals: HashSet<String>,
    /// Locals bound to a guard (`let mut __jux_cell = h.borrow_mut();`).
    guard_locals: HashSet<String>,
    /// Locals bound to an object or to something read out of one
    /// (`let b = a.clone();`, `let __jux_recv = self.0.borrow().child.clone();`),
    /// with the object they name.
    aliases: HashMap<String, String>,
}

impl<'a> Walker<'a> {
    fn new(file: &'a str, jux_fns: &'a HashSet<String>, mutating: &'a HashSet<String>) -> Self {
        Walker {
            file,
            jux_fns,
            mutating,
            live: Vec::new(),
            next_scope: 1,
            temp_scope: 0,
            hits: Vec::new(),
            closure_locals: HashSet::new(),
            guard_locals: HashSet::new(),
            aliases: HashMap::new(),
        }
    }

    fn open(&mut self) -> usize {
        let id = self.next_scope;
        self.next_scope += 1;
        id
    }

    /// End scope `id`: every guard that dies with it (or with a scope opened
    /// inside it, which has a larger id) is released.
    fn close(&mut self, id: usize) {
        self.live.retain(|g| g.scope < id);
    }

    fn hit(&mut self, span: Span, what: String) {
        self.hits.push(Hit { file: self.file.to_string(), line: line_of(span), what });
    }

    fn walk_fn_body(&mut self, body: &Block) {
        let scope = self.open();
        self.temp_scope = scope;
        self.walk_block(body, true);
        self.close(scope);
    }

    /// `own_tail`: the block is a temporary scope of its own (a function,
    /// `if`/loop body, match arm), so its tail's temporaries die with it. A
    /// plain `{ ... }` block's tail temporaries outlive it (edition 2021).
    fn walk_block(&mut self, block: &Block, own_tail: bool) {
        let outer_temp = self.temp_scope;
        let block_scope = self.open();
        let n = block.stmts.len();
        for (i, stmt) in block.stmts.iter().enumerate() {
            let is_tail = i + 1 == n && matches!(stmt, Stmt::Expr(_, None));
            if is_tail && !own_tail {
                self.temp_scope = outer_temp;
                if let Stmt::Expr(e, None) = stmt {
                    self.walk_expr(e);
                }
                continue;
            }
            let s = self.open();
            self.temp_scope = s;
            self.walk_stmt(stmt, block_scope);
            self.close(s);
        }
        self.temp_scope = outer_temp;
        self.close(block_scope);
    }

    fn walk_stmt(&mut self, stmt: &Stmt, block_scope: usize) {
        match stmt {
            Stmt::Local(local) => {
                let Some(init) = &local.init else { return };
                // A guard bound by name, or a reference to a temporary that
                // holds one (lifetime extension), lives to the block's end.
                let extended = matches!(&*init.expr, Expr::Reference(_)) || is_guard_call(&init.expr);
                if extended {
                    let saved = self.temp_scope;
                    self.temp_scope = block_scope;
                    self.walk_expr(&init.expr);
                    self.temp_scope = saved;
                } else {
                    self.walk_expr(&init.expr);
                }
                if is_guard_call(&init.expr) {
                    if let syn::Pat::Ident(pi) = &local.pat {
                        self.guard_locals.insert(pi.ident.to_string());
                    }
                }
                if matches!(&*init.expr, Expr::Closure(_)) {
                    if let syn::Pat::Ident(pi) = &local.pat {
                        self.closure_locals.insert(pi.ident.to_string());
                    }
                }
                // A copy of a handle, or of a function value, read out of an
                // object: the local names what the expression named.
                let copies = matches!(&*init.expr, Expr::Path(_))
                    || matches!(&*init.expr, Expr::MethodCall(m) if m.method == "clone" && m.args.is_empty());
                if let syn::Pat::Ident(pi) = &local.pat {
                    let name = pi.ident.to_string();
                    let object = if copies { self.obj(&init.expr) } else { None };
                    match object {
                        Some(object) if object != name => {
                            self.aliases.insert(name, object);
                        }
                        _ => {
                            self.aliases.remove(&name);
                        }
                    }
                }
                if let Some((_, els)) = &init.diverge {
                    self.walk_expr(els);
                }
            }
            Stmt::Expr(e, _) => self.walk_expr(e),
            Stmt::Macro(m) => self.walk_macro(&m.mac),
            Stmt::Item(_) => {}
        }
    }

    fn walk_macro(&mut self, m: &syn::Macro) {
        if let Some(args) = macro_args(m) {
            for a in &args {
                self.walk_expr(a);
            }
        }
    }

    /// Walk `e` in its own temporary scope (an `if` condition, a lazy
    /// boolean operand, a match guard).
    fn walk_scoped(&mut self, e: &Expr) {
        let saved = self.temp_scope;
        let s = self.open();
        self.temp_scope = s;
        self.walk_expr(e);
        self.close(s);
        self.temp_scope = saved;
    }

    fn walk_expr(&mut self, e: &Expr) {
        match e {
            Expr::MethodCall(m) => {
                self.walk_expr(&m.receiver);
                for a in &m.args {
                    self.walk_expr(a);
                }
                let name = m.method.to_string();
                if m.args.is_empty() && (name == "borrow" || name == "borrow_mut") {
                    self.take_guard(&m.receiver, name == "borrow_mut", m.method.span());
                } else if self.jux_fns.contains(&name) && !self.is_guarded_value(&m.receiver) {
                    self.jux_call(&name, Some(&m.receiver), m.args.iter(), m.method.span());
                }
            }
            Expr::Call(c) => {
                self.walk_expr(&c.func);
                for a in &c.args {
                    self.walk_expr(a);
                }
                match path_call_name(&c.func) {
                    Some(name)
                        if self.jux_fns.contains(&name)
                            && !self.closure_locals.contains(&name)
                            && !self.aliases.contains_key(&name) =>
                    {
                        self.jux_call(&name, None, c.args.iter(), c.func.span());
                    }
                    Some(name) if self.closure_locals.contains(&name) || self.aliases.contains_key(&name) => {
                        self.closure_call(&c.func, c.args.iter(), c.func.span());
                    }
                    Some(_) => {}
                    None => self.closure_call(&c.func, c.args.iter(), c.func.span()),
                }
            }
            Expr::Binary(b) => {
                let lazy = matches!(b.op, syn::BinOp::And(_) | syn::BinOp::Or(_));
                let assign = matches!(
                    b.op,
                    syn::BinOp::AddAssign(_)
                        | syn::BinOp::SubAssign(_)
                        | syn::BinOp::MulAssign(_)
                        | syn::BinOp::DivAssign(_)
                        | syn::BinOp::RemAssign(_)
                        | syn::BinOp::BitXorAssign(_)
                        | syn::BinOp::BitAndAssign(_)
                        | syn::BinOp::BitOrAssign(_)
                        | syn::BinOp::ShlAssign(_)
                        | syn::BinOp::ShrAssign(_)
                );
                if lazy {
                    self.walk_scoped(&b.left);
                    self.walk_scoped(&b.right);
                } else if assign {
                    self.walk_expr(&b.right);
                    self.walk_expr(&b.left);
                } else {
                    self.walk_expr(&b.left);
                    self.walk_expr(&b.right);
                }
            }
            Expr::Assign(a) => {
                self.walk_expr(&a.right);
                self.walk_expr(&a.left);
            }
            Expr::If(i) => {
                if matches!(&*i.cond, Expr::Let(_)) {
                    self.walk_expr(&i.cond);
                } else {
                    self.walk_scoped(&i.cond);
                }
                self.walk_body(&i.then_branch);
                if let Some((_, els)) = &i.else_branch {
                    match &**els {
                        Expr::Block(b) => self.walk_body(&b.block),
                        other => self.walk_expr(other),
                    }
                }
            }
            Expr::While(w) => {
                if matches!(&*w.cond, Expr::Let(_)) {
                    let saved = self.temp_scope;
                    let s = self.open();
                    self.temp_scope = s;
                    self.walk_expr(&w.cond);
                    self.walk_body(&w.body);
                    self.close(s);
                    self.temp_scope = saved;
                } else {
                    self.walk_scoped(&w.cond);
                    self.walk_body(&w.body);
                }
            }
            Expr::ForLoop(f) => {
                // The iterator expression's temporaries live across the body.
                self.walk_expr(&f.expr);
                self.walk_body(&f.body);
            }
            Expr::Loop(l) => self.walk_body(&l.body),
            Expr::Match(m) => {
                // The scrutinee's temporaries live across every arm.
                self.walk_expr(&m.expr);
                for arm in &m.arms {
                    if let Some((_, g)) = &arm.guard {
                        self.walk_scoped(g);
                    }
                    self.walk_scoped(&arm.body);
                }
            }
            Expr::Block(b) => self.walk_block(&b.block, false),
            Expr::Unsafe(u) => self.walk_block(&u.block, false),
            Expr::Closure(c) => {
                // Runs later, under whatever is borrowed then: a body of its own.
                let live = std::mem::take(&mut self.live);
                let saved = self.temp_scope;
                let s = self.open();
                self.temp_scope = s;
                self.walk_expr(&c.body);
                self.close(s);
                self.temp_scope = saved;
                self.live = live;
            }
            Expr::Async(a) => {
                let live = std::mem::take(&mut self.live);
                self.walk_block(&a.block, true);
                self.live = live;
            }
            Expr::Let(l) => self.walk_expr(&l.expr),
            Expr::Paren(p) => self.walk_expr(&p.expr),
            Expr::Group(g) => self.walk_expr(&g.expr),
            Expr::Reference(r) => self.walk_expr(&r.expr),
            Expr::Unary(u) => self.walk_expr(&u.expr),
            Expr::Cast(c) => self.walk_expr(&c.expr),
            Expr::Field(f) => self.walk_expr(&f.base),
            Expr::Index(i) => {
                self.walk_expr(&i.expr);
                self.walk_expr(&i.index);
            }
            Expr::Try(t) => self.walk_expr(&t.expr),
            Expr::Await(a) => self.walk_expr(&a.base),
            Expr::Return(r) => {
                if let Some(e) = &r.expr {
                    self.walk_expr(e);
                }
            }
            Expr::Break(b) => {
                if let Some(e) = &b.expr {
                    self.walk_expr(e);
                }
            }
            Expr::Tuple(t) => t.elems.iter().for_each(|x| self.walk_expr(x)),
            Expr::Array(a) => a.elems.iter().for_each(|x| self.walk_expr(x)),
            Expr::Repeat(r) => self.walk_expr(&r.expr),
            Expr::Struct(s) => {
                for f in &s.fields {
                    self.walk_expr(&f.expr);
                }
                if let Some(rest) = &s.rest {
                    self.walk_expr(rest);
                }
            }
            Expr::Range(r) => {
                if let Some(x) = &r.start {
                    self.walk_expr(x);
                }
                if let Some(x) = &r.end {
                    self.walk_expr(x);
                }
            }
            Expr::Macro(m) => self.walk_macro(&m.mac),
            _ => {}
        }
    }

    /// An `if`/loop body or an `else` block: a temporary scope of its own.
    fn walk_body(&mut self, block: &Block) {
        let saved = self.temp_scope;
        let s = self.open();
        self.temp_scope = s;
        self.walk_block(block, true);
        self.close(s);
        self.temp_scope = saved;
    }

    /// [`object_of`], with a local that copies an object read as that object.
    fn obj(&self, e: &Expr) -> Option<String> {
        let object = object_of(e)?;
        let (root, rest) = match object.split_once('.') {
            Some((root, rest)) => (root, Some(rest)),
            None => (object.as_str(), None),
        };
        match (self.aliases.get(root), rest) {
            (Some(target), Some(rest)) => Some(format!("{target}.{rest}")),
            (Some(target), None) => Some(target.clone()),
            (None, _) => Some(object),
        }
    }

    /// True when `e` is a handle or function value read out of an object's
    /// field: cloned out (`self.0.borrow().child.clone()`), through a local
    /// that copied one, or, with `in_place`, the field itself
    /// (`(self.0.borrow().hook)`). A method called on a field in place is a
    /// value struct's own (ERRATA E20), which holds no way back.
    fn read_out_of_field(&self, e: &Expr, in_place: bool) -> bool {
        if is_cloned_field_handle(e) || (in_place && is_field_read(e)) {
            return true;
        }
        match e {
            Expr::Paren(p) => self.read_out_of_field(&p.expr, in_place),
            Expr::Path(p) => p
                .path
                .get_ident()
                .and_then(|i| self.aliases.get(&i.to_string()))
                .is_some_and(|target| target.contains('.')),
            _ => false,
        }
    }

    /// True when `recv` is the value inside a cell, reached through a guard
    /// (`h.borrow_mut()`, a local bound to one): a method on it is the Rust
    /// value's own (`Vec::push`), never a Jux method, which is called on the
    /// handle itself.
    fn is_guarded_value(&self, recv: &Expr) -> bool {
        match recv {
            Expr::Paren(p) => self.is_guarded_value(&p.expr),
            Expr::Reference(r) => self.is_guarded_value(&r.expr),
            Expr::Unary(u) => self.is_guarded_value(&u.expr),
            Expr::Path(p) => p.path.get_ident().is_some_and(|i| self.guard_locals.contains(&i.to_string())),
            other => is_guard_call(other),
        }
    }

    /// `<recv>.borrow()` / `<recv>.borrow_mut()`: a new guard, which must not
    /// meet a live one on the same object unless both are shared.
    fn take_guard(&mut self, recv: &Expr, exclusive: bool, span: Span) {
        let Some(object) = self.obj(recv) else { return };
        if let Some(g) = self.live.iter().find(|g| g.object == object && (g.exclusive || exclusive)) {
            let what = format!(
                "`{object}` is borrowed {} while an earlier {} borrow of it in the same statement is still held",
                if exclusive { "mutably" } else { "again" },
                if g.exclusive { "mutable" } else { "shared" },
            );
            self.hit(span, what);
        }
        self.live.push(Guard { object, exclusive, scope: self.temp_scope });
    }

    /// A call of the Jux function or method `name`, with its receiver (for a
    /// method) and arguments. Conflicts with a live guard on an object the
    /// call can reach.
    fn jux_call<'e>(&mut self, name: &str, recv: Option<&Expr>, args: impl Iterator<Item = &'e Expr>, span: Span) {
        if self.live.is_empty() {
            return;
        }
        let mutates = self.mutating.contains(name);
        let recv_obj = recv.and_then(|r| self.obj(r));
        let through_field = recv.is_some_and(|r| self.read_out_of_field(r, false));
        let arg_objs: Vec<String> = args.filter_map(|a| self.obj(a)).collect();
        let mut found: Option<String> = None;
        for g in &self.live {
            if !(g.exclusive || mutates) {
                continue;
            }
            let on_it = recv_obj.as_deref() == Some(g.object.as_str());
            let held_by_it = through_field
                && recv_obj.as_deref().is_some_and(|r| r.starts_with(&format!("{}.", g.object)));
            let given_it = arg_objs.contains(&g.object);
            if on_it || held_by_it || given_it {
                let how = if on_it {
                    "is called on it"
                } else if held_by_it {
                    "is called on an object it holds, which can call back into it"
                } else {
                    "is handed it as an argument"
                };
                found = Some(format!(
                    "`{}` is still borrowed{} while `{name}`, which may {} it, {how}",
                    g.object,
                    if g.exclusive { " mutably" } else { "" },
                    if g.exclusive { "use" } else { "change" },
                ));
                break;
            }
        }
        if let Some(what) = found {
            self.hit(span, what);
        }
    }

    /// A call of a function value. One read out of a guarded object's field
    /// is Jux code that object holds, and can reach it; any function value
    /// can reach an object handed to it.
    fn closure_call<'e>(&mut self, func: &Expr, args: impl Iterator<Item = &'e Expr>, span: Span) {
        if self.live.is_empty() {
            return;
        }
        let held = if self.read_out_of_field(func, true) { self.obj(func) } else { None };
        let arg_objs: Vec<String> = args.filter_map(|a| self.obj(a)).collect();
        let found = self.live.iter().find_map(|g| {
            let held_by_it = held.as_deref().is_some_and(|h| h.starts_with(&format!("{}.", g.object)));
            let given_it = arg_objs.contains(&g.object);
            (held_by_it || given_it).then(|| {
                format!(
                    "`{}` is still borrowed while a function value {} runs, and it can change it",
                    g.object,
                    if held_by_it { "stored in it" } else { "handed it" },
                )
            })
        });
        if let Some(what) = found {
            self.hit(span, what);
        }
    }
}

/// `x.borrow()` / `x.borrow_mut()` with no arguments, directly.
fn is_guard_call(e: &Expr) -> bool {
    matches!(e, Expr::MethodCall(m) if m.args.is_empty()
        && (m.method == "borrow" || m.method == "borrow_mut"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrap `body` in a marked Jux method and a mutating `bump`/`inc`.
    fn check(body: &str) -> Vec<Hit> {
        let src = format!(
            "impl C {{\n    pub fn bump(&self) {{\n        // JUX:a.jux:3:5\n        self.0.borrow_mut().n = 1;\n    }}\n    \
             pub fn get(&self) -> isize {{\n        // JUX:a.jux:4:5\n        self.0.borrow().n\n    }}\n    \
             pub fn run(&self) {{\n        // JUX:a.jux:9:9\n        {body}\n    }}\n}}\n"
        );
        check_sources(&[("src/main.rs", &src)])
    }

    #[test]
    fn a_read_held_across_a_mutating_call_on_the_same_object_is_a_hit() {
        let hits = check("println!(\"{}\", self.0.borrow().n + self.bump());");
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].what.contains("`self` is still borrowed while `bump`"), "{hits:?}");
    }

    #[test]
    fn a_hoisted_read_is_clean() {
        assert!(check("let __jux_l = self.0.borrow().n; println!(\"{}\", __jux_l + self.bump());").is_empty());
    }

    #[test]
    fn a_read_across_a_reading_call_is_clean() {
        assert!(check("println!(\"{}\", self.0.borrow().n + self.get());").is_empty());
    }

    #[test]
    fn an_if_condition_releases_its_guard_before_the_branch() {
        assert!(check("if self.0.borrow().n > 0 { self.bump(); }").is_empty());
    }

    #[test]
    fn a_match_scrutinee_holds_its_guard_across_the_arms() {
        let hits = check("match self.0.borrow().n { 0 => self.bump(), _ => {} }");
        assert_eq!(hits.len(), 1, "{hits:?}");
    }

    #[test]
    fn a_stored_function_value_called_under_its_owner_guard_is_a_hit() {
        let hits = check("(self.0.borrow().hook.clone())();");
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].what.contains("function value stored in it"), "{hits:?}");
    }

    #[test]
    fn a_stored_function_value_bound_first_is_clean() {
        assert!(check("let __jux_f = self.0.borrow().hook.clone(); __jux_f();").is_empty());
    }

    #[test]
    fn a_block_tail_keeps_its_temporaries_to_the_statement_end() {
        let hits = check("let x = { self.0.borrow().n } + self.bump();");
        assert_eq!(hits.len(), 1, "{hits:?}");
    }

    #[test]
    fn a_second_borrow_of_the_same_cell_is_a_hit() {
        // The value is evaluated first, and its guard lives to the `;`.
        let hits = check("self.0.borrow_mut().n = self.0.borrow().n + 1;");
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(check("let v = self.0.borrow().n + 1; self.0.borrow_mut().n = v;").is_empty());
        let hits = check("let x = self.0.borrow().n + { self.0.borrow_mut().n = 2; 2 };");
        assert_eq!(hits.len(), 1, "{hits:?}");
    }

    #[test]
    fn code_without_markers_is_not_checked() {
        let src = "impl C { pub fn run(&self) { println!(\"{}\", self.0.borrow().n + self.bump()); } }\n";
        assert!(check_sources(&[("src/main.rs", src)]).is_empty());
    }
}
