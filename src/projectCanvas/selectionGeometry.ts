import type { Project, Rect } from '../types';
import type { CanvasTarget } from '../projectCanvasGeometry';
import { type ElementEntry, isElementTarget } from './interactionTypes';
import { elementPath, elementParents } from './elementGeometry';
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

