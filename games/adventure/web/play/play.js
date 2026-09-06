import init, { BrowserPlayer } from '../inspector/pkg/titan_game.js';
import { bindKeys } from './keys.mjs';
const canvas = document.querySelector('canvas');
const byId = id => document.getElementById(id);
let player, previous, failed = false;
const backend = new URL(location.href).searchParams.get('backend') ?? 'auto';
const report = error => { failed = true; player?.pause(); byId('error').textContent = `Graphics/player error: ${error}. Check GPU support and reload to start a fresh session.`; };
const run = fn => { try { fn(); byId('error').textContent = ''; } catch (error) { byId('error').textContent = String(error); } };
const graphics = fn => { try { fn(); } catch (error) { report(error); } };
const show = () => { const status = player.status(); byId('status').textContent = status; byId('pause').textContent = player.paused() ? 'Resume' : 'Pause'; };
const keys = bindKeys({canvas, key: (...args) => player?.set_key(...args), clear: () => player?.clear_input(), pause: () => { player?.pause(); previous = undefined; }, shortcut: code => {
  if (!player) return false;
  if (code === 'KeyP') { toggle(); return true; }
  if (code === 'KeyN') { run(() => player.step()); return true; }
  if (code === 'Escape') { keys.cancel(); player.pause(); canvas.blur(); return true; }
  if (code === 'KeyR' && player.paused()) { run(() => player.restart()); return true; }
  return false;
}});
function toggle() { run(() => { keys.cancel(); player.paused() ? player.resume() : player.pause(); previous = undefined; canvas.focus(); show(); }); }
function resize() { if (!player) return; const rect = canvas.getBoundingClientRect(); graphics(() => player.resize(Math.round(rect.width * devicePixelRatio), Math.round(rect.height * devicePixelRatio))); }
new ResizeObserver(resize).observe(canvas);
window.addEventListener('blur', () => { player?.pause(); previous = undefined; });
document.addEventListener('visibilitychange', () => { if(document.hidden) { player?.pause(); previous = undefined; } });
byId('play').onclick = async () => {
  byId('play').disabled = true;
  try {
    await init();
    let timer;
    try { player = await Promise.race([BrowserPlayer.create(canvas, backend), new Promise((_, reject) => { timer = setTimeout(() => reject(Error("GPU initialization exceeded 60 seconds")), 60000); })]); } finally { clearTimeout(timer); }
    player.set_control_enabled(byId('control').checked);
    for (const id of ['pause','step','restart','replay','export','import','capture']) byId(id).disabled = false;
    resize(); player.resume(); canvas.focus(); byId('play').hidden = true;
    // Deliberate same-page inspection boundary. Runtime enforces explicit control opt-in.
    window.adventure = { dispatch: json => player.dispatch(json), status: () => JSON.parse(player.status()) };
    function animate(now) { if (failed) return; graphics(() => { player.frame(previous === undefined ? 0 : now - previous); previous = now; show(); }); requestAnimationFrame(animate); }
    requestAnimationFrame(animate);
  } catch (error) { report(error); }
};
// Keep the current pause state until this explicit toggle; focus loss itself pauses.
byId('pause').onpointerdown = event => event.preventDefault();
byId('pause').onclick = toggle;
byId('step').onclick = () => run(() => { keys.cancel(); player.step(); show(); });
byId('restart').onclick = () => run(() => { keys.cancel(); player.restart(); previous = undefined; show(); });
byId('replay').onclick = () => run(() => { keys.cancel(); player.replay_route(); player.resume(); previous = undefined; canvas.focus(); });
byId('control').onchange = () => player?.set_control_enabled(byId('control').checked);
byId('export').onclick = () => run(() => { const url = URL.createObjectURL(new Blob([player.recording()], {type:'application/json'})); const a = document.createElement('a'); a.href=url; a.download='adventure.json'; a.click(); setTimeout(() => URL.revokeObjectURL(url), 0); });
byId('import').onchange = async event => { const file = event.target.files[0]; if (!file) return; if (file.size > 2 * 1024 * 1024) { byId('error').textContent='Recording exceeds 2 MiB.'; return; } try { player.load_recording(await file.text()); previous=undefined; show(); } catch(error) { byId('error').textContent=`Recording rejected: ${error}`; } event.target.value=''; };

let captureSequence = 0, captureDownload;
byId('capture').onclick = async () => {
  byId('capture').disabled = true;
  try {
    const response = JSON.parse(await player.dispatch(JSON.stringify({
      schema_version: 2, request_id: `capture-ui-${++captureSequence}`, request: {type: 'capture'},
    })));
    if (response.status !== 'success') throw Error(response.error.message);
    const capture = response.response;
    byId('capture-image').src = capture.artifact;
    const identity = capture.identity;
    byId('capture-identity').textContent = `Captured tick ${identity.observed_frame}, revision ${identity.state_revision}, session ${identity.session_generation}.`;
    if (captureDownload) URL.revokeObjectURL(captureDownload);
    captureDownload = URL.createObjectURL(new Blob([JSON.stringify(response, null, 2)], {type: 'application/json'}));
    byId('capture-download').href = captureDownload;
    byId('capture-download').download = 'adventure-capture.json';
    byId('capture-result').hidden = false;
    byId('error').textContent = '';
  } catch (error) { byId('error').textContent = `Capture failed: ${error}`; }
  finally { byId('capture').disabled = false; }
};

// One primary gesture; cancellation cannot revive an old press after a reset.
let pointerId;
const pointerSample = event => {
  const rect = canvas.getBoundingClientRect();
  const scale = Math.min(rect.width / 320, rect.height / 180);
  return [(event.clientX - rect.left - (rect.width - 320 * scale) / 2) / scale,
    (event.clientY - rect.top - (rect.height - 180 * scale) / 2) / scale];
};
canvas.addEventListener('pointerdown', event => {
  if (!player || event.button !== 0 || !event.isPrimary) return;
  event.preventDefault(); canvas.focus(); pointerId = event.pointerId;
  canvas.setPointerCapture(pointerId); player.pointer(...pointerSample(event), true);
});
canvas.addEventListener('pointerup', event => {
  if (event.pointerId !== pointerId) return;
  pointerId = undefined;
  player.pointer(...pointerSample(event), false);
  if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
});
const cancelPointer = () => { pointerId = undefined; player?.cancel_pointer(); };
canvas.addEventListener('pointercancel', cancelPointer);
canvas.addEventListener('lostpointercapture', cancelPointer);
window.addEventListener('blur', cancelPointer);
