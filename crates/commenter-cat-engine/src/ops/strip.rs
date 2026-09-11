//! `commenter-cat strip` — scan the tree and remove every comment (Idea §4a, §5).
//!
//! The bulk counterpart to the single-comment [`super::apply`] path: one walk
//! over the `[scan]` universe, every comment extracted and coalesced, then the
//! file spliced and proven **code-invariant by the same leaf-token comparison**
//! the applier uses — so a sweep that touches thousands of comments carries the
//! identical guarantee as one `remove`. A file is written only once its whole
//! batch verifies.
//!
//! Verification is per *file*, not per comment, because per-comment re-parsing
//! is quadratic exactly where a sweep lands hardest: a 1200-line module with 600
//! comments cost ~1200 parses (≈2s) before [`batch_remove`] existed. When a batch
//! does not verify it is only known to be unsafe *somewhere*, so
//! [`remove_one_by_one`] re-runs it through the single-comment applier to keep
//! what is safe and name what is not.
//!
//! Three properties make this safe to point at a whole repository:
//!
//! * **Dry-run by default.** Like `issues sync`, the destructive path is opt-in
//!   (`apply`); the default run computes the complete plan and touches nothing.
//! * **Protected by kind.** Behavior-bearing comments (directive / shebang /
//!   encoding-decl — read by the type-checker, the linters, or the OS) and
//!   license headers survive unless explicitly surrendered. Everything protected
//!   is *counted*, never silently dropped from the picture.
//! * **Per-file degradation.** An unreadable file, a grammar failure, or a
//!   comment the applier refuses (a symbol's only docstring, whose removal would
//!   leave an `IndentationError`) is a recorded skip — visible, never silent
//!   (Idea §5) — so one bad file never aborts the sweep. Only a mid-pass file
//!   **write** failure is fatal.
//!
//! Stripping leaves the persisted index describing comments that no longer
//! exist; [`StripReport::reindex_required`] says so, and `commenter-cat check`
//! re-derives it.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::config::ResolvedConfig;
use commenter_cat_core::error::{cause_chain, CommenterCatError, CommenterCatResult};
use commenter_cat_core::kind::CommentKind;
use commenter_cat_core::lang::Language;

use crate::extract::coalesce::coalesce;
use crate::extract::extract_source;
use crate::ops::apply;
use crate::ops::apply::parse_invariant;
use crate::walk::{to_repo_relative, walk, WalkOptions};

/// Which comments a strip pass may delete, and how tidy it leaves the result.
///
/// The three "surrender" switches are each monotone — every one of them removes
/// protection — while `keep_kinds` only ever adds it, so a policy reads
/// unambiguously however the flags are combined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripPolicy {
    /// Also strip behavior-bearing kinds (directive / shebang / encoding-decl).
    pub allow_significant: bool,
    /// Also strip license / copyright headers.
    pub strip_license: bool,
    /// Additional kinds to preserve, on top of the protected defaults.
    pub keep_kinds: BTreeSet<CommentKind>,
    /// Collapse the blank line (or trailing whitespace) a removal leaves behind.
    pub tidy: bool,
}

impl Default for StripPolicy {
    fn default() -> Self {
        Self {
            allow_significant: false,
            strip_license: false,
            keep_kinds: BTreeSet::new(),
            tidy: true,
        }
    }
}

/// Why a comment survived the pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protection {
    /// Named by `keep_kinds`.
    ByKind,
    /// A license / copyright header.
    License,
    /// Behavior-bearing: directive, shebang, or encoding-decl.
    Significant,
}

impl StripPolicy {
    /// Why `kind` is protected under this policy, or `None` if it may be
    /// stripped.
    fn protection(&self, kind: CommentKind) -> Option<Protection> {
        if self.keep_kinds.contains(&kind) {
            return Some(Protection::ByKind);
        }
        if kind == CommentKind::License && !self.strip_license {
            return Some(Protection::License);
        }
        if kind.is_behavior_bearing() && !self.allow_significant {
            return Some(Protection::Significant);
        }
        None
    }
}

