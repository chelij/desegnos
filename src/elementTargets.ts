import type { ElementSelection, ElementTarget, Rect, Region } from './types';

/// The document an element target lives in. Elements of the page itself carry no document.
export type TargetDocument = ElementTarget['document'];
export function documentKey(document?: TargetDocument | null): string {
  return document ? (document.kind === 'page' ? `page:${document.pageId}` : `site:${document.path}`) : 'page';
}
export function sameDocument(a?: TargetDocument | null, b?: TargetDocument | null): boolean {
  return documentKey(a) === documentKey(b);
}
/// Stable identity for a layer row, since selectors repeat across framed documents.
export function targetKey(target: ElementTarget): string {
  return `${documentKey(target.document)}|${target.selector}`;
}
/// Elements travel to native commands with their document beside them, never inside them.
export function nativeTarget(target: ElementTarget): ElementTarget {
  const { document: _document, ...plain } = target;
  return plain;
}
export function sameTarget(a: ElementTarget, b: ElementTarget): boolean {
  if (!sameDocument(a.document, b.document)) return false;
  return a.id || b.id ? !!a.id && a.id === b.id : a.selector === b.selector && a.sourceSha256 === b.sourceSha256;
}
export function regionTargets(region: Region | undefined): ElementTarget[] { return region?.elements ?? []; }
export function sameTargets(a: ElementTarget[], b: ElementTarget[]): boolean { return a.length === b.length && a.every(target => b.some(other => sameTarget(target, other))); }
export function unionRect(rects: Rect[]): Rect {
  const x = Math.min(...rects.map(rect => rect.x)), y = Math.min(...rects.map(rect => rect.y));
  return { x, y, width: Math.max(...rects.map(rect => rect.x + rect.width)) - x, height: Math.max(...rects.map(rect => rect.y + rect.height)) - y };
}
// Structural paths come from the same rendered page. IDs establish identity; paths establish containment.
export function canonicalSelections(items: ElementSelection[]): ElementSelection[] {
  return items.filter((item, index) => !items.some((other, at) => at !== index && sameDocument(item.element.document, other.element.document) &&
    (sameTarget(item.element, other.element) ? at < index : item.element.selector.startsWith(`${other.element.selector} > `))));
}
