//! Native marker extraction (Idea §3; task 2.5).
//!
//! Commenter-Cat extracts markers itself because they feed triage, search, and blame-skew —
//! the cross-language worklist. This module only *finds and tags* markers; it
//! does **not** validate their format (`# TODO(owner):` is `ruff TD002/003`'s
//! job, Idea §5). Custom markers come from `[markers].custom` (Idea §12).
//!
//! Matching is case-sensitive (markers are conventionally upper-case, matching
//! `ruff`'s TD rules) and word-bounded, so `AUTOTODO` and a lower-case `todo` in
//! prose are not false positives.

use std::collections::BTreeSet;

use commenter_cat_core::comment::Comment;

/// The native marker set (Idea §3).
pub const BUILTIN_MARKERS: [&str; 9] = [
    "TODO",
    "FIXME",
    "HACK",
    "XXX",
    "BUG",
    "NOTE",
    "DEPRECATED",
    "WARNING",
    "REVIEW",
];

/// The set of marker tokens to scan for: the built-ins plus configured customs.
#[derive(Debug, Clone)]
pub struct MarkerSet {
    tokens: Vec<String>,
}

impl MarkerSet {
    /// Builds a marker set from the built-ins and `[markers].custom` (Idea §12).
    #[must_use]
    pub fn new(custom: &[String]) -> Self {
        let mut tokens: BTreeSet<String> =
            BUILTIN_MARKERS.iter().map(|m| (*m).to_owned()).collect();
        tokens.extend(custom.iter().filter(|c| !c.is_empty()).cloned());
        Self {
            tokens: tokens.into_iter().collect(),
        }
    }

    /// The markers present in `text`, sorted and de-duplicated.
    #[must_use]
    pub fn find(&self, text: &str) -> Vec<String> {
        let mut found: BTreeSet<&str> = BTreeSet::new();
        for token in &self.tokens {
            if contains_word(text, token) {
                found.insert(token.as_str());
            }
        }
        found.into_iter().map(str::to_owned).collect()
    }

    /// Tags a comment in place with the markers found in its text.
    pub fn tag(&self, comment: &mut Comment) {
        comment.markers = self.find(&comment.raw_text);
    }
}

/// Whether `needle` occurs in `haystack` as a whole word (boundaries on both
/// sides are non-word characters).
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    haystack.match_indices(needle).any(|(index, matched)| {
        let before_ok = haystack[..index]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word_char(c));
        let after = index + matched.len();
        let after_ok = haystack[after..]
            .chars()
            .next()
            .is_none_or(|c| !is_word_char(c));
        before_ok && after_ok
    })
}

/// Whether `c` is part of an identifier word (alphanumeric or underscore).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_set() -> MarkerSet {
        MarkerSet::new(&[])
    }

    #[test]
    fn test_finds_builtin_markers() {
        assert_eq!(default_set().find("# TODO: fix this"), vec!["TODO"]);
        assert_eq!(
            default_set().find("// FIXME and TODO here"),
            vec!["FIXME", "TODO"]
        );
    }

    #[test]
    fn test_todo_fixme_distinction() {
        assert_eq!(default_set().find("# TODO"), vec!["TODO"]);
        assert_eq!(default_set().find("# FIXME"), vec!["FIXME"]);
    }

    #[test]
    fn test_word_boundaries_and_case() {
        assert!(
            default_set().find("# AUTOTODO marker").is_empty(),
            "TODO inside a word"
        );
        assert!(
            default_set().find("# TODOS list").is_empty(),
            "TODO as a prefix"
        );
        assert!(
            default_set().find("# a todo in prose").is_empty(),
            "lower-case is not a marker"
        );
    }

    #[test]
    fn test_custom_markers() {
        let set = MarkerSet::new(&["SECURITY".to_owned(), "DO_NOT_MERGE".to_owned()]);
        assert_eq!(set.find("# SECURITY: token leak"), vec!["SECURITY"]);
        assert_eq!(
            set.find("// DO_NOT_MERGE before review"),
            vec!["DO_NOT_MERGE"]
        );
    }

    #[test]
    fn test_tag_sets_comment_markers() {
        use commenter_cat_core::finding::Range;
        use commenter_cat_core::kind::CommentKind;
        use commenter_cat_core::lang::Language;

        let mut comment = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            Range::new(0, 14, 1, 1),
            "# TODO: later",
        );
        default_set().tag(&mut comment);
        assert_eq!(comment.markers, vec!["TODO"]);
    }
}
