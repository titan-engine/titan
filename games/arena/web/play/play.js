import init, { BrowserPlayer } from '../inspector/pkg/titan_game.js';
const canvas = document.querySelector('#game');
const start = document.querySelector('#start');
const pause = document.querySelector('#pause');
const replay = document.querySelector('#restart');
const status = document.querySelector('#status');
const result = document.querySelector('#result');
const errorPanel = document.querySelector('#error');
const actions = ['up', 'down', 'left', 'right'];
const keys = new Map([['ArrowUp', 'up'], ['w', 'up'], ['W', 'up'], ['ArrowDown', 'down'], ['s', 'down'], ['S', 'down'], ['ArrowLeft', 'left'], ['a', 'left'], ['A', 'left'], ['ArrowRight', 'right'], ['d', 'right'], ['D', 'right']]);
let player;
let running = false;
let lastTime;
let animation;
const heldKeys = new Map();
const heldPointers = new Map();
function failure(error) {
  running = false; cancelAnimationFrame(animation); lastTime = undefined;
  errorPanel.hidden = false; errorPanel.textContent = `GPU player stopped: ${error.message ?? error}\nRetry starts a fresh scene.`;
  pause.disabled = true; replay.disabled = true;
  document.querySelectorAll('[data-action]').forEach(button => button.disabled = true);
  heldKeys.clear(); heldPointers.clear(); player?.free(); player = undefined;
  start.disabled = false; start.textContent = 'Retry';
}
function updateStatus() { const {run} = JSON.parse(player.status()); status.textContent = `Health ${run.health}/3 · ${(run.elapsed/60).toFixed(1)} / 20 s`; result.textContent = run.outcome === 'Won' ? 'You survived! Restart for another run.' : run.outcome === 'Lost' ? 'Caught! Restart and keep moving.' : 'Stay clear of the pursuers.'; }
function resize() { if (!player) return; const scale = window.devicePixelRatio || 1; player.resize(Math.max(1, Math.round(canvas.clientWidth * scale)), Math.max(1, Math.round(canvas.clientHeight * scale))); if (!running) { player.frame(0); updateStatus(); } }
function syncInputs() { if (player) for (const action of actions) player.set_action(action, [...heldKeys.values(), ...heldPointers.values()].includes(action)); }
function releaseInputs() { heldKeys.clear(); heldPointers.clear(); syncInputs(); }
function loop(time) { try { if (running) { player.frame(lastTime === undefined ? 0 : Math.min(250, time - lastTime)); updateStatus(); } lastTime = time; animation = requestAnimationFrame(loop); } catch (error) { failure(error); } }
start.addEventListener('click', async () => {
  start.disabled = true; errorPanel.hidden = true;
  try { await init(); player = await BrowserPlayer.create(canvas); resize(); lastTime = undefined; running = true; pause.textContent = 'Pause'; pause.disabled = false; replay.disabled = false; document.querySelectorAll('[data-action]').forEach(button => button.disabled = false); start.textContent = 'Playing'; canvas.focus(); animation = requestAnimationFrame(loop); }
  catch (error) { failure(error); start.disabled = false; }
});
pause.addEventListener('click', () => { running = !running; lastTime = undefined; releaseInputs(); pause.textContent = running ? 'Pause' : 'Resume'; if (running) canvas.focus(); });
replay.addEventListener('click', () => { try { running = false; releaseInputs(); player.restart(); player.frame(0); updateStatus(); pause.textContent = 'Resume'; } catch (error) { failure(error); } });
window.addEventListener('keydown', event => { if (player && event.key.toLowerCase() === 'r' && !event.target.closest('input, textarea, select, button, [contenteditable]')) { event.preventDefault(); replay.click(); return; } const action = keys.get(event.key); if (!player || !running || !action || event.target.closest('input, textarea, select, button, [contenteditable]')) return; event.preventDefault(); heldKeys.set(event.code, action); syncInputs(); });
window.addEventListener('keyup', event => { if (heldKeys.delete(event.code)) { event.preventDefault(); syncInputs(); } });
window.addEventListener('blur', releaseInputs);
document.addEventListener('focusin', event => { if (event.target !== canvas) releaseInputs(); });
document.addEventListener('visibilitychange', () => { if (document.hidden) { releaseInputs(); lastTime = undefined; } });
for (const button of document.querySelectorAll('[data-action]')) {
  button.addEventListener('pointerdown', event => { if (!running) return; button.setPointerCapture(event.pointerId); heldPointers.set(event.pointerId, button.dataset.action); syncInputs(); });
  for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) button.addEventListener(type, event => { heldPointers.delete(event.pointerId); syncInputs(); });
}
new ResizeObserver(() => { try { resize(); } catch (error) { failure(error); } }).observe(canvas);
window.addEventListener('pagehide', () => { cancelAnimationFrame(animation); player?.free(); player = undefined; });

// Deliberate local integration hook. No background stepping; each request is
// bounded, and normal pages do not expose access to the player.
if (new URLSearchParams(location.search).get('test') === '1') {
  window.titanPlayerTest = Object.freeze({
    status: () => player ? JSON.parse(player.status()) : null,
    step: (ticks, action = null) => {
      if (!player) throw new Error('Start the player first');
      if (!Number.isInteger(ticks) || ticks < 0 || ticks > 600) throw new Error('ticks must be an integer from 0 to 600');
      if (action !== null && !actions.includes(action)) throw new Error('Unknown action');
      running = false; lastTime = undefined; releaseInputs(); pause.textContent = 'Resume';
      try {
        if (action) player.set_action(action, true);
        for (let tick = 0; tick < ticks; tick++) player.frame(1000 / 60);
        player.frame(0); updateStatus();
        return JSON.parse(player.status());
      } finally { releaseInputs(); }
    },
  });
}
