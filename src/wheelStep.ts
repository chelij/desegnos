// WebKitGTK scrolls panels a long way per mouse-wheel notch, and how far varies by compositor
// and mouse. Cap each wheel event's step instead; touchpad events are smaller and pass through.
const MAX_STEP_PX = 48;

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
    if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;
    const delta = event.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? event.deltaY : Math.sign(event.deltaY) * MAX_STEP_PX;
    if (Math.abs(delta) <= MAX_STEP_PX) return;
    const target = scroller(event.target, delta > 0);
    if (!target) return;
    event.preventDefault();
    target.scrollTop += Math.sign(delta) * MAX_STEP_PX;
  }, { passive: false });
}
