//! Switch-expression and pattern lowering.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::Literal;

use crate::analysis::pattern_has_parens;
use crate::RustEmitter;
use juxc_lex::to_rust_ident;

impl RustEmitter {
    /// Lower a `switch` expression to a Rust `match`. The same node
    /// covers both expression-form (`var y = switch(…) {…}`) and
    /// statement-form (`switch(…) {…}`) — Rust's `match` is always an
    /// expression, so the lowering is identical.
    ///
    /// Each arm becomes `pattern => body,`. Block bodies emit as
    /// `pattern => { stmts… },`. Expression-bodies emit naked.
    /// The type a switch expression's numeric arms meet in (§S.2.6), when
    /// they are all value arms and not all one type already; `None` when no
    /// arm needs a cast. The arm types are the checker's, recorded with each
    /// arm's bindings in scope.
    fn switch_arm_widen_target(&self, s: &juxc_ast::SwitchExpr) -> Option<juxc_tycheck::Primitive> {
        let mut arms: Vec<(&juxc_ast::Expr, juxc_tycheck::Ty)> = Vec::new();
        for arm in &s.arms {
            let juxc_ast::SwitchBody::Expr(e) = &arm.body else { return None };
            let ty = match self.operand_primitive(e) {
                Some(p) => juxc_tycheck::Ty::Primitive(p),
                // A `throw` arm never produces a value.
                None if matches!(&**e, juxc_ast::Expr::Throw(..)) => juxc_tycheck::Ty::Unknown,
                None => return None,
            };
            arms.push((e, ty));
        }
        if arms.len() < 2 {
            return None;
        }
        match juxc_tycheck::infer::unify_numeric_arms(&arms) {
            juxc_tycheck::infer::ArmsNumeric::Meet(p) => Some(p),
            _ => None,
        }
    }

