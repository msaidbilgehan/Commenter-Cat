//! The `RuleProvider` trait and invocation contract (Idea §5; task 6.1).
//!
//! How `cf` drives any analyzer: one subprocess per provider, batched over the
//! cache-miss file set, parallel across providers, JSON-only I/O, with a
//! declared [`Scope`] and [`Capabilities`]. Manifest (Tier 1) and native
//! (Tier 2) providers both implement [`RuleProvider`]; it is the seam mocked in
//! tests — never the parser or the DB (Idea §11).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use cf_core::finding::coordinates::CoordinateSystem;
use cf_core::finding::Finding;
use cf_core::severity::Severity;

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
    /// Whether `cf fix` delegates to the tool's own `--fix`.
    pub supports_fix: bool,
    /// Whether the tool can run incrementally on a file subset.
    pub supports_incremental: bool,
    /// Whether the tool can emit SARIF (mapped by the generic ingester).
    pub supports_sarif: bool,
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

/// How `cf` drives an analyzer (Idea §5). The orchestrator reasons from declared
/// [`Capabilities`], not hardcoded per-tool knowledge.
pub trait RuleProvider {
    /// The provider id (e.g. `"ruff"`, `"eslint"`).
    fn id(&self) -> &str;

    /// The provider's declared capabilities.
    fn capabilities(&self) -> &Capabilities;

    /// Runs over `files` (already narrowed to `effective_scope`), producing
    /// normalized findings and a run state.
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
