//! Verb dispatch — each CLI verb to its engine entry point (Idea §4a, §5).
//!
//! `check` runs the native pass + built-in providers, renders, and **persists**
//! the unified records into the two-layer index. The index-backed verbs (`query`,
//! `context`, `apply-edit`, `remove`) open that index via a [`Session`] and run
//! the find→understand→update loop. `baseline` snapshots/prunes the committed
//! baseline; `suppressions export` (source-mutating) and `issues sync` (network)
//! return an explanatory error until wired deliberately. Output goes to stdout;
//! the return value is the process exit code (Idea §8).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use cf_core::comment::Comment;
use cf_core::config;
use cf_core::error::{CfError, CfResult};
use cf_core::finding::Origin;
use cf_core::severity::Severity;
use cf_engine::ops::baseline;
use cf_engine::ops::check::CheckResult;
use cf_engine::ops::index::{self, Session};
use cf_engine::ops::{self};
use cf_engine::provider::{builtins, RuleProvider, RunState};
use cf_engine::render;
use cf_engine::surface::{ranking, roundtrip, token_economy};

use super::{BaselineAction, Cli, Command};

/// Cap on the number of distinct unattached rule ids listed in diagnostics, so a
/// pathological repo cannot flood stderr.
const MAX_UNATTACHED_RULES_SHOWN: usize = 12;

/// Runs the parsed CLI, returning the process exit code.
///
/// # Errors
/// Returns [`CfError`] on any unrecoverable failure; `main` renders the cause
/// chain and exits non-zero.
pub(crate) fn run(cli: Cli) -> CfResult<i32> {
    match cli.command {
        Command::Check {
            paths,
            format,
            strict,
        } => run_check(&paths, format.into(), strict),
        Command::Candidates { limit } => run_candidates(limit),
        Command::Doctor => run_doctor(),
        Command::Query {
            query,
            limit,
            cursor,
        } => run_query(&query, limit, cursor),
        Command::Context {
            comment_id,
            with_code,
        } => run_context(&comment_id, with_code),
        Command::ApplyEdit {
            comment_id,
            new_text,
            allow_significant,
        } => run_apply_edit(&comment_id, &new_text, allow_significant),
        Command::Remove {
            comment_id,
            allow_significant,
        } => run_remove(&comment_id, allow_significant),
        Command::Baseline { action } => run_baseline(&action),
        Command::Suppressions { .. } => Err(CfError::config(
            "`cf suppressions export` writes native directives into source and depends on the \
             suppression pass being applied during `cf check` (not yet wired) — see ops::suppress",
        )),
        Command::Mcp => run_mcp(),
        Command::InstallHooks => run_install_hooks(),
        Command::Issues { .. } => Err(CfError::config(
            "`cf issues sync` files/closes tracker issues (network + `gh`, outward-facing) and \
             needs the issue ledger persisted to the index — wire deliberately, see issues::",
        )),
    }
}

/// `cf mcp` — serve the MCP protocol over stdio (the agent-facing product).
fn run_mcp() -> CfResult<i32> {
    cf_engine::mcp::server::serve_stdio()?;
    Ok(render::EXIT_OK)
}

/// `cf install-hooks` — install the non-fatal cache-warmer hooks via
/// `core.hooksPath` (Idea §7).
fn run_install_hooks() -> CfResult<i32> {
    let root = root_for(&[])?;
    let report = cf_engine::hooks::install(&root)?;
    print(&format!(
        "Installed {} warmer hook(s) at {} (core.hooksPath).\n",
        report.installed.len(),
        report.hooks_dir.display(),
    ));
    Ok(render::EXIT_OK)
}

