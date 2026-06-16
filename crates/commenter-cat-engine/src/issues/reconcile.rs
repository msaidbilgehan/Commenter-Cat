//! Bidirectional reconciliation: resolved (closed) issues → remove their marker
//! comments (Idea §9). The batch, file-aware counterpart to
//! [`super::sync_resolution`] (which resolves a single comment): it dedups
//! comments, caches tracker lookups by issue id (a comment with several markers
//! costs one call per distinct issue), and removes every resolved comment in a
//! file in **one high-byte→low pass** so earlier byte offsets stay valid through
//! the parse-invariant applier. Tracker / read / unsafe-removal trouble degrades
//! to a recorded skip — *visible, never silent* (Idea §5) — so a sync always
//! completes; only a mid-pass file **write** failure is fatal.

use std::collections::BTreeMap;
use std::path::Path;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};

use super::{IssueBackend, IssueLedger};
use crate::ops::apply;

/// The outcome of a reconciliation pass (Idea §9).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Tokens whose comments were resolved (removed) — the caller drops these
    /// from the ledger. In a dry run, the tokens that *would* be resolved.
    pub resolved_tokens: Vec<String>,
    /// `path:line` of each resolved comment (would-be, in a dry run), sorted.
    pub removed: Vec<String>,
    /// Files modified (would be modified, in a dry run), repo-relative.
    pub files_touched: Vec<String>,
    /// Non-fatal problems (tracker unreachable, a comment unsafe to remove, a
    /// file that could not be read) — surfaced so a degraded sync is visible.
    pub skipped: Vec<String>,
}

/// A comment selected for removal, with every token that resolved it.
struct Resolved<'a> {
    comment: &'a Comment,
    tokens: Vec<String>,
}

/// Removes the marker comments of resolved (closed) issues (Idea §9).
///
/// `tracked` is every `(comment, token)` whose `token` is recorded in `ledger`.
/// The tracker is queried (read-only, cached by issue id); a comment is
/// **resolved** if *any* of its tokens maps to a closed issue. Resolved comments
/// are removed per file in one high→low pass via the parse-invariant applier,
/// each file written only once all its removals succeed. With `dry_run` the
/// tracker is still queried but no file or comment is touched — the report lists
/// what *would* be resolved.
///
/// # Errors
/// Returns [`commenter_cat_core::CommenterCatError`] only on a file **write** failure mid-pass; tracker
/// queries, unreadable files, and unsafe removals degrade to `report.skipped`.
pub fn reconcile_resolved(
    root: &Path,
    tracked: &[(&Comment, String)],
    backend: &dyn IssueBackend,
    ledger: &IssueLedger,
    dry_run: bool,
) -> CommenterCatResult<ReconcileReport> {
    let mut report = ReconcileReport::default();

    // Phase 1 (read-only): which comments are resolved? Group the tracked markers
    // by the comment they sit on (path + start byte), so a multi-marker comment is
    // judged as a whole.
    let mut by_comment: BTreeMap<(&str, u32), (&Comment, Vec<&str>)> = BTreeMap::new();
    for (comment, token) in tracked {
        by_comment
            .entry((comment.path.as_str(), comment.range.start_byte))
            .or_insert_with(|| (*comment, Vec::new()))
            .1
            .push(token.as_str());
    }

    // A comment resolves only when EVERY one of its issues is closed — removing it
    // while any concern is still open would delete live work from source. Tracker
    // lookups are cached by issue id; a query error is a visible skip (not closed).
    let mut closed_by_id: BTreeMap<String, bool> = BTreeMap::new();
    let mut per_file: BTreeMap<String, BTreeMap<u32, Resolved<'_>>> = BTreeMap::new();
    for ((path, start_byte), (comment, tokens)) in by_comment {
        // `fold`, not `all`, so every token is queried — surfacing every skip note
        // — even once we know the comment will not resolve.
        let all_closed = tokens.iter().fold(true, |acc, token| {
            let closed = issue_closed(backend, ledger, token, &mut closed_by_id, &mut report);
            acc && closed
        });
        if !all_closed {
            continue;
        }
        per_file.entry(path.to_owned()).or_default().insert(
            start_byte,
            Resolved {
                comment,
                tokens: tokens.iter().map(|token| (*token).to_owned()).collect(),
            },
        );
    }

    // Phase 2: remove the resolved comments (unless this is a dry run).
    for (path, comments) in per_file {
        if dry_run {
            for resolved in comments.values() {
                report
                    .removed
                    .push(format!("{path}:{}", resolved.comment.range.start_line));
                report
                    .resolved_tokens
                    .extend(resolved.tokens.iter().cloned());
            }
            report.files_touched.push(path);
            continue;
        }

        let absolute = root.join(&path);
        let mut source = match std::fs::read_to_string(&absolute) {
            Ok(text) => text,
            Err(e) => {
                report.skipped.push(format!("{path}: read failed: {e}"));
                continue;
            }
        };

        // Accumulate this file's results locally; commit them to the report only
        // once the write succeeds, so a failed write never tells the ledger a
        // comment resolved that is still on disk.
        let mut local_removed: Vec<String> = Vec::new();
        let mut local_tokens: Vec<String> = Vec::new();
        // High byte → low: each removal leaves every lower offset valid.
        for resolved in comments.values().rev() {
            match apply::remove(&source, resolved.comment, false) {
                Ok(applied) => {
                    source = applied.new_source;
                    local_removed.push(format!("{path}:{}", resolved.comment.range.start_line));
                    local_tokens.extend(resolved.tokens.iter().cloned());
                }
                Err(e) => report.skipped.push(format!(
                    "{path}:{} unsafe to remove: {e}",
                    resolved.comment.range.start_line
                )),
            }
        }
        if local_removed.is_empty() {
            continue;
        }
        std::fs::write(&absolute, &source).map_err(|e| {
            CommenterCatError::storage(format!("writing {}", absolute.display())).caused_by(e)
        })?;
        report.removed.extend(local_removed);
        report.resolved_tokens.extend(local_tokens);
        report.files_touched.push(path);
    }

    Ok(report)
}

