//! The second discovery source: the toolchain's own library SOURCE
//! (JUX-BINDGEN-ADDENDUM §G.6.4.4).
//!
//! rustdoc JSON leaves out `alloc`'s inherent impls on the primitives `core`
//! defines. `alloc` writes `impl<T> [T] { pub fn sort(..) .. }` and
//! `impl str { pub fn to_uppercase(..) .. }`, but rustdoc attaches those
//! blocks to `core`'s `slice` and `str` pages, so no JSON file carries them:
//! not the prebuilt ones, and not one built from `rust-src` either. A `Vec`
//! therefore had `sort_unstable` (from `core`) but not `sort`, `sort_by_key`,
//! `to_vec`, `concat` or `join`.
//!
//! This module reads them from the source instead. It never names a method:
//!
//! - a file is read when it holds an incoherent impl, which Rust marks with
//!   `#[rustc_allow_incoherent_impl]` on every such method;
//! - an `impl` is read when it is inherent and its self type is a slice or a
//!   primitive (the shapes the rustdoc pool keys, `[]` and `prim:str`);
//! - a method is kept when it is `pub`, takes `&self` or `&mut self`, is
//!   marked `#[stable]`, and is neither `#[unstable]` nor `#[doc(hidden)]`.
//!
//! Each kept signature is rebuilt as the rustdoc [`Function`] rustdoc would
//! have written, and then goes through the SAME [`crate::ingest`] mapping as
//! every JSON method. So the rest of bindgen, and the backend, cannot tell the
//! two sources apart: `Vec` reaches `sort` through `Deref` exactly as it
//! reaches `first`.

use rustdoc_types::{
    Abi, Crate, Function, FunctionHeader, FunctionSignature, GenericArg, GenericArgs,
    GenericBound, GenericParamDef, GenericParamDefKind, Generics, Id, Path, TraitBoundModifier,
    Type, WherePredicate,
};

use crate::ingest::InherentPool;

/// The attribute Rust requires on a method of an inherent impl written outside
/// the crate that defines the type. Its presence is what makes a file worth
/// parsing, so the reader never needs a list of file names.
const INCOHERENT_MARKER: &str = "rustc_allow_incoherent_impl";

/// Rust's primitive type names, as rustdoc writes them in `Type::Primitive`.
/// This is the language's own closed set of primitives (not an API surface):
/// it decides whether a written path such as `u8` or `str` is a primitive or a
/// named type.
const RUST_PRIMITIVES: &[&str] = &[
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32",
    "i64", "i128", "isize", "f16", "f32", "f64", "f128",
];

/// Pool the stable inherent slice and primitive methods found in `sources`
/// (`(path, text)` pairs of Rust source files) into `pool`, next to the ones
/// the rustdoc JSON supplied.
///
/// `krate` is any crate of the same ingest; the mapping only consults it for
/// lookups that a source-built signature never needs, so which one does not
/// matter. Returns how many methods were added.
pub(crate) fn collect_source_pool(
    sources: &[(&str, &str)],
    krate: &Crate,
    pool: &mut InherentPool,
) -> usize {
    let mut added = 0usize;
    for (_path, text) in sources {
        if !text.contains(INCOHERENT_MARKER) {
            continue;
        }
        // A file syn cannot parse (a newer syntax than the syn in use) is
        // skipped whole: the surface loses its methods, nothing is guessed.
        let Ok(file) = syn::parse_file(text) else {
            continue;
        };
        for item in &file.items {
            added += pool_items(item, krate, pool);
        }
    }
    added
}

/// Pool the methods of one item, recursing into inline `mod { .. }` blocks.
fn pool_items(item: &syn::Item, krate: &Crate, pool: &mut InherentPool) -> usize {
    match item {
        syn::Item::Impl(im) => pool_impl(im, krate, pool),
        syn::Item::Mod(m) => m
            .content
            .as_ref()
            .map(|(_, items)| items.iter().map(|i| pool_items(i, krate, pool)).sum())
            .unwrap_or(0),
        _ => 0,
    }
}

