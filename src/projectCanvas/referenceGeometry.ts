import type { Rect } from '../types';
import { GRID_SPACING, MIN_SIZE, snapValue, Point } from './camera';
import { BOUNDARY_PUSH_GAP, pushAway } from './boundaryLayout';

export type FrameCorner = 'nw' | 'ne' | 'sw' | 'se';

function frameBox(rect: Rect, boundary: Rect): Rect {
  const width = Math.min(Math.max(MIN_SIZE, Math.round(rect.width)), boundary.width);
  const height = Math.min(Math.max(MIN_SIZE, Math.round(rect.height)), boundary.height);
  const x = Math.min(Math.max(rect.x, boundary.x), boundary.x + boundary.width - width);
  const y = Math.min(Math.max(rect.y, boundary.y), boundary.y + boundary.height - height);
  return { x, y, width, height };
}

// Frames stay inside the boundary they were drawn in.
// A drag that starts inside a boundary draws a snapped frame; starting outside draws nothing.
export function drawnFrameRect(start: Point, end: Point, boundary: Rect, spacing = GRID_SPACING): Rect | null {
  if (!Number.isFinite(start.x) || !Number.isFinite(start.y) || !Number.isFinite(end.x) || !Number.isFinite(end.y)) return null;
  if (start.x < boundary.x || start.x > boundary.x + boundary.width || start.y < boundary.y || start.y > boundary.y + boundary.height) return null;
  // Snap first, then clamp: snapping an edge that is already at the boundary must not make the
  // frame grow past it and shift its anchored corner.
  const inside = (point: Point): Point => ({
    x: Math.min(Math.max(snapValue(point.x, spacing), boundary.x), boundary.x + boundary.width),
    y: Math.min(Math.max(snapValue(point.y, spacing), boundary.y), boundary.y + boundary.height),
  });
  const from = inside(start), to = inside(end);
  const left = Math.min(from.x, to.x), top = Math.min(from.y, to.y);
  return frameBox({ x: left, y: top, width: Math.max(MIN_SIZE, Math.abs(to.x - from.x)),
    height: Math.max(MIN_SIZE, Math.abs(to.y - from.y)) }, boundary);
}

// Reference items are reference: they live in canvas units outside every boundary.
export type ReferenceBox = { itemId: string; rect: Rect };

export type ReferenceMove = { itemId: string; x: number; y: number };

export const CLICK_NOTE_SIZE = { width: 160, height: 80 };

export const REFERENCE_ATTACHMENT_WIDTH = 200;

export const REFERENCE_ATTACHMENT_HEIGHT = 140;

export const REFERENCE_ATTACHMENT_GAP = 16;

export function referenceNoteRect(start: Point, end: Point, spacing = GRID_SPACING): Rect {
  const clampPoint = (point: Point) => ({ x: snapValue(point.x, spacing), y: snapValue(point.y, spacing) });
  const from = clampPoint(start), to = clampPoint(end);
  return {
    x: Math.min(from.x, to.x), y: Math.min(from.y, to.y),
    width: Math.max(MIN_SIZE, Math.abs(to.x - from.x)),
    height: Math.max(MIN_SIZE, Math.abs(to.y - from.y)),
  };
}

// Reference items move freely over the canvas; boundaries push them aside instead.
export function movedReferenceRect(rect: Rect, delta: Point, spacing = GRID_SPACING): Rect {
  return { ...rect, x: snapValue(rect.x + delta.x, spacing), y: snapValue(rect.y + delta.y, spacing) };
}

export function resizedReferenceRect(rect: Rect, corner: FrameCorner, point: Point, spacing = GRID_SPACING): Rect {
  const px = snapValue(point.x, spacing), py = snapValue(point.y, spacing);
  const dragsLeft = corner === 'nw' || corner === 'sw', dragsTop = corner === 'nw' || corner === 'ne';
  let left = dragsLeft ? px : rect.x, right = dragsLeft ? rect.x + rect.width : px;
  let top = dragsTop ? py : rect.y, bottom = dragsTop ? rect.y + rect.height : py;
  if (right - left < MIN_SIZE) { if (dragsLeft) left = right - MIN_SIZE; else right = left + MIN_SIZE; }
  if (bottom - top < MIN_SIZE) { if (dragsTop) top = bottom - MIN_SIZE; else bottom = top + MIN_SIZE; }
  return { x: left, y: top, width: right - left, height: bottom - top };
}

// A frame dragged out becomes a note, and its attachments become cards beside it, in one row.
export function convertedReferenceRects(frame: Rect, attachmentCount: number): { note: Rect; attachments: Rect[] } {
  const note = {
    x: frame.x, y: frame.y,
    width: Math.max(MIN_SIZE, frame.width),
    height: Math.max(MIN_SIZE, frame.height),
  };
  const attachments = Array.from({ length: Math.max(0, attachmentCount) }, (_, index) => ({
    x: note.x + note.width + REFERENCE_ATTACHMENT_GAP + index * (REFERENCE_ATTACHMENT_WIDTH + REFERENCE_ATTACHMENT_GAP),
    y: note.y, width: REFERENCE_ATTACHMENT_WIDTH, height: REFERENCE_ATTACHMENT_HEIGHT,
  }));
  return { note, attachments };
}

// A reference item dropped into a boundary keeps its size where it fits and is clamped inside.
export function referenceFrameRect(rect: Rect, boundary: Rect, spacing = GRID_SPACING): Rect {
  return frameBox({
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
