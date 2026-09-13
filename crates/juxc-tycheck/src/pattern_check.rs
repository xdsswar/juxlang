//! Tuple and record patterns in `switch` (JUX-GRAMMAR-ADDENDUM §A.3,
//! JUX-TYPE-SYSTEM-ADDENDUM §T.5.7).
//!
//! Three jobs, all driven by the scrutinee's type:
//!
//! 1. **Shape.** A tuple pattern needs one sub-pattern per element and a
//!    record pattern one per component, and a record pattern has to name the
//!    record the value is. Anything else can never match, and is `E0439`
//!    rather than a rustc error about struct patterns.
//! 2. **Bindings.** A `var x` inside a tuple or record pattern takes the type
//!    of the element or component it sits on, so the guard and the arm body
//!    are checked against real types.
//! 3. **Exhaustiveness.** A `switch` over a tuple or record value must cover
//!    every value (§T.5.2's product rule). The check is the usual
//!    pattern-matrix one: specialize the arms by each constructor of the first
//!    position's value set, recurse, and report the first value vector no arm
//!    matches as the `E0440` witness.
//!
//! Enum and sealed-class scrutinees keep their own check in `check.rs`; this
//! module only takes over when the value itself is a tuple or a record.

use juxc_ast::{Literal, Pattern, QualifiedName, SwitchExpr};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

use crate::check::Checker;
use crate::ty::{lower_member_type, Primitive, Ty};

/// How deep a value set is unfolded before a component counts as open. A
/// recursive enum (`Expr.Add(Expr, Expr)`) would otherwise unfold forever.
const MAX_DOMAIN_DEPTH: usize = 4;

/// The value set of one position being matched (§T.5.2).
#[derive(Debug, Clone)]
enum Domain {
    /// `true` and `false`.
    Bool,
    /// `null`, plus the values of the inner type.
    Nullable(Box<Domain>),
    /// The variants of an enum, each with its payload's value sets.
    Enum { name: String, variants: Vec<(String, Vec<Domain>)> },
    /// A tuple or a record: one constructor, a value set per part.
    Product { record: Option<String>, parts: Vec<Domain> },
    /// Too many values to enumerate: only a pattern matching all of them
    /// covers it.
    Open,
}

/// A constructor of a [`Domain`].
#[derive(Debug, Clone, PartialEq)]
enum Ctor {
    True,
    False,
    Null,
    Variant(String),
    Product,
    /// A literal, range or type pattern over an open set. Never part of a
    /// complete constructor list, so it never makes a position covered.
    Opaque,
}

/// A pattern reduced to what coverage needs.
#[derive(Debug, Clone)]
enum Cov {
    Any,
    Ctor(Ctor, Vec<Cov>),
}

impl Domain {
    /// Every constructor, when the set is finite.
    fn ctors(&self) -> Option<Vec<Ctor>> {
        match self {
            Domain::Bool => Some(vec![Ctor::True, Ctor::False]),
            Domain::Nullable(inner) => {
                let mut all = vec![Ctor::Null];
                all.extend(inner.ctors()?);
                Some(all)
            }
            Domain::Enum { variants, .. } => {
                Some(variants.iter().map(|(v, _)| Ctor::Variant(v.clone())).collect())
            }
            Domain::Product { .. } => Some(vec![Ctor::Product]),
            Domain::Open => None,
        }
    }

    /// The value sets of constructor `c`'s parts.
    fn parts_of(&self, c: &Ctor) -> Vec<Domain> {
        match (self, c) {
            (Domain::Nullable(inner), c) if *c != Ctor::Null => inner.parts_of(c),
            (Domain::Enum { variants, .. }, Ctor::Variant(v)) => variants
                .iter()
                .find(|(name, _)| name == v)
                .map(|(_, parts)| parts.clone())
                .unwrap_or_default(),
            (Domain::Product { parts, .. }, Ctor::Product) => parts.clone(),
            _ => Vec::new(),
        }
    }
}

