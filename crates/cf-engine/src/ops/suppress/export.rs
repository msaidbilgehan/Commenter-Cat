//! Suppression export (Idea §5; task 7.7).
//!
//! `cf suppressions export` is the **opt-in, one-way inverse of filter-up**: it
//! materializes the suppression set into each tool's native directives
//! (`# noqa: D417`, `// eslint-disable-next-line`, `# shellcheck disable=…`) for
//! teams that also run the tools directly. It is written **through the
//! parse-invariant applier** so source edits stay safe. The default flow never
//! touches source.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cf_core::comment::Comment;
use cf_core::error::{CfError, CfResult};
use cf_core::finding::Origin;
use cf_core::lang::Language;

use super::SuppressedFinding;
use crate::ops::apply::parse_invariant;

/// The native suppression directive text for a single `(origin, rule)`, or `None`
/// for origins with no native directive (Idea §5). Export uses the multi-rule
/// [`merged_directive`]; this single-rule shape is the stable public API.
#[must_use]
pub fn native_directive(origin: &Origin, rule: &str) -> Option<String> {
    directive_marker(origin).map(|_| merged_directive(origin, &[rule.to_owned()]))
}

/// The stable prefix of an origin's native directive, or `None` for origins with
/// none (Idea §5). Used for idempotency: a line already carrying the marker is
/// left alone, so re-export never stacks or clobbers a hand-written directive.
fn directive_marker(origin: &Origin) -> Option<&'static str> {
    match origin {
        Origin::Ruff => Some("# noqa"),
        Origin::Eslint => Some("eslint-disable-next-line"),
        Origin::Shellcheck => Some("# shellcheck disable"),
        Origin::Gitleaks => Some("# gitleaks:allow"),
        // Native CF findings and third-party providers have no native directive.
        Origin::Native | Origin::Other(_) => None,
    }
}

/// The native directive suppressing `rules` for `origin`, joined with the tool's
/// own multi-rule separator (Idea §5) — so several findings on one line collapse
/// into one directive (`# noqa: D400, D415`) the tool actually honors. `rules` is
/// caller-deduplicated and sorted.
fn merged_directive(origin: &Origin, rules: &[String]) -> String {
    match origin {
        Origin::Ruff => format!("# noqa: {}", rules.join(", ")),
        Origin::Eslint => format!("// eslint-disable-next-line {}", rules.join(", ")),
        Origin::Shellcheck => format!("# shellcheck disable={}", rules.join(",")),
        Origin::Gitleaks => "# gitleaks:allow".to_owned(),
        // Filtered out before this point (no native directive).
        Origin::Native | Origin::Other(_) => String::new(),
    }
}

/// Materializes `directive` into `source` at `byte_pos`, through the
/// parse-invariant applier (safe writes).
///
/// # Errors
/// Returns [`cf_core::CfError`] if the insertion would alter code.
pub fn export_directive(
    source: &str,
    language: Language,
    path: &Path,
    byte_pos: usize,
    directive: &str,
) -> CfResult<String> {
    parse_invariant::insert_comment(source, language, path, byte_pos, directive)
}

/// Where a tool's native directive sits relative to the finding's line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// Appended to the end of the finding's line (`# noqa`, `# gitleaks:allow`).
    Trailing,
    /// Inserted on the line directly above (`eslint-disable-next-line`,
    /// `# shellcheck disable=…`).
    LineAbove,
}

/// The placement convention for an origin's native directive (Idea §5).
fn placement(origin: &Origin) -> Placement {
    match origin {
        Origin::Eslint | Origin::Shellcheck => Placement::LineAbove,
        // Trailing covers ruff/gitleaks; the Native/Other arms never reach here
        // (they have no native directive and are filtered before placement).
        Origin::Ruff | Origin::Gitleaks | Origin::Native | Origin::Other(_) => Placement::Trailing,
    }
}

/// The outcome of `cf suppressions export` — what was written and what was
/// deliberately skipped, so the operator sees the full picture (Idea §5).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// Native directives written into source.
    pub exported: usize,
    /// Findings already carrying their native directive (idempotent skip).
    pub already_present: usize,
    /// Suppressed findings whose origin has no native directive (CF-native, etc.).
    pub no_native_directive: usize,
    /// The files modified, repo-relative and sorted.
    pub files: Vec<String>,
}

