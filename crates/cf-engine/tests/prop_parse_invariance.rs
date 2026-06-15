//! Property tests for the safe-write promise (Idea §4a, §11).
//!
//! These are *load-bearing*: they prove, over arbitrary edits rather than chosen
//! examples, that the parse-invariant applier (a) preserves the code when a
//! comment is edited, (b) is idempotent, and (c) **aborts** rather than silently
//! corrupting code when an edit would inject a statement.

// Test code: unwrap/expect on known-good fixtures is idiomatic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use cf_core::comment::Comment;
use cf_core::lang::Language;
use cf_engine::extract::extract_source;
use cf_engine::ops::apply::apply_edit;
use proptest::prelude::*;

/// Re-locates the first comment in `source` (after each edit the byte offsets
/// move, so the comment must be re-extracted).
fn first_comment(source: &str) -> Comment {
    extract_source(source, Language::Python, Path::new("t.py"), "t.py")
        .expect("python parses")
        .into_iter()
        .next()
        .expect("a comment is present")
}

proptest! {
    /// Editing a comment to *any* single-line comment body leaves the surrounding
    /// code byte-for-byte intact.
    #[test]
    fn comment_edit_preserves_code(body in "[a-zA-Z0-9 ._()=+-]{0,40}") {
        let source = "# original explanation\nx = compute(1) + g(2)\nreturn x\n";
        let comment = first_comment(source);

        let edited = apply_edit(source, &comment, &format!("# {body}"), false)
            .expect("a comment-only edit always applies");
        // The two code lines survive untouched.
        prop_assert!(edited.new_source.contains("x = compute(1) + g(2)"));
        prop_assert!(edited.new_source.contains("return x"));
        // And nothing flagged the edit as behavior-bearing.
        prop_assert!(!edited.significant);
    }

    /// Replacing a comment with a bare statement injects code and MUST abort —
    /// the applier never silently corrupts the program.
    #[test]
    fn code_injection_always_aborts(name in "[a-z]{1,5}", value in 0u16..999) {
        let source = "# a comment\ny = 1\n";
        let comment = first_comment(source);
        let injected = format!("{name} = {value}");

        let result = apply_edit(source, &comment, &injected, false);
        prop_assert!(result.is_err(), "code injection must abort, not succeed silently");
    }

    /// Applying the same comment edit twice equals applying it once.
    #[test]
    fn apply_is_idempotent(body in "[a-zA-Z0-9 ]{1,30}") {
        let source = "# old note\nz = f(q)\n";
        let comment = first_comment(source);
        let text = format!("# {body}");

        let once = apply_edit(source, &comment, &text, false)
            .expect("first edit applies")
            .new_source;
        let comment2 = first_comment(&once);
        let twice = apply_edit(&once, &comment2, &text, false)
            .expect("second edit applies")
            .new_source;

        prop_assert_eq!(once, twice);
    }
}
