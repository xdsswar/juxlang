//! Unit tests for the parser.
//!
//! Tests exercise §A.2 productions called out in the parser's coverage
//! comment, plus the milestone-1 vehicle (the full `hello.jux` AST shape).

use crate::parse;
use juxc_ast::{Expr, FnDecl, Literal, ReturnType, Stmt, TopLevelDecl, Visibility};
use juxc_lex::lex;
use juxc_source::SourceFile;

/// Tokenize + parse `src`, asserting that no diagnostics fired. Returns
/// the parsed `CompilationUnit` for inspection.
fn parse_clean(src: &str) -> juxc_ast::CompilationUnit {
    let sf = SourceFile::new("test.jux", src);
    let lex_result = lex(&sf);
    assert!(
        lex_result.diagnostics.is_empty(),
        "unexpected lexer diagnostics: {:?}",
        lex_result.diagnostics,
    );
    let parse_result = parse(&lex_result.tokens);
    assert!(
        parse_result.diagnostics.is_empty(),
        "unexpected parser diagnostics: {:?}",
        parse_result.diagnostics,
    );
    parse_result.ast
}

/// Tokenize + parse `src`, expecting at least one parser diagnostic.
/// Returns the (possibly partial) AST and the diagnostic count.
fn parse_with_errors(src: &str) -> (juxc_ast::CompilationUnit, usize) {
    let sf = SourceFile::new("test.jux", src);
    let lex_result = lex(&sf);
    let parse_result = parse(&lex_result.tokens);
    let n = parse_result.diagnostics.len();
    (parse_result.ast, n)
}

/// True when parsing `src` produces a diagnostic with `code`.
fn parse_has_code(src: &str, code: juxc_diagnostics::code::Code) -> bool {
    let sf = SourceFile::new("test.jux", src);
    let lex_result = lex(&sf);
    let parse_result = parse(&lex_result.tokens);
    parse_result.diagnostics.iter().any(|d| d.code == code)
}

// ---------------------------------------------------------------------------
// Contextual keywords in member position (consuming Rust APIs)
// ---------------------------------------------------------------------------

/// After `.` / `?.` a reserved keyword is just a member name — so a Rust API
/// member like `WindowOptions::default()` / `value.type()` is callable. The
/// keyword-ness is purely contextual (statement position is unaffected).
#[test]
fn keyword_in_member_position_parses() {
    parse_clean(
        "public void f() {\n\
         \x20   var a = opts.default();\n\
         \x20   var b = node.type;\n\
         \x20   var c = handle?.match;\n\
         }",
    );
}

/// The same word stays a keyword in statement position: a `default ->` arm in a
/// switch must still parse as the switch default, not a member access.
#[test]
fn keyword_in_statement_position_still_keyword() {
    parse_clean(
        "public int f(int x) {\n\
         \x20   return switch (x) {\n\
         \x20       case 1 -> 10;\n\
         \x20       default -> 0;\n\
         \x20   };\n\
         }",
    );
}

// ---------------------------------------------------------------------------
// §M.14.5 — parameter binding-mode combination matrix
// ---------------------------------------------------------------------------

/// `ref weak T` (and `weak ref T`) — mutually exclusive binding modes (E0466).
#[test]
fn ref_and_weak_param_is_e0466() {
    use juxc_diagnostics::code::Code;
    assert!(parse_has_code(
        "public class N { } public void f(ref weak N n) { }",
        Code::E0466_InvalidParamBindingCombo,
    ));
    assert!(parse_has_code(
        "public class N { } public void f(weak ref N n) { }",
        Code::E0466_InvalidParamBindingCombo,
    ));
}

/// `ref T...` / `weak T...` — a binding mode on a varargs parameter (E0466).
#[test]
fn binding_mode_varargs_is_e0466() {
    use juxc_diagnostics::code::Code;
    assert!(parse_has_code(
        "public void f(ref int... xs) { }",
        Code::E0466_InvalidParamBindingCombo,
    ));
    assert!(parse_has_code(
        "public class N { } public void f(weak N... ns) { }",
        Code::E0466_InvalidParamBindingCombo,
    ));
}

/// `weak T = default` — a defaulted weak parameter (E0466).
#[test]
fn weak_default_param_is_e0466() {
    use juxc_diagnostics::code::Code;
    assert!(parse_has_code(
        "public class N { } public void f(weak N n = null) { }",
        Code::E0466_InvalidParamBindingCombo,
    ));
}

/// `out ref T` / `out weak T` — `out` is incompatible with a binding mode (E0944).
#[test]
fn out_with_binding_mode_is_e0944() {
    use juxc_diagnostics::code::Code;
    assert!(parse_has_code(
        "public void f(out ref int x) { }",
        Code::E0944_OutParamModifierMisuse,
    ));
    assert!(parse_has_code(
        "public class N { } public void f(out weak N n) { }",
        Code::E0944_OutParamModifierMisuse,
    ));
}

/// Sensible combinations parse cleanly: `final ref`, `final weak`,
/// `final T = default`, `final T...`.
#[test]
fn sensible_param_combos_parse_clean() {
    parse_clean("public class N { } public void f(final ref int x, final weak N n) { }");
    parse_clean("public void f(final int a = 1, final int... xs) { }");
}

// ---------------------------------------------------------------------------
// §L.7-L.8 — `delete` is not a keyword (E0507)
// ---------------------------------------------------------------------------

/// `delete p;` is guided to the drop-block model (E0507), NOT misparsed as a
/// typed local (which would have surfaced a downstream `E0304 cannot find type
/// 'delete'`).
#[test]
fn delete_local_is_e0507() {
    use juxc_diagnostics::code::Code;
    assert!(parse_has_code(
        "public void main() { delete p; }",
        Code::E0507_NoDeleteKeyword,
    ));
}

/// `delete this.buf;` and `delete *p;` also trip the guidance.
#[test]
fn delete_member_and_deref_are_e0507() {
    use juxc_diagnostics::code::Code;
    assert!(parse_has_code(
        "public void main() { delete this.buf; }",
        Code::E0507_NoDeleteKeyword,
    ));
    assert!(parse_has_code(
        "public void main() { delete *p; }",
        Code::E0507_NoDeleteKeyword,
    ));
}

/// `delete` stays usable as an ordinary identifier — a call `delete(x)`, an
/// assignment `delete = v`, or a member access `delete.run()` must NOT trip
/// E0507 (a second operand never follows in those shapes).
#[test]
fn delete_as_identifier_is_not_e0507() {
    use juxc_diagnostics::code::Code;
    assert!(!parse_has_code(
        "public void main() { delete(x); }",
        Code::E0507_NoDeleteKeyword,
    ));
    assert!(!parse_has_code(
        "public void main() { delete = 5; }",
        Code::E0507_NoDeleteKeyword,
    ));
    assert!(!parse_has_code(
        "public void main() { delete.run(); }",
        Code::E0507_NoDeleteKeyword,
    ));
}

// ---------------------------------------------------------------------------
// §L.7 — C FFI `unsafe native` blocks
// ---------------------------------------------------------------------------

/// `@extern(lib="…") unsafe native { … }` parses to a `TopLevelDecl::ExternBlock`
/// carrying the lib name and one bodyless, implicitly-`unsafe` fn per signature.
#[test]
fn extern_native_block_parses() {
    use juxc_ast::{FnModifier, TopLevelDecl};
    let ast = parse_clean(
        "@extern(lib = \"c\") unsafe native { \
            void* malloc(ulong size); void free(void* p); i32 puts(String s); \
         }",
    );
    let TopLevelDecl::ExternBlock(b) = &ast.items[0] else {
        panic!("expected ExternBlock, got {:?}", ast.items[0]);
    };
    assert_eq!(b.lib, "c");
    assert_eq!(b.fns.len(), 3);
    assert!(b.fns.iter().all(|f| f.body.is_none()), "foreign fns are bodyless");
    assert!(
        b.fns.iter().all(|f| f.modifiers.contains(&FnModifier::Unsafe)),
        "every foreign fn is implicitly unsafe"
    );
}

/// A foreign signature with a trailing `...` parses to a C-variadic fn:
/// `is_c_variadic` is set, the fixed params are kept, and the `...` is not a
/// parameter (§L.4.2). A non-variadic signature keeps `is_c_variadic == false`.
#[test]
fn extern_variadic_signature_parses() {
    use juxc_ast::TopLevelDecl;
    let ast = parse_clean(
        "@extern(lib = \"c\") unsafe native { \
            i32 printf(String fmt, ...); i32 puts(String s); \
         }",
    );
    let TopLevelDecl::ExternBlock(b) = &ast.items[0] else {
        panic!("expected ExternBlock, got {:?}", ast.items[0]);
    };
    let printf = &b.fns[0];
    assert!(printf.is_c_variadic, "printf should be C-variadic");
    assert_eq!(printf.params.len(), 1, "the `...` is not a fixed parameter");
    assert_eq!(printf.params[0].name.text, "fmt");
    assert!(!b.fns[1].is_c_variadic, "puts is not variadic");
}

/// A `native` block missing its `@extern(lib=…)` name, or missing `unsafe`,
/// is a diagnostic (but still parses for recovery).
#[test]
fn extern_block_missing_lib_or_unsafe_diagnoses() {
    let (_, n1) = parse_with_errors("@extern unsafe native { void f(); }");
    assert!(n1 >= 1, "missing lib name should diagnose");
    let (_, n2) = parse_with_errors("@extern(lib = \"c\") native { void f(); }");
    assert!(n2 >= 1, "missing `unsafe` should diagnose");
}

// ---------------------------------------------------------------------------
// Triviality
// ---------------------------------------------------------------------------

/// An empty source must parse to an empty compilation unit, no diagnostics.
#[test]
fn empty_source_yields_empty_compilation_unit() {
    let ast = parse_clean("");
    assert!(ast.package.is_none());
    assert!(ast.imports.is_empty());
    assert!(ast.items.is_empty());
}

/// Whitespace-only and comment-only inputs are equivalent to empty.
#[test]
fn whitespace_only_yields_empty_compilation_unit() {
    let ast = parse_clean("   \n\n  // hi\n  /* bye */ \n");
    assert!(ast.items.is_empty());
}

// ---------------------------------------------------------------------------
// Function declarations
// ---------------------------------------------------------------------------

/// `public void main() { }` is the smallest legal Jux entry point.
#[test]
fn empty_main_function() {
    let ast = parse_clean("public void main() { }");
    assert_eq!(ast.items.len(), 1);
    let TopLevelDecl::Function(fn_decl) = &ast.items[0] else {
        panic!("expected a function top-level decl");
    };
    assert_eq!(fn_decl.visibility, Visibility::Public);
    assert!(matches!(fn_decl.return_type, ReturnType::Void));
    assert_eq!(fn_decl.name.text, "main");
    assert!(fn_decl.params.is_empty());
    let body = fn_decl.body.as_ref().expect("body present");
    assert!(body.statements.is_empty());
}

/// No visibility modifier means `Visibility::Package` per §A.2.2.
#[test]
fn missing_visibility_defaults_to_package_private() {
    let ast = parse_clean("void noop() { }");
    let TopLevelDecl::Function(fn_decl) = &ast.items[0] else {
        panic!("expected a function top-level decl");
    };
    assert_eq!(fn_decl.visibility, Visibility::Package);
}

/// The four visibility keywords all classify correctly.
#[test]
fn all_visibilities_parse() {
    let cases = [
        ("public void a() {}",    Visibility::Public),
        ("internal void b() {}",  Visibility::Internal),
        ("protected void c() {}", Visibility::Protected),
        ("private void d() {}",   Visibility::Private),
    ];
    for (src, want) in cases {
        let ast = parse_clean(src);
        let TopLevelDecl::Function(fn_decl) = &ast.items[0] else {
            panic!("expected a function top-level decl");
        };
        assert_eq!(fn_decl.visibility, want, "for src `{src}`");
    }
}

// ---------------------------------------------------------------------------
// Statements and expressions
// ---------------------------------------------------------------------------

/// Bare call statement: `public void main() { foo(); }`.
#[test]
fn call_with_no_args() {
    let ast = parse_clean("public void main() { foo(); }");
    let body = body_of(&ast.items[0]);
    assert_eq!(body.statements.len(), 1);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else {
        panic!("expected call expression statement");
    };
    let Expr::Path(qn) = &*call.callee else {
        panic!("expected path callee");
    };
    assert_eq!(qn.segments.len(), 1);
    assert_eq!(qn.segments[0].text, "foo");
    assert!(call.args.is_empty());
}

