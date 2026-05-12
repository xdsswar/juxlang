//! Phase D + F of the type checker — **statement checking + type-mismatch
//! diagnostics**.
//!
//! Phase C ([`crate::infer`]) is silent: it tells you what type an
//! expression has but never produces a diagnostic. Phase D/F walks every
//! function/method/constructor body and consults the inferred types at
//! "expected vs found" sites, emitting `E0410`–`E0413` when the user's
//! program disagrees with itself.
//!
//! ## Diagnostics
//!
//! - **E0410 — TypeMismatch.** Assignment, return, and call-argument
//!   sites all share this code. The message text differentiates
//!   ("expected X, found Y" for arguments, "cannot assign T to U" for
//!   assignments, "expected return value of type X" for returns).
//! - **E0411 — WrongArgCount.** Wrong number of positional args to a
//!   function, method, or constructor.
//! - **E0412 — UnresolvedField.** `obj.field` where `field` doesn't
//!   exist on the receiver's class (walking the `extends` chain).
//! - **E0413 — UnresolvedMethod.** `obj.method(...)` where `method`
//!   doesn't exist on the receiver's class, OR `new T(...)` where no
//!   class/record `T` is in scope.
//!
//! ## Tolerance rules ([`compatible`])
//!
//! 1. `Unknown` on either side → compatible. Inference's silent-fallback
//!    must not cascade into diagnostic noise.
//! 2. `Ty::Param` on either side → compatible. Phase E substitutes
//!    receiver-side generic args into expected parameter types before
//!    they reach this predicate, so a `new Box<int>("hi")` call now
//!    sees `expected = int, found = String` and emits E0410. The
//!    wildcard rule still catches cases substitution doesn't reach —
//!    method-level generics, raw-type receivers, and members inherited
//!    across an `extends` clause whose generic args weren't propagated.
//! 3. Exact equality (`expected == found`) → compatible.
//! 4. **Default-int / default-float widening.** `Ty::Primitive(Int)`
//!    (the type of an unsuffixed integer literal) is compatible with
//!    any numeric primitive. Same story for `Ty::Primitive(Double)`
//!    (the type of an unsuffixed float literal). This is the
//!    *minimum* coercion needed to accept idiomatic code like
//!    `i32 x = 7;` — true numeric promotion (`int + long`) still
//!    requires an explicit `as` cast.
//! 5. Arrays — element types must be compatible AND kinds must match.
//! 6. User types — same name AND compatible generic-args (pairwise).
//!
//! ## Inheritance chain walks
//!
//! Method and field lookup walk `class.extends` recursively. This means
//! a `Dog extends Animal` can call `getName()` (defined on Animal) and
//! we resolve it correctly. The walk is name-based; we don't try to
//! handle multi-parent (Java's single-inheritance model is enough).
//!
//! ## Built-in receivers
//!
//! Some receivers carry methods/fields the type system doesn't (yet)
//! describe via the symbol table:
//!
//! - **Arrays** carry `.length` (Int) plus the methods `push`, `pop`,
//!   `clone` — known mutators/builtins that Vec exposes. We don't
//!   typecheck their args today.
//! - **Strings** carry `.length` (Int) plus a handful of read-only
//!   methods. Same treatment.
//!
//! These allowlists live as constants near the top of the file
//! ([`BUILTIN_ARRAY_METHODS`] / [`BUILTIN_STRING_METHODS`]) so future
//! turns can grow them without rewiring the call-resolution paths.
//!
//! ## Skipped sites
//!
//! - Class field default initializers (`private int x = 5;`) — Phase D
//!   focuses on bodies; default initializers will join later.
//! - Method calls on `Ty::Param` receivers — see rule 2 above; we'd
//!   need bound-aware lookup to do better than "silently accept".

use std::collections::HashMap;

use juxc_ast::{
    BinaryOp, Block, CallExpr, ClassDecl, CompilationUnit, ConstructorDecl, ElseBranch, Expr,
    FieldExpr, FnDecl, InterpSegment, NewObjectExpr, OperatorDecl, OperatorKind, RecordDecl,
    ReturnType, Stmt, SwitchBody, TopLevelDecl, TypeParam, UnaryOp,
};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

use crate::env::TypeEnv;
use crate::infer::{infer_block, infer_expr};
use crate::symbol_table::{ParamSig, SymbolTable};
use crate::ty::{lower_member_type, substitute, ty_from_ref, Primitive, Ty};

// ============================================================================
// Built-in allowlists
// ============================================================================

/// Single-segment names treated as "built-in function — accepts any args,
/// returns Unknown". `print` is the obvious one; if/when more built-ins
/// land (`assert`, `panic`, …) they go here.
const BUILTINS: &[&str] = &["print"];

/// Methods we let through on **any array receiver** without checking
/// against a class signature. These are the Vec/array methods the
/// backend already knows how to lower; the typechecker plays along.
const BUILTIN_ARRAY_METHODS: &[&str] = &["push", "pop", "clone", "len", "length"];

/// Methods we let through on a **String receiver**. Same idea: the
/// backend understands these, so the typechecker accepts them.
const BUILTIN_STRING_METHODS: &[&str] =
    &["length", "len", "clone", "chars", "bytes", "to_string"];

/// Field/property names we allow on **any array receiver** without a
/// class lookup. Today just `length`; the typechecker treats it as `Int`.
const BUILTIN_ARRAY_FIELDS: &[&str] = &["length"];

/// Field/property names we allow on a **String receiver**.
const BUILTIN_STRING_FIELDS: &[&str] = &["length"];

// ============================================================================
// Checker
// ============================================================================

/// Statement-checker state. Holds an owned `TypeEnv` (pushed/popped as
/// the walker descends), a borrowed [`SymbolTable`] (read-only), a
/// borrowed diagnostic sink (append-only), and the **expected return
/// type** of the function/method currently being walked.
///
/// `current_return` is `None` outside any function body and inside
/// constructors (constructors don't return a value).
pub(crate) struct Checker<'a> {
    /// Per-scope variable bindings. Owned by the checker so it can
    /// push/pop as it descends.
    pub(crate) env: TypeEnv,
    /// Symbol table built by Phase A. Read-only here.
    pub(crate) symbols: &'a SymbolTable,
    /// Diagnostic sink. Append-only — we never read back.
    pub(crate) diagnostics: &'a mut Vec<Diagnostic>,
    /// Expected return type of the function/method we're inside. `None`
    /// outside a function body, and also inside constructor bodies
    /// (constructors don't `return value;`).
    pub(crate) current_return: Option<Ty>,
    /// Per-expression inferred type, keyed by source [`Span`]. Populated
    /// as the checker walks each function/method/constructor body in
    /// [`Self::check_expr`] and friends. The map is moved out via
    /// [`Self::into_expr_types`] when typecheck finishes and exposed to
    /// downstream phases (the Rust backend) through
    /// [`crate::TypeCheckResult::expr_types`].
    ///
    /// Entries with [`Span::DUMMY`] are skipped — they'd collide and
    /// give the wrong type for any expression that happens to carry a
    /// dummy span. The backend's lookup site treats a missing entry as
    /// "fall back to the conservative behavior."
    pub(crate) expr_types: HashMap<Span, Ty>,
}

impl<'a> Checker<'a> {
    /// Construct a fresh checker. `symbols` is the Phase-A symbol table;
    /// `diagnostics` is the same vec the rest of typecheck appends to.
    pub(crate) fn new(symbols: &'a SymbolTable, diagnostics: &'a mut Vec<Diagnostic>) -> Self {
        Self {
            env: TypeEnv::new(),
            symbols,
            diagnostics,
            current_return: None,
            expr_types: HashMap::new(),
        }
    }

