//! Anonymous classes lifted to named ones (ERRATA E137).
//!
//! An anonymous class is a Rust item local to the expression that builds it,
//! so nothing outside that expression can name it. Two things need to: the
//! dispatch of a method with type parameters of its own through a supertype
//! (`crate::generic_dispatch`), which branches on the concrete type behind a
//! value by name, and the `Kind` trait of a class that an anonymous class
//! extends, which the anonymous class has to implement like any subclass.
//!
//! So such an anonymous class is lifted: the checker records, for each one,
//! what a named class standing in for it needs (the enclosing type parameters
//! it uses, the enclosing locals it captures and their types, the types of
//! the arguments its superclass constructor takes), and the driver rewrites
//! the program to declare that class and construct it where the anonymous
//! class was, then checks the program again. The class is
//! `__JuxAnon_<Target>_<source>_<n>`, and prints as `Target$anon`, as the
//! anonymous object did.

use juxc_source::Span;

/// One anonymous class to lift.
#[derive(Debug, Clone)]
pub struct AnonLift {
    /// Span of the `new T(..) { .. }` expression.
    pub span: Span,
    /// Source (unit) index; stamped by `typecheck_workspace`.
    pub unit: usize,
    /// `true` when `T` is a class (the lifted class `extends` it), `false`
    /// for an interface (`implements`).
    pub target_is_class: bool,
    /// `T` with its type arguments, as written (`Visitor<int>`).
    pub target_text: String,
    /// `T`'s simple name, for the lifted class's name.
    pub target_bare: String,
    /// The enclosing type parameters the class uses, with their bounds
    /// written out (`T extends Named & Aged`).
    pub generics: Vec<(String, String)>,
    /// The enclosing locals and parameters it reads, with their types.
    pub captures: Vec<(String, String)>,
    /// The types of the arguments given to `T`'s constructor.
    pub super_arg_types: Vec<String>,
}

/// Whether a type's written form can be declared on a lifted class: no
/// unknown or compiler-internal type in it.
pub(crate) fn declarable(text: &str) -> bool {
    !text.contains("<unknown>") && !text.contains("__jux") && !text.contains("fn(") && !text.is_empty()
}
