//! rustdoc JSON → stub IR — JUX-BINDGEN-ADDENDUM.md §G.3 (type mapping) and
//! §G.6 (Rust crate bindings).
//!
//! This is the only module coupled to the `rustdoc-types` schema. It walks a
//! [`Crate`] and builds the language-agnostic [`StubFile`] that `emit` renders.
//! The §G.3 Rust→Jux type table lives in [`map_type`].

use std::collections::HashSet;

use rustdoc_types::{
    Crate, Enum, Function, GenericArgs, GenericArg, GenericBound, GenericParamDefKind, Generics,
    Item, ItemEnum, Path, Struct, StructKind, Type, VariantKind, Visibility,
};

use crate::model::{
    StubConst, StubCtor, StubField, StubFile, StubFn, StubItem, StubParam, StubType, StubVariant,
    TypeKind, Vis,
};
use crate::naming::{escape_keyword, method_name, snake_to_camel};
use crate::ty::JuxType;

/// Parse a rustdoc-JSON string and generate stubs for `package`.
pub fn generate_from_json(json: &str, package: &str) -> Result<StubFile, serde_json::Error> {
    let krate: Crate = serde_json::from_str(json)?;
    Ok(generate(&krate, package))
}

/// Ingest several rustdoc-JSON crates into a single, deduplicated [`StubFile`]
/// under one `package`.
///
/// This is how Rust's layered standard library (`core` ⊂ `alloc` ⊂ `std`) is
/// surfaced as one Jux package: the bulk of the prelude (`Vec`, `String`,
/// `Box`, `Rc`/`Arc`, `BTreeMap`…) is *defined* in `alloc`/`core` and merely
/// re-exported by `std`, so ingesting `std` alone misses them (their defining
/// items carry a non-zero `crate_id` and are skipped by [`generate`]). Feeding
/// each crate's own JSON in turn — where each is the *local* crate
/// (`crate_id == 0`) — captures every definition.
///
/// Items are keyed by name and the **first** occurrence wins: pass crates in
/// `core, alloc, std` order so the most fundamental definition is the one
/// surfaced. Deduplication also collapses the platform-duplicated names Rust
/// ships (e.g. the several `ChildExt` traits under `std::os::*::process`) that
/// would otherwise collide as duplicate Jux declarations (E0400) once merged
/// into a single package.
pub fn generate_merged(jsons: &[(&str, &str)], package: &str) -> Result<StubFile, serde_json::Error> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut collected: Vec<(String, StubItem)> = Vec::new();
    let mut format_version = 0;

    for (_crate_name, json) in jsons {
        let krate: Crate = serde_json::from_str(json)?;
        format_version = krate.format_version;
        for (name, item) in collect_items(&krate) {
            // First definition wins (crates passed core→alloc→std), and
            // platform-duplicated names are collapsed.
            if seen.insert(name.clone()) {
                collected.push((name, item));
            }
        }
    }

    collected.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(StubFile {
        package: package.to_string(),
        header: vec![format!(
            "bindgen — generated from {} rustdoc JSON crate(s) (format_version {})",
            jsons.len(),
            format_version
        )],
        items: collected.into_iter().map(|(_, it)| it).collect(),
    })
}

/// Build a [`StubFile`] from an already-parsed rustdoc [`Crate`].
///
/// Only public items of the local crate (`crate_id == 0`) are emitted, in a
/// deterministic (name-sorted) order so a stub regenerates identically.
pub fn generate(krate: &Crate, package: &str) -> StubFile {
    let mut collected = collect_items(krate);

    // Deterministic order: by item name.
    collected.sort_by(|a, b| a.0.cmp(&b.0));

    StubFile {
        package: package.to_string(),
        header: vec![format!("bindgen — generated from rustdoc JSON (format_version {})", krate.format_version)],
        items: collected.into_iter().map(|(_, it)| it).collect(),
    }
}