/// One planned insertion within a single file's source.
struct Insertion {
    byte_pos: usize,
    text: String,
    language: Language,
}

/// Materializes the suppression set into each tool's native directives, through
/// the parse-invariant applier (Idea §5; task 7.7). For every suppressed finding
/// whose origin has a native directive, the directive is inserted at the finding's
/// line — trailing (`# noqa`) or on the line above (`eslint-disable-next-line`) —
/// idempotently (a directive already present is left alone). CF-native findings
/// (markers, rot) have no native directive and are counted but skipped.
///
/// Writes are atomic per file: every insertion is computed against the file's
/// original source, then applied high byte → low (so earlier offsets stay valid)
/// in memory; the file is written only once all insertions for it succeed. A
/// parse-invariance violation aborts that file with the source untouched on disk.
///
/// # Errors
/// Returns [`cf_core::CfError`] on a file read/write failure or if an insertion
/// would alter code (parse-invariance).
pub fn export_suppressions(
    root: &Path,
    comments: &[Comment],
    suppressed: &[SuppressedFinding],
) -> CfResult<ExportReport> {
    let mut report = ExportReport::default();

    // Group the suppressed findings by their comment's file (deterministic order).
    let mut by_file: BTreeMap<&str, Vec<&SuppressedFinding>> = BTreeMap::new();
    for item in suppressed {
        let comment = &comments[item.comment_index];
        by_file.entry(comment.path.as_str()).or_default().push(item);
    }

    for (path, group) in by_file {
        let absolute = root.join(path);
        let mut source = std::fs::read_to_string(&absolute).map_err(|e| {
            CfError::storage(format!("reading {}", absolute.display())).caused_by(e)
        })?;

        // Collect, per (line, origin), the set of rules to suppress, so several
        // findings on one line collapse into one directive the tool honors.
        let mut grouped: BTreeMap<(usize, Origin), (Language, BTreeSet<String>)> = BTreeMap::new();
        for item in group {
            let comment = &comments[item.comment_index];
            let finding = &comment.findings[item.finding_index];
            if directive_marker(&finding.origin).is_none() {
                report.no_native_directive += 1;
                continue;
            }
            let byte = (finding.range.start_byte as usize).min(source.len());
            let line_start = start_of_line_byte(&source, byte);
            grouped
                .entry((line_start, finding.origin.clone()))
                .or_insert_with(|| (comment.language, BTreeSet::new()))
                .1
                .insert(finding.canonical_rule_id.clone());
        }

        // Plan one insertion per group, against the original source.
        let mut inserts: Vec<Insertion> = Vec::new();
        for ((line_start, origin), (language, rules)) in grouped {
            // Known `Some` — only origins with a marker reach the group map.
            let Some(marker) = directive_marker(&origin) else {
                continue;
            };
            let rules: Vec<String> = rules.into_iter().collect();
            let directive = merged_directive(&origin, &rules);
            match placement(&origin) {
                Placement::Trailing => {
                    if line_has_marker(&source, line_start, marker) {
                        report.already_present += 1;
                        continue;
                    }
                    inserts.push(Insertion {
                        byte_pos: end_of_line_byte(&source, line_start),
                        text: format!("  {directive}"),
                        language,
                    });
                }
                Placement::LineAbove => {
                    if line_above_has_marker(&source, line_start, marker) {
                        report.already_present += 1;
                        continue;
                    }
                    let indent = line_indent(&source, line_start);
                    inserts.push(Insertion {
                        byte_pos: line_start,
                        text: format!("{indent}{directive}\n"),
                        language,
                    });
                }
            }
        }

        if inserts.is_empty() {
            continue;
        }
        // Apply high byte → low so each insertion leaves lower offsets valid.
        inserts.sort_by_key(|insert| std::cmp::Reverse(insert.byte_pos));
        for insert in &inserts {
            source = export_directive(
                &source,
                insert.language,
                &absolute,
                insert.byte_pos,
                &insert.text,
            )?;
            report.exported += 1;
        }
        std::fs::write(&absolute, &source).map_err(|e| {
            CfError::storage(format!("writing {}", absolute.display())).caused_by(e)
        })?;
        report.files.push(path.to_owned());
    }

    Ok(report)
}

