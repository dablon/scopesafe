//! Minimal MCP (Model Context Protocol) server implementation for scopesafe.
//!
//! Speaks JSON-RPC 2.0 over stdio. No external MCP SDK — the wire protocol
//! is small and the dependency cost of the official SDK is not worth it for
//! five tools.
//!
//! Tools exposed:
//! - `init_scope`        — initialize a new scope
//! - `track_file`        — track a file change against the active scope
//! - `check_file`        — check if a file would be allowed before editing
//! - `audit`             — generate the audit report
//! - `status`            — show current scope status
//! - `list_scopes`       — list scopes in the DB
//!
//! Resources exposed:
//! - `scope://active`    — JSON of the active scope

use crate::auditor::Auditor;
use crate::db::Database;
use crate::error::Error;
use crate::patterns::PatternAnalyzer;
use crate::report::ReportGenerator;
use crate::scope::Scope;
use crate::tracker::{FileEvent, Tracker};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "scopesafe";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

pub struct McpServer {
    db: Database,
    project_root: std::path::PathBuf,
}

impl McpServer {
    pub fn new(project_root: std::path::PathBuf) -> Result<Self> {
        Ok(Self {
            db: Database::new()?,
            project_root,
        })
    }

    pub fn run(&self) -> Result<()> {
        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut out = stdout.lock();

        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let response = self.handle_line(trimmed);
            if let Some(resp) = response {
                let serialized = serde_json::to_string(&resp)?;
                writeln!(out, "{}", serialized)?;
                out.flush()?;
            }
        }
        Ok(())
    }

    fn handle_line(&self, line: &str) -> Option<JsonRpcResponse> {
        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                return Some(self.error(None, -32700, format!("Parse error: {}", e), None));
            }
        };

        // Notifications (no id) get a response only on error
        let id = req.id.clone().unwrap_or(Value::Null);

        let result = self.dispatch(&req.method, req.params.clone());
        match result {
            Ok(value) => Some(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(value),
                error: None,
            }),
            Err(e) => {
                let code = if let Some(scopesafe_err) = e.downcast_ref::<Error>() {
                    match scopesafe_err {
                        Error::NoActiveScope | Error::FileNotTracked(_) => -32004,
                        Error::PermissionDenied(_) => -32003,
                        _ => -32000,
                    }
                } else {
                    -32000
                };
                Some(self.error(
                    Some(id),
                    code,
                    format!("{}", e),
                    None,
                ))
            }
        }
    }

    fn error(
        &self,
        id: Option<Value>,
        code: i32,
        message: String,
        data: Option<Value>,
    ) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: id.unwrap_or(Value::Null),
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data,
            }),
        }
    }

    pub fn dispatch(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
                }
            })),
            "notifications/initialized" | "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": tools_list()
            })),
            "tools/call" => self.call_tool(params),
            "resources/list" => Ok(json!({
                "resources": [
                    {
                        "uri": "scope://active",
                        "name": "Active Scope",
                        "description": "The currently active scope, if any.",
                        "mimeType": "application/json"
                    }
                ]
            })),
            "resources/read" => self.read_resource(params),
            m => anyhow::bail!("method not found: {}", m),
        }
    }

    fn call_tool(&self, params: Value) -> Result<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing tool name"))?;
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        match name {
            "init_scope" => self.tool_init_scope(arguments),
            "track_file" => self.tool_track_file(arguments),
            "check_file" => self.tool_check_file(arguments),
            "audit" => self.tool_audit(),
            "status" => self.tool_status(),
            "list_scopes" => self.tool_list_scopes(),
            "weekly_report" => self.tool_weekly_report(),
            other => anyhow::bail!("unknown tool: {}", other),
        }
    }

    fn read_resource(&self, params: Value) -> Result<Value> {
        let uri = params
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing uri"))?;
        match uri {
            "scope://active" => {
                let scope = self.db.get_active_scope()?;
                let text = serde_json::to_string_pretty(&scope)?;
                Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": text
                    }]
                }))
            }
            _ => anyhow::bail!("unknown resource: {}", uri),
        }
    }

    // -- tool implementations ----------------------------------------------

    fn tool_init_scope(&self, args: Value) -> Result<Value> {
        let task = required_str(&args, "task")?;
        let files = optional_str(&args, "files");
        let exclude = optional_str(&args, "exclude");
        let project_str = optional_str(&args, "project");
        let project = project_str
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.project_root.clone());

        let scope = Scope::new(task, files, exclude, Some(project))?;
        self.db.save_scope(&scope)?;
        Ok(text_result(format!(
            "Scope created: {}\nID: {}\nFiles: {}\nExclude: {}",
            scope.task,
            scope.id,
            scope.files.as_deref().unwrap_or("(all)"),
            scope.exclude.as_deref().unwrap_or("(none)"),
        )))
    }

    fn tool_track_file(&self, args: Value) -> Result<Value> {
        let file = required_str(&args, "file")?;
        let action = required_str(&args, "action")?;
        let scope = self.db.get_active_scope()?;
        let in_scope = scope.is_file_in_scope(&file);
        let is_blocked = scope.is_blocked_file(&file);

        if is_blocked {
            anyhow::bail!(Error::PermissionDenied(format!(
                "blocked file cannot be modified: {}",
                file
            )));
        }

        let event = FileEvent {
            id: 0,
            scope_id: scope.id.clone(),
            file_path: file.clone(),
            action: action.clone(),
            timestamp: chrono::Utc::now(),
            approved: None,
            approved_by: None,
            rejection_reason: None,
            in_scope,
            is_blocked,
        };
        let saved = self.db.save_event(&event)?;
        let tag = if is_blocked {
            "BLOCKED"
        } else if in_scope {
            "in scope"
        } else {
            "OUT OF SCOPE"
        };
        Ok(text_result(format!(
            "tracked: {} ({}, action={}, event_id={})",
            file, tag, action, saved.id
        )))
    }

    fn tool_check_file(&self, args: Value) -> Result<Value> {
        let file = required_str(&args, "file")?;
        let scope = self.db.get_active_scope()?;
        let in_scope = scope.is_file_in_scope(&file);
        let is_blocked = scope.is_blocked_file(&file);
        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({
                    "file": file,
                    "in_scope": in_scope,
                    "is_blocked": is_blocked,
                    "verdict": if is_blocked { "BLOCKED" } else if in_scope { "ALLOWED" } else { "OUT_OF_SCOPE" }
                }))?
            }]
        }))
    }

    fn tool_audit(&self) -> Result<Value> {
        let scope = self.db.get_active_scope()?;
        let events = self.db.get_events(&scope.id)?;
        let summary = summarize_events(&events);
        Ok(text_result(format!(
            "Task: {}\nScope ID: {}\n\n{}\n\nScope score: {:.0}%",
            scope.task,
            scope.id,
            summary,
            crate::report::compute_score(&events),
        )))
    }

    fn tool_status(&self) -> Result<Value> {
        let scope = self.db.get_active_scope()?;
        let events = self.db.get_events(&scope.id)?;
        let in_scope = events.iter().filter(|e| e.in_scope).count();
        let out_of_scope = events.iter().filter(|e| !e.in_scope && !e.is_blocked).count();
        let blocked = events.iter().filter(|e| e.is_blocked).count();
        Ok(text_result(format!(
            "Task: {}\nID: {}\nStatus: {:?}\n\nTracked: {}\nIn scope: {}\nOut of scope: {}\nBlocked: {}\nScope score: {:.0}%",
            scope.task,
            scope.id,
            scope.status,
            events.len(),
            in_scope,
            out_of_scope,
            blocked,
            crate::report::compute_score(&events),
        )))
    }

    fn tool_list_scopes(&self) -> Result<Value> {
        let scopes = self.db.list_all_scopes()?;
        let mut s = String::new();
        for sc in scopes {
            s.push_str(&format!(
                "{} — {} [{}]\n",
                sc.id,
                sc.task,
                match sc.status {
                    crate::scope::ScopeStatus::Active => "Active",
                    crate::scope::ScopeStatus::Completed => "Completed",
                    crate::scope::ScopeStatus::Cancelled => "Cancelled",
                }
            ));
        }
        if s.is_empty() {
            s.push_str("No scopes yet.");
        }
        Ok(text_result(s))
    }

    fn tool_weekly_report(&self) -> Result<Value> {
        let patterns = PatternAnalyzer::new()?;
        let report = ReportGenerator::new()?;
        let weekly = report.generate_weekly(&patterns)?;
        let mut s = format!(
            "Week {} → {}\nTasks: {} (completed: {})\nAvg scope score: {:.0}%\nFiles tracked: {}\nOut-of-scope: {}\nBlocked: {}\n",
            weekly.week_start,
            weekly.week_end,
            weekly.total_tasks,
            weekly.completed_tasks,
            weekly.avg_scope_score,
            weekly.total_files_tracked,
            weekly.total_out_of_scope,
            weekly.total_blocked_attempts,
        );
        if !weekly.top_offending_files.is_empty() {
            s.push_str("\nTop offending files:\n");
            for (path, count) in weekly.top_offending_files {
                s.push_str(&format!("  - {} ({}x)\n", path, count));
            }
        }
        if !weekly.top_violations.is_empty() {
            s.push_str("\nTop violation patterns:\n");
            for v in weekly.top_violations {
                s.push_str(&format!("  - {} ({}x)\n", v.pattern, v.occurrences));
            }
        }
        Ok(text_result(s))
    }
}

