use crate::domain::Usage;
use rusqlite::{Connection, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex};

/// Integer nano-RMB avoids rounding a budget cap down across many small calls.
pub const RMB: u64 = 1_000_000_000;

#[derive(Debug, thiserror::Error)]
#[error("campaign money budget exhausted")]
pub struct BudgetReached;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Prices {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub search: u64,
    pub fetch: u64,
}
impl Prices {
    pub fn cost(&self, usage: &Usage) -> u64 {
        usage
            .input
            .saturating_sub(usage.cached)
            .saturating_mul(self.input)
            .saturating_add(usage.cached.min(usage.input).saturating_mul(self.cached))
            .saturating_add(usage.output.saturating_mul(self.output))
    }
    pub fn reserve(&self, input: u64, output: u64) -> u64 {
        input
            .saturating_mul(self.input)
            .saturating_add(output.saturating_mul(self.output))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Balance {
    pub limit: u64,
    pub used: u64,
    pub uncertain: u64,
    pub reserved: u64,
}

pub struct Money {
    connection: Mutex<Connection>,
    account: String,
    owner: crate::tools::identity::Identity,
    session: Option<String>,
}

/// Reports read the same account without creating accounts, settling reservations
/// or depending on mutable configuration files.
pub struct Ledger {
    connection: Connection,
    account: String,
}
impl Ledger {
    pub fn open(path: &Path, account: &str) -> anyhow::Result<Self> {
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        read_balance(&connection, account)?;
        Ok(Self {
            connection,
            account: account.into(),
        })
    }
    pub fn balance(&self) -> anyhow::Result<Balance> {
        read_balance(&self.connection, &self.account)
    }
    pub fn session_balance(&self, session: &str) -> anyhow::Result<Balance> {
        read_session(&self.connection, &self.account, session)
    }
    pub fn prices(&self) -> anyhow::Result<Prices> {
        let raw: String = self.connection.query_row(
            "SELECT prices FROM accounts WHERE name=?1",
            [&self.account],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    }
}
impl Money {
    pub fn open(path: &Path, account: &str, limit: u64, prices: &Prices) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS accounts(name TEXT PRIMARY KEY, cap INTEGER NOT NULL,
                used INTEGER NOT NULL DEFAULT 0, uncertain INTEGER NOT NULL DEFAULT 0, prices TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS charges(id TEXT PRIMARY KEY, account TEXT NOT NULL,
                amount INTEGER NOT NULL, state TEXT NOT NULL, actual INTEGER);
            CREATE INDEX IF NOT EXISTS charges_account ON charges(account,state);
            CREATE TABLE IF NOT EXISTS charge_owners(id TEXT PRIMARY KEY, identity TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS charge_sessions(id TEXT PRIMARY KEY, session TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS charge_session ON charge_sessions(session);")?;
        let encoded = serde_json::to_string(prices)?;
        connection.execute(
            "INSERT INTO accounts(name,cap,prices) VALUES(?1,?2,?3) ON CONFLICT(name) DO NOTHING",
            params![account, limit, encoded],
        )?;
        let stored: (u64, String) = connection.query_row(
            "SELECT cap,prices FROM accounts WHERE name=?1",
            [account],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        anyhow::ensure!(
            stored == (limit, encoded),
            "existing campaign budget or price schedule differs; cannot silently reset it"
        );
        let money = Self {
            session: None,
            connection: Mutex::new(connection),
            account: account.into(),
            owner: crate::tools::identity::Identity::capture(std::process::id())?
                .ok_or_else(|| anyhow::anyhow!("cannot identify ledger owner"))?,
        };
        money.recover()?;
        Ok(money)
    }
    pub fn reserve(&self, amount: u64) -> anyhow::Result<String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("money lock poisoned"))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let balance = read_balance(&transaction, &self.account)?;
        let available = balance
            .limit
            .saturating_sub(balance.used)
            .saturating_sub(balance.uncertain)
            .saturating_sub(balance.reserved);
        if amount > available {
            return Err(BudgetReached.into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO charges(id,account,amount,state) VALUES(?1,?2,?3,'reserved')",
            params![id, self.account, amount],
        )?;
        transaction.execute(
            "INSERT INTO charge_owners(id,identity) VALUES(?1,?2)",
            params![id, serde_json::to_string(&self.owner)?],
        )?;
        if let Some(session) = &self.session {
            transaction.execute(
                "INSERT INTO charge_sessions(id,session) VALUES(?1,?2)",
                params![id, session],
            )?;
        }
        transaction.commit()?;
        Ok(id)
    }
    pub fn for_session(mut self, session: &str) -> Self {
        self.session = Some(session.into());
        self
    }
    pub fn session_balance(&self, session: &str) -> anyhow::Result<Balance> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("money lock poisoned"))?;
        read_session(&connection, &self.account, session)
    }
    pub fn settle(&self, id: &str, amount: Option<u64>) -> anyhow::Result<()> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("money lock poisoned"))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (reserved, state): (u64, String) = transaction.query_row(
            "SELECT amount,state FROM charges WHERE id=?1 AND account=?2",
            params![id, self.account],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        anyhow::ensure!(state == "reserved", "charge was already settled");
        if let Some(actual) = amount {
            transaction.execute(
                "UPDATE accounts SET used=used+?1 WHERE name=?2",
                params![actual, self.account],
            )?;
        } else {
            transaction.execute(
                "UPDATE accounts SET uncertain=uncertain+?1 WHERE name=?2",
                params![reserved, self.account],
            )?;
        }
        transaction.execute(
            "UPDATE charges SET state=?1,actual=?2 WHERE id=?3",
            params![
                if amount.is_some() {
                    "settled"
                } else {
                    "uncertain"
                },
                amount,
                id
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub fn balance(&self) -> anyhow::Result<Balance> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("money lock poisoned"))?;
        read_balance(&connection, &self.account)
    }
    fn recover(&self) -> anyhow::Result<()> {
        let abandoned: Vec<(String, String)> = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| anyhow::anyhow!("money lock poisoned"))?;
            let mut query = connection.prepare("SELECT c.id,o.identity FROM charges c JOIN charge_owners o ON c.id=o.id WHERE c.account=?1 AND c.state='reserved'")?;
            query
                .query_map([&self.account], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<_, _>>()?
        };
        for (id, raw) in abandoned {
            let owner: crate::tools::identity::Identity = serde_json::from_str(&raw)?;
            if crate::tools::identity::Identity::capture(owner.pid)?.as_ref() != Some(&owner) {
                // Other processes can discover the same orphan simultaneously.
                // Only one settlement succeeds; the second reads the already settled state.
                if let Err(error) = self.settle(&id, None) {
                    let connection = self
                        .connection
                        .lock()
                        .map_err(|_| anyhow::anyhow!("money lock poisoned"))?;
                    let state: String = connection.query_row(
                        "SELECT state FROM charges WHERE id=?1",
                        [&id],
                        |row| row.get(0),
                    )?;
                    if state == "reserved" {
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
}

fn read_balance(connection: &Connection, account: &str) -> anyhow::Result<Balance> {
    Ok(connection.query_row(
        "SELECT cap,used,uncertain,
        (SELECT COALESCE(SUM(amount),0) FROM charges WHERE account=?1 AND state='reserved')
        FROM accounts WHERE name=?1",
        [account],
        |row| {
            Ok(Balance {
                limit: row.get(0)?,
                used: row.get(1)?,
                uncertain: row.get(2)?,
                reserved: row.get(3)?,
            })
        },
    )?)
}

fn read_session(connection: &Connection, account: &str, session: &str) -> anyhow::Result<Balance> {
    let mut balance = read_balance(connection, account)?;
    let values: (u64, u64, u64) = connection.query_row(
        "SELECT COALESCE(SUM(CASE WHEN state='settled' THEN actual ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN state='uncertain' THEN amount ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN state='reserved' THEN amount ELSE 0 END),0)
         FROM charges JOIN charge_sessions USING(id) WHERE account=?1 AND session=?2",
        params![account, session],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    balance.used = values.0;
    balance.uncertain = values.1;
    balance.reserved = values.2;
    Ok(balance)
}
