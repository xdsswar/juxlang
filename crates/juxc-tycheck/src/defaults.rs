//! Which types have a **default value** (JUX-LANG-V1 §5.5, "What a new array
//! holds").
//!
//! `new T[n]` has to put something in each of its `n` elements before the
//! program writes one, and a Rust `vec![Default::default(); n]` asks the
//! element type for it. Tycheck uses this module to reject an element type
//! that has no default (`E0458`), and the backend uses the SAME answer to
//! decide which records derive `Default`, so the two can never disagree: a
//! type the checker accepts is a type the emitted Rust can build.
//!
//! The rule, in the spec's words:
//!
//! - numbers, `bool`, `char` and `String` have one (zero, `false`, `'\0'`, `""`);
//! - a nullable `T?`, a raw pointer `T*` and a function pointer default to `null`;
//! - a runtime-sized array `T[]` defaults to an empty array, and a fixed one
//!   `T[N]` to `N` defaults of `T`;
//! - an enum defaults to its first variant that carries no payload;
//! - a record defaults to every component at its own default;
//! - a `@layout(c)` struct defaults to every field at its initializer, or at
//!   its own default when it has none;
//! - a class, an ordinary struct (a reference type, like a class), an
//!   interface and a function type have none.

use juxc_ast::{ArrayDim, TypeRef};

use crate::symbol_table::SymbolTable;
use crate::ty::{lower_member_type, Ty};

/// Rust implements `Default` for `[T; N]` only up to `N = 32`, so a fixed
/// array longer than that has no derivable default in the emitted Rust.
const LARGEST_DEFAULT_FIXED_ARRAY: i64 = 32;

/// Whether a value of `ty` has a default value.
///
/// A type parameter counts as having one: the lowering adds the `Default`
/// bound where an array of it is built (§T.2.1). An unresolved type does too,
/// so an earlier error is not followed by a second one about the same name.
pub fn ty_has_default(ty: &Ty, symbols: &SymbolTable) -> bool {
    ty_has_default_in(ty, symbols, &mut Vec::new())
}

/// Whether a member declared as `ty_ref` inside the type `owner` (an FQN) has
/// a default value. Names in `ty_ref` resolve the way they do in `owner`'s own
/// declaration, and a raw pointer, which `Ty` does not represent, is `null`.
pub fn member_has_default(ty_ref: &TypeRef, owner: &str, symbols: &SymbolTable) -> bool {
    member_has_default_in(ty_ref, owner, symbols, &mut Vec::new())
}

fn member_has_default_in(
    ty_ref: &TypeRef,
    owner: &str,
    symbols: &SymbolTable,
    visiting: &mut Vec<String>,
) -> bool {
    if ty_ref.ptr_depth > 0 || ty_ref.nullable || ty_ref.fn_pointer_shape().is_some() {
        return true;
    }
    if ty_ref.fn_shape.is_some() {
        return false;
    }
    if let Some(shape) = &ty_ref.array_shape {
        return match shape.outer() {
            // An empty array, whatever it would hold.
            ArrayDim::Dynamic => true,
            ArrayDim::Fixed(len) => {
                let fits = matches!(
                    len.as_ref(),
                    juxc_ast::Expr::Literal(juxc_ast::Literal::Int(lit))
                        if lit.value <= LARGEST_DEFAULT_FIXED_ARRAY
                );
                let element = TypeRef { array_shape: shape.peeled(), ..ty_ref.clone() };
                fits && member_has_default_in(&element, owner, symbols, visiting)
            }
        };
    }
    ty_has_default_in(&lower_member_type(ty_ref, owner, symbols), symbols, visiting)
}

fn ty_has_default_in(ty: &Ty, symbols: &SymbolTable, visiting: &mut Vec<String>) -> bool {
    match ty {
        Ty::Primitive(_)
        | Ty::String
        | Ty::Nullable(_)
        | Ty::FnPtr { .. }
        | Ty::Param(_)
        | Ty::Unknown => true,
        Ty::Array { element, kind } => match kind {
            crate::ty::ArrayKind::Dynamic => true,
            crate::ty::ArrayKind::Fixed => ty_has_default_in(element, symbols, visiting),
        },
        Ty::Fn { .. } | Ty::Void | Ty::Wildcard(_) => false,
        Ty::User { name, .. } => {
            // A type that reaches itself without a nullable in between has no
            // finite value to start from, default or otherwise.
            if visiting.iter().any(|seen| seen == name) {
                return false;
            }
            visiting.push(name.clone());
            let has = user_type_has_default(name, symbols, visiting);
            visiting.pop();
            has
        }
    }
}

fn user_type_has_default(name: &str, symbols: &SymbolTable, visiting: &mut Vec<String>) -> bool {
    if let Some(class) = symbols.classes.get(name) {
        // A foreign type's traits are not visible from here; do not report
        // what cannot be known.
        if class.is_external {
            return true;
        }
        // A value struct (E20) has a default when every field does; a class is
        // a reference and has none.
        return class.is_struct
            && class
                .fields
                .values()
                .filter(|f| !f.is_static)
                .all(|f| f.default.is_some() || member_has_default_in(&f.ty, name, symbols, visiting));
    }
    if let Some(record) = symbols.records.get(name) {
        return record
            .components
            .iter()
            .all(|c| member_has_default_in(&c.ty, name, symbols, visiting));
    }
    if let Some(enum_sig) = symbols.enums.get(name) {
        return enum_sig.is_external || enum_sig.variants.values().any(|v| v.payload.is_empty());
    }
    // Interfaces, and anything else a `User` names.
    false
}