/// Pool one `impl` block when it is inherent and written for a slice or a
/// primitive.
fn pool_impl(im: &syn::ItemImpl, krate: &Crate, pool: &mut InherentPool) -> usize {
    // A trait impl (`impl Join<&str> for [S]`) adds no inherent method.
    if im.trait_.is_some() || is_test_only(&im.attrs) {
        return 0;
    }
    let impl_generics = generic_names(&im.generics);
    let self_ty = convert_type(&im.self_ty, &impl_generics, None);
    let key = match &self_ty {
        Type::Slice(_) => "[]".to_string(),
        Type::Primitive(p) => format!("prim:{p}"),
        _ => return 0,
    };
    let mut added = 0usize;
    for member in &im.items {
        let syn::ImplItem::Fn(f) = member else { continue };
        if !matches!(f.vis, syn::Visibility::Public(_)) || !is_kept_stable(&f.attrs) {
            continue;
        }
        let Some(func) = convert_fn(&f.sig, &impl_generics, &self_ty) else {
            continue;
        };
        let name = f.sig.ident.to_string();
        let mut sf = crate::ingest::map_function(krate, &name, &func);
        sf.is_static = false;
        sf.doc = first_doc_line(&f.attrs);
        pool.entry(key.clone()).or_default().push(sf);
        added += 1;
    }
    added
}

/// Whether a method's attributes make it part of the stable surface:
/// `#[stable(..)]` present, and no `#[unstable(..)]`, `#[doc(hidden)]` or
/// `#[cfg(test)]`.
fn is_kept_stable(attrs: &[syn::Attribute]) -> bool {
    let mut stable = false;
    for a in attrs {
        let path = a.path();
        if path.is_ident("stable") {
            stable = true;
        } else if path.is_ident("unstable") {
            return false;
        } else if path.is_ident("doc") && attr_list_mentions(a, "hidden") {
            return false;
        }
    }
    stable && !is_test_only(attrs)
}

/// `#[cfg(test)]`: code that exists only when `alloc` tests itself.
fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("cfg") && attr_list_mentions(a, "test"))
}

/// Whether a list attribute (`#[doc(hidden)]`, `#[cfg(test)]`) names `word` as
/// one of its bare arguments.
fn attr_list_mentions(a: &syn::Attribute, word: &str) -> bool {
    let mut found = false;
    let _ = a.parse_nested_meta(|meta| {
        if meta.path.is_ident(word) {
            found = true;
        }
        // Skip a value (`feature = ".."`) without failing the walk.
        if meta.input.peek(syn::Token![=]) {
            let _: syn::Expr = meta.value()?.parse()?;
        }
        Ok(())
    });
    found
}

/// The first line of the `///` doc comment, as rustdoc would give it.
fn first_doc_line(attrs: &[syn::Attribute]) -> Option<String> {
    for a in attrs {
        if !a.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &a.meta {
            if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = &nv.value {
                let line = s.value().trim().to_string();
                if !line.is_empty() {
                    return Some(line);
                }
            }
        }
    }
    None
}

/// The type-parameter names a generics list declares.
fn generic_names(g: &syn::Generics) -> Vec<String> {
    g.type_params().map(|p| p.ident.to_string()).collect()
}

