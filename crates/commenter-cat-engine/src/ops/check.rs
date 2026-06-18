//! `commenter-cat check` orchestration (Idea §5; task 7.1).
//!
//! The engine as **conductor**: run the native pass (walk → extract → coalesce →
//! map → markers), gather the native findings the providers can't produce (rot
//! candidates + marker triage), run every provider, then **fuse** — attach all
//! findings to the comment they concern and dedup. "AI proposes, engine
//! guarantees": the output is one unified record per comment (Idea §4, §5).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use commenter_cat_core::comment::Comment;
use commenter_cat_core::config::ResolvedConfig;
use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
use commenter_cat_core::finding::Finding;
use commenter_cat_core::identity::cosmetic_fingerprint;
use commenter_cat_core::lang::Language;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::embed::DeterministicEmbedder;
use crate::extract::coalesce::coalesce;
use crate::extract::extract_source;
use crate::git::{enrich, BlameIndex, Repo};
use crate::map::map_comments;
use crate::markers::MarkerSet;
use crate::ops::{baseline, normalize, provider_cache, suppress, triage};
use crate::provider::{ProviderContext, RuleProvider, RunState};
use crate::rot::path_ref::RepoPaths;
use crate::rot::{rot_pass, FileSource};
use crate::walk::{walk, walk_universe, WalkOptions, WalkedFile};
use commenter_cat_core::config::RotConfig;

/// Per-stage wall-clock breakdown of a `commenter-cat check` run, surfaced by `--stats`
/// (Idea §6 — the budget *shape*: warm interactive, cold provider-bound). These
/// are timing-only and never influence the deterministic findings (Idea §11).
#[derive(Debug, Default, Clone, Copy)]
pub struct Timings {
    /// The file walk — the comment-language set plus the `commenter_cat_scope` universe.
    pub walk: Duration,
    /// The native pass (extract → coalesce → map → tag markers, parallel).
    pub native_pass: Duration,
    /// Provider invocation — cache hashing + lookups + any subprocess.
    pub providers: Duration,
    /// Fusion — native findings, scope filtering, attachment, dedup.
    pub fusion: Duration,
}

/// The unified result of `commenter-cat check` (Idea §5).
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
    /// Provider-cache outcome (hits vs runs) for `--stats` (Idea §6).
    pub cache_stats: provider_cache::CacheStats,
    /// Per-stage wall-clock timings for `--stats` (Idea §6 budget shape).
    pub timings: Timings,
    /// Findings suppressed by inline `commenter-cat:*` directives or the committed baseline
    /// (Idea §5) — located by index, kept in `comments`, excluded from default
    /// views and never gating CI.
    pub suppressed: Vec<suppress::SuppressedFinding>,
}

/// Runs `commenter-cat check` over `root`: the native pass, every provider, then fusion.
///
/// When `use_cache` is set, provider results are served from (and written to) the
/// content-addressed `inputs.db` cache, so a provider never re-runs on unchanged
/// input (Idea §6, the load-bearing performance lever); pass `false` to force a
/// full re-run.
///
/// # Errors
/// Returns [`CommenterCatError`] if the walk, a file read, or a grammar pass fails.
pub fn check(
    root: &Path,
    config: &ResolvedConfig,
    providers: &[&dyn RuleProvider],
    use_cache: bool,
) -> CommenterCatResult<CheckResult> {
    // One walk yields the comment-language files (the native pass + the provider
    // file list); a second yields the broader `commenter_cat_scope` universe (every
    // non-ignored file) used to validate provider findings (Idea §3, §5).
    let scan_options = WalkOptions::from_scan_config(&config.scan);
    let walk_start = Instant::now();
    let walked = walk(root, &scan_options)?;
    let walk_only = walk_start.elapsed();
    let native_start = Instant::now();
    let (mut comments, file_sources) = native_pass(root, config, &walked)?;
    // Git enrichment (stage 2.6): joins blame onto each comment and yields the
    // BlameIndex the git-drift detector reads. A missing/empty repo degrades to
    // an empty index, never an error (Idea §5).
    let blames = blame_index(root, &mut comments);
    let native_time = native_start.elapsed();
    // Carry each file's language so the orchestrator can narrow per provider
    // (Idea §5): shellcheck gets shell files, ruff gets python, etc.
    let files: Vec<(PathBuf, Language)> = walked
        .iter()
        .map(|file| (file.path.clone(), file.language))
        .collect();
    let universe_start = Instant::now();
    let universe = walk_universe(root, &scan_options)?;
    let walk_time = walk_only + universe_start.elapsed();
    let mut result = fuse(
        root,
        config,
        comments,
        file_sources,
        files,
        universe,
        &blames,
        providers,
        use_cache,
    )?;
    result.timings.walk = walk_time;
    result.timings.native_pass = native_time;
    result.suppressed = apply_suppression(root, &result.comments)?;
    Ok(result)
}

