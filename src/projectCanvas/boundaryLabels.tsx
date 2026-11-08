import { useLayoutEffect, useRef } from 'react';
import { boundaryKindLabel, type BoundaryToolKind } from './boundaryDefaults';
import { boundaryLabel } from './sizes';
import type { Boundary, CanvasCamera, CanvasTarget } from '../projectCanvasGeometry';
import type { Project, Rect } from '../types';

const dimensionsLabel = (rect: Rect) => `${Math.round(rect.width)} × ${Math.round(rect.height)}`;

/**
 * Boundary names sit in screen pixels above each boundary's top-left corner, so small type stays
 * crisp at every zoom. The label is also the drag handle: it carries the boundary identity the
 * canvas pointer-down path expects. While a boundary is drawn or moved the label carries its
 * live size, like a frame in a design tool. A label that cannot fit its full name falls back to the
 * size alone ("Phone"), then an ellipsis, but never disappears: it is the only handle a narrow
 * boundary has when zoomed out.
 */
export function BoundaryLabels({ project, boundaries, moving, draft, draftKind = 'page', focused, focusedSize = null, selection, camera, openPage }: {
  project: Project; boundaries: Boundary[];
  moving: { sizeId: string; pageId: string; rect: Rect } | null;
  draft?: Rect | null; draftKind?: BoundaryToolKind;
  focused: string | null; focusedSize?: string | null; selection: CanvasTarget[];
  camera: CanvasCamera; openPage: (pageId: string) => void;
}) {
  const labels = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const occupied: DOMRect[] = [];
    for (const label of labels.current?.querySelectorAll<HTMLElement>('.boundary-label') ?? []) {
      const children = [...label.children] as HTMLElement[];
      const full = label.querySelector<HTMLElement>('[data-label-full]');
      const short = label.querySelector<HTMLElement>('[data-label-short]');
      children.forEach(child => { child.style.visibility = ''; child.style.display = ''; });
      if (short) short.style.display = 'none';
      const fits = () => label.scrollWidth <= label.clientWidth && children.every(child => child.offsetParent === null || child.scrollWidth <= child.clientWidth);
      const overlaps = () => {
        const rect = label.getBoundingClientRect();
        return occupied.some(other => rect.left < other.right + 4 && rect.right + 4 > other.left && rect.top < other.bottom && rect.bottom > other.top);
      };
      if ((!fits() || overlaps()) && full && short) {
        full.style.display = 'none';
        short.style.display = '';
        // The live size is extra; drop it before the name.
        children.filter(child => child.dataset.boundarySize !== undefined).forEach(child => { child.style.display = 'none'; });
      }
      occupied.push(label.getBoundingClientRect());
    }
  }, [boundaries, moving, draft, camera, project]);
  const drafting = draft && (draft.width > 0 || draft.height > 0) ? draft : null;
  return <div ref={labels} className="project-canvas-labels">
    {boundaries.map(({ pageId, sizeId, rect }) => {
      const page = project.pages.find(item => item.id === pageId);
      const size = page?.sizes.find(item => item.id === sizeId);
      if (!page || !size) return null;
      const shown = moving && moving.sizeId === sizeId ? moving.rect : rect;
      const highlighted = focused === pageId && (!focusedSize || focusedSize === sizeId) || selection.some(target => target.kind === 'element' && target.pageId === pageId
        && (target.sizeId === null || target.sizeId === sizeId));
      const resizing = moving?.sizeId === sizeId;
      return <div key={sizeId} className={`boundary-label${highlighted ? ' selected' : ''}`} data-boundary={pageId} data-size={sizeId}
        style={{ left: camera.x + shown.x * camera.zoom, top: camera.y + shown.y * camera.zoom, width: Math.max(0, shown.width * camera.zoom), overflow: 'hidden', pointerEvents: 'auto' }}>
        <span className="boundary-label-name" data-boundary-handle={sizeId} data-label-full
          onDoubleClick={event => { event.stopPropagation(); openPage(pageId); }}>
          {boundaryLabel(page, size)}
        </span>
        {page.sizes.length > 1 && <span className="boundary-label-name" data-boundary-handle={sizeId} data-label-short title={boundaryLabel(page, size)}
          onDoubleClick={event => { event.stopPropagation(); openPage(pageId); }}>
          {size.label}
        </span>}
        {resizing && <span className="boundary-label-size" data-boundary-size>{dimensionsLabel(shown)}</span>}
      </div>;
    })}
    {drafting && <div className="boundary-label draft" data-boundary-draft-size
      style={{ left: camera.x + drafting.x * camera.zoom, top: camera.y + drafting.y * camera.zoom, width: Math.max(0, drafting.width * camera.zoom), overflow: 'hidden' }}>
      <span className="boundary-label-name">{boundaryKindLabel(draftKind)}</span>
      <span className="boundary-label-size">{dimensionsLabel(drafting)}</span>
    </div>}
  </div>;
}
