//! The committed source-language set (Idea §3).
//!
//! Commenter-Cat commits to Python, TypeScript, JavaScript, and Shell. More
//! tree-sitter grammars are low-cost future additions but out of defined scope
//! (Idea §3); this enum is the closed set the substrate and providers target.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A source language CF extracts and maps comments for (Idea §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Python — docstrings bound by PEP 257; provider is `ruff` (Idea §5).
    Python,
    /// TypeScript — JSDoc/TSDoc adjacency; provider is `eslint` (Idea §5).
    TypeScript,
    /// JavaScript — JSDoc adjacency; provider is `eslint` (Idea §5).
    JavaScript,
    /// Shell — provider is `shellcheck` (Idea §5).
    Shell,
}

impl Language {
    /// Every committed language, in canonical order. Doubles as the default
    /// `[scan] languages` set (Idea §12).
    pub const ALL: [Language; 4] = [
        Language::Python,
        Language::TypeScript,
        Language::JavaScript,
        Language::Shell,
    ];

    /// The lowercase config/CLI token for this language (matches the serde form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Shell => "shell",
        }
    }

    /// Parses a config/CLI token into a language, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Language> {
        Language::ALL
            .into_iter()
            .find(|lang| lang.as_str() == token)
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_round_trips_for_all_variants() {
        for lang in Language::ALL {
            assert_eq!(Language::from_token(lang.as_str()), Some(lang));
            assert_eq!(lang.to_string(), lang.as_str());
        }
    }

    #[test]
    fn test_unknown_token_is_none() {
        assert_eq!(Language::from_token("rust"), None);
        assert_eq!(
            Language::from_token("Python"),
            None,
            "tokens are case-sensitive"
        );
    }

    #[test]
    fn test_serde_uses_lowercase_token() {
        let value = toml::Value::try_from(Language::TypeScript).unwrap();
        assert_eq!(
            value.as_str(),
            Some("typescript"),
            "serde form must match as_str"
        );
    }
}