/// Walk a single crate's index and collect every public, local
/// (`crate_id == 0`) item as a `(name, StubItem)` pair, **unsorted**. Shared by
/// [`generate`] (single crate) and [`generate_merged`] (cross-crate dedup) so
/// the item-selection rules live in exactly one place.
fn collect_items(krate: &Crate) -> Vec<(String, StubItem)> {
    // Every id that is a member of some impl or trait — used to tell a free
    // function (top-level `fn`) apart from a method/associated function.
    let member_ids = collect_member_ids(krate);

    let mut collected: Vec<(String, StubItem)> = Vec::new();

    for item in krate.index.values() {
        if item.crate_id != 0 {
            continue; // skip external items referenced locally
        }
        let Some(name) = &item.name else { continue };

        match &item.inner {
            ItemEnum::Struct(s) if is_public(&item.visibility) => {
                collected.push((name.clone(), StubItem::Type(build_struct(krate, name, s, item))));
            }
            ItemEnum::Enum(e) if is_public(&item.visibility) => {
                collected.push((name.clone(), StubItem::Type(build_enum(krate, name, e, item))));
            }
            ItemEnum::Trait(t) if is_public(&item.visibility) => {
                collected.push((name.clone(), StubItem::Type(build_trait(krate, name, &t.generics, &t.items, item))));
            }
            ItemEnum::Function(f) if is_public(&item.visibility) && !member_ids.contains(&item.id.0) => {
                // Free function (§G.5.5). Record its real Rust path so the
                // backend can alias the snake_case Rust name to the camelCase
                // Jux stub name on import.
                let mut sf = map_function(name, f);
                sf.is_static = false;
                sf.rust_path = real_rust_path(krate, item);
                collected.push((name.clone(), StubItem::Function(sf)));
            }
            ItemEnum::Constant { type_, const_: _ } if is_public(&item.visibility) => {
                collected.push((
                    name.clone(),
                    StubItem::Const(StubConst {
                        name: escape_keyword(name),
                        ty: map_type(type_),
                        // The rustdoc value/expr is a *Rust* expression
                        // (`crate::sys::path::SEPARATORS`, `'\\'`, a const fn
                        // call, …) that has no valid Jux spelling. A stub const is
                        // signature-only and never lowered (§G.9), so its
                        // initializer carries no information — elide it to a
                        // bodyless `const T NAME;` rather than emit unparseable
                        // text.
                        value: None,
                    }),
                ));
            }
            ItemEnum::Static(s) if is_public(&item.visibility) => {
                collected.push((
                    name.clone(),
                    StubItem::Const(StubConst {
                        name: escape_keyword(name),
                        ty: map_type(&s.type_),
                        // See the `Constant` arm: the Rust initializer has no Jux
                        // spelling and a stub never lowers it.
                        value: None,
                    }),
                ));
            }
            _ => {}
        }
    }

    collected
}

/// Collect every id referenced as a member of an impl block or a trait, so the
/// driver can exclude those functions from the free-function set.
fn collect_member_ids(krate: &Crate) -> HashSet<u32> {
    let mut ids = HashSet::new();
    for item in krate.index.values() {
        match &item.inner {
            ItemEnum::Impl(im) => ids.extend(im.items.iter().map(|id| id.0)),
            ItemEnum::Trait(t) => ids.extend(t.items.iter().map(|id| id.0)),
            _ => {}
        }
    }
    ids
}

// ============================================================================
// Type-declaration builders (§G.6.3)
// ============================================================================

fn build_struct(krate: &Crate, name: &str, s: &Struct, item: &Item) -> StubType {
    let mut fields = Vec::new();
    let mut all_public = true;

    match &s.kind {
        StructKind::Plain { fields: fids, has_stripped_fields } => {
            if *has_stripped_fields {
                all_public = false;
            }
            for fid in fids {
                let Some(fitem) = krate.index.get(fid) else { continue };
                let ItemEnum::StructField(ty) = &fitem.inner else { continue };
                if !is_public(&fitem.visibility) {
                    all_public = false;
                    continue;
                }
                if let Some(fname) = &fitem.name {
                    fields.push(StubField {
                        visibility: Vis::Public,
                        name: method_name(fname),
                        ty: map_type(ty),
                    });
                }
            }
        }
        // Tuple/unit structs carry no named fields we can surface; treat as a
        // class shell whose constructors come from inherent impls.
        StructKind::Tuple(_) | StructKind::Unit => all_public = false,
    }

    let (ctors, mut methods) = collect_inherent_members(krate, &s.impls, name);
    dedup_methods_by_name(&mut methods);

    // §G.6.3 kind selection: an all-public plain-fielded struct with no methods
    // maps to a Jux `struct`; anything with private fields or behaviour is a
    // `class`.
    let kind = if all_public && !fields.is_empty() && methods.is_empty() && ctors.is_empty() {
        TypeKind::Struct
    } else {
        TypeKind::Class
    };

    let mut st = StubType::new(kind, name);
    st.generics = generic_param_names(&s.generics);
    st.fields = fields;
    st.constructors = ctors;
    st.methods = methods;
    st.doc = first_doc_line(item);
    st.rust_path = real_rust_path(krate, item);
    st.index_ref = has_ref_index_impl(krate, &s.impls);
    st
}