/// Whether `token`'s issue is closed, caching the answer by issue id so a comment
/// with several markers on the same issue costs one tracker call. A query failure
/// is a visible skip, treated as not-closed (a comment is removed only on a
/// *positive* closed confirmation).
fn issue_closed(
    backend: &dyn IssueBackend,
    ledger: &IssueLedger,
    token: &str,
    cache: &mut BTreeMap<String, bool>,
    report: &mut ReconcileReport,
) -> bool {
    let Some(issue) = ledger.get(token) else {
        return false;
    };
    if let Some(state) = cache.get(&issue.id) {
        return *state;
    }
    let state = match backend.get(&issue.id) {
        Ok(found) => found.is_some_and(|current| current.closed),
        Err(e) => {
            report
                .skipped
                .push(format!("issue {}: tracker query failed: {e}", issue.id));
            false
        }
    };
    cache.insert(issue.id.clone(), state);
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_source;
    use crate::issues::{identity_token, IssueRef, IssueRequest};
    use commenter_cat_core::lang::Language;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::path::Path as StdPath;

    /// The mocked network seam (Idea §11): every issue is closed/open, or only the
    /// listed ids are closed. Records how many state lookups it served, to prove
    /// the id cache collapses repeats.
    struct MockTracker {
        all: Option<bool>,
        closed_ids: BTreeSet<String>,
        gets: RefCell<usize>,
    }

    impl MockTracker {
        /// Every issue closed (`true`) or open (`false`).
        fn new(closed: bool) -> Self {
            Self {
                all: Some(closed),
                closed_ids: BTreeSet::new(),
                gets: RefCell::new(0),
            }
        }
        /// Only `ids` are closed; everything else is open.
        fn with_closed(ids: &[&str]) -> Self {
            Self {
                all: None,
                closed_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
                gets: RefCell::new(0),
            }
        }
    }

    impl IssueBackend for MockTracker {
        fn create(&self, _request: &IssueRequest) -> CommenterCatResult<IssueRef> {
            unreachable!("reconcile never files")
        }
        fn get(&self, id: &str) -> CommenterCatResult<Option<IssueRef>> {
            *self.gets.borrow_mut() += 1;
            let closed = self.all.unwrap_or_else(|| self.closed_ids.contains(id));
            Ok(Some(IssueRef {
                id: id.to_owned(),
                url: format!("https://tracker/{id}"),
                closed,
            }))
        }
    }

    /// Writes `source` to `dir/name`, extracts its comments, and records each
    /// `(token, issue)` in a fresh ledger — the setup every test shares.
    fn seed(
        dir: &Path,
        name: &str,
        source: &str,
        markers: &[(&str, &str)],
    ) -> (Vec<Comment>, IssueLedger) {
        std::fs::write(dir.join(name), source).unwrap();
        let comments = extract_source(source, Language::Python, StdPath::new(name), name).unwrap();
        let mut ledger = IssueLedger::new();
        for (index, (_text, marker)) in markers.iter().enumerate() {
            let token = identity_token(&comments[index], marker);
            ledger.record(
                token,
                IssueRef {
                    id: format!("{}", index + 1),
                    url: format!("https://tracker/{}", index + 1),
                    closed: false,
                },
            );
        }
        (comments, ledger)
    }

    fn tracked_pairs<'a>(
        comments: &'a [Comment],
        markers: &[(&str, &str)],
    ) -> Vec<(&'a Comment, String)> {
        markers
            .iter()
            .enumerate()
            .map(|(index, (_text, marker))| {
                (&comments[index], identity_token(&comments[index], marker))
            })
            .collect()
    }

    #[test]
    fn test_reconcile_removes_closed_comment() {
        let dir = tempfile::TempDir::new().unwrap();
        let markers = [("TODO", "TODO")];
        let (comments, ledger) = seed(dir.path(), "a.py", "# TODO ship it\nx = 1\n", &markers);
        let tracked = tracked_pairs(&comments, &markers);

        let backend = MockTracker::new(true);
        let report = reconcile_resolved(dir.path(), &tracked, &backend, &ledger, false).unwrap();

        assert_eq!(report.removed, vec!["a.py:1".to_owned()]);
        assert_eq!(report.resolved_tokens.len(), 1);
        let written = std::fs::read_to_string(dir.path().join("a.py")).unwrap();
        assert!(!written.contains("TODO"), "{written}");
        assert!(written.contains("x = 1"), "{written}");
    }

    #[test]
    fn test_resolved_tokens_drop_from_ledger() {
        // The CLI drops every resolved token after a successful reconcile, retiring
        // the comment-to-issue link so the (now-removed) marker is not reported
        // "already tracked" next run. This proves that exact composition.
        let dir = tempfile::TempDir::new().unwrap();
        let markers = [("TODO", "TODO")];
        let (comments, mut ledger) = seed(dir.path(), "a.py", "# TODO ship it\nx = 1\n", &markers);
        let tracked = tracked_pairs(&comments, &markers);

        let backend = MockTracker::new(true);
        let report = reconcile_resolved(dir.path(), &tracked, &backend, &ledger, false).unwrap();
        assert_eq!(ledger.len(), 1);
        for token in &report.resolved_tokens {
            ledger.remove(token);
        }
        assert!(
            ledger.is_empty(),
            "the resolved link is retired from the ledger"
        );
    }

    #[test]
    fn test_reconcile_keeps_open_comment() {
        let dir = tempfile::TempDir::new().unwrap();
        let markers = [("TODO", "TODO")];
        let source = "# TODO ship it\nx = 1\n";
        let (comments, ledger) = seed(dir.path(), "a.py", source, &markers);
        let tracked = tracked_pairs(&comments, &markers);

        let backend = MockTracker::new(false);
        let report = reconcile_resolved(dir.path(), &tracked, &backend, &ledger, false).unwrap();

        assert!(report.removed.is_empty());
        assert!(report.resolved_tokens.is_empty());
        // An open issue leaves the source untouched.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
            source
        );
    }

    #[test]
    fn test_reconcile_dry_run_previews_without_mutation() {
        let dir = tempfile::TempDir::new().unwrap();
        let markers = [("TODO", "TODO")];
        let source = "# TODO ship it\nx = 1\n";
        let (comments, ledger) = seed(dir.path(), "a.py", source, &markers);
        let tracked = tracked_pairs(&comments, &markers);

        let backend = MockTracker::new(true);
        let report = reconcile_resolved(dir.path(), &tracked, &backend, &ledger, true).unwrap();

        // Previewed (would resolve) but the file is byte-for-byte unchanged.
        assert_eq!(report.removed, vec!["a.py:1".to_owned()]);
        assert_eq!(report.resolved_tokens.len(), 1);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
            source
        );
    }

    #[test]
    fn test_reconcile_two_comments_high_to_low() {
        let dir = tempfile::TempDir::new().unwrap();
        // Two distinct markers so the tokens differ; both issues closed.
        let markers = [("TODO", "TODO"), ("FIXME", "FIXME")];
        let source = "# TODO one\nx = 1\n# FIXME two\ny = 2\n";
        let (comments, ledger) = seed(dir.path(), "a.py", source, &markers);
        let tracked = tracked_pairs(&comments, &markers);

        let backend = MockTracker::new(true);
        let report = reconcile_resolved(dir.path(), &tracked, &backend, &ledger, false).unwrap();

        assert_eq!(report.resolved_tokens.len(), 2);
        let written = std::fs::read_to_string(dir.path().join("a.py")).unwrap();
        // Both comments gone; both code statements intact (high→low kept offsets valid).
        assert!(
            !written.contains("TODO") && !written.contains("FIXME"),
            "{written}"
        );
        assert!(
            written.contains("x = 1") && written.contains("y = 2"),
            "{written}"
        );
    }

    #[test]
    fn test_tracker_lookups_cached_by_issue_id() {
        let dir = tempfile::TempDir::new().unwrap();
        // One comment, two markers → two tokens, but the SAME issue id (1), so the
        // tracker is queried exactly once.
        let source = "# TODO and FIXME together\nx = 1\n";
        std::fs::write(dir.path().join("a.py"), source).unwrap();
        let comments =
            extract_source(source, Language::Python, StdPath::new("a.py"), "a.py").unwrap();
        let mut ledger = IssueLedger::new();
        let issue = IssueRef {
            id: "1".to_owned(),
            url: "https://tracker/1".to_owned(),
            closed: false,
        };
        let todo = identity_token(&comments[0], "TODO");
        let fixme = identity_token(&comments[0], "FIXME");
        ledger.record(todo.clone(), issue.clone());
        ledger.record(fixme.clone(), issue);
        let tracked = vec![(&comments[0], todo), (&comments[0], fixme)];

        let backend = MockTracker::new(true);
        let report = reconcile_resolved(dir.path(), &tracked, &backend, &ledger, true).unwrap();

        assert_eq!(*backend.gets.borrow(), 1, "same issue id queried once");
        // One comment, deduped, with both tokens attributed to it.
        assert_eq!(report.removed, vec!["a.py:1".to_owned()]);
        assert_eq!(report.resolved_tokens.len(), 2);
    }

    #[test]
    fn test_comment_kept_while_any_issue_open() {
        // One comment, two markers → two DISTINCT issues (1 closed, 2 open). A
        // comment resolves only when EVERY issue is closed, so this one stays:
        // removing it would delete the still-open FIXME concern from source.
        let dir = tempfile::TempDir::new().unwrap();
        let source = "# TODO done; FIXME not\nx = 1\n";
        std::fs::write(dir.path().join("a.py"), source).unwrap();
        let comments =
            extract_source(source, Language::Python, StdPath::new("a.py"), "a.py").unwrap();
        let mut ledger = IssueLedger::new();
        let todo = identity_token(&comments[0], "TODO");
        let fixme = identity_token(&comments[0], "FIXME");
        ledger.record(
            todo.clone(),
            IssueRef {
                id: "1".to_owned(),
                url: "https://tracker/1".to_owned(),
                closed: false,
            },
        );
        ledger.record(
            fixme.clone(),
            IssueRef {
                id: "2".to_owned(),
                url: "https://tracker/2".to_owned(),
                closed: false,
            },
        );
        let tracked = vec![(&comments[0], todo), (&comments[0], fixme)];

        // Issue 1 closed, issue 2 still open.
        let backend = MockTracker::with_closed(&["1"]);
        let report = reconcile_resolved(dir.path(), &tracked, &backend, &ledger, false).unwrap();

        assert!(report.removed.is_empty(), "not all issues closed → keep");
        assert!(report.resolved_tokens.is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
            source
        );
    }
}