    pub(crate) fn emit_switch(&mut self, s: &juxc_ast::SwitchExpr) {
        // Numeric arms meet in one type (§S.2.6), as a `? :`'s do: a narrower
        // arm is cast up to it, since a Rust `match` needs every arm to agree.
        let arm_widen = self.switch_arm_widen_target(s);
        // When the surrounding context requires `Option<T>` (the
        // `emitting_nullable_target` flag is set, currently fired
        // by `emit_tail_stmt` for a `T?`-returning fn), push the
        // `Some(...)` wrap into each arm body so mixed
        // `T` / `null` arms unify cleanly. We clear the flag
        // BEFORE walking each arm body so a nested switch inside
        // an arm doesn't re-wrap; the arm-body context resets.
        let wrap_each_arm = self.emitting_nullable_target;
        let prev_nullable_target = self.emitting_nullable_target;
        self.emitting_nullable_target = false;
        // Resolve the scrutinee's enum (if any) so bare `case Variant ->`
        // labels qualify to `Enum::Variant`. Saved/restored for nested switches.
        let prev_switch_enum = self.current_switch_enum.take();
        self.current_switch_enum = self.scrutinee_enum_bare(&s.scrutinee);
        // A `String`-typed scrutinee matches against `&str` literal patterns
        // (`case "a" ->`), so we match on `.as_str()` — `match s.as_str() {
        // "a" => … }` — rather than the owned `String` (which Rust won't
        // compare against `&str` patterns).
        let scrut_is_string = self.scrutinee_is_string(&s.scrutinee);
        // A NULLABLE scrutinee (`String? s`, `int? n`) is an `Option`: `case
        // null` is `None`, and every literal case is that value inside
        // `Some`. A `String?` matches through `.as_deref()` so the literal is
        // a `&str` pattern, as it is for a plain `String`.
        let scrut_nullable = self.scrutinee_nullable_inner(&s.scrutinee);
        // A scrutinee typed as an interface or an open base class is a trait
        // object, and `case Ins i ->` asks for its runtime type. Rust has no
        // pattern for that, so such an arm binds the value by reference and
        // tests it in the guard through the `__jux_as_<T>` hook; a sealed
        // hierarchy lowers to an enum and keeps its variant patterns.
        let dyn_scrutinee = self
            .cast_source_bare(&s.scrutinee)
            .filter(|bare| self.source_is_dyn(bare));
        // An `any` scrutinee (§T.1.2) is matched by type the same way, through
        // the `JuxAny` getter rather than a hook.
        let any_scrutinee = self.expr_is_any(&s.scrutinee);
        let scrutinee_mark = self.w.len();
        self.emit_expr(&s.scrutinee);
        // Enum `&self` method dispatch: clone the receiver so payload
        // binders own their values (`&T` wouldn't satisfy a generic
        // `-> T` return). Enums always derive Clone. A tuple or record
        // pattern that would move a part out of a place matches a copy.
        if (self.in_enum_method && matches!(&*s.scrutinee, juxc_ast::Expr::This(_)))
            || self.switch_moves_out_of_place(s)
        {
            self.w.push_str(".clone()");
        }
        // A scrutinee read through a guard (`self.0.borrow().mode.clone()`)
        // is a temporary of the whole `match`, and Rust keeps it alive until
        // the match ends, so an arm that writes the same object (`mode =
        // Mode.On;`, or a call to a method that does) panicked "already
        // borrowed". Reading it into a `let` first drops the guard before any
        // arm runs, which is also when Java evaluates the scrutinee.
        let scrutinee = self.w.split_off_from(scrutinee_mark);
        let hoist_scrutinee = scrutinee.contains(".borrow()") || scrutinee.contains(".borrow_mut()");
        if hoist_scrutinee {
            self.w.push_str("{ let __jux_scrutinee = ");
            self.w.push_str(&scrutinee);
            // A field read through the guard is a place: the `let` moves it,
            // which a `String` or payload enum cannot do out of a borrow, so
            // it takes a copy. A call result is already owned.
            if !scrutinee.ends_with(')') && !self.scrutinee_is_copy(&s.scrutinee) {
                self.w.push_str(".clone()");
            }
            self.w.push_str("; match __jux_scrutinee");
        } else {
            self.w.push_str("match ");
            self.w.push_str(&scrutinee);
        }
        if scrut_is_string {
            self.w.push_str(".as_str()");
        } else if matches!(scrut_nullable, Some(juxc_tycheck::Ty::String)) {
            self.w.push_str(".as_deref()");
        }
        self.w.push_str(" {\n");
        for arm in &s.arms {
            // Modest indent — switch is usually nested at depth >= 1
            // inside a function body. Two spaces of leading indent
            // keeps the match contents readable while the surrounding
            // emit_block indenter handles outer alignment. The raw
            // 4-space / 8-space prefixes are intentionally **not**
            // bound to the writer's current `indent_level` — they
            // represent the match arms' depth relative to the `match`
            // keyword itself, which was emitted naked above.
            self.w.push_str("    ");
            let prev_guards = std::mem::take(&mut self.pattern_string_guards);
            // The runtime type test as an `Option`-producing expression on the
            // matched value, and the pattern its `Some` takes apart: the binder
            // of `case Circle c`, or the struct pattern of `case Circle(var r)`
            // over an interface the record implements (LANG-V1 §7.5).
            let runtime_type_test = match (&arm.pattern, &dyn_scrutinee) {
                (juxc_ast::Pattern::TypeBind { type_name, binder, .. }, _) if any_scrutinee => {
                    let target = crate::analysis::synth_iface_type_ref(&type_name.text, type_name.span);
                    Some((self.any_getter_text("__jux_subject", &target), to_rust_ident(&binder.text)))
                }
                (juxc_ast::Pattern::TypeBind { type_name, binder, .. }, Some(source))
                    if &type_name.text != source =>
                {
                    Some((format!("__jux_subject.__jux_as_{}()", type_name.text), to_rust_ident(&binder.text)))
                }
                (juxc_ast::Pattern::EnumVariant { span, .. }, Some(_))
                    if self.symbols.record_patterns.contains_key(span) =>
                {
                    let fqn = self.symbols.record_patterns[span].clone();
                    let bare = fqn.rsplit('.').next().unwrap_or(&fqn).to_string();
                    let mark = self.w.len();
                    self.emit_pattern(&arm.pattern);
                    let destructure = self.w.split_off_from(mark);
                    Some((format!("__jux_subject.__jux_as_{bare}()"), destructure))
                }
                _ => None,
            };
            // `case Circle(var r) | Ring(var r, _)` over a trait object: one
            // runtime test per alternative, tried in source order (§A.3).
            let or_type_tests = match (&arm.pattern, &dyn_scrutinee) {
                (juxc_ast::Pattern::Or(alts, _), Some(source))
                    if !any_scrutinee && alts.iter().all(|alt| self.is_runtime_type_alt(alt, source)) =>
                {
                    self.or_runtime_type_tests(alts)
                }
                _ => Vec::new(),
            };
            // Whether the runtime test's `Some(..)` can fail on its own: a
            // record pattern with a literal inside (`Circle(0.0)`) can, a bare
            // binder cannot.
            let refutable_inner = matches!(&arm.pattern, juxc_ast::Pattern::EnumVariant { .. })
                && runtime_type_test.is_some();
            if runtime_type_test.is_some() || !or_type_tests.is_empty() {
                self.w.push_str("ref __jux_subject");
            } else if scrut_nullable.is_some() {
                self.emit_nullable_scrutinee_pattern(&arm.pattern);
            } else {
                self.emit_pattern(&arm.pattern);
            }
            // The arm's bindings shadow any outer name for its guard and body,
            // and a binding typed `T?` is an `Option` the way a nullable local
            // is (`null` when printed, not `None`).
            let binders = self.typed_pattern_binders(&arm.pattern);
            // Their types go in scope for the guard and body as well, so a
            // `double` binder printed through interpolation keeps its decimal
            // point (`warn(50.0)`): string interpolation reads a bare name's
            // type from `local_types`.
            self.local_types.push(
                binders
                    .iter()
                    .filter(|(_, ty)| !matches!(ty, juxc_tycheck::Ty::Unknown))
                    .map(|(name, ty)| (name.clone(), ty.clone()))
                    .collect(),
            );
            let shadowed_nullables: Vec<(String, bool)> = binders
                .iter()
                .map(|(name, ty)| {
                    let was = self.nullable_locals.contains(name);
                    if matches!(ty, juxc_tycheck::Ty::Nullable(_)) {
                        self.nullable_locals.insert(name.clone());
                    } else {
                        self.nullable_locals.remove(name);
                    }
                    (name.clone(), was)
                })
                .collect();
            // `when <cond>` guard (§A.2.8) → Rust match guard
            // `if <cond>`. Pattern bindings are in scope.
            // A string literal nested in a tuple or record pattern came back
            // as a binder; its comparison leads the guard.
            let string_guards = std::mem::replace(&mut self.pattern_string_guards, prev_guards);
            if let Some((getter, binder)) = &runtime_type_test {
                // `case Ins i when i.ok() ->` needs `i` inside the guard, and
                // edition 2021 has no `if let` guards, so the test binds it
                // in a `match` of its own. A string nested in a record pattern
                // compares inside that `match`, where its binder exists.
                self.w.push_str(" if ");
                let mut inner_guards = String::new();
                for (b, lit) in &string_guards {
                    if !inner_guards.is_empty() {
                        inner_guards.push_str(" && ");
                    }
                    let mark = self.w.len();
                    self.emit_rust_string_literal(lit);
                    let text = self.w.split_off_from(mark);
                    inner_guards.push_str(&format!("{b} == {text}"));
                }
                match (&arm.guard, inner_guards.is_empty(), refutable_inner) {
                    (Some(guard), _, _) => {
                        self.w.push_str(&format!("match {getter} {{ Some({binder})"));
                        if !inner_guards.is_empty() {
                            self.w.push_str(&format!(" if {inner_guards}"));
                        }
                        self.w.push_str(" => ");
                        self.emit_expr(guard);
                        self.w.push_str(", _ => false }");
                    }
                    (None, true, false) => self.w.push_str(&format!("{getter}.is_some()")),
                    (None, true, true) => self.w.push_str(&format!("matches!({getter}, Some({binder}))")),
                    (None, false, _) => {
                        self.w.push_str(&format!("matches!({getter}, Some({binder}) if {inner_guards})"));
                    }
                }
            }
            if !or_type_tests.is_empty() {
                self.w.push_str(" if ");
                self.emit_or_type_test_guard(&or_type_tests, arm.guard.as_ref());
            }
            let string_guards = if runtime_type_test.is_some() { Vec::new() } else { string_guards };
            for (i, (binder, literal)) in string_guards.iter().enumerate() {
                self.w.push_str(if i == 0 { " if " } else { " && " });
                self.w.push_str(binder);
                self.w.push_str(" == ");
                self.emit_rust_string_literal(literal);
            }
            if let Some(guard) = arm.guard.as_ref().filter(|_| runtime_type_test.is_none() && or_type_tests.is_empty()) {
                if string_guards.is_empty() {
                    self.w.push_str(" if ");
                    self.emit_expr(guard);
                } else {
                    self.w.push_str(" && (");
                    self.emit_expr(guard);
                    self.w.push(')');
                }
            }
            self.w.push_str(" => ");
            // Recursive-enum binders bound to a boxed slot need a one-time
            // unbox (`let l = *l;`) so the arm body sees a plain enum value
            // (the decl boxed the self-referential slot to avoid E0072).
            let rebinds = self.boxed_recursive_binders(&arm.pattern);
            // The arm matched, so the hook answers `Some`: bind its value.
            let downcast_let = runtime_type_test
                .as_ref()
                .map(|(getter, binder)| format!("let Some({binder}) = {getter} else {{ unreachable!() }};"))
                .or_else(|| or_type_test_let(&or_type_tests, &binders));
            match &arm.body {
                juxc_ast::SwitchBody::Expr(e) => {
                    // Per-arm nullable wrap: skip when the value
                    // is already `null` (a `Literal::Null` lowers
                    // to `None`) or already nullable-shaped (the
                    // generic-arg helper recognizes paths to
                    // nullable locals and `?.`-chain results).
                    let wrap = wrap_each_arm
                        && !matches!(&**e, juxc_ast::Expr::Literal(juxc_ast::Literal::Null))
                        && !self.expression_is_already_nullable(e);
                    // An expression-bodied arm with boxed binders becomes a
                    // block so the unbox `let`s can precede the value.
                    if !rebinds.is_empty() || downcast_let.is_some() {
                        self.w.push_str("{ ");
                        for b in &rebinds {
                            self.w.push_str(&format!("let {b} = *{b}; "));
                        }
                        if let Some(bind) = &downcast_let {
                            self.w.push_str(bind);
                            self.w.push(' ');
                        }
                    }
                    if wrap {
                        self.w.push_str("Some(");
                    }
                    // An untyped literal adapts to an arm type of its own kind
                    // (Rust infers it); an integer literal in a float switch
                    // still needs the cast.
                    let widen = arm_widen.filter(|p| {
                        let float_target = juxc_tycheck::ty::is_float_primitive(*p);
                        let adapts = if float_target {
                            juxc_tycheck::infer::untyped_float_literal(e)
                        } else {
                            juxc_tycheck::infer::untyped_int_literal(e)
                        };
                        self.operand_primitive(e) != Some(*p) && !adapts
                    });
                    if widen.is_some() {
                        self.w.push('(');
                        // `as` binds tighter than any binary operator:
                        // `(n * 2) as i64`, never `n * 2 as i64`.
                        self.emit_expr_with_parent_prec(e, crate::exprs::UNARY_PREC, false);
                    } else {
                        self.emit_expr(e);
                    }
                    if let Some(p) = widen {
                        self.w.push_str(" as ");
                        self.w.push_str(crate::exprs::rust_primitive_name(p));
                        self.w.push(')');
                    }
                    if wrap {
                        self.w.push(')');
                    }
                    if !rebinds.is_empty() || downcast_let.is_some() {
                        self.w.push_str(" }");
                    }
                }
                juxc_ast::SwitchBody::Block(b) => {
                    self.w.push_str("{\n");
                    for bind in &rebinds {
                        self.w.push_str(&format!("        let {bind} = *{bind};\n"));
                    }
                    if let Some(bind) = &downcast_let {
                        self.w.push_str("        ");
                        self.w.push_str(bind);
                        self.w.push('\n');
                    }
                    // Statements inside a block-bodied arm sit at the
                    // arm-depth + 1 (two levels of 4-space prefix from
                    // the surrounding `match`). We emit the indent
                    // explicitly and delegate to `emit_stmt` for the
                    // text itself — `emit_stmt` no longer takes an
                    // indent parameter, so any nested `if` / `while`
                    // inside the arm relies on the writer's current
                    // `indent_level` for further nesting. We don't
                    // adjust that here: the surrounding emitter set
                    // it to the function-body depth, which gives
                    // sensible (if not arithmetically perfect) nested
                    // indents in the rare deeply-nested case.
                    for stmt in &b.statements {
                        self.w.push_str("        ");
                        self.emit_stmt(stmt);
                    }
                    self.w.push_str("    ");
                    self.w.push('}');
                }
            }
            for (name, was) in shadowed_nullables.into_iter().rev() {
                if was {
                    self.nullable_locals.insert(name);
                } else {
                    self.nullable_locals.remove(&name);
                }
            }
            self.local_types.pop();
            self.w.push_str(",\n");
        }
        // Type patterns over a trait object are guards, which Rust does not
        // count toward exhaustiveness. The checker has already proved the arms
        // cover every permitted type of a sealed interface (E0440 otherwise),
        // so the arm Rust asks for can never run.
        let tests_runtime_type = dyn_scrutinee.as_ref().is_some_and(|source| {
            s.arms.iter().any(|arm| match &arm.pattern {
                juxc_ast::Pattern::TypeBind { type_name, .. } => &type_name.text != source,
                juxc_ast::Pattern::EnumVariant { span, .. } => self.symbols.record_patterns.contains_key(span),
                juxc_ast::Pattern::Or(alts, _) => alts.iter().all(|alt| self.is_runtime_type_alt(alt, source)),
                _ => false,
            })
        });
        let has_catch_all = s.arms.iter().any(|arm| {
            arm.guard.is_none()
                && matches!(&arm.pattern, juxc_ast::Pattern::Wildcard(_) | juxc_ast::Pattern::Bind(_))
        });
        if tests_runtime_type && !has_catch_all {
            self.w.push_str("    _ => unreachable!(\"every permitted type has an arm\"),\n");
        }
        self.w.push('}');
        if hoist_scrutinee {
            self.w.push_str(" }");
        }
        self.emitting_nullable_target = prev_nullable_target;
        self.current_switch_enum = prev_switch_enum;
    }