fn build_enum(krate: &Crate, name: &str, e: &Enum, item: &Item) -> StubType {
    let mut st = StubType::new(TypeKind::Enum, name);
    st.generics = generic_param_names(&e.generics);
    st.doc = first_doc_line(item);
    st.rust_path = real_rust_path(krate, item);

    for vid in &e.variants {
        let Some(vitem) = krate.index.get(vid) else { continue };
        let ItemEnum::Variant(v) = &vitem.inner else { continue };
        let Some(vname) = &vitem.name else { continue };

        let payload = match &v.kind {
            VariantKind::Plain => Vec::new(),
            VariantKind::Tuple(fids) => fids
                .iter()
                .filter_map(|opt| {
                    let fitem = krate.index.get(opt.as_ref()?)?;
                    match &fitem.inner {
                        ItemEnum::StructField(ty) => Some(map_type(ty)),
                        _ => None,
                    }
                })
                .collect(),
            // Struct-like variant payloads aren't represented in Pattern C yet.
            VariantKind::Struct { .. } => Vec::new(),
        };
        let discriminant = v
            .discriminant
            .as_ref()
            .and_then(|d| d.value.parse::<i64>().ok());

        st.variants.push(StubVariant { name: vname.clone(), payload, discriminant });
    }
    st
}

fn build_trait(
    krate: &Crate,
    name: &str,
    generics: &Generics,
    item_ids: &[rustdoc_types::Id],
    item: &Item,
) -> StubType {
    // A Rust trait becomes a Jux interface; provided methods (with a body)
    // become `default` methods (§G.6.4).
    let mut st = StubType::new(TypeKind::Interface, name);
    st.generics = generic_param_names(generics);
    st.doc = first_doc_line(item);
    st.rust_path = real_rust_path(krate, item);

    for mid in item_ids {
        let Some(mitem) = krate.index.get(mid) else { continue };
        let Some(mname) = &mitem.name else { continue };
        if let ItemEnum::Function(f) = &mitem.inner {
            let mut sf = map_function(mname, f);
            sf.is_static = !has_self_receiver(f);
            sf.is_default = f.has_body;
            st.methods.push(sf);
        }
    }
    dedup_methods_by_name(&mut st.methods);
    st
}

/// Drop methods whose Jux name collides with an earlier one, keeping the first.
///
/// Rust freely overloads a name across inherent `impl` blocks (and a method may
/// appear once per monomorphisable receiver shape — e.g. `MaybeUninit::<T>` and
/// `MaybeUninit::<[T]>` both yielding `assume_init`). Jux has **no** method
/// overloading (one name, one signature: `E0402`), so a faithful surfacing must
/// pick a single representative. First-wins is deterministic because the caller
/// has already ordered the impl members, and keeps the most general inherent
/// definition that rustdoc lists first.
fn dedup_methods_by_name(methods: &mut Vec<StubFn>) {
    let mut seen: HashSet<String> = HashSet::new();
    methods.retain(|m| seen.insert(m.name.clone()));
}

