import type { Rect, Viewport } from './types';

export type Point = { x: number; y: number };
export type GeometryGesture = { kind: 'new' | 'move' | 'resize'; start: Point; initial: Rect };
export type RectChange = Rect | ((current: Rect) => Rect);
export type CanvasGridSettings = { spacing: number; unit: 'pixels' | 'percent'; snap: boolean; rulers: boolean };
export type WorkspaceBounds = { left: number; top: number; right: number; bottom: number };
export const WORLD_MIN = -100000;
export const WORLD_MAX = 100000;
export const DEFAULT_GRID: CanvasGridSettings = { spacing: 8, unit: 'pixels', snap: true, rulers: true };
export const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));
export const sameRect = (first: Rect, second: Rect) => first.x === second.x && first.y === second.y && first.width === second.width && first.height === second.height;
export const sameBounds = (first: WorkspaceBounds, second: WorkspaceBounds) => first.left === second.left && first.top === second.top && first.right === second.right && first.bottom === second.bottom;

export function gridSteps(guide: Viewport, settings: CanvasGridSettings): Point {
  const spacing = Number.isFinite(settings.spacing) && settings.spacing > 0 ? settings.spacing : DEFAULT_GRID.spacing;
  return settings.unit === 'percent' ? { x: guide.width * spacing / 100, y: guide.height * spacing / 100 } : { x: spacing, y: spacing };
}

export function clientToCanvas(client: Point, origin: { left: number; top: number }, zoom: number, worldOrigin: Point = { x: 0, y: 0 }): Point {
  return { x: (client.x - origin.left) / zoom + worldOrigin.x, y: (client.y - origin.top) / zoom + worldOrigin.y };
}

export function geometryForGesture(gesture: GeometryGesture, point: Point, guide: Viewport, settings: CanvasGridSettings, minimum: Point = { x: 1, y: 1 }): Rect {
  const { kind, initial, start } = gesture;
  const step = gridSteps(guide, settings);
  const snap = (value: number, spacing: number) => settings.snap ? Math.round(value / spacing) * spacing : value;
  const dx = point.x - start.x;
  const dy = point.y - start.y;
  if (kind === 'move') return { ...initial,
    x: clamp(snap(initial.x + dx, step.x), WORLD_MIN, WORLD_MAX - initial.width),
    y: clamp(snap(initial.y + dy, step.y), WORLD_MIN, WORLD_MAX - initial.height),
  };
  if (kind === 'resize') return { ...initial,
    width: clamp(snap(initial.x + initial.width + dx, step.x) - initial.x, Math.min(minimum.x, WORLD_MAX - initial.x), WORLD_MAX - initial.x),
    height: clamp(snap(initial.y + initial.height + dy, step.y) - initial.y, Math.min(minimum.y, WORLD_MAX - initial.y), WORLD_MAX - initial.y),
  };
  const x1 = clamp(snap(start.x, step.x), WORLD_MIN, WORLD_MAX);
  const y1 = clamp(snap(start.y, step.y), WORLD_MIN, WORLD_MAX);
  const x2 = clamp(snap(point.x, step.x), WORLD_MIN, WORLD_MAX);
  const y2 = clamp(snap(point.y, step.y), WORLD_MIN, WORLD_MAX);
  return { x: Math.min(x1, x2), y: Math.min(y1, y2), width: Math.abs(x2 - x1), height: Math.abs(y2 - y1) };
}

export function rectDelta(delta: Point, guide: Viewport, resize = false, minimum: Point = { x: 1, y: 1 }): (current: Rect) => Rect {
  return current => {
    const rect = geometryForGesture({ kind: resize ? 'resize' : 'move', start: { x: 0, y: 0 }, initial: current }, delta, guide, { ...DEFAULT_GRID, snap: false }, minimum);
    return sameRect(rect, current) ? current : rect;
  };
}

export function keyboardRectChange(direction: Point, guide: Viewport, settings: CanvasGridSettings, resize: boolean, fine: boolean, minimum: Point): (current: Rect) => Rect {
  const steps = gridSteps(guide, settings);
  return current => {
    const coordinate = { x: current.x + (resize ? current.width : 0), y: current.y + (resize ? current.height : 0) };
    const next = (value: number, direction: number, spacing: number) => {
      if (!direction) return value;
      if (fine || !settings.snap) return value + direction;
      const cell = value / spacing;
      return (direction > 0 ? Math.floor(cell + 1e-9) + 1 : Math.ceil(cell - 1e-9) - 1) * spacing;
    };
    return rectDelta({ x: next(coordinate.x, direction.x, steps.x) - coordinate.x, y: next(coordinate.y, direction.y, steps.y) - coordinate.y }, guide, resize, minimum)(current);
  };
}

