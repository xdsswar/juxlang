//! `Result.from(() -> ...)` takes its error type from where it is written
//! (EXCEPTIONS §X.5.4).
//!
//! `Result.from` is a compiler intrinsic (`Checker::check_result_from`, the
//! backend's `emit_result_from`), not a static method of the library enum:
//! a generic static there would give every `Result` call in every program one
//! more candidate to infer against, which is the risk gap 35 named when it
//! left the form out. The intrinsic's type is local to the call:
//! `Result<T, E>`, `T` what the function produces and `E` its one explicit
//! type argument, or `Exception` when there is none
//! (`infer::result_from_type`).
//!
//! What makes that pleasant to write is this pass. Before anything is
//! inferred, a `Result.from(...)` with no type argument, written where the
//! program has already SPELLED the `Result` type it expects, is given that
//! type's error argument:
//!
//! - `return Result.from(...)` in a function or method whose written return
//!   type is `Result<T, E>` (an `async` one included);
//! - `Result<T, E> r = Result.from(...)`, a local or a field with an
//!   initializer.
//!
//! Anywhere else (an argument, a `var`, an operand) the call keeps its own
//! default, `Exception`, or the type argument the programmer wrote:
//! `Result.from<ConfigError>(() -> ...)`. Nothing here looks inside a lambda
//! body: a `return` there returns from the lambda.
//!
//! The pass is syntactic, so it needs no expected type to flow through
//! inference, and a mismatch it cannot see is an ordinary `E0410` at the
//! call.

use juxc_ast::{Block, CompilationUnit, ElseBranch, Expr, FnDecl, GenericArg, ReturnType, Stmt, TopLevelDecl, TypeRef};
use juxc_source::Span;

/// Fill in the error type of every `Result.from(...)` written against a
/// spelled `Result<T, E>` (see the module docs).
pub fn fill_error_types(units: &mut [CompilationUnit]) {
    for unit in units.iter_mut().filter(|u| !u.is_external) {
        for item in &mut unit.items {
            top_level(item);
        }
    }
}

/// `E` of a written `Result<T, E>`, with its spans cleared: the copy is the
/// call's type argument, and it must not be a second occurrence of the
/// return type's own tokens for anything that indexes names by span.
fn error_arg(ty: &TypeRef) -> Option<TypeRef> {
    let last = ty.name.segments.last()?;
    if last.text != "Result" || ty.nullable || ty.array_shape.is_some() || ty.fn_shape.is_some() || ty.ptr_depth > 0 {
        return None;
    }
    match ty.generic_args.as_slice() {
        [_, GenericArg::Type(e)] => Some(despan(e.clone())),
        _ => None,
    }
}

fn despan(mut ty: TypeRef) -> TypeRef {
    ty.span = Span::DUMMY;
    ty.name.span = Span::DUMMY;
    for s in &mut ty.name.segments {
        s.span = Span::DUMMY;
    }
    for a in &mut ty.generic_args {
        if let GenericArg::Type(t) = a {
            *t = despan(t.clone());
        }
    }
    ty
}

/// Give `e` the error type `error` when it is a `Result.from(...)` without
/// type arguments.
fn fill(e: &mut Expr, error: Option<&TypeRef>) {
    let Some(error) = error else { return };
    if let Expr::Call(c) = e {
        if c.explicit_generic_args.is_empty() && crate::infer::is_result_from_shape(c) {
            c.explicit_generic_args.push(error.clone());
        }
    }
}

fn top_level(item: &mut TopLevelDecl) {
    match item {
        TopLevelDecl::Function(f) => function(f),
        TopLevelDecl::Class(c) => {
            for nested in &mut c.nested_types {
                top_level(nested);
            }
            for field in &mut c.fields {
                let error = field.ty.as_ref().and_then(error_arg);
                if let Some(init) = &mut field.default {
                    fill(init, error.as_ref());
                }
            }
            for ctor in &mut c.constructors {
                block(&mut ctor.body, None);
            }
            for m in &mut c.methods {
                function(m);
            }
            for b in c.init_blocks.iter_mut().chain(c.static_init_blocks.iter_mut()) {
                block(b, None);
            }
        }
        TopLevelDecl::Enum(e) => {
            for m in &mut e.methods {
                function(m);
            }
        }
        TopLevelDecl::Record(r) => {
            for m in &mut r.methods {
                function(m);
            }
        }
        TopLevelDecl::Interface(i) => {
            for m in &mut i.methods {
                function(m);
            }
        }
        _ => {}
    }
}

