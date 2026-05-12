//! Type-position emission — primitive and composite `TypeRef` lowering,
//! generic-parameter emission, default-value selection per type,
//! visibility keyword emission.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::ArrayShape;

use crate::analysis::is_jux_string_type;
use crate::RustEmitter;

impl RustEmitter {
    /// Map a Jux [`TypeRef`] onto its Rust spelling.
    ///
    /// Full primitive mapping table per `JUX-LANG-V1.md` §5.1:
    ///
    /// | Jux       | Rust   | Notes |
    /// |-----------|--------|-------|
    /// | `bool`    | `bool` | Direct. |
    /// | `byte`    | `i8`   | 8-bit signed. |
    /// | `ubyte`   | `u8`   | 8-bit unsigned. |
    /// | `short`   | `i16`  | 16-bit signed. |
    /// | `ushort`  | `u16`  | 16-bit unsigned. |
    /// | `int`     | `i32`  | 32-bit signed. Matches Java's `int`. |
    /// | `uint`    | `u32`  | 32-bit unsigned. |
    /// | `long`    | `i64`  | 64-bit signed. |
    /// | `ulong`   | `u64`  | 64-bit unsigned. |
    /// | `float`   | `f32`  | IEEE 754 single. |
    /// | `double`  | `f64`  | IEEE 754 double. |
    /// | `char`    | `char` | Unicode scalar. Rust's `char` is 32-bit; matches. |
    /// | `String`  | `&str` | Borrowed slice — see note below. |
    ///
    /// Anything else is emitted verbatim as a `::`-joined path, on faith
    /// that the surrounding project will provide it. When a real type
    /// table lands this becomes a proper lookup.
    ///
    /// Restrictions for the current pass:
    /// - **Generic args** (`List<String>`) and **nullability** (`Foo?`)
    ///   are ignored — they fall through to verbatim path emission. They
    ///   join the table when the type system carries them through.
    ///
    /// **`String` → `&str`:** Java's `String` is immutable and reference-
    /// shaped, which matches Rust's `&str` more naturally than `String`.
    /// Borrowed parameters mean callers can pass string literals without
    /// allocating. Owned-string semantics (mutation, storage in structs)
    /// will need a more nuanced mapping when we get there.
    pub(crate) fn emit_type_as_rust(&mut self, ty: &juxc_ast::TypeRef) {
        // Array types lower to Rust `[ElementType; N]` for fixed-size
        // (Turn 1) or `Vec<ElementType>` for dynamic (Turn 2, deferred).
        if let Some(shape) = &ty.array_shape {
            match shape {
                ArrayShape::Fixed(size) => {
                    // `[ElementType; size]`
                    self.w.push('[');
                    // Recurse with a copy of `ty` minus the array shape
                    // so we emit just the element type.
                    let element_ty = juxc_ast::TypeRef {
                        name: ty.name.clone(),
                        generic_args: ty.generic_args.clone(),
                        nullable: ty.nullable,
                        array_shape: None,
                        span: ty.span,
                    };
                    self.emit_type_as_rust(&element_ty);
                    self.w.push_str("; ");
                    self.emit_expr(size);
                    self.w.push(']');
                }
                ArrayShape::Dynamic => {
                    // `T[]` — runtime-sized array. We pick `Vec<T>` as
                    // the lowering: owned, heap-backed, `.len()` works,
                    // indexable. Trades stack-allocation off (vs Turn-1
                    // `[T; N]`) for size-at-runtime. Future work: when
                    // a function param has `T[]` type, lower to slice
                    // (`&[T]`) instead — needs lifetime threading.
                    self.w.push_str("Vec<");
                    let element_ty = juxc_ast::TypeRef {
                        name: ty.name.clone(),
                        generic_args: ty.generic_args.clone(),
                        nullable: ty.nullable,
                        array_shape: None,
                        span: ty.span,
                    };
                    self.emit_type_as_rust(&element_ty);
                    self.w.push('>');
                }
            }
            return;
        }

        if let Some(rust_ty) = jux_primitive_to_rust(ty) {
            self.w.push_str(rust_ty);
            return;
        }
        // Fall back to a verbatim path emission. Generic args and
        // nullability aren't surfaced yet.
        let path = ty
            .name
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("::");
        self.w.push_str(&path);
        // Emit any generic args attached to this type — `Box<int>`
        // lowers to `Box<isize>` after the path. Recursive: each arg
        // goes through `emit_type_as_rust` so nested generics also map.
        if !ty.generic_args.is_empty() {
            self.w.push('<');
            for (i, arg) in ty.generic_args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                // Inside a generic-arg slot a Jux `String` has to
                // lower to an owned `String`, not `&str` — a stored
                // `T` field can't carry an elided lifetime. The
                // top-level position mapping still uses `&str` for
                // ergonomic param/local positions.
                self.emit_generic_arg_type_as_rust(arg);
            }
            self.w.push('>');
        }
    }

    /// Lower a type that appears as a generic argument (e.g. the
    /// `String` inside `Container<String>`). The only difference
    /// from [`Self::emit_type_as_rust`] is the `String` case, which
    /// gets the owned `String` lowering — `&str` won't work as a
    /// generic-arg slot without an explicit lifetime.
    pub(crate) fn emit_generic_arg_type_as_rust(&mut self, ty: &juxc_ast::TypeRef) {
        if is_jux_string_type(ty) {
            self.w.push_str("String");
            return;
        }
        self.emit_type_as_rust(ty);
    }

    /// Emit a generic-parameter list as a declaration site — `<T, U>`.
    /// No-op when `params` is empty (the common, non-generic case).
    pub(crate) fn emit_generic_params(&mut self, params: &[juxc_ast::TypeParam]) {
        if params.is_empty() {
            return;
        }
        self.w.push('<');
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&p.name.text);
        }
        self.w.push('>');
    }

    /// Emit generic parameters as **type arguments** — `<T, U>` —
    /// used on the `impl<T, U> Name<T, U>` header where the params
    /// declared in the impl header are referenced as args on the
    /// type name. Same textual shape as `emit_generic_params`, but
    /// the call site reads differently.
    pub(crate) fn emit_generic_params_as_args(&mut self, params: &[juxc_ast::TypeParam]) {
        self.emit_generic_params(params);
    }

    /// Emit a generic-bound type position — same as `emit_type_as_rust`
    /// for interface bounds (interfaces already lower to Rust traits),
    /// but suffixed with `Kind` when the bound names a Jux class.
    /// Class membership comes from tycheck's [`SymbolTable`] —
    /// `self.symbols.classes` is the catalog of every top-level class
    /// in the unit, populated once during tycheck.
    pub(crate) fn emit_bound_type(&mut self, ty: &juxc_ast::TypeRef) {
        // Only single-segment, no-generic-args, no-array-shape bounds
        // get rewritten — those that look like a class-name lookup.
        // Anything more complex (`pkg.MyTrait`, `Foo<int>`) flows
        // through `emit_type_as_rust` unchanged.
        let is_simple_class = ty.array_shape.is_none()
            && ty.generic_args.is_empty()
            && ty.name.segments.len() == 1
            && self.symbols.classes.contains_key(ty.name.segments[0].text.as_str());
        if is_simple_class {
            self.w.push_str(&ty.name.segments[0].text);
            self.w.push_str("Kind");
            return;
        }
        self.emit_type_as_rust(ty);
    }

    /// Emit a generic-parameter list with user-declared bounds plus a
    /// uniform `Clone` tail — `<T: Drawable + Clone, U: Clone>`. Used
    /// on `impl` headers for generic classes/records and on the rare
    /// generic function (the latter going through the same helper for
    /// consistency).
    ///
    /// Phase-1 bound semantics: each entry in `param.bounds` is a Jux
    /// type ref; we emit it through `emit_type_as_rust` as a Rust trait
    /// bound. For bounds that resolve to a Jux interface (which we
    /// already emit as a Rust trait), this Just Works. Bounds naming
    /// concrete classes won't resolve until marker-trait synthesis
    /// lands — the user gets a clear Rust error if they try.
    pub(crate) fn emit_generic_params_with_clone_bound(&mut self, params: &[juxc_ast::TypeParam]) {
        if params.is_empty() {
            return;
        }
        self.w.push('<');
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&p.name.text);
            self.w.push_str(": ");
            // User bounds first, then the implicit Clone tail. Clone
            // is needed by our auto-`.clone()` injection on generic
            // field reads, so it always appears.
            //
            // **Class-bound rewriting**: when a bound names a Jux class
            // (rather than an interface), the class itself is a struct
            // and can't be a Rust trait bound on its own. We rewrite
            // the bound to the class's marker trait — `<Name>Kind` —
            // which the class implements directly and subclasses
            // implement transitively. The detection consults
            // `self.symbols.classes` — tycheck's authoritative class
            // catalog (replaces the backend's old `class_names` set
            // since Phase G).
            //
            // Clone the bounds to release the borrow on `params`
            // before the `emit_type_as_rust` calls (which mut-borrow
            // `self`).
            let user_bounds: Vec<juxc_ast::TypeRef> = p.bounds.clone();
            for bound in &user_bounds {
                self.emit_bound_type(bound);
                self.w.push_str(" + ");
            }
            self.w.push_str("Clone");
        }
        self.w.push('>');
    }

    /// Like [`Self::emit_type_as_rust`] but for **class-field type
    /// position** — Jux `String` lowers to owned Rust `String` instead
    /// of `&str`. Everything else falls through to the standard mapping.
    ///
    /// Rationale: a class field needs to own its String so the value
    /// outlives any single method call. Constructor/method parameters
    /// keep their cheap `&str` form; the `emit_assign` coercion takes
    /// care of `name.to_string()` when assigning into the field.
    pub(crate) fn emit_field_type_as_rust(&mut self, ty: &juxc_ast::TypeRef) {
        if is_jux_string_type(ty) {
            self.w.push_str("String");
            return;
        }
        self.emit_type_as_rust(ty);
    }

    /// Like [`Self::emit_type_as_rust`] but for **return-type position** —
    /// Jux `String` lowers to owned Rust `String` so a method or
    /// function can return a value that outlives `&self`. Parameter and
    /// local positions keep `&str`.
    pub(crate) fn emit_return_type_as_rust(&mut self, ty: &juxc_ast::TypeRef) {
        if is_jux_string_type(ty) {
            self.w.push_str("String");
            return;
        }
        self.emit_type_as_rust(ty);
    }

    /// Pick a sensible Rust default value to fill a freshly-allocated
    /// array of the given element type. Falls back to `Default::default()`
    /// for non-primitive types — that requires the user type to
    /// implement `Default + Copy`, otherwise Rust will surface the
    /// constraint failure.
    pub(crate) fn emit_default_value_for(&mut self, ty: &juxc_ast::TypeRef) {
        if let Some(rust_ty) = jux_primitive_to_rust(ty) {
            let default = match rust_ty {
                "bool" => "false",
                "f32" | "f64" => "0.0",
                "char" => "'\\0'",
                "&str" => "\"\"",
                // All integer types (i8/u8/.../i64/u64/isize/usize).
                _ => "0",
            };
            self.w.push_str(default);
        } else {
            self.w.push_str("Default::default()");
        }
    }

    /// Default value for a class field declaration. Differs from
    /// [`Self::emit_default_value_for`] only in the String case — fields
    /// default to an empty owned `String::new()` rather than the empty
    /// `&str` literal.
    pub(crate) fn emit_field_default_value_for(&mut self, ty: &juxc_ast::TypeRef) {
        if is_jux_string_type(ty) {
            self.w.push_str("String::new()");
            return;
        }
        self.emit_default_value_for(ty);
    }

    /// Emit `pub `/`pub(crate) `/`` (empty) for a visibility modifier.
    /// Trailing space included so call sites can paste it before a
    /// keyword without manual padding.
    pub(crate) fn emit_visibility(&mut self, vis: juxc_ast::Visibility) {
        match vis {
            juxc_ast::Visibility::Public => self.w.push_str("pub "),
            juxc_ast::Visibility::Internal | juxc_ast::Visibility::Protected => {
                self.w.push_str("pub(crate) ");
            }
            // Package-private and private fall through with no Rust
            // visibility keyword — Rust's default is module-private.
            juxc_ast::Visibility::Private | juxc_ast::Visibility::Package => {}
        }
    }
}

