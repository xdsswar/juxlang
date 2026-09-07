//! rustdoc JSON → stub IR — JUX-BINDGEN-ADDENDUM.md §G.3 (type mapping) and
//! §G.6 (Rust crate bindings).
//!
//! This is the only module coupled to the `rustdoc-types` schema. It walks a
//! [`Crate`] and builds the language-agnostic [`StubFile`] that `emit` renders.
//! The §G.3 Rust→Jux type table lives in [`map_type`].

use std::collections::{HashMap, HashSet};

use rustdoc_types::{
    Crate, Enum, Function, GenericArg, GenericArgs, GenericBound, GenericParamDefKind, Generics,
    Id, Item, ItemEnum, Path, Struct, StructKind, Type, VariantKind, Visibility, WherePredicate,
};

use crate::model::{
    StubConst, StubCtor, StubField, StubFile, StubFn, StubItem, StubParam, StubType, StubVariant,
    TypeKind, Vis,
};
use crate::naming::method_name;
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
pub fn generate_merged(
    jsons: &[(&str, &str)],
    package: &str,
) -> Result<StubFile, serde_json::Error> {
    generate_merged_with_pool(jsons, &[], package)
}

/// [`generate_merged`], plus crates that are read ONLY to resolve `Deref`
/// targets and never contribute items of their own.
///
/// `core` is the case this exists for. The inherent `impl<T> [T]` blocks that
/// give `Vec` its `get`/`first`/`last`/`iter`/`contains`/`reverse` live there
/// and nowhere else -- not in `alloc.json`, not in `std.json`. But emitting all
/// of `core` into the stub would multiply the surface (and every compile's
/// parse cost) for types nobody asked for. Pooling it separately takes the
/// method knowledge without the bulk.
pub fn generate_merged_with_pool(
    jsons: &[(&str, &str)],
    pool_only: &[(&str, &str)],
    package: &str,
) -> Result<StubFile, serde_json::Error> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut collected: Vec<(String, StubItem)> = Vec::new();
    let mut format_version = 0;

    // Pass 1: pool every crate's INHERENT impls, keyed by the shape of the type
    // they are written for. A `Deref` target usually lives in a different crate
    // of the same ingest -- `Vec` is in `alloc`, the `[T]` impls it derefs to
    // are in `core` -- so the pool has to span all of them before any type is
    // built. Only the mapped methods are kept, so the parsed crates do not have
    // to be held alive together.
    let mut pool = InherentPool::new();
    for (_crate_name, json) in jsons.iter().chain(pool_only.iter()) {
        let krate: Crate = serde_json::from_str(json)?;
        collect_inherent_pool(&krate, &mut pool);
    }

    // Pass 2: ingest, now able to resolve a deref target across crates.
    for (_crate_name, json) in jsons {
        let krate: Crate = serde_json::from_str(json)?;
        format_version = krate.format_version;
        for (name, item) in collect_items_with(&krate, &pool) {
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
        header: vec![format!(
            "bindgen — generated from rustdoc JSON (format_version {})",
            krate.format_version
        )],
        items: collected.into_iter().map(|(_, it)| it).collect(),
    }
}

/// Walk a single crate's index and collect every public, local
/// (`crate_id == 0`) item as a `(name, StubItem)` pair, in a **deterministic**
/// order. Shared by [`generate`] (single crate) and [`generate_merged`]
/// (cross-crate dedup) so the item-selection rules live in exactly one place.
///
/// The order matters for more than tidiness. `generate_merged` keys the stub by
/// SIMPLE NAME and keeps the first definition, and std ships several items per
/// name — `FileExt` under `std::os::{unix,wasi,windows}::fs`, one `stat` per
/// architecture. Walking `krate.index` (a hash map) made the survivor depend on
/// hash order, so regenerating the vendored `rust.std` surface from the same
/// rustdoc JSON produced a different file each run: ~32 `@rust` paths moved
/// between two regenerations. Sorting here fixes the winner.
fn collect_items(krate: &Crate) -> Vec<(String, StubItem)> {
    // Single-crate ingest: the crate is its own pool.
    let mut pool = InherentPool::new();
    collect_inherent_pool(krate, &mut pool);
    collect_items_with(krate, &pool)
}

