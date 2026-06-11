//! Tiny FQN helpers usable from the backend. Mirrors the helpers in
//! `juxc_tycheck::symbol_table` but kept here so the backend doesn't
//! need to import internal tycheck modules.

/// Strip the trailing identifier off an FQN. `"a.lib.Foo"` → `"Foo"`,
/// `"Foo"` → `"Foo"`.
pub(crate) fn fqn_bare(fqn: &str) -> &str {
    match fqn.rsplit_once('.') {
        Some((_, bare)) => bare,
        None => fqn,
    }
}

/// Return the package prefix of an FQN, or `None` for bare
/// (no-package) names. `"a.lib.Foo"` → `Some("a.lib")`,
/// `"Foo"` → `None`.
pub(crate) fn fqn_package(fqn: &str) -> Option<&str> {
    fqn.rsplit_once('.').map(|(pkg, _)| pkg)
}

/// Case-insensitive built-in annotation lookup. Mirrors tycheck's
/// `has_annotation` helper — annotations in Jux are case-insensitive
/// per spec, so we compare against a canonical-lowercase name.
#[allow(dead_code)]
pub(crate) fn has_annotation(
    annotations: &[juxc_ast::Annotation],
    canonical_lower: &str,
) -> bool {
    annotations.iter().any(|a| {
        a.name
            .segments
            .last()
            .map(|s| s.text.eq_ignore_ascii_case(canonical_lower))
            .unwrap_or(false)
    })
}

/// Reserved words that we would emit verbatim from user source but
/// which the Rust parser rejects without the `r#` raw-identifier
/// prefix. Used by [`to_rust_ident`] when lowering record fields,
/// enum payload field names, and similar binding sites that come
/// straight from a Jux `Ident` rather than from a synthesized name.
///
/// Two narrow exceptions: `self` and `Self` cannot become raw
/// identifiers in Rust at all, so the helper drops them through
/// unchanged — letting rustc surface its native error if they ever
/// slip into emitter output (the resolver should already have
/// caught the user-source case).
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const",
    "continue", "crate", "do", "dyn", "else", "enum", "extern", "false",
    "final", "fn", "for", "if", "impl", "in", "let", "loop", "macro",
    "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "union", "unsafe", "unsized", "use", "virtual", "where",
    "while", "yield",
];

/// Wrap a Jux identifier in Rust's `r#` raw-identifier syntax if it
/// would otherwise collide with a Rust reserved word. `self` and
/// `Self` pass through unchanged because Rust forbids them as raw
/// identifiers — those cases ought to be caught upstream.
pub(crate) fn to_rust_ident(name: &str) -> String {
    if name == "self" || name == "Self" {
        return name.to_string();
    }
    if RUST_KEYWORDS.contains(&name) {
        let mut out = String::with_capacity(name.len() + 2);
        out.push_str("r#");
        out.push_str(name);
        return out;
    }
    name.to_string()
}

