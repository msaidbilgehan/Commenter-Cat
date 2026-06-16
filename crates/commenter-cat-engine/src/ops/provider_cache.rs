//! The provider-result cache (Idea §6 — "the load-bearing performance lever").
//!
//! A provider never re-runs on unchanged input. Results live in the
//! content-addressed `inputs.db`, keyed `(content_hash, provider, version)`:
//!
//! * the **content_hash** is the hash of the provider's *input set* — the
//!   `commenter_cat_scope` universe for a project-scoped tool (gitleaks scans the whole
//!   tree, but commenter-cat's *retained* findings depend only on universe content, so that
//!   is the correct and sufficient key), or the comment-language file set for a
//!   file-scoped tool;
//! * the **version** is [`RuleProvider::version_key`] — the resolved tool
//!   binary's content folded with the provider config, so a tool upgrade or a
//!   config change invalidates the cache (Idea §5 comparability).
//!
//! The cache is **best-effort**: any failure (an unopenable store, an unreadable
//! file, an unresolved binary, an undeserializable row) degrades silently to
//! running the provider — never a hard error, never a stale-wrong result. Only
//! `SUCCESS`/`EMPTY` runs are cached; a `PARTIAL` has no trustworthy findings and
//! a `SKIPPED` never ran.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::hash::sha256_hex;
use crate::provider::{ProviderContext, ProviderRun, RuleProvider, RunState, Scope};
use crate::storage::inputs_db::{CachedProviderResult, InputsDb};
use crate::storage::location;
use crate::walk::to_repo_relative;

/// Provider-cache outcome for one `commenter-cat check`, surfaced by `--stats` (Idea §6).
#[derive(Debug, Default, Clone, Copy)]
pub struct CacheStats {
    /// Providers served from the cache (no subprocess re-run).
    pub hits: usize,
    /// Providers that ran (a cache miss, an uncacheable provider, or caching off).
    pub runs: usize,
}

/// The provider-result cache for one `commenter-cat check` run.
///
/// Holds the opened `inputs.db`, each provider's precomputed [`version_key`], and
/// the per-scope input-set hashes — computed once, only for the scopes a present
/// provider can actually use.
///
/// [`version_key`]: RuleProvider::version_key
pub(crate) struct ProviderCache {
    db: Option<InputsDb>,
    version_keys: BTreeMap<String, Option<String>>,
    project_input_hash: Option<String>,
    file_input_hash: Option<String>,
}

impl ProviderCache {
    /// Opens the cache and precomputes the input-set hashes the present providers
    /// need. Returns a **disabled** cache (every run is a miss, nothing stored)
    /// when `use_cache` is false or the store cannot be opened — caching never
    /// blocks a check.
    pub(crate) fn build(
        root: &Path,
        files: &[PathBuf],
        universe: &[String],
        providers: &[&dyn RuleProvider],
        use_cache: bool,
    ) -> Self {
        if !use_cache {
            return Self::disabled();
        }
        let Some(db) = open_db(root) else {
            return Self::disabled();
        };
        // One binary hash per provider, reused as the lookup key below.
        let version_keys: BTreeMap<String, Option<String>> = providers
            .iter()
            .map(|provider| (provider.id().to_owned(), provider.version_key()))
            .collect();
        let has_cacheable = |scope: Scope| {
            providers.iter().any(|provider| {
                provider.capabilities().scope == scope
                    && version_keys
                        .get(provider.id())
                        .is_some_and(|key| key.is_some())
            })
        };
        // Hash an input set only if some present provider of that scope can be
        // cached — never read the tree for nothing.
        let project_input_hash = has_cacheable(Scope::Project)
            .then(|| universe_hash(root, universe))
            .flatten();
        let file_input_hash = has_cacheable(Scope::File)
            .then(|| files_hash(root, files))
            .flatten();
        Self {
            db: Some(db),
            version_keys,
            project_input_hash,
            file_input_hash,
        }
    }

    fn disabled() -> Self {
        Self {
            db: None,
            version_keys: BTreeMap::new(),
            project_input_hash: None,
            file_input_hash: None,
        }
    }

