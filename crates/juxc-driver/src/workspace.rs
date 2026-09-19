//! Workspace-level manifest behavior (JUX-BUILD-SYSTEM-ADDENDUM §B.7).
//!
//! Three things a `[workspace]` root gives its members, beyond the member
//! list itself:
//!
//! - **Member patterns.** `members = ["pkg-a", "tools/*"]`: an entry may hold
//!   `*` / `?` wildcards, one path segment at a time, and expands to every
//!   matching directory that carries a `jux.toml`. `exclude` removes entries
//!   (exact paths or patterns) from that expansion.
//! - **Default members.** `default-members` is what a bare `jux build` at the
//!   root builds (§B.7.2). Absent, it is every member.
//! - **Inheritance.** A member writes `edition.workspace = true` in
//!   `[package]`, or `"com.x.json".workspace = true` in `[dependencies]`, and
//!   takes the value from the root's `[workspace.package]` /
//!   `[workspace.dependencies]`.
//!
//! Inheritance is applied to the raw TOML *before* the manifest is
//! deserialized, so every later stage sees ordinary values and nothing else
//! in the driver needs to know the `workspace = true` form exists.

use std::path::{Path, PathBuf};

/// Expand `[workspace] members` patterns against `root`, then drop anything
/// `exclude` names. The result keeps the order the members were written in
/// (a pattern contributes its matches alphabetically), has no duplicates, and
/// uses `/` separators relative to `root`.
///
/// A plain entry (no wildcard) is kept as written even if its directory does
/// not exist, so the build reports the missing member by name rather than
/// silently skipping it. A pattern only yields directories that hold a
/// `jux.toml`: `tools/*` must not turn a stray `tools/scripts/` folder into a
/// broken member.
pub fn expand_members(root: &Path, members: &[String], exclude: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in members {
        let entry = normalize(entry);
        if has_wildcard(&entry) {
            for dir in expand_pattern(root, &entry) {
                if root.join(&dir).join("jux.toml").is_file() {
                    out.push(dir);
                }
            }
        } else {
            out.push(entry);
        }
    }
    let exclude: Vec<String> = exclude.iter().map(|e| normalize(e)).collect();
    out.retain(|m| !exclude.iter().any(|e| path_matches(e, m)));
    dedup_in_order(out)
}

/// Remove repeats, keeping each entry's first position.
fn dedup_in_order(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|i| seen.insert(i.clone())).collect()
}

/// Resolve `default-members` against the expanded member list. Each entry
/// may be a pattern too; an entry that names no member is reported on stderr
/// (a typo there would otherwise quietly build nothing). An empty or absent
/// `default-members` means "every member".
pub fn expand_default_members(
    root: &Path,
    default_members: &[String],
    members: &[String],
) -> Vec<String> {
    if default_members.is_empty() {
        return members.to_vec();
    }
    let mut out: Vec<String> = Vec::new();
    for entry in default_members {
        let entry = normalize(entry);
        let hits: Vec<&String> = members.iter().filter(|m| path_matches(&entry, m)).collect();
        if hits.is_empty() {
            eprintln!(
                "jux: warning: `default-members` entry `{entry}` in {} is not a workspace member",
                root.join("jux.toml").display(),
            );
        }
        out.extend(hits.into_iter().cloned());
    }
    dedup_in_order(out)
}

