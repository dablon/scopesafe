use scopesafe::mcp::McpServer;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn fresh_xdg() -> std::path::PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "scopesafe-mcp-test-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("XDG_DATA_HOME", &tmp);
    tmp
}

#[test]
fn test_mcp_initialize() {
    let tmp = fresh_xdg();
    let server = McpServer::new(tmp).unwrap();
    let resp = server
        .dispatch("initialize", json!({}))
        .expect("initialize should succeed");
    assert_eq!(resp["protocolVersion"], "2024-11-05");
    assert_eq!(resp["serverInfo"]["name"], "scopesafe");
    assert!(resp["serverInfo"]["version"].is_string());
}

#[test]
fn test_mcp_tools_list_has_required_tools() {
    let tmp = fresh_xdg();
    let server = McpServer::new(tmp).unwrap();
    let resp = server.dispatch("tools/list", json!({})).unwrap();
    let tools = resp["tools"].as_array().expect("tools is an array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for required in ["init_scope", "track_file", "check_file", "audit", "status", "weekly_report"] {
        assert!(names.contains(&required), "missing tool: {}", required);
    }
}

#[test]
fn test_mcp_init_scope_and_check_file() {
    let tmp = fresh_xdg();
    let server = McpServer::new(tmp.clone()).unwrap();

    let resp = server
        .dispatch(
            "tools/call",
            json!({
                "name": "init_scope",
                "arguments": {
                    "task": "fix retry timeout",
                    "files": "payments/*.go",
                    "exclude": "payments/vendor/*"
                }
            }),
        )
        .unwrap();
    let text = resp["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Scope created"));
    assert!(text.contains("fix retry timeout"));

    // check_file in scope
    let resp = server
        .dispatch(
            "tools/call",
            json!({
                "name": "check_file",
                "arguments": {"file": "payments/retry.go"}
            }),
        )
        .unwrap();
    let v: Value = serde_json::from_str(resp["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["verdict"], "ALLOWED");
    assert!(v["in_scope"].as_bool().unwrap());

    // check_file out of scope (excluded)
    let resp = server
        .dispatch(
            "tools/call",
            json!({
                "name": "check_file",
                "arguments": {"file": "payments/vendor/dep.go"}
            }),
        )
        .unwrap();
    let v: Value = serde_json::from_str(resp["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["verdict"], "OUT_OF_SCOPE");

    // check_file blocked
    let resp = server
        .dispatch(
            "tools/call",
            json!({
                "name": "check_file",
                "arguments": {"file": ".env"}
            }),
        )
        .unwrap();
    let v: Value = serde_json::from_str(resp["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["verdict"], "BLOCKED");
}

#[test]
fn test_mcp_track_file_blocks_secrets() {
    let tmp = fresh_xdg();
    let server = McpServer::new(tmp.clone()).unwrap();
    server
        .dispatch(
            "tools/call",
            json!({
                "name": "init_scope",
                "arguments": {"task": "test"}
            }),
        )
        .unwrap();

    let result = server.dispatch(
        "tools/call",
        json!({
            "name": "track_file",
            "arguments": {"file": ".env", "action": "modify"}
        }),
    );
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("blocked") || err_msg.contains("permission"));
}

#[test]
fn test_mcp_track_in_then_audit() {
    let tmp = fresh_xdg();
    let server = McpServer::new(tmp.clone()).unwrap();
    server
        .dispatch(
            "tools/call",
            json!({
                "name": "init_scope",
                "arguments": {"task": "work", "files": "src/*.rs"}
            }),
        )
        .unwrap();

    server
        .dispatch(
            "tools/call",
            json!({
                "name": "track_file",
                "arguments": {"file": "src/main.rs", "action": "modify"}
            }),
        )
        .unwrap();
    server
        .dispatch(
            "tools/call",
            json!({
                "name": "track_file",
                "arguments": {"file": "README.md", "action": "modify"}
            }),
        )
        .unwrap();

    let resp = server
        .dispatch("tools/call", json!({"name": "audit", "arguments": {}}))
        .unwrap();
    let text = resp["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("src/main.rs"));
    assert!(text.contains("README.md"));
    assert!(text.contains("Scope score"));
    // 1 in scope + 0.5 pending / 2 = 75%
    assert!(text.contains("75%"));
}

#[test]
fn test_mcp_resources_read_active_scope() {
    let tmp = fresh_xdg();
    let server = McpServer::new(tmp.clone()).unwrap();
    server
        .dispatch(
            "tools/call",
            json!({
                "name": "init_scope",
                "arguments": {"task": "the work"}
            }),
        )
        .unwrap();
    let resp = server
        .dispatch(
            "resources/read",
            json!({"uri": "scope://active"}),
        )
        .unwrap();
    let text = resp["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("the work"));
}

#[test]
fn test_mcp_stdio_e2e_via_binary() {
    // Spawn the actual binary and talk to it over stdio.
    let bin = env!("CARGO_BIN_EXE_scopesafe");
    let tmp = fresh_xdg();
    let mut child = Command::new(bin)
        .args(["mcp", "--project", tmp.to_str().unwrap()])
        .env("XDG_DATA_HOME", &tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn scopesafe mcp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");

    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}).to_string(),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}).to_string(),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"init_scope","arguments":{"task":"stdio test","files":"src/*.go"}}}).to_string(),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"track_file","arguments":{"file":"src/main.go","action":"modify"}}}).to_string(),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"status","arguments":{}}}).to_string(),
    ];

    for r in &requests {
        writeln!(stdin, "{}", r).expect("write");
    }
    drop(stdin); // close stdin so the child exits

    // Read all of stdout
    let reader = BufReader::new(stdout);
    let mut responses: Vec<Value> = Vec::new();
    for line in reader.lines() {
        let line = line.expect("line");
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line).expect("valid json");
        responses.push(v);
    }

    let _ = child.wait();

    assert_eq!(responses.len(), 5, "expected 5 responses, got: {:?}", responses);

    // Check id 1 (initialize) -> protocolVersion present
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2024-11-05");

    // Check id 2 (tools/list) -> 7 tools
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 7);

    // Check id 3 (init_scope) -> contains "Scope created"
    let t3 = responses[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t3.contains("Scope created"));

    // Check id 4 (track_file) -> "in scope"
    let t4 = responses[3]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t4.contains("in scope"));

    // Check id 5 (status) -> 1 file tracked
    let t5 = responses[4]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t5.contains("Tracked: 1"));
}
