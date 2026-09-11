//! Parse-invariant comment edits (Idea §4a, §5; task 7.2).
//!
//! THE pillar that lets an external agent hold the write path. After a
//! comment-only edit we re-parse and assert the **code-node tree is byte-
//! identical**; if any code node changed, we **abort**. The check compares the
//! ordered stream of non-comment leaf tokens `(kind, text)`:
//!
//! * a normal comment is a tree-sitter `comment` node → excluded by kind, so
//!   the edit can freely change it but cannot inject or swallow code (injected
//!   code appears as new tokens; swallowed code disappears — either way the
//!   streams differ and we abort);
//! * a Python docstring is a `string` node (real code by kind), so it is
//!   excluded by its **dynamic range** in the re-parsed tree — an edit that
//!   breaks the string open (`"""x"""; evil()`) leaves `evil()` as code tokens,
//!   which differ and abort. A *deletion* leaves no string to exclude, and the
//!   token stream alone decides: the `strip` pass drops a docstring this way,
//!   while dropping a symbol's **only** docstring still aborts (the emptied
//!   suite loses its indent/dedent tokens, so the streams differ).
//!
//! The operation is deterministic and idempotent. Parse-invariance is property-
//! tested over arbitrary edits in Phase 10 (the load-bearing proptest).

use std::path::Path;

use tree_sitter::{Node, Parser, Tree};

use commenter_cat_core::comment::Comment;
use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
use commenter_cat_core::kind::CommentKind;
use commenter_cat_core::lang::Language;

use crate::extract::grammars;

/// Applies a comment-only edit, returning the new source, or aborts if the edit
/// would alter any code node (parse-invariance violated).
///
/// # Errors
/// Returns [`CommenterCatError::Apply`] if the range is out of bounds, a grammar fails to
/// parse, a Python docstring is broken into code, or any code node changes.
pub fn apply_comment_edit(
    source: &str,
    comment: &Comment,
    new_text: &str,
) -> CommenterCatResult<String> {
    let start = comment.range.start_byte as usize;
    let end = comment.range.end_byte as usize;
    if start > end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return Err(CommenterCatError::apply(
            "comment range is out of bounds for the source",
        ));
    }
    let new_source = format!("{}{}{}", &source[..start], new_text, &source[end..]);

    let path = Path::new(&comment.path);
    let is_python_docstring = is_code_node(comment);

    let old_tree = parse(source, comment.language, path)?;
    let new_tree = parse(&new_source, comment.language, path)?;

    // The byte range to exclude as "the comment" — only needed for Python
    // docstrings (string nodes); ordinary comments are excluded by kind. When
    // the new text leaves no string at the site the edit *deleted* the docstring
    // (`remove`, or a rewrite into a `#` comment), so there is nothing to
    // exclude and the signature comparison alone is the guarantee: a deletion
    // yields an identical code stream, while a break-out keeps its string
    // (`"""x"""; evil()` → the string is excluded and `evil()` shows up as extra
    // tokens) and still aborts.
    let old_exclude: Vec<(usize, usize)> = is_python_docstring
        .then_some((start, end))
        .into_iter()
        .collect();
    let new_exclude: Vec<(usize, usize)> = is_python_docstring
        .then(|| python_docstring_range(&new_tree, start))
        .flatten()
        .into_iter()
        .collect();

    let old_signature = code_signature(&old_tree, source, &old_exclude);
    let new_signature = code_signature(&new_tree, &new_source, &new_exclude);
    if old_signature != new_signature {
        return Err(CommenterCatError::apply(explain_divergence(
            &old_tree,
            &new_tree,
            comment.language,
        )));
    }
    Ok(new_source)
}

/// The most actionable phrasing for a token-stream divergence.
///
/// A Python suite left with no statement — deleting a symbol's *only* docstring
/// — is the one divergence with a cause specific enough to name, and a bulk
/// `strip` hits it often enough that "would alter a code node" leaves the
/// operator guessing. Everything else is the general violation.
fn explain_divergence(old_tree: &Tree, new_tree: &Tree, language: Language) -> &'static str {
    if empty_block_count(new_tree, language) > empty_block_count(old_tree, language) {
        return "parse-invariance violated: the edit would leave a symbol with an empty body \
                (its only docstring) — the code would no longer parse";
    }
    "parse-invariance violated: the edit would alter a code node"
}

/// The number of childless Python `block` nodes — suites that would raise
/// `IndentationError`. No valid Python has an empty block, so comparing the
/// count before and after an edit names the emptied-suite case exactly.
fn empty_block_count(tree: &Tree, language: Language) -> usize {
    if language != Language::Python {
        return 0;
    }
    let mut count = 0;
    count_empty_blocks(tree.root_node(), &mut count);
    count
}

fn count_empty_blocks(node: Node<'_>, count: &mut usize) {
    if node.kind() == "block" && node.named_child_count() == 0 {
        *count += 1;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count_empty_blocks(child, count);
    }
}

