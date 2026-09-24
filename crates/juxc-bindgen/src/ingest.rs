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
    ProjectionRole, StubAlias, StubConst, StubCtor, StubField, StubFile, StubFn, StubItem,
    StubParam, StubType,
    StubVariant,
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
    generate_merged_with_sources(jsons, pool_only, &[], package)
}

/// [`generate_merged_with_pool`], plus Rust SOURCE files read for the inherent
/// impls rustdoc JSON leaves out (§G.6.4.4): `alloc`'s `impl<T> [T]` and
/// `impl str`. `sources` are `(path, text)` pairs; see [`crate::source`] for
/// what is taken from them. Their methods join the same shape-keyed pool as
/// the JSON's, so they reach `Vec` and `String` through `Deref` like `core`'s.
pub fn generate_merged_with_sources(
    jsons: &[(&str, &str)],
    pool_only: &[(&str, &str)],
    sources: &[(&str, &str)],
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
    let mut sources_pooled = sources.is_empty();
    for (_crate_name, json) in jsons.iter().chain(pool_only.iter()) {
        let krate: Crate = serde_json::from_str(json)?;
        collect_inherent_pool(&krate, &mut pool);
        // The source-built signatures are mapped once, with any parsed crate
        // as the (unused) lookup context. After the JSON's own methods, so a
        // name both sources know keeps rustdoc's reading.
        if !sources_pooled {
            crate::source::collect_source_pool(sources, &krate, &mut pool);
            sources_pooled = true;
        }
    }

    // Pass 2: ingest, now able to resolve a deref target across crates.
    let pool_names: HashSet<&str> = pool_only.iter().map(|(n, _)| *n).collect();
    let mut reexports = PoolReexports::default();
    // The facade is the crate a program names (`std`): the last one given.
    // `alloc` re-exports `core` too, but `alloc::slice::...` is not a path a
    // program's crate can write.
    let facade = jsons.last().map(|(n, _)| *n);
    for (crate_name, json) in jsons {
        let krate: Crate = serde_json::from_str(json)?;
        format_version = krate.format_version;
        if Some(*crate_name) == facade {
            reexports.record(&krate, &pool_names);
        }
        for (name, item) in collect_items_with(&krate, &pool) {
            // First definition wins (crates passed core→alloc→std), and
            // platform-duplicated names are collapsed.
            if seen.insert(name.clone()) {
                collected.push((name, item));
            }
        }
    }

    // Pass 3: what the ingested crates RE-EXPORT from a pool crate. `std`
    // publishes `core::time::Duration` as `std::time::Duration` and
    // `core::cmp::Ordering` through `pub use core::cmp`, and a program reaches
    // them as std's own; skipping every `core` definition left `sleep`
    // taking a `Duration` nobody could name (B25, B44).
    for (_crate_name, json) in pool_only {
        let krate: Crate = serde_json::from_str(json)?;
        for (name, item) in reexports.surface(&krate, &pool, &collected) {
            if seen.insert(name.clone()) {
                collected.push((name, item));
            }
        }
    }

    collected.sort_by(|a, b| a.0.cmp(&b.0));
    retain_declared_implements(&mut collected);
    retain_projection_overloads(&mut collected);
    // A plain alias is kept only when it ends somewhere this stub can name: a
    // primitive, or a type (or alias) the stub declares. Anything else would
    // be an alias to nothing.
    let declared: HashSet<String> = collected.iter().map(|(n, _)| n.clone()).collect();
    collected.retain(|(_, item)| match item {
        StubItem::Alias(a) => is_jux_primitive_name(&a.target) || declared.contains(&a.target),
        _ => true,
    });

    Ok(StubFile {
        package: package.to_string(),
        header: vec![format!(
            "bindgen -- generated from {} rustdoc JSON crate(s) (format_version {})",
            jsons.len(),
            format_version
        )],
        items: collected.into_iter().map(|(_, it)| it).collect(),
    })
}

/// The items the ingested crates re-export from a POOL crate (`core`), by the
/// definition path the pool crate records for them.
#[derive(Default)]
struct PoolReexports {
    /// `pub use core::time::Duration;` in `std::time`: definition path to the
    /// public path (`core::time::Duration` -> `std::time::Duration`).
    items: HashMap<String, String>,
    /// `pub use core::cmp;` in `std`: a whole module, by definition prefix
    /// (`core::cmp` -> `std::cmp`).
    modules: Vec<(String, String)>,
}

impl PoolReexports {
    /// Record every public, non-glob `use` in `krate` whose target lives in
    /// one of `pool` (by crate name).
    fn record(&mut self, krate: &Crate, pool: &HashSet<&str>) {
        for module in krate.index.values() {
            if module.crate_id != 0 || !is_public(&module.visibility) {
                continue;
            }
            let ItemEnum::Module(m) = &module.inner else { continue };
            let Some(mpath) = krate.paths.get(&module.id).map(|s| s.path.join("::")) else {
                continue;
            };
            for child in &m.items {
                let Some(citem) = krate.index.get(child) else { continue };
                if !is_public(&citem.visibility) {
                    continue;
                }
                let ItemEnum::Use(u) = &citem.inner else { continue };
                if u.is_glob {
                    continue;
                }
                let Some(target) = u.id.as_ref().and_then(|id| krate.paths.get(id)) else {
                    continue;
                };
                let from_pool = krate
                    .external_crates
                    .get(&target.crate_id)
                    .is_some_and(|c| pool.contains(c.name.as_str()));
                if !from_pool {
                    continue;
                }
                let def = target.path.join("::");
                let public = format!("{mpath}::{}", u.name);
                if matches!(target.kind, rustdoc_types::ItemKind::Module) {
                    self.modules.push((def, public));
                } else {
                    // The shortest public spelling: `std::iter::Iterator`
                    // over `std::prelude::v1::Iterator`.
                    let slot = self.items.entry(def).or_insert_with(|| public.clone());
                    if public.len() < slot.len() {
                        *slot = public;
                    }
                }
            }
        }
        // Longest prefix first, so `core::f64::consts` beats `core::f64`.
        self.modules.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.cmp(b)));
    }

    /// The public path a pool item is reached by, and whether that is an
    /// item-level re-export (`true`) or one through a re-exported module
    /// (`false`).
    ///
    /// `def` is where the item is defined (`core::iter::adapters::map::Map`,
    /// through private modules), `pool_public` the path the pool crate itself
    /// publishes it at (`core::iter::Map`). An item re-export is keyed by the
    /// definition; a module re-export rewrites the published path, since only
    /// that one threads public modules. The shorter spelling wins when both
    /// apply (`std::option::Option` over `std::prelude::v1::Option`).
    fn public_path(&self, def: &str, pool_public: Option<&str>) -> Option<(String, bool)> {
        let item = self.items.get(def).cloned();
        let pool_crate = def.split("::").next().unwrap_or_default();
        let module = pool_public.and_then(|pp| {
            // Already a facade path: the pool crate's own scan followed the
            // re-export (`core::cmp::Ordering` comes back `std::cmp::Ordering`).
            if pp.split("::").next() != Some(pool_crate) {
                return Some(pp.to_string());
            }
            self.modules.iter().find_map(|(prefix, public)| {
                pp.strip_prefix(prefix.as_str())
                    .filter(|rest| rest.starts_with("::"))
                    .map(|rest| format!("{public}{rest}"))
            })
        });
        match (item, module) {
            (Some(i), Some(m)) if m.len() < i.len() => Some((m, true)),
            (Some(i), _) => Some((i, true)),
            (None, Some(m)) => Some((m, false)),
            (None, None) => None,
        }
    }

    /// The pool crate's items to add to the stub, with their `@rust` paths
    /// rewritten to the public re-export.
    ///
    /// Only TYPES (structs, enums), constants and free functions are taken,
    /// plus the `Iterator` trait. Any other pool trait would put
    /// `implements Debug, Clone, ...` on every type of the stub, and those are
    /// language meaning in Jux (operators, `@RustClone`), not interfaces to
    /// inherit; `Iterator` is the K.5 iteration protocol, whose adaptors a
    /// program calls.
    ///
    /// Everything re-exported ITEM by item is added; that is std choosing to
    /// publish it. Through a re-exported MODULE only what the ingested crates'
    /// own items mention is added (`Ordering`, named by `sort_unstable_by`),
    /// since `pub use core::iter;` alone would otherwise pull all of
    /// `core::iter` in: the reason the pool exists (G.6.2.1). A name several
    /// such items share is decided by the shorter public path
    /// (`std::cmp::Ordering` over `std::sync::atomic::Ordering`); a true tie
    /// (`INFINITY` for `f32` and `f64`) is left out, since the flat package
    /// cannot say which one a program means.
    fn surface(
        &self,
        krate: &Crate,
        pool: &InherentPool,
        already: &[(String, StubItem)],
    ) -> Vec<(String, StubItem)> {
        let mut candidates: Vec<(String, StubItem, bool, String)> = Vec::new();
        for (id, name, mut item) in collect_items_with_ids(krate, pool) {
            let Some(def) = krate.paths.get(&Id(id)).map(|s| s.path.join("::")) else {
                continue;
            };
            // The type map folds `Option` and `Result` into `T?` and `throws`
            // (G.3.1), so they are never types of the stub.
            if map_path_folds(&name) {
                continue;
            }
            let pool_public = match &item {
                StubItem::Type(t) => t.rust_path.clone(),
                StubItem::Function(f) => f.rust_path.clone(),
                StubItem::Const(c) => c.rust_path.clone(),
                StubItem::Alias(_) => None,
            };
            let Some((public, item_level)) = self.public_path(&def, pool_public.as_deref()) else {
                continue;
            };
            match &mut item {
                // Of the pool's traits only `Iterator` is taken: it is the K.5
                // iteration protocol, the one trait Jux gives meaning as an
                // interface (its adaptors: `count`, `sum`, `position`, ...).
                StubItem::Type(t) if t.kind != TypeKind::Interface || name == "Iterator" => {
                    t.rust_path = Some(public.clone());
                    t.name = stub_trait_name(&t.name).to_string();
                }
                StubItem::Function(f) => f.rust_path = Some(public.clone()),
                StubItem::Const(c) => c.rust_path = Some(public.clone()),
                _ => continue,
            }
            let name = if matches!(&item, StubItem::Type(t) if t.kind == TypeKind::Interface) {
                stub_trait_name(&name).to_string()
            } else {
                name
            };
            candidates.push((name, item, item_level, public));
        }
        let declared: HashSet<&str> = already.iter().map(|(n, _)| n.as_str()).collect();
        let mut referenced: HashSet<String> = HashSet::new();
        for (_, item) in already {
            item.referenced_type_names(&mut referenced);
        }
        // ...and what the `Iterator` trait names: its adaptors return `Filter`,
        // `Map`, `Zip`, whose own `count`/`sum` a chained call reaches.
        for (name, item, item_level, _) in &candidates {
            if *item_level && name == "RustIterator" {
                item.referenced_type_names(&mut referenced);
            }
        }
        // One winner per name: the shorter public path; a tie drops the name.
        let mut best: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, (name, _, item_level, _)) in candidates.iter().enumerate() {
            let wanted = *item_level || referenced.contains(name);
            if wanted && !declared.contains(name.as_str()) {
                best.entry(name.as_str()).or_default().push(i);
            }
        }
        let mut chosen: Vec<(String, StubItem)> = Vec::new();
        for (name, idxs) in best {
            let depth = |i: &usize| candidates[*i].3.matches("::").count();
            let Some(min) = idxs.iter().map(depth).min() else { continue };
            let winners: Vec<&usize> = idxs.iter().filter(|i| depth(i) == min).collect();
            if let [only] = winners.as_slice() {
                chosen.push((name.to_string(), candidates[**only].1.clone()));
            }
        }
        chosen
    }
}

/// Keep in each type's `implements` clause only the traits this stub declares
/// as interfaces with no type parameters. A trait taken by name from another
/// crate (see `implemented_trait_names`) is dropped when the stub cannot name
/// it, since an unresolvable name would help nothing.
fn retain_declared_implements(collected: &mut [(String, StubItem)]) {
    let interfaces: HashSet<String> = collected
        .iter()
        .filter_map(|(n, it)| match it {
            StubItem::Type(t) if t.kind == TypeKind::Interface && t.generics.is_empty() => {
                Some(n.clone())
            }
            _ => None,
        })
        .collect();
    for (_, item) in collected.iter_mut() {
        if let StubItem::Type(t) = item {
            // `RustIterator` lives in `rust.std` and is reachable from every
            // stub, so a crate's iterators keep it too.
            t.implements.retain(|n| interfaces.contains(n) || n == "RustIterator");
        }
    }
}