/// Loads the committed baseline (if present) and runs the unified suppression
/// pass — inline `commenter-cat:*` directives + the Tier-2 baseline (Idea §5) — over the
/// fused comments. Suppressed findings are located + annotated, never dropped.
fn apply_suppression(
    root: &Path,
    comments: &[Comment],
) -> CommenterCatResult<Vec<suppress::SuppressedFinding>> {
    let baseline_path = root.join(baseline::BASELINE_FILENAME);
    let committed = if baseline_path.exists() {
        baseline::load(&baseline_path)?
    } else {
        baseline::Baseline::default()
    };
    Ok(suppress::apply(comments, &committed))
}

/// The native pass over the walked files: extract → coalesce → map → tag markers.
///
/// Per-file work is independent and CPU-bound (tree-sitter parse + the
/// comment→code mapping), so it runs across the rayon pool. `walk` returns a
/// deterministically sorted slice and an indexed parallel `collect` preserves that
/// order, so the fused comment stream is byte-for-byte identical to a sequential
/// pass (Idea §11 reproducibility). Each file's source text is carried back as a
/// [`FileSource`] so the rot pass can re-parse without a second read.
fn native_pass(
    root: &Path,
    config: &ResolvedConfig,
    walked: &[WalkedFile],
) -> CommenterCatResult<(Vec<Comment>, Vec<FileSource>)> {
    let marker_set = MarkerSet::new(&config.markers.custom);
    let per_file = walked
        .par_iter()
        .map(|file| native_pass_file(root, file, &marker_set))
        .collect::<CommenterCatResult<Vec<(Vec<Comment>, FileSource)>>>()?;
    let mut comments = Vec::new();
    let mut sources = Vec::with_capacity(per_file.len());
    for (file_comments, source) in per_file {
        comments.extend(file_comments);
        sources.push(source);
    }
    Ok((comments, sources))
}

/// Runs one walked file through the native pass: read → extract → coalesce → map
/// → tag markers. Pure per-file work — no shared mutable state — so it is safe to
/// fan out across threads (each call builds its own tree-sitter parser). Returns
/// the file's comments and its source text (for the rot pass).
fn native_pass_file(
    root: &Path,
    file: &WalkedFile,
    marker_set: &MarkerSet,
) -> CommenterCatResult<(Vec<Comment>, FileSource)> {
    let source = std::fs::read_to_string(&file.path).map_err(|e| {
        CommenterCatError::extract(format!("reading {}", file.path.display())).caused_by(e)
    })?;
    let repo_path = repo_relative(&file.path, root);
    let mut file_comments = extract_source(&source, file.language, &file.path, &repo_path)?;
    file_comments = coalesce(&source, file_comments);
    map_comments(&source, file.language, &file.path, &mut file_comments)?;
    for comment in &mut file_comments {
        marker_set.tag(comment);
    }
    let file_source = FileSource {
        path: repo_path,
        language: file.language,
        text: source,
    };
    Ok((file_comments, file_source))
}

/// Builds the git [`BlameIndex`] for the comments and joins blame onto each
/// (stage 2.6). Git-drift needs blame, but a missing/empty repository is a
/// graceful no-op, not a failure (Idea §5): degrade to an empty index and let
/// git-drift skip.
fn blame_index(root: &Path, comments: &mut [Comment]) -> BlameIndex {
    let Ok(repo) = Repo::discover(root) else {
        return BlameIndex::new();
    };
    enrich(&repo, comments).unwrap_or_default()
}

