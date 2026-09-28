//! Declaring the hidden stand-ins for private members reached through a
//! dispatch value (ERRATA E139; the plan is
//! `juxc_tycheck::private_dispatch`).
//!
//! For a field, a property over it: `T __jux_priv_C_f { get { return
//! this.f; } set { this.f = value; } }` (no setter for a `final` field). For
//! a method, a method of the same signature that calls it. Each is written as
//! Jux text and parsed under a source index of its own, as an anonymous
//! class's skeleton is, with the member's own types put in afterwards.

use juxc_ast::{ClassDecl, CompilationUnit, ReturnType, TopLevelDecl, TypeRef};
use juxc_tycheck::private_dispatch::PrivateAccess;

/// The first source index handed to a stand-in; past the anonymous-class
/// skeletons' range.
const STAND_IN_INDEX_BASE: u32 = 0x5000_0000;

/// The placeholder a stand-in's text writes for the member's type.
const TYPE_PLACEHOLDER: &str = "__JuxPrivT";

/// Declare the stand-ins `accesses` need. Returns whether any was declared.
pub(crate) fn apply(units: &mut [CompilationUnit], accesses: &[PrivateAccess]) -> bool {
    let mut any = false;
    for (k, access) in accesses.iter().enumerate() {
        let index = STAND_IN_INDEX_BASE + k as u32;
        for unit in units.iter_mut().filter(|u| !u.is_external) {
            let pkg = unit
                .package
                .as_ref()
                .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
                .unwrap_or_default();
            for item in &mut unit.items {
                let TopLevelDecl::Class(cd) = item else { continue };
                let fqn = if pkg.is_empty() { cd.name.text.clone() } else { format!("{pkg}.{}", cd.name.text) };
                if let Some(target) = find_class(cd, &fqn, &access.class) {
                    any |= declare(target, access, index);
                }
            }
        }
    }
    any
}

fn find_class<'a>(cd: &'a mut ClassDecl, fqn: &str, want: &str) -> Option<&'a mut ClassDecl> {
    if fqn == want {
        return Some(cd);
    }
    for nested in &mut cd.nested_types {
        if let TopLevelDecl::Class(inner) = nested {
            let inner_fqn = format!("{fqn}__{}", inner.name.text);
            if let Some(found) = find_class(inner, &inner_fqn, want) {
                return Some(found);
            }
        }
    }
    None
}

/// Declare `access`'s stand-in on `class`.
fn declare(class: &mut ClassDecl, access: &PrivateAccess, index: u32) -> bool {
    let hidden = access.hidden_name();
    if class.methods.iter().any(|m| m.name.text == hidden) || class.properties.iter().any(|p| p.name.text == hidden) {
        return false;
    }
    let member = &access.member;
    if !access.is_method {
        let Some(field) = class.fields.iter().find(|f| f.name.text == *member && !f.is_static) else { return false };
        let Some(ty) = field.ty.clone() else { return false };
        let setter = if field.is_final { "" } else { " set { this.MEMBER = value; }" };
        let text = format!(
            "class __JuxPriv {{ public {TYPE_PLACEHOLDER} {hidden} {{ get {{ return this.MEMBER; }}{setter} }} }}\n"
        )
        .replace("MEMBER", member);
        let Some(mut parsed) = parse_class(&text, index) else { return false };
        for p in &mut parsed.properties {
            patch(&mut p.ty, &ty);
        }
        for m in &mut parsed.methods {
            patch_fn(m, &ty);
        }
        class.properties.extend(parsed.properties);
        class.methods.extend(parsed.methods);
        class.fields.extend(parsed.fields);
        return true;
    }
    let Some(original) = class
        .methods
        .iter()
        .find(|m| m.name.text == *member && !m.modifiers.contains(&juxc_ast::FnModifier::Static))
        .cloned()
    else {
        return false;
    };
    let args = original.params.iter().map(|p| p.name.text.clone()).collect::<Vec<_>>().join(", ");
    let body = match &original.return_type {
        ReturnType::Void => format!("this.{member}({args});"),
        ReturnType::Type(_) => format!("return this.{member}({args});"),
        ReturnType::AsyncType(_) => format!("return await this.{member}({args});"),
    };
    let text = format!("class __JuxPriv {{ void __jux_body() {{ {body} }} }}\n");
    let Some(parsed) = parse_class(&text, index) else { return false };
    let Some(stand_in_body) = parsed.methods.into_iter().next().and_then(|m| m.body) else { return false };
    let mut stand_in = original;
    stand_in.name.text = hidden;
    stand_in.visibility = juxc_ast::Visibility::Public;
    stand_in.annotations.clear();
    stand_in.body = Some(stand_in_body);
    class.methods.push(stand_in);
    true
}

fn parse_class(text: &str, index: u32) -> Option<ClassDecl> {
    let mut file = juxc_source::SourceFile::new("<private stand-in>".to_string(), text.to_string());
    file.set_index(index);
    let lexed = juxc_lex::lex(&file);
    let parsed = juxc_parse::parse(&lexed.tokens);
    if parsed.diagnostics.iter().any(|d| matches!(d.severity, juxc_diagnostics::Severity::Error)) {
        return None;
    }
    parsed.ast.items.into_iter().find_map(|item| match item {
        TopLevelDecl::Class(c) => Some(c),
        _ => None,
    })
}

/// Put the member's own type where the text wrote the placeholder.
fn patch(slot: &mut TypeRef, ty: &TypeRef) {
    if slot.name.segments.len() == 1 && slot.name.segments[0].text == TYPE_PLACEHOLDER {
        *slot = ty.clone();
    }
}

fn patch_fn(m: &mut juxc_ast::FnDecl, ty: &TypeRef) {
    if let ReturnType::Type(t) | ReturnType::AsyncType(t) = &mut m.return_type {
        patch(t, ty);
    }
    for p in &mut m.params {
        patch(&mut p.ty, ty);
    }
}
