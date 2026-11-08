import type { Project, Rect } from '../types';
import type { CanvasTarget } from './canvasTargets';
import { type ElementEntry, isElementTarget } from './interactionTypes';
import { elementPath, elementParents, elementWorldRect } from './elementGeometry';
import { unionBounds } from './camera';
import { baseSizeId } from './sizes';

export function selectionEntries(project: Project, targets: CanvasTarget[], boundaryRect: (sizeId: string) => Rect | null): ElementEntry[] {
  const entries: ElementEntry[] = [];
  for (const target of targets.filter(isElementTarget)) {
    const page = project.pages.find(item => item.id === target.pageId);
    if (!page) continue;
    const allSizes = target.sizeId === null;
    const sizeId = target.sizeId ?? baseSizeId(page);
    const path = elementPath(project, target.pageId, target.elementId, sizeId);
    const boundary = boundaryRect(sizeId);
    const last = path?.[path.length - 1];
    if (!last || !boundary) continue;
    const over = last.element.overrides?.[sizeId];
    const rect = over?.rect ?? last.element.rect;
    const origin = { x: last.parentOrigin.x + rect.x, y: last.parentOrigin.y + rect.y };
    entries.push({
      id: last.element.id, element: last.element, pageId: target.pageId, sizeId, allSizes, parentId: last.parentId, index: last.index, rect,
      parentOrigin: last.parentOrigin, boundaryOrigin: { x: boundary.x, y: boundary.y },
      world: { x: boundary.x + origin.x, y: boundary.y + origin.y, width: rect.width, height: rect.height },
    });
  }
  // A selected ancestor moves its subtree; never move both.
  const selected = new Set(entries.map(entry => entry.id));
  return entries.filter(entry => !elementParents(project, entry.pageId, entry.id).some(id => selected.has(id)));
}

function targetCanvasRect(project: Project, target: CanvasTarget): Rect | null {
  if (target.kind === 'reference') return project.reference.find(item => item.id === target.itemId)?.rect ?? null;
  if (target.kind !== 'element') return null;
  const page = project.pages.find(item => item.id === target.pageId);
  const size = page ? page.sizes.find(item => item.id === target.sizeId) ?? page.sizes[0] : null;
  if (!page || !size) return null;
  return elementWorldRect(project, page.id, target.elementId, { x: size.x, y: size.y });
}

export function selectionBounds(project: Project, selection: CanvasTarget[]): Rect | null {
  const rects = selection.flatMap(target => { const rect = targetCanvasRect(project, target); return rect ? [rect] : []; });
  return rects.length ? unionBounds(rects) : null;
}
