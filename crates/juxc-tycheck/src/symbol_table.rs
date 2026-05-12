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
        let mut cursor: Option<&str> = Some(class_name);
        let mut depth = 0usize;
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
        None
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
    /// Span of the whole class declaration.
    pub span: Span,
}

/// Signature of one class field.
#[derive(Debug, Clone)]
pub struct FieldSig {
    /// Field visibility.
    pub visibility: Visibility,
    /// Declared type as written in source.
    pub ty: TypeRef,
    /// Span of the field declaration.
    pub span: Span,
}

/// Signature of one class method.
#[derive(Debug, Clone)]
pub struct MethodSig {
    /// Method visibility.
    pub visibility: Visibility,
    /// Whether the method is declared `abstract` (no body).
    pub is_abstract: bool,
    /// Whether the method is declared `final` (no overriding by
    /// subclasses). Enforced by `check_final_method_overrides`.
    pub is_final: bool,
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
    /// Method signatures indexed by name. Bodies are absent (`body:
    /// None` in the source). Duplicate names emit `E0402`.
    pub methods: HashMap<String, MethodSig>,
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
            insert_top_level(&mut table, item, &unit_pkg, unit_idx, diagnostics);
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
    // final/sealed extends and final-method override checks.
    check_final_and_sealed_extends(&table, diagnostics);
    check_final_method_overrides(&table, diagnostics);
    table
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

fn insert_top_level(
    table: &mut SymbolTable,
    item: &TopLevelDecl,
    package: &[String],
    unit_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Every top-level kind is keyed by FQN. Records / enums /
    // interfaces / free functions don't yet carry a per-decl
    // `package` field on their Sig structs, but they ARE namespaced
    // by FQN in the table, so two `record Foo`s in different
    // packages coexist without firing E0400.
    match item {
        TopLevelDecl::Class(class_decl) => {
            insert_class(table, class_decl, package, diagnostics);
        }
        TopLevelDecl::Record(record_decl) => {
            insert_record(table, record_decl, package, diagnostics);
        }
        TopLevelDecl::Enum(enum_decl) => {
            insert_enum(table, enum_decl, package, diagnostics);
        }
        TopLevelDecl::Interface(interface_decl) => {
            insert_interface(table, interface_decl, package, diagnostics);
        }
        TopLevelDecl::Function(fn_decl) => {
            insert_function(table, fn_decl, package, diagnostics);
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
            ty: decl.ty.clone(),
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
            if !parent.permits.iter().any(|n| n == child_name) {
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
        methods.insert(method.name.text.clone(), method_sig(method));
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

    table.classes.insert(
        fqn,
        ClassSig {
            visibility: class_decl.visibility,
            package: package.to_vec(),
            is_abstract: class_decl.is_abstract,
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
            span: class_decl.span,
        },
    );
}

fn insert_record(
    table: &mut SymbolTable,
    record_decl: &RecordDecl,
    package: &[String],
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
        methods.insert(method.name.text.clone(), method_sig(method));
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
            span: enum_decl.span,
        },
    );
}

fn insert_interface(
    table: &mut SymbolTable,
    interface_decl: &InterfaceDecl,
    package: &[String],
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
        methods.insert(method.name.text.clone(), method_sig(method));
    }
    table.interfaces.insert(
        fqn,
        InterfaceSig {
            visibility: interface_decl.visibility,
            generic_params: interface_decl.generic_params.clone(),
            methods,
            span: interface_decl.span,
        },
    );
}

fn insert_function(
    table: &mut SymbolTable,
    fn_decl: &FnDecl,
    package: &[String],
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
        ty: field.ty.clone(),
        span: field.span,
    }
}

fn method_sig(method: &FnDecl) -> MethodSig {
    MethodSig {
        visibility: method.visibility,
        is_abstract: method.body.is_none(),
        is_final: method
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Final)),
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
    // Top-level constants (§A.2.2)
    // ----------------------------------------------------------------

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