/// Call with one string-literal argument — the hello-world inner.
#[test]
fn call_with_one_string_arg() {
    let ast = parse_clean(r#"public void main() { print("hi"); }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    assert_eq!(call.args.len(), 1);
    let Expr::Literal(Literal::String(s)) = &call.args[0] else { panic!() };
    assert_eq!(s, "hi");
}

/// Call with multiple positional args (no commas allowed at end yet).
#[test]
fn call_with_multiple_args() {
    let ast = parse_clean(r#"public void main() { f(1, 2, "x"); }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    assert_eq!(call.args.len(), 3);
    assert!(matches!(
        &call.args[0],
        Expr::Literal(Literal::Int(juxc_ast::IntLit { value: 1, kind: None, .. }))
    ));
    assert!(matches!(
        &call.args[1],
        Expr::Literal(Literal::Int(juxc_ast::IntLit { value: 2, kind: None, .. }))
    ));
    let Expr::Literal(Literal::String(s)) = &call.args[2] else { panic!() };
    assert_eq!(s, "x");
}

/// Explicit call-site type argument: `identity<int>(5)` parses as a
/// `Call` carrying one `explicit_generic_args` entry — NOT as the
/// less-than/greater-than comparison chain `(identity < int) > (5)`.
#[test]
fn call_with_explicit_type_arg() {
    let ast = parse_clean("public void main() { identity<int>(5); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else {
        panic!("expected call expression statement");
    };
    let Expr::Path(qn) = &*call.callee else { panic!("expected path callee") };
    assert_eq!(qn.segments[0].text, "identity");
    assert_eq!(call.explicit_generic_args.len(), 1);
    assert_eq!(call.explicit_generic_args[0].name.segments[0].text, "int");
    assert_eq!(call.args.len(), 1);
}

/// Multiple explicit type args: `pair<int, String>(1, "a")` — both
/// type args are captured, in order.
#[test]
fn call_with_multiple_explicit_type_args() {
    let ast = parse_clean(r#"public void main() { pair<int, String>(1, "a"); }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    assert_eq!(call.explicit_generic_args.len(), 2);
    assert_eq!(call.explicit_generic_args[0].name.segments[0].text, "int");
    assert_eq!(call.explicit_generic_args[1].name.segments[0].text, "String");
    assert_eq!(call.args.len(), 2);
}

/// Explicit type arg on an instance method: `obj.pick<String>(x)`.
#[test]
fn method_call_with_explicit_type_arg() {
    let ast = parse_clean("public void main() { obj.pick<String>(x); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    assert!(matches!(&*call.callee, Expr::Field(_)));
    assert_eq!(call.explicit_generic_args.len(), 1);
    assert_eq!(call.explicit_generic_args[0].name.segments[0].text, "String");
}

/// A plain `<` comparison must NOT be swallowed by the turbofish
/// lookahead: `a < b` carries no call `(` after the `>`, so it stays a
/// binary comparison and the call's `explicit_generic_args` is empty.
#[test]
fn less_than_is_not_explicit_type_args() {
    let ast = parse_clean("public void main() { f(a < b); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    assert!(call.explicit_generic_args.is_empty());
    assert_eq!(call.args.len(), 1);
    assert!(matches!(&call.args[0], Expr::Binary(_)));
}

/// Const-generic parameter declaration: `class Buf<int N>` (grammar
/// §A.2.6 `'int' identifier`) — the param carries `const_ty: Some(int)`.
#[test]
fn const_generic_param_on_class_parses() {
    let ast = parse_clean("public class Buf<T, int N> { }");
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!("expected class") };
    assert_eq!(c.generic_params.len(), 2);
    assert!(!c.generic_params[0].is_const(), "T is an ordinary type param");
    assert!(c.generic_params[1].is_const(), "N is a const param");
    assert_eq!(c.generic_params[1].name.text, "N");
    assert_eq!(
        c.generic_params[1].const_ty.as_ref().unwrap().name.segments[0].text,
        "int",
    );
}

/// Const-generic parameter on a trailing-generic function:
/// `int cap<int N>()`.
#[test]
fn const_generic_param_on_fn_parses() {
    let ast = parse_clean("public int cap<int N>() { return N; }");
    let TopLevelDecl::Function(f) = &ast.items[0] else { panic!("expected fn") };
    assert_eq!(f.generic_params.len(), 1);
    assert!(f.generic_params[0].is_const());
    assert_eq!(f.generic_params[0].name.text, "N");
}

/// Const-generic argument literal: `new Buf<float, 256>()` — the `256`
/// travels as a synthetic TypeRef recognized by `const_literal_text`.
#[test]
fn const_generic_arg_in_new_parses() {
    let ast = parse_clean("public void main() { var b = new Buf<float, 256>(); }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::NewObject(n)) = &v.init else { panic!("expected new-expr") };
    assert_eq!(n.generic_args.len(), 2);
    assert!(n.generic_args[0].const_literal_text().is_none(), "float is a type");
    assert_eq!(n.generic_args[1].const_literal_text(), Some("256"));
}

/// Const-generic argument in TYPE position: `Buf<4> b = …`.
#[test]
fn const_generic_arg_in_type_position_parses() {
    let ast = parse_clean("public void main() { Buf<4> b = new Buf<4>(); }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("declared type");
    assert_eq!(ty.generic_args.len(), 1);
    let juxc_ast::GenericArg::Type(t) = &ty.generic_args[0] else { panic!() };
    assert_eq!(t.const_literal_text(), Some("4"));
}

/// A const param of an unsupported value type (`<long N>`) parses but
/// fires the Phase-1 E0445 diagnostic.
#[test]
fn const_generic_param_long_is_diagnosed() {
    let (ast, n_errors) = parse_with_errors("public class L<long N> { }");
    assert!(n_errors >= 1, "expected E0445 for <long N>");
    // The AST is still well-formed — the param is present.
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!() };
    assert_eq!(c.generic_params.len(), 1);
    assert!(c.generic_params[0].is_const());
}

/// Non-null assertion `!!` is a chaining postfix (§A.4 level 19):
/// `a.peer!!.id` parses as Field(NotNullAssert(Field(a, peer)), id).
#[test]
fn not_null_assert_chains_postfix() {
    let ast = parse_clean("public void main() { var x = a.peer!!.id; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::Field(outer)) = &v.init else { panic!("expected outer field read") };
    assert_eq!(outer.field.text, "id");
    let Expr::NotNullAssert(inner, _) = &*outer.object else {
        panic!("expected !! under the .id access");
    };
    let Expr::Field(peer) = &**inner else { panic!("expected a.peer under !!") };
    assert_eq!(peer.field.text, "peer");
}

/// `bool` and `null` literals lex as their own token kinds; the parser
/// must propagate them through `parse_primary`.
#[test]
fn bool_and_null_literals() {
    let ast = parse_clean("public void m() { f(true); g(false); h(null); }");
    let body = body_of(&ast.items[0]);
    assert_eq!(body.statements.len(), 3);
    let take_lit = |i: usize| -> Literal {
        let Stmt::Expr(Expr::Call(c)) = &body.statements[i] else { panic!() };
        let Expr::Literal(lit) = &c.args[0] else { panic!() };
        lit.clone()
    };
    assert!(matches!(take_lit(0), Literal::Bool(true)));
    assert!(matches!(take_lit(1), Literal::Bool(false)));
    assert!(matches!(take_lit(2), Literal::Null));
}

/// Dotted callee: `std.io.print("hi")` parses as a chain of `FieldExpr`
/// nodes rooted in a single-segment `Path` — the parser now uses postfix
/// `.field` accumulation rather than greedy multi-segment path
/// consumption for expressions. Type/import positions still use the
/// flat `QualifiedName` shape.
#[test]
fn dotted_path_call() {
    let ast = parse_clean(r#"public void m() { std.io.print("hi"); }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Field(outer) = &*call.callee else {
        panic!("expected top-level Field, got {:?}", call.callee);
    };
    assert_eq!(outer.field.text, "print");
    let Expr::Field(inner) = &*outer.object else {
        panic!("expected nested Field for `std.io`, got {:?}", outer.object);
    };
    assert_eq!(inner.field.text, "io");
    let Expr::Path(qn) = &*inner.object else { panic!() };
    assert_eq!(qn.segments.len(), 1);
    assert_eq!(qn.segments[0].text, "std");
}

/// `return;` produces `Stmt::Return(None)`; `return EXPR;` carries the
/// expression.
#[test]
fn return_statements() {
    let ast = parse_clean(
        r#"public void m() { return; }
           public void n() { return "x"; }"#,
    );
    let body_m = body_of(&ast.items[0]);
    let body_n = body_of(&ast.items[1]);
    assert!(matches!(body_m.statements[0], Stmt::Return(None)));
    assert!(matches!(
        body_n.statements[0],
        Stmt::Return(Some(Expr::Literal(Literal::String(_))))
    ));
}

// ---------------------------------------------------------------------------
// The hello.jux vehicle
// ---------------------------------------------------------------------------

/// The full hello-world AST shape, end to end. This is milestone 1's
/// canary — once this passes through every phase, we have a real compiler.
#[test]
fn hello_jux_full_ast() {
    let src = "public void main() {\n    print(\"Hello, world!\");\n}\n";
    let ast = parse_clean(src);

    assert_eq!(ast.items.len(), 1);
    let TopLevelDecl::Function(fn_decl) = &ast.items[0] else {
        panic!("expected a function top-level decl");
    };

    assert_eq!(fn_decl.visibility, Visibility::Public);
    assert!(matches!(fn_decl.return_type, ReturnType::Void));
    assert_eq!(fn_decl.name.text, "main");
    assert!(fn_decl.params.is_empty());

    let body = fn_decl.body.as_ref().expect("body");
    assert_eq!(body.statements.len(), 1);

    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else {
        panic!("expected print(...) call statement");
    };

    let Expr::Path(qn) = &*call.callee else {
        panic!("expected path callee");
    };
    assert_eq!(qn.segments.len(), 1);
    assert_eq!(qn.segments[0].text, "print");

    assert_eq!(call.args.len(), 1);
    let Expr::Literal(Literal::String(s)) = &call.args[0] else {
        panic!("expected string literal arg");
    };
    assert_eq!(s, "Hello, world!");
}

// ---------------------------------------------------------------------------
// Error reporting
// ---------------------------------------------------------------------------

/// Missing `;` after an expression statement is E0200 (unexpected token).
#[test]
fn missing_semicolon_after_expression_stmt() {
    let (_, n) = parse_with_errors(r#"public void m() { print("x") }"#);
    assert!(n >= 1, "expected an unexpected-token diagnostic");
}

/// Missing `)` after argument list is E0200.
#[test]
fn missing_closing_paren_in_call() {
    let (_, n) = parse_with_errors(r#"public void m() { print("x" ; }"#);
    assert!(n >= 1);
}

/// Missing `(` after function name is E0200.
#[test]
fn missing_open_paren_in_fn_decl() {
    let (_, n) = parse_with_errors("public void main { }");
    assert!(n >= 1);
}

/// Garbage at top level recovers and the parser still hits EOF.
#[test]
fn recovery_skips_garbage_to_next_top_level() {
    let (ast, n) = parse_with_errors("@@@@ public void main() { }");
    assert!(n >= 1);
    // After recovery the function should still be parsed.
    assert!(!ast.items.is_empty(), "function should survive recovery");
}

// ---------------------------------------------------------------------------
// var declarations (§A.2.8)
// ---------------------------------------------------------------------------

/// `var name = expr ;` parses to a `Stmt::VarDecl` with inferred type.
#[test]
fn var_declaration_with_inferred_type() {
    let ast = parse_clean("public void main() { var x = 10; }");
    let body = body_of(&ast.items[0]);
    assert_eq!(body.statements.len(), 1);
    let Stmt::VarDecl(var) = &body.statements[0] else {
        panic!("expected Stmt::VarDecl, got {:?}", body.statements[0]);
    };
    assert_eq!(var.name.text, "x");
    assert!(var.ty.is_none(), "inferred type should leave ty=None");
    assert!(matches!(
        var.init,
        Some(Expr::Literal(Literal::Int(juxc_ast::IntLit { value: 10, kind: None, .. })))
    ));
}

/// Missing initializer is currently a parse error (we expect `=`).
#[test]
fn var_without_initializer_is_e0200() {
    let (_, n) = parse_with_errors("public void main() { var x; }");
    assert!(n >= 1);
}

// ---------------------------------------------------------------------------
// if / else (§A.2.8)
// ---------------------------------------------------------------------------

/// `if (cond) { … }` with no else parses to `Stmt::If` with `else_branch: None`.
#[test]
fn if_without_else() {
    let ast = parse_clean(
        r#"public void main() {
               if (true) { print("yes"); }
           }"#,
    );
    let body = body_of(&ast.items[0]);
    let Stmt::If(if_stmt) = &body.statements[0] else {
        panic!("expected Stmt::If");
    };
    assert!(if_stmt.else_branch.is_none());
}

/// `if (cond) {} else {}` produces an `ElseBranch::Block`.
#[test]
fn if_with_else_block() {
    use juxc_ast::ElseBranch;
    let ast = parse_clean("public void main() { if (true) {} else {} }");
    let body = body_of(&ast.items[0]);
    let Stmt::If(if_stmt) = &body.statements[0] else { panic!() };
    let branch = if_stmt.else_branch.as_ref().expect("else expected");
    assert!(matches!(**branch, ElseBranch::Block(_)));
}

/// `if (a) {} else if (b) {} else {}` produces an else-if chain via
/// nested `ElseBranch::If`.
#[test]
fn if_else_if_chain() {
    use juxc_ast::ElseBranch;
    let ast = parse_clean(
        "public void main() { if (true) {} else if (false) {} else {} }",
    );
    let body = body_of(&ast.items[0]);
    let Stmt::If(outer) = &body.statements[0] else { panic!() };
    let branch = outer.else_branch.as_ref().expect("else expected");
    let ElseBranch::If(inner) = branch.as_ref() else {
        panic!("expected else-if to be ElseBranch::If");
    };
    assert!(matches!(
        inner.else_branch.as_deref(),
        Some(ElseBranch::Block(_))
    ));
}

// ---------------------------------------------------------------------------
// Binary operators (§A.2.9, §A.4)
// ---------------------------------------------------------------------------

/// `1 + 2` parses as a single `Expr::Binary` with `BinaryOp::Add`.
#[test]
fn binary_plus_parses() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(1 + 2); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(b) = &call.args[0] else {
        panic!("expected binary expr, got {:?}", call.args[0]);
    };
    assert_eq!(b.op, BinaryOp::Add);
}

/// `1 + 2 * 3` respects precedence: `*` binds tighter than `+`, so the
/// outer node is `+` with `2 * 3` on the right.
#[test]
fn multiplicative_binds_tighter_than_additive() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(1 + 2 * 3); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(plus) = &call.args[0] else { panic!() };
    assert_eq!(plus.op, BinaryOp::Add);
    let Expr::Binary(mul) = &*plus.right else {
        panic!("rhs of `+` should be a `*` subexpr");
    };
    assert_eq!(mul.op, BinaryOp::Mul);
}

/// Addition is left-associative: `1 + 2 + 3` parses as `(1 + 2) + 3`.
#[test]
fn additive_is_left_associative() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(1 + 2 + 3); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(outer) = &call.args[0] else { panic!() };
    assert_eq!(outer.op, BinaryOp::Add);
    let Expr::Binary(left) = &*outer.left else {
        panic!("lhs of outer `+` should be `(1 + 2)`");
    };
    assert_eq!(left.op, BinaryOp::Add);
}

/// Comparison binds looser than additive: `1 + 2 > 0` parses as
/// `(1 + 2) > 0`.
#[test]
fn comparison_binds_looser_than_additive() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(1 + 2 > 0); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(outer) = &call.args[0] else { panic!() };
    assert_eq!(outer.op, BinaryOp::Gt);
    let Expr::Binary(plus) = &*outer.left else {
        panic!("lhs of `>` should be `(1 + 2)`");
    };
    assert_eq!(plus.op, BinaryOp::Add);
}

/// Equality binds looser than comparison: `1 < 2 == true` parses as
/// `(1 < 2) == true`.
#[test]
fn equality_binds_looser_than_comparison() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(1 < 2 == true); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(outer) = &call.args[0] else { panic!() };
    assert_eq!(outer.op, BinaryOp::Eq);
    let Expr::Binary(lt) = &*outer.left else {
        panic!("lhs of `==` should be `(1 < 2)`");
    };
    assert_eq!(lt.op, BinaryOp::Lt);
}

// ---------------------------------------------------------------------------
// while loops (§A.2.8)
// ---------------------------------------------------------------------------

/// `while (cond) { … }` parses to `Stmt::While` with the body block.
#[test]
fn while_loop_parses() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { while (1 > 0) { print(\"hi\"); } }");
    let body = body_of(&ast.items[0]);
    let Stmt::While(w) = &body.statements[0] else {
        panic!("expected Stmt::While, got {:?}", body.statements[0]);
    };
    let Expr::Binary(cond) = &w.condition else { panic!("expected binary condition") };
    assert_eq!(cond.op, BinaryOp::Gt);
    assert_eq!(w.body.statements.len(), 1);
}

// ---------------------------------------------------------------------------
// Assignment statements (§A.2.9)
// ---------------------------------------------------------------------------

/// `x = expr;` produces an `AssignStmt` whose target is a single-segment
/// `Path` lvalue.
#[test]
fn simple_assignment_parses() {
    let ast = parse_clean("public void main() { var x = 1; x = 2; }");
    let body = body_of(&ast.items[0]);
    let Stmt::Assign(a) = &body.statements[1] else {
        panic!("expected Stmt::Assign, got {:?}", body.statements[1]);
    };
    let Expr::Path(qn) = &a.target else {
        panic!("expected Path lvalue, got {:?}", a.target);
    };
    assert_eq!(qn.segments[0].text, "x");
    assert!(matches!(
        a.value,
        Expr::Literal(Literal::Int(juxc_ast::IntLit { value: 2, kind: None, .. }))
    ));
}

/// Assigning to a field access (`obj.field = v`) is now accepted — the
/// parser produces an `AssignStmt` whose target is `Expr::Field`. Used
/// inside class methods for `this.field = v` writes.
#[test]
fn field_lvalue_is_accepted() {
    let ast = parse_clean("public void main() { var x = 1; x.y = 2; }");
    let body = body_of(&ast.items[0]);
    let Stmt::Assign(a) = &body.statements[1] else {
        panic!("expected Stmt::Assign, got {:?}", body.statements[1]);
    };
    assert!(matches!(&a.target, Expr::Field(_)));
}

// ---------------------------------------------------------------------------
// Compound assignments — parse-time desugar to `target = target op rhs`
// ---------------------------------------------------------------------------
// Elvis / null-coalescing (`?:` and its `??` alias)
// ---------------------------------------------------------------------------

/// Both `a ?: b` and `a ?? b` parse to the same `Expr::Elvis` shape.
/// Per `JUX-GRAMMAR-ADDENDUM.md` §A.1.6 the two spellings are
/// interchangeable aliases.
#[test]
fn elvis_and_double_question_produce_same_ast() {
    let colon = parse_clean("public void main() { var x = a ?: b; }");
    let qq    = parse_clean("public void main() { var x = a ?? b; }");
    let bc = body_of(&colon.items[0]);
    let bq = body_of(&qq.items[0]);
    let Stmt::VarDecl(vc) = &bc.statements[0] else { panic!() };
    let Stmt::VarDecl(vq) = &bq.statements[0] else { panic!() };
    let ic = vc.init.as_ref().unwrap();
    let iq = vq.init.as_ref().unwrap();
    let Expr::Elvis(_) = ic else {
        panic!("?: should parse to Expr::Elvis, got {ic:?}");
    };
    let Expr::Elvis(_) = iq else {
        panic!("?? should parse to Expr::Elvis, got {iq:?}");
    };
}

/// Right-associativity holds for both spellings. `a ?? b ?? c`
/// parses as `a ?? (b ?? c)` — same as `a ?: b ?: c`.
#[test]
fn elvis_double_question_is_right_associative() {
    let ast = parse_clean("public void main() { var x = a ?? b ?? c; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let init = v.init.as_ref().unwrap();
    let Expr::Elvis(outer) = init else { panic!() };
    let Expr::Elvis(_inner) = &*outer.fallback else {
        panic!("right side should be another Elvis, got {:?}", outer.fallback);
    };
}

/// `?:` and `??` can be mixed freely in a single chain (they're
/// the same operator); the chain still parses right-associatively.
#[test]
fn elvis_spellings_can_be_mixed() {
    let ast = parse_clean("public void main() { var x = a ?: b ?? c; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let init = v.init.as_ref().unwrap();
    let Expr::Elvis(outer) = init else { panic!() };
    let Expr::Elvis(_) = &*outer.fallback else {
        panic!("mixed chain should still nest right: {:?}", outer.fallback);
    };
}

// ---------------------------------------------------------------------------

/// `x += 1;` parses to an AssignStmt with `op = Some(Add)` and the
/// bare rhs in `value`. The compound operator no longer desugars at
/// parse time — that's the backend's job (lowers to Rust `+=`).
#[test]
fn plus_equals_preserves_op_on_assign_stmt() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { var x = 1; x += 2; }");
    let body = body_of(&ast.items[0]);
    let Stmt::Assign(a) = &body.statements[1] else {
        panic!("expected Stmt::Assign, got {:?}", body.statements[1]);
    };
    let Expr::Path(t_qn) = &a.target else { panic!("expected Path lvalue") };
    assert_eq!(t_qn.segments[0].text, "x");
    assert_eq!(a.op, Some(BinaryOp::Add), "compound op should be Add");
    // RHS is the bare literal `2`, NOT a synthetic `x + 2` binary.
    let Expr::Literal(_) = &a.value else {
        panic!("rhs should be the literal value, not a binary: {:?}", a.value);
    };
}

/// All five arithmetic compound operators preserve their op on
/// `AssignStmt`.
#[test]
fn all_compound_arithmetic_ops_round_trip() {
    use juxc_ast::BinaryOp;
    let cases = [
        ("+=", BinaryOp::Add),
        ("-=", BinaryOp::Sub),
        ("*=", BinaryOp::Mul),
        ("/=", BinaryOp::Div),
        ("%=", BinaryOp::Rem),
    ];
    for (op_src, expected) in cases {
        let src = format!("public void main() {{ var x = 1; x {op_src} 2; }}");
        let ast = parse_clean(&src);
        let body = body_of(&ast.items[0]);
        let Stmt::Assign(a) = &body.statements[1] else { panic!() };
        assert_eq!(a.op, Some(expected), "wrong op for {op_src}");
    }
}

/// A plain `x = 1;` carries `op = None`.
#[test]
fn plain_assignment_has_no_compound_op() {
    let ast = parse_clean("public void main() { var x = 1; x = 7; }");
    let body = body_of(&ast.items[0]);
    let Stmt::Assign(a) = &body.statements[1] else { panic!() };
    assert!(a.op.is_none(), "plain `=` should have no compound op, got {:?}", a.op);
}

// ---------------------------------------------------------------------------
// §A `incdec` — expression-position ++/-- (value form, N3)
// ---------------------------------------------------------------------------

/// `print(x++)` parses the argument to a postfix `Expr::IncDec`
/// (`is_inc: true`, `is_prefix: false`) wrapping the name `x`.
#[test]
fn postfix_incr_in_call_arg_parses_to_incdec() {
    let ast = parse_clean("public void main() { var x = 1; print(x++); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(c)) = &body.statements[1] else {
        panic!("expected a call statement, got {:?}", body.statements[1]);
    };
    let Expr::IncDec(i) = &c.args[0] else {
        panic!("expected Expr::IncDec arg, got {:?}", c.args[0]);
    };
    assert!(i.is_inc, "++ should set is_inc");
    assert!(!i.is_prefix, "postfix x++ should set is_prefix=false");
    let Expr::Path(qn) = &*i.target else { panic!("target should be a Path") };
    assert_eq!(qn.segments[0].text, "x");
}

/// `var y = ++x;` parses the RHS to a prefix `Expr::IncDec`
/// (`is_inc: true`, `is_prefix: true`).
#[test]
fn prefix_incr_in_initializer_parses_to_incdec() {
    let ast = parse_clean("public void main() { var x = 1; var y = ++x; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[1] else {
        panic!("expected a var decl, got {:?}", body.statements[1]);
    };
    let Some(Expr::IncDec(i)) = &v.init else {
        panic!("expected Expr::IncDec init, got {:?}", v.init);
    };
    assert!(i.is_inc && i.is_prefix, "++x should be inc + prefix");
}

/// `arr[i++]` parses the INDEX to a postfix increment, leaving the
/// outer expression an `Expr::Index`.
#[test]
fn postfix_incr_as_array_index_parses_to_incdec() {
    let ast =
        parse_clean("public void main() { var arr = new int[]{0}; var i = 0; print(arr[i++]); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(c)) = &body.statements[2] else { panic!() };
    let Expr::Index(ix) = &c.args[0] else {
        panic!("expected Expr::Index arg, got {:?}", c.args[0]);
    };
    let Expr::IncDec(i) = &*ix.index else {
        panic!("index should be Expr::IncDec, got {:?}", ix.index);
    };
    assert!(i.is_inc && !i.is_prefix, "i++ in index should be inc + postfix");
}

/// All four value forms (`++x`, `x++`, `--x`, `x--`) parse to
/// `Expr::IncDec` with the right `is_inc` / `is_prefix` flags when used
/// in expression position (here, a call argument).
#[test]
fn all_four_incdec_value_forms_parse() {
    let cases = [
        ("++x", true, true),
        ("x++", true, false),
        ("--x", false, true),
        ("x--", false, false),
    ];
    for (form, is_inc, is_prefix) in cases {
        let src = format!("public void main() {{ var x = 1; print({form}); }}");
        let ast = parse_clean(&src);
        let body = body_of(&ast.items[0]);
        let Stmt::Expr(Expr::Call(c)) = &body.statements[1] else {
            panic!("{form}: expected call");
        };
        let Expr::IncDec(i) = &c.args[0] else {
            panic!("{form}: expected Expr::IncDec, got {:?}", c.args[0]);
        };
        assert_eq!(i.is_inc, is_inc, "{form}: wrong is_inc");
        assert_eq!(i.is_prefix, is_prefix, "{form}: wrong is_prefix");
    }
}

/// REGRESSION: a bare statement `x++;` STILL desugars to the
/// value-less `x += 1` assignment (the statement path is untouched) —
/// it must NOT become an `Expr::IncDec`.
#[test]
fn statement_postfix_incr_stays_compound_assign() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { var x = 1; x++; }");
    let body = body_of(&ast.items[0]);
    let Stmt::Assign(a) = &body.statements[1] else {
        panic!("statement x++ should be an AssignStmt, got {:?}", body.statements[1]);
    };
    assert_eq!(a.op, Some(BinaryOp::Add), "x++ statement should be `x += 1`");
}

/// REGRESSION: a C-style for-update `i++` STILL desugars to `i += 1`
/// (statement path) — no `Expr::IncDec` leaks into the for header.
#[test]
fn for_update_incr_stays_compound_assign() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean(
        "public void main() { for (int i = 0; i < 3; i++) { print(i); } }",
    );
    let body = body_of(&ast.items[0]);
    let Stmt::ForC(f) = &body.statements[0] else {
        panic!("expected a C-style for, got {:?}", body.statements[0]);
    };
    let Some(Stmt::Assign(a)) = f.update.as_deref() else {
        panic!("for-update should be an AssignStmt, got {:?}", f.update);
    };
    assert_eq!(a.op, Some(BinaryOp::Add), "for-update i++ should be `i += 1`");
}

// ---------------------------------------------------------------------------
// break / continue
// ---------------------------------------------------------------------------

/// `break;` and `continue;` parse to their dedicated statement kinds.
#[test]
fn break_and_continue_parse() {
    let ast = parse_clean(
        r#"public void main() {
               while (true) { break; }
               while (true) { continue; }
           }"#,
    );
    let body = body_of(&ast.items[0]);
    let Stmt::While(w1) = &body.statements[0] else { panic!() };
    assert!(matches!(w1.body.statements[0], Stmt::Break(..)));
    let Stmt::While(w2) = &body.statements[1] else { panic!() };
    assert!(matches!(w2.body.statements[0], Stmt::Continue(..)));
}

// ---------------------------------------------------------------------------
// Unary operators (§A.4 level 18)
// ---------------------------------------------------------------------------

/// `-x` parses as `Unary(Neg, Path(x))`.
#[test]
fn unary_negation_on_ident_parses() {
    use juxc_ast::UnaryOp;
    let ast = parse_clean("public void main() { print(-x); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Unary(u) = &call.args[0] else {
        panic!("expected Unary, got {:?}", call.args[0]);
    };
    assert_eq!(u.op, UnaryOp::Neg);
    assert!(matches!(&*u.operand, Expr::Path(_)));
}

/// `!flag` parses with `UnaryOp::Not`.
#[test]
fn unary_logical_not_parses() {
    use juxc_ast::UnaryOp;
    let ast = parse_clean("public void main() { print(!true); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Unary(u) = &call.args[0] else { panic!() };
    assert_eq!(u.op, UnaryOp::Not);
}

/// `~mask` parses with `UnaryOp::BitNot`.
#[test]
fn unary_bitwise_not_parses() {
    use juxc_ast::UnaryOp;
    let ast = parse_clean("public void main() { print(~0); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Unary(u) = &call.args[0] else { panic!() };
    assert_eq!(u.op, UnaryOp::BitNot);
}

/// `--x` parses right-associatively as `Unary(Neg, Unary(Neg, x))`.
#[test]
fn double_negation_parses_right_associative() {
    use juxc_ast::UnaryOp;
    // `--x` now lexes as the DECREMENT operator (greedy, like Java/C),
    // so a genuine double-negation is written with a space: `- -x`
    // parses as `-(-x)`.
    let ast = parse_clean("public void main() { print(- -x); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Unary(outer) = &call.args[0] else { panic!() };
    assert_eq!(outer.op, UnaryOp::Neg);
    let Expr::Unary(inner) = &*outer.operand else {
        panic!("expected nested unary, got {:?}", outer.operand);
    };
    assert_eq!(inner.op, UnaryOp::Neg);
}

/// Unary binds tighter than additive: `-x + y` parses as `(-x) + y`.
#[test]
fn unary_binds_tighter_than_additive() {
    use juxc_ast::{BinaryOp, UnaryOp};
    let ast = parse_clean("public void main() { print(-x + y); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(plus) = &call.args[0] else {
        panic!("expected binary at the top, got {:?}", call.args[0]);
    };
    assert_eq!(plus.op, BinaryOp::Add);
    // The left side of the `+` should be a unary `-x`.
    let Expr::Unary(u) = &*plus.left else {
        panic!("lhs of `+` should be a Unary, got {:?}", plus.left);
    };
    assert_eq!(u.op, UnaryOp::Neg);
}

/// Unary in argument position works: `abs(-7)`.
#[test]
fn unary_in_call_argument() {
    use juxc_ast::UnaryOp;
    let ast = parse_clean("public void main() { abs(-7); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Unary(u) = &call.args[0] else { panic!() };
    assert_eq!(u.op, UnaryOp::Neg);
    assert!(matches!(
        *u.operand,
        Expr::Literal(Literal::Int(juxc_ast::IntLit { value: 7, kind: None, .. }))
    ));
}

// ---------------------------------------------------------------------------
// Range expressions + for-each (§A.2.8, §A.2.9 level 13)
// ---------------------------------------------------------------------------

/// `0..10` parses to a half-open Range.
#[test]
fn exclusive_range_parses() {
    let ast = parse_clean("public void main() { print(0..10); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Range(r) = &call.args[0] else {
        panic!("expected Range, got {:?}", call.args[0]);
    };
    assert!(!r.inclusive);
    assert!(matches!(
        *r.start,
        Expr::Literal(Literal::Int(juxc_ast::IntLit { value: 0, kind: None, .. }))
    ));
    assert!(matches!(
        *r.end,
        Expr::Literal(Literal::Int(juxc_ast::IntLit { value: 10, kind: None, .. }))
    ));
}

/// `0..=10` parses to an inclusive Range.
#[test]
fn inclusive_range_parses() {
    let ast = parse_clean("public void main() { print(0..=10); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Range(r) = &call.args[0] else { panic!() };
    assert!(r.inclusive);
}

/// Range operands are additive expressions: `1 + 2 .. 3 * 4` parses as
/// `(1 + 2)..(3 * 4)`.
#[test]
fn range_operands_can_be_additive() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(1 + 2 .. 3 * 4); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Range(r) = &call.args[0] else { panic!() };
    let Expr::Binary(left_bin) = &*r.start else { panic!() };
    assert_eq!(left_bin.op, BinaryOp::Add);
    let Expr::Binary(right_bin) = &*r.end else { panic!() };
    assert_eq!(right_bin.op, BinaryOp::Mul);
}

/// `for (var i : 0..10) { … }` parses to a `Stmt::ForEach` with
/// `var_type == None` (inferred).
#[test]
fn for_each_var_form_parses() {
    let ast = parse_clean("public void main() { for (var i : 0..10) { print(i); } }");
    let body = body_of(&ast.items[0]);
    let Stmt::ForEach(f) = &body.statements[0] else {
        panic!("expected ForEach, got {:?}", body.statements[0]);
    };
    assert!(f.var_type.is_none(), "var-form should have no var_type");
    assert_eq!(f.var_name.text, "i");
    assert!(matches!(f.iter, Expr::Range(_)));
    assert_eq!(f.body.statements.len(), 1);
}

/// `for (int i : 0..10) { … }` parses with an explicit `var_type`.
#[test]
fn for_each_typed_form_parses() {
    let ast = parse_clean("public void main() { for (int i : 0..10) { print(i); } }");
    let body = body_of(&ast.items[0]);
    let Stmt::ForEach(f) = &body.statements[0] else { panic!() };
    let ty = f.var_type.as_ref().expect("typed form should have a var_type");
    assert_eq!(ty.name.segments[0].text, "int");
}

// ---------------------------------------------------------------------------
// Suffixed integer literals + typed locals (§A.1.4, §A.2.8)
// ---------------------------------------------------------------------------

/// `5L` parses with `IntKind::Long`; `5u` with `UInt`; `5uL` with `ULong`.
#[test]
fn suffixed_int_literals_classify_correctly() {
    use juxc_ast::IntKind;
    let cases = [
        ("42",   None,                  42),
        ("42L",  Some(IntKind::Long),   42),
        ("42u",  Some(IntKind::UInt),   42),
        ("42uL", Some(IntKind::ULong),  42),
        ("42b",  Some(IntKind::Byte),   42),
        ("42ub", Some(IntKind::UByte),  42),
        ("42s",  Some(IntKind::Short),  42),
        ("42us", Some(IntKind::UShort), 42),
    ];
    for (lit, want_kind, want_value) in cases {
        let src = format!("public void main() {{ print({lit}); }}");
        let ast = parse_clean(&src);
        let body = body_of(&ast.items[0]);
        let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
        let Expr::Literal(Literal::Int(int_lit)) = &call.args[0] else {
            panic!("expected Int literal, got {:?}", call.args[0]);
        };
        assert_eq!(int_lit.value, want_value, "for {lit}");
        assert_eq!(int_lit.kind, want_kind, "for {lit}");
    }
}

/// `3.14` parses as a Float literal with no suffix.
#[test]
fn unsuffixed_float_is_double_kind() {
    // `2.5` instead of `3.14` to dodge clippy's
    // `approx_constant` lint (PI-shaped literals).
    let ast = parse_clean("public void main() { print(2.5); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Literal(Literal::Float(f)) = &call.args[0] else {
        panic!("expected Float literal, got {:?}", call.args[0]);
    };
    assert!((f.value - 2.5).abs() < 1e-9);
    assert!(f.kind.is_none(), "expected default (double), got {:?}", f.kind);
}

/// `1.5f` parses as a Float literal with `FloatKind::Float`.
#[test]
fn f_suffixed_float_is_float_kind() {
    use juxc_ast::FloatKind;
    let ast = parse_clean("public void main() { print(1.5f); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Literal(Literal::Float(f)) = &call.args[0] else { panic!() };
    assert!((f.value - 1.5).abs() < 1e-9);
    assert_eq!(f.kind, Some(FloatKind::Float));
}

/// `int x = 5;` parses as a typed local declaration.
#[test]
fn typed_local_decl_parses() {
    let ast = parse_clean("public void main() { int x = 5; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(var) = &body.statements[0] else {
        panic!("expected Stmt::VarDecl, got {:?}", body.statements[0]);
    };
    assert_eq!(var.name.text, "x");
    let ty = var.ty.as_ref().expect("typed local should have a ty");
    assert_eq!(ty.name.segments[0].text, "int");
    assert!(var.init.is_some());
}

/// `bool flag;` — uninitialized typed declaration also works.
#[test]
fn typed_local_decl_without_init_parses() {
    let ast = parse_clean("public void main() { bool flag; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(var) = &body.statements[0] else { panic!() };
    assert_eq!(var.name.text, "flag");
    assert!(var.ty.is_some());
    assert!(var.init.is_none());
}

/// Expression statements like `print(x);` must not get mis-parsed as
/// typed locals — that disambiguator was the whole reason for the
/// peek-3 lookahead.
#[test]
fn expression_statement_is_not_a_typed_local() {
    let ast = parse_clean("public void main() { print(0); }");
    let body = body_of(&ast.items[0]);
    assert!(
        matches!(body.statements[0], Stmt::Expr(_)),
        "expected Stmt::Expr, got {:?}",
        body.statements[0],
    );
}

// ---------------------------------------------------------------------------
// Logical operators (§A.4 levels 4–5)
// ---------------------------------------------------------------------------

/// `a && b` parses with `BinaryOp::And`.
#[test]
fn logical_and_parses() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(a && b); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(b) = &call.args[0] else { panic!() };
    assert_eq!(b.op, BinaryOp::And);
}

/// `a || b` parses with `BinaryOp::Or`.
#[test]
fn logical_or_parses() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(a || b); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(b) = &call.args[0] else { panic!() };
    assert_eq!(b.op, BinaryOp::Or);
}

/// `&&` binds tighter than `||`: `a || b && c` parses as `a || (b && c)`.
#[test]
fn and_binds_tighter_than_or() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(a || b && c); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(or) = &call.args[0] else { panic!() };
    assert_eq!(or.op, BinaryOp::Or);
    let Expr::Binary(and) = &*or.right else {
        panic!("rhs of `||` should be a `&&` subexpr");
    };
    assert_eq!(and.op, BinaryOp::And);
}

/// `==` binds tighter than `&&`: `a && b == c` parses as `a && (b == c)`.
#[test]
fn equality_binds_tighter_than_and() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(a && b == c); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(and) = &call.args[0] else { panic!() };
    assert_eq!(and.op, BinaryOp::And);
    let Expr::Binary(eq) = &*and.right else { panic!() };
    assert_eq!(eq.op, BinaryOp::Eq);
}

/// `||` is left-associative: `a || b || c` is `(a || b) || c`.
#[test]
fn logical_or_is_left_associative() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(a || b || c); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(outer) = &call.args[0] else { panic!() };
    assert_eq!(outer.op, BinaryOp::Or);
    let Expr::Binary(left) = &*outer.left else {
        panic!("lhs of outer `||` should be `(a || b)`");
    };
    assert_eq!(left.op, BinaryOp::Or);
}

// ---------------------------------------------------------------------------
// Bitwise operators (§A.4 levels 6–8 + 14)
// ---------------------------------------------------------------------------

/// `a | b`, `a ^ b`, `a & b` each parse with the right BinaryOp.
#[test]
fn bitwise_ops_parse() {
    use juxc_ast::BinaryOp;
    let cases = [
        ("|", BinaryOp::BitOr),
        ("^", BinaryOp::BitXor),
        ("&", BinaryOp::BitAnd),
    ];
    for (op_src, expected) in cases {
        let src = format!("public void main() {{ print(a {op_src} b); }}");
        let ast = parse_clean(&src);
        let body = body_of(&ast.items[0]);
        let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
        let Expr::Binary(b) = &call.args[0] else { panic!() };
        assert_eq!(b.op, expected, "wrong op for {op_src}");
    }
}

/// Shifts parse with `BinaryOp::Shl` / `BinaryOp::Shr`.
#[test]
fn shifts_parse() {
    use juxc_ast::BinaryOp;
    let cases = [("<<", BinaryOp::Shl), (">>", BinaryOp::Shr)];
    for (op_src, expected) in cases {
        let src = format!("public void main() {{ print(a {op_src} 2); }}");
        let ast = parse_clean(&src);
        let body = body_of(&ast.items[0]);
        let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
        let Expr::Binary(b) = &call.args[0] else { panic!() };
        assert_eq!(b.op, expected, "wrong op for {op_src}");
    }
}

/// Per §A.4, bitwise `&` is LOOSER than equality in Jux (Java-style).
/// `a & b == c` parses as `a & (b == c)`.
#[test]
fn bit_and_is_looser_than_equality() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(a & b == c); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(and) = &call.args[0] else { panic!() };
    assert_eq!(and.op, BinaryOp::BitAnd);
    let Expr::Binary(eq) = &*and.right else {
        panic!("rhs of `&` should be a `==` subexpr — & is looser than ==");
    };
    assert_eq!(eq.op, BinaryOp::Eq);
}

/// `&` is tighter than `^`, which is tighter than `|`. So
/// `a | b ^ c & d` parses as `a | (b ^ (c & d))`.
#[test]
fn bitwise_precedence_chain() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(a | b ^ c & d); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(or) = &call.args[0] else { panic!() };
    assert_eq!(or.op, BinaryOp::BitOr);
    let Expr::Binary(xor) = &*or.right else { panic!() };
    assert_eq!(xor.op, BinaryOp::BitXor);
    let Expr::Binary(and) = &*xor.right else { panic!() };
    assert_eq!(and.op, BinaryOp::BitAnd);
}

/// Shifts are looser than additive: `a + b << c` parses as `(a + b) << c`.
#[test]
fn shift_is_looser_than_additive() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(a + b << c); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(shl) = &call.args[0] else { panic!() };
    assert_eq!(shl.op, BinaryOp::Shl);
    let Expr::Binary(add) = &*shl.left else { panic!() };
    assert_eq!(add.op, BinaryOp::Add);
}

