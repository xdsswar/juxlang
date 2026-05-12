//! Tiny FQN helpers usable from the backend. Mirrors the helpers in
//! `juxc_tycheck::symbol_table` but kept here so the backend doesn't
//! need to import internal tycheck modules.

/// Strip the trailing identifier off an FQN. `"a.lib.Foo"` → `"Foo"`,
/// `"Foo"` → `"Foo"`.
pub(crate) fn fqn_bare(fqn: &str) -> &str {
    match fqn.rsplit_once('.') {
        Some((_, bare)) => bare,
        None => fqn,
    }
}

/// Return the package prefix of an FQN, or `None` for bare
/// (no-package) names. `"a.lib.Foo"` → `Some("a.lib")`,
/// `"Foo"` → `None`.
pub(crate) fn fqn_package(fqn: &str) -> Option<&str> {
    fqn.rsplit_once('.').map(|(pkg, _)| pkg)
}