    /// The bare enum name a switch scrutinee resolves to, or `None` when the
    /// scrutinee isn't an enum. Consults `expr_types` (span-keyed) first, then
    /// the name-keyed `local_types` (params/locals) for a bare path.
    /// Whether matching `s` would move a non-`Copy` part out of a place the
    /// program still owns: a tuple or record pattern binding a `String`,
    /// record or object (or a nested string literal, which binds one to
    /// compare), over a variable read again later or over a field.
    ///
    /// The match then runs on a copy. A variable whose last use is this
    /// switch, and a temporary such as `(a, b)`, are matched as they are.
    fn switch_moves_out_of_place(&self, s: &juxc_ast::SwitchExpr) -> bool {
        let place = match &*s.scrutinee {
            juxc_ast::Expr::Path(qn) if qn.segments.len() == 1 => {
                self.non_final_uses.contains(&qn.span)
                    || self.ref_locals.contains(qn.segments[0].text.as_str())
            }
            juxc_ast::Expr::Field(_) | juxc_ast::Expr::This(_) | juxc_ast::Expr::Index(_) => true,
            _ => false,
        };
        place && s.arms.iter().any(|arm| self.pattern_binds_owned_part(&arm.pattern, 0))
    }

    /// The names a tuple or record pattern binds that the checker typed, each
    /// with its type. Only those patterns' bindings are typed (grammar §A.3);
    /// the others come back empty and change nothing.
    fn typed_pattern_binders(&self, p: &juxc_ast::Pattern) -> Vec<(String, juxc_tycheck::Ty)> {
        let mut out = Vec::new();
        self.collect_typed_binders(p, &mut out);
        out
    }

