//! The apply_edit / remove round-trip (Idea §4a).
//!
//! "The round trip closes the loop." After a comment-only write or a deletion,
//! re-run `check` for **just the touched comment** and return the re-checked
//! findings inline — so an agent's find → update → re-check cycle is one
//! tool-call deep, never a separate `check` round. Reuses the parse-invariant
//! applier (7.2) and the same fuse machinery as `cf check` (7.1), scoped to the
//! one file the edit touched.

use std::path::Path;

use cf_core::comment::Comment;
use cf_core::config::ResolvedConfig;
use cf_core::error::CfResult;
use cf_core::finding::Finding;

use crate::extract::coalesce::coalesce;
use crate::extract::extract_source;
use crate::map::map_comments;
use crate::markers::MarkerSet;
use crate::ops::apply;
use crate::ops::{normalize, triage};
use crate::provider::{ProviderContext, RuleProvider};

/// The result of a round-trip: the new source, whether the edit touched a
/// behavior-bearing comment, and the re-checked findings now on that comment.
#[derive(Debug)]
pub struct RoundTrip {
    /// The edited source (parse-invariance already guaranteed by the applier).
    pub new_source: String,
    /// Whether the edited comment is behavior-bearing (write-protection, 7.3).
    pub significant: bool,
    /// The findings on the touched comment *after* the edit — including any the
    /// edit just introduced (a fresh `TODO`, a doc that now drifts).
    pub findings: Vec<Finding>,
}

/// Applies a comment-only edit, then re-checks the touched comment and returns
/// its findings inline (Idea §4a).
///
/// # Errors
/// Returns [`cf_core::CfError`] if write-protection refuses, parse-invariance is
/// violated, or the re-check's grammar pass fails.
pub fn apply_edit_and_recheck(
    source: &str,
    comment: &Comment,
    new_text: &str,
    allow_significant: bool,
    root: &Path,
    config: &ResolvedConfig,
    providers: &[&dyn RuleProvider],
) -> CfResult<RoundTrip> {
    let applied = apply::apply_edit(source, comment, new_text, allow_significant)?;
    recheck(
        applied.new_source,
        applied.significant,
        comment,
        root,
        config,
        providers,
    )
}

/// Removes a comment, then re-checks the touched location and returns its
/// findings inline (typically empty — the comment is gone).
///
/// # Errors
/// Returns [`cf_core::CfError`] under the same conditions as
/// [`apply_edit_and_recheck`].
pub fn remove_and_recheck(
    source: &str,
    comment: &Comment,
    allow_significant: bool,
    root: &Path,
    config: &ResolvedConfig,
    providers: &[&dyn RuleProvider],
) -> CfResult<RoundTrip> {
    let applied = apply::remove(source, comment, allow_significant)?;
    recheck(
        applied.new_source,
        applied.significant,
        comment,
        root,
        config,
        providers,
    )
}

/// Re-checks the edited file and returns the findings on the comment anchored at
/// the edit's start byte (which the edit did not move).
fn recheck(
    new_source: String,
    significant: bool,
    target: &Comment,
    root: &Path,
    config: &ResolvedConfig,
    providers: &[&dyn RuleProvider],
) -> CfResult<RoundTrip> {
    let path = Path::new(&target.path);
    let mut comments = extract_source(&new_source, target.language, path, &target.path)?;
    comments = coalesce(&new_source, comments);
    map_comments(&new_source, target.language, path, &mut comments)?;

    let marker_set = MarkerSet::new(&config.markers.custom);
    for comment in &mut comments {
        marker_set.tag(comment);
        let mut native = triage::marker_findings(comment, &config.markers.severity);
        native.extend(triage::rot_finding(comment));
        comment.findings.extend(native);
    }

    let context = ProviderContext::new(root, &config.severity.overrides);
    let files = [path.to_path_buf()];
    let mut provider_findings = Vec::new();
    for provider in providers {
        provider_findings.extend(provider.run(&files, &context).findings);
    }
    // A single-file recheck has no cross-file context, so symbol-only findings
    // that match no comment here are simply not surfaced.
    let _unattached = normalize::attach_findings(&mut comments, provider_findings);
    normalize::dedup_comment_findings(&mut comments);

    // The edit started at `target.range.start_byte` and only changed the comment
    // span, so the touched comment is the one covering that anchor.
    let anchor = target.range.start_byte;
    let findings = comments
        .into_iter()
        .find(|comment| comment.range.start_byte <= anchor && anchor < comment.range.end_byte)
        .map(|comment| comment.findings)
        .unwrap_or_default();

    Ok(RoundTrip {
        new_source,
        significant,
        findings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_core::finding::Origin;
    use cf_core::lang::Language;

    fn first_comment(source: &str, root: &Path) -> Comment {
        extract_source(source, Language::Python, &root.join("a.py"), "a.py")
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn test_edit_introducing_a_todo_is_rechecked_inline() {
        let root = Path::new("/tmp/cf-roundtrip-test");
        let source = "# clean explanation\nx = 1\n";
        let comment = first_comment(source, root);
        let config = ResolvedConfig::default();
        let providers: [&dyn RuleProvider; 0] = [];

        // The edit introduces a TODO marker that was not present before.
        let trip = apply_edit_and_recheck(
            source,
            &comment,
            "# TODO revisit this",
            false,
            root,
            &config,
            &providers,
        )
        .unwrap();

        assert_eq!(trip.new_source, "# TODO revisit this\nx = 1\n");
        assert!(
            trip.findings
                .iter()
                .any(|f| f.origin == Origin::Native && f.message.contains("TODO")),
            "the round-trip returns the finding the edit just introduced"
        );
    }

    #[test]
    fn test_clean_edit_has_no_findings() {
        let root = Path::new("/tmp/cf-roundtrip-test");
        let source = "# TODO old\nx = 1\n";
        let comment = first_comment(source, root);
        let config = ResolvedConfig::default();
        let providers: [&dyn RuleProvider; 0] = [];

        // Editing the TODO away clears the marker finding.
        let trip = apply_edit_and_recheck(
            source,
            &comment,
            "# now resolved",
            false,
            root,
            &config,
            &providers,
        )
        .unwrap();
        assert!(trip.findings.is_empty(), "no marker remains after the edit");
    }
}