/// `cf check` — the native pass + built-in providers, rendered, with a CI verdict.
fn run_check(
    paths: &[PathBuf],
    format: cf_core::config::OutputFormat,
    strict: bool,
) -> CfResult<i32> {
    let root = root_for(paths)?;
    let config = config::discover(&root)?;
    let providers = builtins::load_all()?;
    let provider_refs: Vec<&dyn RuleProvider> = providers
        .iter()
        .map(|provider| provider as &dyn RuleProvider)
        .collect();

    let result = ops::check::check(&root, &config, &provider_refs)?;
    let rendered = render::render(&result.comments, format)?;
    print(&rendered);

    // Idea §5: a degraded guarantee must be *visible, not silent*. Findings on
    // stdout; provider trouble + symbol-only findings to stderr so the operator
    // never reads a clean report that was actually missing a provider's results.
    report_diagnostics(&result);

    // Persist the unified records into the two-layer index so the find/understand/
    // update verbs resolve against a real index (Idea §6).
    index::persist(&root, &result.comments, &index::default_embedder())?;

    // `--strict` fails on any finding (fail_on lowered to the floor).
    let fail_on = if strict {
        Severity::Info
    } else {
        config.severity.fail_on
    };
    Ok(render::ci_exit_code(&result.comments, fail_on))
}

/// Writes provider run-state and unattached-finding diagnostics to stderr.
///
/// `cf check` reports findings on stdout, but a provider that crashed
/// (`PARTIAL`) or never ran (`SKIPPED`) produces *no* findings — indistinguishable
/// from a clean result unless it is surfaced. Idea §5 makes this non-negotiable:
/// "a degraded guarantee is visible, not silent." Unattached findings (e.g.
/// doc-coverage on an undocumented symbol — a real finding with no comment to
/// hang on) would otherwise be dropped from the comment-centric output entirely.
fn report_diagnostics(result: &CheckResult) {
    for (provider, state) in &result.run_states {
        match state {
            RunState::Partial => eprintln!(
                "cf: warning: provider {provider:?} PARTIAL — it ran but its output was unusable; \
                 its findings are UNAVAILABLE (not zero). Re-run with --strict to fail the build."
            ),
            RunState::Skipped => eprintln!(
                "cf: note: provider {provider:?} skipped (not installed or unavailable) — \
                 its language's deep rules were not checked."
            ),
            RunState::Success | RunState::Empty => {}
        }
    }

    if !result.unattached.is_empty() {
        eprintln!(
            "cf: note: {} provider finding(s) target a symbol with no comment \
             (e.g. doc-coverage on an undocumented item) and are not shown in the comment view:",
            result.unattached.len()
        );
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for finding in &result.unattached {
            *counts
                .entry(finding.canonical_rule_id.as_str())
                .or_insert(0) += 1;
        }
        for (rule, count) in counts.iter().take(MAX_UNATTACHED_RULES_SHOWN) {
            eprintln!("cf:   {rule}: {count}");
        }
    }
}

/// `cf candidates` — the native worklist (rot + markers), ranked and bounded.
fn run_candidates(limit: usize) -> CfResult<i32> {
    let root = root_for(&[])?;
    let config = config::discover(&root)?;
    let no_providers: [&dyn RuleProvider; 0] = [];
    let result = ops::check::check(&root, &config, &no_providers)?;

    // Native findings only — the agent-judged shortlist (Idea §9).
    let mut candidates: Vec<NativeCandidate> = result
        .comments
        .iter()
        .flat_map(|comment| {
            comment
                .findings
                .iter()
                .filter(|finding| finding.origin == Origin::Native)
                .map(move |finding| NativeCandidate::new(&comment.path, finding, comment))
        })
        .collect();

    ranking::rank_by(&mut candidates, |candidate| candidate.priority);
    let view = token_economy::bound(candidates, &token_economy::Budget::limit(limit), 0, |_| 1);

    let mut out = format!("{} candidate(s)", view.total);
    if view.truncated {
        out.push_str(&format!(" (showing {})", view.returned()));
    }
    out.push_str(":\n");
    for candidate in &view.items {
        out.push_str(&format!(
            "  {:<8} {:<28} {}:{}  {}\n",
            candidate.priority.severity.as_str(),
            candidate.rule,
            candidate.file,
            candidate.line,
            candidate.message,
        ));
    }
    print(&out);
    Ok(render::EXIT_OK)
}