// ---------------------------------------------------------------------------
// `as` cast (§A.4 level 17, §A.5)
// ---------------------------------------------------------------------------

/// `x as int` parses to `Expr::Cast { value: Path(x), ty: int }`.
#[test]
fn simple_cast_parses() {
    let ast = parse_clean("public void main() { print(x as int); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Cast(c) = &call.args[0] else {
        panic!("expected Expr::Cast, got {:?}", call.args[0]);
    };
    assert!(matches!(&*c.value, Expr::Path(_)));
    assert_eq!(c.ty.name.segments[0].text, "int");
}

/// `as` is left-associative: `x as int as long` is `(x as int) as long`.
#[test]
fn cast_is_left_associative() {
    let ast = parse_clean("public void main() { print(x as int as long); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Cast(outer) = &call.args[0] else { panic!() };
    assert_eq!(outer.ty.name.segments[0].text, "long");
    let Expr::Cast(inner) = &*outer.value else {
        panic!("inner should be `x as int`");
    };
    assert_eq!(inner.ty.name.segments[0].text, "int");
}

/// `as` is looser than unary: `-x as int` parses as `(-x) as int`.
#[test]
fn unary_is_tighter_than_cast() {
    use juxc_ast::UnaryOp;
    let ast = parse_clean("public void main() { print(-x as int); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Cast(c) = &call.args[0] else { panic!() };
    let Expr::Unary(u) = &*c.value else {
        panic!("cast operand should be `-x`, got {:?}", c.value);
    };
    assert_eq!(u.op, UnaryOp::Neg);
}

/// `as` is tighter than multiplicative: `x * y as long` parses as
/// `x * (y as long)` per §A.4 levels 16 (mul) and 17 (as).
#[test]
fn cast_is_tighter_than_multiplicative() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { print(x * y as long); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Binary(mul) = &call.args[0] else {
        panic!("expected outer binary, got {:?}", call.args[0]);
    };
    assert_eq!(mul.op, BinaryOp::Mul);
    let Expr::Cast(c) = &*mul.right else {
        panic!("rhs of `*` should be a cast");
    };
    assert_eq!(c.ty.name.segments[0].text, "long");
}

/// C-style cast `(long) x` parses to the same `Expr::Cast` shape as
/// `x as long`. Triggers because `long` is a known primitive name.
#[test]
fn c_style_cast_with_primitive_parses_as_cast() {
    let ast = parse_clean("public void main() { print((long) x); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Cast(c) = &call.args[0] else {
        panic!("expected Expr::Cast, got {:?}", call.args[0]);
    };
    assert_eq!(c.ty.name.segments[0].text, "long");
    assert!(matches!(&*c.value, Expr::Path(_)), "cast target should be ident");
}

/// `(int) -x` — cast binds at unary precedence, so the operand is
/// the **unary expression** `-x`, not the value `x` alone.
#[test]
fn c_style_cast_takes_unary_operand() {
    use juxc_ast::UnaryOp;
    let ast = parse_clean("public void main() { print((int) -x); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Cast(c) = &call.args[0] else { panic!() };
    let Expr::Unary(u) = &*c.value else {
        panic!("operand should be `-x`, got {:?}", c.value);
    };
    assert_eq!(u.op, UnaryOp::Neg);
}

/// `(x) y` — grouping a plain non-primitive ident does **not**
/// become a cast. The grammar addendum §A.5 reserves user-name
/// casts for name-resolution; until then the parens are pure
/// grouping. (Here `(x) + 3` is the canonical shape — the
/// expression after `)` doesn't have to be valid for a cast to
/// have triggered, but our lookahead conservatively rejects
/// non-primitive bare names so this stays a plain `(x) + 3`.)
#[test]
fn paren_grouped_non_primitive_ident_stays_paren_expr() {
    let ast = parse_clean("public void main() { print((x) + 3); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    // The arg is the binary `(x) + 3` — NOT a cast.
    assert!(
        !matches!(&call.args[0], Expr::Cast(_)),
        "should not parse as cast: {:?}",
        call.args[0],
    );
}

/// A user-named type with an array marker (`(Foo[]) x`) DOES
/// trigger the cast path because the markers make the type-shape
/// unambiguous.
#[test]
fn c_style_cast_with_array_marker_on_user_type() {
    let ast = parse_clean("public void main() { print((Foo[]) xs); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Cast(c) = &call.args[0] else {
        panic!("array-marked user type should trigger cast: {:?}", call.args[0]);
    };
    assert_eq!(c.ty.name.segments[0].text, "Foo");
    assert!(c.ty.array_shape.is_some(), "cast type should carry array shape");
}

/// `(x as int)` and `(int) x` should produce structurally
/// identical AST: both an `Expr::Cast` with the same target type.
#[test]
fn postfix_and_prefix_cast_produce_same_ast_shape() {
    let postfix = parse_clean("public void main() { print(x as int); }");
    let prefix  = parse_clean("public void main() { print((int) x); }");
    let pa = body_of(&postfix.items[0]);
    let pp = body_of(&prefix.items[0]);
    let Stmt::Expr(Expr::Call(call_a)) = &pa.statements[0] else { panic!() };
    let Stmt::Expr(Expr::Call(call_p)) = &pp.statements[0] else { panic!() };
    let Expr::Cast(ca) = &call_a.args[0] else { panic!() };
    let Expr::Cast(cp) = &call_p.args[0] else { panic!() };
    assert_eq!(ca.ty.name.segments[0].text, cp.ty.name.segments[0].text);
    // Both cast targets are a single-segment Path `x`.
    assert!(matches!(&*ca.value, Expr::Path(_)));
    assert!(matches!(&*cp.value, Expr::Path(_)));
}

// ---------------------------------------------------------------------------
// sizeof (§5.9)
// ---------------------------------------------------------------------------

/// `sizeof(int)` parses to `Expr::SizeOf` with the operand as a Path
/// (the disambiguation between type/value happens at lowering).
#[test]
fn sizeof_of_primitive_parses() {
    let ast = parse_clean("public void main() { print(sizeof(int)); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::SizeOf(s) = &call.args[0] else {
        panic!("expected SizeOf, got {:?}", call.args[0]);
    };
    let Expr::Path(qn) = &*s.operand else {
        panic!("operand should be a Path for a primitive name");
    };
    assert_eq!(qn.segments[0].text, "int");
}

/// `sizeof(count)` also parses to `SizeOf(Path(count))` — value-form
/// detection happens later at lowering.
#[test]
fn sizeof_of_variable_parses() {
    let ast = parse_clean("public void main() { print(sizeof(count)); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::SizeOf(s) = &call.args[0] else { panic!() };
    let Expr::Path(qn) = &*s.operand else { panic!() };
    assert_eq!(qn.segments[0].text, "count");
}

/// `sizeof(arr[i])` — compound expression body, lowers to value form.
/// We don't test parsing of indexing yet (not implemented), but a
/// compound expression like `sizeof(1 + 2)` should parse as SizeOf with
/// a Binary operand.
#[test]
fn sizeof_of_compound_expression_parses() {
    let ast = parse_clean("public void main() { print(sizeof(1 + 2)); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::SizeOf(s) = &call.args[0] else { panic!() };
    assert!(
        matches!(&*s.operand, Expr::Binary(_)),
        "expected Binary operand, got {:?}",
        s.operand,
    );
}

// ---------------------------------------------------------------------------
// Arrays (Turn 1: fixed-size)
// ---------------------------------------------------------------------------

/// `int[10] xs = new int[10];` — typed local with a fixed-size array
/// type plus a matching `new T[size]` initializer.
#[test]
fn fixed_array_typed_local_and_new_array_parse() {
    use juxc_ast::ArrayDim;
    let ast = parse_clean("public void main() { int[10] xs = new int[10]; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else {
        panic!("expected typed local, got {:?}", body.statements[0]);
    };
    // Type carries the fixed-size shape — a single (1-D) dimension.
    let ty = v.ty.as_ref().expect("typed local has explicit type");
    assert_eq!(ty.name.segments[0].text, "int");
    let shape = ty.array_shape.as_ref().expect("array_shape is set");
    assert_eq!(shape.rank(), 1, "int[10] is one-dimensional");
    let ArrayDim::Fixed(size) = shape.outer() else {
        panic!("expected Fixed outer dim, got {:?}", shape.outer());
    };
    assert!(
        matches!(&**size, Expr::Literal(juxc_ast::Literal::Int(_))),
        "size should parse as an int literal",
    );
    // Initializer is a NewArray with the same element type.
    let init = v.init.as_ref().expect("initializer present");
    let Expr::NewArray(n) = init else { panic!("expected NewArray init") };
    assert_eq!(n.element_type.name.segments[0].text, "int");
    assert!(n.element_type.array_shape.is_none(), "element type itself is not an array");
}

/// `arr[i]` postfix indexing emits an `Expr::Index` node.
#[test]
fn array_index_postfix_parses() {
    let ast = parse_clean("public void main() { var first = xs[0]; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::Index(idx)) = v.init.as_ref() else {
        panic!("expected Index initializer, got {:?}", v.init);
    };
    assert!(matches!(&*idx.array, Expr::Path(_)));
    assert!(matches!(&*idx.index, Expr::Literal(juxc_ast::Literal::Int(_))));
}

/// `arr[i] = v;` — assignment whose lvalue is an `Index` expression.
#[test]
fn array_index_assignment_parses() {
    let ast = parse_clean("public void main() { xs[3] = 42; }");
    let body = body_of(&ast.items[0]);
    let Stmt::Assign(a) = &body.statements[0] else {
        panic!("expected assignment, got {:?}", body.statements[0]);
    };
    let Expr::Index(idx) = &a.target else {
        panic!("expected Index lvalue, got {:?}", a.target);
    };
    let Expr::Path(qn) = &*idx.array else { panic!() };
    assert_eq!(qn.segments[0].text, "xs");
}

/// `arr.length` — Java-style member access parsed as a `Field` expr.
#[test]
fn array_length_field_access_parses() {
    let ast = parse_clean("public void main() { var n = xs.length; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::Field(f)) = v.init.as_ref() else {
        panic!("expected Field initializer, got {:?}", v.init);
    };
    let Expr::Path(qn) = &*f.object else { panic!() };
    assert_eq!(qn.segments[0].text, "xs");
    assert_eq!(f.field.text, "length");
}

// ---------------------------------------------------------------------------
// Arrays (Turn 2: dynamic T[] + initializer-list literal)
// ---------------------------------------------------------------------------

/// `T[]` (no size) parses as a `TypeRef` carrying `ArrayShape::Dynamic`.
#[test]
fn dynamic_array_type_parses() {
    use juxc_ast::ArrayDim;
    let ast = parse_clean("public void main() { int[] xs = new int[]{1, 2}; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has explicit type");
    let shape = ty.array_shape.as_ref().expect("array_shape is set");
    assert_eq!(shape.rank(), 1, "int[] is one-dimensional");
    assert!(matches!(shape.outer(), ArrayDim::Dynamic), "expected Dynamic outer dim");
    assert_eq!(ty.name.segments[0].text, "int");
}

/// `new T[]{a, b, c}` parses to `NewArrayLit` with the right elements
/// and element type.
#[test]
fn new_array_lit_parses() {
    let ast = parse_clean(r#"public void main() { var xs = new int[]{1, 2, 3}; }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::NewArrayLit(n)) = v.init.as_ref() else {
        panic!("expected NewArrayLit init, got {:?}", v.init);
    };
    assert_eq!(n.element_type.name.segments[0].text, "int");
    assert_eq!(n.elements.len(), 3);
}

// ---------------------------------------------------------------------------
// Multi-dimensional array types (`int[][]`, `int[3][4]`, `int[3][]`)
// ---------------------------------------------------------------------------

/// `int[][] m` parses to a TWO-dimension dynamic shape (`[Dynamic, Dynamic]`),
/// outermost-first.
#[test]
fn two_dim_dynamic_array_type_parses() {
    use juxc_ast::ArrayDim;
    let ast = parse_clean("public void main() { int[][] m; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has explicit type");
    let shape = ty.array_shape.as_ref().expect("array_shape is set");
    assert_eq!(shape.rank(), 2, "int[][] has two dimensions");
    assert!(matches!(shape.dims[0], ArrayDim::Dynamic));
    assert!(matches!(shape.dims[1], ArrayDim::Dynamic));
}

/// `int[][][] c` parses to a THREE-dimension dynamic shape.
#[test]
fn three_dim_dynamic_array_type_parses() {
    use juxc_ast::ArrayDim;
    let ast = parse_clean("public void main() { int[][][] c; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has explicit type");
    let shape = ty.array_shape.as_ref().expect("array_shape is set");
    assert_eq!(shape.rank(), 3, "int[][][] has three dimensions");
    assert!(shape.dims.iter().all(|d| matches!(d, ArrayDim::Dynamic)));
}

/// `int[3][4] b` parses to a two-dimension FIXED shape, outermost-first:
/// `dims[0]` is the outer `3`, `dims[1]` the inner `4`.
#[test]
fn fixed_two_dim_array_type_parses_outermost_first() {
    use juxc_ast::ArrayDim;
    let ast = parse_clean("public void main() { int[3][4] b; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has explicit type");
    let shape = ty.array_shape.as_ref().expect("array_shape is set");
    assert_eq!(shape.rank(), 2);
    // Helper to read an int-literal size off a Fixed dim.
    let lit = |d: &ArrayDim| -> i64 {
        let ArrayDim::Fixed(e) = d else { panic!("expected Fixed dim") };
        let Expr::Literal(juxc_ast::Literal::Int(i)) = &**e else { panic!("expected int literal") };
        i.value
    };
    assert_eq!(lit(&shape.dims[0]), 3, "outer dim (leftmost) is 3");
    assert_eq!(lit(&shape.dims[1]), 4, "inner dim is 4");
}

/// `int[3][] r` mixes a fixed OUTER dimension with a dynamic inner one.
#[test]
fn mixed_fixed_dynamic_array_type_parses() {
    use juxc_ast::ArrayDim;
    let ast = parse_clean("public void main() { int[3][] r; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has explicit type");
    let shape = ty.array_shape.as_ref().expect("array_shape is set");
    assert_eq!(shape.rank(), 2);
    assert!(matches!(shape.dims[0], ArrayDim::Fixed(_)), "outer dim fixed");
    assert!(matches!(shape.dims[1], ArrayDim::Dynamic), "inner dim dynamic");
}

/// `new int[3][4]` parses to a `NewArray` carrying the outer size plus
/// one inner size, outermost-first.
#[test]
fn new_multi_dim_array_parses_sizes() {
    let ast = parse_clean("public void main() { var g = new int[3][4]; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::NewArray(n)) = v.init.as_ref() else {
        panic!("expected NewArray init, got {:?}", v.init);
    };
    assert_eq!(n.element_type.name.segments[0].text, "int");
    assert!(n.element_type.array_shape.is_none(), "element type is the scalar");
    // Outer size 3, one inner size (4).
    let outer = match &*n.size {
        Expr::Literal(juxc_ast::Literal::Int(i)) => i.value,
        _ => panic!("outer size not an int literal"),
    };
    assert_eq!(outer, 3, "outer size is 3");
    assert_eq!(n.inner_sizes.len(), 1, "one inner dimension");
    let inner = match &*n.inner_sizes[0] {
        Expr::Literal(juxc_ast::Literal::Int(i)) => i.value,
        _ => panic!("inner size not an int literal"),
    };
    assert_eq!(inner, 4, "inner size is 4");
}

/// Empty initializer `new int[]{}` parses to an empty `NewArrayLit`.
#[test]
fn empty_new_array_lit_parses() {
    let ast = parse_clean(r#"public void main() { var xs = new int[]{}; }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::NewArrayLit(n)) = v.init.as_ref() else {
        panic!("expected NewArrayLit, got {:?}", v.init);
    };
    assert!(n.elements.is_empty(), "expected empty initializer list");
    assert!(!n.fixed, "new T[]{{}} is never fixed");
}

// ---------------------------------------------------------------------------
// Arrays (Turn 3: bare `{a, b, c}` initializer in typed-local RHS)
// ---------------------------------------------------------------------------

/// `int[3] xs = {1, 2, 3};` — bare initializer with fixed-size LHS
/// parses to a NewArrayLit carrying `fixed: true`.
#[test]
fn bare_initializer_with_fixed_lhs_sets_fixed_true() {
    let ast = parse_clean("public void main() { int[3] xs = {1, 2, 3}; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::NewArrayLit(n)) = v.init.as_ref() else {
        panic!("expected NewArrayLit init, got {:?}", v.init);
    };
    assert!(n.fixed, "fixed-LHS bare init should set fixed: true");
    assert_eq!(n.elements.len(), 3);
    assert_eq!(n.element_type.name.segments[0].text, "int");
    assert!(n.element_type.array_shape.is_none(), "element type strips the shape");
}

/// `int[] xs = {1, 2, 3};` — bare initializer with dynamic LHS parses
/// to a NewArrayLit carrying `fixed: false` (same shape as `new T[]{…}`).
#[test]
fn bare_initializer_with_dynamic_lhs_sets_fixed_false() {
    let ast = parse_clean("public void main() { int[] xs = {1, 2, 3}; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::NewArrayLit(n)) = v.init.as_ref() else {
        panic!("expected NewArrayLit init, got {:?}", v.init);
    };
    assert!(!n.fixed, "dynamic-LHS bare init should set fixed: false");
    assert_eq!(n.elements.len(), 3);
}

/// Empty bare initializer `int[] xs = {};` on a dynamic LHS produces
/// an empty NewArrayLit (lowers to `Vec::<T>::new()`).
#[test]
fn empty_bare_initializer_on_dynamic_lhs_parses() {
    let ast = parse_clean("public void main() { int[] xs = {}; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::NewArrayLit(n)) = v.init.as_ref() else {
        panic!("expected NewArrayLit, got {:?}", v.init);
    };
    assert!(n.elements.is_empty());
    assert!(!n.fixed);
}

// ---------------------------------------------------------------------------
// Bounded type params (Turn 2)
// ---------------------------------------------------------------------------

/// `<T extends Drawable>` captures one bound on the type param.
#[test]
fn single_bound_on_type_param_parses() {
    let ast = parse_clean("public class Wrapper<T extends Drawable> { }");
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!() };
    assert_eq!(c.generic_params.len(), 1);
    let param = &c.generic_params[0];
    assert_eq!(param.name.text, "T");
    assert_eq!(param.bounds.len(), 1);
    assert_eq!(param.bounds[0].name.segments[0].text, "Drawable");
}

/// `<T extends A & B>` captures both bounds in source order.
#[test]
fn multi_bound_on_type_param_parses() {
    let ast = parse_clean("public class Holder<T extends Animal & Greeter> { }");
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!() };
    let param = &c.generic_params[0];
    assert_eq!(param.bounds.len(), 2);
    assert_eq!(param.bounds[0].name.segments[0].text, "Animal");
    assert_eq!(param.bounds[1].name.segments[0].text, "Greeter");
}

// ---------------------------------------------------------------------------
// Inheritance (Turn 1) — abstract + extends + super(args)
// ---------------------------------------------------------------------------

/// `abstract class Foo { … }` parses with `is_abstract: true`.
#[test]
fn abstract_class_decl_captures_is_abstract_flag() {
    let ast = parse_clean("public abstract class Animal { public abstract String speak(); }");
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!() };
    assert!(c.is_abstract, "is_abstract should be true");
    assert_eq!(c.methods.len(), 1);
    assert!(c.methods[0].body.is_none(), "abstract method has no body");
}

/// `class Dog extends Animal { … }` parses with the parent TypeRef
/// captured in `extends`.
#[test]
fn extends_clause_captures_parent_type_ref() {
    let ast = parse_clean(
        "public class Dog extends Animal { public Dog() {} }",
    );
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!() };
    let extends = c.extends.as_ref().expect("extends present");
    assert_eq!(extends.name.segments[0].text, "Animal");
}

/// `super(args);` inside a constructor body parses as `Stmt::SuperCall`.
#[test]
fn super_call_in_constructor_body_parses_as_super_call_stmt() {
    let ast = parse_clean(
        "public class Dog extends Animal { public Dog(String name) { super(name); } }",
    );
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!() };
    let ctor = &c.constructors[0];
    let Stmt::SuperCall(args, _) = &ctor.body.statements[0] else {
        panic!("expected SuperCall stmt, got {:?}", ctor.body.statements[0]);
    };
    assert_eq!(args.len(), 1);
    // Argument is `Path(name)` — the constructor parameter `name`.
    let Expr::Path(qn) = &args[0] else { panic!() };
    assert_eq!(qn.segments[0].text, "name");
}

// ---------------------------------------------------------------------------
// Interfaces (Turn 1) — abstract signatures + implements clause
// ---------------------------------------------------------------------------

/// `interface Foo { void bar(); int baz(int x); }` parses with the
/// method signatures captured as body-less FnDecls.
#[test]
fn interface_decl_captures_method_signatures() {
    let ast = parse_clean("public interface Drawable { void draw(); int weight(); }");
    assert_eq!(ast.items.len(), 1);
    let TopLevelDecl::Interface(decl) = &ast.items[0] else {
        panic!("expected interface decl");
    };
    assert_eq!(decl.name.text, "Drawable");
    assert_eq!(decl.methods.len(), 2);
    assert_eq!(decl.methods[0].name.text, "draw");
    assert!(decl.methods[0].body.is_none(), "signatures only");
    assert_eq!(decl.methods[1].name.text, "weight");
}

/// `class C implements A, B { }` parses with two TypeRefs in
/// implements list.
#[test]
fn class_decl_captures_implements_list() {
    let ast = parse_clean(
        "public class Friendly implements Greeter, Drawable { public int magic() { return 1; } }",
    );
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!() };
    assert_eq!(c.implements.len(), 2);
    assert_eq!(c.implements[0].name.segments[0].text, "Greeter");
    assert_eq!(c.implements[1].name.segments[0].text, "Drawable");
}

