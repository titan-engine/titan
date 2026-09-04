/** Same-window bridge. Returns only the runtime's original response envelope. */
export function bridgeResponse(event, { origin, source, handle }) {
  if (origin === "null" || event.source !== source || event.origin !== origin) return null;
  const data = event.data;
  if (!data || data.namespace !== "titan.inspector" || data.type !== "request") return null;
  if (!data.envelope || typeof data.envelope.request_id !== "string" || !data.envelope.request) return null;
  return { namespace: "titan.inspector", type: "response", envelope: JSON.parse(handle(JSON.stringify(data.envelope))) };
}

export function typedArgument(value, typeName) {
  if (/^(u|i)(8|16|32|64|128|size)$/.test(typeName)) {
    const number = Number(value);
    if (!value.trim() || !Number.isSafeInteger(number)) throw new Error(`${typeName} requires a safe integer`);
    return number;
  }
  if (/^f(32|64)$/.test(typeName)) {
    const number = Number(value);
    if (!value.trim() || !Number.isFinite(number)) throw new Error(`${typeName} requires a finite number`);
    return number;
  }
  if (typeName === "bool") {
    if (!["true", "false"].includes(value)) throw new Error("bool requires true or false");
    return value === "true";
  }
  if (typeName === "String" || typeName === "string" || typeName.endsWith("::String")) return value;
  return JSON.parse(value);
}
