//! Comment→code mapping (Idea §3; task 2.4).
//!
//! The load-bearing handoff between drift the engine can *prove* and drift the
//! agent must *judge* (Idea §1, §3). Each comment gets a `bound_symbol` and,
//! when it annotates a specific node, a `bound_node_range`, via three
//! deterministic rules with **no scoring heuristic**:
//!
//! 1. **Doc comment** — bound by the language standard: PEP 257 (a Python
//!    docstring binds to its enclosing module/class/function) or JSDoc/TSDoc
//!    adjacency (a JSDoc block is a lead comment above the symbol).
//! 2. **Trailing comment** (code precedes it on the line) — binds to that
//!    statement.
//! 3. **Lead comment** (own line, next statement immediately below, no blank
//!    line) — binds down to that sibling.
//!
//! Anything else is an **orphan**: `bound_symbol` is the enclosing scope and
//! there is no `bound_node_range`.

pub mod doc_binding;
pub mod geometry;

use std::path::Path;

use tree_sitter::{Node, Parser};

use commenter_cat_core::comment::Comment;
use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
use commenter_cat_core::finding::Range;
use commenter_cat_core::kind::CommentKind;
use commenter_cat_core::lang::Language;
use commenter_cat_core::symbol::BoundSymbol;

use crate::extract::grammars;
use doc_binding::{nearest_scope, qualified_name, unwrap_export};
use geometry::{binds_down, is_own_line, next_code_sibling, start_line, trailing_target};

/// Maps every comment in place, setting `bound_symbol` and `bound_node_range`.
///
/// # Errors
/// Returns [`CommenterCatError::Map`] if the grammar cannot be set or parsing fails.
pub fn map_comments(
    source: &str,
    language: Language,
    grammar_path: &Path,
    comments: &mut [Comment],
) -> CommenterCatResult<()> {
    let grammar = grammars::grammar_for(language, grammar_path);
    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|e| CommenterCatError::map(format!("setting {language} grammar failed: {e}")))?;
    let tree = parser
        .parse(source.as_bytes(), None)
        .ok_or_else(|| CommenterCatError::map("tree-sitter produced no tree for mapping"))?;
    let root = tree.root_node();

    for comment in comments.iter_mut() {
        map_one(root, source, language, comment);
    }
    Ok(())
}

/// Maps a single comment.
fn map_one(root: Node<'_>, source: &str, language: Language, comment: &mut Comment) {
    let start = comment.range.start_byte as usize;
    let end = comment.range.end_byte as usize;
    let Some(node) = root.descendant_for_byte_range(start, end) else {
        comment.bound_symbol = Some(BoundSymbol::new("<module>"));
        return;
    };
    // The comment's container: its parent if `node` is the comment itself, else
    // the smallest node spanning a coalesced block.
    let container = if node.kind() == "comment" {
        node.parent().unwrap_or(node)
    } else {
        node
    };

    let (bound_node, symbol_node) = resolve_binding(node, container, source, language, comment);

    comment.bound_node_range = bound_node.map(node_range);
    comment.bound_symbol = Some(BoundSymbol::new(qualified_name(symbol_node, source)));
}

/// Returns `(bound code node, symbol node)` per the three rules; the symbol node
/// defaults to the enclosing-scope `node` (an orphan / trailing comment).
fn resolve_binding<'tree>(
    node: Node<'tree>,
    container: Node<'tree>,
    source: &str,
    language: Language,
    comment: &Comment,
) -> (Option<Node<'tree>>, Node<'tree>) {
    // Rule 1a — Python docstring binds to its PEP 257 owner scope.
    if comment.kind == CommentKind::Docstring && language == Language::Python {
        let owner = nearest_scope(node);
        return (Some(owner), owner);
    }

    if is_own_line(source, comment.range.start_byte) {
        // Rule 1b/3 — lead comment (incl. a JSDoc block) binds down to the next
        // sibling if it sits on the immediately following line.
        if let Some(sibling) = next_code_sibling(container, comment.range.end_byte) {
            if binds_down(comment, start_line(sibling)) {
                let target = unwrap_export(sibling);
                return (Some(target), target);
            }
        }
        // Orphan — enclosing scope only.
        (None, node)
    } else {
        // Rule 2 — trailing comment binds to the statement on its line.
        let target = trailing_target(
            container,
            comment.range.start_line,
            comment.range.start_byte,
        );
        (target, node)
    }
}

