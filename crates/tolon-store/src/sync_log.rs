use rusqlite::{params, Connection};

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncLogEntry {
    pub id: i64,
    pub calendar_id: Option<String>,
    pub at: String,
    pub outcome: String,
    pub detail: Option<String>,
}

/// Append-only record of sync attempts.
///
/// `detail` is stored exactly as given. The caller must redact it first (`redact_url()`):
/// this repo cannot tell a secret url from any other text.
pub struct SyncLogRepo<'a> {
    conn: &'a Connection,
}

impl<'a> SyncLogRepo<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn append(
        &self,
        calendar_id: Option<&str>,
        at: &str,
        outcome: &str,
        detail: Option<&str>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "insert into sync_log (calendar_id, at, outcome, detail) values (?1, ?2, ?3, ?4)",
            params![calendar_id, at, outcome, detail],
        )?;
        Ok(())
    }

    /// Newest first, at most `limit` rows.
    pub fn list_recent(&self, limit: u32) -> Result<Vec<SyncLogEntry>, StoreError> {
        let mut stmt = self.conn.prepare(
            "select id, calendar_id, at, outcome, detail from sync_log order by id desc limit ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(SyncLogEntry {
                id: r.get(0)?,
                calendar_id: r.get(1)?,
                at: r.get(2)?,
                outcome: r.get(3)?,
                detail: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn append_and_list_recent_newest_first_with_limit() -> TestResult {
        let store = Store::open_in_memory()?;
        let log = store.sync_log();
        assert!(log.list_recent(10)?.is_empty());
        log.append(Some("c1"), "t1", "ok", None)?;
        log.append(Some("c1"), "t2", "error", Some("HTTP 500 on https://host"))?;
        log.append(None, "t3", "ok", None)?;
        let all = log.list_recent(10)?;
        let ats: Vec<_> = all.iter().map(|e| e.at.as_str()).collect();
        assert_eq!(ats, ["t3", "t2", "t1"]);
        assert_eq!(all[1].detail.as_deref(), Some("HTTP 500 on https://host"));
        assert_eq!(all[0].calendar_id, None);
        assert_eq!(log.list_recent(2)?.len(), 2);
        assert!(log.list_recent(0)?.is_empty());
        Ok(())
    }

    #[test]
    fn log_survives_calendar_removal() -> TestResult {
        let store = Store::open_in_memory()?;
        let c = store.calendars().insert("ics", "W", None, true, "t")?;
        store.sync_log().append(Some(&c.id), "t1", "ok", None)?;
        store.calendars().remove(&c.id)?;
        assert_eq!(store.sync_log().list_recent(5)?.len(), 1);
        Ok(())
    }
}