/// Collect constructors and methods from a type's **inherent** impl blocks.
/// `new()` (no receiver) maps to a constructor (§G.5.1); other associated
/// functions without a receiver map to static methods (§G.5.2); functions with
/// a `self` receiver map to instance methods (§G.5.3).
fn collect_inherent_members(
    krate: &Crate,
    impls: &[rustdoc_types::Id],
    type_name: &str,
) -> (Vec<StubCtor>, Vec<StubFn>) {
    let mut ctors = Vec::new();
    let mut methods = Vec::new();

    for impl_id in impls {
        let Some(impl_item) = krate.index.get(impl_id) else { continue };
        let ItemEnum::Impl(im) = &impl_item.inner else { continue };
        if im.trait_.is_some() {
            continue; // only inherent impls contribute the safe wrapper surface
        }
        for mid in &im.items {
            let Some(mitem) = krate.index.get(mid) else { continue };
            if !is_public(&mitem.visibility) {
                continue;
            }
            let Some(mname) = &mitem.name else { continue };
            let ItemEnum::Function(f) = &mitem.inner else { continue };

            let has_self = has_self_receiver(f);
            if mname == "new" && !has_self {
                // A `new() -> Result<Self, E>` surfaces as a `throws E` ctor so
                // the call site unwraps the `Result` (§G.5.4).
                let (_ret, throws) = map_return(&f.sig.output);
                ctors.push(StubCtor {
                    visibility: Vis::Public,
                    name: type_name.to_string(),
                    params: map_params(f),
                    throws,
                });
            } else {
                let mut sf = map_function(mname, f);
                sf.is_static = !has_self;
                methods.push(sf);
            }
        }
    }
    (ctors, methods)
}

// ============================================================================
// Function / parameter mapping (§G.5)
// ============================================================================

fn map_function(name: &str, f: &Function) -> StubFn {
    let (ret, throws) = map_return(&f.sig.output);
    StubFn {
        visibility: Vis::Public,
        is_static: false,
        is_default: false,
        name: method_name(name),
        generics: generic_param_names(&f.generics),
        params: map_params(f),
        ret,
        throws,
        is_unsafe: f.header.is_unsafe,
        is_mut_self: has_mut_self_receiver(f),
        // Set by the free-function call site (which has the rustdoc item); a
        // method leaves this `None` (it's dispatched on its `@rust`-pathed type).
        rust_path: None,
        doc: None,
    }
}

fn map_params(f: &Function) -> Vec<StubParam> {
    f.sig
        .inputs
        .iter()
        .filter(|(n, _)| n != "self")
        .map(|(n, ty)| StubParam {
            name: param_name(n),
            ty: map_type(ty),
            by_ref: is_borrow_param(ty),
        })
        .collect()
}

/// True when the Rust parameter is a borrow that maps to a by-value Jux type but
/// must be passed with a call-site `&` (§G.9.2). A borrowed **slice** (`&[T]`)
/// is excluded — it maps to a Jux array and is lowered through the array path,
/// not as a single `&arg`.
fn is_borrow_param(ty: &Type) -> bool {
    matches!(ty, Type::BorrowedRef { type_, .. } if !matches!(type_.as_ref(), Type::Slice(_)))
}

/// `Result<T, E>` in return position becomes `T throws E` (§G.5.4); `Option<T>`
/// and everything else map through [`map_type`].
fn map_return(output: &Option<Type>) -> (JuxType, Option<JuxType>) {
    match output {
        None => (JuxType::Void, None),
        Some(Type::ResolvedPath(p)) if last_segment(&p.path) == "Result" => {
            let args = collect_type_args(&p.args);
            let ok = args.first().cloned().unwrap_or(JuxType::Void);
            // A 2-arg `Result<T, E>` carries the real error type. A 1-arg crate
            // alias `Result<T>` (= `Result<T, CrateError>`, e.g. `minifb::Result`)
            // hides it but is still fallible, so record an opaque `Error` — the
            // call site unwraps either way (the backend ignores the error type;
            // only its presence drives the `throws` / unwrap, §G.5.4).
            let err = args.get(1).cloned().or_else(|| Some(JuxType::user("Error")));
            (ok, err)
        }
        Some(t) => (map_type(t), None),
    }
}

fn param_name(n: &str) -> String {
    if n.is_empty() || !n.chars().all(|c| c.is_alphanumeric() || c == '_') {
        "arg".to_string()
    } else {
        escape_keyword(&snake_to_camel(n))
    }
}

fn has_self_receiver(f: &Function) -> bool {
    f.sig.inputs.iter().any(|(n, _)| n == "self")
}

