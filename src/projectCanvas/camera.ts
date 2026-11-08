import { DEFAULT_GRID_SPACING } from '../generated/limits';
import type { Rect, Viewport } from '../types';

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

export const MIN_SIZE = 1;

export function snapValue(value: number, spacing: number): number {
  if (spacing === 0) return value;
  const step = Number.isFinite(spacing) && spacing > 0 ? spacing : DEFAULT_GRID_SPACING;
  return Number.isFinite(value) ? Math.round(value / step) * step : 0;
}

export type Point = { x: number; y: number };

export type CanvasCamera = { x: number; y: number; zoom: number };

export const MAX_ZOOM = ZOOM_LEVELS[ZOOM_LEVELS.length - 1];

const FIT_MAX_ZOOM = 1;

export const FIT_PADDING = 48;

export const CANVAS_DOT_MIN_SCREEN = 6;

const CANVAS_DOT_FADE_ZOOM = 0.1;

type DotGrid = { spacing: number; size: number; x: number; y: number; fade: number };

export function dotGrid(camera: CanvasCamera, showGrid: boolean, gridSpacing: number): DotGrid | null {
  const zoom = camera.zoom;
  if (!showGrid || !Number.isFinite(zoom) || zoom <= 0) return null;
  let spacing = Number.isFinite(gridSpacing) && gridSpacing > 0 ? gridSpacing : DEFAULT_GRID_SPACING;
  for (let step = 0; step < 32 && spacing * zoom < CANVAS_DOT_MIN_SCREEN; step++) spacing *= 2;
  const size = spacing * zoom;
  const wrap = (value: number) => Number.isFinite(value) ? ((value % size) + size) % size : 0;
  return { spacing, size, x: wrap(camera.x), y: wrap(camera.y), fade: Math.min(1, zoom / CANVAS_DOT_FADE_ZOOM) };
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

export const screenPoint = (point: Point, camera: CanvasCamera): Point => ({ x: point.x * camera.zoom + camera.x, y: point.y * camera.zoom + camera.y });

export function viewportWorldRect(camera: CanvasCamera, size: { width: number; height: number }): Rect {
  return { x: -camera.x / camera.zoom, y: -camera.y / camera.zoom, width: size.width / camera.zoom, height: size.height / camera.zoom };
}

export function insideRect(rect: Rect, point: Point): boolean {
  return Number.isFinite(point.x) && Number.isFinite(point.y) && point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height;
}

export function rectsIntersect(first: Rect, second: Rect): boolean {
  return first.x < second.x + second.width && first.x + first.width > second.x
    && first.y < second.y + second.height && first.y + first.height > second.y;
}

export const sameRect = (first: Rect, second: Rect) => first.x === second.x && first.y === second.y && first.width === second.width && first.height === second.height;

export function unionBounds(rects: Rect[]): Rect {
  if (!rects.length) return { x: 0, y: 0, width: 0, height: 0 };
  const left = Math.min(...rects.map(rect => rect.x));
  const top = Math.min(...rects.map(rect => rect.y));
  const right = Math.max(...rects.map(rect => rect.x + rect.width));
  const bottom = Math.max(...rects.map(rect => rect.y + rect.height));
  return { x: left, y: top, width: right - left, height: bottom - top };
}