/// Whether `after` carries the identical non-comment leaf-token stream as
/// `before`, once `deleted_code_spans` — ranges in `before` holding comments the
/// parser reads as *code* (see [`is_code_node`]) — are excluded.
///
/// Whole-file parse-invariance, for a transform that is not a single comment
/// edit. The `strip` pass verifies a whole batch of deletions with it (two
/// parses per file instead of two per comment, so a file with hundreds of
/// comments stays linear) and re-checks its blank-line tidy with an empty span
/// list.
///
/// # Errors
/// Returns [`CommenterCatError::Apply`] if either source fails to parse.
pub fn code_unchanged(
    before: &str,
    after: &str,
    deleted_code_spans: &[(usize, usize)],
    language: Language,
    path: &Path,
) -> CommenterCatResult<bool> {
    let old_tree = parse(before, language, path)?;
    let new_tree = parse(after, language, path)?;
    Ok(code_signature(&old_tree, before, deleted_code_spans)
        == code_signature(&new_tree, after, &[]))
}

/// Whether the parser reads this comment as a **code node** — a Python docstring
/// is a `string`, not a `comment`, so its byte range must be excluded by hand
/// wherever a signature is taken. Every other kind is excluded by node kind.
#[must_use]
pub fn is_code_node(comment: &Comment) -> bool {
    comment.kind == CommentKind::Docstring && comment.language == Language::Python
}

/// Inserts comment text at `byte_pos`, returning the new source, or aborts if
/// the insertion would parse as anything other than a comment (Idea §5: the
/// suppression-export path writes native directives this way).
///
/// # Errors
/// Returns [`CommenterCatError::Apply`] if the position is invalid, parsing fails, or the
/// inserted text would change a code node.
pub fn insert_comment(
    source: &str,
    language: Language,
    path: &Path,
    byte_pos: usize,
    text: &str,
) -> CommenterCatResult<String> {
    if byte_pos > source.len() || !source.is_char_boundary(byte_pos) {
        return Err(CommenterCatError::apply(
            "insert position is out of bounds for the source",
        ));
    }
    let new_source = format!("{}{}{}", &source[..byte_pos], text, &source[byte_pos..]);
    let old_tree = parse(source, language, path)?;
    let new_tree = parse(&new_source, language, path)?;
    if code_signature(&old_tree, source, &[]) != code_signature(&new_tree, &new_source, &[]) {
        return Err(CommenterCatError::apply(
            "inserted directive would alter a code node (parse-invariance)",
        ));
    }
    Ok(new_source)
}

/// Parses `source` with the file's grammar.
fn parse(source: &str, language: Language, path: &Path) -> CommenterCatResult<Tree> {
    let grammar = grammars::grammar_for(language, path);
    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|e| CommenterCatError::apply(format!("setting {language} grammar failed: {e}")))?;
    parser
        .parse(source.as_bytes(), None)
        .ok_or_else(|| CommenterCatError::apply("tree-sitter produced no tree for the edit"))
}

/// The ordered `(kind, text)` stream of non-comment leaf tokens, excluding any
/// leaf inside one of the `exclude` ranges (the Python docstrings being edited
/// or deleted — `string` nodes the parser reads as code). Ranges must be sorted
/// by start byte and non-overlapping, which extraction order already guarantees.
fn code_signature(tree: &Tree, source: &str, exclude: &[(usize, usize)]) -> Vec<(String, String)> {
    let mut signature = Vec::new();
    collect_code_leaves(tree.root_node(), source, exclude, &mut signature);
    signature
}

fn collect_code_leaves(
    node: Node<'_>,
    source: &str,
    exclude: &[(usize, usize)],
    out: &mut Vec<(String, String)>,
) {
    if node.child_count() == 0 {
        if node.kind() == "comment" || is_excluded(exclude, node.start_byte(), node.end_byte()) {
            return;
        }
        out.push((
            node.kind().to_owned(),
            source[node.start_byte()..node.end_byte()].to_owned(),
        ));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_code_leaves(child, source, exclude, out);
    }
}

/// Whether `[start, end)` lies inside one of the sorted, non-overlapping
/// `exclude` ranges. Only the last range starting at or before `start` can
/// contain the leaf, so the lookup is a binary search rather than a scan — a
/// whole-file batch may exclude hundreds of docstrings.
fn is_excluded(exclude: &[(usize, usize)], start: usize, end: usize) -> bool {
    exclude
        .partition_point(|(range_start, _)| *range_start <= start)
        .checked_sub(1)
        .and_then(|previous| exclude.get(previous))
        .is_some_and(|(range_start, range_end)| start >= *range_start && end <= *range_end)
}

