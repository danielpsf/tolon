//! `at`, `start` and `state_at` must be RFC 3339 UTC with a `Z` suffix (for example `2026-10-22T08:00:00Z`). Ordering in `list`/`list_active` compares `at` as text; `prune_before` adds 10 minutes to `start` in SQLite first.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::StoreError;

/// States after which an alarm never fires again. The names match `AlarmState` in
/// `docs/architecture.md`; the store keeps states as plain strings.
///
/// Must match `tolon_core::AlarmState::name()` for the final states; tolon-app pins this with a test.
pub const TERMINAL_STATES: [&str; 3] = ["dismissed", "joined", "expired"];

fn terminal_json() -> Result<String, StoreError> {
    Ok(serde_json::to_string(&TERMINAL_STATES)?)
}

/// One alarm row. Every field is an opaque string here; the app maps it to the core types.
///
/// `at`, `start` and `state_at` must be RFC 3339 UTC with a `Z` suffix (for example `2026-10-22T08:00:00Z`). `start` is the meeting start; the alarm expires at `start` + 10 minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmRow {
    pub occurrence_key: String,
    pub kind: String,
    pub at: String,
    pub start: String,
    pub state: String,
    pub state_at: String,
    pub until: Option<String>,
}

// `coalesce`: the column is nullable; a NULL (none expected after the V2 backfill) reads as `at`.
const COLUMNS: &str = "occurrence_key, kind, at, coalesce(start, at), state, state_at, until";

const UPSERT: &str = "insert into alarms (occurrence_key, kind, at, start, state, state_at, until)
     values (?1, ?2, ?3, ?4, ?5, ?6, ?7)
     on conflict(occurrence_key, kind) do update
       set at = excluded.at, start = excluded.start, state = excluded.state,
           state_at = excluded.state_at, until = excluded.until";