/// Does this type implement `Index<&K>` — map-style indexing with a
/// BORROWED key (`HashMap`/`BTreeMap`)? DISCOVERED from the type's
/// real `Index` trait impls in the rustdoc JSON, so the Jux `xs[k]`
/// lowering (`xs[&(k)]` vs the sequence form `xs[(k) as usize]`)
/// tracks the library instead of a name list. Rendered as the
/// `@RustIndexRef` class annotation on the stub.
fn has_ref_index_impl(krate: &Crate, impls: &[rustdoc_types::Id]) -> bool {
    impls.iter().any(|id| {
        let Some(item) = krate.index.get(id) else { return false };
        let ItemEnum::Impl(im) = &item.inner else { return false };
        let Some(tr) = &im.trait_ else { return false };
        if last_segment(&tr.path) != "Index" {
            return false;
        }
        // `Index<Idx>` — map-style impls take `Idx = &K`/`&Q`.
        matches!(
            tr.args.as_deref(),
            Some(GenericArgs::AngleBracketed { args, .. })
                if matches!(
                    args.first(),
                    Some(GenericArg::Type(Type::BorrowedRef { .. }))
                )
        )
    })
}

/// True when the function's receiver is `&mut self` — the method mutates
/// the value it is called on. (A by-value `self` consumes rather than
/// mutates and is not flagged.)
fn has_mut_self_receiver(f: &Function) -> bool {
    f.sig.inputs.iter().any(|(n, t)| {
        n == "self" && matches!(t, Type::BorrowedRef { is_mutable: true, .. })
    })
}

fn generic_param_names(g: &Generics) -> Vec<String> {
    g.params
        .iter()
        .filter_map(|p| match &p.kind {
            // Type params only; lifetimes and consts don't appear in Jux
            // generic lists. Skip synthetic `impl Trait` desugarings.
            GenericParamDefKind::Type { .. } if !p.name.starts_with("impl ") => Some(p.name.clone()),
            _ => None,
        })
        .collect()
}

// ============================================================================
// Type mapping — the §G.3 Rust→Jux table
// ============================================================================

/// Map a rustdoc [`Type`] to a [`JuxType`] per §G.3.
pub fn map_type(t: &Type) -> JuxType {
    match t {
        Type::Primitive(p) => map_primitive(p),
        Type::ResolvedPath(path) => map_path(path),
        Type::Generic(name) if name == "Self" => JuxType::user("Self"),
        Type::Generic(name) => JuxType::Param(name.clone()),
        Type::Tuple(ts) => {
            if ts.is_empty() {
                JuxType::Void // `()` in return position
            } else {
                JuxType::Tuple(ts.iter().map(map_type).collect())
            }
        }
        Type::Slice(inner) => JuxType::Array { elem: Box::new(map_type(inner)), size: None },
        Type::Array { type_, len } => {
            JuxType::Array { elem: Box::new(map_type(type_)), size: len.parse::<u64>().ok() }
        }
        // Borrows vanish (§G.3.4); `&[T]` becomes a dynamic array.
        Type::BorrowedRef { type_, .. } => match type_.as_ref() {
            Type::Slice(inner) => JuxType::Array { elem: Box::new(map_type(inner)), size: None },
            other => map_type(other),
        },
        Type::RawPointer { type_, .. } => JuxType::RawPtr(Box::new(map_type(type_))),
        Type::ImplTrait(bounds) => first_trait_in_bounds(bounds)
            .map(JuxType::user)
            .unwrap_or_else(|| JuxType::Unknown("Object".into())),
        Type::DynTrait(dt) => dt
            .traits
            .first()
            .map(|pt| JuxType::user(last_segment(&pt.trait_.path)))
            .unwrap_or_else(|| JuxType::Unknown("Object".into())),
        Type::FunctionPointer(fp) => {
            let params = fp.sig.inputs.iter().map(|(_, t)| map_type(t)).collect();
            let ret = fp.sig.output.as_ref().map(map_type).unwrap_or(JuxType::Void);
            JuxType::Fn { params, ret: Box::new(ret), is_async: false }
        }
        // Pattern types, qualified paths, and inference markers have no Jux
        // spelling in this slice.
        Type::QualifiedPath { name, .. } => JuxType::Unknown(name.clone()),
        Type::Pat { .. } | Type::Infer => JuxType::Unknown("Object".into()),
    }
}

