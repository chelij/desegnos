import { boundaryName } from './sizes';
import type { Boundary, CanvasCamera, CanvasTarget } from '../projectCanvasGeometry';
import type { Project, Rect } from '../types';

/**
 * Boundary names sit in screen pixels above each boundary's top-left corner, so small type stays
 * crisp at every zoom. The label is also the drag handle: it carries the boundary identity the
 * canvas pointer-down path expects.
 */
export function BoundaryLabels({ project, boundaries, moving, focused, selection, camera, openPage }: {
  project: Project; boundaries: Boundary[];
  moving: { sizeId: string; pageId: string; rect: Rect } | null;
  focused: string | null; selection: CanvasTarget[];
  camera: CanvasCamera; openPage: (pageId: string) => void;
}) {
  return <div className="project-canvas-labels">
    {boundaries.map(({ pageId, sizeId, rect }) => {
      const page = project.pages.find(item => item.id === pageId);
      const size = page?.sizes.find(item => item.id === sizeId);
      if (!page || !size) return null;
      const shown = moving && moving.sizeId === sizeId ? moving.rect : rect;
      const highlighted = focused === pageId || selection.some(target => target.kind === 'element' && target.pageId === pageId
        && (target.sizeId === null || target.sizeId === sizeId));
      return <div key={sizeId} className={`boundary-label${highlighted ? ' selected' : ''}`} data-boundary={pageId} data-size={sizeId}
        style={{ left: camera.x + shown.x * camera.zoom, top: camera.y + shown.y * camera.zoom, maxWidth: Math.max(72, shown.width * camera.zoom) }}>
        <span className="boundary-label-name" data-boundary-handle={sizeId} title={`Move ${page.name}`}
          onDoubleClick={event => { event.stopPropagation(); openPage(pageId); }}>
          {boundaryName(page, size)}
        </span>
      </div>;
    })}
  </div>;
}
