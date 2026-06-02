use crate::db::Database;
use crate::error::Error;
use crate::scope::Scope;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEvent {
    pub id: i64,
    pub scope_id: String,
    pub file_path: String,
    pub action: String,
    pub timestamp: DateTime<Utc>,
    pub approved: Option<bool>,
    pub approved_by: Option<String>,
    pub rejection_reason: Option<String>,
    pub in_scope: bool,
    pub is_blocked: bool,
}

pub struct Tracker {
    pub db: Database,
}

impl Tracker {
    pub fn new() -> Result<Self> {
        let db = Database::new()?;
        Ok(Self { db })
    }

    pub fn save_scope(&self, scope: &Scope) -> Result<()> {
        self.db.save_scope(scope)
    }

    pub fn get_active_scope(&self) -> Result<Scope> {
        self.db.get_active_scope()
    }

    pub fn track_file(
        &self,
        scope_id: &str,
        file_path: &str,
        action: &str,
        in_scope: bool,
        is_blocked: bool,
    ) -> Result<FileEvent> {
        if is_blocked {
            anyhow::bail!(Error::PermissionDenied(format!(
                "blocked file cannot be modified: {}",
                file_path
            )));
        }

        let event = FileEvent {
            id: 0,
            scope_id: scope_id.to_string(),
            file_path: file_path.to_string(),
            action: action.to_string(),
            timestamp: Utc::now(),
            approved: None,
            approved_by: None,
            rejection_reason: None,
            in_scope,
            is_blocked,
        };

        self.db.save_event(&event)
    }

    pub fn get_events(&self, scope_id: &str) -> Result<Vec<FileEvent>> {
        self.db.get_events(scope_id)
    }

    pub fn approve_file(&self, file_path: &str) -> Result<()> {
        self.db.approve_file(file_path)
    }

    pub fn reject_file(&self, file_path: &str, reason: &str) -> Result<()> {
        self.db.reject_file(file_path, reason)
    }

    #[allow(dead_code)]
    pub fn complete_scope(&self, scope_id: &str) -> Result<()> {
        self.db.complete_scope(scope_id)
    }
}
