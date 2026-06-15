//! Provider management & reproducibility (Idea §5; task 6.7).
//!
//! The promise is **same source + same config ⇒ same findings**. This layer
//! delivers it: [`pinning`] (pinned-by-default sourcing + cache layout),
//! [`config_fingerprint`] (hashing the resolved effective config), and
//! [`doctor`] (validating version *and* config against the committed baseline,
//! the comparability key `(cf_ruleset_version, provider_version, config_hash)`).

pub mod config_fingerprint;
pub mod doctor;
pub mod pinning;

pub use config_fingerprint::config_hash;
pub use doctor::{diagnose, DoctorReport, ProviderState};
pub use pinning::{provider_cache_dir, provider_path, PinnedVersion, ProviderSource};
