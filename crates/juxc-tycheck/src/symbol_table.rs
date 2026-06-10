//! Phase A of the type checker — the symbol table.
//!
//! Walks a [`CompilationUnit`] once and produces a flat catalog of every
//! top-level declaration the program contains:
//!
//! - [`ClassSig`] — name, generics, parent, interfaces, fields,
//!   constructors, methods.
//! - [`RecordSig`] — name, generics, header components.
//! - [`EnumSig`] — name, generics, variants (with payload types).
//! - [`InterfaceSig`] — name, generics, method signatures.
//! - [`FunctionSig`] — top-level functions outside any class.
//!
//! The signatures store **the types as written** — no generic
//! instantiation, no inheritance flattening. Later phases (expression
//! typing, method resolution) read from this table and do the
//! instantiation work at usage sites.
//!
//! The build pass emits duplicate-declaration diagnostics
//! (`E0400`–`E0403`) when it sees the same top-level name twice, the
//! same field/method twice inside a class, or the same variant twice
//! inside an enum. Other type-system checks live in later phases.

use std::collections::HashMap;

use juxc_ast::{
    ClassDecl, CompilationUnit, EnumDecl, FieldDecl, FnDecl, InterfaceDecl, OperatorDecl,
    OperatorKind, RecordDecl, ReturnType, TopLevelDecl, TypeParam, TypeRef, Visibility,
};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

// ============================================================================
// Top-level table
// ============================================================================

/// Symbol table for one compilation unit.
///
/// Every public-facing type identity in the unit appears here exactly
/// once. Look up by name; the returned `*Sig` carries everything
/// downstream phases need to reason about that declaration.
///
/// **The table is read-only after [`build`].** Don't mutate during
/// type checking — clone or borrow.
#[derive(Debug, Default, Clone)]
pub struct SymbolTable {
    /// Dotted path from the FIRST unit's `package foo.bar;` line, or
    /// empty when no package was given. Carried into the backend so
    /// single-file emission still wraps in a matching module
    /// hierarchy. Multi-unit emission reads the per-unit package
    /// directly from each `CompilationUnit`.
    pub package: Vec<String>,
    /// Top-level classes indexed by **fully-qualified name** —
    /// `package.dot.path.ClassName` (or bare `ClassName` when the
    /// class has no package). Bare-name references in user source
    /// are resolved to an FQN via the per-unit
    /// [`UnitContext::unqualified`] map before lookup.
    pub classes: HashMap<String, ClassSig>,
    /// Top-level records indexed by FQN. Same shape as `classes`.
    pub records: HashMap<String, RecordSig>,
    /// Top-level enums indexed by FQN. Same shape as `classes`.
    pub enums: HashMap<String, EnumSig>,
    /// Top-level interfaces indexed by FQN. Same shape as `classes`.
    pub interfaces: HashMap<String, InterfaceSig>,
    /// Top-level functions (outside any class) indexed by FQN.
    /// Overloads aren't supported yet — a duplicate emits `E0400`.
    pub functions: HashMap<String, FunctionSig>,
    /// Type aliases (`type Name<...>? = TypeRef;`) indexed by FQN.
    /// Tycheck expands a reference to an alias into its target
    /// before further inference — `Ty::User` never holds an alias
    /// name once `ty_from_ref` is done.
    pub aliases: HashMap<String, TypeAliasSig>,
    /// Top-level constants (`const T NAME = expr;`) indexed by FQN.
    /// Only the declared type is recorded — the initializer is
    /// only walked once by tycheck and lowered by the backend.
    pub consts: HashMap<String, ConstSig>,
    /// One entry per input unit (parallel to the `units` slice
    /// `build_workspace` received). Each entry carries the unit's
    /// package and its bare-name → FQN map (the closure of the
    /// `package` declaration plus every `import` line). Tycheck
    /// reads from here to seed `TypeEnv` at the start of each
    /// per-unit check. Single-unit builds always have a single
    /// entry at index `0`.
    pub units: Vec<UnitContext>,
    /// FQN of every top-level declaration → the index of the unit that declared
    /// it (into the workspace `sources`/`units` slice). Powers goto-definition:
    /// the editor resolves an identifier to its FQN, looks up the declaring unit
    /// here, and pairs it with the matching `*Sig::span` to point at the
    /// declaration's source file — including a generated `rust.std` / crate
    /// `.jux.d` stub. Every kind (class, record, enum, interface, function,
    /// alias, const) is recorded.
    pub decl_unit: HashMap<String, usize>,
}

/// Per-unit name-resolution context built once during
/// [`build_workspace`].
#[derive(Debug, Default, Clone)]
pub struct UnitContext {
    /// Dotted package path declared at the top of the file.
    pub package: Vec<String>,
    /// Bare-name → FQN map seeded from the unit's `package` and
    /// `import` declarations. `ty_from_ref` consults this when it
    /// encounters a single-segment type reference. Names that
    /// aren't in the map fall through to other resolution rules
    /// (primitives, `String`, generic params, etc.).
    pub unqualified: HashMap<String, String>,
}

impl SymbolTable {
    /// True if `name` refers to a class, record, enum, or interface
    /// declared in this unit. Useful for the resolver / backend when
    /// they need to know whether a TypeRef points at a user type.
    pub fn is_type_name(&self, name: &str) -> bool {
        self.classes.contains_key(name)
            || self.records.contains_key(name)
            || self.enums.contains_key(name)
            || self.interfaces.contains_key(name)
            || self.aliases.contains_key(name)
    }

    /// Same as [`Self::is_type_name`] but ALSO accepts a bare name
    /// that matches the last segment of any known FQN. Lets user
    /// code reference stdlib types (`Map<K, V>`, `Throwable`, etc.)
    /// without writing the full `jux.std.…` path — same shape as
    /// Java's implicit `java.lang.*` rule.
    ///
    /// Kept separate from `is_type_name` so the symbol-table
    /// builder's uniqueness check doesn't reject a user `class
    /// Foo` against an unrelated stdlib FQN whose last segment
    /// happens to be `Foo`.
    pub fn is_type_name_or_stdlib(&self, name: &str) -> bool {
        self.is_type_name(name) || self.find_fqn_by_bare(name).is_some()
    }

    /// Look up a bare type name (e.g. `Map`) against every FQN in
    /// the symbol table, returning the first FQN whose last
    /// segment matches. Drives the "implicit `jux.std.*` import"
    /// rule — user code can spell `Map<K, V>` and have it resolve
    /// to `jux.std.collections.Map`. Returns `None` when no FQN
    /// matches.
    ///
    /// Precedence (when multiple FQNs share a last segment):
    /// classes > records > enums > interfaces > aliases. Order
    /// inside each category is HashMap-iteration which is stable
    /// per session.
    pub fn find_fqn_by_bare(&self, name: &str) -> Option<String> {
        let matches_last = |fqn: &String| -> bool {
            fqn.rsplit('.').next().is_some_and(|seg| seg == name)
        };
        // Classes: an unqualified name must prefer a **non-external** class
        // (the user's own / `jux.std`) over an auto-loaded `.jux.d` stub class
        // (the Rust-std view). Names like `Box`, `String`, `Vec`, `HashMap`
        // exist in BOTH `jux.std` and the generated `rust.std` surface; without
        // this preference an unqualified `Box` could bind to `rust.std.Box` and
        // the backend would emit a non-existent `rust::std::Box` path. The
        // Rust-std type stays reachable through an explicit `import rust.std.Box`
        // (exact-FQN resolution, which never comes here).
        if let Some(k) = self
            .classes
            .iter()
            .find(|(k, sig)| matches_last(k) && !sig.is_external)
            .map(|(k, _)| k.clone())
        {
            return Some(k);
        }
        // A user-declared record / enum / interface / alias of this bare name
        // takes precedence over an EXTERNAL (`rust.std`) class of the same name
        // — e.g. a user `enum Dir` must shadow the `std::fs::Dir` stub class.
        // These tables hold user declarations (stub types land in `classes`),
        // so a match here is the user's own type.
        if let Some(k) = self.records.keys().find(|k| matches_last(k)) {
            return Some(k.clone());
        }
        if let Some(k) = self.enums.keys().find(|k| matches_last(k)) {
            return Some(k.clone());
        }
        if let Some(k) = self.interfaces.keys().find(|k| matches_last(k)) {
            return Some(k.clone());
        }
        if let Some(k) = self.aliases.keys().find(|k| matches_last(k)) {
            return Some(k.clone());
        }
        // Last resort: an external stub class (the Rust-std view). Reached only
        // when no user type of this name exists.
        if let Some(k) = self.classes.keys().find(|k| matches_last(k)) {
            return Some(k.clone());
        }
        None
    }

    /// Resolve an identifier to the location of its declaration: the index of
    /// the declaring unit (into the workspace `sources`/`units`) and the
    /// declaration's source span. Powers goto-definition.
    ///
    /// `name` may be a fully-qualified name or a bare last segment; types,
    /// free functions, constants, and type aliases are all considered. Returns
    /// `None` when nothing matches. The span is **relative to the declaring
    /// unit's source** (caller pairs it with `sources[unit]` to build a
    /// `file:line:col` location).
    pub fn definition_of(&self, name: &str) -> Option<(usize, Span)> {
        let fqn = self.canonical_fqn(name)?;
        let unit = *self.decl_unit.get(&fqn)?;
        let span = self.decl_span(&fqn)?;
        Some((unit, span))
    }

    /// Map a bare-or-qualified identifier to the exact FQN key it denotes,
    /// across every top-level kind. Exact-key hits win; otherwise the
    /// last-segment match (`find_fqn_by_bare` for types, plus functions /
    /// consts) applies.
    fn canonical_fqn(&self, name: &str) -> Option<String> {
        if self.is_type_name(name)
            || self.functions.contains_key(name)
            || self.consts.contains_key(name)
        {
            return Some(name.to_string());
        }
        if let Some(fqn) = self.find_fqn_by_bare(name) {
            return Some(fqn);
        }
        let matches_last = |fqn: &String| fqn.rsplit('.').next().is_some_and(|s| s == name);
        self.functions
            .keys()
            .find(|k| matches_last(k))
            .or_else(|| self.consts.keys().find(|k| matches_last(k)))
            .cloned()
    }

    /// The declaration span recorded for an exact FQN, looked up across kinds.
    fn decl_span(&self, fqn: &str) -> Option<Span> {
        if let Some(s) = self.classes.get(fqn) {
            return Some(s.span);
        }
        if let Some(s) = self.records.get(fqn) {
            return Some(s.span);
        }
        if let Some(s) = self.enums.get(fqn) {
            return Some(s.span);
        }
        if let Some(s) = self.interfaces.get(fqn) {
            return Some(s.span);
        }
        if let Some(s) = self.functions.get(fqn) {
            return Some(s.span);
        }
        if let Some(s) = self.aliases.get(fqn) {
            return Some(s.span);
        }
        self.consts.get(fqn).map(|s| s.span)
    }

    /// Walk `class_name`'s `extends` chain looking for a method named
    /// `method_name`. Returns the matching [`MethodSig`] **and** the
    /// name of the class that actually declared it.
    ///
    /// The declaring-class name is the load-bearing return value for
    /// Phase E: substituting a receiver's generic arguments into the
    /// method signature only makes sense when the receiver's class
    /// matches the declaring class. When they differ (the method came
    /// from an ancestor), the caller leaves `Ty::Param` references in
    /// place — cross-extends substitution would need to thread the
    /// ancestor's `extends` clause through, which isn't wired up yet.
    ///
    /// A 64-step recursion cap guards against cycles the build pass
    /// somehow missed (today the build pass doesn't check for cycles at
    /// all — class hierarchies trust the resolver).
    pub fn lookup_method<'a>(
        &'a self,
        class_name: &str,
        method_name: &str,
    ) -> Option<(&'a MethodSig, &'a str)> {
        // Pass 1: walk the class-extends chain. Inherent methods
        // shadow interface defaults, so we look for them first.
        let mut cursor: Option<&str> = Some(class_name);
        let mut depth = 0usize;
        let mut implements_chain: Vec<&'a str> = Vec::new();
        while let Some(name) = cursor {
            if depth > 64 {
                return None;
            }
            // We use `get_key_value` so the borrow of the class signature
            // and the borrow of the key string outlive the loop body in
            // the same lifetime — needed to return `(&MethodSig, &str)`.
            let (class_key, class) = self.classes.get_key_value(name)?;
            if let Some(m) = class.methods.get(method_name) {
                return Some((m, class_key.as_str()));
            }
            // Collect this class's implemented interfaces for the
            // Pass-2 default-method walk below.
            for iface_ty in &class.implements {
                if let Some(seg) = iface_ty.name.segments.last() {
                    implements_chain.push(seg.text.as_str());
                }
            }
            // Hop to the resolved parent FQN (set during the
            // `resolve_class_chain_fqns` finalize pass). Falling
            // back to the bare last segment keeps no-package /
            // single-unit programs working unchanged.
            cursor = class
                .extends_fqn
                .as_deref()
                .or_else(|| {
                    class
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
                });
            depth += 1;
        }
        // Pass 2: no inherent method found — look for a
        // default-method match on any implemented interface. The
        // declaring-class string we return points at the
        // interface, so call-site emission can still find the
        // method's signature; the backend's trait-dispatch
        // emission picks up the default body.
        //
        // The implements list stores the user's written name (often
        // a bare segment like `Loggable` after `import …`), while
        // the interfaces table is keyed by FQN
        // (`shop.users.Loggable`). When the bare name doesn't match
        // a key directly, fall back to scanning for an interface
        // whose FQN's last segment matches. Multiple matches across
        // packages are a future ambiguity to diagnose; for now we
        // pick the first hit.
        for iface_name in implements_chain {
            if let Some((iface_key, iface)) = self.interfaces.get_key_value(iface_name) {
                if let Some(m) = iface.methods.get(method_name) {
                    return Some((m, iface_key.as_str()));
                }
                continue;
            }
            for (iface_key, iface) in &self.interfaces {
                let last = iface_key.rsplit('.').next().unwrap_or(iface_key.as_str());
                if last == iface_name {
                    if let Some(m) = iface.methods.get(method_name) {
                        return Some((m, iface_key.as_str()));
                    }
                }
            }
        }
        None
    }

    /// Look up an enum by name — exact FQN key first, then a unique
    /// `.{name}`-suffix match. Locally-inferred types often carry the
    /// bare name (`Tier`) while the table keys by FQN (`probe.Tier`);
    /// the suffix fallback keeps variant-access inference and switch
    /// exhaustiveness working for those. Ambiguous suffixes (two
    /// packages declaring the same enum name) return `None` rather
    /// than guessing.
    pub fn lookup_enum<'a>(&'a self, name: &str) -> Option<(&'a str, &'a EnumSig)> {
        if let Some((k, e)) = self.enums.get_key_value(name) {
            return Some((k.as_str(), e));
        }
        if name.contains('.') {
            return None;
        }
        let suffix = format!(".{name}");
        let mut hits = self.enums.iter().filter(|(k, _)| k.ends_with(&suffix));
        match (hits.next(), hits.next()) {
            (Some((k, e)), None) => Some((k.as_str(), e)),
            _ => None,
        }
    }

    /// Walk `class_name`'s `extends` chain looking for a field named
    /// `field_name`. Returns the matching [`FieldSig`] **and** the name
    /// of the class that actually declared it. See [`Self::lookup_method`]
    /// for why the declaring-class name matters.
    pub fn lookup_field<'a>(
        &'a self,
        class_name: &str,
        field_name: &str,
    ) -> Option<(&'a FieldSig, &'a str)> {
        let mut cursor: Option<&str> = Some(class_name);
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return None;
            }
            let (class_key, class) = self.classes.get_key_value(name)?;
            if let Some(field) = class.fields.get(field_name) {
                return Some((field, class_key.as_str()));
            }
            // Use the resolved parent FQN — see `lookup_method` for
            // why the fallback to a bare last segment remains.
            cursor = class
                .extends_fqn
                .as_deref()
                .or_else(|| {
                    class
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
                });
            depth += 1;
        }
        None
    }
}

// ============================================================================
// Per-decl signature types
// ============================================================================

/// Signature of a top-level class declaration.
#[derive(Debug, Clone)]
pub struct ClassSig {
    /// Source visibility (`public`, `private`, etc.).
    pub visibility: Visibility,
    /// Dotted path of the package this class lives in (empty when the
    /// declaring unit had no `package foo.bar;` line). Used by
    /// `check_visibility` to decide E0416 cross-package
    /// package-private access.
    pub package: Vec<String>,
    /// True when the class is declared `abstract`.
    pub is_abstract: bool,
    /// True when this class came from a `.jux.d` declaration stub
    /// (JUX-BINDGEN-ADDENDUM §G.9) — an `external`, signature-only view of a
    /// foreign (Rust/C/C++) type. Stub classes are exempt from the Turn-1
    /// "single constructor" limit (§G.5.1/§G.5.2 needs overloaded `new`) and
    /// from the "abstract method only in abstract class" rule (a stub's
    /// bodyless methods are signatures, not abstract members to implement),
    /// because the real foreign type provides the bodies at link time.
    pub is_external: bool,
    /// For an `is_external` stub type: the **real** fully-qualified Rust path of
    /// the foreign type (`std::collections::HashSet`), recovered from the
    /// `@rust("…")` annotation bindgen emits (§G.9.2). The backend lowers an
    /// `import`/reference of this type to its real Rust symbol via this path
    /// (e.g. `use std::collections::HashSet;`) instead of the flat Jux
    /// `rust::std::HashSet` spelling, so user code that *uses* the type compiles.
    /// `None` for ordinary (non-stub) classes and stubs without the annotation.
    pub rust_path: Option<String>,
    /// True when the class declares one or more `static { }` blocks (§S.4.1).
    /// The backend forces the class's once-guarded `__static_init()` on first
    /// observable use — construction, a static method call, or a static-field
    /// read/write — so it runs before the class is observed initialized.
    pub has_static_init: bool,
    /// True when the class is declared `final` — no other class may
    /// extend it. Enforced by `check_final_and_sealed_extends`.
    pub is_final: bool,
    /// True when the class is declared `sealed`. Sealed classes
    /// restrict their subclasses to the explicit `permits` list.
    pub is_sealed: bool,
    /// Subclass names listed in the `permits` clause (only meaningful
    /// when `is_sealed`).
    pub permits: Vec<String>,
    /// Generic parameters in declaration order, e.g. `<T, U>`.
    pub generic_params: Vec<TypeParam>,
    /// Parent type, if `extends Parent` was given. The TypeRef
    /// preserves the user's source spelling (bare or qualified);
    /// resolved-to-FQN form lives in `extends_fqn` for fast lookups.
    pub extends: Option<TypeRef>,
    /// Fully-qualified name of the parent class. Resolved once at
    /// build time using the declaring unit's bare→FQN map, so chain
    /// walks (`lookup_field`, `lookup_method`,
    /// `walk_extends_reaches`, …) can key directly into the
    /// FQN-keyed `classes` table without re-resolving every hop.
    /// `None` when this class has no `extends` clause.
    pub extends_fqn: Option<String>,
    /// Interfaces declared in the `implements` clause.
    pub implements: Vec<TypeRef>,
    /// Fields indexed by name. Duplicate fields emit `E0401` during build
    /// and the second declaration is dropped.
    pub fields: HashMap<String, FieldSig>,
    /// Constructors in declaration order. Multiple constructors aren't
    /// supported by the parser yet; the table records all that arrive.
    pub constructors: Vec<ConstructorSig>,
    /// Methods indexed by name. Overloads aren't supported yet — a
    /// duplicate emits `E0402`.
    pub methods: HashMap<String, MethodSig>,
    /// Operator overload declarations per `JUX-OPERATORS-ADDENDUM.md`
    /// §O.2, indexed by [`OperatorKind`]. Each operator appears at
    /// most once today — a class with two `operator+` declarations
    /// emits `E0402`. Arity-based overloading (binary vs unary `+`)
    /// would need keying by `(kind, arity)`; spec §O.2.3 calls out
    /// "Binary or unary" but doesn't say a single class can declare
    /// both at once, so we keep the simpler keying for now.
    pub operators: HashMap<OperatorKind, OperatorSig>,
    /// C#-style property metadata, indexed by property name
    /// (JUX-MISSING-DEFS §M.7). The property's getter / setter are
    /// *also* present in [`Self::methods`] (the parser desugared them),
    /// but this map preserves the write-access shape — read-only /
    /// init-only / per-accessor visibility — so tycheck can enforce
    /// §M.7.2 access control on `obj.Prop = v` writes. Empty for
    /// classes with no properties.
    pub properties: HashMap<String, PropertySig>,
    /// Span of the whole class declaration.
    pub span: Span,
}

