// WebKitGTK scrolls panels a long way per mouse-wheel notch. Notches report whole multiples of
// 120 in wheelDeltaY; touchpads report fractions and keep native, kinetic scrolling.
const NOTCH_PX = 48;

function scroller(from: EventTarget | null, down: boolean): HTMLElement | null {
  for (let node = from instanceof Element ? from : null; node; node = node.parentElement) {
    if (!(node instanceof HTMLElement) || node.scrollHeight <= node.clientHeight) continue;
    if (!/auto|scroll/.test(getComputedStyle(node).overflowY)) continue;
    if (down ? node.scrollTop + node.clientHeight < node.scrollHeight - 1 : node.scrollTop > 0) return node;
  }
  return null;
}

export function installWheelStep() {
  document.addEventListener('wheel', event => {
    const notches = -((event as WheelEvent & { wheelDeltaY?: number }).wheelDeltaY ?? 0) / 120;
    if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.shiftKey || !notches || !Number.isInteger(notches)) return;
    const target = scroller(event.target, notches > 0);
    if (!target) return;
    event.preventDefault();
    target.scrollTop += notches * NOTCH_PX;
  }, { passive: false });
}
