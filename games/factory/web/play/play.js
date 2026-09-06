import init, { BrowserPlayer } from '../inspector/pkg/titan_game.js';
import { logicalPointer } from './pointer.mjs';
import { bindPlayerInput } from '../shared/input.mjs';
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
let mode = 'place';
let pointerPosition;
const constructionButtons = document.querySelectorAll('[data-kind], [data-mode], #facing, #zoom-in, #zoom-out');
const input = bindPlayerInput({
  canvas, buttons: document.querySelectorAll('[data-action]'), keys, actions,
  isRunning: () => Boolean(player && running),
  setAction: (action, pressed) => player?.set_action(action, pressed),
  cancelAction: action => player?.cancel_action(action),
  clearInput: () => player?.clear_input(),
  onHidden: () => { lastTime = undefined; },
});
function failure(error) {
  running = false; cancelAnimationFrame(animation); lastTime = undefined;
  errorPanel.hidden = false; errorPanel.textContent = `GPU player stopped: ${error.message ?? error}\nRetry starts a fresh scene.`;
  pause.disabled = true; replay.disabled = true;
  document.querySelectorAll('[data-action]').forEach(button => button.disabled = true);
  constructionButtons.forEach(button => button.disabled = true);
  input.cancel(); player?.free(); player = undefined;
  start.disabled = false; start.textContent = 'Retry';
}
function updateStatus() {
  const state = JSON.parse(player.status());
  status.textContent = `${state.selection.kind} ${state.selection.facing} · ${mode} · delivered ${state.delivered}/10`;
  document.querySelector('#facing').textContent = `Q Facing ${state.selection.facing}`;
  document.querySelectorAll('[data-kind]').forEach(button => button.setAttribute('aria-pressed', String(mode === 'place' && button.dataset.kind === state.selection.kind)));
  document.querySelectorAll('[data-mode]').forEach(button => button.setAttribute('aria-pressed', String(mode === button.dataset.mode)));
}
function edit(operation) {
  if (!player) return;
  try { result.textContent = operation(); player.frame(0); updateStatus(); }
  catch (error) { result.textContent = String(error.message ?? error); }
}
function command(value) { return player.command(JSON.stringify(value)); }
function nextFacing() {
  const facings = ['N', 'E', 'S', 'W'];
  const current = JSON.parse(player.status()).selection.facing;
  return command({op:'select', facing:facings[(facings.indexOf(current) + 1) % 4]});
}
// CSS coordinates normalize directly to the logical framebuffer; DPR only affects
// backing resolution. This remains correct after nonuniform CSS resizing.
function pointer(event, action) {
  const p = logicalPointer(event.clientX, event.clientY, canvas.getBoundingClientRect());
  pointerPosition = p;
  return player.pointer(p.x, p.y, action);
}
canvas.addEventListener('pointermove', event => {
  if (player) { try { pointer(event, 'hover'); if (!running) player.frame(0); } catch {} }
});
canvas.addEventListener('pointerleave', () => { pointerPosition = undefined; if (player) { player.pointer(-1,-1,'hover'); if (!running) player.frame(0); } });
canvas.addEventListener('pointerdown', event => {
  if (!player || ![0,2].includes(event.button)) return;
  event.preventDefault(); canvas.focus(); edit(() => pointer(event, event.button === 2 ? 'inspect' : mode));
});
canvas.addEventListener('contextmenu', event => event.preventDefault());
canvas.addEventListener('wheel', event => {
  if (!player) return;
  event.preventDefault();
  const delta = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? canvas.clientHeight : 1);
  edit(() => { player.camera(0,0,Math.exp(-Math.max(-500,Math.min(500,delta)) * .001)); return 'Camera zoom updated'; });
}, {passive:false});
document.querySelectorAll('[data-kind]').forEach(button => button.addEventListener('click', () => edit(() => { mode = 'place'; return command({op:'select',kind:button.dataset.kind}); })));
document.querySelectorAll('[data-mode]').forEach(button => button.addEventListener('click', () => { mode = button.dataset.mode; updateStatus(); }));
document.querySelector('#facing').addEventListener('click', () => edit(nextFacing));
for (const [id,zoom] of [['zoom-in',1.2],['zoom-out',1/1.2]]) document.querySelector(`#${id}`).addEventListener('click', () => edit(() => { player.camera(0,0,zoom); return 'Camera zoom updated'; }));
canvas.addEventListener('keydown', event => {
  if (!player || event.repeat) return;
  const key = event.key.toLowerCase();
  if (['1','2','3'].includes(key)) { event.preventDefault(); edit(() => { mode='place'; return command({op:'select',kind:['conveyor','extractor','processor'][Number(key)-1]}); }); }
  if (key === 'q') { event.preventDefault(); edit(nextFacing); }
  if ((key === 'e' || key === 'x') && pointerPosition) { event.preventDefault(); edit(() => player.pointer(pointerPosition.x,pointerPosition.y,key === 'e' ? 'rotate' : 'remove')); }
  if (key === 'r') { event.preventDefault(); replay.click(); }
});
function resize() { if (!player) return; const scale = window.devicePixelRatio || 1; player.resize(Math.max(1, Math.round(canvas.clientWidth * scale)), Math.max(1, Math.round(canvas.clientHeight * scale))); if (!running) { player.frame(0); updateStatus(); } }
function loop(time) { try { if (running) { player.frame(lastTime === undefined ? 0 : Math.min(250, time - lastTime)); updateStatus(); } lastTime = time; animation = requestAnimationFrame(loop); } catch (error) { failure(error); } }
start.addEventListener('click', async () => {
  start.disabled = true; errorPanel.hidden = true;
  try { await init(); player = await BrowserPlayer.create(canvas); resize(); lastTime = undefined; running = true; pause.textContent = 'Pause'; pause.disabled = false; replay.disabled = false; document.querySelectorAll('[data-action]').forEach(button => button.disabled = false); start.textContent = 'Playing'; constructionButtons.forEach(button => button.disabled = false); canvas.focus(); animation = requestAnimationFrame(loop); }
  catch (error) { failure(error); start.disabled = false; }
});
pause.addEventListener('click', () => { running = !running; lastTime = undefined; input.cancel(); pause.textContent = running ? 'Pause' : 'Resume'; if (running) canvas.focus(); });
replay.addEventListener('click', () => { try { running = false; input.cancel(); player.restart(); mode = "place"; pointerPosition = undefined; lastTime = undefined; player.frame(0); updateStatus(); pause.textContent = 'Resume'; } catch (error) { failure(error); } });
new ResizeObserver(() => { try { resize(); } catch (error) { failure(error); } }).observe(canvas);
window.addEventListener('pagehide', () => { cancelAnimationFrame(animation); input.cancel(); player?.free(); player = undefined; });

