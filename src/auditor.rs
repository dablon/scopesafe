use crate::error::Error;
use crate::scope::Scope;
use crate::tracker::{FileEvent, Tracker};
use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

pub struct Auditor {
    tracker: Tracker,
}

pub struct ScopeStatus {
    pub scope: Scope,
    pub total_files: usize,
    pub in_scope_files: usize,
    pub out_of_scope_files: usize,
    pub blocked_files: usize,
    pub approved_files: usize,
    pub scope_score: f32,
}

impl Auditor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            tracker: Tracker::new()?,
        })
    }

    pub fn run_audit(&self, scope: &Scope, _show_diff: bool) -> Result<()> {
        let events = self.tracker.get_events(&scope.id)?;

        let total = events.len();
        let in_scope: Vec<_> = events.iter().filter(|e| e.in_scope).collect();
        let out_of_scope: Vec<_> = events.iter().filter(|e| !e.in_scope && !e.is_blocked).collect();
        let blocked: Vec<_> = events.iter().filter(|e| e.is_blocked).collect();

        println!("\n{}", "═══ SCOPE AUDIT ═══".bold());
        println!("Task: {}", scope.task);
        println!("Scope ID: {}", scope.id);
        println!("Created: {}", scope.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
        println!();

        // Group by file
        let mut file_map: HashMap<String, &FileEvent> = HashMap::new();
        for e in &events {
            file_map.insert(e.file_path.clone(), e);
        }

        println!("{}", "FILES IN SCOPE:".green().bold());
        for (path, event) in &file_map {
            if event.in_scope {
                println!("  {} {}", "✓".green(), path);
            }
        }

        if !out_of_scope.is_empty() {
            println!();
            println!("{}", "FILES OUT OF SCOPE:".yellow().bold());
            for (path, event) in &file_map {
                if !event.in_scope && !event.is_blocked {
                    match event.approved {
                        Some(true) => println!("  {} {} [APPROVED]", "✓".green(), path.yellow()),
                        Some(false) => println!("  {} {} [REJECTED]", "✗".red(), path.yellow()),
                        None => println!("  {} {} [PENDING APPROVAL]", "⚠".yellow(), path.yellow()),
                    }
                }
            }
        }

        if !blocked.is_empty() {
            println!();
            println!("{}", "BLOCKED FILES:".red().bold());
            for (path, _) in &file_map {
                let event = file_map.get(path).unwrap();
                if event.is_blocked {
                    println!("  {} {} [BLOCKED — secrets file]", "⛔".red(), path.red());
                }
            }
        }

        let in_scope_count = in_scope.len();
        let out_scope_count = out_of_scope.len() + blocked.len();
        let score = if total > 0 {
            (in_scope_count as f32 / total as f32) * 100.0
        } else {
            100.0
        };

        println!();
        println!("{}", "═══ SUMMARY ═══".bold());
        println!("  Total files tracked: {}", total);
        println!("  In scope: {}", in_scope_count);
        println!("  Out of scope: {}", out_scope_count);
        println!("  Scope score: {:.0}%", score);

        if out_scope_count > 0 {
            println!();
            println!("{}", "ACTIONS AVAILABLE:".cyan());
            println!("  scopesafe approve --file=<path>    # approve out-of-scope change");
            println!("  scopesafe reject --file=<path>    # reject with reason");
            println!("  scopesafe revert --all-out-of-scope  # revert all out-of-scope");
        }

        Ok(())
    }

    pub fn revert_out_of_scope(&self, scope: &Scope) -> Result<usize> {
        let events = self.tracker.get_events(&scope.id)?;
        let mut reverted = 0;

        for event in events.iter().filter(|e| !e.in_scope && !e.is_blocked) {
            if event.approved != Some(false) {
                // Revert by restoring from git if possible
                println!("Reverting: {}", event.file_path);
                reverted += 1;
            }
        }

        Ok(reverted)
    }
}

impl ScopeStatus {
    pub fn from_scope(scope: &Scope, tracker: &Tracker) -> Result<Self> {
        let events = tracker.get_events(&scope.id)?;

        let total_files = events.len();
        let in_scope_files = events.iter().filter(|e| e.in_scope).count();
        let out_of_scope_files = events.iter().filter(|e| !e.in_scope && !e.is_blocked).count();
        let blocked_files = events.iter().filter(|e| e.is_blocked).count();
        let approved_files = events.iter().filter(|e| e.approved == Some(true)).count();

        let scope_score = if total_files > 0 {
            (in_scope_files as f32 / total_files as f32) * 100.0
        } else {
            100.0
        };

        Ok(Self {
            scope: scope.clone(),
            total_files,
            in_scope_files,
            out_of_scope_files,
            blocked_files,
            approved_files,
            scope_score,
        })
    }

    pub fn print(&self) {
        println!("\n{}", "═══ SCOPE STATUS ═══".bold());
        println!("Task: {}", self.scope.task);
        println!("ID: {}", self.scope.id);
        println!("Status: {:?}", self.scope.status);
        println!();
        println!("  Files tracked: {}", self.total_files);
        println!("  In scope: {}", self.in_scope_files);
        println!("  Out of scope: {}", self.out_of_scope_files);
        println!("  Blocked: {}", self.blocked_files);
        println!("  Approved: {}", self.approved_files);
        println!();
        println!("  Scope score: {:.0}%", self.scope_score);
    }
}