/// Write-access metadata for one C#-style property (§M.7.2). The
/// getter / setter bodies live in [`ClassSig::methods`]; this captures
/// only what the access-control checks need.
#[derive(Debug, Clone)]
pub struct PropertySig {
    /// The property's outer visibility.
    pub visibility: Visibility,
    /// True when the property is `static`.
    pub is_static: bool,
    /// True when the property has *no* settable accessor — read-only
    /// (`{ get; }` / `T Name => e;`). Writable only inside the
    /// declaring constructor (which the parser already lowered to a
    /// direct backing-field write).
    pub is_read_only: bool,
    /// True when the writable accessor is `init` (settable during
    /// construction only). Like read-only for post-construction
    /// writes; the legitimate ctor write was already desugared.
    pub is_init_only: bool,
    /// The setter / init accessor's effective visibility, when the
    /// property is writable. `None` for read-only properties.
    pub setter_visibility: Option<Visibility>,
    /// Declared property type.
    pub ty: TypeRef,
    /// Span of the property declaration.
    pub span: Span,
}

/// Signature of one class field.
#[derive(Debug, Clone)]
pub struct FieldSig {
    /// Field visibility.
    pub visibility: Visibility,
    /// True if the field is declared `static` (class-scoped, not
    /// per-instance). Drives the `ClassName.FIELD` vs `obj.field`
    /// resolution split.
    pub is_static: bool,
    /// True if the field is declared `final` / `const`. For static
    /// fields, picks `pub const` over `pub static` in the emitted
    /// Rust.
    pub is_final: bool,
    /// Declared type as written in source.
    pub ty: TypeRef,
    /// Default initializer expression (`= expr`) if present. Lifted
    /// onto the sig so the backend can reach it when emitting
    /// static-field constants without re-walking the AST.
    pub default: Option<juxc_ast::Expr>,
    /// Span of the field declaration.
    pub span: Span,
}

/// Signature of one class method.
#[derive(Debug, Clone)]
pub struct MethodSig {
    /// Method visibility.
    pub visibility: Visibility,
    /// Source-order annotation list — `@Override`, `@Deprecated`,
    /// `@Cfg(...)`, etc. Built-in semantics are checked at
    /// build/finalize time; user-defined annotations are
    /// currently informational.
    pub annotations: Vec<juxc_ast::Annotation>,
    /// Whether the method is declared `abstract` (no body).
    pub is_abstract: bool,
    /// Whether the method is declared `final` (no overriding by
    /// subclasses). Enforced by `check_final_method_overrides`.
    pub is_final: bool,
    /// Whether the method is declared `static` (class-scoped, no
    /// implicit `this`). Drives the `ClassName.method()` vs
    /// `obj.method()` resolution split and the `&self` omission in
    /// backend emission.
    pub is_static: bool,
    /// True when this `MethodSig` was synthesized from an
    /// expression-bodied property (per JUX-MISSING-DEFS §M.7.4).
    /// The backend's field-access path checks this flag to rewrite
    /// `obj.name` as `obj.name()` so the call syntax stays
    /// Java-shaped at the use site.
    pub is_property: bool,
    /// Whether the method is declared `unsafe` (§A.2.4). Calling it
    /// requires an `unsafe` context, same rule as a free `unsafe` fn (E0506).
    pub is_unsafe: bool,
    /// True for a foreign (`.jux.d`) method whose `throws E` clause maps a Rust
    /// `Result<T, E>` return (§G.5.4) — the call site unwraps it.
    pub is_foreign_result: bool,
    /// Method-level generic parameters, if any.
    pub generic_params: Vec<TypeParam>,
    /// Formal parameters in declaration order.
    pub params: Vec<ParamSig>,
    /// Declared return type (or `void`).
    pub return_type: ReturnType,
    /// Span of the method declaration.
    pub span: Span,
}

/// Signature of one operator overload declared on a class or record
/// (`JUX-OPERATORS-ADDENDUM.md` §O.2 / §O.3.4). Carries the same shape
/// as [`MethodSig`] minus the name (the name is implicit in
/// [`OperatorKind`]) and the abstract flag, plus an `is_deleted` flag
/// that distinguishes the `= delete;` suppression form.
#[derive(Debug, Clone)]
pub struct OperatorSig {
    /// Operator visibility — most operators are `public` but the parser
    /// preserves whatever the user wrote.
    pub visibility: Visibility,
    /// Which operator. Together with the declaring class identity,
    /// this is the dispatch key for §O.2.6 resolution.
    pub kind: OperatorKind,
    /// Formal parameters in declaration order.
    pub params: Vec<ParamSig>,
    /// Declared return type. Tycheck will eventually validate this
    /// against the spec's fixed return-type table (e.g. `bool` for
    /// `==`, `int` for `hash`).
    pub return_type: ReturnType,
    /// True when the declaration is `operator <op>(...) = delete;`
    /// (§O.3.4). Records use this to suppress auto-derived behavior;
    /// the backend skips emission for deleted operators and tycheck
    /// will (in a future turn) reject use sites with E0935.
    pub is_deleted: bool,
    /// Span of the operator declaration.
    pub span: Span,
}

/// Signature of one constructor (parameters + visibility).
#[derive(Debug, Clone)]
pub struct ConstructorSig {
    /// Constructor visibility.
    pub visibility: Visibility,
    /// Formal parameters in declaration order.
    pub params: Vec<ParamSig>,
    /// Span of the constructor declaration.
    pub span: Span,
}

/// A formal parameter — name + declared type. Used in method and
/// function signatures.
#[derive(Debug, Clone)]
pub struct ParamSig {
    /// Parameter name (no `_` prefix or other normalization).
    pub name: String,
    /// Declared type as written.
    pub ty: TypeRef,
    /// The parameter is a foreign borrow (`&T`) — codegen re-adds the call-site
    /// `&` when invoking this (external) method (§G.9.2). False for user params.
    pub is_ref: bool,
}

