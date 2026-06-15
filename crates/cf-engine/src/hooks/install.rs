//! `cf install-hooks` (Idea §7, §9).
//!
//! The hook **warms the cache**; it never owns the data (source of truth is the
//! scanner-on-demand). It is installed via `core.hooksPath` pointing at a
//! cf-managed directory — we never hand-edit `.git/hooks/` (so a user's existing
//! hooks are untouched and the install is reversible by clearing the config).
//! Each hook scans **only the commit's changed files** (`git diff-tree`) and is
//! **non-fatal** (always exits 0 — warming must never block a commit).

use std::path::{Path, PathBuf};
use std::process::Command;

use cf_core::error::{CfError, CfResult};

/// The git events whose changes can invalidate comment↔code mappings (Idea §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    /// After a commit — the changed set is the new commit.
    PostCommit,
    /// After a checkout — the working tree changed.
    PostCheckout,
    /// After a merge — merged changes landed.
    PostMerge,
    /// After history rewriting (rebase/amend).
    PostRewrite,
}

impl HookEvent {
    /// Every event the warmer covers.
    pub const ALL: [HookEvent; 4] = [
        HookEvent::PostCommit,
        HookEvent::PostCheckout,
        HookEvent::PostMerge,
        HookEvent::PostRewrite,
    ];

    /// The git hook filename for this event.
    #[must_use]
    pub const fn filename(self) -> &'static str {
        match self {
            HookEvent::PostCommit => "post-commit",
            HookEvent::PostCheckout => "post-checkout",
            HookEvent::PostMerge => "post-merge",
            HookEvent::PostRewrite => "post-rewrite",
        }
    }
}

/// The cf-managed hooks directory (set as `core.hooksPath`). It lives under the
/// gitignored cache so it is never committed.
#[must_use]
pub fn hooks_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".comment-finder").join("hooks")
}

/// The shell script for a hook event — a non-fatal, changed-files-only warmer.
#[must_use]
pub fn hook_script(event: HookEvent) -> String {
    // `git diff-tree HEAD` lists the files the latest commit touched; the warm
    // pass runs `cf` on only those. Every line is guarded so a failure (cf
    // absent, detached state) can never block the git operation.
    format!(
        "#!/bin/sh\n\
         # Commenter-Cat cache warmer ({event}) — non-fatal, never blocks.\n\
         changed=$(git diff-tree --no-commit-id --name-only -r HEAD 2>/dev/null)\n\
         if [ -n \"$changed\" ]; then\n\
         \x20 cf check $changed >/dev/null 2>&1 || true\n\
         fi\n\
         exit 0\n",
        event = event.filename()
    )
}

/// What `install` did, for reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    /// The directory the hooks were written to (now `core.hooksPath`).
    pub hooks_dir: PathBuf,
    /// The hook filenames written.
    pub installed: Vec<String>,
}

/// Installs the warmer hooks: writes the four scripts and points
/// `core.hooksPath` at them (Idea §7).
///
/// # Errors
/// Returns [`CfError::Config`] if the directory/scripts cannot be written or
/// `git config` fails.
pub fn install(repo_root: &Path) -> CfResult<InstallReport> {
    let dir = hooks_dir(repo_root);
    std::fs::create_dir_all(&dir).map_err(|e| {
        CfError::config(format!("creating hooks dir {}", dir.display())).caused_by(e)
    })?;

    let mut installed = Vec::new();
    for event in HookEvent::ALL {
        let path = dir.join(event.filename());
        std::fs::write(&path, hook_script(event)).map_err(|e| {
            CfError::config(format!("writing hook {}", path.display())).caused_by(e)
        })?;
        make_executable(&path)?;
        installed.push(event.filename().to_owned());
    }

    set_hooks_path(repo_root, &dir)?;
    Ok(InstallReport {
        hooks_dir: dir,
        installed,
    })
}

/// Points `core.hooksPath` at `dir` via `git config` (never hand-editing
/// `.git/hooks/`).
fn set_hooks_path(repo_root: &Path, dir: &Path) -> CfResult<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "core.hooksPath"])
        .arg(dir)
        .status()
        .map_err(|e| CfError::config("spawning `git config core.hooksPath`").caused_by(e))?;
    if !status.success() {
        return Err(CfError::config("`git config core.hooksPath` failed"));
    }
    Ok(())
}

/// Marks a hook script executable (Unix). A no-op elsewhere.
fn make_executable(path: &Path) -> CfResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .map_err(|e| CfError::config("reading hook permissions").caused_by(e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)
            .map_err(|e| CfError::config("setting hook executable bit").caused_by(e))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn test_script_warms_only_changed_files_and_is_non_fatal() {
        let script = hook_script(HookEvent::PostCommit);
        assert!(script.starts_with("#!/bin/sh"));
        // Scans only the changed set …
        assert!(script.contains("git diff-tree --no-commit-id --name-only -r HEAD"));
        assert!(script.contains("cf check $changed"));
        // … and never blocks the git operation.
        assert!(script.contains("|| true"));
        assert!(script.contains("exit 0"));
    }

    #[test]
    fn test_install_writes_four_events_via_hooks_path() {
        let repo = TestRepo::new();
        let report = install(repo.path()).unwrap();

        // All four events are covered …
        assert_eq!(report.installed.len(), 4);
        for event in HookEvent::ALL {
            let path = report.hooks_dir.join(event.filename());
            assert!(path.exists(), "{} written", event.filename());
        }

        // … and core.hooksPath points at the managed dir (never .git/hooks).
        let configured = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["config", "--get", "core.hooksPath"])
            .output()
            .unwrap();
        let value = String::from_utf8_lossy(&configured.stdout);
        assert!(
            value.trim().ends_with("hooks"),
            "hooksPath set to the managed dir"
        );
    }

    #[test]
    fn test_all_events_have_distinct_filenames() {
        let names: std::collections::BTreeSet<&str> =
            HookEvent::ALL.iter().map(|e| e.filename()).collect();
        assert_eq!(names.len(), 4);
    }
}
