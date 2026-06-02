//! End-to-end MCP integration test that drives the real binary over stdio.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn write_request(stdin: &mut std::process::ChildStdin, id: u64, method: &str, params: serde_json::Value) {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    writeln!(stdin, "{}", req).expect("write to mcp stdin");
}

#[test]
fn mcp_lifecycle_over_stdio() {
    let tmp = TempDir::new().unwrap();
    let bin = env!("CARGO_BIN_EXE_scopesafe");

    let mut child = Command::new(bin)
        .args(["mcp", "--project", tmp.path().to_str().unwrap()])
        .env("XDG_DATA_HOME", tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn scopesafe mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    write_request(&mut stdin, 1, "initialize", serde_json::json!({}));
    write_request(
        &mut stdin,
        2,
        "tools/call",
        serde_json::json!({
            "name": "init_scope",
            "arguments": {"task": "mcp e2e", "files": "src/*.go"}
        }),
    );
    write_request(
        &mut stdin,
        3,
        "tools/call",
        serde_json::json!({
            "name": "track_file",
            "arguments": {"file": "src/main.go", "action": "modify"}
        }),
    );
    write_request(
        &mut stdin,
        4,
        "tools/call",
        serde_json::json!({
            "name": "track_file",
            "arguments": {"file": "README.md", "action": "modify"}
        }),
    );
    write_request(
        &mut stdin,
        5,
        "tools/call",
        serde_json::json!({
            "name": "check_file",
            "arguments": {"file": ".env"}
        }),
    );
    write_request(
        &mut stdin,
        6,
        "tools/call",
        serde_json::json!({"name": "status", "arguments": {}}),
    );
    write_request(
        &mut stdin,
        7,
        "tools/call",
        serde_json::json!({"name": "audit", "arguments": {}}),
    );
    write_request(
        &mut stdin,
        8,
        "tools/call",
        serde_json::json!({"name": "list_scopes", "arguments": {}}),
    );
    drop(stdin);

    let reader = BufReader::new(stdout);
    let mut responses: Vec<serde_json::Value> = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        responses.push(serde_json::from_str(&line).expect("valid json"));
    }
    let _ = child.wait();

    assert_eq!(responses.len(), 8, "expected 8 responses, got: {:?}", responses);

    // 1: initialize
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2024-11-05");
    assert!(responses[0]["result"]["serverInfo"]["name"].as_str().unwrap().contains("scopesafe"));

    // 2: init_scope
    let t2 = responses[1]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t2.contains("Scope created: mcp e2e"));

    // 3: track_file in scope
    let t3 = responses[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t3.contains("in scope"));

    // 4: track_file out of scope
    let t4 = responses[3]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t4.contains("OUT OF SCOPE"));

    // 5: check_file blocked
    let t5 = responses[4]["result"]["content"][0]["text"].as_str().unwrap();
    let v5: serde_json::Value = serde_json::from_str(t5).unwrap();
    assert_eq!(v5["verdict"], "BLOCKED");

    // 6: status
    let t6 = responses[5]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t6.contains("Tracked: 2"));
    assert!(t6.contains("In scope: 1"));
    assert!(t6.contains("Out of scope: 1"));

    // 7: audit
    let t7 = responses[6]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t7.contains("IN SCOPE"));
    assert!(t7.contains("OUT OF SCOPE"));
    assert!(t7.contains("Scope score: 75%")); // 1 + 0.5 / 2

    // 8: list_scopes
    let t8 = responses[7]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t8.contains("mcp e2e"));
    assert!(t8.contains("[Active]"));
}

#[test]
fn mcp_error_for_blocked_file() {
    let tmp = TempDir::new().unwrap();
    let bin = env!("CARGO_BIN_EXE_scopesafe");

    let mut child = Command::new(bin)
        .args(["mcp", "--project", tmp.path().to_str().unwrap()])
        .env("XDG_DATA_HOME", tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn scopesafe mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    write_request(&mut stdin, 1, "initialize", serde_json::json!({}));
    write_request(
        &mut stdin,
        2,
        "tools/call",
        serde_json::json!({"name": "init_scope", "arguments": {"task": "block test"}}),
    );
    write_request(
        &mut stdin,
        3,
        "tools/call",
        serde_json::json!({
            "name": "track_file",
            "arguments": {"file": ".env", "action": "modify"}
        }),
    );
    drop(stdin);

    let reader = BufReader::new(stdout);
    let mut responses: Vec<serde_json::Value> = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        responses.push(serde_json::from_str(&line).expect("valid json"));
    }
    let _ = child.wait();

    assert_eq!(responses.len(), 3);

    // Response 3 should be an error (not a result)
    assert!(responses[2]["error"].is_object(), "expected error, got: {:?}", responses[2]);
    let err = &responses[2]["error"];
    assert_eq!(err["code"], -32003); // permission denied
    assert!(err["message"].as_str().unwrap().contains("blocked"));
}