/// Signature of a top-level record declaration.
#[derive(Debug, Clone)]
pub struct RecordSig {
    /// Record visibility.
    pub visibility: Visibility,
    /// Generic parameters in declaration order.
    pub generic_params: Vec<TypeParam>,
    /// Interfaces declared in the `implements` clause.
    pub implements: Vec<TypeRef>,
    /// Header components in declaration order — each becomes a public
    /// field AND a canonical-constructor parameter.
    pub components: Vec<RecordComponentSig>,
    /// Operator-override declarations on the record body, indexed by
    /// kind. Mirrors [`ClassSig::operators`]; the value carries the
    /// `is_deleted` flag so the backend can distinguish a real
    /// override from a §O.3.4 suppression.
    pub operators: HashMap<OperatorKind, OperatorSig>,
    /// Methods declared in the record body, indexed by name. Mirrors
    /// [`ClassSig::methods`]. Records can declare methods (per
    /// grammar §A.2.4) but not additional fields or constructors —
    /// the header components are the only fields and the canonical
    /// `new(...)` is synthesized. Duplicate names emit `E0402`.
    pub methods: HashMap<String, MethodSig>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// One record header component — name + declared type.
#[derive(Debug, Clone)]
pub struct RecordComponentSig {
    /// Component name (also the public field name).
    pub name: String,
    /// Declared component type.
    pub ty: TypeRef,
}

/// Signature of a top-level enum declaration.
///
/// Enum generic parameters aren't supported in the AST yet — when
/// they land, add `generic_params: Vec<TypeParam>` here alongside the
/// other Sig types.
#[derive(Debug, Clone)]
pub struct EnumSig {
    /// Enum visibility.
    pub visibility: Visibility,
    /// Variants indexed by name. Duplicate variant names emit `E0403`.
    pub variants: HashMap<String, VariantSig>,
    /// Operator-override declarations on the enum body, indexed by
    /// kind. Same shape as [`ClassSig::operators`] /
    /// [`RecordSig::operators`]. Most enums won't have any — natural
    /// variant-order semantics cover the common cases.
    pub operators: HashMap<OperatorKind, OperatorSig>,
    /// Methods declared in the enum body (§A.2.5), keyed by name —
    /// same shape as [`ClassSig::methods`] so call-site inference
    /// reuses the method machinery.
    pub methods: HashMap<String, MethodSig>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// Signature of one enum variant — payload types only. Unit variants
/// have an empty `payload`.
#[derive(Debug, Clone)]
pub struct VariantSig {
    /// Payload component types in declaration order. Empty for unit
    /// variants like `Color.Red`.
    pub payload: Vec<TypeRef>,
    /// Span of the variant declaration.
    pub span: Span,
}

/// Signature of a top-level interface declaration.
#[derive(Debug, Clone)]
pub struct InterfaceSig {
    /// Interface visibility.
    pub visibility: Visibility,
    /// Generic parameters in declaration order.
    pub generic_params: Vec<TypeParam>,
    /// Parent interfaces from the `extends` clause (an interface can
    /// extend multiple interfaces, `classes-rules.md` §3.2). Preserved as
    /// written `TypeRef`s so the subtype walk can follow
    /// interface-extends-interface chains (`class C implements A`, `A
    /// extends B` ⟹ `C <: B`). Empty when there's no `extends` clause.
    pub extends: Vec<TypeRef>,
    /// Method signatures indexed by name. Bodies are absent (`body:
    /// None` in the source). Duplicate names emit `E0402`.
    pub methods: HashMap<String, MethodSig>,
    /// Field signatures indexed by name. Per `classes-rules.md` §3.3
    /// these are implicitly `public static final` — the parser
    /// already forces those flags on the `FieldSig` before it
    /// lands here.
    pub fields: HashMap<String, FieldSig>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// Signature of a top-level function (outside any class).
#[derive(Debug, Clone)]
pub struct FunctionSig {
    /// Function visibility.
    pub visibility: Visibility,
    /// Generic parameters in declaration order.
    pub generic_params: Vec<TypeParam>,
    /// Formal parameters in declaration order.
    pub params: Vec<ParamSig>,
    /// Declared return type (or `void`).
    pub return_type: ReturnType,
    /// Whether the function is declared `unsafe` (§A.2.4). A call to an
    /// `unsafe` function is only legal inside an `unsafe` context — an
    /// `unsafe { … }` block or the body of another `unsafe` fn (E0506).
    /// Foreign `unsafe fn` stubs (e.g. `libc::getpid`) carry this.
    pub is_unsafe: bool,
    /// True for a foreign (`.jux.d`) function whose `throws E` clause maps a
    /// Rust `Result<T, E>` return (§G.5.4). The backend unwraps the `Result`
    /// at the call site and re-throws the error on `Err`.
    pub is_foreign_result: bool,
    /// The real Rust path of a foreign free function (`humantime::parse_duration`)
    /// recovered from its `@rust("…")` annotation. The backend imports it as
    /// `use <rust_path> as <jux_name>;` so the snake_case Rust name resolves
    /// behind the camelCase Jux stub name. `None` for ordinary Jux functions.
    pub rust_path: Option<String>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// Signature of a top-level constant. Spec §A.2.2.
#[derive(Debug, Clone)]
pub struct ConstSig {
    /// Source visibility.
    pub visibility: Visibility,
    /// Declared type — the initializer must match (verified by
    /// tycheck's `check::Checker::check_unit`).
    pub ty: TypeRef,
    /// Span of the whole declaration.
    pub span: Span,
}

/// Signature of a top-level type alias. Spec §A.2.4.
#[derive(Debug, Clone)]
pub struct TypeAliasSig {
    /// Source visibility.
    pub visibility: Visibility,
    /// Generic parameters in declaration order. Empty for a bare
    /// alias like `type StringList = List<String>;`.
    pub generic_params: Vec<TypeParam>,
    /// Target type the alias resolves to.
    pub target: TypeRef,
    /// Index of the unit that declared this alias, into
    /// `SymbolTable::units`. Lets `expand_alias` lower the target
    /// using the **declaring** unit's name-resolution context
    /// rather than the caller's — important when the target
    /// references types that aren't visible from the use site
    /// without an explicit import.
    pub unit_index: Option<usize>,
    /// Span of the whole declaration.
    pub span: Span,
}

// ============================================================================
// Build pass
// ============================================================================

/// Walk `unit` and populate a fresh [`SymbolTable`]. Diagnostics for
/// duplicate top-level / member names are appended to `diagnostics`;
/// the table never contains both halves of a duplicate (the second
/// declaration is dropped on the floor after the diagnostic fires).
///
/// Always returns a table — even on error — so downstream phases can
/// keep running on the surviving subset.
pub fn build(unit: &CompilationUnit, diagnostics: &mut Vec<Diagnostic>) -> SymbolTable {
    build_workspace(std::slice::from_ref(unit), diagnostics)
}

/// Multi-unit variant of [`build`] — feed every unit's top-level
/// declarations into a single workspace [`SymbolTable`]. Duplicate
/// names across files fire `E0400_DuplicateDeclaration` against the
/// second occurrence, same as in-file duplicates do.
///
/// Each unit's `package` declaration is recorded on the per-unit
/// metadata side: the workspace table itself uses the FIRST unit's
/// package as its top-level marker (back-compat with single-file
/// builds). Per-class package tracking lands when we add a
/// `package_path` field on `ClassSig` etc. — for now Phase 1
/// inherits the flat namespace.
pub fn build_workspace(
    units: &[CompilationUnit],
    diagnostics: &mut Vec<Diagnostic>,
) -> SymbolTable {
    let mut table = SymbolTable::default();
    // Capture the first unit's package as the workspace marker so
    // the backend's module-wrapping path keeps working unchanged for
    // single-file workspaces. Multi-file workspaces with distinct
    // packages per unit will need the per-class plumbing in step
    // (this is the seam we extend in the next pass).
    if let Some(first) = units.first() {
        if let Some(pkg) = &first.package {
            table.package = pkg
                .name
                .segments
                .iter()
                .map(|s| s.text.clone())
                .collect();
        }
    }
    // First pass: insert every top-level declaration under its FQN.
    // Track which unit each class came from so the later
    // `extends_fqn`/`implements_fqn` resolution knows which unit's
    // import map to consult.
    let mut class_unit: HashMap<String, usize> = HashMap::new();
    for (unit_idx, unit) in units.iter().enumerate() {
        let unit_pkg: Vec<String> = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
            .unwrap_or_default();
        for item in &unit.items {
            if let TopLevelDecl::Class(c) = item {
                class_unit.insert(make_fqn(&unit_pkg, &c.name.text), unit_idx);
            }
            // Record FQN → declaring-unit for goto-definition (every kind).
            if let Some(name) = top_level_name(item) {
                table.decl_unit.insert(make_fqn(&unit_pkg, name), unit_idx);
            }
            insert_top_level(&mut table, item, &unit_pkg, unit_idx, unit.is_external, diagnostics);
        }
    }
    // Second pass: build per-unit name-resolution contexts now that
    // every class/record/etc. is registered under its FQN.
    table.units = build_unit_contexts(units, &table);
    // Third pass: resolve every class's `extends` / `implements`
    // TypeRefs into FQN strings using the declaring unit's
    // bare→FQN map. Stored on `ClassSig::extends_fqn` so chain
    // walks key directly into the FQN-indexed `classes` table
    // without re-resolving on every hop.
    resolve_class_chain_fqns(&mut table, &class_unit);
    // Cross-class rule passes that need every class registered first:
    // final/sealed extends, final-method override checks, and
    // `@Override`-annotation verification.
    check_final_and_sealed_extends(&table, diagnostics);
    check_final_method_overrides(&table, diagnostics);
    check_override_annotations(&table, diagnostics);
    check_abstract_methods_implemented(&table, diagnostics);
    check_diamond_default_conflicts(&table, diagnostics);
    check_interface_on_exception_class(&table, diagnostics);
    check_polymorphic_base_generic_methods(&table, diagnostics);
    check_method_modifier_combinations(&table, diagnostics);
    check_single_constructor(&table, diagnostics);
    check_top_level_visibility(&table, diagnostics);
    check_override_does_not_narrow_access(&table, diagnostics);
    check_imports_resolve(units, &table, diagnostics);
    table
}

/// Validate that every `import a.b.C;` names a declaration that actually
/// exists. A mismatched import — e.g. `import xss.it.other.Other;` when `Other`
/// is declared in `package xss.it;` — otherwise resolves the type by *bare*
/// name (construction works) but maps method lookups to the non-existent FQN,
/// surfacing as a baffling "no method" error (or leaking to rustc). Flagging the
/// import directly, with a "did you mean" pointing at the real FQN, is the
/// user-actionable error.
///
/// Wildcard imports (`import a.b.*;`) name a package, not a type, and are
/// skipped. External stub types and `jux.std` types are ordinary table entries,
/// so a valid `import rust.std.HashMap;` passes.
fn check_imports_resolve(
    units: &[CompilationUnit],
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use juxc_ast::ImportSpec;
    // A top-level symbol exists under this exact FQN (any kind).
    let exists = |fqn: &str| {
        table.is_type_name(fqn)
            || table.functions.contains_key(fqn)
            || table.consts.contains_key(fqn)
    };
    // A "did you mean" — a known FQN with the same last segment.
    let suggest = |bare: &str| -> Option<String> {
        table
            .classes
            .keys()
            .chain(table.records.keys())
            .chain(table.enums.keys())
            .chain(table.interfaces.keys())
            .chain(table.functions.keys())
            .find(|k| fqn_bare(k) == bare && k.contains('.'))
            .cloned()
    };
    let report = |fqn: String, span: Span, diagnostics: &mut Vec<Diagnostic>| {
        if exists(&fqn) {
            return;
        }
        let bare = fqn_bare(&fqn).to_string();
        let mut msg = format!("unresolved import `{fqn}`: no declaration with that fully-qualified name");
        if let Some(real) = suggest(&bare) {
            if real != fqn {
                msg.push_str(&format!(" (did you mean `{real}`?)"));
            }
        }
        diagnostics.push(
            Diagnostic::error(code::Code::E0301_NameNotFound, msg).with_span(span),
        );
    };
    for unit in units {
        for import in &unit.imports {
            match &import.spec {
                ImportSpec::Path { name, wildcard, .. } => {
                    if *wildcard || name.segments.is_empty() {
                        continue;
                    }
                    let fqn = name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                    report(fqn, import.span, diagnostics);
                }
                ImportSpec::Items { prefix, items } => {
                    let pfx = prefix.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                    for it in items {
                        let fqn = if pfx.is_empty() {
                            it.name.text.clone()
                        } else {
                            format!("{pfx}.{}", it.name.text)
                        };
                        report(fqn, import.span, diagnostics);
                    }
                }
            }
        }
    }
}

/// Insert one top-level item into the table. Factored out of
/// [`build_workspace`] so the per-unit loop stays terse.
/// Compose a fully-qualified name. `make_fqn(["a", "lib"], "Foo")`
/// → `"a.lib.Foo"`. Empty package → bare name unchanged
/// (`make_fqn([], "Foo")` → `"Foo"`) so top-level no-package
/// declarations keep their pre-FQN spelling.
pub(crate) fn make_fqn(package: &[String], bare: &str) -> String {
    if package.is_empty() {
        return bare.to_string();
    }
    let mut out = package.join(".");
    out.push('.');
    out.push_str(bare);
    out
}

/// Strip the trailing identifier off an FQN. `"a.lib.Foo"` → `"Foo"`,
/// `"Foo"` (no package) → `"Foo"`.
pub(crate) fn fqn_bare(fqn: &str) -> &str {
    match fqn.rsplit_once('.') {
        Some((_, bare)) => bare,
        None => fqn,
    }
}

/// Strip the trailing identifier off an FQN and return the package
/// prefix. `"a.lib.Foo"` → `Some("a.lib")`, `"Foo"` → `None`.
pub(crate) fn fqn_package(fqn: &str) -> Option<&str> {
    fqn.rsplit_once('.').map(|(pkg, _)| pkg)
}

/// Compute the per-unit name-resolution contexts after every
/// top-level declaration is registered in `table`. One context per
/// input unit, parallel to the `units` slice.
///
/// For each unit, the resolver map is seeded from:
/// 1. Same-package siblings — every FQN whose package portion
///    matches the unit's own package contributes
///    `bare -> fqn`.
/// 2. Each `import com.foo.Bar;` adds `Bar -> com.foo.Bar` (or
///    `import com.foo.Bar as X;` → `X -> com.foo.Bar`).
/// 3. Wildcard imports `import com.foo.*;` expand to one entry per
///    sibling in the workspace's `com.foo.*` namespace.
/// 4. Grouped imports `import com.foo.{A, B as C};` expand to the
///    obvious mapping.
fn build_unit_contexts(
    units: &[juxc_ast::CompilationUnit],
    table: &SymbolTable,
) -> Vec<UnitContext> {
    let all_fqns: Vec<&String> = table
        .classes
        .keys()
        .chain(table.records.keys())
        .chain(table.enums.keys())
        .chain(table.interfaces.keys())
        .chain(table.functions.keys())
        .chain(table.aliases.keys())
        .collect();
    units
        .iter()
        .map(|unit| {
            let pkg: Vec<String> = unit
                .package
                .as_ref()
                .map(|p| p.name.segments.iter().map(|s| s.text.clone()).collect())
                .unwrap_or_default();
            let pkg_str = pkg.join(".");
            let mut unqualified: HashMap<String, String> = HashMap::new();

            // Same-package siblings reachable by bare name.
            for fqn in &all_fqns {
                let entry_pkg = fqn_package(fqn).unwrap_or("");
                if entry_pkg == pkg_str {
                    unqualified.insert(fqn_bare(fqn).to_string(), (*fqn).clone());
                }
            }

            // Imports.
            for import in &unit.imports {
                seed_unqualified_from_import(&mut unqualified, &import.spec, &all_fqns);
            }

            UnitContext { package: pkg, unqualified }
        })
        .collect()
}

/// After every class is registered and per-unit contexts are built,
/// walk each class and stamp its `extends_fqn` (and, eventually,
/// per-interface `implements_fqn`s) using the declaring unit's
/// bare→FQN map. Resolution rule:
///
/// - Multi-segment `extends a.b.Foo` → take the dot-joined form
///   verbatim if it names a known class; otherwise leave as `None`.
/// - Single-segment `extends Foo` → consult the unit's context.
/// - Unknown name → leave as `None`; downstream resolver/tycheck
///   passes surface the diagnostic.
fn resolve_class_chain_fqns(
    table: &mut SymbolTable,
    class_unit: &HashMap<String, usize>,
) {
    let fqns: Vec<String> = table.classes.keys().cloned().collect();
    for fqn in fqns {
        let Some(&unit_idx) = class_unit.get(&fqn) else {
            continue;
        };
        let ctx = match table.units.get(unit_idx) {
            Some(ctx) => ctx.clone(),
            None => continue,
        };
        let Some(class) = table.classes.get(&fqn) else {
            continue;
        };
        let extends_clone = class.extends.clone();
        if let Some(extends) = extends_clone {
            let resolved = resolve_type_ref_to_fqn(&extends, &ctx, table);
            if let Some(class) = table.classes.get_mut(&fqn) {
                class.extends_fqn = resolved;
            }
        }
    }
}

/// Resolve a `TypeRef` against a unit context to a fully-qualified
/// class name. Mirrors the bare/multi-segment branches in
/// [`crate::ty::ty_from_ref`] but only returns the FQN string —
/// callers downstream use it to key into `table.classes`.
fn resolve_type_ref_to_fqn(
    ty_ref: &TypeRef,
    ctx: &UnitContext,
    table: &SymbolTable,
) -> Option<String> {
    if ty_ref.name.segments.is_empty() {
        return None;
    }
    if ty_ref.name.segments.len() == 1 {
        let bare = &ty_ref.name.segments[0].text;
        if let Some(fqn) = ctx.unqualified.get(bare) {
            if table.classes.contains_key(fqn) {
                return Some(fqn.clone());
            }
        }
        if table.classes.contains_key(bare) {
            return Some(bare.clone());
        }
        // FQN-suffix fallback: a bare `extends Exception` in a user
        // unit must reach the stdlib's `jux.std.exceptions.Exception`
        // even without an explicit import (the exception hierarchy is
        // ambiently available — `throw new Exception(...)` already
        // resolves this way at other sites). Mirrors the backend's
        // `lookup_class_by_bare_or_fqn` suffix scan; without it the
        // member chain stops at the user subclass and inherited
        // fields/methods (`e.message`, `e.getMessage()`) go
        // unresolved (E0412/E0413).
        if let Some(fqn) = table
            .classes
            .keys()
            .find(|k| k.rsplit('.').next().unwrap_or(k.as_str()) == bare)
        {
            return Some(fqn.clone());
        }
        return None;
    }
    let joined: String = ty_ref
        .name
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    if table.classes.contains_key(&joined) {
        return Some(joined);
    }
    None
}

/// Apply a single `import …;` declaration to the bare→FQN map.
fn seed_unqualified_from_import(
    out: &mut HashMap<String, String>,
    spec: &juxc_ast::ImportSpec,
    all_fqns: &[&String],
) {
    use juxc_ast::ImportSpec;
    match spec {
        ImportSpec::Path { name, wildcard: false, alias } => {
            let fqn_segs: Vec<&str> = name.segments.iter().map(|s| s.text.as_str()).collect();
            if fqn_segs.is_empty() {
                return;
            }
            let bare = alias
                .as_ref()
                .map(|a| a.text.clone())
                .unwrap_or_else(|| fqn_segs.last().unwrap().to_string());
            let fqn = fqn_segs.join(".");
            out.insert(bare, fqn);
        }
        ImportSpec::Path { name, wildcard: true, .. } => {
            // `import a.b.*;` — every FQN under `a.b.` (single segment
            // remaining) joins the unqualified set.
            let prefix = name
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(".");
            let pat = format!("{prefix}.");
            for fqn in all_fqns {
                if let Some(rest) = fqn.strip_prefix(&pat) {
                    if !rest.contains('.') {
                        out.insert(rest.to_string(), (*fqn).clone());
                    }
                }
            }
        }
        ImportSpec::Items { prefix, items } => {
            let prefix_str = prefix
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(".");
            for item in items {
                let bare = item
                    .alias
                    .as_ref()
                    .map(|a| a.text.clone())
                    .unwrap_or_else(|| item.name.text.clone());
                let fqn = if prefix_str.is_empty() {
                    item.name.text.clone()
                } else {
                    format!("{prefix_str}.{}", item.name.text)
                };
                out.insert(bare, fqn);
            }
        }
    }
}

/// Extract the real Rust path from a `@rust("std::collections::HashSet")`
/// annotation (§G.9.2), if present. The annotation name is matched
/// case-insensitively (per the built-in annotation rule); the argument is a
/// single positional string literal. Returns `None` when absent or malformed.
fn rust_path_annotation(annotations: &[juxc_ast::Annotation]) -> Option<String> {
    use juxc_ast::{AnnotationArg, Expr, Literal};
    for ann in annotations {
        let Some(seg) = ann.name.segments.last() else { continue };
        if seg.text.to_ascii_lowercase() != "rust" {
            continue;
        }
        if let Some(AnnotationArg::Positional(Expr::Literal(Literal::String(s)))) = ann.args.first()
        {
            if !s.is_empty() {
                return Some(s.clone());
            }
        }
    }
    None
}

/// The declared name of a top-level item, used to key `SymbolTable::decl_unit`.
/// `None` only if a future variant has no name.
fn top_level_name(item: &TopLevelDecl) -> Option<&str> {
    match item {
        TopLevelDecl::Class(d) => Some(&d.name.text),
        TopLevelDecl::Record(d) => Some(&d.name.text),
        TopLevelDecl::Enum(d) => Some(&d.name.text),
        TopLevelDecl::Interface(d) => Some(&d.name.text),
        TopLevelDecl::Function(d) => Some(&d.name.text),
        TopLevelDecl::TypeAlias(d) => Some(&d.name.text),
        TopLevelDecl::Const(d) => Some(&d.name.text),
    }
}

fn insert_top_level(
    table: &mut SymbolTable,
    item: &TopLevelDecl,
    package: &[String],
    unit_idx: usize,
    is_external: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Every top-level kind is keyed by FQN. Records / enums /
    // interfaces / free functions don't yet carry a per-decl
    // `package` field on their Sig structs, but they ARE namespaced
    // by FQN in the table, so two `record Foo`s in different
    // packages coexist without firing E0400.
    match item {
        TopLevelDecl::Class(class_decl) => {
            insert_class(table, class_decl, package, is_external, diagnostics);
        }
        TopLevelDecl::Record(record_decl) => {
            insert_record(table, record_decl, package, is_external, diagnostics);
        }
        TopLevelDecl::Enum(enum_decl) => {
            insert_enum(table, enum_decl, package, diagnostics);
        }
        TopLevelDecl::Interface(interface_decl) => {
            insert_interface(table, interface_decl, package, is_external, diagnostics);
        }
        TopLevelDecl::Function(fn_decl) => {
            insert_function(table, fn_decl, package, is_external, diagnostics);
        }
        TopLevelDecl::TypeAlias(alias) => {
            insert_type_alias(table, alias, package, unit_idx, diagnostics);
        }
        TopLevelDecl::Const(c) => {
            insert_const(table, c, package, diagnostics);
        }
    }
}

/// Register a top-level constant under its FQN. Same E0400-on-
/// duplicate rule as the other top-level kinds.
fn insert_const(
    table: &mut SymbolTable,
    decl: &juxc_ast::ConstDecl,
    package: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fqn = make_fqn(package, &decl.name.text);
    if !ensure_top_level_unique(table, &fqn, decl.span, diagnostics) {
        return;
    }
    table.consts.insert(
        fqn,
        ConstSig {
            visibility: decl.visibility,
            ty: resolve_decl_type(decl.ty.as_ref(), Some(&decl.value), decl.span),
            span: decl.span,
        },
    );
}

fn insert_type_alias(
    table: &mut SymbolTable,
    alias: &juxc_ast::TypeAliasDecl,
    package: &[String],
    unit_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fqn = make_fqn(package, &alias.name.text);
    if !ensure_top_level_unique(table, &fqn, alias.span, diagnostics) {
        return;
    }
    table.aliases.insert(
        fqn,
        TypeAliasSig {
            visibility: alias.visibility,
            generic_params: alias.generic_params.clone(),
            target: alias.target.clone(),
            unit_index: Some(unit_idx),
            span: alias.span,
        },
    );
}


/// Verify every class with an `extends` clause respects its parent's
/// `final` and `sealed` declarations. Emits:
///
/// - **E0420** when the parent is `final` (no subclassing allowed).
/// - **E0422** when the parent is `sealed` and this child isn't in
///   the parent's `permits` list.
///
/// Runs after every class has been inserted, so the lookups can rely
/// on a populated `symbols.classes`.
fn check_final_and_sealed_extends(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (child_name, child) in &table.classes {
        let Some(extends) = child.extends.as_ref() else {
            continue;
        };
        // Prefer the resolved FQN; fall back to the bare last
        // segment so single-unit / no-package builds still work
        // before the FQN finalize pass populates `extends_fqn`.
        let parent_name: &str = match child.extends_fqn.as_deref() {
            Some(fqn) => fqn,
            None => match extends.name.segments.last() {
                Some(seg) => seg.text.as_str(),
                None => continue,
            },
        };
        let Some(parent) = table.classes.get(parent_name) else {
            // Parent isn't a class. If it names a known
            // non-class type (interface / record / enum / alias),
            // fire E0423 — extends only takes classes.
            if table.interfaces.contains_key(parent_name)
                || table.records.contains_key(parent_name)
                || table.enums.contains_key(parent_name)
                || table.aliases.contains_key(parent_name)
            {
                let kind = if table.interfaces.contains_key(parent_name) {
                    "an interface"
                } else if table.records.contains_key(parent_name) {
                    "a record"
                } else if table.enums.contains_key(parent_name) {
                    "an enum"
                } else {
                    "a type alias"
                };
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0423_ExtendsNotAClass,
                        format!(
                            "class `{child_name}` cannot extend `{parent_name}` because it is {kind} (only classes are extensible)",
                        ),
                    )
                    .with_span(extends.span),
                );
            }
            // Else: unknown name — resolver E0301 covers it.
            continue;
        };
        if parent.is_final {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0420_FinalClassExtended,
                    format!(
                        "class `{child_name}` cannot extend `{parent_name}` because `{parent_name}` is declared `final`",
                    ),
                )
                .with_span(extends.span),
            );
        }
        if parent.is_sealed {
            // `permits` stores bare class names (no qualified
            // path allowed by the grammar — `permits Foo, Bar`
            // not `permits pkg.Foo`). The child's table key is
            // FQN. Compare on the child's BARE name so the check
            // works regardless of whether parent and child share
            // a package.
            let child_bare = fqn_bare(child_name);
            if !parent.permits.iter().any(|n| n == child_bare) {
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0422_SealedClassNotPermitted,
                        format!(
                            "class `{child_name}` is not permitted to extend `{parent_name}` (not listed in its `permits` clause)",
                        ),
                    )
                    .with_span(extends.span),
                );
            }
        }
    }
    // Each `implements` entry must name an interface — `class C
    // implements SomeClass` doesn't make sense (it would be a
    // structural error since classes carry concrete impls, not
    // open contracts). Fires E0424 against the offending entry.
    for (child_name, child) in &table.classes {
        for impl_ty in &child.implements {
            let Some(seg) = impl_ty.name.segments.last() else {
                continue;
            };
            let bare = seg.text.as_str();
            // Try the unit's bare→FQN map indirectly: look across
            // every interface/class/record/enum/alias for a
            // matching bare name. Simple-name match is acceptable
            // Phase-1; cross-package implements is unusual.
            let key = if table.interfaces.contains_key(bare) {
                bare.to_string()
            } else {
                // Search FQNs for a matching bare suffix.
                table
                    .interfaces
                    .keys()
                    .chain(table.classes.keys())
                    .chain(table.records.keys())
                    .chain(table.enums.keys())
                    .chain(table.aliases.keys())
                    .find(|fqn| fqn_bare(fqn) == bare)
                    .cloned()
                    .unwrap_or_else(|| bare.to_string())
            };
            if table.interfaces.contains_key(&key) {
                continue; // valid implements
            }
            // Decide which non-interface kind it is (if any) for a
            // precise diagnostic message; unknown names defer to
            // the resolver.
            let kind = if table.classes.contains_key(&key) {
                Some("a class")
            } else if table.records.contains_key(&key) {
                Some("a record")
            } else if table.enums.contains_key(&key) {
                Some("an enum")
            } else if table.aliases.contains_key(&key) {
                Some("a type alias")
            } else {
                None
            };
            if let Some(kind) = kind {
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0424_ImplementsNotAnInterface,
                        format!(
                            "class `{child_name}` cannot implement `{bare}` because it is {kind} (only interfaces appear in `implements`)",
                        ),
                    )
                    .with_span(impl_ty.span),
                );
            }
        }
    }
}

/// Case-insensitive lookup for a built-in annotation by simple
/// name. Per spec: `@Override` ≡ `@override` ≡ `@OVERRIDE`.
/// Multi-segment names (`@foo.Bar`) are only matched on their
/// trailing identifier — built-ins don't live in packages.
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

