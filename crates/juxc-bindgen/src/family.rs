//! Crate families -- a bound crate and the crates it re-exports
//! (JUX-BINDGEN-ADDENDUM.md §G.6.2.4).
//!
//! A crate's public API is routinely made of OTHER crates: `eframe` is
//! `pub use egui;`, `egui` re-exports `emath::Rect` and `ecolor::Color32`, and
//! only `eframe` is linked by a program that names it. Three questions about
//! such a family are answered here, each from the rustdoc JSON alone:
//!
//! - **Which crates belong to it.** [`missing_type_crates`] finds the crates
//!   that define the types a stub mentions but does not declare, so `Color32`
//!   is found in `ecolor` even though only `egui` re-exports it.
//! - **How each member is reached through the host.** [`FamilyPaths`] follows
//!   the host's root re-exports, and those of the crates they lead to, so an
//!   `epaint::CornerRadius` is written `eframe::egui::CornerRadius` in a
//!   program that links `eframe` alone.
//! - **Which definition a shared name means.** When two members declare the
//!   same name (`accesskit::Rect` and `emath::Rect` both reach `egui`), the one
//!   the family PUBLISHES under that name wins ([`FamilyPaths::rank`]), which
//!   is the one Rust code importing it gets.

use std::collections::{HashMap, HashSet, VecDeque};

use rustdoc_types::{Crate, ItemEnum, ItemKind};

use crate::model::{StubFile, StubItem};

/// How the members of a crate family are reached through its host.
#[derive(Debug, Default, Clone)]
pub struct FamilyPaths {
    /// The host crate, as rustdoc names it (`eframe`, `tiny_skia`).
    host: String,
    /// `(defining crate, item name)` -> the shortest public path through the
    /// host: `("ecolor", "Color32")` -> `eframe::egui::Color32`.
    items: HashMap<(String, String), String>,
    /// A re-exported MODULE, by its definition path, and the public path it
    /// is reached by: `emath` -> `eframe::emath`. Longest first.
    modules: Vec<(String, String)>,
    /// How many re-exports deep a member crate sits below the host (0 for
    /// the host itself). A crate reached through no module re-export has none.
    depth: HashMap<String, usize>,
}

