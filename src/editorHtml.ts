// Editor metadata is retained outside the visible text, never discarded on save.
// Attribute spans are collected from opening tags only; comments and raw-text
// elements are skipped so examples in script/style/text remain ordinary code.
type HiddenAttribute = { from: number; to: number; text: string; anchor: number };
type Anchor = { at: number; text: string };
export type HtmlProjection = { text: string; anchors: Anchor[]; rawToVisible: (offset: number) => number };
const RAW = new Set(['script', 'style', 'textarea', 'title']);
export function projectEditorHtml(raw: string): HtmlProjection {
  const hidden: HiddenAttribute[] = [];
  const tag = /<!--[\s\S]*?-->|<![^>]*>|<\/?([a-zA-Z][\w:-]*)(?:\s+(?:[^"'<>]|"[^"]*"|'[^']*')*)?\s*\/?>/g;
  let match: RegExpExecArray | null;
  while ((match = tag.exec(raw))) {
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
  let text = '', at = 0;
  for (const item of hidden) { text += raw.slice(at, item.from); at = item.to; }
  text += raw.slice(at);
  const rawToVisible = (offset: number) => offset - hidden.reduce((removed, item) => removed + Math.max(0, Math.min(offset, item.to) - item.from), 0);
  const grouped = new Map<number, string>();
  for (const item of hidden) { const at = rawToVisible(item.anchor); grouped.set(at, (grouped.get(at) ?? '') + item.text); }
  return { text, anchors: [...grouped].map(([at, text]) => ({ at, text })), rawToVisible };
}
export type HtmlEditRange = { from: number; to: number };
export function updateHtmlProjection(previous: HtmlProjection, text: string, edit?: HtmlEditRange): HtmlProjection {
  let start = 0;
  while (start < previous.text.length && start < text.length && previous.text[start] === text[start]) start++;
  let oldEnd = previous.text.length, newEnd = text.length;
  while (oldEnd > start && newEnd > start && previous.text[oldEnd - 1] === text[newEnd - 1]) { oldEnd--; newEnd--; }
  if (edit && edit.from >= 0 && edit.to >= edit.from && edit.to <= previous.text.length) {
    const insertedEnd = text.length - (previous.text.length - edit.to);
    if (text.slice(0, edit.from) === previous.text.slice(0, edit.from) && text.slice(insertedEnd) === previous.text.slice(edit.to)) { start = edit.from; oldEnd = edit.to; newEnd = insertedEnd; }
  }
  const shift = newEnd - oldEnd;
  const anchors = previous.anchors.flatMap(anchor => anchor.at < start ? [anchor] : anchor.at >= oldEnd ? [{ ...anchor, at: anchor.at + shift }] : []);
  return { text, anchors, rawToVisible: previous.rawToVisible };
}
export function restoreEditorHtml(projection: HtmlProjection): string {
  let raw = projection.text;
  for (const anchor of [...projection.anchors].sort((a, b) => b.at - a.at)) {
    // A user may delete just a tag name. Never attach its metadata to plain text.
    const before = raw.slice(0, anchor.at);
    if (/<[a-zA-Z][\w:-]*(?:\s+(?:[^"'<>]|"[^"]*"|'[^']*')*)?$/.test(before)) raw = raw.slice(0, anchor.at) + anchor.text + raw.slice(anchor.at);
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
