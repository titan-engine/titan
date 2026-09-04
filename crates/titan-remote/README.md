# Native runtime transport

`titan-remote` provides the Unix native transport (macOS and Linux). Windows
registration ownership, secure randomness, and process liveness support remain
future platform work. The engine and `titan-protocol` remain transport-neutral.

Start a `Server` with `ServerConfig::new(project, instance_id, run_mode)`. Keep the
server alive and call the returned `RequestQueue::drain` at runtime safe points.
The bounded queue carries only protocol envelopes; network workers never receive
an `App` or `World`. Each drain processes at most the configured queue capacity.

The server binds an ephemeral IPv4 loopback port and writes a random-token
registration under `<project>/target/titan/instances`. The registry directory
must be owner-only (0700), and registration files are created exclusively with
0600 permissions. Tokens are bearer credentials and must not be printed in
instance listings. `Registration` intentionally redacts its token from `Debug`.
Use `discover`, `select`, and `send` to attach. Discovery ignores malformed,
insecure, wrong-project, dead-process, and unreachable registrations; it does not
remove another process's files. Multiple matches require explicit selection.
Discovery's process/endpoint liveness check is advisory; the authenticated
request and response identity checks establish the actual connection.

The HTTP adapter supports one `POST /request HTTP/1.1` with a content length per
connection. It rejects browser Origin headers, chunked encoding, duplicate
content lengths, oversized headers (8 KiB), and oversized bodies (4 MiB). At most
16 workers may run concurrently. Requests must authenticate with the registration
token. Queue overload and timeout produce HTTP 503 and 504, respectively, which
`send` reports as `RemoteError::Busy` and `RemoteError::Timeout`. Protocol errors
remain runtime-generated response envelopes with authoritative frame/revision.

The parser has a two-second server read deadline. Runtime requests have a
configurable deadline (default five seconds, maximum thirty seconds). `send`
transmits a same-host absolute deadline, clamped to the server limit, to prevent
queued work starting after a shorter client deadline. Expired requests are
skipped before handler entry. **A timeout after a handler has started cannot
roll back its effects**, so callers must inspect state before retrying a mutation.
Custom HTTP clients should send `X-Titan-Deadline-Unix-Ms` to share their deadline.
Dropping the server cancels queued work, removes its registration, and joins its
workers. A slow network worker can take up to its bounded I/O timeout to exit.
