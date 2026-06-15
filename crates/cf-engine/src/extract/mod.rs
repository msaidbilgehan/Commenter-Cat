//! Comment extraction (Idea §3; task 2.2).
//!
//! Parses a source file with its pinned tree-sitter grammar and produces
//! [`Comment`] records — tree-sitter, never regex, so there are **zero
//! string-vs-comment false positives** (Idea §3). It collects:
//!
//! * every `comment` node (line / block / JSDoc, plus shebang / encoding-decl /
//!   directive / license, distinguished by [`kind::classify`]); and
//! * Python **docstrings**, which are `string` nodes in the first-statement
//!   position of a module / class / function body (PEP 257), not comment nodes.
//!
//! Byte offsets come straight from tree-sitter, so CRLF line endings are handled
//! correctly (the `\r` is inside the line, Idea §10/§11 Windows support).

pub mod coalesce;
pub mod grammars;
pub mod kind;

use std::fs;
use std::path::Path;

use tree_sitter::{Node, Parser};

use cf_core::comment::Comment;
use cf_core::error::{CfError, CfResult};
use cf_core::finding::Range;
use cf_core::lang::Language;

use crate::hash::sha256_hex;
use crate::walk::{to_repo_relative, WalkedFile};

/// Extracts comments from a walked file, reading it from disk.
///
/// # Errors
/// Returns [`CfError::Extract`] if the file cannot be read, is not valid UTF-8,
/// or the grammar fails to parse it.
pub fn extract_file(walked: &WalkedFile, root: &Path) -> CfResult<Vec<Comment>> {
    let bytes = fs::read(&walked.path)
        .map_err(|e| CfError::extract(format!("reading {}", walked.path.display())).caused_by(e))?;
    let source = String::from_utf8(bytes).map_err(|e| {
        CfError::extract(format!("{} is not valid UTF-8", walked.path.display())).caused_by(e)
    })?;
    let repo_path = to_repo_relative(&walked.path, root);
    extract_source(&source, walked.language, &walked.path, &repo_path)
}

/// Extracts comments from in-memory source. `grammar_path` selects the TS/TSX
/// grammar by extension; `repo_path` is the `/`-normalized path stored on each
/// record.
///
/// # Errors
/// Returns [`CfError::Extract`] if the grammar cannot be set or parsing yields
/// no tree.
pub fn extract_source(
    source: &str,
    language: Language,
    grammar_path: &Path,
    repo_path: &str,
) -> CfResult<Vec<Comment>> {
    let grammar = grammars::grammar_for(language, grammar_path);
    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|e| CfError::extract(format!("setting {language} grammar failed: {e}")))?;
    let tree = parser
        .parse(source.as_bytes(), None)
        .ok_or_else(|| CfError::extract(format!("tree-sitter produced no tree for {repo_path}")))?;
    let root = tree.root_node();

    let mut comments = Vec::new();
    collect_comment_nodes(root, source, language, repo_path, &mut comments);
    if language == Language::Python {
        collect_python_docstrings(root, source, repo_path, &mut comments);
    }

    // Deterministic source order.
    comments.sort_by_key(|c| (c.range.start_byte, c.range.end_byte));
    Ok(comments)
}

/// Recursively collects `comment` nodes.
fn collect_comment_nodes(
    node: Node<'_>,
    source: &str,
    language: Language,
    repo_path: &str,
    out: &mut Vec<Comment>,
) {
    if node.kind() == "comment" {
        out.push(build_comment(node, source, language, repo_path, false));
        return; // comments have no comment children
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comment_nodes(child, source, language, repo_path, out);
    }
}

/// Recursively collects Python docstrings (first-statement `string` nodes of a
/// module / class / function body — PEP 257).
fn collect_python_docstrings(
    node: Node<'_>,
    source: &str,
    repo_path: &str,
    out: &mut Vec<Comment>,
) {
    let body = match node.kind() {
        "module" => Some(node),
        "function_definition" | "class_definition" => node.child_by_field_name("body"),
        _ => None,
    };
    if let Some(body) = body {
        if let Some(docstring) = first_statement_docstring(body) {
            out.push(build_comment(
                docstring,
                source,
                Language::Python,
                repo_path,
                true,
            ));
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_python_docstrings(child, source, repo_path, out);
    }
}

/// The `string` node of `body`'s first statement, if that statement is a bare
/// string expression (a docstring). Leading comments are skipped.
fn first_statement_docstring<'tree>(body: Node<'tree>) -> Option<Node<'tree>> {
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        match child.kind() {
            // Comments precede but are not themselves statements.
            "comment" => continue,
            "expression_statement" => {
                let mut inner = child.walk();
                return child
                    .named_children(&mut inner)
                    .find(|n| n.kind() == "string");
            }
            // The first real statement is not a string → no docstring.
            _ => return None,
        }
    }
    None
}

