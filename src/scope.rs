use crate::error::Error;
use anyhow::Result;
use chrono::{DateTime, Utc};
use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const BLOCKED_PATTERNS: &[&str] = &[
    ".env",
    ".env.*",
    "*.env",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "*.jks",
    "*.keystore",
    "credentials.json",
    "secrets.json",
    "secrets.yaml",
    "service-account*.json",
    "*.token",
    "id_rsa",
    "id_rsa*",
    "id_ed25519",
    "id_ed25519*",
    "id_ecdsa",
    "id_ecdsa*",
    "*.ppk",
    "*.cert",
    "*.crt",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scope {
    pub id: String,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
    pub project: PathBuf,
    pub created_at: DateTime<Utc>,
    pub owner: String,
    pub status: ScopeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScopeStatus {
    Active,
    Completed,
    Cancelled,
}

impl Scope {
    pub fn new(
        task: String,
        files: Option<String>,
        exclude: Option<String>,
        project: Option<PathBuf>,
    ) -> Result<Self> {
        let project = project.unwrap_or_else(|| PathBuf::from("."));
        let owner = whoami::username();

        if let Some(ref f) = files {
            for pat in f.split(',') {
                Pattern::new(pat.trim()).map_err(|e| Error::InvalidPattern(e.to_string()))?;
            }
        }
        if let Some(ref e) = exclude {
            for pat in e.split(',') {
                Pattern::new(pat.trim()).map_err(|e| Error::InvalidPattern(e.to_string()))?;
            }
        }

        let id = format!("scope-{}", uuid_simple());
        let created_at = Utc::now();

        Ok(Self {
            id,
            task,
            files,
            exclude,
            project,
            created_at,
            owner,
            status: ScopeStatus::Active,
        })
    }

    pub fn files_pattern(&self) -> Option<Vec<&str>> {
        self.files
            .as_ref()
            .map(|f| f.split(',').map(|s| s.trim()).collect())
    }

    pub fn exclude_pattern(&self) -> Option<Vec<&str>> {
        self.exclude
            .as_ref()
            .map(|e| e.split(',').map(|s| s.trim()).collect())
    }

    pub fn is_file_in_scope(&self, file_path: &str) -> bool {
        let file_path_clean = file_path
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_string();

        // First check exclusions — if excluded, immediately out of scope
        if let Some(ref exclude_patterns) = self.exclude {
            for pat in exclude_patterns.split(',') {
                let pat = pat.trim();

                // Special handling for dir/* patterns
                if pat.ends_with("/*") {
                    let dir = pat[..pat.len() - 2].to_string();
                    let normalized_dir = dir.replace('\\', "/");
                    if file_path_clean.starts_with(&normalized_dir) {
                        return false;
                    }
                    continue;
                }

                if let Ok(pattern) = Pattern::new(pat) {
                    if pattern.matches(&file_path_clean)
                        || pattern.matches(&format!("/{}", &file_path_clean))
                    {
                        return false;
                    }
                }
            }
        }

        // If no files specified, everything (not excluded) is in scope
        let in_files = if let Some(ref patterns) = self.files {
            patterns.split(',').any(|pat| {
                let pat = pat.trim();
                if let Ok(pattern) = Pattern::new(pat) {
                    pattern.matches(&file_path_clean)
                        || pattern.matches(&format!("/{}", file_path_clean))
                } else {
                    false
                }
            })
        } else {
            true
        };

        in_files
    }

    pub fn is_blocked_file(&self, file_path: &str) -> bool {
        let file_path_clean = file_path
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_string();

        // Also check the basename (last component) for patterns like .env or id_rsa
        let basename = std::path::Path::new(&file_path_clean)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        for pat in BLOCKED_PATTERNS {
            if let Ok(pattern) = glob::Pattern::new(pat) {
                // Check against full path and basename
                if pattern.matches(&file_path_clean)
                    || pattern.matches(&format!("/{}", file_path_clean))
                {
                    return true;
                }
                if !basename.is_empty()
                    && (pattern.matches(basename) || pattern.matches(&format!("/{}", basename)))
                {
                    return true;
                }
                // Special case: id_rsa*, id_ed25519* should match if basename starts with id_rsa
                if pat.ends_with('*') && basename.starts_with(&pat[..pat.len() - 1]) {
                    return true;
                }
            }
        }
        false
    }
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", now)
}

mod whoami {
    pub fn username() -> String {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string())
    }
}
