import { clamp, MIN_ZOOM, ZOOM_LEVELS } from './canvasGeometry';
import { resolveForSize, sizeLabel } from './projectCanvas/sizes';
import type { BoundaryKind, PageSize, Pointer, PointerSource, PointerTarget, Project, Rect, Viewport } from './types';

export type Point = { x: number; y: number };
export type CanvasCamera = { x: number; y: number; zoom: number };
export type RequestSelection = { pageId: string; elementId: string; sizeId: string | null };
export type RequestBox = RequestSelection & { rect: Rect };
export type FrameCorner = 'nw' | 'ne' | 'sw' | 'se';
/** One boundary per linked size: a page with three sizes draws three boundaries side by side. */
export type BoundarySize = PageSize & { height: number };
export type BoundaryPage = { id: string; sizes: BoundarySize[] };
export type Boundary = { pageId: string; sizeId: string; label: string; rect: Rect; kind: BoundaryKind };
export type BoundaryMove = { pageId: string; sizeId: string; x: number; y: number; kind: BoundaryKind };
export const boundaryKey = (item: { pageId: string; sizeId: string }): string => item.sizeId;
export const BOUNDARY_GAP = 160;
export const BOUNDARY_PUSH_GAP = 40;
export const BOUNDARY_SNAP = 8;
export const FRAME_SNAP = 8;
export const MIN_FRAME_SIZE = 1;
export const MIN_BOUNDARY_WIDTH = 1;
export const MIN_BOUNDARY_DRAW_HEIGHT = 1;
export const MIN_BOUNDARY_HEIGHT = 1;
export const MAX_BOUNDARY_HEIGHT = 6000;
export const MAX_ZOOM = ZOOM_LEVELS.at(-1) ?? 4;
export const FIT_MAX_ZOOM = 1;
export const FIT_PADDING = 48;
export const CANVAS_DOT_SPACING = 8;
export const CANVAS_DOT_MIN_SCREEN = 6;
export const CANVAS_DOT_FADE_ZOOM = 0.1;

export function boundaryHeight(height: number): number {
  return Number.isFinite(height) && height > 0 ? clamp(height, MIN_BOUNDARY_HEIGHT, MAX_BOUNDARY_HEIGHT) : MIN_BOUNDARY_HEIGHT;
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
        pageId: page.id, sizeId: size.id, label: sizeLabel(size.width), kind: size.kind,
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
  return Number.isFinite(height) && height > 0 ? Math.round(height) : MIN_BOUNDARY_HEIGHT;
}

// Canvas units snap to 8px.
export function snapToBoundaryGrid(value: number): number {
  return Number.isFinite(value) ? Math.round(value / BOUNDARY_SNAP) * BOUNDARY_SNAP : 0;
}

export function drawnBoundaryRect(start: Point, end: Point): Rect {
  const left = Math.min(snapToBoundaryGrid(start.x), snapToBoundaryGrid(end.x));
  const top = Math.min(snapToBoundaryGrid(start.y), snapToBoundaryGrid(end.y));
  return { x: left, y: top, width: Math.max(MIN_BOUNDARY_WIDTH, Math.abs(snapToBoundaryGrid(end.x) - snapToBoundaryGrid(start.x))),
    height: Math.max(MIN_BOUNDARY_DRAW_HEIGHT, Math.abs(snapToBoundaryGrid(end.y) - snapToBoundaryGrid(start.y))) };
}

// A frame lives in page coordinates: the boundary origin is the page's top-left corner and a
// boundary is exactly as wide as its page viewport, so canvas and page units differ by that origin.
export function canvasRectFromPage(rect: Rect, origin: Point): Rect {
  return { x: rect.x + origin.x, y: rect.y + origin.y, width: rect.width, height: rect.height };
}

export function pageRectFromCanvas(rect: Rect, origin: Point): Rect {
  return { x: rect.x - origin.x, y: rect.y - origin.y, width: rect.width, height: rect.height };
}

export function snapToFrameGrid(value: number): number {
  return Number.isFinite(value) ? Math.round(value / FRAME_SNAP) * FRAME_SNAP : 0;
}

