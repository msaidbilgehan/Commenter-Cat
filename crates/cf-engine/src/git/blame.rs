//! Git blame (Idea §3; task 2.6).
//!
//! Blames a file at HEAD and exposes, per 1-based line, the commit that last
//! introduced it. Commit metadata is resolved once per commit and cached, since
//! a blame hunk covers many lines from the same commit.

use std::collections::HashMap;

use gix::bstr::BStr;
use gix::hash::ObjectId;

use cf_core::error::{CfError, CfResult};

use super::repo::Repo;

/// Blame attribution for a line: which commit, by whom, and when.
#[derive(Debug, Clone)]
pub struct LineBlame {
    /// The hex commit hash that introduced the line.
    pub commit_id: String,
    /// The commit author's name.
    pub author: String,
    /// The commit author's email, if present.
    pub email: Option<String>,
    /// The commit time as Unix seconds (UTC) — comparable for blame-skew (2.7).
    pub committed_unix: i64,
}

/// Per-file blame: 1-based line → attribution.
#[derive(Debug, Default)]
pub struct FileBlame {
    lines: HashMap<u32, LineBlame>,
}

impl FileBlame {
    /// The attribution for a 1-based line, if known.
    #[must_use]
    pub fn at_line(&self, line: u32) -> Option<&LineBlame> {
        self.lines.get(&line)
    }

    /// The most recently committed attribution across an inclusive 1-based line
    /// range — i.e. the last time anything in the range changed.
    #[must_use]
    pub fn most_recent_in_range(&self, start_line: u32, end_line: u32) -> Option<&LineBlame> {
        (start_line..=end_line)
            .filter_map(|line| self.lines.get(&line))
            .max_by_key(|blame| blame.committed_unix)
    }
}

/// Blames `repo_path` (repo-relative, `/`-separated) at the given HEAD commit.
///
/// # Errors
/// Returns [`CfError::Git`] if the file cannot be blamed (e.g. it is untracked
/// at HEAD) or a referenced commit cannot be read.
pub fn blame_file(repo: &Repo, repo_path: &str, head: ObjectId) -> CfResult<FileBlame> {
    let path = BStr::new(repo_path);
    let outcome = repo
        .inner()
        .blame_file(path, head, gix::repository::blame_file::Options::default())
        .map_err(|e| CfError::git(format!("blaming {repo_path}")).caused_by(e))?;

    let mut commit_cache: HashMap<ObjectId, LineBlame> = HashMap::new();
    let mut lines = HashMap::new();
    for entry in outcome.entries {
        let blame = match commit_cache.get(&entry.commit_id) {
            Some(cached) => cached.clone(),
            None => {
                let resolved = resolve_commit(repo, entry.commit_id)?;
                commit_cache.insert(entry.commit_id, resolved.clone());
                resolved
            }
        };
        // BlameEntry lines are 0-based; the comment record is 1-based.
        let first_line = entry.start_in_blamed_file + 1;
        for offset in 0..entry.len.get() {
            lines.insert(first_line + offset, blame.clone());
        }
    }
    Ok(FileBlame { lines })
}

/// Resolves a commit's author and time into a [`LineBlame`] template.
fn resolve_commit(repo: &Repo, commit_id: ObjectId) -> CfResult<LineBlame> {
    let commit = repo
        .inner()
        .find_commit(commit_id)
        .map_err(|e| CfError::git(format!("reading commit {commit_id}")).caused_by(e))?;
    let time = commit
        .time()
        .map_err(|e| CfError::git(format!("reading time of commit {commit_id}")).caused_by(e))?;
    let author = commit
        .author()
        .map_err(|e| CfError::git(format!("reading author of commit {commit_id}")).caused_by(e))?;

    let email = author.email.to_string();
    Ok(LineBlame {
        commit_id: commit_id.to_hex().to_string(),
        author: author.name.to_string(),
        email: (!email.is_empty()).then_some(email),
        committed_unix: time.seconds,
    })
}
