//! Two-layer file discovery (Idea §5; task 6.1).
//!
//! `effective_scope = provider_filter(cf_scope)`. CF owns the **universe**: the
//! `[scan]` settings are authoritative `cf_scope`, and a provider **never** sees
//! a file CF excluded. But CF does **not force** a provider to analyze a file its
//! own config rejects (a local `.ruff.toml` ignore still applies), so the
//! provider keeps a *veto within* `cf_scope` — never the power to widen it.

use std::path::{Path, PathBuf};

/// Narrows the authoritative `cf_scope` by a provider's veto.
///
/// The provider may only remove files (its own config ignores); it can never add
/// one, so a CF-excluded file — absent from `cf_scope` — can never appear.
pub fn effective_scope(
    cf_scope: &[PathBuf],
    provider_veto: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    cf_scope
        .iter()
        .filter(|file| !provider_veto(file))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cf_scope_is_authoritative_and_provider_only_vetoes() {
        // CF already excluded "vendor/x.py" — it is simply not in cf_scope.
        let cf_scope = vec![
            PathBuf::from("src/a.py"),
            PathBuf::from("src/b.py"),
            PathBuf::from("gen/c.py"),
        ];
        // The provider's own config ignores anything under "gen/".
        let effective = effective_scope(&cf_scope, |p| p.starts_with("gen"));

        assert_eq!(
            effective,
            vec![PathBuf::from("src/a.py"), PathBuf::from("src/b.py")]
        );
        // A CF-excluded file can never reappear via the provider.
        assert!(!effective.iter().any(|p| p.starts_with("vendor")));
    }

    #[test]
    fn test_no_veto_keeps_everything() {
        let cf_scope = vec![PathBuf::from("a"), PathBuf::from("b")];
        assert_eq!(effective_scope(&cf_scope, |_| false), cf_scope);
    }
}