function frameBox(rect: Rect, boundary: Rect): Rect {
  const width = Math.min(Math.max(MIN_FRAME_SIZE, Math.round(rect.width)), boundary.width);
  const height = Math.min(Math.max(MIN_FRAME_SIZE, Math.round(rect.height)), boundary.height);
  const x = Math.min(Math.max(rect.x, boundary.x), boundary.x + boundary.width - width);
  const y = Math.min(Math.max(rect.y, boundary.y), boundary.y + boundary.height - height);
  return { x, y, width, height };
}

// Frames stay inside the boundary they were drawn in.
// A drag that starts inside a boundary draws a snapped frame; starting outside draws nothing.
export function drawnFrameRect(start: Point, end: Point, boundary: Rect): Rect | null {
  if (!Number.isFinite(start.x) || !Number.isFinite(start.y) || !Number.isFinite(end.x) || !Number.isFinite(end.y)) return null;
  if (start.x < boundary.x || start.x > boundary.x + boundary.width || start.y < boundary.y || start.y > boundary.y + boundary.height) return null;
  // Snap first, then clamp: snapping an edge that is already at the boundary must not make the
  // frame grow past it and shift its anchored corner.
  const inside = (point: Point): Point => ({
    x: Math.min(Math.max(snapToFrameGrid(point.x), boundary.x), boundary.x + boundary.width),
    y: Math.min(Math.max(snapToFrameGrid(point.y), boundary.y), boundary.y + boundary.height),
  });
  const from = inside(start), to = inside(end);
  const left = Math.min(from.x, to.x), top = Math.min(from.y, to.y);
  return frameBox({ x: left, y: top, width: Math.max(MIN_FRAME_SIZE, Math.abs(to.x - from.x)),
    height: Math.max(MIN_FRAME_SIZE, Math.abs(to.y - from.y)) }, boundary);
}

// Moving keeps the frame size and snaps its top-left corner, then pushes it back inside.
// The opposite corner stays anchored while the dragged corner follows the pointer.
// Frames render in page order, so the last one drawn is the one a point hits first.
// The anchor stays where it was put; every other boundary is pushed by the shortest move along
// its least-overlapping axis until nothing overlaps, keeping a 40px gap. Repeats so a push into
// a third boundary moves that one too.
export function pushApartBoundaries(items: Boundary[], anchorId: string, gap = BOUNDARY_PUSH_GAP): Boundary[] {
  const result = items.map(item => ({ ...item, rect: { ...item.rect } }));
  const anchor = result.find(item => boundaryKey(item) === anchorId);
  if (!anchor) return result;
  const centre = (rect: Rect) => ({ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 });
  const anchorCentre = centre(anchor.rect), ranks = new Map(result.map((item, index) => [boundaryKey(item), {
    distance: boundaryKey(item) === anchorId ? -1 : Math.hypot(centre(item.rect).x - anchorCentre.x, centre(item.rect).y - anchorCentre.y), index }]));
  const farther = (first: Boundary, second: Boundary) => {
    const a = ranks.get(boundaryKey(first))!, b = ranks.get(boundaryKey(second))!;
    return a.distance > b.distance || (a.distance === b.distance && a.index > b.index) ? first : second;
  };
  for (let pass = 0; pass < 200; pass++) {
    let pushed = false;
    for (let i = 0; i < result.length; i++) for (let j = i + 1; j < result.length; j++) {
      const first = result[i], second = result[j];
      const overlapX = Math.min(first.rect.x + first.rect.width, second.rect.x + second.rect.width) - Math.max(first.rect.x, second.rect.x);
      const overlapY = Math.min(first.rect.y + first.rect.height, second.rect.y + second.rect.height) - Math.max(first.rect.y, second.rect.y);
      if (overlapX <= -gap || overlapY <= -gap) continue;
      const mover = boundaryKey(first) === anchorId ? second : boundaryKey(second) === anchorId ? first : farther(first, second);
      const stayer = mover === first ? second : first;
      const right = stayer.rect.x + stayer.rect.width + gap - mover.rect.x;
      const left = mover.rect.x + mover.rect.width - stayer.rect.x + gap;
      const down = stayer.rect.y + stayer.rect.height + gap - mover.rect.y;
      const up = mover.rect.y + mover.rect.height - stayer.rect.y + gap;
      const dx = centre(mover.rect).x >= centre(stayer.rect).x ? right : -left;
      const dy = centre(mover.rect).y >= centre(stayer.rect).y ? down : -up;
      if (Math.abs(dx) <= Math.abs(dy)) mover.rect.x += dx; else mover.rect.y += dy;
      pushed = true;
    }
    if (!pushed) break;
  }
  return result;
}