// ---------------------------------------------------------------------------
// Records (Turn 1) — header-only form
// ---------------------------------------------------------------------------

/// A primitive-component record declaration parses to the right
/// AST shape — header components captured with their types and names.
#[test]
fn record_decl_captures_header_components() {
    let ast = parse_clean("public record Vector3(double x, double y, double z) {}");
    assert_eq!(ast.items.len(), 1);
    let TopLevelDecl::Record(decl) = &ast.items[0] else {
        panic!("expected record top-level decl");
    };
    assert_eq!(decl.visibility, Visibility::Public);
    assert_eq!(decl.name.text, "Vector3");
    assert_eq!(decl.components.len(), 3);
    assert_eq!(decl.components[0].name.text, "x");
    assert_eq!(decl.components[0].ty.name.segments[0].text, "double");
    assert_eq!(decl.components[2].name.text, "z");
}

/// Record with no body (`record Name(…)` — no trailing `{}`) also parses.
#[test]
fn record_decl_without_body_parses() {
    let ast = parse_clean("public record Point(double x, double y)");
    let TopLevelDecl::Record(decl) = &ast.items[0] else { panic!() };
    assert_eq!(decl.components.len(), 2);
}

/// Generic record `Pair<A, B>(A first, B second)` parses with generic
/// params captured.
#[test]
fn generic_record_decl_captures_type_parameters() {
    let ast = parse_clean("public record Pair<A, B>(A first, B second) {}");
    let TopLevelDecl::Record(decl) = &ast.items[0] else { panic!() };
    assert_eq!(decl.generic_params.len(), 2);
    assert_eq!(decl.generic_params[0].name.text, "A");
    assert_eq!(decl.generic_params[1].name.text, "B");
    assert_eq!(decl.components[0].ty.name.segments[0].text, "A");
    assert_eq!(decl.components[1].ty.name.segments[0].text, "B");
}

