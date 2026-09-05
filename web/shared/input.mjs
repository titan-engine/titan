/** Own browser buttons and focus lifecycle; games own bindings and tick policy.
 * Normal releases retain buffered taps. Cancellation explicitly discards them.
 */
export function bindPlayerInput({ canvas, buttons, keys, actions, isRunning,
  setAction, cancelAction, clearInput, onHidden = () => {}, onKey = () => false,
  window = globalThis.window, document = globalThis.document }) {
  const heldKeys = new Map();
  const heldPointers = new Map();
  const active = action => [...heldKeys.values(), ...heldPointers.values()].includes(action);
  const sync = () => { for (const action of actions) setAction(action, active(action)); };
  const cancel = () => { heldKeys.clear(); heldPointers.clear(); clearInput(); };
  const isControl = target => target?.closest?.('input, textarea, select, button, [contenteditable]');
  window.addEventListener('keydown', event => {
    if (isControl(event.target)) return;
    if (onKey(event)) return;
    const action = keys.get(event.key);
    if (!isRunning() || !action) return;
    event.preventDefault();
    heldKeys.set(event.code, action);
    sync();
  });
  window.addEventListener('keyup', event => {
    if (heldKeys.delete(event.code)) { event.preventDefault(); sync(); }
  });
  window.addEventListener('blur', cancel);
  document.addEventListener('focusin', event => { if (event.target !== canvas) cancel(); });
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) { cancel(); onHidden(); }
  });
  for (const button of buttons) {
    button.addEventListener('pointerdown', event => {
      if (!isRunning()) return;
      event.preventDefault();
      canvas.focus();
      button.setPointerCapture(event.pointerId);
      heldPointers.set(event.pointerId, button.dataset.action);
      sync();
    });
    for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) {
      button.addEventListener(type, event => {
        const action = heldPointers.get(event.pointerId);
        heldPointers.delete(event.pointerId);
        // Capture loss after pointerup has no entry and retains the normal tap.
        // Cancel only this action, preserving other held or buffered actions.
        if (type !== 'pointerup' && action !== undefined && !active(action)) cancelAction(action);
        sync();
      });
    }
  }
  return { cancel };
}
