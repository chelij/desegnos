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
  const drafting = draft && (draft.width > 0 || draft.height > 0) ? draft : null;
  return <div className="project-canvas-labels">
    {boundaries.map(({ pageId, sizeId, rect }) => {
      const page = project.pages.find(item => item.id === pageId);
      const size = page?.sizes.find(item => item.id === sizeId);
      if (!page || !size) return null;
      const shown = moving && moving.sizeId === sizeId ? moving.rect : rect;
      const highlighted = focused === pageId || selection.some(target => target.kind === 'element' && target.pageId === pageId
        && (target.sizeId === null || target.sizeId === sizeId));
      const resizing = moving?.sizeId === sizeId;
      return <div key={sizeId} className={`boundary-label${highlighted ? ' selected' : ''}`} data-boundary={pageId} data-size={sizeId}
        style={{ left: camera.x + shown.x * camera.zoom, top: camera.y + shown.y * camera.zoom, maxWidth: Math.max(72, shown.width * camera.zoom) }}>
        <span className="boundary-label-name" data-boundary-handle={sizeId} title={`Move ${page.name}`}
          onDoubleClick={event => { event.stopPropagation(); openPage(pageId); }}>
          {boundaryName(page, size)}
        </span>
        {resizing && <span className="boundary-label-size" data-boundary-size>{sizeLabel(shown)}</span>}
      </div>;
    })}
    {drafting && <div className="boundary-label draft" data-boundary-draft-size
      style={{ left: camera.x + drafting.x * camera.zoom, top: camera.y + drafting.y * camera.zoom, maxWidth: Math.max(72, drafting.width * camera.zoom) }}>
      <span className="boundary-label-name">{kindName(draftKind)}</span>
      <span className="boundary-label-size">{sizeLabel(drafting)}</span>
    </div>}
  </div>;
}