/// `cf doctor` — list the providers `cf` would run and their declared contract.
fn run_doctor() -> CfResult<i32> {
    let providers = builtins::load_all()?;
    let mut out = String::from("Providers:\n");
    for provider in &providers {
        let caps = provider.capabilities();
        out.push_str(&format!(
            "  {:<12} scope={:<8} fix={:<5} coords={}\n",
            provider.id(),
            format!("{:?}", caps.scope),
            caps.supports_fix,
            caps.coordinate_system.as_str(),
        ));
    }
    out.push_str("  eslint       (Tier-2 native, task 6.6)\n");
    print(&out);
    Ok(render::EXIT_OK)
}

/// `cf query` — FIND: search the persisted index, ranked + bounded.
fn run_query(query: &str, limit: usize, cursor: Option<usize>) -> CfResult<i32> {
    let session = Session::open(&root_for(&[])?)?;
    let hits = session.query(query, limit + cursor.unwrap_or(0))?;
    let offset = cursor.unwrap_or(0);

    let mut out = format!("{} match(es):\n", hits.len().saturating_sub(offset));
    for (id, comment) in hits.iter().skip(offset) {
        out.push_str(&format!(
            "  [{id}] {}:{}  {}  ({} finding(s))\n      {}\n",
            comment.path,
            comment.range.start_line,
            comment.kind.as_str(),
            comment.findings.len(),
            first_line(&comment.raw_text),
        ));
    }
    print(&out);
    Ok(render::EXIT_OK)
}

/// `cf context` — UNDERSTAND: one comment, with its bound code on request.
fn run_context(comment_id: &str, with_code: bool) -> CfResult<i32> {
    let session = Session::open(&root_for(&[])?)?;
    let comment = resolve_comment(&session, comment_id)?;

    let mut out = format!(
        "{}:{}  {}  bound={}\n{}\n",
        comment.path,
        comment.range.start_line,
        comment.kind.as_str(),
        comment
            .bound_symbol
            .as_ref()
            .map_or("<none>", |s| s.as_str()),
        comment.raw_text,
    );
    for finding in &comment.findings {
        out.push_str(&format!(
            "  - {} [{}] {}\n",
            finding.severity.as_str(),
            finding.provider_rule_id,
            finding.message,
        ));
    }
    if with_code {
        // Bound code is opt-in, never bundled into query (Idea §4a token economy).
        if let Some(range) = comment.bound_node_range {
            let source = session.read_source(&comment)?;
            let (start, end) = (range.start_byte as usize, range.end_byte as usize);
            if let Some(code) = source.get(start..end) {
                out.push_str(&format!("--- bound code ---\n{code}\n"));
            }
        }
    }
    print(&out);
    Ok(render::EXIT_OK)
}

/// `cf apply-edit` — UPDATE: a parse-invariant comment edit, written to disk,
/// then re-checked inline (Idea §4a round-trip).
fn run_apply_edit(comment_id: &str, new_text: &str, allow_significant: bool) -> CfResult<i32> {
    let session = Session::open(&root_for(&[])?)?;
    let comment = resolve_comment(&session, comment_id)?;
    let source = session.read_source(&comment)?;
    let config = config::discover(session.repo_root())?;
    let no_providers: [&dyn RuleProvider; 0] = [];

    let trip = roundtrip::apply_edit_and_recheck(
        &source,
        &comment,
        new_text,
        allow_significant,
        session.repo_root(),
        &config,
        &no_providers,
    )?;
    session.write_source(&comment, &trip.new_source)?;
    print(&edit_report("Edited", &comment, &trip.findings));
    Ok(render::EXIT_OK)
}

/// `cf remove` — UPDATE: remove a comment, written to disk, then re-checked.
fn run_remove(comment_id: &str, allow_significant: bool) -> CfResult<i32> {
    let session = Session::open(&root_for(&[])?)?;
    let comment = resolve_comment(&session, comment_id)?;
    let source = session.read_source(&comment)?;
    let config = config::discover(session.repo_root())?;
    let no_providers: [&dyn RuleProvider; 0] = [];

    let trip = roundtrip::remove_and_recheck(
        &source,
        &comment,
        allow_significant,
        session.repo_root(),
        &config,
        &no_providers,
    )?;
    session.write_source(&comment, &trip.new_source)?;
    print(&edit_report("Removed", &comment, &trip.findings));
    Ok(render::EXIT_OK)
}

