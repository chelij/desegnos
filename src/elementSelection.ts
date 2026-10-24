import { sameDocument, sameTarget } from './elementTargets.js';
import type { ElementSelection, Rect } from './types';

/// A screenful of matching items is still a bounded, predictable edit.
export const MAX_SELECTION = 500;
/// How "select similar" recognises peers without relying on the structural selector.
export type ElementProfile = { tag: string; classes: string[]; role: string };

export function rectsIntersect(a: Rect, b: Rect): boolean {
  return Math.min(a.x + a.width, b.x + b.width) > Math.max(a.x, b.x) &&
    Math.min(a.y + a.height, b.y + b.height) > Math.max(a.y, b.y);
}

const depth = (selection: ElementSelection) => selection.element.selector.split(' > ').length;
const nested = (candidate: ElementSelection, outer: ElementSelection) =>
  sameDocument(candidate.element.document, outer.element.document) &&
  (sameTarget(candidate.element, outer.element) || candidate.element.selector.startsWith(`${outer.element.selector} > `));

/// Keep the outermost match for each part of the page: a marquee around a card selects the
/// card, not the card and every wrapper, heading and icon inside it.
export function topmostSelections(items: ElementSelection[]): ElementSelection[] {
  const kept: ElementSelection[] = [];
  for (const item of [...items].sort((left, right) => depth(left) - depth(right))) {
    if (item.element.selector === 'body' || kept.some(other => nested(item, other))) continue;
    kept.push(item);
  }
  return kept;
}

export function marqueeSelections(items: ElementSelection[], rect: Rect, existing: ElementSelection[] = [], limit = MAX_SELECTION): ElementSelection[] {
  const matched = items.filter(item => item.element.selector !== 'body' && rectsIntersect(item.rect, rect));
  return topmostSelections([...existing, ...matched]).slice(0, limit);
}

export function sameProfile(left: ElementProfile, right: ElementProfile): boolean {
  if (left.tag === right.tag && left.classes.length === right.classes.length && left.classes.every((name, index) => name === right.classes[index])) return true;
  return !!left.role && left.role === right.role;
}

/// Every peer of the reference: same tag and class list, or the same structural role
/// (heading level, link, button, field, summary).
export function similarSelections(reference: ElementProfile, items: { selection: ElementSelection; profile: ElementProfile }[], limit = MAX_SELECTION): ElementSelection[] {
  return topmostSelections(items.filter(item => sameProfile(reference, item.profile)).map(item => item.selection)).slice(0, limit);
}
