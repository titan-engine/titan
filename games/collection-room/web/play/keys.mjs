const movement = new Set(['KeyW','KeyA','KeyS','KeyD','ArrowUp','ArrowDown','ArrowLeft','ArrowRight']);
/** Physical codes preserve aliases; blur/focus cancellation discards buffered taps. */
export function bindKeys({canvas,key,clear,shortcut,window=globalThis.window,document=globalThis.document}) {
  const held = new Set();
  const cancel = () => { held.clear(); clear(); };
  window.addEventListener('keydown', event => {
    if (event.target !== canvas) return;
    if (!event.repeat && shortcut(event.code)) { event.preventDefault(); return; }
    if (!movement.has(event.code)) return;
    event.preventDefault(); held.add(event.code); key(event.code,true,event.repeat);
  });
  window.addEventListener('keyup', event => {
    if (!held.delete(event.code)) return;
    event.preventDefault(); key(event.code,false,false);
  });
  window.addEventListener('blur',cancel);
  document.addEventListener('focusin',event => { if(event.target !== canvas) cancel(); });
  document.addEventListener('visibilitychange',() => { if(document.hidden) cancel(); });
  return {cancel};
}