// ---------------------------------------------------------------------------
// Generics (Turn 1)
// ---------------------------------------------------------------------------

/// `class Box<T> { … }` parses with the right generic_params list.
#[test]
fn generic_class_decl_captures_type_parameter() {
    let ast = parse_clean("public class Box<T> { private T value; }");
    assert_eq!(ast.items.len(), 1);
    let TopLevelDecl::Class(c) = &ast.items[0] else {
        panic!("expected class top-level decl");
    };
    assert_eq!(c.generic_params.len(), 1);
    assert_eq!(c.generic_params[0].name.text, "T");
    // The field's type is a single-segment path "T" — same shape as
    // primitives; the backend identifies it as generic via the
    // class's params list.
    assert_eq!(c.fields[0].ty.as_ref().unwrap().name.segments[0].text, "T");
}

/// Multi-parameter generic classes parse: `class Map<K, V> { … }`.
#[test]
fn multi_param_generic_class_decl_captures_all_params() {
    let ast = parse_clean("public class Map<K, V> { private K key; private V value; }");
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!() };
    assert_eq!(c.generic_params.len(), 2);
    assert_eq!(c.generic_params[0].name.text, "K");
    assert_eq!(c.generic_params[1].name.text, "V");
}

/// Generic-args in type position: `Box<int> b;` captures the
/// type-argument list on the field's TypeRef.
#[test]
fn type_position_generic_args_fill_generic_args_vec() {
    let ast = parse_clean(
        "public void main() { Box<int> b = new Box<int>(5); print(b); }",
    );
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has type");
    assert_eq!(ty.name.segments[0].text, "Box");
    assert_eq!(ty.generic_args.len(), 1);
    let inner = ty.generic_args[0]
        .as_type()
        .expect("first arg is concrete type");
    assert_eq!(inner.name.segments[0].text, "int");
}