/// Map a Rust primitive name to its Jux equivalent (§G.3.1). Width-explicit
/// forms (`i32`/`u32`) are preserved; platform-sized maps to platform-sized.
fn map_primitive(p: &str) -> JuxType {
    match p {
        "i8" => JuxType::Prim("byte"),
        "i16" => JuxType::Prim("short"),
        "i32" => JuxType::Prim("i32"),
        "i64" => JuxType::Prim("long"),
        "i128" => JuxType::Prim("i128"),
        "isize" => JuxType::Prim("int"),
        "u8" => JuxType::Prim("ubyte"),
        "u16" => JuxType::Prim("ushort"),
        "u32" => JuxType::Prim("u32"),
        "u64" => JuxType::Prim("ulong"),
        "u128" => JuxType::Prim("u128"),
        "usize" => JuxType::Prim("uint"),
        "f32" => JuxType::Prim("float"),
        "f64" => JuxType::Prim("double"),
        "bool" => JuxType::Prim("bool"),
        "char" => JuxType::Prim("char"),
        "str" => JuxType::String,
        "never" | "!" => JuxType::Never,
        other => JuxType::Unknown(other.to_string()),
    }
}

/// Map a named path type, applying the §G.3.1 stdlib substitutions
/// (`Vec`→`List`, `Option`→`T?`, `HashMap`→`Map`, `Box`/`Rc`/`Arc` unwrap…).
fn map_path(path: &Path) -> JuxType {
    let name = last_segment(&path.path);
    let args = collect_type_args(&path.args);
    let arg0 = || args.first().cloned().unwrap_or(JuxType::Unknown("Object".into()));

    match name {
        "String" => JuxType::String,
        "Vec" => JuxType::list(arg0()),
        "Option" => JuxType::nullable(arg0()),
        "HashMap" | "BTreeMap" => JuxType::map(
            args.first().cloned().unwrap_or(JuxType::Unknown("Object".into())),
            args.get(1).cloned().unwrap_or(JuxType::Unknown("Object".into())),
        ),
        "HashSet" | "BTreeSet" => JuxType::set(arg0()),
        // Smart pointers are transparent to Jux (§G.3.1).
        "Box" | "Rc" | "Arc" => arg0(),
        _ => JuxType::User { name: name.to_string(), args },
    }
}

