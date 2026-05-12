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
            // Bare name — could be a same-package sibling or an
            // import. Search the table for a matching bare suffix.
            let bare = &qn.segments[0].text;
            for fqn in self.symbols.classes.keys() {
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
