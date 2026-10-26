export type ReportedOutsideChange = { document: string; description: string };

const join = (labels: string[]) => labels.length < 2 ? labels.join('') : `${labels.slice(0, -1).join(', ')} and ${labels.at(-1)}`;

/** The AI's own short list of Flexible changes outside the requested anchor. */
export function summarizeReportedOutsideChanges(changes: readonly ReportedOutsideChange[]): string {
  const labels = [...new Set(changes.flatMap(change => {
    const description = change.description.trim().replace(/[.;]+$/, '');
    if (!description) return [];
    const document = change.document.trim();
    return [document ? `${document}: ${description}` : description];
  }))];
  return labels.length ? `Also changed outside this area: ${join(labels)}` : '';
}

/** Collapse repeated validation events and labels into one short result message. */
export function summarizeOutsideChanges(
  events: readonly { kind: string; message: string }[],
  reported: readonly ReportedOutsideChange[] = [],
): string {
  const fromAi = summarizeReportedOutsideChanges(reported);
  if (fromAi) return fromAi;
  const labels = [...new Set(events.filter(event => event.kind === 'outsideChanges').flatMap(event =>
    event.message.split(/Also changed\s*:?\s*/i).filter(Boolean).flatMap(line => line.split(/,\s*|\s+and\s+/)).map(label => label.trim().replace(/[.;]+$/, '')).filter(Boolean)
  ))];
  if (!labels.length) return '';
  const boxes = labels.filter(label => /^Box \d+$/i.test(label));
  const others = labels.filter(label => !boxes.includes(label));
  const short = labels.length > 3 ? [...others.slice(0, 2), ...(boxes.length ? [`${boxes.length} other ${boxes.length === 1 ? 'box' : 'boxes'}`] : []), ...(others.length > 2 ? [`${others.length - 2} other ${others.length === 3 ? 'change' : 'changes'}`] : [])] : labels;
  return `Also changed ${join(short)}`;
}
