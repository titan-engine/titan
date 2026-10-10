//! MCP tool descriptions and dispatch to BRP's existing parameter types.

use std::{
    thread,
    time::{Duration, Instant},
};

use bevy_ecs::entity::Entity;
use bevy_platform::collections::HashMap;
use bevy_remote::builtin_methods::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::Client;

const WINDOW: &str = "bevy_window::window::Window";
const PRIMARY_WINDOW: &str = "bevy_window::window::PrimaryWindow";
const WINDOW_EVENT: &str = "bevy_window::event::WindowEvent";
const CURSOR_MOVED: &str = "bevy_window::event::CursorMoved";
const CURSOR_ENTERED: &str = "bevy_window::event::CursorEntered";
const KEYBOARD_INPUT: &str = "bevy_input::keyboard::KeyboardInput";
const MOUSE_BUTTON_INPUT: &str = "bevy_input::mouse::MouseButtonInput";
const POLL_TIMEOUT: Duration = Duration::from_secs(15);

/// Returns the MCP tools and their input schemas (not a tools/list envelope).
pub fn list() -> Value {
    let string = json!({"type":"string"});
    let entity = json!({"type":"integer","minimum":0,"description":"Bevy entity ID, as returned by query_entities/spawn_entity"});
    let types = json!({"type":"array","items":{"type":"string"},"description":"Full type paths or unambiguous short names"});
    let components = json!({"type":"object","additionalProperties":true,"description":"Map from component type names to reflected JSON values"});
    let limit = json!({"type":"integer","minimum":1,"maximum":100,"default":50});
    json!([
        tool("game_status", "Discover the running game's methods and Titan status, when available", json!({}), &[]),
        tool("query_entities", "Query components and filter entities; use limit to keep results small", json!({"components":types,"with":types,"without":types,"option":types,"has":types,"strict":{"type":"boolean","default":false},"limit":limit}), &[]),
        tool("get_components", "Read components of an entity", json!({"entity":entity,"components":types,"strict":{"type":"boolean","default":false}}), &["entity","components"]),
        tool("list_components", "List registered components, or components on an entity", json!({"entity":entity}), &[]),
        tool("set_component", "Mutate a component field using a Bevy reflection field path", json!({"entity":entity,"component":string,"path":string,"value":{}}), &["entity","component","path","value"]),
        tool("insert_components", "Insert or replace components on an entity", json!({"entity":entity,"components":components}), &["entity","components"]),
        tool("remove_components", "Remove components from an entity", json!({"entity":entity,"components":types}), &["entity","components"]),
        tool("spawn_entity", "Spawn an entity with reflected components", json!({"components":components}), &["components"]),
        tool("despawn_entity", "Despawn an entity", json!({"entity":entity}), &["entity"]),
        tool("list_resources", "List resource type paths", json!({}), &[]),
        tool("get_resource", "Read a reflected resource", json!({"resource":string}), &["resource"]),
        tool("set_resource", "Mutate a resource field, or insert/replace the resource when path is omitted", json!({"resource":string,"path":string,"value":{}}), &["resource","value"]),
        tool("find_types", "Search reflected type names by case-insensitive substring; returns bounded matches, never the whole registry", json!({"query":{"type":"string","minLength":1},"limit":limit}), &["query"]),
        tool("send_key", "Send a Bevy key code (KeyA, Space, ArrowUp); taps separate press and release across frames. Optional logical_key is reflected Key JSON", json!({"key":string,"action":{"type":"string","enum":["press","release","tap"],"default":"tap"},"window":entity,"logical_key":{},"text":string}), &["key"]),
        tool("click", "Move cursor and click in logical window coordinates, separating move, press and release across frames", json!({"x":{"type":"number"},"y":{"type":"number"},"window":entity,"button":{"type":"string","enum":["Left","Right","Middle","Back","Forward"],"default":"Left"}}), &["x","y"]),
        tool("screenshot", "Capture the primary window as an MCP PNG image. Uses titan.screenshot when available, otherwise a slower BRP Screenshot + world.observe fallback", json!({"timeout_secs":{"type":"number","exclusiveMinimum":0,"maximum":60,"default":10}}), &[]),
        tool("pause", "Pause virtual time (requires TitanRemotePlugin)", json!({}), &[]),
        tool("resume", "Resume virtual time (requires TitanRemotePlugin)", json!({}), &[]),
        tool("step", "Advance frames with fixed delta, wait for completion, and pause again (requires TitanRemotePlugin)", json!({"frames":{"type":"integer","minimum":1,"maximum":4294967295_u32},"dt_secs":{"type":"number","exclusiveMinimum":0,"maximum":1,"description":"Seconds per frame; must round to at least one nanosecond (default 1/60)"}}), &["frames"]),
        tool("brp_call", "Call an arbitrary instant BRP method; streaming/watch methods are not supported", json!({"method":string,"params":{}}), &["method"])
    ])
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}

