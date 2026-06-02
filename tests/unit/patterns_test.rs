use scopesafe::db::Database;
use scopesafe::patterns::PatternAnalyzer;
use scopesafe::scope::Scope;
use scopesafe::tracker::FileEvent;

fn fresh_db() -> Database {
    let tmp = std::env::temp_dir().join(format!(
        "scopesafe-test-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let db_path = tmp.join("test.db");
    Database::open_at(&db_path).unwrap()
}

fn mk_event(scope_id: &str, file: &str, in_scope: bool, blocked: bool) -> FileEvent {
    FileEvent {
        id: 0,
        scope_id: scope_id.to_string(),
        file_path: file.to_string(),
        action: "modify".to_string(),
        timestamp: chrono::Utc::now(),
        approved: None,
        approved_by: None,
        rejection_reason: None,
        in_scope,
        is_blocked: blocked,
    }
}

#[test]
fn test_pattern_secrets_touch() {
    let db = fresh_db();
    let scope = Scope::new("fix bug".into(), Some("src/*.go".into()), None, None).unwrap();
    db.save_scope(&scope).unwrap();
    db.save_event(&mk_event(&scope.id, "src/env_loader.go", true, false)).unwrap();
    db.save_event(&mk_event(&scope.id, ".env.example", false, false)).unwrap();

    let patterns = PatternAnalyzer::new().unwrap();
    let v = patterns.analyze_scope_violations_from_scopes(&[scope], &db);
    assert!(!v.is_empty());
    assert!(v.iter().any(|p| p.pattern == "secrets / .env touch" && p.occurrences >= 1));
}

#[test]
fn test_pattern_dependency_change() {
    let db = fresh_db();
    let scope = Scope::new("refactor".into(), Some("src/*.rs".into()), None, None).unwrap();
    db.save_scope(&scope).unwrap();
    db.save_event(&mk_event(&scope.id, "src/lib.rs", true, false)).unwrap();
    db.save_event(&mk_event(&scope.id, "Cargo.toml", false, false)).unwrap();
    db.save_event(&mk_event(&scope.id, "package.json", false, false)).unwrap();

    let patterns = PatternAnalyzer::new().unwrap();
    let v = patterns.analyze_scope_violations_from_scopes(&[scope], &db);
    let dep = v.iter().find(|p| p.pattern == "dependency change").unwrap();
    assert_eq!(dep.occurrences, 2);
}

#[test]
fn test_pattern_config_change() {
    let db = fresh_db();
    let scope = Scope::new("work".into(), Some("src/*.py".into()), None, None).unwrap();
    db.save_scope(&scope).unwrap();
    db.save_event(&mk_event(&scope.id, "src/main.py", true, false)).unwrap();
    db.save_event(&mk_event(&scope.id, "config.yaml", false, false)).unwrap();
    db.save_event(&mk_event(&scope.id, "settings.toml", false, false)).unwrap();
    db.save_event(&mk_event(&scope.id, "app.ini", false, false)).unwrap();

    let patterns = PatternAnalyzer::new().unwrap();
    let v = patterns.analyze_scope_violations_from_scopes(&[scope], &db);
    let cfg = v.iter().find(|p| p.pattern == "config change").unwrap();
    assert_eq!(cfg.occurrences, 3);
}

#[test]
fn test_pattern_blocked_secret() {
    let db = fresh_db();
    let scope = Scope::new("work".into(), Some("src/*.py".into()), None, None).unwrap();
    db.save_scope(&scope).unwrap();
    // The blocked event still gets recorded (for pattern analysis) when we
    // bypass the tracker; the audit logic surfaces it as a hard block.
    db.save_event(&mk_event(&scope.id, "src/main.py", true, false)).unwrap();
    db.save_event(&mk_event(&scope.id, ".env", false, true)).unwrap();

    let patterns = PatternAnalyzer::new().unwrap();
    let v = patterns.analyze_scope_violations_from_scopes(&[scope], &db);
    assert!(v.iter().any(|p| p.pattern == "blocked secret file" && p.occurrences == 1));
}