/// Resolves a CLI comment-id string to its persisted comment record.
fn resolve_comment(session: &Session, comment_id: &str) -> CfResult<Comment> {
    let id: i64 = comment_id.parse().map_err(|_| {
        CfError::config(format!(
            "invalid comment id {comment_id:?} (expected a number from `cf query`)"
        ))
    })?;
    session.comment(id)?.ok_or_else(|| {
        CfError::config(format!("no comment {id} in the index (re-run `cf check`?)"))
    })
}

/// A short report for an apply/remove round-trip: the re-checked findings inline.
fn edit_report(verb: &str, comment: &Comment, findings: &[cf_core::finding::Finding]) -> String {
    let mut out = format!(
        "{verb} comment at {}:{}.\n",
        comment.path, comment.range.start_line
    );
    if findings.is_empty() {
        out.push_str("Re-check: clean.\n");
    } else {
        out.push_str(&format!("Re-check: {} finding(s):\n", findings.len()));
        for finding in findings {
            out.push_str(&format!(
                "  - {} {}\n",
                finding.severity.as_str(),
                finding.message
            ));
        }
    }
    out
}

/// The first non-empty line of a comment's text, for a one-line summary.
fn first_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
}

/// A native worklist entry, carrying its actionable-first priority key.
struct NativeCandidate {
    file: String,
    line: u32,
    rule: String,
    message: String,
    priority: ranking::Priority,
}

impl NativeCandidate {
    fn new(
        file: &str,
        finding: &cf_core::finding::Finding,
        comment: &cf_core::comment::Comment,
    ) -> Self {
        // Blame age is unknown without git enrichment here; marker weight rises
        // with the comment's marker count (DO_NOT_MERGE etc. surface first).
        let blame_age = comment.git.as_ref().map_or(0, |g| g.committed_unix.max(0));
        Self {
            file: file.to_owned(),
            line: finding.range.start_line,
            rule: finding.canonical_rule_id.clone(),
            message: finding.message.clone(),
            priority: ranking::Priority::new(
                finding.severity,
                blame_age,
                u32::try_from(comment.markers.len()).unwrap_or(u32::MAX),
            ),
        }
    }
}

/// The session root: the first path argument (if any) or the current directory.
fn root_for(paths: &[PathBuf]) -> CfResult<PathBuf> {
    match paths.first() {
        Some(path) => Ok(path.clone()),
        None => std::env::current_dir()
            .map_err(|e| CfError::config("determining the working directory").caused_by(e)),
    }
}

/// Prints to stdout (the CLI is the one place stdout *is* the product).
fn print(text: &str) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
}

/// `cf baseline accept|prune` — manage the committed `comment-finder.baseline.toml`.
///
/// `accept` snapshots the current findings (Tier-2 identities) into the baseline;
/// `prune` drops entries whose findings no longer occur. The baseline lives at the
/// repo root (committed, outside the gitignored cache) and is the diff anchor the
/// CI path consumes (Idea §5).
fn run_baseline(action: &BaselineAction) -> CfResult<i32> {
    let root = root_for(&[])?;
    let config = config::discover(&root)?;
    let providers = builtins::load_all()?;
    let provider_refs: Vec<&dyn RuleProvider> = providers
        .iter()
        .map(|provider| provider as &dyn RuleProvider)
        .collect();
    let result = ops::check::check(&root, &config, &provider_refs)?;
    report_diagnostics(&result);

    let identities = current_identities(&result);
    let path = root.join(baseline::BASELINE_FILENAME);

    match action {
        BaselineAction::Accept => {
            let snapshot = baseline::accept(&identities);
            baseline::save(&snapshot, &path)?;
            print(&format!(
                "Baseline accepted: {} finding identit(ies) snapshotted to {}.\n",
                snapshot.entries.len(),
                path.display(),
            ));
        }
        BaselineAction::Prune => {
            if !path.exists() {
                return Err(CfError::config(format!(
                    "no baseline at {} to prune — run `cf baseline accept` first",
                    path.display()
                )));
            }
            let existing = baseline::load(&path)?;
            let before = existing.entries.len();
            let pruned = baseline::prune(&existing, &identities);
            baseline::save(&pruned, &path)?;
            print(&format!(
                "Baseline pruned: removed {} stale entr(ies); {} remain in {}.\n",
                before - pruned.entries.len(),
                pruned.entries.len(),
                path.display(),
            ));
        }
    }
    Ok(render::EXIT_OK)
}

