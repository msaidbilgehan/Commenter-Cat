//! Cache location resolution (Idea §6; task 4.8).
//!
//! The cache lives at `<repo_root>/.comment-finder/` — gitignored, local, and
//! rebuildable (shared truth is CI, Idea §6, §7). The repo root is resolved via
//! gitoxide (handling worktrees/submodules); outside a repo it falls back to the
//! scan root so queries still work with no git.

use std::fs;
use std::path::{Path, PathBuf};

use cf_core::error::{CfError, CfResult};

use crate::git::repo::Repo;

/// The gitignored per-project cache directory name (Idea §6).
pub const CACHE_DIR_NAME: &str = ".comment-finder";

/// The content-addressed input store filename.
pub const INPUTS_DB_FILE: &str = "inputs.db";

/// The derived query index filename.
pub const INDEX_DB_FILE: &str = "index.db";

/// Resolves the repo root for a scan rooted at `start`, falling back to `start`
/// when not inside a git repository.
#[must_use]
pub fn resolve_repo_root(start: &Path) -> PathBuf {
    Repo::discover(start).map_or_else(|_| start.to_path_buf(), |repo| repo.workdir().to_path_buf())
}

/// The cache directory for a repo root.
#[must_use]
pub fn cache_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(CACHE_DIR_NAME)
}

/// The `inputs.db` path for a repo root.
#[must_use]
pub fn inputs_db_path(repo_root: &Path) -> PathBuf {
    cache_dir(repo_root).join(INPUTS_DB_FILE)
}

/// The `index.db` path for a repo root.
#[must_use]
pub fn index_db_path(repo_root: &Path) -> PathBuf {
    cache_dir(repo_root).join(INDEX_DB_FILE)
}

/// Creates the cache directory if needed, returning it.
///
/// # Errors
/// Returns [`CfError::Storage`] if the directory cannot be created.
pub fn ensure_cache_dir(repo_root: &Path) -> CfResult<PathBuf> {
    let dir = cache_dir(repo_root);
    fs::create_dir_all(&dir).map_err(|e| {
        CfError::storage(format!("creating cache dir {}", dir.display())).caused_by(e)
    })?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn test_resolves_repo_root_and_cache_paths() {
        let fixture = TestRepo::new();
        fixture.write("pkg/x.py", "# c\n");
        fixture.commit_all("2020-01-01 00:00:00 +0000");

        let subdir = fixture.path().join("pkg");
        let root = resolve_repo_root(&subdir);
        // gitoxide may canonicalize symlinks (e.g. /var vs /private/var on macOS).
        assert!(root.ends_with(fixture.path().file_name().unwrap()) || root == fixture.path());

        let dir = ensure_cache_dir(&root).unwrap();
        assert!(dir.is_dir());
        assert!(dir.ends_with(CACHE_DIR_NAME));
        assert!(inputs_db_path(&root).ends_with("inputs.db"));
        assert!(index_db_path(&root).ends_with("index.db"));
    }

    #[test]
    fn test_falls_back_outside_a_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        // A plain temp dir is not a git repo → fall back to the start path.
        assert_eq!(resolve_repo_root(dir.path()), dir.path());
    }
}
