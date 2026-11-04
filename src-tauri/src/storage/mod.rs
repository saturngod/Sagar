//! SQLite project storage, owned exclusively by Rust. The frontend never
//! issues SQL; it goes through commands → services → this module.

pub mod ai_providers;
pub mod ai_selection;
pub mod ai_translate;
pub mod documents;
pub mod migrations;
pub mod projects;
pub mod translations;

use std::path::Path;
use std::time::Duration;

use crate::error::AppError;
use rusqlite::{Connection, Transaction, TransactionBehavior};

/// How long SQLite waits on a locked page/row before returning SQLITE_BUSY.
/// Tauri runs each command on its own `spawn_blocking` thread with its own
/// `Connection`, so an autosave can briefly overlap another save, a chapter
/// load (`set_last_chapter` writes), or an `epubres://` resource lookup.
/// Without this timeout every overlap surfaced immediately as
/// "database is locked" — typically on the second paragraph save.
const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

/// Transparent retries for the residual SQLITE_BUSY window (e.g. two
/// `BEGIN IMMEDIATE` writers racing after the timeout). Sleeps stay short so
/// saves still feel instant.
const BUSY_RETRY_ATTEMPTS: usize = 8;

fn configure(conn: &Connection) -> Result<(), AppError> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    // WAL returns the resulting mode as a row, so read it via query_row.
    let _journal_mode: String =
        conn.query_row("PRAGMA journal_mode = WAL;", [], |row| row.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Starts a write transaction that acquires the write lock up front.
///
/// `unchecked_transaction()` begins DEFERRED: two concurrent writers can both
/// hold SHARED and then deadlock on upgrade, surfacing SQLITE_BUSY on the
/// second paragraph save. IMMEDIATE fails-or-waits at BEGIN (covered by
/// `BUSY_TIMEOUT`) instead of mid-transaction.
pub fn begin_write(conn: &mut Connection) -> Result<Transaction<'_>, AppError> {
    Ok(conn.transaction_with_behavior(TransactionBehavior::Immediate)?)
}

/// True for transient lock contention that is safe to retry with the same
/// arguments. Covers SQLITE_BUSY ("database is locked") and SQLITE_LOCKED.
pub fn is_busy_error(error: &AppError) -> bool {
    if error.code == "storage_busy" {
        return true;
    }
    if error.code != "storage_error" {
        return false;
    }
    let message = error.message.to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database is busy")
        || message.contains("database table is locked")
}