/// Map a Jux [`TypeRef`] to its Rust spelling **if** it is one of the
/// primitive types listed in `JUX-LANG-V1.md` §5.1 (or `String`).
/// Returns `None` for any compound or user-defined type — those land in
/// the verbatim-path fallback in [`RustEmitter::emit_type_as_rust`].
///
/// Two naming styles per §5.1:
/// - **Java-family** primary names: `byte`, `ubyte`, `short`, `ushort`,
///   `int`, `uint`, `long`, `ulong`, `float`, `double`.
/// - **Width-explicit** names (fixed widths only): `i8`/`u8`/`i16`/
///   `u16`/`i32`/`u32`/`i64`/`u64`/`f32`/`f64`.
///
/// **Aliases**: `byte ≡ i8`, `short ≡ i16`, `long ≡ i64`, `float ≡ f32`,
/// `double ≡ f64`, etc. — same Rust type. **Not aliases**: `int`/`uint`
/// are *platform-sized* (Rust `isize`/`usize`); `i32`/`u32` are *always*
/// 32-bit. The platform-sized type has no width-explicit synonym — by
/// design (a width-explicit name for an unknown-width type would be
/// meaningless).
///
/// Generic args and nullability disqualify a type from the primitive
/// fast-path — they need real type-system support, not a textual rewrite.
pub(crate) fn jux_primitive_to_rust(t: &juxc_ast::TypeRef) -> Option<&'static str> {
    if !t.generic_args.is_empty() || t.nullable || t.name.segments.len() != 1 {
        return None;
    }
    Some(match t.name.segments[0].text.as_str() {
        // Java-family names
        "bool"   => "bool",
        "byte"   => "i8",
        "ubyte"  => "u8",
        "short"  => "i16",
        "ushort" => "u16",
        // `int` / `uint` are PLATFORM-sized — pointer-width signed/unsigned.
        // Choose Rust's `isize`/`usize`, which is exactly that. No
        // width-explicit synonym — see the module-doc note.
        "int"    => "isize",
        "uint"   => "usize",
        "long"   => "i64",
        "ulong"  => "u64",
        "float"  => "f32",
        "double" => "f64",
        "char"   => "char",
        "String" => "&str",
        // Width-explicit names — fixed widths only.
        "i8"    => "i8",
        "u8"    => "u8",
        "i16"   => "i16",
        "u16"   => "u16",
        "i32"   => "i32",      // explicitly 32-bit — NOT alias for `int`
        "u32"   => "u32",      // explicitly 32-bit — NOT alias for `uint`
        "i64"   => "i64",
        "u64"   => "u64",
        "f32"   => "f32",
        "f64"   => "f64",
        _ => return None,
    })
}
