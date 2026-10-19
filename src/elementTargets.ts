import type { ElementSelection, ElementTarget, Rect, Region } from './types';

export function sameTarget(a: ElementTarget, b: ElementTarget): boolean {
  return a.id || b.id ? !!a.id && a.id === b.id : a.selector === b.selector && a.sourceSha256 === b.sourceSha256;
}
export function regionTargets(region: Region | undefined): ElementTarget[] { return region?.elements?.length ? region.elements : region?.element ? [region.element] : []; }
export function sameTargets(a: ElementTarget[], b: ElementTarget[]): boolean { return a.length === b.length && a.every(target => b.some(other => sameTarget(target, other))); }
export function unionRect(rects: Rect[]): Rect {
  const x = Math.min(...rects.map(rect => rect.x)), y = Math.min(...rects.map(rect => rect.y));
  return { x, y, width: Math.max(...rects.map(rect => rect.x + rect.width)) - x, height: Math.max(...rects.map(rect => rect.y + rect.height)) - y };
}
// Structural paths come from the same rendered page. IDs establish identity; paths establish containment.
export function canonicalSelections(items: ElementSelection[]): ElementSelection[] {
  return items.filter((item, index) => !items.some((other, at) => at !== index &&
    (sameTarget(item.element, other.element) ? at < index : item.element.selector.startsWith(`${other.element.selector} > `))));
}