fn from_row(r: &Row<'_>) -> rusqlite::Result<AlarmRow> {
    Ok(AlarmRow {
        occurrence_key: r.get(0)?,
        kind: r.get(1)?,
        at: r.get(2)?,
        start: r.get(3)?,
        state: r.get(4)?,
        state_at: r.get(5)?,
        until: r.get(6)?,
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
                row.start,
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
                    row.start,
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
    pub fn list(&self) -> Result<Vec<AlarmRow>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "select {COLUMNS} from alarms order by at, occurrence_key, kind"
        ))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Alarms whose state is not in `TERMINAL_STATES`, ordered by `at`.
    pub fn list_active(&self) -> Result<Vec<AlarmRow>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "select {COLUMNS} from alarms
             where state not in (select value from json_each(?1))
             order by at, occurrence_key, kind"
        ))?;
        let rows = stmt.query_map([terminal_json()?], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Deletes terminal alarms whose expiry, `start` + 10 minutes, is before `cutoff`.
    /// Non-terminal rows stay. Returns the number deleted.
    ///
    /// Text comparison cannot add minutes, so SQLite does: `strftime` parses the `Z` start,
    /// adds 10 minutes (carrying over hour and day) and renders the same
    /// `YYYY-MM-DDTHH:MM:SSZ` shape as `cutoff`; the two are then compared as text. `cutoff`
    /// must be UTC `Z`. A `start` SQLite cannot parse gives NULL, and that row is kept.
    pub fn prune_before(&self, cutoff: &str) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "delete from alarms
             where strftime('%Y-%m-%dT%H:%M:%SZ', coalesce(start, at), '+10 minutes') < ?1
               and state in (select value from json_each(?2))",
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
            start: at.into(),
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
        assert_eq!(a.get("k", "lead")?, None);
        let r = row("k", "lead", "t1", "scheduled", None);
        a.upsert(&r)?;
        assert_eq!(a.get("k", "lead")?, Some(r));
        assert_eq!(a.get("k", "start")?, None);
        Ok(())
    }

    #[test]
    fn upsert_overwrites_the_state() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert(&row("k", "lead", "t1", "scheduled", None))?;
        let snoozed = row("k", "lead", "t1", "snoozed", Some("t2"));
        a.upsert(&snoozed)?;
        assert_eq!(a.list()?, std::slice::from_ref(&snoozed));
        let dismissed = row("k", "lead", "t1", "dismissed", None);
        a.upsert(&dismissed)?;
        assert_eq!(a.get("k", "lead")?, Some(dismissed));
        Ok(())
    }

    #[test]
    fn upsert_all_writes_every_row_and_overwrites() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert(&row("a", "lead", "t1", "scheduled", None))?;
        a.upsert_all(&[
            row("a", "lead", "t1", "joined", None),
            row("b", "lead", "t2", "scheduled", None),
            row("b", "start", "t3", "scheduled", None),
        ])?;
        let all = a.list()?;
        assert_eq!(keys(&all), [("a", "lead"), ("b", "lead"), ("b", "start")]);
        assert_eq!(all[0].state, "joined");
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
            row("a", "lead", "t1", "scheduled", None),
            row("b", "lead", "t2", "boom", None),
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
            row("c", "lead", "t3", "scheduled", None),
            row("a", "lead", "t1", "scheduled", None),
            row("b", "lead", "t2", "scheduled", None),
        ])?;
        assert_eq!(
            keys(&a.list()?),
            [("a", "lead"), ("b", "lead"), ("c", "lead")]
        );
        Ok(())
    }

    #[test]
    fn list_active_excludes_terminal_states() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert_all(&[
            row("1", "lead", "t1", "scheduled", None),
            row("2", "lead", "t2", "firing", None),
            row("3", "lead", "t3", "snoozed", Some("t9")),
            row("4", "lead", "t4", "dismissed", None),
            row("5", "lead", "t5", "joined", None),
            row("6", "lead", "t6", "expired", None),
        ])?;
        let active = a.list_active()?;
        assert_eq!(keys(&active), [("1", "lead"), ("2", "lead"), ("3", "lead")]);
        assert_eq!(a.list()?.len(), 6);
        Ok(())
    }

    fn with_start(mut r: AlarmRow, start: &str) -> AlarmRow {
        r.start = start.into();
        r
    }

    #[test]
    fn prune_before_deletes_only_old_terminal_rows() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        let (t1, t3, t5, t8) = (
            "2026-10-22T08:01:00Z",
            "2026-10-22T08:03:00Z",
            "2026-10-22T08:05:00Z",
            "2026-10-22T08:08:00Z",
        );
        a.upsert_all(&[
            row("old-dismissed", "lead", t1, "dismissed", None),
            row("old-joined", "lead", t1, "joined", None),
            row("old-expired", "lead", t3, "expired", None),
            row("old-snoozed", "lead", t1, "snoozed", Some(t8)),
            row("old-scheduled", "lead", t1, "scheduled", None),
            row("new-dismissed", "lead", t8, "dismissed", None),
            row("at-cutoff", "lead", t5, "dismissed", None),
        ])?;
        // Expiry is start + 10 min: t1 -> 08:11, t3 -> 08:13, t5 -> 08:15 (not before the cutoff).
        assert_eq!(a.prune_before("2026-10-22T08:15:00Z")?, 3);
        assert_eq!(
            keys(&a.list()?),
            [
                ("old-scheduled", "lead"),
                ("old-snoozed", "lead"),
                ("at-cutoff", "lead"),
                ("new-dismissed", "lead"),
            ]
        );
        Ok(())
    }

    #[test]
    fn prune_before_keeps_a_snoozed_row() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        let snoozed = row(
            "k",
            "lead",
            "2026-10-22T08:00:00Z",
            "snoozed",
            Some("2026-10-22T09:00:00Z"),
        );
        a.upsert(&snoozed)?;
        assert_eq!(a.prune_before("2026-10-23T00:00:00Z")?, 0);
        assert_eq!(a.get("k", "lead")?, Some(snoozed));
        Ok(())
    }

    #[test]
    fn prune_uses_start_plus_ten_minutes_not_at() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        // A lead alarm: fires at 09:55, the meeting starts at 10:00, so it expires at 10:10.
        a.upsert(&with_start(
            row("m", "lead", "2026-10-22T09:55:00Z", "dismissed", None),
            "2026-10-22T10:00:00Z",
        ))?;
        assert_eq!(a.prune_before("2026-10-22T10:10:00Z")?, 0);
        assert_eq!(a.prune_before("2026-10-22T10:10:01Z")?, 1);
        Ok(())
    }

    #[test]
    fn prune_adds_ten_minutes_across_day_month_and_year_boundaries() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        for (start, expiry) in [
            ("2026-10-22T23:55:00Z", "2026-10-23T00:05:00Z"),
            ("2026-10-31T23:59:59Z", "2026-11-01T00:09:59Z"),
            ("2026-12-31T23:50:00Z", "2027-01-01T00:00:00Z"),
            ("2028-02-28T23:55:00Z", "2028-02-29T00:05:00Z"),
        ] {
            a.upsert(&with_start(
                row("k", "start", start, "expired", None),
                start,
            ))?;
            assert_eq!(
                a.prune_before(expiry)?,
                0,
                "expiry {expiry} is not before itself"
            );
            assert_eq!(a.list()?.len(), 1);
            // One second later the row is past its expiry. Text order is right across the boundary.
            let later = a.conn.query_row(
                "select strftime('%Y-%m-%dT%H:%M:%SZ', ?1, '+1 second')",
                [expiry],
                |r| r.get::<_, String>(0),
            )?;
            assert_eq!(a.prune_before(&later)?, 1, "start {start}");
        }
        Ok(())
    }

    #[test]
    fn prune_keeps_a_row_whose_start_does_not_parse() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert(&row("k", "lead", "not-a-time", "dismissed", None))?;
        assert_eq!(a.prune_before("9999-12-31T23:59:59Z")?, 0);
        Ok(())
    }

    #[test]
    fn delete_missing_keeps_the_listed_rows() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert_all(&[
            row("a", "lead", "t1", "scheduled", None),
            row("a", "start", "t2", "scheduled", None),
            row("b", "lead", "t3", "dismissed", None),
            row("c", "lead", "t4", "snoozed", Some("t9")),
        ])?;
        let keep = [
            ("a".to_string(), "lead".to_string()),
            ("c".to_string(), "lead".to_string()),
            ("zzz".to_string(), "lead".to_string()),
        ];
        assert_eq!(a.delete_missing(&keep)?, 2);
        assert_eq!(keys(&a.list()?), [("a", "lead"), ("c", "lead")]);
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
            row(evil, "lead", "t1", "scheduled", None),
            row("b", "lead", "t2", "scheduled", None),
        ])?;
        a.delete_missing(&[(evil.to_string(), "lead".to_string())])?;
        assert_eq!(keys(&a.list()?), [(evil, "lead")]);
        Ok(())
    }

    #[test]
    fn ordering_and_prune_use_utc_z_timestamps() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.alarms();
        a.upsert_all(&[
            row("c", "lead", "2026-10-23T00:00:00Z", "dismissed", None),
            row("a", "lead", "2026-10-22T23:59:59Z", "dismissed", None),
            row("b", "lead", "2026-10-22T08:00:00Z", "expired", None),
            row(
                "d",
                "lead",
                "2026-10-21T09:00:00Z",
                "snoozed",
                Some("2026-10-24T09:00:00Z"),
            ),
        ])?;
        assert_eq!(
            keys(&a.list()?),
            [("d", "lead"), ("b", "lead"), ("a", "lead"), ("c", "lead")]
        );
        assert_eq!(keys(&a.list_active()?), [("d", "lead")]);
        // Cutoff at 00:05: expiry (start + 10 min) of "a" is 00:09, so only the 08:00 row goes.
        assert_eq!(a.prune_before("2026-10-23T00:05:00Z")?, 1);
        assert_eq!(
            keys(&a.list()?),
            [("d", "lead"), ("a", "lead"), ("c", "lead")]
        );
        Ok(())
    }

    #[test]
    fn ac3_3_alarm_state_survives_restart() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tolon.db");
        let dismissed = with_start(
            row("m1", "lead", "2026-01-01T09:55:00Z", "dismissed", None),
            "2026-01-01T10:00:00Z",
        );
        let snoozed = row(
            "m2",
            "start",
            "2026-01-01T10:00:00Z",
            "snoozed",
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
        assert_eq!(a.get("m1", "lead")?, Some(dismissed));
        assert_eq!(a.get("m2", "start")?, Some(snoozed.clone()));
        assert_eq!(a.list_active()?, [snoozed]);
        Ok(())
    }

    #[test]
    fn ac3_3_alarm_start_survives_restart() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tolon.db");
        let lead = with_start(
            row("m1", "lead", "2026-01-01T09:58:00Z", "scheduled", None),
            "2026-01-01T10:00:00Z",
        );
        Store::open(&path)?.alarms().upsert(&lead)?;
        let store = Store::open(&path)?;
        let got = store.alarms().get("m1", "lead")?;
        assert_eq!(
            got.as_ref().map(|r| r.start.as_str()),
            Some("2026-01-01T10:00:00Z")
        );
        assert_eq!(got, Some(lead));
        Ok(())
    }
}
