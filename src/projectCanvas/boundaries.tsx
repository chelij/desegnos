import { ElementTree } from './elements';
import { idlePlay, playElements, type PlayState } from './play';
import { resolveForSize } from './sizes';
import type { Boundary, CanvasTarget } from '../projectCanvasGeometry';
import type { Element, Project, Rect } from '../types';

/** A faint dashed ghost where a layer exists in another linked size but not this one. */
function placeholders(elements: Element[], sizeId: string): { element: Element; rect: Rect }[] {
  const rows: { element: Element; rect: Rect }[] = [];
  const walk = (children: Element[], base: { x: number; y: number }) => {
    for (const element of children) {
      const origin = { x: base.x + element.rect.x, y: base.y + element.rect.y };
      const over = element.overrides?.[sizeId];
      if (over?.hidden) {
        // One ghost for the topmost hidden layer; its children travel with it.
        if (!over.dismissed) rows.push({ element, rect: { x: origin.x, y: origin.y, width: element.rect.width, height: element.rect.height } });
        continue;
      }
      walk(element.children, origin);
    }
  };
  walk(elements, { x: 0, y: 0 });
  return rows;
}

/** Boundaries render their elements directly; the canvas never draws code. */
export function BoundaryLayer({ project, boundaries, focused, selection, moving, tool, spaceHeld, openPage, editingId, movingElements, onPlaceholder, play }: {
  project: Project; boundaries: Boundary[];
  focused: string | null; selection: CanvasTarget[]; moving: { sizeId: string; pageId: string; rect: Rect } | null;
  tool: string; spaceHeld: boolean; openPage: (pageId: string) => void;
  editingId: string | null; movingElements: Record<string, Rect>;
  onPlaceholder: (pageId: string, sizeId: string, elementId: string) => void;
  play: PlayState;
}) {
  void tool; void spaceHeld;
  return <>
    {boundaries.map(({ pageId, sizeId, label, rect, kind }) => {
      const page = project.pages.find(item => item.id === pageId);
      if (!page) return null;
      const size = page.sizes.find(item => item.id === sizeId);
      if (!size) return null;
      const shown = moving && moving.sizeId === sizeId ? moving.rect : rect;
      const focusedHere = focused === pageId;
      const highlighted = focusedHere || selection.some(target => target.kind === 'element' && target.pageId === pageId
        && (target.sizeId === null || target.sizeId === sizeId));
      const resolved = resolveForSize(project, page, sizeId);
      const ghosts = placeholders(resolved, sizeId);
      const elements = playElements(project, resolved, play.boundary === pageId ? play : idlePlay);
      return <div key={sizeId} className={`page-boundary${highlighted ? ' selected' : ''}${focusedHere ? ' focused' : ''}${kind === 'screen' ? ' screen' : ''}${page.appearance === 'dark' ? ' dark' : ''}`}
        data-boundary={pageId} data-size={sizeId}
        style={{ left: shown.x, top: shown.y, width: shown.width, height: shown.height }} onDoubleClick={() => openPage(pageId)}>
        <ElementTree project={project} pageId={pageId} sizeId={sizeId} elements={elements} boundary={{ pageId, sizeId, label, kind, rect: shown }} editingId={editingId} movingRects={movingElements} />
        {ghosts.map(({ element, rect: ghost }) => <div key={`placeholder:${element.id}`} className="element-placeholder" role="button" tabIndex={0}
          aria-label={`Include ${element.name ?? element.kind} in this size`} title="Include in this size, or dismiss"
          data-placeholder={element.id} data-placeholder-size={sizeId}
          style={{ left: ghost.x, top: ghost.y, width: Math.max(8, ghost.width), height: Math.max(8, ghost.height), zIndex: 1 }}
          onPointerDown={event => { event.preventDefault(); event.stopPropagation(); onPlaceholder(pageId, sizeId, element.id); }}
          onKeyDown={event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onPlaceholder(pageId, sizeId, element.id); } }} />)}
      </div>;
    })}
  </>;
}
