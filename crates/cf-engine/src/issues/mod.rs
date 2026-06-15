//! Comment-to-issue (Idea §9) — adjacent to the loop, never on its critical path.
//!
//! Filing is **mechanical and idempotent**: an issue is keyed on the comment's
//! **Tier-4 identity** (the bound symbol + kind + marker — stable across prose
//! rewordings), so re-running never double-files and a reworded TODO maps to the
//! same issue. Bidirectional `sync` routes marker resolution back through the
//! parse-invariant applier (a closed issue → the marker comment is removed
//! safely). The tracker is the [`backend`] seam; auth is the host's.

pub mod backend;
pub mod github;
pub mod ledger_file;

use std::collections::BTreeMap;

use cf_core::comment::Comment;
use cf_core::error::CfResult;

use crate::ops::apply::{self, ApplyResult};

pub use backend::{IssueBackend, IssueRef, IssueRequest};
pub use ledger_file::{LEDGER_FILENAME, LEDGER_VERSION};

/// The stable Tier-4 identity token an issue is filed against (Idea §9). Built
/// from the parts that survive a prose rewording — the bound symbol, the comment
/// kind, and the marker that triggered filing — so a reworded comment maps to the
/// same token and is never double-filed.
#[must_use]
pub fn identity_token(comment: &Comment, marker: &str) -> String {
    let symbol = comment
        .bound_symbol
        .as_ref()
        .map_or("<none>", |s| s.as_str());
    format!("{symbol}|{}|{marker}", comment.kind.as_str())
}

/// A map from Tier-4 identity token to its filed issue — the idempotency ledger,
/// persisted against the index (Idea §9). In-memory here; the caller hydrates it
/// from / flushes it to storage.
#[derive(Debug, Clone, Default)]
pub struct IssueLedger {
    entries: BTreeMap<String, IssueRef>,
}

impl IssueLedger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The issue already filed for `token`, if any.
    #[must_use]
    pub fn get(&self, token: &str) -> Option<&IssueRef> {
        self.entries.get(token)
    }

    /// Records an issue against a token.
    pub fn record(&mut self, token: String, issue: IssueRef) {
        self.entries.insert(token, issue);
    }

    /// The number of distinct identities filed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Iterates the recorded `(token, issue)` pairs in token order — the
    /// persistence seam ([`ledger_file`]) flushes this to committed truth.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &IssueRef)> {
        self.entries
            .iter()
            .map(|(token, issue)| (token.as_str(), issue))
    }

    /// Whether the ledger is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Files an issue for `token` idempotently: an issue already recorded for the
/// same Tier-4 identity is reused (the backend is **not** called), so re-runs and
/// reworded comments never double-file (Idea §9).
///
/// # Errors
/// Returns [`cf_core::CfError`] if the backend create call fails.
pub fn file_issue(
    token: &str,
    request: &IssueRequest,
    backend: &dyn IssueBackend,
    ledger: &mut IssueLedger,
) -> CfResult<IssueRef> {
    if let Some(existing) = ledger.get(token) {
        return Ok(existing.clone());
    }
    let issue = backend.create(request)?;
    ledger.record(token.to_owned(), issue.clone());
    Ok(issue)
}

