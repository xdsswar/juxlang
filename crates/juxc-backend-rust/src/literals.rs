//! Literal emission — numbers, strings, format strings, escape helpers,
//! and the per-level indent helper.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::Literal;

use crate::RustEmitter;

impl RustEmitter {
    /// Emit a literal in Rust source form, escaping strings as needed.
    ///
    /// **Integers** are emitted unsuffixed when their kind is the
    /// default `int` (Rust's default i32 — no annotation needed) and
    /// **with the appropriate Rust suffix** otherwise. So Jux `42` →
    /// Rust `42`, but Jux `42L` → Rust `42i64`.
    ///
    /// **Floats** are emitted with `.0` when the value has no fractional
    /// part (so Rust doesn't accidentally parse them as integers) and a
    /// type suffix for `f`-suffixed Jux literals.
    pub(crate) fn emit_literal(&mut self, lit: &Literal) {
        match lit {
            Literal::Int(int_lit) => self.emit_int_lit(int_lit),
            Literal::Float(float_lit) => self.emit_float_lit(float_lit),
            Literal::String(s) => {
                // Per JUX-CODEGEN-FIXES.md Fix 1: every Jux string
                // literal lowers to an owned Rust `String` via
                // `.to_string()`. This unifies the type of every
                // string source in emitted Rust — literals,
                // parameters, fields, returns — so match-arm
                // collisions and assignment coercions evaporate.
                //
                // Two contexts skip the self-coerce:
                //
                // - **const-context**: Rust's const evaluator can't
                //   run `.to_string()`, so a `pub const NAME: &str =
                //   "…";` keeps the bare literal shape (the
                //   const-context type emitter pairs by mapping
                //   String to `&'static str`).
                // - **format-arg context**: `format!`/`println!`
                //   borrow their args via `Display`, so a `&'static
                //   str` is just as good as an owned `String`. We
                //   skip the alloc to keep emitted Rust both correct
                //   AND efficient — Fix 1's "everything is owned"
                //   stance was about *types flowing through user
                //   code*, not about forcing heap allocs in places
                //   where the borrow shape was always fine.
                self.emit_rust_string_literal(s);
                if !self.emitting_const_context && !self.emitting_format_arg {
                    self.w.push_str(".to_string()");
                }
            }
            Literal::Bool(b) => self.w.push_str(if *b { "true" } else { "false" }),
            // Rust char literal — re-escape control/quote characters
            // (the parser already decoded the Jux escape into the raw
            // `char`) so the emitted source stays valid Rust.
            Literal::Char(c) => {
                self.w.push('\'');
                match c {
                    '\n' => self.w.push_str("\\n"),
                    '\r' => self.w.push_str("\\r"),
                    '\t' => self.w.push_str("\\t"),
                    '\\' => self.w.push_str("\\\\"),
                    '\'' => self.w.push_str("\\'"),
                    '\0' => self.w.push_str("\\0"),
                    other => self.w.push(*other),
                }
                self.w.push('\'');
            }
            // `null` is the empty value of an `Option<T>`. We always
            // emit `None` and let Rust's type inference fill in the
            // `T`; var decls / returns / fn args carry an explicit
            // `Option<T>` annotation that pins the type. When the
            // surrounding context can't infer T (a free-standing
            // `null` expression), rustc surfaces the ambiguity with
            // a clear "cannot infer" message at the user's site.
            Literal::Null => self.w.push_str("None"),
        }
    }

    /// Emit an integer literal: value in its original radix + Rust type
    /// suffix when needed.
    ///
    /// Radix preservation: a Jux `0xF0` lowers to a Rust `0xF0`, a `0b1010`
    /// to `0b1010`, a `0o17` to `0o17`, and decimal stays decimal.
    ///
    /// Leading-zero preservation: `0x0F` stays `0x0F`, `0b0001` stays
    /// `0b0001`. The `digit_width` field on [`IntLit`] carries the
    /// source's digit count (after underscore stripping); we use it as
    /// the width specifier in `format!`.
    ///
    /// Hex emission uses uppercase digits (Rust style: `0xFF`, not `0xff`).
    /// We don't preserve the user's exact letter case, just the base
    /// and the digit count.
    pub(crate) fn emit_int_lit(&mut self, lit: &juxc_ast::IntLit) {
        // An unsuffixed integer literal in a float slot (`d += 1`, `double[]
        // a = {1, 2}`) is written as the float it becomes (§S.2.7).
        if lit.kind.is_none() && std::mem::take(&mut self.int_literal_as_float) {
            self.w.push_str(&format!("{}.0", lit.value));
            return;
        }
        let width = lit.digit_width as usize;
        match lit.radix {
            juxc_ast::IntRadix::Decimal => {
                // A `uL` literal keeps its bits in the `i64` (see the parser).
                if lit.kind == Some(juxc_ast::IntKind::ULong) {
                    self.w.push_str(&(lit.value as u64).to_string());
                } else {
                    self.w.push_str(&lit.value.to_string());
                }
            }
            juxc_ast::IntRadix::Hex => {
                self.w.push_str(&format!("0x{:0width$X}", lit.value, width = width));
            }
            juxc_ast::IntRadix::Binary => {
                self.w.push_str(&format!("0b{:0width$b}", lit.value, width = width));
            }
            juxc_ast::IntRadix::Octal => {
                self.w.push_str(&format!("0o{:0width$o}", lit.value, width = width));
            }
        }
        if let Some(kind) = lit.kind {
            self.w.push_str(kind.as_rust_suffix());
        }
    }

