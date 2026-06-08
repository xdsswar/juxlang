//! IDE-intelligence helpers over the workspace symbol table.
//!
//! This module is the "render and resolve over KNOWN symbols" layer that
//! powers three editor features (§L.5):
//!
//! - **Hover signatures** — given an identifier under the cursor, find its
//!   declaration in the [`SymbolTable`] (a class/interface/enum/record name, a
//!   free function, or a member reached through a receiver's inferred type) and
//!   render its declaration signature in Jux syntax plus a one-line doc comment.
//! - **Receiver-aware member completion** — given a receiver expression's
//!   inferred [`Ty::User`], list that type's methods and fields (walking the
//!   `extends` / `implements` chain) so `obj.` offers exactly that type's API.
//! - **Auto-import** — map a bare type name to the package that declares it so
//!   an `import a.b.C;` edit can be synthesized.
//!
//! Nothing here re-runs the front end: it reads the already-computed
//! [`SymbolTable`] and the per-expression [`Ty`] map. Cross-file / stdlib
//! resolution stays in the compiler; we only surface what it produced.

// Syntactic AST pieces exposed in the public fields of the `*Sig` types.
use juxc_ast::{ArrayShape, GenericArg, ReturnType, TypeParam, TypeRef, Visibility, WildcardBound};
// Semantic declaration signatures, produced by the symbol-table build pass.
use juxc_tycheck::symbol_table::{
    ClassSig, EnumSig, FieldSig, FunctionSig, InterfaceSig, MethodSig, ParamSig, RecordSig,
};
use juxc_tycheck::{SymbolTable, Ty};

/// A resolved symbol whose declaration signature we can render for hover.
///
/// Each variant borrows the matching `*Sig` out of the [`SymbolTable`] plus the
/// bare name to print. Members (methods/fields) also carry the owning type's
/// bare name so the rendered signature can read like a real declaration.
pub enum Resolved<'a> {
    /// A class type name → `public class Name`.
    Class(String, &'a ClassSig),
    /// An interface type name → `public interface Name`.
    Interface(String, &'a InterfaceSig),
    /// An enum type name → `public enum Name`.
    Enum(String, &'a EnumSig),
    /// A record type name → `public record Name(...)`.
    Record(String, &'a RecordSig),
    /// A free (top-level) function.
    Function(String, &'a FunctionSig),
    /// A method, with the member name to render.
    Method(String, &'a MethodSig),
    /// A field, with the member name to render.
    Field(String, &'a FieldSig),
}

impl<'a> Resolved<'a> {
    /// Render this declaration's signature in Jux syntax — the text shown in
    /// the hover popup's code block.
    pub fn signature(&self) -> String {
        match self {
            Resolved::Class(name, sig) => render_class_header(name, sig),
            Resolved::Interface(name, sig) => {
                format!("{}interface {name}", vis_prefix(sig.visibility))
            }
            Resolved::Enum(name, sig) => format!("{}enum {name}", vis_prefix(sig.visibility)),
            Resolved::Record(name, sig) => render_record_header(name, sig),
            Resolved::Function(name, sig) => render_function(name, sig),
            Resolved::Method(name, sig) => render_method(name, sig),
            Resolved::Field(name, sig) => render_field(name, sig),
        }
    }
}

/// Resolve a bare identifier `ident` to a TYPE declaration in `symbols`.
///
/// Tries a direct key hit (the open file's own no-package types are keyed by
/// bare name) then falls back to [`SymbolTable::find_fqn_by_bare`] so stdlib /
/// cross-package types (keyed by FQN) match on their last segment. Returns the
/// first hit across classes, interfaces, enums, and records.
pub fn resolve_type<'a>(symbols: &'a SymbolTable, ident: &str) -> Option<Resolved<'a>> {
    // Direct FQN/bare-key hits first.
    if let Some(sig) = symbols.classes.get(ident) {
        return Some(Resolved::Class(bare_of(ident).to_string(), sig));
    }
    if let Some(sig) = symbols.interfaces.get(ident) {
        return Some(Resolved::Interface(bare_of(ident).to_string(), sig));
    }
    if let Some(sig) = symbols.enums.get(ident) {
        return Some(Resolved::Enum(bare_of(ident).to_string(), sig));
    }
    if let Some(sig) = symbols.records.get(ident) {
        return Some(Resolved::Record(bare_of(ident).to_string(), sig));
    }
    // Bare-name → FQN fallback (stdlib + cross-package types are FQN-keyed).
    let fqn = symbols.find_fqn_by_bare(ident)?;
    let bare = bare_of(&fqn).to_string();
    if let Some(sig) = symbols.classes.get(&fqn) {
        return Some(Resolved::Class(bare, sig));
    }
    if let Some(sig) = symbols.interfaces.get(&fqn) {
        return Some(Resolved::Interface(bare, sig));
    }
    if let Some(sig) = symbols.enums.get(&fqn) {
        return Some(Resolved::Enum(bare, sig));
    }
    if let Some(sig) = symbols.records.get(&fqn) {
        return Some(Resolved::Record(bare, sig));
    }
    None
}

/// Resolve a free function by bare name (direct key then FQN-by-bare).
pub fn resolve_function<'a>(symbols: &'a SymbolTable, ident: &str) -> Option<Resolved<'a>> {
    if let Some(sig) = symbols.functions.get(ident) {
        return Some(Resolved::Function(bare_of(ident).to_string(), sig));
    }
    let fqn = symbols
        .functions
        .keys()
        .find(|k| bare_of(k) == ident)?
        .clone();
    let sig = symbols.functions.get(&fqn)?;
    Some(Resolved::Function(bare_of(&fqn).to_string(), sig))
}

/// Resolve a member (`ident`) on a receiver whose inferred type is `recv`.
///
/// Only `Ty::User` (and `Ty::Nullable<User>`) receivers have a member table.
/// We resolve the user type's FQN, then walk the class `extends` chain (via
/// [`SymbolTable::lookup_method`] / [`SymbolTable::lookup_field`]) or the
/// interface / record / enum member tables. The first method-or-field match
/// wins (methods shadow fields, matching Jux resolution).
pub fn resolve_member<'a>(
    symbols: &'a SymbolTable,
    recv: &Ty,
    ident: &str,
) -> Option<Resolved<'a>> {
    let type_name = user_type_name(recv)?;
    let class_key = class_key_for(symbols, type_name);

    // Class chain: methods then fields (inherited via the extends walk).
    if let Some(key) = &class_key {
        if let Some((m, _owner)) = symbols.lookup_method(key, ident) {
            return Some(Resolved::Method(ident.to_string(), m));
        }
        if let Some((f, _owner)) = symbols.lookup_field(key, ident) {
            return Some(Resolved::Field(ident.to_string(), f));
        }
    }

    // Interface members.
    if let Some((_, iface)) = lookup_by_bare(&symbols.interfaces, type_name) {
        if let Some(m) = iface.methods.get(ident) {
            return Some(Resolved::Method(ident.to_string(), m));
        }
        if let Some(f) = iface.fields.get(ident) {
            return Some(Resolved::Field(ident.to_string(), f));
        }
    }

    // Record methods (components surface through the canonical fields list).
    if let Some((_, rec)) = lookup_by_bare(&symbols.records, type_name) {
        if let Some(m) = rec.methods.get(ident) {
            return Some(Resolved::Method(ident.to_string(), m));
        }
    }
    None
}

