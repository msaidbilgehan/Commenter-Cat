//! Git enrichment (Idea §3; task 2.6).
//!
//! Joins git blame onto comments: each comment gets the most recent commit that
//! touched its lines (author / date / commit). Enrichment is **best-effort** —
//! a file that is untracked at HEAD simply has no blame and is left un-enriched
//! (the cache is correct with zero git history, Idea §6); only a missing/empty
//! repository surfaces as an error the caller can degrade on.
//!
//! The computed [`BlameIndex`] is returned so the rot detector ([`crate::rot`])
//! reuses it instead of blaming every file twice.

pub mod blame;
pub mod diff;
pub mod repo;

use std::collections::{BTreeSet, HashMap};

use cf_core::comment::{Comment, GitInfo};
use cf_core::error::CfResult;

use blame::{blame_file, FileBlame};
pub use repo::Repo;

/// Per-file blame keyed by repo-relative path.
pub type BlameIndex = HashMap<String, FileBlame>;

/// Blames every distinct file in `comments` and joins git info onto each.
///
/// Returns the [`BlameIndex`] for downstream reuse (rot detection).
///
/// # Errors
/// Returns [`CfError::Git`] only when HEAD cannot be resolved (no repository or
/// an empty one) — the caller treats that as "git unavailable" and proceeds.
pub fn enrich(repo: &Repo, comments: &mut [Comment]) -> CfResult<BlameIndex> {
    let head = repo.head_commit_id()?;
    let index = blame_all(repo, comments, head);
    apply_git_info(comments, &index);
    Ok(index)
}

/// Blames each distinct path, skipping files that cannot be blamed (untracked at
/// HEAD) — expected control flow, not error-hiding (Idea §6 graceful behavior).
fn blame_all(repo: &Repo, comments: &[Comment], head: gix::hash::ObjectId) -> BlameIndex {
    let distinct: BTreeSet<&str> = comments.iter().map(|c| c.path.as_str()).collect();
    let mut index = BlameIndex::new();
    for path in distinct {
        if let Ok(file_blame) = blame_file(repo, path, head) {
            index.insert(path.to_owned(), file_blame);
        }
    }
    index
}

/// Sets each comment's `git` field from the most recent commit over its lines.
fn apply_git_info(comments: &mut [Comment], index: &BlameIndex) {
    for comment in comments.iter_mut() {
        let Some(file_blame) = index.get(&comment.path) else {
            continue;
        };
        if let Some(line) =
            file_blame.most_recent_in_range(comment.range.start_line, comment.range.end_line)
        {
            comment.git = Some(GitInfo {
                author: line.author.clone(),
                email: line.email.clone(),
                commit_id: line.commit_id.clone(),
                committed_unix: line.committed_unix,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::coalesce::coalesce;
    use crate::extract::extract_source;
    use crate::map::map_comments;
    use crate::testutil::TestRepo;
    use cf_core::lang::Language;
    use std::path::Path;

    fn extracted_and_mapped(source: &str, path: &Path) -> Vec<Comment> {
        let repo_path = path.file_name().unwrap().to_str().unwrap();
        let comments = extract_source(source, Language::Python, path, repo_path).unwrap();
        let mut comments = coalesce(source, comments);
        map_comments(source, Language::Python, path, &mut comments).unwrap();
        comments
    }

    #[test]
    fn test_enrich_joins_author_and_date() {
        let fixture = TestRepo::new();
        let body = "# explains compute\ndef compute():\n    return 1\n";
        fixture.write("mod.py", body);
        fixture.commit_all("2020-01-01 00:00:00 +0000");

        let repo = Repo::discover(fixture.path()).unwrap();
        let mut comments = extracted_and_mapped(body, &fixture.path().join("mod.py"));
        enrich(&repo, &mut comments).unwrap();

        let git = comments[0].git.as_ref().expect("comment is enriched");
        assert_eq!(git.author, "Tester");
        assert_eq!(git.committed_unix, 1_577_836_800, "2020-01-01T00:00:00Z");
        assert_eq!(git.commit_id.len(), 40, "full hex commit hash");
    }
}
