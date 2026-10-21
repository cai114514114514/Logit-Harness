use crate::domain::{EventData, SessionEvent};
use rusqlite::{Connection, params};
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;
use std::sync::Mutex;

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
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
            CREATE INDEX IF NOT EXISTS controls_session ON controls(session,id);",
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    /// The replay event and its resulting snapshot commit together, including tool-start intents.
    pub fn commit<T: Serialize>(
        &self,
        session: &str,
        state: &T,
        data: EventData,
    ) -> anyhow::Result<SessionEvent> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
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
        let sequence = transaction.last_insert_rowid() as u64;
        transaction.commit()?;
        Ok(SessionEvent {
            sequence,
            session: session.into(),
            timestamp,
            data,
        })
    }
    pub fn load<T: DeserializeOwned>(&self, session: &str) -> anyhow::Result<T> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let state: String =
            connection.query_row("SELECT state FROM sessions WHERE id=?1", [session], |row| {
                row.get(0)
            })?;
        Ok(serde_json::from_str(&state)?)
    }
    pub fn snapshot<T: DeserializeOwned>(&self, session: &str) -> anyhow::Result<(T, u64)> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let (state, sequence): (String, u64) = connection.query_row(
            "SELECT state,
            (SELECT COALESCE(MAX(sequence),0) FROM events WHERE session=?1)
            FROM sessions WHERE id=?1",
            [session],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok((serde_json::from_str(&state)?, sequence))
    }
    pub fn append(&self, session: &str, data: EventData) -> anyhow::Result<SessionEvent> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        connection.execute(
            "INSERT INTO events(session,timestamp,data) VALUES(?1,?2,?3)",
            params![session, timestamp, serde_json::to_string(&data)?],
        )?;
        Ok(SessionEvent {
            sequence: connection.last_insert_rowid() as u64,
            session: session.into(),
            timestamp,
            data,
        })
    }
    pub fn enqueue(&self, session: &str, data: &serde_json::Value) -> anyhow::Result<u64> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        connection.execute(
            "INSERT INTO controls(session,data) VALUES(?1,?2)",
            params![session, serde_json::to_string(data)?],
        )?;
        Ok(connection.last_insert_rowid() as u64)
    }
    pub fn queued(
        &self,
        session: &str,
        after: u64,
    ) -> anyhow::Result<Vec<(u64, serde_json::Value)>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
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
    pub fn snapshots(&self) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
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
    pub fn events(&self, session: &str, after: u64) -> anyhow::Result<Vec<SessionEvent>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
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