/// One file's contribution to the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrippedFile {
    /// Repo-relative path, `/`-normalized.
    pub path: String,
    /// Comments removed (or, in a dry run, that would be removed).
    pub removed: usize,
    /// The start line of each removal, ascending.
    pub lines: Vec<u32>,
}

/// What a strip pass did, or — in a dry run — would do.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StripReport {
    /// Whether this was a plan (nothing written).
    pub dry_run: bool,
    /// Files the walk selected.
    pub files_scanned: usize,
    /// Comments extracted across those files.
    pub comments_scanned: usize,
    /// Comments removed (would be removed), across all files.
    pub removed: usize,
    /// Behavior-bearing comments preserved (no `allow_significant`).
    pub kept_significant: usize,
    /// License headers preserved (no `strip_license`).
    pub kept_license: usize,
    /// Comments preserved by an explicit `keep_kinds` entry.
    pub kept_by_kind: usize,
    /// Per-file detail, in walk (sorted-path) order; only files with removals.
    pub files: Vec<StrippedFile>,
    /// Non-fatal problems: an unreadable file, a grammar failure, or a comment
    /// the applier refused. Surfaced so a degraded sweep is visible.
    pub skipped: Vec<String>,
}

impl StripReport {
    /// Whether the persisted index now describes comments that are gone — true
    /// once a non-dry run actually removed something. `commenter-cat check`
    /// re-derives it.
    #[must_use]
    pub fn reindex_required(&self) -> bool {
        !self.dry_run && self.removed > 0
    }

    /// Total comments preserved by protection.
    #[must_use]
    pub fn kept(&self) -> usize {
        self.kept_significant + self.kept_license + self.kept_by_kind
    }
}

/// Scans `root` and strips every comment the policy permits.
///
/// With `dry_run` the full plan is computed — including which comments the
/// applier would refuse — but no file is touched.
///
/// # Errors
/// Returns [`CommenterCatError`] if the walk fails or a file cannot be written
/// mid-pass. Per-file read / parse / apply trouble degrades to
/// [`StripReport::skipped`].
pub fn strip(
    root: &Path,
    config: &ResolvedConfig,
    policy: &StripPolicy,
    dry_run: bool,
) -> CommenterCatResult<StripReport> {
    let walked = walk(root, &WalkOptions::from_scan_config(&config.scan))?;
    let mut report = StripReport {
        dry_run,
        files_scanned: walked.len(),
        ..StripReport::default()
    };

    for file in &walked {
        let repo_path = to_repo_relative(&file.path, root);
        let source = match std::fs::read_to_string(&file.path) {
            Ok(text) => text,
            Err(e) => {
                report
                    .skipped
                    .push(format!("{repo_path}: read failed: {e}"));
                continue;
            }
        };
        let extracted = match extract_source(&source, file.language, &file.path, &repo_path) {
            Ok(comments) => coalesce(&source, comments),
            Err(e) => {
                report
                    .skipped
                    .push(format!("{repo_path}: {}", cause_chain(&e)));
                continue;
            }
        };
        report.comments_scanned += extracted.len();

        // Partition by protection first, so the counts describe the whole tree
        // even for files with nothing to remove.
        let mut targets = Vec::new();
        for comment in &extracted {
            match policy.protection(comment.kind) {
                Some(Protection::ByKind) => report.kept_by_kind += 1,
                Some(Protection::License) => report.kept_license += 1,
                Some(Protection::Significant) => report.kept_significant += 1,
                None => targets.push(comment),
            }
        }
        if targets.is_empty() {
            continue;
        }

        // Take the whole file in one splice and verify it once. Deleting comments
        // one at a time re-parses the file twice *per comment*, which is quadratic
        // where it hurts most — a 1200-line module with 600 comments cost ~1200
        // parses (≈2s) before this path existed. Anything the batch cannot prove
        // safe falls through to the per-comment path, which isolates the offender.
        let mut batch = match batch_remove(&source, &targets, file.language, &file.path) {
            Ok(Some(batch)) => batch,
            Ok(None) | Err(_) => {
                remove_one_by_one(&source, &targets, policy, &repo_path, &mut report.skipped)
            }
        };
        if batch.lines.is_empty() {
            continue;
        }
        if policy.tidy {
            batch.stripped = tidy(&batch.stripped, &batch.points, file.language, &file.path);
        }

        if !dry_run {
            std::fs::write(&file.path, &batch.stripped).map_err(|e| {
                CommenterCatError::storage(format!("writing {}", file.path.display())).caused_by(e)
            })?;
        }
        batch.lines.sort_unstable();
        report.removed += batch.lines.len();
        report.files.push(StrippedFile {
            path: repo_path,
            removed: batch.lines.len(),
            lines: batch.lines,
        });
    }

    Ok(report)
}

