//! Function-pointer values (Layout-ABI §L.6.4).
//!
//! A Jux `fn(A) -> R` lowers to `Option<unsafe extern "C" fn(A') -> R'>`, with
//! each primed type the C type of that name (§8.1.1). A free function or a
//! lambda that captures nothing, given where such a pointer is expected, needs
//! an `extern "C"` entry point: the Jux function itself follows the Rust
//! calling convention and takes Jux widths. This module writes that entry
//! point beside the value, as a small nested function:
//!
//! ```rust,ignore
//! Some({
//!     unsafe extern "C" fn __jux_fn(a0: core::ffi::c_int) -> core::ffi::c_int {
//!         let a0 = a0 as isize;
//!         let __r = twice(a0);
//!         __r as core::ffi::c_int
//!     }
//!     __jux_fn
//! })
//! ```
//!
//! The conversions are the ones a native call makes, in the other direction:
//! a C integer widens to the Jux type of the same name, a `const char*` is
//! copied into a `String`, and the result goes back at its C width; a `String`
//! result is kept by the entry point until its next call (§L.3.2). A lambda
//! is first written out as an ordinary function, `__jux_lambda`, and the entry
//! point calls that; tycheck has already made sure it captures nothing, so it
//! means exactly what it meant in place.

use juxc_ast::{Expr, FnTypeShape, Ident, QualifiedName, TypeRef};
use juxc_source::Span;
use juxc_tycheck::Ty;

use crate::RustEmitter;

/// A `TypeRef` naming `name` with no decorations.
fn named(name: &str) -> TypeRef {
    TypeRef {
        name: QualifiedName {
            segments: vec![Ident { text: name.to_string(), span: Span::DUMMY }],
            span: Span::DUMMY,
        },
        generic_args: Vec::new(),
        nullable: false,
        array_shape: None,
        fn_shape: None,
        ptr_depth: 0,
        span: Span::DUMMY,
    }
}

/// Whether `t` is the bare `void` name (a result type with no value).
pub(crate) fn type_ref_is_void_name(t: &TypeRef) -> bool {
    t.fn_shape.is_none()
        && t.ptr_depth == 0
        && t.array_shape.is_none()
        && t.name.segments.len() == 1
        && t.name.segments[0].text == "void"
}

/// The written form of one type in a function-pointer signature, rebuilt from
/// the checker's `Ty` and the pointer depth it keeps beside it. The C
/// signature is decided by these names (§8.1.1), so the rebuild only has to
/// give back the name the program wrote: a primitive, `String`, a user type,
/// `void`, a pointer to any of them, or another function pointer.
pub(crate) fn fn_pointer_sig_type_ref(ty: &Ty, ptr_depth: u8) -> TypeRef {
    let mut t = match ty {
        Ty::FnPtr { params, param_ptr_depths, return_type, return_ptr_depth } => {
            let mut t = named("");
            t.name.segments.clear();
            t.fn_shape = Some(Box::new(FnTypeShape {
                params: params
                    .iter()
                    .zip(param_ptr_depths.iter().chain(std::iter::repeat(&0)))
                    .map(|(p, d)| fn_pointer_sig_type_ref(p, *d))
                    .collect(),
                return_type: fn_pointer_sig_type_ref(return_type, *return_ptr_depth),
                is_async: false,
                throws: Vec::new(),
                is_pointer: true,
            }));
            t
        }
        // `void*` reaches the checker as a pointer to nothing it can name.
        Ty::Void | Ty::Unknown => named("void"),
        other => crate::analysis::ty_to_type_ref(other).unwrap_or_else(|| named("void")),
    };
    t.ptr_depth = ptr_depth;
    t
}

