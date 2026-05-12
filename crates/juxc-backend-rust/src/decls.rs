//! Top-level declaration emitters — class, record, enum, interface,
//! function/method, constructor, and the trailing-return-elision helpers
//! for function bodies.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use std::collections::HashSet;

use juxc_ast::{Block, FnDecl, ReturnType, Stmt};
use juxc_tycheck::symbol_table::MethodSig;

use crate::analysis::{
    body_writes_to_this, collect_mutated_names, extract_simple_ctor_inits, field_supports_copy,
    field_supports_display, field_supports_eq, field_supports_hash, is_jux_string_type_ref,
    SimpleCtorInits,
};
use crate::RustEmitter;

/// Render the `#[derive(...)]` attribute line for an auto-derived value
/// type (record or enum). `Debug, Clone, PartialEq` are unconditional;
/// `Eq`, `Hash`, and `Copy` are added when every field/payload type
/// supports them (see [`field_supports_eq`], [`field_supports_hash`],
/// [`field_supports_copy`]).
///
/// Returned in the order rustc canonicalizes for `cargo fmt` so a
/// snapshot test can pin the exact string.
///
/// Per `JUX-OPERATORS-ADDENDUM.md` §O.3 records and enums auto-provide
/// `operator==` (→ PartialEq), `operator hash` (→ Hash + Eq), and
/// "copy on assignment" (→ Copy when the field set permits).
fn derive_attribute_for_value_type(field_tys: &[&juxc_ast::TypeRef]) -> String {
    let mut derives: Vec<&str> = vec!["Debug", "Clone"];
    let all_eq = field_tys.iter().all(|t| field_supports_eq(t));
    let all_hash = field_tys.iter().all(|t| field_supports_hash(t));
    let all_copy = field_tys.iter().all(|t| field_supports_copy(t));
    // PartialEq is required as the prerequisite for Eq, and we emit it
    // unconditionally today even for shapes Eq can't reach. That's
    // existing behavior — a record field whose type isn't PartialEq
    // would already fail at rustc time before this change.
    derives.push("PartialEq");
    if all_eq {
        derives.push("Eq");
    }
    if all_hash {
        derives.push("Hash");
    }
    if all_copy {
        derives.push("Copy");
    }
    format!("#[derive({})]", derives.join(", "))
}