/// Explicit-generic construction: `new Box<int>(42)` fills the
/// `generic_args` field on the NewObjectExpr.
#[test]
fn new_object_expr_captures_explicit_generic_args() {
    let ast = parse_clean("public void main() { var b = new Box<int>(42); print(b); }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::NewObject(n)) = v.init.as_ref() else {
        panic!("expected NewObject init");
    };
    assert_eq!(n.class_name.segments[0].text, "Box");
    assert_eq!(n.generic_args.len(), 1);
    assert_eq!(n.generic_args[0].name.segments[0].text, "int");
    assert_eq!(n.args.len(), 1);
}

// ---------------------------------------------------------------------------
// Bounded wildcards (§A.2.4 PECS) — `?`, `? extends T`, `? super T`
// ---------------------------------------------------------------------------

/// `List<?>` parses as a single wildcard arg with no bound.
#[test]
fn unbounded_wildcard_parses() {
    use juxc_ast::GenericArg;
    let ast = parse_clean("public void main() { List<?> xs = null; print(xs); }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has type");
    assert_eq!(ty.generic_args.len(), 1);
    match &ty.generic_args[0] {
        GenericArg::Wildcard(w) => assert!(w.bound.is_none()),
        other => panic!("expected unbounded wildcard, got {other:?}"),
    }
}

/// `List<? extends Animal>` parses as a wildcard with an `Extends` bound.
#[test]
fn extends_wildcard_parses() {
    use juxc_ast::{GenericArg, WildcardBound};
    let ast = parse_clean(
        "public void main() { List<? extends Animal> xs = null; print(xs); }",
    );
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has type");
    match &ty.generic_args[0] {
        GenericArg::Wildcard(w) => match &w.bound {
            Some(WildcardBound::Extends(t)) => {
                assert_eq!(t.name.segments[0].text, "Animal");
            }
            other => panic!("expected `? extends Animal`, got {other:?}"),
        },
        other => panic!("expected wildcard, got {other:?}"),
    }
}

/// `List<? super Dog>` parses as a wildcard with a `Super` bound.
#[test]
fn super_wildcard_parses() {
    use juxc_ast::{GenericArg, WildcardBound};
    let ast = parse_clean(
        "public void main() { List<? super Dog> xs = null; print(xs); }",
    );
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has type");
    match &ty.generic_args[0] {
        GenericArg::Wildcard(w) => match &w.bound {
            Some(WildcardBound::Super(t)) => {
                assert_eq!(t.name.segments[0].text, "Dog");
            }
            other => panic!("expected `? super Dog`, got {other:?}"),
        },
        other => panic!("expected wildcard, got {other:?}"),
    }
}

/// Wildcards in `new Foo<? extends T>(...)` are rejected as E0200.
#[test]
fn wildcard_in_new_expr_is_rejected() {
    let (_ast, n) = parse_with_errors(
        "public void main() { var b = new Box<? extends Animal>(); print(b); }",
    );
    assert!(n >= 1, "expected at least one parse diagnostic");
}

// ---------------------------------------------------------------------------
// Pattern matching (§A.2.8 + §A.3) — switch / case / default
// ---------------------------------------------------------------------------

/// Statement-form switch parses an `Expr::Switch` wrapped in `Stmt::Expr`.
#[test]
fn statement_switch_parses_to_expr_stmt_with_switch_expr() {
    use juxc_ast::Pattern;
    let ast = parse_clean(
        r#"public void main() {
               var c = 1;
               switch (c) {
                   case 1 -> print("one");
                   default -> print("other");
               }
           }"#,
    );
    let body = body_of(&ast.items[0]);
    // Last statement is the switch (after the `var c = 1;`).
    let Stmt::Expr(Expr::Switch(s)) = &body.statements[1] else {
        panic!("expected Stmt::Expr(Switch), got {:?}", body.statements[1]);
    };
    assert_eq!(s.arms.len(), 2);
    // First arm is a literal pattern.
    assert!(matches!(&s.arms[0].pattern, Pattern::Literal(_, _)));
    // `default` lowers to a wildcard pattern.
    assert!(matches!(&s.arms[1].pattern, Pattern::Wildcard(_)));
}

/// Expression-form switch lives in expression position — here, the
/// RHS of a `var` declaration.
#[test]
fn expression_switch_parses_in_var_init_position() {
    let ast = parse_clean(
        r#"public void main() {
               var c = 1;
               var label = switch (c) {
                   case 1 -> 10;
                   case 2 -> 20;
                   default -> 0;
               };
           }"#,
    );
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[1] else { panic!() };
    let Some(Expr::Switch(s)) = v.init.as_ref() else {
        panic!("expected Switch init, got {:?}", v.init);
    };
    assert_eq!(s.arms.len(), 3);
}

/// Enum-variant pattern with payload binders: `Token.Number(var n)`.
/// Yields a recursive Pattern shape — outer EnumVariant containing a
/// nested Bind sub-pattern.
#[test]
fn variant_pattern_with_var_binding_parses() {
    use juxc_ast::Pattern;
    let ast = parse_clean(
        r#"public void main() {
               var t = 0;
               switch (t) {
                   case Token.Number(var n) -> print(n);
                   default -> print("other");
               }
           }"#,
    );
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Switch(s)) = &body.statements[1] else { panic!() };
    let Pattern::EnumVariant { path, args, has_parens, .. } = &s.arms[0].pattern else {
        panic!("expected EnumVariant pattern");
    };
    assert!(*has_parens);
    assert_eq!(path.segments.len(), 2);
    assert_eq!(path.segments[0].text, "Token");
    assert_eq!(path.segments[1].text, "Number");
    assert_eq!(args.len(), 1);
    let Pattern::Bind(ident) = &args[0] else {
        panic!("expected nested Bind");
    };
    assert_eq!(ident.text, "n");
}