    fn collect_typed_binders(&self, p: &juxc_ast::Pattern, out: &mut Vec<(String, juxc_tycheck::Ty)>) {
        match p {
            juxc_ast::Pattern::Bind(name) | juxc_ast::Pattern::TypeBind { binder: name, .. } => {
                if let Some(ty) = self.expr_types.get(&name.span) {
                    out.push((name.text.clone(), ty.clone()));
                }
            }
            juxc_ast::Pattern::Tuple(parts, _) | juxc_ast::Pattern::EnumVariant { args: parts, .. } => {
                for part in parts {
                    self.collect_typed_binders(part, out);
                }
            }
            // Every alternative binds the same names at the same types (§A.3,
            // E0447), so the first one speaks for all.
            juxc_ast::Pattern::Or(alts, _) => {
                if let Some(first) = alts.first() {
                    self.collect_typed_binders(first, out);
                }
            }
            _ => {}
        }
    }

    /// Whether `alt`, one alternative of an or-pattern over a trait object
    /// whose static type is `source`, asks for a runtime type: a type pattern
    /// naming another type (`Circle c`), or a record pattern (`Circle(var r)`).
    fn is_runtime_type_alt(&self, alt: &juxc_ast::Pattern, source: &str) -> bool {
        match alt {
            juxc_ast::Pattern::TypeBind { type_name, .. } => type_name.text != source,
            juxc_ast::Pattern::EnumVariant { span, .. } => self.symbols.record_patterns.contains_key(span),
            _ => false,
        }
    }

    /// One runtime type test per or-pattern alternative over a trait object,
    /// in source order (see [`RuntimeTypeAlt`]).
    fn or_runtime_type_tests(&mut self, alts: &[juxc_ast::Pattern]) -> Vec<RuntimeTypeAlt> {
        let outer_guards = std::mem::take(&mut self.pattern_string_guards);
        let mut tests = Vec::new();
        for alt in alts {
            match alt {
                juxc_ast::Pattern::TypeBind { type_name, binder, .. } => tests.push(RuntimeTypeAlt {
                    getter: format!("__jux_subject.__jux_as_{}()", type_name.text),
                    destructure: to_rust_ident(&binder.text),
                    compares: String::new(),
                    refutable: false,
                }),
                juxc_ast::Pattern::EnumVariant { span, args, .. } => {
                    let fqn = self.symbols.record_patterns[span].clone();
                    let bare = fqn.rsplit('.').next().unwrap_or(&fqn).to_string();
                    let mark = self.w.len();
                    self.emit_pattern(alt);
                    let destructure = self.w.split_off_from(mark);
                    // A string literal nested in the record came back as a
                    // binder; its comparison belongs to this alternative only.
                    let mut compares = String::new();
                    for (b, lit) in std::mem::take(&mut self.pattern_string_guards) {
                        if !compares.is_empty() {
                            compares.push_str(" && ");
                        }
                        let mark = self.w.len();
                        self.emit_rust_string_literal(&lit);
                        let text = self.w.split_off_from(mark);
                        compares.push_str(&format!("{b} == {text}"));
                    }
                    tests.push(RuntimeTypeAlt {
                        getter: format!("__jux_subject.__jux_as_{bare}()"),
                        destructure,
                        compares,
                        refutable: args.iter().any(pattern_can_fail),
                    });
                }
                _ => {}
            }
        }
        self.pattern_string_guards = outer_guards;
        tests
    }

