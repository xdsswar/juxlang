//! Git-hosted Jux dependencies (`JUX-BUILD-SYSTEM-ADDENDUM.md` §B.2.2).
//!
//! A `[dependencies]` entry of the form
//!
//! ```toml
//! "com.example.json" = { git = "https://github.com/user/repo", branch = "main" }
//! ```
//!
//! is fetched into a user-level cache directory; once on disk the
//! package is indistinguishable from a `path` dependency, so the whole
//! existing inter-module machinery (`collect_dep_closure`, `PathDep`,
//! topological ordering, Cargo emission) reuses unchanged.
//!
//! ## Mechanism: the `git` CLI, not libgit2
//!
//! Fetching shells out to the user's installed `git`. That keeps
//! `juxc` free of a heavy native dependency AND inherits the user's
//! credential helpers — **private repositories work with whatever
//! authentication `git clone` already has** (SSH agent, credential
//! manager, tokens). A clear diagnostic tells the user to install git
//! when it's missing from `PATH`.
//!
//! ## Cache layout & refresh policy
//!
//! ```text
//! <JUX_HOME>/git/<repo-stem>-<hash8>/   # one checkout per (url, ref)
//! ```
//!
//! `JUX_HOME` defaults to `~/.jux` (`%USERPROFILE%\.jux` on Windows)
//! and is overridable via the environment. The hash folds the URL and
//! the pinned ref, so the same repo at two branches caches twice.
//!
//! - A cached checkout is reused as-is on later builds — deterministic
//!   and offline-friendly (Phase 1 has no lockfile yet; the cache IS
//!   the pin).
//! - `jux update` (or any caller passing `refresh = true`) re-fetches:
//!   the checkout is discarded and cloned fresh, picking up new
//!   commits on branch-pinned deps. `rev`/`tag` pins are immutable in
//!   practice but refresh the same way (cheap, shallow).
//! - When a refresh fails but a cached copy exists, the cache is used
//!   with a warning instead of failing the build (network resilience).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use crate::manifest::{Dependency, GitRef};

/// The Jux user directory — `$JUX_HOME`, else `~/.jux`.
pub fn jux_home() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("JUX_HOME") {
        if !home.trim().is_empty() {
            return Ok(PathBuf::from(home));
        }
    }
    // `USERPROFILE` on Windows, `HOME` everywhere else; checking both
    // keeps the function platform-independent (and testable under
    // either convention).
    let base = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| anyhow!("cannot determine the home directory (set JUX_HOME to override)"))?;
    Ok(PathBuf::from(base).join(".jux"))
}

/// Cache directory for one `(url, ref)` pair:
/// `<JUX_HOME>/git/<repo-stem>-<hash8>`.
pub fn git_dep_cache_dir(url: &str, git_ref: Option<&GitRef>) -> Result<PathBuf> {
    let stem = repo_stem(url);
    let key = match git_ref {
        Some(r) => format!("{url}#{}", r.describe()),
        None => url.to_string(),
    };
    Ok(jux_home()?
        .join("git")
        .join(format!("{stem}-{:08x}", fnv1a_64(&key) as u32)))
}

/// Fetch (or reuse) the checkout for a git dependency, returning the
/// local directory that now behaves like a `path` dependency root.
///
/// `refresh = false`: a cached checkout short-circuits — no network.
/// `refresh = true`: discard and re-clone; fall back to the stale
/// cache (with a warning) if the network fetch fails.
pub fn fetch_git_dep(dep: &Dependency, refresh: bool) -> Result<PathBuf> {
    let url = dep
        .git
        .as_deref()
        .ok_or_else(|| anyhow!("dependency `{}` has no git URL", dep.name))?;
    let dir = git_dep_cache_dir(url, dep.git_ref.as_ref())?;

    if dir.join(".git").is_dir() {
        if !refresh {
            return validated(dep, &dir);
        }
        // Refresh requested: simplest correct shape is re-clone into a
        // temp sibling, then swap — but a plain remove+clone is fine
        // for Phase 1 (the fallback below covers the failure window).
        let fresh = clone_into_temp(dep, url, &dir);
        match fresh {
            Ok(()) => return validated(dep, &dir),
            Err(e) => {
                eprintln!(
                    "juxc: warning: refreshing `{}` from {url} failed ({e:#}); using the cached copy",
                    dep.name
                );
                return validated(dep, &dir);
            }
        }
    }

    clone_into_temp(dep, url, &dir).with_context(|| {
        format!(
            "fetching git dependency `{}` from {url}{}",
            dep.name,
            dep.git_ref
                .as_ref()
                .map(|r| format!(" ({})", r.describe()))
                .unwrap_or_default()
        )
    })?;
    validated(dep, &dir)
}

