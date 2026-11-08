import { findElementIn } from './elementFacts';
import type { Pointer, PointerSource, PointerTarget, Project, Rect } from '../types';
import { Point, rectsIntersect } from './camera';
import { RequestBox } from './canvasTargets';

export const POINTER_COMMENT_ZOOM = 0.35;

export const POINTER_HIT_PX = 6;

export const POINTER_ARROW_PX = 9;

export const POINTER_BREAK_PX = 7;

const POINTER_STUB = 120;

type PointerEdge = 'left' | 'right' | 'top' | 'bottom';

export type PointerWorld = { requests: RequestBox[]; boundaries: { pageId: string; sizeId: string; rect: Rect }[]; reference: { itemId: string; rect: Rect; url?: string }[] };

export type PointerRoute = { from: Point; to: Point; broken: boolean; brokenEnd: 'from' | 'to' | null; source: Rect | null; target: Rect | null; fromEdge: PointerEdge; toEdge: PointerEdge };

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

// A missing layer or page still draws from its page boundary, so a broken line stays visible.
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

// A pointer is culled only when both of its ends are outside the visible world.
export function pointerVisible(source: Rect | null, target: Rect | null, viewport: Rect): boolean {
  if (source && rectsIntersect(source, viewport)) return true;
  if (target && rectsIntersect(target, viewport)) return true;
  return false;
}

export function findIn(project: Project, pageId: string, elementId: string): import('../types').Element | null {
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
      case 'reference': return !project.reference.some(item => item.id === endpoint.itemId);
      case 'url': return !project.reference.some(item => item.kind.kind === 'link' && item.kind.url === endpoint.url);
    }
  };
  return missing(pointer.from) || missing(pointer.to);
}
