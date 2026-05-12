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

/// `x += 1;` parses to an AssignStmt whose `value` is a `Binary(Add, x, 1)`.
#[test]
fn plus_equals_desugars_to_assign_with_binary() {
    use juxc_ast::BinaryOp;
    let ast = parse_clean("public void main() { var x = 1; x += 2; }");
    let body = body_of(&ast.items[0]);
    let Stmt::Assign(a) = &body.statements[1] else {
        panic!("expected Stmt::Assign, got {:?}", body.statements[1]);
    };
    let Expr::Path(t_qn) = &a.target else { panic!("expected Path lvalue") };
    assert_eq!(t_qn.segments[0].text, "x");
    let Expr::Binary(b) = &a.value else {
        panic!("compound assignment should desugar to a Binary expr");
    };
    assert_eq!(b.op, BinaryOp::Add);
    // Left side of the binary should be a read of `x`.
    let Expr::Path(qn) = &*b.left else { panic!() };
    assert_eq!(qn.segments[0].text, "x");
}

/// All five arithmetic compound operators desugar with the right op.
#[test]
fn all_compound_arithmetic_ops_desugar() {
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
        let Expr::Binary(b) = &a.value else { panic!() };
        assert_eq!(b.op, expected, "wrong op for {op_src}");
    }
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
    assert!(matches!(w1.body.statements[0], Stmt::Break(_)));
    let Stmt::While(w2) = &body.statements[1] else { panic!() };
    assert!(matches!(w2.body.statements[0], Stmt::Continue(_)));
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
    let ast = parse_clean("public void main() { print(--x); }");
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
    let ast = parse_clean("public void main() { print(3.14); }");
    let body = body_of(&ast.items[0]);
    let Stmt::Expr(Expr::Call(call)) = &body.statements[0] else { panic!() };
    let Expr::Literal(Literal::Float(f)) = &call.args[0] else {
        panic!("expected Float literal, got {:?}", call.args[0]);
    };
    assert!((f.value - 3.14).abs() < 1e-9);
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
    use juxc_ast::ArrayShape;
    let ast = parse_clean("public void main() { int[10] xs = new int[10]; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else {
        panic!("expected typed local, got {:?}", body.statements[0]);
    };
    // Type carries the fixed-size shape.
    let ty = v.ty.as_ref().expect("typed local has explicit type");
    assert_eq!(ty.name.segments[0].text, "int");
    let shape = ty.array_shape.as_ref().expect("array_shape is set");
    let ArrayShape::Fixed(size) = shape else {
        panic!("expected Fixed shape, got {:?}", shape);
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
    use juxc_ast::ArrayShape;
    let ast = parse_clean("public void main() { int[] xs = new int[]{1, 2}; }");
    let body = body_of(&ast.items[0]);
    let Stmt::VarDecl(v) = &body.statements[0] else { panic!() };
    let ty = v.ty.as_ref().expect("typed local has explicit type");
    assert!(matches!(ty.array_shape, Some(ArrayShape::Dynamic)), "expected Dynamic shape");
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
    assert_eq!(c.fields[0].ty.name.segments[0].text, "T");
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
    assert_eq!(ty.generic_args[0].name.segments[0].text, "int");
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