/// Methods of every inherent impl seen across the whole ingest, keyed by the
/// shape of the type the impl is written for (see [`type_shape_key`]).
pub(crate) type InherentPool = std::collections::HashMap<String, Vec<StubFn>>;

/// Record every inherent impl in `krate` into `pool`.
fn collect_inherent_pool(krate: &Crate, pool: &mut InherentPool) {
    for item in krate.index.values() {
        let ItemEnum::Impl(im) = &item.inner else {
            continue;
        };
        if im.trait_.is_some() {
            continue;
        }
        let Some(key) = type_shape_key(&im.for_) else {
            continue;
        };
        let slot = pool.entry(key).or_default();
        for mid in &im.items {
            let Some(mitem) = krate.index.get(mid) else {
                continue;
            };
            if !is_public(&mitem.visibility) {
                continue;
            }
            let Some(mname) = &mitem.name else { continue };
            let ItemEnum::Function(f) = &mitem.inner else {
                continue;
            };
            // Only real methods carry across a deref. An associated function
            // with no receiver belongs to the target type, not to the type that
            // derefs to it: `<[T]>::from_ref` is not `Vec::from_ref`.
            if !has_self_receiver(f) {
                continue;
            }
            let mut sf = map_function(mname, f);
            sf.is_static = false;
            slot.push(sf);
        }
    }
}

/// A key identifying a type's SHAPE, for matching a `Deref` target against the
/// impls written for it.
///
/// Shape rather than equality because the generic argument names differ between
/// the two sites: `Vec<T>` derefs to `[T]`, while the slice's own impls are
/// written `impl<T> [T]` with a `T` of their own.
fn type_shape_key(t: &Type) -> Option<String> {
    match t {
        Type::Slice(_) => Some("[]".to_string()),
        Type::Primitive(p) => Some(format!("prim:{p}")),
        Type::ResolvedPath(p) => Some(format!("path:{}", p.path)),
        _ => None,
    }
}