/// Runs `op` to completion, retrying transient lock errors with a short
/// backoff. Non-busy errors return immediately. Used by the paragraph and
/// chapter save paths so a brief overlap never becomes a user-visible error.
pub fn with_busy_retry<T>(mut op: impl FnMut() -> Result<T, AppError>) -> Result<T, AppError> {
    let mut attempt = 0;
    loop {
        match op() {
            Ok(value) => return Ok(value),
            Err(error) if is_busy_error(&error) && attempt + 1 < BUSY_RETRY_ATTEMPTS => {
                attempt += 1;
                // 25ms, 50ms, 100ms, … capped at 400ms.
                let backoff = Duration::from_millis(25 * (1 << attempt.min(4)));
                std::thread::sleep(backoff);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Opens (creating if needed) the project database at `path`.
pub struct StorageInfo {
    pub schema_version: i64,
}

/// Opens (creating if needed) the project database at `path`.
pub fn open_database(path: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    Ok(conn)
}

/// Ensures the app-owned data directory and database exist, applies pending
/// migrations and reports the resulting schema version.
pub fn initialize(app_data_dir: &Path) -> Result<StorageInfo, AppError> {
    std::fs::create_dir_all(app_data_dir)?;
    let mut conn = open_database(&app_data_dir.join("sagar.db"))?;
    let schema_version = migrations::apply(&mut conn)?;
    Ok(StorageInfo { schema_version })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens an isolated in-memory database for migration tests.
    fn open_in_memory() -> Result<Connection, AppError> {
        let conn = Connection::open_in_memory()?;
        configure(&conn)?;
        Ok(conn)
    }

    #[test]
    fn fresh_database_is_migrated_to_latest_version() {
        let mut conn = open_in_memory().expect("open in-memory database");
        let version = migrations::apply(&mut conn).expect("apply migrations");
        assert_eq!(version, migrations::latest_version());

        conn.execute(
            "INSERT INTO app_meta (key, value) VALUES ('greeting', 'hello')",
            [],
        )
        .expect("insert into migrated schema");
    }

    #[test]
    fn applying_migrations_twice_is_idempotent() {
        let mut conn = open_in_memory().expect("open in-memory database");
        migrations::apply(&mut conn).expect("first migration run");
        conn.execute(
            "INSERT INTO app_meta (key, value) VALUES ('key', 'kept')",
            [],
        )
        .expect("insert between migration runs");

        let version = migrations::apply(&mut conn).expect("second migration run");
        assert_eq!(version, migrations::latest_version());

        let value: String = conn
            .query_row("SELECT value FROM app_meta WHERE key = 'key'", [], |row| {
                row.get(0)
            })
            .expect("select preserved value");
        assert_eq!(value, "kept");
    }

    #[test]
    fn file_database_persists_schema_and_data_across_reopen() {
        let dir = tempfile::tempdir().expect("create temp directory");
        let db_path = dir.path().join("persistence-test.db");

        {
            let mut conn = open_database(&db_path).expect("open file database");
            let version = migrations::apply(&mut conn).expect("apply migrations");
            assert_eq!(version, migrations::latest_version());
            conn.execute(
                "INSERT INTO app_meta (key, value) VALUES ('seed', 'one')",
                [],
            )
            .expect("insert durable row");
        }

        let conn = open_database(&db_path).expect("reopen file database");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("read schema version");
        assert_eq!(version, migrations::latest_version());

        let value: String = conn
            .query_row("SELECT value FROM app_meta WHERE key = 'seed'", [], |row| {
                row.get(0)
            })
            .expect("select durable row");
        assert_eq!(value, "one");

        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("read foreign_keys pragma");
        assert_eq!(
            foreign_keys, 1,
            "foreign keys must be enabled on every open"
        );
    }

    #[test]
    fn file_database_waits_on_locked_writes_instead_of_failing() {
        // Regression for "database is locked" on the second paragraph save:
        // two Tauri commands (two connections) overlap briefly. The second
        // writer must wait for the first instead of failing immediately.
        let dir = tempfile::tempdir().expect("create temp directory");
        let db_path = dir.path().join("busy-test.db");
        {
            let conn = open_database(&db_path).expect("open file database");
            conn.execute_batch(
                "CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);
                 INSERT INTO t(id, v) VALUES(1, 'a');",
            )
            .expect("seed table");
        }

        let writer_path = db_path.clone();
        let second = std::thread::spawn(move || {
            let mut conn = open_database(&writer_path).expect("open second connection");
            // Holds RESERVED while the main thread tries to write.
            let tx = begin_write(&mut conn).expect("begin first write");
            tx.execute("UPDATE t SET v='b' WHERE id=1", [])
                .expect("first update");
            std::thread::sleep(std::time::Duration::from_millis(300));
            tx.commit().expect("commit first write");
        });

        // Give the writer a head start so its transaction is open.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut conn = open_database(&db_path).expect("open main connection");
        let result = with_busy_retry(|| {
            let tx = begin_write(&mut conn)?;
            tx.execute("UPDATE t SET v='c' WHERE id=1", [])?;
            tx.commit()?;
            Ok(())
        });
        second.join().expect("writer thread");
        result.expect("overlapping write must wait and succeed, not report database is locked");
        let value: String = conn
            .query_row("SELECT v FROM t WHERE id=1", [], |row| row.get(0))
            .expect("read final value");
        assert_eq!(value, "c");
    }

    #[test]
    fn busy_errors_are_detected_for_retry() {
        let busy = crate::error::AppError::new("storage_error", "database is locked");
        assert!(is_busy_error(&busy));
        let busy_code =
            crate::error::AppError::new("storage_busy", "The library is briefly busy saving.");
        assert!(is_busy_error(&busy_code));
        let conflict = crate::error::AppError::new("revision_conflict", "stale");
        assert!(!is_busy_error(&conflict));
    }
}

pub mod content_server;