/// Dispatches a tool, returning raw JSON (screenshot returns MCP image content).
pub fn call(client: &Client, name: &str, args: Value) -> Result<Value, String> {
    validate_args(name, &args)?;
    let mut registry = Registry {
        client,
        schemas: None,
    };
    match name {
        "game_status" => {
            let discovery = discover(client)?;
            let status = if supports(&discovery, "titan.status") {
                client.call("titan.status", None)?
            } else {
                Value::Null
            };
            Ok(
                json!({"reachable":true,"discovery":discovery,"status":status,"titan_remote_available":!status.is_null()}),
            )
        }
        "query_entities" => {
            let params = BrpQueryParams {
                data: BrpQuery {
                    components: registry.paths(&args, "components")?,
                    option: ComponentSelector::Paths(registry.paths(&args, "option")?),
                    has: registry.paths(&args, "has")?,
                },
                filter: BrpQueryFilter {
                    with: registry.paths(&args, "with")?,
                    without: registry.paths(&args, "without")?,
                },
                strict: optional_bool(&args, "strict", false)?,
            };
            let limit = result_limit(&args)?;
            let result = builtin(client, BRP_QUERY_METHOD, &params)?;
            Ok(bounded_array(result, limit))
        }
        "get_components" => {
            let mut params: BrpGetComponentsParams = decode(args)?;
            params.components = registry.resolve_paths(params.components)?;
            builtin(client, BRP_GET_COMPONENTS_METHOD, &params)
        }
        "list_components" => {
            if args.get("entity").is_none() {
                client.call(BRP_LIST_COMPONENTS_METHOD, None)
            } else {
                builtin(
                    client,
                    BRP_LIST_COMPONENTS_METHOD,
                    &decode::<BrpListComponentsParams>(args)?,
                )
            }
        }
        "set_component" => {
            let mut params: BrpMutateComponentsParams = decode(args)?;
            params.component = registry.resolve(&params.component)?;
            builtin(client, BRP_MUTATE_COMPONENTS_METHOD, &params)
        }
        "insert_components" => {
            let mut params: BrpInsertComponentsParams = decode(args)?;
            params.components = registry.component_map(params.components)?;
            builtin(client, BRP_INSERT_COMPONENTS_METHOD, &params)
        }
        "remove_components" => {
            let mut params: BrpRemoveComponentsParams = decode(args)?;
            params.components = registry.resolve_paths(params.components)?;
            builtin(client, BRP_REMOVE_COMPONENTS_METHOD, &params)
        }
        "spawn_entity" => {
            let mut params: BrpSpawnEntityParams = decode(args)?;
            params.components = registry.component_map(params.components)?;
            builtin(client, BRP_SPAWN_ENTITY_METHOD, &params)
        }
        "despawn_entity" => builtin(
            client,
            BRP_DESPAWN_COMPONENTS_METHOD,
            &decode::<BrpDespawnEntityParams>(args)?,
        ),
        "list_resources" => client.call(BRP_LIST_RESOURCES_METHOD, None),
        "get_resource" => {
            let mut params: BrpGetResourcesParams = decode(args)?;
            params.resource = registry.resolve(&params.resource)?;
            builtin(client, BRP_GET_RESOURCE_METHOD, &params)
        }
        "set_resource" => {
            if args.get("path").is_some() {
                let mut params: BrpMutateResourcesParams = decode(args)?;
                params.resource = registry.resolve(&params.resource)?;
                builtin(client, BRP_MUTATE_RESOURCE_METHOD, &params)
            } else {
                let mut params: BrpInsertResourcesParams = decode(args)?;
                params.resource = registry.resolve(&params.resource)?;
                builtin(client, BRP_INSERT_RESOURCE_METHOD, &params)
            }
        }
        "find_types" => {
            let query = required_str(&args, "query")?.trim();
            if query.is_empty() {
                return Err(
                    "find_types query must be nonempty; use a specific name substring".to_owned(),
                );
            }
            let limit = result_limit(&args)?;
            let query = query.to_lowercase();
            let mut matches: Vec<_> = registry
                .schemas()?
                .as_object()
                .ok_or("registry.schema did not return a type map")?
                .iter()
                .filter(|(path, schema)| {
                    path.to_lowercase().contains(&query)
                        || schema
                            .get("shortPath")
                            .and_then(Value::as_str)
                            .is_some_and(|short| short.to_lowercase().contains(&query))
                })
                .map(|(path, _)| path.clone())
                .collect();
            matches.sort();
            let total = matches.len();
            matches.truncate(limit);
            Ok(
                json!({"types":matches,"total":total,"omitted":total.saturating_sub(limit),"note":if total > limit { "Narrow query or raise limit (maximum 100) to see more matches" } else { "" }}),
            )
        }
        "send_key" => send_key(client, &args),
        "click" => click(client, &args),
        "screenshot" => crate::screenshot::capture(client, &args),
        "pause" | "resume" => {
            let method = format!("titan.{name}");
            require_titan(client, &[&method])?;
            client.call(&method, None)
        }
        "step" => step(client, &args),
        "brp_call" => client.call(required_str(&args, "method")?, args.get("params").cloned()),
        _ => Err(format!(
            "Unknown tool `{name}`; use tools/list to see available tools"
        )),
    }
}

