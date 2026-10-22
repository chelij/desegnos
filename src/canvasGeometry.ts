import type { Rect, RegionShape, Viewport } from './types';

export type Point = { x: number; y: number };
export type GeometryGesture = { kind: 'new' | 'move' | 'resize'; start: Point; initial: Rect; proportional?: boolean };
export type RectChange = Rect | ((current: Rect) => Rect);
export type CanvasGridSettings = { spacing: number; unit: 'pixels' | 'percent'; snap: boolean; rulers: boolean };
export type WorkspaceBounds = { left: number; top: number; right: number; bottom: number };
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
    x: snap(initial.x + dx, step.x),
    y: snap(initial.y + dy, step.y),
  };
  if (kind === 'resize') {
    const rect = { ...initial,
    width: Math.max(minimum.x, snap(initial.x + initial.width + dx, step.x) - initial.x),
    height: Math.max(minimum.y, snap(initial.y + initial.height + dy, step.y) - initial.y),
    };
    return gesture.proportional ? { ...rect, width: Math.max(rect.width, rect.height), height: Math.max(rect.width, rect.height) } : rect;
  }
  const x1 = snap(start.x, step.x);
  const y1 = snap(start.y, step.y);
  let x2 = snap(point.x, step.x);
  let y2 = snap(point.y, step.y);
  if (gesture.proportional) {
    const side = Math.max(Math.abs(x2 - x1), Math.abs(y2 - y1));
    x2 = x1 + (point.x < start.x ? -side : side); y2 = y1 + (point.y < start.y ? -side : side);
  }
  return { x: Math.min(x1, x2), y: Math.min(y1, y2), width: Math.abs(x2 - x1), height: Math.abs(y2 - y1) };
}

export function pointInEllipse(point: Point, rect: Rect): boolean {
  if (rect.width <= 0 || rect.height <= 0) return false;
  const x = (point.x - rect.x) / rect.width * 2 - 1, y = (point.y - rect.y) / rect.height * 2 - 1;
  return x * x + y * y <= 1 + 1e-12;
}

export function pointInPolygon(point: Point, polygon: Point[]): boolean {
  let inside = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const a = polygon[j], b = polygon[i];
    const cross = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);
    if (Math.abs(cross) < 1e-8 && point.x >= Math.min(a.x, b.x) && point.x <= Math.max(a.x, b.x) && point.y >= Math.min(a.y, b.y) && point.y <= Math.max(a.y, b.y)) return true;
    if ((a.y > point.y) !== (b.y > point.y) && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x) inside = !inside;
  }
  return inside;
}

export function absolutePoints(points: [number, number][], rect: Rect): Point[] {
  return points.map(([x, y]) => ({ x: rect.x + x * rect.width, y: rect.y + y * rect.height }));
}

export function relativePoints(points: Point[], rect: Rect): [number, number][] {
  return points.map(point => [clamp((point.x - rect.x) / rect.width, 0, 1), clamp((point.y - rect.y) / rect.height, 0, 1)]);
}

export function pointInRegion(point: Point, rect: Rect, shape?: RegionShape): boolean {
  if (shape?.kind === 'ellipse') return pointInEllipse(point, rect);
  if (shape?.kind === 'freehand') return pointInPolygon(point, absolutePoints(shape.points, rect));
  return point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height;
}

// Iterative RDP avoids a deep call stack for long, noisy pen strokes.
function simplifyLine(points: Point[], tolerance: number): Point[] {
  if (points.length <= 2) return points;
  const keep = new Set([0, points.length - 1]), stack = [[0, points.length - 1]];
  while (stack.length) {
    const [start, end] = stack.pop()!, a = points[start], b = points[end];
    let maximum = tolerance * tolerance, at = -1;
    const dx = b.x - a.x, dy = b.y - a.y, length = dx * dx + dy * dy;
    for (let i = start + 1; i < end; i++) {
      const p = points[i], t = length ? clamp(((p.x - a.x) * dx + (p.y - a.y) * dy) / length, 0, 1) : 0;
      const distance = (p.x - a.x - t * dx) ** 2 + (p.y - a.y - t * dy) ** 2;
      if (distance > maximum) { maximum = distance; at = i; }
    }
    if (at >= 0) { keep.add(at); stack.push([start, at], [at, end]); }
  }
  return [...keep].sort((a, b) => a - b).map(i => points[i]);
}

