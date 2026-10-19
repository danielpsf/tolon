//! SQLite schema, migrations and repositories.

use std::path::Path;

use refinery::Runner;
use rusqlite::Connection;

mod alarms;
mod calendars;
mod events;
mod settings;
mod sync_log;

pub use alarms::{AlarmRow, AlarmsRepo, TERMINAL_STATES};
pub use calendars::{Calendar, CalendarsRepo};
pub use events::{Event, EventsRepo, FeedEvent};
pub use settings::SettingsRepo;
pub use sync_log::{SyncLogEntry, SyncLogRepo};

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
    #[error("Tolon could not find that calendar.")]
    NotFound,
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

    pub fn alarms(&self) -> AlarmsRepo<'_> {
        AlarmsRepo::new(&self.conn)
    }

    pub fn calendars(&self) -> CalendarsRepo<'_> {
        CalendarsRepo::new(&self.conn)
    }

    pub fn events(&self) -> EventsRepo<'_> {
        EventsRepo::new(&self.conn)
    }

    pub fn sync_log(&self) -> SyncLogRepo<'_> {
        SyncLogRepo::new(&self.conn)
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
        assert_eq!(store.schema_version()?, 2);
        assert_eq!(
            store.migration_status()?,
            MigrationStatus {
                applied: 2,
                expected: 2
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
                &[
                    "occurrence_key",
                    "kind",
                    "at",
                    "state",
                    "state_at",
                    "until",
                    "start",
                ],
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
        assert_eq!(again.schema_version()?, 2);
        let history: i64 =
            again
                .conn
                .query_row("select count(*) from refinery_schema_history", [], |r| {
                    r.get(0)
                })?;
        assert_eq!(history, 2);
        assert_eq!(again.settings().get("k")?, Some(serde_json::json!(1)));
        Ok(())
    }

    #[test]
    fn broken_migration_errors_and_rolls_back() -> TestResult {
        let mut migrations = embedded::migrations::runner().get_migrations().clone();
        migrations.push(Migration::unapplied(
            "V3__broken",
            "create table half_done (id integer); insert into no_such_table values (1);",
        )?);
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tolon.db");
        let result = Store::init(Connection::open(&path)?, Runner::new(&migrations));
        assert!(matches!(result, Err(StoreError::Migration(_))));

        // V1 and V2 stayed applied, V3 left nothing behind, and a normal open still works.
        let store = Store::open(&path)?;
        assert_eq!(store.schema_version()?, 2);
        let leftover: i64 = store.conn.query_row(
            "select count(*) from sqlite_master where name = 'half_done'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(leftover, 0);
        Ok(())
    }

    fn v1_runner() -> Runner {
        let v1: Vec<Migration> = embedded::migrations::runner()
            .get_migrations()
            .iter()
            .filter(|m| m.version() == 1)
            .cloned()
            .collect();
        Runner::new(&v1)
    }

    #[test]
    fn v1_then_v2_on_an_empty_db() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tolon.db");
        let v1 = Store::init(Connection::open(&path)?, v1_runner())?;
        assert_eq!(v1.schema_version()?, 1);
        assert!(!columns(&v1, "alarms")?.contains(&"start".to_string()));
        drop(v1);
        let v2 = Store::open(&path)?;
        assert_eq!(v2.schema_version()?, 2);
        assert!(v2.migration_status()?.is_current());
        assert_eq!(
            columns(&v2, "alarms")?.last().map(String::as_str),
            Some("start")
        );
        assert!(v2.alarms().list()?.is_empty());
        Ok(())
    }

    #[test]
    fn ac3_3_v1_alarm_row_migrates_with_start_equal_to_at() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tolon.db");
        let v1 = Store::init(Connection::open(&path)?, v1_runner())?;
        v1.conn.execute(
            "insert into alarms (occurrence_key, kind, at, state, state_at, until)
             values ('m1', 'lead', '2026-10-22T09:58:00Z', 'snoozed', '2026-10-22T09:59:00Z', '2026-10-22T10:03:00Z')",
            [],
        )?;
        drop(v1);
        let store = Store::open(&path)?;
        assert_eq!(store.schema_version()?, 2);
        let row = store.alarms().get("m1", "lead")?;
        assert_eq!(
            row,
            Some(AlarmRow {
                occurrence_key: "m1".into(),
                kind: "lead".into(),
                at: "2026-10-22T09:58:00Z".into(),
                start: "2026-10-22T09:58:00Z".into(),
                state: "snoozed".into(),
                state_at: "2026-10-22T09:59:00Z".into(),
                until: Some("2026-10-22T10:03:00Z".into()),
            })
        );
        let raw: Option<String> = store.conn.query_row(
            "select start from alarms where occurrence_key = 'm1'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(raw.as_deref(), Some("2026-10-22T09:58:00Z"));
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
