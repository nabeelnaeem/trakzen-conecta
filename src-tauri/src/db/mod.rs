use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

use crate::error::Result;

mod migrations;

pub const DB_FILE_NAME: &str = "trakzen-conecta.db";

/// Single shared SQLite connection. SQLite serialises writes anyway, and a
/// mutex keeps the API simple; all queries are short so contention is a
/// non-issue at this scale.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join(DB_FILE_NAME))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        migrations::run(&conn)?;
        crate::mail::store::register_sql_functions(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("db mutex poisoned")
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
