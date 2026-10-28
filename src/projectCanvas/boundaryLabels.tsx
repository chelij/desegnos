import { useLayoutEffect, useRef } from 'react';
import { boundaryName as kindName, type BoundaryToolKind } from './boundaryDefaults';
import { boundaryName } from './sizes';
import type { Boundary, CanvasCamera, CanvasTarget } from '../projectCanvasGeometry';
import type { Project, Rect } from '../types';

const sizeLabel = (rect: Rect) => `${Math.round(rect.width)} × ${Math.round(rect.height)}`;

/**
 * Boundary names sit in screen pixels above each boundary's top-left corner, so small type stays
 * crisp at every zoom. The label is also the drag handle: it carries the boundary identity the
 * canvas pointer-down path expects. While a boundary is drawn or moved the label carries its
 * live size, like a frame in a design tool.
 */
export function BoundaryLabels({ project, boundaries, moving, draft, draftKind = 'page', focused, selection, camera, openPage }: {
  project: Project; boundaries: Boundary[];
  moving: { sizeId: string; pageId: string; rect: Rect } | null;
  draft?: Rect | null; draftKind?: BoundaryToolKind;
  focused: string | null; selection: CanvasTarget[];
  camera: CanvasCamera; openPage: (pageId: string) => void;
}) {
  const labels = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const occupied: DOMRect[] = [];
    for (const label of labels.current?.querySelectorAll<HTMLElement>('.boundary-label') ?? []) {
      const children = [...label.children] as HTMLElement[];
      children.forEach(child => { child.style.visibility = ''; });
      const rect = label.getBoundingClientRect();
      const fits = label.scrollWidth <= label.clientWidth && children.every(child => child.scrollWidth <= child.clientWidth);
      const overlaps = occupied.some(other => rect.left < other.right + 4 && rect.right + 4 > other.left && rect.top < other.bottom && rect.bottom > other.top);
      if (fits && !overlaps) occupied.push(rect);
      else children.forEach(child => { child.style.visibility = 'hidden'; });
    }
  }, [boundaries, moving, draft, camera, project]);
  const drafting = draft && (draft.width > 0 || draft.height > 0) ? draft : null;
  return <div ref={labels} className="project-canvas-labels">
    {boundaries.map(({ pageId, sizeId, rect }) => {
      const page = project.pages.find(item => item.id === pageId);
      const size = page?.sizes.find(item => item.id === sizeId);
      if (!page || !size) return null;
      const shown = moving && moving.sizeId === sizeId ? moving.rect : rect;
      const highlighted = focused === pageId || selection.some(target => target.kind === 'element' && target.pageId === pageId
        && (target.sizeId === null || target.sizeId === sizeId));
      const resizing = moving?.sizeId === sizeId;
      return <div key={sizeId} className={`boundary-label${highlighted ? ' selected' : ''}`} data-boundary={pageId} data-size={sizeId} title={boundaryName(page, size)}
        style={{ left: camera.x + shown.x * camera.zoom, top: camera.y + shown.y * camera.zoom, width: Math.max(0, shown.width * camera.zoom), overflow: 'hidden', pointerEvents: 'auto' }}>
        <span className="boundary-label-name" data-boundary-handle={sizeId} title={boundaryName(page, size)}
          onDoubleClick={event => { event.stopPropagation(); openPage(pageId); }}>
          {boundaryName(page, size)}
        </span>
        {resizing && <span className="boundary-label-size" data-boundary-size>{sizeLabel(shown)}</span>}
      </div>;
    })}
    {drafting && <div className="boundary-label draft" data-boundary-draft-size
      style={{ left: camera.x + drafting.x * camera.zoom, top: camera.y + drafting.y * camera.zoom, width: Math.max(0, drafting.width * camera.zoom), overflow: 'hidden' }}>
      <span className="boundary-label-name">{kindName(draftKind)}</span>
      <span className="boundary-label-size">{sizeLabel(drafting)}</span>
    </div>}
  </div>;
}
