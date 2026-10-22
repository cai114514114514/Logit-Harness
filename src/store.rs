use crate::domain::{EventData, SessionEvent};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// Concurrent daemons share one database file; a blocked writer waits this
/// long for the current transaction before failing with SQLITE_BUSY.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
            PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY, state TEXT NOT NULL, updated TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                session TEXT NOT NULL, timestamp TEXT NOT NULL, data TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS events_session ON events(session, sequence);
            CREATE TABLE IF NOT EXISTS controls (
                id INTEGER PRIMARY KEY AUTOINCREMENT, session TEXT NOT NULL, data TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS controls_session ON controls(session,id);
            CREATE TABLE IF NOT EXISTS control_keys (
                session TEXT NOT NULL, key TEXT NOT NULL, control INTEGER NOT NULL,
                PRIMARY KEY(session,key)
            );",
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    /// Lock the single shared connection. A poisoned lock means a previous
    /// holder panicked mid-transaction; every call treats that as
    /// unrecoverable rather than risking a torn commit.
    fn conn(&self) -> anyhow::Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))
    }
    /// The replay event and its resulting snapshot commit together, including tool-start intents.
    pub fn commit<T: Serialize>(
        &self,
        session: &str,
        state: &T,
        data: EventData,
    ) -> anyhow::Result<SessionEvent> {
        let mut connection = self.conn()?;
        let transaction = connection.transaction()?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        transaction.execute(
            "INSERT INTO sessions(id, state, updated) VALUES(?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET state=excluded.state, updated=excluded.updated",
            params![session, serde_json::to_string(state)?, timestamp],
        )?;
        transaction.execute(
            "INSERT INTO events(session, timestamp, data) VALUES(?1, ?2, ?3)",
            params![session, timestamp, serde_json::to_string(&data)?],
        )?;
        // AUTOINCREMENT rowids are positive monotonically increasing i64s, so
        // this conversion cannot fail in practice; a negative rowid would be a
        // database corruption signal worth surfacing instead of wrapping.
        let sequence = u64::try_from(transaction.last_insert_rowid())?;
        transaction.commit()?;
        Ok(SessionEvent {
            sequence,
            session: session.into(),
            timestamp,
            data,
        })
    }
    pub fn load<T: DeserializeOwned>(&self, session: &str) -> anyhow::Result<T> {
        let connection = self.conn()?;
        let state: String = connection
            .query_row("SELECT state FROM sessions WHERE id=?1", [session], |row| {
                row.get(0)
            })
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    anyhow::anyhow!("no persisted state for session {session}")
                }
                error => error.into(),
            })?;
        decode(&state, session)
    }
    pub fn has_session(&self, session: &str) -> anyhow::Result<bool> {
        Ok(self.conn()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)",
            [session],
            |row| row.get(0),
        )?)
    }
    pub fn snapshot<T: DeserializeOwned>(&self, session: &str) -> anyhow::Result<(T, u64)> {
        let connection = self.conn()?;
        let (state, sequence): (String, u64) = connection.query_row(
            "SELECT state,
            (SELECT COALESCE(MAX(sequence),0) FROM events WHERE session=?1)
            FROM sessions WHERE id=?1",
            [session],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok((decode(&state, session)?, sequence))
    }
    pub fn append(&self, session: &str, data: EventData) -> anyhow::Result<SessionEvent> {
        let connection = self.conn()?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        connection.execute(
            "INSERT INTO events(session,timestamp,data) VALUES(?1,?2,?3)",
            params![session, timestamp, serde_json::to_string(&data)?],
        )?;
        // See commit: rowids are positive monotonically increasing i64s.
        let sequence = u64::try_from(connection.last_insert_rowid())?;
        Ok(SessionEvent {
            sequence,
            session: session.into(),
            timestamp,
            data,
        })
    }
    pub fn enqueue(&self, session: &str, data: &serde_json::Value) -> anyhow::Result<u64> {
        let connection = self.conn()?;
        connection.execute(
            "INSERT INTO controls(session,data) VALUES(?1,?2)",
            params![session, serde_json::to_string(data)?],
        )?;
        // See commit: rowids are positive monotonically increasing i64s.
        Ok(u64::try_from(connection.last_insert_rowid())?)
    }
    pub fn queued(
        &self,
        session: &str,
        after: u64,
    ) -> anyhow::Result<Vec<(u64, serde_json::Value)>> {
        let connection = self.conn()?;
        let mut query = connection
            .prepare("SELECT id,data FROM controls WHERE session=?1 AND id>?2 ORDER BY id")?;
        let rows = query.query_map(params![session, after], |row| {
            Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (id, data) = row?;
            Ok((id, serde_json::from_str(&data)?))
        })
        .collect()
    }
    /// A benchmark correction or resumed client operation can be retried without adding a
    /// second message. The same key with different content is a protocol error, not an update.
    pub fn enqueue_once(
        &self,
        session: &str,
        key: &str,
        data: &serde_json::Value,
    ) -> anyhow::Result<u64> {
        let mut connection = self.conn()?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let saved:Option<(u64,String)>=transaction.query_row(
            "SELECT controls.id,controls.data FROM control_keys JOIN controls ON controls.id=control_keys.control WHERE control_keys.session=?1 AND control_keys.key=?2",
            params![session,key],|row|Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        if let Some((id, saved)) = saved {
            anyhow::ensure!(
                serde_json::from_str::<serde_json::Value>(&saved)? == *data,
                "control idempotency key changed meaning"
            );
            return Ok(id);
        }
        transaction.execute(
            "INSERT INTO controls(session,data) VALUES(?1,?2)",
            params![session, serde_json::to_string(data)?],
        )?;
        let id = u64::try_from(transaction.last_insert_rowid())?;
        transaction.execute(
            "INSERT INTO control_keys(session,key,control) VALUES(?1,?2,?3)",
            params![session, key, id],
        )?;
        transaction.commit()?;
        Ok(id)
    }
    pub fn control_key(
        &self,
        session: &str,
        key: &str,
    ) -> anyhow::Result<Option<(u64, serde_json::Value)>> {
        let connection = self.conn()?;
        let saved:Option<(u64,String)>=connection.query_row(
            "SELECT controls.id,controls.data FROM control_keys JOIN controls ON controls.id=control_keys.control WHERE control_keys.session=?1 AND control_keys.key=?2",
            params![session,key],|row|Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        saved
            .map(|(id, data)| Ok((id, serde_json::from_str(&data)?)))
            .transpose()
    }
    /// Returns every persisted session snapshot without pagination. Sessions
    /// are rare (one per user task), so a full scan stays cheap; revisit if a
    /// long-lived deployment accumulates thousands.
    pub fn snapshots(&self) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
        let connection = self.conn()?;
        let mut query =
            connection.prepare("SELECT id,state FROM sessions ORDER BY updated DESC")?;
        let rows = query.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (id, state) = row?;
            Ok((id, serde_json::from_str(&state)?))
        })
        .collect()
    }
    /// Replays every event after `after` with no pagination. A chatty session
    /// (one row per streamed delta) can make this large; subscribers use it
    /// only to catch up after `after`, and lagged-replay bursts are short.
    pub fn events(&self, session: &str, after: u64) -> anyhow::Result<Vec<SessionEvent>> {
        let connection = self.conn()?;
        let mut statement = connection.prepare(
            "SELECT sequence, timestamp, data FROM events
            WHERE session=?1 AND sequence>?2 ORDER BY sequence",
        )?;
        let records = statement.query_map(params![session, after], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        records
            .map(|record| {
                let (sequence, timestamp, data) = record?;
                Ok(SessionEvent {
                    sequence,
                    session: session.into(),
                    timestamp,
                    data: serde_json::from_str(&data)?,
                })
            })
            .collect()
    }
}

/// Legacy rows had no goal/turn IDs. Derive their initial identities from the
/// saved session and actor keys, so repeated read-only loads agree before the
/// first new commit persists the migrated fields. Event rows are never rewritten.
fn decode<T: DeserializeOwned>(state: &str, session: &str) -> anyhow::Result<T> {
    let mut value: serde_json::Value = serde_json::from_str(state)?;
    if let Some(goal) = value
        .get_mut("goal")
        .and_then(serde_json::Value::as_object_mut)
    {
        goal.entry("id")
            .or_insert_with(|| serde_json::json!(format!("{session}:goal")));
    }
    if let Some(actors) = value
        .get_mut("actors")
        .and_then(serde_json::Value::as_object_mut)
    {
        for (id, actor) in actors {
            if let Some(actor) = actor.as_object_mut() {
                actor
                    .entry("turn")
                    .or_insert_with(|| serde_json::json!(format!("{session}:{id}:turn")));
            }
        }
    }
    Ok(serde_json::from_value(value)?)
}
