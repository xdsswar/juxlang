//! Which types have `operator hash`, and which containers need it.
//!
//! One analysis, shared by the checker and the backend the way
//! [`SymbolTable::worker_share_blocker`] is, so the two can never disagree:
//! the checker reports `E0933` where a type with no hash is used as a key, and
//! the backend derives (or writes) `Hash` for exactly the records, structs and
//! enums this module calls hashable.
//!
//! The rule is JUX-OPERATORS-ADDENDUM §O.3.1 / §O.2.7: a record, struct or
//! enum derives `operator hash` from its components unless one of them has
//! none. Everything has one except
//!
//! - a function value,
//! - an array or a collection (a shared handle whose contents change under
//!   it; Rust gives neither `Hash`),
//! - a floating-point value used as a key BY ITSELF (Rust's `f64` is not
//!   `Eq`). As a component of a record, struct or enum a float hashes by its
//!   bits, `0.0` and `-0.0` alike, which is what keeps `Point(double x,
//!   double y)` (the §O.3.1 example) a key,
//! - a record, struct or enum that deletes `operator hash`, and a class that
//!   declares `operator<=>` without `operator hash`.
//!
//! A class otherwise hashes by identity, or by its own `operator hash`. A type
//! parameter is assumed hashable here: the backend bounds it (§T.2.1), and the
//! instantiation is what gets checked.

use std::collections::HashSet;

use juxc_ast::OperatorKind;

use crate::symbol_table::SymbolTable;
use crate::ty::{Primitive, Ty};

/// The bound a keyed container puts on its key type parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyBound {
    /// `HashMap` / `HashSet`: the key needs `Eq + Hash` (`operator==` and
    /// `operator hash`).
    Hash,
    /// `BTreeMap` / `BTreeSet`: the key needs `Ord` (`operator<=>`).
    Ord,
}

/// The key bound a container type puts on its first type argument, by the
/// container's name (the last path segment). `None` for anything that is not
/// a keyed container.
///
/// The one table of these: the backend's §T.2.1 key-bound inference and the
/// checker's `E0933` both read it.
pub fn key_bound_of(container: &str) -> Option<KeyBound> {
    match container.rsplit('.').next().unwrap_or(container) {
        "HashMap" | "HashSet" => Some(KeyBound::Hash),
        "BTreeMap" | "BTreeSet" => Some(KeyBound::Ord),
        _ => None,
    }
}

/// True for the floating-point primitives, which Rust gives no `Hash`.
pub fn is_float_primitive(p: Primitive) -> bool {
    matches!(p, Primitive::Float | Primitive::Double | Primitive::F32 | Primitive::F64)
}

impl SymbolTable {
    /// Why a value of `ty` cannot be a hash key, or `None` when it can.
    ///
    /// The reason is written to finish the sentence "... cannot be a hash key
    /// because ...", naming the component that has no hash, so the diagnostic
    /// can point at the field the programmer has to change.
    pub fn hash_key_blocker(&self, ty: &Ty) -> Option<String> {
        let ty = match ty {
            Ty::Nullable(inner) => inner.as_ref(),
            other => other,
        };
        // A float is a key only as a component: by itself it has no `Eq`.
        if let Ty::Primitive(p) = ty {
            if is_float_primitive(*p) {
                return Some(format!(
                    "a `{ty}` has no hash by itself (floating-point values are not a key); \
                     a record or struct field of type `{ty}` hashes by its bits"
                ));
            }
        }
        self.hash_blocker(ty)
    }

    /// Why `ty` has no `operator hash`, or `None` when it has one. Unlike
    /// [`Self::hash_key_blocker`], a float counts as hashable: this is the
    /// question a record asks about its components.
    pub fn hash_blocker(&self, ty: &Ty) -> Option<String> {
        self.hash_blocker_in(ty, &mut HashSet::new())
    }

    /// The name of a record, struct or enum whose derived `operator hash`
    /// has no blocker: the backend asks this before deriving `Hash`.
    pub fn declaration_is_hashable(&self, fqn: &str) -> bool {
        let generic_args: Vec<Ty> = self
            .records
            .get(fqn)
            .map(|r| r.generic_params.iter().map(|p| Ty::Param(p.name.text.clone())).collect())
            .or_else(|| {
                self.classes
                    .get(fqn)
                    .map(|c| c.generic_params.iter().map(|p| Ty::Param(p.name.text.clone())).collect())
            })
            .unwrap_or_default();
        self.hash_blocker(&Ty::User { name: fqn.to_string(), generic_args }).is_none()
    }