// Validate the schema vocabulary used by list(), keeping schemas and dispatch
// in sync without introducing a general-purpose schema dependency.
fn validate_args(name: &str, args: &Value) -> Result<(), String> {
    let tools = list();
    let tool = tools
        .as_array()
        .and_then(|tools| tools.iter().find(|tool| tool["name"] == name))
        .ok_or_else(|| format!("Unknown tool `{name}`; use tools/list to see available tools"))?;
    validate_schema(args, &tool["inputSchema"], name)
}

fn validate_schema(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        let valid = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_u64().is_some() || value.as_i64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            _ => false,
        };
        if !valid {
            return Err(format!("`{path}` must be {kind}"));
        }
    }
    if let Some(variants) = schema.get("enum").and_then(Value::as_array)
        && !variants.contains(value)
    {
        return Err(format!(
            "`{path}` must be one of {}",
            Value::Array(variants.clone())
        ));
    }
    if let Some(fields) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for field in required.iter().filter_map(Value::as_str) {
                if !fields.contains_key(field) {
                    return Err(format!("Missing required argument `{path}.{field}`"));
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (field, value) in fields {
            if let Some(child) = properties.and_then(|props| props.get(field)) {
                validate_schema(value, child, &format!("{path}.{field}"))?;
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(format!(
                    "Unknown argument `{path}.{field}`; check tools/list for valid fields"
                ));
            }
        }
    }
    if let Some(items) = value.as_array()
        && let Some(child) = schema.get("items")
    {
        for (index, item) in items.iter().enumerate() {
            validate_schema(item, child, &format!("{path}[{index}]"))?;
        }
    }
    if let Some(text) = value.as_str()
        && let Some(minimum) = schema.get("minLength").and_then(Value::as_u64)
        && (text.chars().count() as u64) < minimum
    {
        return Err(format!(
            "`{path}` must contain at least {minimum} characters"
        ));
    }
    if let Some(number) = value.as_f64() {
        for key in ["minimum", "maximum", "exclusiveMinimum"] {
            if let Some(bound) = schema.get(key).and_then(Value::as_f64) {
                let invalid = match key {
                    "minimum" => number < bound,
                    "maximum" => number > bound,
                    _ => number <= bound,
                };
                if invalid {
                    return Err(format!("`{path}` violates {key} {bound}"));
                }
            }
        }
    }
    Ok(())
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| format!("Invalid tool arguments: {e}"))
}

fn builtin(client: &Client, method: &str, params: &impl Serialize) -> Result<Value, String> {
    let params =
        serde_json::to_value(params).map_err(|e| format!("Invalid {method} parameters: {e}"))?;
    client.call(method, Some(params))
}

fn required_str<'a>(args: &'a Value, field: &str) -> Result<&'a str, String> {
    args.get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("`{field}` must be a nonempty string"))
}

fn optional_bool(args: &Value, field: &str, default: bool) -> Result<bool, String> {
    match args.get(field) {
        None => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("`{field}` must be a boolean")),
    }
}

