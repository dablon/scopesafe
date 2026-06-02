use scopesafe::db::Database;
use scopesafe::patterns::PatternAnalyzer;
use scopesafe::report::{compute_score, ReportGenerator, WeeklyReport};
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

fn mk_event(scope_id: &str, file: &str, in_scope: bool, approved: Option<bool>) -> FileEvent {
    FileEvent {
        id: 0,
        scope_id: scope_id.to_string(),
        file_path: file.to_string(),
        action: "modify".to_string(),
        timestamp: chrono::Utc::now(),
        approved,
        approved_by: if approved == Some(true) { Some("tester".to_string()) } else { None },
        rejection_reason: if approved == Some(false) { Some("nope".to_string()) } else { None },
        in_scope,
        is_blocked: false,
    }
}

#[test]
fn test_compute_score_all_in_scope() {
    let s = "scope-1";
    let events = vec![
        mk_event(s, "src/a.rs", true, None),
        mk_event(s, "src/b.rs", true, None),
    ];
    let score = compute_score(&events);
    assert!((score - 100.0).abs() < 0.01);
}

#[test]
fn test_compute_score_rejected_doesnt_count() {
    let s = "scope-1";
    let events = vec![
        mk_event(s, "src/a.rs", true, None),
        mk_event(s, "README.md", false, Some(false)),
    ];
    // 1 in scope (full credit), 1 rejected (no credit) => 1/2 = 50%
    let score = compute_score(&events);
    assert!((score - 50.0).abs() < 0.01);
}

#[test]
fn test_compute_score_approved_counts_as_in_scope() {
    let s = "scope-1";
    let events = vec![
        mk_event(s, "src/a.rs", true, None),
        mk_event(s, "README.md", false, Some(true)),
    ];
    let score = compute_score(&events);
    assert!((score - 100.0).abs() < 0.01);
}

#[test]
fn test_compute_score_pending_partial_credit() {
    let s = "scope-1";
    let events = vec![
        mk_event(s, "src/a.rs", true, None),
        mk_event(s, "README.md", false, None),
    ];
    // 1 + 0.5 / 2 = 75%
    let score = compute_score(&events);
    assert!((score - 75.0).abs() < 0.01);
}

#[test]
fn test_compute_score_empty() {
    assert_eq!(compute_score(&[]), 100.0);
}

#[test]
fn test_weekly_report_empty() {
    let db = fresh_db();
    let patterns = PatternAnalyzer::new().unwrap();
    let gen = ReportGenerator::with_db(db);
    let weekly = gen.generate_weekly(&patterns).unwrap();
    assert_eq!(weekly.total_tasks, 0);
    assert_eq!(weekly.total_files_tracked, 0);
    assert!((weekly.avg_scope_score - 100.0).abs() < 0.01);
}

#[test]
fn test_weekly_report_with_data() {
    let db = fresh_db();
    let scope = Scope::new("task1".into(), Some("src/*.rs".into()), None, None).unwrap();
    db.save_scope(&scope).unwrap();
    db.save_event(&mk_event(&scope.id, "src/main.rs", true, None)).unwrap();
    db.save_event(&mk_event(&scope.id, "src/lib.rs", true, None)).unwrap();
    db.save_event(&mk_event(&scope.id, "README.md", false, None)).unwrap();

    let patterns = PatternAnalyzer::new().unwrap();
    let gen = ReportGenerator::with_db(db);
    let weekly = gen.generate_weekly(&patterns).unwrap();
    assert_eq!(weekly.total_tasks, 1);
    assert_eq!(weekly.total_files_tracked, 3);
    assert_eq!(weekly.total_out_of_scope, 1);
    // 2 in scope + 0.5 (pending) / 3 = 83.33%
    assert!((weekly.avg_scope_score - 83.333).abs() < 1.0);
}

#[test]
fn test_weekly_report_print_doesnt_panic() {
    let db = fresh_db();
    let patterns = PatternAnalyzer::new().unwrap();
    let gen = ReportGenerator::with_db(db);
    let weekly: WeeklyReport = gen.generate_weekly(&patterns).unwrap();
    weekly.print();
}
