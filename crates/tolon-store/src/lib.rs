//! SQLite schema, migrations and repositories.

use std::path::Path;

use refinery::Runner;
use rusqlite::Connection;

mod settings;

pub use settings::SettingsRepo;

mod embedded {
    refinery::embed_migrations!("migrations");
}

/// Everything that can go wrong in the store. Each message is a sentence fit for the UI.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A migration failed; the app must show a dialog and stop. Its transaction was rolled back.
    #[error("Tolon could not update its database, so it stopped to protect your data.")]
    Migration(#[source] refinery::Error),
    #[error("Tolon could not read or write its database.")]
    Database(#[from] rusqlite::Error),
    #[error("A saved setting is not valid JSON.")]
    Json(#[from] serde_json::Error),
}

/// Applied versus expected migrations, for the startup check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationStatus {
    pub applied: i32,
    pub expected: i32,
}

impl MigrationStatus {
    /// True when the database has every migration this build knows.
    pub fn is_current(&self) -> bool {
        self.applied == self.expected
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the database file, enables WAL and foreign keys, and migrates.
    pub fn open(path: impl AsRef<Path>) -> Result<Store, StoreError> {
        let conn = Connection::open(path)?;
        // journal_mode returns a row, so it must be read rather than executed.
        conn.query_row("pragma journal_mode = wal", [], |_| Ok(()))?;
        Self::init(conn, embedded::migrations::runner())
    }

    pub fn open_in_memory() -> Result<Store, StoreError> {
        Self::init(
            Connection::open_in_memory()?,
            embedded::migrations::runner(),
        )
    }

    fn init(mut conn: Connection, runner: Runner) -> Result<Store, StoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        runner.run(&mut conn).map_err(StoreError::Migration)?;
        Ok(Store { conn })
    }

    /// Highest applied migration version, 0 for an empty database.
    pub fn schema_version(&self) -> Result<i32, StoreError> {
        let version: Option<i32> = self.conn.query_row(
            "select max(version) from refinery_schema_history",
            [],
            |r| r.get(0),
        )?;
        Ok(version.unwrap_or(0))
    }

    pub fn migration_status(&self) -> Result<MigrationStatus, StoreError> {
        let expected = embedded::migrations::runner()
            .get_migrations()
            .iter()
            .map(|m| m.version())
            .max()
            .unwrap_or(0);
        Ok(MigrationStatus {
            applied: self.schema_version()?,
            expected,
        })
    }

    pub fn settings(&self) -> SettingsRepo<'_> {
        SettingsRepo::new(&self.conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use refinery::Migration;
    use std::collections::BTreeSet;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn columns(store: &Store, table: &str) -> Result<Vec<String>, rusqlite::Error> {
        let mut stmt = store
            .conn
            .prepare(&format!("select name from pragma_table_info('{table}')"))?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    }

    #[test]
    fn migrations_apply_to_empty_db() -> TestResult {
        let store = Store::open_in_memory()?;
        assert_eq!(store.schema_version()?, 1);
        assert_eq!(
            store.migration_status()?,
            MigrationStatus {
                applied: 1,
                expected: 1
            }
        );
        assert!(store.migration_status()?.is_current());
        Ok(())
    }

    #[test]
    fn all_five_tables_have_expected_columns() -> TestResult {
        let store = Store::open_in_memory()?;
        let expected: [(&str, &[&str]); 5] = [
            (
                "calendars",
                &[
                    "id",
                    "kind",
                    "name",
                    "enabled",
                    "url",
                    "account",
                    "color",
                    "etag",
                    "last_sync_at",
                    "last_error",
                    "created_at",
                ],
            ),
            (
                "events",
                &["id", "calendar_id", "uid", "payload", "updated_at"],
            ),
            (
                "alarms",
                &["occurrence_key", "kind", "at", "state", "state_at", "until"],
            ),
            ("settings", &["key", "value"]),
            (
                "sync_log",
                &["id", "calendar_id", "at", "outcome", "detail"],
            ),
        ];
        for (table, cols) in expected {
            assert_eq!(columns(&store, table)?, cols, "table {table}");
        }
        let tables: BTreeSet<String> = store
            .conn
            .prepare(
                "select name from sqlite_master where type = 'table' and name not like 'sqlite_%'",
            )?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        assert!(tables.contains("refinery_schema_history"));
        assert_eq!(tables.len(), 6);
        Ok(())
    }

    #[test]
    fn wal_and_foreign_keys_are_on() -> TestResult {
        let dir = tempfile::tempdir()?;
        let store = Store::open(dir.path().join("tolon.db"))?;
        let mode: String = store
            .conn
            .query_row("pragma journal_mode", [], |r| r.get(0))?;
        let fk: i64 = store
            .conn
            .query_row("pragma foreign_keys", [], |r| r.get(0))?;
        assert_eq!(mode, "wal");
        assert_eq!(fk, 1);
        Ok(())
    }

    #[test]
    fn events_cascade_with_their_calendar() -> TestResult {
        let store = Store::open_in_memory()?;
        let c = &store.conn;
        c.execute(
            "insert into calendars (id, kind, name, created_at) values ('c1', 'ics', 'Work', 'now')",
            [],
        )?;
        c.execute(
            "insert into events (id, calendar_id, uid, payload, updated_at) values ('e1', 'c1', 'u1', '{}', 'now')",
            [],
        )?;
        assert!(c
            .execute(
                "insert into events (id, calendar_id, uid, payload, updated_at) values ('e2', 'nope', 'u2', '{}', 'now')",
                [],
            )
            .is_err());
        c.execute("delete from calendars where id = 'c1'", [])?;
        let n: i64 = c.query_row("select count(*) from events", [], |r| r.get(0))?;
        assert_eq!(n, 0);
        Ok(())
    }

    #[test]
    fn reopening_a_migrated_db_is_a_no_op() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tolon.db");
        Store::open(&path)?
            .settings()
            .set("k", &serde_json::json!(1))?;
        let again = Store::open(&path)?;
        assert_eq!(again.schema_version()?, 1);
        let history: i64 =
            again
                .conn
                .query_row("select count(*) from refinery_schema_history", [], |r| {
                    r.get(0)
                })?;
        assert_eq!(history, 1);
        assert_eq!(again.settings().get("k")?, Some(serde_json::json!(1)));
        Ok(())
    }

    #[test]
    fn broken_migration_errors_and_rolls_back() -> TestResult {
        let mut migrations = embedded::migrations::runner().get_migrations().clone();
        migrations.push(Migration::unapplied(
            "V2__broken",
            "create table half_done (id integer); insert into no_such_table values (1);",
        )?);
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tolon.db");
        let result = Store::init(Connection::open(&path)?, Runner::new(&migrations));
        assert!(matches!(result, Err(StoreError::Migration(_))));

        // V1 stayed applied, V2 left nothing behind, and a normal open still works.
        let store = Store::open(&path)?;
        assert_eq!(store.schema_version()?, 1);
        let leftover: i64 = store.conn.query_row(
            "select count(*) from sqlite_master where name = 'half_done'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(leftover, 0);
        Ok(())
    }

    #[test]
    fn non_database_file_is_an_error() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tolon.db");
        std::fs::write(
            &path,
            b"this is not a sqlite database, just text padding padding padding",
        )?;
        assert!(Store::open(&path).is_err());
        Ok(())
    }
}
