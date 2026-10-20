use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::StoreError;

/// Key/value settings; values are stored as JSON text.
pub struct SettingsRepo<'a> {
    conn: &'a Connection,
}

impl<'a> SettingsRepo<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn get(&self, key: &str) -> Result<Option<Value>, StoreError> {
        let text: Option<String> = self
            .conn
            .query_row("select value from settings where key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(text.map(|t| serde_json::from_str(&t)).transpose()?)
    }

    pub fn set(&self, key: &str, value: &Value) -> Result<(), StoreError> {
        self.conn.execute(
            "insert into settings (key, value) values (?1, ?2)
             on conflict(key) do update set value = excluded.value",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    /// Sets every pair in one transaction: all or none (AC9.1, a settings change saves whole).
    pub fn set_all(&self, pairs: &[(&str, Value)]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        for (key, value) in pairs {
            self.set(key, value)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn all(&self) -> Result<BTreeMap<String, Value>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("select key, value from settings order by key")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (k, v) = row?;
            out.insert(k, serde_json::from_str(&v)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;
    use serde_json::json;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn settings_roundtrip_overwrite_and_missing() -> TestResult {
        let store = Store::open_in_memory()?;
        let s = store.settings();
        assert_eq!(s.get("theme")?, None);
        s.set("theme", &json!("dark"))?;
        s.set("lead", &json!({"minutes": 5, "on": true}))?;
        assert_eq!(s.get("theme")?, Some(json!("dark")));
        s.set("theme", &json!("light"))?;
        assert_eq!(s.get("theme")?, Some(json!("light")));
        let all = s.all()?;
        assert_eq!(all.len(), 2);
        assert_eq!(all["lead"], json!({"minutes": 5, "on": true}));
        Ok(())
    }

    #[test]
    fn set_all_writes_every_pair_or_none() -> TestResult {
        let store = Store::open_in_memory()?;
        let s = store.settings();
        s.set_all(&[("a", json!(1)), ("b", json!("x"))])?;
        assert_eq!(s.all()?.len(), 2);
        assert_eq!(s.get("b")?, Some(json!("x")));
        // A failure part way rolls back the pairs before it.
        store.conn.execute_batch(
            "create trigger no_c before insert on settings when new.key = 'c'
             begin select raise(abort, 'no'); end;",
        )?;
        assert!(s.set_all(&[("a", json!(2)), ("c", json!(3))]).is_err());
        assert_eq!(s.get("a")?, Some(json!(1)));
        Ok(())
    }
}