/// Verify every method annotated with `@Override` actually
/// overrides a method from an ancestor class. Fires E0426 when
/// no matching method exists in the extends chain.
fn check_override_annotations(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (child_name, child) in &table.classes {
        for (method_name, method) in &child.methods {
            if !has_annotation(&method.annotations, "override") {
                continue;
            }
            let mut found = false;

            // Check the implemented interfaces first — Java's
            // `@Override` is valid on a method that satisfies an
            // interface contract, not just one that shadows a
            // superclass method.
            for impl_ty in &child.implements {
                let Some(seg) = impl_ty.name.segments.last() else { continue };
                let bare = seg.text.as_str();
                // Try the FQN'd lookup via the workspace: an
                // interface named `Bar` declared in `pkg` is keyed
                // `pkg.Bar`. Bare-name match is enough since the
                // grammar doesn't yet allow `implements pkg.Bar`
                // and bare imports route through the unit's
                // resolver before reaching this table.
                let key = if table.interfaces.contains_key(bare) {
                    bare.to_string()
                } else {
                    table
                        .interfaces
                        .keys()
                        .find(|fqn| fqn_bare(fqn) == bare)
                        .cloned()
                        .unwrap_or_default()
                };
                if let Some(iface) = table.interfaces.get(&key) {
                    if iface.methods.contains_key(method_name) {
                        found = true;
                        break;
                    }
                }
            }

            // Walk the extends chain looking for the same-named
            // method. Reuses the same FQN-aware cursor pattern as
            // `check_final_method_overrides`.
            let mut cursor: Option<&str> = if found {
                None
            } else {
                child
                    .extends_fqn
                    .as_deref()
                    .or_else(|| {
                        child
                            .extends
                            .as_ref()
                            .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
                    })
            };
            let mut depth = 0usize;
            while let Some(ancestor_name) = cursor {
                if depth > 64 {
                    break;
                }
                let Some(ancestor) = table.classes.get(ancestor_name) else {
                    break;
                };
                if ancestor.methods.contains_key(method_name) {
                    found = true;
                    break;
                }
                cursor = ancestor.extends_fqn.as_deref().or_else(|| {
                    ancestor
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
                });
                depth += 1;
            }
            if !found {
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0426_OverrideMissing,
                        format!(
                            "method `{method_name}` on `{child_name}` is annotated `@Override` but doesn't override any ancestor method",
                        ),
                    )
                    .with_span(method.span),
                );
            }
        }
    }
}

/// Verify that no subclass redeclares a method that the parent
/// marked `final`. Walks each class's `methods` map and, for each
/// method that shadows one further up the extends chain, fires
/// **E0421** if the ancestor's signature had the `final` modifier.
fn check_final_method_overrides(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (child_name, child) in &table.classes {
        for (method_name, child_method) in &child.methods {
            // Walk up the extends chain from the IMMEDIATE parent —
            // skip self. Uses the resolved FQN where possible so
            // cross-package final-method checks work.
            let mut cursor: Option<&str> = child
                .extends_fqn
                .as_deref()
                .or_else(|| {
                    child
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
                });
            let mut depth = 0usize;
            while let Some(ancestor_name) = cursor {
                if depth > 64 {
                    break;
                }
                let Some(ancestor) = table.classes.get(ancestor_name) else {
                    break;
                };
                if let Some(ancestor_method) = ancestor.methods.get(method_name) {
                    if ancestor_method.is_final {
                        diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0421_FinalMethodOverridden,
                                format!(
                                    "method `{method_name}` on `{child_name}` cannot override `{ancestor_name}::{method_name}` because the parent declares it `final`",
                                ),
                            )
                            .with_span(child_method.span),
                        );
                        break;
                    }
                }
                cursor = ancestor.extends_fqn.as_deref().or_else(|| {
                    ancestor
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
                });
                depth += 1;
            }
        }
    }
}

/// For each non-abstract class with `implements`, verify that
/// every abstract method on the implemented interfaces has a
/// matching concrete implementation reachable through either the
/// class itself, the class's extends chain, or a default method
/// on one of the implemented interfaces. Missing implementations
/// fire **E0429** with the offending method names listed inline.
///
/// Abstract classes are skipped — they're allowed to leave
/// abstract methods unimplemented for a concrete subclass to
/// satisfy. The same is true for interfaces themselves; this
/// pass only looks at classes.
fn check_abstract_methods_implemented(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (class_name, class) in &table.classes {
        if class.is_abstract {
            continue;
        }
        if class.implements.is_empty() && class.extends.is_none() {
            continue;
        }
        let mut missing: Vec<(String, String)> = Vec::new();
        // Source 1: abstract methods on each implemented interface.
        for iface_ty in &class.implements {
            let Some(iface_name) = iface_ty.name.segments.last().map(|s| s.text.as_str()) else {
                continue;
            };
            let Some(iface) = resolve_interface(table, iface_name) else {
                continue;
            };
            for (m_name, m_sig) in &iface.methods {
                if !m_sig.is_abstract || m_sig.is_static {
                    continue;
                }
                // Reachable through the class's own extends chain?
                if class_provides_method(table, class_name, m_name) {
                    continue;
                }
                // Reachable through another implemented interface
                // as a default method?
                if implements_provides_default(table, &class.implements, m_name) {
                    continue;
                }
                missing.push((iface_name.to_string(), m_name.clone()));
            }
        }
        // Source 2: abstract methods inherited from an abstract
        // ancestor class. A concrete subclass must implement every
        // such method directly or via another concrete ancestor
        // further down the chain.
        let mut cursor: Option<&str> = class
            .extends_fqn
            .as_deref()
            .or_else(|| {
                class
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
            });
        let mut depth = 0usize;
        while let Some(ancestor_name) = cursor {
            if depth > 64 {
                break;
            }
            let Some(ancestor) = table.classes.get(ancestor_name) else {
                break;
            };
            for (m_name, m_sig) in &ancestor.methods {
                if !m_sig.is_abstract || m_sig.is_static {
                    continue;
                }
                if class_provides_method(table, class_name, m_name) {
                    continue;
                }
                missing.push((ancestor_name.to_string(), m_name.clone()));
            }
            cursor = ancestor
                .extends_fqn
                .as_deref()
                .or_else(|| {
                    ancestor
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
                });
            depth += 1;
        }
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            let list = missing
                .iter()
                .map(|(owner, m)| format!("`{owner}.{m}`"))
                .collect::<Vec<_>>()
                .join(", ");
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0429_AbstractNotImplemented,
                    format!(
                        "class `{class_name}` doesn't implement abstract method(s): {list}",
                    ),
                )
                .with_span(class.span),
            );
        }
    }
}

/// True if `class_name` (or one of its ancestor classes) has an
/// own (non-abstract) method named `method_name`.
/// Resolve a written interface name (often a bare segment like `Infer` after an
/// `import`) to its signature. Tries a direct key hit first, then falls back to
/// matching an FQN-keyed interface (`xss.it.follow.Infer`) by its last segment —
/// the same fallback [`SymbolTable::lookup_method`] uses. Without this, the
/// completeness checks silently skip cross-package interfaces and the error
/// leaks to rustc as `E0046`.
pub(crate) fn resolve_interface<'a>(table: &'a SymbolTable, written_name: &str) -> Option<&'a InterfaceSig> {
    if let Some(iface) = table.interfaces.get(written_name) {
        return Some(iface);
    }
    table.interfaces.iter().find_map(|(key, iface)| {
        let last = key.rsplit('.').next().unwrap_or(key.as_str());
        (last == written_name).then_some(iface)
    })
}

/// Why an interface can't (yet) back a **dynamically-dispatched value type**
/// (`Rc<dyn Trait>`) in stage-1 interface dispatch. Each variant names a
/// deliberately-deferred shape; the value-type use-site checker turns it into
/// an [`E0435`](code::Code::E0435_InterfaceNotDynDispatchable) diagnostic
/// instead of emitting `Rc<dyn Trait>` that rustc would reject with `E0038` /
/// `E0107`. None of these block the interface as a *declaration* — only its
/// use as a `dyn` value type; it can still be implemented and called through
/// concrete classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynDispatchBlock {
    /// `interface A<T>` — a `dyn` value slot needs the concrete argument
    /// (`dyn A<int>`); threading generic args through value positions is a
    /// later sub-stage. Carries the interface's declared arity for the message.
    GenericInterface(usize),
    /// The interface declares a non-`static` method with its own generic
    /// parameters (`<R> R map(...)`), which makes the trait not object-safe in
    /// Rust. Carries the offending method name.
    GenericMethod(String),
}

/// Classify whether the interface named `iface_name` can back a `Rc<dyn Trait>`
/// value type in stage-1 interface dispatch.
///
/// Returns:
/// - `None` when `iface_name` doesn't resolve to an interface at all (the
///   caller decides what a non-interface name means — typically "not our
///   concern, fall through").
/// - `Some(Ok(()))` when the interface is `dyn`-dispatch ready.
/// - `Some(Err(reason))` when its shape is deferred (see [`DynDispatchBlock`]).
///
/// Only the interface's own signature shape is inspected. `static` methods are
/// emitted as free functions (not trait items) so their generics never affect
/// object safety; `default` methods are object-safe and don't block dispatch.
pub fn interface_dyn_dispatch_support(
    table: &SymbolTable,
    iface_name: &str,
) -> Option<Result<(), DynDispatchBlock>> {
    let iface = resolve_interface(table, iface_name)?;
    if !iface.generic_params.is_empty() {
        return Some(Err(DynDispatchBlock::GenericInterface(
            iface.generic_params.len(),
        )));
    }
    // A non-static method with its own type parameters breaks object safety.
    // Sort the candidates so the reported method name is deterministic — the
    // `methods` map iterates in arbitrary order.
    let mut generic_methods: Vec<&String> = iface
        .methods
        .iter()
        .filter(|(_, m)| !m.is_static && !m.generic_params.is_empty())
        .map(|(name, _)| name)
        .collect();
    generic_methods.sort();
    if let Some(name) = generic_methods.first() {
        return Some(Err(DynDispatchBlock::GenericMethod((*name).clone())));
    }
    Some(Ok(()))
}

fn class_provides_method(
    table: &SymbolTable,
    class_name: &str,
    method_name: &str,
) -> bool {
    let mut cursor: Option<&str> = Some(class_name);
    let mut depth = 0usize;
    while let Some(name) = cursor {
        if depth > 64 {
            return false;
        }
        let Some(class) = table.classes.get(name) else {
            return false;
        };
        if let Some(m) = class.methods.get(method_name) {
            if !m.is_abstract {
                return true;
            }
        }
        cursor = class
            .extends_fqn
            .as_deref()
            .or_else(|| {
                class
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
            });
        depth += 1;
    }
    false
}

/// True if any interface in `implements` provides a non-abstract
/// (default) method named `method_name`. Used by the
/// abstract-implementation check to recognize that a sibling
/// interface's default covers the gap.
fn implements_provides_default(
    table: &SymbolTable,
    implements: &[TypeRef],
    method_name: &str,
) -> bool {
    for iface_ty in implements {
        let Some(iface_name) = iface_ty.name.segments.last().map(|s| s.text.as_str()) else {
            continue;
        };
        let Some(iface) = resolve_interface(table, iface_name) else {
            continue;
        };
        if let Some(m) = iface.methods.get(method_name) {
            if !m.is_abstract && !m.is_static {
                return true;
            }
        }
    }
    false
}

/// Bare names of every **polymorphic base class** — a non-sealed, non-final,
/// non-generic class extended by ≥1 other class. Mirrors the backend's
/// `compute_polymorphic_base_classes`: these are the classes whose value slots
/// lower to `Rc<dyn <Name>Kind>` for Stage-2 virtual dispatch. Keyed on bare
/// (last-segment) names to match the backend's wrapper/dispatch gates.
pub fn polymorphic_base_bare_names(table: &SymbolTable) -> std::collections::HashSet<String> {
    let mut candidate: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut extended: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (fqn, sig) in &table.classes {
        let bare = fqn.rsplit('.').next().unwrap_or(fqn).to_string();
        if !sig.is_sealed && sig.permits.is_empty() && !sig.is_final && sig.generic_params.is_empty()
        {
            candidate.insert(bare);
        }
        let parent_bare = sig
            .extends_fqn
            .as_deref()
            .map(|f| f.rsplit('.').next().unwrap_or(f).to_string())
            .or_else(|| {
                sig.extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
            });
        if let Some(p) = parent_bare {
            extended.insert(p);
        }
    }
    candidate.intersection(&extended).cloned().collect()
}

/// Stage-2 virtual dispatch (E0438): reject a **generic virtual method on a
/// polymorphic base class**. The base lowers to a `dyn <Name>Kind` trait
/// object so overrides dispatch dynamically; a method with its own generic
/// type parameters makes that trait not object-safe (rustc `E0038`). Mirrors
/// the interface object-safety rule E0435.
fn check_polymorphic_base_generic_methods(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let bases = polymorphic_base_bare_names(table);
    if bases.is_empty() {
        return;
    }
    for (fqn, sig) in &table.classes {
        let bare = fqn.rsplit('.').next().unwrap_or(fqn);
        if !bases.contains(bare) {
            continue;
        }
        // Deterministic order for a stable message (HashMap iteration isn't).
        let mut offenders: Vec<&String> = sig
            .methods
            .iter()
            .filter(|(_, m)| !m.is_static && !m.generic_params.is_empty())
            .filter(|(_, m)| {
                matches!(m.visibility, Visibility::Public | Visibility::Protected)
            })
            .map(|(name, _)| name)
            .collect();
        offenders.sort();
        if let Some(name) = offenders.first() {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0438_GenericVirtualMethod,
                    format!(
                        "class `{bare}` is a polymorphic base (it's extended), so its virtual \
                         method `{name}` would dispatch through a `dyn` trait object — but `{name}` \
                         has generic type parameters, which isn't object-safe; make it non-generic, \
                         mark it `final`, or seal the hierarchy",
                    ),
                )
                .with_span(sig.span),
            );
        }
    }
}

/// Stage-1 interface dispatch (E0436): reject a class that **extends the
/// exception hierarchy** and also implements an interface (directly or via a
/// superclass's `implements`).
///
/// Interface trait methods are emitted with a `&self` receiver so the
/// interface can back a `Rc<dyn Trait>` value type; that's only satisfiable
/// by the interior-mutable wrapper representation. Exception classes can't be
/// wrapped (a `panic_any` payload must be `Send`; `Rc<RefCell<…>>` is
/// `!Send`), so they stay on the legacy `&mut self` value path and the
/// `impl Trait for ExcClass` would fail to compile. Catching it here turns a
/// leaked rustc `E0308`/`E0596` into a clear, deferred-feature diagnostic.
fn check_interface_on_exception_class(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (class_name, class) in &table.classes {
        if class.is_external {
            continue;
        }
        if !class_or_ancestor_implements_any(table, class_name) {
            continue;
        }
        if !class_extends_exception_hierarchy(table, class_name) {
            continue;
        }
        diagnostics.push(
            Diagnostic::error(
                code::Code::E0436_InterfaceOnExceptionClass,
                format!(
                    "class `{class_name}` extends the exception hierarchy and implements an \
                     interface — interface dynamic dispatch isn't supported for exception \
                     classes yet (they can't use the interior-mutable representation that \
                     `Rc<dyn Trait>` requires)",
                ),
            )
            .with_span(class.span),
        );
    }
}

/// True iff `class_name` or any of its ancestor classes declares a non-empty
/// `implements` clause. Walks the `extends` chain (FQN-preferred, bare
/// fallback) with a depth guard.
fn class_or_ancestor_implements_any(table: &SymbolTable, class_name: &str) -> bool {
    let mut cursor: Option<&str> = Some(class_name);
    let mut depth = 0usize;
    while let Some(name) = cursor {
        if depth > 64 {
            return false;
        }
        let Some(class) = table.classes.get(name) else {
            return false;
        };
        if !class.implements.is_empty() {
            return true;
        }
        cursor = class.extends_fqn.as_deref().or_else(|| {
            class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
        });
        depth += 1;
    }
    false
}

/// True iff `class_name`'s `extends` chain reaches `Exception` or
/// `Throwable` (matched on the bare last segment so a no-package fallback
/// chain still resolves). Mirrors the backend's exception taint used to keep
/// thrown classes off the `Rc<RefCell>` wrapper path.
fn class_extends_exception_hierarchy(table: &SymbolTable, class_name: &str) -> bool {
    let mut cursor: Option<String> = Some(class_name.to_string());
    let mut depth = 0usize;
    while let Some(name) = cursor {
        if depth > 64 {
            return false;
        }
        let bare = name.rsplit('.').next().unwrap_or(&name);
        if bare == "Exception" || bare == "Throwable" {
            return true;
        }
        let Some(class) = table.classes.get(&name) else {
            // Unknown class key — try the bare name once before giving up
            // (handles the `extends Exception` leaf where `Exception` has
            // no user ClassSig).
            return false;
        };
        cursor = class.extends_fqn.clone().or_else(|| {
            class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
        });
        depth += 1;
    }
    false
}

/// Diamond-default detection: for every class with multiple
/// `implements`, find methods that two or more implemented
/// interfaces each provide as a **default** method, and the class
/// itself does not override. Fires **E0430** so users see a clear
/// resolution prompt instead of rustc's "multiple applicable
/// items" message.
fn check_diamond_default_conflicts(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (class_name, class) in &table.classes {
        if class.implements.len() < 2 {
            continue;
        }
        // Collect default methods per interface, then look for
        // names that appear in more than one interface and that
        // the class doesn't override locally. Walking once per
        // class keeps the cost linear in the number of (iface,
        // method) pairs.
        use std::collections::BTreeMap;
        let mut sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for iface_ty in &class.implements {
            let Some(iface_name) = iface_ty.name.segments.last().map(|s| s.text.as_str()) else {
                continue;
            };
            let Some(iface) = resolve_interface(table, iface_name) else {
                continue;
            };
            for (m_name, m_sig) in &iface.methods {
                if m_sig.is_abstract || m_sig.is_static {
                    continue;
                }
                sources
                    .entry(m_name.clone())
                    .or_default()
                    .push(iface_name.to_string());
            }
        }
        for (m_name, ifaces) in &sources {
            if ifaces.len() < 2 {
                continue;
            }
            if class.methods.contains_key(m_name) {
                continue;
            }
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0430_AmbiguousDefaultMethod,
                    format!(
                        "class `{class_name}` inherits conflicting default implementations of `{m_name}` from interfaces {}; override `{m_name}` on `{class_name}` to disambiguate",
                        ifaces.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(", "),
                    ),
                )
                .with_span(class.span),
            );
        }
    }
}

