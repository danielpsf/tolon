use rusqlite::{params, Connection, Row};

use crate::StoreError;

/// One event as fetched: the feed UID and its canonical JSON payload (opaque here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedEvent {
    pub uid: String,
    pub payload: String,
}

/// A stored event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    pub calendar_id: String,
    pub uid: String,
    pub payload: String,
    pub updated_at: String,
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        id: r.get(0)?,
        calendar_id: r.get(1)?,
        uid: r.get(2)?,
        payload: r.get(3)?,
        updated_at: r.get(4)?,
    })
}

/// Events of each calendar, as last fetched.
pub struct EventsRepo<'a> {
    conn: &'a Connection,
}

impl<'a> EventsRepo<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Makes the calendar's stored events match `feed`, in one transaction: upsert by uid
    /// (id kept, `updated_at` moves only when the payload changed), delete uids not in the feed.
    pub fn replace_for_calendar(
        &self,
        calendar_id: &str,
        feed: &[FeedEvent],
        now: &str,
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut upsert = tx.prepare(
                "insert into events (id, calendar_id, uid, payload, updated_at)
                 values (?1, ?2, ?3, ?4, ?5)
                 on conflict(calendar_id, uid) do update
                   set payload = excluded.payload, updated_at = excluded.updated_at
                   where events.payload != excluded.payload",
            )?;
            for e in feed {
                let id = uuid::Uuid::new_v4().to_string();
                upsert.execute(params![id, calendar_id, e.uid, e.payload, now])?;
            }
        }
        let uids = serde_json::to_string(&feed.iter().map(|e| &e.uid).collect::<Vec<_>>())?;
        tx.execute(
            "delete from events where calendar_id = ?1
               and uid not in (select value from json_each(?2))",
            params![calendar_id, uids],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_by_calendar(&self, calendar_id: &str) -> Result<Vec<Event>, StoreError> {
        let mut stmt = self.conn.prepare(
            "select id, calendar_id, uid, payload, updated_at from events
             where calendar_id = ?1 order by uid",
        )?;
        let rows = stmt.query_map([calendar_id], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Events of every enabled calendar.
    pub fn list_enabled(&self) -> Result<Vec<Event>, StoreError> {
        let mut stmt = self.conn.prepare(
            "select e.id, e.calendar_id, e.uid, e.payload, e.updated_at from events e
             join calendars c on c.id = e.calendar_id
             where c.enabled = 1 order by e.calendar_id, e.uid",
        )?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::FeedEvent;
    use crate::Store;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn feed(items: &[(&str, &str)]) -> Vec<FeedEvent> {
        items
            .iter()
            .map(|(u, p)| FeedEvent {
                uid: (*u).into(),
                payload: (*p).into(),
            })
            .collect()
    }

    #[test]
    fn upsert_then_delete_missing() -> TestResult {
        let store = Store::open_in_memory()?;
        let cal = store.calendars().insert("ics", "W", None, true, "t")?;
        let ev = store.events();
        ev.replace_for_calendar(&cal.id, &feed(&[("a", "1"), ("b", "1"), ("c", "1")]), "t1")?;
        let first = ev.list_by_calendar(&cal.id)?;
        assert_eq!(first.len(), 3);

        ev.replace_for_calendar(&cal.id, &feed(&[("a", "1"), ("b", "2")]), "t2")?;
        let now = ev.list_by_calendar(&cal.id)?;
        let uids: Vec<_> = now.iter().map(|e| e.uid.as_str()).collect();
        assert_eq!(uids, ["a", "b"]);
        // ids are stable; updated_at moves only for the changed payload.
        assert_eq!(now[0].id, first[0].id);
        assert_eq!(
            (now[0].payload.as_str(), now[0].updated_at.as_str()),
            ("1", "t1")
        );
        assert_eq!(
            (now[1].payload.as_str(), now[1].updated_at.as_str()),
            ("2", "t2")
        );

        ev.replace_for_calendar(&cal.id, &[], "t3")?;
        assert!(ev.list_by_calendar(&cal.id)?.is_empty());
        Ok(())
    }

    #[test]
    fn replace_is_scoped_to_its_calendar() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.calendars().insert("ics", "A", None, true, "t")?;
        let b = store.calendars().insert("ics", "B", None, true, "t")?;
        let ev = store.events();
        ev.replace_for_calendar(&a.id, &feed(&[("x", "a")]), "t")?;
        ev.replace_for_calendar(&b.id, &feed(&[("x", "b")]), "t")?;
        ev.replace_for_calendar(&a.id, &[], "t2")?;
        assert!(ev.list_by_calendar(&a.id)?.is_empty());
        assert_eq!(ev.list_by_calendar(&b.id)?.len(), 1);
        Ok(())
    }

    #[test]
    fn replace_for_unknown_calendar_errors_and_stores_nothing() -> TestResult {
        let store = Store::open_in_memory()?;
        let ev = store.events();
        assert!(ev
            .replace_for_calendar("no-such-calendar", &feed(&[("a", "1")]), "t")
            .is_err());
        assert!(ev.list_enabled()?.is_empty());
        Ok(())
    }

    #[test]
    fn s1_5_not_modified_keeps_events() -> TestResult {
        let store = Store::open_in_memory()?;
        let cal = store.calendars().insert("ics", "W", None, true, "t")?;
        store
            .events()
            .replace_for_calendar(&cal.id, &feed(&[("a", "1")]), "t1")?;
        store.calendars().record_not_modified(&cal.id, "t2")?;
        assert_eq!(store.events().list_by_calendar(&cal.id)?.len(), 1);
        Ok(())
    }

    #[test]
    fn s1_3_failure_keeps_events() -> TestResult {
        let store = Store::open_in_memory()?;
        let cal = store.calendars().insert("ics", "W", None, true, "t")?;
        store
            .events()
            .replace_for_calendar(&cal.id, &feed(&[("a", "1")]), "t1")?;
        store.calendars().record_failure(&cal.id, "HTTP 500")?;
        assert_eq!(store.events().list_by_calendar(&cal.id)?.len(), 1);
        Ok(())
    }

    #[test]
    fn remove_calendar_cascades_to_events() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.calendars().insert("ics", "A", None, true, "t")?;
        let b = store.calendars().insert("ics", "B", None, true, "t")?;
        let ev = store.events();
        ev.replace_for_calendar(&a.id, &feed(&[("x", "1")]), "t")?;
        ev.replace_for_calendar(&b.id, &feed(&[("y", "1")]), "t")?;
        assert!(store.calendars().remove(&a.id)?);
        assert!(ev.list_by_calendar(&a.id)?.is_empty());
        assert_eq!(ev.list_by_calendar(&b.id)?.len(), 1);
        Ok(())
    }

    #[test]
    fn set_enabled_filters_list_enabled() -> TestResult {
        let store = Store::open_in_memory()?;
        let a = store.calendars().insert("ics", "A", None, true, "t")?;
        let b = store.calendars().insert("ics", "B", None, true, "t")?;
        let ev = store.events();
        ev.replace_for_calendar(&a.id, &feed(&[("x", "1")]), "t")?;
        ev.replace_for_calendar(&b.id, &feed(&[("y", "1")]), "t")?;
        assert_eq!(ev.list_enabled()?.len(), 2);
        store.calendars().set_enabled(&b.id, false)?;
        let on = ev.list_enabled()?;
        assert_eq!(on.len(), 1);
        assert_eq!(on[0].calendar_id, a.id);
        // Disabled events stay stored and listable per calendar.
        assert_eq!(ev.list_by_calendar(&b.id)?.len(), 1);
        Ok(())
    }
}
