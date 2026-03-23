pub mod queries;
pub mod schema;

use crate::AppResult;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

pub fn db_path() -> PathBuf {
    if let Ok(path) = std::env::var("HARN_DB_PATH") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".harn")
        .join("harn.db")
}

pub fn connect() -> AppResult<Connection> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_millis(5_000))?;
    schema::init(&mut conn)?;
    Ok(conn)
}
