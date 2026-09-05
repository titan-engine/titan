import { bridgeResponse, typedArgument } from "./bridge.mjs";

async function initialize() {
  const $ = (id) => document.getElementById(id);
  let runtime, BrowserRuntime, currentFrame = 0, sequence = 0, selectedEntity = null, operations = new Set(), nextCursor = null;
  const text = (id, value) => { $(id).textContent = value; };
  function showError(error) {
    $("error").hidden = false;
    text("error", error.code ? `${error.code}: ${error.message}\n${JSON.stringify(error.details || {}, null, 2)}` : String(error.message || error));
  }
  function action(callback) {
    return (event) => {
      event?.preventDefault();
      $("error").hidden = true;
      try { callback(); } catch (error) { showError(error); }
    };
  }
  function request(body) {
    const envelope = { schema_version: 2, request_id: `browser-ui-${++sequence}`, request: body };
    const result = JSON.parse(runtime.handle(JSON.stringify(envelope)));
    text("last-response", JSON.stringify(result, null, 2));
    currentFrame = result.observed_frame;
    if (result.status === "failure") throw result.error;
    return result.response;
  }
  function capture() {
    const result = request({ type: "capture" });
    if (!result.artifact.startsWith("data:image/png;base64,")) throw new Error("Runtime returned an unsupported capture artifact");
    $("scene").src = result.artifact;
    $("scene").hidden = false;
    $("capture-placeholder").hidden = true;
    text("capture-info", `${result.width} × ${result.height} · ${result.format} · checksum ${result.checksum}`);
  }
  function status() {
    const result = request({ type: "status" });
    currentFrame = result.current_frame;
    text("runtime-status", `Frame ${currentFrame} · ${result.paused ? "paused" : "running"}`);
    $("input-frame").value = currentFrame + 1;
    $("status-fields").replaceChildren();
    for (const [name, value] of Object.entries(result)) {
      if (name === "type") continue;
      const term = document.createElement("dt"), detail = document.createElement("dd");
      term.textContent = name.replaceAll("_", " "); detail.textContent = String(value);
      $("status-fields").append(term, detail);
    }
  }
  function entityDetails(entity) {
    const details = request({ type: "entity", entity });
    selectedEntity = entity;
    $("entity-placeholder").hidden = true;
    text("entity-details", JSON.stringify(details, null, 2));
    $("mutation-form").hidden = !operations.has("mutate");
    $("mutation-component").replaceChildren();
    for (const component of Object.keys(details.components)) {
      const option = document.createElement("option"); option.value = component; option.textContent = component;
      $("mutation-component").append(option);
    }
    for (const button of $("entities").children) button.setAttribute("aria-pressed", String(button.dataset.entity === JSON.stringify(entity)));
  }
  function entities(append = false) {
    const result = request({ type: "entities", query: {}, page: { limit: 100, ...(append && nextCursor ? { cursor: nextCursor } : {}) } });
    if (!append) $("entities").replaceChildren();
    for (const entity of result.entities) {
      const button = document.createElement("button"), name = document.createElement("span"), components = document.createElement("small");
      name.textContent = `${entity.name || "Unnamed entity"} · ${entity.id.index}:${entity.id.generation}`;
      components.textContent = entity.components.join(", ");
      button.dataset.entity = JSON.stringify(entity.id); button.setAttribute("aria-pressed", "false");
      button.append(name, components); button.addEventListener("click", action(() => entityDetails(entity.id)));
      $("entities").append(button);
    }
    nextCursor = result.next_cursor || null;
    $("more-entities").hidden = !nextCursor;
    text("entity-count", `${$("entities").children.length}${nextCursor ? "+" : ""} entities`);
  }
  function refresh() {
    status(); entities();
    if (selectedEntity) {
      const exists = Array.from($("entities").children).some((button) => button.dataset.entity === JSON.stringify(selectedEntity));
      if (exists) entityDetails(selectedEntity);
      else { selectedEntity = null; text("entity-details", ""); $("entity-placeholder").hidden = false; $("mutation-form").hidden = true; }
    }
    if (operations.has("capture")) capture();
  }
  function commands() {
    const result = request({ type: "commands" });
    $("commands").replaceChildren();
    text("commands-note", result.commands.length ? "Typed arguments are checked by the runtime before the command executes." : "No commands advertised. Enable controls to see the game's commands.");
    for (const command of result.commands) {
      const panel = document.createElement("div"), title = document.createElement("h3"), description = document.createElement("p"), form = document.createElement("form");
      panel.className = "command"; title.textContent = command.name; description.textContent = command.description;
      const inputs = [];
      for (const [name, metadata] of Object.entries(command.arguments)) {
        const label = document.createElement("label"), caption = document.createElement("span"), help = document.createElement("small");
        const input = document.createElement(metadata.type_name === "bool" ? "select" : "input");
        caption.textContent = `${name} · ${metadata.type_name}`; help.textContent = metadata.description || "";
        if (metadata.type_name === "bool") {
          for (const value of ["false", "true"]) { const option = document.createElement("option"); option.value = value; option.textContent = value; input.append(option); }
        } else if (/^(u|i|f)\d+$/.test(metadata.type_name)) {
          input.type = "number"; input.step = metadata.type_name.startsWith("f") ? "any" : "1";
          if (metadata.minimum != null) input.min = metadata.minimum;
          if (metadata.maximum != null) input.max = metadata.maximum;
          input.value = metadata.minimum ?? 0;
        } else { input.value = metadata.type_name.includes("String") ? "" : "null"; }
        input.required = true; label.append(caption, input, help); form.append(label); inputs.push([name, input, metadata.type_name]);
      }
      const submit = document.createElement("button"); submit.textContent = "Invoke"; submit.disabled = !operations.has("invoke"); form.append(submit);
      form.addEventListener("submit", action(() => {
        const args = Object.fromEntries(inputs.map(([name, input, type]) => [name, typedArgument(input.value, type)]));
        request({ type: "invoke", name: command.name, arguments: args }); refresh();
      }));
      panel.append(title, description, form); $("commands").append(panel);
    }
  }
  function start(control) {
    runtime?.free(); runtime = new BrowserRuntime(control); selectedEntity = null;
    $("entity-placeholder").hidden = false; text("entity-details", ""); $("mutation-form").hidden = true;
    const capabilities = request({ type: "capabilities" }); operations = new Set(capabilities.operations);
    text("mode", capabilities.operations.some((operation) => ["step", "invoke", "inject_input"].includes(operation)) ? "Controls enabled" : "Read-only");
    text("session-note", control ? "Fresh controlled session. Enabling controls reset the scene to frame 0." : "Read-only session. Enabling controls starts a fresh scene at frame 0.");
    text("capabilities", capabilities.operations.join(" · "));
    $("enable-controls").textContent = control ? "Reset controlled scene" : "Enable controls";
    $("enable-controls").disabled = false; $("refresh").disabled = false;
    $("capture").disabled = !operations.has("capture");
    $("time-panel").hidden = !operations.has("step"); $("input-panel").hidden = !operations.has("inject_input");
    commands(); refresh();
  }
  $("enable-controls").addEventListener("click", action(() => start(true)));
  $("refresh").addEventListener("click", action(refresh));
  $("capture").addEventListener("click", action(capture));
  $("more-entities").addEventListener("click", action(() => entities(true)));
  $("step-form").addEventListener("submit", action(() => { request({ type: "step", frames: Number($("ticks").value) }); refresh(); }));
  $("input-form").addEventListener("submit", action(() => { request({ type: "inject_input", frame: Number($("input-frame").value), actions: JSON.parse($("input-actions").value) }); status(); }));
  $("mutation-form").addEventListener("submit", action(() => { request({ type: "set_field", entity: selectedEntity, component: $("mutation-component").value, field: $("mutation-field").value, value: JSON.parse($("mutation-value").value) }); refresh(); entityDetails(selectedEntity); }));
  try {
    const wasm = await import("./pkg/titan_game.js"); await wasm.default(); BrowserRuntime = wasm.BrowserRuntime; start(false);
    window.addEventListener("message", async (event) => {
      try {
        const response = await bridgeResponse(event, { origin: location.origin, source: window, handle: (json) => runtime.dispatch(json) });
        if (!response) return;
        window.postMessage(response, location.origin);
        if (response.envelope.status === "failure") showError(response.envelope.error);
        else $("error").hidden = true;
        if (response.envelope.status === "success" && ["step", "invoke", "set_field"].includes(event.data.envelope.request.type)) refresh();
        else status();
        text("last-response", JSON.stringify(response.envelope, null, 2));
      } catch (error) { showError(error); }
    });
  } catch (error) { text("mode", "Runtime unavailable"); showError(error); }
}

if (typeof document !== "undefined") initialize();
