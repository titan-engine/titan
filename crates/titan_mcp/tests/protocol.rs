//! Exercise the actual binary's newline-delimited stdio transport.

use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn exchange(requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_titan_mcp"))
        .args(["--url", "http://127.0.0.1:1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for request in requests {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn initialize() -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})
}

#[test]
fn initialize_list_call_and_notifications() {
    let responses = exchange(&[
        initialize(),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":"tools","method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"game_status","arguments":{}}}),
    ]);
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "titan_mcp");
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(responses[1]["id"], "tools");
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    for name in [
        "game_status",
        "query_entities",
        "get_components",
        "list_components",
        "set_component",
        "insert_components",
        "remove_components",
        "spawn_entity",
        "despawn_entity",
        "list_resources",
        "get_resource",
        "set_resource",
        "find_types",
        "send_key",
        "click",
        "screenshot",
        "pause",
        "resume",
        "step",
        "brp_call",
    ] {
        assert!(tools.iter().any(|t| t["name"] == name), "missing {name}");
    }
    assert_eq!(responses[2]["id"], 3);
    // A dead game must not kill the MCP connection or produce a protocol error.
    assert!(responses[2].get("result").is_some());
    assert!(responses[2]["result"]["content"].as_array().is_some());
    assert_eq!(responses[2]["result"]["isError"], true);
    assert!(responses[2]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("RemoteHttpPlugin"));
}

#[test]
fn malformed_params_unknown_tools_and_preinitialization() {
    let responses = exchange(&[
        json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}),
        initialize(),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"missing"}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"query_entities","arguments":[]}}),
        json!({"jsonrpc":"2.0","id":4,"method":"unknown"}),
        json!({"jsonrpc":"2.0","id":null,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":0.5,"method":"ping"}),
    ]);
    assert_eq!(responses[0]["error"]["code"], -32000);
    assert_eq!(responses[2]["error"]["code"], -32602);
    assert_eq!(responses[3]["error"]["code"], -32602);
    assert_eq!(responses[4]["error"]["code"], -32601);
    assert_eq!(responses[5]["error"]["code"], -32600);
    assert_eq!(responses[6]["error"]["code"], -32600);
}

#[test]
fn cli_url_overrides_environment_and_rejects_remote_hosts() {
    let output = Command::new(env!("CARGO_BIN_EXE_titan_mcp"))
        .env("TITAN_BRP_URL", "http://example.com")
        .args(["--url", "http://127.0.0.1:1"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = Command::new(env!("CARGO_BIN_EXE_titan_mcp"))
        .args(["--url", "http://example.com"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