fn function(f: &mut FnDecl) {
    let error = match &f.return_type {
        ReturnType::Type(t) | ReturnType::AsyncType(t) => error_arg(t),
        ReturnType::Void => None,
    };
    if let Some(body) = &mut f.body {
        block(body, error.as_ref());
    }
}

/// `ret` is the error type the enclosing function's `Result` return names.
fn block(b: &mut Block, ret: Option<&TypeRef>) {
    for s in &mut b.statements {
        stmt(s, ret);
    }
}

fn stmt(s: &mut Stmt, ret: Option<&TypeRef>) {
    match s {
        Stmt::Return(Some(e), _) => fill(e, ret),
        Stmt::VarDecl(v) => {
            let error = v.ty.as_ref().and_then(error_arg);
            if let Some(init) = &mut v.init {
                fill(init, error.as_ref());
            }
        }
        Stmt::If(i) => if_stmt(i, ret),
        Stmt::While(w) => block(&mut w.body, ret),
        Stmt::DoWhile(d) => block(&mut d.body, ret),
        Stmt::ForEach(f) => block(&mut f.body, ret),
        Stmt::ForC(f) => block(&mut f.body, ret),
        Stmt::Labeled { stmt: inner, .. } => stmt(inner, ret),
        Stmt::Try(t) => {
            block(&mut t.body, ret);
            for c in &mut t.catches {
                block(&mut c.body, ret);
            }
            if let Some(f) = &mut t.finally {
                block(f, ret);
            }
        }
        Stmt::Block(b) | Stmt::Unsafe(b) => block(b, ret),
        _ => {}
    }
}

fn if_stmt(i: &mut juxc_ast::IfStmt, ret: Option<&TypeRef>) {
    block(&mut i.then_block, ret);
    if let Some(else_branch) = &mut i.else_branch {
        match &mut **else_branch {
            ElseBranch::If(elif) => if_stmt(elif, ret),
            ElseBranch::Block(b) => block(b, ret),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> CompilationUnit {
        let lexed = juxc_lex::lex(&juxc_source::SourceFile::new(std::path::PathBuf::from("t.jux"), src.to_string()));
        juxc_parse::parse(&lexed.tokens).ast
    }

    /// The error type argument each `Result.from` call in `unit` carries, in
    /// source order.
    fn filled(unit: &CompilationUnit) -> Vec<Option<String>> {
        let mut out = Vec::new();
        for item in &unit.items {
            if let TopLevelDecl::Function(f) = item {
                if let Some(body) = &f.body {
                    juxc_ast::visit::for_each_expr(body, &mut |e| {
                        if let Expr::Call(c) = e {
                            if crate::infer::is_result_from_shape(c) {
                                out.push(c.explicit_generic_args.first().map(|t| t.name.segments[0].text.clone()));
                            }
                        }
                    });
                }
            }
        }
        out
    }

    /// A `return` against a written `Result<T, E>` and a typed local take `E`;
    /// a `var`, a written type argument, a lambda's own `return` and a
    /// function with another return type are left alone.
    #[test]
    fn a_spelled_result_type_gives_its_error_type() {
        let mut unit = parse(
            "Result<int, ConfigError> a() { return Result.from(() -> 1); }\n\
             Result<int, ConfigError> b() { if (true) { Result<int, IoError> r = Result.from(() -> 2); } return Result.from<Other>(() -> 3); }\n\
             void c() { var r = Result.from(() -> 4); }\n\
             int d() { return Result.from(() -> 5).unwrap(); }\n",
        );
        fill_error_types(std::slice::from_mut(&mut unit));
        assert_eq!(
            filled(&unit),
            vec![
                Some("ConfigError".to_string()),
                Some("IoError".to_string()),
                Some("Other".to_string()),
                None,
                None,
            ]
        );
    }
}
