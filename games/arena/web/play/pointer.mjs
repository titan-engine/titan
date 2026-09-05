// Canvas coordinates use CSS pixels; the game renders across the whole canvas.
export function canvasPoint(canvas, event) {
  const rect = canvas.getBoundingClientRect();
  const x = event.clientX - rect.left;
  const y = event.clientY - rect.top;
  if (![x, y, rect.width, rect.height].every(Number.isFinite)
      || rect.width <= 0 || rect.height <= 0 || x < 0 || y < 0 || x >= rect.width || y >= rect.height) return null;
  return [Math.min(159, Math.floor(x / rect.width * 160)), Math.min(111, Math.floor(y / rect.height * 112))];
}

export function bindCanvasPointer({ canvas, enabled, pointer, cancelPointer, afterPointer = () => {},
  window = globalThis.window, document = globalThis.document }) {
  let activeId = null;
  let captured = false;
  const consume = event => { event.preventDefault(); event.stopPropagation(); };
  function cancel() {
    const id = activeId;
    activeId = null;
    captured = false;
    cancelPointer();
    if (id !== null && canvas.hasPointerCapture(id)) canvas.releasePointerCapture(id);
  }
  function route(event, pressed) {
    const position = canvasPoint(canvas, event);
    if (!position) {
      if (captured) consume(event);
      cancel();
      return false;
    }
    const consumed = pointer(...position, pressed);
    if (consumed) consume(event);
    afterPointer();
    return consumed;
  }
  canvas.addEventListener('pointerdown', event => {
    if (!enabled() || activeId !== null || event.button !== 0 || event.isPrimary === false) return;
    canvas.focus();
    cancelPointer();
    activeId = event.pointerId;
    captured = route(event, true);
    if (activeId !== null && captured) canvas.setPointerCapture(activeId);
  });
  canvas.addEventListener('pointermove', event => {
    if (!enabled() || activeId !== event.pointerId) return;
    route(event, true);
  });
  function release(event) {
    if (activeId !== event.pointerId) return;
    const id = activeId;
    activeId = null;
    route(event, false);
    captured = false;
    if (canvas.hasPointerCapture(id)) canvas.releasePointerCapture(id);
  }
  canvas.addEventListener('pointerup', release);
  window.addEventListener('pointerup', release);
  for (const type of ['pointercancel', 'lostpointercapture']) {
    canvas.addEventListener(type, event => { if (activeId === event.pointerId) cancel(); });
  }
  canvas.addEventListener('pointerleave', () => { if (!captured) cancel(); });
  window.addEventListener('blur', cancel);
  document.addEventListener('focusin', event => { if (event.target !== canvas) cancel(); });
  document.addEventListener('visibilitychange', () => { if (document.hidden) cancel(); });
  return { cancel };
}