/// Map the type arguments of a path's angle-bracketed generic list, dropping
/// lifetimes and const args.
fn collect_type_args(args: &Option<Box<GenericArgs>>) -> Vec<JuxType> {
    let Some(ga) = args else { return Vec::new() };
    match ga.as_ref() {
        GenericArgs::AngleBracketed { args, .. } => args
            .iter()
            .filter_map(|a| match a {
                GenericArg::Type(t) => Some(map_type(t)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Name of the first trait bound in an `impl Trait` bound list.
fn first_trait_in_bounds(bounds: &[GenericBound]) -> Option<String> {
    bounds.iter().find_map(|b| match b {
        GenericBound::TraitBound { trait_, .. } => Some(last_segment(&trait_.path).to_string()),
        _ => None,
    })
}

// ============================================================================
// Small helpers
// ============================================================================

fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn is_public(v: &Visibility) -> bool {
    // `Default` covers trait items and enum variants of public parents.
    matches!(v, Visibility::Public | Visibility::Default)
}

/// The real, fully-qualified Rust path of `item` (`std::collections::HashSet`),
/// from the rustdoc `paths` summary. Used to populate `StubType::rust_path` so
/// the backend can lower a reference to this external type to its true Rust path
/// (§G.9.2) rather than the flat Jux `rust.std.X` spelling.
fn real_rust_path(krate: &Crate, item: &Item) -> Option<String> {
    let summary = krate.paths.get(&item.id)?;
    if summary.path.is_empty() {
        return None;
    }
    Some(public_rust_path(&summary.path))
}

/// Normalise a rustdoc **definition** path to a **publicly-importable** Rust
/// path. rustdoc's `paths` summary reports where an item is *defined*, which
/// includes private intermediate modules (`std::collections::hash::set::HashSet`,
/// `alloc::collections::btree::map::BTreeMap`) that are not themselves `pub`.
///
/// Two normalisations cover the std surface this slice targets:
/// 1. The defining crate `alloc` / `core` is re-exported wholesale under `std`,
///    so its leading segment maps to `std` (a binary always links `std`).
/// 2. The `collections` types are re-exported at `std::collections::<Type>`, so a
///    path that threads through a `collections` segment collapses to
///    `std::collections::<Type>`, dropping the private `{btree,hash,…}::{set,map}`
///    nesting.
///
/// Other multi-segment paths are kept as-is (crate-normalised). This is a
/// heuristic — a few deeply-nested non-collection types (e.g. `std::os::unix::…`)
/// keep their definition path; full public-path resolution via rustdoc re-export
/// (`Use`) items is a follow-up.
fn public_rust_path(path: &[String]) -> String {
    let mut segs: Vec<String> = path.to_vec();
    if matches!(segs.first().map(String::as_str), Some("alloc" | "core")) {
        segs[0] = "std".to_string();
    }
    if segs.iter().any(|s| s == "collections") {
        if let Some(last) = segs.last() {
            return format!("std::collections::{last}");
        }
    }
    segs.join("::")
}

fn first_doc_line(item: &Item) -> Option<String> {
    item.docs
        .as_ref()
        .and_then(|d| d.lines().next())
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustdoc_types::{Id, Path};

    /// Build a `ResolvedPath` type with optional type arguments.
    fn resolved(path: &str, type_args: Vec<Type>) -> Type {
        let args = if type_args.is_empty() {
            None
        } else {
            Some(Box::new(GenericArgs::AngleBracketed {
                args: type_args.into_iter().map(GenericArg::Type).collect(),
                constraints: Vec::new(),
            }))
        };
        Type::ResolvedPath(Path { path: path.to_string(), id: Id(0), args })
    }

    #[test]
    fn primitives_map_per_table() {
        assert_eq!(map_type(&Type::Primitive("i8".into())), JuxType::Prim("byte"));
        assert_eq!(map_type(&Type::Primitive("i32".into())), JuxType::Prim("i32"));
        assert_eq!(map_type(&Type::Primitive("usize".into())), JuxType::Prim("uint"));
        assert_eq!(map_type(&Type::Primitive("f64".into())), JuxType::Prim("double"));
        assert_eq!(map_type(&Type::Primitive("str".into())), JuxType::String);
        assert_eq!(map_type(&Type::Primitive("never".into())), JuxType::Never);
    }

    #[test]
    fn stdlib_containers_map() {
        assert_eq!(
            map_type(&resolved("Vec", vec![Type::Primitive("u8".into())])).to_string(),
            "List<ubyte>",
        );
        assert_eq!(
            map_type(&resolved("Option", vec![resolved("String", vec![])])).to_string(),
            "String?",
        );
        assert_eq!(
            map_type(&resolved(
                "std::collections::HashMap",
                vec![resolved("String", vec![]), Type::Primitive("i32".into())],
            ))
            .to_string(),
            // i32 is width-explicit (§G.3.1) — kept as `i32`, not `int`.
            "Map<String, i32>",
        );
        // Smart pointers are transparent.
        assert_eq!(
            map_type(&resolved("Box", vec![resolved("Widget", vec![])])).to_string(),
            "Widget",
        );
    }

    #[test]
    fn borrows_vanish_and_slices_become_arrays() {
        // &i32 → int (borrow inferred at the call site, §G.3.4)
        let borrowed = Type::BorrowedRef {
            lifetime: None,
            is_mutable: false,
            type_: Box::new(Type::Primitive("i32".into())),
        };
        assert_eq!(map_type(&borrowed), JuxType::Prim("i32"));

        // &[u8] → ubyte[]
        let slice_ref = Type::BorrowedRef {
            lifetime: None,
            is_mutable: true,
            type_: Box::new(Type::Slice(Box::new(Type::Primitive("u8".into())))),
        };
        assert_eq!(map_type(&slice_ref).to_string(), "ubyte[]");
    }

    #[test]
    fn result_return_becomes_throws() {
        let result_ty = resolved(
            "Result",
            vec![resolved("Config", vec![]), resolved("ConfigError", vec![])],
        );
        let (ret, throws) = map_return(&Some(result_ty));
        assert_eq!(ret.to_string(), "Config");
        assert_eq!(throws.map(|e| e.to_string()), Some("ConfigError".to_string()));

        // Plain return, no throws.
        let (ret, throws) = map_return(&Some(Type::Primitive("bool".into())));
        assert_eq!(ret, JuxType::Prim("bool"));
        assert!(throws.is_none());
    }
}
