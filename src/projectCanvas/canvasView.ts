import type { Project, Rect } from '../types';
import { layoutBoundaries, type Boundary, type RequestBox } from '../projectCanvasGeometry';
import { elementBoundsHeight, elementIndex } from './elementGeometry';
import { resolveForSize, reuseResolvedTrees } from './sizes';

const views = new WeakMap<Project, { revision: number; view: CanvasView }>();
// Keep only the active snapshot strongly reachable. Tab switches can use their weak views;
// native refreshes of an unchanged revision reuse the active snapshot's trees and geometry.
let activeView: { project: Project; revision: number; view: CanvasView } | null = null;
export type CanvasView = { boundaries: Boundary[]; boxes: RequestBox[] };
const boxViews = new WeakMap<Boundary[], { project: Project; revision: number; boxes: RequestBox[] }>();

/** Model geometry remains available even when a boundary's DOM is outside the viewport. */
export function canvasBoxes(project: Project, boundaries: Boundary[]): RequestBox[] {
  const cached = boxViews.get(boundaries);
  if (cached?.project === project && cached.revision === project.revision) return cached.boxes;
  const boxes: RequestBox[] = [];
  for (const boundary of boundaries) {
    const page = project.pages.find(page => page.id === boundary.pageId);
    if (!page) continue;
    for (const entry of elementIndex(resolveForSize(project, page, boundary.sizeId)).values()) {
      boxes.push({ pageId: page.id, sizeId: boundary.sizeId, elementId: entry.element.id,
        rect: { ...entry.element.rect, x: boundary.rect.x + entry.origin.x, y: boundary.rect.y + entry.origin.y } });
    }
  }
  boxViews.set(boundaries, { project, revision: project.revision, boxes });
  return boxes;
}

/** One resolved canvas per revision, shared by layout, selection, hit testing and render. */
export function canvasView(project: Project): CanvasView {
  const cached = views.get(project);
  if (cached?.revision === project.revision) return cached.view;
  if (activeView?.project.id === project.id && activeView.revision === project.revision) {
    reuseResolvedTrees(activeView.project, project);
    const view = activeView.view;
    views.set(project, { revision: project.revision, view });
    boxViews.set(view.boundaries, { project, revision: project.revision, boxes: view.boxes });
    activeView = { project, revision: project.revision, view };
    return view;
  }
  const boundaries = layoutBoundaries(project.pages.map(page => ({ id: page.id, sizes: page.sizes.map(size => ({
    ...size, height: Math.max(size.height, elementBoundsHeight(resolveForSize(project, page, size.id), size.height)),
  })) })), project.homePageId);
  const view = { boundaries, boxes: canvasBoxes(project, boundaries) };
  views.set(project, { revision: project.revision, view });
  activeView = { project, revision: project.revision, view };
  return view;
}

export function boundaryVisible(rect: Rect, viewport: Rect, margin: number): boolean {
  return rect.x <= viewport.x + viewport.width + margin && rect.x + rect.width >= viewport.x - margin
    && rect.y <= viewport.y + viewport.height + margin && rect.y + rect.height >= viewport.y - margin;
}
