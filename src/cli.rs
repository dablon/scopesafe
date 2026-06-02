use crate::auditor::{Auditor, ScopeStatus};
use crate::patterns::PatternAnalyzer;
use crate::report::ReportGenerator;
use crate::scope::Scope;
use crate::tracker::Tracker;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "scopesafe")]
#[command(about = "AI Agent Scope Guardrail — define scope, track changes, audit results", long_about = None)]
#[command(version = "0.1.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new scope for a task
    Init {
        /// Task description
        #[arg(short, long)]
        task: String,
        /// Files to include (glob patterns, comma-separated)
        #[arg(short, long)]
        files: Option<String>,
        /// Files to exclude (glob patterns, comma-separated)
        #[arg(short, long)]
        exclude: Option<String>,
        /// Project root (defaults to current directory)
        #[arg(short, long)]
        project: Option<PathBuf>,
    },
    /// Track a file operation
    Track {
        /// File path
        #[arg(short, long)]
        file: String,
        /// Action type
        #[arg(short, long)]
        action: String,
    },
    /// Audit the current scope and generate a report
    Audit {
        /// Show diff for out-of-scope files
        #[arg(short, long, default_value_t = false)]
        show_diff: bool,
    },
    /// Show current scope status
    Status,
    /// Approve an out-of-scope file change
    Approve {
        /// File path to approve
        #[arg(short, long)]
        file: String,
    },
    /// Reject an out-of-scope file change
    Reject {
        /// File path to reject
        #[arg(short, long)]
        file: String,
        /// Reason for rejection
        #[arg(short, long)]
        reason: String,
    },
    /// Revert all out-of-scope changes
    RevertAll,
    /// Generate a weekly drift report
    ReportWeekly,
    /// Run as MCP server
    Mcp,
}

impl Cli {
    pub fn execute(&self) -> Result<()> {
        match &self.command {
            Commands::Init {
                task,
                files,
                exclude,
                project,
            } => {
                let scope = Scope::new(
                    task.clone(),
                    files.clone(),
                    exclude.clone(),
                    project.clone(),
                )?;
                let tracker = Tracker::new()?;
                tracker.save_scope(&scope)?;
                println!("✓ Scope created: {}", task);
                println!("  ID: {}", scope.id);
                println!("  Files: {:?}", scope.files_pattern());
                if let Some(excl) = &scope.exclude_pattern() {
                    println!("  Exclude: {:?}", excl);
                }
                Ok(())
            }
            Commands::Track { file, action } => {
                let tracker = Tracker::new()?;
                let active_scope = tracker.get_active_scope()?;

                let in_scope = active_scope.is_file_in_scope(file);
                let is_blocked = active_scope.is_blocked_file(file);

                let _event =
                    tracker.track_file(&active_scope.id, file, action, in_scope, is_blocked)?;

                if is_blocked {
                    println!("⛔ BLOCKED: {} (secrets file)", file);
                } else if in_scope {
                    println!("✓ tracked: {} (in scope)", file);
                } else {
                    println!("⚠  tracked: {} (OUT OF SCOPE)", file);
                }

                Ok(())
            }
            Commands::Audit { show_diff } => {
                let tracker = Tracker::new()?;
                let auditor = Auditor::new()?;
                let active_scope = tracker.get_active_scope()?;

                auditor.run_audit(&active_scope, *show_diff)
            }
            Commands::Status => {
                let tracker = Tracker::new()?;
                let active_scope = tracker.get_active_scope()?;

                let status = ScopeStatus::from_scope(&active_scope, &tracker)?;
                status.print();

                Ok(())
            }
            Commands::Approve { file } => {
                let tracker = Tracker::new()?;
                tracker.approve_file(file)?;
                println!("✓ approved: {}", file);
                Ok(())
            }
            Commands::Reject { file, reason } => {
                let tracker = Tracker::new()?;
                tracker.reject_file(file, reason)?;
                println!("✗ rejected: {} — {}", file, reason);
                Ok(())
            }
            Commands::RevertAll => {
                let tracker = Tracker::new()?;
                let auditor = Auditor::new()?;
                let active_scope = tracker.get_active_scope()?;

                let reverted = auditor.revert_out_of_scope(&active_scope)?;
                println!("✓ reverted {} out-of-scope files", reverted);
                Ok(())
            }
            Commands::ReportWeekly => {
                let tracker = Tracker::new()?;
                let patterns = PatternAnalyzer::new()?;
                let report = ReportGenerator::new()?;

                let weekly = report.generate_weekly(&tracker, &patterns)?;
                weekly.print();

                Ok(())
            }
            Commands::Mcp => {
                // MCP server mode — future v1.0
                anyhow::bail!("MCP server mode coming in v1.0")
            }
        }
    }
}
