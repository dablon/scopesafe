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

    pub fn run_audit(&self, scope: &Scope, show_diff: bool) -> Result<()> {
        let events = self.tracker.get_events(&scope.id)?;

        let total = events.len();
        let in_scope: Vec<_> = events.iter().filter(|e| e.in_scope).collect();
        let out_of_scope: Vec<_> = events
            .iter()
            .filter(|e| !e.in_scope && !e.is_blocked)
            .collect();
        let blocked: Vec<_> = events.iter().filter(|e| e.is_blocked).collect();

        println!("\n{}", "═══ SCOPE AUDIT ═══".bold());
        println!("Task: {}", scope.task);
        println!("Scope ID: {}", scope.id);
        println!(
            "Created: {}",
            scope.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
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
            for (_path, event) in &file_map {
                if event.is_blocked {
                    println!("  {} {} [BLOCKED — secrets file]", "⛔".red(), _path.red());
                }
            }
        }

        // Approval-aware scope score:
        // in-scope files = full credit
        // approved out-of-scope = full credit (treated as if in scope)
        // rejected out-of-scope = no credit
        // pending out-of-scope = 0.5 credit (still in limbo)
        // blocked = no credit
        let in_scope_count = in_scope.len();
        let mut credit = in_scope_count as f32;
        let mut pending = 0usize;
        for e in &out_of_scope {
            match e.approved {
                Some(true) => credit += 1.0,
                Some(false) => {}
                None => {
                    credit += 0.5;
                    pending += 1;
                }
            }
        }
        let score = if total > 0 {
            (credit / total as f32) * 100.0
        } else {
            100.0
        };

        println!();
        println!("{}", "═══ SUMMARY ═══".bold());
        println!("  Total files tracked: {}", total);
        println!("  In scope: {}", in_scope_count);
        println!("  Out of scope (pending): {}", pending);
        println!("  Out of scope (approved): {}", out_of_scope.iter().filter(|e| e.approved == Some(true)).count());
        println!("  Out of scope (rejected): {}", out_of_scope.iter().filter(|e| e.approved == Some(false)).count());
        println!("  Blocked: {}", blocked.len());
        println!("  Scope score: {:.0}%", score);

        if show_diff {
            println!();
            println!("{}", "═══ DIFFS ═══".bold());
            for event in events
                .iter()
                .filter(|e| !e.is_blocked)
            {
                if event.approved == Some(false) {
                    continue;
                }
                let diff = git_diff(&scope.project, &event.file_path);
                match diff {
                    Ok(Some(d)) if !d.trim().is_empty() => {
                        println!("\n--- {} ({})", event.file_path, event.action);
                        for line in d.lines().take(40) {
                            if line.starts_with('+') {
                                println!("{}", line.green());
                            } else if line.starts_with('-') {
                                println!("{}", line.red());
                            } else {
                                println!("{}", line);
                            }
                        }
                        if d.lines().count() > 40 {
                            println!("  ... ({} more lines)", d.lines().count() - 40);
                        }
                    }
                    _ => {}
                }
            }
        }

        if pending + out_of_scope.iter().filter(|e| e.approved.is_none()).count() > 0
            || !out_of_scope.is_empty()
        {
            let any_pending = out_of_scope.iter().any(|e| e.approved.is_none());
            if any_pending {
            println!();
            println!("{}", "ACTIONS AVAILABLE:".cyan());
            println!("  scopesafe approve --file=<path>    # approve out-of-scope change");
            println!("  scopesafe reject --file=<path>     # reject with reason");
            println!("  scopesafe revert --all-out-of-scope  # revert all out-of-scope");
            }
        }

        Ok(())
    }

    pub fn revert_out_of_scope(&self, scope: &Scope) -> Result<usize> {
        let events = self.tracker.get_events(&scope.id)?;
        let mut reverted = 0;

        for event in events.iter().filter(|e| !e.in_scope && !e.is_blocked) {
            // Skip approved files
            if event.approved == Some(true) {
                continue;
            }
            // Skip already-rejected files (just acknowledge)
            if event.approved == Some(false) {
                println!("Skipping rejected (already marked): {}", event.file_path);
                continue;
            }

            match revert_file(&scope.project, &event.file_path) {
                Ok(true) => {
                    println!("Reverted: {}", event.file_path);
                    reverted += 1;
                }
                Ok(false) => {
                    println!(
                        "Could not revert (no git tracking or uncommitted): {}",
                        event.file_path
                    );
                }
                Err(e) => {
                    eprintln!("Error reverting {}: {}", event.file_path, e);
                }
            }
        }

        Ok(reverted)
    }
}

/// Return the `git diff` for a single file, or None if the file is not tracked
/// or has no diff. Truncates to a sensible length.
pub fn git_diff(project_root: &std::path::Path, file_path: &str) -> Result<Option<String>> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["diff", "HEAD", "--", file_path])
        .current_dir(project_root)
        .output()?;

    if !output.status.success() {
        // No HEAD yet (initial commit) — diff against the empty tree
        let staged = Command::new("git")
            .args(["diff", "--", file_path])
            .current_dir(project_root)
            .output()?;
        if staged.status.success() {
            let s = String::from_utf8_lossy(&staged.stdout).to_string();
            return Ok(if s.is_empty() { None } else { Some(s) });
        }
        return Ok(None);
    }

    let diff_text = String::from_utf8_lossy(&output.stdout).to_string();
    if diff_text.trim().is_empty() {
        // No diff vs HEAD — likely an untracked new file. Show contents.
        let full_path = project_root.join(file_path);
        if full_path.exists() {
            let content = std::fs::read_to_string(&full_path).unwrap_or_default();
            if content.is_empty() {
                return Ok(None);
            }
            let mut out = String::new();
            for line in content.lines() {
                out.push('+');
                out.push_str(line);
                out.push('\n');
            }
            return Ok(Some(out));
        }
        return Ok(None);
    }
    Ok(Some(diff_text))
}

/// Revert a single file to its last committed state using `git checkout HEAD -- <path>`.
/// Returns Ok(true) if reverted, Ok(false) if the file is not tracked by git or the
/// file no longer differs from HEAD, and Err if git itself errored.
pub fn revert_file(project_root: &std::path::Path, file_path: &str) -> Result<bool> {
    use std::process::Command;

    let full_path = project_root.join(file_path);
    if !full_path.exists() {
        // File was created during the run, just remove it (best effort)
        let _ = std::fs::remove_file(&full_path);
        return Ok(true);
    }

    // Check if file is tracked by git
    let tracked = Command::new("git")
        .args(["ls-files", "--error-unmatch", "--", file_path])
        .current_dir(project_root)
        .output()?;
    if !tracked.status.success() {
        return Ok(false);
    }

    // `git checkout HEAD -- <path>` restores the file to the last committed state
    let out = Command::new("git")
        .args(["checkout", "HEAD", "--", file_path])
        .current_dir(project_root)
        .output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git checkout failed: {}", stderr.trim());
    }
    Ok(true)
}

impl ScopeStatus {
    pub fn from_scope(scope: &Scope, tracker: &Tracker) -> Result<Self> {
        let events = tracker.get_events(&scope.id)?;

        let total_files = events.len();
        let in_scope_files = events.iter().filter(|e| e.in_scope).count();
        let out_of_scope_files = events
            .iter()
            .filter(|e| !e.in_scope && !e.is_blocked)
            .count();
        let blocked_files = events.iter().filter(|e| e.is_blocked).count();
        let approved_files = events.iter().filter(|e| e.approved == Some(true)).count();

        let scope_score = crate::report::compute_score(&events);

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
