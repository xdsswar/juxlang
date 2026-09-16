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
        // A USER class carries the marker on its AST…
        if let Some(cd) = self.class_ast_by_bare(bare) {
            if cd.annotations.iter().any(is_index_ref) {
                return true;
            }
        }
        // …and a SCANNED class carries it on its signature. The AST map holds
        // user classes only, so without this half every question about a
        // `rust.std` type missed and the caller fell back to naming the two
        // types it knew about — which is how `HashMap`/`BTreeMap` came to be
        // hardcoded next to a marker that already said the same thing.
        self.lookup_class_by_bare_or_fqn(bare)
            .is_some_and(|sig| sig.annotations.iter().any(is_index_ref))
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
        // The element is emitted with the lvalue flag taken (see below), and
        // the flag is put back afterwards: `t[0].color = c` writes a FIELD of
        // the element, and the field access around this index still has to
        // know it is a place. Left cleared, it read `.color` as a value and
        // appended the `.clone()` of a read, which cannot be assigned to.
        let was_lvalue = self.emitting_lvalue;
        self.emit_index_place(i);
        self.emitting_lvalue = was_lvalue;
    }

    fn emit_index_place(&mut self, i: &IndexExpr) {
        // **`s.bytes()[i]`** (§S.3.2): the byte at a position. `bytes()` is an
        // iterator, which has no index, so the indexed form reads the string's
        // bytes directly.
        if let Expr::Call(call) = &*i.array {
            if let Expr::Field(f) = &*call.callee {
                if f.field.text == "bytes"
                    && call.args.is_empty()
                    && matches!(self.receiver_ty_of(&f.object), Some(juxc_tycheck::Ty::String))
                {
                    self.emit_expr_with_parent_prec(&f.object, u8::MAX, false);
                    self.w.push_str(".as_bytes()[(");
                    let prev = std::mem::take(&mut self.emitting_lvalue);
                    self.emit_expr(&i.index);
                    self.emitting_lvalue = prev;
                    self.w.push_str(") as usize]");
                    return;
                }
            }
        }
        // **`p[i]` on a raw pointer (§L.6.2)** is `*(p + i)`. Rust pointers
        // cannot be indexed, so it lowers to a dereference of the offset
        // pointer; as the target of an assignment the same place is written.
        if self.expr_is_raw_pointer(&i.array) {
            self.emitting_lvalue = false;
            self.w.push_str("(*");
            self.emit_pointer_receiver(&i.array);
            self.w.push_str(".offset(");
            self.emit_pointer_step(&i.index);
            self.w.push_str("))");
            return;
        }
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
        // TAKE the flag, do not copy it. Only the OUTERMOST index is the place
        // being written; `i.array` and `i.index` are rvalues. Leaving it set
        // made a nested write take the exclusive borrow at every level --
        // `outer.borrow_mut()[0].borrow_mut()[1]` -- which is two guards on
        // one cell and a runtime panic (§CR.4.1).
        let emitting_lvalue = std::mem::take(&mut self.emitting_lvalue);
        // MAP indexing (`scores["alice"]`): a container whose real
        // Rust `Index` impl takes a BORROWED key (`Index<&K>`) gets
        // `map[&key]`, not the sequence cast. DISCOVERED from the
        // stub's `@RustIndexRef` marker (bindgen reads the type's
        // actual trait impls); the name fallback only covers stub
        // caches generated before the marker existed.
        let map_index = self.index_takes_ref_key(&i.array);
        // The indexed array is a borrowed PLACE — `xs[i]` never owns
        // `xs`. Mark it like a method receiver so a collection-typed
        // field read (`this.items[0]`) doesn't take the value-position
        // auto-`.clone()` of the whole Vec (S15).
        let raw_place = std::mem::take(&mut self.emitting_raw_place);
        let array_mark = self.w.mark();
        self.emitting_method_receiver = true;
        // The array of an addressed place is part of that place too
        // (`&grid[1][0].n`), so the flag goes down into it.
        self.emitting_raw_place = raw_place;
        self.emit_expr(&i.array);
        self.emitting_raw_place = false;
        self.emitting_method_receiver = false;
        // A collection is a reference type (§6.5.1): the `Index` impl is on
        // the sequence inside the cell, not on the handle. An `xs[i] = v` write
        // reaches here as the assignment's LHS, and `IndexMut` needs the
        // exclusive borrow - `Ref` derefs to `&Vec<T>`, which cannot be written
        // through (rustc E0596).
        if self.expr_is_collection_handle(&i.array) {
            if raw_place {
                // Under `&`: the element's address, with no guard to outlive
                // the statement (see `emitting_raw_place`).
                self.w.insert_at(array_mark, "(&mut *");
                self.w.push_str(".as_ptr())");
            } else {
                self.w.push_str(if emitting_lvalue {
                    ".borrow_mut()"
                } else {
                    ".borrow()"
                });
            }
        }
        self.emit_index_key(map_index, &i.index);
        // Rvalue index reads of non-Copy elements (String, value
        // classes, nested arrays) clone out — `xs[0]` would otherwise
        // move out of the Vec (rustc E0507). Lvalue positions
        // (`xs[i] = v`) and wrapper-class elements (whose share-clone
        // the wrapper machinery appends at the use site) are skipped.
        if !emitting_lvalue {
            if let Some(elem_ty) = self.expr_types.get(&i.span).cloned() {
                let needs = match &elem_ty {
                    juxc_tycheck::Ty::String | juxc_tycheck::Ty::Array { .. } => true,
                    // A generic type parameter (`T` of `Vec<T>` inside a
                    // generic class) is non-`Copy` in the general case, so an
                    // rvalue index read must clone out of the Vec — otherwise
                    // `self.data[i]` moves out of a borrowed `Vec<T>` (E0507).
                    // The enclosing impl already bounds the param `T: Clone`.
                    juxc_tycheck::Ty::Param(_) => true,
                    // A nullable element is an `Option`, which is `Copy` only
                    // when its payload is. `Obj? a = objs[0];` and
                    // `objs[0]!!` over a `Vec<Obj?>` moved the element out of
                    // the vector (E0507). The use-site share-clone a wrapper
                    // class gets does not reach inside an `Option`, so every
                    // non-primitive payload clones here.
                    juxc_tycheck::Ty::Nullable(inner) => {
                        !matches!(**inner, juxc_tycheck::Ty::Primitive(_))
                    }
                    juxc_tycheck::Ty::User { name, .. } => {
                        let bare = name.rsplit('.').next().unwrap_or(name);
                        // Wrapper classes share-clone at use sites; tuple
                        // sentinel and unknown names stay un-cloned.
                        (!self.is_wrapper_class(bare)
                            && (self.symbols.classes.contains_key(name.as_str())
                                || self
                                    .symbols
                                    .classes
                                    .keys()
                                    .any(|k| k.rsplit('.').next().unwrap_or(k) == bare)))
                            // A record is a value that derives `Clone`, and
                            // `var l = loans[0];` moved it out of the Vec.
                            || self.type_name_is_value_type(name)
                    }
                    _ => false,
                };
                if needs {
                    self.w.push_str(".clone()");
                }
            }
        }
    }

    /// Emit `[key]` in whichever of the three shapes the container wants.
    ///
    /// - A container whose real Rust `Index` impl takes a BORROWED key
    ///   (`map_index`, DISCOVERED from the stub's `@RustIndexRef` marker) gets
    ///   `[&(key)]`.
    /// - A bare integer literal indexes directly; Rust infers `usize`.
    /// - Anything else is a Jux `int` (`isize`) and needs the cast.
    ///
    /// Shared with the assignment path so an indexed WRITE shapes its key the
    /// same way an indexed read does -- the store used to hard-code the
    /// `as usize` form, which turned a map-typed field write into
    /// `("a".to_string()) as usize`.
    pub(crate) fn emit_index_key(&mut self, map_index: bool, key: &Expr) {
        self.w.push('[');
        if map_index {
            self.w.push_str("&(");
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            self.emit_expr(key);
            self.emitting_format_arg = prev;
            self.w.push(')');
        } else if matches!(key, Expr::Literal(Literal::Int(_))) {
            self.emit_expr(key);
        } else {
            self.w.push('(');
            self.emit_expr(key);
            self.w.push_str(") as usize");
        }
        self.w.push(']');
    }

    /// True when indexing `array` uses a BORROWED key (`map[&k]`) rather than
    /// the sequence cast. Discovered from the container's `@RustIndexRef`.
    pub(crate) fn index_takes_ref_key(&self, array: &Expr) -> bool {
        match self.expr_types.get(&crate::exprs::expr_span_of(array)) {
            Some(juxc_tycheck::Ty::User { name, .. }) => {
                let bare = name.rsplit('.').next().unwrap_or(name);
                self.class_indexes_by_ref(bare)
            }
            _ => false,
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
        // §6.5.2: constructing an array produces the shared handle.
        let handle = self.arrays_are_handles_here();
        let elem = n
            .element_type
            .name
            .segments
            .last()
            .map(|s| s.text.clone())
            .unwrap_or_default();
        let (open, close) = self.array_handle_new(&elem);
        if handle {
            self.w.push_str(open);
        }
        self.emit_new_array_inner(n);
        if handle {
            self.w.push_str(close);
        }
    }

    fn emit_new_array_inner(&mut self, n: &NewArrayExpr) {
        // Collect every dimension's size, outermost-first: the outer
        // `size` plus any `inner_sizes` from a multi-dim `new T[a][b]`.
        let mut sizes: Vec<&Expr> = Vec::with_capacity(1 + n.inner_sizes.len());
        sizes.push(&n.size);
        for s in &n.inner_sizes {
            sizes.push(s);
        }
        // Snapshot the LHS shape's per-dimension kinds (if any) so each
        // level can independently pick fixed vs dynamic — keeps borrow
        // of `self` out of the recursion.
        let target_dims: Vec<bool> = self
            .target_array_shape
            .as_ref()
            .map(|s| {
                s.dims
                    .iter()
                    .map(|d| matches!(d, juxc_ast::ArrayDim::Dynamic))
                    .collect()
            })
            .unwrap_or_default();
        // Whether there is a DECLARED array slot at all. Without one,
        // `new T[n]` is Java's array expression and produces `T[]` -- the
        // dynamic form -- because that is the only thing it can be passed to
        // (JUX-LANG-V1 5.6, and 563: "`T[]` and `T[N]` are interchangeable when
        // passing a fixed-size array to a function expecting a runtime-sized
        // one"). Defaulting to the stack form instead made `var b = new
        // ubyte[64]; read(b);` a type error between two spellings of `ubyte[]`.
        // The stack array is what an explicit `T[N]` slot asks for, and it stays
        // exactly that.
        let has_target = self.target_array_shape.is_some() || self.dynamic_array_target;
        self.emit_new_array_dim(n, &sizes, 0, &target_dims, has_target);
    }

    /// Recursively emit ONE dimension of a `new T[…]…` allocation,
    /// outermost-first. `sizes` holds every dimension's size
    /// (outermost-first); `depth` is the current dimension index;
    /// `target_dims[i]` is `true` when the LHS slot's i-th dimension is
    /// dynamic (empty when there's no array-typed LHS to consult).
    ///
    /// At each level we emit either a `Vec` repeat (`vec![inner; len]`)
    /// or a fixed-array repeat (`[inner; len]`), where `inner` is the
    /// recursively-emitted next dimension — or, at the innermost level,
    /// the element default value.
    fn emit_new_array_dim(
        &mut self,
        n: &NewArrayExpr,
        sizes: &[&Expr],
        depth: usize,
        target_dims: &[bool],
        has_target: bool,
    ) {
        let size = sizes[depth];
        let is_innermost = depth + 1 == sizes.len();

        // **Generic element** (`new T[N]` where `T` is a type param in
        // scope): the `[VALUE; N]` repeat form would require `T: Copy`
        // on top of `Default` — Jux generics carry `Clone`, not `Copy`.
        // `std::array::from_fn` evaluates the closure per element, so
        // only `T: Default` is needed (added to the class's bound by
        // `class_default_bound_params` when a `T[N]` field exists).
        // The array's size/type are inferred from the assignment
        // target, so `from_fn` needs no explicit length. Only relevant
        // at the innermost level (the element is the type param).
        let elem_is_type_param = is_innermost
            && n.element_type.array_shape.is_none()
            && !n.element_type.nullable
            && n.element_type.generic_args.is_empty()
            && n.element_type.fn_shape.is_none()
            && n.element_type.name.segments.len() == 1
            && self
                .current_type_params
                .contains(n.element_type.name.segments[0].text.as_str());

        // **Dynamic (heap `Vec`) dimension** — `int[] a = new int[N]`
        // (§5.6, Java-standard). Required whenever the size is a RUNTIME
        // value (a Rust `[v; N]` demands a *const* `N`). Otherwise the
        // dimension's kind is taken from the LHS slot: the outer dim
        // honors `dynamic_array_target` / `target_dims[0]`; inner dims
        // honor `target_dims[depth]`, defaulting to fixed (stack) when
        // there's no LHS shape to consult. A const-generic param
        // (`new T[N]` inside `<int N>` scope) is a compile-time
        // constant — it stays fixed, so it is NOT a runtime size.
        let size_is_const = self.try_const_int(size).is_some()
            || matches!(
                size,
                Expr::Path(qn)
                    if qn.segments.len() == 1
                        && self.const_int_params.contains(qn.segments[0].text.as_str())
            );
        let lhs_says_dynamic = if depth < target_dims.len() {
            target_dims[depth]
        } else if depth == 0 {
            // No per-dim shape but the legacy outer flag may still apply.
            self.dynamic_array_target
        } else {
            false
        };
        let want_dynamic = lhs_says_dynamic || !size_is_const || !has_target;

        if want_dynamic {
            if is_innermost && elem_is_type_param {
                self.w.push_str("(0..");
                self.emit_array_repeat_len(size);
                self.w
                    .push_str(").map(|_| Default::default()).collect::<Vec<_>>()");
                return;
            }
            // An INNER dimension is an array in its own right, so it gets its
            // own handle (§6.5.2) - `grid[1]` has to BE the row.
            //
            // And it cannot go through `vec![row; n]`, which clones one value
            // n times: cloning a handle shares it, so every row of
            // `new int[2][2]` would be the same array. Building each row
            // separately is what makes them distinct, which is what Java does
            // and what anyone writing a grid expects.
            let inner_is_handle = !is_innermost && self.arrays_are_handles_here();
            if inner_is_handle {
                let elem = n
                    .element_type
                    .name
                    .segments
                    .last()
                    .map(|s| s.text.clone())
                    .unwrap_or_default();
                let (open, close) = self.array_handle_new(&elem);
                self.w.push_str("(0..");
                self.emit_array_repeat_len(size);
                self.w.push_str(").map(|_| ");
                self.w.push_str(open);
                self.emit_new_array_dim(n, sizes, depth + 1, target_dims, has_target);
                self.w.push_str(close);
                self.w.push_str(").collect::<Vec<_>>()");
                return;
            }
            self.w.push_str("vec![");
            if is_innermost {
                self.emit_default_value_for(&n.element_type);
            } else {
                self.emit_new_array_dim(n, sizes, depth + 1, target_dims, has_target);
            }
            self.w.push_str("; ");
            self.emit_array_repeat_len(size);
            self.w.push(']');
            return;
        }

        // Fixed (stack `[…; N]`) dimension.
        if is_innermost && elem_is_type_param {
            self.w
                .push_str("std::array::from_fn(|_| Default::default())");
            return;
        }
        // Same rule as the dynamic branch: an inner dimension is an array of
        // its own, so it carries its own handle, and it cannot be built by
        // repetition -- `[row; n]` clones one value, and cloning a handle
        // shares it, so every row of `new int[3][4]` would be the same array.
        // `from_fn` builds each one separately.
        if !is_innermost && self.arrays_are_handles_here() {
            let elem = n
                .element_type
                .name
                .segments
                .last()
                .map(|x| x.text.clone())
                .unwrap_or_default();
            let (open, close) = self.array_handle_new(&elem);
            self.w.push_str("std::array::from_fn(|_| ");
            self.w.push_str(open);
            self.emit_new_array_dim(n, sizes, depth + 1, target_dims, has_target);
            self.w.push_str(close);
            self.w.push(')');
            return;
        }
        self.w.push('[');
        if is_innermost {
            self.emit_default_value_for(&n.element_type);
        } else {
            self.emit_new_array_dim(n, sizes, depth + 1, target_dims, has_target);
        }
        self.w.push_str("; ");
        self.emit_array_repeat_len(size);
        self.w.push(']');
    }

    /// Emit the repeat-length of a `new T[N]`: a const-evaluable length
    /// (`SIZE * 2`) becomes its computed `usize` literal (§T.11);
    /// otherwise the slot stays a raw `usize` expression (a
    /// const-generic `N`, or a runtime size for the `vec!` form), never
    /// the `(N as isize)` value-cast.
    fn emit_array_repeat_len(&mut self, size: &Expr) {
        // A const literal emits its computed `usize` value.
        if let Some(v) = self.try_const_int(size) {
            self.w.push_str(&v.to_string());
            return;
        }
        // A const-generic param `N` emits BARE — it's already a
        // `usize` const generic, and a const operation on it
        // (`N as usize`) is forbidden in a fixed-array `[T; N]`
        // position ("generic parameters may not be used in const
        // operations"). `in_array_size_position` keeps it raw.
        if let Expr::Path(qn) = size {
            if qn.segments.len() == 1
                && self.const_int_params.contains(qn.segments[0].text.as_str())
            {
                let prev = self.in_array_size_position;
                self.in_array_size_position = true;
                self.emit_expr(size);
                self.in_array_size_position = prev;
                return;
            }
        }
        // A RUNTIME `int` size is `isize`, but the `vec![v; N]` repeat
        // position wants `usize` — cast. (Runtime sizes only reach the
        // `vec!` / dynamic form; a fixed `[v; N]` always has a const
        // size handled above.)
        self.w.push('(');
        self.emit_expr(size);
        self.w.push_str(") as usize");
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
        // §6.5.2: an array literal produces the shared handle too.
        let handle = self.arrays_are_handles_here();
        // An array literal has no written element type; take it from the
        // literal's checked type, which is where the exception case shows up
        // (`new Exception[]{cause}` inside the Throwable chain).
        let elem = match self.expr_types.get(&n.span) {
            Some(juxc_tycheck::Ty::Array { element, .. }) => match element.as_ref() {
                juxc_tycheck::Ty::User { name, .. } => name.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let (open, close) = self.array_handle_new(&elem);
        if handle {
            self.w.push_str(open);
        }
        self.emit_new_array_lit_inner(n);
        if handle {
            self.w.push_str(close);
        }
    }

    fn emit_new_array_lit_inner(&mut self, n: &NewArrayLitExpr) {
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
        // A numeric element widens into the element type (§S.2.7): `double[]
        // a = {1, 2, 3}` is three doubles, and `long[] b = {anInt}` a long.
        let widen = self
            .type_ref_primitive(element_type)
            .and_then(|t| self.numeric_widen_or_arm(elem, t));
        let widen_inner = widen.is_some() && crate::exprs::cast_needs_inner_parens(elem);
        if widen.is_some() {
            self.w.push('(');
            if widen_inner {
                self.w.push('(');
            }
        }
        self.emit_expr(elem);
        if self.wrapper_value_needs_clone(elem) || self.value_place_needs_clone(elem) {
            self.w.push_str(".clone()");
        }
        if let Some(cast) = widen {
            if widen_inner {
                self.w.push(')');
            }
            self.w.push_str(" as ");
            self.w.push_str(cast);
            self.w.push(')');
        }
    }
}

/// True when an annotation is the bindgen `@RustIndexRef` marker — the type
/// indexes by reference (`&map[k]`), which bindgen reads from its real trait
/// impls. Annotations are case-insensitive per the Jux rules.
fn is_index_ref(a: &juxc_ast::Annotation) -> bool {
    a.name.segments.len() == 1 && a.name.segments[0].text.eq_ignore_ascii_case("rustindexref")
}
