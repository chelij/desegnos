import type { Rect } from '../types';
import { MIN_SIZE, snapValue, Point } from './camera';
import { BOUNDARY_PUSH_GAP, pushAway } from './boundaryLayout';

export type ReferenceCorner = 'nw' | 'ne' | 'sw' | 'se';

function clampedBox(rect: Rect, boundary: Rect): Rect {
  const width = Math.min(Math.max(MIN_SIZE, Math.round(rect.width)), boundary.width);
  const height = Math.min(Math.max(MIN_SIZE, Math.round(rect.height)), boundary.height);
  const x = Math.min(Math.max(rect.x, boundary.x), boundary.x + boundary.width - width);
  const y = Math.min(Math.max(rect.y, boundary.y), boundary.y + boundary.height - height);
  return { x, y, width, height };
}

// Reference items are reference: they live in canvas units outside every boundary.
export type ReferenceBox = { itemId: string; rect: Rect };

export type ReferenceMove = { itemId: string; x: number; y: number };

export const CLICK_NOTE_SIZE = { width: 160, height: 80 };

export function referenceNoteRect(start: Point, end: Point, spacing: number): Rect {
  const clampPoint = (point: Point) => ({ x: snapValue(point.x, spacing), y: snapValue(point.y, spacing) });
  const from = clampPoint(start), to = clampPoint(end);
  return {
    x: Math.min(from.x, to.x), y: Math.min(from.y, to.y),
    width: Math.max(MIN_SIZE, Math.abs(to.x - from.x)),
    height: Math.max(MIN_SIZE, Math.abs(to.y - from.y)),
  };
}

// Reference items move freely over the canvas; boundaries push them aside instead.
export function movedReferenceRect(rect: Rect, delta: Point, spacing: number): Rect {
  return { ...rect, x: snapValue(rect.x + delta.x, spacing), y: snapValue(rect.y + delta.y, spacing) };
}

export function resizedReferenceRect(rect: Rect, corner: ReferenceCorner, point: Point, spacing: number): Rect {
  const px = snapValue(point.x, spacing), py = snapValue(point.y, spacing);
  const dragsLeft = corner === 'nw' || corner === 'sw', dragsTop = corner === 'nw' || corner === 'ne';
  let left = dragsLeft ? px : rect.x, right = dragsLeft ? rect.x + rect.width : px;
  let top = dragsTop ? py : rect.y, bottom = dragsTop ? rect.y + rect.height : py;
  if (right - left < MIN_SIZE) { if (dragsLeft) left = right - MIN_SIZE; else right = left + MIN_SIZE; }
  if (bottom - top < MIN_SIZE) { if (dragsTop) top = bottom - MIN_SIZE; else bottom = top + MIN_SIZE; }
  return { x: left, y: top, width: right - left, height: bottom - top };
}

// A reference item dropped into a boundary keeps its size where it fits and is clamped inside.
export function clampedReferenceRect(rect: Rect, boundary: Rect, spacing: number): Rect {
  return clampedBox({
    x: snapValue(rect.x, spacing), y: snapValue(rect.y, spacing),
    width: Math.max(MIN_SIZE, Math.round(rect.width)), height: Math.max(MIN_SIZE, Math.round(rect.height)),
  }, boundary);
}

// Boundaries never overlap reference items: each item moves by the shortest axis out of the way.
// Items do not push each other, so a reference stays a free collage.
export function pushApartReference(items: ReferenceBox[], boundaries: { rect: Rect }[], gap = BOUNDARY_PUSH_GAP): ReferenceBox[] {
  const result = items.map(item => ({ ...item, rect: { ...item.rect } }));
  for (let pass = 0; pass < 200; pass++) {
    let pushed = false;
    for (const item of result) for (const boundary of boundaries) {
      pushed = pushAway(item.rect, boundary.rect, gap) || pushed;
    }
    if (!pushed) break;
  }
  return result;
}

export function referenceMoves(items: ReferenceBox[], previous: ReferenceBox[]): ReferenceMove[] {
  return items.flatMap(item => {
    const before = previous.find(entry => entry.itemId === item.itemId);
    return before && before.rect.x === item.rect.x && before.rect.y === item.rect.y ? [] : [{ itemId: item.itemId, x: item.rect.x, y: item.rect.y }];
  });
}