/// One file's stripped text and the sites the removals left behind.
struct Batch {
    /// The file with every removal spliced out.
    stripped: String,
    /// The start line of each removal.
    lines: Vec<u32>,
    /// Each removal's byte offset **in `stripped`**, for the blank-line tidy.
    points: Vec<usize>,
}

/// Splices every target out of `source` in one forward pass and verifies the
/// whole file once, returning `None` when the result is not code-invariant.
///
/// `targets` is in ascending source order (extraction sorts, coalescing keeps
/// the order and guarantees no overlap), so rebuilding the file forward yields
/// each site's final offset for free. A `None` is not a verdict on any
/// particular comment — it only says the batch is unsafe *somewhere*; the caller
/// then removes them one at a time so the report names the offender.
fn batch_remove(
    source: &str,
    targets: &[&Comment],
    language: Language,
    path: &Path,
) -> CommenterCatResult<Option<Batch>> {
    let mut batch = Batch {
        stripped: String::with_capacity(source.len()),
        lines: Vec::with_capacity(targets.len()),
        points: Vec::with_capacity(targets.len()),
    };
    // Python docstrings are `string` nodes, so the parser reads them as code:
    // their ranges must leave the "before" signature by hand, exactly as the
    // single-comment applier excludes the one it is editing.
    let mut code_spans = Vec::new();
    let mut cursor = 0usize;

    for comment in targets {
        let start = comment.range.start_byte as usize;
        let end = comment.range.end_byte as usize;
        if start < cursor
            || end > source.len()
            || start > end
            || !source.is_char_boundary(start)
            || !source.is_char_boundary(end)
        {
            return Ok(None);
        }
        batch.stripped.push_str(&source[cursor..start]);
        batch.points.push(batch.stripped.len());
        batch.lines.push(comment.range.start_line);
        if parse_invariant::is_code_node(comment) {
            code_spans.push((start, end));
        }
        cursor = end;
    }
    batch.stripped.push_str(&source[cursor..]);

    if parse_invariant::code_unchanged(source, &batch.stripped, &code_spans, language, path)? {
        Ok(Some(batch))
    } else {
        Ok(None)
    }
}

/// Removes the targets one at a time through the single-comment applier, keeping
/// what it accepts and recording what it refuses — the isolating slow path the
/// batch falls back to, and the only one that can name an unsafe comment.
///
/// High byte → low, so each removal leaves every lower offset — and so every
/// remaining comment's recorded range — valid.
fn remove_one_by_one(
    source: &str,
    targets: &[&Comment],
    policy: &StripPolicy,
    repo_path: &str,
    skipped: &mut Vec<String>,
) -> Batch {
    let mut batch = Batch {
        stripped: source.to_owned(),
        lines: Vec::new(),
        points: Vec::new(),
    };
    for comment in targets.iter().rev() {
        match apply::remove(&batch.stripped, comment, policy.allow_significant) {
            Ok(applied) => {
                batch.stripped = applied.new_source;
                batch.lines.push(comment.range.start_line);
                // Every point recorded so far sits above this removal, so it
                // slides down by the span that just collapsed; this one lands
                // exactly at the comment's start.
                let span = (comment.range.end_byte - comment.range.start_byte) as usize;
                for point in &mut batch.points {
                    *point = point.saturating_sub(span);
                }
                batch.points.push(comment.range.start_byte as usize);
            }
            Err(e) => skipped.push(format!(
                "{repo_path}:{}: {}",
                comment.range.start_line,
                cause_chain(&e)
            )),
        }
    }
    batch
}

