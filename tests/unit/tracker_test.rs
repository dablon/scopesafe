use scopesafe::db::Database;
use scopesafe::scope::Scope;
use scopesafe::tracker::{FileEvent, Tracker};

fn fresh_db() -> Database {
    // Each test gets its own DB in a unique temp file
    let tmp = std::env::temp_dir().join(format!(
        "scopesafe-test-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let db_path = tmp.join("test.db");
    Database::open_at(&db_path).unwrap()
}

#[test]
fn test_save_and_get_active_scope() {
    let db = fresh_db();
    let scope = Scope::new("test".into(), Some("src/*.rs".into()), None, None).unwrap();
    db.save_scope(&scope).unwrap();

    let active = db.get_active_scope().unwrap();
    assert_eq!(active.id, scope.id);
    assert_eq!(active.task, "test");
    assert_eq!(active.files.as_deref(), Some("src/*.rs"));
}

#[test]
fn test_track_file_in_scope() {
    let db = fresh_db();
    let scope = Scope::new("test".into(), Some("payments/*.go".into()), None, None).unwrap();
    db.save_scope(&scope).unwrap();

    let event = FileEvent {
        id: 0,
        scope_id: scope.id.clone(),
        file_path: "payments/retry.go".into(),
        action: "modify".into(),
        timestamp: chrono::Utc::now(),
        approved: None,
        approved_by: None,
        rejection_reason: None,
        in_scope: true,
        is_blocked: false,
    };
    let saved = db.save_event(&event).unwrap();
    assert!(saved.id > 0);

    let events = db.get_events(&scope.id).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].file_path, "payments/retry.go");
    assert!(events[0].in_scope);
    assert!(!events[0].is_blocked);
}

#[test]
fn test_approve_reject_files() {
    let db = fresh_db();
    let scope = Scope::new("test".into(), Some("src/*.rs".into()), None, None).unwrap();
    db.save_scope(&scope).unwrap();

    let event = FileEvent {
        id: 0,
        scope_id: scope.id.clone(),
        file_path: "README.md".into(),
        action: "modify".into(),
        timestamp: chrono::Utc::now(),
        approved: None,
        approved_by: None,
        rejection_reason: None,
        in_scope: false,
        is_blocked: false,
    };
    db.save_event(&event).unwrap();

    db.approve_file("README.md").unwrap();
    let events = db.get_events(&scope.id).unwrap();
    assert_eq!(events[0].approved, Some(true));

    // Add another and reject
    let event2 = FileEvent {
        id: 0,
        scope_id: scope.id.clone(),
        file_path: "package.json".into(),
        action: "modify".into(),
        timestamp: chrono::Utc::now(),
        approved: None,
        approved_by: None,
        rejection_reason: None,
        in_scope: false,
        is_blocked: false,
    };
    db.save_event(&event2).unwrap();

    db.reject_file("package.json", "out of scope").unwrap();
    let events = db.get_events(&scope.id).unwrap();
    let pkg = events.iter().find(|e| e.file_path == "package.json").unwrap();
    assert_eq!(pkg.approved, Some(false));
    assert_eq!(pkg.rejection_reason.as_deref(), Some("out of scope"));
}

#[test]
fn test_list_all_scopes() {
    let db = fresh_db();
    let s1 = Scope::new("a".into(), None, None, None).unwrap();
    let s2 = Scope::new("b".into(), None, None, None).unwrap();
    db.save_scope(&s1).unwrap();
    db.save_scope(&s2).unwrap();

    let scopes = db.list_all_scopes().unwrap();
    assert_eq!(scopes.len(), 2);
}

#[test]
fn test_tracker_blocked_file_bails() {
    let db = fresh_db();
    let scope = Scope::new("test".into(), Some("*.go".into()), None, None).unwrap();
    db.save_scope(&scope).unwrap();

    let tracker = Tracker { db };
    let result = tracker.track_file(&scope.id, ".env", "modify", false, true);
    assert!(result.is_err());
}
