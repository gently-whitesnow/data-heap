use rusqlite::Connection;

use crate::domain::error::{Error, Result};

/// Ordered migration list. Index + 1 is the schema version a migration brings
/// the database to; `PRAGMA user_version` tracks the applied version.
const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_init.sql")];

/// Apply every migration not yet reflected in `user_version`. Idempotent.
pub fn run(conn: &Connection) -> Result<()> {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| Error::Migration(e.to_string()))?;

    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = (idx + 1) as i64;
        if version <= current {
            continue;
        }
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Migration(e.to_string()))?;
        tx.execute_batch(sql)
            .map_err(|e| Error::Migration(format!("migration {version} failed: {e}")))?;
        // user_version does not accept bound parameters.
        tx.execute_batch(&format!("PRAGMA user_version = {version}"))
            .map_err(|e| Error::Migration(e.to_string()))?;
        tx.commit().map_err(|e| Error::Migration(e.to_string()))?;
    }
    Ok(())
}