/// Collapses the whitespace a removal leaves behind: a line left blank by a
/// removal is deleted outright, and a line that merely lost its trailing comment
/// loses its trailing whitespace too.
///
/// `points` are the removal sites as byte offsets **in `source`** (the stripped
/// text), each one where a comment's span collapsed to zero length. Only those
/// lines are considered; the rest of the file is reproduced byte for byte.
///
/// The rewrite is then re-checked against the same leaf-token stream the applier
/// compares, and discarded wholesale if it differs — a blank line inside a
/// string, say, is string *content*, and touching it would change code. On a
/// parse failure the untidied source is returned unchanged.
fn tidy(source: &str, points: &[usize], language: Language, path: &Path) -> String {
    let starts = line_starts(source);
    let touched: HashSet<usize> = points
        .iter()
        .map(|point| line_index(&starts, (*point).min(source.len())))
        .collect();

    let mut out = String::with_capacity(source.len());
    let mut changed = false;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        if !touched.contains(&index) {
            out.push_str(line);
            continue;
        }
        let (body, newline) = match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        };
        let trimmed = body.trim_end();
        if trimmed.is_empty() {
            // The comment was the whole line — drop the line, newline included.
            changed = true;
            continue;
        }
        if trimmed.len() != body.len() {
            changed = true;
        }
        out.push_str(trimmed);
        out.push_str(newline);
    }

    if !changed {
        return source.to_owned();
    }
    match parse_invariant::code_unchanged(source, &out, &[], language, path) {
        Ok(true) => out,
        Ok(false) | Err(_) => source.to_owned(),
    }
}

/// The byte offset at which each line of `source` begins.
fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        source
            .match_indices('\n')
            .map(|(index, _)| index + 1)
            .filter(|start| *start < source.len()),
    );
    starts
}

