//! Manifest `[capabilities]` (Idea §5; task 6.4).
//!
//! `[capabilities]` is the single declarative source for the §5 invocation
//! behavior — the orchestrator reasons from *declared* capabilities, not
//! hardcoded per-tool knowledge. `coordinate_system` routes to the Phase-3
//! byte-offset conversion.

use serde::Deserialize;

use cf_core::finding::coordinates::CoordinateSystem;

use crate::provider::contract::{Capabilities, Scope};

/// The `[capabilities]` block of a manifest (Idea §5).
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestCapabilities {
    /// Whether `cf fix` delegates to the tool's own `--fix`.
    #[serde(default)]
    pub supports_fix: bool,
    /// Whether the tool runs incrementally on a file subset.
    #[serde(default)]
    pub supports_incremental: bool,
    /// Whether the tool emits SARIF.
    #[serde(default)]
    pub supports_sarif: bool,
    /// The tool's declared coordinate convention (routes to Phase-3 conversion).
    pub coordinate_system: CoordinateSystem,
}

impl ManifestCapabilities {
    /// Builds the orchestrator-facing [`Capabilities`] using the manifest's scope.
    #[must_use]
    pub fn to_capabilities(&self, scope: Scope) -> Capabilities {
        Capabilities {
            scope,
            supports_fix: self.supports_fix,
            supports_incremental: self.supports_incremental,
            supports_sarif: self.supports_sarif,
            coordinate_system: self.coordinate_system,
        }
    }
}
