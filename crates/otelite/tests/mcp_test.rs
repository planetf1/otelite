//! End-to-end test for `otelite mcp` (#37): the acceptance criterion is
//! that `echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | otelite mcp`
//! returns the tool list. We pipe a full handshake plus a couple of tool
//! calls through the real binary (fresh temp database per run) and check
//! the newline-delimited JSON-RPC responses on stdout.

use std::io::Write;
use std::process::{Command, Stdio};

fn run_mcp_script(data_dir: &std::path::Path, script: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_otelite"))
        .arg("mcp")
        .env("OTELITE_DATA_DIR", data_dir)
        // Keep stdout pure: any tracing goes to a throwaway file.
        .env("RUST_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn otelite mcp");

    let mut stdin = child.stdin.take().expect("stdin pipe");
    stdin.write_all(script.as_bytes()).expect("write script");
    // Dropping the write-end signals EOF — the server exits after
    // draining its lines, so the drop must happen before we wait.
    drop(stdin);

    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "otelite mcp exited {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn mcp_handshake_tools_list_and_empty_query() {
    let dir = tempfile::tempdir().expect("temp data dir");
    // initialize, a notification (no response expected), tools/list, and
    // a query against the empty database — four input lines, three
    // response lines (the notification is fire-and-forget).
    let script = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"query_logs","arguments":{"limit":5}}}
"#;
    let out = run_mcp_script(dir.path(), script);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "expected 3 response lines (notification gets none): {out}"
    );

    let parse = |line: &str| -> serde_json::Value {
        serde_json::from_str(line).expect("response is valid JSON")
    };

    let init = parse(lines[0]);
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "otelite");

    let list = parse(lines[1]);
    assert_eq!(list["id"], 2);
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(
        names,
        vec!["query_logs", "query_traces", "get_trace", "get_usage"]
    );
    // Every tool carries a valid JSON Schema object.
    for tool in list["result"]["tools"].as_array().unwrap() {
        assert!(tool["inputSchema"]["type"] == "object");
        assert!(!tool["description"].as_str().unwrap().is_empty());
    }

    let call = parse(lines[2]);
    assert_eq!(call["id"], 3);
    let result = &call["result"];
    assert!(result["isError"] == false || result["isError"].is_null());
    let text: String = result["content"][0]["text"]
        .as_str()
        .expect("text content")
        .to_string();
    let body: serde_json::Value = serde_json::from_str(&text).expect("tool text is JSON");
    assert_eq!(body["count"], 0);
    assert!(body["logs"].as_array().expect("logs array").is_empty());
}

#[test]
fn mcp_unknown_method_and_malformed_line_are_jsonrpc_errors() {
    let dir = tempfile::tempdir().expect("temp data dir");
    let script = "not json at all\n\
                  {\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"no/such\"}\n";
    let out = run_mcp_script(dir.path(), script);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 response lines: {out}");

    let bad = serde_json::from_str::<serde_json::Value>(lines[0]).unwrap();
    assert_eq!(bad["error"]["code"], -32700);
    assert!(bad["id"].is_null());

    let unknown = serde_json::from_str::<serde_json::Value>(lines[1]).unwrap();
    assert_eq!(unknown["id"], 9);
    assert_eq!(unknown["error"]["code"], -32601);
}