export function simplifyFreehand(input: Point[], tolerance = 1.5, limit = 200): Point[] {
  const points = input.filter((p, i) => Number.isFinite(p.x) && Number.isFinite(p.y) && (!i || p.x !== input[i - 1].x || p.y !== input[i - 1].y));
  if (points.length > 1 && points[0].x === points.at(-1)!.x && points[0].y === points.at(-1)!.y) points.pop();
  if (points.length <= 3) return points;
  const first = points[0];
  const split = points.reduce((best, point, i) => Math.hypot(point.x - first.x, point.y - first.y) > Math.hypot(points[best].x - first.x, points[best].y - first.y) ? i : best, 1);
  const simplify = (epsilon: number) => [...simplifyLine(points.slice(0, split + 1), epsilon).slice(0, -1), ...simplifyLine([...points.slice(split), first], epsilon).slice(0, -1)];
  let epsilon = Math.max(0, tolerance), result = simplify(epsilon);
  while (result.length > Math.max(3, limit)) { epsilon = Math.max(0.01, epsilon * 1.5); result = simplify(epsilon); }
  // Keep a third vertex for small but valid loops that RDP collapses into a line.
  if (result.length < 3) {
    const a = points[split];
    const third = points.reduce((best, p, i) => Math.abs((a.x-first.x)*(p.y-first.y)-(a.y-first.y)*(p.x-first.x)) > Math.abs((a.x-first.x)*(points[best].y-first.y)-(a.y-first.y)*(points[best].x-first.x)) ? i : best, 0);
    return [0, split, third].sort((a,b) => a-b).map(i => points[i]);
  }
  return result;
}

export function freehandArea(input: Point[], tolerance = 1.5): { rect: Rect; shape: Extract<RegionShape, { kind: 'freehand' }> } | null {
  const points = simplifyFreehand(input, tolerance);
  if (points.length < 3) return null;
  const x = Math.min(...points.map(p => p.x)), y = Math.min(...points.map(p => p.y));
  const rect = { x, y, width: Math.max(...points.map(p => p.x)) - x, height: Math.max(...points.map(p => p.y)) - y };
  if (rect.width < 1 || rect.height < 1 || !points.some(p => Math.abs((points[1].x-points[0].x)*(p.y-points[0].y)-(points[1].y-points[0].y)*(p.x-points[0].x)) > 1e-8)) return null;
  return { rect, shape: { kind: 'freehand', points: relativePoints(points, rect) } };
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
  const width = Math.max(160 / zoom, rect.width);
  const height = Math.max(80 / zoom, rect.height);
  return { x: rect.x, y: rect.y, width, height };
}

export function requestAt(point: Point, zoom: number): Rect {
  return requestPlacement({ x: point.x, y: point.y, width: 320 / zoom, height: 160 / zoom }, zoom);
}

// Keep browser scroll coordinates small. Rebase the window around the camera
// instead of growing a DOM element across the entire logical canvas.
export function workspaceWindow(origin: Point, visible: Point): WorkspaceBounds {
  const padding = { x: Math.max(600, visible.x), y: Math.max(600, visible.y) };
  return { left: origin.x - padding.x, top: origin.y - padding.y,
    right: origin.x + visible.x + padding.x, bottom: origin.y + visible.y + padding.y };
}

export function expandWorkspace(bounds: WorkspaceBounds, visible: WorkspaceBounds, padding: Point): WorkspaceBounds {
  const nearEdge = visible.left - bounds.left < padding.x / 3 || bounds.right - visible.right < padding.x / 3
    || visible.top - bounds.top < padding.y / 3 || bounds.bottom - visible.bottom < padding.y / 3;
  return nearEdge ? workspaceWindow({ x: visible.left, y: visible.top }, { x: visible.right - visible.left, y: visible.bottom - visible.top }) : bounds;
}

export function intersectsWorkspace(rect: Rect, bounds: WorkspaceBounds): boolean {
  return rect.x + rect.width >= bounds.left && rect.x <= bounds.right && rect.y + rect.height >= bounds.top && rect.y <= bounds.bottom;
}

export function clipToWorkspace(rect: Rect, bounds: WorkspaceBounds): Rect | null {
  if (!intersectsWorkspace(rect, bounds)) return null;
  const x = Math.max(rect.x, bounds.left), y = Math.max(rect.y, bounds.top);
  return { x, y, width: Math.min(rect.x + rect.width, bounds.right) - x, height: Math.min(rect.y + rect.height, bounds.bottom) - y };
}

