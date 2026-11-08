import { TEMPLATE_GAP } from '../generated/limits';
import type { BoundaryKind, PageSize, Rect } from '../types';
import { clamp, MIN_SIZE, snapValue, Point, insideRect } from './camera';

/** One boundary per linked size: a page with three sizes draws three boundaries side by side. */
type BoundarySize = PageSize & { height: number };

type BoundaryPage = { id: string; sizes: BoundarySize[] };

export type Boundary = { pageId: string; sizeId: string; label: string; rect: Rect; kind: BoundaryKind };

export type BoundaryMove = { pageId: string; sizeId: string; x: number; y: number; kind: BoundaryKind };

export const BOUNDARY_GAP = TEMPLATE_GAP;

export const BOUNDARY_PUSH_GAP = 40;

export const MAX_BOUNDARY_HEIGHT = 6000;

export function boundaryHeight(height: number): number {
  return Number.isFinite(height) && height > 0 ? clamp(height, MIN_SIZE, MAX_BOUNDARY_HEIGHT) : MIN_SIZE;
}

const placeable = (size: PageSize): boolean => Number.isFinite(size.x) && Number.isFinite(size.y);

// Linked sizes are boundaries of their own, base first, laid out side by side by default.
// Stored placements keep their spot; everything else follows left to right past them.
// A screen boundary is exactly its height and clips; a page boundary grows with content.
export function layoutBoundaries(pages: BoundaryPage[], homePageId: string, gap = BOUNDARY_GAP): Boundary[] {
  const ordered = [...pages.filter(page => page.id === homePageId), ...pages.filter(page => page.id !== homePageId)];
  const placed = ordered.flatMap(page => page.sizes.filter(placeable));
  let x = placed.length ? Math.max(...placed.map(size => size.x + pageWidth(size))) + gap : 0;
  const rows: Boundary[] = [];
  for (const page of ordered) {
    for (const size of page.sizes) {
      const width = pageWidth(size);
      const placedHere = placeable(size);
      const height = size.kind === 'screen' ? fixedHeight(size.height) : boundaryHeight(size.height);
      rows.push({
        pageId: page.id, sizeId: size.id, label: size.label, kind: size.kind,
        rect: placedHere ? { x: size.x, y: size.y, width, height } : { x, y: 0, width, height },
      });
      if (!placedHere) x += width + gap;
    }
  }
  return rows;
}

function pageWidth(size: PageSize): number {
  return Number.isFinite(size.width) && size.width > 0 ? Math.round(size.width) : 0;
}

function fixedHeight(height: number): number {
  return Number.isFinite(height) && height > 0 ? Math.round(height) : MIN_SIZE;
}

export function drawnBoundaryRect(start: Point, end: Point, spacing: number): Rect {
  const left = Math.min(snapValue(start.x, spacing), snapValue(end.x, spacing));
  const top = Math.min(snapValue(start.y, spacing), snapValue(end.y, spacing));
  return { x: left, y: top, width: Math.max(MIN_SIZE, Math.abs(snapValue(end.x, spacing) - snapValue(start.x, spacing))),
    height: Math.max(MIN_SIZE, Math.abs(snapValue(end.y, spacing) - snapValue(start.y, spacing))) };
}

// A frame lives in page coordinates: the boundary origin is the page's top-left corner and a
// boundary is exactly as wide as its page viewport, so canvas and page units differ by that origin.
export function canvasRectFromPage(rect: Rect, origin: Point): Rect {
  return { x: rect.x + origin.x, y: rect.y + origin.y, width: rect.width, height: rect.height };
}

export function pageRectFromCanvas(rect: Rect, origin: Point): Rect {
  return { x: rect.x - origin.x, y: rect.y - origin.y, width: rect.width, height: rect.height };
}

