//! The comment record (Idea §4).
//!
//! [`Comment`] is the central substrate entity: one extracted comment plus
//! everything the native pass and providers attach to it. Its fields populate
//! in stages across the build, each stage owning a slice:
//!
//! | Stage | Phase | Fields |
//! |---|---|---|
//! | extraction | 2.2 | `path`, `content_hash`, `language`, `kind`, `range`, `raw_text` |
//! | mapping | 2.4 | `bound_symbol`, `bound_node_range` |
//! | markers | 2.5 | `markers` |
//! | git enrichment | 2.6 | `git` |
//! | rot candidates | 2.7 | `is_rot_candidate` |
//! | identity | 5 | `cosmetic_fingerprint` |
//! | normalize/ops | 3+ | `findings` |
//!
//! Like [`crate::Finding`], the *type* lives in the domain crate while the
//! *production* lives in `commenter-cat-engine`; `commenter-cat-core` stays infrastructure-free.

use serde::{Deserialize, Serialize};

use crate::finding::{Finding, Range};
use crate::kind::CommentKind;
use crate::lang::Language;
use crate::symbol::BoundSymbol;

/// Git blame facts joined onto a comment (Idea §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInfo {
    /// Commit author name.
    pub author: String,
    /// Commit author email, when available.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub email: Option<String>,
    /// The commit hash (hex) that last touched the comment's lines.
    pub commit_id: String,
    /// Commit time as Unix seconds (UTC) — the comparable instant used for the
    /// blame-skew rot test (Idea §3, task 2.7).
    pub committed_unix: i64,
}

/// One extracted, mapped, and enriched comment — the Idea §4 "Comment record".
///
/// Constructed by extraction via [`Comment::new`]; later stages set the optional
/// fields directly. All fields are public so each stage populates its own slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    /// Repo-relative path, normalized to `/` separators for cross-platform
    /// byte-identical output (Idea §11 reproducibility).
    pub path: String,
    /// SHA-256 (hex) of `raw_text` — the exact-bytes cache key and Tier-1
    /// identity (Idea §4).
    pub content_hash: String,
    /// The source language.
    pub language: Language,
    /// The classified comment kind.
    pub kind: CommentKind,
    /// The comment's own location (byte + line span).
    pub range: Range,
    /// The exact comment text, delimiters included.
    pub raw_text: String,
    /// The symbol this comment binds to (set by mapping, 2.4).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bound_symbol: Option<BoundSymbol>,
    /// The code-node span this comment binds down to (set by mapping, 2.4).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bound_node_range: Option<Range>,
    /// Marker tags found in the text, e.g. `TODO`, `FIXME` (set by 2.5).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub markers: Vec<String>,
    /// Git blame facts (set by 2.6).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub git: Option<GitInfo>,
    /// Whether blame-skew flags this as a rot candidate (set by 2.7).
    #[serde(default)]
    pub is_rot_candidate: bool,
    /// Cosmetic identity fingerprint (set by Phase 5).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cosmetic_fingerprint: Option<String>,
    /// Normalized findings attached to this comment (set by Phase 3+/ops).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub findings: Vec<Finding>,
}

impl Comment {
    /// Builds a freshly-extracted comment; later stages set the optional fields.
    pub fn new(
        path: impl Into<String>,
        content_hash: impl Into<String>,
        language: Language,
        kind: CommentKind,
        range: Range,
        raw_text: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            content_hash: content_hash.into(),
            language,
            kind,
            range,
            raw_text: raw_text.into(),
            bound_symbol: None,
            bound_node_range: None,
            markers: Vec::new(),
            git: None,
            is_rot_candidate: false,
            cosmetic_fingerprint: None,
            findings: Vec::new(),
        }
    }

    /// Whether this comment is shown in default views — `false` for the
    /// default-suppressed kinds (`shebang`, `license`, Idea §3). A convenience
    /// over [`CommentKind::is_default_suppressed`].
    #[must_use]
    pub fn is_visible_by_default(&self) -> bool {
        !self.kind.is_default_suppressed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_defaults_unset_stage_fields() {
        let c = Comment::new(
            "src/a.py",
            "deadbeef",
            Language::Python,
            CommentKind::Line,
            Range::new(0, 5, 1, 1),
            "# hi",
        );
        assert!(c.bound_symbol.is_none());
        assert!(c.markers.is_empty());
        assert!(c.git.is_none());
        assert!(!c.is_rot_candidate);
        assert!(c.findings.is_empty());
        assert!(c.is_visible_by_default());
    }

    #[test]
    fn test_record_json_round_trips() {
        let mut c = Comment::new(
            "src/a.ts",
            "abc",
            Language::TypeScript,
            CommentKind::Docstring,
            Range::new(0, 20, 1, 2),
            "/** doc */",
        );
        c.bound_symbol = Some(BoundSymbol::new("a.f"));
        c.markers = vec!["TODO".to_owned()];
        let json = serde_json::to_string(&c).unwrap();
        let back: Comment = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn test_default_suppressed_kinds_not_visible() {
        let shebang = Comment::new(
            "run.sh",
            "h",
            Language::Shell,
            CommentKind::Shebang,
            Range::new(0, 11, 1, 1),
            "#!/bin/bash",
        );
        assert!(!shebang.is_visible_by_default());
    }
}