    /// Consume the checker, returning the per-expression type map it
    /// built up during [`Self::check_unit`]. Called once at the end of
    /// the top-level `typecheck()` driver.
    pub(crate) fn into_expr_types(self) -> HashMap<Span, Ty> {
        self.expr_types
    }

    /// Infer the type of `expr` against the current env, then record it
    /// keyed by the expression's span. Returns the inferred type so
    /// existing call sites that used `infer_expr(...)` can drop in this
    /// method as a replacement.
    ///
    /// Dummy spans (`Span::DUMMY`) are not recorded — they'd collide
    /// across unrelated expressions and give the backend wrong type
    /// info.
    pub(crate) fn infer_and_record(&mut self, expr: &Expr) -> Ty {
        let ty = infer_expr(expr, &self.env, self.symbols);
        let span = expr_span(expr);
        if span != Span::DUMMY {
            self.expr_types.insert(span, ty.clone());
        }
        ty
    }


    /// Walk every top-level item in `unit`. Functions get checked
    /// directly; classes / records dispatch to `check_class` /
    /// `check_record` which handle members.
    pub(crate) fn check_unit(&mut self, unit: &CompilationUnit) {
        for item in &unit.items {
            match item {
                TopLevelDecl::Function(fn_decl) => self.check_function(fn_decl),
                TopLevelDecl::Class(class) => self.check_class(class),
                TopLevelDecl::Record(record) => self.check_record(record),
                TopLevelDecl::Enum(enum_decl) => self.check_enum(enum_decl),
                // Interfaces carry only signatures (body: None) — no
                // bodies to walk.
                TopLevelDecl::Interface(_) => {}
            }
        }
    }

    /// Walk an enum's operator bodies. Same scope shape as records:
    /// `this` is the enum's type, operator params are declared into
    /// the body's scope. Deleted operators have no body and are
    /// skipped inside `check_operator`.
    fn check_enum(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        let name = enum_decl.name.text.clone();
        self.env.set_class(&name);
        let this_ty = Ty::User {
            name: name.clone(),
            generic_args: Vec::new(),
        };
        for op in &enum_decl.operators {
            self.check_operator(op, &this_ty);
        }
        self.env.clear_class();
    }

    // ------------------------------------------------------------------
    // Function / method / constructor walkers
    // ------------------------------------------------------------------

    /// Walk a top-level function. Pushes a parameter scope, sets the
    /// expected return type, walks the body, then restores both.
    /// Abstract / native functions (body = None) are skipped.
    fn check_function(&mut self, fn_decl: &FnDecl) {
        let Some(body) = &fn_decl.body else { return };
        self.env.push_scope();
        // Declare each parameter into the new scope so name lookups
        // inside the body resolve.
        for param in &fn_decl.params {
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
        }
        let saved = self.current_return.take();
        self.current_return = Some(return_type_to_ty(
            &fn_decl.return_type,
            &self.env,
            self.symbols,
        ));
        self.check_block(body);
        self.current_return = saved;
        self.env.pop_scope();
    }

    /// Walk a class declaration — for each constructor and each method,
    /// set up the class context (current_class + generic params + `this`
    /// binding), run the body checker, then tear it down. Abstract
    /// methods (body = None) are skipped.
    fn check_class(&mut self, class: &ClassDecl) {
        let class_name = class.name.text.clone();
        self.env.set_class(&class_name);
        // Register every generic param so `T` in declared types lowers
        // to `Ty::Param("T")` rather than `Unknown`.
        for tp in &class.generic_params {
            self.env.add_generic_param(&tp.name.text);
        }
        // Pre-compute the `this` type: User<class_name, [Param(T)…]>.
        let this_ty = Ty::User {
            name: class_name.clone(),
            generic_args: class
                .generic_params
                .iter()
                .map(|tp| Ty::Param(tp.name.text.clone()))
                .collect(),
        };

        for ctor in &class.constructors {
            self.check_constructor(ctor, &this_ty);
        }
        for method in &class.methods {
            self.check_method(method, &this_ty);
        }
        for op in &class.operators {
            self.check_operator(op, &this_ty);
        }

        self.env.clear_generic_params();
        self.env.clear_class();
    }

    /// Walk a constructor body. Like [`check_function`] but with no
    /// expected return type (constructors don't return values) and with
    /// `this` pre-declared.
    fn check_constructor(&mut self, ctor: &ConstructorDecl, this_ty: &Ty) {
        self.env.push_scope();
        self.env.declare("this", this_ty.clone());
        for param in &ctor.params {
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
        }
        let saved = self.current_return.take();
        self.current_return = None; // constructors don't return values
        self.check_block(&ctor.body);
        self.current_return = saved;
        self.env.pop_scope();
    }

    /// Walk an instance method body. Same scope shape as a function
    /// plus a `this` binding. Abstract methods (body = None) are
    /// skipped.
    fn check_method(&mut self, method: &FnDecl, this_ty: &Ty) {
        let Some(body) = &method.body else { return };
        self.env.push_scope();
        self.env.declare("this", this_ty.clone());
        // Method-level generic params extend the class-level set.
        for tp in &method.generic_params {
            self.env.add_generic_param(&tp.name.text);
        }
        for param in &method.params {
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
        }
        let saved = self.current_return.take();
        self.current_return = Some(return_type_to_ty(
            &method.return_type,
            &self.env,
            self.symbols,
        ));
        self.check_block(body);
        self.current_return = saved;
        // Method-local generic params would also clear here, but the
        // class's params are still active until check_class finishes.
        // We can't surgically remove just the method's — for Turn 1 we
        // accept the over-broadening (no method-local generics in any
        // existing example).
        self.env.pop_scope();
    }