export function requestPlacement(rect: Rect, zoom: number): Rect {
  const width = Math.min(WORLD_MAX - WORLD_MIN, Math.max(160 / zoom, rect.width));
  const height = Math.min(WORLD_MAX - WORLD_MIN, Math.max(80 / zoom, rect.height));
  return { x: clamp(rect.x, WORLD_MIN, WORLD_MAX - width), y: clamp(rect.y, WORLD_MIN, WORLD_MAX - height), width, height };
}

export function requestAt(point: Point, zoom: number): Rect {
  return requestPlacement({ x: point.x, y: point.y, width: 320 / zoom, height: 160 / zoom }, zoom);
}

export function initialWorkspaceBounds(guide: Viewport, regions: Rect[], visible: Point): WorkspaceBounds {
  const padding = { x: Math.max(600, visible.x), y: Math.max(600, visible.y) };
  return {
    left: Math.max(WORLD_MIN, Math.min(0, ...regions.map(rect => rect.x)) - padding.x),
    top: Math.max(WORLD_MIN, Math.min(0, ...regions.map(rect => rect.y)) - padding.y),
    right: Math.min(WORLD_MAX, Math.max(guide.width, ...regions.map(rect => rect.x + rect.width)) + padding.x),
    bottom: Math.min(WORLD_MAX, Math.max(guide.height, ...regions.map(rect => rect.y + rect.height)) + padding.y),
  };
}

export function expandWorkspace(bounds: WorkspaceBounds, visible: WorkspaceBounds, padding: Point): WorkspaceBounds {
  return {
    left: visible.left - bounds.left < padding.x / 3 ? Math.max(WORLD_MIN, visible.left - padding.x) : bounds.left,
    top: visible.top - bounds.top < padding.y / 3 ? Math.max(WORLD_MIN, visible.top - padding.y) : bounds.top,
    right: bounds.right - visible.right < padding.x / 3 ? Math.min(WORLD_MAX, visible.right + padding.x) : bounds.right,
    bottom: bounds.bottom - visible.bottom < padding.y / 3 ? Math.min(WORLD_MAX, visible.bottom + padding.y) : bounds.bottom,
  };
}

export function percent(value: number, reference: number): string {
  const percentage = Math.round(value / reference * 1000) / 10;
  return `${percentage < 0 ? '−' : ''}${Math.abs(percentage)}%`;
}

export function percentageTicks(start: number, screenLength: number, reference: number, zoom: number): { value: number; position: number }[] {
  const needed = 70 / (reference * zoom) * 100;
  const power = 10 ** Math.floor(Math.log10(needed));
  const spacing = [1, 2, 5, 10].map(value => value * power).find(value => value >= needed)!;
  const first = Math.ceil(start / reference * 100 / spacing) * spacing;
  const count = Math.ceil(screenLength / zoom / reference * 100 / spacing) + 1;
  return Array.from({ length: count }, (_, index) => {
    const value = (first + index * spacing) / 100 * reference;
    return { value, position: (value - start) * zoom };
  }).filter(tick => tick.position >= 0 && tick.position <= screenLength);
}

export function pixelTicks(start: number, screenLength: number, zoom: number): { value: number; position: number }[] {
  const needed = 70 / zoom;
  const power = 10 ** Math.floor(Math.log10(needed));
  const spacing = [1, 2, 5, 10].map(value => value * power).find(value => value >= needed)!;
  const first = Math.ceil(start / spacing) * spacing;
  return Array.from({ length: Math.ceil(screenLength / zoom / spacing) + 1 }, (_, index) => {
    const value = (first + index * spacing) || 0;
    return { value, position: (value - start) * zoom };
  }).filter(tick => tick.position >= 0 && tick.position <= screenLength);
}

// Search above and below first, then beside the selection, and finally along
// the viewport edges. Score actual nearby content as well as viewport clipping.
export function elementToolbarPosition(rect: Rect, size: Point, viewport: Rect, occupied: Rect[] = []): Point {
  const gap = 8;
  const at = (x: number, y: number) => ({ x: clamp(x, viewport.x + gap, Math.max(viewport.x + gap, viewport.x + viewport.width - size.x - gap)),
    y: clamp(y, viewport.y + gap, Math.max(viewport.y + gap, viewport.y + viewport.height - size.y - gap)) });
  const candidates = [at(rect.x, rect.y - size.y - gap), at(rect.x, rect.y + rect.height + gap),
    at(rect.x - size.x - gap, rect.y), at(rect.x + rect.width + gap, rect.y),
    at(viewport.x + gap, viewport.y + gap), at(viewport.x + viewport.width - size.x - gap, viewport.y + viewport.height - size.y - gap)];
  const intersection = (point: Point, other: Rect) => Math.max(0, Math.min(point.x + size.x, other.x + other.width) - Math.max(point.x, other.x))
    * Math.max(0, Math.min(point.y + size.y, other.y + other.height) - Math.max(point.y, other.y));
  const score = (point: Point) => intersection(point, rect) * 2 + occupied.reduce((sum, other) => sum + intersection(point, other), 0);
  return candidates.reduce((best, point) => score(point) < score(best) ? point : best);
}

