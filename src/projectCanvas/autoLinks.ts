import { findElementIn } from './elementFacts';
import { resolveForSize } from './sizes';
import type { Project, Rect } from '../types';
import { Point } from './camera';
import { Boundary } from './boundaryLayout';
import { RequestBox } from './canvasTargets';
import { nearestEdgeAnchors } from './pointerGeometry';

export type AutoLink = { fromPageId: string; toPageId: string; source: Rect; target: Rect; route: { from: Point; to: Point } };

const pageLinkPath = (path: string): string => path.replace(/\/$/, '') || '/';

/** One line per linked boundary pair, from the layer that links to the boundary it names. */
export function automaticLinks(project: Project, boundaries: Boundary[], boxes: RequestBox[]): AutoLink[] {
  const links = new Map<string, AutoLink>();
  for (const box of boxes) {
    const page = project.pages.find(item => item.id === box.pageId);
    if (!page) continue;
    const sizeId = box.sizeId ?? page.sizes[0]?.id;
    if (!sizeId) continue;
    const element = findElementIn(resolveForSize(project, page, sizeId), box.elementId);
    const url = element?.url?.trim();
    if (!url) continue;
    const wanted = pageLinkPath(url);
    const targetPage = project.pages.find(other => other.id !== box.pageId && pageLinkPath(other.path) === wanted);
    if (!targetPage) continue;
    const key = `${box.pageId}:${targetPage.id}`;
    if (links.has(key)) continue;
    const boundary = boundaries.find(item => item.pageId === targetPage.id && item.sizeId === targetPage.sizes[0]?.id)
      ?? boundaries.find(item => item.pageId === targetPage.id);
    if (!boundary) continue;
    links.set(key, {
      fromPageId: box.pageId, toPageId: targetPage.id, source: box.rect, target: boundary.rect,
      route: nearestEdgeAnchors(box.rect, boundary.rect),
    });
  }
  return [...links.values()];
}
