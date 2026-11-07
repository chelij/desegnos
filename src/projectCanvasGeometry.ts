export { findElementIn } from './projectCanvas/elementFacts';
import { elementLabel, findElementIn } from './projectCanvas/elementFacts';
export const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));
export const MIN_ZOOM = 0.001;
export const ZOOM_LEVELS = [0.1, 0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];
export const zoomLabel = (zoom: number) => `${Math.round(zoom * 100)}%`;
export function steppedZoom(zoom: number, direction: number): number {
  return direction > 0 ? ZOOM_LEVELS.find(value => value > zoom + 1e-6) ?? MAX_ZOOM
    : [...ZOOM_LEVELS].reverse().find(value => value < zoom - 1e-6) ?? MIN_ZOOM;
}
export function wheelZoom(zoom: number, delta: number): number {
  return Number.isFinite(delta) ? clamp(Math.round(zoom * Math.exp(-clamp(delta, -800, 800) * 0.0015) * 1e6) / 1e6, MIN_ZOOM, MAX_ZOOM) : zoom;
}

export const GRID_SPACING = 8;
export const MIN_SIZE = 1;
export function snapValue(value: number, spacing = GRID_SPACING): number {
  if (spacing === 0) return value;
  const step = Number.isFinite(spacing) && spacing > 0 ? spacing : GRID_SPACING;
  return Number.isFinite(value) ? Math.round(value / step) * step : 0;
}
import { resolveForSize, sizeLabel } from './projectCanvas/sizes';
import { TEMPLATE_GAP } from './generated/limits';
import type { BoundaryKind, PageSize, Pointer, PointerSource, PointerTarget, Project, Rect, Viewport } from './types';

export type Point = { x: number; y: number };
export type CanvasCamera = { x: number; y: number; zoom: number };
type RequestSelection = { pageId: string; elementId: string; sizeId: string | null };
export type RequestBox = RequestSelection & { rect: Rect };
export type FrameCorner = 'nw' | 'ne' | 'sw' | 'se';
/** One boundary per linked size: a page with three sizes draws three boundaries side by side. */
type BoundarySize = PageSize & { height: number };
type BoundaryPage = { id: string; sizes: BoundarySize[] };
export type Boundary = { pageId: string; sizeId: string; label: string; rect: Rect; kind: BoundaryKind };
export type BoundaryMove = { pageId: string; sizeId: string; x: number; y: number; kind: BoundaryKind };
export const BOUNDARY_GAP = TEMPLATE_GAP;
export const BOUNDARY_PUSH_GAP = 40;
export const MAX_BOUNDARY_HEIGHT = 6000;
export const MAX_ZOOM = ZOOM_LEVELS[ZOOM_LEVELS.length - 1];
const FIT_MAX_ZOOM = 1;
export const FIT_PADDING = 48;
export const CANVAS_DOT_MIN_SCREEN = 6;
const CANVAS_DOT_FADE_ZOOM = 0.1;

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
  return Number.isFinite(height) && height > 0 ? Math.round(height) : MIN_SIZE;
}

export function drawnBoundaryRect(start: Point, end: Point, spacing = GRID_SPACING): Rect {
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

function pushAway(mover: Rect, stayer: Rect, gap: number): boolean {
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

type DotGrid = { spacing: number; size: number; x: number; y: number; fade: number };

export function dotGrid(camera: CanvasCamera, showGrid: boolean, gridSpacing = GRID_SPACING): DotGrid | null {
  const zoom = camera.zoom;
  if (!showGrid || !Number.isFinite(zoom) || zoom <= 0) return null;
  let spacing = Number.isFinite(gridSpacing) && gridSpacing > 0 ? gridSpacing : GRID_SPACING;
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

// Inline editing needs readable type, not a fit of every linked page size. Tiny
// type (<12px at 100%) may magnify past 100%, within the usual camera limits.
export function textEditingCamera(camera: CanvasCamera, bounds: Rect, fontSize: number, viewport: Viewport): CanvasCamera {
  if (!Number.isFinite(fontSize) || fontSize <= 0 || fontSize * camera.zoom >= 12
    || viewport.width < 1 || viewport.height < 1) return camera;
  const targetZoom = Math.min(16 / fontSize, fontSize < 12 ? MAX_ZOOM : 1);
  // Prefer the whole text box when a very large element cannot fit at 16px.
  return fitCamera(bounds, viewport, 24, targetZoom);
}

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

export const CLICK_NOTE_SIZE = { width: 160, height: 80 };
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

export const canvasTargetKey = (target: CanvasTarget): string => target.kind === 'pointer' ? `pointer:${target.pointerId}`
  : target.kind === 'request' ? `request:${target.requestId}`
  : target.kind === 'element' ? `element:${target.pageId}:${target.elementId}:${target.sizeId ?? 'all'}` : `reference:${target.itemId}`;

export const selectedPointers = (selection: CanvasTarget[]): string[] => selection.flatMap(item => item.kind === 'pointer' ? [item.pointerId] : []);

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
export const sizeTarget = (target: { sizeId?: string | null; allSizes?: boolean }): { size_id?: string | null; all_sizes?: boolean } =>
  target.allSizes || target.sizeId === null ? { all_sizes: true } : { size_id: target.sizeId };

// ----------------------------------------------------------------------------
// Pointers: a thin line from a frame or boundary to what its comment may read or
// change. Everything here is pure so the canvas and its tests share one answer.
// ----------------------------------------------------------------------------

export const POINTER_COMMENT_ZOOM = 0.35;
export const POINTER_HIT_PX = 6;
export const POINTER_ARROW_PX = 9;
export const POINTER_BREAK_PX = 7;
const POINTER_STUB = 120;

type PointerEdge = 'left' | 'right' | 'top' | 'bottom';
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

export const sameRect = (first: Rect, second: Rect) => first.x === second.x && first.y === second.y && first.width === second.width && first.height === second.height;

export function findIn(project: Project, pageId: string, elementId: string): import('./types').Element | null {
  return findElementIn(project.pages.find(page => page.id === pageId)?.elements ?? [], elementId);
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

const referenceLabel = (item: { kind: { kind: string; text?: string; title?: string; url?: string; name?: string; color?: string; element?: { name: string | null; kind: string } } }): string => {
  switch (item.kind.kind) {
    case 'element': return item.kind.element?.name || item.kind.element?.kind || 'Layer';
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

const pageLinkPath = (path: string): string => path.replace(/\/$/, '') || '/';

/** One line per linked boundary pair, from the layer that links to the boundary it names. */
export function automaticLinks(project: Project, boundaries: Boundary[], boxes: RequestBox[]): AutoLink[] {
  const links = new Map<string, AutoLink>();
  for (const box of boxes) {
    const page = project.pages.find(item => item.id === box.pageId);
    if (!page) continue;
    const sizeId = box.sizeId ?? page.sizes[0]?.id;
    if (!sizeId) continue;
    const element = findElementIn(resolveForSize(project, page, sizeId), box.elementId);
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
