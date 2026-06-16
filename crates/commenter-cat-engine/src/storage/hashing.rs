//! Content-hash cache keying (Idea §6; task 4.3).
//!
//! Cache entries key on **content hash, never path+mtime** — "mtime is a liar"
//! (Idea §6). [`hash_file`] is the authoritative key; [`FileStat`] is a cheap
//! fast-path *hint* only (skip re-hashing when mtime+size are unchanged), never
//! a key.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};

use crate::hash::sha256_hex;

/// The authoritative content hash of a file's bytes (Idea §6 cache key).
///
/// # Errors
/// Returns [`CommenterCatError::Storage`] if the file cannot be read.
pub fn hash_file(path: &Path) -> CommenterCatResult<String> {
    let bytes = fs::read(path).map_err(|e| {
        CommenterCatError::storage(format!("hashing {}", path.display())).caused_by(e)
    })?;
    Ok(sha256_hex(&bytes))
}

/// A cheap filesystem fingerprint — `(modified, size)`. **A hint only**: mtime
/// can change without content changing (and vice versa), so this never keys the
/// cache; it only decides whether re-hashing is worthwhile (Idea §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    /// Last-modified time, if the platform reports it.
    pub modified: Option<SystemTime>,
    /// File size in bytes.
    pub size: u64,
}

impl FileStat {
    /// Reads the fingerprint of a file.
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] if the file cannot be stat-ed.
    pub fn of(path: &Path) -> CommenterCatResult<Self> {
        let meta = fs::metadata(path).map_err(|e| {
            CommenterCatError::storage(format!("stat {}", path.display())).caused_by(e)
        })?;
        Ok(Self {
            modified: meta.modified().ok(),
            size: meta.len(),
        })
    }

    /// Whether content *might* have changed since `previous` — a fast-path hint.
    /// `true` means "re-hash to be sure"; `false` means "almost certainly
    /// unchanged" (Idea §6: the DB is still the source of truth on hash).
    #[must_use]
    pub fn maybe_changed_since(&self, previous: &FileStat) -> bool {
        self != previous
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::Duration;

    #[test]
    fn test_content_hash_changes_with_content_only() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.py");

        fs::write(&path, "# original\n").unwrap();
        let h1 = hash_file(&path).unwrap();

        // An mtime-only change (same bytes) does not change the content hash.
        let later = SystemTime::now() + Duration::from_secs(10_000);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(
            hash_file(&path).unwrap(),
            h1,
            "mtime is not part of the key"
        );

        // A byte change does change the content hash.
        fs::write(&path, "# changed\n").unwrap();
        assert_ne!(hash_file(&path).unwrap(), h1);
    }

    #[test]
    fn test_filestat_hint_detects_size_change() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.py");
        fs::write(&path, "abc").unwrap();
        let before = FileStat::of(&path).unwrap();
        fs::write(&path, "abcdef").unwrap();
        let after = FileStat::of(&path).unwrap();
        assert!(after.maybe_changed_since(&before));
    }
}