fn result_limit(args: &Value) -> Result<usize, String> {
    match args.get("limit") {
        None => Ok(50),
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=100).contains(n))
            .map(|n| n as usize)
            .ok_or_else(|| "`limit` must be an integer between 1 and 100".to_owned()),
    }
}

fn bounded_array(result: Value, limit: usize) -> Value {
    match result {
        Value::Array(mut rows) if rows.len() > limit => {
            let omitted = rows.len() - limit;
            rows.truncate(limit);
            json!({"items":rows,"omitted":omitted,"note":"Result truncated; narrow with/without filters or raise limit (maximum 100)"})
        }
        other => other,
    }
}

struct Registry<'a> {
    client: &'a Client,
    schemas: Option<Value>,
}

impl Registry<'_> {
    fn schemas(&mut self) -> Result<&Value, String> {
        if self.schemas.is_none() {
            self.schemas = Some(builtin(
                self.client,
                BRP_REGISTRY_SCHEMA_METHOD,
                &BrpJsonSchemaQueryFilter::default(),
            )?);
        }
        self.schemas
            .as_ref()
            .ok_or_else(|| "Missing registry schema".to_owned())
    }

    fn resolve(&mut self, name: &str) -> Result<String, String> {
        if name.is_empty() {
            return Err("Type name must not be empty".to_owned());
        }
        // Full paths already satisfy BRP's contract; only short names require discovery.
        if name.contains("::") && !name.contains('<') {
            return Ok(name.to_owned());
        }
        resolve_short(self.schemas()?, name)
    }

    fn resolve_paths(&mut self, paths: Vec<String>) -> Result<Vec<String>, String> {
        paths.into_iter().map(|p| self.resolve(&p)).collect()
    }

    fn paths(&mut self, args: &Value, field: &str) -> Result<Vec<String>, String> {
        let paths = match args.get(field) {
            None => Vec::new(),
            Some(value) => decode::<Vec<String>>(value.clone())?,
        };
        self.resolve_paths(paths)
    }

    fn component_map(
        &mut self,
        components: HashMap<String, Value>,
    ) -> Result<HashMap<String, Value>, String> {
        let mut resolved = HashMap::default();
        for (name, value) in components {
            let path = self.resolve(&name)?;
            if resolved.insert(path.clone(), value).is_some() {
                return Err(format!(
                    "Duplicate component `{path}` via different type names"
                ));
            }
        }
        Ok(resolved)
    }
}

fn resolve_short(schemas: &Value, name: &str) -> Result<String, String> {
    let map = schemas
        .as_object()
        .ok_or("registry.schema did not return a type map")?;
    // Schema shortPath also handles generic types whose final :: segment is inside a type parameter.
    let mut candidates: Vec<_> = map
        .iter()
        .filter(|(path, schema)| {
            path.as_str() == name
                || path.rsplit("::").next() == Some(name)
                || schema.get("shortPath").and_then(Value::as_str) == Some(name)
        })
        .map(|(path, _)| path.clone())
        .collect();
    candidates.sort();
    match candidates.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(format!("Type `{name}` not found in registry; use find_types to search. Is it #[derive(Reflect)] and registered with app.register_type()?")),
        _ => Err(format!("Type `{name}` is ambiguous; use a full path: {}", candidates.join(", "))),
    }
}

fn discover(client: &Client) -> Result<Value, String> {
    client.call(RPC_DISCOVER_METHOD, None)
}

fn supports(discovery: &Value, method: &str) -> bool {
    discovery
        .get("methods")
        .and_then(Value::as_array)
        .is_some_and(|methods| {
            methods
                .iter()
                .any(|m| m.get("name").and_then(Value::as_str) == Some(method))
        })
}

fn require_titan(client: &Client, methods: &[&str]) -> Result<(), String> {
    let discovery = discover(client)?;
    for method in methods {
        if !supports(&discovery, method) {
            return Err(format!("This game does not expose `{method}`; add TitanRemotePlugin to enable pause/resume/step"));
        }
    }
    Ok(())
}

