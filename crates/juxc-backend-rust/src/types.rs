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
        // Raw pointer `T*` is the OUTERMOST modifier (§5.5 / §A.2.7), peeled
        // first: each `*` level emits a Rust `*mut`, then we recurse on the
        // type with the pointer suffix stripped. So `int*` → `*mut isize`,
        // `int**` → `*mut *mut isize`, and `T[]*` (array then pointer) →
        // `*mut Vec<T>`. Pointers are `unsafe`-only; the type checker gates
        // their use behind an `unsafe` context.
        if ty.ptr_depth > 0 {
            for _ in 0..ty.ptr_depth {
                self.w.push_str("*mut ");
            }
            let mut inner = ty.clone();
            inner.ptr_depth = 0;
            self.emit_type_as_rust(&inner);
            return;
        }
        // Nullable types `T?` lower to Rust's `Option<T>`. We peel
        // the `nullable` flag here and recurse on the inner type
        // (which is `ty` with `nullable = false`). All other shape
        // flags — array, generics, fn-shape — apply to the inner
        // type, NOT to the `Option` wrapper: `int?[]` is
        // `Option<Vec<isize>>` is wrong; it should be
        // `Vec<Option<isize>>`. So the order is:
        //
        //   1. Function-type? (always outermost shape — fn-types are
        //      first-class).
        //   2. Array? Element keeps the `nullable` flag so `int?[]`
        //      → `Vec<Option<isize>>`.
        //   3. Nullable? Recurse on non-nullable inner.
        //   4. Primitive / user type.
        //
        // (1) and (2) are already handled below; the nullable
        // pass-through inside the array-shape recursion preserves
        // `ty.nullable` so the inner element-type emit hits the
        // nullable branch with the right per-element type.
        // Async-runtime builtin types (§18.3): `Channel<T>` /
        // `AsyncMutex<T>` in type positions map to the emitted
        // helpers.
        if ty.fn_shape.is_none() && ty.name.segments.len() == 1 {
            let bare = ty.name.segments[0].text.as_str();
            let mapped = match bare {
                "Channel" if !self.symbols.classes.contains_key("Channel") => {
                    Some("crate::JuxChannel")
                }
                "AsyncMutex" if !self.symbols.classes.contains_key("AsyncMutex") => {
                    Some("crate::JuxAsyncMutex")
                }
                _ => None,
            };
            if let Some(path) = mapped {
                self.w.push_str(path);
                if !ty.generic_args.is_empty() {
                    self.w.push('<');
                    for (i, arg) in ty.generic_args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        if let Some(t) = arg.as_type() {
                            self.emit_type_as_rust(t);
                        }
                    }
                    self.w.push('>');
                }
                return;
            }
        }
        // Tuple type — `(A, B)` (§5.3, `__tuple` sentinel encoding)
        // emits as Rust's structurally identical tuple type.
        if let Some(elems) = ty.tuple_elems() {
            self.w.push('(');
            for (i, el) in elems.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.emit_type_as_rust(el);
            }
            self.w.push(')');
            return;
        }
        if let Some(fn_shape) = &ty.fn_shape {
            self.w.push_str("std::rc::Rc<dyn Fn(");
            for (i, p) in fn_shape.params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.emit_type_as_rust(p);
            }
            self.w.push_str(") -> ");
            // `(int) -> void` returns Rust unit — `void` is a return-
            // slot keyword, not a type name `emit_type_as_rust` knows.
            let returns_void = fn_shape.return_type.array_shape.is_none()
                && !fn_shape.return_type.nullable
                && fn_shape.return_type.fn_shape.is_none()
                && fn_shape
                    .return_type
                    .name
                    .segments
                    .last()
                    .map(|s| s.text == "void")
                    .unwrap_or(false);
            if returns_void {
                self.w.push_str("()");
            } else {
                self.emit_type_as_rust(&fn_shape.return_type);
            }
            self.w.push('>');
            return;
        }
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
                        fn_shape: ty.fn_shape.clone(),
                        ptr_depth: 0,
                        span: ty.span,
                    };
                    self.emit_type_as_rust(&element_ty);
                    self.w.push_str("; ");
                    // The size slot must be a raw `usize` — flag the
                    // position so a const-generic param (`[T; N]`)
                    // skips its `(N as isize)` value-cast.
                    let prev = self.in_array_size_position;
                    self.in_array_size_position = true;
                    self.emit_expr(size);
                    self.in_array_size_position = prev;
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
                        fn_shape: ty.fn_shape.clone(),
                        ptr_depth: 0,
                        span: ty.span,
                    };
                    self.emit_type_as_rust(&element_ty);
                    self.w.push('>');
                }
            }
            return;
        }

        // Nullable peeled here, after array — `T?` wraps the
        // already-emitted inner type in `Option<…>`. Done after
        // arrays so `T?[]` lowers to `Vec<Option<T>>` (the `?`
        // applies to each element, not the Vec).
        if ty.nullable {
            let inner = juxc_ast::TypeRef {
                name: ty.name.clone(),
                generic_args: ty.generic_args.clone(),
                nullable: false,
                array_shape: ty.array_shape.clone(),
                fn_shape: ty.fn_shape.clone(),
                ptr_depth: 0,
                span: ty.span,
            };
            self.w.push_str("Option<");
            self.emit_type_as_rust(&inner);
            self.w.push('>');
            return;
        }

        // **Stdlib compiler primitives.** ArrayList / HashMap /
        // HashSet lower directly to their Rust std counterparts.
        // The Jux source files under `jux.std/collections/`
        // document the API contract; the compiler knows the
        // mapping by FQN (a small fixed set, on par with how
        // `int` and `String` are also hardcoded primitives).
        if let Some(seg) = ty.name.segments.last() {
            let bare = seg.text.as_str();
            match bare {
                "HashMap" if ty.generic_args.len() == 2 => {
                    self.w.push_str("std::collections::HashMap<");
                    for (i, arg) in ty.generic_args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        if let juxc_ast::GenericArg::Type(t) = arg {
                            self.emit_type_as_rust(t);
                        }
                    }
                    self.w.push('>');
                    return;
                }
                "HashSet" if ty.generic_args.len() == 1 => {
                    self.w.push_str("std::collections::HashSet<");
                    if let Some(juxc_ast::GenericArg::Type(t)) = ty.generic_args.first() {
                        self.emit_type_as_rust(t);
                    }
                    self.w.push('>');
                    return;
                }
                "ArrayList" if ty.generic_args.len() == 1 => {
                    self.w.push_str("Vec<");
                    if let Some(juxc_ast::GenericArg::Type(t)) = ty.generic_args.first() {
                        self.emit_type_as_rust(t);
                    }
                    self.w.push('>');
                    return;
                }
                "Deque" if ty.generic_args.len() == 1 => {
                    self.w.push_str("std::collections::VecDeque<");
                    if let Some(juxc_ast::GenericArg::Type(t)) = ty.generic_args.first() {
                        self.emit_type_as_rust(t);
                    }
                    self.w.push('>');
                    return;
                }
                // Monotonic time-point (jux.std.time) — a plain Copy
                // value, not a wrapper class.
                "Instant" if ty.generic_args.is_empty() => {
                    self.w.push_str("std::time::Instant");
                    return;
                }
                // Atomic counters (§S.6.2) — Arc-backed so handles
                // share the same cell across spawn boundaries.
                "AtomicInt" if ty.generic_args.is_empty() => {
                    self.w
                        .push_str("std::sync::Arc<std::sync::atomic::AtomicIsize>");
                    return;
                }
                "AtomicLong" if ty.generic_args.is_empty() => {
                    self.w
                        .push_str("std::sync::Arc<std::sync::atomic::AtomicI64>");
                    return;
                }
                _ => {}
            }
        }
        if let Some(rust_ty) = jux_primitive_to_rust(ty) {
            // Const-context override: a `const`/`static` decl can't
            // run `.to_string()` at init time, so `String` lowers to
            // `&'static str` in this position. The matching
            // `emit_literal` path drops its `.to_string()` wrap when
            // `emitting_const_context` is set.
            if self.emitting_const_context && rust_ty == "String" {
                self.w.push_str("&'static str");
                return;
            }
            self.w.push_str(rust_ty);
            return;
        }
        // **External (rust.std / crate) type in type position (§G.9.2).** A
        // field/param/return/local typed with a bound foreign type — e.g.
        // `Vec<Todo>` — emits its real Rust path (`std::vec::Vec<Todo>`) from
        // the stub's `@rust("…")` annotation, not the flat `crate::rust::std::Vec`
        // module path (which doesn't exist). Placed AFTER `jux_primitive_to_rust`
        // so names that are also Jux primitives (`String`, …) keep their
        // primitive lowering. Generic args recurse so they get their own mapping.
        if let Some(real) = self.external_class_real_path(&ty.name) {
            self.w.push_str(&real);
            if !ty.generic_args.is_empty() {
                self.w.push('<');
                for (i, arg) in ty.generic_args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    if let juxc_ast::GenericArg::Type(t) = arg {
                        self.emit_type_as_rust(t);
                    }
                }
                self.w.push('>');
            }
            return;
        }
        // Cross-package bare-name reference — when a single
        // segment like `IllegalArgumentException` resolves to an
        // FQN in a different package than the current unit's,
        // emit the fully-qualified `crate::a::b::Name` form so
        // Rust's name resolver finds it through the emitted
        // `pub mod` tree. Same-package references stay bare —
        // they reach their sibling through normal Rust module
        // visibility.
        let path = if ty.name.segments.len() == 1 {
            let bare = ty.name.segments[0].text.as_str();
            let mut resolved_path: Option<String> = None;
            // §M.9 enclosing-class fallback: a bare `Config` inside
            // `HttpServer` (or a sibling nested type) names the
            // lifted `HttpServer__Config`. Walk the owner chain
            // outward; the lifted sibling lives in the same module,
            // so the mangled bare name resolves directly.
            if let Some(enclosing) = self.enclosing_class.clone() {
                let mut scope: Option<&str> = Some(enclosing.as_str());
                while let Some(s) = scope {
                    let candidate = format!("{s}__{bare}");
                    if self.lookup_class_by_bare_or_fqn(&candidate).is_some()
                        || self.symbols.records.contains_key(&candidate)
                    {
                        resolved_path = Some(candidate);
                        break;
                    }
                    scope = s.rsplit_once("__").map(|(outer, _)| outer);
                }
            }
            if resolved_path.is_none() {
            if let Some(fqn) = self.symbols.find_fqn_by_bare(bare) {
                if fqn.contains('.') {
                    let cur_pkg = self.symbols.package.join(".");
                    let fqn_pkg = fqn
                        .rsplit_once('.')
                        .map(|(p, _)| p.to_string())
                        .unwrap_or_default();
                    if fqn_pkg != cur_pkg {
                        let joined = fqn
                            .split('.')
                            .collect::<Vec<_>>()
                            .join("::");
                        resolved_path = Some(format!("crate::{joined}"));
                    }
                }
            }
            }
            resolved_path.unwrap_or_else(|| bare.to_string())
        } else {
            // §M.9 qualified nested-type access in a TYPE position:
            // `HttpServer.Config` names the lifted
            // `HttpServer__Config`, not a module path. Resolve the
            // first segment as a class and try the mangled form;
            // package-qualified module paths fall through to the
            // plain `::` join.
            let first = ty.name.segments[0].text.as_str();
            let rest = ty.name.segments[1..]
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join("__");
            let mangled = format!("{first}__{rest}");
            if self.lookup_class_by_bare_or_fqn(&mangled).is_some()
                || self.symbols.records.contains_key(&mangled)
            {
                mangled
            } else {
                ty.name
                    .segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("::")
            }
        };
        // **Interface in a value position → `Rc<dyn Trait>`.** When this
        // name resolves to an interface AND we're emitting a value slot
        // (variable / param / field / return — `in_value_type_position`),
        // wrap the trait spelling in a clone-able `Rc<dyn …>` trait object
        // so the slot can hold any implementer and dispatch dynamically.
        // Trait-impl headers, generic bounds, and `From<>` headers don't set
        // the flag, so they keep the bare name. Generic interfaces and
        // generic-method interfaces are rejected at tycheck (E0435) before
        // reaching here, so the produced `dyn` is always object-safe.
        let last_seg = ty.name.segments.last().map(|s| s.text.as_str());
        let value_iface = self.in_value_type_position
            && last_seg
                .map(|s| self.lookup_interface_by_bare_or_fqn(s).is_some())
                .unwrap_or(false);
        // **Polymorphic-base CLASS in a value position → `Rc<dyn <Name>Kind>`.**
        // A base-typed slot holds any subclass and dispatches virtually through
        // the populated `<Name>Kind` trait (Stage-2), NOT the class struct.
        // Poly bases are non-generic (gated at classification), so there are no
        // generic args to thread — emit the trait object and return.
        let value_polybase = self.in_value_type_position
            && !value_iface
            && last_seg
                .map(|s| self.poly_base_classes.contains(s))
                .unwrap_or(false);
        if value_polybase {
            self.w.push_str("std::rc::Rc<dyn ");
            self.w.push_str(&path);
            self.w.push_str("Kind>");
            return;
        }
        if value_iface {
            self.w.push_str("std::rc::Rc<dyn ");
        }
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
        if value_iface {
            self.w.push('>');
        }
    }

    /// Emit a [`TypeRef`] in **value position** — a variable / parameter /
    /// field / return slot. Identical to [`Self::emit_type_as_rust`] except
    /// an interface name lowers to `Rc<dyn Trait>` (a dynamic-dispatch trait
    /// object) instead of its bare trait spelling. Use this at every site
    /// where a value of the type is stored or passed; keep
    /// [`Self::emit_type_as_rust`] for trait-impl headers, generic bounds,
    /// and `From<>` headers where the bare name is required.
    pub(crate) fn emit_value_type_as_rust(&mut self, ty: &juxc_ast::TypeRef) {
        let prev = self.in_value_type_position;
        self.in_value_type_position = true;
        self.emit_type_as_rust(ty);
        self.in_value_type_position = prev;
    }

    /// Lower a type that appears as a generic argument (e.g. the
    /// `String` inside `Container<String>`). Differs from
    /// [`Self::emit_type_as_rust`] in two cases:
    ///
    /// 1. **Jux `String` → owned Rust `String`** — `&str` won't work
    ///    as a generic-arg slot without an explicit lifetime.
    /// 2. **Wildcards (`?`, `? extends T`, `? super T`)** — Phase 1
    ///    erases the wildcard to its bound's marker trait via a
    ///    `Box<dyn Trait>` shape in storage position. Unbounded `?`
    ///    falls back to `Box<dyn std::any::Any>`. This is a
    ///    placeholder strategy; the function-generic lift for
    ///    parameter positions is wired in a later phase.
    pub(crate) fn emit_generic_arg_type_as_rust(&mut self, arg: &juxc_ast::GenericArg) {
        match arg {
            juxc_ast::GenericArg::Type(ty) => {
                if is_jux_string_type(ty) {
                    self.w.push_str("String");
                    return;
                }
                self.emit_type_as_rust(ty);
            }
            juxc_ast::GenericArg::Wildcard(w) => {
                self.emit_wildcard_arg_placeholder(w);
            }
        }
    }

    /// Emit a Rust type for a wildcard generic arg in storage
    /// position (field, local, return). Strategy: trait-object
    /// erasure via `std::rc::Rc<dyn Bound>`.
    ///
    /// **Why `Rc` and not `Box`?** Our class wrappers `#[derive(Clone)]`,
    /// and `Box<dyn Trait>` doesn't implement Clone (the inner
    /// `dyn` is `?Sized`). `Rc<dyn Trait>` is always Clone — the
    /// refcount bumps without touching the value. That matches the
    /// shape `class-representation-addendum.md` already lists as
    /// the "shared-ownership" wrapper. Phase 1 stays single-threaded
    /// so `Rc` is the right pick; multi-threaded code would want
    /// `Arc` (deferred — needs a thread-safety flag on the type).
    ///
    /// **Why not `Self: Sized` errors?** The marker trait `<Name>Kind`
    /// no longer has `Clone` as a supertrait, so `dyn AnimalKind` is
    /// dyn-compatible. The user-side `<T: AnimalKind + Clone>`
    /// bounds at use sites still pull `Clone` in explicitly.
    fn emit_wildcard_arg_placeholder(&mut self, w: &juxc_ast::WildcardArg) {
        match &w.bound {
            None => self.w.push_str("std::rc::Rc<dyn std::any::Any>"),
            Some(juxc_ast::WildcardBound::Extends(bound)) => {
                self.w.push_str("std::rc::Rc<dyn ");
                self.emit_bound_type(bound);
                self.w.push('>');
            }
            Some(juxc_ast::WildcardBound::Super(bound)) => {
                // `? super T` accepts T and any supertype. In the
                // erased Rust form we can't express "supertype of T"
                // directly; fall back to the same shape as
                // `? extends T` since the marker-trait of T covers T
                // and itself. A precise contravariance-aware
                // lowering would need a separate signature-rewrite
                // pass — out of scope for Phase 1.
                self.w.push_str("std::rc::Rc<dyn ");
                self.emit_bound_type(bound);
                self.w.push('>');
            }
        }
    }

    /// Emit one **const-generic parameter declaration** — the Rust
    /// `const N: usize` for a Jux `<int N>` (spec §T.11.3). The value
    /// type maps `int → usize` rather than the literal `isize`: a
    /// fixed array size `[T; N]` must be *exactly* `usize` on stable
    /// Rust (`[T; N as usize]` over a generic param needs nightly
    /// `generic_const_exprs`), and `T[N]` storage is the feature's
    /// headline use. Where `N` is read as an int *value*, the
    /// expression emitter casts back (`(N as isize)` — see
    /// `const_generic_params` tracking). `bool` maps to itself.
    fn emit_const_generic_param_decl(&mut self, p: &juxc_ast::TypeParam) {
        self.w.push_str("const ");
        self.w.push_str(&p.name.text);
        self.w.push_str(": ");
        let value_ty = p
            .const_ty
            .as_ref()
            .and_then(|t| t.name.segments.last())
            .map(|s| s.text.as_str())
            .unwrap_or("int");
        // Only `int` and `bool` survive the parser's E0445 gate; the
        // fallback keeps emission total if that ever changes.
        self.w.push_str(if value_ty == "bool" { "bool" } else { "usize" });
    }

    /// Emit a generic-parameter list as a declaration site — `<T, U>`,
    /// const params as `const N: usize`.
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
            if p.is_const() {
                self.emit_const_generic_param_decl(p);
            } else {
                self.w.push_str(&p.name.text);
            }
        }
        self.w.push('>');
    }

    /// Emit generic parameters as **type arguments** — `<T, U>` —
    /// used on the `impl<T, U> Name<T, U>` header where the params
    /// declared in the impl header are referenced as args on the
    /// type name. In argument position a const param is just its
    /// name (`Name<T, N>`), so this no longer simply forwards to
    /// `emit_generic_params` (which emits the `const N: usize`
    /// declaration form).
    pub(crate) fn emit_generic_params_as_args(&mut self, params: &[juxc_ast::TypeParam]) {
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
            // A const param (`<int N>`) declares as `const N: usize` —
            // value params take no trait bounds, so the Clone/Debug
            // tail below never applies to them.
            if p.is_const() {
                self.emit_const_generic_param_decl(p);
                continue;
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
            // `Clone` for the auto-`.clone()` on generic field reads,
            // plus `std::fmt::Debug` so generic structs whose marker
            // trait now carries a `Debug` supertrait (see
            // `emit_class_marker_trait`) satisfy `<Class><T>: Debug`.
            // Every Jux type derives `Debug`, so the extra bound is
            // always satisfiable and keeps `#[derive(Debug)]` on
            // generic containers (and their marker impls) sound —
            // including storage-position wildcards that erase a
            // generic arg to `Box<dyn …Kind>`.
            self.w.push_str("Clone + std::fmt::Debug");
        }
        self.w.push('>');
    }

    /// Like [`Self::emit_generic_params_with_clone_bound`] but adds a
    /// `+ std::fmt::Display` bound to every param whose name is in
    /// `display_params`. Used on a generic class's **inherent impl**
    /// when a method formats a value of that type parameter (Jux
    /// `toString`/interpolation semantics — `$"…${this.left}…"` on an
    /// `A`-typed field requires `A: Display`). We bound only the
    /// params actually formatted so a generic class that merely
    /// *stores* a non-`Display` value stays usable.
    pub(crate) fn emit_generic_params_with_clone_bound_plus_display(
        &mut self,
        params: &[juxc_ast::TypeParam],
        display_params: &std::collections::HashSet<String>,
        default_params: &std::collections::HashSet<String>,
    ) {
        if params.is_empty() {
            return;
        }
        self.w.push('<');
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            // Const params take no trait bounds — Display included.
            if p.is_const() {
                self.emit_const_generic_param_decl(p);
                continue;
            }
            self.w.push_str(&p.name.text);
            self.w.push_str(": ");
            let user_bounds: Vec<juxc_ast::TypeRef> = p.bounds.clone();
            for bound in &user_bounds {
                self.emit_bound_type(bound);
                self.w.push_str(" + ");
            }
            self.w.push_str("Clone + std::fmt::Debug");
            if display_params.contains(&p.name.text) {
                self.w.push_str(" + std::fmt::Display");
            }
            // A param used as a fixed-array-field element (`T[N]`)
            // needs `Default` for the `from_fn` construction — see
            // `class_default_bound_params`.
            if default_params.contains(&p.name.text) {
                self.w.push_str(" + Default");
            }
        }
        self.w.push('>');
    }

    /// Like [`Self::emit_type_as_rust`] but for **class-field type
    /// position** — kept as a thin wrapper so a future divergence
    /// (e.g. lifetime threading for borrowed-field designs) has a
    /// natural seam.
    ///
    /// Post Fix 1 the standard mapping already lowers Jux `String`
    /// to owned `String` in every position, so this wrapper is
    /// effectively a forward — except in `emitting_const_context`,
    /// where the standard mapping uses `&'static str`. That's the
    /// behavior we want for `pub const`/`pub static` fields too,
    /// so the forwarding path is correct without any extra branch.
    pub(crate) fn emit_field_type_as_rust(&mut self, ty: &juxc_ast::TypeRef) {
        // Field slots are value positions: an interface-typed field lowers to
        // `Rc<dyn Trait>` so the struct can store any implementer.
        self.emit_value_type_as_rust(ty);
    }

    /// Like [`Self::emit_type_as_rust`] but for **return-type position**.
    /// Post Fix 1 every position lowers Jux `String` to owned Rust
    /// `String`, so this is a plain forward. Kept named for call-site
    /// readability and to leave room for a future divergence (e.g.
    /// borrow-thread `&'a str` returns when borrow inference lands).
    pub(crate) fn emit_return_type_as_rust(&mut self, ty: &juxc_ast::TypeRef) {
        // `async void` synthesizes a sentinel `void`-named TypeRef in
        // `parse_return_type` — emit Rust's unit `()` so the produced
        // signature is `async fn name(...) -> ()`. Without this
        // shortcut the type emitter would treat `void` as a user-
        // defined class name and emit it literally.
        if ty.array_shape.is_none()
            && !ty.nullable
            && ty.generic_args.is_empty()
            && ty.fn_shape.is_none()
        {
            if let Some(seg) = ty.name.segments.last() {
                if seg.text == "void" {
                    self.w.push_str("()");
                    return;
                }
            }
        }
        // A return slot is a value position: an interface-typed return
        // lowers to `Rc<dyn Trait>` — the same trait-object representation a
        // local / field / param uses, so a returned interface value flows
        // into any of those slots without a representation mismatch. (An
        // earlier revision emitted `impl Trait` here, which is incompatible
        // with `Rc<dyn Trait>` value slots and can't carry two different
        // concrete impls from one factory.) The `Rc<dyn …>` wrap itself is
        // produced by the value-position path in `emit_type_as_rust`.
        self.emit_value_type_as_rust(ty);
    }

    /// Pick a sensible Rust default value to fill a freshly-allocated
    /// array of the given element type. Falls back to `Default::default()`
    /// for non-primitive types — that requires the user type to
    /// implement `Default + Copy`, otherwise Rust will surface the
    /// constraint failure.
    pub(crate) fn emit_default_value_for(&mut self, ty: &juxc_ast::TypeRef) {
        // Shape modifiers come first — `int[]` is a Vec (its ELEMENT
        // primitive must not leak through as the default), and a
        // nullable slot's default is `None` regardless of the inner.
        if ty.nullable {
            self.w.push_str("None");
            return;
        }
        if let Some(shape) = &ty.array_shape {
            match shape {
                juxc_ast::ArrayShape::Dynamic => self.w.push_str("Vec::new()"),
                juxc_ast::ArrayShape::Fixed(_) => self.w.push_str("Default::default()"),
            }
            return;
        }
        if let Some(rust_ty) = jux_primitive_to_rust(ty) {
            let default = match rust_ty {
                "bool" => "false",
                "f32" | "f64" => "0.0",
                "char" => "'\\0'",
                // Per Fix 1, Jux `String` lowers to owned Rust
                // `String`. The empty value is `String::new()`,
                // matching the field-default path.
                "String" => "String::new()",
                // All integer types (i8/u8/.../i64/u64/isize/usize).
                _ => "0",
            };
            self.w.push_str(default);
        } else {
            self.w.push_str("Default::default()");
        }
    }

    /// Default value for a class field declaration. Forwards to
    /// [`Self::emit_default_value_for`]; post Fix 1 both paths
    /// produce `String::new()` for Jux `String` already.
    pub(crate) fn emit_field_default_value_for(&mut self, ty: &juxc_ast::TypeRef) {
        self.emit_default_value_for(ty);
    }

    /// Emit the **no-initializer default** for a class field slot. A `weak`
    /// field (§6.5) defaults to an empty `std::rc::Weak::new()` (a handle that
    /// upgrades to `None` until assigned); every other field falls back to its
    /// type's natural default. Used at the constructor field-init sites. (A
    /// weak field can never have an explicit initializer — tycheck E0456 — so
    /// this is the only place a weak slot is seeded.)
    pub(crate) fn emit_field_storage_default(&mut self, field: &juxc_ast::FieldDecl) {
        if field.is_weak {
            self.w.push_str("std::rc::Weak::new()");
        } else {
            self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
        }
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
        "bool"     => "bool",
        // `boolean` is Java's spelling; we accept both so a Java
        // developer's muscle memory doesn't trip a confusing
        // "cannot find type `boolean`" rustc error on the emitted
        // crate. Both spell the same Rust `bool`.
        "boolean"  => "bool",
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
        // Per JUX-CODEGEN-FIXES.md Fix 1: Jux `String` always lowers
        // to owned Rust `String` — never `&str`. Parameters, locals,
        // fields, returns, and string literals all share the same
        // type, so `match`-arm unification and value flow Just Work.
        // The cost is one heap alloc per string literal; Java does
        // exactly this and nobody notices.
        "String" => "String",
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
