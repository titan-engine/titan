import test from "node:test";
import assert from "node:assert/strict";
import { bridgeResponse, typedArgument } from "./bridge.mjs";

const source = {};
const origin = "http://127.0.0.1:8080";
const envelope = { schema_version: 2, request_id: "bridge-test", request: { type: "status" } };
const event = { source, origin, data: { namespace: "titan.inspector", type: "request", envelope } };

test("same-window bridge preserves request and response envelopes", async () => {
  const response = { schema_version: 2, request_id: "bridge-test", instance_id: "browser", observed_frame: 11, state_revision: 12, status: "success", response: { type: "status", current_frame: 11 } };
  const result = await bridgeResponse(event, { origin, source, handle: (json) => { assert.deepEqual(JSON.parse(json), envelope); return JSON.stringify(response); } });
  assert.deepEqual(result, { namespace: "titan.inspector", type: "response", envelope: response });
});

test("bridge rejects foreign sources, origins, opaque origins, and reply loops", async () => {
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
  ]) assert.equal(await bridgeResponse(invalid, options), null);
  assert.equal(await bridgeResponse({ ...event, origin: "null" }, { ...options, origin: "null" }), null);
});

test("bridge preserves structured runtime failures", async () => {
  const response = { schema_version: 2, request_id: "bridge-test", instance_id: "browser", observed_frame: 0, state_revision: 0, status: "failure", error: { code: "mutation_disabled", message: "disabled", details: {}, retryable: false } };
  assert.deepEqual((await bridgeResponse(event, { origin, source, handle: () => JSON.stringify(response) })).envelope, response);
});

test("typed command input preserves strings and rejects ambiguous numeric input", async () => {
  assert.equal(typedArgument("42", "u32"), 42);
  assert.equal(typedArgument("-1.5", "f32"), -1.5);
  assert.equal(typedArgument("false", "bool"), false);
  assert.equal(typedArgument("hello", "alloc::string::String"), "hello");
  assert.deepEqual(typedArgument('{"x":1}', "Custom"), { x: 1 });
  for (const value of ["", "1.5", "9007199254740993", "NaN"]) assert.throws(() => typedArgument(value, "i64"));
  assert.throws(() => typedArgument("Infinity", "f64"));
  assert.throws(() => typedArgument("maybe", "bool"));
});


test("pending responses correlate out of order without animation frames", async () => {
  let finish;
  let calls = 0;
  const capture = { ...envelope, request_id: "capture", request: { type: "capture" } };
  const delayed = bridgeResponse({ ...event, data: { ...event.data, envelope: capture } }, {
    origin, source, handle: json => {
      calls++;
      const request = JSON.parse(json);
      return new Promise(resolve => { finish = () => resolve(JSON.stringify({ request_id: request.request_id, observed_frame: 7, state_revision: 2, status: "success" })); });
    },
  });
  const status = await bridgeResponse(event, { origin, source, handle: async () => JSON.stringify({ request_id: "bridge-test", observed_frame: 8 }) });
  assert.equal(status.envelope.observed_frame, 8);
  finish();
  const result = await delayed;
  assert.equal(result.envelope.request_id, "capture");
  assert.equal(result.envelope.observed_frame, 7);
  assert.equal(calls, 1);
});
