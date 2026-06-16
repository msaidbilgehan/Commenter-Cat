//! Changed-file sets via tree diff (Idea §3, §7; task 2.6).
//!
//! The git-hook cache-warmer and CI scan only a commit's changed files
//! (`git diff-tree`), so these helpers expose the repo-relative paths that
//! changed between two commits, or in a single commit versus its first parent.

use std::collections::BTreeSet;

use gix::hash::ObjectId;
use gix::object::tree::diff::ChangeDetached;

use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};

use super::repo::Repo;

/// Repo-relative paths added/modified/deleted between two commits' trees.
///
/// # Errors
/// Returns [`CommenterCatError::Git`] if a commit or tree cannot be read, or the diff fails.
pub fn changed_between(
    repo: &Repo,
    old: ObjectId,
    new: ObjectId,
) -> CommenterCatResult<Vec<String>> {
    let inner = repo.inner();
    let old_commit = inner
        .find_commit(old)
        .map_err(|e| CommenterCatError::git(format!("reading commit {old}")).caused_by(e))?;
    let new_commit = inner
        .find_commit(new)
        .map_err(|e| CommenterCatError::git(format!("reading commit {new}")).caused_by(e))?;
    let old_tree = old_commit
        .tree()
        .map_err(|e| CommenterCatError::git(format!("reading tree of {old}")).caused_by(e))?;
    let new_tree = new_commit
        .tree()
        .map_err(|e| CommenterCatError::git(format!("reading tree of {new}")).caused_by(e))?;

    let changes = inner
        .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
        .map_err(|e| CommenterCatError::git("diffing commit trees").caused_by(e))?;
    Ok(collect_paths(&changes))
}

/// Repo-relative paths changed in `commit` versus its first parent (versus the
/// empty tree for a root commit, so every file counts as added).
///
/// # Errors
/// Returns [`CommenterCatError::Git`] if a commit or tree cannot be read, or the diff fails.
pub fn changed_in_commit(repo: &Repo, commit: ObjectId) -> CommenterCatResult<Vec<String>> {
    let inner = repo.inner();
    let commit_obj = inner
        .find_commit(commit)
        .map_err(|e| CommenterCatError::git(format!("reading commit {commit}")).caused_by(e))?;
    let new_tree = commit_obj
        .tree()
        .map_err(|e| CommenterCatError::git(format!("reading tree of {commit}")).caused_by(e))?;

    let parent = commit_obj.parent_ids().next();
    let old_tree = match parent {
        Some(parent_id) => {
            let parent_commit = inner
                .find_commit(parent_id.detach())
                .map_err(|e| CommenterCatError::git("reading parent commit").caused_by(e))?;
            Some(
                parent_commit
                    .tree()
                    .map_err(|e| CommenterCatError::git("reading parent tree").caused_by(e))?,
            )
        }
        None => None,
    };

    let changes = inner
        .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)
        .map_err(|e| CommenterCatError::git("diffing against parent").caused_by(e))?;
    Ok(collect_paths(&changes))
}

/// Extracts the sorted, de-duplicated set of changed paths.
fn collect_paths(changes: &[ChangeDetached]) -> Vec<String> {
    let mut paths: BTreeSet<String> = BTreeSet::new();
    for change in changes {
        paths.insert(change_location(change));
    }
    paths.into_iter().collect()
}

/// The repo-relative location of a change (the new path).
fn change_location(change: &ChangeDetached) -> String {
    match change {
        ChangeDetached::Addition { location, .. }
        | ChangeDetached::Deletion { location, .. }
        | ChangeDetached::Modification { location, .. }
        | ChangeDetached::Rewrite { location, .. } => location.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn test_changed_in_commit_reports_only_modified_file() {
        let fixture = TestRepo::new();
        fixture.write("a.py", "# a\nx = 1\n");
        fixture.write("b.py", "# b\ny = 1\n");
        fixture.commit_all("2020-01-01 00:00:00 +0000");
        fixture.write("a.py", "# a\nx = 2\n");
        fixture.commit_all("2021-01-01 00:00:00 +0000");

        let repo = Repo::discover(fixture.path()).unwrap();
        let head = repo.head_commit_id().unwrap();
        assert_eq!(changed_in_commit(&repo, head).unwrap(), vec!["a.py"]);
    }

    #[test]
    fn test_root_commit_counts_all_files_added() {
        let fixture = TestRepo::new();
        fixture.write("a.py", "# a\n");
        fixture.write("b.py", "# b\n");
        fixture.commit_all("2020-01-01 00:00:00 +0000");

        let repo = Repo::discover(fixture.path()).unwrap();
        let head = repo.head_commit_id().unwrap();
        assert_eq!(
            changed_in_commit(&repo, head).unwrap(),
            vec!["a.py", "b.py"]
        );
    }
}
