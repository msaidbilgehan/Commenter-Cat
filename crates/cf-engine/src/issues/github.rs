//! The GitHub issue backend via the host `gh` CLI (Idea §9).
//!
//! Shelling out to `gh` means auth is the host's (`gh auth` / `GITHUB_TOKEN`) —
//! never read from the config or index (Idea §9 auth rule). The command
//! construction is pure and unit-tested; the subprocess calls are exercised by
//! the integration suite (Phase 10), not mocked here.

use std::process::Command;

use cf_core::error::{CfError, CfResult};

use super::backend::{IssueBackend, IssueRef, IssueRequest};

/// A GitHub backend driven by the `gh` CLI.
#[derive(Debug, Clone, Default)]
pub struct GhCliBackend {
    /// `owner/name`, or `None` to use the current directory's repository.
    repo: Option<String>,
}

impl GhCliBackend {
    /// A backend targeting the current directory's repository.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A backend targeting an explicit `owner/name` repository.
    #[must_use]
    pub fn for_repo(repo: impl Into<String>) -> Self {
        Self {
            repo: Some(repo.into()),
        }
    }

    /// The `gh issue create` arguments for a request (pure — unit-tested).
    fn create_args(&self, request: &IssueRequest) -> Vec<String> {
        let mut args = vec![
            "issue".to_owned(),
            "create".to_owned(),
            "--title".to_owned(),
            request.title.clone(),
            "--body".to_owned(),
            request.body.clone(),
        ];
        for label in &request.labels {
            args.push("--label".to_owned());
            args.push(label.clone());
        }
        if let Some(repo) = &self.repo {
            args.push("--repo".to_owned());
            args.push(repo.clone());
        }
        args
    }

    /// The `gh issue view` arguments for an issue id (pure — unit-tested).
    fn view_args(&self, id: &str) -> Vec<String> {
        let mut args = vec![
            "issue".to_owned(),
            "view".to_owned(),
            id.to_owned(),
            "--json".to_owned(),
            "url,state".to_owned(),
        ];
        if let Some(repo) = &self.repo {
            args.push("--repo".to_owned());
            args.push(repo.clone());
        }
        args
    }
}

impl IssueBackend for GhCliBackend {
    fn create(&self, request: &IssueRequest) -> CfResult<IssueRef> {
        let output = Command::new("gh")
            .args(self.create_args(request))
            .output()
            .map_err(|e| CfError::provider("gh", "spawning `gh issue create`").caused_by(e))?;
        if !output.status.success() {
            return Err(CfError::provider(
                "gh",
                format!(
                    "`gh issue create` failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        // `gh issue create` prints the new issue URL on stdout.
        let url = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let id = url.rsplit('/').next().unwrap_or_default().to_owned();
        Ok(IssueRef {
            id,
            url,
            closed: false,
        })
    }

    fn get(&self, id: &str) -> CfResult<Option<IssueRef>> {
        let output = Command::new("gh")
            .args(self.view_args(id))
            .output()
            .map_err(|e| CfError::provider("gh", "spawning `gh issue view`").caused_by(e))?;
        if !output.status.success() {
            // A missing issue is `None`, not an error.
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| CfError::provider("gh", "parsing `gh issue view` JSON").caused_by(e))?;
        let url = value
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let closed = value
            .get("state")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|state| state.eq_ignore_ascii_case("closed"));
        Ok(Some(IssueRef {
            id: id.to_owned(),
            url: url.to_owned(),
            closed,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_args_include_title_body_labels_and_repo() {
        let backend = GhCliBackend::for_repo("acme/widgets");
        let request = IssueRequest {
            title: "TODO: fix parser".to_owned(),
            body: "see app.py:12".to_owned(),
            labels: vec!["commenter-cat".to_owned(), "TODO".to_owned()],
        };
        let args = backend.create_args(&request);
        assert_eq!(args[0..2], ["issue", "create"]);
        assert!(args.contains(&"--title".to_owned()));
        assert!(args.contains(&"TODO: fix parser".to_owned()));
        // Both labels are passed.
        assert_eq!(args.iter().filter(|a| *a == "--label").count(), 2);
        // The explicit repo is targeted.
        let repo_idx = args.iter().position(|a| a == "--repo").unwrap();
        assert_eq!(args[repo_idx + 1], "acme/widgets");
    }

    #[test]
    fn test_view_args_request_url_and_state() {
        let args = GhCliBackend::new().view_args("42");
        assert_eq!(args, vec!["issue", "view", "42", "--json", "url,state"]);
    }
}