/// Bidirectional sync (opt-in, Idea §9): if the issue filed for `token` is now
/// closed, resolve its marker comment by routing a removal through the
/// parse-invariant applier. Returns the edited source, or `None` if there is
/// nothing to resolve (no issue, or still open).
///
/// # Errors
/// Returns [`cf_core::CfError`] if the backend lookup or the safe-write fails.
pub fn sync_resolution(
    source: &str,
    comment: &Comment,
    token: &str,
    backend: &dyn IssueBackend,
    ledger: &IssueLedger,
) -> CfResult<Option<ApplyResult>> {
    let Some(filed) = ledger.get(token) else {
        return Ok(None);
    };
    let current = backend.get(&filed.id)?;
    if current.is_some_and(|issue| issue.closed) {
        // The tracker says resolved → remove the marker comment safely.
        return Ok(Some(apply::remove(source, comment, false)?));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_source;
    use cf_core::lang::Language;
    use std::cell::RefCell;
    use std::path::Path;

    /// An in-memory tracker — the mocked network seam (Idea §11).
    struct MockTracker {
        created: RefCell<Vec<IssueRequest>>,
        closed: bool,
    }

    impl MockTracker {
        fn new(closed: bool) -> Self {
            Self {
                created: RefCell::new(Vec::new()),
                closed,
            }
        }
        fn create_count(&self) -> usize {
            self.created.borrow().len()
        }
    }

    impl IssueBackend for MockTracker {
        fn create(&self, request: &IssueRequest) -> CfResult<IssueRef> {
            self.created.borrow_mut().push(request.clone());
            let n = self.created.borrow().len();
            Ok(IssueRef {
                id: n.to_string(),
                url: format!("https://tracker/{n}"),
                closed: self.closed,
            })
        }
        fn get(&self, id: &str) -> CfResult<Option<IssueRef>> {
            Ok(Some(IssueRef {
                id: id.to_owned(),
                url: format!("https://tracker/{id}"),
                closed: self.closed,
            }))
        }
    }

    fn request() -> IssueRequest {
        IssueRequest {
            title: "TODO".to_owned(),
            body: "body".to_owned(),
            labels: vec![],
        }
    }

    #[test]
    fn test_filing_is_idempotent_on_identity() {
        let tracker = MockTracker::new(false);
        let mut ledger = IssueLedger::new();
        let first = file_issue("app.f|line|TODO", &request(), &tracker, &mut ledger).unwrap();
        let again = file_issue("app.f|line|TODO", &request(), &tracker, &mut ledger).unwrap();
        // Same identity → same issue, backend called exactly once.
        assert_eq!(first, again);
        assert_eq!(tracker.create_count(), 1);
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn test_reworded_comment_does_not_double_file() {
        let tracker = MockTracker::new(false);
        let mut ledger = IssueLedger::new();
        // Two comments, same symbol + kind + marker but different prose — the
        // Tier-4 token is identical, so the second reuses the first's issue.
        let mut original = Comment::new(
            "a.py",
            "h1",
            Language::Python,
            cf_core::kind::CommentKind::Line,
            range(),
            "# TODO fix the parser",
        );
        original.bound_symbol = Some(cf_core::symbol::BoundSymbol::new("app.parse"));
        let mut reworded = original.clone();
        reworded.raw_text = "# TODO repair the parser later".to_owned();

        let t1 = identity_token(&original, "TODO");
        let t2 = identity_token(&reworded, "TODO");
        assert_eq!(t1, t2, "rewording does not change the Tier-4 token");

        file_issue(&t1, &request(), &tracker, &mut ledger).unwrap();
        file_issue(&t2, &request(), &tracker, &mut ledger).unwrap();
        assert_eq!(tracker.create_count(), 1, "no double-file");
    }

    #[test]
    fn test_sync_routes_resolution_through_applier() {
        let source = "# TODO remove me\nx = 1\n";
        let comment = extract_source(source, Language::Python, Path::new("a.py"), "a.py")
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let token = identity_token(&comment, "TODO");

        // File against a tracker whose issue is already closed.
        let tracker = MockTracker::new(true);
        let mut ledger = IssueLedger::new();
        file_issue(&token, &request(), &tracker, &mut ledger).unwrap();

        let resolved = sync_resolution(source, &comment, &token, &tracker, &ledger).unwrap();
        // The closed issue → the marker comment is removed via the applier
        // (parse-invariant: the code survives).
        let applied = resolved.expect("a closed issue resolves its comment");
        assert!(!applied.new_source.contains("TODO"));
        assert!(applied.new_source.contains("x = 1"));
    }

    fn range() -> cf_core::finding::Range {
        cf_core::finding::Range::new(0, 16, 1, 1)
    }
}
