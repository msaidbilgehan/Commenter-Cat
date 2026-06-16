//! Tier-2 native providers (Idea §5).
//!
//! The escape hatch for tools needing *runtime* behavior, not just JSON shaping
//! — the eslint Node stack (the lone documented exception to manifest-first),
//! plus its [`node_runtime`] tiers. Reserved for TS-program management, language
//! servers, and remote/AI-backed analyzers.

pub mod eslint;
pub mod node_runtime;

pub use eslint::EslintProvider;
pub use node_runtime::{NodeRuntime, ReproducibilityLevel};
