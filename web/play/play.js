import init, { BrowserPlayer, verify_recording_json_with_pngs } from '../inspector/pkg/titan_browser.js';
import { bindPlayerInput } from '../shared/input.mjs';
import { bridgeResponse } from '../inspector/bridge.mjs';
import { readRecordingForSession } from './replay.mjs';
import { bindJournalInput } from './journal.mjs';
import { loadRpgPngs } from '../shared/player-asset.mjs';
const canvas = document.querySelector('#game');
const start = document.querySelector('#start');
const pause = document.querySelector('#pause');
const replay = document.querySelector('#replay');
const status = document.querySelector('#status');
const result = document.querySelector('#result');
const errorPanel = document.querySelector('#error');
const loadRecording = document.querySelector('#load-recording');
const exportRecording = document.querySelector('#recording');
const step = document.querySelector('#step');
const restartPlayback = document.querySelector('#restart-playback');
const exitPlayback = document.querySelector('#exit-playback');
const playbackStatus = document.querySelector('#playback-status');
const inspect = document.querySelector('#inspect');
const controls = document.querySelector('#enable-controls');
const output = document.querySelector('#live-output');
const recordingResult = document.querySelector('#recording-result');
const actions = ['up', 'down', 'left', 'right'];
const keys = new Map([['ArrowUp', 'up'], ['w', 'up'], ['W', 'up'], ['ArrowDown', 'down'], ['s', 'down'], ['S', 'down'], ['ArrowLeft', 'left'], ['a', 'left'], ['A', 'left'], ['ArrowRight', 'right'], ['d', 'right'], ['D', 'right']]);
let player;
let pngs;
let lastTime;
let animation;
let loading = false;
let requestId = 0;
let epoch;
const journal = bindJournalInput({ canvas, player: () => player, changed: () => { if (player) refresh(); } });
const input = bindPlayerInput({
  canvas, buttons: document.querySelectorAll('[data-action]'), keys, actions,
  isRunning: () => Boolean(player && !player.paused() && !player.playback_active() && !player.journal_open()),
  setAction: (action, pressed) => player?.set_action(action, pressed),
  cancelAction: action => player?.cancel_action(action),
  clearInput: () => player?.clear_input(),
  onKey: event => journal.onKey(event),
  onHidden: () => { lastTime = undefined; },
});
function failure(error) {
  cancelAnimationFrame(animation); lastTime = undefined;
  errorPanel.hidden = false;
  errorPanel.textContent = `GPU player stopped: ${error.message ?? error}\nRetry starts a fresh scene.`;
  for (const button of [pause, replay, loadRecording, exportRecording, step, restartPlayback, exitPlayback, inspect]) button.disabled = true;
  document.querySelectorAll('[data-action]').forEach(button => button.disabled = true);
  input.cancel(); player?.free(); player = undefined; pngs = undefined;
  start.disabled = false; start.textContent = 'Retry';
}
function updateStatus() {
  if (epoch !== player.clock_epoch()) {
    input.cancel(); journal.cancelHeld(); lastTime = undefined; epoch = player.clock_epoch();
    output.textContent = ''; recordingResult.textContent = '';
  }
  const state = JSON.parse(player.status());
  const playback = JSON.parse(player.playback_status());
  status.textContent = `Frame ${state.frame} · ${state.collected_shards} / 3 shards`;
  result.textContent = state.shrine_active ? 'The shrine is active. All three shards collected.' : '';
  pause.textContent = player.paused() ? 'Resume' : 'Pause';
  pause.disabled = player.journal_open() || Boolean(playback.complete);
  replay.disabled = player.journal_open() || playback.active;
  loadRecording.disabled = player.journal_open() || !player.paused() || loading;
  step.disabled = player.journal_open() || !playback.active || !player.paused() || playback.complete;
  restartPlayback.disabled = exitPlayback.disabled = player.journal_open() || !playback.active;
  document.querySelectorAll('[data-action]').forEach(button => button.disabled = player.paused() || playback.active);
  playbackStatus.textContent = playback.active
    ? `Playback ${playback.position}/${playback.total} · ${playback.complete ? (playback.verified ? 'Complete · MATCH' : `Complete · MISMATCH: ${playback.error}`) : (player.paused() ? 'Paused' : 'Playing')}`
    : 'Live game';
}
function refresh() { player.frame(0); updateStatus(); }
function resize() {
  if (!player) return;
  input.cancel(); journal.cancel();
  const scale = window.devicePixelRatio || 1;
  player.resize(Math.max(1, Math.round(canvas.clientWidth * scale)), Math.max(1, Math.round(canvas.clientHeight * scale)));
  refresh();
}
function loop(time) {
  try {
    if (player) { player.frame(lastTime === undefined ? 0 : Math.min(250, time - lastTime)); updateStatus(); }
    lastTime = time; animation = requestAnimationFrame(loop);
  } catch (error) { failure(error); }
}
function query(name) {
  const response = JSON.parse(player.handle(JSON.stringify({ schema_version: 2, request_id: `page-${++requestId}`, request: { type: 'query', name, arguments: {} } })));
  if (response.status === 'failure') throw new Error(response.error.message);
  return response.response.value;
}
function local(action) { try { action(); refresh(); } catch (error) { recordingResult.textContent = error.message ?? String(error); } }
start.addEventListener('click', async () => {
  start.disabled = true; errorPanel.hidden = true;
  try {
    start.textContent = 'Loading sprites…';
    await init();
    pngs = await loadRpgPngs();
    player = await BrowserPlayer.create_with_pngs(canvas, pngs.player, pngs.tree); player.resume();
    resize(); lastTime = undefined; pause.disabled = false; replay.disabled = false;
    exportRecording.disabled = inspect.disabled = false; controls.checked = false;
    start.textContent = 'Playing'; canvas.focus(); animation = requestAnimationFrame(loop);
  } catch (error) { failure(error); }
});
pause.addEventListener('click', () => local(() => {
  input.cancel(); lastTime = undefined;
  if (player.paused()) { player.resume(); canvas.focus(); } else player.pause();
}));
replay.addEventListener('click', () => local(() => { input.cancel(); player.replay_reference(); }));
step.addEventListener('click', () => local(() => player.step_playback()));
restartPlayback.addEventListener('click', () => local(() => { player.pause(); player.restart_playback(); }));
exitPlayback.addEventListener('click', () => local(() => { player.pause(); player.exit_playback(); }));
controls.addEventListener('change', () => { player?.set_control_enabled(controls.checked); });
inspect.addEventListener('click', () => local(() => { output.textContent = JSON.stringify(query('rpg_state'), null, 2); }));
loadRecording.addEventListener('change', async () => {
  const file = loadRecording.files?.[0]; loadRecording.value = '';
  if (!file || !player) return;
  const original = player; loading = true; updateStatus();
  try {
    const text = await readRecordingForSession(file, original, () => player);
    original.load_recording(text); input.cancel(); refresh();
    recordingResult.textContent = 'Recording verified and loaded. Resume or step through the quest.';
  } catch (error) { recordingResult.textContent = `Load failed: ${error.message ?? error}`; }
  finally { loading = false; if (player) updateStatus(); }
});
exportRecording.addEventListener('click', () => local(() => {
  const recording = query('recording');
  const text = JSON.stringify(recording);
  const verification = JSON.parse(verify_recording_json_with_pngs(text, pngs.player, pngs.tree));
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const link = document.createElement('a'); link.href = url; link.download = 'rpg-recording.json'; link.click();
  URL.revokeObjectURL(url);
  recordingResult.textContent = `Exported ${verification.ticks} verified ticks. Final checksum ${verification.checksum}.`;
}));
window.addEventListener('message', async event => {
  if (!player) return;
  const response = await bridgeResponse(event, { origin: location.origin, source: window, handle: request => player.dispatch(request) });
  if (response) { window.postMessage(response, location.origin); refresh(); }
});
new ResizeObserver(() => { try { resize(); } catch (error) { failure(error); } }).observe(canvas);
window.addEventListener('pagehide', () => { cancelAnimationFrame(animation); input.cancel(); player?.free(); player = undefined; });