fn collect_items_with(krate: &Crate, pool: &InherentPool) -> Vec<(String, StubItem)> {
    // Every id that is a member of some impl or trait — used to tell a free
    // function (top-level `fn`) apart from a method/associated function.
    let member_ids = collect_member_ids(krate);

    let mut collected: Vec<(u32, String, StubItem)> = Vec::new();

    for item in krate.index.values() {
        if item.crate_id != 0 {
            continue; // skip external items referenced locally
        }
        let Some(name) = &item.name else { continue };

        match &item.inner {
            ItemEnum::Struct(s) if is_public(&item.visibility) => {
                collected.push((
                    item.id.0,
                    name.clone(),
                    StubItem::Type(build_struct(krate, name, s, item, pool)),
                ));
            }
            ItemEnum::Enum(e) if is_public(&item.visibility) => {
                collected.push((
                    item.id.0,
                    name.clone(),
                    StubItem::Type(build_enum(krate, name, e, item)),
                ));
            }
            ItemEnum::Trait(t) if is_public(&item.visibility) => {
                collected.push((
                    item.id.0,
                    name.clone(),
                    StubItem::Type(build_trait(krate, name, &t.generics, &t.items, item)),
                ));
            }
            ItemEnum::Function(f)
                if is_public(&item.visibility) && !member_ids.contains(&item.id.0) =>
            {
                // Free function (§G.5.5). Record its real Rust path so the
                // backend can `use` the fully-qualified Rust path under the
                // (verbatim, snake_case) Jux stub name on import.
                let mut sf = map_function(name, f);
                sf.is_static = false;
                sf.rust_path = real_rust_path(krate, item);
                collected.push((item.id.0, name.clone(), StubItem::Function(sf)));
            }
            ItemEnum::Constant { type_, const_: _ } if is_public(&item.visibility) => {
                collected.push((
                    item.id.0,
                    name.clone(),
                    StubItem::Const(StubConst {
                        name: name.clone(),
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
                    item.id.0,
                    name.clone(),
                    StubItem::Const(StubConst {
                        name: name.clone(),
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

    // Sort by (name, path-segment count, path): among same-name duplicates the
    // most general path wins, and ties break lexicographically rather than by
    // hash order.
    collected.sort_by(|a, b| {
        let ka = stub_item_path(&a.2);
        let kb = stub_item_path(&b.2);
        a.1.cmp(&b.1)
            .then_with(|| ka.matches("::").count().cmp(&kb.matches("::").count()))
            .then_with(|| ka.cmp(kb))
            // Last resort, for the items rustdoc records no path for (a
            // `Cursor` exists on both `LinkedList` and `BTreeMap`): the rustdoc
            // id is intrinsic to the JSON, so it decides the same way every run.
            .then_with(|| a.0.cmp(&b.0))
    });
    collected
        .into_iter()
        .map(|(_, name, item)| (name, item))
        .collect()
}

/// A collected item's recorded Rust path, or the empty string when it has none
/// — the sort key that makes duplicate-name selection reproducible.
fn stub_item_path(item: &StubItem) -> &str {
    match item {
        StubItem::Type(t) => t.rust_path.as_deref().unwrap_or(""),
        StubItem::Function(f) => f.rust_path.as_deref().unwrap_or(""),
        StubItem::Const(_) => "",
    }
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

fn build_struct(
    krate: &Crate,
    name: &str,
    s: &Struct,
    item: &Item,
    pool: &InherentPool,
) -> StubType {
    let mut fields = Vec::new();
    let mut all_public = true;

    match &s.kind {
        StructKind::Plain {
            fields: fids,
            has_stripped_fields,
        } => {
            if *has_stripped_fields {
                all_public = false;
            }
            for fid in fids {
                let Some(fitem) = krate.index.get(fid) else {
                    continue;
                };
                let ItemEnum::StructField(ty) = &fitem.inner else {
                    continue;
                };
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
    // Rust's method resolution follows `Deref`, so `Vec<T>` really does have
    // every `[T]` method — and a stub that stops at the inherent impls is
    // simply an incomplete description of the type. `Vec` came out with 58
    // methods and without `sort`, `get`, `iter`, `first`, `last`, `contains`
    // or `reverse`, which is why the backend had grown hardcoded branches for
    // some of them: the scan was not telling it the truth.
    methods.extend(deref_members(krate, &s.impls, pool));
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
    st.is_clone = implements_trait(krate, &s.impls, "Clone");
    st
}

fn build_enum(krate: &Crate, name: &str, e: &Enum, item: &Item) -> StubType {
    let mut st = StubType::new(TypeKind::Enum, name);
    st.generics = generic_param_names(&e.generics);
    st.doc = first_doc_line(item);
    st.rust_path = real_rust_path(krate, item);

    for vid in &e.variants {
        let Some(vitem) = krate.index.get(vid) else {
            continue;
        };
        let ItemEnum::Variant(v) = &vitem.inner else {
            continue;
        };
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

        st.variants.push(StubVariant {
            name: vname.clone(),
            payload,
            discriminant,
        });
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
        let Some(mitem) = krate.index.get(mid) else {
            continue;
        };
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
        let Some(impl_item) = krate.index.get(impl_id) else {
            continue;
        };
        let ItemEnum::Impl(im) = &impl_item.inner else {
            continue;
        };
        if im.trait_.is_some() {
            continue; // only inherent impls contribute the safe wrapper surface
        }
        for mid in &im.items {
            let Some(mitem) = krate.index.get(mid) else {
                continue;
            };
            if !is_public(&mitem.visibility) {
                continue;
            }
            let Some(mname) = &mitem.name else { continue };
            let ItemEnum::Function(f) = &mitem.inner else {
                continue;
            };

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

/// The methods a type inherits through `Deref`.
///
/// Rust resolves `vec.first()` by dereferencing `Vec<T>` to `[T]` and finding
/// `first` there, so those methods are part of the type's real surface. A stub
/// that stops at the inherent impls is an incomplete description of the type:
/// `Vec` came out with 58 methods and none of `get`, `first`, `last`, `iter`,
/// `contains` or `reverse`, which is exactly why the backend had grown
/// hardcoded branches for some of them.
///
/// One level only. Deref chains deeper than one step are rare, and following
/// them blindly pulls in a large unrelated surface.
fn deref_members(krate: &Crate, impls: &[rustdoc_types::Id], pool: &InherentPool) -> Vec<StubFn> {
    let Some(target) = deref_target(krate, impls) else {
        return Vec::new();
    };
    let Some(key) = type_shape_key(&target) else {
        return Vec::new();
    };
    pool.get(&key).cloned().unwrap_or_default()
}

/// The `Target` of a type's `Deref` impl, if it has one.
fn deref_target(krate: &Crate, impls: &[rustdoc_types::Id]) -> Option<Type> {
    for impl_id in impls {
        let Some(item) = krate.index.get(impl_id) else {
            continue;
        };
        let ItemEnum::Impl(im) = &item.inner else {
            continue;
        };
        let Some(tr) = &im.trait_ else { continue };
        if tr.path.rsplit("::").next() != Some("Deref") {
            continue;
        }
        // `type Target = [T];` is an associated type inside the impl.
        for aid in &im.items {
            let Some(aitem) = krate.index.get(aid) else {
                continue;
            };
            if aitem.name.as_deref() != Some("Target") {
                continue;
            }
            if let ItemEnum::AssocType { type_: Some(t), .. } = &aitem.inner {
                return Some(t.clone());
            }
        }
    }
    None
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
        returns_borrow: f.sig.output.as_ref().is_some_and(returns_borrowed),
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
            ty: map_param_type(ty, &f.generics),
            by_ref: is_borrow_param(ty),
        })
        .collect()
}

/// Like [`map_type`], but with closure-parameter recovery: a parameter typed
/// by a generic with an `Fn`/`FnMut`/`FnOnce` bound (`fn f<F: Fn(A) -> B>(cb: F)`)
/// surfaces as the Jux function type `(A) -> B` so a Jux lambda can be passed,
/// instead of an opaque type parameter `F`. The unused `<F>` stays on the
/// method's generic list and is inferred at the Rust call site from the bare
/// closure the backend emits. (The syntactic `impl Fn(..)` form is handled
/// directly in [`map_type`].)
fn map_param_type(ty: &Type, generics: &Generics) -> JuxType {
    if let Type::Generic(name) = ty {
        if let Some(fnty) = generic_fn_bound(name, generics) {
            return fnty;
        }
    }
    map_type(ty)
}

/// Recover the closure signature for a generic param `name` whose bound is an
/// `Fn`-family trait — checking both the param's own bound list and the
/// `where` clause. `None` when `name` has no Fn bound.
fn generic_fn_bound(name: &str, generics: &Generics) -> Option<JuxType> {
    for p in &generics.params {
        if p.name == name {
            if let GenericParamDefKind::Type { bounds, .. } = &p.kind {
                if let Some(fnty) = fn_trait_to_jux(bounds) {
                    return Some(fnty);
                }
            }
        }
    }
    for w in &generics.where_predicates {
        if let WherePredicate::BoundPredicate { type_, bounds, .. } = w {
            if matches!(type_, Type::Generic(n) if n == name) {
                if let Some(fnty) = fn_trait_to_jux(bounds) {
                    return Some(fnty);
                }
            }
        }
    }
    None
}

/// True when the Rust parameter is a borrow that maps to a by-value Jux type but
/// must be passed with a call-site `&` (§G.9.2). A borrowed **slice** (`&[T]`)
/// is excluded — it maps to a Jux array and is lowered through the array path,
/// not as a single `&arg`.
/// True when a Rust RETURN type hands back a borrow of the receiver: `&T` /
/// `&mut T`, or an `Option` / `Result` wrapping one. `Option<&T>` is what
/// `Vec::first`, `HashMap::get` and every positional getter return, and it is
/// indistinguishable from an owned `Option<T>` once the Jux type has dropped
/// the `&`.
///
/// Recursion into generic arguments is one level deep on purpose: that is
/// where every shape in the std surface puts it, and going deeper would start
/// flagging containers that merely CONTAIN references.
fn returns_borrowed(ty: &Type) -> bool {
    match ty {
        Type::BorrowedRef { .. } => true,
        Type::ResolvedPath(p) => matches!(
            p.args.as_deref(),
            Some(GenericArgs::AngleBracketed { args, .. })
                if args
                    .iter()
                    .any(|a| matches!(a, GenericArg::Type(Type::BorrowedRef { .. })))
        ),
        _ => false,
    }
}

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
            let err = args
                .get(1)
                .cloned()
                .or_else(|| Some(JuxType::user("Error")));
            (ok, err)
        }
        Some(t) => (map_type(t), None),
    }
}

fn param_name(n: &str) -> String {
    // Parameter names are surfaced verbatim (§G.4); only non-identifier names
    // (rare in rustdoc — e.g. destructured patterns) fall back to `arg`. Keyword
    // spellings are kept and handled at the parser (foreign mode) and backend.
    if n.is_empty() || !n.chars().all(|c| c.is_alphanumeric() || c == '_') {
        "arg".to_string()
    } else {
        n.to_string()
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
        let Some(item) = krate.index.get(id) else {
            return false;
        };
        let ItemEnum::Impl(im) = &item.inner else {
            return false;
        };
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

/// Does this type implement `trait_name`? DISCOVERED from its real trait
/// impls, so the answer tracks the library rather than a list of type names.
///
/// Synthetic impls are excluded: rustdoc emits those for auto traits
/// (`Send`/`Sync`), which are not what any caller here is asking about.
fn implements_trait(krate: &Crate, impls: &[rustdoc_types::Id], trait_name: &str) -> bool {
    impls.iter().any(|id| {
        let Some(item) = krate.index.get(id) else {
            return false;
        };
        let ItemEnum::Impl(im) = &item.inner else {
            return false;
        };
        if im.is_synthetic || im.is_negative {
            return false;
        }
        im.trait_
            .as_ref()
            .is_some_and(|tr| last_segment(&tr.path) == trait_name)
    })
}

/// True when the function's receiver is `&mut self` — the method mutates
/// the value it is called on. (A by-value `self` consumes rather than
/// mutates and is not flagged.)
fn has_mut_self_receiver(f: &Function) -> bool {
    f.sig.inputs.iter().any(|(n, t)| {
        n == "self"
            && matches!(
                t,
                Type::BorrowedRef {
                    is_mutable: true,
                    ..
                }
            )
    })
}

fn generic_param_names(g: &Generics) -> Vec<String> {
    g.params
        .iter()
        .filter_map(|p| match &p.kind {
            // Type params only; lifetimes and consts don't appear in Jux
            // generic lists. Skip synthetic `impl Trait` desugarings.
            GenericParamDefKind::Type { .. } if !p.name.starts_with("impl ") => {
                Some(p.name.clone())
            }
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
        Type::Slice(inner) => JuxType::Array {
            elem: Box::new(map_type(inner)),
            size: None,
        },
        Type::Array { type_, len } => JuxType::Array {
            elem: Box::new(map_type(type_)),
            size: len.parse::<u64>().ok(),
        },
        // Borrows vanish (§G.3.4); `&[T]` becomes a dynamic array.
        Type::BorrowedRef { type_, .. } => match type_.as_ref() {
            Type::Slice(inner) => JuxType::Array {
                elem: Box::new(map_type(inner)),
                size: None,
            },
            other => map_type(other),
        },
        Type::RawPointer { type_, .. } => JuxType::RawPtr(Box::new(map_type(type_))),
        // `impl Fn(A) -> B` (and `FnMut`/`FnOnce`) recovers its CALL SIGNATURE
        // as a Jux function type `(A) -> B`, so a Jux lambda can be passed to
        // the foreign API (§G.3 closures). A bare `impl Fn*` param accepts a
        // bare Rust closure, which the backend emits for foreign fn-typed
        // params. A non-Fn `impl Trait` keeps its first-trait name.
        Type::ImplTrait(bounds) => fn_trait_to_jux(bounds).unwrap_or_else(|| {
            first_trait_in_bounds(bounds)
                .map(JuxType::user)
                .unwrap_or_else(|| JuxType::Unknown("Object".into()))
        }),
        Type::DynTrait(dt) => dt
            .traits
            .first()
            .map(|pt| JuxType::user(last_segment(&pt.trait_.path)))
            .unwrap_or_else(|| JuxType::Unknown("Object".into())),
        Type::FunctionPointer(fp) => {
            let params = fp.sig.inputs.iter().map(|(_, t)| map_type(t)).collect();
            let ret = fp
                .sig
                .output
                .as_ref()
                .map(map_type)
                .unwrap_or(JuxType::Void);
            JuxType::Fn {
                params,
                ret: Box::new(ret),
                is_async: false,
            }
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
    let arg0 = || {
        args.first()
            .cloned()
            .unwrap_or(JuxType::Unknown("Object".into()))
    };

    match name {
        "String" => JuxType::String,
        "Vec" => JuxType::list(arg0()),
        "Option" => JuxType::nullable(arg0()),
        "HashMap" | "BTreeMap" => JuxType::map(
            args.first()
                .cloned()
                .unwrap_or(JuxType::Unknown("Object".into())),
            args.get(1)
                .cloned()
                .unwrap_or(JuxType::Unknown("Object".into())),
        ),
        "HashSet" | "BTreeSet" => JuxType::set(arg0()),
        // Smart pointers are transparent to Jux (§G.3.1).
        "Box" | "Rc" | "Arc" => arg0(),
        _ => JuxType::User {
            name: name.to_string(),
            args,
        },
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

/// If a bound list contains an `Fn`/`FnMut`/`FnOnce` trait, recover its
/// parenthesized call signature (`Fn(A, B) -> R`) as a Jux function type
/// `(A, B) -> R`. Returns `None` for a non-Fn bound list. The Fn-family
/// distinction isn't carried in the surface type: a bare Rust closure (what
/// the backend emits for a foreign fn-typed param) satisfies `Fn`, `FnMut`,
/// AND `FnOnce`, so all three map to the same `(A) -> R`.
fn fn_trait_to_jux(bounds: &[GenericBound]) -> Option<JuxType> {
    for b in bounds {
        let GenericBound::TraitBound { trait_, .. } = b else {
            continue;
        };
        if !matches!(last_segment(&trait_.path), "Fn" | "FnMut" | "FnOnce") {
            continue;
        }
        // The call signature lives in the trait path's PARENTHESIZED args
        // (`Fn(inputs) -> output`), distinct from the angle-bracketed form.
        if let Some(args) = &trait_.args {
            if let GenericArgs::Parenthesized { inputs, output } = args.as_ref() {
                let params = inputs.iter().map(map_type).collect();
                let ret = output.as_ref().map(map_type).unwrap_or(JuxType::Void);
                return Some(JuxType::Fn {
                    params,
                    ret: Box::new(ret),
                    is_async: false,
                });
            }
        }
        // An `Fn` bound with no parenthesized args (rare) → a no-arg closure.
        return Some(JuxType::Fn {
            params: Vec::new(),
            ret: Box::new(JuxType::Void),
            is_async: false,
        });
    }
    None
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
    let summary = krate.paths.get(&item.id);
    // The definition path is preferred WHENEVER it is importable — it is the
    // canonical one, and it distinguishes items that share a simple name across
    // sibling modules (`std::os::unix::process::ChildExt` vs the `linux` one).
    // Only when it threads a private module is it unusable, and then the
    // re-export path is the answer.
    if let Some(summary) = summary {
        if !summary.path.is_empty() && definition_path_is_public(krate, &summary.path) {
            return Some(public_rust_path(&summary.path));
        }
    }
    if let Some(p) = reexport_paths(krate).get(&item.id).cloned() {
        return Some(p);
    }
    let summary = summary?;
    if summary.path.is_empty() {
        return None;
    }
    Some(public_rust_path(&summary.path))
}

/// True when every module along `path` (all but the final item segment) is a
/// PUBLIC module of the crate, so the path can be written in a `use`.
///
/// `std::io::copy::copy` fails here: `std::io` has a private `mod copy` that
/// exists only to be re-exported, so the definition path names something the
/// user cannot import.
fn definition_path_is_public(krate: &Crate, path: &[String]) -> bool {
    if path.len() <= 2 {
        // `crate::Item` — the crate root is always importable.
        return true;
    }
    let modules = public_module_paths(krate);
    (1..path.len()).all(|end| modules.contains(&public_module_path(&path[..end])))
}

/// Every public module of the crate, by public path. Memoized alongside the
/// re-export table — both are whole-crate scans over an index with tens of
/// thousands of entries.
fn public_module_paths(krate: &Crate) -> &HashSet<String> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<usize, &'static HashSet<String>>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let key = std::ptr::from_ref(krate) as usize;
    if let Some(hit) = cache.lock().expect("module cache").get(&key) {
        return hit;
    }
    let mut out: HashSet<String> = HashSet::new();
    for item in krate.index.values() {
        if !matches!(item.inner, ItemEnum::Module(_)) || !is_public(&item.visibility) {
            continue;
        }
        if let Some(summary) = krate.paths.get(&item.id) {
            if !summary.path.is_empty() {
                out.insert(public_module_path(&summary.path));
            }
        }
    }
    let built: &'static HashSet<String> = Box::leak(Box::new(out));
    cache.lock().expect("module cache").insert(key, built);
    built
}

/// Normalise a MODULE path. Only the crate-root remap applies: the
/// `collections` collapse in [`public_rust_path`] is about the *type* being
/// re-exported at `std::collections::<Type>`, and folding it into a module path
/// would turn `alloc::collections` into `std::collections::collections`.
fn public_module_path(path: &[String]) -> String {
    let mut segs: Vec<String> = path.to_vec();
    if matches!(segs.first().map(String::as_str), Some("alloc" | "core")) {
        segs[0] = "std".to_string();
    }
    segs.join("::")
}

/// Item id → the shortest **publicly importable** path, read off the crate's
/// re-export (`pub use`) statements.
///
/// rustdoc's `paths` summary reports where an item is DEFINED, and std defines
/// plenty of things in private modules that exist only to be re-exported:
/// `std::io` has a private `mod copy` holding `pub fn copy`, published as
/// `pub use self::copy::copy;`. The definition path `std::io::copy::copy` names
/// a private module and does not compile in a `use`. The re-export says the
/// public name is `std::io::copy`.
///
/// Built once per crate and memoized — std has tens of thousands of items and
/// this runs for each of them.
///
/// Not every path is recoverable this way: a glob (`pub use self::x::*;`) names
/// no individual item, so those fall back to the definition path plus the
/// [`public_rust_path`] normalisations.
fn reexport_paths(krate: &Crate) -> &HashMap<Id, String> {
    // Keyed by crate identity: `generate_merged` ingests several crates in one
    // process, and each has its own re-export graph.
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<HashMap<usize, &'static HashMap<Id, String>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let key = std::ptr::from_ref(krate) as usize;
    if let Some(hit) = cache.lock().expect("reexport cache").get(&key) {
        return hit;
    }
    let built: &'static HashMap<Id, String> = Box::leak(Box::new(build_reexport_paths(krate)));
    cache.lock().expect("reexport cache").insert(key, built);
    built
}

fn build_reexport_paths(krate: &Crate) -> HashMap<Id, String> {
    let mut out: HashMap<Id, String> = HashMap::new();
    for module_item in krate.index.values() {
        let ItemEnum::Module(m) = &module_item.inner else {
            continue;
        };
        if !is_public(&module_item.visibility) {
            continue;
        }
        // Where this module itself lives publicly.
        let Some(summary) = krate.paths.get(&module_item.id) else {
            continue;
        };
        if summary.path.is_empty() {
            continue;
        }
        let module_path = public_module_path(&summary.path);
        for child in &m.items {
            let Some(child_item) = krate.index.get(child) else {
                continue;
            };
            // Two shapes reach the same conclusion. An explicit `pub use` names
            // its target; a re-export of a PRIVATE item is instead INLINED by
            // rustdoc — `std::io`'s private `mod copy` contributes its `pub fn
            // copy` straight into `std::io`'s item list — and then membership in
            // a public module IS the public path.
            let (target, name) = match &child_item.inner {
                ItemEnum::Use(u) if !u.is_glob => match u.id {
                    Some(id) => (id, u.name.clone()),
                    None => continue,
                },
                ItemEnum::Struct(_)
                | ItemEnum::Enum(_)
                | ItemEnum::Trait(_)
                | ItemEnum::Function(_)
                | ItemEnum::TypeAlias(_) => match &child_item.name {
                    Some(n) if is_public(&child_item.visibility) => (child_item.id, n.clone()),
                    _ => continue,
                },
                _ => continue,
            };
            let candidate = format!("{module_path}::{name}");
            // Shortest wins: `std::io::copy` beats a deeper alias of the same
            // function, and the result is stable across runs.
            // Shortest wins, then lexicographic — `std::io::copy` beats a
            // deeper alias, and the choice between two equally short aliases is
            // the same on every run (a HashMap-order tie-break would churn the
            // generated stub).
            match out.get(&target) {
                Some(existing)
                    if (existing.len(), existing.as_str())
                        <= (candidate.len(), candidate.as_str()) => {}
                _ => {
                    out.insert(target, candidate);
                }
            }
        }
    }
    out
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
        Type::ResolvedPath(Path {
            path: path.to_string(),
            id: Id(0),
            args,
        })
    }

    /// A module path keeps its shape — only the defining crate is remapped.
    /// The `collections` collapse belongs to TYPE paths; applying it to a
    /// module turned `alloc::collections` into `std::collections::collections`,
    /// and every type re-exported through it inherited the doubled segment.
    #[test]
    fn module_paths_are_not_collection_collapsed() {
        let segs = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            public_module_path(&segs(&["alloc", "collections"])),
            "std::collections"
        );
        assert_eq!(public_module_path(&segs(&["core", "fmt"])), "std::fmt");
        assert_eq!(public_module_path(&segs(&["std", "io"])), "std::io");
        // The TYPE rule still collapses, which is what re-exports std types at
        // `std::collections::<Type>`.
        assert_eq!(
            public_rust_path(&segs(&["alloc", "collections", "btree", "map", "BTreeMap"])),
            "std::collections::BTreeMap",
        );
    }

    #[test]
    fn primitives_map_per_table() {
        assert_eq!(
            map_type(&Type::Primitive("i8".into())),
            JuxType::Prim("byte")
        );
        assert_eq!(
            map_type(&Type::Primitive("i32".into())),
            JuxType::Prim("i32")
        );
        assert_eq!(
            map_type(&Type::Primitive("usize".into())),
            JuxType::Prim("uint")
        );
        assert_eq!(
            map_type(&Type::Primitive("f64".into())),
            JuxType::Prim("double")
        );
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

    /// `impl Fn(A) -> B` recovers its parenthesized call signature as the Jux
    /// function type `(A) -> B`, so a Jux lambda can be passed (§G.3).
    #[test]
    fn impl_fn_recovers_call_signature() {
        let bound = GenericBound::TraitBound {
            trait_: Path {
                path: "Fn".into(),
                id: Id(0),
                args: Some(Box::new(GenericArgs::Parenthesized {
                    inputs: vec![Type::Primitive("i32".into())],
                    output: Some(Type::Primitive("bool".into())),
                })),
            },
            generic_params: Vec::new(),
            modifier: rustdoc_types::TraitBoundModifier::None,
        };
        assert_eq!(
            map_type(&Type::ImplTrait(vec![bound])).to_string(),
            "(i32) -> bool"
        );
    }

    /// A non-Fn `impl Trait` keeps its first-trait name (no closure recovery).
    #[test]
    fn impl_non_fn_trait_keeps_name() {
        let bound = GenericBound::TraitBound {
            trait_: Path {
                path: "Display".into(),
                id: Id(0),
                args: None,
            },
            generic_params: Vec::new(),
            modifier: rustdoc_types::TraitBoundModifier::None,
        };
        assert_eq!(
            map_type(&Type::ImplTrait(vec![bound])).to_string(),
            "Display"
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
        assert_eq!(
            throws.map(|e| e.to_string()),
            Some("ConfigError".to_string())
        );

        // Plain return, no throws.
        let (ret, throws) = map_return(&Some(Type::Primitive("bool".into())));
        assert_eq!(ret, JuxType::Prim("bool"));
        assert!(throws.is_none());
    }
}
