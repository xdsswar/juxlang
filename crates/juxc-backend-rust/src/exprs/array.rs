//! Array-shaped expression emitters — indexing, `new T[size]` fills,
//! and `new T[]{a, b, c}` (or the bare `{a, b, c}`) initializer lit.

use juxc_ast::{Expr, IndexExpr, Literal, NewArrayExpr, NewArrayLitExpr};

use crate::RustEmitter;

impl RustEmitter {
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
        self.emit_expr(&i.array);
        self.w.push('[');
        if matches!(&*i.index, Expr::Literal(Literal::Int(_))) {
            self.emit_expr(&i.index);
        } else {
            self.w.push('(');
            self.emit_expr(&i.index);
            self.w.push_str(") as usize");
        }
        self.w.push(']');
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
        self.w.push('[');
        self.emit_default_value_for(&n.element_type);
        self.w.push_str("; ");
        self.emit_expr(&n.size);
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
                self.emit_expr(elem);
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
            self.emit_expr(elem);
        }
        self.w.push(']');
    }
}