/// `when` guard (§A.2.8) — parses into `SwitchArm.guard`; unguarded
/// arms carry `None`.
#[test]
fn switch_arm_when_guard_parses() {
    use juxc_ast::Pattern;
    let ast = parse_clean(
        r#"public void main() {
               var n = 5;
               switch (n) {
                   case var x when x > 3 -> print("big");
                   default -> print("small");
               }
           }"#,
    );
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Switch(s)) = &body.statements[1] else { panic!() };
    assert!(matches!(&s.arms[0].pattern, Pattern::Bind(_)));
    assert!(
        matches!(s.arms[0].guard.as_ref(), Some(Expr::Binary(_))),
        "expected Binary guard, got {:?}",
        s.arms[0].guard,
    );
    assert!(s.arms[1].guard.is_none());
}

/// Or-pattern (§A.3) — `case 1 | 2 | 3 ->` folds the alternatives
/// into a single `Pattern::Or`.
#[test]
fn switch_arm_or_pattern_parses() {
    use juxc_ast::Pattern;
    let ast = parse_clean(
        r#"public void main() {
               var n = 5;
               switch (n) {
                   case 1 | 2 | 3 -> print("low");
                   default -> print("high");
               }
           }"#,
    );
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Switch(s)) = &body.statements[1] else { panic!() };
    let Pattern::Or(alts, _) = &s.arms[0].pattern else {
        panic!("expected Or pattern, got {:?}", s.arms[0].pattern);
    };
    assert_eq!(alts.len(), 3);
    assert!(alts.iter().all(|p| matches!(p, Pattern::Literal(_, _))));
}

// ---------------------------------------------------------------------------
// Enums (§7.7)
// ---------------------------------------------------------------------------

/// Unit-variant enum declaration parses into the right shape.
#[test]
fn unit_enum_decl_parses() {
    let ast = parse_clean("public enum Color { Red, Green, Blue }");
    assert_eq!(ast.items.len(), 1);
    let TopLevelDecl::Enum(decl) = &ast.items[0] else {
        panic!("expected enum top-level decl, got {:?}", ast.items[0]);
    };
    assert_eq!(decl.visibility, Visibility::Public);
    assert_eq!(decl.name.text, "Color");
    assert_eq!(decl.variants.len(), 3);
    assert_eq!(decl.variants[0].name.text, "Red");
    assert!(decl.variants[0].payload.is_empty());
    assert_eq!(decl.variants[2].name.text, "Blue");
}

/// Tuple-payload variant: `Number(int)`. Payload type captured.
#[test]
fn tuple_payload_enum_variant_parses() {
    let ast = parse_clean("public enum Token { Number(int) }");
    let TopLevelDecl::Enum(decl) = &ast.items[0] else { panic!() };
    assert_eq!(decl.variants.len(), 1);
    let v = &decl.variants[0];
    assert_eq!(v.name.text, "Number");
    assert_eq!(v.payload.len(), 1);
    assert_eq!(v.payload[0].ty.name.segments[0].text, "int");
    assert!(v.payload[0].name.is_none());
}

/// Named payload slots: `Ok(int status, String body)`.
#[test]
fn named_payload_slots_parse() {
    let ast = parse_clean(
        "public enum HttpResponse { Ok(int status, String body) }",
    );
    let TopLevelDecl::Enum(decl) = &ast.items[0] else { panic!() };
    let v = &decl.variants[0];
    assert_eq!(v.payload.len(), 2);
    assert_eq!(v.payload[0].ty.name.segments[0].text, "int");
    assert_eq!(v.payload[0].name.as_ref().unwrap().text, "status");
    assert_eq!(v.payload[1].ty.name.segments[0].text, "String");
    assert_eq!(v.payload[1].name.as_ref().unwrap().text, "body");
}

// ---------------------------------------------------------------------------
// String interpolation (§3.4)
// ---------------------------------------------------------------------------

/// Literal-only interpolated string parses as a single `Literal` segment.
#[test]
fn interp_string_with_no_interpolation_yields_one_literal_segment() {
    use juxc_ast::InterpSegment;
    let ast = parse_clean(r#"public void main() { var s = $"plain"; }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::InterpString(s)) = v.init.as_ref() else {
        panic!("expected InterpString, got {:?}", v.init);
    };
    assert_eq!(s.segments.len(), 1);
    assert!(matches!(&s.segments[0], InterpSegment::Literal(t) if t == "plain"));
}

/// `$name` bare-ident form yields literal-then-Bare segments.
#[test]
fn interp_string_bare_ident_yields_bare_segment() {
    use juxc_ast::InterpSegment;
    let ast = parse_clean(r#"public void main() { var s = $"hi $name!"; }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::InterpString(s)) = v.init.as_ref() else { panic!() };
    assert_eq!(s.segments.len(), 3);
    assert!(matches!(&s.segments[0], InterpSegment::Literal(t) if t == "hi "));
    let InterpSegment::Bare(ident) = &s.segments[1] else {
        panic!("expected Bare segment");
    };
    assert_eq!(ident.text, "name");
    assert!(matches!(&s.segments[2], InterpSegment::Literal(t) if t == "!"));
}

/// `${expr}` form parses the inner text as a Jux expression.
#[test]
fn interp_string_expr_form_recursively_parses_inner() {
    use juxc_ast::InterpSegment;
    let ast = parse_clean(r#"public void main() { var s = $"sum=${a + b}"; }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::InterpString(s)) = v.init.as_ref() else { panic!() };
    assert_eq!(s.segments.len(), 2);
    assert!(matches!(&s.segments[0], InterpSegment::Literal(t) if t == "sum="));
    let InterpSegment::Expr(inner) = &s.segments[1] else {
        panic!("expected Expr segment");
    };
    assert!(matches!(&**inner, Expr::Binary(_)));
}

/// Empty interpolated string `$""` parses with no segments.
#[test]
fn interp_empty_string_yields_no_segments() {
    let ast = parse_clean(r#"public void main() { var s = $""; }"#);
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::InterpString(s)) = v.init.as_ref() else { panic!() };
    assert!(s.segments.is_empty(), "empty $\"\" should have zero segments");
}

// ---------------------------------------------------------------------------
// Imports (§A.2.1)
// ---------------------------------------------------------------------------

/// `import com.example.Foo;` — bare single-name import. Path captures
/// every dotted segment in declaration order; no wildcard, no alias.
#[test]
fn bare_import_captures_dotted_path() {
    let ast = parse_clean("import com.example.Foo;");
    assert_eq!(ast.imports.len(), 1);
    match &ast.imports[0].spec {
        juxc_ast::ImportSpec::Path { name, wildcard, alias } => {
            assert!(!wildcard, "expected no wildcard");
            assert!(alias.is_none(), "expected no alias");
            let segs: Vec<_> = name.segments.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(segs, vec!["com", "example", "Foo"]);
        }
        other => panic!("expected Path, got {other:?}"),
    }
}

/// `import com.example.*;` — wildcard sets the flag and stops the path
/// at the segment before the star.
#[test]
fn wildcard_import_sets_flag() {
    let ast = parse_clean("import com.example.*;");
    match &ast.imports[0].spec {
        juxc_ast::ImportSpec::Path { name, wildcard, alias } => {
            assert!(*wildcard);
            assert!(alias.is_none());
            let segs: Vec<_> = name.segments.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(segs, vec!["com", "example"]);
        }
        other => panic!("expected Path, got {other:?}"),
    }
}

/// `import com.example.Foo as Bar;` — alias names the import locally.
#[test]
fn aliased_import_captures_rename() {
    let ast = parse_clean("import com.example.Foo as Bar;");
    match &ast.imports[0].spec {
        juxc_ast::ImportSpec::Path { name, wildcard, alias } => {
            assert!(!wildcard);
            let segs: Vec<_> = name.segments.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(segs, vec!["com", "example", "Foo"]);
            assert_eq!(alias.as_ref().map(|i| i.text.as_str()), Some("Bar"));
        }
        other => panic!("expected Path, got {other:?}"),
    }
}

/// `import com.example.{ A, B as B2, C };` — grouped form with mixed
/// per-item aliases. Prefix captures the dotted path before `{`, items
/// preserve declaration order.
#[test]
fn grouped_import_captures_items() {
    let ast = parse_clean("import com.example.{ A, B as B2, C };");
    match &ast.imports[0].spec {
        juxc_ast::ImportSpec::Items { prefix, items } => {
            let segs: Vec<_> = prefix.segments.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(segs, vec!["com", "example"]);
            assert_eq!(items.len(), 3);
            assert_eq!(items[0].name.text, "A");
            assert!(items[0].alias.is_none());
            assert_eq!(items[1].name.text, "B");
            assert_eq!(items[1].alias.as_ref().map(|i| i.text.as_str()), Some("B2"));
            assert_eq!(items[2].name.text, "C");
            assert!(items[2].alias.is_none());
        }
        other => panic!("expected Items, got {other:?}"),
    }
}

/// Single-item group is the same shape as a bare-import — but `{...}`
/// syntax is still accepted (Java does the same).
#[test]
fn single_item_grouped_import_parses() {
    let ast = parse_clean("import foo.{ Bar };");
    match &ast.imports[0].spec {
        juxc_ast::ImportSpec::Items { items, .. } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].name.text, "Bar");
        }
        other => panic!("expected Items, got {other:?}"),
    }
}

/// Multiple imports in a row land in `ast.imports` in source order.
#[test]
fn multiple_imports_preserve_order() {
    let ast = parse_clean(
        r#"
        import a.A;
        import b.*;
        import c.{ X, Y };
        "#,
    );
    assert_eq!(ast.imports.len(), 3);
    assert!(matches!(
        &ast.imports[0].spec,
        juxc_ast::ImportSpec::Path { wildcard: false, alias: None, .. }
    ));
    assert!(matches!(
        &ast.imports[1].spec,
        juxc_ast::ImportSpec::Path { wildcard: true, .. }
    ));
    assert!(matches!(&ast.imports[2].spec, juxc_ast::ImportSpec::Items { .. }));
}

/// `import foo.* as Bar;` — wildcard + alias is a shape error. The
/// parser still produces an ImportDecl (with the wildcard flag set), so
/// downstream phases can proceed.
#[test]
fn wildcard_with_alias_is_diagnostic() {
    let (ast, n) = parse_with_errors("import foo.* as Bar;");
    assert!(n >= 1, "expected at least one diagnostic, got {n}");
    assert_eq!(ast.imports.len(), 1);
}

/// `import com.example.{};` — empty group is a shape error.
#[test]
fn empty_grouped_import_is_diagnostic() {
    let (_ast, n) = parse_with_errors("import com.example.{};");
    assert!(n >= 1, "expected at least one diagnostic, got {n}");
}

/// `import foo.{ Bar, };` — trailing comma rejected. The parser
/// recovers to `}` and emits a diagnostic.
#[test]
fn trailing_comma_in_grouped_import_is_diagnostic() {
    let (_ast, n) = parse_with_errors("import foo.{ Bar, };");
    assert!(n >= 1, "expected at least one diagnostic, got {n}");
}

/// `import foo.;` — trailing dot. Parser bails out cleanly and the
/// next top-level decl still parses.
#[test]
fn malformed_import_does_not_swallow_next_decl() {
    let (ast, n) = parse_with_errors(
        r#"
        import foo.;
        public void main() {}
        "#,
    );
    assert!(n >= 1, "expected at least one diagnostic, got {n}");
    assert_eq!(ast.items.len(), 1);
}

/// Imports come before top-level decls in the AST regardless of any
/// surrounding whitespace.
#[test]
fn imports_separate_from_top_level_items() {
    let ast = parse_clean(
        r#"
        package com.example.app;
        import std.io.*;
        import std.fmt.{ println };

        public void main() {}
        "#,
    );
    assert!(ast.package.is_some());
    assert_eq!(ast.imports.len(), 2);
    assert_eq!(ast.items.len(), 1);
}

// ---------------------------------------------------------------------------
// Operator overloading (§O.2) — class member declarations
// ---------------------------------------------------------------------------

/// Helper: pull the first class declaration out of a CompilationUnit.
fn first_class(unit: &juxc_ast::CompilationUnit) -> &juxc_ast::ClassDecl {
    for item in &unit.items {
        if let TopLevelDecl::Class(class) = item {
            return class;
        }
    }
    panic!("no top-level class in unit");
}

/// `public bool operator==(Path other) { ... }` parses into an
/// OperatorDecl in `class.operators` with kind Eq and one parameter.
#[test]
fn operator_eq_parses_with_one_param() {
    let ast = parse_clean(
        r#"
        public class Path {
            public bool operator==(Path other) { return true; }
        }
        "#,
    );
    let class = first_class(&ast);
    assert!(class.methods.is_empty(), "operator should not land in methods");
    assert_eq!(class.operators.len(), 1);
    let op = &class.operators[0];
    assert_eq!(op.kind, juxc_ast::OperatorKind::Eq);
    assert_eq!(op.params.len(), 1);
    assert_eq!(op.params[0].name.text, "other");
}

/// `operator<=>` parses as the Cmp three-way comparison kind.
#[test]
fn operator_cmp_parses() {
    let ast = parse_clean(
        r#"
        public class Path {
            public int operator<=>(Path other) { return 0; }
        }
        "#,
    );
    let op = &first_class(&ast).operators[0];
    assert_eq!(op.kind, juxc_ast::OperatorKind::Cmp);
}

/// `operator hash()` and `operator string()` parse as the bareword
/// operator kinds with zero parameters.
#[test]
fn operator_hash_and_string_parse_as_barewords() {
    let ast = parse_clean(
        r#"
        public class Path {
            public int operator hash() { return 0; }
            public String operator string() { return "x"; }
        }
        "#,
    );
    let ops = &first_class(&ast).operators;
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].kind, juxc_ast::OperatorKind::Hash);
    assert!(ops[0].params.is_empty());
    assert_eq!(ops[1].kind, juxc_ast::OperatorKind::ToString);
    assert!(ops[1].params.is_empty());
}