fn text_result(s: String) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": s
        }]
    })
}

fn required_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing required argument: {}", key))
}

fn optional_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn summarize_events(events: &[FileEvent]) -> String {
    use std::collections::HashMap;
    let mut file_map: HashMap<String, &FileEvent> = HashMap::new();
    for e in events {
        file_map.insert(e.file_path.clone(), e);
    }
    let mut s = String::new();
    s.push_str("IN SCOPE:\n");
    for e in file_map.values() {
        if e.in_scope {
            s.push_str(&format!("  ✓ {}\n", e.file_path));
        }
    }
    let oos: Vec<&&FileEvent> = file_map
        .values()
        .filter(|e| !e.in_scope && !e.is_blocked)
        .collect();
    if !oos.is_empty() {
        s.push_str("\nOUT OF SCOPE:\n");
        for e in oos {
            let tag = match e.approved {
                Some(true) => "APPROVED",
                Some(false) => "REJECTED",
                None => "PENDING",
            };
            s.push_str(&format!("  ⚠ {} [{}]\n", e.file_path, tag));
        }
    }
    let blocked: Vec<&&FileEvent> = file_map.values().filter(|e| e.is_blocked).collect();
    if !blocked.is_empty() {
        s.push_str("\nBLOCKED:\n");
        for e in blocked {
            s.push_str(&format!("  ⛔ {}\n", e.file_path));
        }
    }
    s
}