    /// If `receiver_ty` is a user class/record AND the matching
    /// operator on that type is marked `= delete;` (§O.3.4), emit
    /// `E0935_DeletedOperator` anchored at `span`. No-op otherwise.
    ///
    /// Inherited deletion isn't traced — only the receiver's own
    /// class/record is consulted. That matches the rest of operator
    /// resolution in tycheck today (Phase E substitution only fires
    /// on the receiver's own class) and keeps the diagnostic precise.
    fn check_op_not_deleted(&mut self, receiver_ty: &Ty, kind: OperatorKind, span: Span) {
        let Ty::User { name, .. } = receiver_ty else { return };
        let deleted = self
            .symbols
            .classes
            .get(name)
            .and_then(|c| c.operators.get(&kind))
            .map(|op| op.is_deleted)
            .unwrap_or(false)
            || self
                .symbols
                .records
                .get(name)
                .and_then(|r| r.operators.get(&kind))
                .map(|op| op.is_deleted)
                .unwrap_or(false)
            || self
                .symbols
                .enums
                .get(name)
                .and_then(|e| e.operators.get(&kind))
                .map(|op| op.is_deleted)
                .unwrap_or(false);
        if deleted {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0935_DeletedOperator,
                    format!(
                        "operator `{}` is deleted on type `{}`",
                        operator_kind_user_spelling(kind),
                        name,
                    ),
                )
                .with_span(span),
            );
        }
    }

    /// Walk an operator-overload body. Same scope shape as
    /// [`Self::check_method`]: push a fresh scope, declare `this` with
    /// the class's `Ty::User` shape, declare each formal param, set
    /// `current_return` from the operator's declared return type, walk
    /// the body, then tear it back down.
    ///
    /// Per `JUX-OPERATORS-ADDENDUM.md` §O.2 operators have no
    /// modifiers, no method-level generics, and (today) always have a
    /// body — so the bookkeeping is simpler than `check_method`.
    fn check_operator(&mut self, op: &OperatorDecl, this_ty: &Ty) {
        let Some(body) = &op.body else { return };
        self.env.push_scope();
        self.env.declare("this", this_ty.clone());
        for param in &op.params {
            let ty = ty_from_ref(&param.ty, &self.env, self.symbols);
            self.env.declare(&param.name.text, ty);
        }
        let saved = self.current_return.take();
        self.current_return = Some(return_type_to_ty(&op.return_type, &self.env, self.symbols));
        self.check_block(body);
        self.current_return = saved;
        self.env.pop_scope();
    }

    /// Walk a record's operator bodies. Records compose their value
    /// type from the header components; the only walkable code lives
    /// in operator overrides (§O.3.4 customizations like a custom
    /// `operator string`). `= delete;` operators have no body and are
    /// skipped.
    fn check_record(&mut self, record: &RecordDecl) {
        let name = record.name.text.clone();
        self.env.set_class(&name);
        for tp in &record.generic_params {
            self.env.add_generic_param(&tp.name.text);
        }
        let this_ty = Ty::User {
            name: name.clone(),
            generic_args: record
                .generic_params
                .iter()
                .map(|tp| Ty::Param(tp.name.text.clone()))
                .collect(),
        };
        for op in &record.operators {
            self.check_operator(op, &this_ty);
        }
        self.env.clear_generic_params();
        self.env.clear_class();
    }

    // ------------------------------------------------------------------
    // Statement walker
    // ------------------------------------------------------------------

    /// Walk a block — each statement in source order. Doesn't push a
    /// scope; callers wrap if they need scope nesting (e.g. method
    /// body, for-each loop body).
    fn check_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
    }

    /// Walk one statement, emitting diagnostics where types disagree.
    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl(v) => {
                // If both a declared type and an initializer are
                // present, the two must be compatible. Otherwise the
                // present one wins.
                let declared = v.ty.as_ref().map(|t| ty_from_ref(t, &self.env, self.symbols));
                let inferred = v.init.as_ref().map(|e| {
                    // Walk the initializer for nested checks (e.g. a
                    // call inside the RHS) before reading its type.
                    self.check_expr(e);
                    infer_expr(e, &self.env, self.symbols)
                });
                let final_ty = match (&declared, &inferred) {
                    (Some(d), Some(i)) => {
                        if !compatible(d, i) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    code::Code::E0410_TypeMismatch,
                                    format!(
                                        "type mismatch in declaration of `{}`: expected {}, found {}",
                                        v.name.text, d, i,
                                    ),
                                )
                                .with_span(v.span),
                            );
                        }
                        d.clone()
                    }
                    (Some(d), None) => d.clone(),
                    (None, Some(i)) => i.clone(),
                    (None, None) => Ty::Unknown,
                };
                self.env.declare(&v.name.text, final_ty);
            }

            Stmt::Assign(a) => {
                // Walk both sides for nested checks first.
                self.check_expr(&a.target);
                self.check_expr(&a.value);
                let target_ty = infer_expr(&a.target, &self.env, self.symbols);
                let value_ty = infer_expr(&a.value, &self.env, self.symbols);
                if !compatible(&target_ty, &value_ty) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!("cannot assign {value_ty} to {target_ty}"),
                        )
                        .with_span(a.span),
                    );
                }
            }

            Stmt::Return(opt) => {
                // Clone the expected return type up front so we can
                // mutably borrow `self` to walk the expression below
                // without a borrow conflict on `current_return`.
                let expected = self.current_return.clone();
                match (&expected, opt) {
                    // Bare `return;` inside a void function — fine.
                    (Some(t), None) if t.is_void() => {}
                    // Bare `return;` outside any function — fine.
                    (None, None) => {}
                    // Bare `return;` in a value-returning function.
                    (Some(exp), None) => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0410_TypeMismatch,
                                format!(
                                    "expected return value of type {exp}, found bare `return`",
                                ),
                            ),
                        );
                    }
                    (_, Some(expr)) => {
                        self.check_expr(expr);
                        let found = infer_expr(expr, &self.env, self.symbols);
                        if let Some(exp) = &expected {
                            if !compatible(exp, &found) {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        code::Code::E0410_TypeMismatch,
                                        format!(
                                            "return type mismatch: expected {exp}, found {found}",
                                        ),
                                    )
                                    .with_span(expr_span(expr)),
                                );
                            }
                        }
                        // If `expected` is None (top-level statement
                        // outside a function), nothing to check.
                    }
                }
            }

            Stmt::If(if_stmt) => {
                self.check_expr(&if_stmt.condition);
                let cond_ty = infer_expr(&if_stmt.condition, &self.env, self.symbols);
                if !is_boolish(&cond_ty) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!("expected bool condition, found {cond_ty}"),
                        )
                        .with_span(expr_span(&if_stmt.condition)),
                    );
                }
                self.env.push_scope();
                self.check_block(&if_stmt.then_block);
                self.env.pop_scope();
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.check_else_branch(else_branch);
                }
            }

            Stmt::While(w) => {
                self.check_expr(&w.condition);
                let cond_ty = infer_expr(&w.condition, &self.env, self.symbols);
                if !is_boolish(&cond_ty) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!("expected bool condition, found {cond_ty}"),
                        )
                        .with_span(expr_span(&w.condition)),
                    );
                }
                self.env.push_scope();
                self.check_block(&w.body);
                self.env.pop_scope();
            }

            Stmt::ForEach(f) => {
                self.check_expr(&f.iter);
                let iter_ty = infer_expr(&f.iter, &self.env, self.symbols);
                // Loop-var binding: explicit annotation wins; else
                // element-of-array if iter is an array; else Unknown.
                let var_ty = if let Some(declared) = &f.var_type {
                    ty_from_ref(declared, &self.env, self.symbols)
                } else {
                    match iter_ty {
                        Ty::Array { element, .. } => *element,
                        _ => Ty::Unknown,
                    }
                };
                self.env.push_scope();
                self.env.declare(&f.var_name.text, var_ty);
                self.check_block(&f.body);
                self.env.pop_scope();
            }

            Stmt::Expr(e) => {
                self.check_expr(e);
            }

            Stmt::SuperCall(args, span) => self.check_super_call(args, *span),

            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    /// Recurse through else / else-if chains, mirroring [`Self::check_stmt`]'s
    /// handling for the top-level `if`.
    fn check_else_branch(&mut self, branch: &ElseBranch) {
        match branch {
            ElseBranch::If(if_stmt) => {
                self.check_expr(&if_stmt.condition);
                let cond_ty = infer_expr(&if_stmt.condition, &self.env, self.symbols);
                if !is_boolish(&cond_ty) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0410_TypeMismatch,
                            format!("expected bool condition, found {cond_ty}"),
                        )
                        .with_span(expr_span(&if_stmt.condition)),
                    );
                }
                self.env.push_scope();
                self.check_block(&if_stmt.then_block);
                self.env.pop_scope();
                if let Some(nested) = &if_stmt.else_branch {
                    self.check_else_branch(nested);
                }
            }
            ElseBranch::Block(block) => {
                self.env.push_scope();
                self.check_block(block);
                self.env.pop_scope();
            }
        }
    }

    // ------------------------------------------------------------------
    // Expression walker (depth-first, drives field/method/call checks)
    // ------------------------------------------------------------------

    /// Walk an expression for the side effects of its sub-checks
    /// (field-resolution, call-arg checks, etc.) and call recursively
    /// into sub-expressions. The expression's inferred type is fed by
    /// [`infer_expr`] when callers need it — this method returns
    /// nothing.
    ///
    /// Side-effect (Phase H): the inferred type of `expr` is recorded
    /// into [`Self::expr_types`] keyed by its span before dispatching.
    /// Together with the recursive descent below, this guarantees every
    /// expression the checker visits has its type captured for later
    /// consumption by the Rust backend.
    #[allow(clippy::only_used_in_recursion)]
    fn check_expr(&mut self, expr: &Expr) {
        // Record this expression's type up-front. Sub-expressions get
        // recorded when their containing check_expr recurses into them.
        let _ = self.infer_and_record(expr);
        match expr {
            Expr::Literal(_) | Expr::Path(_) | Expr::This(_) => {}

            Expr::Field(f) => {
                self.check_expr(&f.object);
                self.check_field_access(f);
            }

            Expr::Index(i) => {
                self.check_expr(&i.array);
                self.check_expr(&i.index);
            }

            Expr::Call(c) => self.check_call(c),

            Expr::NewObject(n) => self.check_new_object(n),

            Expr::NewArray(n) => self.check_expr(&n.size),

            Expr::NewArrayLit(n) => {
                for el in &n.elements {
                    self.check_expr(el);
                }
            }

            Expr::Cast(c) => self.check_expr(&c.value),

            Expr::Range(r) => {
                self.check_expr(&r.start);
                self.check_expr(&r.end);
            }

            Expr::Unary(u) => {
                self.check_expr(&u.operand);
                // §O.3.4 — unary operator on a user type whose
                // matching operator was deleted with `= delete;`.
                if let Some(kind) = op_kind_for_unary(u.op) {
                    let receiver_ty = infer_expr(&u.operand, &self.env, self.symbols);
                    self.check_op_not_deleted(&receiver_ty, kind, u.span);
                }
            }

            Expr::Binary(b) => {
                self.check_expr(&b.left);
                self.check_expr(&b.right);
                // §O.3.4 — binary operator on a user type whose
                // matching operator was deleted with `= delete;`.
                // The receiver is the LHS; that's what determines
                // dispatch per §O.2.6.
                if let Some(kind) = op_kind_for_binary(b.op) {
                    let receiver_ty = infer_expr(&b.left, &self.env, self.symbols);
                    self.check_op_not_deleted(&receiver_ty, kind, b.span);
                }
            }

            Expr::SizeOf(s) => self.check_expr(&s.operand),

            Expr::InterpString(s) => {
                for seg in &s.segments {
                    match seg {
                        InterpSegment::Expr(e) => {
                            self.check_expr(e);
                            // `$"${x}"` interpolates via `operator
                            // string` (which lowers to Display). When
                            // the type's `string` was deleted, that
                            // dispatch isn't available — flag here so
                            // the user gets a Jux diagnostic instead
                            // of a downstream rustc error.
                            let ty = infer_expr(e, &self.env, self.symbols);
                            self.check_op_not_deleted(&ty, OperatorKind::ToString, s.span);
                        }
                        InterpSegment::Bare(ident) => {
                            // `$"$x"` — `x` is a single identifier;
                            // its type is whatever `env` has for it.
                            // Same dispatch through `operator string`.
                            if let Some(ty) = self.env.lookup(&ident.text).cloned() {
                                self.check_op_not_deleted(&ty, OperatorKind::ToString, s.span);
                            }
                        }
                        InterpSegment::Literal(_) => {}
                    }
                }
            }

            Expr::Switch(s) => {
                self.check_expr(&s.scrutinee);
                for arm in &s.arms {
                    match &arm.body {
                        SwitchBody::Expr(e) => self.check_expr(e),
                        SwitchBody::Block(b) => {
                            self.env.push_scope();
                            // The arm's pattern may introduce
                            // bindings; let infer_block declare them so
                            // body expression checks resolve. (Phase
                            // C's walker already does this for variant
                            // bindings; we just reuse it here for the
                            // statements-only walk.)
                            infer_block(b, &mut self.env, self.symbols);
                            self.env.pop_scope();
                        }
                    }
                }
            }
        }
    }

    /// Resolve an `obj.field` access. If the receiver type is a known
    /// class/record AND the field name isn't found anywhere in the
    /// inheritance chain, emit **E0412**. Built-in receivers (arrays,
    /// strings) get an allowlist pass.
    fn check_field_access(&mut self, f: &FieldExpr) {
        let receiver_ty = infer_expr(&f.object, &self.env, self.symbols);
        let field_name = f.field.text.as_str();

        match &receiver_ty {
            // Arrays: allow .length and friends silently.
            Ty::Array { .. } => {
                if BUILTIN_ARRAY_FIELDS.contains(&field_name) {
                    return;
                }
                // Unknown field on array — stay quiet today. A future
                // pass may tighten this.
            }
            // Strings: same allowlist treatment.
            Ty::String => {
                if BUILTIN_STRING_FIELDS.contains(&field_name) {
                    return;
                }
            }
            // User types: walk the inheritance chain looking for the
            // field. Emit E0412 if not found anywhere.
            Ty::User { name, .. } => {
                if self.symbols.lookup_field(name, field_name).is_some() {
                    return;
                }
                // Records: check components directly.
                if let Some(record) = self.symbols.records.get(name) {
                    if record.components.iter().any(|c| c.name == field_name) {
                        return;
                    }
                }
                // Enum variant access (`Color.Red`) lives on the enum
                // itself, not as a "field" in the symbol sense; the
                // receiver-name lookup against env was already Unknown
                // for these in practice, so we should only get here
                // when the receiver is actually a known class/record
                // type. Even so, suppress if the name is a known enum.
                if self.symbols.enums.contains_key(name) {
                    return;
                }
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0412_UnresolvedField,
                        format!("no field `{field_name}` on type `{name}`"),
                    )
                    .with_span(f.span),
                );
            }
            // Param receivers, Unknown, primitives — silent. We don't
            // know enough to flag a problem.
            _ => {}
        }
    }

    /// Resolve a call expression. Three shapes:
    ///
    /// - Bare path `foo(args)` → top-level function. Built-in `print`
    ///   accepts anything.
    /// - `obj.method(args)` → look up `method` on the receiver's class
    ///   (walking the chain). Built-in receivers (arrays, strings) get
    ///   the allowlist treatment. When the receiver carries concrete
    ///   generic args, each parameter type is substituted before
    ///   arg-type checking, so `new Box<int>(...).set("hi")` flags as
    ///   a mismatch instead of silently passing on the `Ty::Param`
    ///   wildcard.
    /// - Anything else → walk sub-expressions only.
    fn check_call(&mut self, c: &CallExpr) {
        // Always walk args first, regardless of callee shape, so nested
        // checks still fire.
        match c.callee.as_ref() {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = &qn.segments[0].text;
                // Built-in functions accept anything.
                if BUILTINS.contains(&name.as_str()) {
                    for arg in &c.args {
                        self.check_expr(arg);
                    }
                    return;
                }
                if let Some(fn_sig) = self.symbols.functions.get(name) {
                    let params = fn_sig.params.clone();
                    // Top-level functions take no per-call receiver
                    // substitution. Function-level generics would feed
                    // through here, but the parser's `<...>` turbofish
                    // syntax for free functions isn't wired up to the
                    // checker yet. `declaring_class = None` because the
                    // params lower against the caller's env, not a
                    // member-owning type.
                    self.check_call_args(name, &params, &c.args, c.span, None, &[], &[]);
                    return;
                }
                // Unknown bare callee — walk args silently. The
                // resolver phase already flagged unresolved names.
                for arg in &c.args {
                    self.check_expr(arg);
                }
            }

            Expr::Field(field) => {
                // Walk the receiver sub-expression first.
                self.check_expr(&field.object);
                let receiver_ty = infer_expr(&field.object, &self.env, self.symbols);
                let method_name = field.field.text.as_str();
                // Built-in receivers: short-circuit.
                if let Ty::Array { .. } = &receiver_ty {
                    if BUILTIN_ARRAY_METHODS.contains(&method_name) {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                }
                if let Ty::String = &receiver_ty {
                    if BUILTIN_STRING_METHODS.contains(&method_name) {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                }
                // Skip method-resolution on Param / Unknown / primitive
                // receivers. We don't have the metadata to do better.
                let (name, generic_args) = match &receiver_ty {
                    Ty::User { name, generic_args } => (name.clone(), generic_args.clone()),
                    _ => {
                        for arg in &c.args {
                            self.check_expr(arg);
                        }
                        return;
                    }
                };
                // Walk the inheritance chain for the method. Substitute
                // generic args only when the method is declared on the
                // receiver's own class — cross-extends substitution is
                // deferred (see `infer.rs` module docs for why).
                if let Some((method, declaring_class)) =
                    self.symbols.lookup_method(&name, method_name)
                {
                    let params = method.params.clone();
                    let owner_on_receiver = declaring_class == name;
                    // Clone the declaring-class name into an owned
                    // String so it outlives the immutable borrow on
                    // `self.symbols` we'd otherwise need.
                    let owner_name = declaring_class.to_string();
                    let subst_params: Vec<TypeParam> = if owner_on_receiver {
                        self.symbols
                            .classes
                            .get(&name)
                            .map(|c| c.generic_params.clone())
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let subst_args: Vec<Ty> = if owner_on_receiver {
                        generic_args
                    } else {
                        Vec::new()
                    };
                    self.check_call_args(
                        method_name,
                        &params,
                        &c.args,
                        c.span,
                        Some(&owner_name),
                        &subst_params,
                        &subst_args,
                    );
                    return;
                }
                // Interfaces — same lookup (no chain). Substitute the
                // interface's generic params against the receiver's
                // args; the interface IS the declaring scope here so
                // there's no cross-extends complication.
                if let Some(iface) = self.symbols.interfaces.get(&name) {
                    if let Some(method) = iface.methods.get(method_name) {
                        let params = method.params.clone();
                        let subst_params = iface.generic_params.clone();
                        self.check_call_args(
                            method_name,
                            &params,
                            &c.args,
                            c.span,
                            Some(&name),
                            &subst_params,
                            &generic_args,
                        );
                        return;
                    }
                }
                // Records: no methods in Turn 1.
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0413_UnresolvedMethod,
                        format!("no method `{method_name}` on type `{name}`"),
                    )
                    .with_span(c.span),
                );
            }

            // Some other callee shape (call-of-call, call-of-index,
            // etc.) — walk sub-expressions only.
            _ => {
                self.check_expr(&c.callee);
                for arg in &c.args {
                    self.check_expr(arg);
                }
            }
        }
    }

    /// Resolve a `new T(args)`. Looks up `T` against classes first,
    /// then records (records have a synthesized canonical constructor
    /// matching their components). Emits **E0413** when the class/record
    /// isn't found, **E0411** on arg-count mismatch, and **E0410** for
    /// each per-argument type mismatch.
    ///
    /// When the user wrote explicit generic args (`new Box<int>(42)`),
    /// each parameter type carrying a `Ty::Param("T")` is substituted
    /// through those args before comparison. `new Box(42)` (no
    /// turbofish) leaves substitution off — the wildcard rule in
    /// [`compatible`] then accepts whatever argument the user passed.
    fn check_new_object(&mut self, n: &NewObjectExpr) {
        // Walk arg expressions for nested checks regardless of resolution.
        for arg in &n.args {
            self.check_expr(arg);
        }
        let class_name = match n.class_name.segments.last() {
            Some(seg) => seg.text.clone(),
            None => return,
        };

        // Lower the explicit generic args (if any) into `Ty`s. Empty
        // when the user wrote the bare `new Box(...)` form.
        let generic_args: Vec<Ty> = n
            .generic_args
            .iter()
            .map(|g| ty_from_ref(g, &self.env, self.symbols))
            .collect();

        if let Some(class) = self.symbols.classes.get(&class_name) {
            // At most one constructor in Turn 1. If there are none, the
            // synthesized default takes zero args.
            let params: Vec<ParamSig> = class
                .constructors
                .first()
                .map(|c| c.params.clone())
                .unwrap_or_default();
            let subst_params = class.generic_params.clone();
            self.check_call_args(
                &class_name,
                &params,
                &n.args,
                n.span,
                Some(&class_name),
                &subst_params,
                &generic_args,
            );
            return;
        }
        if let Some(record) = self.symbols.records.get(&class_name) {
            // Canonical constructor: one param per component.
            let params: Vec<ParamSig> = record
                .components
                .iter()
                .map(|c| ParamSig {
                    name: c.name.clone(),
                    ty: c.ty.clone(),
                })
                .collect();
            let subst_params = record.generic_params.clone();
            self.check_call_args(
                &class_name,
                &params,
                &n.args,
                n.span,
                Some(&class_name),
                &subst_params,
                &generic_args,
            );
            return;
        }
        // Not a known class or record. Stay silent if the resolver
        // already flagged the name (it lands in `resolve` as E0301);
        // emitting a parallel E0413 would be double-counting.
        // (When we have a "no class N" code in the future, swap this
        // for an emit.)
    }

    /// Resolve a `super(args)` invocation inside a constructor body
    /// against the parent class's constructor signature. Reuses
    /// [`Self::check_call_args`] so the same E0410 / E0411 codes apply.
    ///
    /// Substitution: when the child writes `extends Animal<int>`, every
    /// `Ty::Param("T")` in Animal's constructor signature is mapped
    /// through that `int` before comparison. A bare `extends Animal`
    /// (no explicit args) leaves substitution off; the wildcard rule
    /// in [`compatible`] then accepts whatever the user passed.
    ///
    /// Stays silent on shapes Phase E can't decide:
    ///
    /// - Outside a class context (`env.current_class` is `None`) — the
    ///   parser already rejects bare `super(...)`, but be defensive.
    /// - The child has no `extends` clause.
    /// - The parent name doesn't resolve to a known class (extends a
    ///   built-in or an unresolved name — the resolver will have
    ///   already complained about the latter).
    fn check_super_call(&mut self, args: &[Expr], call_span: Span) {
        // Walk arg sub-expressions for nested checks even if we can't
        // resolve the parent — keeps E0410/E0413 from earlier passes
        // firing inside the args.
        for arg in args {
            self.check_expr(arg);
        }

        let Some(child_name) = self.env.current_class.clone() else { return };
        let Some(child) = self.symbols.classes.get(&child_name) else { return };
        let Some(extends) = child.extends.as_ref() else { return };
        let Some(parent_name_seg) = extends.name.segments.last() else { return };
        let parent_name = parent_name_seg.text.clone();

        // Lower the extends-clause generic args. `extends Animal<int>`
        // gives us [Int]; `extends Animal` gives us []. Empty disables
        // substitution per `substitute`'s rules.
        let parent_generic_args: Vec<Ty> = extends
            .generic_args
            .iter()
            .map(|g| ty_from_ref(g, &self.env, self.symbols))
            .collect();

        let Some(parent) = self.symbols.classes.get(&parent_name) else { return };
        let params: Vec<ParamSig> = parent
            .constructors
            .first()
            .map(|c| c.params.clone())
            .unwrap_or_default();
        let subst_params = parent.generic_params.clone();

        // Clone the slice off so we can re-borrow `self` mutably for the
        // arg-check walk without overlapping the immutable borrow above.
        let params_owned = params;
        self.check_call_args(
            &format!("super (={parent_name})"),
            &params_owned,
            args,
            call_span,
            Some(&parent_name),
            &subst_params,
            &parent_generic_args,
        );
    }

    /// Shared core for argument-count + per-argument type-check. Used
    /// by top-level fn calls, method calls, and constructor calls.
    /// Emits **E0411** for count mismatch and **E0410** per-arg.
    ///
    /// `callee_name` is just for diagnostic phrasing.
    ///
    /// `declaring_class` is the name of the type that owns the
    /// parameter list (for member calls and constructors). It lets the
    /// checker lower a parameter `T value` to `Ty::Param("T")` even
    /// when called from outside the declaring class's body, where the
    /// checker's own env wouldn't have `T` registered. Pass `None` for
    /// top-level function calls — those parameters lower against the
    /// caller's env, where free-function generic params would be in
    /// scope (when free-function generics get wired up).
    ///
    /// `subst_params` / `subst_args` carry an optional generic
    /// substitution that's applied to each expected parameter type
    /// before comparison — see [`crate::ty::substitute`] for the
    /// rules. Pass empty slices when no substitution applies (top-level
    /// function calls, calls on non-generic receivers, calls whose
    /// receiver is a raw type).
    #[allow(clippy::too_many_arguments)]
    fn check_call_args(
        &mut self,
        callee_name: &str,
        params: &[ParamSig],
        args: &[Expr],
        call_span: Span,
        declaring_class: Option<&str>,
        subst_params: &[TypeParam],
        subst_args: &[Ty],
    ) {
        if params.len() != args.len() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0411_WrongArgCount,
                    format!(
                        "`{}` expects {} argument{}, got {}",
                        callee_name,
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        args.len(),
                    ),
                )
                .with_span(call_span),
            );
            // Still check the overlapping prefix for type mismatches so
            // the user gets every problem at once.
        }
        for (i, arg) in args.iter().enumerate() {
            self.check_expr(arg);
            let Some(param) = params.get(i) else { break };
            let expected_raw = match declaring_class {
                Some(class) => lower_member_type(&param.ty, class, self.symbols),
                None => ty_from_ref(&param.ty, &self.env, self.symbols),
            };
            let expected = substitute(&expected_raw, subst_params, subst_args);
            let found = infer_expr(arg, &self.env, self.symbols);
            if !compatible(&expected, &found) {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0410_TypeMismatch,
                        format!(
                            "argument {} to `{}`: expected {}, found {}",
                            i + 1,
                            callee_name,
                            expected,
                            found,
                        ),
                    )
                    .with_span(expr_span(arg)),
                );
            }
        }
    }

}