    fn hash_blocker_in(&self, ty: &Ty, visiting: &mut HashSet<String>) -> Option<String> {
        match ty {
            Ty::Nullable(inner) => self.hash_blocker_in(inner, visiting),
            Ty::Primitive(_) | Ty::String | Ty::Param(_) | Ty::Unknown | Ty::Void | Ty::Wildcard(_) | Ty::Never => None,
            Ty::Array { .. } => Some("an array has no hash (it is a shared handle whose elements can change)".into()),
            Ty::Fn { .. } | Ty::FnPtr { .. } => Some("a function value has no hash".into()),
            // `any` supports only `===`, `=>` and its text (§T.1.2).
            Ty::Any => Some("an `any` has no hash (it supports only `===` and `=>`)".into()),
            Ty::User { name, generic_args } => self.user_hash_blocker(name, generic_args, visiting),
        }
    }

    /// [`Self::hash_blocker`] for a named type.
    fn user_hash_blocker(&self, name: &str, args: &[Ty], visiting: &mut HashSet<String>) -> Option<String> {
        // A type met again while its own components are being checked is
        // hashable as far as this walk can tell: whatever blocks it is found
        // on the first visit.
        if !visiting.insert(name.to_string()) {
            return None;
        }
        let result = self.user_hash_blocker_uncached(name, args, visiting);
        visiting.remove(name);
        result
    }

    fn user_hash_blocker_uncached(&self, name: &str, args: &[Ty], visiting: &mut HashSet<String>) -> Option<String> {
        let bare = name.rsplit('.').next().unwrap_or(name);
        if let Some((fqn, record)) = self.lookup_by(name, &self.records) {
            if let Some(verdict) = declared_hash(&record.operators, bare) {
                return verdict;
            }
            for component in &record.components {
                let declared = crate::ty::lower_member_type(&component.ty, fqn, self);
                let actual = crate::ty::substitute(&declared, &record.generic_params, args);
                if let Some(why) = self.component_blocker(&declared, &actual, visiting) {
                    return Some(format!("its component `{bare}.{}` {why}", component.name));
                }
            }
            return None;
        }
        if let Some((fqn, en)) = self.lookup_by(name, &self.enums) {
            if en.is_external {
                return None;
            }
            if let Some(verdict) = declared_hash(&en.operators, bare) {
                return verdict;
            }
            // Deterministic: report the first offender by variant name.
            let mut variants: Vec<_> = en.variants.iter().collect();
            variants.sort_by(|a, b| a.0.cmp(b.0));
            for (variant, sig) in variants {
                for (i, payload) in sig.payload.iter().enumerate() {
                    let declared = crate::ty::lower_member_type(payload, fqn, self);
                    if let Some(why) = self.component_blocker(&declared, &declared, visiting) {
                        return Some(format!("payload {} of `{bare}.{variant}` {why}", i + 1));
                    }
                }
            }
            return None;
        }
        if let Some((fqn, class)) = self.resolve_class(name) {
            if class.is_external {
                // A collection is a shared handle (§6.5.1) with no hash; any
                // other foreign type answers for itself.
                if self.type_is_rust_clone(fqn)
                    && class.annotations.iter().any(|a| {
                        a.name.segments.len() == 1 && a.name.segments[0].text.eq_ignore_ascii_case("rustcollection")
                    })
                {
                    return Some("a collection has no hash (it is a shared handle whose contents can change)".into());
                }
                return None;
            }
            if class.is_struct {
                if let Some(verdict) = declared_hash(&class.operators, bare) {
                    return verdict;
                }
                // Deterministic: fields are a map, so walk them by name.
                let mut fields: Vec<_> = class.fields.iter().filter(|(_, f)| !f.is_static).collect();
                fields.sort_by(|a, b| a.0.cmp(b.0));
                for (field, sig) in fields {
                    let declared = crate::ty::lower_member_type(&sig.ty, fqn, self);
                    let actual = crate::ty::substitute(&declared, &class.generic_params, args);
                    if let Some(why) = self.component_blocker(&declared, &actual, visiting) {
                        return Some(format!("its field `{bare}.{field}` {why}"));
                    }
                }
                return None;
            }
            // A class hashes by identity unless its chain declares equality of
            // its own; then it needs its own `operator hash` (E0931 makes
            // `operator==` bring one, so only `<=>` can leave it without).
            let mut cursor = Some(class);
            let mut hops = 0;
            while let Some(c) = cursor {
                if c.operators.get(&OperatorKind::Hash).is_some_and(|o| !o.is_deleted) {
                    // Its own hash, which a key needs paired with its own `==`.
                    if !c.operators.contains_key(&OperatorKind::Eq) && !c.operators.contains_key(&OperatorKind::Cmp) {
                        return Some(format!("`{bare}` declares `operator hash` but no `operator==`"));
                    }
                    return None;
                }
                if c.operators.contains_key(&OperatorKind::Cmp) {
                    return Some(format!(
                        "`{bare}` declares `operator<=>` but no `operator hash`, so it has no hash"
                    ));
                }
                hops += 1;
                if hops > 64 {
                    break;
                }
                cursor = c
                    .extends_fqn
                    .as_deref()
                    .or_else(|| c.extends.as_ref().and_then(|t| t.name.segments.last()).map(|s| s.text.as_str()))
                    .and_then(|p| self.resolve_class(p))
                    .map(|(_, sig)| sig);
            }
            return None;
        }
        // Interfaces hash by identity; anything unknown answers for itself.
        None
    }

