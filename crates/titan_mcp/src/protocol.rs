//! Newline-delimited MCP JSON-RPC over stdio. Stdout contains protocol messages only.

use crate::{client::Client, tools};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 24 * 1024;
const MAX_ITEMS: usize = 100;
const PROTOCOL_VERSION: &str = "2025-11-25";

/// Serve MCP requests until EOF, with bounded request size and flushed responses.
pub fn serve(client: &Client, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut initialized = false;
    loop {
        let mut line = Vec::new();
        // Bound allocation even when an untrusted client never sends a newline.
        let mut oversized = false;
        loop {
            let available = input.fill_buf()?;
            if available.is_empty() {
                break;
            }
            let length = available
                .iter()
                .position(|b| *b == b'\n')
                .map_or(available.len(), |i| i + 1);
            let ended = available[length - 1] == b'\n';
            if line.len().saturating_add(length) > MAX_REQUEST_BYTES {
                oversized = true;
            } else if !oversized {
                line.extend_from_slice(&available[..length]);
            }
            input.consume(length);
            if ended {
                break;
            }
        }
        if line.is_empty() && !oversized {
            return Ok(());
        }
        let response = if oversized {
            Some(error(
                Value::Null,
                -32600,
                "Request exceeds 1 MiB; send a smaller request",
            ))
        } else {
            match serde_json::from_slice::<Value>(&line) {
                Ok(request) => handle(client, request, &mut initialized),
                Err(_) => Some(error(Value::Null, -32700, "Invalid JSON")),
            }
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

fn handle(client: &Client, request: Value, initialized: &mut bool) -> Option<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if !request.is_object()
        || request["jsonrpc"] != "2.0"
        || !request["method"].is_string()
        || request
            .get("id")
            .is_some_and(|id| !(id.is_string() || id.is_i64() || id.is_u64()))
    {
        return Some(error(Value::Null, -32600, "Invalid JSON-RPC request"));
    }
    // Notifications, including notifications/initialized, never receive a response.
    request.get("id")?;
    if request.get("params").is_some_and(|p| !p.is_object()) {
        return Some(error(id, -32602, "params must be an object"));
    }
    let result = match request["method"].as_str().unwrap_or_default() {
        "initialize" => {
            let params = &request["params"];
            if !params["protocolVersion"].is_string()
                || !params["capabilities"].is_object()
                || !params["clientInfo"].is_object()
            {
                return Some(error(
                    id,
                    -32602,
                    "initialize requires protocolVersion, capabilities, and clientInfo",
                ));
            }
            *initialized = true;
            // MCP permits offering our supported version when the requested version differs.
            let requested = params["protocolVersion"].as_str().unwrap_or_default();
            let version = match requested {
                "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25" => requested,
                _ => PROTOCOL_VERSION,
            };
            json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"titan_mcp","version":env!("CARGO_PKG_VERSION")},"instructions":"Inspect and control a local game exposing BRP. Use game_status first; time control needs TitanRemotePlugin."})
        }
        "ping" => json!({}),
        _ if !*initialized => return Some(error(id, -32000, "Call initialize before using tools")),
        "tools/list" => json!({"tools":tools::list()}),
        "tools/call" => {
            let params = &request["params"];
            let Some(name) = params["name"].as_str() else {
                return Some(error(id, -32602, "tools/call requires a tool name"));
            };
            if !tools::list()
                .as_array()
                .is_some_and(|list| list.iter().any(|tool| tool["name"] == name))
            {
                return Some(error(
                    id,
                    -32602,
                    "Unknown tool; use tools/list to see available tools",
                ));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !args.is_object() {
                return Some(error(id, -32602, "Tool arguments must be an object"));
            }
            match tools::call(client, name, args) {
                Ok(result) if name == "screenshot" => result,
                Ok(result) => {
                    json!({"content":[{"type":"text","text":compact(result)}],"isError":false})
                }
                Err(message) => json!({"content":[{"type":"text","text":message}],"isError":true}),
            }
        }
        _ => {
            return Some(error(
                id,
                -32601,
                "Method not found; supported methods: initialize, ping, tools/list, tools/call",
            ))
        }
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

// Keep structured JSON valid rather than slicing a string in the middle of a value.
fn compact(value: Value) -> String {
    let full = value.to_string();
    if full.len() <= MAX_TEXT_BYTES && value.as_array().is_none_or(|a| a.len() <= MAX_ITEMS) {
        return full;
    }
    let guidance = "Narrow the query with with/without filters or fetch fewer components; brp_call can request a smaller result.";
    let (items, already_omitted) = match value {
        Value::Array(items) => (Some(items), 0),
        Value::Object(mut object) if object.get("items").is_some_and(Value::is_array) => {
            let omitted = object.get("omitted").and_then(Value::as_u64).unwrap_or(0);
            let items = object.remove("items").and_then(|v| v.as_array().cloned());
            (items, omitted)
        }
        Value::Object(object) => {
            return json!({"truncated":true,"omitted_items":object.len(),"omitted_bytes":full.len(),"note":format!("Result exceeds the text limit; object entries were omitted. {guidance}")}).to_string();
        }
        _ => (None, 0),
    };
    if let Some(items) = items {
        let total = items.len();
        let mut kept = Vec::new();
        let mut bytes = 0;
        for item in items.into_iter().take(MAX_ITEMS) {
            let size = item.to_string().len();
            if bytes + size > MAX_TEXT_BYTES / 2 {
                break;
            }
            bytes += size;
            kept.push(item);
        }
        return json!({"omitted_items":already_omitted + (total-kept.len()) as u64,"items":kept,"truncated":true,"note":guidance}).to_string();
    }
    json!({"truncated":true,"omitted_bytes":full.len(),"note":format!("Result exceeds the text limit and was omitted. {guidance}")}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_and_malformed_requests_do_not_break_stdio() {
        let client = Client::new("http://127.0.0.1:1").unwrap();
        let mut input = vec![b'x'; MAX_REQUEST_BYTES + 1];
        input
            .extend_from_slice(b"\nnot json\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n");
        let mut output = Vec::new();
        serve(&client, input.as_slice(), &mut output).unwrap();
        let responses: Vec<Value> = output
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0]["error"]["code"], -32600);
        assert_eq!(responses[1]["error"]["code"], -32700);
        assert_eq!(responses[2]["id"], 7);
        assert_eq!(responses[2]["result"], json!({}));
    }

    #[test]
    fn truncation_keeps_json_valid_and_counts_omitted_items() {
        let result: Value =
            serde_json::from_str(&compact(json!((0..150).collect::<Vec<_>>()))).unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 100);
        assert_eq!(result["omitted_items"], 50);
        assert!(result["note"].as_str().unwrap().contains("Narrow"));
        let result: Value =
            serde_json::from_str(&compact(json!({"large":"x".repeat(MAX_TEXT_BYTES)}))).unwrap();
        assert_eq!(result["truncated"], true);
        let result: Value = serde_json::from_str(&compact(
            json!({"items":["x".repeat(MAX_TEXT_BYTES)],"omitted":42}),
        ))
        .unwrap();
        assert_eq!(result["omitted_items"], 43);
    }
}
