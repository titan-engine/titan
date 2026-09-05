import init, { BrowserPlayer } from '../inspector/pkg/titan_game.js';
import { bindKeys } from './keys.mjs';
const canvas = document.querySelector('canvas');
const byId = id => document.getElementById(id);
let player, previous, failed = false;
const backend = new URL(location.href).searchParams.get('backend') ?? 'auto';
const report = error => { failed = true; player?.pause(); byId('error').textContent = `Graphics/player error: ${error}. Check GPU support and reload to start a fresh session.`; };
const run = fn => { try { fn(); byId('error').textContent = ''; } catch (error) { byId('error').textContent = String(error); } };
const graphics = fn => { try { fn(); } catch (error) { report(error); } };
const show = () => { byId('status').textContent = player.status(); byId('pause').textContent = player.paused() ? 'Resume' : 'Pause'; };
const keys = bindKeys({canvas, key: (...args) => player?.set_key(...args), clear: () => player?.clear_input(), shortcut: code => {
  if (!player) return false;
  if (code === 'Space') { toggle(); return true; }
  if (code === 'KeyN') { run(() => player.step()); return true; }
  if (code === 'KeyR') { run(() => player.restart()); return true; }
  return false;
}});
function toggle() { run(() => { keys.cancel(); player.paused() ? player.resume() : player.pause(); previous = undefined; show(); }); }
function resize() { if (!player) return; const rect = canvas.getBoundingClientRect(); graphics(() => player.resize(Math.round(rect.width * devicePixelRatio), Math.round(rect.height * devicePixelRatio))); }
new ResizeObserver(resize).observe(canvas);
window.addEventListener('blur', () => { player?.pause(); previous = undefined; });
document.addEventListener('visibilitychange', () => { if(document.hidden) { player?.pause(); previous = undefined; } });
byId('play').onclick = async () => {
  byId('play').disabled = true;
  try {
    await init();
    player = await BrowserPlayer.create(canvas, backend);
    player.set_control_enabled(byId('control').checked);
    for (const id of ['pause','step','restart','replay','export','import']) byId(id).disabled = false;
    resize(); player.resume(); canvas.focus();
    // Deliberate same-page inspection boundary. Runtime enforces explicit control opt-in.
    window.collectionRoom = { dispatch: json => player.dispatch(json), status: () => JSON.parse(player.status()) };
    function animate(now) { if (failed) return; graphics(() => { player.frame(previous === undefined ? 0 : now - previous); previous = now; show(); }); requestAnimationFrame(animate); }
    requestAnimationFrame(animate);
  } catch (error) { report(error); }
};
byId('pause').onclick = toggle;
byId('step').onclick = () => run(() => { keys.cancel(); player.step(); show(); });
byId('restart').onclick = () => run(() => { keys.cancel(); player.restart(); previous = undefined; show(); });
byId('replay').onclick = () => run(() => { keys.cancel(); player.replay_route(); player.resume(); previous = undefined; canvas.focus(); });
byId('control').onchange = () => player?.set_control_enabled(byId('control').checked);
byId('export').onclick = () => run(() => { const url = URL.createObjectURL(new Blob([player.recording()], {type:'application/json'})); const a = document.createElement('a'); a.href=url; a.download='collection-room.json'; a.click(); setTimeout(() => URL.revokeObjectURL(url), 0); });
byId('import').onchange = async event => { const file = event.target.files[0]; if (!file) return; if (file.size > 2 * 1024 * 1024) { byId('error').textContent='Recording exceeds 2 MiB.'; return; } try { player.load_recording(await file.text()); previous=undefined; show(); } catch(error) { byId('error').textContent=`Recording rejected: ${error}`; } event.target.value=''; };