fn step(client: &Client, args: &Value) -> Result<Value, String> {
    let frames = args
        .get("frames")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0 && *n <= u64::from(u32::MAX))
        .ok_or("`frames` must be a positive u32 integer")?;
    let mut params = json!({"frames":frames});
    if let Some(dt) = args.get("dt_secs") {
        let n = dt
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.0 && *n <= 1.0)
            .ok_or("`dt_secs` must be finite and in (0, 1]")?;
        if Duration::from_secs_f32(n as f32).is_zero() {
            return Err("`dt_secs` must round to at least one nanosecond".to_owned());
        }
        params["dt_secs"] = dt.clone();
    }
    require_titan(client, &["titan.step", "titan.status"])?;
    let deadline = Instant::now() + POLL_TIMEOUT;
    let response = client.call_with_deadline("titan.step", Some(params), deadline)?;
    let target = response
        .get("target_frame")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("titan.step returned no valid u32 target_frame")?;
    loop {
        let status = client.call_with_deadline("titan.status", None, deadline)?;
        // FrameCount continues while paused and wraps at u32::MAX. The
        // server's pending count, not numerical ordering against target_frame,
        // is authoritative for completion. Use a single controller for steps.
        if parse_titan_status(&status)?.step_completed()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(format!("Timed out waiting for step target {target}; last status: {status}. Check that the app update loop is running"));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[derive(Deserialize)]
struct TitanStatus {
    paused: bool,
    frame: u32,
    pending_steps: u32,
}

impl TitanStatus {
    fn step_completed(&self) -> Result<bool, String> {
        if self.pending_steps != 0 {
            return Ok(false);
        }
        if !self.paused {
            return Err("Step ended without pausing; another controller may have resumed or cancelled it. Check game_status".to_owned());
        }
        Ok(true)
    }
}

fn parse_titan_status(value: &Value) -> Result<TitanStatus, String> {
    serde_json::from_value(value.clone())
        .map_err(|e| format!("titan.status returned invalid status (expected paused, u32 frame and pending_steps): {e}"))
}

fn two_frames_elapsed(before: u32, after: u32) -> bool {
    after.wrapping_sub(before) >= 2
}

fn input_window(client: &Client, args: &Value, allow_headless: bool) -> Result<Value, String> {
    if let Some(window) = args.get("window") {
        let entity: Entity = decode(window.clone())?;
        return serde_json::to_value(entity).map_err(|e| e.to_string());
    }
    let query = |with: Vec<String>| {
        builtin(
            client,
            BRP_QUERY_METHOD,
            &BrpQueryParams {
                data: BrpQuery::default(),
                filter: BrpQueryFilter {
                    with,
                    without: Vec::new(),
                },
                strict: false,
            },
        )
    };
    // Use full paths so a headless app need not register Window or PrimaryWindow.
    let rows = query(vec![WINDOW.to_owned(), PRIMARY_WINDOW.to_owned()])?;
    if let Some(row) = rows.as_array().and_then(|rows| rows.first()) {
        return row
            .get("entity")
            .cloned()
            .ok_or_else(|| "Window query returned no entity".to_owned());
    }
    let rows = query(vec![WINDOW.to_owned()])?;
    if let Some(rows) = rows.as_array() {
        if rows.len() == 1 {
            return Ok(rows[0]["entity"].clone());
        }
        if rows.len() > 1 {
            return Err("Multiple windows found; pass `window` explicitly".to_owned());
        }
    }
    if allow_headless {
        serde_json::to_value(Entity::PLACEHOLDER).map_err(|e| e.to_string())
    } else {
        Err("No window found; pass a valid `window` entity or run a windowed app".to_owned())
    }
}

fn write_message(client: &Client, message: &str, value: Value) -> Result<Value, String> {
    builtin(
        client,
        BRP_WRITE_MESSAGE_METHOD,
        &BrpWriteMessageParams {
            message: message.to_owned(),
            value: Some(value),
        },
    )
}

// With Titan status we can confirm that the input has traversed PreUpdate.
// Without it, allow 100 ms (six frames at 60 Hz) between messages. This is a
// best-effort fallback for vanilla BRP, which exposes no frame barrier.
fn separate_frames(client: &Client, has_status: bool) -> Result<(), String> {
    if !has_status {
        thread::sleep(Duration::from_millis(100));
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = client.call_with_deadline("titan.status", None, deadline)?;
    let frame = parse_titan_status(&status)?.frame;
    loop {
        thread::sleep(Duration::from_millis(10));
        let status = client.call_with_deadline("titan.status", None, deadline)?;
        let now = parse_titan_status(&status)?.frame;
        if two_frames_elapsed(frame, now) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(
                "Timed out separating input frames; check that the game's update loop is running"
                    .to_owned(),
            );
        }
    }
}

fn send_key(client: &Client, args: &Value) -> Result<Value, String> {
    let key = required_str(args, "key")?;
    let action = args
        .get("action")
        .map_or(Ok("tap"), |_| required_str(args, "action"))?;
    if !["press", "release", "tap"].contains(&action) {
        return Err("`action` must be press, release, or tap".to_owned());
    }
    let window = input_window(client, args, true)?;
    let logical = args
        .get("logical_key")
        .cloned()
        .unwrap_or_else(|| logical_key(key));
    let text = args
        .get("text")
        .cloned()
        .unwrap_or_else(|| logical.get("Character").cloned().unwrap_or(Value::Null));
    if !text.is_null() && !text.is_string() {
        return Err("`text` must be a string".to_owned());
    }
    let has_status = action == "tap" && supports(&discover(client)?, "titan.status");
    let send = |state: &str| {
        let value = json!({
            "key_code":key,"logical_key":logical,"state":state,"text":if state == "Pressed" { text.clone() } else { Value::Null },"repeat":false,"window":window
        });
        // Match winit's fan-out: ButtonInput/raw readers and ordered window
        // readers must both receive each keyboard phase. Attempt both even if
        // one delivery fails, especially when releasing a partially sent press.
        let input = write_message(client, KEYBOARD_INPUT, value.clone());
        let aggregate = write_message(client, WINDOW_EVENT, json!({"KeyboardInput":value}));
        input?;
        aggregate
    };
    if action == "release" {
        return send("Released");
    }
    let result = match send("Pressed") {
        Ok(result) => result,
        Err(error) => {
            let _ = send("Released");
            return Err(error);
        }
    };
    if action == "tap" {
        let barrier = separate_frames(client, has_status);
        // Even if the barrier fails, release the key to avoid leaving it held.
        let release = send("Released");
        barrier?;
        let release = release?;
        // Return only once the release has traversed PreUpdate, so a follow-up
        // query or screenshot never observes the key still held.
        separate_frames(client, has_status)?;
        Ok(release)
    } else {
        Ok(result)
    }
}

fn logical_key(key: &str) -> Value {
    if let Some(letter) = key.strip_prefix("Key").filter(|s| s.len() == 1) {
        return json!({"Character":letter.to_lowercase()});
    }
    if let Some(digit) = key.strip_prefix("Digit").filter(|s| s.len() == 1) {
        return json!({"Character":digit});
    }
    if key == "Space" {
        return json!({"Character":" "});
    }
    // Named Key variants are unit variants in Bevy's reflected representation.
    match key {
        "ShiftLeft" | "ShiftRight" => json!("Shift"),
        "ControlLeft" | "ControlRight" => json!("Control"),
        "AltLeft" | "AltRight" => json!("Alt"),
        "SuperLeft" | "SuperRight" => json!("Super"),
        "Enter" | "NumpadEnter" => json!("Enter"),
        "Escape" | "Tab" | "Backspace" | "Delete" | "Insert" | "Home" | "End" | "PageUp"
        | "PageDown" | "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" => json!(key),
        _ if key
            .strip_prefix('F')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=35).contains(&n)) =>
        {
            json!(key)
        }
        _ => json!({"Unidentified":"Unidentified"}),
    }
}

