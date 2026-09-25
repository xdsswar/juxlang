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

/// Wrap a Jux identifier in Rust's `r#` raw-identifier syntax when it would
/// collide with a Rust reserved word. The keyword list and escape rule are the
/// single source of truth in [`juxc_lex::rust_keywords`] — shared with the
/// resolver's user-identifier check so the two can't drift. Used here when
/// lowering record fields, enum payload field names, foreign members, and
/// similar binding sites that come straight from a Jux `Ident`.
pub(crate) use juxc_lex::to_rust_ident;

impl crate::RustEmitter {
    /// Emit Rust `#[…]` attributes for the built-in Jux
    /// annotations on `annotations`. Per spec the lookup is
    /// case-insensitive (`@Deprecated` ≡ `@deprecated`).
    ///
    /// Recognized today:
    /// - `@Deprecated` → `#[deprecated]`. An optional `message =
    ///   "…"` named arg passes through as
    ///   `#[deprecated(note = "…")]`.
    /// - `@cfg(...)` emits nothing: the driver already removed what it
    ///   excludes.
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
                    // Decided before names were resolved (JUX-LANG-V1 11, the
                    // driver's cfg pass): a declaration that reaches the backend
                    // is part of this build, so there is nothing left to say.
                    // Handing the predicate to rustc as `#[cfg]` would be wrong
                    // anyway, since `os = "linux"` is not a Rust cfg key.
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
    if let juxc_ast::Expr::Literal(juxc_ast::Literal::String(s)) = expr {
        return Some(s.clone());
    }
    None
}

impl crate::RustEmitter {
    /// Returns `Some(fqn)` when `qn` names a known class in the
    /// workspace symbol table — the backend's lightweight version
    /// of tycheck's `path_resolves_to_class`. Used by the static-
    /// member emission paths to detect `ClassName.X` /
    /// `ClassName.method()` shapes.
    /// A FIELD CHAIN that spells a class's fully-qualified name, as the
    /// multi-segment path it really is. The rule lives in
    /// [`juxc_tycheck::infer::field_chain_class_path`] so the checker and the
    /// backend cannot disagree about it; this supplies the backend's view of
    /// which names are locals in scope.
    pub(crate) fn field_chain_class_path(
        &self,
        e: &juxc_ast::Expr,
    ) -> Option<juxc_ast::QualifiedName> {
        juxc_tycheck::infer::field_chain_class_path(
            e,
            &|n| self.local_types.iter().any(|scope| scope.contains_key(n)),
            &|n| self.resolve_bare_class_fqn(n),
            &self.symbols,
        )
    }

    /// The FQN of a nested type written through its owner -- `Order.Status`
    /// or `shop.orders.Order.Status` -- as the lifted `shop.orders.Order__Status`
    /// the declaration lowered to, or `None` when `segments` names no nested
    /// type.
    ///
    /// Every split is tried, longest owner first, exactly as the checker's
    /// `juxc_tycheck::infer::field_chain_class_path` does: an owner written in
    /// full, or a single bare name resolved in this unit's context.
    pub(crate) fn lifted_nested_type_fqn(&self, segments: &[&str]) -> Option<String> {
        if segments.len() < 2 {
            return None;
        }
        let is_type = |name: &str| {
            let hit = |k: &String| k == name;
            self.symbols.classes.keys().any(hit)
                || self.symbols.enums.keys().any(hit)
                || self.symbols.records.keys().any(hit)
                || self.symbols.interfaces.keys().any(hit)
        };
        (1..segments.len()).rev().find_map(|split| {
            let owner = if split == 1 {
                self.resolve_bare_class_fqn(segments[0])?
            } else {
                let prefix = segments[..split].join(".");
                is_type(&prefix).then_some(prefix)?
            };
            let candidate = format!("{owner}__{}", segments[split..].join("__"));
            is_type(&candidate).then_some(candidate)
        })
    }