// Deliberate local integration hook. No background stepping; each request is
// bounded, and normal pages do not expose access to the player.
if (new URLSearchParams(location.search).get('test') === '1') {
  window.titanPlayerTest = Object.freeze({
    status: () => player ? JSON.parse(player.status()) : null,
    command: value => { const response = command(value); player.frame(0); updateStatus(); return JSON.parse(response); },
    camera: (dx,dy,zoom) => { player.camera(dx,dy,zoom); player.frame(0); updateStatus(); },
    pointer: (x,y,action) => { const response = player.pointer(x,y,action); player.frame(0); updateStatus(); return JSON.parse(response); },
    resize,

    step: (ticks, action = null) => {
      if (!player) throw new Error('Start the player first');
      if (!Number.isInteger(ticks) || ticks < 0 || ticks > 600) throw new Error('ticks must be an integer from 0 to 600');
      if (action !== null && !actions.includes(action)) throw new Error('Unknown action');
      running = false; lastTime = undefined; input.cancel(); pause.textContent = 'Resume';
      try {
        if (action) player.set_action(action, true);
        for (let tick = 0; tick < ticks; tick++) player.frame(1000 / 60);
        player.frame(0); updateStatus();
        return JSON.parse(player.status());
      } finally { input.cancel(); }
    },
  });
}