export function pushAway(mover: Rect, stayer: Rect, gap: number): boolean {
  const overlapX = Math.min(mover.x + mover.width, stayer.x + stayer.width) - Math.max(mover.x, stayer.x);
  const overlapY = Math.min(mover.y + mover.height, stayer.y + stayer.height) - Math.max(mover.y, stayer.y);
  if (overlapX <= -gap || overlapY <= -gap) return false;
  const dx = mover.x + mover.width / 2 >= stayer.x + stayer.width / 2
    ? stayer.x + stayer.width + gap - mover.x : -(mover.x + mover.width - stayer.x + gap);
  const dy = mover.y + mover.height / 2 >= stayer.y + stayer.height / 2
    ? stayer.y + stayer.height + gap - mover.y : -(mover.y + mover.height - stayer.y + gap);
  if (Math.abs(dx) <= Math.abs(dy)) mover.x += dx; else mover.y += dy;
  return true;
}

export function pushApartBoundaries(items: Boundary[], anchorId: string, gap = BOUNDARY_PUSH_GAP): Boundary[] {
  const result = items.map(item => ({ ...item, rect: { ...item.rect } }));
  const anchor = result.find(item => item.sizeId === anchorId);
  if (!anchor) return result;
  const centre = (rect: Rect) => ({ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 });
  const anchorCentre = centre(anchor.rect), ranks = new Map(result.map((item, index) => [item.sizeId, {
    distance: item.sizeId === anchorId ? -1 : Math.hypot(centre(item.rect).x - anchorCentre.x, centre(item.rect).y - anchorCentre.y), index }]));
  const farther = (first: Boundary, second: Boundary) => {
    const a = ranks.get(first.sizeId)!, b = ranks.get(second.sizeId)!;
    return a.distance > b.distance || (a.distance === b.distance && a.index > b.index) ? first : second;
  };
  for (let pass = 0; pass < 200; pass++) {
    let pushed = false;
    for (let i = 0; i < result.length; i++) for (let j = i + 1; j < result.length; j++) {
      const first = result[i], second = result[j];
      const mover = first.sizeId === anchorId ? second : second.sizeId === anchorId ? first : farther(first, second);
      const stayer = mover === first ? second : first;
      pushed = pushAway(mover.rect, stayer.rect, gap) || pushed;
    }
    if (!pushed) break;
  }
  return result;
}

export function boundaryMoves(boundaries: Boundary[], previous: Boundary[]): BoundaryMove[] {
  return boundaries.flatMap(item => {
    const before = previous.find(entry => entry.sizeId === item.sizeId);
    if (before && before.rect.x === item.rect.x && before.rect.y === item.rect.y && before.kind === item.kind) return [];
    return [{ pageId: item.pageId, sizeId: item.sizeId, x: item.rect.x, y: item.rect.y, kind: item.kind }];
  });
}

export function boundariesByPosition(boundaries: Boundary[]): Boundary[] {
  return [...boundaries].sort((first, second) => first.rect.y - second.rect.y || first.rect.x - second.rect.x || first.sizeId.localeCompare(second.sizeId));
}

export function sameBoundaries(first: Boundary[], second: Boundary[]): boolean {
  return first.length === second.length && first.every((item, index) => {
    const other = second[index];
    return item.pageId === other.pageId && item.sizeId === other.sizeId && item.label === other.label && item.kind === other.kind
      && item.rect.x === other.rect.x && item.rect.y === other.rect.y
      && item.rect.width === other.rect.width && item.rect.height === other.rect.height;
  });
}

export function boundaryAt<T extends { rect: Rect }>(items: T[], point: Point): T | null {
  for (let index = items.length - 1; index >= 0; index--) if (insideRect(items[index].rect, point)) return items[index];
  return null;
}

/** The boundary a press within `slack` of its edge belongs to, nearest first. */
export function boundaryNear<T extends { rect: Rect }>(items: T[], point: Point, slack: number): T | null {
  let best: T | null = null;
  let bestDistance = slack;
  for (const item of items) {
    const { x, y, width, height } = item.rect;
    const distance = Math.hypot(Math.max(x - point.x, 0, point.x - x - width), Math.max(y - point.y, 0, point.y - y - height));
    if (distance <= bestDistance) { best = item; bestDistance = distance; }
  }
  return best;
}

export function insideAnyBoundary(boundaries: { rect: Rect }[], point: Point): boolean {
  return boundaries.some(boundary => insideRect(boundary.rect, point));
}