/// Builds a [`Comment`] from a node, classifying its kind and hashing its text.
fn build_comment(
    node: Node<'_>,
    source: &str,
    language: Language,
    repo_path: &str,
    is_docstring_node: bool,
) -> Comment {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    let raw_text = &source[start_byte..end_byte];
    let start_line = node.start_position().row as u32 + 1;
    let end_line = node.end_position().row as u32 + 1;

    let comment_kind = kind::classify(language, raw_text, start_line, is_docstring_node);
    let range = Range::new(start_byte as u32, end_byte as u32, start_line, end_line);
    Comment::new(
        repo_path,
        sha256_hex(raw_text.as_bytes()),
        language,
        comment_kind,
        range,
        raw_text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_core::kind::CommentKind;

    fn extract(source: &str, language: Language) -> Vec<Comment> {
        let ext = match language {
            Language::Python => "py",
            Language::TypeScript => "ts",
            Language::JavaScript => "js",
            Language::Shell => "sh",
        };
        extract_source(
            source,
            language,
            Path::new(&format!("t.{ext}")),
            &format!("t.{ext}"),
        )
        .unwrap()
    }

    fn kinds(comments: &[Comment]) -> Vec<CommentKind> {
        comments.iter().map(|c| c.kind).collect()
    }

    #[test]
    fn test_python_full_spectrum() {
        let src = "#!/usr/bin/env python\n# -*- coding: utf-8 -*-\n# a line\n\"\"\"module doc\"\"\"\ndef f(x):\n    \"\"\"func doc\"\"\"\n    return x  # trailing\n";
        let comments = extract(src, Language::Python);
        assert_eq!(
            kinds(&comments),
            vec![
                CommentKind::Shebang,
                CommentKind::EncodingDecl,
                CommentKind::Line,
                CommentKind::Docstring, // module
                CommentKind::Docstring, // function
                CommentKind::Line,      // trailing
            ]
        );
        // The module docstring text is captured with its delimiters.
        assert_eq!(comments[3].raw_text, "\"\"\"module doc\"\"\"");
    }

    #[test]
    fn test_zero_string_vs_comment_false_positives() {
        // The `#` and `//` live inside string literals — never comments.
        let py = extract(
            "x = \"# not a comment\"\ny = '// also not'\n",
            Language::Python,
        );
        assert!(py.is_empty(), "no comments, got {:?}", kinds(&py));
        let js = extract(
            "const s = \"// not a comment\";\nconst u = `# nope`;\n",
            Language::JavaScript,
        );
        assert!(js.is_empty(), "no comments, got {:?}", kinds(&js));
    }

    #[test]
    fn test_typescript_kinds() {
        let src = "/** doc */\n// @ts-expect-error legacy\nconst x: number = 1; // trailing\n";
        let comments = extract(src, Language::TypeScript);
        assert_eq!(
            kinds(&comments),
            vec![
                CommentKind::Docstring,
                CommentKind::Directive,
                CommentKind::Line
            ]
        );
    }

    #[test]
    fn test_shell_kinds() {
        let src = "#!/bin/bash\n# a note\n# shellcheck disable=SC2086\necho \"$x\"\n";
        let comments = extract(src, Language::Shell);
        assert_eq!(
            kinds(&comments),
            vec![
                CommentKind::Shebang,
                CommentKind::Line,
                CommentKind::Directive
            ]
        );
    }

    #[test]
    fn test_byte_and_line_ranges() {
        let comments = extract("# first\n# second\n", Language::Python);
        assert_eq!(comments.len(), 2);
        assert_eq!(
            (comments[0].range.start_byte, comments[0].range.end_byte),
            (0, 7)
        );
        assert_eq!(comments[0].range.start_line, 1);
        // "# first\n" is 8 bytes, so the second comment starts at byte 8.
        assert_eq!(comments[1].range.start_byte, 8);
        assert_eq!(comments[1].range.start_line, 2);
    }

    #[test]
    fn test_crlf_byte_offsets() {
        // CRLF: "# a\r\n" is 5 bytes, so the second comment starts at byte 5.
        let comments = extract("# a\r\n# b\r\n", Language::Python);
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].range.start_byte, 0);
        assert_eq!(comments[1].range.start_byte, 5);
        assert_eq!(comments[1].range.start_line, 2);
    }

    #[test]
    fn test_content_hash_is_set() {
        let comments = extract("# hello\n", Language::Python);
        assert_eq!(comments[0].content_hash, sha256_hex(b"# hello"));
    }
}