impl FamilyPaths {
    /// Read the host's re-exports, and those of every crate a MODULE
    /// re-export leads to, out of the family's rustdoc JSON. `jsons` pairs a
    /// crate name with its JSON; the host must be among them for anything to
    /// be found.
    pub fn read(host: &str, jsons: &[(&str, &str)]) -> Result<FamilyPaths, serde_json::Error> {
        let ident = |s: &str| s.replace('-', "_");
        let host = ident(host);
        let texts: HashMap<String, &str> = jsons.iter().map(|(n, j)| (ident(n), *j)).collect();
        let mut fam = FamilyPaths { host: host.clone(), ..FamilyPaths::default() };
        fam.depth.insert(host.clone(), 0);
        let mut queue: VecDeque<(String, String)> = VecDeque::new();
        queue.push_back((host.clone(), host.clone()));
        let mut visited: HashSet<String> = HashSet::new();
        while let Some((name, prefix)) = queue.pop_front() {
            if !visited.insert(name.clone()) {
                continue;
            }
            let Some(text) = texts.get(&name) else { continue };
            let krate: Crate = serde_json::from_str(text)?;
            let here = fam.depth.get(&name).copied().unwrap_or(0);
            for (target, public_name) in root_publications(&krate) {
                let Some(summary) = krate.paths.get(&target) else { continue };
                // Only what points OUT of this crate: its own items already
                // carry their own public paths.
                if summary.crate_id == 0 || summary.path.is_empty() {
                    continue;
                }
                let public = format!("{prefix}::{public_name}");
                let def = summary.path.join("::");
                if matches!(summary.kind, ItemKind::Module) {
                    keep_shorter_module(&mut fam.modules, def, public.clone());
                    // A whole CRATE re-exported as a module (`pub use egui;`)
                    // is a member whose own re-exports lead further.
                    if summary.path.len() == 1 {
                        let member = summary.path[0].clone();
                        let depth = fam.depth.entry(member.clone()).or_insert(here + 1);
                        *depth = (*depth).min(here + 1);
                        queue.push_back((member, public));
                    }
                } else {
                    let key = (summary.path[0].clone(), summary.path.last().cloned().unwrap_or_default());
                    let slot = fam.items.entry(key).or_insert_with(|| public.clone());
                    if (public.len(), public.as_str()) < (slot.len(), slot.as_str()) {
                        *slot = public;
                    }
                }
            }
        }
        fam.modules.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.cmp(b)));
        Ok(fam)
    }

    /// The host crate's name.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Whether the host PUBLISHES anything of `krate`: the crate is reached
    /// through a chain of at most two module re-exports (`eframe` ->
    /// `egui` -> `ecolor`), or some member re-exports one of its items by
    /// name. Only such a crate is part of the host's API; the rest of the
    /// dependency graph (a graphics backend's `windows` bindings, say) is an
    /// implementation detail even when a signature deep inside mentions it.
    pub fn publishes(&self, krate: &str) -> bool {
        let krate = krate.replace('-', "_");
        self.depth.get(&krate).is_some_and(|d| *d <= 2) || self.items.keys().any(|(c, _)| *c == krate)
    }

    /// The path a program linking only the host writes for the item whose
    /// own crate calls it `rust_path` (`ecolor::Color32`), or `None` when the
    /// item is the host's own or the family publishes no path to it.
    pub fn public_path(&self, rust_path: &str) -> Option<String> {
        let mut segs = rust_path.split("::");
        let krate = segs.next()?;
        if krate == self.host {
            return None;
        }
        let name = rust_path.rsplit("::").next()?;
        if let Some(p) = self.items.get(&(krate.to_string(), name.to_string())) {
            return Some(p.clone());
        }
        self.modules.iter().find_map(|(def, public)| {
            if rust_path == def {
                Some(public.clone())
            } else {
                rust_path
                    .strip_prefix(def.as_str())
                    .filter(|rest| rest.starts_with("::"))
                    .map(|rest| format!("{public}{rest}"))
            }
        })
    }

    /// How strongly the family means `krate`'s item called `name` when
    /// several members declare that name; lower wins. The host's own item
    /// first, then one a member publishes under that very name (egui's
    /// `pub use emath::Rect`), then the member closest to the host.
    pub fn rank(&self, krate: &str, name: &str) -> (u8, usize) {
        if krate == self.host {
            return (0, 0);
        }
        if let Some(p) = self.items.get(&(krate.to_string(), name.to_string())) {
            return (1, p.len());
        }
        (2, self.depth.get(krate).copied().unwrap_or(usize::MAX))
    }
}

/// Keep `(def, public)` in `modules`, replacing a longer public path for the
/// same definition.
fn keep_shorter_module(modules: &mut Vec<(String, String)>, def: String, public: String) {
    match modules.iter_mut().find(|(d, _)| *d == def) {
        Some(slot) if public.len() < slot.1.len() => slot.1 = public,
        Some(_) => {}
        None => modules.push((def, public)),
    }
}

/// Every `(target, name)` a crate's ROOT module publishes: its `pub use`
/// items (globs expanded) and the items it declares there.
fn root_publications(krate: &Crate) -> Vec<(rustdoc_types::Id, String)> {
    let Some(root) = krate.index.get(&krate.root) else { return Vec::new() };
    let ItemEnum::Module(m) = &root.inner else { return Vec::new() };
    m.items
        .iter()
        .filter_map(|id| krate.index.get(id))
        .flat_map(|item| crate::ingest::published_names(krate, item))
        .collect()
}