fn tools_list() -> Value {
    json!([
        {
            "name": "init_scope",
            "description": "Initialize a new scope for a task. Defines what files the agent is allowed to touch.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "Task description"},
                    "files": {"type": "string", "description": "Comma-separated glob patterns for allowed files"},
                    "exclude": {"type": "string", "description": "Comma-separated glob patterns for excluded files"},
                    "project": {"type": "string", "description": "Project root path (optional)"}
                },
                "required": ["task"]
            }
        },
        {
            "name": "track_file",
            "description": "Track a file operation against the active scope. Returns whether the file was in scope, out of scope, or blocked.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "File path"},
                    "action": {"type": "string", "description": "Action: create, modify, or delete"}
                },
                "required": ["file", "action"]
            }
        },
        {
            "name": "check_file",
            "description": "Check whether a file would be allowed under the active scope, without logging anything.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "File path"}
                },
                "required": ["file"]
            }
        },
        {
            "name": "audit",
            "description": "Generate the scope audit report for the active scope."
        },
        {
            "name": "status",
            "description": "Show the current scope status (counts, score)."
        },
        {
            "name": "list_scopes",
            "description": "List all scopes in the database."
        },
        {
            "name": "weekly_report",
            "description": "Generate the weekly scope drift report."
        }
    ])
}

// Re-export Tracker to silence unused warnings when the binary does not use it
#[allow(dead_code)]
fn _tracker_anchor() -> Tracker {
    Tracker::new().unwrap()
}

// Pull the Auditor type into the public surface so the binary can call it.
#[allow(dead_code)]
fn _auditor_anchor() -> Auditor {
    Auditor::new().unwrap()
}
