//! The pluggable issue-tracker seam (Idea §9).
//!
//! `comment-to-issue` is at the **edge** of the loop, never its center. The
//! tracker is abstracted behind [`IssueBackend`] so GitHub, Jira, and GitLab are
//! the same interface — and so tests mock only the **network seam** (Idea §11),
//! never the filing logic. Auth is always the host's (`gh`/`GITHUB_TOKEN`, env,
//! or a secrets manager) — never the config or the index.

use commenter_cat_core::error::CommenterCatResult;

/// A request to file an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRequest {
    /// The issue title (e.g. the marker line).
    pub title: String,
    /// The body (the comment, its location, and a back-reference).
    pub body: String,
    /// Labels to apply (e.g. `commenter-cat`, the marker name).
    pub labels: Vec<String>,
}

/// A reference to a filed issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    /// The tracker id/number (backend-specific).
    pub id: String,
    /// The canonical URL, stored against the comment's identity for idempotency.
    pub url: String,
    /// Whether the issue is resolved/closed (drives bidirectional sync).
    pub closed: bool,
}

/// The tracker seam. Implementations shell out to host tooling (`gh`) or a host
/// API client; the filing/idempotency logic in [`super`] never knows which.
pub trait IssueBackend {
    /// Creates an issue, returning its reference.
    ///
    /// # Errors
    /// Returns [`commenter_cat_core::CommenterCatError`] if the tracker call fails.
    fn create(&self, request: &IssueRequest) -> CommenterCatResult<IssueRef>;

    /// Fetches an issue's current state (for sync), or `None` if it is gone.
    ///
    /// # Errors
    /// Returns [`commenter_cat_core::CommenterCatError`] if the tracker call fails.
    fn get(&self, id: &str) -> CommenterCatResult<Option<IssueRef>>;
}
