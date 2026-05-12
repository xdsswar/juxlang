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
    ClassDecl, CompilationUnit, EnumDecl, FieldDecl, FnDecl, InterfaceDecl, RecordDecl,
    ReturnType, TopLevelDecl, TypeParam, TypeRef, Visibility,
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
    table.enums.insert(
        enum_decl.name.text.clone(),
        EnumSig {
            visibility: enum_decl.visibility,
            variants,
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

fn param_sig(p: &juxc_ast::Param) -> ParamSig {
    ParamSig {
        name: p.name.text.clone(),
        ty: p.ty.clone(),
    }
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
}