    /// How to spell the type `fqn` from the unit being emitted: its bare name
    /// when it lives in this package, a `crate::`-rooted path otherwise.
    pub(crate) fn rust_path_for_type_fqn(&self, fqn: &str) -> String {
        match fqn.rsplit_once('.') {
            Some((pkg, bare)) => {
                let here = self.current_package_path();
                if pkg == here {
                    juxc_lex::to_rust_ident(bare)
                } else {
                    format!("crate::{}", juxc_lex::to_rust_path(fqn))
                }
            }
            None => juxc_lex::to_rust_ident(fqn),
        }
    }

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
        // A single segment goes through the unit's context first; see
        // `resolve_bare_class_fqn` for why an exact key match cannot win.
        if qn.segments.len() > 1 && self.symbols.classes.contains_key(&joined) {
            return Some(joined);
        }
        if qn.segments.len() == 1 {
            // Single bare segment: defer to the shared package-aware resolver
            // (current unit's `unqualified` map → current package → a
            // deterministic, user-preferring fallback). This keeps a bare
            // `Child` bound to THIS unit's package even when another package
            // (or a `rust.std` stub) declares a same-named class.
            // An imported alias of a foreign class resolves to that class, so a
            // static call through `Pcg64Dxsm` is one on the class it aliases
            // (B20).
            return self.resolve_bare_class_fqn(&qn.segments[0].text);
        }
        None
    }

    /// The dotted package of the unit being emitted -- the package a bare name
    /// in it is resolved against. Falls back to the workspace's root package
    /// outside a unit.
    ///
    /// `symbols.package` is the ROOT package only; comparing a type's package
    /// with it made every type of a non-root unit look foreign (or, worse, a
    /// root-package type look local to a unit in another package).
    pub(crate) fn current_package_path(&self) -> String {
        self.current_unit_idx
            .and_then(|i| self.symbols.units.get(i))
            .map(|u| u.package.join("."))
            .unwrap_or_else(|| self.symbols.package.join("."))
    }

    /// The free function a bare callee name means **in the unit being emitted**
    /// (§M.16). Always prefer this over `SymbolTable::lookup_function`, whose
    /// context-free answer let a user's own `void pred(String? s)` re-shape the
    /// arguments of `jux.std.collections.Iterator.any`'s `pred` parameter and
    /// fail the build inside the standard library (ERRATA E96).
    pub(crate) fn lookup_function_here(
        &self,
        name: &str,
    ) -> Option<(&str, &juxc_tycheck::symbol_table::FunctionSig)> {
        self.symbols
            .lookup_function_in(name, &self.current_package_path())
    }

    /// The FQN a bare TYPE name -- class, record, enum, interface or alias --
    /// means in the unit being emitted: an import or alias of it, then the
    /// unit's own package, then the table's package-preferring scan.
    ///
    /// The context-free `SymbolTable::find_fqn_by_bare` answered with the
    /// same type in every unit. With `import app.model.Failure;` beside an
    /// `app.errors.Failure`, `new Failure("disk")` then built the exception.
    pub(crate) fn resolve_bare_type_fqn(&self, name: &str) -> Option<String> {
        let is_type = |fqn: &str| {
            self.symbols.classes.contains_key(fqn)
                || self.symbols.records.contains_key(fqn)
                || self.symbols.enums.contains_key(fqn)
                || self.symbols.interfaces.contains_key(fqn)
                || self.symbols.aliases.contains_key(fqn)
        };
        if name.contains('.') {
            return is_type(name).then(|| name.to_string());
        }
        let pkg = self.current_package_path();
        if let Some(ctx) = self.current_unit_idx.and_then(|i| self.symbols.units.get(i)) {
            if let Some(fqn) = ctx.unqualified.get(name).filter(|f| is_type(f)) {
                return Some(fqn.clone());
            }
        }
        self.symbols.find_fqn_by_bare_in(name, &pkg)
    }

    /// Resolve a bare (or already-FQN) class name to its FQN key in
    /// `symbols.classes`, using the **current unit's package context** so that
    /// same-named classes in different packages stay distinct and a user class
    /// shadows an auto-loaded `rust.std` stub. Resolution order:
    ///
    /// 1. `name` is already a known FQN key → use it.
    /// 2. the current unit's `unqualified` bare→FQN map — the authoritative
    ///    answer for same-package siblings and explicit imports / aliases.
    /// 3. the current package prefix (`pkg.name`) if that names a known class —
    ///    a same-package safety net.
    /// 4. fallback for an unqualified reference to another package (lenient
    ///    Phase-1 behavior): prefer a non-`external` (user) match over a stub,
    ///    then the lexicographically smallest FQN so the result is
    ///    **deterministic** (a raw `HashMap` scan is iteration-order dependent,
    ///    which would make emission non-reproducible).
    pub(crate) fn resolve_bare_class_fqn(&self, name: &str) -> Option<String> {
        let ctx = self.current_unit_idx.and_then(|i| self.symbols.units.get(i));
        let pkg = ctx.map(|c| c.package.join(".")).unwrap_or_default();
        resolve_class_name(&self.symbols, ctx, &pkg, name).or_else(|| {
            // A FOREIGN crate's alias of one of its classes (`Pcg64Dxsm` for
            // `Lcg128CmDxsm64`) is that class; a Jux alias is expanded by the
            // checker before the backend sees it.
            ctx.and_then(|c| c.unqualified.get(name))
                .and_then(|fqn| self.symbols.alias_class(fqn))
                .filter(|class| self.symbols.classes.get(class).is_some_and(|c| c.is_external))
        })
    }

    /// Whether the class `name` (bare, as written, or an FQN) lowers to a
    /// newtype handle. The representation sets are keyed by FQN, so the name
    /// is resolved in the unit being emitted first -- two packages' classes of
    /// one name can take different representations.
    pub(crate) fn is_wrapper_class(&self, name: &str) -> bool {
        self.resolve_bare_class_fqn(name).is_some_and(|fqn| self.wrapper_classes.contains(&fqn))
    }

    /// Whether the class `name` is a polymorphic base, reached through
    /// `Rc<dyn …Kind>`. Keyed by FQN; see [`Self::is_wrapper_class`].
    pub(crate) fn is_poly_base_class(&self, name: &str) -> bool {
        self.resolve_bare_class_fqn(name).is_some_and(|fqn| self.poly_base_classes.contains(&fqn))
    }

    /// Whether the class `name` uses the interior-mutable `Rc<RefCell>` handle.
    /// See [`Self::is_wrapper_class`].
    pub(crate) fn is_refcell_class(&self, name: &str) -> bool {
        self.resolve_bare_class_fqn(name).is_some_and(|fqn| self.refcell_classes.contains(&fqn))
    }

    /// Whether the class `name` uses the unique `Box` handle. See
    /// [`Self::is_wrapper_class`].
    pub(crate) fn is_box_class(&self, name: &str) -> bool {
        self.resolve_bare_class_fqn(name).is_some_and(|fqn| self.box_classes.contains(&fqn))
    }

    /// Resolve a bare or FQN class name to its [`ClassSig`], package-aware via
    /// [`Self::resolve_bare_class_fqn`]. Used by emission helpers that hold
    /// `self.enclosing_class` (bare in the source) but need the FQN-keyed
    /// `symbols.classes` entry.
    pub(crate) fn lookup_class_by_bare_or_fqn(
        &self,
        name: &str,
    ) -> Option<&juxc_tycheck::symbol_table::ClassSig> {
        let fqn = self.resolve_bare_class_fqn(name)?;
        self.symbols.classes.get(&fqn)
    }

    /// A field of class `name` or of one of its ancestors, with the class that
    /// declares it, resolving a bare `name` in the unit being emitted first
    /// (see [`Self::lookup_class_by_bare_or_fqn`]).
    ///
    /// `SymbolTable::lookup_field` is keyed by FQN. The backend mostly holds
    /// bare names (`receiver_class_bare`, `enclosing_class`), which match the
    /// key only for a class in the default package: inside `package zoo;` the
    /// key is `zoo.Animal`, the bare lookup missed, and a field read through
    /// an `Animal` reference fell back to a direct struct access on the
    /// `Rc<dyn AnimalKind>` handle.
    pub(crate) fn lookup_field_by_bare_or_fqn(
        &self,
        name: &str,
        field: &str,
    ) -> Option<(&juxc_tycheck::symbol_table::FieldSig, &str)> {
        let fqn = self.resolve_bare_class_fqn(name)?;
        self.symbols.lookup_field(&fqn, field)
    }

    /// Bare-or-FQN sibling of [`Self::lookup_class_by_bare_or_fqn`] for
    /// interfaces — same current-unit package-context resolution (unit
    /// `unqualified` map, then current package, then a deterministic
    /// lowest-FQN fallback).
    pub(crate) fn lookup_interface_by_bare_or_fqn(
        &self,
        name: &str,
    ) -> Option<(&str, &juxc_tycheck::symbol_table::InterfaceSig)> {
        // A name written in full is that name.
        if name.contains('.') {
            if let Some((k, i)) = self.symbols.interfaces.get_key_value(name) {
                return Some((k.as_str(), i));
            }
        }
        // **The unit being emitted resolves the name first** (§M.16): its imports
        // and its own package. An exact key lookup used to come ahead of this,
        // and for a bare name an exact key is a ROOT-package declaration, so a
        // user `interface Iterable` answered for
        // `jux.std.collections.LazyIterable implements Iterable`. LazyIterable
        // then got an EMPTY trait impl (the user's interface declares methods it
        // does not have), and rustc reported E0046 against a standard-library
        // file (ERRATA E96).
        if let Some(idx) = self.current_unit_idx {
            if let Some(ctx) = self.symbols.units.get(idx) {
                if let Some(fqn) = ctx.unqualified.get(name) {
                    if let Some((k, i)) = self.symbols.interfaces.get_key_value(fqn) {
                        return Some((k.as_str(), i));
                    }
                }
                if !ctx.package.is_empty() {
                    let cand = format!("{}.{}", ctx.package.join("."), name);
                    if let Some((k, i)) = self.symbols.interfaces.get_key_value(&cand) {
                        return Some((k.as_str(), i));
                    }
                }
            }
        }
        let pkg = self.current_package_path();
        let here_is_library = juxc_tycheck::symbol_table::is_library_realm_package(&pkg);
        let realm_ok = |k: &str| {
            !here_is_library
                || juxc_tycheck::symbol_table::is_library_realm_package(
                    k.rsplit_once('.').map(|(p, _)| p).unwrap_or(""),
                )
        };
        if let Some((k, i)) = self
            .symbols
            .interfaces
            .get_key_value(name)
            .filter(|(k, _)| realm_ok(k))
        {
            return Some((k.as_str(), i));
        }
        self.symbols
            .interfaces
            .iter()
            .filter(|(k, _)| fqn_bare(k) == name && realm_ok(k))
            .min_by(|a, b| a.0.cmp(b.0))
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
                // A package segment may be a Rust keyword (`demo.box`).
                self.w.push_str(&juxc_lex::to_rust_ident(seg));
                self.w.push_str("::");
            }
        } else if self.split_files.is_some() {
            // Multi-file output: a no-package user type lives at the crate root
            // (in `main.rs`). A packaged unit now lives in its own deeper module
            // file, where a bare `Foo` would resolve relative to that module —
            // so crate-root it. `crate::Foo` also resolves fine from `main.rs`.
            self.w.push_str("crate::");
        }
        self.w.push_str(fqn_bare(fqn));
    }
}

