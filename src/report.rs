use crate::db::Database;
use crate::patterns::{PatternAnalyzer, ViolationPattern};
use crate::tracker::FileEvent;
use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, Utc, Weekday};
use colored::Colorize;
use std::collections::HashMap;

pub struct ReportGenerator {
    db: Database,
}

pub struct WeeklyReport {
    pub week_start: String,
    pub week_end: String,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub avg_scope_score: f32,
    pub top_violations: Vec<ViolationPattern>,
    pub top_offending_files: Vec<(String, usize)>,
    pub agent_breakdown: HashMap<String, AgentStats>,
    pub total_files_tracked: usize,
    pub total_blocked_attempts: usize,
    pub total_out_of_scope: usize,
}

#[derive(Debug, Clone, Default)]
pub struct AgentStats {
    pub tasks: usize,
    pub avg_score: f32,
}

impl ReportGenerator {
    pub fn new() -> Result<Self> {
        Ok(Self {
            db: Database::new()?,
        })
    }

    #[allow(dead_code)]
    pub fn with_db(db: Database) -> Self {
        Self { db }
    }

    pub fn generate_weekly(
        &self,
        patterns: &PatternAnalyzer,
    ) -> Result<WeeklyReport> {
        // ISO week: Monday 00:00 UTC of the current week
        let now = Utc::now();
        let days_from_monday = now.weekday().num_days_from_monday() as i64;
        let week_start_date = (now - Duration::days(days_from_monday))
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();
        let week_end_date = week_start_date + Duration::days(7);

        // Pull all scopes in the window
        let scopes = self.db.list_scopes_in_window(week_start_date, week_end_date)?;
        let mut offending_files: HashMap<String, usize> = HashMap::new();
        let mut agent_stats: HashMap<String, AgentStats> = HashMap::new();
        let mut total_score_sum = 0.0f32;
        let mut total_score_n = 0usize;
        let mut total_files_tracked = 0usize;
        let mut total_blocked = 0usize;
        let mut total_out_of_scope = 0usize;
        let mut completed = 0usize;

        for scope in &scopes {
            let events = self.db.get_events(&scope.id)?;
            total_files_tracked += events.len();
            total_blocked += events.iter().filter(|e| e.is_blocked).count();
            total_out_of_scope +=
                events.iter().filter(|e| !e.in_scope && !e.is_blocked).count();

            if scope.status == crate::scope::ScopeStatus::Completed {
                completed += 1;
            }

            let score = compute_score(&events);
            total_score_sum += score;
            total_score_n += 1;

            let owner = scope.owner.clone();
            let entry = agent_stats.entry(owner).or_default();
            entry.tasks += 1;
            entry.avg_score = ((entry.avg_score * (entry.tasks as f32 - 1.0)) + score)
                / entry.tasks as f32;

            for e in events.iter().filter(|e| !e.in_scope && !e.is_blocked) {
                *offending_files
                    .entry(e.file_path.clone())
                    .or_insert(0usize) += 1;
            }
        }

        let all_violations = patterns.analyze_scope_violations_from_scopes(&scopes, &self.db);
        let mut top_offending_files: Vec<(String, usize)> = offending_files.into_iter().collect();
        top_offending_files.sort_by_key(|a| std::cmp::Reverse(a.1));
        top_offending_files.truncate(5);

        Ok(WeeklyReport {
            week_start: week_start_date.format("%Y-%m-%d").to_string(),
            week_end: (week_end_date - Duration::seconds(1)).format("%Y-%m-%d").to_string(),
            total_tasks: scopes.len(),
            completed_tasks: completed,
            avg_scope_score: if total_score_n > 0 {
                total_score_sum / total_score_n as f32
            } else {
                100.0
            },
            top_violations: all_violations.into_iter().take(5).collect(),
            top_offending_files,
            agent_breakdown: agent_stats,
            total_files_tracked,
            total_blocked_attempts: total_blocked,
            total_out_of_scope,
        })
    }
}

