import test from "node:test";
import assert from "node:assert/strict";
import { bridgeResponse, typedArgument } from "./bridge.mjs";

const source = {};
const origin = "http://127.0.0.1:8080";
const envelope = { schema_version: 1, request_id: "bridge-test", request: { type: "status" } };
const event = { source, origin, data: { namespace: "titan.inspector", type: "request", envelope } };

test("same-window bridge preserves request and response envelopes", () => {
  const response = { schema_version: 1, request_id: "bridge-test", instance_id: "browser", observed_frame: 11, state_revision: 12, status: "success", response: { type: "status", current_frame: 11 } };
  const result = bridgeResponse(event, { origin, source, handle: (json) => { assert.deepEqual(JSON.parse(json), envelope); return JSON.stringify(response); } });
  assert.deepEqual(result, { namespace: "titan.inspector", type: "response", envelope: response });
});

test("bridge rejects foreign sources, origins, opaque origins, and reply loops", () => {
  const handle = () => { throw new Error("untrusted request reached runtime"); };
  const options = { origin, source, handle };
  for (const invalid of [
    { ...event, source: {} },
    { ...event, origin: "https://example.com" },
    { ...event, origin: "null" },
    { ...event, data: { ...event.data, type: "response" } },
    { ...event, data: { ...event.data, namespace: "other-app" } },
    { ...event, data: null },
    { ...event, data: { ...event.data, envelope: { request: {} } } },
  ]) assert.equal(bridgeResponse(invalid, options), null);
  assert.equal(bridgeResponse({ ...event, origin: "null" }, { ...options, origin: "null" }), null);
});

test("bridge preserves structured runtime failures", () => {
  const response = { schema_version: 1, request_id: "bridge-test", instance_id: "browser", observed_frame: 0, state_revision: 0, status: "failure", error: { code: "mutation_disabled", message: "disabled", details: {}, retryable: false } };
  assert.deepEqual(bridgeResponse(event, { origin, source, handle: () => JSON.stringify(response) }).envelope, response);
});

test("typed command input preserves strings and rejects ambiguous numeric input", () => {
  assert.equal(typedArgument("42", "u32"), 42);
  assert.equal(typedArgument("-1.5", "f32"), -1.5);
  assert.equal(typedArgument("false", "bool"), false);
  assert.equal(typedArgument("hello", "alloc::string::String"), "hello");
  assert.deepEqual(typedArgument('{"x":1}', "Custom"), { x: 1 });
  for (const value of ["", "1.5", "9007199254740993", "NaN"]) assert.throws(() => typedArgument(value, "i64"));
  assert.throws(() => typedArgument("Infinity", "f64"));
  assert.throws(() => typedArgument("maybe", "bool"));
});
