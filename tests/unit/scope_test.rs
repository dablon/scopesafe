use scopesafe::scope::Scope;
use std::path::PathBuf;

#[test]
fn test_scope_creation_basic() {
    let scope = Scope::new("fix bug in payments".to_string(), None, None, None).unwrap();

    assert!(scope.id.starts_with("scope-"));
    assert_eq!(scope.task, "fix bug in payments");
    assert_eq!(scope.files, None);
    assert_eq!(scope.exclude, None);
}

#[test]
fn test_scope_creation_with_patterns() {
    let scope = Scope::new(
        "refactor".to_string(),
        Some("src/*.rs".to_string()),
        Some("target/*,*.log".to_string()),
        Some(PathBuf::from("/tmp")),
    )
    .unwrap();

    assert_eq!(scope.task, "refactor");
    assert_eq!(scope.files, Some("src/*.rs".to_string()));
    assert_eq!(scope.exclude, Some("target/*,*.log".to_string()));
    assert_eq!(scope.project, PathBuf::from("/tmp"));
}

#[test]
fn test_scope_creation_with_invalid_pattern() {
    let result = Scope::new("test".to_string(), Some("[invalid".to_string()), None, None);

    assert!(result.is_err());
}

#[test]
fn test_scope_files_pattern_parsing() {
    let scope = Scope::new(
        "test".to_string(),
        Some("src/*.rs, lib/*.go".to_string()),
        None,
        None,
    )
    .unwrap();

    let patterns = scope.files_pattern().unwrap();
    assert_eq!(patterns, vec!["src/*.rs", "lib/*.go"]);
}

#[test]
fn test_scope_exclude_pattern_parsing() {
    let scope = Scope::new(
        "test".to_string(),
        None,
        Some("target/*,*.log,node_modules/*".to_string()),
        None,
    )
    .unwrap();

    let patterns = scope.exclude_pattern().unwrap();
    assert_eq!(patterns, vec!["target/*", "*.log", "node_modules/*"]);
}

#[test]
fn test_is_file_in_scope_with_no_patterns() {
    let scope = Scope::new("test".to_string(), None, None, None).unwrap();

    // No patterns means everything is in scope
    assert!(scope.is_file_in_scope("anything.rs"));
    assert!(scope.is_file_in_scope("deeply/nested/path/file.go"));
}

#[test]
fn test_is_file_in_scope_with_files_pattern_match() {
    let scope = Scope::new(
        "test".to_string(),
        Some("payments/*.go".to_string()),
        None,
        None,
    )
    .unwrap();

    assert!(scope.is_file_in_scope("payments/retry.go"));
    assert!(scope.is_file_in_scope("payments/timeout.go"));
    // Note: glob *.go matches across directories, so payments/subscriptions/main.go
    // matches payments/*.go. This is standard glob behavior.
    // Use exclude patterns to filter further.
    assert!(scope.is_file_in_scope("payments/subscriptions/main.go"));
    assert!(!scope.is_file_in_scope("src/main.rs"));
}

#[test]
fn test_is_file_in_scope_with_exclude_pattern() {
    let scope = Scope::new(
        "test".to_string(),
        Some("payments/*.go".to_string()),
        Some("payments/subscriptions/*".to_string()),
        None,
    )
    .unwrap();

    // In payments/* but excluded
    assert!(!scope.is_file_in_scope("payments/subscriptions/main.go"));
    // In payments/* and not excluded
    assert!(scope.is_file_in_scope("payments/retry.go"));
}

#[test]
fn test_is_file_in_scope_with_windows_path() {
    let scope = Scope::new(
        "test".to_string(),
        Some("payments/*.go".to_string()),
        None,
        None,
    )
    .unwrap();

    // Windows path separator
    assert!(scope.is_file_in_scope("payments\\retry.go"));
}

#[test]
fn test_is_file_in_scope_strips_leading_dot_slash() {
    let scope = Scope::new(
        "test".to_string(),
        Some("payments/*.go".to_string()),
        None,
        None,
    )
    .unwrap();

    assert!(scope.is_file_in_scope("./payments/retry.go"));
}

#[test]
fn test_is_blocked_file_env() {
    let scope = Scope::new("test".to_string(), None, None, None).unwrap();

    assert!(scope.is_blocked_file(".env"));
    assert!(scope.is_blocked_file(".env.local"));
    assert!(scope.is_blocked_file("config/.env"));
    assert!(scope.is_blocked_file("backend/.env.production"));
}

#[test]
fn test_is_blocked_file_pem() {
    let scope = Scope::new("test".to_string(), None, None, None).unwrap();

    assert!(scope.is_blocked_file("server.pem"));
    assert!(scope.is_blocked_file("certs/private.pem"));
    assert!(scope.is_blocked_file("keys/api.pem"));
}

#[test]
fn test_is_blocked_file_key() {
    let scope = Scope::new("test".to_string(), None, None, None).unwrap();

    assert!(scope.is_blocked_file("private.key"));
    assert!(scope.is_blocked_file("ssh/id_rsa"));
    assert!(scope.is_blocked_file("ssh/id_ed25519"));
    assert!(scope.is_blocked_file("ssh/id_ecdsa"));
}

#[test]
fn test_is_blocked_file_credentials() {
    let scope = Scope::new("test".to_string(), None, None, None).unwrap();

    assert!(scope.is_blocked_file("credentials.json"));
    assert!(scope.is_blocked_file("secrets.json"));
    assert!(scope.is_blocked_file("service-account.json"));
    assert!(scope.is_blocked_file("service-account-abc123.json"));
    assert!(scope.is_blocked_file("auth.token"));
}

#[test]
fn test_is_blocked_file_normal_files() {
    let scope = Scope::new("test".to_string(), None, None, None).unwrap();

    // Normal files should NOT be blocked
    assert!(!scope.is_blocked_file("main.rs"));
    assert!(!scope.is_blocked_file("README.md"));
    assert!(!scope.is_blocked_file("src/lib.rs"));
    assert!(!scope.is_blocked_file("config.yaml"));
    assert!(!scope.is_blocked_file("package.json"));
}

#[test]
fn test_is_blocked_file_strips_leading_dot_slash() {
    let scope = Scope::new("test".to_string(), None, None, None).unwrap();

    assert!(scope.is_blocked_file("./.env"));
    assert!(scope.is_blocked_file("./certs/server.pem"));
}
