import type { Point } from '../projectCanvasGeometry';
import type { Rect, RequestShape } from '../types';

/** Axis-aligned bounds of a request outline. */
export function requestShapeBounds(shape: RequestShape): Rect {
  if (shape.kind === 'rect' || shape.kind === 'ellipse') return shape.rect;
  const xs = shape.points.map(point => point.x);
  const ys = shape.points.map(point => point.y);
  const left = Math.min(...xs), top = Math.min(...ys);
  return { x: left, y: top, width: Math.max(1, Math.max(...xs) - left), height: Math.max(1, Math.max(...ys) - top) };
}

function distanceToSegment(point: Point, start: Point, end: Point): number {
  const dx = end.x - start.x, dy = end.y - start.y;
  const length = dx * dx + dy * dy;
  if (!length) return Math.hypot(point.x - start.x, point.y - start.y);
  const t = Math.max(0, Math.min(1, ((point.x - start.x) * dx + (point.y - start.y) * dy) / length));
  return Math.hypot(point.x - (start.x + t * dx), point.y - (start.y + t * dy));
}

/** True when a point falls inside the real outline, not its bounding box. */
export function requestAreaContains(shape: RequestShape, point: Point): boolean {
  if (shape.kind === 'rect') {
    const { rect } = shape;
    return point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height;
  }
  if (shape.kind === 'ellipse') {
    const { rect } = shape;
    const rx = rect.width / 2, ry = rect.height / 2;
    if (rx <= 0 || ry <= 0) return false;
    const dx = (point.x - (rect.x + rx)) / rx, dy = (point.y - (rect.y + ry)) / ry;
    return dx * dx + dy * dy <= 1;
  }
  const points = shape.points;
  if (points.length < 3) return false;
  let inside = false;
  let previous = points[points.length - 1];
  for (const current of points) {
    if (distanceToSegment(point, previous, current) <= 0.5) return true;
    if ((current.y > point.y) !== (previous.y > point.y)) {
      const x = (previous.x - current.x) * (point.y - current.y) / (previous.y - current.y) + current.x;
      if (point.x < x) inside = !inside;
    }
    previous = current;
  }
  return inside;
}

/** SVG path data for an outline, in coordinates relative to its bounding box. */
export function requestShapePath(shape: RequestShape): string {
  const bounds = requestShapeBounds(shape);
  if (shape.kind === 'rect') return `M 0 0 H ${bounds.width} V ${bounds.height} H 0 Z`;
  if (shape.kind === 'ellipse') {
    const rx = bounds.width / 2, ry = bounds.height / 2;
    return `M 0 ${ry} A ${rx} ${ry} 0 1 0 ${bounds.width} ${ry} A ${rx} ${ry} 0 1 0 0 ${ry} Z`;
  }
  return shape.points.map((point, index) => `${index ? 'L' : 'M'} ${point.x - bounds.x} ${point.y - bounds.y}`).join(' ') + ' Z';
}
