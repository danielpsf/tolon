//! `at` and `state_at` must be RFC 3339 UTC with a `Z` suffix (for example `2026-10-22T08:00:00Z`). Ordering in `list`/`list_active` and `prune_before` compare them as text.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::StoreError;

/// States after which an alarm never fires again. The names match `AlarmState` in
/// `docs/architecture.md`; the store keeps states as plain strings.
pub const TERMINAL_STATES: [&str; 3] = ["Dismissed", "Joined", "Expired"];

fn terminal_json() -> Result<String, StoreError> {
    Ok(serde_json::to_string(&TERMINAL_STATES)?)
}

/// One alarm row. Every field is an opaque string here; the app maps it to the core types.
///
/// `at` and `state_at` must be RFC 3339 UTC with a `Z` suffix (for example `2026-10-22T08:00:00Z`). Ordering in `list`/`list_active` and `prune_before` compare them as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmRow {
    pub occurrence_key: String,
    pub kind: String,
    pub at: String,
    pub state: String,
    pub state_at: String,
    pub until: Option<String>,
}

const COLUMNS: &str = "occurrence_key, kind, at, state, state_at, until";

const UPSERT: &str = "insert into alarms (occurrence_key, kind, at, state, state_at, until)
     values (?1, ?2, ?3, ?4, ?5, ?6)
     on conflict(occurrence_key, kind) do update
       set at = excluded.at, state = excluded.state,
           state_at = excluded.state_at, until = excluded.until";

fn from_row(r: &Row<'_>) -> rusqlite::Result<AlarmRow> {
    Ok(AlarmRow {
        occurrence_key: r.get(0)?,
        kind: r.get(1)?,
        at: r.get(2)?,
        state: r.get(3)?,
        state_at: r.get(4)?,
        until: r.get(5)?,
    })
}

/// Alarm state, kept so it survives a restart.
pub struct AlarmsRepo<'a> {
    conn: &'a Connection,
}