pub fn compute_score(events: &[FileEvent]) -> f32 {
    if events.is_empty() {
        return 100.0;
    }
    let mut credit = 0.0f32;
    for e in events {
        if e.in_scope {
            credit += 1.0;
        } else if e.is_blocked {
            // blocked never credits
        } else {
            match e.approved {
                Some(true) => credit += 1.0,
                Some(false) => {}
                None => credit += 0.5,
            }
        }
    }
    (credit / events.len() as f32) * 100.0
}

impl WeeklyReport {
    pub fn print(&self) {
        println!("\n{}", "═══ WEEKLY SCOPE DRIFT REPORT ═══".bold());
        println!(
            "Week: {} → {} (Mon 00:00 UTC start)",
            self.week_start, self.week_end
        );
        println!();
        println!("{}", "TASK SUMMARY".bold());
        println!("  Total tasks: {}", self.total_tasks);
        println!("  Completed: {}", self.completed_tasks);
        println!("  Avg scope score: {:.0}%", self.avg_scope_score);
        println!("  Files tracked: {}", self.total_files_tracked);
        println!("  Out-of-scope touches: {}", self.total_out_of_scope);
        println!("  Blocked attempts: {}", self.total_blocked_attempts);
        println!();

        if !self.top_violations.is_empty() {
            println!("{}", "TOP VIOLATIONS".yellow().bold());
            for (i, v) in self.top_violations.iter().enumerate() {
                println!(
                    "  {}. {} ({}x) — {}",
                    i + 1,
                    v.pattern.bold(),
                    v.occurrences,
                    v.description
                );
            }
            println!();
        }

        if !self.top_offending_files.is_empty() {
            println!("{}", "TOP OFFENDING FILES".yellow().bold());
            for (i, (path, count)) in self.top_offending_files.iter().enumerate() {
                println!("  {}. {} ({}x touched out-of-scope)", i + 1, path, count);
            }
            println!();
        }

        if !self.agent_breakdown.is_empty() {
            println!("{}", "AGENT / OWNER BREAKDOWN".cyan().bold());
            let mut entries: Vec<(&String, &AgentStats)> =
                self.agent_breakdown.iter().collect();
            entries.sort_by(|a, b| b.1.avg_score.partial_cmp(&a.1.avg_score).unwrap_or(std::cmp::Ordering::Equal));
            for (owner, stats) in entries {
                println!(
                    "  {}: {} tasks, {:.0}% avg scope score",
                    owner, stats.tasks, stats.avg_score
                );
            }
            println!();
        }

        if self.total_tasks == 0 {
            println!(
                "{}",
                "No scopes tracked this week yet. Run 'scopesafe init' to start."
                    .dimmed()
            );
        } else {
            println!("{}", "RECOMMENDATIONS".green().bold());
            for r in self.recommendations() {
                println!("  • {}", r);
            }
        }
    }

    fn recommendations(&self) -> Vec<String> {
        let mut recs = Vec::new();
        if self.total_blocked_attempts > 0 {
            recs.push(format!(
                "Add the {} blocked file(s) to your agent's system prompt as 'do not touch'.",
                self.total_blocked_attempts
            ));
        }
        if self.avg_scope_score < 80.0 && self.total_tasks > 0 {
            recs.push(
                "Avg scope score < 80%. Consider tighter file patterns and stronger exclude rules."
                    .to_string(),
            );
        }
        if !self.top_offending_files.is_empty() {
            let (path, _) = &self.top_offending_files[0];
            recs.push(format!(
                "Most-touched out-of-scope file is '{}'. Add it to your --exclude patterns.",
                path
            ));
        }
        if recs.is_empty() {
            recs.push("Scope discipline is on point. Keep it up.".to_string());
        }
        recs
    }
}

// Helper: get the start of the current ISO week (Monday 00:00 UTC) for the
// given time. Exposed for testing.
#[allow(dead_code)]
pub fn current_week_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let days = now.weekday().num_days_from_monday() as i64;
    (now - Duration::days(days))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
}

// Suppress unused warning for Weekday (kept for explicit semantic anchor)
#[allow(dead_code)]
fn _monday_is_week_start() -> Weekday {
    Weekday::Mon
}