/// One completion candidate harvested from a receiver type — a method or field.
pub struct Member {
    /// The member's bare name (what the user types).
    pub name: String,
    /// True for a method (gets a `name()` presentation + `()` insert), false
    /// for a field.
    pub is_method: bool,
    /// Rendered detail (the full signature) shown to the right in the list.
    pub detail: String,
}

/// Collect every method + field reachable on `recv`'s user type, walking the
/// `extends` chain for classes (so inherited members appear) and the direct
/// member tables for interfaces / records. Deduplicated by name (a subclass
/// override shadows the ancestor). Returns an empty vec when `recv` isn't a
/// resolvable user type — callers fall back to the flat name-bag.
pub fn members_of(symbols: &SymbolTable, recv: &Ty) -> Vec<Member> {
    let Some(type_name) = user_type_name(recv) else {
        return Vec::new();
    };
    let mut out: Vec<Member> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let push_method = |name: &str, sig: &MethodSig, seen: &mut std::collections::HashSet<String>, out: &mut Vec<Member>| {
        if seen.insert(name.to_string()) {
            out.push(Member {
                name: name.to_string(),
                is_method: true,
                detail: render_method(name, sig),
            });
        }
    };
    let push_field = |name: &str, sig: &FieldSig, seen: &mut std::collections::HashSet<String>, out: &mut Vec<Member>| {
        if seen.insert(name.to_string()) {
            out.push(Member {
                name: name.to_string(),
                is_method: false,
                detail: render_field(name, sig),
            });
        }
    };

    // Class + ancestor classes.
    if let Some(start) = class_key_for(symbols, type_name) {
        let mut cursor: Option<String> = Some(start);
        let mut depth = 0usize;
        while let Some(key) = cursor {
            if depth > 64 {
                break;
            }
            let Some(class) = symbols.classes.get(&key) else { break };
            for (name, sig) in &class.methods {
                push_method(name, sig, &mut seen, &mut out);
            }
            for (name, sig) in &class.fields {
                push_field(name, sig, &mut seen, &mut out);
            }
            cursor = class.extends_fqn.clone().or_else(|| {
                class
                    .extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.clone()))
            });
            depth += 1;
        }
    }

    // Interface members.
    if let Some((_, iface)) = lookup_by_bare(&symbols.interfaces, type_name) {
        for (name, sig) in &iface.methods {
            push_method(name, sig, &mut seen, &mut out);
        }
        for (name, sig) in &iface.fields {
            push_field(name, sig, &mut seen, &mut out);
        }
    }

    // Record methods + components (components are public fields).
    if let Some((_, rec)) = lookup_by_bare(&symbols.records, type_name) {
        for (name, sig) in &rec.methods {
            push_method(name, sig, &mut seen, &mut out);
        }
        for comp in &rec.components {
            if seen.insert(comp.name.clone()) {
                out.push(Member {
                    name: comp.name.clone(),
                    is_method: false,
                    detail: format!("public {} {}", render_type(&comp.ty), comp.name),
                });
            }
        }
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ============================================================================
// Signature rendering — Jux syntax, IntelliJ-style one-liners
// ============================================================================

/// `public class Name<T> extends Parent implements A, B`
fn render_class_header(name: &str, sig: &ClassSig) -> String {
    let mut s = vis_prefix(sig.visibility);
    if sig.is_abstract {
        s.push_str("abstract ");
    }
    if sig.is_final {
        s.push_str("final ");
    }
    if sig.is_sealed {
        s.push_str("sealed ");
    }
    s.push_str("class ");
    s.push_str(name);
    s.push_str(&render_generics(&sig.generic_params));
    if let Some(ext) = &sig.extends {
        s.push_str(" extends ");
        s.push_str(&render_type(ext));
    }
    if !sig.implements.is_empty() {
        s.push_str(" implements ");
        s.push_str(
            &sig.implements
                .iter()
                .map(render_type)
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    s
}

/// `public record Name(T a, U b)`
fn render_record_header(name: &str, sig: &RecordSig) -> String {
    let mut s = vis_prefix(sig.visibility);
    s.push_str("record ");
    s.push_str(name);
    s.push_str(&render_generics(&sig.generic_params));
    s.push('(');
    s.push_str(
        &sig.components
            .iter()
            .map(|c| format!("{} {}", render_type(&c.ty), c.name))
            .collect::<Vec<_>>()
            .join(", "),
    );
    s.push(')');
    s
}

/// `public T name(P a, Q b)` for a free function.
fn render_function(name: &str, sig: &FunctionSig) -> String {
    let mut s = vis_prefix(sig.visibility);
    s.push_str(&render_generics(&sig.generic_params));
    if !sig.generic_params.is_empty() {
        s.push(' ');
    }
    s.push_str(&render_return(&sig.return_type));
    s.push(' ');
    s.push_str(name);
    s.push_str(&render_params(&sig.params));
    s
}

/// `public static T name(P a) throws E` for a method.
fn render_method(name: &str, sig: &MethodSig) -> String {
    let mut s = vis_prefix(sig.visibility);
    if sig.is_static {
        s.push_str("static ");
    }
    if sig.is_abstract {
        s.push_str("abstract ");
    }
    if sig.is_final {
        s.push_str("final ");
    }
    if !sig.generic_params.is_empty() {
        s.push_str(&render_generics(&sig.generic_params));
        s.push(' ');
    }
    s.push_str(&render_return(&sig.return_type));
    s.push(' ');
    s.push_str(name);
    s.push_str(&render_params(&sig.params));
    s
}

/// `public static final T name`
fn render_field(name: &str, sig: &FieldSig) -> String {
    let mut s = vis_prefix(sig.visibility);
    if sig.is_static {
        s.push_str("static ");
    }
    if sig.is_final {
        s.push_str("final ");
    }
    s.push_str(&render_type(&sig.ty));
    s.push(' ');
    s.push_str(name);
    s
}

/// `(T a, U b)` — formal parameter list.
fn render_params(params: &[ParamSig]) -> String {
    let inner = params
        .iter()
        .map(|p| format!("{} {}", render_type(&p.ty), p.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!("({inner})")
}

/// `<T, U extends Bound>` — generic parameter list, or empty when none.
fn render_generics(params: &[TypeParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let inner = params
        .iter()
        .map(render_type_param)
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{inner}>")
}

/// Render one generic parameter — `T` or `T extends Bound`.
fn render_type_param(p: &TypeParam) -> String {
    if p.bounds.is_empty() {
        p.name.text.clone()
    } else {
        let bounds = p
            .bounds
            .iter()
            .map(render_type)
            .collect::<Vec<_>>()
            .join(" & ");
        format!("{} extends {bounds}", p.name.text)
    }
}

/// Render a [`ReturnType`] as written: `void`, the type, or `async T`.
fn render_return(rt: &ReturnType) -> String {
    match rt {
        ReturnType::Void => "void".to_string(),
        ReturnType::Type(t) => render_type(t),
        ReturnType::AsyncType(t) => format!("async {}", render_type(t)),
    }
}

/// Render a [`TypeRef`] back to its Jux source spelling: `List<String>?`,
/// `int[]`, `Map<K, V>`. Function-shaped types render as `(A) -> R`.
pub fn render_type(t: &TypeRef) -> String {
    if let Some(fns) = &t.fn_shape {
        let params = fns
            .params
            .iter()
            .map(render_type)
            .collect::<Vec<_>>()
            .join(", ");
        let asy = if fns.is_async { " async" } else { "" };
        let mut s = format!("({params}){asy} -> {}", render_type(&fns.return_type));
        if t.nullable {
            s.push('?');
        }
        return s;
    }
    let mut s: String = t
        .name
        .segments
        .iter()
        .map(|seg| seg.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    if !t.generic_args.is_empty() {
        let args = t
            .generic_args
            .iter()
            .map(render_generic_arg)
            .collect::<Vec<_>>()
            .join(", ");
        s.push('<');
        s.push_str(&args);
        s.push('>');
    }
    if let Some(shape) = &t.array_shape {
        match shape {
            ArrayShape::Fixed(_) => s.push_str("[N]"),
            ArrayShape::Dynamic => s.push_str("[]"),
        }
    }
    if t.nullable {
        s.push('?');
    }
    s
}

/// Render one generic argument — a concrete type or a bounded wildcard.
fn render_generic_arg(g: &GenericArg) -> String {
    match g {
        GenericArg::Type(t) => render_type(t),
        GenericArg::Wildcard(w) => match &w.bound {
            None => "?".to_string(),
            Some(WildcardBound::Extends(t)) => format!("? extends {}", render_type(t)),
            Some(WildcardBound::Super(t)) => format!("? super {}", render_type(t)),
        },
    }
}

/// Visibility keyword + trailing space, or empty for package-private.
fn vis_prefix(v: Visibility) -> String {
    match v {
        Visibility::Public => "public ".to_string(),
        Visibility::Internal => "internal ".to_string(),
        Visibility::Protected => "protected ".to_string(),
        Visibility::Private => "private ".to_string(),
        Visibility::Package => String::new(),
    }
}

// ============================================================================
// Small symbol-table helpers
// ============================================================================

/// Bare (last-segment) name of an FQN string.
fn bare_of(fqn: &str) -> &str {
    fqn.rsplit('.').next().unwrap_or(fqn)
}

/// The bare type name of a `Ty::User`, peeling a single `Nullable` wrapper.
/// Returns `None` for primitives / arrays / unresolved types.
fn user_type_name(ty: &Ty) -> Option<&str> {
    match ty {
        Ty::User { name, .. } => Some(bare_of(name)),
        Ty::Nullable(inner) => user_type_name(inner),
        _ => None,
    }
}

/// Resolve `type_name` (bare or FQN) to the key under which its class is stored,
/// so chain-walking lookups can key directly into `symbols.classes`.
fn class_key_for(symbols: &SymbolTable, type_name: &str) -> Option<String> {
    if symbols.classes.contains_key(type_name) {
        return Some(type_name.to_string());
    }
    symbols
        .classes
        .keys()
        .find(|k| bare_of(k) == type_name)
        .cloned()
}

/// Look up a value in an FQN-keyed map by bare name (direct hit then
/// last-segment scan). Returns the matched key and value.
fn lookup_by_bare<'a, V>(
    map: &'a std::collections::HashMap<String, V>,
    bare: &str,
) -> Option<(&'a String, &'a V)> {
    if let Some((k, v)) = map.get_key_value(bare) {
        return Some((k, v));
    }
    map.iter().find(|(k, _)| bare_of(k) == bare)
}
