//! Comment-kind classification (Idea §3).
//!
//! Classifies an extracted comment into one of the [`CommentKind`] variants from
//! its text, language, and position. The order matters — the most specific,
//! position-anchored kinds (shebang, encoding-decl) are tested before the
//! content kinds (directive, docstring, license), with line/block as the
//! fallback. Python docstrings are string nodes, not comment nodes, so the
//! extractor flags them via `is_docstring_node`.

use cf_core::kind::CommentKind;
use cf_core::lang::Language;

/// Lines from the file top within which a license header is recognized.
const LICENSE_HEADER_MAX_LINE: u32 = 15;

/// Lead tokens (after the comment delimiter is stripped) that mark a control
/// directive (Idea §3, §4a) — `cf:*` plus the native tool directives CF must
/// treat as behavior-bearing.
const DIRECTIVE_PREFIXES: [&str; 14] = [
    "cf:",
    "noqa",
    "type: ignore",
    "type:ignore",
    "eslint-disable",
    "eslint-enable",
    "@ts-expect-error",
    "@ts-ignore",
    "@ts-nocheck",
    "shellcheck disable",
    "shellcheck source",
    "pylint:",
    "pyright:",
    "prettier-ignore",
];

/// Strong, low-false-positive license-header markers.
const LICENSE_MARKERS: [&str; 5] = [
    "copyright",
    "spdx-license-identifier",
    "all rights reserved",
    "licensed under",
    "permission is hereby granted",
];

/// Classifies a comment (Idea §3). `is_docstring_node` is set by the extractor
/// for Python docstring string-literals (which are not comment nodes).
#[must_use]
pub fn classify(
    language: Language,
    raw_text: &str,
    start_line: u32,
    is_docstring_node: bool,
) -> CommentKind {
    if is_docstring_node {
        return CommentKind::Docstring;
    }

    let trimmed_raw = raw_text.trim_start();
    let inner = strip_delimiters(raw_text);
    let inner_trimmed = inner.trim_start();

    // Shebang: a `#!` on the very first line.
    if start_line == 1 && trimmed_raw.starts_with("#!") {
        return CommentKind::Shebang;
    }
    // Encoding declaration (PEP 263): a `coding:` line at the file top.
    if language == Language::Python && start_line <= 2 && is_encoding_decl(inner_trimmed) {
        return CommentKind::EncodingDecl;
    }
    // Control directive (`cf:*`, `# noqa`, `// eslint-disable`, …).
    if is_directive(inner_trimmed) {
        return CommentKind::Directive;
    }
    // JSDoc/TSDoc doc-comment (`/** … */`).
    if matches!(language, Language::TypeScript | Language::JavaScript)
        && trimmed_raw.starts_with("/**")
    {
        return CommentKind::Docstring;
    }
    // License header near the file top.
    if start_line <= LICENSE_HEADER_MAX_LINE && is_license(inner) {
        return CommentKind::License;
    }
    // Block (`/* … */` or multi-line) vs. single line.
    if is_block(trimmed_raw) {
        return CommentKind::Block;
    }
    CommentKind::Line
}

/// Removes the opening (and closing, for block) comment delimiters.
fn strip_delimiters(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("/**") {
        return rest.strip_suffix("*/").unwrap_or(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("/*") {
        return rest.strip_suffix("*/").unwrap_or(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        return rest;
    }
    if let Some(rest) = trimmed.strip_prefix('#') {
        // Covers `#` and `#!` (shebang is detected from the raw text first).
        return rest;
    }
    trimmed
}

/// Whether the stripped content is a PEP 263 encoding declaration.
fn is_encoding_decl(inner: &str) -> bool {
    inner.contains("coding:") || inner.contains("coding=")
}

/// Whether the stripped content leads with a control-directive token.
fn is_directive(inner_trimmed: &str) -> bool {
    DIRECTIVE_PREFIXES
        .iter()
        .any(|prefix| inner_trimmed.starts_with(prefix))
}

/// Whether the content carries a strong license-header marker.
fn is_license(inner: &str) -> bool {
    let lowered = inner.to_ascii_lowercase();
    LICENSE_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

/// Whether the comment is a block comment (C-style or multi-line).
fn is_block(trimmed_raw: &str) -> bool {
    trimmed_raw.starts_with("/*") || trimmed_raw.contains('\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_line(language: Language, text: &str, line: u32) -> CommentKind {
        classify(language, text, line, false)
    }

    #[test]
    fn test_shebang_only_on_first_line() {
        assert_eq!(
            classify_line(Language::Shell, "#!/bin/bash", 1),
            CommentKind::Shebang
        );
        assert_eq!(
            classify_line(Language::Python, "#!/usr/bin/env python", 1),
            CommentKind::Shebang
        );
        // A `#!`-looking comment not on line 1 is just a line comment.
        assert_eq!(
            classify_line(Language::Shell, "#! not really", 5),
            CommentKind::Line
        );
    }

    #[test]
    fn test_encoding_declaration() {
        assert_eq!(
            classify_line(Language::Python, "# -*- coding: utf-8 -*-", 1),
            CommentKind::EncodingDecl
        );
        assert_eq!(
            classify_line(Language::Python, "# coding=latin-1", 2),
            CommentKind::EncodingDecl
        );
        // Only near the top, and only Python.
        assert_eq!(
            classify_line(Language::Python, "# coding: utf-8", 9),
            CommentKind::Line
        );
    }

    #[test]
    fn test_directives_across_languages() {
        assert_eq!(
            classify_line(Language::Python, "# noqa: E501", 4),
            CommentKind::Directive
        );
        assert_eq!(
            classify_line(Language::Python, "# type: ignore", 4),
            CommentKind::Directive
        );
        assert_eq!(
            classify_line(Language::JavaScript, "// eslint-disable-next-line", 4),
            CommentKind::Directive
        );
        assert_eq!(
            classify_line(Language::TypeScript, "// @ts-expect-error legacy", 4),
            CommentKind::Directive
        );
        assert_eq!(
            classify_line(Language::Shell, "# shellcheck disable=SC2086", 4),
            CommentKind::Directive
        );
        assert_eq!(
            classify_line(Language::Python, "# cf:disable=D417", 4),
            CommentKind::Directive
        );
    }

    #[test]
    fn test_jsdoc_docstring_and_block() {
        assert_eq!(
            classify_line(Language::TypeScript, "/** doc */", 3),
            CommentKind::Docstring
        );
        assert_eq!(
            classify_line(Language::JavaScript, "/* plain block */", 3),
            CommentKind::Block
        );
        assert_eq!(
            classify_line(Language::Python, "# regular", 3),
            CommentKind::Line
        );
    }

    #[test]
    fn test_python_docstring_node_flag() {
        assert_eq!(
            classify(Language::Python, "\"\"\"module doc\"\"\"", 1, true),
            CommentKind::Docstring
        );
    }

    #[test]
    fn test_license_header() {
        assert_eq!(
            classify_line(Language::Python, "# Copyright 2026 Muhammed Said", 1),
            CommentKind::License
        );
        assert_eq!(
            classify_line(
                Language::JavaScript,
                "// SPDX-License-Identifier: BSD-3-Clause",
                1
            ),
            CommentKind::License
        );
        // The same keyword far down the file is not a header.
        assert_eq!(
            classify_line(Language::Python, "# discusses copyright law", 40),
            CommentKind::Line
        );
    }
}