/// Rebuild a method signature as the rustdoc [`Function`] rustdoc writes for
/// it. `None` for a receiver the deref'd type cannot use (`self: Box<Self>`,
/// by-value `self` on an unsized type) or no receiver at all.
fn convert_fn(sig: &syn::Signature, impl_generics: &[String], self_ty: &Type) -> Option<Function> {
    let mut in_scope: Vec<String> = impl_generics.to_vec();
    in_scope.extend(generic_names(&sig.generics));
    let mut inputs: Vec<(String, Type)> = Vec::new();
    let mut has_receiver = false;
    for arg in &sig.inputs {
        match arg {
            syn::FnArg::Receiver(r) => {
                // Only `&self` / `&mut self`: the forms a value reached
                // through `Deref` can be called with. rustdoc writes the
                // receiver as a borrow of the generic `Self`.
                let syn::Type::Reference(rf) = r.ty.as_ref() else {
                    return None;
                };
                if !matches!(rf.elem.as_ref(), syn::Type::Path(p) if p.path.is_ident("Self")) {
                    return None;
                }
                has_receiver = true;
                inputs.push((
                    "self".to_string(),
                    Type::BorrowedRef {
                        lifetime: None,
                        is_mutable: rf.mutability.is_some(),
                        type_: Box::new(Type::Generic("Self".to_string())),
                    },
                ));
            }
            syn::FnArg::Typed(pt) => {
                let name = match pt.pat.as_ref() {
                    syn::Pat::Ident(pi) => pi.ident.to_string(),
                    _ => "arg".to_string(),
                };
                inputs.push((name, convert_type(&pt.ty, &in_scope, Some(self_ty))));
            }
        }
    }
    if !has_receiver {
        return None;
    }
    let output = match &sig.output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, t) => Some(convert_type(t, &in_scope, Some(self_ty))),
    };
    Some(Function {
        sig: FunctionSignature { inputs, output, is_c_variadic: sig.variadic.is_some() },
        generics: convert_generics(&sig.generics, &in_scope, self_ty),
        header: FunctionHeader {
            is_const: sig.constness.is_some(),
            is_unsafe: sig.unsafety.is_some(),
            is_async: sig.asyncness.is_some(),
            abi: Abi::Rust,
        },
        has_body: true,
    })
}

/// A method's generic parameters and `where` clause, as rustdoc writes them.
fn convert_generics(g: &syn::Generics, in_scope: &[String], self_ty: &Type) -> Generics {
    let mut params: Vec<GenericParamDef> = Vec::new();
    for p in &g.params {
        match p {
            syn::GenericParam::Type(tp) => params.push(GenericParamDef {
                name: tp.ident.to_string(),
                kind: GenericParamDefKind::Type {
                    bounds: tp.bounds.iter().filter_map(|b| convert_bound(b, in_scope, self_ty)).collect(),
                    default: None,
                    is_synthetic: false,
                },
            }),
            syn::GenericParam::Lifetime(lp) => params.push(GenericParamDef {
                name: format!("'{}", lp.lifetime.ident),
                kind: GenericParamDefKind::Lifetime { outlives: Vec::new() },
            }),
            syn::GenericParam::Const(cp) => params.push(GenericParamDef {
                name: cp.ident.to_string(),
                kind: GenericParamDefKind::Const {
                    type_: convert_type(&cp.ty, in_scope, Some(self_ty)),
                    default: None,
                },
            }),
        }
    }
    let mut where_predicates: Vec<WherePredicate> = Vec::new();
    if let Some(wc) = &g.where_clause {
        for pred in &wc.predicates {
            if let syn::WherePredicate::Type(pt) = pred {
                where_predicates.push(WherePredicate::BoundPredicate {
                    type_: convert_type(&pt.bounded_ty, in_scope, Some(self_ty)),
                    bounds: pt.bounds.iter().filter_map(|b| convert_bound(b, in_scope, self_ty)).collect(),
                    generic_params: Vec::new(),
                });
            }
        }
    }
    Generics { params, where_predicates }
}

/// A trait bound (`F: FnMut(&T) -> K`, `K: Ord`, `?Sized`) as rustdoc writes
/// it. Lifetime bounds become `Outlives`; anything else is dropped.
fn convert_bound(b: &syn::TypeParamBound, in_scope: &[String], self_ty: &Type) -> Option<GenericBound> {
    match b {
        syn::TypeParamBound::Trait(tb) => Some(GenericBound::TraitBound {
            trait_: convert_path(&tb.path, in_scope, Some(self_ty)),
            generic_params: Vec::new(),
            modifier: match tb.modifier {
                syn::TraitBoundModifier::Maybe(_) => TraitBoundModifier::Maybe,
                syn::TraitBoundModifier::None => TraitBoundModifier::None,
            },
        }),
        syn::TypeParamBound::Lifetime(l) => Some(GenericBound::Outlives(format!("'{}", l.ident))),
        _ => None,
    }
}