impl<'a> AlarmsRepo<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Inserts the row, or overwrites every other column of the row with the same
    /// (occurrence_key, kind).
    pub fn upsert(&self, row: &AlarmRow) -> Result<(), StoreError> {
        self.conn.execute(
            UPSERT,
            params![
                row.occurrence_key,
                row.kind,
                row.at,
                row.state,
                row.state_at,
                row.until
            ],
        )?;
        Ok(())
    }

    /// Upserts all rows in one transaction: all or none.
    pub fn upsert_all(&self, rows: &[AlarmRow]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(UPSERT)?;
            for row in rows {
                stmt.execute(params![
                    row.occurrence_key,
                    row.kind,
                    row.at,
                    row.state,
                    row.state_at,
                    row.until
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get(&self, occurrence_key: &str, kind: &str) -> Result<Option<AlarmRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("select {COLUMNS} from alarms where occurrence_key = ?1 and kind = ?2"),
                params![occurrence_key, kind],
                from_row,
            )
            .optional()?)
    }

    /// Every alarm, ordered by `at`.
    ///
    /// `at` and `state_at` must be RFC 3339 UTC with a `Z` suffix (for example `2026-10-22T08:00:00Z`). Ordering in `list`/`list_active` and `prune_before` compare them as text.
    pub fn list(&self) -> Result<Vec<AlarmRow>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "select {COLUMNS} from alarms order by at, occurrence_key, kind"
        ))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Alarms whose state is not in `TERMINAL_STATES`, ordered by `at`.
    ///
    /// `at` and `state_at` must be RFC 3339 UTC with a `Z` suffix (for example `2026-10-22T08:00:00Z`). Ordering in `list`/`list_active` and `prune_before` compare them as text.
    pub fn list_active(&self) -> Result<Vec<AlarmRow>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "select {COLUMNS} from alarms
             where state not in (select value from json_each(?1))
             order by at, occurrence_key, kind"
        ))?;
        let rows = stmt.query_map([terminal_json()?], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Deletes terminal alarms whose `at` is before `cutoff`. Non-terminal rows stay. Returns
    /// the number deleted.
    ///
    /// `at` and `state_at` must be RFC 3339 UTC with a `Z` suffix (for example `2026-10-22T08:00:00Z`). Ordering in `list`/`list_active` and `prune_before` compare them as text.
    pub fn prune_before(&self, cutoff: &str) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "delete from alarms
             where at < ?1 and state in (select value from json_each(?2))",
            params![cutoff, terminal_json()?],
        )?)
    }

    /// Deletes every alarm whose (occurrence_key, kind) is not in `keep`, in one transaction.
    /// Returns the number deleted.
    pub fn delete_missing(&self, keep: &[(String, String)]) -> Result<usize, StoreError> {
        let keys = serde_json::to_string(keep)?;
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "delete from alarms
             where (occurrence_key, kind) not in
               (select json_extract(value, '$[0]'), json_extract(value, '$[1]')
                from json_each(?1))",
            params![keys],
        )?;
        tx.commit()?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::AlarmRow;
    use crate::Store;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn row(key: &str, kind: &str, at: &str, state: &str, until: Option<&str>) -> AlarmRow {
        AlarmRow {
            occurrence_key: key.into(),
            kind: kind.into(),
            at: at.into(),
            state: state.into(),
            state_at: "2026-01-01T00:00:00Z".into(),
            until: until.map(Into::into),
        }
    }

    fn keys(rows: &[AlarmRow]) -> Vec<(&str, &str)> {
        rows.iter()
            .map(|r| (r.occurrence_key.as_str(), r.kind.as_str()))
            .collect()
    }

    #[test]
    fn upsert_and_get() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        assert_eq!(a.get("k", "Lead")?, None);
        let r = row("k", "Lead", "t1", "Scheduled", None);
        a.upsert(&r)?;
        assert_eq!(a.get("k", "Lead")?, Some(r));
        assert_eq!(a.get("k", "Start")?, None);
        Ok(())
    }

    #[test]
    fn upsert_overwrites_the_state() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert(&row("k", "Lead", "t1", "Scheduled", None))?;
        let snoozed = row("k", "Lead", "t1", "Snoozed", Some("t2"));
        a.upsert(&snoozed)?;
        assert_eq!(a.list()?, std::slice::from_ref(&snoozed));
        let dismissed = row("k", "Lead", "t1", "Dismissed", None);
        a.upsert(&dismissed)?;
        assert_eq!(a.get("k", "Lead")?, Some(dismissed));
        Ok(())
    }

    #[test]
    fn upsert_all_writes_every_row_and_overwrites() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert(&row("a", "Lead", "t1", "Scheduled", None))?;
        a.upsert_all(&[
            row("a", "Lead", "t1", "Joined", None),
            row("b", "Lead", "t2", "Scheduled", None),
            row("b", "Start", "t3", "Scheduled", None),
        ])?;
        let all = a.list()?;
        assert_eq!(keys(&all), [("a", "Lead"), ("b", "Lead"), ("b", "Start")]);
        assert_eq!(all[0].state, "Joined");
        a.upsert_all(&[])?;
        assert_eq!(a.list()?.len(), 3);
        Ok(())
    }

    #[test]
    fn upsert_all_is_atomic() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        // A trigger makes the second row fail, so the first must roll back.
        store.conn.execute_batch(
            "create trigger boom before insert on alarms when new.state = 'boom'
             begin select raise(abort, 'boom'); end;",
        )?;
        let res = a.upsert_all(&[
            row("a", "Lead", "t1", "Scheduled", None),
            row("b", "Lead", "t2", "boom", None),
        ]);
        assert!(res.is_err());
        assert!(a.list()?.is_empty());
        Ok(())
    }

    #[test]
    fn list_is_ordered_by_at() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert_all(&[
            row("c", "Lead", "t3", "Scheduled", None),
            row("a", "Lead", "t1", "Scheduled", None),
            row("b", "Lead", "t2", "Scheduled", None),
        ])?;
        assert_eq!(
            keys(&a.list()?),
            [("a", "Lead"), ("b", "Lead"), ("c", "Lead")]
        );
        Ok(())
    }

    #[test]
    fn list_active_excludes_terminal_states() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert_all(&[
            row("1", "Lead", "t1", "Scheduled", None),
            row("2", "Lead", "t2", "Firing", None),
            row("3", "Lead", "t3", "Snoozed", Some("t9")),
            row("4", "Lead", "t4", "Dismissed", None),
            row("5", "Lead", "t5", "Joined", None),
            row("6", "Lead", "t6", "Expired", None),
        ])?;
        let active = a.list_active()?;
        assert_eq!(keys(&active), [("1", "Lead"), ("2", "Lead"), ("3", "Lead")]);
        assert_eq!(a.list()?.len(), 6);
        Ok(())
    }

    #[test]
    fn prune_before_deletes_only_old_terminal_rows() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert_all(&[
            row("old-dismissed", "Lead", "t1", "Dismissed", None),
            row("old-joined", "Lead", "t2", "Joined", None),
            row("old-expired", "Lead", "t3", "Expired", None),
            row("old-snoozed", "Lead", "t1", "Snoozed", Some("t9")),
            row("old-scheduled", "Lead", "t1", "Scheduled", None),
            row("new-dismissed", "Lead", "t8", "Dismissed", None),
            row("at-cutoff", "Lead", "t5", "Dismissed", None),
        ])?;
        assert_eq!(a.prune_before("t5")?, 3);
        assert_eq!(
            keys(&a.list()?),
            [
                ("old-scheduled", "Lead"),
                ("old-snoozed", "Lead"),
                ("at-cutoff", "Lead"),
                ("new-dismissed", "Lead"),
            ]
        );
        Ok(())
    }

    #[test]
    fn prune_before_keeps_a_snoozed_row() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        let snoozed = row("k", "Lead", "t1", "Snoozed", Some("t99"));
        a.upsert(&snoozed)?;
        assert_eq!(a.prune_before("t50")?, 0);
        assert_eq!(a.get("k", "Lead")?, Some(snoozed));
        Ok(())
    }

    #[test]
    fn delete_missing_keeps_the_listed_rows() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert_all(&[
            row("a", "Lead", "t1", "Scheduled", None),
            row("a", "Start", "t2", "Scheduled", None),
            row("b", "Lead", "t3", "Dismissed", None),
            row("c", "Lead", "t4", "Snoozed", Some("t9")),
        ])?;
        let keep = [
            ("a".to_string(), "Lead".to_string()),
            ("c".to_string(), "Lead".to_string()),
            ("zzz".to_string(), "Lead".to_string()),
        ];
        assert_eq!(a.delete_missing(&keep)?, 2);
        assert_eq!(keys(&a.list()?), [("a", "Lead"), ("c", "Lead")]);
        assert_eq!(a.delete_missing(&[])?, 2);
        assert!(a.list()?.is_empty());
        Ok(())
    }

    #[test]
    fn delete_missing_binds_hostile_keys_as_data() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        let evil = "x'\"],[\"y";
        a.upsert_all(&[
            row(evil, "Lead", "t1", "Scheduled", None),
            row("b", "Lead", "t2", "Scheduled", None),
        ])?;
        a.delete_missing(&[(evil.to_string(), "Lead".to_string())])?;
        assert_eq!(keys(&a.list()?), [(evil, "Lead")]);
        Ok(())
    }

    #[test]
    fn ordering_and_prune_use_utc_z_timestamps() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert_all(&[
            row("c", "Lead", "2026-10-23T00:00:00Z", "Dismissed", None),
            row("a", "Lead", "2026-10-22T23:59:59Z", "Dismissed", None),
            row("b", "Lead", "2026-10-22T08:00:00Z", "Expired", None),
            row(
                "d",
                "Lead",
                "2026-10-21T09:00:00Z",
                "Snoozed",
                Some("2026-10-24T09:00:00Z"),
            ),
        ])?;
        assert_eq!(
            keys(&a.list()?),
            [("d", "Lead"), ("b", "Lead"), ("a", "Lead"), ("c", "Lead")]
        );
        assert_eq!(keys(&a.list_active()?), [("d", "Lead")]);
        // Cutoff at midnight: removes the terminal rows of the 22nd, keeps the 23rd and the snoozed row.
        assert_eq!(a.prune_before("2026-10-23T00:00:00Z")?, 2);
        assert_eq!(keys(&a.list()?), [("d", "Lead"), ("c", "Lead")]);
        Ok(())
    }

    #[test]
    fn ac3_3_alarm_state_survives_restart() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tolon.db");
        let dismissed = row("m1", "Lead", "2026-01-01T09:55:00Z", "Dismissed", None);
        let snoozed = row(
            "m2",
            "Start",
            "2026-01-01T10:00:00Z",
            "Snoozed",
            Some("2026-01-01T10:05:00Z"),
        );
        {
            let store = Store::open(&path)?;
            store
                .alarms()
                .upsert_all(&[dismissed.clone(), snoozed.clone()])?;
        }
        let store = Store::open(&path)?;
        let a = store.alarms();
        assert_eq!(a.get("m1", "Lead")?, Some(dismissed));
        assert_eq!(a.get("m2", "Start")?, Some(snoozed.clone()));
        assert_eq!(a.list_active()?, [snoozed]);
        Ok(())
    }
}