impl RustEmitter {
    /// Emit `value` -- a free function's name or a lambda -- as the function
    /// pointer `slot`. Returns `false` when `value` is not one of those (a
    /// local that already holds a pointer, say), leaving it to the ordinary
    /// expression path.
    pub(crate) fn emit_fn_pointer_value(&mut self, value: &Expr, slot: &Ty) -> bool {
        let Ty::FnPtr { params, param_ptr_depths, return_type, return_ptr_depth } = slot else {
            return false;
        };
        let param_refs: Vec<TypeRef> = params
            .iter()
            .zip(param_ptr_depths.iter().chain(std::iter::repeat(&0)))
            .map(|(p, d)| fn_pointer_sig_type_ref(p, *d))
            .collect();
        let ret_ref = fn_pointer_sig_type_ref(return_type, *return_ptr_depth);
        let returns = !type_ref_is_void_name(&ret_ref);

        // What the entry point calls.
        let target: QualifiedName = match value {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                let is_local = self.current_fn_params.contains(name)
                    || self.local_types.iter().any(|scope| scope.contains_key(name));
                let suffix = format!(".{name}");
                let is_function =
                    self.symbols.functions.keys().any(|k| k == name || k.ends_with(&suffix));
                if is_local || !is_function {
                    return false;
                }
                QualifiedName {
                    segments: vec![Ident { text: name.to_string(), span: Span::DUMMY }],
                    span: Span::DUMMY,
                }
            }
            Expr::Lambda(_) => QualifiedName {
                segments: vec![Ident { text: "__jux_lambda".to_string(), span: Span::DUMMY }],
                span: Span::DUMMY,
            },
            _ => return false,
        };

        // The value is written in the middle of whatever expression holds it;
        // none of that context applies inside the functions below.
        let prev_format = std::mem::take(&mut self.emitting_format_arg);
        let prev_lvalue = std::mem::take(&mut self.emitting_lvalue);
        let prev_receiver = std::mem::take(&mut self.emitting_method_receiver);
        let prev_callee = std::mem::take(&mut self.emitting_call_callee);
        let prev_class = self.enclosing_class.take();
        let prev_this = self.this_alias.take();

        self.w.push_str("Some({\n");
        self.w.indent_inc();

        // A lambda becomes an ordinary Jux function first.
        if let Expr::Lambda(l) = value {
            let params = l
                .params
                .iter()
                .zip(&param_refs)
                .map(|(p, ty)| juxc_ast::Param {
                    annotations: Vec::new(),
                    name: p.name.clone(),
                    ty: p.ty.clone().unwrap_or_else(|| ty.clone()),
                    is_final: false,
                    final_kw: juxc_ast::FinalKw::None,
                    is_ref: false,
                    is_mut_ref: false,
                    default: None,
                    is_varargs: false,
                    is_out: false,
                    is_shared_ref: false,
                    is_weak: false,
                    span: p.span,
                })
                .collect();
            let body = match &l.body {
                juxc_ast::LambdaBody::Block(b) => (**b).clone(),
                juxc_ast::LambdaBody::Expr(e) => juxc_ast::Block {
                    statements: vec![if returns {
                        juxc_ast::Stmt::Return(Some((**e).clone()), l.span)
                    } else {
                        juxc_ast::Stmt::Expr((**e).clone())
                    }],
                    span: l.span,
                },
            };
            // `unsafe`, because the lambda may be written inside an `unsafe`
            // block and use what that block allows (`p -> *p + 1`); tycheck
            // has already judged the body in its real context.
            let decl = juxc_ast::FnDecl {
                annotations: Vec::new(),
                visibility: juxc_ast::Visibility::Private,
                modifiers: vec![juxc_ast::FnModifier::Unsafe],
                return_type: if returns {
                    juxc_ast::ReturnType::Type(ret_ref.clone())
                } else {
                    juxc_ast::ReturnType::Void
                },
                name: target.segments[0].clone(),
                generic_params: Vec::new(),
                params,
                throws: Vec::new(),
                wheres: Vec::new(),
                body: Some(body),
                is_property: false,
                is_c_variadic: false,
                span: l.span,
            };
            // `emit_fn_decl` is written for a top-level function: it replaces
            // the per-function sets rather than nesting them. The function
            // being emitted around this value still needs its own afterwards
            // (without `mutated_in_fn`, a later `let` lost its `mut`).
            let outer_mutated = std::mem::take(&mut self.mutated_in_fn);
            let outer_nullable = std::mem::take(&mut self.nullable_locals);
            let outer_refs = std::mem::take(&mut self.ref_locals);
            let outer_weak = std::mem::take(&mut self.weak_params);
            self.emit_fn_decl(&decl);
            self.mutated_in_fn = outer_mutated;
            self.nullable_locals = outer_nullable;
            self.ref_locals = outer_refs;
            self.weak_params = outer_weak;
        }