/// Cross-class pass enforcing the modifier-combination rules
/// listed in `classes-rules.md` §1.4:
/// - `abstract` is only legal inside an `abstract` class
///   (R-M1).
/// - `abstract` cannot coexist with `static`, `final`, or
///   `private` on the same method (R-M2..M4).
///
/// Interface methods are exempted from R-M1 because they're
/// implicitly abstract by construction. The interface
/// abstract+static collision (`static` on an unbodied
/// signature) is already enforced by the parser (E0200).
/// Enforce the Turn-1 "at most one constructor" limit (constructor
/// overloading lands in a later turn). This moved out of the parser so that
/// `.jux.d` declaration stubs — which legitimately declare overloaded `new`
/// constructors (`HashMap()` + `HashMap(int)`, JUX-BINDGEN §G.5.1/§G.5.2) —
/// parse and resolve cleanly: the stub class is flagged `is_external` and
/// exempted here, while ordinary user classes still get the limit enforced.
fn check_single_constructor(table: &SymbolTable, diagnostics: &mut Vec<Diagnostic>) {
    for (class_name, class) in &table.classes {
        if class.is_external {
            continue;
        }
        // The first constructor is allowed; every subsequent one fires.
        for extra in class.constructors.iter().skip(1) {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    format!(
                        "class `{class_name}` declares more than one constructor — \
                         Turn-1 classes support only one constructor (overloading lands later)",
                    ),
                )
                .with_span(extra.span),
            );
        }
    }
}

fn check_method_modifier_combinations(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (class_name, class) in &table.classes {
        // External `.jux.d` stub classes (JUX-BINDGEN §G.9) hold bodyless
        // *signatures*, not abstract members to implement — the real foreign
        // type provides the bodies. So they're exempt from the abstract-method
        // modifier rules (which would otherwise reject every stub method).
        if class.is_external {
            continue;
        }
        for (method_name, method) in &class.methods {
            if method.is_abstract && !class.is_abstract {
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0431_InvalidMethodModifiers,
                        format!(
                            "method `{class_name}.{method_name}` is declared `abstract` but `{class_name}` is not — `abstract` methods are only allowed in `abstract` classes",
                        ),
                    )
                    .with_span(method.span),
                );
            }
            if method.is_abstract && method.is_static {
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0431_InvalidMethodModifiers,
                        format!(
                            "method `{class_name}.{method_name}` declares both `abstract` and `static` — these modifiers cannot coexist",
                        ),
                    )
                    .with_span(method.span),
                );
            }
            if method.is_abstract && method.is_final {
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0431_InvalidMethodModifiers,
                        format!(
                            "method `{class_name}.{method_name}` declares both `abstract` and `final` — an abstract method must be overridable",
                        ),
                    )
                    .with_span(method.span),
                );
            }
            if method.is_abstract
                && matches!(method.visibility, juxc_ast::Visibility::Private)
            {
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0431_InvalidMethodModifiers,
                        format!(
                            "method `{class_name}.{method_name}` is `private` and `abstract` — a private method can't be overridden, so it can't be abstract",
                        ),
                    )
                    .with_span(method.span),
                );
            }
        }
    }
}

/// Top-level types may only be `public` or package-private
/// (no modifier). Nested types — when Jux gets them — can use
/// the narrower modifiers, but at the unit's top level
/// `private` and `protected` are nonsense: nothing can name the
/// type from outside the file where it was declared. Fires
/// **E0432**.
fn check_top_level_visibility(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use juxc_ast::Visibility;
    for (name, class) in &table.classes {
        match class.visibility {
            Visibility::Private | Visibility::Protected => {
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0432_InvalidTopLevelVisibility,
                        format!(
                            "top-level class `{name}` cannot be `{}` — use `public` or omit the modifier",
                            visibility_label(class.visibility),
                        ),
                    )
                    .with_span(class.span),
                );
            }
            _ => {}
        }
    }
    for (name, iface) in &table.interfaces {
        match iface.visibility {
            Visibility::Private | Visibility::Protected => {
                diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0432_InvalidTopLevelVisibility,
                        format!(
                            "top-level interface `{name}` cannot be `{}` — use `public` or omit the modifier",
                            visibility_label(iface.visibility),
                        ),
                    )
                    .with_span(iface.span),
                );
            }
            _ => {}
        }
    }
}

/// Helper: human-readable visibility name for diagnostics.
fn visibility_label(v: juxc_ast::Visibility) -> &'static str {
    use juxc_ast::Visibility;
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
        Visibility::Package => "package-private",
        Visibility::Internal => "internal",
    }
}

/// Liskov rule (`classes-rules.md` §1.4): an overriding method
/// must be **at least as visible** as the method it overrides.
/// Narrowing `public greet()` to `private greet()` in a subclass
/// breaks substitutability — code holding the parent reference
/// can call the method, but code holding the subclass reference
/// cannot. We walk the extends chain for every concrete method
/// and compare against the ancestor's visibility; widening is
/// fine. Fires **E0433**.
fn check_override_does_not_narrow_access(
    table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (child_name, child) in &table.classes {
        for (method_name, child_method) in &child.methods {
            let mut cursor: Option<&str> = child
                .extends_fqn
                .as_deref()
                .or_else(|| {
                    child
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
                });
            let mut depth = 0usize;
            while let Some(ancestor_name) = cursor {
                if depth > 64 {
                    break;
                }
                let Some(ancestor) = table.classes.get(ancestor_name) else {
                    break;
                };
                if let Some(ancestor_method) = ancestor.methods.get(method_name) {
                    if visibility_rank(child_method.visibility)
                        < visibility_rank(ancestor_method.visibility)
                    {
                        diagnostics.push(
                            Diagnostic::error(
                                code::Code::E0433_OverrideNarrowsAccess,
                                format!(
                                    "override `{child_name}.{method_name}` is `{}` but the inherited method `{ancestor_name}.{method_name}` is `{}` — an override cannot narrow visibility",
                                    visibility_label(child_method.visibility),
                                    visibility_label(ancestor_method.visibility),
                                ),
                            )
                            .with_span(child_method.span),
                        );
                        break;
                    }
                }
                cursor = ancestor
                    .extends_fqn
                    .as_deref()
                    .or_else(|| {
                        ancestor
                            .extends
                            .as_ref()
                            .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
                    });
                depth += 1;
            }
        }
    }
}

/// Visibility ordering for the narrowing check. Higher rank =
/// more visible. Package-private and protected aren't strictly
/// ordered in Java (each lets through some access the other
/// doesn't), but for the override-narrowing rule both sit
/// strictly above `private` and below `public`, which is what
/// the rule's "at least as visible" comparison actually cares
/// about.
fn visibility_rank(v: juxc_ast::Visibility) -> u8 {
    use juxc_ast::Visibility;
    match v {
        Visibility::Private => 0,
        Visibility::Package => 1,
        // `internal` is a module-scoped variant sitting alongside
        // package-private semantically — same rank for the
        // narrowing comparison.
        Visibility::Internal => 1,
        Visibility::Protected => 2,
        Visibility::Public => 3,
    }
}

/// Reject the second declaration of a top-level name with a single
/// E0400 pointing at it. Returns `true` when the name is fresh (caller
/// proceeds to insert), `false` when it's a duplicate (caller drops).
fn ensure_top_level_unique(
    table: &SymbolTable,
    name: &str,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if table.is_type_name(name)
        || table.functions.contains_key(name)
        || table.consts.contains_key(name)
    {
        diagnostics.push(
            Diagnostic::error(
                code::Code::E0400_DuplicateDeclaration,
                format!("`{name}` is declared more than once at the top level"),
            )
            .with_span(span),
        );
        false
    } else {
        true
    }
}

fn insert_class(
    table: &mut SymbolTable,
    class_decl: &ClassDecl,
    package: &[String],
    is_external: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fqn = make_fqn(package, &class_decl.name.text);
    if !ensure_top_level_unique(table, &fqn, class_decl.span, diagnostics) {
        return;
    }

    // Fields — duplicate names within the same class emit E0401.
    let mut fields = HashMap::new();
    for field in &class_decl.fields {
        if let Some(existing) = fields.get(&field.name.text) {
            let _: &FieldSig = existing; // silence unused-binding warning
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0401_DuplicateField,
                    format!(
                        "field `{}` is declared more than once in class `{}`",
                        field.name.text, class_decl.name.text,
                    ),
                )
                .with_span(field.span),
            );
            continue;
        }
        fields.insert(field.name.text.clone(), field_sig(field));
    }

    // Constructors — multiple allowed at AST level (parser caps at one
    // today, but we record what's there for the future overload pass).
    let constructors = class_decl
        .constructors
        .iter()
        .map(|c| ConstructorSig {
            visibility: c.visibility,
            params: c.params.iter().map(param_sig).collect(),
            span: c.span,
        })
        .collect();

    // Methods — duplicates emit E0402.
    let mut methods = HashMap::new();
    for method in &class_decl.methods {
        if methods.contains_key(&method.name.text) {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0402_DuplicateMethod,
                    format!(
                        "method `{}` is declared more than once in class `{}`",
                        method.name.text, class_decl.name.text,
                    ),
                )
                .with_span(method.span),
            );
            continue;
        }
        methods.insert(method.name.text.clone(), method_sig(method, is_external));
    }

    // Operators — same E0402 treatment for duplicates, keyed by kind.
    let mut operators: HashMap<OperatorKind, OperatorSig> = HashMap::new();
    for op in &class_decl.operators {
        if operators.contains_key(&op.kind) {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0402_DuplicateMethod,
                    format!(
                        "operator `{}` is declared more than once in class `{}`",
                        operator_kind_display(op.kind),
                        class_decl.name.text,
                    ),
                )
                .with_span(op.span),
            );
            continue;
        }
        operators.insert(op.kind, operator_sig(op));
    }
    // §O.2.7 pairing: `operator==` requires `operator hash`.
    let class_ops: Vec<&juxc_ast::OperatorDecl> = class_decl.operators.iter().collect();
    check_eq_hash_pairing(&class_ops, "class", &class_decl.name.text, diagnostics);
    // §O.2.1: `<=>` conflicts with individual `<`/`<=`/`>`/`>=`.
    check_cmp_individual_conflict(&class_ops, "class", &class_decl.name.text, diagnostics);
    // §O.2.1/§O.2.2: fixed return types for ==, <=>, hash, string, etc.
    check_operator_return_types(&class_ops, "class", &class_decl.name.text, diagnostics);
    // §O.2.1: individual ordering operators (<, <=, >, >=) must be
    // declared as a complete set or not at all.
    check_individual_ordering_completeness(
        &class_ops,
        "class",
        &class_decl.name.text,
        diagnostics,
    );

    // Property write-access metadata (§M.7.2). The getter / setter
    // already landed in `methods` via desugaring; here we record only
    // the access shape (read-only / init-only / setter visibility).
    let mut properties: HashMap<String, PropertySig> = HashMap::new();
    for prop in &class_decl.properties {
        let setter_visibility = prop.setter.as_ref().map(|s| {
            s.visibility.unwrap_or(prop.visibility)
        });
        properties.insert(
            prop.name.text.clone(),
            PropertySig {
                visibility: prop.visibility,
                is_static: prop.is_static,
                is_read_only: prop.setter.is_none(),
                is_init_only: prop.setter.as_ref().map_or(false, |s| s.is_init),
                setter_visibility,
                ty: prop.ty.clone(),
                span: prop.span,
            },
        );
    }

    table.classes.insert(
        fqn,
        ClassSig {
            visibility: class_decl.visibility,
            package: package.to_vec(),
            is_abstract: class_decl.is_abstract,
            is_external,
            rust_path: rust_path_annotation(&class_decl.annotations),
            has_static_init: !class_decl.static_init_blocks.is_empty(),
            is_final: class_decl.is_final,
            is_sealed: class_decl.is_sealed,
            permits: class_decl
                .permits
                .iter()
                .map(|n| n.text.clone())
                .collect(),
            generic_params: class_decl.generic_params.clone(),
            extends: class_decl.extends.clone(),
            extends_fqn: None, // resolved in a finalize pass once
                                // every class is registered and
                                // the unit-context maps are built.
            implements: class_decl.implements.clone(),
            fields,
            constructors,
            methods,
            operators,
            properties,
            span: class_decl.span,
        },
    );
}

fn insert_record(
    table: &mut SymbolTable,
    record_decl: &RecordDecl,
    package: &[String],
    is_external: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fqn = make_fqn(package, &record_decl.name.text);
    if !ensure_top_level_unique(table, &fqn, record_decl.span, diagnostics) {
        return;
    }
    // Operators on records — same E0402-on-duplicate treatment as on
    // classes. `= delete;` declarations land here too; the
    // `is_deleted` flag carries through from the AST.
    let mut operators: HashMap<OperatorKind, OperatorSig> = HashMap::new();
    for op in &record_decl.operators {
        if operators.contains_key(&op.kind) {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0402_DuplicateMethod,
                    format!(
                        "operator `{}` is declared more than once in record `{}`",
                        operator_kind_display(op.kind),
                        record_decl.name.text,
                    ),
                )
                .with_span(op.span),
            );
            continue;
        }
        operators.insert(op.kind, operator_sig(op));
    }
    // §O.2.7 pairing on records too — same rule, same message shape.
    let record_ops: Vec<&juxc_ast::OperatorDecl> = record_decl.operators.iter().collect();
    check_eq_hash_pairing(&record_ops, "record", &record_decl.name.text, diagnostics);
    check_cmp_individual_conflict(&record_ops, "record", &record_decl.name.text, diagnostics);
    check_operator_return_types(&record_ops, "record", &record_decl.name.text, diagnostics);
    check_individual_ordering_completeness(
        &record_ops,
        "record",
        &record_decl.name.text,
        diagnostics,
    );
    // Method dedup (same E0402 treatment as classes).
    let mut methods: HashMap<String, MethodSig> = HashMap::new();
    for method in &record_decl.methods {
        if methods.contains_key(&method.name.text) {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0402_DuplicateMethod,
                    format!(
                        "method `{}` is declared more than once in record `{}`",
                        method.name.text, record_decl.name.text,
                    ),
                )
                .with_span(method.span),
            );
            continue;
        }
        methods.insert(method.name.text.clone(), method_sig(method, is_external));
    }
    table.records.insert(
        fqn,
        RecordSig {
            visibility: record_decl.visibility,
            generic_params: record_decl.generic_params.clone(),
            implements: Vec::new(), // records don't carry implements yet; parser drops the clause
            components: record_decl
                .components
                .iter()
                .map(|c| RecordComponentSig {
                    name: c.name.text.clone(),
                    ty: c.ty.clone(),
                })
                .collect(),
            operators,
            methods,
            span: record_decl.span,
        },
    );
}

fn insert_enum(
    table: &mut SymbolTable,
    enum_decl: &EnumDecl,
    package: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fqn = make_fqn(package, &enum_decl.name.text);
    if !ensure_top_level_unique(table, &fqn, enum_decl.span, diagnostics) {
        return;
    }
    let mut variants = HashMap::new();
    for variant in &enum_decl.variants {
        if variants.contains_key(&variant.name.text) {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0403_DuplicateVariant,
                    format!(
                        "variant `{}` is declared more than once in enum `{}`",
                        variant.name.text, enum_decl.name.text,
                    ),
                )
                .with_span(variant.span),
            );
            continue;
        }
        variants.insert(
            variant.name.text.clone(),
            VariantSig {
                payload: variant.payload.iter().map(|p| p.ty.clone()).collect(),
                span: variant.span,
            },
        );
    }
    let mut operators: HashMap<OperatorKind, OperatorSig> = HashMap::new();
    for op in &enum_decl.operators {
        if operators.contains_key(&op.kind) {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0402_DuplicateMethod,
                    format!(
                        "operator `{}` is declared more than once in enum `{}`",
                        operator_kind_display(op.kind),
                        enum_decl.name.text,
                    ),
                )
                .with_span(op.span),
            );
            continue;
        }
        operators.insert(op.kind, operator_sig(op));
    }
    // §O.2.7 pairing on enums too.
    let enum_ops: Vec<&juxc_ast::OperatorDecl> = enum_decl.operators.iter().collect();
    check_eq_hash_pairing(&enum_ops, "enum", &enum_decl.name.text, diagnostics);
    check_cmp_individual_conflict(&enum_ops, "enum", &enum_decl.name.text, diagnostics);
    check_operator_return_types(&enum_ops, "enum", &enum_decl.name.text, diagnostics);
    check_individual_ordering_completeness(
        &enum_ops,
        "enum",
        &enum_decl.name.text,
        diagnostics,
    );
    table.enums.insert(
        fqn,
        EnumSig {
            visibility: enum_decl.visibility,
            variants,
            operators,
            methods: enum_decl
                .methods
                .iter()
                .map(|m| (m.name.text.clone(), method_sig(m, false)))
                .collect(),
            span: enum_decl.span,
        },
    );
}

fn insert_interface(
    table: &mut SymbolTable,
    interface_decl: &InterfaceDecl,
    package: &[String],
    is_external: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fqn = make_fqn(package, &interface_decl.name.text);
    if !ensure_top_level_unique(
        table,
        &fqn,
        interface_decl.span,
        diagnostics,
    ) {
        return;
    }
    let mut methods = HashMap::new();
    for method in &interface_decl.methods {
        if methods.contains_key(&method.name.text) {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0402_DuplicateMethod,
                    format!(
                        "method `{}` is declared more than once in interface `{}`",
                        method.name.text, interface_decl.name.text,
                    ),
                )
                .with_span(method.span),
            );
            continue;
        }
        methods.insert(method.name.text.clone(), method_sig(method, is_external));
    }
    let mut fields = HashMap::new();
    for field in &interface_decl.fields {
        if fields.contains_key(&field.name.text) {
            diagnostics.push(
                Diagnostic::error(
                    code::Code::E0401_DuplicateField,
                    format!(
                        "field `{}` is declared more than once in interface `{}`",
                        field.name.text, interface_decl.name.text,
                    ),
                )
                .with_span(field.span),
            );
            continue;
        }
        fields.insert(field.name.text.clone(), field_sig(field));
    }
    table.interfaces.insert(
        fqn,
        InterfaceSig {
            visibility: interface_decl.visibility,
            generic_params: interface_decl.generic_params.clone(),
            extends: interface_decl.extends.clone(),
            methods,
            fields,
            span: interface_decl.span,
        },
    );
}