    /// Emit a float literal: the shortest spelling of its value that reads
    /// back exactly, plus a suffix for an `f`-suffixed Jux literal or for a
    /// double too large to be read as a `float`.
    pub(crate) fn emit_float_lit(&mut self, lit: &juxc_ast::FloatLit) {
        // `Debug` is the shortest spelling that reads back as the value and
        // keeps an exponent (`1e40`, not forty digits), and always has a `.` or
        // an `e`, so Rust never reads it as an integer.
        let s = format!("{:?}", lit.value);
        self.w.push_str(&s);
        if let Some(kind) = lit.kind {
            self.w.push_str(kind.as_rust_suffix());
        } else if lit.value.is_finite() && lit.value.abs() > f64::from(f32::MAX) {
            // A double literal beyond `float`'s range, as in `(float) 1e40`.
            // Rust types an unsuffixed literal from its cast, so `1e40 as f32`
            // read the literal as an out-of-range `f32` and refused to build;
            // the cast itself is `Infinity`, as §S.2.4 says.
            self.w.push_str("f64");
        }
    }

    /// Emit a Jux string in Rust source form, escaping the characters
    /// that have special meaning inside `"..."`. The Jux lexer hands us
    /// the raw bytes between Jux's quotes; we re-escape those for Rust.
    /// The folded value for a `String`-typed const slot whose initializer is
    /// not already a plain literal, or `None` to emit the initializer as it
    /// stands (§T.11.7).
    ///
    /// A literal is left alone deliberately: the ordinary literal path already
    /// emits it, and it may be a raw string whose exact spelling is worth
    /// keeping in the generated Rust.
    pub(crate) fn const_string_fold(
        &self,
        ty: &juxc_ast::TypeRef,
        init: &juxc_ast::Expr,
    ) -> Option<String> {
        if ty.array_shape.is_some()
            || ty.nullable
            || !crate::analysis::is_jux_string_type(ty)
            || matches!(init, juxc_ast::Expr::Literal(juxc_ast::Literal::String(_)))
        {
            return None;
        }
        self.try_const_string(init)
    }

    pub(crate) fn emit_rust_string_literal(&mut self, s: &str) {
        self.w.push('"');
        for c in s.chars() {
            self.push_escaped_for_rust(c, /*format_string=*/ false);
        }
        self.w.push('"');
    }

    /// Same shape as [`Self::emit_rust_string_literal`], but additionally
    /// doubles `{` and `}` so the literal can be used as a `println!`
    /// format string without the macro parser mis-reading them as
    /// placeholders.
    pub(crate) fn emit_rust_format_string_literal(&mut self, s: &str) {
        self.w.push('"');
        for c in s.chars() {
            self.push_escaped_for_rust(c, /*format_string=*/ true);
        }
        self.w.push('"');
    }

    /// Push a single character into `self.out`, applying the appropriate
    /// Rust escape. When `format_string` is true, `{` and `}` are
    /// additionally doubled so format-macro parsers leave them alone.
    pub(crate) fn push_escaped_for_rust(&mut self, c: char, format_string: bool) {
        match c {
            '"' => self.w.push_str("\\\""),
            '\\' => self.w.push_str("\\\\"),
            '\n' => self.w.push_str("\\n"),
            '\r' => self.w.push_str("\\r"),
            '\t' => self.w.push_str("\\t"),
            '{' if format_string => self.w.push_str("{{"),
            '}' if format_string => self.w.push_str("}}"),
            c => self.w.push(c),
        }
    }

}