    /// The match guard of an or-pattern arm over a trait object. With no
    /// `when`, any alternative's test passing is enough:
    ///
    /// ```text
    /// __jux_subject.__jux_as_Circle().is_some() || __jux_subject.__jux_as_Ring().is_some()
    /// ```
    ///
    /// A `when` guard reads the bindings, so it runs under the FIRST
    /// alternative that matches, whose bindings are the ones in scope:
    ///
    /// ```text
    /// match __jux_subject.__jux_as_Circle() { Some(Circle { r }) => r > 1.0, _ => match … }
    /// ```
    fn emit_or_type_test_guard(&mut self, tests: &[RuntimeTypeAlt], guard: Option<&juxc_ast::Expr>) {
        let Some(guard) = guard else {
            for (i, t) in tests.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(" || ");
                }
                let (getter, destructure) = (&t.getter, &t.destructure);
                if t.compares.is_empty() && !t.refutable {
                    self.w.push_str(&format!("{getter}.is_some()"));
                } else if t.compares.is_empty() {
                    self.w.push_str(&format!("matches!({getter}, Some({destructure}))"));
                } else {
                    self.w.push_str(&format!("matches!({getter}, Some({destructure}) if {})", t.compares));
                }
            }
            return;
        };
        for t in tests {
            self.w.push_str(&format!("match {} {{ Some({})", t.getter, t.destructure));
            if !t.compares.is_empty() {
                self.w.push_str(&format!(" if {}", t.compares));
            }
            self.w.push_str(" => ");
            self.emit_expr(guard);
            self.w.push_str(", _ => ");
        }
        self.w.push_str("false");
        for _ in tests {
            self.w.push_str(" }");
        }
    }

    fn pattern_binds_owned_part(&self, p: &juxc_ast::Pattern, depth: usize) -> bool {
        match p {
            juxc_ast::Pattern::Bind(name) => {
                depth > 0
                    && self
                        .expr_types
                        .get(&name.span)
                        .is_some_and(|ty| self.ty_needs_clone_on_field_read(ty))
            }
            juxc_ast::Pattern::Literal(Literal::String(_), _) => depth > 0,
            juxc_ast::Pattern::Tuple(parts, _) | juxc_ast::Pattern::EnumVariant { args: parts, .. } => {
                parts.iter().any(|sub| self.pattern_binds_owned_part(sub, depth + 1))
            }
            juxc_ast::Pattern::Or(alts, _) => {
                alts.iter().any(|alt| self.pattern_binds_owned_part(alt, depth))
            }
            _ => false,
        }
    }

    fn scrutinee_enum_bare(&self, scrutinee: &juxc_ast::Expr) -> Option<String> {
        // Gather every type the scrutinee might carry — `expr_types` (which can
        // be `Unknown` for a param) AND the name-keyed `local_types` — and
        // return the first that names a known enum.
        let mut candidates: Vec<juxc_tycheck::Ty> = Vec::new();
        if let Some(t) = self.expr_types.get(&crate::exprs::expr_span_of(scrutinee)) {
            candidates.push(t.clone());
        }
        if let juxc_ast::Expr::Path(qn) = scrutinee {
            if qn.segments.len() == 1 {
                if let Some(t) = self
                    .local_types
                    .iter()
                    .rev()
                    .find_map(|s| s.get(&qn.segments[0].text))
                {
                    candidates.push(t.clone());
                }
            }
        }
        for ty in candidates {
            let juxc_tycheck::Ty::User { name, .. } = ty else {
                continue;
            };
            let bare = name.rsplit('.').next().unwrap_or(&name).to_string();
            let is_enum = self.symbols.enums.contains_key(&name)
                || self
                    .symbols
                    .enums
                    .keys()
                    .any(|k| k.rsplit('.').next() == Some(bare.as_str()));
            if is_enum {
                return Some(bare);
            }
        }
        None
    }

    /// Whether a `switch` scrutinee's type is `Copy` (a primitive, or an enum
    /// whose derive adds `Copy`), so a hoisted read of it needs no `.clone()`.
    fn scrutinee_is_copy(&self, scrutinee: &juxc_ast::Expr) -> bool {
        match self.expr_types.get(&crate::exprs::expr_span_of(scrutinee)) {
            Some(juxc_tycheck::Ty::Primitive(_)) => true,
            Some(juxc_tycheck::Ty::User { name, generic_args }) => {
                generic_args.is_empty() && self.enum_is_copy(name)
            }
            _ => false,
        }
    }

    /// True when the switch scrutinee is `String`-typed (so the `match` should
    /// be on `.as_str()` to compare against `&str` literal patterns). Mirrors
    /// [`Self::scrutinee_enum_bare`]'s type resolution.
    /// The value type inside a nullable scrutinee (`String` for `String? s`),
    /// or `None` when the scrutinee is not nullable.
    fn scrutinee_nullable_inner(&self, scrutinee: &juxc_ast::Expr) -> Option<juxc_tycheck::Ty> {
        let ty = self.expr_types.get(&crate::exprs::expr_span_of(scrutinee)).cloned().or_else(|| match scrutinee {
            juxc_ast::Expr::Path(qn) if qn.segments.len() == 1 => {
                self.local_types.iter().rev().find_map(|s| s.get(&qn.segments[0].text)).cloned()
            }
            _ => None,
        })?;
        match ty {
            juxc_tycheck::Ty::Nullable(inner) => Some(*inner),
            _ => None,
        }
    }

    /// A top-level case pattern against a nullable scrutinee: a non-null
    /// literal or range sits inside `Some(...)`; `null`, `_` and bindings keep
    /// their own shape.
    fn emit_nullable_scrutinee_pattern(&mut self, pattern: &juxc_ast::Pattern) {
        match pattern {
            juxc_ast::Pattern::Literal(Literal::Null, _) => self.emit_pattern(pattern),
            juxc_ast::Pattern::Literal(..) | juxc_ast::Pattern::Range { .. } => {
                self.w.push_str("Some(");
                self.emit_pattern(pattern);
                self.w.push(')');
            }
            juxc_ast::Pattern::Or(alts, _) => {
                for (i, alt) in alts.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(" | ");
                    }
                    self.emit_nullable_scrutinee_pattern(alt);
                }
            }
            _ => self.emit_pattern(pattern),
        }
    }

    fn scrutinee_is_string(&self, scrutinee: &juxc_ast::Expr) -> bool {
        // String literal scrutinee (`switch ("x")`): `Span::DUMMY`, so it never
        // appears in `expr_types` — recognize it directly.
        if let juxc_ast::Expr::Literal(juxc_ast::Literal::String(_)) = scrutinee {
            return true;
        }
        if let Some(t) = self.expr_types.get(&crate::exprs::expr_span_of(scrutinee)) {
            if matches!(t, juxc_tycheck::Ty::String) {
                return true;
            }
        }
        if let juxc_ast::Expr::Path(qn) = scrutinee {
            if qn.segments.len() == 1 {
                if let Some(t) = self
                    .local_types
                    .iter()
                    .rev()
                    .find_map(|s| s.get(&qn.segments[0].text))
                {
                    return matches!(t, juxc_tycheck::Ty::String);
                }
            }
        }
        false
    }

    /// True when `name` is a variant of the enum currently being switched on.
    fn is_current_switch_variant(&self, name: &str) -> bool {
        let Some(enum_bare) = &self.current_switch_enum else {
            return false;
        };
        // Find the enum's signature (by FQN or bare) and check its variants.
        self.symbols
            .enums
            .iter()
            .find(|(k, _)| {
                k.as_str() == enum_bare || k.rsplit('.').next() == Some(enum_bare.as_str())
            })
            .is_some_and(|(_, sig)| sig.variants.contains_key(name))
    }

    /// For a match arm whose pattern destructures a recursive enum variant,
    /// return the binder names bound to **boxed** self-referential payload slots
    /// (`case Tree.Branch(var l, var r)` over `Branch(Box<Tree>, Box<Tree>)` →
    /// `["l", "r"]`). The enum decl boxes such slots (E0072 avoidance), so the
    /// binder's Rust type is `Box<Enum>`; the arm body must unbox it once via a
    /// leading `let l = *l;` so every use sees a plain `Enum` value. Only `var`
    /// binders (`Pattern::Bind`) at boxed positions are returned — a `_` wildcard
    /// binds nothing, and a nested destructure isn't a plain binder.
    pub(crate) fn boxed_recursive_binders(&self, pattern: &juxc_ast::Pattern) -> Vec<String> {
        let juxc_ast::Pattern::EnumVariant { path, args, .. } = pattern else {
            return Vec::new();
        };
        // Variant name is the last path segment; the enum is the segment before
        // it, or the switch's enum for a bare `case Variant(...)`.
        let variant_name = path.segments.last().map(|s| s.text.as_str());
        let Some(variant_name) = variant_name else {
            return Vec::new();
        };
        let enum_bare: Option<String> = if path.segments.len() >= 2 {
            Some(path.segments[path.segments.len() - 2].text.clone())
        } else {
            self.current_switch_enum.clone()
        };
        let Some(enum_bare) = enum_bare else {
            return Vec::new();
        };
        // Resolve the enum signature (by FQN or bare last-segment).
        let Some((fqn, sig)) = self.symbols.enums.iter().find(|(k, _)| {
            k.as_str() == enum_bare.as_str()
                || k.rsplit('.').next() == Some(enum_bare.as_str())
        }) else {
            return Vec::new();
        };
        let Some(variant) = sig.variants.get(variant_name) else {
            return Vec::new();
        };
        let enum_name = fqn.rsplit('.').next().unwrap_or(fqn.as_str());
        let mut out = Vec::new();
        for (i, slot) in variant.payload.iter().enumerate() {
            if !crate::decls::enums::is_recursive_enum_slot(slot, enum_name) {
                continue;
            }
            // Only a plain `var x` binder needs unboxing on use.
            if let Some(juxc_ast::Pattern::Bind(name)) = args.get(i) {
                out.push(name.text.clone());
            }
        }
        out
    }

    /// Emit a single pattern in Rust source. Recursive for variant
    /// patterns with nested sub-patterns. The `Color.Variant` form
    /// rewrites through the `::`-path syntax used by Rust enums; for
    /// dotted paths with non-enum first-segment we fall back on raw
    /// `.`-joining, but that path doesn't arise from real Jux source
    /// today.
    pub(crate) fn emit_pattern(&mut self, pattern: &juxc_ast::Pattern) {
        match pattern {
            juxc_ast::Pattern::Wildcard(_) => self.w.push('_'),
            // Or-pattern `A | B | C` → Rust's identical `|` syntax.
            juxc_ast::Pattern::Or(alts, _) => {
                for (i, alt) in alts.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(" | ");
                    }
                    self.emit_pattern(alt);
                }
            }
            juxc_ast::Pattern::Literal(lit, _) => {
                // Pattern context: Rust match patterns require bare
                // literals (not `String` values), so we suppress the
                // `.to_string()` wrap that `emit_literal` applies for
                // value-position uses. Pure `&str` literal goes into
                // the pattern slot; the scrutinee side is unaffected.
                if let Literal::String(s) = lit {
                    if self.pattern_depth > 0 {
                        // On a `String` part: bind it, compare in the guard.
                        let binder = format!("__jux_str{}", self.pattern_string_guards.len());
                        self.w.push_str(&binder);
                        self.pattern_string_guards.push((binder, s.clone()));
                    } else {
                        self.emit_rust_string_literal(s);
                    }
                } else {
                    self.emit_literal(lit);
                }
            }
            juxc_ast::Pattern::Bind(name) => {
                // A bare `case Variant ->` (Java-style unqualified enum label)
                // parses as a Bind. When the switch scrutinee is an enum and
                // this name is one of its variants, emit the qualified
                // `Enum::Variant` pattern; otherwise it's a genuine binding.
                if self.is_current_switch_variant(&name.text) {
                    if let Some(enum_bare) = self.current_switch_enum.clone() {
                        self.w.push_str(&enum_bare);
                        self.w.push_str("::");
                    }
                }
                self.w.push_str(&to_rust_ident(&name.text));
            }
            juxc_ast::Pattern::Range { start, end, inclusive, .. } => {
                // Rust has every form the Jux pattern does: `0..10`, `0..=9`,
                // `100..`, `..0`, `..=0`. The endpoints go through the literal
                // path, so the Rust reads exactly as the Jux was written.
                if let Some(start) = start {
                    self.emit_literal(start);
                }
                self.w.push_str(if *inclusive { "..=" } else { ".." });
                if let Some(end) = end {
                    self.emit_literal(end);
                }
            }
            juxc_ast::Pattern::TypeBind { type_name, binder, .. } => {
                // `case Type ident ->` — shorthand for
                // `case Type(var ident) ->`. We rewrite into the
                // same shape the sealed-subclass EnumVariant path
                // handles below, but bind the WHOLE variant value
                // rather than destructuring fields. Output shape:
                // `Sealed::Type(ident)` so the user can call
                // `ident.method(...)` directly on the subclass.
                let parent_fqn = self
                    .lookup_class_by_bare_or_fqn(&type_name.text)
                    .and_then(|sub| sub.extends_fqn.clone())
                    .unwrap_or_default();
                let parent_bare = parent_fqn
                    .rsplit('.')
                    .next()
                    .unwrap_or(parent_fqn.as_str())
                    .to_string();
                if parent_bare.is_empty() {
                    // No sealed parent — emit as a bare bind.
                    // (Rust will fail to type-check; the diagnostic
                    // points at the source.)
                    self.w.push_str(&to_rust_ident(&binder.text));
                } else {
                    self.w.push_str(&parent_bare);
                    self.w.push_str("::");
                    self.w.push_str(&to_rust_ident(&type_name.text));
                    self.w.push('(');
                    self.w.push_str(&to_rust_ident(&binder.text));
                    self.w.push(')');
                }
            }
            // `(p, q)` is Rust's own tuple pattern.
            juxc_ast::Pattern::Tuple(elements, _) => {
                self.pattern_depth += 1;
                self.w.push('(');
                for (i, element) in elements.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_pattern(element);
                }
                self.w.push(')');
                self.pattern_depth -= 1;
            }
            // A record pattern (the checker resolved `Name` to a record):
            // `Point(var x, 0)` is the struct pattern `Point { x, y: 0 }`.
            juxc_ast::Pattern::EnumVariant { args, span, .. }
                if self.symbols.record_patterns.contains_key(span) =>
            {
                let fqn = self.symbols.record_patterns[span].clone();
                let components: Vec<String> = self
                    .symbols
                    .records
                    .get(&fqn)
                    .map(|r| r.components.iter().map(|c| c.name.clone()).collect())
                    .unwrap_or_default();
                let path = self.rust_path_for_type_fqn(&fqn);
                self.w.push_str(&path);
                self.pattern_depth += 1;
                // A `_` part is left out for `..`, which reads the way the
                // user meant it: that component does not matter.
                let written: Vec<(&String, &juxc_ast::Pattern)> = components
                    .iter()
                    .zip(args.iter())
                    .filter(|(_, p)| !matches!(p, juxc_ast::Pattern::Wildcard(_)))
                    .collect();
                if written.is_empty() {
                    self.w.push_str(" { .. }");
                } else {
                    self.w.push_str(" { ");
                    for (i, (component, sub)) in written.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        let field = to_rust_ident(component);
                        match sub {
                            juxc_ast::Pattern::Bind(name) if name.text == **component => {
                                self.w.push_str(&field);
                            }
                            _ => {
                                self.w.push_str(&field);
                                self.w.push_str(": ");
                                self.emit_pattern(sub);
                            }
                        }
                    }
                    if written.len() < components.len() {
                        self.w.push_str(", ..");
                    }
                    self.w.push_str(" }");
                }
                self.pattern_depth -= 1;
            }
            juxc_ast::Pattern::EnumVariant { path, args, .. } => {
                // Three shapes to handle:
                //
                // 1. **Enum variant** (`Color.Red`, `Token.Number(_)`)
                //    — rewrite `.` to `::`. The match scrutinee's
                //    type is an enum.
                //
                // 2. **Sealed-class subclass pattern** (`Red(var s)`
                //    inside a `switch (light) { … }` where `Light`
                //    is `sealed permits Red, Yellow, Green`). The
                //    pattern path is the bare subclass name; the
                //    lowered match is on a Rust enum
                //    `Light::Red(Red { seconds: s, .. })`. We need
                //    to (a) prepend the sealed parent's name, and
                //    (b) translate positional pattern args to the
                //    subclass struct's named-field pattern.
                //
                // 3. **Single bare name** (`Red` without parens,
                //    inside an enum-variant context) — kept as-is;
                //    falls into shape 1.
                //
                // Detection: single-segment path AND the bare name
                // resolves to a class whose parent is sealed.
                let is_single_subclass = path.segments.len() == 1
                    && self
                        .lookup_class_by_bare_or_fqn(&path.segments[0].text)
                        .and_then(|sub| sub.extends_fqn.clone())
                        .and_then(|fqn| {
                            // Look up the parent class; check sealed.
                            // Use the last FQN segment as the bare
                            // name for our lookup helper.
                            let bare = fqn
                                .rsplit('.')
                                .next()
                                .unwrap_or(&fqn)
                                .to_string();
                            self.lookup_class_by_bare_or_fqn(&bare)
                                .map(|p| p.is_sealed)
                        })
                        .unwrap_or(false);
                if is_single_subclass {
                    // Sealed-subclass shape — emit
                    // `Sealed::Sub(Sub { f0: arg0, f1: arg1, .. })`.
                    let sub_name = &path.segments[0].text;
                    let sub_class = self
                        .lookup_class_by_bare_or_fqn(sub_name)
                        .cloned();
                    let parent_fqn = sub_class
                        .as_ref()
                        .and_then(|c| c.extends_fqn.clone())
                        .unwrap_or_default();
                    let parent_bare = parent_fqn
                        .rsplit('.')
                        .next()
                        .unwrap_or(parent_fqn.as_str())
                        .to_string();
                    // Field-name lookup: positional pattern arg i
                    // maps to subclass field i in declaration order
                    // (static fields filtered out — they aren't
                    // instance state, can't appear in a struct
                    // pattern).
                    let field_names: Vec<String> = self
                        .class_asts
                        .get(sub_name.as_str())
                        .map(|ast| {
                            ast.fields
                                .iter()
                                .filter(|f| !f.is_static)
                                .map(|f| f.name.text.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    self.w.push_str(&parent_bare);
                    self.w.push_str("::");
                    self.w.push_str(sub_name);
                    self.w.push('(');
                    if args.is_empty() {
                        // Unit-style subclass pattern — empty
                        // struct. Use `..` to match any contents
                        // even if Rust later requires it; for an
                        // empty struct this just lowers to `Sub`.
                        self.w.push_str(sub_name);
                        self.w.push_str(" { .. }");
                    } else {
                        self.w.push_str(sub_name);
                        self.w.push_str(" { ");
                        for (i, sub) in args.iter().enumerate() {
                            if i > 0 {
                                self.w.push_str(", ");
                            }
                            // Field-name : pattern. We need the
                            // sub-pattern's text rendered — recurse
                            // through `emit_pattern`. The field name
                            // comes from the i-th non-static field;
                            // out-of-range positions get a synthetic
                            // `__pos{i}` so the error message points
                            // at the right shape.
                            let fname = field_names
                                .get(i)
                                .cloned()
                                .unwrap_or_else(|| format!("__pos{i}"));
                            self.w.push_str(&fname);
                            self.w.push_str(": ");
                            self.pattern_depth += 1;
                            self.emit_pattern(sub);
                            self.pattern_depth -= 1;
                        }
                        // `..` rest pattern in case the subclass
                        // has more fields than the pattern listed
                        // (Java would let the user destructure
                        // only the prefix they care about; Rust's
                        // struct patterns require exhaustiveness
                        // unless `..` is present).
                        self.w.push_str(", .. }");
                    }
                    self.w.push(')');
                    return;
                }
                // Shape 1 / 3: rewrite `.` to `::` verbatim. A bare
                // single-segment variant (Java-style `case Pending ->`, no
                // `Enum.` prefix) is qualified with the switch's enum so Rust
                // matches the variant instead of treating it as a catch-all
                // binding (E0170).
                if path.segments.len() == 1
                    && self.is_current_switch_variant(&path.segments[0].text)
                {
                    if let Some(enum_bare) = self.current_switch_enum.clone() {
                        self.w.push_str(&enum_bare);
                        self.w.push_str("::");
                    }
                }
                let segs: Vec<&str> =
                    path.segments.iter().map(|s| s.text.as_str()).collect();
                // `case Order.Status.Pending`: a nested enum named through its
                // owner is the lifted type the declaration lowered to (M.9),
                // spelled from the current package.
                let lifted = (segs.len() >= 3)
                    .then(|| self.lifted_nested_type_fqn(&segs[..segs.len() - 1]))
                    .flatten();
                // `case Level.Junior` inside the owner of `Level` (or a
                // subclass of it) names the lifted `Employee__Level`.
                let nested_simple = (segs.len() == 2 && lifted.is_none())
                    .then(|| self.enclosing_nested_type(segs[0]))
                    .flatten();
                if let Some(enum_name) = nested_simple {
                    self.w.push_str(&juxc_lex::to_rust_ident(&enum_name));
                    self.w.push_str("::");
                    self.w.push_str(&juxc_lex::to_rust_ident(segs[1]));
                    if !args.is_empty() || pattern_has_parens(pattern) {
                        self.w.push('(');
                        self.pattern_depth += 1;
                        for (i, sub) in args.iter().enumerate() {
                            if i > 0 {
                                self.w.push_str(", ");
                            }
                            self.emit_pattern(sub);
                        }
                        self.pattern_depth -= 1;
                        self.w.push(')');
                    }
                    return;
                }
                match lifted {
                    Some(fqn) => {
                        let path = self.rust_path_for_type_fqn(&fqn);
                        self.w.push_str(&path);
                        self.w.push_str("::");
                        self.w.push_str(&juxc_lex::to_rust_ident(segs[segs.len() - 1]));
                    }
                    None => self.w.push_str(&juxc_lex::join_rust_path(&segs)),
                }
                if !args.is_empty() || pattern_has_parens(pattern) {
                    self.w.push('(');
                    self.pattern_depth += 1;
                    for (i, sub) in args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.emit_pattern(sub);
                    }
                    self.pattern_depth -= 1;
                    self.w.push(')');
                }
            }
        }
    }
}

