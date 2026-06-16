//! The provider-result cache (Idea §6; task 4.3).
//!
//! Keyed on `(content_hash, provider, provider_version)`, a hit means **the
//! provider need not re-run on this unchanged file** — re-running the linters is
//! the dominant cost, so this is the load-bearing performance lever (Idea §6,
//! §7). The cache lives in `inputs.db`; the DB is only a cache, so queries stay
//! correct with zero hooks installed (scanner-on-demand, Idea §6).

use commenter_cat_core::error::CommenterCatResult;

use super::inputs_db::{CachedProviderResult, InputsDb};

/// A thin, content-addressed view over `inputs.db`'s provider-result store.
pub struct ProviderCache<'a> {
    inputs: &'a InputsDb,
}

impl<'a> ProviderCache<'a> {
    /// Binds a cache to an input store.
    #[must_use]
    pub fn new(inputs: &'a InputsDb) -> Self {
        Self { inputs }
    }

    /// Looks up a cached provider run for a file's content hash. `Some` ⇒ skip
    /// the provider for this file (Idea §6).
    ///
    /// # Errors
    /// Returns [`commenter_cat_core::CommenterCatError`] on a storage read error.
    pub fn get(
        &self,
        content_hash: &str,
        provider: &str,
        provider_version: &str,
    ) -> CommenterCatResult<Option<CachedProviderResult>> {
        self.inputs
            .provider_result(content_hash, provider, provider_version)
    }

    /// Stores a provider run against a file's content hash.
    ///
    /// # Errors
    /// Returns [`commenter_cat_core::CommenterCatError`] on a storage write error.
    pub fn put(
        &self,
        content_hash: &str,
        provider: &str,
        provider_version: &str,
        result: &CachedProviderResult,
    ) -> CommenterCatResult<()> {
        self.inputs
            .store_provider_result(content_hash, provider, provider_version, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::hashing::hash_file;
    use std::fs;

    fn result() -> CachedProviderResult {
        CachedProviderResult {
            run_state: "SUCCESS".to_owned(),
            findings_json: "[]".to_owned(),
        }
    }

    #[test]
    fn test_unchanged_file_hits_changed_file_misses() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.py");
        fs::write(&path, "# v1\nx = 1\n").unwrap();

        let inputs = InputsDb::open_in_memory().unwrap();
        let cache = ProviderCache::new(&inputs);

        let h1 = hash_file(&path).unwrap();
        cache.put(&h1, "ruff", "0.14.2", &result()).unwrap();

        // Same content (recomputing the hash) → cache hit, no re-run needed.
        assert_eq!(
            cache
                .get(&hash_file(&path).unwrap(), "ruff", "0.14.2")
                .unwrap(),
            Some(result())
        );

        // Change a byte → new content hash → cache miss (must re-run).
        fs::write(&path, "# v2\nx = 2\n").unwrap();
        let h2 = hash_file(&path).unwrap();
        assert_ne!(h2, h1);
        assert!(cache.get(&h2, "ruff", "0.14.2").unwrap().is_none());
    }

    #[test]
    fn test_mtime_only_change_still_hits() {
        use std::fs::File;
        use std::time::{Duration, SystemTime};

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.py");
        fs::write(&path, "# stable\n").unwrap();

        let inputs = InputsDb::open_in_memory().unwrap();
        let cache = ProviderCache::new(&inputs);
        let hash = hash_file(&path).unwrap();
        cache.put(&hash, "ruff", "0.14.2", &result()).unwrap();

        // Touch the file (mtime forward, identical bytes).
        let later = SystemTime::now() + Duration::from_secs(10_000);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();

        // Content hash is unchanged → still a hit (mtime is not the key).
        assert_eq!(
            cache
                .get(&hash_file(&path).unwrap(), "ruff", "0.14.2")
                .unwrap(),
            Some(result())
        );
    }
}