fn insert_function(
    table: &mut SymbolTable,
    fn_decl: &FnDecl,
    package: &[String],
    is_external: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // `main()` always lives at the crate root for entry-point
    // discovery, regardless of the unit's package — there can be
    // only one. Other free functions are FQN'd like everything else.
    let fqn = if fn_decl.name.text == "main" {
        "main".to_string()
    } else {
        make_fqn(package, &fn_decl.name.text)
    };
    if !ensure_top_level_unique(table, &fqn, fn_decl.span, diagnostics) {
        return;
    }
    table.functions.insert(
        fqn,
        FunctionSig {
            visibility: fn_decl.visibility,
            generic_params: fn_decl.generic_params.clone(),
            params: fn_decl.params.iter().map(param_sig).collect(),
            return_type: fn_decl.return_type.clone(),
            is_unsafe: fn_decl
                .modifiers
                .iter()
                .any(|m| matches!(m, juxc_ast::FnModifier::Unsafe)),
            // A foreign (`.jux.d`) function with a `throws` clause came from a
            // Rust `fn -> Result<T, E>` (§G.5.4). Its call site must unwrap the
            // `Result` (and re-throw the error) since Jux sees only `T`.
            is_foreign_result: is_external && !fn_decl.throws.is_empty(),
            // `@rust("real::path")` on a foreign free function records its true
            // Rust path (snake_case name) so the backend can alias it on import.
            rust_path: if is_external {
                rust_path_annotation(&fn_decl.annotations)
            } else {
                None
            },
            span: fn_decl.span,
        },
    );
}

// ============================================================================
// Helpers
// ============================================================================

fn field_sig(field: &FieldDecl) -> FieldSig {
    FieldSig {
        visibility: field.visibility,
        is_static: field.is_static,
        is_final: field.is_final,
        // Resolved type: the written type, or one inferred from the
        // initializer when the field omits it (`const I = 2;` → `int`).
        ty: resolve_decl_type(field.ty.as_ref(), field.default.as_ref(), field.span),
        default: field.default.clone(),
        span: field.span,
    }
}

/// Resolve a field/const's type: use the written type if present, otherwise
/// infer it from the (literal) initializer. Falls back to `int` only when an
/// inferred declaration has no usable initializer — that case is reported as a
/// type error elsewhere; the placeholder just keeps the table well-formed.
pub(crate) fn resolve_decl_type(
    declared: Option<&TypeRef>,
    init: Option<&juxc_ast::Expr>,
    span: Span,
) -> TypeRef {
    if let Some(t) = declared {
        return t.clone();
    }
    infer_decl_type(init, span).unwrap_or_else(|| synth_type_ref("int", span))
}

/// Infer a [`TypeRef`] from a literal initializer (`2` → `int`, `"x"` →
/// `String`, …). Returns `None` for a missing or non-literal initializer.
fn infer_decl_type(init: Option<&juxc_ast::Expr>, span: Span) -> Option<TypeRef> {
    match init {
        Some(juxc_ast::Expr::Literal(lit)) => {
            ty_to_type_ref(&crate::infer::infer_literal(lit), span)
        }
        _ => None,
    }
}

/// Map a simple inferred [`crate::ty::Ty`] (primitive or `String`) to a
/// synthetic single-segment [`TypeRef`].
fn ty_to_type_ref(ty: &crate::ty::Ty, span: Span) -> Option<TypeRef> {
    use crate::ty::Ty;
    let name = match ty {
        Ty::Primitive(p) => crate::ty::primitive_name(*p),
        Ty::String => "String",
        _ => return None,
    };
    Some(synth_type_ref(name, span))
}

/// Build a single-segment named [`TypeRef`] (no generics, not nullable, not an
/// array) — used to materialize an inferred primitive/`String` type.
fn synth_type_ref(name: &str, span: Span) -> TypeRef {
    TypeRef {
        name: juxc_ast::QualifiedName {
            segments: vec![juxc_ast::Ident { text: name.to_string(), span }],
            span,
        },
        generic_args: Vec::new(),
        nullable: false,
        array_shape: None,
        fn_shape: None,
        ptr_depth: 0,
        span,
    }
}

fn method_sig(method: &FnDecl, is_external: bool) -> MethodSig {
    MethodSig {
        visibility: method.visibility,
        annotations: method.annotations.clone(),
        is_abstract: method.body.is_none(),
        is_property: method.is_property,
        is_final: method
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Final)),
        is_static: method
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Static)),
        is_unsafe: method
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Unsafe)),
        is_foreign_result: is_external && !method.throws.is_empty(),
        generic_params: method.generic_params.clone(),
        params: method.params.iter().map(param_sig).collect(),
        return_type: method.return_type.clone(),
        span: method.span,
    }
}

/// Lower an [`OperatorDecl`] into the symbol-table's [`OperatorSig`]
/// shape. Mirrors [`method_sig`].
fn operator_sig(op: &OperatorDecl) -> OperatorSig {
    OperatorSig {
        visibility: op.visibility,
        kind: op.kind,
        params: op.params.iter().map(param_sig).collect(),
        return_type: op.return_type.clone(),
        is_deleted: op.is_deleted,
        span: op.span,
    }
}

/// Human-readable spelling of an [`OperatorKind`] suitable for embedding
/// in a diagnostic message. Matches the source-level spelling the user
/// would have written (`==`, `<=>`, `hash`, `string`, etc.).
fn operator_kind_display(kind: OperatorKind) -> &'static str {
    match kind {
        OperatorKind::Eq => "==",
        OperatorKind::In => "in",
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

fn param_sig(p: &juxc_ast::Param) -> ParamSig {
    ParamSig {
        name: p.name.text.clone(),
        ty: p.ty.clone(),
        is_ref: p.is_ref,
    }
}

/// Enforce the spec's fixed return types for operators that have them
/// (`JUX-OPERATORS-ADDENDUM.md` §O.2.1 + §O.2.2). Operators whose
/// signature shape is user-defined (arithmetic, bitwise, shift,
/// indexing, call, range) get no signature check at this stage —
/// rustc catches mismatches downstream.
///
/// | Operator                                | Required return |
/// |-----------------------------------------|------------------|
/// | `==`, `<`, `<=`, `>`, `>=`              | `bool`           |
/// | `<=>`, `hash`                           | `int`            |
/// | `string`                                | `String`         |
///
/// Deleted operators (`= delete;`) skip the check — they declare a
/// signature only to opt out of the auto-derive, not to provide an
/// override.
///
/// Emits `E0410_TypeMismatch` anchored at the operator's declaration
/// span when the declared return type doesn't match the required one.
fn check_operator_return_types(
    operators: &[&juxc_ast::OperatorDecl],
    kind_label: &str,
    decl_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for op in operators {
        if op.is_deleted {
            continue;
        }
        let Some(expected) = required_return_type_for_operator(op.kind) else {
            // Operator has no fixed return type per spec — skip.
            continue;
        };
        if return_type_matches_primitive(&op.return_type, expected) {
            continue;
        }
        let actual_str = render_return_type(&op.return_type);
        diagnostics.push(
            Diagnostic::error(
                code::Code::E0410_TypeMismatch,
                format!(
                    "operator `{}` on {kind_label} `{decl_name}` must return `{expected}`, \
                     found `{actual_str}`",
                    operator_kind_display(op.kind),
                ),
            )
            .with_span(op.span)
            .with_help(&format!(
                "the spec fixes this operator's return type; change the signature to \
                 `{expected} operator {}(...)`",
                operator_kind_display(op.kind),
            )),
        );
    }
}

/// Per spec §O.2.1 / §O.2.2, the fixed return-type name for each
/// operator kind whose return shape is constrained. Returns `None`
/// for operators whose return type is user-defined (arithmetic
/// family, indexing, call, range).
fn required_return_type_for_operator(kind: OperatorKind) -> Option<&'static str> {
    match kind {
        OperatorKind::Eq
        | OperatorKind::In
        | OperatorKind::Lt
        | OperatorKind::Le
        | OperatorKind::Gt
        | OperatorKind::Ge => Some("bool"),
        OperatorKind::Cmp | OperatorKind::Hash => Some("int"),
        OperatorKind::ToString => Some("String"),
        // Free-form return types — no signature check.
        OperatorKind::Plus
        | OperatorKind::Minus
        | OperatorKind::Mul
        | OperatorKind::Div
        | OperatorKind::Rem
        | OperatorKind::BitAnd
        | OperatorKind::BitOr
        | OperatorKind::BitXor
        | OperatorKind::BitNot
        | OperatorKind::Shl
        | OperatorKind::Shr
        | OperatorKind::Index
        | OperatorKind::IndexSet
        | OperatorKind::Call
        | OperatorKind::Range
        | OperatorKind::RangeInclusive => None,
    }
}

/// True iff `rt` is `ReturnType::Type(t)` where `t` is exactly the
/// single-segment primitive / String type named `expected`. No
/// nullable, no array, no generic args — those would all be a
/// mismatch.
fn return_type_matches_primitive(rt: &ReturnType, expected: &str) -> bool {
    let ReturnType::Type(t) = rt else { return false };
    t.array_shape.is_none()
        && !t.nullable
        && t.generic_args.is_empty()
        && t.name.segments.len() == 1
        && t.name.segments[0].text == expected
}

/// Render a `ReturnType` for inclusion in a diagnostic message.
/// Picks a flavored string for the user-visible spelling (e.g.
/// `"void"` / `"bool"` / `"async T"`). Wrappers (arrays, nullables,
/// generic args) get a best-effort approximation that's still
/// human-readable.
fn render_return_type(rt: &ReturnType) -> String {
    match rt {
        ReturnType::Void => "void".to_string(),
        ReturnType::AsyncType(t) => format!("async {}", render_type_ref(t)),
        ReturnType::Type(t) => render_type_ref(t),
    }
}

/// Render a single generic-arg slot for diagnostic display —
/// concrete types delegate to [`render_type_ref`]; wildcards
/// render as `?`, `? extends T`, or `? super T`.
fn render_generic_arg(arg: &juxc_ast::GenericArg) -> String {
    match arg {
        juxc_ast::GenericArg::Type(t) => render_type_ref(t),
        juxc_ast::GenericArg::Wildcard(w) => match &w.bound {
            None => "?".to_string(),
            Some(juxc_ast::WildcardBound::Extends(t)) => {
                format!("? extends {}", render_type_ref(t))
            }
            Some(juxc_ast::WildcardBound::Super(t)) => {
                format!("? super {}", render_type_ref(t))
            }
        },
    }
}

fn render_type_ref(t: &TypeRef) -> String {
    let mut out: String = t
        .name
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    if !t.generic_args.is_empty() {
        out.push('<');
        let parts: Vec<String> = t.generic_args.iter().map(render_generic_arg).collect();
        out.push_str(&parts.join(", "));
        out.push('>');
    }
    if t.nullable {
        out.push('?');
    }
    if t.array_shape.is_some() {
        out.push_str("[]");
    }
    out
}

/// Enforce the completeness rule for individual ordering operators
/// per `JUX-OPERATORS-ADDENDUM.md` §O.2.1: "individual `operator<`
/// etc. — must define all four; no partial sets". A type that
/// declares ANY of `<`, `<=`, `>`, `>=` must declare ALL four.
///
/// Deletions count as opt-out, not as a declaration — a type with
/// `operator< = delete;` and nothing else has zero defined individuals
/// and stays silent. A type with three defined and one deleted has
/// three defined → fires for the missing one (the deletion doesn't
/// satisfy completeness).
///
/// Emits one `E0930_OperatorConflict` per missing operator, anchored
/// at the FIRST defined individual operator's span (since the missing
/// ones have no span to anchor at).
fn check_individual_ordering_completeness(
    operators: &[&juxc_ast::OperatorDecl],
    kind_label: &str,
    decl_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    const ORDERING: [OperatorKind; 4] = [
        OperatorKind::Lt,
        OperatorKind::Le,
        OperatorKind::Gt,
        OperatorKind::Ge,
    ];
    // Find each defined-individual; the first one's span anchors the
    // diagnostic when others are missing.
    let defined: Vec<&juxc_ast::OperatorDecl> = operators
        .iter()
        .copied()
        .filter(|o| ORDERING.contains(&o.kind) && !o.is_deleted)
        .collect();
    if defined.is_empty() {
        return;
    }
    let anchor_span = defined[0].span;
    let missing: Vec<&'static str> = ORDERING
        .iter()
        .filter(|kind| {
            !operators
                .iter()
                .any(|o| o.kind == **kind && !o.is_deleted)
        })
        .map(|kind| operator_kind_display(*kind))
        .collect();
    if missing.is_empty() {
        return;
    }
    let missing_list = missing
        .iter()
        .map(|s| format!("`operator{s}`"))
        .collect::<Vec<_>>()
        .join(", ");
    diagnostics.push(
        Diagnostic::error(
            code::Code::E0930_OperatorConflict,
            format!(
                "{kind_label} `{decl_name}` declares some individual ordering operators \
                 but not all four — missing {missing_list}",
            ),
        )
        .with_span(anchor_span)
        .with_help(
            "either define all four of `operator<`, `operator<=`, `operator>`, `operator>=`, \
             or replace them with a single `operator<=>` that auto-derives the four",
        ),
    );
}

/// Enforce the `<=>` / individual-ordering conflict rule per
/// `JUX-OPERATORS-ADDENDUM.md` §O.2.1: defining BOTH `operator<=>` AND
/// any of the individual `<`/`<=`/`>`/`>=` operators on the same type
/// is a conflict — pick one form, not both. Emits `E0930` anchored at
/// each redundant individual-ordering decl when `<=>` is also present.
///
/// "Defines" excludes `= delete;` (deletion isn't definition). So a
/// type that deletes `<=>` and defines `<`/etc. is fine, and vice
/// versa.
fn check_cmp_individual_conflict(
    operators: &[&juxc_ast::OperatorDecl],
    kind_label: &str,
    decl_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let active_cmp = operators
        .iter()
        .any(|o| o.kind == OperatorKind::Cmp && !o.is_deleted);
    if !active_cmp {
        return;
    }
    for op in operators {
        let is_individual = matches!(
            op.kind,
            OperatorKind::Lt | OperatorKind::Le | OperatorKind::Gt | OperatorKind::Ge,
        );
        if !is_individual || op.is_deleted {
            continue;
        }
        diagnostics.push(
            Diagnostic::error(
                code::Code::E0930_OperatorConflict,
                format!(
                    "{kind_label} `{decl_name}` defines both `operator<=>` and \
                     `operator{}` — pick one form, not both",
                    operator_kind_display(op.kind),
                ),
            )
            .with_span(op.span)
            .with_help(
                "delete this individual operator or remove the `<=>` declaration; \
                 `<=>` auto-derives `<`, `<=`, `>`, `>=` from sign",
            ),
        );
    }
}