/// Fuses native + provider findings onto `comments` (Idea §4, §5). Extracted so
/// it is unit-testable with mock providers, independent of the filesystem walk.
/// `files` is the comment-language set (each tagged with its language); every
/// provider is handed only the subset matching its declared languages. `universe`
/// is the `commenter_cat_scope` set (every non-ignored file) provider findings are
/// validated against.
#[allow(clippy::too_many_arguments)]
fn fuse(
    root: &Path,
    config: &ResolvedConfig,
    mut comments: Vec<Comment>,
    file_sources: Vec<FileSource>,
    files: Vec<(PathBuf, Language)>,
    universe: Vec<String>,
    blames: &BlameIndex,
    providers: &[&dyn RuleProvider],
    use_cache: bool,
) -> CommenterCatResult<CheckResult> {
    let fuse_start = Instant::now();
    // 1. Native findings attach to their own comment: the silent-rot detectors
    //    (run once over all comments, in a fixed deterministic order) plus marker
    //    triage. The cosmetic fingerprint (the §4 identity field) is filled in so
    //    the persisted record, the baseline diff, and cross-scan identity have it.
    let rot_findings = if rot_enabled(&config.rot) {
        let repo_paths = RepoPaths::from_paths(&universe);
        rot_pass(
            &comments,
            &file_sources,
            &repo_paths,
            blames,
            &DeterministicEmbedder,
            &config.rot,
        )
    } else {
        Vec::new()
    };
    for (index, comment) in comments.iter_mut().enumerate() {
        comment.cosmetic_fingerprint = Some(cosmetic_fingerprint(&comment.raw_text));
        let mut native = triage::marker_findings(comment, &config.markers.severity);
        if let Some(rot) = rot_findings.get(index) {
            native.extend(rot.iter().cloned());
        }
        comment.findings.extend(native);
    }

    // 2. Run every provider through the result cache (Idea §6: a provider never
    //    re-runs on unchanged input — the load-bearing performance lever), and
    //    record each run state (Idea §5: state, not exit code).
    let context = ProviderContext::new(root, &config.severity.overrides);
    let providers_start = Instant::now();
    // The full comment-language path set keys the file-scoped provider cache; each
    // provider is then handed only the files matching its declared languages.
    let all_files: Vec<PathBuf> = files.iter().map(|(path, _)| path.clone()).collect();
    let cache =
        provider_cache::ProviderCache::build(root, &all_files, &universe, providers, use_cache);
    let mut provider_findings = Vec::new();
    let mut run_states = Vec::new();
    let mut cache_stats = provider_cache::CacheStats::default();
    for provider in providers {
        // Idea §5: the orchestrator narrows each provider's file set to the
        // languages it declares, so shellcheck never lints a `.py` file. A
        // provider whose language has no files here gets an empty set → SKIPPED.
        let provider_files = files_for_provider(&files, provider.languages());
        let (run, was_hit) = cache.run(*provider, &provider_files, &context);
        if was_hit {
            cache_stats.hits += 1;
        } else {
            cache_stats.runs += 1;
        }
        if provider.capabilities().comment_scoped {
            // Comment-scoped (gitleaks): a hit counts only when it sits inside an
            // extracted comment span — a secret *in a comment*, not in code or a
            // build artifact. Comments exist only for walked files (⊆
            // `commenter_cat_scope`), so comment-membership already implies in-scope.
            // The state is recomputed from what survives, so a provider whose every
            // hit was outside comments reads EMPTY rather than a misleading SUCCESS;
            // PARTIAL/SKIPPED are trust signals and pass through (Idea §5). Survivors
            // attach in step 3 — `within_comment_span` IS the attach-by-location test.
            let kept: Vec<Finding> = run
                .findings
                .into_iter()
                .filter(|finding| {
                    comments
                        .iter()
                        .any(|comment| normalize::within_comment_span(comment, finding))
                })
                .collect();
            let state = match run.state {
                RunState::Partial | RunState::Skipped => run.state,
                RunState::Success | RunState::Empty => {
                    if kept.is_empty() {
                        RunState::Empty
                    } else {
                        RunState::Success
                    }
                }
            };
            run_states.push((provider.id().to_owned(), state));
            provider_findings.extend(kept);
        } else {
            run_states.push((provider.id().to_owned(), run.state));
            provider_findings.extend(run.findings);
        }
    }
    let providers_time = providers_start.elapsed();

    // Commenter-Cat owns the file universe (Idea §3, §5): `commenter_cat_scope` is *every*
    // non-ignored file, not just the comment-language subset — so a (non-comment-
    // scoped) project tool keeps a hit it finds in `.env`/config (surfaced as
    // unattached, since Commenter-Cat extracts no comments there), while a hit in a
    // gitignored/excluded path (node_modules/.venv) is dropped. A provider only
    // vetoes within `commenter_cat_scope`, never widens it. (Comment-scoped providers
    // like gitleaks were already narrowed to comment spans in the loop above.)
    let in_scope: HashSet<String> = universe.into_iter().collect();
    provider_findings.retain(|finding| in_scope.contains(&finding.file));

    // 3. Attach provider findings + dedup every comment's fused set.
    let unattached = normalize::attach_findings(&mut comments, provider_findings);
    normalize::dedup_comment_findings(&mut comments);

    let timings = Timings {
        providers: providers_time,
        fusion: fuse_start.elapsed().saturating_sub(providers_time),
        ..Timings::default()
    };
    Ok(CheckResult {
        comments,
        run_states,
        unattached,
        cache_stats,
        timings,
        suppressed: Vec::new(),
    })
}