/// The first value vector over `doms` that no row of `rows` matches, as
/// patterns with `_` for the parts that do not matter, or `None` when the rows
/// cover every value.
fn missing(rows: &[Vec<Cov>], doms: &[Domain]) -> Option<Vec<Cov>> {
    let Some((dom, rest)) = doms.split_first() else {
        return rows.is_empty().then(Vec::new);
    };
    let heads: Vec<&Ctor> = rows
        .iter()
        .filter_map(|r| match r.first() {
            Some(Cov::Ctor(c, _)) => Some(c),
            _ => None,
        })
        .collect();
    let all = dom.ctors();
    let complete = all
        .as_ref()
        .is_some_and(|all| !heads.is_empty() && all.iter().all(|c| heads.contains(&c)));
    if complete {
        // Every constructor appears: the rows cover the position exactly when
        // they cover what each constructor carries.
        for c in all.unwrap_or_default() {
            let parts = dom.parts_of(&c);
            let arity = parts.len();
            let specialized: Vec<Vec<Cov>> = rows
                .iter()
                .filter_map(|r| {
                    let (head, tail) = r.split_first()?;
                    let mut out = match head {
                        Cov::Ctor(hc, args) if *hc == c => args.clone(),
                        Cov::Ctor(..) => return None,
                        Cov::Any => vec![Cov::Any; arity],
                    };
                    // A malformed pattern (wrong part count, already E0439)
                    // is padded so the matrix stays rectangular.
                    out.resize(arity, Cov::Any);
                    out.extend(tail.iter().cloned());
                    Some(out)
                })
                .collect();
            let mut sub_doms = parts;
            sub_doms.extend(rest.iter().cloned());
            if let Some(mut witness) = missing(&specialized, &sub_doms) {
                let tail = witness.split_off(arity);
                let mut out = vec![Cov::Ctor(c, witness)];
                out.extend(tail);
                return Some(out);
            }
        }
        None
    } else {
        // Some constructor is absent, or the set is open: only the rows that
        // match anything here can cover the values that constructor stands
        // for.
        let defaults: Vec<Vec<Cov>> = rows
            .iter()
            .filter(|r| matches!(r.first(), Some(Cov::Any)))
            .map(|r| r[1..].to_vec())
            .collect();
        let tail = missing(&defaults, rest)?;
        let head = all
            .and_then(|all| all.into_iter().find(|c| !heads.contains(&c)))
            .map(|c| {
                let arity = dom.parts_of(&c).len();
                Cov::Ctor(c, vec![Cov::Any; arity])
            })
            .unwrap_or(Cov::Any);
        let mut out = vec![head];
        out.extend(tail);
        Some(out)
    }
}

/// Spell a witness the way the user would write the pattern.
fn render(w: &Cov, dom: &Domain) -> String {
    match (w, dom) {
        (Cov::Any, _) | (Cov::Ctor(Ctor::Opaque, _), _) => "_".to_string(),
        (Cov::Ctor(Ctor::True, _), _) => "true".to_string(),
        (Cov::Ctor(Ctor::False, _), _) => "false".to_string(),
        (Cov::Ctor(Ctor::Null, _), _) => "null".to_string(),
        (Cov::Ctor(_, _), Domain::Nullable(inner)) => render(w, inner),
        (Cov::Ctor(Ctor::Variant(v), args), Domain::Enum { name, .. }) => {
            let parts = dom.parts_of(&Ctor::Variant(v.clone()));
            let bare = name.rsplit('.').next().unwrap_or(name);
            if parts.is_empty() {
                format!("{bare}.{v}")
            } else {
                format!("{bare}.{v}({})", render_list(args, &parts))
            }
        }
        (Cov::Ctor(Ctor::Product, args), Domain::Product { record, parts }) => match record {
            Some(name) => {
                let bare = name.rsplit('.').next().unwrap_or(name);
                format!("{bare}({})", render_list(args, parts))
            }
            None => format!("({})", render_list(args, parts)),
        },
        _ => "_".to_string(),
    }
}

