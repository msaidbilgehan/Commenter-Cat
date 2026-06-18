//! The `RuleProvider` trait and invocation contract (Idea §5; task 6.1).
//!
//! How `commenter-cat` drives any analyzer: one subprocess per provider, batched over the
//! cache-miss file set, parallel across providers, JSON-only I/O, with a
//! declared [`Scope`] and [`Capabilities`]. Manifest (Tier 1) and native
//! (Tier 2) providers both implement [`RuleProvider`]; it is the seam mocked in
//! tests — never the parser or the DB (Idea §11).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use commenter_cat_core::finding::coordinates::CoordinateSystem;
use commenter_cat_core::finding::Finding;
use commenter_cat_core::lang::Language;
use commenter_cat_core::severity::Severity;

use super::run_state::RunState;

/// Invocation scope (Idea §5): per-`File` (cache per file, invoke the changed
/// subset) or whole-`Project` (TS type-aware rules; cache by tree hash).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// File-scoped: invoke on the changed file subset.
    File,
    /// Project-scoped: invoke over the whole tree.
    Project,
}

/// Declared provider capabilities — the single declarative source the
/// orchestrator reasons from, never hardcoded per-tool knowledge (Idea §5).
#[derive(Debug, Clone)]
pub struct Capabilities {
    /// File- vs project-scoped invocation.
    pub scope: Scope,
    /// Whether `commenter-cat fix` delegates to the tool's own `--fix`.
    pub supports_fix: bool,
    /// Whether the tool can run incrementally on a file subset.
    pub supports_incremental: bool,
    /// Whether the tool can emit SARIF (mapped by the generic ingester).
    pub supports_sarif: bool,
    /// Whether this provider's findings count **only inside a comment span** — a
    /// secret in a comment, not in code. A hit landing outside every extracted
    /// comment is dropped at fusion (never surfaced as unattached). gitleaks opts
    /// in so Commenter-Cat stays on its one job: comment intelligence (Idea §5).
    pub comment_scoped: bool,
    /// The tool's declared coordinate convention (Idea §5; converted via Phase 3).
    pub coordinate_system: CoordinateSystem,
}

/// Context a provider needs to normalize its output into canonical findings.
pub struct ProviderContext<'a> {
    /// The repo/scan root, for resolving and relativizing file paths.
    pub root: &'a Path,
    /// Resolved `[severity]` overrides (Idea §5 tier-1 of severity resolution).
    pub severity_overrides: &'a BTreeMap<String, Severity>,
}

impl<'a> ProviderContext<'a> {
    /// Builds a context.
    #[must_use]
    pub fn new(root: &'a Path, severity_overrides: &'a BTreeMap<String, Severity>) -> Self {
        Self {
            root,
            severity_overrides,
        }
    }
}

/// The outcome of one provider invocation (Idea §5): exactly one [`RunState`]
/// plus the normalized findings (empty unless `state == Success`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRun {
    /// The resolved run state.
    pub state: RunState,
    /// Normalized findings (in canonical order).
    pub findings: Vec<Finding>,
}

impl ProviderRun {
    /// A run that produced findings or ran empty (classified from the findings).
    #[must_use]
    pub fn ran(findings: Vec<Finding>) -> Self {
        let state = if findings.is_empty() {
            RunState::Empty
        } else {
            RunState::Success
        };
        Self { state, findings }
    }

    /// A run that was intentionally not executed (provider absent / language off).
    #[must_use]
    pub fn skipped() -> Self {
        Self {
            state: RunState::Skipped,
            findings: Vec::new(),
        }
    }

    /// A run that failed (crash / timeout / malformed JSON) — findings
    /// **unavailable**, not zero (Idea §5).
    #[must_use]
    pub fn partial() -> Self {
        Self {
            state: RunState::Partial,
            findings: Vec::new(),
        }
    }
}

/// How `commenter-cat` drives an analyzer (Idea §5). The orchestrator reasons from declared
/// [`Capabilities`], not hardcoded per-tool knowledge.
pub trait RuleProvider {
    /// The provider id (e.g. `"ruff"`, `"eslint"`).
    fn id(&self) -> &str;

    /// The provider's declared capabilities.
    fn capabilities(&self) -> &Capabilities;

    /// The source languages this provider handles (Idea §5). The orchestrator
    /// narrows each provider's file set to these *before* invocation, so a tool
    /// only ever sees files it can lint — shellcheck never receives a `.py` file
    /// (the dogfooded SC2148-on-Python bug). An empty slice means **no affinity**:
    /// every walked file — a user adapter that declares none, and project/`{root}`
    /// tools like gitleaks that ignore the explicit list. The default is empty.
    fn languages(&self) -> &[Language] {
        &[]
    }

    /// Runs over `files` (already narrowed to `effective_scope` and the provider's
    /// declared [`languages`]), producing normalized findings and a run state.
    ///
    /// [`languages`]: RuleProvider::languages
    fn run(&self, files: &[PathBuf], context: &ProviderContext<'_>) -> ProviderRun;

    /// A cache-invalidation key for this provider's results (Idea §6) — combining
    /// the resolved tool binary's content with the provider's config, so a tool
    /// upgrade or a config change invalidates cached findings (Idea §5
    /// comparability). `None` disables caching for this provider — a provider with
    /// no stable external binary (an in-process native, a test mock) or one whose
    /// binary cannot be resolved. The default is `None`.
    fn version_key(&self) -> Option<String> {
        None
    }
}
