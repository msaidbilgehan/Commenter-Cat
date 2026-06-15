//! Test-only helpers (compiled under `#[cfg(test)]`).
//!
//! [`TestRepo`] builds a real, isolated git repository via the `git` CLI with
//! deterministic commit dates — real systems tell the truth, so blame/rot are
//! tested against actual git, never a mock (Idea §11).

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// A throwaway git repository for blame/rot tests.
pub(crate) struct TestRepo {
    dir: TempDir,
}

impl TestRepo {
    /// Creates an empty repo with a configured identity, isolated from the
    /// machine's global/system git config.
    pub(crate) fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let repo = Self { dir };
        repo.git(&["init", "-q"], None);
        repo.git(&["config", "user.name", "Tester"], None);
        repo.git(&["config", "user.email", "t@example.com"], None);
        repo
    }

    /// The repository's work-directory root.
    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Writes a file relative to the repo root, creating parent directories.
    pub(crate) fn write(&self, relative: &str, contents: &str) {
        let target = self.dir.path().join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, contents).unwrap();
    }

    /// Stages everything and commits at the given git date
    /// (e.g. `"2020-01-01 00:00:00 +0000"`).
    pub(crate) fn commit_all(&self, date: &str) {
        self.git(&["add", "-A"], None);
        self.git(
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "snapshot",
            ],
            Some(date),
        );
    }

    /// Runs a git command in the repo, isolated and with a fixed identity. When
    /// `date` is set, it stamps both author and committer time.
    fn git(&self, args: &[&str], date: Option<&str>) {
        let mut command = Command::new("git");
        command
            .args(args)
            .current_dir(self.dir.path())
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            // Neutralize the machine's global/system config (gpg signing, hooks).
            .env("GIT_CONFIG_GLOBAL", self.dir.path().join("__no_global__"))
            .env("GIT_CONFIG_SYSTEM", self.dir.path().join("__no_system__"));
        if let Some(date) = date {
            command
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date);
        }
        let status = command.status().expect("git CLI is available");
        assert!(status.success(), "git {args:?} failed");
    }
}