/// The FQN a class name means in a unit with context `ctx` and package `pkg`:
/// an already-qualified key; then, for a BARE name, the unit's imports and
/// same-package types; then a no-package class of exactly that name; then a
/// deterministic scan that prefers user classes over external stubs.
///
/// The unit's own context comes first. The reverse order let a program's
/// no-package `Registry` (keyed just `Registry`) win inside `jux.meta`, where
/// `Registry` means `jux.meta.Registry`. Free of the emitter so the whole-program
/// analyses that run before emission resolve names the same way.
pub(crate) fn resolve_class_name(
    symbols: &juxc_tycheck::SymbolTable,
    ctx: Option<&juxc_tycheck::symbol_table::UnitContext>,
    pkg: &str,
    name: &str,
) -> Option<String> {
    if name.contains('.') && symbols.classes.contains_key(name) {
        return Some(name.to_string());
    }
    if let Some(ctx) = ctx {
        if let Some(fqn) = ctx.unqualified.get(name).filter(|f| symbols.classes.contains_key(*f)) {
            return Some(fqn.clone());
        }
    }
    if !pkg.is_empty() {
        let cand = format!("{pkg}.{name}");
        if symbols.classes.contains_key(&cand) {
            return Some(cand);
        }
    }
    // A NON-CLASS type of this name, in the unit's own package or brought in by
    // one of its imports, shadows any same-named class elsewhere. Inside
    // `package jux.std.result` the bare `Result` is that package's enum, and the
    // searches below would otherwise walk past it and land on a user's
    // `class Result` in the default package: `examples/stress_upcast.jux`
    // declares one, and its `Rc<dyn ResultKind>` lowering then leaked into the
    // stdlib's own `Result<R, E>` return types (rustc E0405).
    let names_a_non_class = |fqn: &str| {
        symbols.enums.contains_key(fqn)
            || symbols.records.contains_key(fqn)
            || symbols.interfaces.contains_key(fqn)
    };
    if !pkg.is_empty() && names_a_non_class(&format!("{pkg}.{name}")) {
        return None;
    }
    if ctx.is_some_and(|c| c.unqualified.get(name).is_some_and(|f| names_a_non_class(f))) {
        return None;
    }
    if symbols.classes.contains_key(name) {
        return Some(name.to_string());
    }
    symbols
        .classes
        .iter()
        .filter(|(k, _)| fqn_bare(k) == name)
        .min_by(|a, b| a.1.is_external.cmp(&b.1.is_external).then_with(|| a.0.cmp(b.0)))
        .map(|(k, _)| k.clone())
}
