// CSS coordinates normalize to the logical framebuffer, independently of DPR.
export function logicalPointer(clientX, clientY, rect) {
  return { x: (clientX - rect.left) * 384 / rect.width, y: (clientY - rect.top) * 256 / rect.height };
}