/// One alternative of an or-pattern over a trait object (`case Circle(var r)
/// | Ring(var r, _)`), as a runtime type test.
struct RuntimeTypeAlt {
    /// The `__jux_as_<T>` hook call: `Some` when the value is a `T`.
    getter: String,
    /// The pattern the hook's `Some` takes apart: a record's struct pattern,
    /// or the binder of a type pattern.
    destructure: String,
    /// The comparisons for string literals nested in this alternative, `&&`
    /// joined; empty when there are none.
    compares: String,
    /// Whether the destructure can fail on its own, a literal or range inside
    /// it (`Circle(0.0)`); a plain binder or `_` cannot.
    refutable: bool,
}

/// Whether a sub-pattern can fail to match a value of its own type: a literal
/// or a range can, and so can any shape holding one.
fn pattern_can_fail(p: &juxc_ast::Pattern) -> bool {
    match p {
        juxc_ast::Pattern::Literal(..) | juxc_ast::Pattern::Range { .. } => true,
        juxc_ast::Pattern::EnumVariant { args: parts, .. }
        | juxc_ast::Pattern::Tuple(parts, _)
        | juxc_ast::Pattern::Or(parts, _) => parts.iter().any(pattern_can_fail),
        juxc_ast::Pattern::Wildcard(_) | juxc_ast::Pattern::Bind(_) | juxc_ast::Pattern::TypeBind { .. } => false,
    }
}

/// The `let` that binds an or-pattern arm's names in its body over a trait
/// object, taken from the first alternative that matches (the guard already
/// proved one does):
///
/// ```text
/// let r = match __jux_subject.__jux_as_Circle() { Some(Circle { r }) => r, _ => match … };
/// ```
///
/// `None` when the arm binds nothing.
fn or_type_test_let(tests: &[RuntimeTypeAlt], binders: &[(String, juxc_tycheck::Ty)]) -> Option<String> {
    if tests.is_empty() || binders.is_empty() {
        return None;
    }
    let names: Vec<String> = binders.iter().map(|(n, _)| to_rust_ident(n)).collect();
    let value = if names.len() == 1 { names[0].clone() } else { format!("({})", names.join(", ")) };
    let mut text = format!("let {value} = ");
    for t in tests {
        text.push_str(&format!("match {} {{ Some({})", t.getter, t.destructure));
        if !t.compares.is_empty() {
            text.push_str(&format!(" if {}", t.compares));
        }
        text.push_str(&format!(" => {value}, _ => "));
    }
    text.push_str("unreachable!()");
    for _ in tests {
        text.push_str(" }");
    }
    text.push(';');
    Some(text)
}