/// A written path (`Vec<T>`, `FnMut(&T, &T) -> Ordering`) as a rustdoc
/// [`Path`]. The id is a placeholder no crate index holds: the mapping reads
/// a source-built path by its written name, which is all it has.
fn convert_path(p: &syn::Path, in_scope: &[String], self_ty: Option<&Type>) -> Path {
    let path = p
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    let args = p.segments.last().and_then(|last| match &last.arguments {
        syn::PathArguments::None => None,
        syn::PathArguments::AngleBracketed(ab) => {
            let args: Vec<GenericArg> = ab
                .args
                .iter()
                .filter_map(|a| match a {
                    syn::GenericArgument::Type(t) => Some(GenericArg::Type(convert_type(t, in_scope, self_ty))),
                    syn::GenericArgument::Lifetime(l) => Some(GenericArg::Lifetime(format!("'{}", l.ident))),
                    _ => None,
                })
                .collect();
            Some(Box::new(GenericArgs::AngleBracketed { args, constraints: Vec::new() }))
        }
        syn::PathArguments::Parenthesized(pa) => Some(Box::new(GenericArgs::Parenthesized {
            inputs: pa.inputs.iter().map(|t| convert_type(t, in_scope, self_ty)).collect(),
            output: match &pa.output {
                syn::ReturnType::Default => None,
                syn::ReturnType::Type(_, t) => Some(convert_type(t, in_scope, self_ty)),
            },
        })),
    });
    Path { path, id: Id(u32::MAX), args }
}

