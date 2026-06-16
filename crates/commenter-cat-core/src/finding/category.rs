//! The coarse cross-language finding category (Idea §5).
//!
//! `category` is the consistency anchor: it is what makes `doc_drift` resolve to
//! the same canonical severity whether `ruff` or `eslint` reported it. Filters,
//! `explain`, and suppression accept a category as well as a rule id or origin,
//! so an agent can stay coarse or drill to the exact rule (Idea §5).

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::severity::Severity;

/// The coarse cross-language bucket a finding falls into (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Missing docstring / JSDoc (`ruff D1xx`, `eslint require-jsdoc`).
    DocMissing,
    /// Documented params disagree with the signature (`ruff D417`,
    /// `jsdoc/check-param-names`).
    DocDrift,
    /// Docstring/JSDoc style nits (`ruff D2xx–D4xx`).
    DocStyle,
    /// Commented-out code rather than prose (`ruff ERA`, eslint).
    CommentedCode,
    /// A secret / credential in a comment or config (`gitleaks`).
    Secret,
    /// A malformed marker, e.g. `# TODO` without owner (`ruff TD002/003`).
    TodoFormat,
    /// A marker whose tracked work looks stale (native marker × blame-age).
    MarkerStale,
    /// General comment hygiene / width / spacing.
    CommentStyle,
    /// A `#!` interpreter line (shebang) finding.
    Shebang,
    /// A malformed control directive (`commenter-cat:*`, `# noqa`, …).
    Directive,
    /// A native blame-skew rot candidate (Idea §3, §9).
    RotCandidate,
}

impl Category {
    /// Every category, in canonical order.
    pub const ALL: [Category; 11] = [
        Category::DocMissing,
        Category::DocDrift,
        Category::DocStyle,
        Category::CommentedCode,
        Category::Secret,
        Category::TodoFormat,
        Category::MarkerStale,
        Category::CommentStyle,
        Category::Shebang,
        Category::Directive,
        Category::RotCandidate,
    ];

    /// The snake_case config/CLI token (matches the serde form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Category::DocMissing => "doc_missing",
            Category::DocDrift => "doc_drift",
            Category::DocStyle => "doc_style",
            Category::CommentedCode => "commented_code",
            Category::Secret => "secret",
            Category::TodoFormat => "todo_format",
            Category::MarkerStale => "marker_stale",
            Category::CommentStyle => "comment_style",
            Category::Shebang => "shebang",
            Category::Directive => "directive",
            Category::RotCandidate => "rot_candidate",
        }
    }

    /// Parses a category token, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.as_str() == token)
    }

    /// The canonical-severity **anchor** for this category (Idea §5 table) — the
    /// second and decisive tier of severity resolution, guaranteeing cross-
    /// language consistency. A config override (tier 1) can still supersede it.
    #[must_use]
    pub const fn canonical_severity(self) -> Severity {
        match self {
            Category::Secret => Severity::Critical,
            Category::DocDrift | Category::Directive => Severity::Error,
            Category::CommentedCode
            | Category::DocMissing
            | Category::MarkerStale
            | Category::Shebang => Severity::Warning,
            Category::DocStyle
            | Category::TodoFormat
            | Category::CommentStyle
            | Category::RotCandidate => Severity::Info,
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_round_trips_for_all_variants() {
        for category in Category::ALL {
            assert_eq!(Category::from_token(category.as_str()), Some(category));
            assert_eq!(category.to_string(), category.as_str());
        }
    }

    #[test]
    fn test_canonical_severity_table_matches_idea_5() {
        assert_eq!(Category::Secret.canonical_severity(), Severity::Critical);
        assert_eq!(Category::DocDrift.canonical_severity(), Severity::Error);
        assert_eq!(Category::Directive.canonical_severity(), Severity::Error);
        assert_eq!(
            Category::CommentedCode.canonical_severity(),
            Severity::Warning
        );
        assert_eq!(Category::DocMissing.canonical_severity(), Severity::Warning);
        assert_eq!(
            Category::MarkerStale.canonical_severity(),
            Severity::Warning
        );
        assert_eq!(Category::Shebang.canonical_severity(), Severity::Warning);
        assert_eq!(Category::DocStyle.canonical_severity(), Severity::Info);
        assert_eq!(Category::TodoFormat.canonical_severity(), Severity::Info);
        assert_eq!(Category::CommentStyle.canonical_severity(), Severity::Info);
        assert_eq!(Category::RotCandidate.canonical_severity(), Severity::Info);
    }

    #[test]
    fn test_every_category_has_an_anchor() {
        // Exhaustiveness guard: the match above must cover all 11.
        for category in Category::ALL {
            let _ = category.canonical_severity();
        }
    }
}