/// The byte offset of the newline ending `byte`'s line (or EOF).
fn end_of_line_byte(source: &str, byte: usize) -> usize {
    source[byte..]
        .find('\n')
        .map_or(source.len(), |off| byte + off)
}

/// The byte offset of the first character of `byte`'s line.
fn start_of_line_byte(source: &str, byte: usize) -> usize {
    source[..byte].rfind('\n').map_or(0, |nl| nl + 1)
}

/// The leading whitespace (indent) of the line beginning at `line_start`.
fn line_indent(source: &str, line_start: usize) -> String {
    source[line_start..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// Whether the line starting at `line_start` already carries `marker`
/// (trailing-idempotency).
fn line_has_marker(source: &str, line_start: usize, marker: &str) -> bool {
    let end = end_of_line_byte(source, line_start);
    source[line_start..end].contains(marker)
}

/// Whether the line directly above the one at `line_start` carries `marker`
/// (line-above-idempotency).
fn line_above_has_marker(source: &str, line_start: usize, marker: &str) -> bool {
    if line_start == 0 {
        return false;
    }
    let above_end = line_start - 1;
    let above_start = start_of_line_byte(source, above_end);
    source[above_start..above_end].contains(marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_core::finding::{Category, Finding, FindingTarget, Fix, Range};
    use cf_core::kind::CommentKind;
    use cf_core::severity::Severity;
    use cf_core::symbol::CommentId;

    /// A finding at `byte` for `origin`/`rule` — only origin, rule, and start
    /// byte matter to export; the rest is filler.
    fn finding_at(origin: Origin, rule: &str, byte: u32) -> Finding {
        Finding {
            file: "x".to_owned(),
            target: FindingTarget::Comment(CommentId::new("c")),
            range: Range::new(byte, byte + 1, 1, 1),
            origin,
            provider_rule_id: format!("p:{rule}"),
            canonical_rule_id: rule.to_owned(),
            category: Category::DocDrift,
            severity: Severity::Warning,
            severity_native: None,
            message: "m".to_owned(),
            fix: Fix::None,
            url: None,
            also_from: Default::default(),
        }
    }

    /// A comment carrying one finding, at `path`/`lang`.
    fn comment_with(path: &str, lang: Language, byte: u32, finding: Finding) -> Comment {
        let mut comment = Comment::new(
            path,
            "h",
            lang,
            CommentKind::Line,
            Range::new(byte, byte + 1, 1, 1),
            "# c",
        );
        comment.findings = vec![finding];
        comment
    }

    fn suppressed_first() -> Vec<SuppressedFinding> {
        vec![SuppressedFinding {
            comment_index: 0,
            finding_index: 0,
            suppressed_by: "cf:disable".to_owned(),
        }]
    }

    #[test]
    fn test_export_writes_trailing_noqa_and_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = "def f():\n    return 1  # bad\n";
        std::fs::write(dir.path().join("a.py"), source).unwrap();
        let byte = source.find("return").unwrap() as u32;
        let comments = vec![comment_with(
            "a.py",
            Language::Python,
            byte,
            finding_at(Origin::Ruff, "D417", byte),
        )];
        let suppressed = suppressed_first();

        let report = export_suppressions(dir.path(), &comments, &suppressed).unwrap();
        assert_eq!(report.exported, 1);
        assert_eq!(report.files, vec!["a.py".to_owned()]);
        let written = std::fs::read_to_string(dir.path().join("a.py")).unwrap();
        assert!(written.contains("# noqa: D417"), "{written}");

        // Re-export: the directive is already present → idempotent skip, no rewrite.
        let again = export_suppressions(dir.path(), &comments, &suppressed).unwrap();
        assert_eq!(again.exported, 0);
        assert_eq!(again.already_present, 1);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
            written
        );
    }

    #[test]
    fn test_same_line_findings_merge_into_one_noqa() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = "def f():\n    return 1  # bad\n";
        std::fs::write(dir.path().join("a.py"), source).unwrap();
        let byte = source.find("return").unwrap() as u32;
        let mut comment = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            Range::new(byte, byte + 1, 2, 2),
            "# bad",
        );
        comment.findings = vec![
            finding_at(Origin::Ruff, "D415", byte),
            finding_at(Origin::Ruff, "D400", byte),
        ];
        let comments = vec![comment];
        let suppressed = vec![
            SuppressedFinding {
                comment_index: 0,
                finding_index: 0,
                suppressed_by: "cf:disable".to_owned(),
            },
            SuppressedFinding {
                comment_index: 0,
                finding_index: 1,
                suppressed_by: "cf:disable".to_owned(),
            },
        ];

        let report = export_suppressions(dir.path(), &comments, &suppressed).unwrap();
        assert_eq!(
            report.exported, 1,
            "two findings on one line → one directive"
        );
        let written = std::fs::read_to_string(dir.path().join("a.py")).unwrap();
        // Sorted, comma-joined, exactly one `# noqa` ruff actually honors.
        assert!(written.contains("# noqa: D400, D415"), "{written}");
        assert_eq!(
            written.matches("# noqa").count(),
            1,
            "exactly one noqa: {written}"
        );
    }

    #[test]
    fn test_native_finding_has_no_native_directive() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = "# TODO x\nx = 1\n";
        std::fs::write(dir.path().join("a.py"), source).unwrap();
        let comments = vec![comment_with(
            "a.py",
            Language::Python,
            0,
            finding_at(Origin::Native, "marker:TODO", 0),
        )];
        let report = export_suppressions(dir.path(), &comments, &suppressed_first()).unwrap();
        assert_eq!(report.exported, 0);
        assert_eq!(report.no_native_directive, 1);
        // A native finding has nothing to export → source is untouched.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
            source
        );
    }

    #[test]
    fn test_export_writes_line_above_for_shellcheck() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = "echo $foo\n";
        std::fs::write(dir.path().join("s.sh"), source).unwrap();
        let comments = vec![comment_with(
            "s.sh",
            Language::Shell,
            0,
            finding_at(Origin::Shellcheck, "SC2086", 0),
        )];
        let report = export_suppressions(dir.path(), &comments, &suppressed_first()).unwrap();
        assert_eq!(report.exported, 1);
        let written = std::fs::read_to_string(dir.path().join("s.sh")).unwrap();
        assert_eq!(
            written, "# shellcheck disable=SC2086\necho $foo\n",
            "{written}"
        );
    }

    #[test]
    fn test_per_tool_native_directives() {
        assert_eq!(
            native_directive(&Origin::Ruff, "D417").as_deref(),
            Some("# noqa: D417")
        );
        assert_eq!(
            native_directive(&Origin::Eslint, "jsdoc/check-param-names").as_deref(),
            Some("// eslint-disable-next-line jsdoc/check-param-names")
        );
        assert_eq!(
            native_directive(&Origin::Shellcheck, "SC2086").as_deref(),
            Some("# shellcheck disable=SC2086")
        );
        assert_eq!(native_directive(&Origin::Native, "rot").as_deref(), None);
    }

    #[test]
    fn test_export_inserts_directive_via_parse_invariance() {
        // Append `  # noqa: D417` to the end of the statement line.
        let source = "x = 1\n";
        let directive = native_directive(&Origin::Ruff, "D417").unwrap();
        let exported = export_directive(
            source,
            Language::Python,
            Path::new("a.py"),
            5,
            &format!("  {directive}"),
        )
        .unwrap();
        assert_eq!(exported, "x = 1  # noqa: D417\n");
    }

    #[test]
    fn test_export_refuses_to_inject_code() {
        // Inserting code (not a comment) is rejected by parse-invariance.
        let source = "x = 1\n";
        assert!(
            export_directive(source, Language::Python, Path::new("a.py"), 5, "; evil()").is_err()
        );
    }
}