        // The `extern "C"` entry point.
        self.w.emit_indent();
        self.w.push_str("unsafe extern \"C\" fn __jux_fn(");
        for (i, p) in param_refs.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&format!("a{i}: "));
            self.emit_ffi_type(p);
        }
        self.w.push(')');
        if returns {
            self.w.push_str(" -> ");
            self.emit_ffi_type(&ret_ref);
        }
        self.w.push_str(" {\n");
        self.w.indent_inc();
        // Inbound: each argument from its C type to the Jux one.
        let mut locals = std::collections::HashMap::new();
        for (i, p) in param_refs.iter().enumerate() {
            let jux = juxc_tycheck::ty_from_ref_in_env(p, &self.symbols);
            if p.ptr_depth == 0 && p.fn_shape.is_none() {
                match p.name.segments.last().map(|s| s.text.as_str()) {
                    Some("String") => self.w.line(&format!(
                        "let a{i} = if a{i}.is_null() {{ String::new() }} else {{ \
                         ::std::ffi::CStr::from_ptr(a{i}).to_string_lossy().into_owned() }};"
                    )),
                    Some("char") => self.w.line(&format!("let a{i} = (a{i} as u8) as char;")),
                    Some(name) if crate::decls::functions::c_abi_type(name).is_some() => {
                        if let Some(rust) = crate::types::jux_primitive_to_rust(p) {
                            if crate::decls::functions::c_abi_type(name) != Some(rust) {
                                self.w.line(&format!("let a{i} = a{i} as {rust};"));
                            }
                        }
                    }
                    _ => {}
                }
            }
            locals.insert(format!("a{i}"), jux);
        }
        // The call, through the ordinary call emission so a function in
        // another package, or one that needs its arguments cloned, is reached
        // the way any other call reaches it.
        let call = Expr::Call(juxc_ast::CallExpr {
            callee: Box::new(Expr::Path(target)),
            explicit_generic_args: Vec::new(),
            args: (0..param_refs.len())
                .map(|i| {
                    Expr::Path(QualifiedName {
                        segments: vec![Ident { text: format!("a{i}"), span: Span::DUMMY }],
                        span: Span::DUMMY,
                    })
                })
                .collect(),
            arg_names: vec![None; param_refs.len()],
            eval_order: Vec::new(),
            span: Span::DUMMY,
        });
        self.local_types.push(locals);
        self.w.emit_indent();
        if returns {
            self.w.push_str("let __r = ");
        }
        // C calls this entry point, so an exception must not unwind out of it
        // (Exceptions §X.6.5): the call runs behind the FFI barrier.
        self.w.push_str("crate::__jux_ffi_barrier(\"a function pointer\", || ");
        self.emit_expr(&call);
        self.w.push_str(");\n");
        self.local_types.pop();
        // Outbound: the result at its C width.
        if returns {
            let last = ret_ref.name.segments.last().map(|s| s.text.as_str());
            if ret_ref.ptr_depth == 0 && ret_ref.fn_shape.is_none() {
                match last {
                    // Kept by this entry point until its next call, not leaked.
                    Some("String") => self.emit_held_c_string_return(),
                    Some("char") => self.w.line("__r as core::ffi::c_char"),
                    Some(name) => match crate::decls::functions::c_abi_type(name) {
                        Some(c) if crate::types::jux_primitive_to_rust(&ret_ref) != Some(c) => {
                            self.w.line(&format!("__r as {c}"))
                        }
                        _ => self.w.line("__r"),
                    },
                    None => self.w.line("__r"),
                }
            } else {
                self.w.line("__r");
            }
        }
        self.w.indent_dec();
        self.w.line("}");
        // Cast to the pointer type: a function ITEM has a type of its own, and
        // only a slot that already names the pointer type coerces it. A value
        // hoisted into a temporary first would not.
        self.w.emit_indent();
        self.w.push_str("__jux_fn as unsafe extern \"C\" fn(");
        for (i, p) in param_refs.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.emit_ffi_type(p);
        }
        self.w.push(')');
        if returns {
            self.w.push_str(" -> ");
            self.emit_ffi_type(&ret_ref);
        }
        self.w.push('\n');
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("})");

        self.emitting_format_arg = prev_format;
        self.emitting_lvalue = prev_lvalue;
        self.emitting_method_receiver = prev_receiver;
        self.emitting_call_callee = prev_callee;
        self.enclosing_class = prev_class;
        self.this_alias = prev_this;
        true
    }
}