// ============================================================================
// Helpers
// ============================================================================

/// Lower a [`ReturnType`] to a [`Ty`]. Duplicated from `infer.rs` so the
/// checker can use it without exporting an internal helper. `async T`
/// unwraps to `T` (no `Future<T>` wrapper in Phase 1).
fn return_type_to_ty(rt: &ReturnType, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    match rt {
        ReturnType::Void => Ty::Void,
        ReturnType::Type(t) | ReturnType::AsyncType(t) => ty_from_ref(t, env, symbols),
    }
}

/// True iff a condition expression's type is acceptable in a boolean
/// position — exactly `bool` or `Unknown` (suppression).
fn is_boolish(ty: &Ty) -> bool {
    ty.is_unknown() || ty.is_bool()
}

/// Map a [`BinaryOp`] to the [`OperatorKind`] whose deletion would
/// suppress this op. `None` for ops that aren't user-overloadable
/// (logical `&&` / `||`) or that auto-derive from another operator
/// at the Rust level (`!=` derives from `==`, the four ordering ops
/// auto-derive from `<=>`). Phase-1 simplification: only the
/// "primary" operator is checked — if the user deleted `==` but the
/// program writes `a != b`, the deletion goes uncaught here. A future
/// pass can chase the auto-derive graph.
fn op_kind_for_binary(op: BinaryOp) -> Option<OperatorKind> {
    Some(match op {
        BinaryOp::Eq => OperatorKind::Eq,
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
        // !=, comparison, &&, || — skipped (see fn doc).
        _ => return None,
    })
}

