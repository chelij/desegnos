/** Collapse repeated validation events and labels into one short result message. */
export function summarizeOutsideChanges(events: readonly { kind: string; message: string }[]): string {
  const labels = [...new Set(events.filter(event => event.kind === 'outsideChanges').flatMap(event =>
    event.message.split(/Also changed\s*:?\s*/i).filter(Boolean).flatMap(line => line.split(/,\s*|\s+and\s+/)).map(label => label.trim().replace(/[.;]+$/, '')).filter(Boolean)
  ))];
  if (!labels.length) return '';
  const boxes = labels.filter(label => /^Box \d+$/i.test(label));
  const others = labels.filter(label => !boxes.includes(label));
  const short = labels.length > 3 ? [...others.slice(0, 2), ...(boxes.length ? [`${boxes.length} other ${boxes.length === 1 ? 'box' : 'boxes'}`] : []), ...(others.length > 2 ? [`${others.length - 2} other ${others.length === 3 ? 'change' : 'changes'}`] : [])] : labels;
  return `Also changed ${short.length < 2 ? short.join('') : `${short.slice(0, -1).join(', ')} and ${short.at(-1)}`}`;
}
