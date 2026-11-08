/** Escape closes the newest thing that is open on top of the canvas. Overlays register here; the editor's key handler asks once. */
type Dismiss = () => void;
const open: Dismiss[] = [];

export function onEscape(dismiss: Dismiss): () => void {
  open.push(dismiss);
  return () => { const index = open.lastIndexOf(dismiss); if (index >= 0) open.splice(index, 1); };
}

/** Dismisses the newest overlay; false when none is open. */
export function dismissNewestOverlay(): boolean {
  const dismiss = open.at(-1);
  if (!dismiss) return false;
  dismiss();
  return true;
}
