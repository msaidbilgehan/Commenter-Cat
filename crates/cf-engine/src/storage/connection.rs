//! SQLite connection management (Idea §6; tasks 4.1, 4.6).
//!
//! Every CF connection is opened through here so it gets the same setup: WAL
//! mode (concurrent readers + the engine as sole writer), a `busy_timeout` to
//! cover hook-vs-foreground contention, and the statically-linked **sqlite-vec**
//! extension registered as a process-wide auto-extension. Because sqlite-vec is
//! compiled into the binary, no per-platform `vec0` artifact is shipped — the
//! native-artifact matrix (Idea §10, risk R1) does not apply to vectors.

use std::ffi::{c_char, c_int};
use std::path::Path;
use std::sync::Once;
use std::time::Duration;

use rusqlite::Connection;

use cf_core::error::{CfError, CfResult};

/// `busy_timeout` for the single-writer / many-reader model (Idea §6).
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

static VEC_REGISTERED: Once = Once::new();

/// Registers sqlite-vec as a SQLite auto-extension exactly once per process.
#[allow(unsafe_code)]
fn register_sqlite_vec() {
    VEC_REGISTERED.call_once(|| {
        // SAFETY: `sqlite3_vec_init` is sqlite-vec's C entry point. Transmuting it
        // to SQLite's auto-extension entry-point type is the documented rusqlite
        // registration pattern, and `Once` guarantees a single registration.
        type SqliteEntryPoint = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> c_int;
        unsafe {
            let init = std::mem::transmute::<*const (), SqliteEntryPoint>(
                sqlite_vec::sqlite3_vec_init as *const (),
            );
            rusqlite::ffi::sqlite3_auto_extension(Some(init));
        }
    });
}

/// Opens (creating if needed) a CF database at `path` with the standard setup.
///
/// # Errors
/// Returns [`CfError::Storage`] if the database cannot be opened or configured.
pub fn open(path: &Path) -> CfResult<Connection> {
    register_sqlite_vec();
    let conn = Connection::open(path).map_err(|e| {
        CfError::storage(format!("opening database at {}", path.display())).caused_by(e)
    })?;
    configure(&conn)?;
    Ok(conn)
}

/// Opens an in-memory CF database (for tests and ephemeral derivations).
///
/// # Errors
/// Returns [`CfError::Storage`] if the database cannot be opened or configured.
pub fn open_in_memory() -> CfResult<Connection> {
    register_sqlite_vec();
    let conn = Connection::open_in_memory()
        .map_err(|e| CfError::storage("opening in-memory database").caused_by(e))?;
    configure(&conn)?;
    Ok(conn)
}

/// Applies WAL, `busy_timeout`, and foreign-key enforcement.
fn configure(conn: &Connection) -> CfResult<()> {
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| CfError::storage("setting busy_timeout").caused_by(e))?;
    // WAL is a no-op ("memory") on in-memory databases, which is fine.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| CfError::storage("enabling WAL").caused_by(e))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| CfError::storage("enabling foreign_keys").caused_by(e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_vec_extension_loads() {
        let conn = open_in_memory().unwrap();
        let version: String = conn
            .query_row("SELECT vec_version()", [], |row| row.get(0))
            .unwrap();
        assert!(version.starts_with('v'), "sqlite-vec loaded: {version}");
    }

    #[test]
    fn test_fts5_is_available() {
        let conn = open_in_memory().unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(body);")
            .expect("FTS5 is compiled into bundled SQLite");
    }

    #[test]
    fn test_foreign_keys_enforced() {
        let conn = open_in_memory().unwrap();
        let on: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(on, 1);
    }
}