/// The walked files a provider should receive: those whose language is in its
/// declared `languages`. An empty `languages` means **no affinity** — every file
/// (a user adapter that declares none, and project/`{root}` tools like gitleaks
/// that ignore the explicit list). Idea §5: narrowing is the orchestrator's job,
/// so a provider never sees a file in a language it cannot lint.
fn files_for_provider(files: &[(PathBuf, Language)], languages: &[Language]) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|(_, language)| languages.is_empty() || languages.contains(language))
        .map(|(path, _)| path.clone())
        .collect()
}

/// Whether any `[rot]` detector is enabled — when all are off the rot pass (and
/// its symbol-index build) is skipped entirely.
fn rot_enabled(rot: &RotConfig) -> bool {
    rot.reference_liveness
        || rot.signature_contract
        || rot.path_existence
        || rot.git_drift
        || rot.semantic_contradiction
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;
    use crate::provider::{Capabilities, ProviderRun, Scope};
    use crate::testutil::TestRepo;
    use commenter_cat_core::finding::{
        Category, CoordinateSystem, FindingTarget, Fix, Origin, Range,
    };
    use commenter_cat_core::severity::Severity;
    use commenter_cat_core::symbol::{BoundSymbol, CommentId};

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
                    comment_scoped: false,
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

    /// A provider that reports a finding for a file Commenter-Cat never walked (e.g. a
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
        // Commenter-Cat owns the universe: a finding in a file Commenter-Cat did not walk is dropped
        // entirely — not attached, not even surfaced as unattached (Idea §5).
        let repo = TestRepo::new();
        repo.write("pkg/m.py", "x = 1\n");
        let oos = OutOfScopeProvider {
            capabilities: Capabilities {
                scope: Scope::Project,
                supports_fix: false,
                supports_incremental: false,
                supports_sarif: false,
                comment_scoped: false,
                coordinate_system: CoordinateSystem::tree_sitter(),
            },
        };
        let providers: [&dyn RuleProvider; 1] = [&oos];
        let result = check(repo.path(), &ResolvedConfig::default(), &providers, false).unwrap();

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

    /// A project-scoped, *non*-comment-scoped secrets mock reporting a `secret`
    /// for each named file — to prove the `commenter_cat_scope` universe veto keeps
    /// config-file hits (`.env`) but drops gitignored ones, independent of the
    /// comment-scoping real gitleaks now layers on top (see the test below).
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
                    comment_scoped: false,
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
        // `commenter_cat_scope` is the non-ignored universe, NOT just comment-language files:
        // a secret in `.env` (config Commenter-Cat does not comment-analyze, Idea §3) is kept
        // and surfaced as unattached, while a secret in a gitignored path is
        // dropped (Idea §5 — a provider vetoes within `commenter_cat_scope`, never widens it).
        let repo = TestRepo::new();
        repo.write("pkg/m.py", "x = 1\n"); // a source file, no comments
        repo.write(".env", "APP_ENV=local\n"); // config dotfile — in scope
        repo.write(".gitignore", "vendored/\n");
        repo.write("vendored/leak.py", "VALUE = 1\n"); // gitignored — out of scope

        let provider = SecretScanProvider::new(&[".env", "vendored/leak.py"]);
        let providers: [&dyn RuleProvider; 1] = [&provider];
        let result = check(repo.path(), &ResolvedConfig::default(), &providers, false).unwrap();

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

    /// A **comment-scoped** secret scanner (gitleaks-shaped): it reports two
    /// secrets in one file — one whose byte lands inside the file's comment, one
    /// out in the code — so the test can prove comment-scoping keeps the former
    /// and drops the latter entirely (Idea §5).
    struct CommentScopedSecretProvider {
        capabilities: Capabilities,
        emit_comment_hit: bool,
        emit_code_hit: bool,
    }

    impl CommentScopedSecretProvider {
        fn new(emit_comment_hit: bool, emit_code_hit: bool) -> Self {
            Self {
                capabilities: Capabilities {
                    scope: Scope::Project,
                    supports_fix: false,
                    supports_incremental: false,
                    supports_sarif: false,
                    comment_scoped: true,
                    coordinate_system: CoordinateSystem::tree_sitter(),
                },
                emit_comment_hit,
                emit_code_hit,
            }
        }
    }

    impl RuleProvider for CommentScopedSecretProvider {
        fn id(&self) -> &str {
            "gitleaks"
        }
        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }
        fn run(&self, _files: &[PathBuf], _ctx: &ProviderContext<'_>) -> ProviderRun {
            let secret = |range: Range, message: &str| Finding {
                file: "pkg/m.py".to_owned(),
                target: FindingTarget::Symbol(BoundSymbol::new("pkg/m.py")),
                range,
                origin: Origin::Other("gitleaks".to_owned()),
                provider_rule_id: "gitleaks:generic-api-key".to_owned(),
                canonical_rule_id: "generic-api-key".to_owned(),
                category: Category::Secret,
                severity: Severity::Critical,
                severity_native: None,
                message: message.to_owned(),
                fix: Fix::None,
                url: None,
                also_from: Default::default(),
            };
            // File `# api key\nTOKEN = "x"\n`: the comment spans bytes 0..9, code
            // begins at byte 10. The comment hit sits on the `#` (byte 0), the code
            // hit on `TOKEN` (byte 12).
            let mut hits = Vec::new();
            if self.emit_comment_hit {
                hits.push(secret(Range::new(0, 0, 1, 1), "secret in a comment"));
            }
            if self.emit_code_hit {
                hits.push(secret(Range::new(12, 12, 2, 2), "secret in code"));
            }
            ProviderRun::ran(hits)
        }
    }

    #[test]
    fn test_comment_scoped_provider_keeps_secret_in_comment_drops_code() {
        // gitleaks is comment-scoped (Idea §5): a secret *inside a comment*
        // attaches and surfaces; a secret out in code is dropped at fusion — not
        // attached, and crucially not surfaced as unattached either.
        let repo = TestRepo::new();
        repo.write("pkg/m.py", "# api key\nTOKEN = \"x\"\n");
        let provider = CommentScopedSecretProvider::new(true, true);
        let providers: [&dyn RuleProvider; 1] = [&provider];

        let result = check(repo.path(), &ResolvedConfig::default(), &providers, false).unwrap();

        assert!(
            result.unattached.is_empty(),
            "the in-code secret is dropped, never surfaced as unattached"
        );
        let secrets: Vec<&Finding> = result
            .comments
            .iter()
            .flat_map(|comment| &comment.findings)
            .filter(|finding| finding.category == Category::Secret)
            .collect();
        assert_eq!(
            secrets.len(),
            1,
            "only the secret sitting inside the comment survives"
        );
        assert_eq!(secrets[0].message, "secret in a comment");
        // One hit survived the comment filter → SUCCESS.
        assert_eq!(
            result.run_states,
            vec![("gitleaks".to_owned(), RunState::Success)]
        );
    }

    #[test]
    fn test_comment_scoped_code_only_hit_recomputes_to_empty() {
        // The original puzzle: gitleaks *ran* and found a secret (raw SUCCESS), but
        // it was out in code (or a build artifact), not a comment. A comment-scoped
        // run drops it and recomputes the state to EMPTY — no misleading SUCCESS,
        // nothing surfaced (Idea §5).
        let repo = TestRepo::new();
        repo.write("pkg/m.py", "# api key\nTOKEN = \"x\"\n");
        let provider = CommentScopedSecretProvider::new(false, true);
        let providers: [&dyn RuleProvider; 1] = [&provider];

        let result = check(repo.path(), &ResolvedConfig::default(), &providers, false).unwrap();

        assert_eq!(
            result.run_states,
            vec![("gitleaks".to_owned(), RunState::Empty)],
            "a raw SUCCESS with no in-comment hit recomputes to EMPTY"
        );
        assert!(result.unattached.is_empty());
        assert!(
            result
                .comments
                .iter()
                .flat_map(|comment| &comment.findings)
                .all(|finding| finding.category != Category::Secret),
            "the code secret never surfaces"
        );
    }

    #[test]
    fn test_check_fuses_provider_and_native_findings() {
        let repo = TestRepo::new();
        // `# TODO` triggers native marker triage; the mock adds a provider finding.
        repo.write("pkg/m.py", "# TODO clean this up\nx = 1\n");
        let config = ResolvedConfig::default();
        let mock = MockProvider::new();
        let providers: [&dyn RuleProvider; 1] = [&mock];

        let result = check(repo.path(), &config, &providers, false).unwrap();

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

    /// A provider that counts its invocations and carries a fixed `version_key`,
    /// so a test can prove the result cache skipped a re-run (Idea §6).
    struct CountingProvider {
        capabilities: Capabilities,
        runs: AtomicUsize,
    }

    impl CountingProvider {
        fn project() -> Self {
            Self {
                capabilities: Capabilities {
                    scope: Scope::Project,
                    supports_fix: false,
                    supports_incremental: false,
                    supports_sarif: true,
                    comment_scoped: false,
                    coordinate_system: CoordinateSystem::tree_sitter(),
                },
                runs: AtomicUsize::new(0),
            }
        }

        fn run_count(&self) -> usize {
            self.runs.load(Ordering::SeqCst)
        }
    }

    impl RuleProvider for CountingProvider {
        fn id(&self) -> &str {
            "counting"
        }
        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }
        fn version_key(&self) -> Option<String> {
            Some("v-test".to_owned())
        }
        fn run(&self, _files: &[PathBuf], _ctx: &ProviderContext<'_>) -> ProviderRun {
            self.runs.fetch_add(1, Ordering::SeqCst);
            ProviderRun::ran(vec![Finding {
                file: "pkg/m.py".to_owned(),
                target: FindingTarget::Symbol(BoundSymbol::new("pkg/m.py")),
                range: Range::new(0, 4, 1, 1),
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
            }])
        }
    }

    #[test]
    fn test_provider_cache_skips_rerun_on_unchanged_input() {
        // Two checks over an unchanged tree: the project-scoped provider runs once,
        // then is served from the content-addressed cache (Idea §6).
        let repo = TestRepo::new();
        repo.write("pkg/m.py", "# c\nx = 1\n");
        let provider = CountingProvider::project();
        let providers: [&dyn RuleProvider; 1] = [&provider];

        let first = check(repo.path(), &ResolvedConfig::default(), &providers, true).unwrap();
        let second = check(repo.path(), &ResolvedConfig::default(), &providers, true).unwrap();

        assert_eq!(provider.run_count(), 1, "the second check is a cache hit");
        assert_eq!(
            first.cache_stats.runs, 1,
            "the first check ran the provider"
        );
        assert_eq!(second.cache_stats.hits, 1, "the second check hit the cache");
        // The cached findings come back: the secret is present on both runs.
        let has_secret = |result: &CheckResult| {
            result.comments.iter().any(|comment| {
                comment
                    .findings
                    .iter()
                    .any(|f| f.category == Category::Secret)
            })
        };
        assert!(has_secret(&first) && has_secret(&second));
    }

    #[test]
    fn test_provider_cache_invalidates_on_content_change() {
        let repo = TestRepo::new();
        repo.write("pkg/m.py", "# c\nx = 1\n");
        let provider = CountingProvider::project();
        let providers: [&dyn RuleProvider; 1] = [&provider];

        check(repo.path(), &ResolvedConfig::default(), &providers, true).unwrap();
        // A change to a commenter_cat_scope file shifts the tree hash → cache miss → re-run.
        repo.write("pkg/m.py", "# changed\nx = 2\n");
        check(repo.path(), &ResolvedConfig::default(), &providers, true).unwrap();

        assert_eq!(
            provider.run_count(),
            2,
            "a content change re-runs the provider"
        );
    }

    #[test]
    fn test_no_cache_always_reruns() {
        let repo = TestRepo::new();
        repo.write("pkg/m.py", "# c\nx = 1\n");
        let provider = CountingProvider::project();
        let providers: [&dyn RuleProvider; 1] = [&provider];

        check(repo.path(), &ResolvedConfig::default(), &providers, false).unwrap();
        check(repo.path(), &ResolvedConfig::default(), &providers, false).unwrap();

        assert_eq!(provider.run_count(), 2, "use_cache=false never caches");
    }

    /// A file-scoped provider that records the files it was handed, to prove the
    /// orchestrator narrows by declared language before invocation (Idea §5).
    struct RecordingProvider {
        capabilities: Capabilities,
        languages: Vec<Language>,
        seen: Mutex<Vec<PathBuf>>,
    }

    impl RecordingProvider {
        fn new(languages: Vec<Language>) -> Self {
            Self {
                capabilities: Capabilities {
                    scope: Scope::File,
                    supports_fix: false,
                    supports_incremental: true,
                    supports_sarif: false,
                    comment_scoped: false,
                    coordinate_system: CoordinateSystem::tree_sitter(),
                },
                languages,
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl RuleProvider for RecordingProvider {
        fn id(&self) -> &str {
            "recording"
        }
        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }
        fn languages(&self) -> &[Language] {
            &self.languages
        }
        fn run(&self, files: &[PathBuf], _ctx: &ProviderContext<'_>) -> ProviderRun {
            if let Ok(mut seen) = self.seen.lock() {
                seen.extend_from_slice(files);
            }
            ProviderRun::ran(Vec::new())
        }
    }

    #[test]
    fn test_orchestrator_narrows_files_to_provider_languages() {
        // The dogfooded shellcheck bug: a shell-only provider must receive only
        // shell files, never the `.py` (which would mis-fire SC2148). The orchestrator
        // narrows by `languages()` before invoking, so the `.py` never reaches it.
        let repo = TestRepo::new();
        repo.write("a.py", "# c\nx = 1\n");
        repo.write("s.sh", "# c\necho hi\n");

        let shell_only = RecordingProvider::new(vec![Language::Shell]);
        let providers: [&dyn RuleProvider; 1] = [&shell_only];
        check(repo.path(), &ResolvedConfig::default(), &providers, false).unwrap();

        let seen = shell_only.seen.lock().unwrap();
        assert_eq!(
            seen.len(),
            1,
            "the shell-only provider received exactly one file"
        );
        assert!(
            seen[0].extension().is_some_and(|ext| ext == "sh"),
            "and it was the shell file, never the .py: {seen:?}"
        );
    }

    #[test]
    fn test_provider_with_no_language_affinity_sees_every_file() {
        // A provider that declares no languages (empty) keeps the old behavior:
        // it is handed every walked file (gitleaks-style project tools rely on this).
        let repo = TestRepo::new();
        repo.write("a.py", "# c\nx = 1\n");
        repo.write("s.sh", "# c\necho hi\n");

        let any = RecordingProvider::new(Vec::new());
        let providers: [&dyn RuleProvider; 1] = [&any];
        check(repo.path(), &ResolvedConfig::default(), &providers, false).unwrap();

        assert_eq!(
            any.seen.lock().unwrap().len(),
            2,
            "no declared languages → every file is in scope"
        );
    }
}
