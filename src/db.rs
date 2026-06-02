use crate::error::Error;
use crate::scope::{Scope, ScopeStatus};
use crate::tracker::FileEvent;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = Self::db_path()?;

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Open a database at a specific path. Used by tests to isolate state.
    #[allow(dead_code)]
    pub fn open_at(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn db_path() -> Result<PathBuf> {
        let base = dirs::data_local_dir()
            .or_else(dirs::data_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(base.join("scopesafe").join("scopesafe.db"))
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS scopes (
                id TEXT PRIMARY KEY,
                task TEXT NOT NULL,
                files TEXT,
                exclude TEXT,
                project TEXT NOT NULL,
                created_at TEXT NOT NULL,
                owner TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'Active'
            );

            CREATE TABLE IF NOT EXISTS file_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scope_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                action TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                approved INTEGER,
                approved_by TEXT,
                rejection_reason TEXT,
                in_scope INTEGER NOT NULL,
                is_blocked INTEGER NOT NULL,
                FOREIGN KEY (scope_id) REFERENCES scopes(id)
            );

            CREATE INDEX IF NOT EXISTS idx_events_scope ON file_events(scope_id);
            CREATE INDEX IF NOT EXISTS idx_events_file ON file_events(file_path);
            ",
        )?;
        Ok(())
    }

    pub fn save_scope(&self, scope: &Scope) -> Result<()> {
        let status_str = match scope.status {
            ScopeStatus::Active => "Active",
            ScopeStatus::Completed => "Completed",
            ScopeStatus::Cancelled => "Cancelled",
        };
        self.conn.execute(
            "INSERT INTO scopes (id, task, files, exclude, project, created_at, owner, status) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET status = 'Active'",
            params![
                scope.id,
                scope.task,
                scope.files,
                scope.exclude,
                scope.project.to_string_lossy().to_string(),
                scope.created_at.to_rfc3339(),
                scope.owner,
                status_str,
            ],
        )?;
        Ok(())
    }

    pub fn get_active_scope(&self) -> Result<Scope> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task, files, exclude, project, created_at, owner, status
             FROM scopes WHERE status = 'Active' ORDER BY created_at DESC LIMIT 1",
        )?;

        let scope = stmt
            .query_row([], |row| {
                let project_str: String = row.get(4)?;
                let status_str: String = row.get(7)?;
                let status = match status_str.as_str() {
                    "Completed" => ScopeStatus::Completed,
                    "Cancelled" => ScopeStatus::Cancelled,
                    _ => ScopeStatus::Active,
                };

                Ok(Scope {
                    id: row.get(0)?,
                    task: row.get(1)?,
                    files: row.get(2)?,
                    exclude: row.get(3)?,
                    project: PathBuf::from(project_str),
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .unwrap_or_else(|_| chrono::Utc::now().into())
                        .with_timezone(&chrono::Utc),
                    owner: row.get(6)?,
                    status,
                })
            })
            .map_err(|_| Error::NoActiveScope)?;

        Ok(scope)
    }

    pub fn save_event(&self, event: &FileEvent) -> Result<FileEvent> {
        self.conn.execute(
            "INSERT INTO file_events (scope_id, file_path, action, timestamp, approved, approved_by, rejection_reason, in_scope, is_blocked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                event.scope_id,
                event.file_path,
                event.action,
                event.timestamp.to_rfc3339(),
                event.approved,
                event.approved_by,
                event.rejection_reason,
                event.in_scope as i32,
                event.is_blocked as i32,
            ],
        )?;

        let id = self.conn.last_insert_rowid();
        Ok(FileEvent {
            id,
            ..event.clone()
        })
    }

    pub fn get_events(&self, scope_id: &str) -> Result<Vec<FileEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope_id, file_path, action, timestamp, approved, approved_by, rejection_reason, in_scope, is_blocked
             FROM file_events WHERE scope_id = ?1 ORDER BY timestamp ASC"
        )?;

        let events = stmt.query_map([scope_id], |row| {
            let approved: Option<i32> = row.get(5)?;
            Ok(FileEvent {
                id: row.get(0)?,
                scope_id: row.get(1)?,
                file_path: row.get(2)?,
                action: row.get(3)?,
                timestamp: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .unwrap_or_else(|_| chrono::Utc::now().into())
                    .with_timezone(&chrono::Utc),
                approved: approved.map(|v| v != 0),
                approved_by: row.get(6)?,
                rejection_reason: row.get(7)?,
                in_scope: row.get::<_, i32>(8)? != 0,
                is_blocked: row.get::<_, i32>(9)? != 0,
            })
        })?;

        let events: Result<Vec<_>> = events.map(|r| r.map_err(|e| anyhow::anyhow!(e))).collect();
        events
    }

    pub fn approve_file(&self, file_path: &str) -> Result<()> {
        let rows = self.conn.execute(
            "UPDATE file_events SET approved = 1, approved_by = ?1 
             WHERE file_path = ?2 AND approved IS NULL",
            params![whoami::username(), file_path],
        )?;

        if rows == 0 {
            return Err(Error::FileNotTracked(file_path.to_string()).into());
        }
        Ok(())
    }

    pub fn reject_file(&self, file_path: &str, reason: &str) -> Result<()> {
        let rows = self.conn.execute(
            "UPDATE file_events SET approved = 0, rejection_reason = ?1 
             WHERE file_path = ?2 AND approved IS NULL",
            params![reason, file_path],
        )?;

        if rows == 0 {
            return Err(Error::FileNotTracked(file_path.to_string()).into());
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn complete_scope(&self, scope_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE scopes SET status = 'Completed' WHERE id = ?1",
            [scope_id],
        )?;
        Ok(())
    }

    /// List all scopes in the database, newest first.
    pub fn list_all_scopes(&self) -> Result<Vec<Scope>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task, files, exclude, project, created_at, owner, status
             FROM scopes ORDER BY created_at DESC",
        )?;
        let scopes = stmt
            .query_map([], |row| {
                let project_str: String = row.get(4)?;
                let status_str: String = row.get(7)?;
                let status = match status_str.as_str() {
                    "Completed" => ScopeStatus::Completed,
                    "Cancelled" => ScopeStatus::Cancelled,
                    _ => ScopeStatus::Active,
                };
                Ok(Scope {
                    id: row.get(0)?,
                    task: row.get(1)?,
                    files: row.get(2)?,
                    exclude: row.get(3)?,
                    project: PathBuf::from(project_str),
                    created_at: chrono::DateTime::parse_from_rfc3339(
                        &row.get::<_, String>(5)?,
                    )
                    .unwrap_or_else(|_| chrono::Utc::now().into())
                    .with_timezone(&chrono::Utc),
                    owner: row.get(6)?,
                    status,
                })
            })?
            .map(|r| r.map_err(|e| anyhow::anyhow!(e)))
            .collect::<Result<Vec<_>>>()?;
        Ok(scopes)
    }

    /// List scopes created within `[start, end)`.
    pub fn list_scopes_in_window(
        &self,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<Scope>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task, files, exclude, project, created_at, owner, status
             FROM scopes WHERE created_at >= ?1 AND created_at < ?2
             ORDER BY created_at ASC",
        )?;
        let scopes = stmt
            .query_map(
                rusqlite::params![start.to_rfc3339(), end.to_rfc3339()],
                |row| {
                    let project_str: String = row.get(4)?;
                    let status_str: String = row.get(7)?;
                    let status = match status_str.as_str() {
                        "Completed" => ScopeStatus::Completed,
                        "Cancelled" => ScopeStatus::Cancelled,
                        _ => ScopeStatus::Active,
                    };
                    Ok(Scope {
                        id: row.get(0)?,
                        task: row.get(1)?,
                        files: row.get(2)?,
                        exclude: row.get(3)?,
                        project: PathBuf::from(project_str),
                        created_at: chrono::DateTime::parse_from_rfc3339(
                            &row.get::<_, String>(5)?,
                        )
                        .unwrap_or_else(|_| chrono::Utc::now().into())
                        .with_timezone(&chrono::Utc),
                        owner: row.get(6)?,
                        status,
                    })
                },
            )?
            .map(|r| r.map_err(|e| anyhow::anyhow!(e)))
            .collect::<Result<Vec<_>>>()?;
        Ok(scopes)
    }
}

mod whoami {
    pub fn username() -> String {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string())
    }
}
