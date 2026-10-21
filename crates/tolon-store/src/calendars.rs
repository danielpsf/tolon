use std::fmt;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::StoreError;

/// A subscribed calendar. Timestamps are caller-supplied text (RFC 3339 by convention).
#[derive(Clone, PartialEq, Eq)]
pub struct Calendar {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub enabled: bool,
    /// May carry a secret; never log it.
    pub url: Option<String>,
    pub account: Option<String>,
    pub color: Option<String>,
    pub etag: Option<String>,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
}

// Hand-written so a `{:?}` in a log line cannot leak the url.
impl fmt::Debug for Calendar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Calendar")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("enabled", &self.enabled)
            .field("url", &self.url.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

const COLUMNS: &str =
    "id, kind, name, enabled, url, account, color, etag, last_sync_at, last_error, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Calendar> {
    Ok(Calendar {
        id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        enabled: r.get(3)?,
        url: r.get(4)?,
        account: r.get(5)?,
        color: r.get(6)?,
        etag: r.get(7)?,
        last_sync_at: r.get(8)?,
        last_error: r.get(9)?,
        created_at: r.get(10)?,
    })
}

/// Calendar subscriptions and their sync state.
pub struct CalendarsRepo<'a> {
    conn: &'a Connection,
}

impl<'a> CalendarsRepo<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Inserts a calendar with a fresh uuid v4 id and returns it.
    pub fn insert(
        &self,
        kind: &str,
        name: &str,
        url: Option<&str>,
        enabled: bool,
        created_at: &str,
    ) -> Result<Calendar, StoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "insert into calendars (id, kind, name, enabled, url, created_at)
             values (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, kind, name, enabled, url, created_at],
        )?;
        self.get(&id)?.ok_or(StoreError::NotFound)
    }

    pub fn get(&self, id: &str) -> Result<Option<Calendar>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("select {COLUMNS} from calendars where id = ?1"),
                [id],
                from_row,
            )
            .optional()?)
    }

    /// All calendars, oldest first.
    pub fn list(&self) -> Result<Vec<Calendar>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "select {COLUMNS} from calendars order by created_at, id"
        ))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Deletes the calendar and, by cascade, its events. False when it did not exist.
    pub fn remove(&self, id: &str) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .execute("delete from calendars where id = ?1", [id])?
            > 0)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), StoreError> {
        self.touch(
            "update calendars set enabled = ?2 where id = ?1",
            params![id, enabled],
        )
    }

    /// The signed-in account a Google calendar belongs to (its address).
    pub fn set_account(&self, id: &str, account: &str) -> Result<(), StoreError> {
        self.touch(
            "update calendars set account = ?2 where id = ?1",
            params![id, account],
        )
    }

    /// A 2xx fetch: store the new etag, stamp the sync time, clear any error.
    pub fn record_success(&self, id: &str, etag: Option<&str>, at: &str) -> Result<(), StoreError> {
        self.touch(
            "update calendars set etag = ?2, last_sync_at = ?3, last_error = null where id = ?1",
            params![id, etag, at],
        )
    }

    /// A 304: only the sync time moves; etag, error and events stay as they were.
    pub fn record_not_modified(&self, id: &str, at: &str) -> Result<(), StoreError> {
        self.touch(
            "update calendars set last_sync_at = ?2 where id = ?1",
            params![id, at],
        )
    }

    /// A failed fetch. `error` must already be redacted by the caller (no secret urls).
    pub fn record_failure(&self, id: &str, error: &str) -> Result<(), StoreError> {
        self.touch(
            "update calendars set last_error = ?2 where id = ?1",
            params![id, error],
        )
    }

    fn touch(&self, sql: &str, p: impl rusqlite::Params) -> Result<(), StoreError> {
        match self.conn.execute(sql, p)? {
            0 => Err(StoreError::NotFound),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Store, StoreError};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn insert_get_list_remove() -> TestResult {
        let store = Store::open_in_memory()?;
        let c = store.calendars();
        let a = c.insert("ics", "Work", Some("https://x.test/s3cret"), true, "t1")?;
        let b = c.insert("ics", "Home", None, false, "t2")?;
        assert_ne!(a.id, b.id);
        assert_eq!(a.id.len(), 36);
        assert_eq!(c.get(&a.id)?, Some(a.clone()));
        assert!(!b.enabled && b.url.is_none() && b.etag.is_none());
        assert_eq!(c.list()?, vec![a.clone(), b.clone()]);
        assert!(c.remove(&a.id)?);
        assert!(!c.remove(&a.id)?);
        assert_eq!(c.get(&a.id)?, None);
        assert_eq!(c.list()?, vec![b]);
        Ok(())
    }

    #[test]
    fn set_enabled_toggles_and_missing_is_not_found() -> TestResult {
        let store = Store::open_in_memory()?;
        let c = store.calendars();
        let a = c.insert("ics", "Work", None, true, "t")?;
        c.set_enabled(&a.id, false)?;
        assert!(!c.get(&a.id)?.ok_or("gone")?.enabled);
        c.set_enabled(&a.id, true)?;
        assert!(c.get(&a.id)?.ok_or("gone")?.enabled);
        assert!(matches!(
            c.set_enabled("nope", true),
            Err(StoreError::NotFound)
        ));
        Ok(())
    }

    #[test]
    fn s1_3_failure_keeps_calendar_listed() -> TestResult {
        let store = Store::open_in_memory()?;
        let c = store.calendars();
        let a = c.insert("ics", "Work", Some("https://x.test/s"), true, "t")?;
        c.record_failure(&a.id, "HTTP 404")?;
        let got = c.get(&a.id)?.ok_or("gone")?;
        assert_eq!(got.last_error.as_deref(), Some("HTTP 404"));
        assert_eq!(c.list()?.len(), 1);
        assert!(matches!(
            c.record_failure("nope", "x"),
            Err(StoreError::NotFound)
        ));
        Ok(())
    }

    #[test]
    fn ac1_4_success_stores_etag_and_clears_error() -> TestResult {
        let store = Store::open_in_memory()?;
        let c = store.calendars();
        let a = c.insert("ics", "Work", None, true, "t")?;
        c.record_failure(&a.id, "timeout")?;
        c.record_success(&a.id, Some("\"v1\""), "t2")?;
        let got = c.get(&a.id)?.ok_or("gone")?;
        assert_eq!(got.etag.as_deref(), Some("\"v1\""));
        assert_eq!(got.last_sync_at.as_deref(), Some("t2"));
        assert_eq!(got.last_error, None);
        assert!(matches!(
            c.record_success("nope", None, "t"),
            Err(StoreError::NotFound)
        ));
        Ok(())
    }

    #[test]
    fn s1_5_not_modified_keeps_etag() -> TestResult {
        let store = Store::open_in_memory()?;
        let c = store.calendars();
        let a = c.insert("ics", "Work", None, true, "t")?;
        c.record_success(&a.id, Some("\"v1\""), "t2")?;
        c.record_not_modified(&a.id, "t3")?;
        let got = c.get(&a.id)?.ok_or("gone")?;
        assert_eq!(got.etag.as_deref(), Some("\"v1\""));
        assert_eq!(got.last_sync_at.as_deref(), Some("t3"));
        assert!(matches!(
            c.record_not_modified("nope", "t"),
            Err(StoreError::NotFound)
        ));
        Ok(())
    }

    #[test]
    fn set_account_stores_the_google_account() -> TestResult {
        let store = Store::open_in_memory()?;
        let c = store.calendars();
        let a = c.insert(
            "google",
            "Team",
            Some("team@group.calendar.google.com"),
            true,
            "t",
        )?;
        c.set_account(&a.id, "me@example.com")?;
        assert_eq!(
            c.get(&a.id)?.ok_or("gone")?.account.as_deref(),
            Some("me@example.com")
        );
        assert!(matches!(
            c.set_account("nope", "x"),
            Err(StoreError::NotFound)
        ));
        Ok(())
    }

    #[test]
    fn s1_4_debug_output_hides_url() -> TestResult {
        let store = Store::open_in_memory()?;
        let a =
            store
                .calendars()
                .insert("ics", "Work", Some("https://x.test/TOKEN123"), true, "t")?;
        assert!(!format!("{a:?}").contains("TOKEN123"));
        Ok(())
    }
}
