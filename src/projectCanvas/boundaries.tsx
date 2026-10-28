import { boundaryVisible } from './canvasView';
import { memo, useCallback, useMemo, useRef } from 'react';
import { elementIndex } from './elementGeometry';
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

type BoundaryViewProps = {
  project: Project; boundary: Boundary; shown: Rect; focused: boolean; highlighted: boolean;
  editingId: string | null; movingElements: Record<string, Rect>; visible: boolean;
  openPage: (pageId: string) => void;
  onPlaceholder: (pageId: string, sizeId: string, elementId: string) => void;
  play: PlayState;
};

const BoundaryView = memo(function BoundaryView({ project, boundary, shown, focused, highlighted, editingId, movingElements, visible, openPage, onPlaceholder, play }: BoundaryViewProps) {
  const { pageId, sizeId, label, kind } = boundary;
  const page = project.pages.find(item => item.id === pageId)!;
  const resolved = resolveForSize(project, page, sizeId);
  const ghosts = useMemo(() => visible ? placeholders(resolved, sizeId) : [], [resolved, sizeId, visible]);
  const elements = useMemo(() => visible ? playElements(project, resolved, play) : resolved, [project, resolved, play, visible]);
  return <div className={`page-boundary${highlighted ? ' selected' : ''}${focused ? ' focused' : ''}${kind === 'screen' ? ' screen' : ''}${page.appearance === 'dark' ? ' dark' : ''}`}
    data-boundary={pageId} data-size={sizeId}
    style={{ left: shown.x, top: shown.y, width: shown.width, height: shown.height }} onDoubleClick={() => openPage(pageId)}>
    {visible && <>
      <ElementTree project={project} pageId={pageId} sizeId={sizeId} elements={elements} boundary={{ pageId, sizeId, label, kind, rect: shown }} editingId={editingId} movingRects={movingElements} />
      {ghosts.map(({ element, rect: ghost }) => <div key={`placeholder:${element.id}`} className="element-placeholder" role="button" tabIndex={0}
        aria-label={`Include ${element.name ?? element.kind} in this size`} title="Include in this size, or dismiss"
        data-placeholder={element.id} data-placeholder-size={sizeId}
        style={{ left: ghost.x, top: ghost.y, width: Math.max(8, ghost.width), height: Math.max(8, ghost.height), zIndex: 1 }}
        onPointerDown={event => { event.preventDefault(); event.stopPropagation(); onPlaceholder(pageId, sizeId, element.id); }}
        onKeyDown={event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onPlaceholder(pageId, sizeId, element.id); } }} />)}
    </>}
  </div>;
});

const EMPTY_RECTS: Record<string, Rect> = {};
/** Offscreen boundaries keep their outline; model geometry continues to own hit testing. */
export function BoundaryLayer({ project, boundaries, focused, selection, moving, openPage, editingId, movingElements, onPlaceholder, play, viewport, margin = 200 }: {
  project: Project; boundaries: Boundary[];
  focused: string | null; selection: CanvasTarget[]; moving: { sizeId: string; pageId: string; rect: Rect } | null;
  tool: string; spaceHeld: boolean; openPage: (pageId: string) => void;
  editingId: string | null; movingElements: Record<string, Rect>;
  onPlaceholder: (pageId: string, sizeId: string, elementId: string) => void;
  play: PlayState; viewport: Rect; margin?: number;
}) {
  const callbacks = useRef({ openPage, onPlaceholder });
  callbacks.current = { openPage, onPlaceholder };
  const open = useCallback((id: string) => callbacks.current.openPage(id), []);
  const placeholder = useCallback((page: string, size: string, element: string) => callbacks.current.onPlaceholder(page, size, element), []);
  return <>{boundaries.map(boundary => {
    const { pageId, sizeId, rect } = boundary;
    const page = project.pages.find(item => item.id === pageId);
    if (!page) return null;
    const shown = moving?.sizeId === sizeId ? moving.rect : rect;
    const focusedHere = focused === pageId;
    const highlighted = focusedHere || selection.some(target => target.kind === 'element' && target.pageId === pageId && (target.sizeId === null || target.sizeId === sizeId));
    const visible = boundaryVisible(shown, viewport, margin);
    const owns = (id: string) => elementIndex(resolveForSize(project, page, sizeId)).has(id);
    const movingHere = Object.keys(movingElements).some(owns) ? movingElements : EMPTY_RECTS;
    return <BoundaryView key={sizeId} project={project} boundary={boundary} shown={shown} focused={focusedHere} highlighted={highlighted}
      editingId={editingId && owns(editingId) ? editingId : null} movingElements={movingHere} visible={visible}
      openPage={open} onPlaceholder={placeholder} play={play.boundary === pageId ? play : idlePlay} />;
  })}</>;
}