/// `operator[]` (indexed read) and `operator[]=` (indexed write) parse
/// as distinct OperatorKind values.
#[test]
fn operator_index_and_index_set_parse() {
    let ast = parse_clean(
        r#"
        public class Vec {
            public int operator[](int i) { return 0; }
            public void operator[]=(int i, int v) { }
        }
        "#,
    );
    let ops = &first_class(&ast).operators;
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].kind, juxc_ast::OperatorKind::Index);
    assert_eq!(ops[0].params.len(), 1);
    assert_eq!(ops[1].kind, juxc_ast::OperatorKind::IndexSet);
    assert_eq!(ops[1].params.len(), 2);
}

/// `operator()` (callable) parses with arbitrary param arity — the
/// parser just records what the user wrote; tycheck decides what's
/// valid.
#[test]
fn operator_call_parses() {
    let ast = parse_clean(
        r#"
        public class Func {
            public int operator()(int x, int y) { return x; }
        }
        "#,
    );
    let op = &first_class(&ast).operators[0];
    assert_eq!(op.kind, juxc_ast::OperatorKind::Call);
    assert_eq!(op.params.len(), 2);
}

/// Each punctuator-operator (`+`, `-`, `*`, etc.) round-trips into the
/// matching OperatorKind. One representative test pinning the full
/// mapping for arithmetic + bitwise + shift + range.
#[test]
fn punctuator_operators_round_trip() {
    let ast = parse_clean(
        r#"
        public class M {
            public int operator+(int o) { return 0; }
            public int operator-(int o) { return 0; }
            public int operator*(int o) { return 0; }
            public int operator/(int o) { return 0; }
            public int operator%(int o) { return 0; }
            public int operator&(int o) { return 0; }
            public int operator|(int o) { return 0; }
            public int operator^(int o) { return 0; }
            public int operator~() { return 0; }
            public int operator<<(int o) { return 0; }
            public int operator>>(int o) { return 0; }
            public int operator..(int o) { return 0; }
            public int operator..=(int o) { return 0; }
        }
        "#,
    );
    let kinds: Vec<_> = first_class(&ast)
        .operators
        .iter()
        .map(|o| o.kind)
        .collect();
    use juxc_ast::OperatorKind::*;
    assert_eq!(
        kinds,
        vec![Plus, Minus, Mul, Div, Rem, BitAnd, BitOr, BitXor, BitNot, Shl, Shr, Range,
             RangeInclusive],
    );
}

/// Class with mixed methods + operators preserves both lists
/// independently; operators don't leak into the methods slot.
#[test]
fn class_with_method_and_operator_preserves_both_lists() {
    let ast = parse_clean(
        r#"
        public class Path {
            public String value;
            public Path(String v) { this.value = v; }
            public String describe() { return this.value; }
            public bool operator==(Path other) { return true; }
        }
        "#,
    );
    let class = first_class(&ast);
    assert_eq!(class.fields.len(), 1);
    assert_eq!(class.constructors.len(), 1);
    assert_eq!(class.methods.len(), 1);
    assert_eq!(class.methods[0].name.text, "describe");
    assert_eq!(class.operators.len(), 1);
    assert_eq!(class.operators[0].kind, juxc_ast::OperatorKind::Eq);
}

/// Unknown operator symbol (e.g. `operator !`) emits E0200 and the
/// parser recovers without consuming the rest of the class body
/// catastrophically.
#[test]
fn unknown_operator_symbol_is_diagnostic() {
    let (_ast, n) = parse_with_errors(
        r#"
        public class Path {
            public bool operator!(Path other) { return true; }
        }
        "#,
    );
    assert!(n >= 1, "expected at least one diagnostic, got {n}");
}

/// `operator <op>(...) = delete;` parses with `is_deleted = true` and
/// no body. Per §O.3.4 the user opts out of auto-derivation for that
/// operator on records / structs / enums.
#[test]
fn operator_delete_form_round_trips() {
    let ast = parse_clean(
        r#"
        public class C {
            public String operator string() = delete;
        }
        "#,
    );
    let op = &first_class(&ast).operators[0];
    assert_eq!(op.kind, juxc_ast::OperatorKind::ToString);
    assert!(op.is_deleted, "expected is_deleted = true");
    assert!(op.body.is_none(), "deleted operator must have no body");
}

/// `operator <op>(...) { body }` (the normal form) parses with
/// `is_deleted = false` and a present body — the existing happy path.
#[test]
fn operator_with_body_has_is_deleted_false() {
    let ast = parse_clean(
        r#"
        public class C {
            public bool operator==(C other) { return true; }
        }
        "#,
    );
    let op = &first_class(&ast).operators[0];
    assert!(!op.is_deleted);
    assert!(op.body.is_some());
}

/// Records can host operator declarations in their body. Each entry
/// lands in `record.operators`; the header components stay in
/// `record.components` as before.
#[test]
fn record_body_holds_operator_decls() {
    let ast = parse_clean(
        r#"
        public record Money(int cents) {
            public String operator string() {
                return "$";
            }
        }
        "#,
    );
    for item in &ast.items {
        if let TopLevelDecl::Record(r) = item {
            assert_eq!(r.components.len(), 1);
            assert_eq!(r.operators.len(), 1);
            assert_eq!(r.operators[0].kind, juxc_ast::OperatorKind::ToString);
            assert!(!r.operators[0].is_deleted);
            return;
        }
    }
    panic!("no record decl in unit");
}

/// Records can declare methods alongside operators per grammar
/// §A.2.4. Each method lands in `record.methods`; operators stay in
/// `record.operators`. Header components live in `components`.
#[test]
fn record_body_holds_methods() {
    let ast = parse_clean(
        r#"
        public record Money(int cents) {
            public int doubled() { return this.cents * 2; }
            public String operator string() { return "Money"; }
        }
        "#,
    );
    for item in &ast.items {
        if let TopLevelDecl::Record(r) = item {
            assert_eq!(r.components.len(), 1);
            assert_eq!(r.methods.len(), 1, "method should land in record.methods");
            assert_eq!(r.methods[0].name.text, "doubled");
            assert_eq!(r.operators.len(), 1, "operator should stay in record.operators");
            return;
        }
    }
    panic!("no record decl in unit");
}

/// `record Foo(...) { operator string() = delete; }` is the canonical
/// §O.3.4 example — record body carries a single `= delete;` operator.
#[test]
fn record_with_operator_delete_parses() {
    let ast = parse_clean(
        r#"
        public record OpaqueToken(String secret) {
            public String operator string() = delete;
        }
        "#,
    );
    for item in &ast.items {
        if let TopLevelDecl::Record(r) = item {
            assert_eq!(r.operators.len(), 1);
            let op = &r.operators[0];
            assert_eq!(op.kind, juxc_ast::OperatorKind::ToString);
            assert!(op.is_deleted);
            assert!(op.body.is_none());
            return;
        }
    }
    panic!("no record decl in unit");
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Extract the body block of a top-level function decl, panicking with
/// a clear message if the decl isn't a function or has no body.
fn body_of(item: &TopLevelDecl) -> &juxc_ast::Block {
    let TopLevelDecl::Function(fn_decl) = item else {
        panic!("expected a function top-level decl, got {item:?}");
    };
    body_of_fn(fn_decl)
}

/// Same as [`body_of`] but takes the `FnDecl` directly.
fn body_of_fn(fn_decl: &FnDecl) -> &juxc_ast::Block {
    fn_decl.body.as_ref().expect("function body present")
}

// ---------------------------------------------------------------------------
// Type aliases (§A.2.4 `type-alias`)
// ---------------------------------------------------------------------------

/// `type UserId = int;` parses into a `TypeAlias` top-level decl
/// with the right name and target.
#[test]
fn bare_type_alias_parses() {
    let ast = parse_clean("public type UserId = int;");
    let TopLevelDecl::TypeAlias(alias) = &ast.items[0] else {
        panic!("expected TypeAlias, got {:?}", ast.items[0]);
    };
    assert_eq!(alias.name.text, "UserId");
    assert!(alias.generic_params.is_empty());
    assert_eq!(alias.target.name.segments[0].text, "int");
}

// ---------------------------------------------------------------------------
// Lambdas (§A.2.9)
// ---------------------------------------------------------------------------

/// Single-param untyped form: `x -> x * 2`.
#[test]
fn single_param_lambda_parses() {
    use juxc_ast::{LambdaBody, Stmt};
    let ast = parse_clean("public void main() { var f = x -> x; print(f(1)); }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::Lambda(l)) = v.init.as_ref() else {
        panic!("expected Lambda init, got {:?}", v.init);
    };
    assert_eq!(l.params.len(), 1);
    assert_eq!(l.params[0].name.text, "x");
    assert!(l.params[0].ty.is_none());
    matches!(l.body, LambdaBody::Expr(_));
}

/// Multi-param parenthesized form: `(a, b) -> a + b`.
#[test]
fn multi_param_lambda_parses() {
    use juxc_ast::Stmt;
    let ast = parse_clean(
        "public void main() { var f = (a, b) -> a; print(f(1, 2)); }",
    );
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::Lambda(l)) = v.init.as_ref() else { panic!() };
    assert_eq!(l.params.len(), 2);
    assert_eq!(l.params[0].name.text, "a");
    assert_eq!(l.params[1].name.text, "b");
}

/// Typed-param form: `(int x) -> x * 2`.
#[test]
fn typed_param_lambda_parses() {
    use juxc_ast::Stmt;
    let ast = parse_clean(
        "public void main() { var f = (int x) -> x; print(f(7)); }",
    );
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::Lambda(l)) = v.init.as_ref() else { panic!() };
    assert_eq!(l.params.len(), 1);
    assert!(l.params[0].ty.is_some());
}

/// Block-body form: `(x) -> { return x; }`.
#[test]
fn block_body_lambda_parses() {
    use juxc_ast::{LambdaBody, Stmt};
    let ast = parse_clean(
        "public void main() { var f = (x) -> { return x; }; print(f(1)); }",
    );
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let Some(Expr::Lambda(l)) = v.init.as_ref() else { panic!() };
    matches!(l.body, LambdaBody::Block(_));
}

/// `type Pair<A, B> = Tuple<A, B>;` carries its generic params and
/// target.
#[test]
fn generic_type_alias_parses() {
    let ast = parse_clean("public type Pair<A, B> = Tuple<A, B>;");
    let TopLevelDecl::TypeAlias(alias) = &ast.items[0] else {
        panic!("expected TypeAlias");
    };
    assert_eq!(alias.name.text, "Pair");
    assert_eq!(alias.generic_params.len(), 2);
    assert_eq!(alias.generic_params[0].name.text, "A");
    assert_eq!(alias.generic_params[1].name.text, "B");
    assert_eq!(alias.target.name.segments[0].text, "Tuple");
    assert_eq!(alias.target.generic_args.len(), 2);
}

// ============================================================================
// Production syntax coverage (JUX-GRAMMAR §A.2.4–§A.2.7) — features the Rust
// std/crate stub surface exercises end-to-end. Each must parse with ZERO
// diagnostics (`parse_clean` asserts that).
// ============================================================================

/// Nested generics whose closing `>>` is glued into one `GtGt` token by the
/// lexer still close two lists (`List<List<int>>`, `Map<K, Vec<V>>`).
#[test]
fn nested_generics_with_glued_gtgt_parse() {
    let ast = parse_clean(
        "public class C {\n\
            public List<List<int>> a();\n\
            public Map<String, List<int>> b();\n\
            public A<B<C<int>>> c();\n\
         }",
    );
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!("class") };
    assert_eq!(c.methods.len(), 3);
}

/// A `struct` declaration parses (grammar §A.2.5) as a `ClassDecl` flagged
/// `is_struct`, with its fields and generics.
#[test]
fn struct_decl_parses_with_fields_and_generics() {
    let ast = parse_clean(
        "public struct Pair<A, B> {\n\
            public A first;\n\
            public B second;\n\
         }",
    );
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!("struct→class") };
    assert!(c.is_struct, "struct origin recorded");
    assert!(c.is_final, "structs are implicitly final");
    assert_eq!(c.generic_params.len(), 2);
    assert_eq!(c.fields.len(), 2);
}

/// Generic enums carry their type parameters (`enum Cow<B>`,
/// `enum Entry<K, V, A>`), and variant payloads may reference them.
#[test]
fn generic_enum_parses() {
    let ast = parse_clean(
        "public enum Cow<B> {\n\
            Borrowed(B), Owned(B)\n\
         }",
    );
    let TopLevelDecl::Enum(e) = &ast.items[0] else { panic!("enum") };
    assert_eq!(e.generic_params.len(), 1);
    assert_eq!(e.generic_params[0].name.text, "B");
    assert_eq!(e.variants.len(), 2);
}

/// Method-level generic parameters between the name and the parameter list
/// (`T map<U>(U f)`) are classified as methods, not fields.
#[test]
fn method_level_generics_parse() {
    let ast = parse_clean(
        "public class C {\n\
            public U map<U>(U f);\n\
         }",
    );
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!("class") };
    assert_eq!(c.methods.len(), 1);
    assert_eq!(c.methods[0].generic_params.len(), 1);
}

/// Function types in return and parameter position, including a `void` result
/// (`(A) -> void`), parse.
#[test]
fn function_types_in_return_and_param_parse() {
    let ast = parse_clean(
        "public class C {\n\
            public (int) -> void onClick();\n\
            public void setHook((int) -> bool h);\n\
         }",
    );
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!("class") };
    assert_eq!(c.methods.len(), 2);
}

/// Nullable and array suffixes parse in EITHER order — `T?[]` and `T[]?`.
#[test]
fn nullable_and_array_suffix_order_independent() {
    let ast = parse_clean(
        "public class C {\n\
            public int?[] a();\n\
            public int[]? b();\n\
         }",
    );
    let TopLevelDecl::Class(c) = &ast.items[0] else { panic!("class") };
    assert_eq!(c.methods.len(), 2);
}

/// A constant may be declared without an initializer (`.jux.d` stub form,
/// §G.2) — `public const char SEP;`.
#[test]
fn bodyless_const_parses() {
    let ast = parse_clean("public const char SEP;");
    let TopLevelDecl::Const(c) = &ast.items[0] else { panic!("const") };
    assert_eq!(c.name.text, "SEP");
}