    /// Returns the provider's findings + run state, served from the cache when its
    /// input set is unchanged, otherwise running it (and caching a usable result).
    /// The returned bool is `true` on a cache hit.
    pub(crate) fn run(
        &self,
        provider: &dyn RuleProvider,
        files: &[PathBuf],
        context: &ProviderContext<'_>,
    ) -> (ProviderRun, bool) {
        let Some(db) = &self.db else {
            return (provider.run(files, context), false);
        };
        let Some(Some(version)) = self.version_keys.get(provider.id()) else {
            return (provider.run(files, context), false);
        };
        let input_hash = match provider.capabilities().scope {
            Scope::Project => self.project_input_hash.as_deref(),
            Scope::File => self.file_input_hash.as_deref(),
        };
        let Some(input_hash) = input_hash else {
            return (provider.run(files, context), false);
        };
        if let Some(run) = lookup(db, input_hash, provider.id(), version) {
            return (run, true);
        }
        let run = provider.run(files, context);
        store(db, input_hash, provider.id(), version, &run);
        (run, false)
    }
}

/// Opens (creating the cache dir) the content-addressed store, or `None` on any
/// failure — the caller then disables caching for the run.
fn open_db(root: &Path) -> Option<InputsDb> {
    let path = location::inputs_db_path(root);
    std::fs::create_dir_all(path.parent()?).ok()?;
    InputsDb::open(&path).ok()
}

/// Reads a cached run, or `None` to (re)run on a miss or an unusable row.
fn lookup(db: &InputsDb, input_hash: &str, provider: &str, version: &str) -> Option<ProviderRun> {
    let cached = db.provider_result(input_hash, provider, version).ok()??;
    let state = RunState::from_token(&cached.run_state)?;
    let findings = serde_json::from_str(&cached.findings_json).ok()?;
    Some(ProviderRun { state, findings })
}

/// Caches a usable run — `SUCCESS`/`EMPTY` only (a `PARTIAL` has no trustworthy
/// findings, a `SKIPPED` never ran). Best-effort: a write error is swallowed.
fn store(db: &InputsDb, input_hash: &str, provider: &str, version: &str, run: &ProviderRun) {
    if !matches!(run.state, RunState::Success | RunState::Empty) {
        return;
    }
    let Ok(findings_json) = serde_json::to_string(&run.findings) else {
        return;
    };
    let _ = db.store_provider_result(
        input_hash,
        provider,
        version,
        &CachedProviderResult {
            run_state: run.state.as_str().to_owned(),
            findings_json,
        },
    );
}

/// The `commenter_cat_scope` tree hash — over the universe (every non-ignored file). `None`
/// if any file cannot be read (project caching is then skipped for the run).
fn universe_hash(root: &Path, universe: &[String]) -> Option<String> {
    let items: Vec<(String, PathBuf)> = universe
        .iter()
        .map(|rel| (rel.clone(), root.join(rel)))
        .collect();
    content_set_hash(&items)
}

/// The file-scoped input hash — over the comment-language file set.
fn files_hash(root: &Path, files: &[PathBuf]) -> Option<String> {
    let items: Vec<(String, PathBuf)> = files
        .iter()
        .map(|abs| (to_repo_relative(abs, root), abs.clone()))
        .collect();
    content_set_hash(&items)
}

/// Hashes a set of files into one stable digest: each file's content hash bound to
/// its repo-relative path, sorted for order-independence. `None` on any read
/// error (mtime is never consulted — content is the only truth, Idea §6).
fn content_set_hash(items: &[(String, PathBuf)]) -> Option<String> {
    let mut entries: Vec<(String, String)> = Vec::with_capacity(items.len());
    for (rel, abs) in items {
        let bytes = std::fs::read(abs).ok()?;
        entries.push((rel.clone(), sha256_hex(&bytes)));
    }
    entries.sort();
    let mut combined = String::new();
    for (rel, hash) in entries {
        combined.push_str(&rel);
        combined.push('\0');
        combined.push_str(&hash);
        combined.push('\n');
    }
    Some(sha256_hex(combined.as_bytes()))
}
