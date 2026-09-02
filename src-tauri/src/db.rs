//! Phase R.0 — real SQLite persistence scaffold.
//!
//! This module does two things, and deliberately nothing else yet (no business logic —
//! that's R.1-R.3):
//!   1. Opens a real SQLite database file on disk (not an in-memory `Map`/`HashMap`).
//!   2. Runs the existing `migrations/*.sql` files against it, once each, tracked via a
//!      small `schema_migrations` ledger table so re-launching the app doesn't matter.
//!
//! KNOWN GAP FLAGGED, NOT SILENTLY PATCHED: `server/ipc_handlers.js`'s in-memory
//! `quotesDb` deduplicates/replaces an existing quote by `offer_ref` alone (see
//! `existingIndex = tenantQuotes.findIndex(q => q.offer_ref === quoteData.offer_ref)`
//! in ipc_handlers.js). The real schema's `quotes` table, however, has
//! `UNIQUE(tenant_id, offer_ref, rev_suffix)` — i.e. it treats each `rev_suffix` (e.g.
//! Rev.01 vs Rev.02) as a distinct row, not a replacement of the prior one. Route-map
//! Phase 5.1 ("Document Revision Control System... locking prior approved versions")
//! reads as though per-revision rows are the intended design, and the JS in-memory
//! version's per-offer_ref-only replace looks like it predates that requirement rather
//! than being an intentional simplification. This phase does not resolve that
//! discrepancy — it isn't scoped to R.0 and touching `save_quote` semantics is R.3's
//! job — but it's real and worth the R.3 Worker/Architect pair confirming intent
//! before porting `save_quote`'s upsert logic.

use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Every migration file, in the order it must be applied, embedded at compile time so
/// the running binary never depends on the source tree existing on disk at runtime.
/// Add new entries here as new `migrations/*.sql` files are added in later phases.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "001_core_schema.sql",
        include_str!("../../migrations/001_core_schema.sql"),
    ),
    (
        "002_asset_directory.sql",
        include_str!("../../migrations/002_asset_directory.sql"),
    ),
];

/// Resolves the on-disk path for the real SQLite database file, inside the app's
/// platform-appropriate data directory (so it survives app restarts and doesn't require
/// write access to the install location).
pub fn resolve_db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("borge_equipment_rental.sqlite3")
}

/// Opens (creating if absent) the SQLite file at `db_path`, enables foreign key
/// enforcement (off by default in SQLite — required for migrations/002's composite-FK
/// tenant-isolation constraints to actually do anything), and applies any migration
/// not yet recorded in `schema_migrations`.
pub fn init_db(db_path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some(format!(
                    "could not create app data directory {:?}: {}",
                    parent, e
                )),
            )
        })?;
    }

    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    run_migrations(&conn)?;
    Ok(conn)
}

fn run_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            filename VARCHAR(255) PRIMARY KEY,
            applied_at VARCHAR(30) NOT NULL
        );",
    )?;

    for (filename, sql) in MIGRATIONS {
        let already_applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE filename = ?1)",
            [filename],
            |row| row.get(0),
        )?;
        if already_applied {
            continue;
        }

        // Each migration file mixes engine-neutral DDL (Section A) with SQLite-only
        // triggers (Section B) and PostgreSQL-only DDL that's already commented out
        // with `--` (Section C) — safe to execute the whole file verbatim as a batch
        // against a SQLite connection.
        conn.execute_batch(sql)?;
        conn.execute(
            "INSERT INTO schema_migrations (filename, applied_at) VALUES (?1, ?2)",
            rusqlite::params![filename, current_timestamp()],
        )?;
    }

    Ok(())
}

fn current_timestamp() -> String {
    // Matches the migrations' own VARCHAR(30) ISO-8601-with-millis convention
    // (strftime('%Y-%m-%dT%H:%M:%fZ', 'now') in the SQLite trigger sections), without
    // pulling in a datetime crate for one timestamp column — computed via SQLite itself
    // so the format is guaranteed identical to what the triggers produce.
    // (Kept here as a tiny helper rather than inline at each call site.)
    let conn = Connection::open_in_memory().expect("in-memory connection for timestamp helper");
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
        row.get(0)
    })
    .expect("strftime timestamp query")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Throwaway per-test DB path so parallel test threads don't collide on one file.
    fn temp_db_path(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "borge_phase_r0_test_{}_{}_{}.sqlite3",
            label,
            std::process::id(),
            n
        ))
    }

    /// Phase R.0 Completion Check: "A row can be written to and read back from the
    /// real SQLite file via a throwaway Rust test — proving the connection is
    /// functional, not just configured." This is that test.
    #[test]
    fn writes_and_reads_back_a_real_row_from_disk() {
        let path = temp_db_path("write_read");
        let _ = std::fs::remove_file(&path);

        let conn = init_db(&path).expect("init_db should open and migrate a fresh file");

        conn.execute(
            "INSERT INTO tenants (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            rusqlite::params!["tenant-r0-test", "Phase R.0 Test Tenant", current_timestamp()],
        )
        .expect("insert should succeed against the real schema");

        drop(conn);

        // Re-open the same file path fresh (simulating an app restart) and confirm the
        // row is actually persisted on disk, not just visible within one connection.
        let conn2 = init_db(&path).expect("re-opening the same file should not re-run migrations");
        let name: String = conn2
            .query_row(
                "SELECT name FROM tenants WHERE id = ?1",
                ["tenant-r0-test"],
                |row| row.get(0),
            )
            .expect("row should be readable back from the real file");
        assert_eq!(name, "Phase R.0 Test Tenant");

        drop(conn2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn foreign_keys_pragma_is_actually_on() {
        let path = temp_db_path("fk_pragma");
        let _ = std::fs::remove_file(&path);
        let conn = init_db(&path).expect("init_db should succeed");
        let fk_enabled: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("pragma read");
        assert_eq!(fk_enabled, 1, "foreign_keys must be ON for migrations/002's tenant-isolation FKs to enforce anything");
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migrations_are_not_reapplied_on_second_open() {
        let path = temp_db_path("no_reapply");
        let _ = std::fs::remove_file(&path);

        let conn = init_db(&path).expect("first open should apply migrations");
        let applied_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("count migrations");
        assert_eq!(applied_count, MIGRATIONS.len() as i64);
        drop(conn);

        // Re-open: CREATE TABLE/TRIGGER IF NOT EXISTS would be harmless even if
        // re-run, but the ledger should still report each migration applied exactly
        // once, not growing on every launch.
        let conn2 = init_db(&path).expect("second open should skip already-applied migrations");
        let applied_count2: i64 = conn2
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("count migrations again");
        assert_eq!(applied_count2, MIGRATIONS.len() as i64);

        drop(conn2);
        let _ = std::fs::remove_file(&path);
    }
}