//! First-class comment-kind classification (Idea §3).
//!
//! `kind` is not cosmetic — it drives three behaviors:
//!
//! * **Default suppression** — [`CommentKind::Shebang`] and
//!   [`CommentKind::License`] are noise by default (Idea §3).
//! * **Finding eligibility** — [`CommentKind::Directive`] comments (`cf:*`) are
//!   control-only and are *never* themselves finding targets (Idea §3, §5).
//! * **Write-protection by kind** — `directive`, `shebang`, and `encoding-decl`
//!   comments are *parse-invariant yet behavior-bearing* (read by the
//!   type-checker, the orchestrated linters, or the OS, not the grammar), so the
//!   safe-apply path refuses to rewrite them without `allow_significant`
//!   (Idea §4a, §5).
//!
//! The classification logic (deciding a node's kind during extraction) lands in
//! Phase 2; this module owns the taxonomy and the rules that hang off it.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The classification of an extracted comment (Idea §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommentKind {
    /// A single-line comment (`#`, `//`).
    Line,
    /// A block / multi-line comment (`/* */`, `""" """` used as a block).
    Block,
    /// A documentation string bound to a symbol (PEP 257, JSDoc/TSDoc).
    Docstring,
    /// A `#!` interpreter line; behavior-bearing and default-suppressed.
    Shebang,
    /// A license / copyright header; default-suppressed.
    License,
    /// A source-encoding declaration (e.g. `# -*- coding: utf-8 -*-`);
    /// behavior-bearing.
    EncodingDecl,
    /// A control directive (`cf:*`, `# noqa`, `// eslint-disable`,
    /// `# type: ignore`, `// @ts-expect-error`); behavior-bearing and never a
    /// finding target.
    Directive,
}

impl CommentKind {
    /// Every kind, in canonical order.
    pub const ALL: [CommentKind; 7] = [
        CommentKind::Line,
        CommentKind::Block,
        CommentKind::Docstring,
        CommentKind::Shebang,
        CommentKind::License,
        CommentKind::EncodingDecl,
        CommentKind::Directive,
    ];

    /// The config/CLI token for this kind (matches the kebab-case serde form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CommentKind::Line => "line",
            CommentKind::Block => "block",
            CommentKind::Docstring => "docstring",
            CommentKind::Shebang => "shebang",
            CommentKind::License => "license",
            CommentKind::EncodingDecl => "encoding-decl",
            CommentKind::Directive => "directive",
        }
    }

    /// Parses a config/CLI token into a kind, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<CommentKind> {
        CommentKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == token)
    }

    /// Whether this kind is suppressed by default (Idea §3): `shebang`,
    /// `license`. Reflected in the default `[scan] suppress_kinds` (Idea §12).
    #[must_use]
    pub const fn is_default_suppressed(self) -> bool {
        matches!(self, CommentKind::Shebang | CommentKind::License)
    }

    /// Whether this kind is *behavior-bearing* (Idea §4a, §5): parse-invariant
    /// yet semantically significant, so the safe-apply path refuses to rewrite
    /// it without an explicit `allow_significant` acknowledgment.
    #[must_use]
    pub const fn is_behavior_bearing(self) -> bool {
        matches!(
            self,
            CommentKind::Directive | CommentKind::Shebang | CommentKind::EncodingDecl
        )
    }

    /// Whether a comment of this kind can itself be a finding target. Directive
    /// comments are control-only and are excluded (Idea §3, §5).
    #[must_use]
    pub const fn can_be_finding_target(self) -> bool {
        !matches!(self, CommentKind::Directive)
    }
}

impl fmt::Display for CommentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_round_trips_for_all_variants() {
        for kind in CommentKind::ALL {
            assert_eq!(CommentKind::from_token(kind.as_str()), Some(kind));
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }

    #[test]
    fn test_encoding_decl_uses_kebab_case() {
        assert_eq!(CommentKind::EncodingDecl.as_str(), "encoding-decl");
        let value = toml::Value::try_from(CommentKind::EncodingDecl).unwrap();
        assert_eq!(value.as_str(), Some("encoding-decl"));
    }

    #[test]
    fn test_default_suppressed_set() {
        assert!(CommentKind::Shebang.is_default_suppressed());
        assert!(CommentKind::License.is_default_suppressed());
        assert!(!CommentKind::Line.is_default_suppressed());
        assert!(!CommentKind::Docstring.is_default_suppressed());
    }

    #[test]
    fn test_behavior_bearing_set() {
        // The three write-protected kinds (Idea §4a, §5).
        assert!(CommentKind::Directive.is_behavior_bearing());
        assert!(CommentKind::Shebang.is_behavior_bearing());
        assert!(CommentKind::EncodingDecl.is_behavior_bearing());
        // Ordinary prose is freely rewritable.
        assert!(!CommentKind::Line.is_behavior_bearing());
        assert!(!CommentKind::Block.is_behavior_bearing());
        assert!(!CommentKind::Docstring.is_behavior_bearing());
    }

    #[test]
    fn test_directive_is_never_a_finding_target() {
        assert!(!CommentKind::Directive.can_be_finding_target());
        for kind in CommentKind::ALL {
            if kind != CommentKind::Directive {
                assert!(kind.can_be_finding_target(), "{kind} should be targetable");
            }
        }
    }
}
