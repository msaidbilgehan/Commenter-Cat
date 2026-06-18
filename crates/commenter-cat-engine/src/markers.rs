//! Native marker extraction (Idea §3; task 2.5).
//!
//! Commenter-Cat extracts markers itself because they feed triage, search, and blame-skew —
//! the cross-language worklist. This module only *finds and tags* markers; it
//! does **not** validate their format (`# TODO(owner):` is `ruff TD002/003`'s
//! job, Idea §5). Custom markers come from `[markers].custom` (Idea §12).
//!
//! Matching is **leading-position**: a marker counts only when it *begins* a
//! comment line — after stripping that line's comment delimiters and furniture
//! (`#`, `//`, `* `, `<!--`, `;`, quotes) — followed by a boundary (`:`, an
//! `(owner)`, whitespace, or end of line). It is case-sensitive (markers are
//! conventionally upper-case, matching `ruff`'s TD rules). So `# TODO: fix` and
//! ` * FIXME(me):` tag, while a mid-sentence mention ("log at `WARNING` level",
//! "see the TODO above"), a non-word-bounded `AUTOTODO`/`TODOS`, and a lower-case
//! `todo` in prose do not — the precision rule that keeps marker triage to
//! *actual* markers, not every comment that merely names one (dogfooding: a
//! substring scan flagged comments *about* WARNING-level logging as markers).

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

    /// The markers leading a line of `text`, sorted and de-duplicated. A marker is
    /// recognized only in leading position (after the line's comment furniture),
    /// so a mid-sentence mention is not a false positive (module docs).
    #[must_use]
    pub fn find(&self, text: &str) -> Vec<String> {
        let mut found: BTreeSet<&str> = BTreeSet::new();
        for line in text.lines() {
            let lead = strip_comment_lead(line);
            for token in &self.tokens {
                if leads_with_marker(lead, token) {
                    found.insert(token.as_str());
                }
            }
        }
        found.into_iter().map(str::to_owned).collect()
    }

    /// Tags a comment in place with the markers found in its text.
    pub fn tag(&self, comment: &mut Comment) {
        comment.markers = self.find(&comment.raw_text);
    }
}

/// Whether any line of `text` leads with a built-in marker token — the
/// directive shape the rot intent classifier recognizes (Idea §3). Shares the
/// leading-position rule with [`MarkerSet::find`], so a mid-sentence mention
/// ("log at `WARNING` level") is not a directive.
#[must_use]
pub fn leads_with_builtin_marker(text: &str) -> bool {
    text.lines().any(|line| {
        let lead = strip_comment_lead(line);
        BUILTIN_MARKERS
            .iter()
            .any(|marker| leads_with_marker(lead, marker))
    })
}

/// A line with its leading comment delimiters and furniture stripped — where a
/// marker would lead. Drops leading whitespace and the punctuation that opens or
/// continues a comment across the committed languages (`#`, `//`, `/*`, ` * `,
/// `<!--`, `;`, `%`, docstring quotes), stopping at the first content character.
fn strip_comment_lead(line: &str) -> &str {
    line.trim_start_matches(|c: char| c.is_whitespace() || is_comment_furniture(c))
}

/// Whether `c` is comment-delimiter furniture (never part of a marker token, so
/// safe to strip when locating the leading content of a comment line).
fn is_comment_furniture(c: char) -> bool {
    matches!(
        c,
        '#' | '/' | '*' | '<' | '>' | '!' | '-' | ';' | '%' | '"' | '\''
    )
}

/// Whether `lead` begins with `marker` in marker position: the token, then a
/// boundary (a non-word char — `:`, `(`, whitespace, `-`, … — or end of line).
/// Case-sensitive; a trailing word char (`TODOS`, `TODO_NEXT`) is not a match.
fn leads_with_marker(lead: &str, marker: &str) -> bool {
    if marker.is_empty() {
        return false;
    }
    match lead.strip_prefix(marker) {
        Some(rest) => rest.chars().next().is_none_or(|c| !is_word_char(c)),
        None => false,
    }
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
        // Leading position only: the line leads with FIXME; the mid-sentence TODO
        // is a reference, not a marker (precision over the old substring scan).
        assert_eq!(default_set().find("// FIXME and TODO here"), vec!["FIXME"]);
    }

    #[test]
    fn test_marker_must_lead_the_comment_not_just_appear_in_it() {
        // The dogfooding false positives: comments *about* warnings/notes, not
        // stale markers. None of these lead with a marker token, so none tag.
        let set = default_set();
        assert!(
            set.find("// log at WARNING level when the queue is full")
                .is_empty(),
            "a WARNING log level mentioned mid-sentence is not a marker"
        );
        assert!(
            set.find("# emits a WARNING and a NOTE to the operator")
                .is_empty(),
            "marker words inside prose are not markers"
        );
        assert!(
            set.find("// see the TODO in the parser above").is_empty(),
            "a reference to a TODO elsewhere is not itself a marker"
        );
        // But a genuine leading callout still tags — even with a colon.
        assert_eq!(
            set.find("# WARNING: this mutates global state"),
            vec!["WARNING"]
        );
        assert_eq!(
            set.find("// NOTE(team): revisit after the migration"),
            vec!["NOTE"]
        );
    }

    #[test]
    fn test_marker_leads_after_block_comment_furniture() {
        // A `/* … */` block: the marker leads its line after the ` * ` continuation
        // furniture is stripped; the prose-mention line below it does not tag.
        let set = default_set();
        let block = "/*\n * TODO: refactor this\n * the WARNING path is fine\n */";
        assert_eq!(set.find(block), vec!["TODO"]);
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
    fn test_leads_with_builtin_marker_matches_find() {
        // The intent classifier's directive probe shares the leading-position
        // rule: a leading marker matches, a mid-sentence mention does not.
        assert!(leads_with_builtin_marker("# TODO: later"));
        assert!(leads_with_builtin_marker("/*\n * NOTE: revisit\n */"));
        assert!(!leads_with_builtin_marker("// log at WARNING level"));
        assert!(!leads_with_builtin_marker("# see the TODO above"));
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