export function validWorkspace(bounds: WorkspaceBounds, visible: Point): boolean {
  return Object.values(bounds).every(Number.isFinite) && bounds.right - bounds.left >= visible.x && bounds.bottom - bounds.top >= visible.y;
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
// Labels may clamp to the visible viewport, but must stay above or below the area.
// If neither side is clear, hiding is safer than covering text or controls.
export function selectionLabelPosition(rect: Rect, size: Point, viewport: Rect, occupied: Rect[] = []): Point | null {
  const margin = 8, gap = 28; // Leave room for the 24px corner handles outside the area.
  if (size.x + margin * 2 > viewport.width || size.y + margin * 2 > viewport.height) return null;
  const x = clamp(rect.x, viewport.x + margin, viewport.x + viewport.width - size.x - margin);
  for (const preferredY of [rect.y - size.y - gap, rect.y + rect.height + gap]) {
    const y = clamp(preferredY, viewport.y + margin, viewport.y + viewport.height - size.y - margin);
    if (y + size.y > rect.y - gap && y < rect.y + rect.height + gap) continue;
    if (occupied.some(other => x < other.x + other.width && x + size.x > other.x && y < other.y + other.height && y + size.y > other.y)) continue;
    return { x, y };
  }
  return null;
}

export function elementToolbarPosition(rect: Rect, size: Point, viewport: Rect, occupied: Rect[] = []): Point {
  const gap = 8;
  const at = (x: number, y: number) => ({ x: clamp(x, viewport.x + gap, Math.max(viewport.x + gap, viewport.x + viewport.width - size.x - gap)),
    y: clamp(y, viewport.y + gap, Math.max(viewport.y + gap, viewport.y + viewport.height - size.y - gap)) });
  // Spots touching the selection come first; covering other boxes there is better than
  // floating far away. The view's corners are used only when every nearby spot covers the selection.
  const near = [at(rect.x, rect.y - size.y - gap), at(rect.x, rect.y + rect.height + gap),
    at(rect.x + rect.width - size.x, rect.y - size.y - gap), at(rect.x + rect.width - size.x, rect.y + rect.height + gap),
    at(rect.x - size.x - gap, rect.y), at(rect.x + rect.width + gap, rect.y)];
  const corners = [at(viewport.x + gap, viewport.y + gap), at(viewport.x + viewport.width - size.x - gap, viewport.y + viewport.height - size.y - gap)];
  const intersection = (point: Point, other: Rect) => Math.max(0, Math.min(point.x + size.x, other.x + other.width) - Math.max(point.x, other.x))
    * Math.max(0, Math.min(point.y + size.y, other.y + other.height) - Math.max(point.y, other.y));
  const score = (point: Point) => intersection(point, rect) * 2 + occupied.reduce((sum, other) => sum + intersection(point, other), 0);
  const best = (points: Point[]) => points.reduce((winner, point) => score(point) < score(winner) ? point : winner);
  const clear = near.filter(point => intersection(point, rect) === 0);
  return clear.length ? best(clear) : best([...near, ...corners]);
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
    if (result.x >= minimum.x && result.y >= minimum.y && Number.isFinite(result.x) && Number.isFinite(result.y)) return result;
  }
  const u = a * initial.x + c * initial.y, v = b * initial.x + d * initial.y;
  const magnitude = u * u + v * v;
  if (magnitude < 1e-12) return null;
  const low = Math.max(minimum.x / initial.x, minimum.y / initial.y);
  const scale = Math.max(low, 1 + (u * delta.x + v * delta.y) / magnitude);
  const result = { x: initial.x * scale, y: initial.y * scale };
  return Number.isFinite(result.x) && Number.isFinite(result.y) ? result : null;
}
export function elementResizeRect(initial: Rect, delta: Point, corner: ResizeCorner, step: Point, snap: boolean): Rect {
  const west = corner.includes('w'); const north = corner.includes('n');
  const align = (value: number, spacing: number) => snap ? Math.round(value / spacing) * spacing : value;
  const opposite = { x: initial.x + (west ? initial.width : 0), y: initial.y + (north ? initial.height : 0) };
  const x = (west ? Math.min : Math.max)(align(initial.x + (west ? 0 : initial.width) + delta.x, step.x), opposite.x + (west ? -1 : 1));
  const y = (north ? Math.min : Math.max)(align(initial.y + (north ? 0 : initial.height) + delta.y, step.y), opposite.y + (north ? -1 : 1));
  return { x: Math.min(x, opposite.x), y: Math.min(y, opposite.y), width: Math.abs(x - opposite.x), height: Math.abs(y - opposite.y) };
}