/// Map a [`UnaryOp`] to the [`OperatorKind`] whose deletion would
/// suppress this op. `!x` (logical NOT) isn't overloadable per spec
/// §O.2.5.
fn op_kind_for_unary(op: UnaryOp) -> Option<OperatorKind> {
    Some(match op {
        UnaryOp::Neg => OperatorKind::Minus,
        UnaryOp::BitNot => OperatorKind::BitNot,
        UnaryOp::Not => return None,
    })
}

/// Human-readable spelling of an [`OperatorKind`] for diagnostics.
/// Matches the form the user would have written (`==`, `<=>`, `hash`,
/// `string`, …). Mirrors the same helper in `symbol_table.rs`.
fn operator_kind_user_spelling(kind: OperatorKind) -> &'static str {
    match kind {
        OperatorKind::Eq => "==",
        OperatorKind::Cmp => "<=>",
        OperatorKind::Lt => "<",
        OperatorKind::Le => "<=",
        OperatorKind::Gt => ">",
        OperatorKind::Ge => ">=",
        OperatorKind::Hash => "hash",
        OperatorKind::ToString => "string",
        OperatorKind::Plus => "+",
        OperatorKind::Minus => "-",
        OperatorKind::Mul => "*",
        OperatorKind::Div => "/",
        OperatorKind::Rem => "%",
        OperatorKind::BitAnd => "&",
        OperatorKind::BitOr => "|",
        OperatorKind::BitXor => "^",
        OperatorKind::BitNot => "~",
        OperatorKind::Shl => "<<",
        OperatorKind::Shr => ">>",
        OperatorKind::Index => "[]",
        OperatorKind::IndexSet => "[]=",
        OperatorKind::Call => "()",
        OperatorKind::Range => "..",
        OperatorKind::RangeInclusive => "..=",
    }
}

