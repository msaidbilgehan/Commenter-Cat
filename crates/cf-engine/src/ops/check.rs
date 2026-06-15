//! `cf check` orchestration (Idea §5; task 7.1).
//!
//! The engine as **conductor**: run the native pass (walk → extract → coalesce →
//! map → markers), gather the native findings the providers can't produce (rot
//! candidates + marker triage), run every provider, then **fuse** — attach all
//! findings to the comment they concern and dedup. "AI proposes, engine
//! guarantees": the output is one unified record per comment (Idea §4, §5).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use cf_core::comment::Comment;
use cf_core::config::ResolvedConfig;
use cf_core::error::{CfError, CfResult};
use cf_core::finding::Finding;
use cf_core::identity::cosmetic_fingerprint;

use crate::extract::coalesce::coalesce;
use crate::extract::extract_source;
use crate::map::map_comments;
use crate::markers::MarkerSet;
use crate::ops::{normalize, triage};
use crate::provider::{ProviderContext, RuleProvider, RunState};
use crate::walk::{walk, walk_universe, WalkOptions, WalkedFile};

/// The unified result of `cf check` (Idea §5).
#[derive(Debug)]
pub struct CheckResult {
    /// Comment records with fused, deduped `findings[]`.
    pub comments: Vec<Comment>,
    /// Per-provider run state (`SUCCESS`/`EMPTY`/`PARTIAL`/`SKIPPED`) — the
    /// trustworthiness signal, *not* the process exit code (Idea §5).
    pub run_states: Vec<(String, RunState)>,
    /// Provider findings that attached to no comment (symbol-targeted, e.g.
    /// `doc_missing` on an undocumented symbol).
    pub unattached: Vec<Finding>,
}

/// Runs `cf check` over `root`: the native pass, every provider, then fusion.
///
/// # Errors
/// Returns [`CfError`] if the walk, a file read, or a grammar pass fails.
pub fn check(
    root: &Path,
    config: &ResolvedConfig,
    providers: &[&dyn RuleProvider],
) -> CfResult<CheckResult> {
    // One walk yields the comment-language files (the native pass + the provider
    // file list); a second yields the broader `cf_scope` universe (every
    // non-ignored file) used to validate provider findings (Idea §3, §5).
    let scan_options = WalkOptions::from_scan_config(&config.scan);
    let walked = walk(root, &scan_options)?;
    let comments = native_pass(root, config, &walked)?;
    let files: Vec<PathBuf> = walked.iter().map(|file| file.path.clone()).collect();
    let universe = walk_universe(root, &scan_options)?;
    fuse(root, config, comments, files, universe, providers)
}

/// The native pass over the walked files: extract → coalesce → map → tag markers.
fn native_pass(
    root: &Path,
    config: &ResolvedConfig,
    walked: &[WalkedFile],
) -> CfResult<Vec<Comment>> {
    let marker_set = MarkerSet::new(&config.markers.custom);
    let mut comments = Vec::new();
    for file in walked {
        let source = std::fs::read_to_string(&file.path).map_err(|e| {
            CfError::extract(format!("reading {}", file.path.display())).caused_by(e)
        })?;
        let repo_path = repo_relative(&file.path, root);
        let mut file_comments = extract_source(&source, file.language, &file.path, &repo_path)?;
        file_comments = coalesce(&source, file_comments);
        map_comments(&source, file.language, &file.path, &mut file_comments)?;
        for comment in &mut file_comments {
            marker_set.tag(comment);
        }
        comments.extend(file_comments);
    }
    Ok(comments)
}