    /// Why one component of a record, struct or enum has no hash. `declared`
    /// is its type as written, `actual` after substituting the type arguments.
    ///
    /// A float component hashes by its bits, but only when the FIELD is a
    /// float: a type parameter that happens to be `double` is hashed through
    /// its own `Hash`, which a float does not have.
    fn component_blocker(&self, declared: &Ty, actual: &Ty, visiting: &mut HashSet<String>) -> Option<String> {
        let strip = |t: &Ty| -> Ty {
            match t {
                Ty::Nullable(inner) => inner.as_ref().clone(),
                other => other.clone(),
            }
        };
        if let (Ty::Param(param), Ty::Primitive(p)) = (strip(declared), strip(actual)) {
            if is_float_primitive(p) {
                return Some(format!(
                    "is the type parameter `{param}`, here `{float}`, and a floating-point type argument \
                     has no hash (a field declared `{float}` hashes by its bits)",
                    float = Ty::Primitive(p),
                ));
            }
        }
        // A derived hash hashes every component through its own `Hash`, so a
        // component whose type is foreign or unresolved blocks it: nothing
        // here knows whether that type has one. (Used as a key directly, a
        // foreign type answers for itself.)
        match strip(actual) {
            Ty::Unknown => return Some("has a type that could not be resolved, so its hash is not known".into()),
            Ty::User { ref name, .. } if self.is_foreign_type(name) => {
                return Some(format!(
                    "is `{}`, a foreign type, and a derived hash cannot tell whether it has one",
                    strip(actual)
                ));
            }
            _ => {}
        }
        self.hash_blocker_in(actual, visiting).map(|why| format!("is `{actual}`, and {why}"))
    }

    /// True when `name` is not a Jux-declared type: a bindgen stub (other than
    /// a collection, which [`Self::hash_blocker`] reports on its own terms) or
    /// a name no table knows.
    fn is_foreign_type(&self, name: &str) -> bool {
        if let Some((_, class)) = self.resolve_class(name) {
            let collection = class.annotations.iter().any(|a| {
                a.name.segments.len() == 1 && a.name.segments[0].text.eq_ignore_ascii_case("rustcollection")
            });
            return class.is_external && !collection;
        }
        if let Some((_, en)) = self.lookup_by(name, &self.enums) {
            return en.is_external;
        }
        if let Some((_, iface)) = self.lookup_by(name, &self.interfaces) {
            return iface.is_external;
        }
        self.lookup_by(name, &self.records).is_none()
    }

    /// Find a declaration by FQN key or by a unique bare-name suffix, the rule
    /// [`Self::resolve_class`] applies to classes.
    fn lookup_by<'a, V>(
        &'a self,
        name: &str,
        map: &'a std::collections::HashMap<String, V>,
    ) -> Option<(&'a String, &'a V)> {
        if let Some(kv) = map.get_key_value(name) {
            return Some(kv);
        }
        if name.contains('.') {
            return None;
        }
        let suffix = format!(".{name}");
        let mut hits = map.iter().filter(|(k, _)| k.ends_with(&suffix));
        match (hits.next(), hits.next()) {
            (Some(kv), None) => Some(kv),
            _ => None,
        }
    }
}

/// What a declaration's own operators say about its hash: `Some(None)` when it
/// declares `operator hash`, `Some(Some(why))` when it deletes it, `None` when
/// it says nothing and the components decide.
fn declared_hash(
    operators: &std::collections::HashMap<OperatorKind, crate::symbol_table::OperatorSig>,
    bare: &str,
) -> Option<Option<String>> {
    let op = operators.get(&OperatorKind::Hash)?;
    Some(op.is_deleted.then(|| format!("`{bare}` deletes `operator hash`")))
}
