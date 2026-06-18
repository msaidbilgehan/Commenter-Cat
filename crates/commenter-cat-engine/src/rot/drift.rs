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
//!
//! [`flag_rot_candidates`] sets the boolean `is_rot_candidate` flag (any skew —
//! the persisted signal); [`git_drift_finding`] is detector 4's first-class
//! finding, which additionally honors `git_drift_age_threshold_days` so only a
//! *material* skew (code newer than the comment by more than the configured days)
//! surfaces, cutting churn noise.

use commenter_cat_core::comment::Comment;
use commenter_cat_core::config::RotConfig;
use commenter_cat_core::finding::{Category, Finding};

use crate::git::BlameIndex;
use crate::ops::triage::native_finding;

/// The stable canonical rule id for git-drift findings.
const RULE: &str = "rot_drift";

/// Seconds in a day — the unit the age threshold is expressed in.
const SECONDS_PER_DAY: i64 = 86_400;

/// Sets `is_rot_candidate` on every comment from the blame index (Idea §3).
pub fn flag_rot_candidates(comments: &mut [Comment], blames: &BlameIndex) {
    for comment in comments.iter_mut() {
        comment.is_rot_candidate = is_rot_candidate(comment, blames);
    }
}

/// Detector 4: a `rot_drift` finding when the bound code's blame is newer than
/// the comment's blame by more than `git_drift_age_threshold_days`. Honors the
/// `git_drift` toggle and degrades to `None` when blame is unavailable for either
/// span (no git repo, untracked file, or an unbound comment) — never a false
/// finding (Idea §5, generalized).
#[must_use]
pub fn git_drift_finding(
    comment: &Comment,
    blames: &BlameIndex,
    config: &RotConfig,
) -> Option<Finding> {
    if !config.git_drift {
        return None;
    }
    let (comment_unix, code_unix) = blame_skew(comment, blames)?;
    let skew_seconds = code_unix.checked_sub(comment_unix)?;
    let threshold_seconds = i64::from(config.git_drift_age_threshold_days) * SECONDS_PER_DAY;
    if skew_seconds <= threshold_seconds {
        return None;
    }
    let days = skew_seconds / SECONDS_PER_DAY;
    Some(native_finding(
        comment,
        comment.range,
        Category::RotCandidate,
        RULE.to_owned(),
        Category::RotCandidate.canonical_severity(),
        format!(
            "comment predates its bound code by {days} days — the code changed after the comment was written"
        ),
    ))
}

/// Whether the comment's bound code was committed strictly after the comment.
fn is_rot_candidate(comment: &Comment, blames: &BlameIndex) -> bool {
    match blame_skew(comment, blames) {
        Some((comment_unix, code_unix)) => comment_unix < code_unix,
        None => false,
    }
}

/// The `(comment_committed_unix, code_committed_unix)` pair for a comment and its
/// bound code, or `None` when either has no blame (unbound, or git unavailable).
fn blame_skew(comment: &Comment, blames: &BlameIndex) -> Option<(i64, i64)> {
    let code = comment.bound_node_range?;
    let file_blame = blames.get(&comment.path)?;
    let comment_date =
        file_blame.most_recent_in_range(comment.range.start_line, comment.range.end_line)?;
    let code_date = file_blame.most_recent_in_range(code.start_line, code.end_line)?;
    Some((comment_date.committed_unix, code_date.committed_unix))
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

    #[test]
    fn test_git_drift_finding_honors_the_age_threshold() {
        use commenter_cat_core::config::ResolvedConfig;
        use commenter_cat_core::finding::Category;

        let fixture = TestRepo::new();
        fixture.write("mod.py", V1);
        fixture.commit_all("2020-01-01 00:00:00 +0000");
        fixture.write("mod.py", V2);
        fixture.commit_all("2022-01-01 00:00:00 +0000"); // code ~730 days newer

        let repo = Repo::discover(fixture.path()).unwrap();
        let path = fixture.path().join("mod.py");
        let mut comments = coalesce(
            V2,
            extract_source(V2, Language::Python, &path, "mod.py").unwrap(),
        );
        map_comments(V2, Language::Python, &path, &mut comments).unwrap();
        let blames = enrich(&repo, &mut comments).unwrap();

        let compute = comments
            .iter()
            .find(|c| c.raw_text.contains("compute"))
            .unwrap();
        let helper = comments
            .iter()
            .find(|c| c.raw_text.contains("helper"))
            .unwrap();

        // Default 30-day threshold: the ~730-day skew on `compute` surfaces.
        let config = ResolvedConfig::default().rot;
        let finding = git_drift_finding(compute, &blames, &config).expect("drift over threshold");
        assert_eq!(finding.canonical_rule_id, RULE);
        assert_eq!(finding.category, Category::RotCandidate);
        assert!(finding.message.contains("days"));
        // The unmoved `helper` comment shares its code's commit → no skew.
        assert!(git_drift_finding(helper, &blames, &config).is_none());

        // A threshold larger than the actual skew suppresses the finding.
        let mut patient = config;
        patient.git_drift_age_threshold_days = 100_000;
        assert!(git_drift_finding(compute, &blames, &patient).is_none());

        // The toggle off suppresses it regardless of skew.
        let mut disabled = config;
        disabled.git_drift = false;
        assert!(git_drift_finding(compute, &blames, &disabled).is_none());
    }
}
