import { unionBounds, viewportWorldRect, type Boundary, type CanvasCamera } from '../projectCanvasGeometry';
import type { ReferenceItem, Rect } from '../types';

/** The minimap's fixed screen size. Its SVG viewBox uses the same units. */
export const MINIMAP_WIDTH = 180;
export const MINIMAP_HEIGHT = 120;
export const MINIMAP_PADDING = 8;
/** Content this close to the viewport edge still counts as fitting. */
const FIT_TOLERANCE = 1;

export type MinimapTransform = { scale: number; x: number; y: number };

/** Boundaries and reference items, everything the minimap draws besides the viewport. */
export function overviewRects(boundaries: Boundary[], reference: ReferenceItem[]): Rect[] {
  return [...boundaries.map(boundary => boundary.rect), ...reference.map(item => item.rect)];
}

/** True when the whole canvas already fits on screen, so the minimap would add nothing. */
export function contentFitsViewport(rects: Rect[], viewport: Rect): boolean {
  if (!rects.length) return true;
  const content = unionBounds(rects);
  return content.x >= viewport.x - FIT_TOLERANCE
    && content.y >= viewport.y - FIT_TOLERANCE
    && content.x + content.width <= viewport.x + viewport.width + FIT_TOLERANCE
    && content.y + content.height <= viewport.y + viewport.height + FIT_TOLERANCE;
}

/** The world rectangle the minimap maps. It follows the content, not the camera, so panning
 * only redraws the viewport rectangle. */
export function minimapWorld(rects: Rect[], camera: CanvasCamera, viewSize: { width: number; height: number }): Rect {
  return rects.length ? unionBounds(rects) : viewportWorldRect(camera, viewSize);
}

/** Fit a world rectangle into the minimap, centred with a small screen margin. */
export function minimapTransform(world: Rect): MinimapTransform {
  const scale = Math.min(
    (MINIMAP_WIDTH - MINIMAP_PADDING * 2) / Math.max(1, world.width),
    (MINIMAP_HEIGHT - MINIMAP_PADDING * 2) / Math.max(1, world.height),
  );
  return {
    scale,
    x: (MINIMAP_WIDTH - world.width * scale) / 2 - world.x * scale,
    y: (MINIMAP_HEIGHT - world.height * scale) / 2 - world.y * scale,
  };
}

/** World → minimap screen units. */
export function minimapPoint(transform: MinimapTransform, point: { x: number; y: number }) {
  return { x: point.x * transform.scale + transform.x, y: point.y * transform.scale + transform.y };
}

/** Minimap screen units → world, for click and drag on the overview. */
export function minimapWorldPoint(transform: MinimapTransform, point: { x: number; y: number }) {
  return { x: (point.x - transform.x) / transform.scale, y: (point.y - transform.y) / transform.scale };
}

/** The camera that puts a world point at the centre of the surface, keeping the zoom. */
export function cameraCentredOn(world: { x: number; y: number }, camera: CanvasCamera, viewSize: { width: number; height: number }): CanvasCamera {
  return {
    zoom: camera.zoom,
    x: viewSize.width / 2 - world.x * camera.zoom,
    y: viewSize.height / 2 - world.y * camera.zoom,
  };
}