/// Enforce the `==` / `hash` pairing rule per `JUX-OPERATORS-ADDENDUM.md`
/// §O.2.7: if a class/record/enum defines `operator==`, it must also
/// define `operator hash`. Emits `E0931_EqWithoutHash` anchored at the
/// `operator==` declaration when the rule is violated.
///
/// "Defines" here means present AND not `= delete;` — a deletion is the
/// user opting out, not a definition. So a class with `operator==(...)
/// { ... }` and `operator hash() = delete;` is still a rule violation
/// (the user signaled structural equality but turned off hashing).
///
/// `kind_label` shapes the diagnostic message — `"class"` / `"record"`
/// / `"enum"`. `decl_name` is the declaring type's name.
fn check_eq_hash_pairing(
    operators: &[&juxc_ast::OperatorDecl],
    kind_label: &str,
    decl_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let active_eq = operators
        .iter()
        .find(|o| o.kind == OperatorKind::Eq && !o.is_deleted);
    let Some(eq) = active_eq else { return };
    let active_hash = operators
        .iter()
        .any(|o| o.kind == OperatorKind::Hash && !o.is_deleted);
    if active_hash {
        return;
    }
    diagnostics.push(
        Diagnostic::error(
            code::Code::E0931_EqWithoutHash,
            format!(
                "{kind_label} `{decl_name}` defines `operator==` but no `operator hash`; \
                 structural equality requires consistent hashing",
            ),
        )
        .with_span(eq.span)
        .with_help(
            "add `public int operator hash() { ... }` so the type can serve as a \
             `HashMap`/`HashSet` key with consistent semantics",
        ),
    );
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use juxc_lex::lex;
    use juxc_parse::parse;
    use juxc_source::SourceFile;

    /// Drive lex → parse → build and return (table, diagnostics).
    fn build_table(src: &str) -> (SymbolTable, Vec<Diagnostic>) {
        let sf = SourceFile::new("test.jux", src);
        let lex_result = lex(&sf);
        assert!(lex_result.diagnostics.is_empty(), "lex: {:?}", lex_result.diagnostics);
        let parse_result = parse(&lex_result.tokens);
        assert!(
            parse_result.diagnostics.is_empty(),
            "parse: {:?}",
            parse_result.diagnostics,
        );
        let mut diags = Vec::new();
        let table = build(&parse_result.ast, &mut diags);
        (table, diags)
    }

    /// A simple class lands in `table.classes` with its members captured.
    #[test]
    fn class_with_fields_and_methods_is_indexed() {
        let (table, diags) = build_table(
            r#"
            public class Point {
                private int x;
                private int y;
                public Point(int x, int y) { this.x = x; this.y = y; }
                public int sum() { return this.x + this.y; }
            }
            "#,
        );
        assert!(diags.is_empty(), "{:?}", diags);
        let class = table.classes.get("Point").expect("Point in table");
        assert!(!class.is_abstract);
        assert_eq!(class.fields.len(), 2);
        assert!(class.fields.contains_key("x"));
        assert!(class.fields.contains_key("y"));
        assert_eq!(class.methods.len(), 1);
        assert!(class.methods.contains_key("sum"));
        assert_eq!(class.constructors.len(), 1);
    }

    /// Regression: a class implementing a same-package interface (so the
    /// interface is FQN-keyed `xss.it.follow.Infer` while `implements` writes
    /// the bare `Infer`) without supplying the abstract method must fire
    /// `E0429` — previously the simple-name lookup missed it and the error
    /// leaked to rustc as `E0046`.
    #[test]
    fn abstract_method_unimplemented_in_package_fires_e0429() {
        let (_table, diags) = build_table(
            r#"
            package xss.it.follow;
            public interface Infer { void call(); }
            public class Follow implements Infer {
                public void printVal() { }
            }
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code.as_str() == "E0429"),
            "expected E0429, got: {diags:?}",
        );
    }

    /// Two `class Foo`s in the same unit → E0400 on the second.
    #[test]
    fn duplicate_class_name_emits_e0400() {
        let (table, diags) = build_table(
            "public class Foo {} public class Foo {}",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, code::Code::E0400_DuplicateDeclaration);
        // Only the first survives.
        assert!(table.classes.contains_key("Foo"));
        assert_eq!(table.classes.len(), 1);
    }

    /// Two `Foo` fields in the same class → E0401 on the second.
    #[test]
    fn duplicate_field_emits_e0401() {
        let (_table, diags) = build_table(
            r#"
            public class Foo {
                private int x;
                private int x;
            }
            "#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, code::Code::E0401_DuplicateField);
    }

    /// Two methods with the same name in the same class → E0402.
    /// (Will lift once overloads land.)
    #[test]
    fn duplicate_method_emits_e0402() {
        let (_table, diags) = build_table(
            r#"
            public class Foo {
                public void bar() {}
                public void bar() {}
            }
            "#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, code::Code::E0402_DuplicateMethod);
    }

    /// Two `Red` variants in the same enum → E0403.
    #[test]
    fn duplicate_enum_variant_emits_e0403() {
        let (_table, diags) = build_table(
            "public enum Color { Red, Green, Red }",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, code::Code::E0403_DuplicateVariant);
    }

    /// A record's components land in `table.records`.
    #[test]
    fn record_with_components_is_indexed() {
        let (table, diags) = build_table(
            "public record Pair(int first, int second) {}",
        );
        assert!(diags.is_empty(), "{:?}", diags);
        let record = table.records.get("Pair").expect("Pair in table");
        assert_eq!(record.components.len(), 2);
        assert_eq!(record.components[0].name, "first");
    }

    /// An enum with variants — payload and unit — both end up indexed.
    #[test]
    fn enum_with_unit_and_payload_variants_is_indexed() {
        let (table, _diags) = build_table(
            "public enum Token { Stop, Number(int), Word(String) }",
        );
        let e = table.enums.get("Token").expect("Token in table");
        assert_eq!(e.variants.len(), 3);
        assert!(e.variants["Stop"].payload.is_empty());
        assert_eq!(e.variants["Number"].payload.len(), 1);
        assert_eq!(e.variants["Word"].payload.len(), 1);
    }

    /// An interface's method signatures land in `table.interfaces`.
    #[test]
    fn interface_with_method_sigs_is_indexed() {
        let (table, diags) = build_table(
            "public interface Drawable { void draw(); int weight(); }",
        );
        assert!(diags.is_empty(), "{:?}", diags);
        let iface = table.interfaces.get("Drawable").expect("Drawable in table");
        assert_eq!(iface.methods.len(), 2);
        assert!(iface.methods["draw"].is_abstract);
        assert!(iface.methods["weight"].is_abstract);
    }

    /// A top-level function lands in `table.functions`.
    #[test]
    fn top_level_function_is_indexed() {
        let (table, diags) = build_table("public int helper(int x) { return x; }");
        assert!(diags.is_empty(), "{:?}", diags);
        let f = table.functions.get("helper").expect("helper in table");
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].name, "x");
    }

    /// A class and a function sharing the same name → E0400.
    #[test]
    fn class_and_function_with_same_name_collide() {
        let (_table, diags) = build_table(
            "public class Foo {} public void Foo() {}",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, code::Code::E0400_DuplicateDeclaration);
    }

    /// Operator declarations land in `ClassSig::operators` indexed by
    /// kind. Methods are unaffected.
    #[test]
    fn class_operator_is_indexed_by_kind() {
        let (table, diags) = build_table(
            r#"
            public class Path {
                public bool operator==(Path other) { return true; }
                public int operator hash() { return 0; }
            }
            "#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let class = table.classes.get("Path").expect("Path in table");
        assert_eq!(class.operators.len(), 2);
        assert!(class.operators.contains_key(&OperatorKind::Eq));
        assert!(class.operators.contains_key(&OperatorKind::Hash));
        assert!(class.methods.is_empty(), "operators should not leak into methods");
    }

    /// Two `operator+` declarations in the same class → E0402.
    #[test]
    fn duplicate_operator_emits_e0402() {
        let (_table, diags) = build_table(
            r#"
            public class M {
                public int operator+(int o) { return 0; }
                public int operator+(int o) { return 0; }
            }
            "#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, code::Code::E0402_DuplicateMethod);
    }

    /// Phase E.1 — `lookup_method` finds a method on the queried class
    /// directly and returns the queried name as the declaring class.
    #[test]
    fn lookup_method_direct_hit_reports_self_as_declaring() {
        let (table, _) = build_table(
            r#"
            public class A {
                public int m() { return 1; }
            }
            "#,
        );
        let (_method, declaring) = table
            .lookup_method("A", "m")
            .expect("A has m");
        assert_eq!(declaring, "A");
    }

    /// Phase E.1 — `lookup_method` walks the extends chain and reports
    /// the **parent** as the declaring class when only the parent
    /// defines the method.
    #[test]
    fn lookup_method_walks_extends_chain() {
        let (table, _) = build_table(
            r#"
            public class A {
                public int m() { return 1; }
            }
            public class B extends A {}
            "#,
        );
        let (_method, declaring) = table
            .lookup_method("B", "m")
            .expect("B inherits m from A");
        assert_eq!(declaring, "A");
    }

    /// Phase E.1 — a missing method returns None even when the chain
    /// is multi-hop.
    #[test]
    fn lookup_method_missing_returns_none() {
        let (table, _) = build_table(
            r#"
            public class A {}
            public class B extends A {}
            public class C extends B {}
            "#,
        );
        assert!(table.lookup_method("C", "nope").is_none());
    }

    /// Phase E.1 — `lookup_field` mirrors `lookup_method` for fields.
    /// A child class with no fields of its own finds the parent's.
    #[test]
    fn lookup_field_walks_extends_chain() {
        let (table, _) = build_table(
            r#"
            public class A { public int age; }
            public class B extends A {}
            "#,
        );
        let (_field, declaring) = table
            .lookup_field("B", "age")
            .expect("B inherits age from A");
        assert_eq!(declaring, "A");
    }

    // ----------------------------------------------------------------
    // E0931 — `==` / `hash` pairing rule (§O.2.7)
    // ----------------------------------------------------------------

    /// A class with `operator==` but no `operator hash` fires E0931.
    #[test]
    fn class_eq_without_hash_emits_e0931() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public bool operator==(P other) { return true; }
            }
            "#,
        );
        assert!(diags.iter().any(|d| d.code == code::Code::E0931_EqWithoutHash), "{diags:?}");
    }

    /// A class with both `operator==` AND `operator hash` is fine.
    #[test]
    fn class_eq_with_hash_is_ok() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public bool operator==(P other) { return true; }
                public int operator hash() { return 0; }
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0931_EqWithoutHash),
            "{diags:?}",
        );
    }

    /// `operator==` + `operator hash() = delete;` still fires E0931 —
    /// deletion isn't definition. The user signaled "structural eq
    /// without consistent hash," which §O.2.7 forbids.
    #[test]
    fn class_eq_with_deleted_hash_emits_e0931() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public bool operator==(P other) { return true; }
                public int operator hash() = delete;
            }
            "#,
        );
        assert!(diags.iter().any(|d| d.code == code::Code::E0931_EqWithoutHash), "{diags:?}");
    }

    /// `operator==(...) = delete;` is not a definition, so no E0931
    /// even when there's no `operator hash`. The user opted out of
    /// equality entirely.
    #[test]
    fn class_deleted_eq_does_not_emit_e0931() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public bool operator==(P other) = delete;
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0931_EqWithoutHash),
            "{diags:?}",
        );
    }

    /// Same rule applies to records.
    #[test]
    fn record_eq_without_hash_emits_e0931() {
        let (_table, diags) = build_table(
            r#"
            public record R(int x) {
                public bool operator==(R other) { return true; }
            }
            "#,
        );
        assert!(diags.iter().any(|d| d.code == code::Code::E0931_EqWithoutHash), "{diags:?}");
    }

    /// And to enums.
    #[test]
    fn enum_eq_without_hash_emits_e0931() {
        let (_table, diags) = build_table(
            r#"
            public enum E {
                A, B;
                public bool operator==(E other) { return true; }
            }
            "#,
        );
        assert!(diags.iter().any(|d| d.code == code::Code::E0931_EqWithoutHash), "{diags:?}");
    }

    /// `operator hash` alone (no `operator==`) is fine — the spec
    /// rule is one-directional. The user could want hashing via
    /// identity equality.
    #[test]
    fn hash_without_eq_is_ok() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public int operator hash() { return 0; }
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0931_EqWithoutHash),
            "{diags:?}",
        );
    }

    // ----------------------------------------------------------------
    // E0930 — `<=>` vs individual ordering ops (§O.2.1)
    // ----------------------------------------------------------------

    /// Defining both `operator<=>` and `operator<` fires E0930 on the
    /// individual decl (the redundant one — spec says `<=>` is the
    /// preferred form).
    #[test]
    fn cmp_with_lt_emits_e0930() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public int operator<=>(V other) { return 0; }
                public bool operator<(V other) { return false; }
            }
            "#,
        );
        assert!(diags.iter().any(|d| d.code == code::Code::E0930_OperatorConflict), "{diags:?}");
    }

    /// `<=>` paired with multiple individual ops — emit one E0930 per
    /// redundant individual op.
    #[test]
    fn cmp_with_all_four_individuals_emits_four_e0930() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public int operator<=>(V other) { return 0; }
                public bool operator<(V other) { return false; }
                public bool operator<=(V other) { return false; }
                public bool operator>(V other) { return false; }
                public bool operator>=(V other) { return false; }
            }
            "#,
        );
        let count = diags
            .iter()
            .filter(|d| d.code == code::Code::E0930_OperatorConflict)
            .count();
        assert_eq!(count, 4, "expected one E0930 per individual op: {diags:?}");
    }

    /// `<=>` alone (no individuals) is fine — the natural form.
    #[test]
    fn cmp_alone_is_ok() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public int operator<=>(V other) { return 0; }
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0930_OperatorConflict),
            "{diags:?}",
        );
    }

    /// Individual ops alone (no `<=>`) is also fine — the user opted
    /// into the four-operator form. (Whether the four are partial is
    /// a separate spec rule, not E0930.)
    #[test]
    fn individuals_alone_are_ok() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public bool operator<(V other) { return false; }
                public bool operator<=(V other) { return false; }
                public bool operator>(V other) { return false; }
                public bool operator>=(V other) { return false; }
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0930_OperatorConflict),
            "{diags:?}",
        );
    }

    /// `<=>` deleted + complete individual set defined — no conflict.
    /// Deletion isn't definition, so the user has switched to the
    /// four-operator form. All four must come together (per the
    /// partial-set rule below).
    #[test]
    fn deleted_cmp_with_individuals_is_ok() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public int operator<=>(V other) = delete;
                public bool operator<(V other) { return false; }
                public bool operator<=(V other) { return false; }
                public bool operator>(V other) { return false; }
                public bool operator>=(V other) { return false; }
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0930_OperatorConflict),
            "{diags:?}",
        );
    }

    /// `<=>` defined + `operator<` deleted — also no conflict.
    /// Deleting the individual op is the user opting out of the
    /// override even though the spec auto-derives them.
    #[test]
    fn cmp_with_deleted_lt_is_ok() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public int operator<=>(V other) { return 0; }
                public bool operator<(V other) = delete;
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0930_OperatorConflict),
            "{diags:?}",
        );
    }

    // ----------------------------------------------------------------
    // Operator return-type signature validation (§O.2.1, §O.2.2)
    // ----------------------------------------------------------------

    /// `String operator==` violates spec §O.2.1's "returns bool" rule
    /// — fires E0410 at signature-validation time. Catches the bug
    /// cleanly instead of letting rustc complain about a trait impl
    /// mismatch later.
    #[test]
    fn operator_eq_wrong_return_emits_e0410() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public String operator==(P other) { return "x"; }
                public int operator hash() { return 0; }
            }
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "{diags:?}",
        );
    }

    /// `operator<=>` must return `int`; declaring it as `bool` fires
    /// E0410.
    #[test]
    fn operator_cmp_wrong_return_emits_e0410() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public bool operator<=>(V other) { return false; }
            }
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "{diags:?}",
        );
    }

    /// `operator hash` must return `int`; declaring it as `String`
    /// fires E0410.
    #[test]
    fn operator_hash_wrong_return_emits_e0410() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public String operator hash() { return "x"; }
            }
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "{diags:?}",
        );
    }

    /// `operator string` must return `String`; declaring it as `int`
    /// fires E0410.
    #[test]
    fn operator_string_wrong_return_emits_e0410() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public int operator string() { return 0; }
            }
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0410_TypeMismatch),
            "{diags:?}",
        );
    }

    /// Correct-signature operators are not flagged.
    #[test]
    fn correct_operator_signatures_are_silent() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public bool operator==(P other) { return true; }
                public int operator hash() { return 0; }
                public String operator string() { return "x"; }
                public int operator<=>(P other) { return 0; }
            }
            "#,
        );
        // Only the signature-related E0410s would fire — there should
        // be none. (Body-checks may fire other E0410s but we're
        // counting only signature-rule violations here, which would
        // anchor at the operator span. For this test we just check
        // that no E0410 with the operator-return phrasing appears.)
        let any_op_e0410 = diags.iter().any(|d| {
            d.code == code::Code::E0410_TypeMismatch
                && d.message.contains("must return")
        });
        assert!(!any_op_e0410, "{diags:?}");
    }

    /// `= delete;` operators are exempt from the return-type rule —
    /// the user is opting out, not providing a real signature.
    #[test]
    fn deleted_operator_skips_return_type_check() {
        let (_table, diags) = build_table(
            r#"
            public class P {
                public int x;
                public P(int x) { this.x = x; }
                public String operator==(P other) = delete;
            }
            "#,
        );
        let any_op_e0410 = diags.iter().any(|d| {
            d.code == code::Code::E0410_TypeMismatch
                && d.message.contains("must return")
        });
        assert!(!any_op_e0410, "deletion should bypass rule: {diags:?}");
    }

    // ----------------------------------------------------------------
    // Partial individual-ordering set (§O.2.1)
    // ----------------------------------------------------------------

    /// Declaring only `operator<` fires E0930 listing the missing
    /// three: `<=`, `>`, `>=`. Spec §O.2.1 says "must define all
    /// four; no partial sets."
    #[test]
    fn lt_alone_emits_e0930_listing_missing_three() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public bool operator<(V other) { return false; }
            }
            "#,
        );
        let hit = diags.iter().find(|d| {
            d.code == code::Code::E0930_OperatorConflict
                && d.message.contains("partial set") || d.message.contains("not all four")
        });
        assert!(hit.is_some(), "{diags:?}");
        let msg = &hit.unwrap().message;
        assert!(msg.contains("`operator<=`"), "missing <= in message: {msg}");
        assert!(msg.contains("`operator>`"), "missing > in message: {msg}");
        assert!(msg.contains("`operator>=`"), "missing >= in message: {msg}");
    }

    /// Three out of four → E0930 lists the one missing.
    #[test]
    fn three_individuals_missing_one_emits_e0930() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public bool operator<(V other) { return false; }
                public bool operator<=(V other) { return false; }
                public bool operator>(V other) { return false; }
            }
            "#,
        );
        let hit = diags.iter().find(|d| {
            d.code == code::Code::E0930_OperatorConflict
                && d.message.contains("not all four")
        });
        assert!(hit.is_some(), "{diags:?}");
        let msg = &hit.unwrap().message;
        assert!(msg.contains("`operator>=`"), "should list >=: {msg}");
        assert!(!msg.contains("`operator<`"), "shouldn't list <: {msg}");
    }

    /// All four declared → silent.
    #[test]
    fn all_four_individuals_is_ok() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public bool operator<(V other) { return false; }
                public bool operator<=(V other) { return false; }
                public bool operator>(V other) { return false; }
                public bool operator>=(V other) { return false; }
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| {
                d.code == code::Code::E0930_OperatorConflict
                    && d.message.contains("not all four")
            }),
            "{diags:?}",
        );
    }

    /// Three defined + one deleted → still a partial-set violation.
    /// Deletion is opt-out, not a declaration that satisfies the
    /// rule.
    #[test]
    fn three_defined_plus_one_deleted_still_partial() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public bool operator<(V other) { return false; }
                public bool operator<=(V other) { return false; }
                public bool operator>(V other) { return false; }
                public bool operator>=(V other) = delete;
            }
            "#,
        );
        let hit = diags.iter().find(|d| {
            d.code == code::Code::E0930_OperatorConflict
                && d.message.contains("not all four")
        });
        assert!(hit.is_some(), "{diags:?}");
        assert!(
            hit.unwrap().message.contains("`operator>=`"),
            "should call out >=: {hit:?}",
        );
    }

    /// All four deleted → no diagnostic. The user has explicitly
    /// opted out of every individual; the rule has nothing to enforce.
    #[test]
    fn all_four_deleted_is_silent() {
        let (_table, diags) = build_table(
            r#"
            public class V {
                public int x;
                public V(int x) { this.x = x; }
                public bool operator<(V other) = delete;
                public bool operator<=(V other) = delete;
                public bool operator>(V other) = delete;
                public bool operator>=(V other) = delete;
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| {
                d.code == code::Code::E0930_OperatorConflict
                    && d.message.contains("not all four")
            }),
            "{diags:?}",
        );
    }

    /// Same rule applies to records.
    #[test]
    fn record_lt_alone_emits_e0930() {
        let (_table, diags) = build_table(
            r#"
            public record R(int x) {
                public bool operator<(R other) { return false; }
            }
            "#,
        );
        let hit = diags.iter().any(|d| {
            d.code == code::Code::E0930_OperatorConflict
                && d.message.contains("not all four")
        });
        assert!(hit, "{diags:?}");
    }

    /// Arithmetic-family operators have no spec-fixed return type;
    /// the user can declare any type. No E0410 fires here.
    #[test]
    fn arithmetic_operator_return_is_free() {
        let (_table, diags) = build_table(
            r#"
            public class M {
                public int x;
                public M(int x) { this.x = x; }
                public String operator+(M other) { return "x"; }
            }
            "#,
        );
        let any_op_e0410 = diags.iter().any(|d| {
            d.code == code::Code::E0410_TypeMismatch
                && d.message.contains("must return")
        });
        assert!(!any_op_e0410, "arithmetic ops have free return: {diags:?}");
    }

    /// Same rule applies to records.
    #[test]
    fn record_cmp_with_lt_emits_e0930() {
        let (_table, diags) = build_table(
            r#"
            public record R(int x) {
                public int operator<=>(R other) { return 0; }
                public bool operator<(R other) { return false; }
            }
            "#,
        );
        assert!(diags.iter().any(|d| d.code == code::Code::E0930_OperatorConflict), "{diags:?}");
    }

    // ----------------------------------------------------------------
    // final / sealed / permits (Step 6 — encapsulation)
    // ----------------------------------------------------------------

    /// `final class F {} ; class C extends F {}` → E0420.
    #[test]
    fn extending_final_class_emits_e0420() {
        let (_table, diags) = build_table(
            r#"
            public final class Animal {}
            public class Dog extends Animal {}
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0420_FinalClassExtended),
            "expected E0420, got: {diags:?}",
        );
    }

    /// Final method on parent + same name on child → E0421.
    #[test]
    fn overriding_final_method_emits_e0421() {
        let (_table, diags) = build_table(
            r#"
            public class Animal {
                public final String speak() { return ""; }
            }
            public class Dog extends Animal {
                public String speak() { return "woof"; }
            }
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0421_FinalMethodOverridden),
            "expected E0421, got: {diags:?}",
        );
    }

    /// Sealed class — extender NOT in permits list → E0422.
    #[test]
    fn sealed_class_not_permitted_emits_e0422() {
        let (_table, diags) = build_table(
            r#"
            public sealed class Shape permits Circle {}
            public class Circle extends Shape {}
            public class Square extends Shape {}
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0422_SealedClassNotPermitted),
            "expected E0422 for `Square`, got: {diags:?}",
        );
    }

    /// Sealed class — extender IS in permits list → no diagnostic.
    #[test]
    fn sealed_class_permitted_subclass_is_ok() {
        let (_table, diags) = build_table(
            r#"
            public sealed class Shape permits Circle, Square {}
            public class Circle extends Shape {}
            public class Square extends Shape {}
            "#,
        );
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0422_SealedClassNotPermitted),
            "permitted subclasses should be OK: {diags:?}",
        );
    }

    /// Non-final, non-sealed parent — any class can extend.
    #[test]
    fn ordinary_extends_remains_unaffected() {
        let (_table, diags) = build_table(
            r#"
            public class Animal {}
            public class Dog extends Animal {}
            "#,
        );
        assert!(
            !diags.iter().any(|d| {
                matches!(
                    d.code,
                    code::Code::E0420_FinalClassExtended
                        | code::Code::E0422_SealedClassNotPermitted
                )
            }),
            "ordinary inheritance should not fire E0420/E0422: {diags:?}",
        );
    }

    // ----------------------------------------------------------------
    // Workspace symbol table (multi-file compilation)
    // ----------------------------------------------------------------

    /// Helper — lex+parse multiple sources and feed them through
    /// `build_workspace`. Returns the merged table and the
    /// concatenated diagnostics in unit order.
    fn build_workspace_table(srcs: &[&str]) -> (SymbolTable, Vec<Diagnostic>) {
        let mut units: Vec<juxc_ast::CompilationUnit> = Vec::new();
        for src in srcs {
            let sf = SourceFile::new("test.jux", *src);
            let lex_result = lex(&sf);
            assert!(lex_result.diagnostics.is_empty());
            let parse_result = parse(&lex_result.tokens);
            assert!(parse_result.diagnostics.is_empty());
            units.push(parse_result.ast);
        }
        let mut diags = Vec::new();
        let table = build_workspace(&units, &mut diags);
        (table, diags)
    }

    /// Two units with classes in different packages — both end up in
    /// the merged table keyed by FQN; no duplicate diagnostic.
    #[test]
    fn workspace_merges_classes_from_two_units() {
        let (table, diags) = build_workspace_table(&[
            "package a.lib;\npublic class Foo {}",
            "package b.app;\npublic class Bar {}",
        ]);
        assert!(table.classes.contains_key("a.lib.Foo"));
        assert!(table.classes.contains_key("b.app.Bar"));
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0400_DuplicateDeclaration),
            "no duplicates expected: {diags:?}",
        );
    }

    /// Two units defining the **same FQN** fire E0400 against the
    /// second occurrence. Same bare name in DIFFERENT packages is
    /// fine — that's the whole point of FQN keys.
    #[test]
    fn workspace_duplicate_fqn_emits_e0400() {
        let (_table, diags) = build_workspace_table(&[
            "package a.lib;\npublic class Foo {}",
            "package a.lib;\npublic class Foo {}",
        ]);
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0400_DuplicateDeclaration),
            "same FQN twice should fire E0400: {diags:?}",
        );
    }

    /// Same bare name in different packages — coexist with no E0400.
    /// This is the cross-package namespacing payoff.
    #[test]
    fn workspace_same_bare_name_in_different_packages_is_ok() {
        let (table, diags) = build_workspace_table(&[
            "package a.lib;\npublic class Foo {}",
            "package b.app;\npublic class Foo {}",
        ]);
        assert!(table.classes.contains_key("a.lib.Foo"));
        assert!(table.classes.contains_key("b.app.Foo"));
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0400_DuplicateDeclaration),
            "Foo in two packages should coexist: {diags:?}",
        );
    }

    /// Each class records its own package on `ClassSig::package`.
    #[test]
    fn workspace_class_records_its_unit_package() {
        let (table, _diags) = build_workspace_table(&[
            "package a.lib;\npublic class Foo {}",
            "package b.app;\npublic class Bar {}",
        ]);
        assert_eq!(
            table.classes["a.lib.Foo"].package,
            vec!["a".to_string(), "lib".to_string()],
        );
        assert_eq!(
            table.classes["b.app.Bar"].package,
            vec!["b".to_string(), "app".to_string()],
        );
    }

    /// `UnitContext::unqualified` carries the bare→FQN map for each
    /// unit — same-package siblings plus every `import` line.
    #[test]
    fn workspace_unit_context_maps_imports_and_siblings() {
        let (table, _diags) = build_workspace_table(&[
            "package com.lib;\npublic class Greeter {}",
            "package app.main;\nimport com.lib.Greeter;\npublic class App {}",
        ]);
        // Unit 1 (com.lib): only same-package sibling Greeter.
        assert_eq!(
            table.units[0].unqualified.get("Greeter"),
            Some(&"com.lib.Greeter".to_string()),
        );
        // Unit 2 (app.main): App as same-package sibling, Greeter
        // via import.
        assert_eq!(
            table.units[1].unqualified.get("App"),
            Some(&"app.main.App".to_string()),
        );
        assert_eq!(
            table.units[1].unqualified.get("Greeter"),
            Some(&"com.lib.Greeter".to_string()),
        );
    }

    // ----------------------------------------------------------------
    // Type aliases (§A.2.4)
    // ----------------------------------------------------------------

    /// Bare aliases land in `table.aliases` keyed by FQN and the
    /// `is_type_name` check picks them up.
    #[test]
    fn type_alias_lands_in_table() {
        let (table, diags) = build_table("public type UserId = int;");
        assert!(diags.is_empty(), "no diagnostics expected: {diags:?}");
        assert!(table.aliases.contains_key("UserId"));
        assert!(table.is_type_name("UserId"));
    }

    // ----------------------------------------------------------------
    // Inheritance shape — single non-final class + multi-interface
    // ----------------------------------------------------------------

    /// `class C extends Drawable` where Drawable is an interface
    /// fires E0423.
    #[test]
    fn class_extending_interface_emits_e0423() {
        let (_table, diags) = build_table(
            r#"
            public interface Drawable {}
            public class Shape extends Drawable {}
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0423_ExtendsNotAClass),
            "expected E0423: {diags:?}",
        );
    }

    /// `class C implements SomeClass` fires E0424.
    #[test]
    fn class_implementing_class_emits_e0424() {
        let (_table, diags) = build_table(
            r#"
            public class Base {}
            public class Sub implements Base {}
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0424_ImplementsNotAnInterface),
            "expected E0424: {diags:?}",
        );
    }

    /// `class C implements I1, I2, I3` — multiple interfaces is
    /// fine (the Java rule).
    #[test]
    fn class_implementing_multiple_interfaces_is_ok() {
        let (_table, diags) = build_table(
            r#"
            public interface A {}
            public interface B {}
            public interface C {}
            public class X implements A, B, C {}
            "#,
        );
        assert!(
            !diags.iter().any(|d| matches!(
                d.code,
                code::Code::E0423_ExtendsNotAClass
                    | code::Code::E0424_ImplementsNotAnInterface
            )),
            "expected no E0423/E0424: {diags:?}",
        );
    }

    // ----------------------------------------------------------------
    // Interface dyn-dispatch object-safety gate (E0435 predicate)
    // ----------------------------------------------------------------

    /// A plain, non-generic interface with only ordinary instance methods
    /// is ready to back a `Rc<dyn Trait>` value type.
    #[test]
    fn plain_interface_is_dyn_dispatchable() {
        let (table, diags) = build_table(
            r#"
            public interface Shape {
                double area();
                String name();
            }
            "#,
        );
        assert!(diags.is_empty(), "no diagnostics: {diags:?}");
        assert_eq!(
            interface_dyn_dispatch_support(&table, "Shape"),
            Some(Ok(())),
            "plain interface should be dyn-dispatchable",
        );
    }

    /// `static` and `default` methods don't block dispatch — statics become
    /// free functions, defaults are object-safe trait items.
    #[test]
    fn interface_with_static_and_default_is_dyn_dispatchable() {
        let (table, diags) = build_table(
            r#"
            public interface MathLike {
                static int doubled(int n) { return n + n; }
                default int quadrupled(int n) { return n + n + n + n; }
                int compute(int n);
            }
            "#,
        );
        assert!(diags.is_empty(), "no diagnostics: {diags:?}");
        assert_eq!(
            interface_dyn_dispatch_support(&table, "MathLike"),
            Some(Ok(())),
            "static/default methods must not block dispatch",
        );
    }

    /// A generic interface (`interface A<T>`) is a perfectly valid
    /// declaration, but is deferred as a `dyn` value type for stage 1.
    #[test]
    fn generic_interface_is_deferred_for_dyn() {
        let (table, diags) = build_table(
            r#"
            public interface Box<T> {
                T get();
            }
            "#,
        );
        assert!(diags.is_empty(), "declaration itself is valid: {diags:?}");
        assert_eq!(
            interface_dyn_dispatch_support(&table, "Box"),
            Some(Err(DynDispatchBlock::GenericInterface(1))),
            "generic interface as a dyn value type is deferred",
        );
    }

    /// An interface with a generic *method* (`<R> R map(...)`) is not
    /// object-safe — deferred, reporting the offending method.
    #[test]
    fn interface_with_generic_method_is_deferred_for_dyn() {
        let (table, diags) = build_table(
            r#"
            public interface Mapper {
                <R> R map(R input);
                String name();
            }
            "#,
        );
        assert!(diags.is_empty(), "declaration itself is valid: {diags:?}");
        assert_eq!(
            interface_dyn_dispatch_support(&table, "Mapper"),
            Some(Err(DynDispatchBlock::GenericMethod("map".to_string()))),
            "generic interface method blocks object safety",
        );
    }

    /// A non-interface name (a class) resolves to `None` — the predicate
    /// only speaks about interfaces.
    #[test]
    fn non_interface_name_is_none_for_dyn_support() {
        let (table, _diags) = build_table(
            r#"
            public class Circle {}
            "#,
        );
        assert_eq!(
            interface_dyn_dispatch_support(&table, "Circle"),
            None,
            "a class is not an interface",
        );
        assert_eq!(
            interface_dyn_dispatch_support(&table, "DoesNotExist"),
            None,
            "an unknown name is not an interface",
        );
    }

    // ----------------------------------------------------------------
    // Interface on exception class (E0436)
    // ----------------------------------------------------------------

    /// An exception-hierarchy class that implements an interface is
    /// rejected (deferred for stage-1 dispatch) rather than mis-lowered.
    #[test]
    fn exception_class_implementing_interface_emits_e0436() {
        let (_table, diags) = build_table(
            r#"
            public interface Tagged { void bump(); }
            public class Boom extends Exception implements Tagged {
                public void bump() {}
            }
            "#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == code::Code::E0436_InterfaceOnExceptionClass),
            "expected E0436: {diags:?}",
        );
    }

    /// A subclass that inherits both the exception base and the interface
    /// obligation through its parent also trips E0436.
    #[test]
    fn inherited_exception_and_interface_emits_e0436() {
        let (_table, diags) = build_table(
            r#"
            public interface Tagged { void bump(); }
            public class Base extends Exception implements Tagged {
                public void bump() {}
            }
            public class Derived extends Base {}
            "#,
        );
        assert!(
            diags
                .iter()
                .filter(|d| d.code == code::Code::E0436_InterfaceOnExceptionClass)
                .count()
                >= 1,
            "expected E0436 on the exception+interface hierarchy: {diags:?}",
        );
    }

    /// A normal (non-exception) class implementing an interface must NOT
    /// trip E0436 — this is the common, supported case.
    #[test]
    fn normal_interface_implementer_no_e0436() {
        let (_table, diags) = build_table(
            r#"
            public interface Shape { double area(); }
            public class Circle implements Shape {
                public double area() { return 1.0; }
            }
            "#,
        );
        assert!(
            !diags
                .iter()
                .any(|d| d.code == code::Code::E0436_InterfaceOnExceptionClass),
            "normal implementer must not trip E0436: {diags:?}",
        );
    }

    // ----------------------------------------------------------------
    // Interface subtyping (is_subtype: class <: implemented interface)
    // ----------------------------------------------------------------

    fn user_ty(name: &str) -> crate::ty::Ty {
        crate::ty::Ty::User {
            name: name.to_string(),
            generic_args: vec![],
        }
    }

    /// A class is a subtype of an interface it directly implements.
    #[test]
    fn class_is_subtype_of_directly_implemented_interface() {
        let (table, diags) = build_table(
            r#"
            public interface Shape { double area(); }
            public class Circle implements Shape {
                public double area() { return 1.0; }
            }
            "#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert!(crate::ty::is_subtype(&user_ty("Circle"), &user_ty("Shape"), &table));
        // Not the reverse.
        assert!(!crate::ty::is_subtype(&user_ty("Shape"), &user_ty("Circle"), &table));
    }

    /// A class inherits its superclass's `implements`, so it's a subtype
    /// of an interface the parent implements.
    #[test]
    fn class_is_subtype_of_inherited_interface() {
        let (table, diags) = build_table(
            r#"
            public interface Shape { double area(); }
            public class Base implements Shape {
                public double area() { return 1.0; }
            }
            public class Derived extends Base {}
            "#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert!(crate::ty::is_subtype(&user_ty("Derived"), &user_ty("Shape"), &table));
    }

    /// Transitive interface-extends: `class C implements A`, `A extends B`
    /// ⟹ `C <: B`.
    #[test]
    fn class_is_subtype_through_interface_extends() {
        let (table, diags) = build_table(
            r#"
            public interface B { void b(); }
            public interface A extends B { void a(); }
            public class C implements A {
                public void a() {}
                public void b() {}
            }
            "#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert!(crate::ty::is_subtype(&user_ty("C"), &user_ty("A"), &table));
        assert!(crate::ty::is_subtype(&user_ty("C"), &user_ty("B"), &table));
    }

    /// An unrelated class is not a subtype of an interface it doesn't
    /// implement, and the existing class-extends relation still holds.
    #[test]
    fn unrelated_class_not_subtype_and_extends_still_works() {
        let (table, diags) = build_table(
            r#"
            public interface Shape { double area(); }
            public class Other {}
            public class Animal {}
            public class Dog extends Animal {}
            "#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert!(!crate::ty::is_subtype(&user_ty("Other"), &user_ty("Shape"), &table));
        // Regression: class-extends subtyping is untouched.
        assert!(crate::ty::is_subtype(&user_ty("Dog"), &user_ty("Animal"), &table));
    }

    // ----------------------------------------------------------------
    // Top-level constants (§A.2.2)
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // Static members (class-scoped fields and methods)
    // ----------------------------------------------------------------

    /// A `static final double` field lands on ClassSig with the
    /// flags set.
    #[test]
    fn static_field_records_flags() {
        let (table, diags) = build_table(
            r#"
            public class Math {
                public static final double PI = 3.14;
            }
            "#,
        );
        assert!(diags.is_empty(), "no diagnostics: {diags:?}");
        let class = &table.classes["Math"];
        let pi = &class.fields["PI"];
        assert!(pi.is_static);
        assert!(pi.is_final);
    }

    // ----------------------------------------------------------------
    // Annotations + @Override semantics
    // ----------------------------------------------------------------

    /// `@Override` on a method that actually overrides emits no
    /// diagnostic.
    #[test]
    fn override_present_is_ok() {
        let (_table, diags) = build_table(
            r#"
            public class Animal { public String speak() { return ""; } }
            public class Dog extends Animal {
                @Override
                public String speak() { return "woof"; }
            }
            "#,
        );
        assert!(
            !diags.iter().any(|d| d.code == code::Code::E0426_OverrideMissing),
            "no E0426 expected: {diags:?}",
        );
    }

    /// `@Override` on a method that doesn't override fires E0426.
    #[test]
    fn override_missing_emits_e0426() {
        let (_table, diags) = build_table(
            r#"
            public class Animal {}
            public class Dog extends Animal {
                @Override
                public String speak() { return "woof"; }
            }
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0426_OverrideMissing),
            "expected E0426: {diags:?}",
        );
    }

    /// Lowercase `@override` is the same annotation as `@Override`
    /// per spec — case-insensitive matching applies.
    #[test]
    fn override_case_insensitive() {
        let (_table, diags) = build_table(
            r#"
            public class Animal {}
            public class Dog extends Animal {
                @override
                public String speak() { return "woof"; }
            }
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0426_OverrideMissing),
            "lowercase @override should be matched the same: {diags:?}",
        );
    }

    /// A `static int max(...)` method lands on ClassSig with
    /// `is_static = true`.
    #[test]
    fn static_method_records_flag() {
        let (table, diags) = build_table(
            r#"
            public class Math {
                public static int max(int a, int b) { return a; }
            }
            "#,
        );
        assert!(diags.is_empty(), "no diagnostics: {diags:?}");
        let class = &table.classes["Math"];
        let max = &class.methods["max"];
        assert!(max.is_static);
    }

    /// `const int MAX = 100;` lands in `table.consts` with the
    /// declared type recorded.
    #[test]
    fn top_level_const_is_indexed() {
        let (table, diags) = build_table("public const int MAX = 100;");
        assert!(diags.is_empty(), "no diagnostics: {diags:?}");
        assert!(table.consts.contains_key("MAX"));
    }

    /// `final` is a synonym for `const` at the top-level constant
    /// position — registered exactly the same way.
    #[test]
    fn top_level_final_is_alias_for_const() {
        let (table, diags) = build_table("public final String NAME = \"x\";");
        assert!(diags.is_empty(), "no diagnostics: {diags:?}");
        assert!(table.consts.contains_key("NAME"));
    }

    /// Two `const` declarations with the same name in the same
    /// package fire E0400.
    #[test]
    fn duplicate_const_emits_e0400() {
        let (_table, diags) = build_table(
            r#"
            public const int X = 1;
            public const int X = 2;
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0400_DuplicateDeclaration),
            "expected E0400 for duplicate constant: {diags:?}",
        );
    }

    /// A type alias name conflicting with a class name fires E0400.
    #[test]
    fn type_alias_duplicate_with_class_emits_e0400() {
        let (_table, diags) = build_table(
            r#"
            public class Foo {}
            public type Foo = int;
            "#,
        );
        assert!(
            diags.iter().any(|d| d.code == code::Code::E0400_DuplicateDeclaration),
            "expected E0400 for alias vs class name clash: {diags:?}",
        );
    }
}