/// Replace every `key.workspace = true` in `value`'s `[package]` and
/// `[dependencies]` tables with the root workspace's value for that key.
///
/// The workspace root is the nearest directory at or above `project_root`
/// whose `jux.toml` has a `[workspace]` table (a root package may inherit from
/// its own workspace, as in Cargo). A reference with nothing to inherit from
/// is reported on stderr and the key is dropped, which leaves the manifest in
/// the state it would have had without the key.
pub fn inherit_from_workspace(value: &mut toml::Value, project_root: &Path) {
    if !mentions_workspace_inheritance(value) {
        return;
    }
    let workspace = find_workspace_table(project_root);
    let Some(table) = value.as_table_mut() else {
        return;
    };

    // `[package]` keys: `edition.workspace = true` becomes the plain value of
    // `[workspace.package] edition`.
    if let Some(toml::Value::Table(pkg)) = table.get_mut("package") {
        let shared = workspace
            .as_ref()
            .and_then(|w| w.get("package"))
            .and_then(|p| p.as_table());
        let keys: Vec<String> = pkg.keys().cloned().collect();
        for key in keys {
            if !is_inherit_marker(&pkg[&key]) {
                continue;
            }
            match shared.and_then(|s| s.get(&key)) {
                Some(v) => {
                    pkg.insert(key, v.clone());
                }
                None => {
                    warn_missing(project_root, &format!("[workspace.package] {key}"));
                    pkg.remove(&key);
                }
            }
        }
    }

    // `[dependencies]` entries: `"x".workspace = true` becomes the root's
    // `[workspace.dependencies] "x"`, with the member's own `features` added on
    // top (Cargo's rule: a member may widen features, never narrow them).
    if let Some(toml::Value::Table(deps)) = table.get_mut("dependencies") {
        let shared = workspace
            .as_ref()
            .and_then(|w| w.get("dependencies"))
            .and_then(|d| d.as_table());
        let names: Vec<String> = deps.keys().cloned().collect();
        for name in names {
            if !is_inherit_marker(&deps[&name]) {
                continue;
            }
            let Some(base) = shared.and_then(|s| s.get(&name)) else {
                warn_missing(project_root, &format!("[workspace.dependencies] \"{name}\""));
                deps.remove(&name);
                continue;
            };
            let mut merged = match base {
                // A bare version string is the table `{ version = "..." }`.
                toml::Value::String(v) => {
                    let mut t = toml::map::Map::new();
                    t.insert("version".into(), toml::Value::String(v.clone()));
                    t
                }
                toml::Value::Table(t) => t.clone(),
                other => {
                    let mut t = toml::map::Map::new();
                    t.insert("version".into(), other.clone());
                    t
                }
            };
            // Paths in the root's table are relative to the ROOT, but the
            // member resolves its dependency paths against itself.
            if let (Some(toml::Value::String(p)), Some(root)) =
                (merged.get("path").cloned(), workspace_root_dir(project_root))
            {
                let abs = root.join(&p);
                merged.insert("path".into(), toml::Value::String(abs.display().to_string()));
            }
            if let Some(toml::Value::Table(own)) = deps.get(&name) {
                if let Some(toml::Value::Array(extra)) = own.get("features") {
                    let mut features = match merged.get("features") {
                        Some(toml::Value::Array(a)) => a.clone(),
                        _ => Vec::new(),
                    };
                    for f in extra {
                        if !features.contains(f) {
                            features.push(f.clone());
                        }
                    }
                    merged.insert("features".into(), toml::Value::Array(features));
                }
            }
            deps.insert(name, toml::Value::Table(merged));
        }
    }
}

/// True when `v` is the `{ workspace = true, ... }` marker.
fn is_inherit_marker(v: &toml::Value) -> bool {
    v.as_table()
        .and_then(|t| t.get("workspace"))
        .and_then(|w| w.as_bool())
        .unwrap_or(false)
}

/// Cheap pre-check so ordinary manifests never go looking for a root.
fn mentions_workspace_inheritance(value: &toml::Value) -> bool {
    let Some(table) = value.as_table() else {
        return false;
    };
    ["package", "dependencies"].iter().any(|section| {
        table
            .get(*section)
            .and_then(|s| s.as_table())
            .is_some_and(|s| s.values().any(is_inherit_marker))
    })
}

/// The `[workspace]` table of the nearest workspace root at or above
/// `project_root`.
fn find_workspace_table(project_root: &Path) -> Option<toml::map::Map<String, toml::Value>> {
    let dir = workspace_root_dir(project_root)?;
    let text = std::fs::read_to_string(dir.join("jux.toml")).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    value.get("workspace")?.as_table().cloned()
}

/// The nearest directory at or above `project_root` whose `jux.toml` declares
/// a `[workspace]` table.
pub fn workspace_root_dir(project_root: &Path) -> Option<PathBuf> {
    let mut dir = Some(project_root);
    while let Some(d) = dir {
        if let Ok(text) = std::fs::read_to_string(d.join("jux.toml")) {
            if let Ok(value) = toml::from_str::<toml::Value>(&text) {
                if value.get("workspace").is_some_and(|w| w.is_table()) {
                    return Some(d.to_path_buf());
                }
            }
        }
        dir = d.parent();
    }
    None
}

fn warn_missing(project_root: &Path, what: &str) {
    eprintln!(
        "jux: warning: {} inherits `{what}` from its workspace, but no workspace root defines it",
        project_root.join("jux.toml").display(),
    );
}

/// `./tools/*/` → `tools/*`: forward slashes, no leading `./`, no trailing `/`.
fn normalize(entry: &str) -> String {
    let s = entry.replace('\\', "/");
    let s = s.trim_start_matches("./").trim_end_matches('/');
    s.to_string()
}

fn has_wildcard(s: &str) -> bool {
    s.contains('*') || s.contains('?')
}

/// Every directory under `root` whose relative path matches `pattern`,
/// segment by segment. Hidden directories and `target/` are never members.
fn expand_pattern(root: &Path, pattern: &str) -> Vec<String> {
    let segments: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let mut found: Vec<String> = Vec::new();
    walk_pattern(root, "", &segments, &mut found);
    found
}