// Only boundaries whose stored placement differs need a native round trip.
export function boundaryMoves(boundaries: Boundary[], previous: Boundary[]): BoundaryMove[] {
  return boundaries.flatMap(item => {
    const before = previous.find(entry => boundaryKey(entry) === boundaryKey(item));
    if (before && before.rect.x === item.rect.x && before.rect.y === item.rect.y && before.kind === item.kind) return [];
    return [{ pageId: item.pageId, sizeId: item.sizeId, x: item.rect.x, y: item.rect.y, kind: item.kind }];
  });
}

// Layers list boundaries top-left first, so a row maps to what the eye reads on the canvas.
export function boundariesByPosition(boundaries: Boundary[]): Boundary[] {
  return [...boundaries].sort((first, second) => first.rect.y - second.rect.y || first.rect.x - second.rect.x || boundaryKey(first).localeCompare(boundaryKey(second)));
}

// The spreadsheet dot grid: dots sit every 8 canvas pixels. They step up by powers of two
// so screen gaps never fall below ~6px, and fade out below 0.1 zoom instead of turning to noise.
export type DotGrid = { spacing: number; size: number; x: number; y: number; fade: number };

export function dotGrid(camera: CanvasCamera, showGrid: boolean): DotGrid | null {
  const zoom = camera.zoom;
  if (!showGrid || !Number.isFinite(zoom) || zoom <= 0) return null;
  let spacing = CANVAS_DOT_SPACING;
  for (let step = 0; step < 32 && spacing * zoom < CANVAS_DOT_MIN_SCREEN; step++) spacing *= 2;
  const size = spacing * zoom;
  const wrap = (value: number) => Number.isFinite(value) ? ((value % size) + size) % size : 0;
  return { spacing, size, x: wrap(camera.x), y: wrap(camera.y), fade: Math.min(1, zoom / CANVAS_DOT_FADE_ZOOM) };
}

export function unionBounds(rects: Rect[]): Rect {
  if (!rects.length) return { x: 0, y: 0, width: 0, height: 0 };
  const left = Math.min(...rects.map(rect => rect.x));
  const top = Math.min(...rects.map(rect => rect.y));
  const right = Math.max(...rects.map(rect => rect.x + rect.width));
  const bottom = Math.max(...rects.map(rect => rect.y + rect.height));
  return { x: left, y: top, width: right - left, height: bottom - top };
}

export function sameBoundaries(first: Boundary[], second: Boundary[]): boolean {
  return first.length === second.length && first.every((item, index) => {
    const other = second[index];
    return item.pageId === other.pageId && item.sizeId === other.sizeId && item.label === other.label && item.kind === other.kind
      && item.rect.x === other.rect.x && item.rect.y === other.rect.y
      && item.rect.width === other.rect.width && item.rect.height === other.rect.height;
  });
}

export function fitCamera(bounds: Rect, viewport: Viewport, padding = FIT_PADDING, maxZoom = FIT_MAX_ZOOM): CanvasCamera {
  const width = Math.max(1, viewport.width - padding * 2);
  const height = Math.max(1, viewport.height - padding * 2);
  const zoom = clamp(Math.min(width / Math.max(1, bounds.width), height / Math.max(1, bounds.height)), MIN_ZOOM, maxZoom);
  return { zoom, x: (viewport.width - bounds.width * zoom) / 2 - bounds.x * zoom, y: (viewport.height - bounds.height * zoom) / 2 - bounds.y * zoom };
}

// Screen = world * zoom + camera offset, so the world point under `point` stays put.
export function zoomCameraAt(camera: CanvasCamera, point: Point, zoom: number): CanvasCamera {
  const next = clamp(Number.isFinite(zoom) ? zoom : camera.zoom, MIN_ZOOM, MAX_ZOOM);
  return { zoom: next, x: point.x - (point.x - camera.x) / camera.zoom * next, y: point.y - (point.y - camera.y) / camera.zoom * next };
}

export function panCamera(camera: CanvasCamera, delta: Point): CanvasCamera {
  if (!Number.isFinite(delta.x) || !Number.isFinite(delta.y)) return camera;
  return { ...camera, x: camera.x + delta.x, y: camera.y + delta.y };
}