/// The Tier-2 `(bound_symbol, cosmetic_fingerprint, provider_rule_id)` identity of
/// every current finding — the keys the baseline snapshots and matches on (Idea §5;
/// the CI diff matches both provider and canonical rule ids, so the precise
/// `provider_rule_id` is stored).
fn current_identities(result: &CheckResult) -> Vec<baseline::SuppressedIdentity> {
    let mut identities = Vec::new();
    for comment in &result.comments {
        let symbol = comment.bound_symbol.as_ref().map(|s| s.as_str().to_owned());
        let fingerprint = comment.cosmetic_fingerprint.clone().unwrap_or_default();
        for finding in &comment.findings {
            identities.push((
                symbol.clone(),
                fingerprint.clone(),
                finding.provider_rule_id.clone(),
            ));
        }
    }
    identities
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command as ProcessCommand;

    /// A throwaway directory seeded with one Python file carrying a TODO marker.
    fn seeded_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("m.py"), "# TODO clean this up\nx = 1\n").unwrap();
        // `check` discovers config by walking up; a git repo bounds the walk.
        let _ = ProcessCommand::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(dir.path())
            .status();
        dir
    }

    fn check_dir(dir: &Path, strict: bool) -> i32 {
        run_check(
            &[dir.to_path_buf()],
            cf_core::config::OutputFormat::Jsonl,
            strict,
        )
        .unwrap()
    }

    #[test]
    fn test_baseline_accept_snapshots_and_prune_keeps_live() {
        // `cf baseline accept` snapshots the current findings' Tier-2 identities;
        // pruning against the same findings keeps every entry (nothing is stale).
        let dir = seeded_repo();
        let config = cf_core::config::ResolvedConfig::default();
        let no_providers: [&dyn RuleProvider; 0] = [];
        let result = ops::check::check(dir.path(), &config, &no_providers).unwrap();

        let identities = current_identities(&result);
        assert!(
            !identities.is_empty(),
            "the TODO marker yields a baseline identity"
        );

        let path = dir.path().join(baseline::BASELINE_FILENAME);
        let snapshot = baseline::accept(&identities);
        baseline::save(&snapshot, &path).unwrap();

        let loaded = baseline::load(&path).unwrap();
        assert_eq!(loaded.entries.len(), snapshot.entries.len(), "round-trips");
        let pruned = baseline::prune(&loaded, &identities);
        assert_eq!(
            pruned.entries.len(),
            loaded.entries.len(),
            "no entry is stale when findings are unchanged"
        );
    }

    #[test]
    fn test_check_runs_native_pass_and_exits() {
        let dir = seeded_repo();
        // Default fail_on=error: the TODO marker (warning) does not fail.
        assert_eq!(check_dir(dir.path(), false), render::EXIT_OK);
        // --strict fails on any finding, and the TODO is a finding.
        assert_eq!(check_dir(dir.path(), true), render::EXIT_FINDINGS);
    }

    #[test]
    fn test_candidates_runs_native_worklist() {
        let dir = seeded_repo();
        let prev = std::env::current_dir().unwrap();
        // candidates uses the current directory as the root.
        std::env::set_current_dir(dir.path()).unwrap();
        let code = run_candidates(20);
        std::env::set_current_dir(prev).unwrap();
        assert_eq!(code.unwrap(), render::EXIT_OK);
    }

    #[test]
    fn test_doctor_lists_builtin_providers() {
        assert_eq!(run_doctor().unwrap(), render::EXIT_OK);
    }

    #[test]
    fn test_check_persists_an_index_for_the_read_verbs() {
        // `cf check` must leave a populated index behind so query/context resolve.
        let dir = seeded_repo();
        check_dir(dir.path(), false);
        let session = Session::open(dir.path()).expect("check persisted an index");
        let hits = session.query("clean", 10).unwrap();
        assert!(!hits.is_empty(), "the indexed comment is queryable");
    }
}
