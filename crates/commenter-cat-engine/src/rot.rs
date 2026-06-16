//! Blame-skew rot candidates (Idea §3, §9; task 2.7).
//!
//! Flags a comment whose blame **predates** its bound code's blame — the code
//! changed after the comment was written, so the comment *may* be stale. This is
//! a native capability no provider offers (it needs Commenter-Cat's mapping + git join,
//! Idea §3). It is a **token-free shortlist the agent judges**, never a verdict
//! (Idea §9): the engine proves the skew; the agent decides if it is rot.
//!
//! A comment with no `bound_node_range` (orphan / unbound) is never a candidate
//! — there is no code to have drifted from.

use commenter_cat_core::comment::Comment;

use crate::git::BlameIndex;

/// Sets `is_rot_candidate` on every comment from the blame index (Idea §3).
pub fn flag_rot_candidates(comments: &mut [Comment], blames: &BlameIndex) {
    for comment in comments.iter_mut() {
        comment.is_rot_candidate = is_rot_candidate(comment, blames);
    }
}

/// Whether the comment's bound code was committed strictly after the comment.
fn is_rot_candidate(comment: &Comment, blames: &BlameIndex) -> bool {
    let Some(code) = comment.bound_node_range else {
        return false;
    };
    let Some(file_blame) = blames.get(&comment.path) else {
        return false;
    };
    let comment_date =
        file_blame.most_recent_in_range(comment.range.start_line, comment.range.end_line);
    let code_date = file_blame.most_recent_in_range(code.start_line, code.end_line);
    match (comment_date, code_date) {
        (Some(comment), Some(code)) => comment.committed_unix < code.committed_unix,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::coalesce::coalesce;
    use crate::extract::extract_source;
    use crate::git::{enrich, Repo};
    use crate::map::map_comments;
    use crate::testutil::TestRepo;
    use commenter_cat_core::lang::Language;

    const V1: &str = "# explains compute\ndef compute():\n    return 1\n\n\n# stable helper\ndef helper():\n    return 0\n";
    // Only `compute`'s body changes in v2; the comments and `helper` are untouched.
    const V2: &str = "# explains compute\ndef compute():\n    return 2\n\n\n# stable helper\ndef helper():\n    return 0\n";

    #[test]
    fn test_blame_skew_flags_only_the_drifted_comment() {
        let fixture = TestRepo::new();
        fixture.write("mod.py", V1);
        fixture.commit_all("2020-01-01 00:00:00 +0000");
        fixture.write("mod.py", V2);
        fixture.commit_all("2022-01-01 00:00:00 +0000");

        let repo = Repo::discover(fixture.path()).unwrap();
        let path = fixture.path().join("mod.py");
        let comments = extract_source(V2, Language::Python, &path, "mod.py").unwrap();
        let mut comments = coalesce(V2, comments);
        map_comments(V2, Language::Python, &path, &mut comments).unwrap();

        let index = enrich(&repo, &mut comments).unwrap();
        flag_rot_candidates(&mut comments, &index);

        let compute = comments
            .iter()
            .find(|c| c.raw_text.contains("compute"))
            .unwrap();
        let helper = comments
            .iter()
            .find(|c| c.raw_text.contains("helper"))
            .unwrap();
        assert!(
            compute.is_rot_candidate,
            "code changed (2022) after the comment (2020)"
        );
        assert!(
            !helper.is_rot_candidate,
            "comment and its code share a commit date"
        );
    }

    #[test]
    fn test_unbound_comment_is_never_a_candidate() {
        // An orphan comment (blank line below) has no bound code to drift from.
        let fixture = TestRepo::new();
        let src = "# orphan\n\ndef f():\n    pass\n";
        fixture.write("o.py", src);
        fixture.commit_all("2020-01-01 00:00:00 +0000");

        let repo = Repo::discover(fixture.path()).unwrap();
        let path = fixture.path().join("o.py");
        let mut comments = coalesce(
            src,
            extract_source(src, Language::Python, &path, "o.py").unwrap(),
        );
        map_comments(src, Language::Python, &path, &mut comments).unwrap();
        let index = enrich(&repo, &mut comments).unwrap();
        flag_rot_candidates(&mut comments, &index);

        assert!(comments.iter().all(|c| !c.is_rot_candidate));
    }
}
