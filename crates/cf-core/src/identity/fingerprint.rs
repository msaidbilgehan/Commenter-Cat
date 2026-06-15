//! Cosmetic fingerprint (Idea §4; task 5.1).
//!
//! `cosmetic_fingerprint = hash(normalize(text))`. The normalization is
//! **lexical only** — no stemming, no stopword removal — and deliberately
//! *keeps the marker*: a `TODO`→`FIXME` escalation is a real change and must
//! alter the fingerprint. It strips the comment delimiter, collapses whitespace,
//! lowercases, and strips trailing punctuation, so a one-word reflow or a
//! capitalization edit does **not** orphan a suppression (Idea §4).

use sha2::{Digest, Sha256};

/// Trailing punctuation removed during normalization.
const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?'];

/// Longest-first leading comment delimiters stripped per line.
const LEADING_DELIMITERS: [&str; 5] = ["/**", "/*", "///", "//", "#"];

/// Normalizes comment text for cosmetic identity (Idea §4).
#[must_use]
pub fn normalize(text: &str) -> String {
    let stripped = strip_delimiters(text);
    let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let lowered = collapsed.to_lowercase();
    lowered
        .trim_end_matches(TRAILING_PUNCTUATION)
        .trim()
        .to_owned()
}

/// The cosmetic fingerprint: SHA-256 hex of the normalized text (Idea §4).
#[must_use]
pub fn cosmetic_fingerprint(text: &str) -> String {
    let normalized = normalize(text);
    Sha256::digest(normalized.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Strips comment delimiters from each line and joins the results with a space.
fn strip_delimiters(text: &str) -> String {
    text.lines()
        .map(strip_line_delimiters)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Strips one leading delimiter, a trailing block close, surrounding quotes
/// (docstrings), and a JSDoc continuation `*` from a single line.
fn strip_line_delimiters(line: &str) -> String {
    let mut body = line.trim();
    for delimiter in LEADING_DELIMITERS {
        if let Some(rest) = body.strip_prefix(delimiter) {
            body = rest;
            break;
        }
    }
    if let Some(rest) = body.strip_suffix("*/") {
        body = rest;
    }
    body = body.trim_matches(|c| c == '"' || c == '\'');
    body = body.trim_start_matches('*');
    body.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosmetic_edits_preserve_fingerprint() {
        let base = cosmetic_fingerprint("# TODO: fix the retry loop");
        // Whitespace reflow, case change, and trailing punctuation are cosmetic.
        assert_eq!(
            cosmetic_fingerprint("#   TODO:   fix the retry   loop"),
            base
        );
        assert_eq!(cosmetic_fingerprint("# todo: Fix The Retry Loop"), base);
        assert_eq!(cosmetic_fingerprint("# TODO: fix the retry loop."), base);
        // A different delimiter for the same content also matches.
        assert_eq!(cosmetic_fingerprint("// TODO: fix the retry loop"), base);
    }

    #[test]
    fn test_marker_change_alters_fingerprint() {
        let todo = cosmetic_fingerprint("# TODO: fix the retry loop");
        let fixme = cosmetic_fingerprint("# FIXME: fix the retry loop");
        assert_ne!(todo, fixme, "TODO→FIXME is a real change");
    }

    #[test]
    fn test_content_change_alters_fingerprint() {
        let a = cosmetic_fingerprint("# explains the cache");
        let b = cosmetic_fingerprint("# explains the index");
        assert_ne!(a, b);
    }

    #[test]
    fn test_normalize_strips_block_and_docstring_delimiters() {
        assert_eq!(normalize("/** documents foo */"), "documents foo");
        assert_eq!(
            normalize("\"\"\"module docstring\"\"\""),
            "module docstring"
        );
        assert_eq!(normalize("# a note"), "a note");
    }

    #[test]
    fn test_multiline_block_collapses() {
        let block = "// first line\n// second line";
        assert_eq!(normalize(block), "first line second line");
    }
}
