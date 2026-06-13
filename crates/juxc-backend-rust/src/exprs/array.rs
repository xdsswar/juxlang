//! Array-shaped expression emitters — indexing, `new T[size]` fills,
//! and `new T[]{a, b, c}` (or the bare `{a, b, c}`) initializer lit.

use juxc_ast::{Expr, IndexExpr, Literal, NewArrayExpr, NewArrayLitExpr};

use crate::RustEmitter;

impl RustEmitter {
    /// Does `bare` name a class whose Rust `Index` impl takes a
    /// BORROWED key (`Index<&K>`, map-style)? Reads the bindgen
    /// `@RustIndexRef` marker off the class AST — discovered from the
    /// library's real trait impls, never a name list.
    pub(crate) fn class_indexes_by_ref(&self, bare: &str) -> bool {
        self.class_ast_by_bare(bare)
            .map(|cd| {
                cd.annotations.iter().any(|a| {
                    a.name.segments.len() == 1
                        && a.name.segments[0]
                            .text
                            .eq_ignore_ascii_case("rustindexref")
                })
            })
            .unwrap_or(false)
    }

    /// Lower `arr[index]` to Rust `arr[index_as_usize]`.
    ///
    /// Rust requires `usize` for array/slice/Vec indexing. Jux's
    /// platform-sized `int` lowers to Rust `isize`, so a Jux user
    /// writing `arr[i]` with `int i` would hit a Rust type error
    /// without coercion. We sidestep that by:
    ///
    /// - **Integer literal indices** (`arr[0]`) → emit raw; Rust infers
    ///   `usize` from the indexing context.
    /// - **Anything else** (`arr[i]`, `arr[i + 1]`) → wrap as
    ///   `(expr) as usize`. The redundant cast is a no-op when the
    ///   operand is already `usize`.
    ///
    /// A future pass with a real type table can drop the cast when the
    /// index expression's static type is already `usize` (Jux `uint`).
    pub(crate) fn emit_index(&mut self, i: &IndexExpr) {
        // `operator[]` dispatch (§O.2.4): a user type declaring the
        // overload routes through its `__op_index` method. (Lvalue
        // writes never reach here — `emit_assign` intercepts the
        // whole `obj[i] = v` statement for `operator[]=`.)
        if self.expr_declares_operator(&i.array, juxc_ast::OperatorKind::Index) {
            self.emit_expr_with_parent_prec(&i.array, u8::MAX, false);
            self.w.push_str(".__op_index(");
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            self.emit_expr(&i.index);
            self.emitting_format_arg = prev;
            self.w.push(')');
            return;
        }
        let emitting_lvalue = self.emitting_lvalue;
        // MAP indexing (`scores["alice"]`): a container whose real
        // Rust `Index` impl takes a BORROWED key (`Index<&K>`) gets
        // `map[&key]`, not the sequence cast. DISCOVERED from the
        // stub's `@RustIndexRef` marker (bindgen reads the type's
        // actual trait impls); the name fallback only covers stub
        // caches generated before the marker existed.
        let map_index = match self.expr_types.get(&crate::exprs::expr_span_of(&i.array)) {
            Some(juxc_tycheck::Ty::User { name, .. }) => {
                let bare = name.rsplit('.').next().unwrap_or(name);
                self.class_indexes_by_ref(bare)
                    || matches!(bare, "HashMap" | "BTreeMap")
            }
            _ => false,
        };
        // The indexed array is a borrowed PLACE — `xs[i]` never owns
        // `xs`. Mark it like a method receiver so a collection-typed
        // field read (`this.items[0]`) doesn't take the value-position
        // auto-`.clone()` of the whole Vec (S15).
        self.emitting_method_receiver = true;
        self.emit_expr(&i.array);
        self.emitting_method_receiver = false;
        self.w.push('[');
        if map_index {
            self.w.push_str("&(");
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            self.emit_expr(&i.index);
            self.emitting_format_arg = prev;
            self.w.push(')');
        } else if matches!(&*i.index, Expr::Literal(Literal::Int(_))) {
            self.emit_expr(&i.index);
        } else {
            self.w.push('(');
            self.emit_expr(&i.index);
            self.w.push_str(") as usize");
        }
        self.w.push(']');
        // Rvalue index reads of non-Copy elements (String, value
        // classes, nested arrays) clone out — `xs[0]` would otherwise
        // move out of the Vec (rustc E0507). Lvalue positions
        // (`xs[i] = v`) and wrapper-class elements (whose share-clone
        // the wrapper machinery appends at the use site) are skipped.
        if !emitting_lvalue {
            if let Some(elem_ty) = self
                .expr_types
                .get(&i.span)
                .cloned()
            {
                let needs = match &elem_ty {
                    juxc_tycheck::Ty::String | juxc_tycheck::Ty::Array { .. } => true,
                    juxc_tycheck::Ty::User { name, .. } => {
                        let bare = name.rsplit('.').next().unwrap_or(name);
                        // Wrapper classes share-clone at use sites; tuple
                        // sentinel and unknown names stay un-cloned.
                        !self.wrapper_classes.contains(bare)
                            && (self.symbols.classes.contains_key(name.as_str())
                                || self
                                    .symbols
                                    .classes
                                    .keys()
                                    .any(|k| k.rsplit('.').next().unwrap_or(k) == bare))
                    }
                    _ => false,
                };
                if needs {
                    self.w.push_str(".clone()");
                }
            }
        }
    }

