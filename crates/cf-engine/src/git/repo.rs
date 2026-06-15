//! Repository discovery and root resolution (Idea §3, §6; task 2.6).
//!
//! Uses gitoxide (`gix`) — pure Rust, so the hot path and the static-binary
//! distribution stay free of a C git dependency (Idea §10). `gix::discover`
//! walks up to find the repository and resolves work-tree / common-dir for
//! worktrees and submodules.

use std::path::{Path, PathBuf};

use gix::hash::ObjectId;

use cf_core::error::{CfError, CfResult};

/// A discovered git repository and its work-directory root.
pub struct Repo {
    inner: gix::Repository,
    workdir: PathBuf,
}

impl Repo {
    /// Discovers the repository containing `start` (Idea §6 repo-root resolution).
    ///
    /// # Errors
    /// Returns [`CfError::Git`] if no repository is found or it is bare.
    pub fn discover(start: &Path) -> CfResult<Repo> {
        let inner = gix::discover(start).map_err(|e| {
            CfError::git(format!("discovering repository at {}", start.display())).caused_by(e)
        })?;
        let workdir = inner
            .workdir()
            .ok_or_else(|| CfError::git("repository is bare (no work directory)"))?
            .to_path_buf();
        Ok(Repo { inner, workdir })
    }

    /// The work-directory root (the repo root for a normal checkout).
    #[must_use]
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// The underlying gitoxide handle.
    pub(crate) fn inner(&self) -> &gix::Repository {
        &self.inner
    }

    /// The HEAD commit's object id.
    ///
    /// # Errors
    /// Returns [`CfError::Git`] if HEAD has no commit yet (an empty repository).
    pub fn head_commit_id(&self) -> CfResult<ObjectId> {
        let head = self
            .inner
            .head_id()
            .map_err(|e| CfError::git("resolving HEAD commit").caused_by(e))?;
        Ok(head.detach())
    }
}