/// A written Rust type as the rustdoc [`Type`] rustdoc would record.
///
/// `self_ty` is the impl's own type: `Self` inside a signature is written as
/// rustdoc writes it, the generic `Self`, and the mapping substitutes it the
/// same way for both sources. A shape the mapping could not use anyway (a bare
/// `fn` pointer, a macro type) becomes `Infer`, which it reads as unknown.
fn convert_type(t: &syn::Type, in_scope: &[String], self_ty: Option<&Type>) -> Type {
    match t {
        syn::Type::Path(tp) => {
            // `<Self as Join<Sep>>::Output`: a projection.
            if let Some(q) = &tp.qself {
                let name = tp.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
                let trait_segments: syn::Path = syn::Path {
                    leading_colon: None,
                    segments: tp.path.segments.iter().take(q.position).cloned().collect(),
                };
                return Type::QualifiedPath {
                    name,
                    args: None,
                    self_type: Box::new(convert_type(&q.ty, in_scope, self_ty)),
                    trait_: (q.position > 0).then(|| convert_path(&trait_segments, in_scope, self_ty)),
                };
            }
            if tp.path.segments.len() == 1 {
                let seg = &tp.path.segments[0];
                let ident = seg.ident.to_string();
                if matches!(seg.arguments, syn::PathArguments::None) {
                    if ident == "Self" || in_scope.contains(&ident) {
                        return Type::Generic(ident);
                    }
                    if RUST_PRIMITIVES.contains(&ident.as_str()) {
                        return Type::Primitive(ident);
                    }
                }
            }
            Type::ResolvedPath(convert_path(&tp.path, in_scope, self_ty))
        }
        syn::Type::Reference(r) => Type::BorrowedRef {
            lifetime: r.lifetime.as_ref().map(|l| format!("'{}", l.ident)),
            is_mutable: r.mutability.is_some(),
            type_: Box::new(convert_type(&r.elem, in_scope, self_ty)),
        },
        syn::Type::Slice(s) => Type::Slice(Box::new(convert_type(&s.elem, in_scope, self_ty))),
        syn::Type::Array(a) => Type::Array {
            type_: Box::new(convert_type(&a.elem, in_scope, self_ty)),
            len: "N".to_string(),
        },
        syn::Type::Tuple(tt) => Type::Tuple(tt.elems.iter().map(|e| convert_type(e, in_scope, self_ty)).collect()),
        syn::Type::Ptr(p) => Type::RawPointer {
            is_mutable: p.mutability.is_some(),
            type_: Box::new(convert_type(&p.elem, in_scope, self_ty)),
        },
        syn::Type::ImplTrait(it) => Type::ImplTrait(
            it.bounds
                .iter()
                .filter_map(|b| self_ty.and_then(|st| convert_bound(b, in_scope, st)))
                .collect(),
        ),
        syn::Type::Paren(p) => convert_type(&p.elem, in_scope, self_ty),
        syn::Type::Group(g) => convert_type(&g.elem, in_scope, self_ty),
        syn::Type::Never(_) => Type::Primitive("never".to_string()),
        _ => Type::Infer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny `alloc`-shaped source: one incoherent slice impl, one `str` impl,
    /// a trait impl and a private helper. Only the stable `&self`/`&mut self`
    /// methods of the inherent impls come out.
    const SAMPLE: &str = r#"
        impl<T> [T] {
            /// Sorts the slice.
            #[rustc_allow_incoherent_impl]
            #[stable(feature = "rust1", since = "1.0.0")]
            pub fn sort(&mut self) where T: Ord {}

            #[rustc_allow_incoherent_impl]
            #[stable(feature = "slice_sort_by_key", since = "1.7.0")]
            pub fn sort_by_key<K, F>(&mut self, mut f: F) where F: FnMut(&T) -> K, K: Ord {}

            #[rustc_allow_incoherent_impl]
            #[stable(feature = "rust1", since = "1.0.0")]
            pub fn to_vec(&self) -> Vec<T> where T: Clone { todo!() }

            #[rustc_allow_incoherent_impl]
            #[stable(feature = "rust1", since = "1.0.0")]
            pub fn into_vec(self: Box<Self>) -> Vec<T> { todo!() }

            #[rustc_allow_incoherent_impl]
            #[unstable(feature = "nope", issue = "none")]
            pub fn unstable_one(&self) {}

            #[rustc_allow_incoherent_impl]
            #[stable(feature = "rust1", since = "1.0.0")]
            pub fn join<Separator>(&self, sep: Separator) -> <Self as Join<Separator>>::Output
            where Self: Join<Separator> { todo!() }

            fn private_helper(&self) {}
        }
        impl str {
            #[rustc_allow_incoherent_impl]
            #[stable(feature = "rust1", since = "1.0.0")]
            pub fn to_uppercase(&self) -> String { todo!() }
        }
        impl<S: Borrow<str>> Join<&str> for [S] {
            type Output = String;
            fn join(slice: &Self, sep: &str) -> String { todo!() }
        }
    "#;

    fn empty_crate() -> Crate {
        serde_json::from_str(
            r#"{"root":0,"crate_version":null,"includes_private":false,"index":{},"paths":{},
                "external_crates":{},"target":{"triple":"x","target_features":[]},"format_version":57}"#,
        )
        .expect("minimal crate")
    }

    #[test]
    fn stable_receiver_methods_of_slice_and_str_impls_are_pooled() {
        let mut pool = InherentPool::new();
        let n = collect_source_pool(&[("slice.rs", SAMPLE)], &empty_crate(), &mut pool);
        let names = |key: &str| -> Vec<String> {
            pool.get(key).map(|v| v.iter().map(|f| f.name.clone()).collect()).unwrap_or_default()
        };
        assert_eq!(names("[]"), vec!["sort", "sort_by_key", "to_vec", "join"]);
        assert_eq!(names("prim:str"), vec!["to_uppercase"]);
        assert_eq!(n, 5);
        let sort = &pool["[]"][0];
        assert!(sort.is_mut_self, "`&mut self` must reach the backend as a mutating call");
        assert_eq!(sort.doc.as_deref(), Some("Sorts the slice."));
        assert!(!pool["[]"][2].is_mut_self, "to_vec reads the slice");
    }

    #[test]
    fn a_file_without_the_incoherent_marker_is_not_read() {
        let mut pool = InherentPool::new();
        let text = SAMPLE.replace(INCOHERENT_MARKER, "inline");
        assert_eq!(collect_source_pool(&[("x.rs", &text)], &empty_crate(), &mut pool), 0);
        assert!(pool.is_empty());
    }
}
