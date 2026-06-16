//! Golden-file tests for per-grammar extraction + mapping (Idea §3, §11).
//!
//! `insta` snapshots pin the observable output of the native pass for each
//! grammar — the comment `kind`, the `bound_symbol` the mapping resolved, and the
//! line span. A grammar bump or a mapping regression shows up as a snapshot diff,
//! reviewed deliberately (`cargo insta review`), never silently.

// Test code: unwrap/expect on known-good fixtures is idiomatic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use commenter_cat_core::lang::Language;
use commenter_cat_engine::extract::coalesce::coalesce;
use commenter_cat_engine::extract::extract_source;
use commenter_cat_engine::map::map_comments;

/// A stable, snapshot-friendly projection of a comment record. Fields are read
/// only through the `Debug` snapshot, so clippy can't see the use.
#[derive(Debug)]
#[allow(dead_code)]
struct GoldenComment {
    kind: String,
    bound_symbol: Option<String>,
    start_line: u32,
    end_line: u32,
    text: String,
}

/// Runs the native pass (extract → coalesce → map) and projects to goldens.
fn extract_and_map(source: &str, language: Language, file: &str) -> Vec<GoldenComment> {
    let path = Path::new(file);
    let extracted = extract_source(source, language, path, file).expect("grammar parses");
    let mut comments = coalesce(source, extracted);
    map_comments(source, language, path, &mut comments).expect("mapping succeeds");
    comments
        .iter()
        .map(|c| GoldenComment {
            kind: c.kind.as_str().to_owned(),
            bound_symbol: c.bound_symbol.as_ref().map(|s| s.as_str().to_owned()),
            start_line: c.range.start_line,
            end_line: c.range.end_line,
            text: c.raw_text.clone(),
        })
        .collect()
}

#[test]
fn golden_python_extraction() {
    let source = "\"\"\"Module docstring.\"\"\"\n\
                  import os\n\n\
                  # a leading note\n\
                  def parse(data):\n\
                  \x20   \"\"\"Parse the data.\"\"\"\n\
                  \x20   # inline TODO: validate\n\
                  \x20   return data\n\n\
                  class Cache:\n\
                  \x20   # cache size note\n\
                  \x20   size = 100\n";
    insta::assert_debug_snapshot!(extract_and_map(source, Language::Python, "m.py"));
}

#[test]
fn golden_javascript_extraction() {
    let source = "// top-level note\n\
                  function add(a, b) {\n\
                  \x20 // adds two numbers\n\
                  \x20 return a + b;\n\
                  }\n\n\
                  /* block comment\n\
                  \x20  spanning lines */\n\
                  const x = 1;\n";
    insta::assert_debug_snapshot!(extract_and_map(source, Language::JavaScript, "m.js"));
}

#[test]
fn golden_typescript_extraction() {
    let source = "/** JSDoc for greet. */\n\
                  export function greet(name: string): string {\n\
                  \x20 // build the greeting\n\
                  \x20 return `hi ${name}`;\n\
                  }\n";
    insta::assert_debug_snapshot!(extract_and_map(source, Language::TypeScript, "m.ts"));
}

#[test]
fn golden_shell_extraction() {
    let source = "#!/bin/sh\n\
                  # configure the run\n\
                  set -e\n\n\
                  # greet the user\n\
                  echo hello\n";
    insta::assert_debug_snapshot!(extract_and_map(source, Language::Shell, "m.sh"));
}