impl RustEmitter {
    /// Lower a Jux interface to a Rust `trait`. Method signatures
    /// emit directly — `void foo();` becomes `fn foo(&self);` —
    /// and Turn-1 interfaces have no default-method bodies.
    ///
    /// **Receiver kind.** Trait methods always use `&self` in Turn 1.
    /// If a class implementing the interface needs to mutate state in
    /// its method body, the user has to mark that method non-interface
    /// — the cross-class receiver-kind analysis isn't in yet. See the
    /// Turn-1 limitations note in the interface doc.
    pub(crate) fn emit_interface_decl(&mut self, interface: &juxc_ast::InterfaceDecl) {
        // (Migrated to Writer indent-aware API)
        self.w.emit_indent();
        self.emit_visibility(interface.visibility);
        self.w.push_str("trait ");
        self.w.push_str(&interface.name.text);
        // Generic params follow without bounds — the trait doesn't
        // imply `Clone` itself; implementing types pick up bounds as
        // needed on their own impls.
        self.emit_generic_params(&interface.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        for method in &interface.methods {
            self.w.emit_indent();
            self.w.push_str("fn ");
            self.w.push_str(&method.name.text);
            self.emit_generic_params(&method.generic_params);
            self.w.push_str("(&self");
            for param in &method.params {
                self.w.push_str(", ");
                self.w.push_str(&param.name.text);
                self.w.push_str(": ");
                self.emit_type_as_rust(&param.ty);
            }
            self.w.push(')');
            match &method.return_type {
                ReturnType::Void => {}
                ReturnType::Type(t) => {
                    self.w.push_str(" -> ");
                    self.emit_return_type_as_rust(t);
                }
                ReturnType::AsyncType(_) => {
                    self.w.push_str(" -> ()");
                }
            }
            self.w.push_str(";\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// Emit a Jux record declaration as a Rust `pub struct` with the
    /// auto-derives that Java records guarantee — Debug/Clone for free
    /// use and `PartialEq` for record-equality. The auto-canonical
    /// constructor lives in an `impl` block as `pub fn new(...)`.
    ///
    /// **Position-aware String handling** mirrors classes: a `String`
    /// component lowers to an owned Rust `String` field, the
    /// constructor parameter is `&str`, and the field init injects
    /// `.to_string()`. Reads of String fields (and generic fields)
    /// auto-`.clone()` via the same machinery — so the user can write
    /// `print(v.x)` without thinking about ownership.
    ///
    /// **`Hash` and `Eq`** are intentionally not derived in Turn 1 —
    /// records carrying `f32`/`f64` components break both. A future
    /// pass can derive them conditionally per component types.
    pub(crate) fn emit_record_decl(&mut self, record_decl: &juxc_ast::RecordDecl) {
        // (Migrated to Writer indent-aware API)
        // Per `JUX-OPERATORS-ADDENDUM.md` §O.3.1 records auto-provide
        // `operator==`, `operator hash`, and copy-on-assignment when
        // their fields permit. The conditional derive list reflects
        // that: Debug/Clone/PartialEq are unconditional, and Eq, Hash,
        // and Copy are added when every component type qualifies.
        let component_tys: Vec<&juxc_ast::TypeRef> =
            record_decl.components.iter().map(|c| &c.ty).collect();
        self.w.line(&derive_attribute_for_value_type(&component_tys));

        // pub struct Name<T, U> { …components… }
        self.w.emit_indent();
        self.emit_visibility(record_decl.visibility);
        self.w.push_str("struct ");
        self.w.push_str(&record_decl.name.text);
        self.emit_generic_params(&record_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        for comp in &record_decl.components {
            self.w.emit_indent();
            // Records expose their components publicly — matching Java's
            // `record X(int a)` where `x.a()` is part of the API.
            // (Rust public fields are the simplest analog; auto-accessor
            // methods would be polish-only.)
            self.w.push_str("pub ");
            self.w.push_str(&comp.name.text);
            self.w.push_str(": ");
            self.emit_field_type_as_rust(&comp.ty);
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // impl[<T: Clone, U: Clone>] Name<T, U> { pub fn new(…) }
        self.w.emit_indent();
        self.w.push_str("impl");
        self.emit_generic_params_with_clone_bound(&record_decl.generic_params);
        self.w.push(' ');
        self.w.push_str(&record_decl.name.text);
        self.emit_generic_params_as_args(&record_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("pub fn new(");
        for (i, comp) in record_decl.components.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&comp.name.text);
            self.w.push_str(": ");
            // Ctor params follow normal type emission — Jux `String`
            // lowers to `&str` (cheap borrow). The field init below
            // injects `.to_string()` to convert into the owned field.
            self.emit_type_as_rust(&comp.ty);
        }
        self.w.push_str(") -> Self {\n");
        self.w.indent_inc();
        self.w.line("Self {");
        self.w.indent_inc();
        for comp in &record_decl.components {
            self.w.emit_indent();
            self.w.push_str(&comp.name.text);
            self.w.push_str(": ");
            self.w.push_str(&comp.name.text);
            // String-component coercion: the corresponding pre-pass
            // (`collect_record_string_component_names`) tracks which
            // components are String-typed; the field init writes the
            // `&str` parameter as `name.to_string()` to land it in the
            // owned `String` field.
            if crate::analysis::is_jux_string_type(&comp.ty) {
                self.w.push_str(".to_string()");
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // Auto-derived `operator string` per §O.3.1 — `"Point(x: 1.5, y: 2.7)"`.
        //
        // Skipped when:
        //   - the record is generic (we don't yet thread the
        //     `T: Display` bound through `emit_generic_params_*`), or
        //   - any component's type doesn't support Display (arrays,
        //     nullables, user-defined classes).
        // In those cases the record still gets `Debug` from the derive
        // line above, so `println!("{:?}", r)` keeps working.
        let display_ok = record_decl.generic_params.is_empty()
            && record_decl
                .components
                .iter()
                .all(|c| field_supports_display(&c.ty));
        if display_ok {
            self.emit_record_display_impl(record_decl);
        }
    }

    /// Generate the `impl std::fmt::Display for Name { … }` block for a
    /// record. Format mirrors §O.3.1's example: `"Name(field: value,
    /// other: value)"`. Empty-component records emit `"Name()"`.
    ///
    /// Called by [`Self::emit_record_decl`] only when
    /// [`field_supports_display`] returns true for every component AND
    /// the record has no generic parameters — keeps the emitted Rust
    /// guaranteed to compile.
    fn emit_record_display_impl(&mut self, record_decl: &juxc_ast::RecordDecl) {
        let name = &record_decl.name.text;
        // Build the format string and arg list in one pass — keeping
        // them in lockstep is important so the `{}` count matches the
        // arg count exactly.
        let mut fmt_body = format!("{name}(");
        let mut args = Vec::new();
        for (i, comp) in record_decl.components.iter().enumerate() {
            if i > 0 {
                fmt_body.push_str(", ");
            }
            fmt_body.push_str(&comp.name.text);
            fmt_body.push_str(": {}");
            args.push(format!("self.{}", comp.name.text));
        }
        fmt_body.push(')');

        self.w.emit_indent();
        self.w.push_str("impl std::fmt::Display for ");
        self.w.push_str(name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {");
        self.w.indent_inc();
        self.w.emit_indent();
        if args.is_empty() {
            // Zero-component record — just write the literal name.
            // `write!` accepts a no-arg format string.
            self.w.push_str(&format!("write!(f, \"{fmt_body}\")\n"));
        } else {
            self.w.push_str(&format!(
                "write!(f, \"{fmt_body}\", {})\n",
                args.join(", "),
            ));
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// Emit a Jux enum declaration as a Rust `pub enum` with auto-derives
    /// and a hand-written `Display` impl that mirrors Java's
    /// `enum.name()` (variant name only, no payload rendering).
    ///
    /// **Derives.** Per `JUX-OPERATORS-ADDENDUM.md` §O.3.3 sealed enums
    /// auto-provide `operator==`, `operator hash`, and copy-on-assign
    /// — all conditional on their payload types. The conditional
    /// derive list emits `Debug`, `Clone`, `PartialEq` unconditionally
    /// and adds `Eq`, `Hash`, `Copy` when every payload slot across
    /// every variant qualifies (no floats, no Strings for `Copy`, no
    /// user-defined types for any of the three).
    ///
    /// **Display.** Java's spec (§7.7.2) requires `operator string()` on
    /// every enum returning the variant's declared name. We implement
    /// `std::fmt::Display` by matching on `self` and writing the name
    /// — that makes `print(Color.Red)` and `$"…${color}…"` produce
    /// `Red` without surfacing the payload.
    ///
    /// **Variant emission.** Unit variants emit as bare identifiers
    /// (`Red,`); tuple-payload variants emit as `Red(Type, Type),`. We
    /// lower payload types through [`Self::emit_field_type_as_rust`] so
    /// Jux `String` payloads land as owned Rust `String`s.
    pub(crate) fn emit_enum_decl(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        // **Migrated to the indent-aware `Writer` API as a proof of
        // concept for Phase 2 of the backend-split work.** Indent depth
        // is now tracked by `self.w` (`indent_inc` / `indent_dec`)
        // rather than each call site recomputing how many spaces to
        // emit via a raw spacing helper. The emitted Rust is identical
        // — same line breaks, same content — but the source is
        // shorter, less error-prone (no off-by-one indent counts), and
        // serves as the template the remaining decl emitters follow.
        //
        // **Pattern.** `w.line(s)` writes `indent + s + '\n'` for whole
        // lines. `w.emit_indent()` + a sequence of `push_str` /
        // `push` calls assemble lines piecewise when the line is built
        // up from heterogenous pieces (visibility prefix, name, etc.).
        // `w.indent_inc()` after an opening `{`; matching
        // `w.indent_dec()` before the closing `}`. `w.newline()` for
        // bare blank lines between blocks.

        // `#[derive(...)] pub enum Name {`
        //
        // Aggregate eligibility across every variant's payload slots —
        // a single float (or class-typed) payload disqualifies the
        // whole enum from `Eq`/`Hash`, just as a single `String` payload
        // disqualifies `Copy`.
        let payload_tys: Vec<&juxc_ast::TypeRef> = enum_decl
            .variants
            .iter()
            .flat_map(|v| v.payload.iter().map(|p| &p.ty))
            .collect();
        self.w.line(&derive_attribute_for_value_type(&payload_tys));
        self.w.emit_indent();
        self.emit_visibility(enum_decl.visibility);
        self.w.push_str("enum ");
        self.w.push_str(&enum_decl.name.text);
        self.w.push_str(" {\n");

        self.w.indent_inc();
        for variant in &enum_decl.variants {
            self.w.emit_indent();
            self.w.push_str(&variant.name.text);
            if !variant.payload.is_empty() {
                self.w.push('(');
                for (i, slot) in variant.payload.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    // Payload slots act like class fields — owned
                    // values, so reuse the field-type mapping.
                    self.emit_field_type_as_rust(&slot.ty);
                }
                self.w.push(')');
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // `impl std::fmt::Display for Name { fn fmt(...) { match self { … } } }`
        self.w.emit_indent();
        self.w.push_str("impl std::fmt::Display for ");
        self.w.push_str(&enum_decl.name.text);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {");
        self.w.indent_inc();
        self.w.line("match self {");
        self.w.indent_inc();
        for variant in &enum_decl.variants {
            self.w.emit_indent();
            self.w.push_str(&enum_decl.name.text);
            self.w.push_str("::");
            self.w.push_str(&variant.name.text);
            // Wildcard the payload — we only render the variant name.
            if !variant.payload.is_empty() {
                self.w.push_str("(..)");
            }
            self.w.push_str(" => write!(f, \"");
            self.w.push_str(&variant.name.text);
            self.w.push_str("\"),\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// Emit a Jux class declaration as a Rust `pub struct` plus an
    /// `impl` block carrying its constructor and methods.
    ///
    /// **Constructor lowering.** A Jux constructor body runs statement-
    /// by-statement, doing `this.field = …` assignments. Rust struct
    /// literals require all fields up-front, so we synthesize a builder
    /// pattern:
    ///
    /// ```text
    /// pub fn new(x: isize) -> Self {
    ///     let mut __self = Self { x: 0, y: 0 };  // defaults
    ///     __self.x = x;                          // body, with this→__self
    ///     __self
    /// }
    /// ```
    ///
    /// `this` in the constructor body rewrites to `__self`. Inside
    /// instance methods it rewrites to `self`. This rewrite is controlled
    /// by the [`Self::this_alias`] field, which we set for the duration
    /// of each constructor/method emission.
    ///
    /// **Receiver kind for methods.** Methods that assign to `self.field`
    /// need `&mut self`; otherwise `&self`. We re-run `collect_mutated_names`
    /// over the body and look for the `__this__` sentinel that
    /// `emit_assign` produces for `this.field = …` patterns. Plain locals
    /// in the body still drive `let mut` promotion as before.
    pub(crate) fn emit_class_decl(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // (Migrated to Writer indent-aware API)
        // Derive Clone unconditionally so the `T: Clone` bound used on
        // generic impls (and the auto-`.clone()` injected on field
        // reads) keeps working when the user nests classes — `Box<User>`
        // needs `User: Clone`, which falls out for free here.
        self.w.line("#[derive(Clone)]");
        // pub struct Name<T, U> { …fields… }
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("struct ");
        self.w.push_str(&class_decl.name.text);
        self.emit_generic_params(&class_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        // Inheritance: embed the parent struct as `__parent`. Field
        // access on the child auto-dereffs through `impl Deref<Target=Parent>`
        // (emitted below), so `child.parent_field` and inherited
        // method calls Just Work. Always emit `__parent` first so the
        // struct layout is consistent across the hierarchy.
        if let Some(parent_ty) = &class_decl.extends {
            self.w.emit_indent();
            self.w.push_str("__parent: ");
            self.emit_type_as_rust(parent_ty);
            self.w.push_str(",\n");
        }
        for field in &class_decl.fields {
            self.w.emit_indent();
            self.emit_visibility(field.visibility);
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            // Field-position type mapping (String → owned `String`).
            self.emit_field_type_as_rust(&field.ty);
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // Emit Deref + DerefMut impls for child classes so inherited
        // methods and field access flow through Rust's auto-deref —
        // `child.method()` finds methods on the parent transparently,
        // `child.parent_field = x` works via DerefMut, etc.
        if let Some(parent_ty) = &class_decl.extends {
            // impl Deref for Child { type Target = Parent; … }
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params(&class_decl.generic_params);
            self.w.push_str(" std::ops::Deref for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            self.w.emit_indent();
            self.w.push_str("type Target = ");
            self.emit_type_as_rust(parent_ty);
            self.w.push_str(";\n");
            self.w.line("fn deref(&self) -> &Self::Target { &self.__parent }");
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
            // impl DerefMut for Child { … }
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params(&class_decl.generic_params);
            self.w.push_str(" std::ops::DerefMut for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            self.w.line("fn deref_mut(&mut self) -> &mut Self::Target { &mut self.__parent }");
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
        }

        // impl[<T: Clone, U: Clone>] Name<T, U> { …members… }
        //
        // For generic classes we emit a `T: Clone` bound on every type
        // parameter (Phase-1 simplification). Reads of generic-typed
        // fields call `.clone()`, so they need the bound. Every Jux
        // primitive and emitted class/enum is `Clone`, so the
        // constraint never blocks a real Jux program. Replace with
        // proper user-declared bounds once `<T extends Animal>` lands.
        self.w.emit_indent();
        self.w.push_str("impl");
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push(' ');
        self.w.push_str(&class_decl.name.text);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {\n");

        // Constructor → `pub fn new(args) -> Self` with the __self pattern.
        for ctor in &class_decl.constructors {
            self.emit_constructor(class_decl, ctor);
        }
        // If no constructor was declared, synthesize an implicit zero-
        // arg `new()` per §7.3.1 (declaring any constructor removes it).
        if class_decl.constructors.is_empty() {
            self.emit_synthetic_default_constructor(class_decl);
        }
        for method in &class_decl.methods {
            self.emit_method(class_decl, method);
        }
        self.w.line("}");
        self.w.newline();

        // For each `implements I`, emit a trait-impl block that
        // **delegates** to the inherent methods on this class. The
        // inherent methods own the canonical bodies; the trait impl
        // forwards every call. This keeps emitted code DRY and lets
        // trait-bound contexts (`<T: I>`) dispatch through `I` while
        // direct `c.method()` calls still hit the inherent path.
        self.emit_class_trait_impls(class_decl);

        // Marker trait — `pub trait <Name>Kind: Clone {}` — and impls
        // for this class plus every ancestor in the chain. Lets
        // `<T extends ClassName>` bounds work for type restriction
        // (the bound rewriter in `emit_generic_params_with_clone_bound`
        // routes class-name bounds through `<Name>Kind`).
        self.emit_class_marker_trait(class_decl);
    }

    /// Emit a class's marker trait and the transitive marker impls
    /// covering its parent chain. Marker traits are empty (`{}`) —
    /// they exist purely to let generic bounds reference Jux classes
    /// in a way Rust's type system accepts. The user can't call
    /// class-defined methods on a marker-bounded `T` (Phase-1
    /// limitation); to expose methods, combine the class bound with an
    /// interface that re-declares them.
    pub(crate) fn emit_class_marker_trait(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // (Migrated to Writer indent-aware API)
        // pub trait <Name>Kind: Clone {}
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("trait ");
        self.w.push_str(&class_decl.name.text);
        self.w.push_str("Kind: Clone {}\n");
        // impl<T: Clone, …> <Name>Kind for <Name><T, …> {}
        //
        // The class's own generic params (with their full bound list)
        // travel onto the marker impl so `<T: Clone>`-style traits
        // satisfy when the class is generic. Without this, an
        // `impl BoxKind for Box<T>` would fail to derive Clone because
        // `Box<T>` only Clones when `T: Clone`.
        self.w.emit_indent();
        self.w.push_str("impl");
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push(' ');
        self.w.push_str(&class_decl.name.text);
        self.w.push_str("Kind for ");
        self.w.push_str(&class_decl.name.text);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {}\n");

        // Walk the ancestor chain and emit one transitive impl per
        // ancestor. So `class Spaniel extends Dog extends Animal`
        // emits `impl DogKind for Spaniel {}` AND
        // `impl AnimalKind for Spaniel {}`.
        let mut ancestor = class_decl.extends.clone();
        while let Some(parent_ty) = ancestor {
            let parent_name = parent_ty
                .name
                .segments
                .first()
                .map(|s| s.text.clone());
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
            self.w.push(' ');
            self.emit_type_as_rust(&parent_ty);
            self.w.push_str("Kind for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {}\n");
            // Step up the chain through tycheck's symbol table. Clone
            // the optional TypeRef out of the ClassSig so we don't
            // hold a borrow on `self.symbols` across the next loop
            // iteration (which calls `self.emit_*` mutably).
            ancestor = parent_name
                .and_then(|n| self.symbols.classes.get(&n))
                .and_then(|c| c.extends.clone());
        }
        self.w.newline();
    }

    /// Emit one `impl Interface for Class { … delegating methods … }`
    /// block per interface listed in the class's `implements` clause.
    ///
    /// Each method's **declared signature** comes from the interface's
    /// [`MethodSig`] in tycheck's [`SymbolTable`] (Phase G), not from
    /// the class — that way the trait impl's signature matches the
    /// trait declaration exactly, even when the class has incidental
    /// extra methods or differing param names. The body is always a
    /// delegating `self.method(args)` call.
    ///
    /// **Iteration order.** The symbol table stores methods in a
    /// `HashMap<String, MethodSig>` keyed by name. The map has no
    /// inherent order, so we sort the (name, sig) entries
    /// alphabetically before emission. Trait impls don't care about
    /// method order (Rust resolves by name), and a deterministic sort
    /// keeps the emitted output stable across runs.
    pub(crate) fn emit_class_trait_impls(&mut self, class_decl: &juxc_ast::ClassDecl) {
        if class_decl.implements.is_empty() {
            return;
        }
        // Clone the per-interface signature lists upfront so we can
        // mutably-call `self.emit_*` inside the loop without fighting
        // the borrow checker.
        let interfaces: Vec<juxc_ast::TypeRef> = class_decl.implements.clone();
        for interface_ty in &interfaces {
            // Interface name must be a single-segment path today —
            // imports and module-qualified interfaces are a future
            // extension.
            let Some(iface_name) = interface_ty.name.segments.first() else {
                continue;
            };
            // Pull (name, MethodSig) pairs from the symbol table and
            // sort by name for deterministic emission order. Empty
            // when the interface isn't in the table (e.g. unresolved
            // name — tycheck would have already flagged that).
            let mut methods: Vec<(String, MethodSig)> = self
                .symbols
                .interfaces
                .get(iface_name.text.as_str())
                .map(|sig| {
                    sig.methods
                        .iter()
                        .map(|(name, m)| (name.clone(), m.clone()))
                        .collect()
                })
                .unwrap_or_default();
            if methods.is_empty() {
                continue;
            }
            methods.sort_by(|a, b| a.0.cmp(&b.0));
            self.w.emit_indent();
            self.w.push_str("impl");
            // The class's own generic params (with the Clone bound)
            // travel onto the trait impl too — `impl<T: Clone>
            // Interface for Box<T>`.
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
            self.w.push(' ');
            self.emit_type_as_rust(interface_ty);
            self.w.push_str(" for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            for (method_name, method) in &methods {
                self.w.emit_indent();
                self.w.push_str("fn ");
                self.w.push_str(method_name);
                // Match the interface's declared receiver: always
                // `&self` in Turn 1. Implementing classes must also be
                // non-mutating for the delegation to type-check.
                self.w.push_str("(&self");
                // `MethodSig.params` is `Vec<ParamSig>`; ParamSig
                // carries `name: String` (not `Ident`) and `ty: TypeRef`.
                for param in &method.params {
                    self.w.push_str(", ");
                    self.w.push_str(&param.name);
                    self.w.push_str(": ");
                    self.emit_type_as_rust(&param.ty);
                }
                self.w.push(')');
                match &method.return_type {
                    ReturnType::Void => {}
                    ReturnType::Type(t) => {
                        self.w.push_str(" -> ");
                        self.emit_return_type_as_rust(t);
                    }
                    ReturnType::AsyncType(_) => {
                        self.w.push_str(" -> ()");
                    }
                }
                // Delegating body — `self.method(params)` forwards to
                // the inherent impl. Void methods drop the value; the
                // unit-return body still compiles for them.
                self.w.push_str(" {\n");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str("self.");
                self.w.push_str(method_name);
                self.w.push('(');
                for (i, param) in method.params.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.w.push_str(&param.name);
                }
                self.w.push_str(")\n");
                self.w.indent_dec();
                self.w.line("}");
            }
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
        }
    }

    pub(crate) fn emit_constructor(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        ctor: &juxc_ast::ConstructorDecl,
    ) {
        // (Migrated to Writer indent-aware API)
        // Caller (`emit_class_decl`) is at level 0; the ctor signature
        // sits at depth 1 (inside the `impl` block), and the body at
        // depth 2.
        self.w.indent_inc();
        self.w.emit_indent();
        self.emit_visibility(ctor.visibility);
        self.w.push_str("fn new(");
        for (i, param) in ctor.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_type_as_rust(&param.ty);
        }
        self.w.push_str(") -> Self {\n");
        self.w.indent_inc();

        // Try the **simple-ctor fast path** first: when every statement
        // in the body is `this.field = expr;` (with an optional leading
        // `super(args);`), collapse to a direct `Self { field: expr, … }`
        // literal. Idiomatic Rust, AND it sidesteps the "need `Default`
        // for generic-typed fields" problem inherent to the fallback
        // `__self`-builder pattern.
        if let Some(simple) = extract_simple_ctor_inits(ctor) {
            self.emit_simple_ctor_body(class_decl, &simple);
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
            self.w.indent_dec();
            return;
        }

        // Fallback: the body has stmts other than this.field-init (e.g.,
        // a `print(…)` mixed in). Use the `__self` builder pattern,
        // which requires fields without explicit init to be
        // `Default`-initialized — fine for primitives, breaks for
        // unconstrained generic types. The user has to keep the ctor
        // body simple in that case.
        self.w.line("let mut __self = Self {");
        self.w.indent_inc();
        for field in &class_decl.fields {
            self.w.emit_indent();
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            if let Some(default) = &field.default {
                self.emit_expr(default);
            } else {
                self.emit_field_default_value_for(&field.ty);
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("};");

        // Body — `this` rewrites to `__self`.
        self.this_alias = Some("__self".to_string());
        let mut muts = HashSet::new();
        collect_mutated_names(&ctor.body, &mut muts, &self.user_mut_methods);
        self.mutated_in_fn = muts;
        for stmt in &ctor.body.statements {
            self.w.emit_indent();
            self.emit_stmt(stmt);
        }
        self.this_alias = None;

        // Return the constructed value.
        self.w.line("__self");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Emit the direct `Self { field: expr, … }` body for a simple
    /// constructor — one whose body is purely `this.field = expr;`
    /// lines. `inits` carries one `(field-name, init-expr)` entry per
    /// statement in source order; if the same field is assigned more
    /// than once, the **last** assignment wins (matching Java semantics
    /// for a sequence of plain assignments).
    pub(crate) fn emit_simple_ctor_body(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        simple: &SimpleCtorInits,
    ) {
        // (Migrated to Writer indent-aware API)
        // Caller (`emit_constructor`) has the writer at level 2 — the
        // depth of statements inside `pub fn new(...) -> Self { … }`.
        // The `Self { … }` literal body sits one deeper at level 3.
        // Resolve field-name → init-expr, last assignment wins.
        let mut chosen: std::collections::HashMap<&str, &juxc_ast::Expr> =
            std::collections::HashMap::new();
        for (name, expr) in &simple.inits {
            chosen.insert(name.as_str(), expr);
        }

        self.w.line("Self {");
        self.w.indent_inc();
        // Inherited parent — emit the `__parent` slot first, before
        // the class's own fields, matching the struct declaration's
        // field order.
        if let Some(parent_ty) = &class_decl.extends {
            self.w.emit_indent();
            self.w.push_str("__parent: ");
            self.emit_type_as_rust(parent_ty);
            self.w.push_str("::new(");
            // If the constructor wrote `super(args);`, lift those args
            // here. If it didn't, Phase 1 calls `Parent::new()` with
            // no arguments — fine for parameterless parents, breaks
            // (with a clear Rust error) when the parent's ctor needs
            // arguments and the user forgot to write `super(...)`.
            if let Some(args) = &simple.super_args {
                // Clone to release the borrow on `simple` before the
                // `emit_expr` calls (which need `&mut self`).
                let args = args.clone();
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
            }
            self.w.push_str("),\n");
        }
        for field in &class_decl.fields {
            self.w.emit_indent();
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            if let Some(init_expr) = chosen.get(field.name.text.as_str()) {
                // Field assigned in body — emit its init expression.
                // Inline the String-field coercion that `emit_assign`
                // would have added: if the field is a known String
                // field, append `.to_string()` so `&str` arguments
                // become owned `String`s.
                //
                // Phase H: source the String-ness decision from the
                // class field's declared `TypeRef` directly rather
                // than from the retired `string_field_names` pre-pass.
                // The result is identical for well-formed input but no
                // longer mis-fires when an unrelated class shares the
                // same field name.
                self.emit_expr(init_expr);
                if is_jux_string_type_ref(&field.ty) {
                    self.w.push_str(".to_string()");
                }
            } else if let Some(default) = &field.default {
                // Field carries a Jux-source default initializer.
                self.emit_expr(default);
            } else {
                // No assignment and no source default — fall back to
                // the type's natural default. Generic-typed fields
                // will surface a Rust compile error here, signaling
                // the user has to assign them in the constructor body.
                self.emit_field_default_value_for(&field.ty);
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
    }

    /// Synthesize a zero-argument default constructor when the class
    /// declared none — per §7.3.1's "implicit zero-arg constructor".
    pub(crate) fn emit_synthetic_default_constructor(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // (Migrated to Writer indent-aware API)
        // Caller (`emit_class_decl`) is at level 0; synth ctor sits at
        // depth 1 inside the `impl` block.
        self.w.indent_inc();
        self.w.line("pub fn new() -> Self {");
        self.w.indent_inc();
        self.w.line("Self {");
        self.w.indent_inc();
        // Inherited parent — invoke the parent's zero-arg constructor.
        // For parents whose ctor takes arguments, the user MUST declare
        // an explicit constructor with `super(args);`; the synthetic
        // path is only valid for trivially-defaulted hierarchies.
        if let Some(parent_ty) = &class_decl.extends {
            self.w.emit_indent();
            self.w.push_str("__parent: ");
            self.emit_type_as_rust(parent_ty);
            self.w.push_str("::new(),\n");
        }
        for field in &class_decl.fields {
            self.w.emit_indent();
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            if let Some(default) = &field.default {
                self.emit_expr(default);
            } else {
                self.emit_field_default_value_for(&field.ty);
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    pub(crate) fn emit_method(&mut self, _class_decl: &juxc_ast::ClassDecl, method: &FnDecl) {
        // (Migrated to Writer indent-aware API)
        // Caller (`emit_class_decl`) is at level 0; method signature
        // sits at depth 1 (inside the `impl` block), body at depth 2.
        // Walk the body once to decide on `&self` vs `&mut self`. A
        // method that contains `this.field = …` (lvalue base is `this`)
        // needs a mutable receiver in Rust. The lvalue walker we use
        // for locals also recognizes `Expr::This` as a root.
        let body = method.body.as_ref();
        let needs_mut_self = body
            .map(|b| body_writes_to_this(b))
            .unwrap_or(false);

        self.w.indent_inc();
        self.w.emit_indent();
        self.emit_visibility(method.visibility);
        self.w.push_str("fn ");
        self.w.push_str(&method.name.text);
        // Method's own generic parameters (rare but supported).
        self.emit_generic_params(&method.generic_params);
        self.w.push('(');
        if needs_mut_self {
            self.w.push_str("&mut self");
        } else {
            self.w.push_str("&self");
        }
        for param in &method.params {
            self.w.push_str(", ");
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_type_as_rust(&param.ty);
        }
        self.w.push(')');
        match &method.return_type {
            ReturnType::Void => {}
            ReturnType::Type(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
            ReturnType::AsyncType(_) => {
                self.w.push_str(" -> ()");
            }
        }
        self.w.push_str(" {\n");
        // Body sits at depth 2 — push one more level so
        // `emit_fn_body_at` sees the writer at the body depth.
        self.w.indent_inc();
        if let Some(body) = body {
            self.this_alias = Some("self".to_string());
            let mut muts = HashSet::new();
            collect_mutated_names(body, &mut muts, &self.user_mut_methods);
            self.mutated_in_fn = muts;
            self.emit_fn_body_at(body, &method.return_type);
            self.this_alias = None;
        } else {
            // Abstract method — no Jux body. Emit `unimplemented!()`
            // so the Rust compiler accepts the function and any
            // accidental call against the base class itself panics
            // clearly. Subclass overrides shadow this body via Rust's
            // inherent-method-shadowing-via-Deref behavior.
            self.w.emit_indent();
            self.w.push_str("unimplemented!(\"abstract method ");
            self.w.push_str(&method.name.text);
            self.w.push_str("\")\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Emit a Rust `fn` for a Jux function declaration.
    ///
    /// Visibility is intentionally dropped — every emitted function is
    /// crate-private. Inheritance and trait dispatch don't exist in this
    /// milestone, so there's nothing for visibility to mediate.
    pub(crate) fn emit_fn_decl(&mut self, fn_decl: &FnDecl) {
        // (Migrated to Writer indent-aware API)
        // Caller is at level 0 — top-level functions sit at depth 0,
        // body at depth 1.
        // `fn name<T, U>(params) -> return {`
        self.w.emit_indent();
        self.w.push_str("fn ");
        self.w.push_str(&fn_decl.name.text);
        self.emit_generic_params(&fn_decl.generic_params);
        self.w.push('(');
        for (i, param) in fn_decl.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_type_as_rust(&param.ty);
        }
        self.w.push(')');

        match &fn_decl.return_type {
            ReturnType::Void => {} // `void` → omit return arrow
            ReturnType::Type(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
            ReturnType::AsyncType(_) => {
                // TODO: async lowering — needs a real runtime story per §15.
                // Placeholder: emit `()` so the resulting Rust at least
                // parses. (No Jux program in flight actually uses this.)
                self.w.push_str(" -> ()");
            }
        }

        self.w.push_str(" {\n");
        // Body sits at depth 1 — push one level for `emit_fn_body`.
        self.w.indent_inc();
        if let Some(body) = &fn_decl.body {
            // Per-function mutation pass: figure out which locals get
            // reassigned anywhere in this body. The result drives the
            // `let` vs `let mut` choice in emit_var_decl.
            let mut muts = HashSet::new();
            collect_mutated_names(body, &mut muts, &self.user_mut_methods);
            self.mutated_in_fn = muts;
            self.emit_fn_body(body, &fn_decl.return_type);
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// Emit a function's body block with **trailing-return elision** —
    /// the cosmetic rule that makes our output match idiomatic Rust:
    ///
    /// - A non-void function ending in `return expr;` emits `expr` as a
    ///   bare tail expression (no `return` keyword, no `;`). This is the
    ///   form a Rust developer would write — `fn add(a: i32, b: i32) -> i32 { a + b }`.
    /// - A `void` function ending in `return;` drops the statement
    ///   entirely (Rust returns `()` implicitly from a `{}` body).
    /// - Mid-function `return` statements stay as `return expr;` — early
    ///   returns are common and explicit `return` reads better there
    ///   than a labeled break.
    ///
    /// The pre-tail statements are emitted normally through
    /// [`Self::emit_stmt`]. This keeps `if`/`while`/`loop` bodies as
    /// regular statement blocks, so any `return` inside them stays
    /// statement-form (which is correct — those returns are early
    /// exits, not the function's value).
    pub(crate) fn emit_fn_body(&mut self, body: &Block, return_type: &ReturnType) {
        self.emit_fn_body_at(body, return_type);
    }

    /// Same as [`Self::emit_fn_body`] — kept as a separate entry point
    /// for historical reasons; both names land here. Callers
    /// (`emit_fn_decl`, `emit_method`) must have called
    /// `self.w.indent_inc()` to position the writer at the body depth
    /// before invoking.
    pub(crate) fn emit_fn_body_at(&mut self, body: &Block, return_type: &ReturnType) {
        // (Migrated to Writer indent-aware API)
        // Callers have set the writer's indent level to the body depth
        // before invoking. Body content emits via `self.w.emit_indent()`
        // (statements) or via `emit_tail_stmt` (the elided trailing
        // return).
        let elide_tail = matches!(
            (body.statements.last(), return_type),
            // Non-void function with explicit trailing `return expr;`.
            (Some(Stmt::Return(Some(_))), _)
            // Void function ending with a bare `return;` — equivalent
            // to "fall off the end," which Rust does for free.
            | (Some(Stmt::Return(None)), ReturnType::Void)
        );

        let last_idx = body.statements.len().saturating_sub(1);
        for (i, stmt) in body.statements.iter().enumerate() {
            if elide_tail && i == last_idx {
                self.emit_tail_stmt(stmt);
            } else {
                self.w.emit_indent();
                self.emit_stmt(stmt);
            }
        }
    }

    /// Emit the *tail* statement of a function body — the one targeted
    /// by trailing-return elision. The caller guarantees this is a
    /// `Return` statement, and that elision applies (so we know what to
    /// drop). The writer's current `indent_level` is the body depth, so
    /// `emit_indent()` produces the right leading whitespace.
    pub(crate) fn emit_tail_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return(Some(expr)) => {
                // `return expr;` → bare `expr` on its own line.
                self.w.emit_indent();
                self.emit_expr(expr);
                self.w.push('\n');
            }
            Stmt::Return(None) => {
                // Void tail `return;` — drop entirely. Nothing to emit.
            }
            _ => unreachable!("emit_tail_stmt called on non-Return stmt"),
        }
    }
}
