//! Write-protection by kind (Idea §4a, §5; task 7.3).
//!
//! Parse-invariance proves the *parser* sees no change — it does **not** prove
//! *behavior* is unchanged. `directive` (`cf:*`, `# noqa`, `// eslint-disable`,
//! `# type: ignore`, `// @ts-expect-error`), `shebang`, and `encoding-decl`
//! comments are parse-invariant yet behavior-bearing (read by the type-checker,
//! the orchestrated linters, or the OS, not the grammar). The applier **refuses**
//! to rewrite them without an explicit `allow_significant` acknowledgment, and
//! flags the edit as significant when permitted. The kind taxonomy (§3) enforces
//! this.

use cf_core::comment::Comment;
use cf_core::error::{CfError, CfResult};

/// Checks write-protection for an edit to `comment`. Returns whether the edit is
/// **significant** (behavior-bearing).
///
/// # Errors
/// Returns [`CfError::Apply`] if the comment is behavior-bearing and
/// `allow_significant` was not given.
pub fn check(comment: &Comment, allow_significant: bool) -> CfResult<bool> {
    let significant = comment.kind.is_behavior_bearing();
    if significant && !allow_significant {
        return Err(CfError::apply(format!(
            "refusing to rewrite a behavior-bearing '{}' comment without allow_significant — \
             it is read by the type-checker / linter / OS, not the grammar",
            comment.kind
        )));
    }
    Ok(significant)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_core::finding::Range;
    use cf_core::kind::CommentKind;
    use cf_core::lang::Language;

    fn comment(kind: CommentKind, text: &str) -> Comment {
        Comment::new(
            "a.py",
            "h",
            Language::Python,
            kind,
            Range::new(0, 1, 1, 1),
            text,
        )
    }

    #[test]
    fn test_ordinary_comment_is_not_significant() {
        assert!(!check(&comment(CommentKind::Line, "# prose"), false).unwrap());
        assert!(!check(&comment(CommentKind::Docstring, "\"\"\"d\"\"\""), false).unwrap());
    }

    #[test]
    fn test_directive_refused_without_ack_permitted_with() {
        let directive = comment(CommentKind::Directive, "# type: ignore");
        assert!(
            check(&directive, false).is_err(),
            "refused without allow_significant"
        );
        assert!(
            check(&directive, true).unwrap(),
            "permitted + flagged significant"
        );
    }

    #[test]
    fn test_shebang_and_encoding_decl_protected() {
        assert!(check(&comment(CommentKind::Shebang, "#!/bin/sh"), false).is_err());
        assert!(check(
            &comment(CommentKind::EncodingDecl, "# -*- coding: utf-8 -*-"),
            false
        )
        .is_err());
    }
}