/// The crates that define types `stub` mentions but does not declare, read
/// from each JSON's `paths` table: `egui`'s signatures say `Color32`, and its
/// JSON records that `Color32` is `ecolor`'s. `known` are the crates already
/// in the family; the standard library's own crates are never named.
pub fn missing_type_crates(
    jsons: &[(&str, &str)],
    stub: &StubFile,
    known: &[String],
) -> Result<Vec<String>, serde_json::Error> {
    let mut declared: HashSet<String> = HashSet::new();
    let mut referenced: HashSet<String> = HashSet::new();
    for item in &stub.items {
        match item {
            StubItem::Type(t) => {
                declared.insert(t.name.clone());
            }
            StubItem::Alias(a) => {
                declared.insert(a.name.clone());
            }
            _ => {}
        }
        item.referenced_type_names(&mut referenced);
    }
    let missing: HashSet<&String> = referenced.iter().filter(|n| !declared.contains(*n)).collect();
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let skip: HashSet<&str> =
        ["std", "core", "alloc", "proc_macro", "test"].into_iter().chain(known.iter().map(String::as_str)).collect();
    let mut out: Vec<String> = Vec::new();
    for (_, text) in jsons {
        let krate: Crate = serde_json::from_str(text)?;
        for summary in krate.paths.values() {
            if summary.crate_id == 0 {
                continue;
            }
            if !matches!(
                summary.kind,
                ItemKind::Struct | ItemKind::Enum | ItemKind::Trait | ItemKind::TypeAlias
            ) {
                continue;
            }
            let Some(last) = summary.path.last() else { continue };
            if !missing.contains(last) {
                continue;
            }
            let Some(ext) = krate.external_crates.get(&summary.crate_id) else { continue };
            if skip.contains(ext.name.as_str()) || out.contains(&ext.name) {
                continue;
            }
            out.push(ext.name.clone());
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-crate family in the shape of `eframe` -> `egui` -> `ecolor`, as
    /// rustdoc JSON text: the host re-exports `egui` as a module, and `egui`
    /// re-exports `ecolor::Color32` by name and `ecolor` itself as a module.
    fn json(root_items: &str, index: &str, paths: &str, external: &str) -> String {
        format!(
            r#"{{"root": 0, "crate_version": null, "includes_private": false,
                "index": {{"0": {{"id": 0, "crate_id": 0, "name": "root", "span": null,
                    "visibility": "public", "docs": null, "links": {{}}, "attrs": [],
                    "deprecation": null,
                    "inner": {{"module": {{"is_crate": true, "items": [{root_items}], "is_stripped": false}}}}}}
                    {index}}},
                "paths": {{{paths}}},
                "external_crates": {{{external}}},
                "target": {{"triple": "x86_64-unknown-linux-gnu", "target_features": []}},
                "format_version": 57}}"#
        )
    }

    fn use_item(id: u32, name: &str, target: u32) -> String {
        format!(
            r#", "{id}": {{"id": {id}, "crate_id": 0, "name": null, "span": null,
                "visibility": "public", "docs": null, "links": {{}}, "attrs": [],
                "deprecation": null,
                "inner": {{"use": {{"source": "{name}", "name": "{name}", "id": {target}, "is_glob": false}}}}}}"#
        )
    }

    #[test]
    fn a_member_is_reached_through_the_module_its_host_re_exports() {
        let host = json(
            "1",
            &use_item(1, "egui", 100),
            r#""100": {"crate_id": 1, "path": ["egui"], "kind": "module"}"#,
            r#""1": {"name": "egui", "html_root_url": null, "path": ""}"#,
        );
        let egui = json(
            "1, 2",
            &(use_item(1, "Color32", 200) + &use_item(2, "ecolor", 201)),
            r#""200": {"crate_id": 1, "path": ["ecolor", "color32", "Color32"], "kind": "struct"},
               "201": {"crate_id": 1, "path": ["ecolor"], "kind": "module"}"#,
            r#""1": {"name": "ecolor", "html_root_url": null, "path": ""}"#,
        );
        let fam = FamilyPaths::read("eframe", &[("eframe", &host), ("egui", &egui)]).unwrap();
        // An egui item: through the host's `pub use egui;`.
        assert_eq!(fam.public_path("egui::Ui").as_deref(), Some("eframe::egui::Ui"));
        // An ecolor item egui publishes BY NAME: the short path.
        assert_eq!(
            fam.public_path("ecolor::Color32").as_deref(),
            Some("eframe::egui::Color32")
        );
        // Any other ecolor item: through the module egui re-exports.
        assert_eq!(
            fam.public_path("ecolor::Rgba").as_deref(),
            Some("eframe::egui::ecolor::Rgba")
        );
        // The host's own items keep their paths.
        assert_eq!(fam.public_path("eframe::NativeOptions"), None);
        // A published name outranks a member that merely declares the name.
        assert!(fam.rank("ecolor", "Color32") < fam.rank("accesskit", "Color32"));
        assert!(fam.rank("eframe", "Theme") < fam.rank("egui", "Theme"));
    }
}
