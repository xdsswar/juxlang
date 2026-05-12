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
    /// Top-level classes indexed by name.
    pub classes: HashMap<String, ClassSig>,
    /// Top-level records indexed by name.
    pub records: HashMap<String, RecordSig>,
    /// Top-level enums indexed by name.
    pub enums: HashMap<String, EnumSig>,
    /// Top-level interfaces indexed by name.
    pub interfaces: HashMap<String, InterfaceSig>,
    /// Top-level functions (outside any class) indexed by name.
    /// Overloads aren't supported yet — a duplicate emits `E0400`.
    pub functions: HashMap<String, FunctionSig>,
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
            cursor = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()));
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
            cursor = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()));
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
    /// True when the class is declared `abstract`.
    pub is_abstract: bool,
    /// Generic parameters in declaration order, e.g. `<T, U>`.
    pub generic_params: Vec<TypeParam>,
    /// Parent type, if `extends Parent` was given.
    pub extends: Option<TypeRef>,
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
    let mut table = SymbolTable::default();
    for item in &unit.items {
        match item {
            TopLevelDecl::Class(class_decl) => insert_class(&mut table, class_decl, diagnostics),
            TopLevelDecl::Record(record_decl) => {
                insert_record(&mut table, record_decl, diagnostics);
            }
            TopLevelDecl::Enum(enum_decl) => insert_enum(&mut table, enum_decl, diagnostics),
            TopLevelDecl::Interface(interface_decl) => {
                insert_interface(&mut table, interface_decl, diagnostics);
            }
            TopLevelDecl::Function(fn_decl) => {
                insert_function(&mut table, fn_decl, diagnostics);
            }
        }
    }
    table
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
    if table.is_type_name(name) || table.functions.contains_key(name) {
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !ensure_top_level_unique(table, &class_decl.name.text, class_decl.span, diagnostics) {
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
        class_decl.name.text.clone(),
        ClassSig {
            visibility: class_decl.visibility,
            is_abstract: class_decl.is_abstract,
            generic_params: class_decl.generic_params.clone(),
            extends: class_decl.extends.clone(),
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !ensure_top_level_unique(table, &record_decl.name.text, record_decl.span, diagnostics) {
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
        record_decl.name.text.clone(),
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !ensure_top_level_unique(table, &enum_decl.name.text, enum_decl.span, diagnostics) {
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
        enum_decl.name.text.clone(),
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !ensure_top_level_unique(
        table,
        &interface_decl.name.text,
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
        interface_decl.name.text.clone(),
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !ensure_top_level_unique(table, &fn_decl.name.text, fn_decl.span, diagnostics) {
        return;
    }
    table.functions.insert(
        fn_decl.name.text.clone(),
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
        let parts: Vec<String> = t.generic_args.iter().map(render_type_ref).collect();
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
}