impl crate::RustEmitter {
    /// Emit Rust `#[…]` attributes for the built-in Jux
    /// annotations on `annotations`. Per spec the lookup is
    /// case-insensitive (`@Deprecated` ≡ `@deprecated`).
    ///
    /// Recognized today:
    /// - `@Deprecated` → `#[deprecated]`. An optional `message =
    ///   "…"` named arg passes through as
    ///   `#[deprecated(note = "…")]`.
    /// - `@Cfg(name)` / `@Cfg(target = "linux")` →
    ///   `#[cfg(name)]` / `#[cfg(target = "linux")]`. Phase 1
    ///   only passes through identifiers and string literals;
    ///   compound predicates need the proper cfg-expr lowering.
    ///
    /// `@Override` is a tycheck-only marker — no Rust attribute.
    /// Unknown user-defined annotations are silently dropped
    /// (they parse but currently have no semantic effect).
    pub(crate) fn emit_annotation_attrs(
        &mut self,
        annotations: &[juxc_ast::Annotation],
    ) {
        for ann in annotations {
            let Some(seg) = ann.name.segments.last() else { continue };
            let name = seg.text.to_ascii_lowercase();
            match name.as_str() {
                "deprecated" => {
                    self.w.emit_indent();
                    if ann.args.is_empty() {
                        self.w.push_str("#[deprecated]\n");
                    } else {
                        // Try to pull a `message = "…"` (or
                        // `note = "…"`) named arg out for the Rust
                        // `note` attribute. Anything else is
                        // dropped silently in Phase 1.
                        let note = ann.args.iter().find_map(|a| match a {
                            juxc_ast::AnnotationArg::Named { name, value } => {
                                let key = name.text.to_ascii_lowercase();
                                if key == "message" || key == "note" {
                                    string_literal_text(value)
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        });
                        if let Some(n) = note {
                            self.w.push_str("#[deprecated(note = \"");
                            self.w.push_str(&n.replace('"', "\\\""));
                            self.w.push_str("\")]\n");
                        } else {
                            self.w.push_str("#[deprecated]\n");
                        }
                    }
                }
                "cfg" => {
                    // `@Cfg(linux)` → `#[cfg(linux)]`. `@Cfg(target = "linux")`
                    // → `#[cfg(target = "linux")]`. Multi-arg /
                    // nested predicates are deferred.
                    self.w.emit_indent();
                    self.w.push_str("#[cfg(");
                    for (i, arg) in ann.args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        match arg {
                            juxc_ast::AnnotationArg::Positional(expr) => {
                                if let Some(text) = path_text(expr) {
                                    self.w.push_str(&text);
                                } else if let Some(text) = string_literal_text(expr) {
                                    self.w.push('"');
                                    self.w.push_str(&text.replace('"', "\\\""));
                                    self.w.push('"');
                                }
                            }
                            juxc_ast::AnnotationArg::Named { name, value } => {
                                self.w.push_str(&name.text);
                                self.w.push_str(" = ");
                                if let Some(text) = string_literal_text(value) {
                                    self.w.push('"');
                                    self.w.push_str(&text.replace('"', "\\\""));
                                    self.w.push('"');
                                } else if let Some(text) = path_text(value) {
                                    self.w.push_str(&text);
                                }
                            }
                        }
                    }
                    self.w.push_str(")]\n");
                }
                "override" => {
                    // Compile-time-only — tycheck verifies the
                    // override relationship. No Rust attribute.
                }
                _ => {
                    // Unknown annotation — drop. User-defined
                    // annotations are still Phase-2 work.
                }
            }
        }
    }
}

/// Extract the raw text of a string-literal expression, or `None`
/// for anything else. Used by the annotation lowering to pull
/// `message = "…"` and similar string-arg shapes.
fn string_literal_text(expr: &juxc_ast::Expr) -> Option<String> {
    if let juxc_ast::Expr::Literal(lit) = expr {
        if let juxc_ast::Literal::String(s) = lit {
            return Some(s.clone());
        }
    }
    None
}

/// Extract a dotted-path expression's text (`linux`, `target_os`,
/// `unix`), or `None` for anything more elaborate. Used by `@Cfg`
/// to forward bare-identifier predicates into the Rust `#[cfg]`.
fn path_text(expr: &juxc_ast::Expr) -> Option<String> {
    if let juxc_ast::Expr::Path(qn) = expr {
        if qn.segments.is_empty() {
            return None;
        }
        return Some(
            qn.segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join("::"),
        );
    }
    None
}

impl crate::RustEmitter {
    /// Returns `Some(fqn)` when `qn` names a known class in the
    /// workspace symbol table — the backend's lightweight version
    /// of tycheck's `path_resolves_to_class`. Used by the static-
    /// member emission paths to detect `ClassName.X` /
    /// `ClassName.method()` shapes.
    pub(crate) fn path_resolves_to_class_in_emit(
        &self,
        qn: &juxc_ast::QualifiedName,
    ) -> Option<String> {
        if qn.segments.is_empty() {
            return None;
        }
        let joined: String = qn
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".");
        if self.symbols.classes.contains_key(&joined) {
            return Some(joined);
        }
        if qn.segments.len() == 1 {
            let bare = &qn.segments[0].text;
            // 1. Import-alias-aware lookup: the current unit's
            //    `unqualified` map carries every name that's
            //    visible bare in this unit, including aliased
            //    grouped imports (`{ X as Y }` registers `Y → FQN`).
            //    A hit here gets us the FQN even when the bare
            //    name doesn't match any FQN's last segment (which
            //    is the bog-standard "import alias renames the
            //    target" case).
            if let Some(idx) = self.current_unit_idx {
                if let Some(ctx) = self.symbols.units.get(idx) {
                    if let Some(fqn) = ctx.unqualified.get(bare.as_str()) {
                        if self.symbols.classes.contains_key(fqn) {
                            return Some(fqn.clone());
                        }
                    }
                }
            }
            // 2. Fallback suffix scan — works for same-package
            //    siblings and any class whose bare name matches
            //    the source token (the common single-file or
            //    no-alias case). Pick the lexicographically smallest
            //    match: `classes` is a `HashMap`, so returning "the
            //    first match" would be non-deterministic across runs
            //    on a bare-name collision (flaky codegen).
            if let Some(fqn) = self
                .symbols
                .classes
                .keys()
                .filter(|fqn| fqn_bare(fqn) == bare.as_str())
                .min()
            {
                return Some(fqn.clone());
            }
        }
        None
    }

