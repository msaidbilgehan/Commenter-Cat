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
//!   which differ and abort.
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
    let is_python_docstring =
        comment.kind == CommentKind::Docstring && comment.language == Language::Python;

    let old_tree = parse(source, comment.language, path)?;
    let new_tree = parse(&new_source, comment.language, path)?;

    // The byte range to exclude as "the comment" — only needed for Python
    // docstrings (string nodes); ordinary comments are excluded by kind.
    let old_exclude = is_python_docstring.then_some((start, end));
    let new_exclude = if is_python_docstring {
        Some(python_docstring_range(&new_tree, start).ok_or_else(|| {
            CommenterCatError::apply("edit would break the docstring into code (parse-invariance)")
        })?)
    } else {
        None
    };

    let old_signature = code_signature(&old_tree, source, old_exclude);
    let new_signature = code_signature(&new_tree, &new_source, new_exclude);
    if old_signature != new_signature {
        return Err(CommenterCatError::apply(
            "parse-invariance violated: the edit would alter a code node",
        ));
    }
    Ok(new_source)
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
    if code_signature(&old_tree, source, None) != code_signature(&new_tree, &new_source, None) {
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
/// leaf within `exclude` (the edited Python docstring's range).
fn code_signature(
    tree: &Tree,
    source: &str,
    exclude: Option<(usize, usize)>,
) -> Vec<(String, String)> {
    let mut signature = Vec::new();
    collect_code_leaves(tree.root_node(), source, exclude, &mut signature);
    signature
}

fn collect_code_leaves(
    node: Node<'_>,
    source: &str,
    exclude: Option<(usize, usize)>,
    out: &mut Vec<(String, String)>,
) {
    if node.child_count() == 0 {
        if node.kind() == "comment" {
            return;
        }
        if let Some((start, end)) = exclude {
            if node.start_byte() >= start && node.end_byte() <= end {
                return;
            }
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
    fn test_block_comment_swallowing_code_aborts() {
        let source = "// note\nconst x = 1;\n";
        let comment = first_comment(source, Language::JavaScript, "note");
        // Turning the line comment into an unterminated block comment swallows
        // the following statement.
        assert!(apply_comment_edit(source, &comment, "/* note").is_err());
    }
}
