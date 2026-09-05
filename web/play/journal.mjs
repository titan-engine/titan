/** Journal gestures use the same physical sequence on every host. */
export function bindJournalInput({ canvas, player, changed, window = globalThis.window, document = globalThis.document }) {
  let pointer;
  const cancel = () => { pointer = undefined; player()?.cancel_journal_input(); };
  const send = (event, pressed) => {
    const bounds = canvas.getBoundingClientRect();
    return player()?.journal_pointer(
      (event.clientX - bounds.left) * canvas.width / bounds.width,
      (event.clientY - bounds.top) * canvas.height / bounds.height, pressed);
  };
  canvas.addEventListener('pointerdown', event => {
    if (event.button !== 0 || pointer !== undefined || !player()) return;
    canvas.focus();
    pointer = event.pointerId;
    canvas.setPointerCapture(pointer);
    if (send(event, true)) event.preventDefault();
    changed();
  });
  canvas.addEventListener('pointermove', event => {
    if (pointer !== undefined && pointer !== event.pointerId) return;
    if (send(event, pointer !== undefined)) event.preventDefault();
    changed();
  });
  canvas.addEventListener('pointerup', event => {
    if (pointer !== event.pointerId) return;
    pointer = undefined;
    if (send(event, false)) event.preventDefault();
    changed();
  });
  for (const type of ['pointercancel', 'lostpointercapture']) {
    canvas.addEventListener(type, event => {
      if (pointer !== event.pointerId) return;
      cancel(); changed();
    });
  }
  canvas.addEventListener('pointerleave', () => {
    if (pointer === undefined) { player()?.journal_pointer(NaN, NaN, false); changed(); }
  });
  window.addEventListener('blur', cancel);
  document.addEventListener('focusin', event => { if (event.target !== canvas) cancel(); });
  document.addEventListener('visibilitychange', () => { if (document.hidden) cancel(); });
  return {
    cancel,
    // Session transitions have already reset logical gestures and focus.
    cancelHeld: () => { pointer = undefined; },
    onKey(event) {
      const session = player();
      if (!session) return false;
      const open = session.journal_open();
      // A held movement key must not reappear after an epoch/focus cancellation.
      if (event.repeat && (open || /^(ArrowUp|ArrowDown|ArrowLeft|ArrowRight|[wasdWASDjJ])$/.test(event.key))) {
        event.preventDefault();
        return true;
      }
      const key = ({ j: 'toggle', J: 'toggle', ArrowDown: 'next', ArrowUp: 'previous',
        Tab: event.shiftKey ? 'previous' : 'next', Enter: 'activate', ' ': 'activate', Escape: 'close' })[event.key];
      const consumed = (!event.repeat && key && session.journal_key(key)) || open;
      if (consumed) { event.preventDefault(); changed(); }
      return Boolean(consumed);
    },
  };
}