    /// Resolve a bare or FQN class name to the class signature in
    /// the symbol table. Direct hit by key first; otherwise scans
    /// FQNs for a matching last segment. Used by emission helpers
    /// that hold `self.enclosing_class` (bare in the source) but
    /// need to access the FQN-keyed `symbols.classes` map.
    ///
    /// **Bare-name disambiguation.** A bare name can match several FQNs — most
    /// commonly a user class whose name collides with an auto-loaded `rust.std`
    /// stub (e.g. user `Child` vs `std::process::Child` → `rust.std.Child`). A
    /// user declaration must SHADOW the foreign stub, so a non-`external` match
    /// is preferred. Ties are then broken by FQN order so the result is
    /// **deterministic** (a `HashMap` scan is otherwise iteration-order
    /// dependent, which would make emission non-reproducible).
    pub(crate) fn lookup_class_by_bare_or_fqn(
        &self,
        name: &str,
    ) -> Option<&juxc_tycheck::symbol_table::ClassSig> {
        if let Some(c) = self.symbols.classes.get(name) {
            return Some(c);
        }
        self.symbols
            .classes
            .iter()
            .filter(|(k, _)| k.rsplit('.').next().unwrap_or(k.as_str()) == name)
            // Prefer a user (non-external) class over a stub; then lowest FQN.
            .min_by(|a, b| {
                a.1.is_external
                    .cmp(&b.1.is_external)
                    .then_with(|| a.0.cmp(b.0))
            })
            .map(|(_, c)| c)
    }

    /// Bare-or-FQN sibling of [`Self::lookup_class_by_bare_or_fqn`]
    /// for interfaces. Direct hit by key first; otherwise scans
    /// interface FQNs for a matching last segment.
    pub(crate) fn lookup_interface_by_bare_or_fqn(
        &self,
        name: &str,
    ) -> Option<(&str, &juxc_tycheck::symbol_table::InterfaceSig)> {
        if let Some((k, i)) = self.symbols.interfaces.get_key_value(name) {
            return Some((k.as_str(), i));
        }
        self.symbols
            .interfaces
            .iter()
            .find(|(k, _)| k.rsplit('.').next().unwrap_or(k.as_str()) == name)
            .map(|(k, i)| (k.as_str(), i))
    }

    /// Sibling of [`path_resolves_to_class_in_emit`] that maps a
    /// path to an interface FQN. Used by the static-member
    /// emission paths to recognize `IfaceName.FIELD` or
    /// `IfaceName.method()` shapes whose lowering goes through
    /// the `Iface_FIELD` / `Iface_method` free-fn naming.
    pub(crate) fn path_resolves_to_interface_in_emit(
        &self,
        qn: &juxc_ast::QualifiedName,
    ) -> Option<String> {
        if qn.segments.is_empty() {
            return None;
        }
        let joined: String = qn
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".");
        if self.symbols.interfaces.contains_key(&joined) {
            return Some(joined);
        }
        if qn.segments.len() == 1 {
            let bare = &qn.segments[0].text;
            // Import-alias-aware lookup — same shape as the class
            // path's unit-context consultation above.
            if let Some(idx) = self.current_unit_idx {
                if let Some(ctx) = self.symbols.units.get(idx) {
                    if let Some(fqn) = ctx.unqualified.get(bare.as_str()) {
                        if self.symbols.interfaces.contains_key(fqn) {
                            return Some(fqn.clone());
                        }
                    }
                }
            }
            for fqn in self.symbols.interfaces.keys() {
                if fqn_bare(fqn) == bare.as_str() {
                    return Some(fqn.clone());
                }
            }
        }
        None
    }

    /// Emit an FQN as a Rust path. Any FQN with a package portion
    /// gets a `crate::` root so it resolves correctly regardless
    /// of how deep the surrounding `pub mod` nest is.
    /// No-package (bare-name) FQNs come through as-is.
    ///
    /// **The `force_root` parameter is retained for the rare
    /// "always crate-rooted" case but is now redundant for multi-
    /// segment FQNs** — they always crate-root. We kept the
    /// signature so existing call sites still compile; new code
    /// can pass `false` and get the right behavior.
    pub(crate) fn emit_fqn_path_in_rust(&mut self, fqn: &str, _force_root: bool) {
        if let Some(pkg) = fqn_package(fqn) {
            self.w.push_str("crate::");
            for seg in pkg.split('.') {
                self.w.push_str(seg);
                self.w.push_str("::");
            }
        }
        self.w.push_str(fqn_bare(fqn));
    }
}