/// The byte range of the `string` node containing `byte`, if any (the re-parsed
/// docstring).
fn python_docstring_range(tree: &Tree, byte: usize) -> Option<(usize, usize)> {
    let mut current = tree.root_node().descendant_for_byte_range(byte, byte);
    while let Some(node) = current {
        if node.kind() == "string" {
            return Some((node.start_byte(), node.end_byte()));
        }
        current = node.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_source;

    fn first_comment(source: &str, language: Language, matching: &str) -> Comment {
        let path = match language {
            Language::Python => "t.py",
            Language::JavaScript => "t.js",
            Language::TypeScript => "t.ts",
            Language::Shell => "t.sh",
        };
        extract_source(source, language, Path::new(path), path)
            .unwrap()
            .into_iter()
            .find(|c| c.raw_text.contains(matching))
            .expect("comment present")
    }

    #[test]
    fn test_comment_only_edit_applies() {
        let source = "# old explanation\nx = 1\n";
        let comment = first_comment(source, Language::Python, "old");
        let result = apply_comment_edit(source, &comment, "# new explanation").unwrap();
        assert_eq!(result, "# new explanation\nx = 1\n");
    }

    #[test]
    fn test_injecting_code_aborts() {
        let source = "# a comment\nx = 1\n";
        let comment = first_comment(source, Language::Python, "comment");
        // Replacing the comment with an assignment injects code.
        assert!(apply_comment_edit(source, &comment, "y = 2").is_err());
    }

    #[test]
    fn test_idempotent() {
        let source = "# old\nreturn_value = compute()\n";
        let comment = first_comment(source, Language::Python, "old");
        let once = apply_comment_edit(source, &comment, "# revised").unwrap();
        // Re-locating the comment in the edited source and re-applying is a no-op.
        let comment2 = first_comment(&once, Language::Python, "revised");
        let twice = apply_comment_edit(&once, &comment2, "# revised").unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn test_python_docstring_edit_applies() {
        let source = "def f(x):\n    \"\"\"old doc\"\"\"\n    return x\n";
        let comment = first_comment(source, Language::Python, "old doc");
        assert_eq!(comment.kind, CommentKind::Docstring);
        let result = apply_comment_edit(source, &comment, "\"\"\"new doc\"\"\"").unwrap();
        assert!(result.contains("\"\"\"new doc\"\"\""));
        assert!(result.contains("return x"));
    }

    #[test]
    fn test_breaking_docstring_into_code_aborts() {
        let source = "def f(x):\n    \"\"\"old\"\"\"\n    return x\n";
        let comment = first_comment(source, Language::Python, "old");
        // Closing the string early and appending a call injects code.
        assert!(apply_comment_edit(source, &comment, "\"\"\"x\"\"\"; evil()").is_err());
    }

    #[test]
    fn test_docstring_deletion_applies() {
        // Deleting a docstring leaves no string at the site; the signature
        // comparison (not a "there must still be a string here" rule) is what
        // proves the code is untouched.
        let source = "def f(x):\n    \"\"\"old doc\"\"\"\n    return x\n";
        let comment = first_comment(source, Language::Python, "old doc");
        let result = apply_comment_edit(source, &comment, "").unwrap();
        assert_eq!(result, "def f(x):\n    \n    return x\n");
    }

    #[test]
    fn test_deleting_a_symbols_only_docstring_aborts() {
        // Deletion is permitted in general (above), but not when the docstring
        // *is* the body: the emptied suite drops its indent/dedent tokens, the
        // streams diverge, and the edit aborts rather than writing unparseable
        // Python.
        let source = "def f(x):\n    \"\"\"the whole body\"\"\"\n";
        let comment = first_comment(source, Language::Python, "whole body");
        let error = apply_comment_edit(source, &comment, "")
            .expect_err("emptying the suite would leave an IndentationError on disk");
        assert!(
            error.to_string().contains("empty body"),
            "the message names the cause, not just the violation: {error}"
        );
    }

    #[test]
    fn test_code_unchanged_tracks_code_not_whitespace() {
        let path = Path::new("t.py");
        // Dropping a blank line and trailing whitespace is code-invariant.
        assert!(code_unchanged(
            "x = 1  \n\ny = 2\n",
            "x = 1\ny = 2\n",
            &[],
            Language::Python,
            path
        )
        .unwrap());
        // Changing a statement is not.
        assert!(!code_unchanged("x = 1\n", "x = 2\n", &[], Language::Python, path).unwrap());
        // Neither is emptying a suite.
        assert!(!code_unchanged(
            "def f():\n    pass\n",
            "def f():\n\n",
            &[],
            Language::Python,
            path
        )
        .unwrap());
    }

    #[test]
    fn test_block_comment_swallowing_code_aborts() {
        let source = "// note\nconst x = 1;\n";
        let comment = first_comment(source, Language::JavaScript, "note");
        // Turning the line comment into an unterminated block comment swallows
        // the following statement.
        assert!(apply_comment_edit(source, &comment, "/* note").is_err());
    }
}