fn render_list(args: &[Cov], doms: &[Domain]) -> String {
    args.iter()
        .zip(doms)
        .map(|(a, d)| render(a, d))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Checker<'_> {
    /// The record a `Name(...)` pattern names, by FQN, or `None` when the path
    /// names no record (an enum variant or a sealed subclass instead).
    ///
    /// A bare name resolves in this unit's context first (imports and
    /// same-package types); a dotted one is a full name or a nested record
    /// written through its owner (`Shape.Point`, lifted to `Shape__Point`).
    pub(crate) fn pattern_record_fqn(&self, path: &QualifiedName) -> Option<String> {
        let segs: Vec<&str> = path.segments.iter().map(|s| s.text.as_str()).collect();
        self.record_fqn_for(&segs)
    }

    /// [`Self::pattern_record_fqn`] over the path's segments.
    fn record_fqn_for(&self, segs: &[&str]) -> Option<String> {
        let is_record = |fqn: &str| self.symbols.records.contains_key(fqn);
        match segs {
            [] => None,
            [bare] => {
                let pkg = self.env.current_package.join(".");
                self.env
                    .unqualified
                    .get(*bare)
                    .filter(|fqn| is_record(fqn))
                    .cloned()
                    .or_else(|| {
                        let here = if pkg.is_empty() { bare.to_string() } else { format!("{pkg}.{bare}") };
                        is_record(&here).then_some(here)
                    })
                    .or_else(|| is_record(bare).then(|| bare.to_string()))
                    .or_else(|| {
                        // The enclosing class's own nested record, written bare.
                        let owner = self.env.current_class.as_deref()?;
                        let lifted = format!("{owner}__{bare}");
                        is_record(&lifted).then_some(lifted)
                    })
                    .or_else(|| {
                        let suffix = format!(".{bare}");
                        let mut hits = self.symbols.records.keys().filter(|k| k.ends_with(&suffix));
                        match (hits.next(), hits.next()) {
                            (Some(k), None) => Some(k.clone()),
                            _ => None,
                        }
                    })
            }
            [first, rest @ ..] => {
                let dotted = segs.join(".");
                if is_record(&dotted) {
                    return Some(dotted);
                }
                let owner = crate::infer::owner_type_fqn(first, &self.env, self.symbols)?;
                let lifted = format!("{owner}__{}", rest.join("__"));
                is_record(&lifted).then_some(lifted)
            }
        }
    }

    /// The FQN of the record type `ty` is, if it is one.
    fn record_fqn_of_ty(&self, ty: &Ty) -> Option<String> {
        let Ty::User { name, .. } = ty else { return None };
        if self.symbols.records.contains_key(name) {
            return Some(name.clone());
        }
        let segs: Vec<&str> = name.split('.').collect();
        self.record_fqn_for(&segs)
    }

    /// The component types of the record `fqn`, as seen through the value
    /// type `ty` (whose generic arguments stand for the record's parameters).
    fn record_component_types(&self, fqn: &str, ty: &Ty) -> Vec<Ty> {
        let Some(record) = self.symbols.records.get(fqn) else { return Vec::new() };
        let args: &[Ty] = match ty {
            Ty::User { generic_args, .. } => generic_args,
            _ => &[],
        };
        record
            .components
            .iter()
            .map(|c| {
                let param = (c.ty.name.segments.len() == 1
                    && c.ty.generic_args.is_empty()
                    && c.ty.array_shape.is_none()
                    && c.ty.fn_shape.is_none())
                .then(|| {
                    record
                        .generic_params
                        .iter()
                        .position(|p| p.name.text == c.ty.name.segments[0].text)
                })
                .flatten();
                match param {
                    Some(i) => args.get(i).cloned().unwrap_or(Ty::Unknown),
                    None => lower_member_type(&c.ty, fqn, self.symbols),
                }
            })
            .collect()
    }

    /// The payload types of variant `variant` of the enum `ty` is, when `ty`
    /// is an enum and has that variant.
    fn variant_payload_types(&self, ty: &Ty, variant: &str) -> Option<Vec<Ty>> {
        let Ty::User { name, .. } = ty else { return None };
        let (fqn, sig) = self
            .symbols
            .lookup_enum_in(name, &self.env.current_package.join("."))?;
        let v = sig.variants.get(variant)?;
        Some(v.payload.iter().map(|t| lower_member_type(t, fqn, self.symbols)).collect())
    }

    /// Whether `ty` is a value tuple and record patterns take apart: a tuple
    /// or a record. The switches this module owns are over these.
    pub(crate) fn is_product_ty(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::User { name, .. } if name == juxc_ast::TUPLE_SENTINEL)
            || self.record_fqn_of_ty(ty).is_some()
    }

    /// Check `pattern` against a value of type `ty`, reporting a tuple or
    /// record pattern that cannot match it (E0439), and collect the bindings
    /// it introduces with their types. Records each record pattern's resolved
    /// FQN for the backend.
    ///
    /// Only the shapes this module owns are judged; an enum-variant or literal
    /// pattern is left to the checks that already exist, apart from typing
    /// the bindings inside it when it sits in a tuple or record.
    pub(crate) fn check_pattern_shape(
        &mut self,
        pattern: &Pattern,
        ty: &Ty,
        bindings: &mut Vec<(juxc_ast::Ident, Ty)>,
    ) {
        match pattern {
            Pattern::Bind(name) => bindings.push((name.clone(), ty.clone())),
            Pattern::Tuple(elements, span) => {
                let element_tys = match ty {
                    Ty::User { name, generic_args } if name == juxc_ast::TUPLE_SENTINEL => {
                        // Fewer than two elements is already a parse error.
                        if elements.len() >= 2 && generic_args.len() != elements.len() {
                            self.shape_mismatch(
                                *span,
                                format!(
                                    "this tuple pattern has {} elements, but the value is `{ty}`, \
                                     which has {}",
                                    elements.len(),
                                    generic_args.len()
                                ),
                            );
                        }
                        generic_args.clone()
                    }
                    Ty::Unknown => Vec::new(),
                    other => {
                        self.shape_mismatch(
                            *span,
                            format!("a tuple pattern cannot match a value of type `{other}`"),
                        );
                        Vec::new()
                    }
                };
                for (i, element) in elements.iter().enumerate() {
                    let element_ty = element_tys.get(i).cloned().unwrap_or(Ty::Unknown);
                    self.check_pattern_shape(element, &element_ty, bindings);
                }
            }
            Pattern::EnumVariant { path, args, has_parens, span } => {
                if let Some(record) = self.pattern_record_fqn(path) {
                    self.record_patterns.insert(*span, record.clone());
                    let bare = record.rsplit('.').next().unwrap_or(&record).to_string();
                    let value_record = self.record_fqn_of_ty(ty);
                    let through = match (&value_record, ty) {
                        (Some(v), _) if *v == record => ty.clone(),
                        (_, Ty::Unknown) => Ty::Unknown,
                        _ => {
                            self.shape_mismatch(
                                *span,
                                format!("a `{bare}` pattern cannot match a value of type `{ty}`"),
                            );
                            Ty::Unknown
                        }
                    };
                    let components = self.record_component_types(&record, &through);
                    if *has_parens && args.len() != components.len() {
                        self.shape_mismatch(
                            *span,
                            format!(
                                "`{bare}` has {} component{}, and this pattern gives {}",
                                components.len(),
                                if components.len() == 1 { "" } else { "s" },
                                args.len()
                            ),
                        );
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let component_ty = components.get(i).cloned().unwrap_or(Ty::Unknown);
                        self.check_pattern_shape(arg, &component_ty, bindings);
                    }
                    return;
                }
                // An enum variant: type the bindings in its payload.
                let variant = path.segments.last().map(|s| s.text.as_str()).unwrap_or("");
                let payload = self.variant_payload_types(ty, variant).unwrap_or_default();
                for (i, arg) in args.iter().enumerate() {
                    let payload_ty = payload.get(i).cloned().unwrap_or(Ty::Unknown);
                    self.check_pattern_shape(arg, &payload_ty, bindings);
                }
            }
            Pattern::Or(alternatives, _) => {
                // Bindings in alternatives are E0447 already; walk for shape.
                let mut ignored = Vec::new();
                for alt in alternatives {
                    self.check_pattern_shape(alt, ty, &mut ignored);
                }
            }
            Pattern::TypeBind { binder, .. } => bindings.push((binder.clone(), Ty::Unknown)),
            Pattern::Wildcard(_) | Pattern::Literal(..) | Pattern::Range { .. } => {}
        }
    }

    fn shape_mismatch(&mut self, span: Span, message: String) {
        self.diagnostics.push(
            Diagnostic::error(code::Code::E0439_PatternShapeMismatch, message).with_span(span),
        );
    }

    /// The value set of type `ty`, unfolded `depth` levels deep.
    fn domain_of(&self, ty: &Ty, depth: usize) -> Domain {
        if depth > MAX_DOMAIN_DEPTH {
            return Domain::Open;
        }
        match ty {
            Ty::Primitive(Primitive::Bool) => Domain::Bool,
            Ty::Nullable(inner) => Domain::Nullable(Box::new(self.domain_of(inner, depth + 1))),
            Ty::User { name, generic_args } if name == juxc_ast::TUPLE_SENTINEL => Domain::Product {
                record: None,
                parts: generic_args.iter().map(|t| self.domain_of(t, depth + 1)).collect(),
            },
            Ty::User { name, .. } => {
                if let Some(record) = self.record_fqn_of_ty(ty) {
                    let parts = self
                        .record_component_types(&record, ty)
                        .iter()
                        .map(|t| self.domain_of(t, depth + 1))
                        .collect();
                    return Domain::Product { record: Some(record), parts };
                }
                match self.symbols.lookup_enum_in(name, &self.env.current_package.join(".")) {
                    Some((fqn, sig)) if !sig.is_external => {
                        let mut variants: Vec<(&String, &crate::symbol_table::VariantSig)> =
                            sig.variants.iter().collect();
                        // Declaration order, so the witness is the first
                        // variant the user wrote that no arm covers.
                        variants.sort_by_key(|(_, v)| v.span.start);
                        Domain::Enum {
                            name: fqn.to_string(),
                            variants: variants
                                .into_iter()
                                .map(|(v, sig)| {
                                    let parts = sig
                                        .payload
                                        .iter()
                                        .map(|t| {
                                            self.domain_of(&lower_member_type(t, fqn, self.symbols), depth + 1)
                                        })
                                        .collect();
                                    (v.clone(), parts)
                                })
                                .collect(),
                        }
                    }
                    _ => Domain::Open,
                }
            }
            _ => Domain::Open,
        }
    }

    /// `pattern` as coverage over a position whose value set is `dom`; an
    /// or-pattern contributes one row per alternative, so it comes back as a
    /// list.
    fn coverage(&self, pattern: &Pattern, dom: &Domain) -> Vec<Cov> {
        match pattern {
            Pattern::Wildcard(_) | Pattern::Bind(_) => vec![Cov::Any],
            Pattern::Literal(Literal::Bool(true), _) => vec![Cov::Ctor(Ctor::True, Vec::new())],
            Pattern::Literal(Literal::Bool(false), _) => vec![Cov::Ctor(Ctor::False, Vec::new())],
            Pattern::Literal(Literal::Null, _) => vec![Cov::Ctor(Ctor::Null, Vec::new())],
            Pattern::Literal(..) | Pattern::Range { .. } | Pattern::TypeBind { .. } => {
                vec![Cov::Ctor(Ctor::Opaque, Vec::new())]
            }
            Pattern::Or(alternatives, _) => {
                alternatives.iter().flat_map(|alt| self.coverage(alt, dom)).collect()
            }
            Pattern::Tuple(elements, _) => {
                let parts = dom.parts_of(&Ctor::Product);
                self.product_coverage(Ctor::Product, elements, &parts)
            }
            Pattern::EnumVariant { path, args, has_parens, .. } => {
                if self.pattern_record_fqn(path).is_some() {
                    let parts = dom.parts_of(&Ctor::Product);
                    let args = if *has_parens { args.clone() } else { Vec::new() };
                    return self.product_coverage(Ctor::Product, &args, &parts);
                }
                let variant = path.segments.last().map(|s| s.text.clone()).unwrap_or_default();
                let is_variant = matches!(inner_domain(dom), Domain::Enum { variants, .. }
                    if variants.iter().any(|(v, _)| *v == variant));
                if !is_variant {
                    return vec![Cov::Ctor(Ctor::Opaque, Vec::new())];
                }
                let ctor = Ctor::Variant(variant);
                let parts = dom.parts_of(&ctor);
                self.product_coverage(ctor, args, &parts)
            }
        }
    }

    /// Coverage for a constructor with sub-patterns: every combination of the
    /// sub-patterns' own alternatives.
    fn product_coverage(&self, ctor: Ctor, subs: &[Pattern], parts: &[Domain]) -> Vec<Cov> {
        let mut combos: Vec<Vec<Cov>> = vec![Vec::new()];
        for (i, dom) in parts.iter().enumerate() {
            let options = match subs.get(i) {
                Some(p) => self.coverage(p, dom),
                None => vec![Cov::Any],
            };
            combos = combos
                .into_iter()
                .flat_map(|prefix| {
                    options.iter().map(move |o| {
                        let mut next = prefix.clone();
                        next.push(o.clone());
                        next
                    })
                })
                .collect();
        }
        combos.into_iter().map(|args| Cov::Ctor(ctor.clone(), args)).collect()
    }

    /// Exhaustiveness for a `switch` whose value is a tuple or record
    /// (§T.5.7). Guarded arms do not count (§T.5.6).
    pub(crate) fn check_product_switch_exhaustive(&mut self, s: &SwitchExpr, scrutinee: &Ty) {
        let dom = self.domain_of(scrutinee, 0);
        let rows: Vec<Vec<Cov>> = s
            .arms
            .iter()
            .filter(|arm| arm.guard.is_none())
            .flat_map(|arm| self.coverage(&arm.pattern, &dom))
            .map(|c| vec![c])
            .collect();
        let Some(witness) = missing(&rows, std::slice::from_ref(&dom)) else { return };
        let shown = witness.first().map(|w| render(w, &dom)).unwrap_or_else(|| "_".to_string());
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0440_NotExhaustive,
                format!(
                    "non-exhaustive `switch` on `{scrutinee}`: no arm matches `{shown}`; add a \
                     `case` for it, or a `default ->` arm (§T.5.7)"
                ),
            )
            .with_span(s.span),
        );
    }
}

/// `dom` without its nullable layer.
fn inner_domain(dom: &Domain) -> &Domain {
    match dom {
        Domain::Nullable(inner) => inner_domain(inner),
        other => other,
    }
}
