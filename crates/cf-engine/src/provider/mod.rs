//! The manifest-first provider layer (Idea §5).
//!
//! `cf` is a **conductor**, not an analyzer: it drives best-in-class external
//! tools through swappable adapters behind one [`RuleProvider`] trait, then
//! normalizes their output into the canonical [`cf_core::Finding`]. The layer is
//! manifest-first — most integrations are declarative TOML (Tier 1); a compiled
//! native provider is the escape hatch (Tier 2, e.g. the eslint Node stack).
//!
//! * [`contract`] — the trait + invocation contract + capabilities (6.1).
//! * [`discovery`] — `effective_scope = provider_filter(cf_scope)` (6.1).
//! * [`run_state`] — the SUCCESS/EMPTY/PARTIAL/SKIPPED machine (6.2).

pub mod builtins;
pub mod contract;
pub mod discovery;
pub mod management;
pub mod manifest;
pub mod native;
pub mod run_state;

pub use contract::{Capabilities, ProviderContext, ProviderRun, RuleProvider, Scope};
pub use discovery::effective_scope;
pub use run_state::{baseline_state, RunState};
