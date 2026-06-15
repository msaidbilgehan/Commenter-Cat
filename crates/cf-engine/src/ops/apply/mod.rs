//! The parse-invariant safe-apply path (Idea §4a, §5; tasks 7.2, 7.3).
//!
//! **AI proposes, the engine guarantees.** The agent authors the new comment
//! text (via `context` + `candidates`); the engine applies it deterministically,
//! proving (1) the code-node tree is byte-identical ([`parse_invariant`]) and
//! (2) no behavior-bearing comment changed silently ([`write_protection`]).

pub mod parse_invariant;
pub mod write_protection;

use cf_core::comment::Comment;
use cf_core::error::CfResult;

/// The result of a safe-apply: the new source plus whether the edit touched a
/// behavior-bearing (significant) comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyResult {
    /// The rewritten source.
    pub new_source: String,
    /// Whether this was a significant (behavior-bearing) edit (Idea §4a).
    pub significant: bool,
}

/// Applies an agent-authored comment-only edit under parse-invariance +
/// write-protection (Idea §4a).
///
/// # Errors
/// Returns [`cf_core::CfError`] if write-protection refuses the edit or
/// parse-invariance is violated.
pub fn apply_edit(
    source: &str,
    comment: &Comment,
    new_text: &str,
    allow_significant: bool,
) -> CfResult<ApplyResult> {
    let significant = write_protection::check(comment, allow_significant)?;
    let new_source = parse_invariant::apply_comment_edit(source, comment, new_text)?;
    Ok(ApplyResult {
        new_source,
        significant,
    })
}

/// Removes a comment (replacing it with empty text) under the same guarantees
/// (Idea §4a `remove`).
///
/// # Errors
/// Returns [`cf_core::CfError`] if write-protection refuses or parse-invariance
/// is violated.
pub fn remove(source: &str, comment: &Comment, allow_significant: bool) -> CfResult<ApplyResult> {
    apply_edit(source, comment, "", allow_significant)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_source;
    use cf_core::lang::Language;
    use std::path::Path;

    fn comment_matching(source: &str, text: &str) -> Comment {
        extract_source(source, Language::Python, Path::new("t.py"), "t.py")
            .unwrap()
            .into_iter()
            .find(|c| c.raw_text.contains(text))
            .unwrap()
    }

    #[test]
    fn test_apply_edit_on_prose_succeeds_not_significant() {
        let source = "# explain\nx = 1\n";
        let comment = comment_matching(source, "explain");
        let result = apply_edit(source, &comment, "# clarify", false).unwrap();
        assert_eq!(result.new_source, "# clarify\nx = 1\n");
        assert!(!result.significant);
    }

    #[test]
    fn test_apply_edit_refuses_directive_without_ack() {
        let source = "x = 1  # type: ignore\n";
        let comment = comment_matching(source, "type: ignore");
        assert!(apply_edit(source, &comment, "# type: ignore[assignment]", false).is_err());
        // With the acknowledgment it applies and is flagged significant.
        let result = apply_edit(source, &comment, "# type: ignore[assignment]", true).unwrap();
        assert!(result.significant);
    }

    #[test]
    fn test_remove_comment() {
        let source = "# remove me\nx = 1\n";
        let comment = comment_matching(source, "remove me");
        let result = remove(source, &comment, false).unwrap();
        assert_eq!(result.new_source, "\nx = 1\n");
    }
}
