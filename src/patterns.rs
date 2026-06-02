use crate::db::Database;
use crate::scope::Scope;
use crate::tracker::FileEvent;
use anyhow::Result;
use std::collections::HashMap;

pub struct PatternAnalyzer {}

#[derive(Debug, Clone, PartialEq)]
pub struct ViolationPattern {
    pub pattern: String,
    pub occurrences: usize,
    pub description: String,
}

impl PatternAnalyzer {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Analyze violations across many scopes and surface recurring patterns.
    /// Detection rules:
    /// - "secret touch": any blocked file attempt (`.env`, `*.pem`, etc.)
    /// - "refactor adjacent code": out-of-scope touches in directories that share
    ///   a parent with the scope's primary directory
    /// - "add logging": paths matching `*log*` or `*logger*` out-of-scope
    /// - "fix typo in comment": out-of-scope file with low churn (heuristic: <5 events)
    /// - "dependency change": touches to `package.json`, `Cargo.toml`, `go.mod`,
    ///   `requirements.txt`, `pyproject.toml`
    /// - "config change": touches to `*.yaml`, `*.yml`, `*.toml`, `*.ini`
    pub fn analyze_scope_violations_from_scopes(
        &self,
        scopes: &[Scope],
        db: &Database,
    ) -> Vec<ViolationPattern> {
        // Collect owned FileEvents by scope to avoid lifetime issues.
        let mut all_events: Vec<FileEvent> = Vec::new();
        for scope in scopes {
            if let Ok(events) = db.get_events(&scope.id) {
                for e in events.into_iter().filter(|e| !e.in_scope && !e.is_blocked) {
                    all_events.push(e);
                }
            }
        }

        let mut counts: HashMap<&'static str, usize> = HashMap::new();
        let mut blocked_counts: HashMap<&'static str, usize> = HashMap::new();
        let mut scope_dirs: Vec<String> = Vec::new();

        for scope in scopes {
            if let Some(files) = &scope.files {
                for pat in files.split(',') {
                    let p = pat.trim().trim_end_matches("/*").trim_end_matches("/*.go");
                    if !p.is_empty() {
                        scope_dirs.push(p.to_string());
                    }
                }
            }
        }

        for e in &all_events {
            let path = e.file_path.to_lowercase();
            if path.contains("env") || path.ends_with(".env") {
                *counts.entry("secrets / .env touch").or_insert(0) += 1;
            }
            if path.ends_with("package.json")
                || path.ends_with("cargo.toml")
                || path.ends_with("go.mod")
                || path.ends_with("requirements.txt")
                || path.ends_with("pyproject.toml")
            {
                *counts.entry("dependency change").or_insert(0) += 1;
            }
            if path.ends_with(".yaml")
                || path.ends_with(".yml")
                || path.ends_with(".toml")
                || path.ends_with(".ini")
                || path.ends_with(".conf")
            {
                *counts.entry("config change").or_insert(0) += 1;
            }
            if path.contains("/log") || path.contains("logger") {
                *counts.entry("add logging").or_insert(0) += 1;
            }
            if path.ends_with("readme.md") || path.ends_with("docs/") {
                *counts.entry("docs / readme edit").or_insert(0) += 1;
            }
            // Adjacent refactor: out-of-scope path shares first segment with a scope dir
            for d in &scope_dirs {
                if !d.is_empty() && !path.starts_with(&d.to_lowercase()) {
                    let first = path.split('/').next().unwrap_or("");
                    if !first.is_empty() && d.split('/').next().unwrap_or("") == first {
                        *counts.entry("refactor adjacent code").or_insert(0) += 1;
                        break;
                    }
                }
            }
        }

        // Also surface blocked attempts as a pattern
        for scope in scopes {
            if let Ok(events) = db.get_events(&scope.id) {
                for e in events.iter().filter(|e| e.is_blocked) {
                    let key: &'static str = "blocked secret file";
                    *blocked_counts.entry(key).or_insert(0) += 1;
                    let _ = e;
                }
            }
        }

        let mut patterns: Vec<ViolationPattern> = counts
            .into_iter()
            .map(|(p, c)| ViolationPattern {
                pattern: p.to_string(),
                occurrences: c,
                description: description_for(p),
            })
            .collect();

        for (p, c) in blocked_counts {
            patterns.push(ViolationPattern {
                pattern: p.to_string(),
                occurrences: c,
                description: "Agent attempted to modify a secret/key/credential file."
                    .to_string(),
            });
        }

        patterns.sort_by_key(|a| std::cmp::Reverse(a.occurrences));
        patterns
    }

    /// Analyze a single scope's events for a tighter report.
    #[allow(dead_code)]
    pub fn analyze_scope_violations(&self, events: &[FileEvent]) -> Vec<ViolationPattern> {
        let mut counts: HashMap<&'static str, usize> = HashMap::new();
        for e in events.iter().filter(|e| !e.in_scope && !e.is_blocked) {
            let path = e.file_path.to_lowercase();
            if path.contains("env") {
                *counts.entry("secrets touch").or_insert(0) += 1;
            }
            if path.ends_with("package.json")
                || path.ends_with("cargo.toml")
                || path.ends_with("go.mod")
            {
                *counts.entry("dependency change").or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .map(|(p, c)| ViolationPattern {
                pattern: p.to_string(),
                occurrences: c,
                description: description_for(p),
            })
            .collect()
    }
}

fn description_for(p: &str) -> String {
    match p {
        "secrets / .env touch" => "Agent reached for environment or secrets files.".to_string(),
        "dependency change" => "Agent touched dependency manifests.".to_string(),
        "config change" => "Agent modified configuration files.".to_string(),
        "add logging" => "Agent added logging or telemetry code.".to_string(),
        "docs / readme edit" => "Agent edited documentation without being asked.".to_string(),
        "refactor adjacent code" => {
            "Agent refactored code adjacent to the scope's target directory.".to_string()
        }
        "blocked secret file" => "Agent attempted to modify a secret/key/credential file."
            .to_string(),
        _ => "Repeated out-of-scope behaviour.".to_string(),
    }
}
