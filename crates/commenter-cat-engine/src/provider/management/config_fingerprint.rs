//! Provider config fingerprinting (Idea §5; task 6.7).
//!
//! Findings lose comparability not only on a version bump but when the tool's
//! config changes. The fingerprint hashes the **resolved effective config** —
//! from the tool's own dump (`eslint --print-config`, `ruff check
//! --show-settings`, `tsc --showConfig`) so cascaded `extends`/inherited
//! settings are captured, not one file's bytes (Idea §5). The dump itself is
//! produced at the adapter boundary; this hashes whatever dump it is given.

use crate::hash::sha256_hex;

/// The config hash over a resolved effective-config dump (Idea §5), in the
/// `sha256:…` form used by the baseline `[provider_state]`.
#[must_use]
pub fn config_hash(resolved_config_dump: &str) -> String {
    format!("sha256:{}", sha256_hex(resolved_config_dump.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_change_changes_the_hash() {
        let base = config_hash("line-length = 88\nconvention = google");
        // A whitespace-only difference still changes the bytes → different hash;
        // and any real settings change does too.
        assert_ne!(config_hash("line-length = 100\nconvention = google"), base);
        assert_eq!(config_hash("line-length = 88\nconvention = google"), base);
    }

    #[test]
    fn test_hash_is_prefixed() {
        assert!(config_hash("x").starts_with("sha256:"));
    }
}