/// Fuses native + provider findings onto `comments` (Idea §4, §5). Extracted so
/// it is unit-testable with mock providers, independent of the filesystem walk.
/// `files` is the comment-language set handed to providers; `universe` is the
/// `cf_scope` set (every non-ignored file) provider findings are validated against.
fn fuse(
    root: &Path,
    config: &ResolvedConfig,
    mut comments: Vec<Comment>,
    files: Vec<PathBuf>,
    universe: Vec<String>,
    providers: &[&dyn RuleProvider],
) -> CfResult<CheckResult> {
    // 1. Native findings (rot + marker triage) attach to their own comment, and
    //    the cosmetic fingerprint (the §4 identity field) is filled in so the
    //    persisted record, the baseline diff, and cross-scan identity all have it.
    for comment in &mut comments {
        comment.cosmetic_fingerprint = Some(cosmetic_fingerprint(&comment.raw_text));
        let mut native = triage::marker_findings(comment, &config.markers.severity);
        native.extend(triage::rot_finding(comment));
        comment.findings.extend(native);
    }

    // 2. Run every provider; record its run state (Idea §5: state, not exit code).
    let context = ProviderContext::new(root, &config.severity.overrides);
    let mut provider_findings = Vec::new();
    let mut run_states = Vec::new();
    for provider in providers {
        let run = provider.run(&files, &context);
        run_states.push((provider.id().to_owned(), run.state));
        provider_findings.extend(run.findings);
    }

    // CF owns the file universe (Idea §3, §5): `cf_scope` is *every* non-ignored
    // file, not just the comment-language subset — so a project-scoped tool (e.g.
    // gitleaks) keeps a secret it finds in `.env`/config (surfaced as unattached,
    // since CF extracts no comments there), while a hit in a gitignored/excluded
    // path (node_modules/.venv) is dropped. A provider only vetoes within
    // `cf_scope`, never widens it.
    let in_scope: HashSet<String> = universe.into_iter().collect();
    provider_findings.retain(|finding| in_scope.contains(&finding.file));

    // 3. Attach provider findings + dedup every comment's fused set.
    let unattached = normalize::attach_findings(&mut comments, provider_findings);
    normalize::dedup_comment_findings(&mut comments);

    Ok(CheckResult {
        comments,
        run_states,
        unattached,
    })
}

