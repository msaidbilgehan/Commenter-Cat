//! Block coalescing (Idea §3; task 2.3).
//!
//! Adjacent **own-line** line comments with no blank line between them merge into
//! one logical block before mapping, so a multi-line `//`/`#` comment is treated
//! as a single unit. Docstrings are already single nodes and are untouched; a
//! trailing comment (code precedes it on the line) never coalesces with the next
//! line's lead comment; and any non-line comment (directive, docstring, …)
//! breaks a run.

use commenter_cat_core::comment::Comment;
use commenter_cat_core::finding::Range;
use commenter_cat_core::kind::CommentKind;

use crate::hash::sha256_hex;

/// Whether a comment starting at `start_byte` is alone on its line (only
/// whitespace precedes it) — i.e. a lead/standalone comment, not a trailing one.
#[must_use]
pub fn is_own_line(source: &str, start_byte: u32) -> bool {
    let start = start_byte as usize;
    let line_start = source[..start].rfind('\n').map_or(0, |nl| nl + 1);
    source[line_start..start].chars().all(char::is_whitespace)
}

/// Coalesces adjacent own-line line comments into logical blocks (Idea §3).
#[must_use]
pub fn coalesce(source: &str, comments: Vec<Comment>) -> Vec<Comment> {
    let mut result = Vec::new();
    let mut run: Vec<Comment> = Vec::new();

    for comment in comments {
        let mergeable =
            comment.kind == CommentKind::Line && is_own_line(source, comment.range.start_byte);
        let contiguous = run
            .last()
            .is_some_and(|last| comment.range.start_line == last.range.end_line + 1);

        if mergeable && (run.is_empty() || contiguous) {
            run.push(comment);
        } else {
            flush_run(&mut run, source, &mut result);
            if mergeable {
                run.push(comment);
            } else {
                result.push(comment);
            }
        }
    }
    flush_run(&mut run, source, &mut result);

    result.sort_by_key(|c| (c.range.start_byte, c.range.end_byte));
    result
}

/// Emits the pending run: a single comment passes through unchanged; two or more
/// merge into one `Block`.
fn flush_run(run: &mut Vec<Comment>, source: &str, result: &mut Vec<Comment>) {
    match run.len() {
        0 => {}
        1 => result.push(run.remove(0)),
        _ => {
            let first = &run[0];
            let last = &run[run.len() - 1];
            let start_byte = first.range.start_byte;
            let end_byte = last.range.end_byte;
            let raw = &source[start_byte as usize..end_byte as usize];
            let range = Range::new(
                start_byte,
                end_byte,
                first.range.start_line,
                last.range.end_line,
            );
            let merged = Comment::new(
                first.path.clone(),
                sha256_hex(raw.as_bytes()),
                first.language,
                CommentKind::Block,
                range,
                raw,
            );
            result.push(merged);
            run.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_source;
    use commenter_cat_core::lang::Language;
    use std::path::Path;

    fn coalesced(source: &str, language: Language) -> Vec<Comment> {
        let comments = extract_source(source, language, Path::new("t.py"), "t.py").unwrap();
        coalesce(source, comments)
    }

    #[test]
    fn test_adjacent_line_comments_merge() {
        let out = coalesced("# a\n# b\n# c\n", Language::Python);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, CommentKind::Block);
        assert_eq!(out[0].raw_text, "# a\n# b\n# c");
        assert_eq!(out[0].range.start_line, 1);
        assert_eq!(out[0].range.end_line, 3);
    }

    #[test]
    fn test_blank_line_breaks_the_run() {
        let out = coalesced("# a\n\n# b\n", Language::Python);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].kind, CommentKind::Line);
        assert_eq!(out[1].kind, CommentKind::Line);
    }

    #[test]
    fn test_trailing_comment_does_not_coalesce_with_next_lead() {
        let out = coalesced("x = 1  # trailing\n# lead\n", Language::Python);
        assert_eq!(out.len(), 2, "trailing + lead stay separate");
        assert!(out.iter().all(|c| c.kind == CommentKind::Line));
    }

    #[test]
    fn test_directive_interrupts_a_run() {
        let out = coalesced("# a\n# noqa: E501\n# b\n", Language::Python);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].kind, CommentKind::Directive);
    }

    #[test]
    fn test_single_line_comment_unchanged() {
        let out = coalesced("# solo\n", Language::Python);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, CommentKind::Line);
    }
}
