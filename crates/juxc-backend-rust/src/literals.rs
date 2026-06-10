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
        let width = lit.digit_width as usize;
        match lit.radix {
            juxc_ast::IntRadix::Decimal => {
                self.w.push_str(&lit.value.to_string());
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

    /// Emit a float literal: value formatted to keep its float-ness,
    /// plus optional `f32` suffix.
    ///
    /// `f64::to_string()` may produce `"3"` for `3.0` (and Rust would
    /// then parse it as integer). We append `.0` when the formatted
    /// text contains neither a `.` nor an `e`, so the emitted literal is
    /// unambiguously a float to rustc.
    pub(crate) fn emit_float_lit(&mut self, lit: &juxc_ast::FloatLit) {
        let s = lit.value.to_string();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            self.w.push_str(&s);
        } else {
            self.w.push_str(&s);
            self.w.push_str(".0");
        }
        if let Some(kind) = lit.kind {
            self.w.push_str(kind.as_rust_suffix());
        }
    }

    /// Emit a Jux string in Rust source form, escaping the characters
    /// that have special meaning inside `"..."`. The Jux lexer hands us
    /// the raw bytes between Jux's quotes; we re-escape those for Rust.
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
