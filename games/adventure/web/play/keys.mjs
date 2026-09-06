const gameplay = new Set(['KeyW','KeyA','KeyS','KeyD','ArrowUp','ArrowDown','ArrowLeft','ArrowRight','KeyQ','KeyR','KeyE','Space']);
/** Physical codes preserve aliases; blur/focus cancellation discards buffered taps. */
export function bindKeys({canvas,key,clear,pause=()=>{},shortcut,window=globalThis.window,document=globalThis.document}) {
  const held = new Set();
  const cancel = () => { held.clear(); clear(); };
  window.addEventListener('keydown', event => {
    if (event.target !== canvas) return;
    if (!event.repeat && shortcut(event.code)) { event.preventDefault(); return; }
    if (!gameplay.has(event.code)) return;
    event.preventDefault(); held.add(event.code); key(event.code,true,event.repeat);
  });
  window.addEventListener('keyup', event => {
    const wasHeld = held.delete(event.code);
    if (!wasHeld && !gameplay.has(event.code)) return;
    event.preventDefault(); key(event.code,false,false);
  });
  const loseFocus = () => { cancel(); pause(); };
  window.addEventListener('blur',loseFocus);
  document.addEventListener('focusin',event => { if(event.target !== canvas) loseFocus(); });
  document.addEventListener('visibilitychange',() => { if(document.hidden) loseFocus(); });
  return {cancel};
}