/// Clone `url` (at the dep's pinned ref) into `dir`, going through a
/// temporary sibling directory so a half-finished clone never poses as
/// a valid cache entry.
fn clone_into_temp(dep: &Dependency, url: &str, dir: &Path) -> Result<()> {
    ensure_git_available()?;
    let parent = dir
        .parent()
        .ok_or_else(|| anyhow!("cache dir has no parent: {}", dir.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating cache directory {}", parent.display()))?;
    let tmp = parent.join(format!(
        "{}.tmp",
        dir.file_name().and_then(|n| n.to_str()).unwrap_or("dep")
    ));
    let _ = std::fs::remove_dir_all(&tmp);

    match &dep.git_ref {
        // Branch / tag pins ride `git clone --branch` (works for both);
        // no pin clones the remote's default branch. `--depth 1` keeps
        // the checkout shallow — sources are all we need.
        None => run_git(&["clone", "--depth", "1", url, path_str(&tmp)?], None)?,
        Some(GitRef::Branch(b)) | Some(GitRef::Tag(b)) => run_git(
            &["clone", "--depth", "1", "--branch", b, url, path_str(&tmp)?],
            None,
        )?,
        // An exact commit can't be `clone --branch`ed; init + fetch the
        // single rev + checkout FETCH_HEAD. GitHub (and any server with
        // `uploadpack.allowReachableSHA1InWant`) serves this shallowly.
        Some(GitRef::Rev(rev)) => {
            std::fs::create_dir_all(&tmp)
                .with_context(|| format!("creating {}", tmp.display()))?;
            run_git(&["init", "--quiet"], Some(&tmp))?;
            run_git(&["remote", "add", "origin", url], Some(&tmp))?;
            run_git(&["fetch", "--depth", "1", "origin", rev], Some(&tmp))?;
            run_git(&["checkout", "--quiet", "FETCH_HEAD"], Some(&tmp))?;
        }
    }

    // Atomic-ish swap: drop any stale checkout, move the fresh one in.
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&tmp, dir)
        .with_context(|| format!("installing checkout into {}", dir.display()))?;
    Ok(())
}

/// A fetched checkout must actually BE a Jux package.
fn validated(dep: &Dependency, dir: &Path) -> Result<PathBuf> {
    if !dir.join("jux.toml").is_file() {
        bail!(
            "git dependency `{}` ({}) has no jux.toml at its root — not a Jux package",
            dep.name,
            dep.git.as_deref().unwrap_or("?"),
        );
    }
    Ok(dir.to_path_buf())
}

/// Run one git command, surfacing stderr in the error message.
fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow!("`git` is not installed or not on PATH — git dependencies need the git CLI")
        } else {
            anyhow!("failed to spawn git: {e}")
        }
    })?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().unwrap_or(&"?"),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

fn ensure_git_available() -> Result<()> {
    run_git(&["--version"], None)
}

fn path_str(p: &Path) -> Result<&str> {
    p.to_str()
        .ok_or_else(|| anyhow!("non-UTF8 cache path: {}", p.display()))
}

/// The last path-ish segment of a repo URL, sans `.git`, sanitized for
/// use in a directory name: `https://github.com/u/my-lib.git` →
/// `my-lib`.
fn repo_stem(url: &str) -> String {
    let tail = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("dep");
    let tail = tail.strip_suffix(".git").unwrap_or(tail);
    let cleaned: String = tail
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "dep".to_string()
    } else {
        cleaned
    }
}

/// Deterministic 64-bit FNV-1a — a stable cache-key hash with no extra
/// dependency (std's `DefaultHasher` isn't guaranteed stable across
/// releases, and a cache keyed on it would silently re-clone after a
/// toolchain upgrade).
fn fnv1a_64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_stem_strips_git_suffix_and_sanitizes() {
        assert_eq!(repo_stem("https://github.com/u/my-lib.git"), "my-lib");
        assert_eq!(repo_stem("https://github.com/u/my-lib"), "my-lib");
        assert_eq!(repo_stem("git@github.com:u/weird name.git"), "weird_name");
        assert_eq!(repo_stem(""), "dep");
    }

    #[test]
    fn cache_key_distinguishes_refs() {
        let a = git_dep_cache_dir("https://x/r.git", None).unwrap();
        let b = git_dep_cache_dir(
            "https://x/r.git",
            Some(&GitRef::Branch("dev".to_string())),
        )
        .unwrap();
        let c = git_dep_cache_dir(
            "https://x/r.git",
            Some(&GitRef::Rev("abc".to_string())),
        )
        .unwrap();
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn fnv_is_stable() {
        // The empty-string value is the FNV-1a 64 offset basis — the
        // cache layout depends on this hash never changing.
        assert_eq!(fnv1a_64(""), 0xcbf2_9ce4_8422_2325);
        assert_ne!(fnv1a_64("a"), fnv1a_64("b"));
    }
}