// Reference items are reference: they live in canvas units outside every boundary.
export type ReferenceBox = { itemId: string; rect: Rect };
export type ReferenceMove = { itemId: string; x: number; y: number };
/** A selected layer names the size it was clicked in; `sizeId: null` means every linked size. */
export type ElementTarget = { kind: 'element'; pageId: string; elementId: string; sizeId: string | null };
export type CanvasTarget = { kind: 'reference'; itemId: string } | { kind: 'pointer'; pointerId: string } | { kind: 'request'; requestId: string } | ElementTarget;

export const MIN_REFERENCE_WIDTH = 1;
export const MIN_REFERENCE_HEIGHT = 1;
// A click with the Frame tool outside boundaries places a note this size.
export const CLICK_NOTE_SIZE = { width: 160, height: 80 };
export const MIN_REFERENCE_ITEM_SIZE = 1;
export const REFERENCE_NOTE_MIN_WIDTH = 1;
export const REFERENCE_NOTE_MIN_HEIGHT = 1;
export const REFERENCE_ATTACHMENT_WIDTH = 200;
export const REFERENCE_ATTACHMENT_HEIGHT = 140;
export const REFERENCE_ATTACHMENT_GAP = 16;

export function insideRect(rect: Rect, point: Point): boolean {
  return Number.isFinite(point.x) && Number.isFinite(point.y) && point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height;
}

export function boundaryAt<T extends { rect: Rect }>(items: T[], point: Point): T | null {
  for (let index = items.length - 1; index >= 0; index--) if (insideRect(items[index].rect, point)) return items[index];
  return null;
}

export function insideAnyBoundary(boundaries: { rect: Rect }[], point: Point): boolean {
  return boundaries.some(boundary => insideRect(boundary.rect, point));
}

export function referenceItemAt<T extends { rect: Rect }>(items: T[], point: Point): T | null {
  return boundaryAt(items, point);
}

// The Frame tool outside every boundary draws a note of whatever size was dragged.
export function referenceNoteRect(start: Point, end: Point): Rect {
  const clampPoint = (point: Point) => ({ x: snapToFrameGrid(point.x), y: snapToFrameGrid(point.y) });
  const from = clampPoint(start), to = clampPoint(end);
  return {
    x: Math.min(from.x, to.x), y: Math.min(from.y, to.y),
    width: Math.max(MIN_REFERENCE_WIDTH, Math.abs(to.x - from.x)),
    height: Math.max(MIN_REFERENCE_HEIGHT, Math.abs(to.y - from.y)),
  };
}

// Reference items move freely over the canvas; boundaries push them aside instead.
export function movedReferenceRect(rect: Rect, delta: Point): Rect {
  return { ...rect, x: snapToFrameGrid(rect.x + delta.x), y: snapToFrameGrid(rect.y + delta.y) };
}

export function resizedReferenceRect(rect: Rect, corner: FrameCorner, point: Point): Rect {
  const px = snapToFrameGrid(point.x), py = snapToFrameGrid(point.y);
  const dragsLeft = corner === 'nw' || corner === 'sw', dragsTop = corner === 'nw' || corner === 'ne';
  let left = dragsLeft ? px : rect.x, right = dragsLeft ? rect.x + rect.width : px;
  let top = dragsTop ? py : rect.y, bottom = dragsTop ? rect.y + rect.height : py;
  if (right - left < MIN_REFERENCE_ITEM_SIZE) { if (dragsLeft) left = right - MIN_REFERENCE_ITEM_SIZE; else right = left + MIN_REFERENCE_ITEM_SIZE; }
  if (bottom - top < MIN_REFERENCE_ITEM_SIZE) { if (dragsTop) top = bottom - MIN_REFERENCE_ITEM_SIZE; else bottom = top + MIN_REFERENCE_ITEM_SIZE; }
  return { x: left, y: top, width: right - left, height: bottom - top };
}

// A frame dragged out becomes a note, and its attachments become cards beside it, in one row.
export function convertedReferenceRects(frame: Rect, attachmentCount: number): { note: Rect; attachments: Rect[] } {
  const note = {
    x: frame.x, y: frame.y,
    width: Math.max(REFERENCE_NOTE_MIN_WIDTH, frame.width),
    height: Math.max(REFERENCE_NOTE_MIN_HEIGHT, frame.height),
  };
  const attachments = Array.from({ length: Math.max(0, attachmentCount) }, (_, index) => ({
    x: note.x + note.width + REFERENCE_ATTACHMENT_GAP + index * (REFERENCE_ATTACHMENT_WIDTH + REFERENCE_ATTACHMENT_GAP),
    y: note.y, width: REFERENCE_ATTACHMENT_WIDTH, height: REFERENCE_ATTACHMENT_HEIGHT,
  }));
  return { note, attachments };
}

