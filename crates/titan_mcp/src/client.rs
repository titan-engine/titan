//! Bounded, loopback-only HTTP transport for the Bevy Remote Protocol.

use core::{
    net::IpAddr,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use std::{io::Read, time::Instant};

use bevy_remote::{BrpPayload, BrpRequest, BrpResponse};
use serde_json::Value;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// A synchronous BRP client. No proxy, redirect, or remote host is permitted.
pub struct Client {
    url: String,
    agent: ureq::Agent,
    next_id: AtomicU64,
}

impl Client {
    /// Creates a client for an HTTP loopback URL, pinning `localhost` to IPv4
    /// loopback rather than trusting DNS or the system's hosts configuration.
    pub fn new(url: &str) -> Result<Self, String> {
        let url = local_url(url)?;
        let config = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(10)))
            .timeout_connect(Some(Duration::from_secs(2)))
            .build();
        Ok(Self {
            url,
            agent: ureq::Agent::new_with_config(config),
            next_id: AtomicU64::new(1),
        })
    }

    /// The validated, loopback-pinned BRP endpoint URL.
    ///
    /// Used by callers that need their own streaming (`+watch`) request.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Calls an instant BRP method and returns its JSON result.
    /// Streaming/watch methods require a separate streaming transport.
    pub fn call(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        self.call_with_deadline(method, params, Instant::now() + Duration::from_secs(10))
    }

    /// Calls an instant BRP method within a shared operation deadline.
    ///
    /// Both connection and response-body reads use the remaining budget. No request
    /// is started when the deadline has expired; a caller can share one deadline
    /// across discovery, capture, and completion polling.
    pub fn call_with_deadline(
        &self,
        method: &str,
        params: Option<Value>,
        deadline: Instant,
    ) -> Result<Value, String> {
        if method.ends_with("+watch") {
            return Err(format!(
                "BRP {method} is a streaming method; brp_call supports instant methods only"
            ));
        }
        let id = Value::from(self.next_id.fetch_add(1, Ordering::Relaxed));
        let request = BrpRequest {
            method: method.to_owned(),
            id: Some(id.clone()),
            params,
        };
        let body =
            serde_json::to_vec(&request).map_err(|e| format!("Cannot encode {method}: {e}"))?;
        if body.len() > MAX_REQUEST_BYTES {
            return Err(format!(
                "BRP request for {method} exceeds the 1 MiB limit; send less data"
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!(
                "BRP {method} timed out before request; operation deadline expired"
            ));
        }
        let mut response = self.agent.post(&self.url)
            .config()
            .timeout_global(Some(remaining))
            .timeout_connect(Some(Duration::from_secs(2).min(remaining)))
            .build()
            .header("Content-Type", "application/json")
            .send(body.as_slice())
            .map_err(|e| format!("BRP {method} at {} failed: {e}. Is the game running with RemotePlugin and RemoteHttpPlugin?", self.url))?;
        if !response.status().is_success() {
            return Err(format!(
                "BRP {method} returned HTTP {}; redirects are not allowed",
                response.status()
            ));
        }
        // Read at most limit+1 even when Content-Length is absent or misleading.
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take((MAX_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Reading BRP {method} response failed: {e}"))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(format!(
                "BRP {method} response exceeds the 32 MiB limit; narrow the query"
            ));
        }
        let response: BrpResponse = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid JSON-RPC response to {method}: {e}"))?;
        if response.id.as_ref() != Some(&id) {
            return Err(format!("BRP {method} returned a mismatched response id"));
        }
        match response.payload {
            BrpPayload::Result(result) => Ok(result),
            BrpPayload::Error(error) => Err(format!(
                "BRP {method} error {}: {}. Check the method parameters and use find_types to check reflected, registered types.",
                error.code, error.message
            )),
        }
    }
}

fn local_url(url: &str) -> Result<String, String> {
    let invalid = || {
        "BRP URL must be http://localhost, http://127.x.x.x, or http://[::1], with an optional port/path; credentials and fragments are not allowed".to_owned()
    };
    if url.contains('#') || url.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    let uri: ureq::http::Uri = url.parse().map_err(|_| invalid())?;
    if uri.scheme_str() != Some("http") {
        return Err(invalid());
    }
    let authority = uri.authority().ok_or_else(invalid)?;
    if authority.as_str().contains('@')
        || (authority.as_str().contains(':')
            && authority.port_u16().is_none()
            && !authority.as_str().ends_with(']'))
    {
        return Err(invalid());
    }
    let host = uri.host().ok_or_else(invalid)?;
    let host = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    let normalized = if host.eq_ignore_ascii_case("localhost") {
        "127.0.0.1".to_owned()
    } else {
        let ip: IpAddr = host.parse().map_err(|_| invalid())?;
        if !ip.is_loopback() {
            return Err(invalid());
        }
        match ip {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        }
    };
    let port = authority
        .port_u16()
        .map(|p| format!(":{p}"))
        .unwrap_or_default();
    let path = uri.path_and_query().map_or("/", |p| p.as_str());
    Ok(format!("http://{normalized}{port}{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    fn serve_once(status: &str, headers: &str, body: &str) -> (Client, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = Client::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        );
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
        });
        (client, worker)
    }

    #[test]
    fn response_ids_and_rpc_errors_are_checked() {
        for (body, expected) in [
            (r#"{"jsonrpc":"2.0","id":2,"result":null}"#, "mismatched"),
            (
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"missing method"}}"#,
                "-32601: missing method",
            ),
            ("not json", "Invalid JSON-RPC"),
        ] {
            let (client, worker) = serve_once("200 OK", "", body);
            assert!(client
                .call("world.query", None)
                .unwrap_err()
                .contains(expected));
            worker.join().unwrap();
        }
        let (client, worker) = serve_once(
            "200 OK",
            "",
            r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#,
        );
        assert_eq!(
            client.call("world.query", None).unwrap(),
            json!({"ok":true})
        );
        worker.join().unwrap();
    }

    #[test]
    fn redirects_are_not_followed() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let headers = format!(
            "Location: http://{}/\r\n",
            destination.local_addr().unwrap()
        );
        let (client, worker) = serve_once("307 Temporary Redirect", &headers, "");
        assert!(client.call("rpc.discover", None).is_err());
        worker.join().unwrap();
        assert_eq!(
            destination.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn oversized_requests_and_watch_methods_fail_without_connecting() {
        let client = Client::new("http://127.0.0.1:1").unwrap();
        assert!(client
            .call(
                "world.query",
                Some(json!({"data":"x".repeat(MAX_REQUEST_BYTES)}))
            )
            .unwrap_err()
            .contains("1 MiB"));
        assert!(client
            .call("world.observe+watch", None)
            .unwrap_err()
            .contains("streaming"));
    }

    #[test]
    fn only_loopback_http_is_allowed() {
        for url in [
            "https://localhost",
            "http://example.com",
            "http://192.168.1.1",
            "http://127.0.0.1.evil",
            "http://user@localhost",
            "http://localhost/#fragment",
            "http://localhost:99999",
            "http://[::ffff:127.0.0.1]",
        ] {
            assert!(Client::new(url).is_err(), "accepted {url}");
        }
        for url in [
            "http://127.0.0.1:15702",
            "http://127.2.3.4",
            "http://[::1]:15702/path",
        ] {
            assert!(Client::new(url).is_ok(), "rejected {url}");
        }
        assert_eq!(
            local_url("http://localhost:15702/brp?x=1").unwrap(),
            "http://127.0.0.1:15702/brp?x=1"
        );
    }
}