/// Reach into an expression for its span, mirroring the parser's
/// `expr_span`. Synth literals from inference don't carry a span, so
/// `Span::DUMMY` is the fallback.
fn expr_span(e: &Expr) -> Span {
    match e {
        Expr::Literal(_) => Span::DUMMY,
        Expr::Path(qn) => qn.span,
        Expr::Call(c) => c.span,
        Expr::Binary(b) => b.span,
        Expr::Unary(u) => u.span,
        Expr::Range(r) => r.span,
        Expr::Cast(c) => c.span,
        Expr::SizeOf(s) => s.span,
        Expr::NewArray(n) => n.span,
        Expr::NewArrayLit(n) => n.span,
        Expr::Index(i) => i.span,
        Expr::Field(f) => f.span,
        Expr::InterpString(s) => s.span,
        Expr::This(s) => *s,
        Expr::NewObject(n) => n.span,
        Expr::Switch(s) => s.span,
    }
}

/// Type-compatibility predicate. See module docs for the full rule
/// table; the short version:
///
/// - `Unknown` or `Ty::Param` on either side → true (don't cascade).
/// - Exact equality → true.
/// - Unsuffixed-int literal (`Primitive::Int`) widens silently to any
///   numeric primitive on the **expected** side; same story for
///   unsuffixed-float literal (`Primitive::Double`).
/// - Arrays compare element-wise + kind.
/// - User types compare by name + pairwise generic-args.
/// - Everything else: false.
pub(crate) fn compatible(expected: &Ty, found: &Ty) -> bool {
    // Wildcards.
    if expected.is_unknown() || found.is_unknown() {
        return true;
    }
    if matches!(expected, Ty::Param(_)) || matches!(found, Ty::Param(_)) {
        return true;
    }
    // Exact match.
    if expected == found {
        return true;
    }
    match (expected, found) {
        // Default-int / default-float widening — only when the FOUND
        // side is the unsuffixed-literal default. Going the other
        // direction (`int x = 7L;` for instance) is rejected.
        (Ty::Primitive(_), Ty::Primitive(Primitive::Int))
            if expected.is_numeric() && !matches!(expected, Ty::Primitive(Primitive::Bool | Primitive::Char)) =>
        {
            true
        }
        (Ty::Primitive(_), Ty::Primitive(Primitive::Double))
            if matches!(
                expected,
                Ty::Primitive(
                    Primitive::Float
                        | Primitive::F32
                        | Primitive::F64
                        | Primitive::Double,
                ),
            ) =>
        {
            true
        }
        // Arrays — recurse on element and require matching kind.
        (
            Ty::Array { element: e1, kind: k1 },
            Ty::Array { element: e2, kind: k2 },
        ) => k1 == k2 && compatible(e1, e2),
        // User types — same name AND pairwise compatible generic args.
        (
            Ty::User { name: n1, generic_args: a1 },
            Ty::User { name: n2, generic_args: a2 },
        ) => {
            if n1 != n2 {
                return false;
            }
            // Length-mismatch is only a problem if neither side is
            // empty. Empty generic args on one side typically means
            // "user didn't write the args" — be lenient.
            if a1.is_empty() || a2.is_empty() {
                return true;
            }
            if a1.len() != a2.len() {
                return false;
            }
            a1.iter().zip(a2.iter()).all(|(x, y)| compatible(x, y))
        }
        _ => false,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_table::build;
    use juxc_lex::lex;
    use juxc_parse::parse;
    use juxc_source::SourceFile;

    /// Drive lex → parse → symbol-table build → check, returning every
    /// diagnostic emitted across symbol-table + check passes. Caller
    /// filters by code when they want to assert a specific shape.
    fn run(src: &str) -> Vec<Diagnostic> {
        let sf = SourceFile::new("test.jux", src);
        let lex_result = lex(&sf);
        assert!(
            lex_result.diagnostics.is_empty(),
            "lex errors: {:?}",
            lex_result.diagnostics,
        );
        let parse_result = parse(&lex_result.tokens);
        assert!(
            parse_result.diagnostics.is_empty(),
            "parse errors: {:?}",
            parse_result.diagnostics,
        );
        let mut diags = Vec::new();
        let symbols = build(&parse_result.ast, &mut diags);
        let mut checker = Checker::new(&symbols, &mut diags);
        checker.check_unit(&parse_result.ast);
        diags
    }

    /// Convenience: did any diagnostic with `code` fire?
    fn has(diags: &[Diagnostic], wanted: code::Code) -> bool {
        diags.iter().any(|d| d.code == wanted)
    }

    /// Convenience: count diagnostics matching `code`.
    fn count(diags: &[Diagnostic], wanted: code::Code) -> usize {
        diags.iter().filter(|d| d.code == wanted).count()
    }

    /// Bare `return;` in a void function is fine.
    #[test]
    fn void_return_in_void_function_is_ok() {
        let d = run("public void main() { return; }");
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    /// `return 42;` in an int function is fine.
    #[test]
    fn int_return_in_int_function_is_ok() {
        let d = run("public int main() { return 42; }");
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    /// `return "hi";` in an int function → E0410.
    #[test]
    fn string_return_in_int_function_emits_e0410() {
        let d = run(r#"public int main() { return "hi"; }"#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Bare `return;` in a value-returning function → E0410.
    #[test]
    fn bare_return_in_int_function_emits_e0410() {
        let d = run("public int main() { return; }");
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// `if (1) {}` — non-bool condition → E0410.
    #[test]
    fn non_bool_if_condition_emits_e0410() {
        let d = run("public void main() { if (1) {} }");
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// `if (true) {}` is fine.
    #[test]
    fn bool_if_condition_is_ok() {
        let d = run("public void main() { if (true) {} }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// Assigning a String to an int local → E0410.
    #[test]
    fn assign_string_to_int_emits_e0410() {
        let d = run(r#"public void main() { var x = 1; x = "hi"; }"#);
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Assigning an int to an int local is fine.
    #[test]
    fn assign_int_to_int_is_ok() {
        let d = run("public void main() { var x = 1; x = 2; }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// `new Foo(1)` against a zero-arg synthesized constructor → E0411.
    #[test]
    fn wrong_arg_count_to_synth_ctor_emits_e0411() {
        let d = run("public class Foo {} public void main() { var f = new Foo(1); }");
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// `new Foo()` against a 1-arg constructor → E0411.
    #[test]
    fn missing_ctor_arg_emits_e0411() {
        let d = run(
            "public class Foo { public Foo(int x) {} } public void main() { var f = new Foo(); }",
        );
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// `new Foo("hi")` against a 1-int ctor → E0410.
    #[test]
    fn wrong_ctor_arg_type_emits_e0410() {
        let d = run(
            r#"public class Foo { public Foo(int x) {} } public void main() { var f = new Foo("hi"); }"#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Reading a field that doesn't exist → E0412.
    #[test]
    fn unresolved_field_emits_e0412() {
        let d = run(
            "public class Foo { public int x; } \
             public void main() { var f = new Foo(); print(f.y); }",
        );
        assert!(has(&d, code::Code::E0412_UnresolvedField), "{d:?}");
    }

    /// Calling a method that doesn't exist → E0413.
    #[test]
    fn unresolved_method_emits_e0413() {
        let d = run(
            "public class Foo { public int x; public int sum() { return this.x; } } \
             public void main() { new Foo().notThere(); }",
        );
        assert!(has(&d, code::Code::E0413_UnresolvedMethod), "{d:?}");
    }

    /// A method defined on a parent class is resolvable from a child.
    #[test]
    fn inherited_method_resolves() {
        let d = run(
            "public class Animal { public int age() { return 5; } } \
             public class Dog extends Animal {} \
             public void main() { var d = new Dog(); print(d.age()); }",
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// A field defined on a parent class is resolvable from a child.
    #[test]
    fn inherited_field_resolves() {
        let d = run(
            "public class Animal { public int age; } \
             public class Dog extends Animal {} \
             public void main() { var d = new Dog(); print(d.age); }",
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// `print` accepts any single argument shape.
    #[test]
    fn print_is_builtin_no_arg_check() {
        let d = run(
            r#"public void main() { print("x"); print(42); print(true); }"#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// `.push` on a dynamic int array is a built-in receiver method —
    /// no error.
    #[test]
    fn array_push_is_builtin() {
        let d = run(
            "public void main() { var xs = new int[]{1, 2, 3}; xs.push(4); }",
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// `.length` on an int array reads as int — no error.
    #[test]
    fn array_length_is_builtin() {
        let d = run(
            "public void main() { var xs = new int[]{1}; print(xs.length); }",
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Unsuffixed int literal is compatible with typed int locals.
    #[test]
    fn unsuffixed_int_widens_to_i32() {
        let d = run("public void main() { i32 always32 = 7; print(always32); }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// While-loop with bool condition is fine.
    #[test]
    fn bool_while_is_ok() {
        let d = run("public void main() { while (true) { break; } }");
        assert!(d.is_empty(), "{d:?}");
    }

    /// While-loop with non-bool condition → E0410.
    #[test]
    fn non_bool_while_emits_e0410() {
        let d = run("public void main() { while (1) { break; } }");
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Top-level call with wrong number of args → E0411.
    #[test]
    fn top_level_wrong_arg_count_emits_e0411() {
        let d = run(
            "public int add(int a, int b) { return a + b; } \
             public void main() { print(add(1)); }",
        );
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// Top-level call with wrong arg type → E0410. (Note: this exercises
    /// the path even though "Int → String" wouldn't be tolerated.)
    #[test]
    fn top_level_wrong_arg_type_emits_e0410() {
        let d = run(
            r#"public void greet(String name) { print(name); }
               public void main() { greet(42); }"#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Multiple wrong-type args still emit at least one E0410.
    #[test]
    fn multiple_wrong_args_each_emit_e0410() {
        let d = run(
            r#"public void f(String a, String b) {}
               public void main() { f(1, 2); }"#,
        );
        assert!(count(&d, code::Code::E0410_TypeMismatch) >= 2, "{d:?}");
    }

    // ----------------------------------------------------------------
    // Phase E
    // ----------------------------------------------------------------

    /// Phase E.2 — `new Box<int>("hi")` against `Box(T)` substitutes
    /// `T → int` and rejects the String. Before Phase E the param type
    /// was left as `Ty::Param("T")` and the wildcard rule accepted it.
    #[test]
    fn instantiated_ctor_arg_mismatch_emits_e0410() {
        let d = run(
            r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
            }
            public void main() {
                var b = new Box<int>("hi");
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Phase E.2 — same ctor, correct type → no diagnostic.
    #[test]
    fn instantiated_ctor_matching_arg_is_ok() {
        let d = run(
            r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
            }
            public void main() {
                var b = new Box<int>(42);
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Phase E.2 — raw-type construction (`new Box(...)` with no
    /// turbofish) leaves substitution off, so any arg passes.
    #[test]
    fn raw_ctor_accepts_any_arg() {
        let d = run(
            r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
            }
            public void main() {
                var a = new Box(42);
                var b = new Box("hi");
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Phase E.2 — method call on `Box<int>` substitutes the parameter
    /// type, so passing a String to a `set(T v)` is rejected.
    #[test]
    fn instantiated_method_arg_mismatch_emits_e0410() {
        let d = run(
            r#"
            public class Box<T> {
                public T value;
                public Box(T value) { this.value = value; }
                public void set(T v) { this.value = v; }
            }
            public void main() {
                var b = new Box<int>(0);
                b.set("hi");
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Phase E.3 — `super("hi")` against a parent expecting `int`
    /// emits E0410.
    #[test]
    fn super_call_wrong_arg_type_emits_e0410() {
        let d = run(
            r#"
            public class Animal {
                public Animal(int age) {}
            }
            public class Dog extends Animal {
                public Dog() { super("hi"); }
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Phase E.3 — `super(42)` against `Animal(int)` is fine.
    #[test]
    fn super_call_matching_args_is_ok() {
        let d = run(
            r#"
            public class Animal {
                public Animal(int age) {}
            }
            public class Dog extends Animal {
                public Dog() { super(42); }
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Phase E.3 — `super()` (no args) against `Animal(int age)` is a
    /// wrong-arg-count, emits E0411.
    #[test]
    fn super_call_wrong_arg_count_emits_e0411() {
        let d = run(
            r#"
            public class Animal {
                public Animal(int age) {}
            }
            public class Dog extends Animal {
                public Dog() { super(); }
            }
            "#,
        );
        assert!(has(&d, code::Code::E0411_WrongArgCount), "{d:?}");
    }

    /// Phase E.3 — `super(name)` with substitution through the extends
    /// clause's generic arg. Animal<T> with `Animal(T name)` lets Dog
    /// (extends Animal<String>) pass a String.
    #[test]
    fn super_call_substitutes_extends_generic_arg() {
        let d = run(
            r#"
            public class Animal<T> {
                public Animal(T name) {}
            }
            public class Dog extends Animal<String> {
                public Dog() { super("rex"); }
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    // ----------------------------------------------------------------
    // Operator body checks (§O.2)
    // ----------------------------------------------------------------

    /// A well-formed `operator==` body type-checks cleanly: `this` and
    /// the formal parameter are in scope, the return type matches.
    /// Also defines the paired `operator hash` — without it the §O.2.7
    /// pairing rule would fire `E0931`.
    #[test]
    fn operator_eq_body_typechecks_cleanly() {
        let d = run(
            r#"
            public class Path {
                public String value;
                public Path(String v) { this.value = v; }
                public bool operator==(Path other) {
                    return true;
                }
                public int operator hash() {
                    return 0;
                }
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    /// Returning the wrong type from an operator body fires E0410 via
    /// the same path methods use — the operator walker sets
    /// `current_return` to the declared return type before walking.
    #[test]
    fn operator_return_type_mismatch_emits_e0410() {
        let d = run(
            r#"
            public class Path {
                public bool operator==(Path other) {
                    return 42;
                }
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// Calling a method with the wrong-typed argument from inside an
    /// operator body still fires the standard E0410 path — proves the
    /// arg-type check path is reachable from within an operator body.
    #[test]
    fn operator_body_call_arg_mismatch_emits_e0410() {
        let d = run(
            r#"
            public class Path {
                public String value;
                public Path(String v) { this.value = v; }
                public void greet(String name) {}
                public bool operator==(Path other) {
                    this.greet(42);
                    return true;
                }
            }
            "#,
        );
        assert!(has(&d, code::Code::E0410_TypeMismatch), "{d:?}");
    }

    /// `operator hash()` is zero-arg and its body type-checks cleanly
    /// when the return type matches.
    #[test]
    fn operator_hash_body_typechecks_cleanly() {
        let d = run(
            r#"
            public class Path {
                public int operator hash() {
                    return 1;
                }
            }
            "#,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    // ----------------------------------------------------------------
    // E0935 — use-of-deleted-operator (§O.3.4)
    // ----------------------------------------------------------------

    /// `$"$t"` on a record whose `operator string()` is deleted fires
    /// E0935 at the interp-string site.
    #[test]
    fn interp_string_on_deleted_string_op_emits_e0935() {
        let d = run(
            r#"
            public record OpaqueToken(int secret) {
                public String operator string() = delete;
            }
            public void main() {
                var t = new OpaqueToken(42);
                print($"$t");
            }
            "#,
        );
        assert!(has(&d, code::Code::E0935_DeletedOperator), "{d:?}");
    }

    /// `a + b` where `a`'s class deleted `operator+` fires E0935.
    #[test]
    fn arithmetic_on_deleted_op_emits_e0935() {
        let d = run(
            r#"
            public class M {
                public int x;
                public M(int x) { this.x = x; }
                public M operator+(M other) = delete;
            }
            public void main() {
                var a = new M(1);
                var b = new M(2);
                var c = a + b;
            }
            "#,
        );
        assert!(has(&d, code::Code::E0935_DeletedOperator), "{d:?}");
    }

    /// `-x` where `x`'s class deleted unary `operator-` fires E0935.
    #[test]
    fn unary_minus_on_deleted_op_emits_e0935() {
        let d = run(
            r#"
            public class N {
                public int x;
                public N(int x) { this.x = x; }
                public N operator-() = delete;
            }
            public void main() {
                var v = new N(1);
                var w = -v;
            }
            "#,
        );
        assert!(has(&d, code::Code::E0935_DeletedOperator), "{d:?}");
    }

    /// Primitives + non-deleted classes don't fire E0935. Pins that the
    /// check is gated on receiver class + deletion flag.
    #[test]
    fn no_e0935_for_primitives_or_undeleted() {
        let d = run(
            r#"
            public class M {
                public int x;
                public M(int x) { this.x = x; }
                public bool operator==(M other) { return true; }
            }
            public void main() {
                var a = 1 + 2;
                var b = new M(1);
                var c = new M(2);
                var eq = b == c;
            }
            "#,
        );
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0935_DeletedOperator),
            "should not emit E0935: {d:?}",
        );
    }

    /// `$"$x"` where x is a primitive (int, String, etc.) doesn't fire
    /// E0935 — primitives don't have an operator-string declaration.
    #[test]
    fn no_e0935_for_primitive_in_interp() {
        let d = run(
            r#"
            public void main() {
                var x = 42;
                var s = "hi";
                print($"x=$x, s=$s");
            }
            "#,
        );
        assert!(
            !d.iter().any(|d| d.code == code::Code::E0935_DeletedOperator),
            "should not emit E0935 for primitive: {d:?}",
        );
    }
}