fn click(client: &Client, args: &Value) -> Result<Value, String> {
    let x = args
        .get("x")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && (*n as f32).is_finite())
        .map(|n| n as f32 as f64)
        .ok_or("`x` must be a finite number representable by Bevy's Vec2")?;
    let y = args
        .get("y")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && (*n as f32).is_finite())
        .map(|n| n as f32 as f64)
        .ok_or("`y` must be a finite number representable by Bevy's Vec2")?;
    let button = args
        .get("button")
        .map_or(Ok("Left"), |_| required_str(args, "button"))?;
    if !["Left", "Right", "Middle", "Back", "Forward"].contains(&button) {
        return Err("`button` must be Left, Right, Middle, Back, or Forward".to_owned());
    }
    let window = input_window(client, args, false)?;
    let has_status = supports(&discover(client)?, "titan.status");
    let entity = decode(window.clone())?;
    let components = builtin(
        client,
        BRP_GET_COMPONENTS_METHOD,
        &BrpGetComponentsParams {
            entity,
            components: vec![WINDOW.to_owned()],
            strict: true,
        },
    )?;
    let resolution = &components[WINDOW]["resolution"];
    let scale_value = match resolution.get("scale_factor_override") {
        Some(Value::Null) => &resolution["scale_factor"],
        Some(value) => value,
        None => return Err("Window resolution has no scale_factor_override field".to_owned()),
    };
    let scale = scale_value
        .as_f64()
        .filter(|scale| (*scale as f32).is_finite() && (*scale as f32) > 0.0)
        .map(|scale| scale as f32 as f64)
        .ok_or("Window resolution must have a positive, finite effective scale factor")?;
    let physical = [x * scale, y * scale];
    if physical
        .iter()
        .any(|n| !n.is_finite() || !(*n as f32).is_finite())
    {
        return Err("Scaled cursor position isn't representable by Bevy's Vec2".to_owned());
    }
    // Match Window::physical_cursor_position(): an absent or out-of-bounds
    // previous physical position means the cursor has just entered the window.
    // Reuse the fetched snapshot so this adds no requests to the frame barrier.
    let previous: Option<[f64; 2]> =
        decode(components[WINDOW]["internal"]["physical_cursor_position"].clone())?;
    let delta = if let Some(previous) = previous {
        let width = resolution["physical_width"]
            .as_u64()
            .ok_or("Window resolution has no physical_width")? as f64;
        let height = resolution["physical_height"]
            .as_u64()
            .ok_or("Window resolution has no physical_height")? as f64;
        (previous[0] >= 0.0 && previous[1] >= 0.0 && previous[0] < width && previous[1] < height)
            .then(|| {
                // Winit converts both physical positions to Vec2 before subtracting
                // and dividing by the effective scale, all in f32.
                [
                    (physical[0] as f32 - previous[0] as f32) / scale as f32,
                    (physical[1] as f32 - previous[1] as f32) / scale as f32,
                ]
            })
    } else {
        None
    };
    if delta.is_some_and(|delta| delta.iter().any(|n| !n.is_finite())) {
        return Err("Cursor delta isn't representable by Bevy's Vec2".to_owned());
    }
    // Like winit, synchronize the target Window before announcing the move.
    // Legacy UI reads this physical position; CursorMoved/picking use logical
    // coordinates. Mutate only the cursor field, never replace a stale Window.
    builtin(
        client,
        BRP_MUTATE_COMPONENTS_METHOD,
        &BrpMutateComponentsParams {
            entity,
            component: WINDOW.to_owned(),
            path: "internal.physical_cursor_position".to_owned(),
            value: json!(physical),
        },
    )?;
    // Winit announces entry before the first move inside the window; games
    // tracking hover through CursorEntered would otherwise ignore the click.
    if delta.is_none() {
        let entered = json!({"window":window});
        let standalone = write_message(client, CURSOR_ENTERED, entered.clone());
        let aggregate = write_message(client, WINDOW_EVENT, json!({"CursorEntered":entered}));
        standalone?;
        aggregate?;
    }
    let cursor = json!({"window":window,"position":[x,y],"delta":delta});
    let standalone = write_message(client, CURSOR_MOVED, cursor.clone());
    let aggregate = write_message(client, WINDOW_EVENT, json!({"CursorMoved":cursor}));
    standalone?;
    aggregate?;
    separate_frames(client, has_status)?;
    let send = |state: &str| {
        let value = json!({"button":button,"state":state,"window":window});
        // BRP bypasses winit's fan-out. ButtonInput and raw mouse readers need
        // the standalone message, while picking consumes the WindowEvent.
        let input = write_message(client, MOUSE_BUTTON_INPUT, value.clone());
        let aggregate = write_message(client, WINDOW_EVENT, json!({"MouseButtonInput":value}));
        // Attempt both deliveries, including release if one consumer is unavailable.
        input?;
        aggregate
    };
    if let Err(error) = send("Pressed") {
        let _ = send("Released");
        return Err(error);
    }
    let barrier = separate_frames(client, has_status);
    let release = send("Released");
    barrier?;
    let release = release?;
    // As with key taps, wait until the release (and any click observers it
    // triggers) has been processed before reporting success.
    separate_frames(client, has_status)?;
    Ok(release)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_status_and_input_frame_barriers() {
        let pending =
            parse_titan_status(&json!({"paused":false,"frame":u32::MAX,"pending_steps":1}))
                .unwrap();
        assert!(!pending.step_completed().unwrap());
        let finished =
            parse_titan_status(&json!({"paused":true,"frame":0,"pending_steps":0})).unwrap();
        assert!(finished.step_completed().unwrap());
        assert!(
            parse_titan_status(&json!({"paused":true,"frame":u64::MAX,"pending_steps":0})).is_err()
        );
        assert!(
            parse_titan_status(&json!({"paused":false,"frame":1,"pending_steps":0}))
                .unwrap()
                .step_completed()
                .is_err()
        );
        assert!(!two_frames_elapsed(u32::MAX, 0));
        assert!(two_frames_elapsed(u32::MAX - 1, 0));
        assert!(two_frames_elapsed(u32::MAX, 1));
        assert!(!two_frames_elapsed(5, 6));
        assert!(two_frames_elapsed(5, 7));
    }

    #[test]
    fn schemas_are_unique_objects() {
        let tools = list();
        let mut names = std::collections::HashSet::new();
        for tool in tools.as_array().unwrap() {
            assert!(names.insert(tool["name"].as_str().unwrap()));
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
        assert_eq!(names.len(), 20);
    }

    #[test]
    fn short_names_require_unique_matches() {
        let schemas = json!({"a::Transform":{},"b::Transform":{},"a::Player":{}});
        assert_eq!(resolve_short(&schemas, "Player").unwrap(), "a::Player");
        let error = resolve_short(&schemas, "Transform").unwrap_err();
        assert!(error.contains("a::Transform") && error.contains("b::Transform"));
        assert!(resolve_short(&schemas, "Missing")
            .unwrap_err()
            .contains("register_type"));
    }

    #[test]
    fn arguments_are_validated_before_destructive_dispatch() {
        let client = Client::new("http://127.0.0.1:1").unwrap();
        for (tool, args, expected) in [
            (
                "despawn_entity",
                json!({"entity":1,"extra":2}),
                "Unknown argument",
            ),
            ("remove_components", json!({"entity":1}), "Missing required"),
            ("despawn_entity", json!({"entity":"1"}), "integer"),
            (
                "remove_components",
                json!({"entity":1,"components":[false]}),
                "string",
            ),
            ("send_key", json!({"key":"KeyA","action":"typo"}), "one of"),
            ("click", json!({"x":1e100,"y":0}), "representable"),
            ("click", json!({"x":0,"y":-1e100}), "representable"),
            ("step", json!({"frames":0}), "minimum"),
            ("step", json!({"frames":1,"dt_secs":2}), "maximum"),
            ("step", json!({"frames":1,"dt_secs":1e-10}), "nanosecond"),
        ] {
            assert!(call(&client, tool, args).unwrap_err().contains(expected));
        }
        assert!(validate_args(
            "set_component",
            &json!({"entity":1,"component":"Foo","path":"field","value":{"arbitrary":true}})
        )
        .is_ok());
        assert!(validate_args(
            "brp_call",
            &json!({"method":"world.query","params":{"anything":[1,2]}})
        )
        .is_ok());
    }

    #[test]
    fn generic_short_paths_use_the_actual_schema_wire_field() {
        use bevy_remote::schemas::json_schema::JsonSchemaBevyType;
        let schema = JsonSchemaBevyType {
            short_path: "Wrapper<Foo>".to_owned(),
            type_path: "game::Wrapper<other::Foo>".to_owned(),
            ..Default::default()
        };
        let schema = serde_json::to_value(schema).unwrap();
        assert_eq!(schema["shortPath"], "Wrapper<Foo>");
        let registry = json!({"game::Wrapper<other::Foo>":schema});
        assert_eq!(
            resolve_short(&registry, "Wrapper<Foo>").unwrap(),
            "game::Wrapper<other::Foo>"
        );
        assert_eq!(
            resolve_short(&registry, "game::Wrapper<other::Foo>").unwrap(),
            "game::Wrapper<other::Foo>"
        );
    }

    #[test]
    fn truncation_reports_omitted_rows() {
        assert_eq!(bounded_array(json!([1, 2, 3]), 2)["omitted"], 1);
        assert_eq!(bounded_array(json!([1]), 2), json!([1]));
        assert!(result_limit(&json!({"limit":0})).is_err());
        assert!(result_limit(&json!({"limit":101})).is_err());
    }

    #[test]
    fn common_keys_have_logical_values() {
        assert_eq!(logical_key("KeyA"), json!({"Character":"a"}));
        assert_eq!(logical_key("Space"), json!({"Character":" "}));
        assert_eq!(logical_key("ArrowUp"), json!("ArrowUp"));
    }
}
