//! Content hashing (Idea §4, §6).
//!
//! SHA-256 hex is Commenter-Cat's content-address: the Tier-1 comment identity and the
//! `inputs.db` cache key. Centralized here so every layer hashes identically.

use sha2::{Digest, Sha256};

/// The lowercase hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_vector() {
        // SHA-256("") — a fixed, well-known vector.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_stable_and_distinct() {
        assert_eq!(sha256_hex(b"# hello"), sha256_hex(b"# hello"));
        assert_ne!(sha256_hex(b"# hello"), sha256_hex(b"# world"));
        assert_eq!(sha256_hex(b"x").len(), 64);
    }
}