// A reference item dropped into a boundary keeps its size where it fits and is clamped inside.
export function referenceFrameRect(rect: Rect, boundary: Rect): Rect {
  return frameBox({
    x: snapToFrameGrid(rect.x), y: snapToFrameGrid(rect.y),
    width: Math.max(MIN_FRAME_SIZE, Math.round(rect.width)), height: Math.max(MIN_FRAME_SIZE, Math.round(rect.height)),
  }, boundary);
}

// Boundaries never overlap reference items: each item moves by the shortest axis out of the way.
// Items do not push each other, so a reference stays a free collage.
export function pushApartReference(items: ReferenceBox[], boundaries: { rect: Rect }[], gap = BOUNDARY_PUSH_GAP): ReferenceBox[] {
  const result = items.map(item => ({ ...item, rect: { ...item.rect } }));
  const centre = (rect: Rect) => ({ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 });
  for (let pass = 0; pass < 200; pass++) {
    let pushed = false;
    for (const item of result) for (const boundary of boundaries) {
      const overlapX = Math.min(item.rect.x + item.rect.width, boundary.rect.x + boundary.rect.width) - Math.max(item.rect.x, boundary.rect.x);
      const overlapY = Math.min(item.rect.y + item.rect.height, boundary.rect.y + boundary.rect.height) - Math.max(item.rect.y, boundary.rect.y);
      if (overlapX <= -gap || overlapY <= -gap) continue;
      const right = boundary.rect.x + boundary.rect.width + gap - item.rect.x;
      const left = item.rect.x + item.rect.width - boundary.rect.x + gap;
      const down = boundary.rect.y + boundary.rect.height + gap - item.rect.y;
      const up = item.rect.y + item.rect.height - boundary.rect.y + gap;
      const dx = centre(item.rect).x >= centre(boundary.rect).x ? right : -left;
      const dy = centre(item.rect).y >= centre(boundary.rect).y ? down : -up;
      if (Math.abs(dx) <= Math.abs(dy)) item.rect.x += dx; else item.rect.y += dy;
      pushed = true;
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

export const canvasTargetKey = (target: CanvasTarget): string => target.kind === 'pointer' ? `pointer:${target.pointerId}`
  : target.kind === 'request' ? `request:${target.requestId}`
  : target.kind === 'element' ? `element:${target.pageId}:${target.elementId}:${target.sizeId ?? 'all'}` : `reference:${target.itemId}`;

export const selectedPointers = (selection: CanvasTarget[]): string[] => selection.flatMap(item => item.kind === 'pointer' ? [item.pointerId] : []);
export const selectedRequests = (selection: CanvasTarget[]): string[] => selection.flatMap(item => item.kind === 'request' ? [item.requestId] : []);

export function hasCanvasTarget(selection: CanvasTarget[], target: CanvasTarget): boolean {
  const key = canvasTargetKey(target);
  return selection.some(item => canvasTargetKey(item) === key);
}

// Shift or Ctrl/Cmd toggles one item; a plain click replaces the selection.
export function toggleCanvasTarget(selection: CanvasTarget[], target: CanvasTarget, additive: boolean): CanvasTarget[] {
  if (!additive) return [target];
  const key = canvasTargetKey(target);
  return selection.some(item => canvasTargetKey(item) === key) ? selection.filter(item => canvasTargetKey(item) !== key) : [...selection, target];
}

export const selectedReference = (selection: CanvasTarget[]): string[] => selection.flatMap(item => item.kind === 'reference' ? [item.itemId] : []);
export const selectedElementIds = (selection: CanvasTarget[]): string[] => selection.flatMap(item => item.kind === 'element' ? [item.elementId] : []);
export const selectedElementTargets = (selection: CanvasTarget[]): ElementTarget[] => selection.flatMap(item => item.kind === 'element' ? [item] : []);
/** The patch a selection's edits use: one size's override, or all sizes together. */
export const sizeTarget = (target: ElementTarget): { size_id?: string | null; all_sizes?: boolean } =>
  target.sizeId === null ? { all_sizes: true } : { size_id: target.sizeId };

// ----------------------------------------------------------------------------
// Pointers: a thin line from a frame or boundary to what its comment may read or
// change. Everything here is pure so the canvas and its tests share one answer.
// ----------------------------------------------------------------------------

export const POINTER_COMMENT_ZOOM = 0.35;
export const POINTER_HIT_PX = 6;
export const POINTER_ARROW_PX = 9;
export const POINTER_BREAK_PX = 7;
export const POINTER_STUB = 120;

export type PointerEdge = 'left' | 'right' | 'top' | 'bottom';
export type PointerWorld = { requests: RequestBox[]; boundaries: { pageId: string; sizeId: string; rect: Rect }[]; reference: { itemId: string; rect: Rect; url?: string }[] };
export type PointerRoute = { from: Point; to: Point; broken: boolean; brokenEnd: 'from' | 'to' | null; source: Rect | null; target: Rect | null; fromEdge: PointerEdge; toEdge: PointerEdge };

export const screenPoint = (point: Point, camera: CanvasCamera): Point => ({ x: point.x * camera.zoom + camera.x, y: point.y * camera.zoom + camera.y });

export function pointerEndpointKey(endpoint: PointerSource | PointerTarget): string {
  switch (endpoint.kind) {
    case 'element': return `element:${endpoint.pageId}:${endpoint.elementId}:${endpoint.sizeId ?? ''}`;
    case 'boundary': return `boundary:${endpoint.pageId}:${endpoint.sizeId ?? ''}`;
    case 'reference': return `reference:${endpoint.itemId}`;
    case 'url': return `url:${endpoint.url}`;
  }
}

export function samePointerEndpoint(first: PointerSource | PointerTarget, second: PointerSource | PointerTarget): boolean {
  return pointerEndpointKey(first) === pointerEndpointKey(second);
}

// The nearest edge midpoints of two rectangles, so a line leaves the side that faces the target.
export function nearestEdgeAnchors(source: Rect, target: Rect): { from: Point; to: Point; fromEdge: PointerEdge; toEdge: PointerEdge } {
  const sourceCentre = { x: source.x + source.width / 2, y: source.y + source.height / 2 };
  const targetCentre = { x: target.x + target.width / 2, y: target.y + target.height / 2 };
  const dx = targetCentre.x - sourceCentre.x, dy = targetCentre.y - sourceCentre.y;
  if (Math.abs(dx) >= Math.abs(dy)) return dx >= 0
    ? { from: { x: source.x + source.width, y: sourceCentre.y }, to: { x: target.x, y: targetCentre.y }, fromEdge: 'right', toEdge: 'left' }
    : { from: { x: source.x, y: sourceCentre.y }, to: { x: target.x + target.width, y: targetCentre.y }, fromEdge: 'left', toEdge: 'right' };
  return dy >= 0
    ? { from: { x: sourceCentre.x, y: source.y + source.height }, to: { x: targetCentre.x, y: target.y }, fromEdge: 'bottom', toEdge: 'top' }
    : { from: { x: sourceCentre.x, y: source.y }, to: { x: targetCentre.x, y: target.y + target.height }, fromEdge: 'top', toEdge: 'bottom' };
}

const boundaryRectFor = (world: PointerWorld, pageId: string, sizeId?: string | null): Rect | null =>
  (world.boundaries.find(item => item.pageId === pageId && (!sizeId || item.sizeId === sizeId))
    ?? world.boundaries.find(item => item.pageId === pageId))?.rect ?? null;

// Where an endpoint is right now, ignoring fallbacks: null means the line is broken.
export function pointerEndpointRect(endpoint: PointerSource | PointerTarget, world: PointerWorld): Rect | null {
  switch (endpoint.kind) {
    case 'element': return world.requests.find(request => request.pageId === endpoint.pageId && request.elementId === endpoint.elementId
      && (!endpoint.sizeId || request.sizeId === endpoint.sizeId))?.rect ?? null;
    case 'boundary': return boundaryRectFor(world, endpoint.pageId, endpoint.sizeId);
    case 'reference': return world.reference.find(item => item.itemId === endpoint.itemId)?.rect ?? null;
    case 'url': return world.reference.find(item => item.url === endpoint.url)?.rect ?? null;
  }
}

// A missing frame or page still draws from its page boundary, so a broken line stays visible.
function pointerFallbackRect(endpoint: PointerSource | PointerTarget, world: PointerWorld): Rect | null {
  if (endpoint.kind === 'element' || endpoint.kind === 'boundary') return boundaryRectFor(world, endpoint.pageId, endpoint.sizeId);
  return null;
}

const stubPoint = (rect: Rect): Point => ({ x: rect.x + rect.width + POINTER_STUB, y: rect.y + rect.height + POINTER_STUB });

// A route always has two drawable ends: the real endpoint, its page boundary, or a short stub
// beside the end that still exists.
export function pointerRoute(pointer: Pointer, world: PointerWorld): PointerRoute | null {
  const source = pointerEndpointRect(pointer.from, world) ?? pointerFallbackRect(pointer.from, world);
  const target = pointerEndpointRect(pointer.to, world) ?? pointerFallbackRect(pointer.to, world);
  const brokenEnd = pointerEndpointRect(pointer.from, world) ? (pointerEndpointRect(pointer.to, world) ? null : 'to') : 'from';
  const broken = brokenEnd !== null;
  if (source && target) return { ...nearestEdgeAnchors(source, target), broken, brokenEnd, source, target };
  if (source) {
    const point = stubPoint(source);
    return { ...nearestEdgeAnchors(source, { ...point, width: 0, height: 0 }), broken, brokenEnd, source, target: null };
  }
  if (target) {
    const point = stubPoint(target);
    return { ...nearestEdgeAnchors({ ...point, width: 0, height: 0 }, target), broken, brokenEnd, source: null, target };
  }
  return null;
}

// The pointer a click hits, nearest to the line, within the screen tolerance in world units.
export function pointerAt<T extends { route: { from: Point; to: Point } }>(items: T[], point: Point, tolerance: number): T | null {
  let best: T | null = null, bestDistance = Number.POSITIVE_INFINITY;
  // Backwards, and strictly nearer: on a tie the last drawn pointer stays on top.
  for (let index = items.length - 1; index >= 0; index--) {
    const distance = pointSegmentDistance(point, items[index].route.from, items[index].route.to);
    if (distance <= tolerance && distance < bestDistance) { best = items[index]; bestDistance = distance; }
  }
  return best;
}

export function pointSegmentDistance(point: Point, start: Point, end: Point): number {
  const dx = end.x - start.x, dy = end.y - start.y;
  const length = dx * dx + dy * dy;
  if (!length) return Math.hypot(point.x - start.x, point.y - start.y);
  const t = Math.max(0, Math.min(1, ((point.x - start.x) * dx + (point.y - start.y) * dy) / length));
  return Math.hypot(point.x - (start.x + t * dx), point.y - (start.y + t * dy));
}

export function rectsIntersect(first: Rect, second: Rect): boolean {
  return first.x < second.x + second.width && first.x + first.width > second.x
    && first.y < second.y + second.height && first.y + first.height > second.y;
}

// A pointer is culled only when both of its ends are outside the visible world.
export function pointerVisible(source: Rect | null, target: Rect | null, viewport: Rect): boolean {
  if (source && rectsIntersect(source, viewport)) return true;
  if (target && rectsIntersect(target, viewport)) return true;
  return false;
}

export function viewportWorldRect(camera: CanvasCamera, size: { width: number; height: number }): Rect {
  return { x: -camera.x / camera.zoom, y: -camera.y / camera.zoom, width: size.width / camera.zoom, height: size.height / camera.zoom };
}

// Pure broken check against the current project: a target or source that no longer exists.

export function findIn(project: Project, pageId: string, elementId: string): import('./types').Element | null {
  const find = (elements: import('./types').Element[]): import('./types').Element | null => {
    for (const element of elements) {
      if (element.id === elementId) return element;
      const child = find(element.children);
      if (child) return child;
    }
    return null;
  };
  const page = project.pages.find(item => item.id === pageId);
  return page ? find(page.elements) : null;
}

export function elementLabel(element: import('./types').Element): string {
  if (element.name?.trim()) return element.name;
  const line = element.text.trim().split('\n')[0];
  return line || element.kind[0].toUpperCase() + element.kind.slice(1);
}

export function pointerBroken(pointer: Pointer, project: Project): boolean {
  const hasPage = (pageId: string) => project.pages.some(page => page.id === pageId);
  const hasSize = (pageId: string, sizeId?: string | null) => !sizeId
    || !!project.pages.find(page => page.id === pageId)?.sizes.some(size => size.id === sizeId);
  const hasElement = (pageId: string, elementId: string) => !!findIn(project, pageId, elementId);
  const missing = (endpoint: PointerSource | PointerTarget) => {
    switch (endpoint.kind) {
      case 'element': return !hasElement(endpoint.pageId, endpoint.elementId) || !hasSize(endpoint.pageId, endpoint.sizeId);
      case 'boundary': return !hasPage(endpoint.pageId) || !hasSize(endpoint.pageId, endpoint.sizeId);
      case 'reference': return !(project.reference ?? []).some(item => item.id === endpoint.itemId);
      case 'url': return !(project.reference ?? []).some(item => item.kind.kind === 'link' && item.kind.url === endpoint.url);
    }
  };
  return missing(pointer.from) || missing(pointer.to);
}

const referenceLabel = (item: { kind: { kind: string; text?: string; title?: string; url?: string; name?: string; color?: string } }): string => {
  switch (item.kind.kind) {
    case 'note': return (item.kind.text ?? '').trim().split('\n')[0] || 'Note';
    case 'link': return (item.kind.title ?? '').trim() || item.kind.url || 'Link';
    case 'image': return item.kind.name || 'Picture';
    default: return item.kind.color || 'Colour';
  }
};

// Source and target names for Properties, Layers and a pointer's empty label.
export function pointerEndpointName(endpoint: PointerSource | PointerTarget, project: Project, role: 'source' | 'target'): string {
  const missing = role === 'source' ? 'Missing source' : 'Missing target';
  switch (endpoint.kind) {
    case 'element': {
      const page = project.pages.find(item => item.id === endpoint.pageId);
      const element = page ? findIn(project, endpoint.pageId, endpoint.elementId) : null;
      if (!page || !element) return missing;
      return elementLabel(element);
    }
    case 'boundary': return project.pages.find(item => item.id === endpoint.pageId)?.name ?? missing;
    case 'reference': {
      const item = (project.reference ?? []).find(entry => entry.id === endpoint.itemId);
      return item ? referenceLabel(item) : missing;
    }
    case 'url': return endpoint.url;
  }
}

export function pointerLabel(pointer: Pointer, project: Project): string {
  return pointer.comment.trim().split('\n')[0]
    || `${pointerEndpointName(pointer.from, project, 'source')} → ${pointerEndpointName(pointer.to, project, 'target')}`;
}

// ----------------------------------------------------------------------------
// Link lines: when a layer inside a boundary links to another boundary's path, a
// faint line draws itself. Nobody draws those by hand.
// ----------------------------------------------------------------------------

export type AutoLink = { fromPageId: string; toPageId: string; source: Rect; target: Rect; route: { from: Point; to: Point } };

function findResolved(elements: import('./types').Element[], id: string): import('./types').Element | null {
  for (const element of elements) {
    if (element.id === id) return element;
    const child = findResolved(element.children, id);
    if (child) return child;
  }
  return null;
}

const pageLinkPath = (path: string): string => path.replace(/\/$/, '') || '/';

/** One line per linked boundary pair, from the layer that links to the boundary it names. */
export function automaticLinks(project: Project, boundaries: Boundary[], boxes: RequestBox[]): AutoLink[] {
  const links = new Map<string, AutoLink>();
  for (const box of boxes) {
    const page = project.pages.find(item => item.id === box.pageId);
    if (!page) continue;
    const sizeId = box.sizeId ?? page.sizes[0]?.id;
    if (!sizeId) continue;
    const element = findResolved(resolveForSize(project, page, sizeId), box.elementId);
    const url = element?.url?.trim();
    if (!url) continue;
    const wanted = pageLinkPath(url);
    const targetPage = project.pages.find(other => other.id !== box.pageId && pageLinkPath(other.path) === wanted);
    if (!targetPage) continue;
    const key = `${box.pageId}:${targetPage.id}`;
    if (links.has(key)) continue;
    const boundary = boundaries.find(item => item.pageId === targetPage.id && item.sizeId === targetPage.sizes[0]?.id)
      ?? boundaries.find(item => item.pageId === targetPage.id);
    if (!boundary) continue;
    links.set(key, {
      fromPageId: box.pageId, toPageId: targetPage.id, source: box.rect, target: boundary.rect,
      route: nearestEdgeAnchors(box.rect, boundary.rect),
    });
  }
  return [...links.values()];
}
