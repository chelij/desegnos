import { clamp, MIN_ZOOM, ZOOM_LEVELS } from './canvasGeometry';
import type { Rect, SiteContent, Viewport } from './types';

export type Point = { x: number; y: number };
export type CanvasCamera = { x: number; y: number; zoom: number };
export type BoundaryPage = { id: string; width: number; height: number };
export type Boundary = { pageId: string; rect: Rect };
export type SnapshotPage = { id: string; viewport: Viewport; content: SiteContent | null };
export type CachedSnapshot = { key: string; html: string; height: number };
export type SnapshotCache = Map<string, CachedSnapshot>;
export type SnapshotCapture = { page: SnapshotPage; key: string };
export type SnapshotState = { snapshots: Record<string, { html: string }>; heights: Record<string, number> };

export const BOUNDARY_GAP = 160;
export const MIN_BOUNDARY_HEIGHT = 200;
export const MAX_BOUNDARY_HEIGHT = 6000;
export const MAX_ZOOM = ZOOM_LEVELS.at(-1) ?? 4;
export const FIT_MAX_ZOOM = 1;
export const FIT_PADDING = 48;

export function boundaryHeight(height: number): number {
  return Number.isFinite(height) && height > 0 ? clamp(height, MIN_BOUNDARY_HEIGHT, MAX_BOUNDARY_HEIGHT) : MIN_BOUNDARY_HEIGHT;
}

// Home page first, then the project's page order. Tops align; each page keeps its own width.
export function layoutBoundaries(pages: BoundaryPage[], homePageId: string, gap = BOUNDARY_GAP): Boundary[] {
  const ordered = [...pages.filter(page => page.id === homePageId), ...pages.filter(page => page.id !== homePageId)];
  let x = 0;
  return ordered.map(page => {
    const width = Number.isFinite(page.width) && page.width > 0 ? Math.round(page.width) : 0;
    const boundary = { pageId: page.id, rect: { x, y: 0, width, height: boundaryHeight(page.height) } };
    x += width + gap;
    return boundary;
  });
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
    return item.pageId === other.pageId && item.rect.x === other.rect.x && item.rect.y === other.rect.y
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

export const snapshotCacheKey = (projectId: string, pageId: string): string => `${projectId}\u0000${pageId}`;

// FNV-1a over length-prefixed parts: cheap, and a changed byte anywhere changes the key.
export function snapshotContentKey(content: SiteContent | null, viewport: Viewport): string {
  const parts = [content?.html ?? '', content?.css ?? '', content?.javascript ?? '', String(viewport.width), String(viewport.height)];
  let hash = 0x811c9dc5;
  for (const part of parts) {
    hash = Math.imul(hash ^ part.length, 0x01000193);
    for (let index = 0; index < part.length; index++) hash = Math.imul(hash ^ part.charCodeAt(index), 0x01000193);
  }
  return (hash >>> 0).toString(36);
}

export function cachedSnapshotState(cache: SnapshotCache, projectId: string, pages: SnapshotPage[]): SnapshotState {
  const snapshots: SnapshotState['snapshots'] = {}, heights: SnapshotState['heights'] = {};
  for (const page of pages) {
    const entry = cache.get(snapshotCacheKey(projectId, page.id));
    if (!entry) continue;
    snapshots[page.id] = { html: entry.html };
    if (entry.height > 0) heights[page.id] = entry.height;
  }
  return { snapshots, heights };
}

export function pendingSnapshots(cache: SnapshotCache, projectId: string, pages: SnapshotPage[]): SnapshotCapture[] {
  return pages.flatMap(page => {
    if (!page.content) return [];
    const key = snapshotContentKey(page.content, page.viewport);
    return cache.get(snapshotCacheKey(projectId, page.id))?.key === key ? [] : [{ page, key }];
  });
}

export function rememberSnapshot(cache: SnapshotCache, projectId: string, pageId: string, entry: CachedSnapshot): void {
  cache.set(snapshotCacheKey(projectId, pageId), entry);
}

export function updateSnapshotHeight(cache: SnapshotCache, projectId: string, pageId: string, height: number): void {
  const key = snapshotCacheKey(projectId, pageId), entry = cache.get(key);
  if (entry && height > 0 && entry.height !== height) cache.set(key, { ...entry, height });
}

// Pages keep a snapshot until their own content or viewport changes; deleted pages leave the cache.
export function planSnapshots(cache: SnapshotCache, projectId: string, pages: SnapshotPage[]): SnapshotState & { pending: SnapshotCapture[] } {
  const prefix = `${projectId}\u0000`, live = new Set(pages.map(page => snapshotCacheKey(projectId, page.id)));
  for (const key of cache.keys()) if (key.startsWith(prefix) && !live.has(key)) cache.delete(key);
  return { ...cachedSnapshotState(cache, projectId, pages), pending: pendingSnapshots(cache, projectId, pages) };
}