/// Converts a tree-sitter node into a [`Range`].
fn node_range(node: Node<'_>) -> Range {
    Range::new(
        node.start_byte() as u32,
        node.end_byte() as u32,
        node.start_position().row as u32 + 1,
        node.end_position().row as u32 + 1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::coalesce::coalesce;
    use crate::extract::extract_source;

    fn mapped(source: &str, language: Language) -> Vec<Comment> {
        let ext = match language {
            Language::Python => "py",
            Language::TypeScript => "ts",
            Language::JavaScript => "js",
            Language::Shell => "sh",
        };
        let path = format!("t.{ext}");
        let comments = extract_source(source, language, Path::new(&path), &path).unwrap();
        let mut comments = coalesce(source, comments);
        map_comments(source, language, Path::new(&path), &mut comments).unwrap();
        comments
    }

    fn symbol(c: &Comment) -> &str {
        c.bound_symbol.as_ref().map_or("", |s| s.as_str())
    }

    #[test]
    fn test_python_docstrings_bind_to_scope() {
        let src = "\"\"\"mod\"\"\"\nclass Foo:\n    \"\"\"cls\"\"\"\n    def bar(self):\n        \"\"\"meth\"\"\"\n        pass\n";
        let comments = mapped(src, Language::Python);
        let docstrings: Vec<_> = comments
            .iter()
            .filter(|c| c.kind == CommentKind::Docstring)
            .collect();
        assert_eq!(docstrings.len(), 3);
        assert_eq!(symbol(docstrings[0]), "<module>");
        assert_eq!(symbol(docstrings[1]), "Foo");
        assert_eq!(symbol(docstrings[2]), "Foo.bar");
        // The method docstring binds to the function node (has a node range).
        assert!(docstrings[2].bound_node_range.is_some());
    }

    #[test]
    fn test_python_lead_comment_binds_down_to_function() {
        let src = "# leads the function\ndef compute():\n    return 1\n";
        let comments = mapped(src, Language::Python);
        assert_eq!(comments.len(), 1);
        assert_eq!(symbol(&comments[0]), "compute");
        let bound = comments[0].bound_node_range.expect("binds down");
        assert_eq!(bound.start_line, 2);
    }

    #[test]
    fn test_python_trailing_comment_binds_to_statement() {
        let src = "def f():\n    x = 1  # trailing\n    return x\n";
        let comments = mapped(src, Language::Python);
        let trailing = comments
            .iter()
            .find(|c| c.raw_text.contains("trailing"))
            .unwrap();
        // Enclosing scope is the function; it binds to the assignment statement.
        assert_eq!(symbol(trailing), "f");
        let bound = trailing
            .bound_node_range
            .expect("binds to the same-line statement");
        assert_eq!(bound.start_line, 2);
    }

    #[test]
    fn test_blank_line_makes_an_orphan() {
        let src = "# orphaned by the blank line\n\ndef g():\n    pass\n";
        let comments = mapped(src, Language::Python);
        assert_eq!(
            comments[0].bound_node_range, None,
            "blank line breaks the lead binding"
        );
        assert_eq!(symbol(&comments[0]), "<module>");
    }

    #[test]
    fn test_typescript_jsdoc_adjacency() {
        let src = "/** documents foo */\nexport function foo(): void {}\n";
        let comments = mapped(src, Language::TypeScript);
        let doc = comments
            .iter()
            .find(|c| c.kind == CommentKind::Docstring)
            .unwrap();
        assert_eq!(symbol(doc), "foo", "JSDoc binds across the export wrapper");
        assert!(doc.bound_node_range.is_some());
    }

    #[test]
    fn test_javascript_lead_comment_binds_to_class() {
        let src = "// a class\nclass Widget {}\n";
        let comments = mapped(src, Language::JavaScript);
        assert_eq!(symbol(&comments[0]), "Widget");
    }

    #[test]
    fn test_shell_function_scope() {
        let src = "deploy() {\n  # inside deploy\n  echo hi\n}\n";
        let comments = mapped(src, Language::Shell);
        assert_eq!(symbol(&comments[0]), "deploy");
    }
}