fn walk_pattern(base: &Path, prefix: &str, segments: &[&str], out: &mut Vec<String>) {
    let Some((first, rest)) = segments.split_first() else {
        if !prefix.is_empty() {
            out.push(prefix.to_string());
        }
        return;
    };
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        }
    };
    if !has_wildcard(first) {
        let next = base.join(first);
        if next.is_dir() {
            walk_pattern(&next, &join(first), rest, out);
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| !n.starts_with('.') && n != "target")
        .collect();
    names.sort();
    for name in names {
        if segment_matches(first, &name) {
            walk_pattern(&base.join(&name), &join(&name), rest, out);
        }
    }
}

/// Does the (possibly wildcarded) relative path `pattern` name `path`?
fn path_matches(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let q: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    p.len() == q.len() && p.iter().zip(&q).all(|(a, b)| segment_matches(a, b))
}

/// Shell-style match of one path segment: `*` is any run of characters, `?`
/// is exactly one.
fn segment_matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    // Classic two-pointer wildcard match with one backtrack point.
    let (mut i, mut j) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while j < n.len() {
        if i < p.len() && (p[i] == '?' || p[i] == n[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some((i, j));
            i += 1;
        } else if let Some((si, sj)) = star {
            i = si + 1;
            j = sj + 1;
            star = Some((si, sj + 1));
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory for one test, removed afterwards.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let p = std::env::temp_dir().join(format!(
                "jux-ws-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0),
            ));
            std::fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
        fn member(&self, rel: &str) {
            let d = self.0.join(rel);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("jux.toml"), "[package]\nname = \"m\"\n").unwrap();
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn segment_wildcards() {
        assert!(segment_matches("*", "codegen"));
        assert!(segment_matches("pkg-?", "pkg-a"));
        assert!(!segment_matches("pkg-?", "pkg-ab"));
        assert!(segment_matches("*-cli", "tool-cli"));
        assert!(!segment_matches("*-cli", "tool-cli-x"));
    }

    /// `tools/*` finds the member directories and skips one with no manifest;
    /// `exclude` removes by exact path and by pattern.
    #[test]
    fn members_expand_and_exclude() {
        let s = Scratch::new("expand");
        s.member("pkg-a");
        s.member("pkg-experimental");
        s.member("tools/codegen");
        s.member("tools/lint");
        std::fs::create_dir_all(s.0.join("tools/scripts")).unwrap();
        let members = vec!["pkg-*".to_string(), "tools/*".to_string()];
        let got = expand_members(&s.0, &members, &["pkg-experimental".to_string()]);
        assert_eq!(got, vec!["pkg-a", "tools/codegen", "tools/lint"]);
        let got = expand_members(&s.0, &members, &["tools/*".to_string()]);
        assert_eq!(got, vec!["pkg-a", "pkg-experimental"]);
    }

    #[test]
    fn default_members_default_to_all() {
        let s = Scratch::new("defaults");
        let all = vec!["a".to_string(), "b".to_string()];
        assert_eq!(expand_default_members(&s.0, &[], &all), all);
        assert_eq!(expand_default_members(&s.0, &["b".to_string()], &all), vec!["b"]);
    }

    /// A member inherits `[workspace.package]` keys and `[workspace.dependencies]`
    /// entries, keeping its own extra features.
    #[test]
    fn inheritance_from_the_root() {
        let s = Scratch::new("inherit");
        std::fs::write(
            s.0.join("jux.toml"),
            "[workspace]\nmembers = [\"app\"]\n\n[workspace.package]\nedition = \"2026\"\nlicense = \"MIT\"\n\n[workspace.dependencies]\n\"com.x.json\" = { version = \"1.0\", features = [\"a\"] }\n\"com.x.util\" = \"2.0\"\n",
        )
        .unwrap();
        let member = s.0.join("app");
        std::fs::create_dir_all(&member).unwrap();
        let mut v: toml::Value = toml::from_str(
            "[package]\nname = \"app\"\nedition.workspace = true\nlicense.workspace = true\n\n[dependencies]\n\"com.x.json\" = { workspace = true, features = [\"b\"] }\n\"com.x.util\".workspace = true\n",
        )
        .unwrap();
        inherit_from_workspace(&mut v, &member);
        assert_eq!(v["package"]["edition"].as_str(), Some("2026"));
        assert_eq!(v["package"]["license"].as_str(), Some("MIT"));
        assert_eq!(v["dependencies"]["com.x.util"]["version"].as_str(), Some("2.0"));
        let features: Vec<&str> = v["dependencies"]["com.x.json"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|f| f.as_str())
            .collect();
        assert_eq!(features, vec!["a", "b"]);
    }
}
