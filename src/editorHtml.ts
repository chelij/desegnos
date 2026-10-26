// Editor metadata and inline resources stay outside the visible document. Keep
// replacements reversible so ordinary code edits never discard pictures or IDs.
type Anchor = { at: number; text: string; placeholder?: string };
type Replacement = { from: number; to: number; insert: string };
export type HtmlProjection = {
  text: string; anchors: Anchor[]; rawToVisible: (offset: number) => number;
  replacements?: Replacement[]; source?: string;
};
const RAW = new Set(['script', 'style', 'textarea', 'title']);
export function projectEditorHtml(raw: string, showMetadata = false): HtmlProjection {
  const hidden: { from: number; to: number; text: string; anchor: number }[] = [];
  // Alternatives are disjoint: whitespace cannot be consumed by two nested
  // repetitions. Long/minified tags must not cause backtracking on a toggle.
  const tag = /<!--[\s\S]*?-->|<![^>]*>|<\/?([a-zA-Z][\w:-]*)(?:[^"'<>]|"[^"]*"|'[^']*')*>/g;
  let match: RegExpExecArray | null;
  while (!showMetadata && (match = tag.exec(raw))) {
    if (!match[1] || match[0].startsWith('</')) continue;
    const nameEnd = match.index + 1 + match[1].length;
    const attrs = /\s+([^\s=/>]+)(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?/g;
    attrs.lastIndex = 1 + match[1].length;
    let attr: RegExpExecArray | null;
    while ((attr = attrs.exec(match[0]))) if (attr[1].toLowerCase().startsWith('data-desegnos-')) hidden.push({ from: match.index + attr.index, to: match.index + attrs.lastIndex, text: attr[0], anchor: nameEnd });
    if (RAW.has(match[1].toLowerCase())) {
      const close = new RegExp(`</${match[1]}\\s*>`, 'ig'); close.lastIndex = tag.lastIndex;
      if (close.exec(raw)) tag.lastIndex = close.lastIndex; else break;
    }
  }
  return projectSource(raw, hidden);
}
// Also used for CSS and JavaScript. Never give CodeMirror embedded asset bytes,
// even for imported content that has not yet reached native storage.
export function projectCodeSource(raw: string): HtmlProjection { return projectSource(raw, []); }
function projectSource(raw: string, hidden: { from: number; to: number; text: string; anchor: number }[]): HtmlProjection {
  const replacements: Replacement[] = hidden.map(({ from, to }) => ({ from, to, insert: '' }));
  const assets: { from: number; text: string; placeholder: string }[] = [];
  let hiddenIndex = 0;
  for (const match of raw.matchAll(/\bdata:(?:[\w.+-]+\/[\w.+-]+)?(?:;[^\s,"'<>`)]*)?,[^\s"'<>`)]*/gi)) {
    const from = match.index;
    while (hiddenIndex < hidden.length && hidden[hiddenIndex].to <= from) hiddenIndex++;
    if (hidden[hiddenIndex]?.from <= from && hidden[hiddenIndex].to > from) continue;
    const placeholder = `desegnos-inline-asset:${from}`;
    assets.push({ from, text: match[0], placeholder });
    replacements.push({ from, to: from + match[0].length, insert: placeholder });
  }
  replacements.sort((a, b) => a.from - b.from);
  const parts: string[] = [], removed: number[] = [];
  let at = 0, total = 0;
  for (const item of replacements) {
    parts.push(raw.slice(at, item.from), item.insert); at = item.to;
    total += item.to - item.from - item.insert.length; removed.push(total);
  }
  parts.push(raw.slice(at));
  // Source maps can contain thousands of ranges. Binary search avoids scanning
  // every hidden attribute for every range (and again for every anchor).
  const rawToVisible = (offset: number) => {
    let lo = 0, hi = replacements.length;
    while (lo < hi) { const mid = (lo + hi) >>> 1; if (replacements[mid].to <= offset) lo = mid + 1; else hi = mid; }
    const item = replacements[lo], before = lo ? removed[lo - 1] : 0;
    return item && offset > item.from ? item.from - before + Math.min(offset - item.from, item.insert.length) : offset - before;
  };
  const grouped = new Map<number, string>();
  for (const item of hidden) { const at = rawToVisible(item.anchor); grouped.set(at, (grouped.get(at) ?? '') + item.text); }
  const anchors: Anchor[] = [...grouped].map(([at, text]) => ({ at, text }));
  for (const asset of assets) anchors.push({ at: rawToVisible(asset.from), text: asset.text, placeholder: asset.placeholder });
  return { text: parts.join(''), anchors, rawToVisible, replacements, source: raw };
}
export function restoreEditorHtml(projection: HtmlProjection): string {
  let raw = projection.text;
  for (const anchor of [...projection.anchors].sort((a, b) => b.at - a.at)) {
    if (anchor.placeholder) {
      if (raw.slice(anchor.at, anchor.at + anchor.placeholder.length) === anchor.placeholder) raw = raw.slice(0, anchor.at) + anchor.text + raw.slice(anchor.at + anchor.placeholder.length);
    } else {
      // A user may delete just a tag name. Never attach its metadata to plain text.
      const before = raw.slice(0, anchor.at);
      if (/<[a-zA-Z][\w:-]*(?:[^"'<>]|"[^"]*"|'[^']*')*$/.test(before)) raw = raw.slice(0, anchor.at) + anchor.text + raw.slice(anchor.at);
    }
  }
  return raw;
}
export function sourceOffset(text: string, line: number, column: number): number {
  let at = 0;
  for (let n = 1; n < line; n++) { const end = text.indexOf('\n', at); if (end < 0) return text.length; at = end + 1; }
  return Math.min(text.length, at + column - 1);
}
export function sourcePosition(text: string, offset: number) {
  const before = text.slice(0, offset); return { line: before.split('\n').length, column: offset - before.lastIndexOf('\n') };
}