/// The repo-relative, forward-slash path used as a stable comment id component.
fn repo_relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Capabilities, ProviderRun, Scope};
    use crate::testutil::TestRepo;
    use cf_core::finding::{Category, CoordinateSystem, FindingTarget, Fix, Origin, Range};
    use cf_core::severity::Severity;
    use cf_core::symbol::{BoundSymbol, CommentId};

    /// A mock provider returning one canned finding on the file's first comment.
    struct MockProvider {
        capabilities: Capabilities,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                capabilities: Capabilities {
                    scope: Scope::File,
                    supports_fix: false,
                    supports_incremental: true,
                    supports_sarif: false,
                    coordinate_system: CoordinateSystem::tree_sitter(),
                },
            }
        }
    }

    impl RuleProvider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }
        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }
        fn run(&self, _files: &[PathBuf], _ctx: &ProviderContext<'_>) -> ProviderRun {
            // A provider finding pointing at the first comment's bytes.
            ProviderRun::ran(vec![Finding {
                file: "pkg/m.py".to_owned(),
                target: FindingTarget::Comment(CommentId::new("x")),
                range: Range::new(0, 6, 1, 1),
                origin: Origin::Ruff,
                provider_rule_id: "ruff:ERA001".to_owned(),
                canonical_rule_id: "ERA001".to_owned(),
                category: Category::CommentedCode,
                severity: Severity::Warning,
                severity_native: None,
                message: "commented-out code".to_owned(),
                fix: Fix::ProviderAutofix,
                url: None,
                also_from: Default::default(),
            }])
        }
    }

    /// A provider that reports a finding for a file CF never walked (e.g. a
    /// project-scoped tool reaching into `node_modules`).
    struct OutOfScopeProvider {
        capabilities: Capabilities,
    }

    impl RuleProvider for OutOfScopeProvider {
        fn id(&self) -> &str {
            "oos"
        }
        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }
        fn run(&self, _files: &[PathBuf], _ctx: &ProviderContext<'_>) -> ProviderRun {
            ProviderRun::ran(vec![Finding {
                file: "node_modules/dep/leak.js".to_owned(),
                target: FindingTarget::Comment(CommentId::new("x")),
                range: Range::new(0, 6, 1, 1),
                origin: Origin::Other("gitleaks".to_owned()),
                provider_rule_id: "gitleaks:aws-key".to_owned(),
                canonical_rule_id: "aws-key".to_owned(),
                category: Category::Secret,
                severity: Severity::Critical,
                severity_native: None,
                message: "secret in a dependency".to_owned(),
                fix: Fix::None,
                url: None,
                also_from: Default::default(),
            }])
        }
    }

    #[test]
    fn test_out_of_scope_provider_finding_is_dropped() {
        // CF owns the universe: a finding in a file CF did not walk is dropped
        // entirely — not attached, not even surfaced as unattached (Idea §5).
        let repo = TestRepo::new();
        repo.write("pkg/m.py", "x = 1\n");
        let oos = OutOfScopeProvider {
            capabilities: Capabilities {
                scope: Scope::Project,
                supports_fix: false,
                supports_incremental: false,
                supports_sarif: false,
                coordinate_system: CoordinateSystem::tree_sitter(),
            },
        };
        let providers: [&dyn RuleProvider; 1] = [&oos];
        let result = check(repo.path(), &ResolvedConfig::default(), &providers).unwrap();

        assert!(
            result.unattached.is_empty(),
            "out-of-scope finding is filtered before attachment, not surfaced"
        );
        assert!(
            result
                .comments
                .iter()
                .all(|comment| comment.findings.is_empty()),
            "no comment carries the out-of-scope dependency finding"
        );
        assert_eq!(result.run_states[0].1, RunState::Success);
    }

    /// A project-scoped mock (gitleaks-shaped) reporting a `secret` finding for
    /// each named file — to prove `cf_scope` filtering keeps config-file hits
    /// (`.env`) but drops gitignored ones.
    struct SecretScanProvider {
        files: Vec<String>,
        capabilities: Capabilities,
    }

    impl SecretScanProvider {
        fn new(files: &[&str]) -> Self {
            Self {
                files: files.iter().map(|f| (*f).to_owned()).collect(),
                capabilities: Capabilities {
                    scope: Scope::Project,
                    supports_fix: false,
                    supports_incremental: false,
                    supports_sarif: true,
                    coordinate_system: CoordinateSystem::tree_sitter(),
                },
            }
        }
    }

    impl RuleProvider for SecretScanProvider {
        fn id(&self) -> &str {
            "secretscan"
        }
        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }
        fn run(&self, _files: &[PathBuf], _ctx: &ProviderContext<'_>) -> ProviderRun {
            ProviderRun::ran(
                self.files
                    .iter()
                    .map(|file| Finding {
                        file: file.clone(),
                        target: FindingTarget::Symbol(BoundSymbol::new(file.as_str())),
                        range: Range::new(0, 12, 1, 1),
                        origin: Origin::Other("gitleaks".to_owned()),
                        provider_rule_id: "gitleaks:generic-api-key".to_owned(),
                        canonical_rule_id: "generic-api-key".to_owned(),
                        category: Category::Secret,
                        severity: Severity::Critical,
                        severity_native: None,
                        message: "secret".to_owned(),
                        fix: Fix::None,
                        url: None,
                        also_from: Default::default(),
                    })
                    .collect(),
            )
        }
    }

    #[test]
    fn test_secret_scope_keeps_env_but_drops_gitignored() {
        // `cf_scope` is the non-ignored universe, NOT just comment-language files:
        // a secret in `.env` (config CF does not comment-analyze, Idea §3) is kept
        // and surfaced as unattached, while a secret in a gitignored path is
        // dropped (Idea §5 — a provider vetoes within `cf_scope`, never widens it).
        let repo = TestRepo::new();
        repo.write("pkg/m.py", "x = 1\n"); // a source file, no comments
        repo.write(".env", "APP_ENV=local\n"); // config dotfile — in scope
        repo.write(".gitignore", "vendored/\n");
        repo.write("vendored/leak.py", "VALUE = 1\n"); // gitignored — out of scope

        let provider = SecretScanProvider::new(&[".env", "vendored/leak.py"]);
        let providers: [&dyn RuleProvider; 1] = [&provider];
        let result = check(repo.path(), &ResolvedConfig::default(), &providers).unwrap();

        // The `.env` secret has no comment to attach to → unattached, but kept;
        // the gitignored hit is dropped entirely.
        assert_eq!(
            result.unattached.len(),
            1,
            "the .env secret is retained; the gitignored one is dropped"
        );
        assert_eq!(result.unattached[0].file, ".env");
        assert_eq!(result.unattached[0].category, Category::Secret);
    }

    #[test]
    fn test_check_fuses_provider_and_native_findings() {
        let repo = TestRepo::new();
        // `# TODO` triggers native marker triage; the mock adds a provider finding.
        repo.write("pkg/m.py", "# TODO clean this up\nx = 1\n");
        let config = ResolvedConfig::default();
        let mock = MockProvider::new();
        let providers: [&dyn RuleProvider; 1] = [&mock];

        let result = check(repo.path(), &config, &providers).unwrap();

        // One comment, carrying BOTH the native marker finding and the provider one.
        assert_eq!(result.comments.len(), 1);
        let findings = &result.comments[0].findings;
        assert!(
            findings
                .iter()
                .any(|f| f.origin == Origin::Native && f.message.contains("TODO")),
            "native marker triage fused in"
        );
        assert!(
            findings
                .iter()
                .any(|f| f.origin == Origin::Ruff && f.canonical_rule_id == "ERA001"),
            "provider finding fused in"
        );
        // The provider's run state is recorded (Success — it produced a finding).
        assert_eq!(
            result.run_states,
            vec![("mock".to_owned(), RunState::Success)]
        );
    }
}
