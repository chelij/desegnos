import type { Boundary } from './boundaryLayout';
import type { RequestBox } from './canvasTargets';
import { canvasBoxes } from './canvasView';
import type { PointerWorld } from './pointerGeometry';
import type { Project, Rect } from '../types';

export type BoundaryShift = { pageId: string; sizeId: string; rect: Rect } | null;

/** A moving boundary offsets its cached geometry; all other boxes retain their identity. */
export function elementBoxes(project: Project, boundaries: Boundary[], moving: BoundaryShift): RequestBox[] {
  const boxes = canvasBoxes(project, boundaries);
  if (!moving) return boxes;
  const boundary = boundaries.find(boundary => boundary.sizeId === moving.sizeId);
  if (!boundary) return boxes;
  return boxes.map(box => box.sizeId === moving.sizeId ? { ...box, rect: { ...box.rect,
    x: box.rect.x + moving.rect.x - boundary.rect.x, y: box.rect.y + moving.rect.y - boundary.rect.y } } : box);
}

export const elementBoxesNow = (project: Project, boundaries: Boundary[]): RequestBox[] => elementBoxes(project, boundaries, null);

/** The layout a pointer is drawn against, including items and boundaries mid-drag. */
export function pointerWorld(project: Project, boundaries: Boundary[], moving: { pageId: string; sizeId: string; rect: Rect } | null, _movingElements: Record<string, Rect>, movingItems: Record<string, Rect>): PointerWorld {
  return {
    requests: elementBoxes(project, boundaries, moving),
    boundaries: boundaries.map(item => ({ pageId: item.pageId, sizeId: item.sizeId, rect: moving && moving.sizeId === item.sizeId && moving.pageId === item.pageId ? moving.rect : item.rect })),
    reference: project.reference.map(item => ({ itemId: item.id, rect: movingItems[item.id] ?? item.rect, url: item.kind.kind === 'link' ? item.kind.url : undefined })),
  };
}