/// Build a [`StubFile`] from an already-parsed rustdoc [`Crate`].
///
/// Only public items of the local crate (`crate_id == 0`) are emitted, in a
/// deterministic (name-sorted) order so a stub regenerates identically.
pub fn generate(krate: &Crate, package: &str) -> StubFile {
    let mut collected = collect_items(krate);

    // Deterministic order: by item name.
    collected.sort_by(|a, b| a.0.cmp(&b.0));
    retain_declared_implements(&mut collected);
    retain_projection_overloads(&mut collected);

    StubFile {
        package: package.to_string(),
        header: vec![format!(
            "bindgen -- generated from rustdoc JSON (format_version {})",
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
    // The projection fan-out (§G.6.4.5) records the real Rust path of every
    // named type an overload's parameter mentions, so the late pass can check
    // that this stub's declaration of that name really is that type.
    let public = PublicPaths::of(krate);
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
            for mut sf in map_function_surface(krate, &public, mname, f, Some(&im.for_)) {
                sf.is_static = false;
                slot.push(sf);
            }
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

/// A key for one INSTANTIATION of a generic type: `Handle<SkPaint>` and
/// `Handle<SkCanvas>` are different types with different methods, so unlike
/// [`type_shape_key`] (which answers "what does this deref to?") the generic
/// arguments are part of the answer.
fn type_instance_key(t: &Type) -> Option<String> {
    let Type::ResolvedPath(p) = t else {
        return None;
    };
    // Keyed by resolved ID, never by the written path: an alias may write
    // `Handle<skia_bindings::SkPaint>` where the impl writes `Handle<SkPaint>`,
    // and those are the same type spelled two ways. The id is the identity.
    let mut key = p.id.0.to_string();
    if let Some(GenericArgs::AngleBracketed { args, .. }) = p.args.as_deref() {
        let rendered: Vec<String> = args
            .iter()
            .filter_map(|a| match a {
                GenericArg::Type(Type::ResolvedPath(q)) => Some(q.id.0.to_string()),
                GenericArg::Type(Type::Primitive(prim)) => Some(prim.clone()),
                _ => None,
            })
            .collect();
        if !rendered.is_empty() {
            key.push('<');
            key.push_str(&rendered.join(","));
            key.push('>');
        }
    }
    Some(key)
}

/// Inherent impl blocks grouped by the exact instantiation they are written
/// for, so `impl Handle<SkPaint>` can be found from `type Paint =
/// Handle<SkPaint>`.
fn inherent_impls_by_instance(
    krate: &Crate,
) -> std::collections::HashMap<String, Vec<rustdoc_types::Id>> {
    let mut out: std::collections::HashMap<String, Vec<rustdoc_types::Id>> =
        std::collections::HashMap::new();
    for item in krate.index.values() {
        let ItemEnum::Impl(im) = &item.inner else {
            continue;
        };
        if im.trait_.is_some() {
            continue;
        }
        let Some(key) = type_instance_key(&im.for_) else {
            continue;
        };
        out.entry(key).or_default().push(item.id);
    }
    out
}

/// Trait impls grouped the same way, so a handle alias can ask whether its
/// exact instantiation implements `Default` or `Clone`.
fn trait_impls_by_instance(
    krate: &Crate,
) -> std::collections::HashMap<String, Vec<rustdoc_types::Id>> {
    let mut out: std::collections::HashMap<String, Vec<rustdoc_types::Id>> =
        std::collections::HashMap::new();
    for item in krate.index.values() {
        let ItemEnum::Impl(im) = &item.inner else {
            continue;
        };
        if im.trait_.is_none() {
            continue;
        }
        let Some(key) = type_instance_key(&im.for_) else {
            continue;
        };
        out.entry(key).or_default().push(item.id);
    }
    out
}

/// A public `type Alias = Generic<Arg>` that has methods written for exactly
/// that instantiation becomes a class of its own.
///
/// The alias is the name the library means people to use, and it is a real
/// Rust path, so the emitted code can name it directly. Its own type
/// parameters are not substituted: an alias that still takes parameters is
/// left alone, since the methods would then belong to no single instantiation.
fn build_handle_alias(
    krate: &Crate,
    name: &str,
    alias: &rustdoc_types::TypeAlias,
    item: &Item,
    public: &PublicPaths,
    by_instance: &std::collections::HashMap<String, Vec<rustdoc_types::Id>>,
    traits_by_instance: &std::collections::HashMap<String, Vec<rustdoc_types::Id>>,
) -> Option<StubType> {
    if !alias.generics.params.is_empty() {
        return None;
    }
    let key = type_instance_key(&alias.type_)?;
    // A bare name is an ordinary alias to a type that already exists in the
    // stub under its own name; only an instantiation needs a class built.
    if !key.contains('<') {
        return None;
    }
    let impls = by_instance.get(&key)?;
    let (mut ctors, mut methods) = collect_inherent_members(krate, public, impls, name);
    let trait_impls: &[rustdoc_types::Id] =
        traits_by_instance.get(&key).map(|v| v.as_slice()).unwrap_or(&[]);
    add_default_ctor(&mut ctors, name, implements_trait(krate, trait_impls, "Default"));
    dedup_methods_by_name(&mut methods);
    if ctors.is_empty() && methods.is_empty() {
        return None;
    }
    let mut st = StubType::new(TypeKind::Class, name);
    st.constructors = ctors;
    st.methods = methods;
    st.doc = first_doc_line(item);
    st.rust_path = real_rust_path(krate, item, public);
    st.is_clone = implements_trait(krate, trait_impls, "Clone");
    Some(st)
}

fn collect_items_with(krate: &Crate, pool: &InherentPool) -> Vec<(String, StubItem)> {
    collect_items_with_ids(krate, pool)
        .into_iter()
        .map(|(_, name, item)| (name, item))
        .collect()
}

/// [`collect_items_with`], keeping each item's rustdoc id so the caller can
/// ask the crate where the item is DEFINED (`krate.paths`).
fn collect_items_with_ids(krate: &Crate, pool: &InherentPool) -> Vec<(u32, String, StubItem)> {
    // Every id that is a member of some impl or trait — used to tell a free
    // function (top-level `fn`) apart from a method/associated function.
    let member_ids = collect_member_ids(krate);

    // Both whole-crate scans, done once here and handed to every item that
    // needs a Rust path. They are deliberately NOT memoized across calls: a
    // crate is a short-lived local in `generate_merged_with_pool`, so any
    // cache keyed by its identity would hand the next crate this one's
    // answers.
    let public = PublicPaths::of(krate);
    // Inherent impls keyed by instantiation, for the handle-alias pattern.
    let by_instance = inherent_impls_by_instance(krate);
    let traits_by_instance = trait_impls_by_instance(krate);

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
                    StubItem::Type(build_struct(krate, name, s, item, pool, &public)),
                ));
            }
            ItemEnum::Enum(e) if is_public(&item.visibility) => {
                collected.push((
                    item.id.0,
                    name.clone(),
                    StubItem::Type(build_enum(krate, name, e, item, &public)),
                ));
            }
            ItemEnum::TypeAlias(alias) if is_public(&item.visibility) => {
                if let Some(st) =
                    build_handle_alias(
                        krate,
                        name,
                        alias,
                        item,
                        &public,
                        &by_instance,
                        &traits_by_instance,
                    )
                {
                    collected.push((item.id.0, name.clone(), StubItem::Type(st)));
                } else if alias.generics.params.is_empty() {
                    // `pub type scalar = f32;` -- a crate's own name for a
                    // primitive. Its signatures say `scalar`, and unless the
                    // stub says what that is, a `scalar` parameter is a type
                    // nobody knows is a float, so no conversion reaches it.
                    // The target may itself be an alias (`scalar = SkScalar`,
                    // which is `f32`); unresolvable chains are dropped once the
                    // whole stub is known, in `generate_merged_with_pool`.
                    let target = match map_type(&alias.type_) {
                        JuxType::Prim(prim) => Some(prim.to_string()),
                        JuxType::User { name: t, args } if args.is_empty() => Some(t),
                        _ => None,
                    };
                    if let Some(target) = target.filter(|t| t != name) {
                        collected.push((
                            item.id.0,
                            name.clone(),
                            StubItem::Alias(StubAlias { name: name.clone(), target }),
                        ));
                    }
                }
            }
            // The inherent methods of a Rust PRIMITIVE (Bindgen G.6.4.4). The
            // facade crate's primitive items gather the impls of `core`, `alloc`
            // and `std` in one place: `f64` has `powf` from std and `total_cmp`
            // from core. `str` is left out, since `String` already carries its
            // methods through `Deref`.
            ItemEnum::Primitive(prim) if primitive_has_jux_type(&prim.name) => {
                let class = format!("{}_methods", prim.name);
                let (ctors, mut methods) = collect_inherent_members(krate, &public, &prim.impls, &class);
                // A primitive has no constructor; `from_bits` and friends are
                // associated functions, called on the type.
                for c in ctors {
                    methods.push(StubFn {
                        visibility: Vis::Public,
                        is_static: true,
                        is_default: false,
                        name: c.name,
                        generics: Vec::new(),
                        params: c.params,
                        ret: JuxType::Prim(primitive_jux_name(&prim.name)),
                        throws: c.throws,
                        is_unsafe: false,
                        is_mut_self: false,
                        returns_borrow: false,
                        carries_borrow: false,
                        rust_path: None,
                        doc: None,
                        closure_ref_params: Vec::new(),
                        bounds: Vec::new(),
                        projection_role: None,
                    });
                }
                dedup_methods_by_name(&mut methods);
                let mut st = StubType::new(TypeKind::Class, &class);
                st.methods = methods;
                st.primitive = Some(prim.name.clone());
                st.rust_path = Some(prim.name.clone());
                collected.push((item.id.0, class, StubItem::Type(st)));
            }
            ItemEnum::Trait(t) if is_public(&item.visibility) => {
                collected.push((
                    item.id.0,
                    name.clone(),
                    StubItem::Type({
                        let mut st =
                            build_trait(krate, name, &t.generics, &t.items, item, &public);
                        let (blanket, by) = trait_impl_reach(krate, &t.implementations);
                        st.blanket_over = blanket;
                        st.implemented_by = by;
                        st
                    }),
                ));
            }
            ItemEnum::Function(f)
                if is_public(&item.visibility) && !member_ids.contains(&item.id.0) =>
            {
                // Free function (§G.5.5). Record its real Rust path so the
                // backend can `use` the fully-qualified Rust path under the
                // (verbatim, snake_case) Jux stub name on import.
                let mut sf = map_function(krate, name, f);
                sf.is_static = false;
                sf.rust_path = real_rust_path(krate, item, &public);
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
                        rust_path: real_rust_path(krate, item, &public),
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
                        rust_path: real_rust_path(krate, item, &public),
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
}

/// A collected item's recorded Rust path, or the empty string when it has none
/// — the sort key that makes duplicate-name selection reproducible.
fn stub_item_path(item: &StubItem) -> &str {
    match item {
        StubItem::Type(t) => t.rust_path.as_deref().unwrap_or(""),
        StubItem::Function(f) => f.rust_path.as_deref().unwrap_or(""),
        StubItem::Const(_) | StubItem::Alias(_) => "",
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
    public: &PublicPaths,
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

    let (mut ctors, mut methods) = collect_inherent_members(krate, public, &s.impls, name);
    add_default_ctor(&mut ctors, name, implements_trait(krate, &s.impls, "Default"));
    // Rust's method resolution follows `Deref`, so `Vec<T>` really does have
    // every `[T]` method — and a stub that stops at the inherent impls is
    // simply an incomplete description of the type. `Vec` came out with 58
    // methods and without `sort`, `get`, `iter`, `first`, `last`, `contains`
    // or `reverse`, which is why the backend had grown hardcoded branches for
    // some of them: the scan was not telling it the truth.
    methods.extend(deref_members(krate, &s.impls, pool));
    methods.extend(iterator_next(krate, &s.impls));
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
    st.rust_path = real_rust_path(krate, item, public);
    st.index_ref = has_ref_index_impl(krate, &s.impls);
    st.is_clone = implements_trait(krate, &s.impls, "Clone");
    st.owned_as = owned_counterpart(krate, &s.impls, name);
    st.derefs_to = deref_target(krate, &s.impls).as_ref().and_then(shape_name);
    st.is_collection = implements_collection_trait(krate, item.id, &s.impls);
    st.implements = implemented_trait_names(krate, item.id, &s.impls);
    st
}

fn build_enum(
    krate: &Crate,
    name: &str,
    e: &Enum,
    item: &Item,
    public: &PublicPaths,
) -> StubType {
    let mut st = StubType::new(TypeKind::Enum, name);
    st.generics = generic_param_names(&e.generics);
    st.doc = first_doc_line(item);
    st.rust_path = real_rust_path(krate, item, public);
    st.implements = implemented_trait_names(krate, item.id, &e.impls);
    st.is_clone = implements_trait(krate, &e.impls, "Clone");
    // An enum carries behaviour in its inherent impls exactly as a struct
    // does, and a Jux enum can hold methods too (§7.7). Keeping only the
    // variants described `image::DynamicImage` as a bare tag union, so
    // `decoded.width()` was an unknown method on a type that plainly has one.
    let (mut ctors, mut methods) = collect_inherent_members(krate, public, &e.impls, name);
    add_default_ctor(&mut ctors, name, implements_trait(krate, &e.impls, "Default"));
    dedup_methods_by_name(&mut methods);
    st.constructors = ctors;
    st.methods = methods;
    let has_members = !st.methods.is_empty() || !st.constructors.is_empty();

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
    // A Jux enum body cannot be empty (§A.2.5), so an enum whose variants did
    // not survive mapping is spelled as a class: an opaque handle carrying its
    // methods, which is all any call site does with it. Emitted as an enum it
    // was a parse error, and a stub that does not parse takes its whole crate
    // surface down with it (G.12).
    if st.variants.is_empty() && has_members {
        st.kind = TypeKind::Class;
    }
    st
}

fn build_trait(
    krate: &Crate,
    name: &str,
    generics: &Generics,
    item_ids: &[rustdoc_types::Id],
    item: &Item,
    public: &PublicPaths,
) -> StubType {
    // A Rust trait becomes a Jux interface; provided methods (with a body)
    // become `default` methods (§G.6.4).
    let mut st = StubType::new(TypeKind::Interface, name);
    st.generics = generic_param_names(generics);
    st.doc = first_doc_line(item);
    st.rust_path = real_rust_path(krate, item, public);

    for mid in item_ids {
        let Some(mitem) = krate.index.get(mid) else {
            continue;
        };
        let Some(mname) = &mitem.name else { continue };
        if let ItemEnum::Function(f) = &mitem.inner {
            let mut sf = map_function(krate, mname, f);
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
    // A §G.6.4.5 fan-out RESOLVED a name that some other crate of the same
    // ingest also declares plainly. `core` holds the `SliceIndex` trait item,
    // so only `core`'s copy of `impl str` can resolve `str::get`; `alloc` and
    // `std` document the same method with the trait out of reach and surface
    // the unresolved `I.Output`. Both land in one shape-keyed pool, and
    // first-wins would keep whichever crate happened to be read first. The
    // resolved reading is strictly the better description, so it wins outright.
    let resolved: HashSet<String> = methods
        .iter()
        .filter(|m| matches!(m.projection_role, Some(ProjectionRole::Overload { .. })))
        .map(|m| m.name.clone())
        .collect();
    methods.retain(|m| m.projection_role.is_some() || !resolved.contains(&m.name));

    let mut seen: HashSet<String> = HashSet::new();
    methods.retain(|m| {
        let key = match &m.projection_role {
            // A fan-out group is several declarations of ONE Rust method, told
            // apart by their parameter types the way any Jux overload group is
            // (§T.3.1). Keying those on the name alone would keep one arbitrary
            // member and throw the rest of the group away, which is the whole
            // point of having fanned it out. They still dedup against each
            // other, so a method reached twice does not double its group.
            Some(ProjectionRole::Overload { .. }) => format!(
                "{}#{}",
                m.name,
                m.params.iter().map(|p| p.ty.to_string()).collect::<Vec<_>>().join(","),
            ),
            // The fallback shares the plain name's slot, because it IS the
            // plain declaration: it is kept only until the late pass finds out
            // whether any overload of the group can be named at all.
            None | Some(ProjectionRole::Fallback) => m.name.clone(),
        };
        seen.insert(key)
    });
}

/// Replace every `Self` in a mapped type with the type it stands for.
///
/// `Self` is written inside an `impl`; a stub file has no impl, so the name
/// has to be resolved while the owner is still known. It nests
/// (`Self?`, `Vec<Self>`, `(Self) -> Self`), so the walk is recursive.
fn substitute_self(ty: &JuxType, owner: &str) -> JuxType {
    match ty {
        JuxType::User { name, args } if name == "Self" && args.is_empty() => JuxType::user(owner),
        JuxType::User { name, args } => JuxType::User {
            name: name.clone(),
            args: args.iter().map(|a| substitute_self(a, owner)).collect(),
        },
        JuxType::Nullable(inner) => JuxType::Nullable(Box::new(substitute_self(inner, owner))),
        JuxType::Array { elem, size } => JuxType::Array {
            elem: Box::new(substitute_self(elem, owner)),
            size: *size,
        },
        JuxType::Tuple(items) => {
            JuxType::Tuple(items.iter().map(|t| substitute_self(t, owner)).collect())
        }
        JuxType::Fn { params, ret, is_async } => JuxType::Fn {
            params: params.iter().map(|t| substitute_self(t, owner)).collect(),
            ret: Box::new(substitute_self(ret, owner)),
            is_async: *is_async,
        },
        JuxType::RawPtr(inner) => JuxType::RawPtr(Box::new(substitute_self(inner, owner))),
        other => other.clone(),
    }
}

/// Apply [`substitute_self`] across a method's whole signature.
fn substitute_self_in_fn(f: &mut StubFn, owner: &str) {
    f.ret = substitute_self(&f.ret, owner);
    if let Some(t) = &f.throws {
        f.throws = Some(substitute_self(t, owner));
    }
    for p in &mut f.params {
        p.ty = substitute_self(&p.ty, owner);
    }
}

/// Declare the zero-arg constructor an `impl Default` implies, unless the type
/// already has a zero-arg one of its own.
fn add_default_ctor(ctors: &mut Vec<StubCtor>, type_name: &str, implements_default: bool) {
    if !implements_default || ctors.iter().any(|c| c.params.is_empty()) {
        return;
    }
    ctors.push(StubCtor {
        visibility: Vis::Public,
        name: type_name.to_string(),
        params: Vec::new(),
        throws: None,
        is_default: true,
    });
}

/// Collect constructors and methods from a type's **inherent** impl blocks.
/// `new()` (no receiver) maps to a constructor (§G.5.1); other associated
/// functions without a receiver map to static methods (§G.5.2); functions with
/// a `self` receiver map to instance methods (§G.5.3).
fn collect_inherent_members(
    krate: &Crate,
    public: &PublicPaths,
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
            // A `new() -> Option<Self>` is not a constructor. A Jux
            // constructor always produces the object, and there is no error
            // value here to throw either, so the only faithful surface is a
            // static that returns `Self?` (§G.5.5). Rendered as a ctor, the
            // `None` case disappeared and the emitted Rust handed an
            // `Option<Pixmap>` to something expecting a `Pixmap`.
            let (ret, throws) = map_return(krate, &f.sig.output);
            let fallible_new = matches!(ret, JuxType::Nullable(_));
            if mname == "new" && !has_self && !fallible_new {
                // A `new() -> Result<Self, E>` surfaces as a `throws E` ctor so
                // the call site unwraps the `Result` (§G.5.4).
                ctors.push(StubCtor {
                    is_default: false,
                    visibility: Vis::Public,
                    name: type_name.to_string(),
                    params: map_params(f),
                    throws,
                });
            } else {
                for mut sf in map_function_surface(krate, public, mname, f, Some(&im.for_)) {
                    sf.is_static = !has_self;
                    methods.push(sf);
                }
            }
        }
    }
    // `Self` is the type these members were written for.
    for c in &mut ctors {
        for p in &mut c.params {
            p.ty = substitute_self(&p.ty, type_name);
        }
    }
    for m in &mut methods {
        substitute_self_in_fn(m, type_name);
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

/// The name a type's shape is written with in `@RustDerefs` /
/// `@RustImplementedBy`: `[]` for a slice, the primitive's name (`str`), or a
/// named type's last segment.
fn shape_name(t: &Type) -> Option<String> {
    match t {
        Type::Slice(_) => Some("[]".to_string()),
        Type::Primitive(p) => Some(p.clone()),
        Type::ResolvedPath(p) => Some(last_segment(&p.path).to_string()),
        _ => None,
    }
}

/// How far a trait's impls reach beyond the types that name it
/// (Bindgen G.6.4.3), from the impls rustdoc lists for the trait:
///
/// - a BLANKET impl over a type parameter bounded by one trait
///   (`impl<R: RngCore + ?Sized> Rng for R`) gives that bound;
/// - an impl for a slice or a primitive (`impl<T> SliceRandom for [T]`,
///   `impl UnicodeSegmentation for str`) gives that shape.
fn trait_impl_reach(krate: &Crate, impls: &[rustdoc_types::Id]) -> (Vec<String>, Vec<String>) {
    let mut blanket: Vec<String> = Vec::new();
    let mut shapes: Vec<String> = Vec::new();
    for id in impls {
        let Some(item) = krate.index.get(id) else { continue };
        let ItemEnum::Impl(im) = &item.inner else { continue };
        if im.is_synthetic || im.is_negative {
            continue;
        }
        match &im.for_ {
            Type::Generic(param) => {
                let mut bounds: Vec<String> = Vec::new();
                let mut take = |bs: &[GenericBound]| {
                    for b in bs {
                        if let GenericBound::TraitBound { trait_, modifier, .. } = b {
                            let name = last_segment(&trait_.path);
                            if !matches!(modifier, rustdoc_types::TraitBoundModifier::Maybe)
                                && name != "Sized"
                            {
                                bounds.push(name.to_string());
                            }
                        }
                    }
                };
                for gp in &im.generics.params {
                    if &gp.name == param {
                        if let GenericParamDefKind::Type { bounds: bs, .. } = &gp.kind {
                            take(bs);
                        }
                    }
                }
                for wp in &im.generics.where_predicates {
                    if let WherePredicate::BoundPredicate { type_: Type::Generic(g), bounds: bs, .. } = wp {
                        if g == param {
                            take(bs);
                        }
                    }
                }
                if let [only] = bounds.as_slice() {
                    blanket.push(only.clone());
                }
            }
            Type::Slice(_) | Type::Primitive(_) => {
                if let Some(shape) = shape_name(&im.for_) {
                    shapes.push(shape);
                }
            }
            _ => {}
        }
    }
    blanket.sort();
    blanket.dedup();
    shapes.sort();
    shapes.dedup();
    (blanket, shapes)
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

pub(crate) fn map_function(krate: &Crate, name: &str, f: &Function) -> StubFn {
    let (ret, throws) = map_return(krate, &f.sig.output);
    let closure_ref_params = f
        .sig
        .inputs
        .iter()
        .filter(|(n, _)| n != "self")
        .enumerate()
        .filter(|(_, (_, ty))| closure_takes_refs(ty, &f.generics))
        .map(|(i, _)| i)
        .collect();
    StubFn {
        closure_ref_params,
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
        carries_borrow: has_self_receiver(f)
            && f.sig.output.as_ref().is_some_and(carries_receiver_lifetime),
        // Set by the free-function call site (which has the rustdoc item); a
        // method leaves this `None` (it's dispatched on its `@rust`-pathed type).
        rust_path: None,
        doc: None,
        bounds: type_param_bounds(&f.generics),
        projection_role: None,
    }
}

/// The trait bounds a method puts on its TYPE's parameters, written
/// `T: Ord`: `sort` on `[T]` says `where T: Ord`. A bound on the method's own
/// generics, on `Self`, or a closure/`Sized` bound is not one: those are
/// settled at the call (Bindgen G.6.4.4).
fn type_param_bounds(g: &Generics) -> Vec<String> {
    let own: HashSet<&str> = g.params.iter().map(|p| p.name.as_str()).collect();
    let mut out: Vec<String> = Vec::new();
    for wp in &g.where_predicates {
        let WherePredicate::BoundPredicate { type_: Type::Generic(param), bounds, .. } = wp else {
            continue;
        };
        if param == "Self" || own.contains(param.as_str()) {
            continue;
        }
        for b in bounds {
            let GenericBound::TraitBound { trait_, modifier, .. } = b else { continue };
            let name = last_segment(&trait_.path);
            if matches!(modifier, rustdoc_types::TraitBoundModifier::Maybe)
                || matches!(name, "Sized" | "Fn" | "FnMut" | "FnOnce")
            {
                continue;
            }
            let bound = format!("{param}: {name}");
            if !out.contains(&bound) {
                out.push(bound);
            }
        }
    }
    out
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
            by_mut_ref: is_mut_borrow_param(ty),
        })
        .collect()
}

/// Is this Rust parameter a MUTABLE borrow -- `&mut T` or `&mut [T]`?
///
/// The slice case matters as much as the scalar one: `Read::read`'s buffer is
/// `&mut [u8]`, which [`is_borrow_param`] deliberately does not count as a
/// borrow (a Jux array reaches a slice on its own), but the call site still has
/// to lend it mutably.
fn is_mut_borrow_param(ty: &Type) -> bool {
    matches!(ty, Type::BorrowedRef { is_mutable: true, .. })
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

/// Whether a closure-typed Rust parameter takes any argument by reference:
/// `P: FnMut(&Self::Item) -> bool`, or `impl Fn(&T)`.
fn closure_takes_refs(ty: &Type, generics: &Generics) -> bool {
    let takes_refs = |bounds: &[GenericBound]| {
        bounds.iter().any(|b| {
            let GenericBound::TraitBound { trait_, .. } = b else { return false };
            if !matches!(last_segment(&trait_.path), "Fn" | "FnMut" | "FnOnce") {
                return false;
            }
            matches!(
                trait_.args.as_deref(),
                Some(GenericArgs::Parenthesized { inputs, .. })
                    if inputs.iter().any(|t| matches!(t, Type::BorrowedRef { .. }))
            )
        })
    };
    match ty {
        Type::ImplTrait(bounds) => takes_refs(bounds),
        Type::Generic(name) => {
            generics.params.iter().any(|p| {
                &p.name == name
                    && matches!(&p.kind, GenericParamDefKind::Type { bounds, .. } if takes_refs(bounds))
            }) || generics.where_predicates.iter().any(|w| {
                matches!(w, WherePredicate::BoundPredicate { type_: Type::Generic(n), bounds, .. }
                    if n == name && takes_refs(bounds))
            })
        }
        _ => false,
    }
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

/// Whether a return type holds a borrow without being a reference: a type
/// instantiated with a non-`'static` lifetime (`Pixmap<'_>`), directly or as
/// the value of an `Option` / `Result`. With a `self` receiver, lifetime
/// elision ties that lifetime to the receiver.
fn carries_receiver_lifetime(ty: &Type) -> bool {
    let Type::ResolvedPath(p) = ty else { return false };
    let Some(GenericArgs::AngleBracketed { args, .. }) = p.args.as_deref() else {
        return false;
    };
    args.iter().any(|a| match a {
        GenericArg::Lifetime(l) => l != "'static",
        GenericArg::Type(inner) => carries_receiver_lifetime(inner),
        _ => false,
    })
}

fn is_borrow_param(ty: &Type) -> bool {
    matches!(ty, Type::BorrowedRef { type_, .. } if !matches!(type_.as_ref(), Type::Slice(_)))
}

/// `Result<T, E>` in return position becomes `T throws E` (§G.5.4); `Option<T>`
/// and everything else map through [`map_type`].
fn map_return(krate: &Crate, output: &Option<Type>) -> (JuxType, Option<JuxType>) {
    match output {
        None => (JuxType::Void, None),
        Some(Type::ResolvedPath(p)) if resolved_name(krate, p) == "Result" => {
            let args = collect_type_args(&p.args);
            let ok = args.first().cloned().unwrap_or(JuxType::Void);
            // A 2-arg `Result<T, E>` carries the real error type. A 1-arg crate
            // alias `Result<T>` (= `Result<T, CrateError>`, e.g. `minifb::Result`)
            // hides it but is still fallible, so record an opaque `Error` — the
            // call site unwraps either way (the backend ignores the error type;
            // only its presence drives the `throws` / unwrap, §G.5.4).
            // `Result<T, ()>` is the third shape: fallible, with an error that
            // carries nothing. Its argument maps to `Void`, and `throws void`
            // is not a type -- it does not parse, and a stub that does not
            // parse takes its whole crate surface down with it. The opaque
            // `Error` says the same thing the unit error says.
            // A `throws` clause names a TYPE, so an error that has no name to
            // write -- a tuple, a slice, a function -- becomes the opaque
            // `Error` like the unit and the one-argument alias. `throws (T, T)`
            // does not parse, and an unparsable line takes the whole stub with
            // it (G.12).
            let err = args
                .get(1)
                .filter(|t| matches!(t, JuxType::User { .. } | JuxType::Param(_)))
                .cloned()
                .or_else(|| Some(JuxType::user("Error")));
            (ok, err)
        }
        // A crate's OWN alias for `Result` is still a `Result`:
        // `pub type ImageResult<T> = Result<T, ImageError>` hides the
        // fallibility behind a name, and `resolved_name` cannot see through it
        // (the alias IS the resolved item). Unfollowed, every fallible call in
        // `image` returned a raw `Result` object Jux has no syntax to open.
        Some(Type::ResolvedPath(p)) => {
            if let Some((ok, err)) = result_behind_alias(krate, p) {
                return (ok, Some(err));
            }
            (map_type(&Type::ResolvedPath(p.clone())), None)
        }
        Some(t) => (map_type(t), None),
    }
}

/// The `(ok, error)` pair behind a crate-local alias for `Result`, if `p`
/// names one.
///
/// The alias's own type parameters are substituted with the arguments written
/// at the use site, so `ImageResult<DynamicImage>` yields `DynamicImage` and
/// `ImageError`. Only one level is followed: an alias of an alias is rare, and
/// each extra level is another place to get the substitution wrong.
fn result_behind_alias(krate: &Crate, p: &rustdoc_types::Path) -> Option<(JuxType, JuxType)> {
    let item = krate.index.get(&p.id)?;
    let ItemEnum::TypeAlias(alias) = &item.inner else {
        return None;
    };
    let Type::ResolvedPath(target) = &alias.type_ else {
        return None;
    };
    if resolved_name(krate, target) != "Result" {
        return None;
    }

    // The alias's parameters, in order, bound to the arguments written here.
    let params: Vec<&str> = alias
        .generics
        .params
        .iter()
        .map(|gp| gp.name.as_str())
        .collect();
    let supplied = collect_type_args(&p.args);
    let resolve = |t: &JuxType| -> JuxType {
        if let JuxType::Param(name) = t {
            if let Some(i) = params.iter().position(|pn| pn == name) {
                if let Some(actual) = supplied.get(i) {
                    return actual.clone();
                }
            }
        }
        t.clone()
    };

    let args = collect_type_args(&target.args);
    let ok = args.first().map(&resolve).unwrap_or(JuxType::Void);
    // Same rule as a written `Result`: an error with no name to write becomes
    // the opaque `Error`, because `throws` names a type.
    let err = args
        .get(1)
        .map(&resolve)
        .filter(|t| matches!(t, JuxType::User { .. } | JuxType::Param(_)))
        .unwrap_or_else(|| JuxType::user("Error"));
    Some((ok, err))
}

/// What a written path RESOLVES to, by its final segment.
///
/// rustdoc records a path as the author spelled it, so an import alias
/// (`use std::io::Result as IoResult;`) or a re-export hides what the type
/// actually is. The `paths` summary carries the resolved item, and that is the
/// name any "which type is this?" test wants: `IoResult<Request>` is a
/// `Result`, and a method returning one is fallible.
///
/// Falls back to the written spelling when the id is not in the summary --
/// a generic parameter, or an item from a crate rustdoc did not index.
fn resolved_name(krate: &Crate, p: &rustdoc_types::Path) -> String {
    krate
        .paths
        .get(&p.id)
        .and_then(|summary| summary.path.last())
        .cloned()
        .unwrap_or_else(|| last_segment(&p.path).to_string())
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
/// The traits `own` implements that the stub can NAME, sorted.
///
/// A trait's methods live on the trait, so without this a foreign type has
/// none of them: `TcpStream::read` is a `std::io::Read` member, and the stub
/// that did not say `implements Read` made every socket unreadable.
///
/// Three restrictions, each because the stub could not write the result
/// otherwise:
///
/// * the trait must be PUBLIC and LOCAL to this crate, because that is exactly
///   the set the stub emits as interfaces -- a name it never declares would not
///   resolve;
/// * the trait must carry no generic PARAMETERS, since `implements Extend`
///   would need a type argument rustdoc records per-impl and the Jux clause has
///   nowhere to put;
/// * the impl must be written for this type in its own plain generic form, the
///   same restriction [`implements_collection_trait`] makes and for the same
///   reason.
///
/// `Clone`, `Index` and the collection traits are deliberately still read
/// through their own markers (`@RustClone`, `@RustIndexRef`,
/// `@RustCollection`): Jux gives them language meaning rather than a method
/// surface.
fn implemented_trait_names(krate: &Crate, own: Id, impls: &[rustdoc_types::Id]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in impls {
        let Some(item) = krate.index.get(id) else {
            continue;
        };
        let ItemEnum::Impl(im) = &item.inner else {
            continue;
        };
        if im.is_synthetic || im.is_negative {
            continue;
        }
        let Some(tr) = &im.trait_ else { continue };
        if !impl_target_is_the_plain_type(own, &im.for_) {
            continue;
        }
        let Some(decl) = krate.index.get(&tr.id) else {
            // A trait from ANOTHER crate (`rand_core::SeedableRng` on a
            // `rand_pcg` generator). Its declaration is not in this JSON, so
            // it is taken by name when the impl names it without type
            // arguments; the finished stub keeps it only if it declares an
            // interface of that name (`retain_declared_implements`).
            let generic = matches!(
                tr.args.as_deref(),
                Some(GenericArgs::AngleBracketed { args, .. }) if !args.is_empty()
            );
            if !generic {
                out.push(stub_trait_name(last_segment(&tr.path)).to_string());
            }
            continue;
        };
        let ItemEnum::Trait(t) = &decl.inner else {
            continue;
        };
        if decl.crate_id != 0 || !is_public(&decl.visibility) {
            continue;
        }
        if t.generics
            .params
            .iter()
            .any(|p| matches!(p.kind, GenericParamDefKind::Type { .. }))
        {
            continue;
        }
        let Some(name) = &decl.name else { continue };
        out.push(stub_trait_name(name).to_string());
    }
    out.sort();
    out.dedup();
    out
}

/// The name a Rust trait is declared under in a stub. Rust's `Iterator` is
/// `RustIterator`: Jux's own `Iterator<T>` is the K.5 protocol a program writes
/// (`jux.std.collections.Iterator`), and two interfaces of one name made every
/// by-name lookup a guess between them. Every other trait keeps its name.
fn stub_trait_name(name: &str) -> &str {
    if name == "Iterator" {
        "RustIterator"
    } else {
        name
    }
}

/// Is this type a COLLECTION -- something you can build from elements and add
/// more to?
///
/// `Extend` and `FromIterator` are Rust's own answer, and every std collection
/// implements both. The impl must be written for the type in its OWN generic
/// form, though: `impl<T> FromIterator<T> for Box<[T]>` is an impl on a boxed
/// SLICE, and counting it made `Box`, `Rc` and `Arc` collections. So a type
/// argument that is anything but a bare type parameter disqualifies the impl.
fn implements_collection_trait(krate: &Crate, own: Id, impls: &[rustdoc_types::Id]) -> bool {
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
        let named = im
            .trait_
            .as_ref()
            .is_some_and(|tr| matches!(last_segment(&tr.path), "Extend" | "FromIterator"));
        named && impl_target_is_the_plain_type(own, &im.for_)
    })
}

/// True when `ty` is `own` applied to nothing but bare type parameters --
/// `Vec<T>`, `HashMap<K, V, S>`, `String` -- rather than a specialization such
/// as `Box<[T]>`, and not some other type entirely.
///
/// A type's `impls` list is not confined to impls written for it: `Box`'s holds
/// `impl FromIterator<..> for String`, and taking that at face value made
/// `Box` a collection.
fn impl_target_is_the_plain_type(own: Id, ty: &Type) -> bool {
    let Type::ResolvedPath(p) = ty else {
        return false;
    };
    if p.id != own {
        return false;
    }
    match p.args.as_deref() {
        None => true,
        Some(GenericArgs::AngleBracketed { args, .. }) => args.iter().all(|a| match a {
            GenericArg::Type(Type::Generic(_)) => true,
            // Lifetimes and const arguments say nothing about the shape.
            GenericArg::Lifetime(_) | GenericArg::Const(_) => true,
            _ => false,
        }),
        Some(_) => false,
    }
}

/// `next()` for a type that implements Rust's `Iterator`, from the impl's
/// `type Item = ...` binding (Bindgen G.6.4.2).
///
/// `Iterator` is a `core` trait, so its methods are not part of the type's
/// surface otherwise, and the item type of `path.components()` or
/// `read_dir(dir)` was unknown: a for-each over one bound an untyped
/// variable. `next()` is the one method that carries the element type, and
/// it is what K.5 makes the iteration protocol, so it is the one surfaced.
///
/// An item that is itself a `Result` (`read_dir` yields `io::Result<DirEntry>`)
/// is a `next()` that throws, like every other `Result` from Rust (G.5.4), so
/// the element a for-each binds is the `DirEntry` (B28).
fn iterator_next(krate: &Crate, impls: &[rustdoc_types::Id]) -> Option<StubFn> {
    let item = impls.iter().find_map(|id| {
        let it = krate.index.get(id)?;
        let ItemEnum::Impl(im) = &it.inner else { return None };
        if im.is_synthetic || im.is_negative || im.blanket_impl.is_some() {
            return None;
        }
        if !im.trait_.as_ref().is_some_and(|tr| last_segment(&tr.path) == "Iterator") {
            return None;
        }
        im.items.iter().find_map(|aid| {
            let a = krate.index.get(aid)?;
            if a.name.as_deref() != Some("Item") {
                return None;
            }
            match &a.inner {
                ItemEnum::AssocType { type_: Some(t), .. } => Some(t.clone()),
                _ => None,
            }
        })
    })?;
    // An iterator over BORROWED items (`slice::Iter<T>` yields `&T`) is
    // marked `@RustRefOut` on its `next()`: Jux has no references, so the
    // program sees owned elements, and the backend takes them with
    // `.cloned()` where the iterator is made.
    let borrowed = matches!(item, Type::BorrowedRef { .. });
    let (elem, throws) = map_return(krate, &Some(item));
    Some(StubFn {
        visibility: Vis::Public,
        is_static: false,
        is_default: false,
        name: "next".to_string(),
        generics: Vec::new(),
        params: Vec::new(),
        ret: JuxType::nullable(elem),
        throws,
        is_unsafe: false,
        is_mut_self: true,
        returns_borrow: borrowed,
        carries_borrow: false,
        rust_path: None,
        doc: None,
        closure_ref_params: Vec::new(),
        bounds: Vec::new(),
        projection_role: None,
    })
}

/// The `Owned` type of the type's OWN `ToOwned` impl, when it names another
/// type: `impl ToOwned for Path { type Owned = PathBuf; }` gives `PathBuf`.
///
/// Only a type written for itself counts. The blanket
/// `impl<T: Clone> ToOwned for T` names `T` again and says nothing, and the
/// types that have their own impl are exactly the unsized views (`Path`,
/// `OsStr`, `CStr`) whose values need an owned home.
fn owned_counterpart(krate: &Crate, impls: &[rustdoc_types::Id], name: &str) -> Option<String> {
    for id in impls {
        let Some(item) = krate.index.get(id) else { continue };
        let ItemEnum::Impl(im) = &item.inner else { continue };
        if im.is_synthetic || im.is_negative || im.blanket_impl.is_some() {
            continue;
        }
        if !im.trait_.as_ref().is_some_and(|tr| last_segment(&tr.path) == "ToOwned") {
            continue;
        }
        for aid in &im.items {
            let Some(aitem) = krate.index.get(aid) else { continue };
            if aitem.name.as_deref() != Some("Owned") {
                continue;
            }
            if let ItemEnum::AssocType { type_: Some(Type::ResolvedPath(p)), .. } = &aitem.inner {
                let owned = last_segment(&p.path).to_string();
                if owned != name {
                    return Some(owned);
                }
            }
        }
    }
    None
}

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
// Associated-type projections over a method's own parameter (§G.6.4.5)
// ============================================================================

/// The most overloads one projection fan-out may contribute to a method.
///
/// The fan-out keeps one overload per distinct RESULT type (see
/// [`settle_projection_group`]), which is already a small number: `SliceIndex`
/// has seventeen impls for `[T]` and exactly two results, the element and a
/// sub-slice. The cap is the backstop for a trait whose associated type varies
/// more widely, so that one method can never multiply a surface every single
/// compile parses (§G.6.4.5).
const PROJECTION_FANOUT_CAP: usize = 4;

/// A `<P as Trait<...>>::Assoc` written in a signature, where `P` is one of the
/// METHOD's own type parameters.
///
/// `Vec::get` is `fn get<I>(&self, index: I) -> Option<&I::Output> where I:
/// SliceIndex<[T]>`: `I` is settled by the argument the call passes, so the
/// projection is knowable once the argument type is, unlike a `Self::Item` the
/// receiver settles (§G.6.4.2).
struct MethodProjection {
    /// The method type parameter the projection is taken over (`I`).
    param: String,
    /// The associated type's name (`Output`).
    assoc: String,
    /// The projection's trait, by rustdoc ID. The ID is what is matched, never
    /// the name: rustdoc writes a projection's `trait_.path` as the empty
    /// string and carries only the id, and matching on a name would be the
    /// hardcoded trait list §G.6.1 forbids anyway.
    trait_id: Id,
}

/// One impl of the projection's trait, resolved against the method's bound.
struct ResolvedImpl {
    /// The impl's self type, substituted: the Jux parameter type.
    index: JuxType,
    /// The impl's `type Assoc = ...` binding, substituted: the Jux result.
    result: JuxType,
    /// Every named type the parameter mentions, as `(Jux name, real Rust
    /// path)`. [`settle_projection_group`] checks that the stub's declaration
    /// of each name really is that Rust type before keeping the overload:
    /// `rust.std` declares `Range` for `std::collections::btree_map::Range`, so
    /// an overload taking `core::ops::Range` must not be written `Range` there.
    param_paths: Vec<(String, String)>,
    /// How complicated the parameter's shape is, [`jux_shape_rank`]. Several
    /// impls of one trait often give the same result from different index
    /// types, and the simplest of those is the one a program can write.
    param_rank: u8,
}

/// How complicated a Jux type's shape is, as a tie-break between impls that
/// resolve to the SAME result (§G.6.4.5).
///
/// `SliceIndex<[T]>` gives the element back for `usize`, for the internal
/// `Last`, and for `Clamp<usize>`. All three say the same thing about the
/// result, so the one the fan-out keeps should be the one a program would
/// actually write, and a bare primitive beats a generic wrapper.
fn jux_shape_rank(t: &JuxType) -> u8 {
    match t {
        JuxType::Prim(_) | JuxType::String => 0,
        JuxType::User { args, .. } if args.is_empty() => 1,
        JuxType::User { .. } | JuxType::Array { .. } => 2,
        JuxType::Tuple(_) => 3,
        _ => 4,
    }
}

/// The single projection over one of `f`'s own type parameters, if there is
/// exactly one.
///
/// Exactly one, because the fan-out swaps one parameter for one impl self type:
/// two projections over two different parameters would need the cross product
/// of two impl lists, and no surface we ingest asks for that. Several
/// occurrences of the SAME projection are fine and common, a method that takes
/// and returns `I::Output` being the usual shape.
fn method_projection(f: &Function) -> Option<MethodProjection> {
    let own: HashSet<&str> = f.generics.params.iter().map(|p| p.name.as_str()).collect();
    let mut found: Option<MethodProjection> = None;
    let mut single = true;
    let mut visit = |t: &Type| {
        let Type::QualifiedPath { name, self_type, trait_, .. } = t else { return };
        let Type::Generic(owner) = self_type.as_ref() else { return };
        if !own.contains(owner.as_str()) {
            return;
        }
        let Some(tr) = trait_ else { return };
        match &found {
            Some(prev) if prev.param != *owner || prev.assoc != *name => single = false,
            Some(_) => {}
            None => {
                found = Some(MethodProjection {
                    param: owner.clone(),
                    assoc: name.clone(),
                    trait_id: tr.id,
                })
            }
        }
    };
    for (_, ty) in &f.sig.inputs {
        walk_rust_type(ty, &mut visit);
    }
    if let Some(out) = &f.sig.output {
        walk_rust_type(out, &mut visit);
    }
    if single { found } else { None }
}

/// Call `visit` on `t` and on every type nested inside it.
fn walk_rust_type(t: &Type, visit: &mut impl FnMut(&Type)) {
    visit(t);
    match t {
        Type::ResolvedPath(p) => {
            for a in path_type_args(&p.args) {
                walk_rust_type(a, visit);
            }
        }
        Type::Slice(inner)
        | Type::BorrowedRef { type_: inner, .. }
        | Type::RawPointer { type_: inner, .. }
        | Type::Array { type_: inner, .. } => walk_rust_type(inner, visit),
        Type::Tuple(ts) => ts.iter().for_each(|t| walk_rust_type(t, visit)),
        Type::QualifiedPath { self_type, .. } => walk_rust_type(self_type, visit),
        Type::FunctionPointer(fp) => {
            for (_, t) in &fp.sig.inputs {
                walk_rust_type(t, visit);
            }
            if let Some(o) = &fp.sig.output {
                walk_rust_type(o, visit);
            }
        }
        _ => {}
    }
}

/// The TYPE arguments of an angle-bracketed generic list, by reference.
fn path_type_args(args: &Option<Box<GenericArgs>>) -> Vec<&Type> {
    match args.as_deref() {
        Some(GenericArgs::AngleBracketed { args, .. }) => args
            .iter()
            .filter_map(|a| match a {
                GenericArg::Type(t) => Some(t),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The trait arguments the method's own bound puts on `proj.param`
/// (`where I: SliceIndex<Self>` gives `[Self]`), with `Self` replaced by the
/// type the enclosing impl is written for.
///
/// `Self` has to be substituted here and not later: the bound is written inside
/// `impl<T> [T]`, so its `Self` IS `[T]`, and matching that against
/// `impl<T> SliceIndex<[T]> for usize` is the whole reason the slice's impls
/// are told apart from `impl SliceIndex<str> for Range<usize>`.
fn projection_bound_args(
    f: &Function,
    proj: &MethodProjection,
    self_ty: Option<&Type>,
) -> Option<Vec<Type>> {
    let take = |bounds: &[GenericBound]| -> Option<Vec<Type>> {
        for b in bounds {
            let GenericBound::TraitBound { trait_, .. } = b else { continue };
            if trait_.id != proj.trait_id {
                continue;
            }
            return Some(
                path_type_args(&trait_.args)
                    .into_iter()
                    .map(|t| substitute_rust_self(t, self_ty))
                    .collect(),
            );
        }
        None
    };
    for p in &f.generics.params {
        if p.name != proj.param {
            continue;
        }
        if let GenericParamDefKind::Type { bounds, .. } = &p.kind {
            if let Some(args) = take(bounds) {
                return Some(args);
            }
        }
    }
    for wp in &f.generics.where_predicates {
        let WherePredicate::BoundPredicate { type_: Type::Generic(g), bounds, .. } = wp else {
            continue;
        };
        if g != &proj.param {
            continue;
        }
        if let Some(args) = take(bounds) {
            return Some(args);
        }
    }
    None
}

/// Replace every `Self` in a rustdoc type with the type the enclosing impl is
/// written for. With no `self_ty` the type is returned unchanged, and the match
/// against an impl's arguments then simply fails, which is the safe direction.
fn substitute_rust_self(t: &Type, self_ty: Option<&Type>) -> Type {
    let Some(self_ty) = self_ty else { return t.clone() };
    let subst: HashMap<String, Type> =
        HashMap::from([("Self".to_string(), self_ty.clone())]);
    substitute_rust_type(t, &subst)
}

/// Apply a type substitution to a rustdoc type.
fn substitute_rust_type(t: &Type, subst: &HashMap<String, Type>) -> Type {
    match t {
        Type::Generic(name) => subst.get(name).cloned().unwrap_or_else(|| t.clone()),
        Type::ResolvedPath(p) => {
            let mut p = p.clone();
            if let Some(GenericArgs::AngleBracketed { args, constraints }) = p.args.as_deref() {
                let args = args
                    .iter()
                    .map(|a| match a {
                        GenericArg::Type(inner) => {
                            GenericArg::Type(substitute_rust_type(inner, subst))
                        }
                        other => other.clone(),
                    })
                    .collect();
                p.args = Some(Box::new(GenericArgs::AngleBracketed {
                    args,
                    constraints: constraints.clone(),
                }));
            }
            Type::ResolvedPath(p)
        }
        Type::Slice(inner) => Type::Slice(Box::new(substitute_rust_type(inner, subst))),
        Type::Array { type_, len } => Type::Array {
            type_: Box::new(substitute_rust_type(type_, subst)),
            len: len.clone(),
        },
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(|t| substitute_rust_type(t, subst)).collect()),
        Type::BorrowedRef { lifetime, is_mutable, type_ } => Type::BorrowedRef {
            lifetime: lifetime.clone(),
            is_mutable: *is_mutable,
            type_: Box::new(substitute_rust_type(type_, subst)),
        },
        Type::RawPointer { is_mutable, type_ } => Type::RawPointer {
            is_mutable: *is_mutable,
            type_: Box::new(substitute_rust_type(type_, subst)),
        },
        other => other.clone(),
    }
}

/// Match an impl's written type (`[T2]`, where `T2` is one of the impl's own
/// parameters) against the concrete type the method's bound names (`[T]`),
/// binding the impl's parameters as it goes.
///
/// Structural and deliberately strict: anything it cannot line up is `false`,
/// and that impl then contributes no overload rather than one built on a guess.
fn unify_rust_type(
    pattern: &Type,
    value: &Type,
    vars: &HashSet<String>,
    subst: &mut HashMap<String, Type>,
) -> bool {
    if let Type::Generic(name) = pattern {
        if vars.contains(name.as_str()) {
            // A parameter already bound must bind the same way again:
            // `Trait<T2, T2>` against `Trait<int, String>` is not a match.
            return match subst.get(name) {
                Some(prev) => prev == value,
                None => {
                    subst.insert(name.clone(), value.clone());
                    true
                }
            };
        }
    }
    match (pattern, value) {
        (Type::Generic(a), Type::Generic(b)) => a == b,
        (Type::Primitive(a), Type::Primitive(b)) => a == b,
        (Type::Slice(a), Type::Slice(b)) => unify_rust_type(a, b, vars, subst),
        (Type::Tuple(a), Type::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| unify_rust_type(x, y, vars, subst))
        }
        (Type::Array { type_: a, len: la }, Type::Array { type_: b, len: lb }) => {
            la == lb && unify_rust_type(a, b, vars, subst)
        }
        (
            Type::BorrowedRef { is_mutable: ma, type_: a, .. },
            Type::BorrowedRef { is_mutable: mb, type_: b, .. },
        ) => ma == mb && unify_rust_type(a, b, vars, subst),
        (
            Type::RawPointer { is_mutable: ma, type_: a },
            Type::RawPointer { is_mutable: mb, type_: b },
        ) => ma == mb && unify_rust_type(a, b, vars, subst),
        (Type::ResolvedPath(a), Type::ResolvedPath(b)) => {
            // The id is the identity. Two modules may publish two DIFFERENT
            // types under one simple name (`core::ops::Range` and
            // `core::range::Range` both exist), so the name would not do.
            if a.id != b.id {
                return false;
            }
            let (pa, pb) = (path_type_args(&a.args), path_type_args(&b.args));
            pa.len() == pb.len()
                && pa.iter().zip(pb).all(|(x, y)| unify_rust_type(x, y, vars, subst))
        }
        _ => false,
    }
}

/// Does `t` still mention a generic the match left unbound, or a projection of
/// its own?
///
/// After substitution the only generics that may remain are the OWNER's
/// (the `T` of `impl<T> [T]`, which the stub's class declares). One of the
/// IMPL's parameters surviving means the match did not pin it down, and a
/// nested projection would only swap one unknown for another.
fn projection_result_is_incomplete(t: &Type, unbound: &HashSet<String>) -> bool {
    let mut hit = false;
    walk_rust_type(t, &mut |x| match x {
        Type::Generic(name) if unbound.contains(name.as_str()) => hit = true,
        Type::QualifiedPath { .. } | Type::Infer => hit = true,
        _ => {}
    });
    hit
}

/// Whether [`map_path`] gives this Rust name a Jux spelling of its own, so the
/// stub never declares the Rust name and there is nothing to check against.
fn map_path_has_own_spelling(name: &str) -> bool {
    map_path_folds(name) || matches!(name, "String" | "HashMap" | "BTreeMap" | "HashSet" | "BTreeSet")
}

/// Every named type `t` mentions, paired with its real Rust path, for the late
/// identity check in [`settle_projection_group`]. A name whose path this crate
/// cannot state is paired with the empty string, which that pass reads as
/// "unverifiable" and drops.
fn named_type_paths(
    krate: &Crate,
    public: &PublicPaths,
    t: &Type,
    out: &mut Vec<(String, String)>,
) {
    walk_rust_type(t, &mut |x| {
        let Type::ResolvedPath(p) = x else { return };
        let name = last_segment(&p.path);
        if map_path_has_own_spelling(name) {
            return;
        }
        let path = match krate.index.get(&p.id) {
            Some(item) => real_rust_path(krate, item, public),
            None => krate
                .paths
                .get(&p.id)
                .filter(|s| !s.path.is_empty())
                .map(|s| public_rust_path(&s.path)),
        };
        out.push((name.to_string(), path.unwrap_or_default()));
    });
}

/// Resolve every impl of `proj`'s trait that the method's own bound admits.
fn resolve_projection_impls(
    krate: &Crate,
    public: &PublicPaths,
    proj: &MethodProjection,
    bound_args: &[Type],
) -> Vec<ResolvedImpl> {
    let Some(trait_item) = krate.index.get(&proj.trait_id) else {
        return Vec::new();
    };
    let ItemEnum::Trait(tr) = &trait_item.inner else {
        return Vec::new();
    };

    let mut out: Vec<ResolvedImpl> = Vec::new();
    let mut seen_params: HashSet<String> = HashSet::new();
    for impl_id in &tr.implementations {
        let Some(item) = krate.index.get(impl_id) else { continue };
        let ItemEnum::Impl(im) = &item.inner else { continue };
        if im.is_synthetic || im.is_negative || im.blanket_impl.is_some() {
            continue;
        }
        let Some(tr_path) = &im.trait_ else { continue };
        if tr_path.id != proj.trait_id {
            continue;
        }
        // The impl's own parameters are the match's variables; every other name
        // in it is concrete, or belongs to the method's own scope.
        let vars: HashSet<String> = im
            .generics
            .params
            .iter()
            .filter(|p| matches!(p.kind, GenericParamDefKind::Type { .. }))
            .map(|p| p.name.clone())
            .collect();
        let impl_args = path_type_args(&tr_path.args);
        if impl_args.len() != bound_args.len() {
            continue;
        }
        let mut subst: HashMap<String, Type> = HashMap::new();
        if !impl_args
            .iter()
            .zip(bound_args)
            .all(|(p, v)| unify_rust_type(p, v, &vars, &mut subst))
        {
            continue; // a different haystack: `SliceIndex<str>` under a `[T]` bound
        }
        // `type Output = ...;` inside the impl. Its absence means rustdoc did
        // not record the binding, and the projection stays unresolved.
        let Some(binding) = im.items.iter().find_map(|aid| {
            let a = krate.index.get(aid)?;
            if a.name.as_deref() != Some(proj.assoc.as_str()) {
                return None;
            }
            match &a.inner {
                ItemEnum::AssocType { type_: Some(t), .. } => Some(t.clone()),
                _ => None,
            }
        }) else {
            continue;
        };

        let bound_names: HashSet<String> = subst.keys().cloned().collect();
        let unbound: HashSet<String> = vars.difference(&bound_names).cloned().collect();
        let index_ty = substitute_rust_type(&im.for_, &subst);
        let result_ty = substitute_rust_type(&binding, &subst);
        if projection_result_is_incomplete(&index_ty, &unbound)
            || projection_result_is_incomplete(&result_ty, &unbound)
        {
            continue;
        }
        let index = map_type(&index_ty);
        let result = map_type(&result_ty);
        if !index.is_spellable() || !result.is_spellable() {
            continue;
        }
        // One parameter type, one overload: two impls that map to the same Jux
        // parameter could never be told apart at a call, and a type published
        // under two module paths is the usual reason two of them collide.
        if !seen_params.insert(index.to_string()) {
            continue;
        }
        let mut param_paths = Vec::new();
        named_type_paths(krate, public, &index_ty, &mut param_paths);
        let param_rank = jux_shape_rank(&index);
        out.push(ResolvedImpl { index, result, param_paths, param_rank });
    }
    out
}

/// Reduce the resolved impls to one per distinct RESULT type: the one whose
/// parameter has the simplest shape, the crate's own impl order breaking a tie
/// (§G.6.4.5).
///
/// The fan-out exists to make the RESULT knowable. The seventeen
/// `SliceIndex<[T]>` impls say only two things: `usize` gives the element back,
/// every range shape gives a sub-slice. A second parameter shape with the same
/// result adds surface without adding an answer, so the shapes that agree are
/// represented by the simplest of them, which is also the one a program would
/// write: `usize` rather than `core`'s internal `Last` or `Clamp<usize>`.
///
/// Reducing HERE and not in the late pass matters: a type is pulled into the
/// stub when some declaration mentions it (the pool re-export surface,
/// §G.6.2.1), and an overload that was never going to be kept would pull in an
/// orphan. `Clamp` reached the stub exactly that way, as an empty class under
/// `std::index::Clamp`, a Rust path that does not exist.
fn keep_one_impl_per_result(resolved: Vec<ResolvedImpl>) -> Vec<ResolvedImpl> {
    let mut results: Vec<String> = Vec::new();
    for r in &resolved {
        let key = r.result.to_string();
        if !results.contains(&key) {
            results.push(key);
        }
    }
    let mut keep: Vec<usize> = results
        .iter()
        .filter_map(|want| {
            resolved
                .iter()
                .enumerate()
                .filter(|(_, r)| &r.result.to_string() == want)
                .min_by_key(|(i, r)| (r.param_rank, *i))
                .map(|(i, _)| i)
        })
        .collect();
    keep.sort_unstable();
    resolved
        .into_iter()
        .enumerate()
        .filter(|(i, _)| keep.binary_search(i).is_ok())
        .map(|(_, r)| r)
        .collect()
}

/// Replace, inside a mapped Jux type, the method's projected parameter with the
/// impl's self type and the projection with the impl's associated type.
///
/// Both are found by the names the ingest itself wrote: [`map_type`] renders
/// the parameter as `JuxType::Param("I")` and the projection as
/// `JuxType::Unknown("I.Output")`.
fn substitute_projection(
    ty: &JuxType,
    param: &str,
    projected: &str,
    index: &JuxType,
    result: &JuxType,
) -> JuxType {
    let rec = |t: &JuxType| substitute_projection(t, param, projected, index, result);
    match ty {
        JuxType::Param(p) if p == param => index.clone(),
        JuxType::Unknown(name) if name == projected => result.clone(),
        JuxType::User { name, args } => JuxType::User {
            name: name.clone(),
            args: args.iter().map(rec).collect(),
        },
        JuxType::Nullable(inner) => JuxType::Nullable(Box::new(rec(inner))),
        JuxType::RawPtr(inner) => JuxType::RawPtr(Box::new(rec(inner))),
        JuxType::Array { elem, size } => JuxType::Array {
            elem: Box::new(rec(elem)),
            size: *size,
        },
        JuxType::Tuple(ts) => JuxType::Tuple(ts.iter().map(rec).collect()),
        JuxType::Fn { params, ret, is_async } => JuxType::Fn {
            params: params.iter().map(rec).collect(),
            ret: Box::new(rec(ret)),
            is_async: *is_async,
        },
        other => other.clone(),
    }
}

/// Map one Rust method to the declarations it surfaces as: the ordinary single
/// one, or a §G.6.4.5 fan-out followed by its fallback.
///
/// `self_ty` is the type the enclosing impl is written for, which the method's
/// own `where I: SliceIndex<Self>` bound needs to be read at all.
fn map_function_surface(
    krate: &Crate,
    public: &PublicPaths,
    name: &str,
    f: &Function,
    self_ty: Option<&Type>,
) -> Vec<StubFn> {
    let base = map_function(krate, name, f);
    let Some(proj) = method_projection(f) else { return vec![base] };
    let Some(bound_args) = projection_bound_args(f, &proj, self_ty) else {
        return vec![base];
    };
    let resolved = keep_one_impl_per_result(resolve_projection_impls(
        krate,
        public,
        &proj,
        &bound_args,
    ));
    if resolved.is_empty() {
        return vec![base];
    }

    let projected = format!("{}.{}", proj.param, proj.assoc);
    let mut out: Vec<StubFn> = Vec::with_capacity(resolved.len() + 1);
    for r in &resolved {
        let mut sf = base.clone();
        sf.ret = substitute_projection(&sf.ret, &proj.param, &projected, &r.index, &r.result);
        sf.throws = sf
            .throws
            .as_ref()
            .map(|t| substitute_projection(t, &proj.param, &projected, &r.index, &r.result));
        for p in &mut sf.params {
            p.ty = substitute_projection(&p.ty, &proj.param, &projected, &r.index, &r.result);
        }
        // The parameter is no longer generic: it IS the impl's self type.
        sf.generics.retain(|g| g != &proj.param);
        sf.projection_role = Some(ProjectionRole::Overload {
            param_paths: r.param_paths.clone(),
            param_rank: r.param_rank,
        });
        out.push(sf);
    }
    // The original, kept only if every overload is later dropped as unnameable.
    let mut fallback = base;
    fallback.projection_role = Some(ProjectionRole::Fallback);
    out.push(fallback);
    out
}

/// Settle every projection fan-out in the finished stub (§G.6.4.5).
///
/// This runs LATE, once the whole stub's declared types are known, because the
/// guard it applies needs them: an overload survives only when this stub
/// declares its parameter's type name AND that declaration is the very Rust
/// type the impl was written for. `rust.std` declares `Range` for
/// `std::collections::btree_map::Range`, so an overload taking
/// `core::ops::Range` cannot be written `Range` there, and emitting it anyway
/// would aim the checker at a two-parameter map view.
fn retain_projection_overloads(collected: &mut [(String, StubItem)]) {
    // Declared type name -> its real Rust path. A declaration with no recorded
    // path cannot be checked against, so it never matches.
    let declared: HashMap<String, String> = collected
        .iter()
        .filter_map(|(n, it)| match it {
            StubItem::Type(t) => Some((n.clone(), t.rust_path.clone().unwrap_or_default())),
            _ => None,
        })
        .collect();

    for (_, item) in collected.iter_mut() {
        let StubItem::Type(t) = item else { continue };
        if t.methods.iter().all(|m| m.projection_role.is_none()) {
            continue;
        }
        let old = std::mem::take(&mut t.methods);
        let mut settled: HashSet<String> = HashSet::new();
        let mut out: Vec<StubFn> = Vec::with_capacity(old.len());
        for m in &old {
            if m.projection_role.is_none() {
                out.push(m.clone());
            } else if settled.insert(m.name.clone()) {
                // The whole group lands where its first member stood.
                out.extend(settle_projection_group(&declared, &old, &m.name));
            }
        }
        t.methods = out;
    }
}

/// The members of one fan-out group that survive (§G.6.4.5): the overloads
/// this stub can NAME, capped; or the fallback when it can name none of them.
///
/// The group already holds one overload per distinct result
/// ([`keep_one_impl_per_result`]); what is left to decide is whether the stub
/// can write each one's parameter type, and that needs the whole stub's
/// declared set, which only exists once every crate has been ingested.
fn settle_projection_group(
    declared: &HashMap<String, String>,
    methods: &[StubFn],
    name: &str,
) -> Vec<StubFn> {
    let group: Vec<&StubFn> = methods
        .iter()
        .filter(|m| m.name == name && m.projection_role.is_some())
        .collect();
    let mut chosen: Vec<usize> = Vec::new();
    for (i, m) in group.iter().enumerate() {
        let Some(ProjectionRole::Overload { param_paths, .. }) = &m.projection_role else {
            continue;
        };
        // Every named type the parameter mentions must be one this stub
        // declares FOR THAT VERY RUST TYPE. `rust.std` declares `Range` for
        // `std::collections::btree_map::Range`, so an overload taking
        // `core::ops::Range` cannot be written `Range` there.
        let names_it = param_paths
            .iter()
            .all(|(n, p)| !p.is_empty() && declared.get(n).is_some_and(|d| d == p));
        if names_it && chosen.len() < PROJECTION_FANOUT_CAP {
            chosen.push(i);
        }
    }

    if chosen.is_empty() {
        return group
            .into_iter()
            .filter(|m| matches!(m.projection_role, Some(ProjectionRole::Fallback)))
            .cloned()
            .collect();
    }
    chosen
        .into_iter()
        .map(|i| {
            let mut m = group[i].clone();
            m.projection_role = None; // settled; nothing downstream reads it
            m
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
        Type::ImplTrait(bounds) => fn_trait_to_jux(bounds)
            .or_else(|| conversion_bound_target(bounds))
            .unwrap_or_else(|| {
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
        // An associated-type projection (`<I as SliceIndex<[T]>>::Output`,
        // `Self::Item`) names a type the stub cannot know: it depends on the
        // argument the call is made with. It is written `I.Output`, a
        // two-segment name no stub declares, so the checker reads it as an
        // unknown type and leaves the value to the declared slot. Written as
        // the bare `Output` it resolved to `std::process::Output`, and
        // `final int? first = v.get(0);` was refused (B16).
        Type::QualifiedPath { name, self_type, .. } => {
            let owner = match self_type.as_ref() {
                Type::Generic(g) => g.clone(),
                _ => "Self".to_string(),
            };
            JuxType::Unknown(format!("{owner}.{name}"))
        }
        // Pattern types and inference markers have no Jux spelling.
        Type::Pat { .. } | Type::Infer => JuxType::Unknown("Object".into()),
    }
}

/// Whether a Rust primitive has a Jux primitive type whose values can call
/// its methods: the integers, the floats, `char` and `bool`.
fn primitive_has_jux_type(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" | "f32"
            | "f64" | "char" | "bool"
    )
}

/// The Jux spelling of a Rust primitive (see [`map_primitive`]).
fn primitive_jux_name(name: &str) -> &'static str {
    match map_primitive(name) {
        JuxType::Prim(p) => p,
        _ => "int",
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

/// Whether [`map_path`] folds the Rust type of this name into a Jux spelling
/// of its own (`Option` to `T?`, `Result` to `throws`, the smart pointers to
/// their pointee), so the type itself is never one the stub declares.
fn map_path_folds(name: &str) -> bool {
    matches!(name, "Option" | "Result" | "Box" | "Rc" | "Arc")
}

/// Map a named path type, applying the §G.3.1 stdlib substitutions
/// (`Vec` kept, `Option`→`T?`, `HashMap`→`Map`, `Box`/`Rc`/`Arc` unwrap…).
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
        "Vec" => JuxType::vec(arg0()),
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

/// The type behind a CONVERSION bound: the `T` of `impl Into<T>`,
/// `impl AsRef<T>`, `impl Borrow<T>`.
///
/// Such a bound does not name a type the caller can hold. It says "anything
/// that converts to T", and what the call site actually passes is a T (or
/// something Rust converts for it). Surfaced as the trait's own name instead,
/// every one of these parameters read `Into` / `AsRef`, which no Jux value can
/// ever be. A crate that wraps a C++ library is written almost entirely in
/// this style, so the whole surface was unusable.
fn conversion_bound_target(bounds: &[GenericBound]) -> Option<JuxType> {
    for b in bounds {
        let GenericBound::TraitBound { trait_, .. } = b else {
            continue;
        };
        if !matches!(last_segment(&trait_.path), "Into" | "AsRef" | "Borrow" | "AsMut") {
            continue;
        }
        let args = collect_type_args(&trait_.args);
        if let Some(target) = args.into_iter().next() {
            return Some(target);
        }
    }
    None
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

/// The two whole-crate tables that turn a rustdoc DEFINITION path into one a
/// user can actually write in a `use`.
///
/// Built once per ingested crate and passed down. An earlier version memoized
/// them behind the crate's address, which is wrong for the way they are
/// consumed: [`generate_merged_with_pool`] parses each crate into a local and
/// drops it before parsing the next, so every crate in an ingest sits at the
/// same address and the second one was answered with the first one's tables.
/// The symptom was silent -- every path fell through to the definition path,
/// and `std::net::tcp::TcpListener` (a private `tcp` module) reached the stub.
pub(crate) struct PublicPaths {
    /// Every public module of the crate, by public path.
    modules: HashSet<String>,
    /// Item id -> the shortest publicly importable path for it.
    reexports: HashMap<Id, String>,
}

impl PublicPaths {
    /// Scan `krate` once for both tables.
    fn of(krate: &Crate) -> Self {
        Self {
            modules: build_public_module_paths(krate),
            reexports: build_reexport_paths(krate),
        }
    }
}

/// The real, fully-qualified Rust path of `item` (`std::collections::HashSet`),
/// from the rustdoc `paths` summary. Used to populate `StubType::rust_path` so
/// the backend can lower a reference to this external type to its true Rust path
/// (§G.9.2) rather than the flat Jux `rust.std.X` spelling.
fn real_rust_path(krate: &Crate, item: &Item, public: &PublicPaths) -> Option<String> {
    let summary = krate.paths.get(&item.id);
    // The definition path is preferred WHENEVER it is importable -- it is the
    // canonical one, and it distinguishes items that share a simple name across
    // sibling modules (`std::os::unix::process::ChildExt` vs the `linux` one).
    // Only when it threads a private module is it unusable, and then the
    // re-export path is the answer.
    if let Some(summary) = summary {
        if !summary.path.is_empty() && definition_path_is_public(public, &summary.path) {
            return Some(public_rust_path(&summary.path));
        }
    }
    if let Some(p) = public.reexports.get(&item.id).cloned() {
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
fn definition_path_is_public(public: &PublicPaths, path: &[String]) -> bool {
    if path.len() <= 2 {
        // `crate::Item` -- the crate root is always importable.
        return true;
    }
    (1..path.len()).all(|end| public.modules.contains(&public_module_path(&path[..end])))
}

/// Every public module of the crate, by public path.
fn build_public_module_paths(krate: &Crate) -> HashSet<String> {
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
    out
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

/// Item id -> the shortest **publicly importable** path, read off the crate's
/// re-export (`pub use`) statements.
///
/// rustdoc's `paths` summary reports where an item is DEFINED, and std defines
/// plenty of things in private modules that exist only to be re-exported:
/// `std::io` has a private `mod copy` holding `pub fn copy`, published as
/// `pub use self::copy::copy;`. The definition path `std::io::copy::copy` names
/// a private module and does not compile in a `use`. The re-export says the
/// public name is `std::io::copy`.
///
/// Not every path is recoverable this way: a glob (`pub use self::x::*;`) names
/// no individual item, so those fall back to the definition path plus the
/// [`public_rust_path`] normalisations.
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
            for (target, name) in published_names(krate, child_item) {
                let candidate = format!("{module_path}::{name}");
                // Shortest wins, then lexicographic — `std::io::copy` beats a
                // deeper alias, and the choice between two equally short
                // aliases is the same on every run (a HashMap-order tie-break
                // would churn the generated stub).
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
    }
    out
}

/// Point every re-exported item at the crate that re-exports it.
///
/// Only the bound crate is linked, so a type defined in one of ITS
/// dependencies has to be named through it: `tiny_skia::PathBuilder`, never
/// `tiny_skia_path::PathBuilder`, which the emitted crate cannot resolve
/// (`unresolved import` from rustc, for a stub that type-checked). The
/// re-export names are read from the host crate's root module.
pub fn rewrite_reexported_paths(
    stub: &mut StubFile,
    host_crate: &str,
    host_json: &str,
) -> Result<(), serde_json::Error> {
    let krate: Crate = serde_json::from_str(host_json)?;
    // Defining name -> the name the host publishes it under. Root-level
    // re-exports only: a deeper one would need the module path to rebuild,
    // and crates re-export their surface at the root.
    let mut exported: HashMap<String, String> = HashMap::new();
    // Every root re-export, paired as (defining name, published name). The
    // path rewrite below uses the ones that point OUTSIDE this crate; the
    // alias pass at the end uses the ones that RENAME.
    let mut renames: Vec<(String, String)> = Vec::new();
    if let Some(root) = krate.index.get(&krate.root) {
        if let ItemEnum::Module(m) = &root.inner {
            // Globs included: a large crate publishes most of its surface as
            // `pub use core::*`, and the renames live inside that module
            // (`pub use paint::Style as PaintStyle` is in skia-safe's `core`).
            let published: Vec<(Id, String)> = m
                .items
                .iter()
                .filter_map(|id| krate.index.get(id))
                .flat_map(|item| published_names(&krate, item))
                .collect();
            for (target, exported_name) in published {
                // A re-export may point at another re-export
                // (`pub use paint::Style as PaintStyle` where `paint::Style`
                // is itself `pub use sb::SkPaint_Style as Style`). Follow the
                // chain to the name the type is actually declared under.
                let mut cursor = target;
                let mut defining = None;
                for _ in 0..4 {
                    match krate.index.get(&cursor) {
                        Some(t) => match &t.inner {
                            ItemEnum::Use(next) => match next.id {
                                Some(id) => cursor = id,
                                None => {
                                    defining = t.name.clone();
                                    break;
                                }
                            },
                            _ => {
                                defining = t.name.clone();
                                break;
                            }
                        },
                        None => {
                            defining =
                                krate.paths.get(&cursor).and_then(|s| s.path.last().cloned());
                            break;
                        }
                    }
                }
                let Some(defining) = defining else { continue };
                renames.push((defining.clone(), exported_name.clone()));
                if krate.index.contains_key(&target) {
                    continue; // defined here; its own path is already right
                }
                exported.insert(defining, exported_name);
            }
        }
    }

    let host = host_crate.replace('-', "_");
    let retarget = |path: &mut Option<String>, name: &str| {
        let Some(current) = path.as_deref() else { return };
        // Already reached through the host crate: nothing to do.
        if current.split("::").next() == Some(host.as_str()) {
            return;
        }
        if let Some(exported_as) = exported.get(name) {
            *path = Some(format!("{host}::{exported_as}"));
        }
    };

    for item in &mut stub.items {
        if exported.is_empty() {
            break;
        }
        match item {
            StubItem::Type(t) => {
                let name = t.name.clone();
                retarget(&mut t.rust_path, &name);
            }
            StubItem::Function(f) => {
                let name = f.name.clone();
                retarget(&mut f.rust_path, &name);
            }
            StubItem::Const(_) | StubItem::Alias(_) => {}
        }
    }

    // A rename on the way out (`pub use paint::Style as PaintStyle`) is part of
    // the public surface: the crate's own signatures use the public name. The
    // type keeps the name it was defined with, and the rename is declared as
    // the alias it is, so both spellings resolve.
    let declared: std::collections::HashSet<&str> = stub
        .items
        .iter()
        .filter_map(|it| match it {
            StubItem::Type(t) => Some(t.name.as_str()),
            _ => None,
        })
        .collect();
    let mut aliases: Vec<StubAlias> = renames
        .into_iter()
        .filter(|(defining, exported)| exported != defining && declared.contains(defining.as_str()))
        .map(|(defining, exported)| StubAlias { name: exported, target: defining })
        .collect();
    aliases.sort_by(|a, b| a.name.cmp(&b.name));
    aliases.dedup_by(|a, b| a.name == b.name);
    // A rename that collides with a type of that name already in the stub is
    // dropped: the type wins, and declaring both would be a duplicate.
    aliases.retain(|a| !declared.contains(a.name.as_str()));
    stub.items.extend(aliases.into_iter().map(StubItem::Alias));
    Ok(())
}

/// Whether `name` is a Jux primitive type spelling -- the language's own
/// keywords, which is what `map_type` renders a Rust primitive as.
fn is_jux_primitive_name(name: &str) -> bool {
    matches!(
        name,
        "bool" | "char" | "byte" | "ubyte" | "short" | "ushort" | "int" | "uint" | "long"
            | "ulong" | "float" | "double" | "i8" | "u8" | "i16" | "u16" | "i32" | "u32"
            | "i64" | "u64" | "f32" | "f64"
    )
}

/// The crates whose types a crate re-exports as part of its own public API.
///
/// `tiny-skia` is largely `pub use tiny_skia_path::{Path, PathBuilder, Rect,
/// Transform, Stroke, ...}`. Those names are its public surface, but their
/// DEFINITIONS live in another crate's rustdoc JSON, so ingesting this crate
/// alone produced a stub whose own method signatures named types the stub
/// never declared: `Pixmap.fill_path(&Path ...)` with no `Path` anywhere.
///
/// The answer is read out of the JSON rather than listed anywhere: a `pub use`
/// whose target id is absent from this crate's index points into another
/// crate, and `paths` says which. The caller documents those crates too and
/// merges them in.
pub fn reexported_crate_names(json: &str) -> Result<Vec<String>, serde_json::Error> {
    let krate: Crate = serde_json::from_str(json)?;
    let mut names: Vec<String> = Vec::new();
    for item in krate.index.values() {
        let ItemEnum::Use(u) = &item.inner else {
            continue;
        };
        let Some(id) = u.id else { continue };
        if krate.index.contains_key(&id) {
            continue; // resolved in this crate -- nothing external about it
        }
        let Some(summary) = krate.paths.get(&id) else {
            continue;
        };
        // crate_id 0 is this crate itself.
        if summary.crate_id == 0 {
            continue;
        }
        if let Some(ext) = krate.external_crates.get(&summary.crate_id) {
            if !names.contains(&ext.name) {
                names.push(ext.name.clone());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// The names `child` publishes into its parent module, as `(id, name)` pairs.
///
/// Three shapes reach the same conclusion:
///
/// * an explicit `pub use` names its target, one name;
/// * a re-export of a PRIVATE item is instead INLINED by rustdoc — `std::io`'s
///   private `mod copy` contributes its `pub fn copy` straight into `std::io`'s
///   item list — and then membership in a public module IS the public path;
/// * a GLOB (`pub use owned::*;`) names nothing, so it is expanded against the
///   target module's own public items. `std::os::fd` is built entirely this
///   way, and without the expansion `AsFd` keeps the private
///   `std::os::fd::owned::AsFd`.
///
/// A glob is expanded one level only. Nesting them is rare, and a second level
/// would need cycle detection for what it buys.
fn published_names(krate: &Crate, child: &Item) -> Vec<(Id, String)> {
    match &child.inner {
        ItemEnum::Use(u) if u.is_glob => {
            let Some(id) = u.id else { return Vec::new() };
            // Globs nest: a crate root says `pub use core::*` and that module
            // says `pub use rect::*`. Stopping at the first level left the
            // types below it with no public path at all, so the emitted `use`
            // named the private module they are defined in.
            let mut out = Vec::new();
            let mut seen: HashSet<Id> = HashSet::new();
            let mut stack = vec![(id, 0usize)];
            while let Some((module_id, depth)) = stack.pop() {
                if depth > 3 || !seen.insert(module_id) {
                    continue;
                }
                let Some(target_mod) = krate.index.get(&module_id) else {
                    continue;
                };
                let ItemEnum::Module(m) = &target_mod.inner else {
                    continue;
                };
                for inner_id in &m.items {
                    let Some(inner) = krate.index.get(inner_id) else {
                        continue;
                    };
                    match &inner.inner {
                        ItemEnum::Use(u) if u.is_glob => {
                            if let Some(next) = u.id {
                                stack.push((next, depth + 1));
                            }
                        }
                        ItemEnum::Use(u) => {
                            if let Some(id) = u.id {
                                out.push((id, u.name.clone()));
                            }
                        }
                        _ => out.extend(named_item(inner)),
                    }
                }
            }
            out
        }
        ItemEnum::Use(u) => u.id.map(|id| (id, u.name.clone())).into_iter().collect(),
        _ => named_item(child).into_iter().collect(),
    }
}

/// `(id, name)` for a public, named item that can be referred to by path.
fn named_item(item: &Item) -> Option<(Id, String)> {
    match &item.inner {
        ItemEnum::Struct(_)
        | ItemEnum::Enum(_)
        | ItemEnum::Trait(_)
        | ItemEnum::Function(_)
        | ItemEnum::TypeAlias(_) => match &item.name {
            Some(n) if is_public(&item.visibility) => Some((item.id, n.clone())),
            _ => None,
        },
        _ => None,
    }
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
            "Vec<ubyte>",
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

    /// An empty crate, for the mapping tests that need one only to look a
    /// path up in (and find nothing, falling back to the written spelling).
    fn empty_crate() -> Crate {
        serde_json::from_str(
            r#"{"root": 0, "crate_version": null, "includes_private": false,
                "index": {}, "paths": {}, "external_crates": {},
                "target": {"triple": "x86_64-unknown-linux-gnu",
                           "target_features": []},
                "format_version": 58}"#,
        )
        .expect("minimal rustdoc crate parses")
    }


    // ------------------------------------------------------------------
    // §G.6.4.5 — projection over the method's own type parameter
    // ------------------------------------------------------------------

    /// A bare public item of `crate_id == 0`.
    fn item(id: u32, name: Option<&str>, inner: ItemEnum) -> Item {
        Item {
            id: Id(id),
            crate_id: 0,
            name: name.map(str::to_string),
            span: None,
            visibility: Visibility::Public,
            docs: None,
            links: std::collections::HashMap::new(),
            attrs: Vec::new(),
            deprecation: None,
            inner,
        }
    }

    /// A type parameter with no bounds.
    fn type_param(name: &str) -> rustdoc_types::GenericParamDef {
        rustdoc_types::GenericParamDef {
            name: name.to_string(),
            kind: GenericParamDefKind::Type {
                bounds: Vec::new(),
                default: None,
                is_synthetic: false,
            },
        }
    }

    /// `Name<args>` as a rustdoc path with a known id.
    fn path_id(name: &str, id: u32, args: Vec<Type>) -> rustdoc_types::Path {
        let args = if args.is_empty() {
            None
        } else {
            Some(Box::new(GenericArgs::AngleBracketed {
                args: args.into_iter().map(GenericArg::Type).collect(),
                constraints: Vec::new(),
            }))
        };
        rustdoc_types::Path { path: name.to_string(), id: Id(id), args }
    }

    /// A synthetic crate in the shape the bug was found in:
    ///
    /// ```text
    /// pub struct Whole;
    /// pub trait Idx<Haystack> { type Output; }
    /// impl<X> Idx<Holder<X>> for usize { type Output = X; }
    /// impl<X> Idx<Holder<X>> for Whole { type Output = [X]; }
    /// pub struct Holder<T>;
    /// impl<T> Holder<T> {
    ///     pub fn get<I>(&self, index: I) -> Option<&I::Output> where I: Idx<Self>;
    /// }
    /// ```
    ///
    /// Everything the fan-out needs is in here and nowhere else: no network,
    /// no toolchain, no cargo.
    fn projection_crate(with_trait: bool) -> Crate {
        const HOLDER: u32 = 1;
        const WHOLE: u32 = 2;
        const IDX: u32 = 3;
        const INHERENT: u32 = 10;
        const IMPL_USIZE: u32 = 11;
        const IMPL_WHOLE: u32 = 12;
        const GET: u32 = 20;
        const OUT_USIZE: u32 = 30;
        const OUT_WHOLE: u32 = 31;

        let mut krate = empty_crate();
        let holder_self = Type::ResolvedPath(path_id(
            "Holder",
            HOLDER,
            vec![Type::Generic("T".into())],
        ));

        // `fn get<I>(&self, index: I) -> Option<&I::Output> where I: Idx<Self>`
        let projection = Type::QualifiedPath {
            name: "Output".into(),
            args: None,
            self_type: Box::new(Type::Generic("I".into())),
            // rustdoc writes a projection's trait path as the empty string and
            // carries only the id, which is exactly what the ingest matches on.
            trait_: Some(path_id("", IDX, Vec::new())),
        };
        let get = Function {
            sig: rustdoc_types::FunctionSignature {
                inputs: vec![
                    (
                        "self".into(),
                        Type::BorrowedRef {
                            lifetime: None,
                            is_mutable: false,
                            type_: Box::new(Type::Generic("Self".into())),
                        },
                    ),
                    ("index".into(), Type::Generic("I".into())),
                ],
                output: Some(Type::ResolvedPath(path_id(
                    "Option",
                    99,
                    vec![Type::BorrowedRef {
                        lifetime: None,
                        is_mutable: false,
                        type_: Box::new(projection),
                    }],
                ))),
                is_c_variadic: false,
            },
            generics: Generics {
                params: vec![type_param("I")],
                where_predicates: vec![WherePredicate::BoundPredicate {
                    type_: Type::Generic("I".into()),
                    bounds: vec![GenericBound::TraitBound {
                        trait_: path_id("Idx", IDX, vec![Type::Generic("Self".into())]),
                        generic_params: Vec::new(),
                        modifier: rustdoc_types::TraitBoundModifier::None,
                    }],
                    generic_params: Vec::new(),
                }],
            },
            header: rustdoc_types::FunctionHeader {
                is_const: false,
                is_unsafe: false,
                is_async: false,
                abi: rustdoc_types::Abi::Rust,
            },
            has_body: true,
        };

        // One impl of the bound per index type, each naming its own `Output`.
        let assoc = |id: u32, ty: Type| {
            item(
                id,
                Some("Output"),
                ItemEnum::AssocType {
                    generics: Generics { params: Vec::new(), where_predicates: Vec::new() },
                    bounds: Vec::new(),
                    type_: Some(ty),
                },
            )
        };
        let bound_impl = |id: u32, for_: Type, assoc_id: u32| {
            item(
                id,
                None,
                ItemEnum::Impl(rustdoc_types::Impl {
                    is_unsafe: false,
                    generics: Generics {
                        params: vec![type_param("X")],
                        where_predicates: Vec::new(),
                    },
                    provided_trait_methods: Vec::new(),
                    trait_: Some(path_id(
                        "Idx",
                        IDX,
                        vec![Type::ResolvedPath(path_id(
                            "Holder",
                            HOLDER,
                            vec![Type::Generic("X".into())],
                        ))],
                    )),
                    for_,
                    items: vec![Id(assoc_id)],
                    is_negative: false,
                    is_synthetic: false,
                    blanket_impl: None,
                }),
            )
        };

        let items = vec![
            item(
                HOLDER,
                Some("Holder"),
                ItemEnum::Struct(Struct {
                    kind: StructKind::Unit,
                    generics: Generics {
                        params: vec![type_param("T")],
                        where_predicates: Vec::new(),
                    },
                    impls: vec![Id(INHERENT)],
                }),
            ),
            item(
                WHOLE,
                Some("Whole"),
                ItemEnum::Struct(Struct {
                    kind: StructKind::Unit,
                    generics: Generics { params: Vec::new(), where_predicates: Vec::new() },
                    impls: Vec::new(),
                }),
            ),
            item(
                INHERENT,
                None,
                ItemEnum::Impl(rustdoc_types::Impl {
                    is_unsafe: false,
                    generics: Generics {
                        params: vec![type_param("T")],
                        where_predicates: Vec::new(),
                    },
                    provided_trait_methods: Vec::new(),
                    trait_: None,
                    for_: holder_self,
                    items: vec![Id(GET)],
                    is_negative: false,
                    is_synthetic: false,
                    blanket_impl: None,
                }),
            ),
            item(GET, Some("get"), ItemEnum::Function(get)),
            bound_impl(IMPL_USIZE, Type::Primitive("usize".into()), OUT_USIZE),
            bound_impl(
                IMPL_WHOLE,
                Type::ResolvedPath(path_id("Whole", WHOLE, Vec::new())),
                OUT_WHOLE,
            ),
            assoc(OUT_USIZE, Type::Generic("X".into())),
            assoc(OUT_WHOLE, Type::Slice(Box::new(Type::Generic("X".into())))),
        ];
        for it in items {
            krate.index.insert(it.id, it);
        }
        if with_trait {
            let tr = item(
                IDX,
                Some("Idx"),
                ItemEnum::Trait(rustdoc_types::Trait {
                    is_auto: false,
                    is_unsafe: false,
                    is_dyn_compatible: true,
                    items: Vec::new(),
                    generics: Generics {
                        params: vec![type_param("Haystack")],
                        where_predicates: Vec::new(),
                    },
                    bounds: Vec::new(),
                    implementations: vec![Id(IMPL_USIZE), Id(IMPL_WHOLE)],
                }),
            );
            krate.index.insert(tr.id, tr);
            krate.paths.insert(
                Id(IDX),
                rustdoc_types::ItemSummary {
                    crate_id: 0,
                    path: vec!["probe".into(), "Idx".into()],
                    kind: rustdoc_types::ItemKind::Trait,
                },
            );
        }
        for (id, name, kind) in [
            (HOLDER, "Holder", rustdoc_types::ItemKind::Struct),
            (WHOLE, "Whole", rustdoc_types::ItemKind::Struct),
        ] {
            krate.paths.insert(
                Id(id),
                rustdoc_types::ItemSummary {
                    crate_id: 0,
                    path: vec!["probe".into(), name.into()],
                    kind,
                },
            );
        }
        krate
    }

    /// Every `get` the stub declares, rendered `<ret> get(<params>)`.
    fn get_signatures(stub: &StubFile) -> Vec<String> {
        stub.items
            .iter()
            .find_map(|i| match i {
                StubItem::Type(t) if t.name == "Holder" => Some(t),
                _ => None,
            })
            .expect("Holder is emitted")
            .methods
            .iter()
            .filter(|m| m.name == "get")
            .map(|m| {
                format!(
                    "{}{} get({})",
                    m.ret,
                    if m.generics.is_empty() {
                        String::new()
                    } else {
                        format!(" <{}>", m.generics.join(", "))
                    },
                    m.params
                        .iter()
                        .map(|p| p.ty.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            })
            .collect()
    }

    /// §G.6.4.5: `fn get<I>(..) -> Option<&I::Output> where I: Idx<Self>` is
    /// resolved from the bound's own impls and fans out into one overload per
    /// distinct result: the element for `usize`, the whole slice for `Whole`.
    ///
    /// Before this, the projection was `JuxType::Unknown("I.Output")` and every
    /// `get` on every instantiation typed as `<unknown>?`, which fits any slot
    /// at all, so a wrong call reached rustc instead of the checker.
    #[test]
    fn projection_over_a_method_parameter_fans_out_per_impl() {
        let stub = generate(&projection_crate(true), "probe");
        assert_eq!(
            get_signatures(&stub),
            vec!["T? get(uint)".to_string(), "T[]? get(Whole)".to_string()],
        );
    }

    /// The guard: with the trait item missing from the rustdoc being ingested,
    /// nothing can be resolved, and the method keeps its §G.6.4.2 form rather
    /// than losing the declaration or gaining a guessed one.
    #[test]
    fn projection_without_its_trait_stays_unresolved() {
        let stub = generate(&projection_crate(false), "probe");
        assert_eq!(
            get_signatures(&stub),
            vec!["I.Output? <I> get(I)".to_string()],
        );
    }
    #[test]
    fn result_return_becomes_throws() {
        let krate = empty_crate();
        let result_ty = resolved(
            "Result",
            vec![resolved("Config", vec![]), resolved("ConfigError", vec![])],
        );
        let (ret, throws) = map_return(&krate, &Some(result_ty));
        assert_eq!(ret.to_string(), "Config");
        assert_eq!(
            throws.map(|e| e.to_string()),
            Some("ConfigError".to_string())
        );

        // Plain return, no throws.
        let (ret, throws) = map_return(&krate, &Some(Type::Primitive("bool".into())));
        assert_eq!(ret, JuxType::Prim("bool"));
        assert!(throws.is_none());
    }
}
