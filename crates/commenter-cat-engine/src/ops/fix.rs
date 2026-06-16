//! `commenter-cat fix` / `commenter-cat tighten` (Idea §5; task 7.6).
//!
//! Two fix sources, by design (Idea §5):
//!
//! * **Provider autofixes** are *delegated* to each tool's own `--fix`
//!   (`ruff --fix`, `eslint --fix`) where `capabilities.supports_fix` — Commenter-Cat never
//!   hand-applies a tool's edit, so each tool owns its edit safety.
//! * **Agent-authored comment edits** route through the parse-invariant applier
//!   ([`crate::ops::apply`]) — the native safe-write path.
//!
//! A finding the provider can't fix is reported as `agent_only`.

use commenter_cat_core::comment::Comment;
use commenter_cat_core::error::CommenterCatResult;
use commenter_cat_core::finding::{Finding, Fix};

use crate::ops::apply::{self, ApplyResult};

/// How a finding will be fixed (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixRoute {
    /// Delegate to the provider's own `--fix` (Commenter-Cat never hand-applies).
    ProviderAutofix,
    /// Only an agent can resolve it — reported, not auto-fixed.
    AgentOnly,
    /// No fix is offered.
    None,
}

/// Routes a finding's fix, honoring whether its provider can actually delegate a
/// `--fix` (`capabilities.supports_fix`, Phase 6).
#[must_use]
pub fn fix_route(finding: &Finding, provider_supports_fix: bool) -> FixRoute {
    match finding.fix {
        Fix::ProviderAutofix if provider_supports_fix => FixRoute::ProviderAutofix,
        // The provider advertised an autofix but the adapter can't delegate it
        // (capabilities say no) → the agent must resolve it.
        Fix::ProviderAutofix => FixRoute::AgentOnly,
        Fix::AgentOnly => FixRoute::AgentOnly,
        Fix::None => FixRoute::None,
    }
}

/// Applies an agent-authored comment edit through the parse-invariant applier
/// (Idea §5). Deterministic + idempotent; refuses unsafe edits.
///
/// # Errors
/// Returns [`commenter_cat_core::CommenterCatError`] if write-protection refuses or parse-invariance
/// is violated.
pub fn apply_agent_edit(
    source: &str,
    comment: &Comment,
    new_text: &str,
    allow_significant: bool,
) -> CommenterCatResult<ApplyResult> {
    apply::apply_edit(source, comment, new_text, allow_significant)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_source;
    use commenter_cat_core::finding::{Category, FindingTarget, Origin, Range};
    use commenter_cat_core::lang::Language;
    use commenter_cat_core::severity::Severity;
    use commenter_cat_core::symbol::CommentId;
    use std::path::Path;

    fn finding(fix: Fix) -> Finding {
        Finding {
            file: "a.py".to_owned(),
            target: FindingTarget::Comment(CommentId::new("c")),
            range: Range::new(0, 1, 1, 1),
            origin: Origin::Ruff,
            provider_rule_id: "ruff:ERA001".to_owned(),
            canonical_rule_id: "ERA001".to_owned(),
            category: Category::CommentedCode,
            severity: Severity::Warning,
            severity_native: None,
            message: "m".to_owned(),
            fix,
            url: None,
            also_from: Default::default(),
        }
    }

    #[test]
    fn test_provider_autofix_delegated_when_supported() {
        assert_eq!(
            fix_route(&finding(Fix::ProviderAutofix), true),
            FixRoute::ProviderAutofix
        );
        // …but downgraded to agent-only when the provider can't delegate.
        assert_eq!(
            fix_route(&finding(Fix::ProviderAutofix), false),
            FixRoute::AgentOnly
        );
    }

    #[test]
    fn test_non_fixable_is_agent_only_or_none() {
        assert_eq!(
            fix_route(&finding(Fix::AgentOnly), true),
            FixRoute::AgentOnly
        );
        assert_eq!(fix_route(&finding(Fix::None), true), FixRoute::None);
    }

    #[test]
    fn test_agent_edit_goes_through_parse_invariance() {
        let source = "# stale comment\nx = 1\n";
        let comment = extract_source(source, Language::Python, Path::new("a.py"), "a.py")
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        // A comment edit succeeds…
        let ok = apply_agent_edit(source, &comment, "# updated comment", false).unwrap();
        assert_eq!(ok.new_source, "# updated comment\nx = 1\n");
        // …and a code-injecting "fix" is refused by parse-invariance.
        assert!(apply_agent_edit(source, &comment, "x = 99", false).is_err());
    }
}
