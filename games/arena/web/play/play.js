import init, { BrowserPlayer, verify_recording_json } from '../inspector/pkg/titan_game.js';
import { bindPlayerInput } from '../shared/input.mjs';
import { bridgeResponse } from '../inspector/bridge.mjs';
import { inspectEntities, entityRow, inspectionDetails } from './entities.mjs';
import { bindCanvasPointer } from './pointer.mjs';
const canvas = document.querySelector('#game');
const start = document.querySelector('#start');
const pause = document.querySelector('#pause');
const replay = document.querySelector('#restart');
const status = document.querySelector('#status');
const result = document.querySelector('#result');
const errorPanel = document.querySelector('#error');
const actions = ['up', 'down', 'left', 'right', 'dash'];
const keys = new Map([['ArrowUp', 'up'], ['w', 'up'], ['W', 'up'], ['ArrowDown', 'down'], ['s', 'down'], ['S', 'down'], ['ArrowLeft', 'left'], ['a', 'left'], ['A', 'left'], ['ArrowRight', 'right'], ['d', 'right'], ['D', 'right'], [' ', 'dash']]);
let player;
let epoch;
let requestId = 0;
let lastTime;
let animation;
const input = bindPlayerInput({
  canvas, buttons: document.querySelectorAll('[data-action]'), keys, actions,
  isRunning: () => Boolean(player && !player.paused()),
  setAction: (action, pressed) => player?.set_action(action, pressed),
  cancelAction: action => player?.cancel_action(action),
  clearInput: () => player?.clear_input(),
  onHidden: () => { lastTime = undefined; },
  onKey: event => {
    if (!player || event.key.toLowerCase() !== 'r') return false;
    event.preventDefault(); replay.click(); return true;
  },
});
const canvasPointer = bindCanvasPointer({
  canvas,
  enabled: () => Boolean(player),
  pointer: (x, y, pressed) => {
    try { return player.pointer(x, y, pressed); }
    catch (error) { failure(error); return false; }
  },
  cancelPointer: () => player?.cancel_pointer(),
  afterPointer: () => {
    if (!player) return;
    try { syncSession(); player.frame(0); updateStatus(); }
    catch (error) { failure(error); }
  },
});
function failure(error) {
  cancelAnimationFrame(animation); lastTime = undefined;
  errorPanel.hidden = false; errorPanel.textContent = `GPU player stopped: ${error.message ?? error}\nRetry starts a fresh scene.`;
  pause.disabled = true; replay.disabled = true;
  document.querySelectorAll('[data-action], [data-live], #step').forEach(button => button.disabled = true);
  input.cancel(); canvasPointer.cancel(); player?.free(); player = undefined;
  start.disabled = false; start.textContent = 'Retry';
}
function updateStatus() { const {run} = JSON.parse(player.status()); status.textContent = `Health ${run.health}/3 · ${(run.elapsed/60).toFixed(1)} / 20 s · ${run.dash_ready ? 'Dash ready' : `Dash ${(Math.ceil(run.dash_cooldown/6)/10).toFixed(1)} s`}`; result.textContent = run.outcome === 'Won' ? 'You survived! Restart for another run.' : run.outcome === 'Lost' ? 'Caught! Restart and keep moving.' : 'Stay clear of the pursuers.'; }
function resize() { if (!player) return; canvasPointer.cancel(); const scale = window.devicePixelRatio || 1; player.resize(Math.max(1, Math.round(canvas.clientWidth * scale)), Math.max(1, Math.round(canvas.clientHeight * scale))); if (player.paused()) { player.frame(0); updateStatus(); } }
function syncSession() {
  if (!player) return;
  const next = player.clock_epoch();
  if (epoch !== next) {
    epoch = next;
    lastTime = undefined;
    input.cancel();
    canvasPointer.cancel();
  }
  pause.textContent = player.paused() ? 'Resume' : 'Pause';
  document.querySelector('#enable-controls').checked = player.control_enabled();
  document.querySelector('#step').disabled = !player.control_enabled() || !player.paused();
  document.querySelector('#live-mode').textContent = `${player.paused() ? 'Paused' : 'Playing'} · Inspection ${player.control_enabled() ? 'controls enabled' : 'read-only'}`;
}
function loop(time) {
  try {
    syncSession();
    if (player && !player.paused()) player.frame(lastTime === undefined ? 0 : Math.min(250, time - lastTime));
    if (player) updateStatus();
    lastTime = time;
    animation = requestAnimationFrame(loop);
  } catch (error) { failure(error); }
}
start.addEventListener('click', async () => {
  start.disabled = true; errorPanel.hidden = true;
  try {
    await init();
    player = await BrowserPlayer.create(canvas);
    player.resume();
    syncSession(); resize();
    pause.disabled = false; replay.disabled = false;
    document.querySelectorAll('[data-action], [data-live]').forEach(button => button.disabled = false);
    syncSession(); start.textContent = 'Playing'; canvas.focus();
    animation = requestAnimationFrame(loop);
  } catch (error) { failure(error); }
});
pause.addEventListener('click', () => {
  if (player.paused()) player.resume(); else player.pause();
  syncSession();
  if (!player.paused()) canvas.focus();
});
replay.addEventListener('click', () => {
  try { player.restart(); syncSession(); player.frame(0); updateStatus(); }
  catch (error) { failure(error); }
});
function handle(requestJson) {
  const response = player.handle(requestJson);
  syncSession(); player.frame(0); updateStatus();
  return response;
}
function request(request) {
  const envelope = JSON.parse(handle(JSON.stringify({ schema_version: 1, request_id: `live-${++requestId}`, request })));
  if (envelope.status === 'failure') throw new Error(envelope.error.message);
  return envelope;
}
const liveOutput = document.querySelector('#live-output');
const liveSummary = document.querySelector('#live-summary');
function showDetails(value) {
  const json = JSON.stringify(value, null, 2);
  liveOutput.textContent = json.length > 12000 ? `${json.slice(0, 12000)}\n…details truncated` : json;
}
function summarize(state) {
  const dash = state.run.dash_ready ? 'ready' : `${(Math.ceil(state.run.dash_cooldown / 6) / 10).toFixed(1)} s cooldown`;
  liveSummary.textContent = `${state.paused ? 'Paused' : 'Playing'} at frame ${state.frame} · Player (${state.position.x}, ${state.position.y}) · Health ${state.run.health}/3 · Dash ${dash} · ${state.recording.recorded_ticks}/${state.recording.max_ticks} ticks recorded${state.recording.truncated ? ' (truncated)' : ''}${state.recording.invalid_reason ? ` · Replay unavailable: ${state.recording.invalid_reason}` : ''}`;
}
function panel(action) {
  try { action(); }
  catch (error) { liveSummary.textContent = error.message ?? String(error); }
}
document.querySelector('#inspect').addEventListener('click', () => panel(() => {
  const state = request({ type: 'query', name: 'arena_state', arguments: {} });
  summarize(state.response.value);
  const snapshot = inspectEntities(request);
  const body = document.querySelector('#live-entity-body');
  const rows = snapshot.entities.map(entity => {
    const row = document.createElement('tr');
    for (const value of entityRow(entity)) {
      const cell = document.createElement('td'); cell.textContent = value; row.append(cell);
    }
    return row;
  });
  body.replaceChildren(...rows);
  document.querySelector('#live-entity-caption').textContent = `${snapshot.entities.length} entities at frame ${snapshot.observed_frame}${snapshot.truncated ? ' · display limited to 1,000 entities' : ''}`;
  document.querySelector('#live-entities').hidden = false;
  showDetails(inspectionDetails(state, snapshot));
}));
document.querySelector('#capture').addEventListener('click', () => panel(() => {
  const response = request({ type: 'capture' });
  const image = document.querySelector('#live-capture');
  image.src = response.response.artifact; image.hidden = false;
  liveSummary.textContent = `Capture of frame ${response.observed_frame} · checksum ${response.response.checksum}`;
  image.alt = `Live arena capture at frame ${response.observed_frame}`;
  showDetails({ ...response, response: { ...response.response, artifact: '(shown below)' } });
}));
document.querySelector('#recording').addEventListener('click', () => panel(() => {
  const response = request({ type: 'query', name: 'recording', arguments: {} });
  const recording = response.response.value;
  let verification;
  try { verification = JSON.parse(verify_recording_json(JSON.stringify(recording))); }
  catch (error) { verification = { verified: false, reason: error.message ?? String(error) }; }
  const blob = new Blob([JSON.stringify(recording, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a'); link.href = url; link.download = 'arena-recording.json'; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  liveSummary.textContent = `Exported ${recording.recorded_ticks}/${recording.max_ticks} ticks · ${verification.verified ? `Headless replay verified: state and image match (${verification.checksum})` : `Replay unavailable: ${verification.reason}`}`;
  const { frames, ...header } = recording;
  showDetails({ recording: header, verification });
}));
document.querySelector('#enable-controls').addEventListener('change', event => panel(() => {
  player.set_control_enabled(event.target.checked); syncSession();
}));
document.querySelector('#step').addEventListener('click', () => panel(() => {
  const response = request({ type: 'step', frames: 1 });
  summarize(request({ type: 'query', name: 'arena_state' }).response.value);
  showDetails(response);
}));
window.addEventListener('message', event => {
  if (!player) return;
  const response = bridgeResponse(event, { origin: location.origin, source: window, handle });
  if (response) window.postMessage(response, location.origin);
});
new ResizeObserver(() => { try { resize(); } catch (error) { failure(error); } }).observe(canvas);
window.addEventListener('pagehide', () => { cancelAnimationFrame(animation); input.cancel(); canvasPointer.cancel(); player?.free(); player = undefined; });

// Deliberate local integration hook. No background stepping; each request is
// bounded, and normal pages do not expose access to the player.
if (new URLSearchParams(location.search).get('test') === '1') {
  window.titanPlayerTest = Object.freeze({
    status: () => player ? JSON.parse(player.status()) : null,
    request: envelope => JSON.parse(handle(JSON.stringify(envelope))),
    step: (ticks, action = null) => {
      if (!player) throw new Error('Start the player first');
      if (!Number.isInteger(ticks) || ticks < 0 || ticks > 600) throw new Error('ticks must be an integer from 0 to 600');
      if (action !== null && !actions.includes(action)) throw new Error('Unknown action');
      player.pause(); syncSession();
      try {
        player.resume(); syncSession();
        if (action) player.set_action(action, true);
        for (let tick = 0; tick < ticks; tick++) player.frame(1000 / 60);
        player.frame(0); updateStatus();
        return JSON.parse(player.status());
      } finally { player.pause(); syncSession(); }
    },
  });
}
