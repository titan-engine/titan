import { bindKeys } from './keys.mjs';
const canvas = document.querySelector('canvas');
const byId = id => document.getElementById(id);
let player, previous, failed = false, initialResumePending = true;
const controls = ['pause','step','restart','replay','export','import','capture'];
const backend = new URL(location.href).searchParams.get('backend') ?? 'auto';
const report = error => {
  failed = true; initialResumePending = false; player?.pause(); keys.cancel();
  for (const id of controls) byId(id).disabled = true;
  byId('play').hidden = false; byId('play').disabled = false;
  byId('play').textContent = 'Retry';
  byId('status').textContent = 'Error — game stopped.';
  byId('error').textContent = `Graphics/player error: ${error}. Check GPU support. Retry starts a fresh session.`;
};
const run = fn => { try { fn(); byId('error').textContent = ''; } catch (error) { byId('error').textContent = String(error); } };
const graphics = fn => { try { fn(); } catch (error) { report(error); } };
const show = () => {
  if (!player || failed) return;
  byId('status').textContent = `${initialResumePending ? 'Ready — waiting for page focus.' : player.paused() ? 'Paused.' : 'Running.'}\n${player.status()}`;
  byId('pause').textContent = player.paused() ? 'Resume' : 'Pause';
  byId('step').disabled = !player.paused();
};
function startWhenFocused() {
  if (!player || failed || !initialResumePending || document.hidden || !document.hasFocus()) return;
  initialResumePending = false;
  player.resume(); previous = undefined; canvas.focus(); show();
}
function pauseForFocusLoss() {
  player?.pause(); previous = undefined; show();
}
function deliberateAction() { initialResumePending = false; keys.cancel(); }
const keys = bindKeys({canvas, key: (...args) => player?.set_key(...args), clear: () => player?.clear_input(), shortcut: code => {
  if (!player || failed) return false;
  if (code === 'Space') { toggle(); return true; }
  if (code === 'KeyN') { run(() => { deliberateAction(); player.step(); show(); }); return true; }
  if (code === 'KeyR') { run(() => { deliberateAction(); player.restart(); show(); }); return true; }
  return false;
}});
function toggle() { run(() => { deliberateAction(); player.paused() ? player.resume() : player.pause(); previous = undefined; show(); }); }
function resize() { if (!player) return; const rect = canvas.getBoundingClientRect(); graphics(() => player.resize(Math.round(rect.width * devicePixelRatio), Math.round(rect.height * devicePixelRatio))); }
new ResizeObserver(resize).observe(canvas);
window.addEventListener('blur', pauseForFocusLoss);
window.addEventListener('focus', () => graphics(startWhenFocused));
document.addEventListener('visibilitychange', () => { if (document.hidden) pauseForFocusLoss(); else graphics(startWhenFocused); });
// A fresh document also discards any timed-out GPU creation and its canvas.
byId('play').onclick = () => location.reload();
async function initialize() {
  byId('play').hidden = true;
  byId('status').textContent = 'Loading game — initializing WebAssembly and graphics…';
  try {
    const { default: init, BrowserPlayer } = await import('../inspector/pkg/titan_game.js');
    await init();
    let timer;
    try { player = await Promise.race([BrowserPlayer.create(canvas, backend), new Promise((_, reject) => { timer = setTimeout(() => reject(Error("GPU initialization exceeded 60 seconds")), 60000); })]); } finally { clearTimeout(timer); }
    player.set_control_enabled(byId('control').checked);
    for (const id of controls) byId(id).disabled = false;
    resize();
    if (failed) return;
    startWhenFocused(); show();
    // Deliberate same-page inspection boundary. Runtime enforces explicit control opt-in.
    window.collectionRoom = { dispatch: async json => {
      const response = await player.dispatch(json);
      // Successful inspector writes express deliberate user intent, even before first focus.
      const result = JSON.parse(response);
      if (result.status === 'success' && ['invoke','step','inject_input','set_field'].includes(JSON.parse(json).request.type)) initialResumePending = false;
      show(); return response;
    }, status: () => JSON.parse(player.status()) };
    function animate(now) { if (failed) return; graphics(() => { player.frame(previous === undefined ? 0 : now - previous); previous = now; show(); }); requestAnimationFrame(animate); }
    requestAnimationFrame(animate);
  } catch (error) { report(error); }
}
byId('pause').onclick = toggle;
byId('step').onclick = () => run(() => { deliberateAction(); player.step(); show(); });
byId('restart').onclick = () => run(() => { deliberateAction(); player.restart(); previous = undefined; show(); });
byId('replay').onclick = () => run(() => { deliberateAction(); player.replay_route(); player.resume(); previous = undefined; canvas.focus(); });
byId('control').onchange = () => player?.set_control_enabled(byId('control').checked);
byId('export').onclick = () => run(() => { const url = URL.createObjectURL(new Blob([player.recording()], {type:'application/json'})); const a = document.createElement('a'); a.href=url; a.download='collection-room.json'; a.click(); setTimeout(() => URL.revokeObjectURL(url), 0); });
byId('import').onchange = async event => { const file = event.target.files[0]; if (!file) return; if (file.size > 2 * 1024 * 1024) { byId('error').textContent='Recording exceeds 2 MiB.'; return; } try { const recording = await file.text(); player.load_recording(recording); deliberateAction(); previous=undefined; show(); } catch(error) { byId('error').textContent=`Recording rejected: ${error}`; } event.target.value=''; };

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
    byId('capture-download').download = 'collection-room-capture.json';
    byId('capture-result').hidden = false;
    byId('error').textContent = '';
  } catch (error) { byId('error').textContent = `Capture failed: ${error}`; }
  finally { byId('capture').disabled = failed; }
};

void initialize();
