//! End-to-end CLI integration test.
//!
//! Exercises the full lifecycle of a scope: init -> track in-scope files -> track
//! out-of-scope files -> track blocked secret -> audit -> approve/reject -> revert.
//!
//! Uses the compiled binary via `assert_cmd`.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

#[allow(dead_code)]
fn scopesafe_cmd(tmp: &Path) -> Command {
    let bin = env!("CARGO_BIN_EXE_scopesafe");
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", tmp);
    cmd.env("USER", "test-user");
    cmd
}

fn setup_repo(tmp: &Path) {
    // Create a sample project tree and an initial git commit
    let payments = tmp.join("payments");
    let subs = payments.join("subscriptions");
    let vendor = payments.join("vendor");
    std::fs::create_dir_all(&subs).unwrap();
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(payments.join("retry.go"), "package payments\nfunc Retry() {}\n").unwrap();
    std::fs::write(
        payments.join("timeout.go"),
        "package payments\nfunc Timeout() {}\n",
    )
    .unwrap();
    std::fs::write(subs.join("main.go"), "package subscriptions\n").unwrap();
    std::fs::write(vendor.join("dep.go"), "package vendor\n").unwrap();
    std::fs::write(tmp.join(".env"), "SECRET=hidden\n").unwrap();

    // git init + commit so revert has something to restore
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp)
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .args(["config", "user.email", "t@t.com"])
        .current_dir(tmp)
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(tmp)
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(tmp)
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .args(["commit", "-qm", "init"])
        .current_dir(tmp)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn full_lifecycle_in_scope_only() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    setup_repo(tmp.path());

    let bin = env!("CARGO_BIN_EXE_scopesafe");

    // 1. init scope
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args([
            "init",
            "--task=fix retry timeout",
            "--files=payments/*.go",
            "--exclude=payments/subscriptions/*,payments/vendor/*",
            "--project",
            tmp.path().to_str().unwrap(),
        ]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Scope created: fix retry timeout"));

    // 2. track in-scope file
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["track", "--file=payments/retry.go", "--action=modify"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("✓ tracked: payments/retry.go (in scope)"));

    // 3. track second in-scope file
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["track", "--file=payments/timeout.go", "--action=modify"]);
    cmd.assert().success();

    // 4. track out-of-scope file
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["track", "--file=payments/subscriptions/main.go", "--action=modify"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("OUT OF SCOPE"));

    // 5. blocked file — should fail
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["track", "--file=.env", "--action=modify"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("blocked file cannot be modified"));

    // 6. status
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data).arg("status");
    let output = cmd.output().expect("run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files tracked: 3"));
    assert!(stdout.contains("In scope: 2"));
    assert!(stdout.contains("Out of scope: 1"));
    assert!(stdout.contains("Scope score: 83%")); // 2 + 0.5 / 3 ≈ 83

    // 7. audit
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data).arg("audit");
    let output = cmd.output().expect("run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("SCOPE AUDIT"));
    assert!(stdout.contains("payments/retry.go"));
    assert!(stdout.contains("payments/subscriptions/main.go"));
    assert!(stdout.contains("PENDING APPROVAL"));

    // 8. approve the out-of-scope change
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["approve", "--file=payments/subscriptions/main.go"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("approved: payments/subscriptions/main.go"));

    // 9. audit shows it as APPROVED
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data).arg("audit");
    let output = cmd.output().expect("run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("APPROVED"));
    assert!(stdout.contains("Scope score: 100%"));
}

#[test]
fn reject_and_revert_workflow() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    setup_repo(tmp.path());

    let bin = env!("CARGO_BIN_EXE_scopesafe");

    // init scope on src/main.rs only
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["init", "--task=tight scope", "--files=src/*.rs", "--project", tmp.path().to_str().unwrap()]);
    cmd.assert().success();

    // Modify the out-of-scope file on disk
    let target = tmp.path().join("payments/retry.go");
    let original = std::fs::read_to_string(&target).unwrap();
    std::fs::write(&target, "package payments\nfunc Retry() { /* bad change */ }\n").unwrap();

    // Track it as out-of-scope
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["track", "--file=payments/retry.go", "--action=modify"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("OUT OF SCOPE"));

    // Reject it
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args([
            "reject",
            "--file=payments/retry.go",
            "--reason=intentional regression",
        ]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("rejected: payments/retry.go"));

    // Revert: should restore the file via `git checkout HEAD -- <path>`
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .arg("revert-all");
    // Rejected files are skipped, not reverted, so 0 is expected.
    let output = cmd.output().expect("run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("reverted 0 out-of-scope files"));

    // File should still contain the bad change (rejected = not reverted)
    let after = std::fs::read_to_string(&target).unwrap();
    assert!(after.contains("bad change"));
    assert_eq!(after, "package payments\nfunc Retry() { /* bad change */ }\n");

    // Now test the actual revert path: reset, track, don't reject, then revert-all
    std::fs::write(&target, &original).unwrap();
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["track", "--file=payments/retry.go", "--action=modify"]);
    cmd.assert().success();

    // Make another bad change
    std::fs::write(&target, "package payments\nfunc Retry() { /* another bad change */ }\n").unwrap();

    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .arg("revert-all");
    let output = cmd.output().expect("run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("reverted 1 out-of-scope files"));

    // File restored to the version we re-wrote above (which is the same as HEAD
    // because we wrote the original content back). Since we wrote original back
    // and then committed... wait, we didn't commit. The "current" file is now
    // the "bad change". `git checkout HEAD -- payments/retry.go` should restore
    // to the original "package payments\nfunc Retry() {}\n".
    let after = std::fs::read_to_string(&target).unwrap();
    assert_eq!(after, original, "file should be reverted to HEAD");
}

#[test]
fn weekly_report_shows_real_data() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    setup_repo(tmp.path());

    let bin = env!("CARGO_BIN_EXE_scopesafe");

    // Create a scope with mixed in/out/blocked files
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["init", "--task=test", "--files=src/*.go", "--project", tmp.path().to_str().unwrap()]);
    cmd.assert().success();

    // Track some files
    for f in ["src/a.go", "src/b.go", "Cargo.toml", "package.json"] {
        let mut cmd = Command::new(bin);
        cmd.env("XDG_DATA_HOME", &data)
            .args(["track", "--file", f, "--action=modify"]);
        let _ = cmd.output();
    }

    // Run weekly report
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["report-weekly"]);
    let output = cmd.output().expect("run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WEEKLY SCOPE DRIFT REPORT"));
    assert!(stdout.contains("Total tasks: 1"));
    assert!(stdout.contains("Out-of-scope touches: 2"));
    assert!(stdout.contains("dependency change"));
}

#[test]
fn invalid_glob_fails_cleanly() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let bin = env!("CARGO_BIN_EXE_scopesafe");

    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["init", "--task=bad", "--files=[invalid", "--project", tmp.path().to_str().unwrap()]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("invalid pattern"));
}

#[test]
fn no_active_scope_errors() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let bin = env!("CARGO_BIN_EXE_scopesafe");

    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data).arg("status");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("no active scope"));
}

#[test]
fn file_not_tracked_error() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    setup_repo(tmp.path());
    let bin = env!("CARGO_BIN_EXE_scopesafe");

    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["init", "--task=work", "--files=src/*.go", "--project", tmp.path().to_str().unwrap()]);
    cmd.assert().success();

    // Try to approve a file we never tracked
    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &data)
        .args(["approve", "--file=never-tracked.go"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("file not tracked"));
}
