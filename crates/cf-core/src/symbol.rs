//! Cross-scan reference types for the comment substrate (Idea §3, §4).
//!
//! [`BoundSymbol`] names the code a comment binds to (`bound_symbol`, Idea §3);
//! [`CommentId`] is a stable opaque handle to one comment record. Both are
//! shared: mapping (Phase 2) produces a `BoundSymbol`, the [`crate::finding`]
//! model targets either, and identity (Phase 5) composes both into cross-scan
//! matches. They are newtypes — not bare `String`s — so the type system keeps a
//! symbol path and a comment handle from being confused.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The symbol a comment binds to — a qualified path like `module.Class.method`
/// (Idea §3). For an orphan comment this is the enclosing scope.
///
/// Its exact construction (the deterministic mapping rules) is owned by the
/// Phase 2 mapping module; this type is the shared vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BoundSymbol(pub String);

impl BoundSymbol {
    /// Wraps a qualified symbol path.
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// The qualified path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BoundSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for BoundSymbol {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for BoundSymbol {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// A stable opaque handle to one comment record (Idea §4).
///
/// The engine derives it from the comment's stable identity (Phase 5); the
/// domain layer treats it as an opaque token so storage can choose its concrete
/// form (content hash, composite identity, or row id) without leaking into the
/// model.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommentId(pub String);

impl CommentId {
    /// Wraps a stable comment-identity token.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The handle as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for CommentId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for CommentId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bound_symbol_transparent_serde() {
        let sym = BoundSymbol::new("pkg.module.func");
        let json = serde_json::to_string(&sym).unwrap();
        assert_eq!(
            json, "\"pkg.module.func\"",
            "must serialize as a bare string"
        );
        let back: BoundSymbol = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sym);
    }

    #[test]
    fn test_comment_id_transparent_serde() {
        let id = CommentId::new("abc123");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"abc123\"");
        assert_eq!(serde_json::from_str::<CommentId>(&json).unwrap(), id);
    }

    #[test]
    fn test_display_and_conversions() {
        assert_eq!(BoundSymbol::from("x").to_string(), "x");
        assert_eq!(CommentId::from("y".to_owned()).as_str(), "y");
    }
}