/// The zero-based index of the line containing `byte`.
fn line_index(starts: &[usize], byte: usize) -> usize {
    starts
        .partition_point(|start| *start <= byte)
        .saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use commenter_cat_core::config::ResolvedConfig;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A throwaway tree plus the resolved default config every test strips with.
    struct Tree {
        dir: TempDir,
        config: ResolvedConfig,
    }

    impl Tree {
        fn new() -> Self {
            Self {
                dir: TempDir::new().unwrap(),
                config: ResolvedConfig::default(),
            }
        }

        fn write(&self, name: &str, source: &str) -> PathBuf {
            let path = self.dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, source).unwrap();
            path
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.dir.path().join(name)).unwrap()
        }

        fn strip(&self, policy: &StripPolicy, dry_run: bool) -> StripReport {
            super::strip(self.dir.path(), &self.config, policy, dry_run).unwrap()
        }
    }

    #[test]
    fn test_strips_comments_and_leaves_code_intact() {
        let tree = Tree::new();
        tree.write(
            "m.py",
            "# a leading note\nx = 1  # a trailing note\n\n\ndef f():\n    # inside\n    return x\n",
        );

        let report = tree.strip(&StripPolicy::default(), false);

        assert_eq!(report.removed, 3);
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].path, "m.py");
        assert_eq!(report.files[0].lines, vec![1, 2, 6]);
        // Every comment gone, every statement kept, no litter left behind.
        assert_eq!(tree.read("m.py"), "x = 1\n\n\ndef f():\n    return x\n");
    }

    #[test]
    fn test_dry_run_plans_without_touching_the_file() {
        let tree = Tree::new();
        let source = "# note\nx = 1\n";
        tree.write("m.py", source);

        let report = tree.strip(&StripPolicy::default(), true);

        assert!(report.dry_run);
        assert_eq!(report.removed, 1);
        assert_eq!(report.files[0].lines, vec![1]);
        assert!(!report.reindex_required(), "a plan leaves the index valid");
        assert_eq!(tree.read("m.py"), source, "the file is byte-identical");
    }

    #[test]
    fn test_behavior_bearing_comments_survive_by_default() {
        let tree = Tree::new();
        let source = "#!/usr/bin/env python3\n# -*- coding: utf-8 -*-\nx = 1  # type: ignore\n# prose\ny = 2\n";
        tree.write("m.py", source);

        let report = tree.strip(&StripPolicy::default(), false);

        assert_eq!(report.removed, 1, "only the prose comment");
        assert_eq!(report.kept_significant, 3);
        let written = tree.read("m.py");
        assert!(written.contains("#!/usr/bin/env python3"), "{written}");
        assert!(written.contains("coding: utf-8"), "{written}");
        assert!(written.contains("type: ignore"), "{written}");
        assert!(!written.contains("# prose"), "{written}");
    }

    #[test]
    fn test_allow_significant_surrenders_the_protected_kinds() {
        let tree = Tree::new();
        tree.write("m.py", "#!/usr/bin/env python3\nx = 1  # type: ignore\n");

        let policy = StripPolicy {
            allow_significant: true,
            ..StripPolicy::default()
        };
        let report = tree.strip(&policy, false);

        assert_eq!(report.removed, 2);
        assert_eq!(report.kept_significant, 0);
        assert_eq!(tree.read("m.py"), "x = 1\n");
    }

    #[test]
    fn test_license_header_survives_until_surrendered() {
        let tree = Tree::new();
        let source =
            "# Copyright 2026 Example Inc.\n# SPDX-License-Identifier: MIT\n# prose\nx = 1\n";
        tree.write("m.py", source);

        let kept = tree.strip(&StripPolicy::default(), true);
        // The copyright line and the SPDX line each classify as `license`, and a
        // non-`Line` kind breaks a coalesce run — two protected comments, not one.
        assert_eq!(kept.kept_license, 2);
        assert_eq!(kept.removed, 1, "the prose comment only");

        let policy = StripPolicy {
            strip_license: true,
            ..StripPolicy::default()
        };
        let report = tree.strip(&policy, false);
        assert_eq!(report.kept_license, 0);
        assert_eq!(tree.read("m.py"), "x = 1\n");
    }

    #[test]
    fn test_keep_kinds_adds_protection() {
        let tree = Tree::new();
        tree.write(
            "m.py",
            "def f(x):\n    \"\"\"Doc.\"\"\"\n    # note\n    return x\n",
        );

        let policy = StripPolicy {
            keep_kinds: BTreeSet::from([CommentKind::Docstring]),
            ..StripPolicy::default()
        };
        let report = tree.strip(&policy, false);

        assert_eq!(report.kept_by_kind, 1);
        assert_eq!(report.removed, 1);
        assert_eq!(
            tree.read("m.py"),
            "def f(x):\n    \"\"\"Doc.\"\"\"\n    return x\n"
        );
    }

    #[test]
    fn test_python_docstrings_are_stripped() {
        let tree = Tree::new();
        tree.write(
            "m.py",
            "def f(x):\n    \"\"\"Return x.\"\"\"\n    return x\n",
        );

        let report = tree.strip(&StripPolicy::default(), false);

        assert_eq!(report.removed, 1);
        assert_eq!(tree.read("m.py"), "def f(x):\n    return x\n");
    }

    #[test]
    fn test_a_symbols_only_docstring_is_a_visible_skip() {
        let tree = Tree::new();
        let source = "def f(x):\n    \"\"\"The whole body.\"\"\"\n";
        tree.write("m.py", source);

        let report = tree.strip(&StripPolicy::default(), false);

        // Removing it would leave an IndentationError, so the applier refuses and
        // the pass records why instead of writing broken source.
        assert_eq!(report.removed, 0);
        assert_eq!(report.skipped.len(), 1);
        assert!(
            report.skipped[0].starts_with("m.py:2:"),
            "{:?}",
            report.skipped
        );
        assert_eq!(tree.read("m.py"), source);
    }

    #[test]
    fn test_an_unsafe_comment_does_not_cost_its_neighbours() {
        let tree = Tree::new();
        // `bare`'s docstring is its whole body and cannot go; everything else in
        // the file can. The batch fails on the pair, so the slow path isolates it.
        tree.write(
            "m.py",
            "# note\ndef total(x):\n    \"\"\"Doc.\"\"\"\n    return x\n\n\ndef bare():\n    \"\"\"Whole body.\"\"\"\n",
        );

        let report = tree.strip(&StripPolicy::default(), false);

        assert_eq!(report.removed, 2, "the note and the safe docstring");
        assert_eq!(report.skipped.len(), 1, "only the unsafe one");
        assert_eq!(
            tree.read("m.py"),
            "def total(x):\n    return x\n\n\ndef bare():\n    \"\"\"Whole body.\"\"\"\n"
        );
    }

    #[test]
    fn test_no_tidy_leaves_the_blank_line() {
        let tree = Tree::new();
        tree.write("m.py", "# note\nx = 1  # trailing\n");

        let policy = StripPolicy {
            tidy: false,
            ..StripPolicy::default()
        };
        tree.strip(&policy, false);

        assert_eq!(tree.read("m.py"), "\nx = 1  \n");
    }

    #[test]
    fn test_tidy_never_touches_string_content() {
        let tree = Tree::new();
        // The blank line inside the triple-quoted string is string *content*; the
        // comment above it is what gets removed.
        let source = "# note\nbanner = \"\"\"top\n   \nbottom\"\"\"\n";
        tree.write("m.py", source);

        tree.strip(&StripPolicy::default(), false);

        assert_eq!(tree.read("m.py"), "banner = \"\"\"top\n   \nbottom\"\"\"\n");
    }

    #[test]
    fn test_multi_language_sweep() {
        let tree = Tree::new();
        tree.write("a.py", "# py note\nx = 1\n");
        tree.write("b.ts", "// ts note\nconst x = 1;\n");
        tree.write("c.js", "/* js note */\nconst y = 2;\n");
        tree.write("d.sh", "#!/bin/sh\n# sh note\necho hi\n");

        let report = tree.strip(&StripPolicy::default(), false);

        assert_eq!(report.removed, 4, "one per file; d.sh keeps its shebang");
        assert_eq!(report.files.len(), 4);
        assert_eq!(tree.read("a.py"), "x = 1\n");
        assert_eq!(tree.read("b.ts"), "const x = 1;\n");
        assert_eq!(tree.read("c.js"), "const y = 2;\n");
        assert_eq!(tree.read("d.sh"), "#!/bin/sh\necho hi\n");
    }

    #[test]
    fn test_coalesced_block_is_one_removal() {
        let tree = Tree::new();
        tree.write("m.py", "# line one\n# line two\n# line three\nx = 1\n");

        let report = tree.strip(&StripPolicy::default(), false);

        // Adjacent own-line comments coalesce into one logical block (Idea §3),
        // so the run is a single removal, not three.
        assert_eq!(report.removed, 1);
        assert_eq!(report.comments_scanned, 1);
        assert_eq!(tree.read("m.py"), "x = 1\n");
    }

    #[test]
    fn test_second_pass_is_a_no_op() {
        let tree = Tree::new();
        tree.write("m.py", "# note\nx = 1\n");

        tree.strip(&StripPolicy::default(), false);
        let again = tree.strip(&StripPolicy::default(), false);

        assert_eq!(again.removed, 0);
        assert!(again.files.is_empty());
        assert!(!again.reindex_required());
        assert_eq!(tree.read("m.py"), "x = 1\n");
    }

    #[test]
    fn test_reindex_required_only_after_a_real_removal() {
        let tree = Tree::new();
        tree.write("m.py", "# note\nx = 1\n");
        assert!(tree
            .strip(&StripPolicy::default(), false)
            .reindex_required());
    }
}