export function alignmentLines(rect: Rect | undefined, guide: Viewport, others: Rect[], zoom: number): { x: number[]; y: number[] } {
  if (!rect) return { x: [], y: [] };
  const targetX = [0, guide.width / 2, guide.width];
  const targetY = [0, guide.height / 2, guide.height];
  for (const other of others) {
    targetX.push(other.x, other.x + other.width / 2, other.x + other.width);
    targetY.push(other.y, other.y + other.height / 2, other.y + other.height);
  }
  const matching = (own: number[], targets: number[]) => [...new Set(targets.filter(target => own.some(value => Math.abs(value - target) * zoom < 0.75)))];
  return { x: matching([rect.x, rect.x + rect.width / 2, rect.x + rect.width], targetX), y: matching([rect.y, rect.y + rect.height / 2, rect.y + rect.height], targetY) };
}

export const MIN_ZOOM = 0.001;
export const ZOOM_LEVELS = [0.1, 0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];
export const zoomLabel = (zoom: number) => `${Math.round(zoom * 1000) / 10}%`;
export function steppedZoom(zoom: number, direction: number): number {
  return direction > 0 ? ZOOM_LEVELS.find(value => value > zoom + 1e-6) ?? 4
    : [...ZOOM_LEVELS].reverse().find(value => value < zoom - 1e-6) ?? MIN_ZOOM;
}
export function wheelZoom(zoom: number, delta: number): number {
  return Number.isFinite(delta) ? clamp(Math.round(zoom * Math.exp(-clamp(delta, -800, 800) * 0.0015) * 1e6) / 1e6, MIN_ZOOM, 4) : zoom;
}
export function zoomOrigin(origin: Point, pointer: Point, from: number, to: number): Point {
  return { x: origin.x + pointer.x / from - pointer.x / to, y: origin.y + pointer.y / from - pointer.y / to };
}

export type ResizeCorner = 'nw' | 'ne' | 'sw' | 'se';
// Outer bounds cannot independently resolve local width/height near a 45-degree
// rotation. Project those drags onto proportional scaling instead of inverting
// an ill-conditioned matrix and creating enormous opposing dimensions.
export function resizedElementSize(initial: Point, minimum: Point, matrix: [number, number, number, number], delta: Point): Point | null {
  const [a, b, c, d] = matrix;
  if (![...matrix, initial.x, initial.y, delta.x, delta.y].every(Number.isFinite) || initial.x <= 0 || initial.y <= 0) return null;
  const determinant = a * d - b * c;
  const norm = a * a + b * b + c * c + d * d;
  if (norm < 1e-12) return null;
  if (Math.abs(determinant) > .01 * norm) {
    const result = { x: initial.x + (d * delta.x - c * delta.y) / determinant, y: initial.y + (a * delta.y - b * delta.x) / determinant };
    if (result.x >= minimum.x && result.y >= minimum.y && result.x <= WORLD_MAX && result.y <= WORLD_MAX) return result;
  }
  const u = a * initial.x + c * initial.y, v = b * initial.x + d * initial.y;
  const magnitude = u * u + v * v;
  if (magnitude < 1e-12) return null;
  const low = Math.max(minimum.x / initial.x, minimum.y / initial.y);
  const high = Math.min(WORLD_MAX / initial.x, WORLD_MAX / initial.y, WORLD_MAX / Math.max(Math.abs(u), Math.abs(v)));
  if (low > high) return null;
  const scale = clamp(1 + (u * delta.x + v * delta.y) / magnitude, low, high);
  return { x: initial.x * scale, y: initial.y * scale };
}
export function elementResizeRect(initial: Rect, delta: Point, corner: ResizeCorner, step: Point, snap: boolean): Rect {
  const west = corner.includes('w'); const north = corner.includes('n');
  const align = (value: number, spacing: number) => snap ? Math.round(value / spacing) * spacing : value;
  const opposite = { x: initial.x + (west ? initial.width : 0), y: initial.y + (north ? initial.height : 0) };
  const x = clamp(align(initial.x + (west ? 0 : initial.width) + delta.x, step.x), west ? WORLD_MIN : opposite.x + 1, west ? opposite.x - 1 : WORLD_MAX);
  const y = clamp(align(initial.y + (north ? 0 : initial.height) + delta.y, step.y), north ? WORLD_MIN : opposite.y + 1, north ? opposite.y - 1 : WORLD_MAX);
  return { x: Math.min(x, opposite.x), y: Math.min(y, opposite.y), width: Math.abs(x - opposite.x), height: Math.abs(y - opposite.y) };
}