    /// Lower `new T[size]` to Rust `[default_for_T; size]`.
    ///
    /// Rust's `[VALUE; N]` literal requires `N` to be a `const` expr
    /// and `VALUE` to be `Copy` (or evaluated once for `const`). For
    /// Turn 1 we emit:
    ///
    /// - `new int[10]`     → `[0; 10]`
    /// - `new bool[5]`     → `[false; 5]`
    /// - `new double[3]`   → `[0.0; 3]`
    /// - `new char[8]`     → `['\\0'; 8]`
    /// - `new MyType[N]`   → `[Default::default(); N]` (works iff MyType: Default + Copy)
    pub(crate) fn emit_new_array(&mut self, n: &NewArrayExpr) {
        // **Generic element** (`new T[N]` where `T` is a type param in
        // scope): the `[VALUE; N]` repeat form would require `T: Copy`
        // on top of `Default` — Jux generics carry `Clone`, not `Copy`.
        // `std::array::from_fn` evaluates the closure per element, so
        // only `T: Default` is needed (added to the class's bound by
        // `class_default_bound_params` when a `T[N]` field exists).
        // The array's size/type are inferred from the assignment
        // target, so `from_fn` needs no explicit length.
        let elem_is_type_param = n.element_type.array_shape.is_none()
            && !n.element_type.nullable
            && n.element_type.generic_args.is_empty()
            && n.element_type.fn_shape.is_none()
            && n.element_type.name.segments.len() == 1
            && self
                .current_type_params
                .contains(n.element_type.name.segments[0].text.as_str());
        if elem_is_type_param {
            self.w
                .push_str("std::array::from_fn(|_| Default::default())");
            return;
        }
        self.w.push('[');
        self.emit_default_value_for(&n.element_type);
        self.w.push_str("; ");
        // A const-evaluable length (`new int[SIZE * 2]`) emits the computed
        // `usize` literal (§T.11). Otherwise the repeat-length slot is a const
        // `usize` position — a const-generic param (`new int[N]`) must stay raw
        // `N`, not the `(N as isize)` value-cast.
        if let Some(v) = self.try_const_int(&n.size) {
            self.w.push_str(&v.to_string());
        } else {
            let prev = self.in_array_size_position;
            self.in_array_size_position = true;
            self.emit_expr(&n.size);
            self.in_array_size_position = prev;
        }
        self.w.push(']');
    }

    /// Lower an array initializer literal — `new T[]{a, b, c}` or the
    /// bare `{a, b, c}` form in a typed-local RHS.
    ///
    /// Dispatch is on `n.fixed`:
    ///
    /// - **`fixed: true`** → Rust array literal `[a, b, c]`. Used when
    ///   the binding's LHS type is `T[N]` (compile-time-known size).
    ///   Rust verifies the element count matches `N` at compile time.
    /// - **`fixed: false`** → `vec![a, b, c]` (or `Vec::<T>::new()`
    ///   when the list is empty — `vec![]` alone is type-ambiguous).
    ///   Used when the binding's LHS type is `T[]` or when the literal
    ///   came from a `new T[]{…}` new-expression.
    ///
    /// Element-type inference quirk (dynamic case): `let xs = vec![1, 2, 3];`
    /// alone defaults to `Vec<i32>` even when the Jux source said
    /// `int` (isize). That's fine for printing/indexing; a future pass
    /// with full type-tracking can emit a `: Vec<isize>` annotation
    /// when a typed local makes the intended element type explicit.
    pub(crate) fn emit_new_array_lit(&mut self, n: &NewArrayLitExpr) {
        // Fixed → Rust array literal `[a, b, c]`. Empty fixed literals
        // can't be written in Jux (the parser never produces them) so
        // we don't have a special path for them.
        if n.fixed {
            self.w.push('[');
            for (i, elem) in n.elements.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.emit_array_element(elem, &n.element_type);
            }
            self.w.push(']');
            return;
        }

        // Dynamic — Vec lowering.
        if n.elements.is_empty() {
            // Empty literal — turbofish-constructed empty Vec so Rust
            // knows the element type without an annotation.
            self.w.push_str("Vec::<");
            self.emit_type_as_rust(&n.element_type);
            self.w.push_str(">::new()");
            return;
        }
        self.w.push_str("vec![");
        for (i, elem) in n.elements.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.emit_array_element(elem, &n.element_type);
        }
        self.w.push(']');
    }

    /// Emit one element of an array/collection literal in **value/move
    /// position**. A wrapped-class place element (`vec![c]` where `c`
    /// names a wrapper class) gets a trailing `.clone()` so the stored
    /// slot holds a SHARED `Rc` handle instead of moving the source out
    /// (§CR.4.1) — a bare move would leave `c` invalidated after the
    /// literal and break Java reference semantics. A `Field` element
    /// already self-clones in `emit_field`, so the helper excludes it.
    fn emit_array_element(&mut self, elem: &Expr, element_type: &juxc_ast::TypeRef) {
        // Interface-element array (`Shape[] = { new Circle(), … }`): each
        // element is wrapped into the `Rc<dyn Trait>` element representation.
        if !matches!(
            self.iface_coercion_to(element_type, elem),
            crate::analysis::IfaceCoercion::None,
        ) {
            self.emit_expr_coerced_to_iface(element_type, elem);
            return;
        }
        self.emit_expr(elem);
        if self.wrapper_value_needs_clone(elem) {
            self.w.push_str(".clone()");
        }
    }
}
